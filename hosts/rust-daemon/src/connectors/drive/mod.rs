//! Selected native Drive reads. Behavioral attribution: HERMES-NOTICE.
mod authorization;
pub(crate) mod client;
#[cfg(test)]
mod manager_tests;
pub(crate) mod model;
#[cfg(test)]
mod model_tests;
mod reads;
pub(crate) mod store;
use crate::app::SharedDaemonState;
use crate::connectors::oauth_apps::{OAuthAppService, OAuthProvider};
use crate::state::DaemonState;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use client::DriveTransport;
pub(crate) use model::*;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Weak};
use store::DriveCredentialStore;
use tokio::sync::{Mutex, RwLock};
use zeroize::Zeroizing;
const AUTH_TTL_MS: u64 = 600_000;
#[derive(Clone)]
struct Pending {
    connector_id: String,
    agent_id: String,
    generation: String,
    selected_digest: String,
    revision: u64,
    expires_at_ms: u64,
    verifier: Zeroizing<String>,
}
struct DriveManagerInner {
    state: Weak<RwLock<DaemonState>>,
    transactions: Arc<Mutex<()>>,
    oauth_apps: OAuthAppService,
    credentials: Arc<dyn DriveCredentialStore>,
    transport: Arc<dyn DriveTransport>,
    pending: Mutex<HashMap<String, Pending>>,
    gates: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    #[cfg(test)]
    callback_clock_ms: std::sync::atomic::AtomicU64,
}
#[derive(Clone)]
pub(crate) struct DriveManager {
    inner: Arc<DriveManagerInner>,
}
#[derive(Clone)]
pub(crate) struct WeakDriveManager(Weak<DriveManagerInner>);
impl WeakDriveManager {
    pub(crate) fn upgrade(&self) -> Option<DriveManager> {
        self.0.upgrade().map(|inner| DriveManager { inner })
    }
}
impl DriveManager {
    pub(crate) fn new(
        state: &SharedDaemonState,
        transactions: Arc<Mutex<()>>,
        credentials: Arc<dyn DriveCredentialStore>,
        transport: Arc<dyn DriveTransport>,
        oauth_apps: OAuthAppService,
    ) -> Self {
        Self {
            inner: Arc::new(DriveManagerInner {
                state: Arc::downgrade(state),
                transactions,
                credentials,
                transport,
                oauth_apps,
                pending: Mutex::new(HashMap::new()),
                gates: Mutex::new(HashMap::new()),
                #[cfg(test)]
                callback_clock_ms: std::sync::atomic::AtomicU64::new(0),
            }),
        }
    }
    fn callback_now_ms(&self) -> u64 {
        #[cfg(test)]
        {
            let clock = self
                .inner
                .callback_clock_ms
                .load(std::sync::atomic::Ordering::SeqCst);
            if clock != 0 {
                return clock;
            }
        }
        now_ms()
    }
    pub(crate) fn weak(&self) -> WeakDriveManager {
        WeakDriveManager(Arc::downgrade(&self.inner))
    }
    fn shared(&self) -> Result<SharedDaemonState, DriveError> {
        self.inner.state.upgrade().ok_or(DriveError::NotFound)
    }
    async fn gate(&self, agent: &str) -> Arc<Mutex<()>> {
        self.inner
            .gates
            .lock()
            .await
            .entry(agent.into())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }
    async fn owned<T: Send + 'static>(
        &self,
        f: impl std::future::Future<Output = Result<T, DriveError>> + Send + 'static,
    ) -> Result<T, DriveError> {
        tokio::spawn(f).await.map_err(|_| DriveError::Persistence)?
    }
    pub(crate) async fn status(
        &self,
        agent: &str,
    ) -> Result<(bool, Option<DriveRecord>), DriveError> {
        let shared = self.shared()?;
        let record = {
            let s = shared.read().await;
            if s.get_agent(agent).is_none() {
                return Err(DriveError::NotFound);
            }
            s.drive_records
                .values()
                .filter(|r| r.agent_id == agent)
                .max_by_key(|r| r.updated_at_ms)
                .cloned()
        };
        let app = self
            .inner
            .oauth_apps
            .locked_config(OAuthProvider::Google)
            .await
            .map_err(|_| DriveError::Credential)?;
        Ok((app.config().is_some(), record))
    }
    pub(crate) async fn has_provider_dependency(&self, provider: OAuthProvider) -> bool {
        if provider != OAuthProvider::Google {
            return false;
        }
        let Ok(shared) = self.shared() else {
            return false;
        };
        let s = shared.read().await;
        s.drive_records
            .values()
            .any(|r| r.status != DriveStatus::Disconnected || !r.cleanup_generations.is_empty())
    }
    async fn current(&self, capture: &DriveRecord, tool: Option<&str>) -> Result<(), DriveError> {
        let shared = self.shared()?;
        let _txn = self.inner.transactions.lock().await;
        let s = shared.read().await;
        let agent = s.get_agent(&capture.agent_id).ok_or(DriveError::Conflict)?;
        if s.drive_records.get(&capture.id) != Some(capture) {
            return Err(DriveError::Conflict);
        }
        if let Some(tool) = tool {
            if capture.status != DriveStatus::Active
                || !agent
                    .state
                    .config
                    .tools
                    .as_ref()
                    .is_some_and(|tools| tools.iter().any(|t| t.name == tool))
            {
                return Err(DriveError::NotConnected);
            }
        }
        Ok(())
    }
    async fn edit_record(
        &self,
        previous: Option<DriveRecord>,
        next: DriveRecord,
        grant: Option<bool>,
    ) -> Result<(), DriveError> {
        let shared = self.shared()?;
        let _txn = self.inner.transactions.lock().await;
        let (prior_tools, next_tools, persist) = {
            let mut s = shared.write().await;
            if s.drive_records.get(&next.id) != previous.as_ref() {
                return Err(DriveError::Conflict);
            }
            let agent = s.get_agent(&next.agent_id).ok_or(DriveError::NotFound)?;
            let prior = agent.state.config.tools;
            if let Some(add) = grant {
                set_drive_tools(&mut s, &next.agent_id, add)?;
            }
            let after = s
                .get_agent(&next.agent_id)
                .and_then(|a| a.state.config.tools);
            s.drive_records.insert(next.id.clone(), next.clone());
            (prior, after, s.control_plane_persist_request())
        };
        if persist.save().await.is_err() {
            let mut s = shared.write().await;
            if s.drive_records.get(&next.id) == Some(&next) {
                if let Some(previous) = previous {
                    s.drive_records.insert(next.id.clone(), previous);
                } else {
                    s.drive_records.remove(&next.id);
                }
            }
            if s.get_agent(&next.agent_id)
                .is_some_and(|a| a.state.config.tools == next_tools)
            {
                if let Some(tools) = prior_tools {
                    s.update_agent(
                        &next.agent_id,
                        anima_core::AgentConfigUpdate {
                            tools: Some(tools),
                            ..Default::default()
                        },
                    )
                    .map_err(|_| DriveError::Persistence)?;
                } else if let Some(runtime) = s.agents.get_mut(&next.agent_id) {
                    let mut config = runtime.config().clone();
                    config.tools = None;
                    s.restore_agent_config(&next.agent_id, config);
                }
            }
            return Err(DriveError::Persistence);
        }
        Ok(())
    }
    async fn cleanup_record(&self, id: &str) -> Result<(), DriveError> {
        let shared = self.shared()?;
        let record = shared
            .read()
            .await
            .drive_records
            .get(id)
            .cloned()
            .ok_or(DriveError::NotFound)?;
        for generation in record.cleanup_generations.clone() {
            self.inner.credentials.delete(id, &generation).await?;
            let _txn = self.inner.transactions.lock().await;
            let (previous, next, persist) = {
                let mut s = shared.write().await;
                let Some(previous) = s.drive_records.get(id).cloned() else {
                    continue;
                };
                let mut next = previous.clone();
                next.cleanup_generations.retain(|g| g != &generation);
                s.drive_records.insert(id.into(), next.clone());
                (previous, next, s.control_plane_persist_request())
            };
            if persist.save().await.is_err() {
                let mut s = shared.write().await;
                if s.drive_records.get(id) == Some(&next) {
                    s.drive_records.insert(id.into(), previous);
                }
                return Err(DriveError::Persistence);
            }
        }
        Ok(())
    }
    pub(crate) async fn start_restored(&self) {
        let Ok(shared) = self.shared() else {
            return;
        };
        let ids = shared
            .read()
            .await
            .drive_records
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        for id in ids {
            let _ = self.cleanup_record(&id).await;
        }
    }
    pub(crate) async fn disconnect(&self, agent: String, id: String) -> Result<(), DriveError> {
        let manager = self.clone();
        self.owned(async move { manager.disconnect_owned(&agent, &id).await })
            .await
    }
    async fn disconnect_owned(&self, agent: &str, id: &str) -> Result<(), DriveError> {
        let gate = self.gate(agent).await;
        let _gate = gate.lock().await;
        let shared = self.shared()?;
        let previous = shared
            .read()
            .await
            .drive_records
            .get(id)
            .filter(|r| r.agent_id == agent)
            .cloned()
            .ok_or(DriveError::NotFound)?;
        if shared.read().await.get_agent(agent).is_none() {
            return Err(DriveError::NotFound);
        }
        let mut next = previous.clone();
        next.status = DriveStatus::Disconnected;
        next.updated_at_ms = now_ms();
        queue_cleanup(&mut next, previous.generation.clone())?;
        self.edit_record(Some(previous), next, Some(false)).await?;
        self.inner
            .pending
            .lock()
            .await
            .retain(|_, p| p.agent_id != agent);
        // Local revoke is complete even if deletion remains queued; never call Google's project-wide revoke.
        let _ = self.cleanup_record(id).await;
        Ok(())
    }
    pub(crate) async fn disconnect_agent(&self, agent: &str) -> Result<(), DriveError> {
        let shared = self.shared()?;
        let ids = shared
            .read()
            .await
            .drive_records
            .values()
            .filter(|r| r.agent_id == agent && r.status != DriveStatus::Disconnected)
            .map(|r| r.id.clone())
            .collect::<Vec<_>>();
        for id in ids {
            self.disconnect(agent.into(), id).await?;
        }
        Ok(())
    }
}
fn now_ms() -> u64 {
    crate::connectors::gcalendar::now_ms()
}
fn selected_ids(ids: Vec<String>) -> Result<Vec<DriveFileId>, DriveError> {
    if ids.is_empty() || ids.len() > 16 {
        return Err(DriveError::InvalidInput);
    }
    let mut selected = ids
        .into_iter()
        .map(DriveFileId::parse)
        .collect::<Result<Vec<_>, _>>()?;
    selected.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    if selected.windows(2).any(|w| w[0] == w[1]) {
        return Err(DriveError::InvalidInput);
    }
    Ok(selected)
}
fn selection_digest(ids: &[DriveFileId]) -> String {
    let joined = ids
        .iter()
        .map(|id| id.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    format!("{:x}", Sha256::digest(joined.as_bytes()))
}
fn queue_cleanup(record: &mut DriveRecord, generation: String) -> Result<(), DriveError> {
    if !record.cleanup_generations.contains(&generation) {
        if record.cleanup_generations.len() >= 16 {
            return Err(DriveError::Conflict);
        }
        record.cleanup_generations.push(generation);
    }
    Ok(())
}
fn set_drive_tools(s: &mut DaemonState, agent: &str, add: bool) -> Result<(), DriveError> {
    let snapshot = s.get_agent(agent).ok_or(DriveError::NotFound)?;
    let mut names = snapshot
        .state
        .config
        .tools
        .unwrap_or_default()
        .into_iter()
        .map(|t| t.name)
        .filter(|name| add || !DRIVE_TOOL_NAMES.contains(&name.as_str()))
        .collect::<Vec<_>>();
    if add {
        for name in DRIVE_TOOL_NAMES {
            if !names.iter().any(|n| n == name) {
                names.push(name.into());
            }
        }
    }
    let tools = s
        .tool_registry
        .resolve_descriptors(names)
        .map_err(|_| DriveError::Persistence)?;
    s.update_agent(
        agent,
        anima_core::AgentConfigUpdate {
            tools: Some(tools),
            ..Default::default()
        },
    )
    .map_err(|_| DriveError::NotFound)?;
    Ok(())
}
pub(crate) fn validate_snapshot(
    snapshot: &crate::control_plane_store::ControlPlaneSnapshot,
) -> Result<(), String> {
    let agents = snapshot
        .agents
        .iter()
        .map(|a| a.state.id.as_str())
        .collect::<HashSet<_>>();
    let mut ids = HashSet::new();
    let mut owners = HashSet::new();
    for r in &snapshot.drive_records {
        if r.id.is_empty()
            || !ids.insert(&r.id)
            || r.generation.is_empty()
            || selected_ids(
                r.selected_file_ids
                    .iter()
                    .map(|id| id.as_str().into())
                    .collect(),
            )
            .ok()
            .as_ref()
                != Some(&r.selected_file_ids)
            || r.cleanup_generations.len() > 16
            || r.cleanup_generations.iter().any(|g| g.is_empty())
            || r.cleanup_generations.iter().collect::<HashSet<_>>().len()
                != r.cleanup_generations.len()
            || (r.status != DriveStatus::Disconnected
                && (!agents.contains(r.agent_id.as_str()) || !owners.insert(&r.agent_id)))
            || (r.status == DriveStatus::Active
                && (r
                    .account_permission_id
                    .as_ref()
                    .is_none_or(|id| id.is_empty())
                    || r.oauth_config_revision == 0))
        {
            return Err("invalid native Drive authority snapshot".into());
        }
    }
    Ok(())
}

pub(crate) struct UnconfiguredDriveTransport;
#[async_trait::async_trait]
impl DriveTransport for UnconfiguredDriveTransport {
    async fn exchange_code(
        &self,
        _: &DriveOAuthConfig,
        _: &str,
        _: &str,
    ) -> Result<DriveTokenGrant, DriveError> {
        Err(DriveError::Unconfigured)
    }
    async fn refresh_tokens(
        &self,
        _: &DriveOAuthConfig,
        _: &str,
    ) -> Result<DriveTokenGrant, DriveError> {
        Err(DriveError::Unconfigured)
    }
    async fn identify(&self, _: &str) -> Result<DriveAccount, DriveError> {
        Err(DriveError::Unconfigured)
    }
    async fn metadata(&self, _: &str, _: &DriveFileId) -> Result<DriveFileMetadata, DriveError> {
        Err(DriveError::Unconfigured)
    }
    async fn read_text(
        &self,
        _: &str,
        _: &DriveFileMetadata,
    ) -> Result<DriveTextBytes, DriveError> {
        Err(DriveError::Unconfigured)
    }
}
impl DriveManager {
    pub(crate) fn unconfigured(
        state: &SharedDaemonState,
        transactions: Arc<Mutex<()>>,
        oauth_apps: OAuthAppService,
    ) -> Self {
        Self::new(
            state,
            transactions,
            Arc::new(store::InMemoryDriveCredentialStore::default()),
            Arc::new(UnconfiguredDriveTransport),
            oauth_apps,
        )
    }
}

/// Invoked under the final agent-removal transaction, never as a preceding independent save.
pub(crate) fn can_tombstone_agent(state: &DaemonState, agent: &str) -> bool {
    state
        .drive_records
        .values()
        .filter(|r| r.agent_id == agent && r.status != DriveStatus::Disconnected)
        .all(|r| r.cleanup_generations.contains(&r.generation) || r.cleanup_generations.len() < 16)
}
pub(crate) fn tombstone_agent_records(
    state: &mut DaemonState,
    agent: &str,
) -> Result<Vec<(DriveRecord, DriveRecord)>, DriveError> {
    if !can_tombstone_agent(state, agent) {
        return Err(DriveError::Conflict);
    }
    let mut delta = Vec::new();
    for record in state
        .drive_records
        .values_mut()
        .filter(|r| r.agent_id == agent && r.status != DriveStatus::Disconnected)
    {
        let previous = record.clone();
        record.status = DriveStatus::Disconnected;
        record.updated_at_ms = now_ms();
        if !record.cleanup_generations.contains(&record.generation) {
            record.cleanup_generations.push(record.generation.clone());
        }
        delta.push((previous, record.clone()));
    }
    Ok(delta)
}
pub(crate) fn rollback_agent_records(
    state: &mut DaemonState,
    delta: Vec<(DriveRecord, DriveRecord)>,
) {
    for (previous, changed) in delta {
        if state.drive_records.get(&changed.id) == Some(&changed) {
            state.drive_records.insert(previous.id.clone(), previous);
        }
    }
}
impl DriveManager {
    pub(crate) async fn cleanup_deleted_agent(&self, agent: &str) {
        let gate = self.gate(agent).await;
        let _gate = gate.lock().await;
        self.inner
            .pending
            .lock()
            .await
            .retain(|_, p| p.agent_id != agent);
        let Ok(shared) = self.shared() else {
            return;
        };
        let ids = shared
            .read()
            .await
            .drive_records
            .values()
            .filter(|r| r.agent_id == agent && r.status == DriveStatus::Disconnected)
            .map(|r| r.id.clone())
            .collect::<Vec<_>>();
        for id in ids {
            let _ = self.cleanup_record(&id).await;
        }
    }
}
