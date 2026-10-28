//! Lifecycle for separately consented, owner-selected Microsoft files.
use super::{
    client::{MicrosoftFileOAuthConfig, MicrosoftFileTransport},
    model::*,
    store::MicrosoftFileCredentialStore,
};
use crate::{
    app::SharedDaemonState,
    connectors::oauth_apps::{OAuthAppService, OAuthProvider},
    state::DaemonState,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Weak},
};
use tokio::sync::{Mutex, RwLock};
use zeroize::Zeroizing;

#[derive(Clone)]
struct Pending {
    agent_id: String,
    connector_id: String,
    generation: String,
    mode: MicrosoftFileMode,
    site_url: Option<String>,
    app_revision: u64,
    expires_at_ms: u64,
    verifier: Zeroizing<String>,
    consent_url: String,
}
#[derive(Clone)]
struct BrowserItem {
    metadata: MicrosoftFileMetadata,
    connector_id: String,
    generation: String,
    expires_at_ms: u64,
}
#[derive(Clone)]
struct BrowserCursor {
    connector_id: String,
    generation: String,
    folder: Option<(String, String)>,
    url: String,
    expires_at_ms: u64,
}
struct Inner {
    state: Weak<RwLock<DaemonState>>,
    transactions: Arc<Mutex<()>>,
    oauth_apps: OAuthAppService,
    credentials: Arc<dyn MicrosoftFileCredentialStore>,
    transport: Arc<dyn MicrosoftFileTransport>,
    pending: Mutex<HashMap<String, Pending>>,
    items: Mutex<HashMap<String, BrowserItem>>,
    cursors: Mutex<HashMap<String, BrowserCursor>>,
    gates: Mutex<HashMap<String, Arc<Mutex<()>>>>,
}
#[derive(Clone)]
pub(crate) struct MicrosoftFileManager(Arc<Inner>);
#[derive(Clone)]
pub(crate) struct WeakMicrosoftFileManager(Weak<Inner>);
impl WeakMicrosoftFileManager {
    pub(crate) fn upgrade(&self) -> Option<MicrosoftFileManager> {
        self.0.upgrade().map(MicrosoftFileManager)
    }
}

#[derive(Clone, Debug, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MicrosoftFileConnect {
    pub(crate) connector_id: String,
    pub(crate) consent_url: String,
    pub(crate) expires_at_ms: u64,
}
#[derive(Clone, Debug, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MicrosoftFileChoice {
    pub(crate) key: String,
    pub(crate) name: String,
    pub(crate) is_folder: bool,
    pub(crate) mime: Option<String>,
}
#[derive(Clone, Debug, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MicrosoftFileChoices {
    pub(crate) files: Vec<MicrosoftFileChoice>,
    pub(crate) next_cursor: Option<String>,
}
pub(crate) struct MicrosoftSelectedRead {
    pub(crate) metadata: MicrosoftFileMetadata,
    pub(crate) bytes: Option<Vec<u8>>,
    pub(crate) status: String,
    pub(crate) generation: String,
    pub(crate) selection_revision: u64,
}

fn now() -> u64 {
    crate::connectors::gcalendar::now_ms()
}
fn opaque(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}
fn canonical_site_url(value: &str) -> Result<String, MicrosoftFileError> {
    if !valid_site_url(value) {
        return Err(MicrosoftFileError::Invalid);
    }
    let url = reqwest::Url::parse(value).map_err(|_| MicrosoftFileError::Invalid)?;
    Ok(url.as_str().trim_end_matches('/').to_owned())
}
fn item_key(generation: &str, drive: &str, item: &str) -> String {
    let mut hash = Sha256::new();
    for value in [generation, drive, item] {
        hash.update((value.len() as u64).to_be_bytes());
        hash.update(value.as_bytes());
    }
    hex::encode(hash.finalize())
}

impl MicrosoftFileManager {
    pub(crate) fn new(
        state: &SharedDaemonState,
        transactions: Arc<Mutex<()>>,
        credentials: Arc<dyn MicrosoftFileCredentialStore>,
        transport: Arc<dyn MicrosoftFileTransport>,
        oauth_apps: OAuthAppService,
    ) -> Self {
        Self(Arc::new(Inner {
            state: Arc::downgrade(state),
            transactions,
            credentials,
            transport,
            oauth_apps,
            pending: Mutex::new(HashMap::new()),
            items: Mutex::new(HashMap::new()),
            cursors: Mutex::new(HashMap::new()),
            gates: Mutex::new(HashMap::new()),
        }))
    }
    pub(crate) fn unconfigured(
        state: &SharedDaemonState,
        transactions: Arc<Mutex<()>>,
        oauth_apps: OAuthAppService,
    ) -> Self {
        Self::new(
            state,
            transactions,
            Arc::new(super::store::InMemoryMicrosoftFileCredentialStore::default()),
            Arc::new(super::client::UnconfiguredMicrosoftFileTransport),
            oauth_apps,
        )
    }
    pub(crate) fn weak(&self) -> WeakMicrosoftFileManager {
        WeakMicrosoftFileManager(Arc::downgrade(&self.0))
    }
    fn shared(&self) -> Result<SharedDaemonState, MicrosoftFileError> {
        self.0
            .state
            .upgrade()
            .ok_or(MicrosoftFileError::Unavailable)
    }
    async fn gate(&self, agent: &str) -> Arc<Mutex<()>> {
        self.0
            .gates
            .lock()
            .await
            .entry(agent.into())
            .or_default()
            .clone()
    }
    async fn owned<T: Send + 'static>(
        &self,
        task: impl std::future::Future<Output = Result<T, MicrosoftFileError>> + Send + 'static,
    ) -> Result<T, MicrosoftFileError> {
        tokio::spawn(task)
            .await
            .map_err(|_| MicrosoftFileError::Persistence)?
    }

    pub(crate) async fn has_provider_dependency(&self, provider: OAuthProvider) -> bool {
        if provider != OAuthProvider::Microsoft {
            return false;
        }
        let Ok(state) = self.shared() else {
            return false;
        };
        let state = state.read().await;
        state.microsoft_file_records.values().any(|record| {
            record.status != MicrosoftFileStatus::Disconnected
                || !record.cleanup_generations.is_empty()
        })
    }
    pub(crate) async fn status(
        &self,
        agent: &str,
    ) -> Result<(bool, Vec<MicrosoftFileRecord>), MicrosoftFileError> {
        let state = self.shared()?;
        let records = {
            let state = state.read().await;
            if state.get_agent(agent).is_none() {
                return Err(MicrosoftFileError::NotFound);
            }
            state
                .microsoft_file_records
                .values()
                .filter(|record| record.agent_id == agent)
                .cloned()
                .collect()
        };
        let lease = self
            .0
            .oauth_apps
            .locked_config(OAuthProvider::Microsoft)
            .await
            .map_err(|_| MicrosoftFileError::Credential)?;
        Ok((lease.config().is_some(), records))
    }
    async fn current(
        &self,
        capture: &MicrosoftFileRecord,
        read_tool: bool,
    ) -> Result<(), MicrosoftFileError> {
        let shared = self.shared()?;
        let _transaction = self.0.transactions.lock().await;
        let state = shared.read().await;
        let agent = state
            .get_agent(&capture.agent_id)
            .ok_or(MicrosoftFileError::Conflict)?;
        if state.microsoft_file_records.get(&capture.id) != Some(capture) {
            return Err(MicrosoftFileError::Conflict);
        }
        if read_tool
            && (capture.status != MicrosoftFileStatus::Active
                || !agent.state.config.tools.as_ref().is_some_and(|tools| {
                    tools.iter().any(|tool| tool.name == "microsoft_files_read")
                }))
        {
            return Err(MicrosoftFileError::PermissionDenied);
        }
        Ok(())
    }
    async fn edit_record(
        &self,
        previous: Option<MicrosoftFileRecord>,
        next: MicrosoftFileRecord,
        grant: Option<bool>,
    ) -> Result<(), MicrosoftFileError> {
        let shared = self.shared()?;
        let _transaction = self.0.transactions.lock().await;
        let (old_index, old_rules, old_tools, new_tools, persist) = {
            let mut state = shared.write().await;
            if state.microsoft_file_records.get(&next.id) != previous.as_ref() {
                return Err(MicrosoftFileError::Conflict);
            }
            let old_tools = match state.get_agent(&next.agent_id) {
                Some(agent) => agent.state.config.tools,
                None if next.status == MicrosoftFileStatus::Disconnected && grant.is_none() => None,
                None => return Err(MicrosoftFileError::NotFound),
            };
            let old_index = state.connected_search.clone();
            let old_rules = state.account_workflows.clone();
            if let Some(add) = grant {
                let mut names = old_tools
                    .clone()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|tool| tool.name)
                    .collect::<Vec<_>>();
                if add && !names.iter().any(|name| name == "microsoft_files_read") {
                    names.push("microsoft_files_read".into());
                }
                if !add
                    && next.status != MicrosoftFileStatus::Active
                    && !state.microsoft_file_records.values().any(|record| {
                        record.id != next.id
                            && record.agent_id == next.agent_id
                            && record.status == MicrosoftFileStatus::Active
                    })
                {
                    names.retain(|name| name != "microsoft_files_read");
                }
                let tools = state
                    .tool_registry
                    .resolve_descriptors(names)
                    .map_err(|_| MicrosoftFileError::Unavailable)?;
                state
                    .update_agent(
                        &next.agent_id,
                        anima_core::AgentConfigUpdate {
                            tools: Some(tools),
                            ..Default::default()
                        },
                    )
                    .map_err(|_| MicrosoftFileError::NotFound)?;
            }
            state
                .microsoft_file_records
                .insert(next.id.clone(), next.clone());
            let selection_changed = previous.as_ref().is_some_and(|old| {
                old.selected_files != next.selected_files || old.status != next.status
            });
            if let Some(index) = state
                .connected_search
                .sources
                .get_mut(&next.id)
                .filter(|_| selection_changed)
            {
                index.verified_membership.clear();
                index.microsoft_denial_fences.clear();
                let selected = next
                    .selected_files
                    .iter()
                    .map(|file| file.key.as_str())
                    .collect::<HashSet<_>>();
                index
                    .documents
                    .retain(|document| selected.contains(document.external_id.as_str()));
                index.historical.clear();
                crate::connected_search::retain_enrichment(index);
            }
            let new_tools = state
                .get_agent(&next.agent_id)
                .and_then(|agent| agent.state.config.tools);
            (
                old_index,
                old_rules,
                old_tools,
                new_tools,
                state.control_plane_persist_request(),
            )
        };
        if persist.save().await.is_err() {
            let mut state = shared.write().await;
            if state.microsoft_file_records.get(&next.id) == Some(&next) {
                if let Some(previous) = previous {
                    state
                        .microsoft_file_records
                        .insert(next.id.clone(), previous);
                } else {
                    state.microsoft_file_records.remove(&next.id);
                }
            }
            state.connected_search = old_index;
            state.account_workflows = old_rules;
            if state
                .get_agent(&next.agent_id)
                .is_some_and(|agent| agent.state.config.tools == new_tools)
            {
                if let Some(runtime) = state.agents.get(&next.agent_id) {
                    let mut config = runtime.config().clone();
                    config.tools = old_tools;
                    state.restore_agent_config(&next.agent_id, config);
                }
            }
            return Err(MicrosoftFileError::Persistence);
        }
        Ok(())
    }

    pub(crate) async fn begin_connect(
        &self,
        agent: String,
        mode: MicrosoftFileMode,
        site_url: Option<String>,
    ) -> Result<MicrosoftFileConnect, MicrosoftFileError> {
        let manager = self.clone();
        self.owned(async move { manager.begin_owned(&agent, mode, site_url).await })
            .await
    }
    async fn begin_owned(
        &self,
        agent: &str,
        mode: MicrosoftFileMode,
        site_url: Option<String>,
    ) -> Result<MicrosoftFileConnect, MicrosoftFileError> {
        match mode {
            MicrosoftFileMode::OneDrive if site_url.is_some() => {
                return Err(MicrosoftFileError::Invalid)
            }
            MicrosoftFileMode::SharePoint
                if site_url.as_ref().is_none_or(|url| !valid_site_url(url)) =>
            {
                return Err(MicrosoftFileError::Invalid)
            }
            _ => {}
        }
        let site_url = site_url.as_deref().map(canonical_site_url).transpose()?;
        let gate = self.gate(agent).await;
        let _gate = gate.lock().await;
        let lease = self
            .0
            .oauth_apps
            .locked_config(OAuthProvider::Microsoft)
            .await
            .map_err(|_| MicrosoftFileError::Credential)?;
        let config = MicrosoftFileOAuthConfig::from_resolved(
            lease.config().ok_or(MicrosoftFileError::Unconfigured)?,
        );
        let shared = self.shared()?;
        let prior = {
            let state = shared.read().await;
            if state.get_agent(agent).is_none() {
                return Err(MicrosoftFileError::NotFound);
            }
            state
                .microsoft_file_records
                .values()
                .find(|record| {
                    record.agent_id == agent
                        && record.mode == mode
                        && record.status != MicrosoftFileStatus::Disconnected
                })
                .cloned()
        };
        if prior
            .as_ref()
            .is_some_and(|record| record.status == MicrosoftFileStatus::Active)
        {
            return Err(MicrosoftFileError::Conflict);
        }
        if let Some(existing) = self.0.pending.lock().await.values().find(|pending| {
            pending.agent_id == agent && pending.mode == mode && pending.expires_at_ms > now()
        }) {
            if existing.site_url != site_url {
                return Err(MicrosoftFileError::Conflict);
            }
            return Ok(MicrosoftFileConnect {
                connector_id: existing.connector_id.clone(),
                consent_url: existing.consent_url.clone(),
                expires_at_ms: existing.expires_at_ms,
            });
        }
        {
            let mut pending = self.0.pending.lock().await;
            pending.retain(|_, value| value.expires_at_ms > now());
            if pending.len() >= 32 {
                return Err(MicrosoftFileError::Capacity);
            }
        }
        let id = prior.as_ref().map_or_else(
            || uuid::Uuid::new_v4().to_string(),
            |record| record.id.clone(),
        );
        let generation = uuid::Uuid::new_v4().to_string();
        let verifier = Zeroizing::new(format!("{}{}", uuid::Uuid::new_v4(), uuid::Uuid::new_v4()));
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let nonce = uuid::Uuid::new_v4().to_string();
        let expires_at_ms = now().saturating_add(600_000);
        let consent_url = config.consent_url(mode, &nonce, &challenge)?;
        let mut cleanup = prior
            .as_ref()
            .map(|record| record.cleanup_generations.clone())
            .unwrap_or_default();
        if let Some(record) = &prior {
            if !cleanup.contains(&record.generation) {
                cleanup.push(record.generation.clone());
            }
        }
        if cleanup.len() >= 16 {
            return Err(MicrosoftFileError::Capacity);
        }
        cleanup.push(generation.clone());
        let record = MicrosoftFileRecord {
            id: id.clone(),
            agent_id: agent.into(),
            mode,
            status: MicrosoftFileStatus::Pairing,
            generation: generation.clone(),
            selection_revision: 1,
            oauth_config_revision: config.revision,
            account_id: None,
            account_label: None,
            approved_site_id: None,
            approved_site_url: site_url.clone(),
            selected_files: vec![],
            created_at_ms: prior
                .as_ref()
                .map_or_else(now, |record| record.created_at_ms),
            updated_at_ms: now(),
            cleanup_generations: cleanup,
        };
        self.edit_record(prior, record, Some(false)).await?;
        let mut pending = self.0.pending.lock().await;
        pending.insert(
            nonce,
            Pending {
                agent_id: agent.into(),
                connector_id: id.clone(),
                generation,
                mode,
                site_url,
                app_revision: config.revision,
                expires_at_ms,
                verifier,
                consent_url: consent_url.clone(),
            },
        );
        Ok(MicrosoftFileConnect {
            connector_id: id,
            consent_url,
            expires_at_ms,
        })
    }
    pub(crate) async fn complete_connect(
        &self,
        nonce: String,
        code: Option<String>,
    ) -> Result<(), MicrosoftFileError> {
        let manager = self.clone();
        self.owned(async move { manager.complete_owned(&nonce, code.as_deref()).await })
            .await
    }
    async fn complete_owned(
        &self,
        nonce: &str,
        code: Option<&str>,
    ) -> Result<(), MicrosoftFileError> {
        if !opaque(nonce, 128) {
            return Err(MicrosoftFileError::Invalid);
        }
        let pending = self
            .0
            .pending
            .lock()
            .await
            .remove(nonce)
            .ok_or(MicrosoftFileError::Invalid)?;
        if pending.expires_at_ms <= now() {
            return Err(MicrosoftFileError::Invalid);
        }
        let code = code
            .filter(|value| opaque(value, 8192))
            .ok_or(MicrosoftFileError::Invalid)?;
        let gate = self.gate(&pending.agent_id).await;
        let _gate = gate.lock().await;
        let lease = self
            .0
            .oauth_apps
            .locked_config(OAuthProvider::Microsoft)
            .await
            .map_err(|_| MicrosoftFileError::Credential)?;
        if lease.revision() != pending.app_revision {
            return Err(MicrosoftFileError::Conflict);
        }
        let config = MicrosoftFileOAuthConfig::from_resolved(
            lease.config().ok_or(MicrosoftFileError::Unconfigured)?,
        );
        let shared = self.shared()?;
        let previous = shared
            .read()
            .await
            .microsoft_file_records
            .get(&pending.connector_id)
            .filter(|record| {
                record.agent_id == pending.agent_id
                    && record.generation == pending.generation
                    && record.status == MicrosoftFileStatus::Pairing
            })
            .cloned()
            .ok_or(MicrosoftFileError::Conflict)?;
        let grant = self
            .0
            .transport
            .exchange(&config, pending.mode, code, &pending.verifier)
            .await?;
        let identity = self.0.transport.identity(&grant.access_token).await?;
        let site = if pending.mode == MicrosoftFileMode::SharePoint {
            Some(
                self.0
                    .transport
                    .site_id(
                        &grant.access_token,
                        previous
                            .approved_site_url
                            .as_deref()
                            .ok_or(MicrosoftFileError::Invalid)?,
                    )
                    .await?,
            )
        } else {
            None
        };
        self.current(&previous, false).await?;
        self.0
            .credentials
            .put(&previous.id, &previous.generation, grant)
            .await
            .map_err(|_| MicrosoftFileError::Credential)?;
        let mut next = previous.clone();
        next.status = MicrosoftFileStatus::Active;
        next.account_id = Some(identity.id);
        next.account_label = identity.label;
        next.approved_site_id = site;
        next.updated_at_ms = now();
        next.cleanup_generations
            .retain(|generation| generation != &next.generation);
        if let Err(error) = self
            .edit_record(Some(previous.clone()), next, Some(true))
            .await
        {
            let _ = self
                .0
                .credentials
                .delete(&previous.id, &previous.generation)
                .await;
            return Err(error);
        }
        Ok(())
    }

    async fn capture(
        &self,
        agent: &str,
        id: &str,
    ) -> Result<MicrosoftFileRecord, MicrosoftFileError> {
        let shared = self.shared()?;
        let record = shared
            .read()
            .await
            .microsoft_file_records
            .get(id)
            .filter(|record| {
                record.agent_id == agent && record.status == MicrosoftFileStatus::Active
            })
            .cloned()
            .ok_or(MicrosoftFileError::NotFound)?;
        self.current(&record, true).await?;
        Ok(record)
    }
    async fn grant(
        &self,
        record: &MicrosoftFileRecord,
        config: &MicrosoftFileOAuthConfig,
    ) -> Result<super::store::MicrosoftFileGrant, MicrosoftFileError> {
        if record.oauth_config_revision != config.revision {
            return Err(MicrosoftFileError::Conflict);
        }
        let grant = self
            .0
            .credentials
            .load(&record.id, &record.generation)
            .await
            .map_err(|_| MicrosoftFileError::Credential)?
            .ok_or(MicrosoftFileError::Reauthorization)?;
        if grant.mode != record.mode {
            return Err(MicrosoftFileError::Credential);
        }
        if grant.expires_at_ms > now().saturating_add(60_000) {
            return Ok(grant);
        }
        let refreshed = self.0.transport.refresh(config, &grant).await?;
        self.current(record, true).await?;
        self.0
            .credentials
            .put(&record.id, &record.generation, refreshed.clone())
            .await
            .map_err(|_| MicrosoftFileError::Credential)?;
        Ok(refreshed)
    }

    pub(crate) async fn browse(
        &self,
        agent: String,
        id: String,
        folder_key: Option<String>,
        cursor: Option<String>,
    ) -> Result<MicrosoftFileChoices, MicrosoftFileError> {
        let manager = self.clone();
        self.owned(async move { manager.browse_owned(&agent, &id, folder_key, cursor).await })
            .await
    }
    async fn browse_owned(
        &self,
        agent: &str,
        id: &str,
        folder_key: Option<String>,
        cursor: Option<String>,
    ) -> Result<MicrosoftFileChoices, MicrosoftFileError> {
        let gate = self.gate(agent).await;
        let _gate = gate.lock().await;
        let record = self.capture(agent, id).await?;
        let folder = if let Some(key) = folder_key {
            let items = self.0.items.lock().await;
            let item = items
                .get(&key)
                .filter(|item| {
                    item.connector_id == id
                        && item.generation == record.generation
                        && item.expires_at_ms > now()
                        && item.metadata.is_folder
                })
                .ok_or(MicrosoftFileError::NotFound)?;
            Some((
                item.metadata.drive_id.clone(),
                item.metadata.item_id.clone(),
            ))
        } else {
            None
        };
        let next = if let Some(cursor) = cursor {
            let cursors = self.0.cursors.lock().await;
            let cursor = cursors
                .get(&cursor)
                .filter(|cursor| {
                    cursor.connector_id == id
                        && cursor.generation == record.generation
                        && cursor.expires_at_ms > now()
                        && cursor.folder == folder
                })
                .ok_or(MicrosoftFileError::Conflict)?;
            Some(cursor.url.clone())
        } else {
            None
        };
        let lease = self
            .0
            .oauth_apps
            .locked_config(OAuthProvider::Microsoft)
            .await
            .map_err(|_| MicrosoftFileError::Credential)?;
        let config = MicrosoftFileOAuthConfig::from_resolved(
            lease.config().ok_or(MicrosoftFileError::Unconfigured)?,
        );
        let grant = match self.grant(&record, &config).await {
            Ok(grant) => grant,
            Err(error) => {
                if error == MicrosoftFileError::Reauthorization {
                    self.retire_source(&record).await?;
                }
                return Err(error);
            }
        };
        let page = match self
            .0
            .transport
            .list(
                &grant.access_token,
                record.mode,
                record.approved_site_id.as_deref(),
                folder
                    .as_ref()
                    .map(|(drive, item)| (drive.as_str(), item.as_str())),
                next.as_deref(),
            )
            .await
        {
            Ok(page) => page,
            Err(error) => {
                if matches!(
                    error,
                    MicrosoftFileError::Reauthorization | MicrosoftFileError::PermissionDenied
                ) {
                    self.retire_source(&record).await?;
                }
                return Err(error);
            }
        };
        self.current(&record, true).await?;
        let expires = now().saturating_add(600_000);
        let mut items = self.0.items.lock().await;
        items.retain(|_, item| item.expires_at_ms > now());
        if items.len().saturating_add(page.files.len()) > 500 {
            return Err(MicrosoftFileError::Capacity);
        }
        let mut files = Vec::new();
        for metadata in page.files {
            let key = item_key(&record.generation, &metadata.drive_id, &metadata.item_id);
            files.push(MicrosoftFileChoice {
                key: key.clone(),
                name: metadata.name.clone(),
                is_folder: metadata.is_folder,
                mime: metadata.mime.clone(),
            });
            items.insert(
                key,
                BrowserItem {
                    metadata,
                    connector_id: id.into(),
                    generation: record.generation.clone(),
                    expires_at_ms: expires,
                },
            );
        }
        drop(items);
        let next_cursor = if let Some(url) = page.next_url {
            let mut cursors = self.0.cursors.lock().await;
            cursors.retain(|_, cursor| cursor.expires_at_ms > now());
            if cursors.len() >= 128 {
                return Err(MicrosoftFileError::Capacity);
            }
            let key = uuid::Uuid::new_v4().to_string();
            cursors.insert(
                key.clone(),
                BrowserCursor {
                    connector_id: id.into(),
                    generation: record.generation.clone(),
                    folder,
                    url,
                    expires_at_ms: expires,
                },
            );
            Some(key)
        } else {
            None
        };
        Ok(MicrosoftFileChoices { files, next_cursor })
    }

    pub(crate) async fn select(
        &self,
        agent: String,
        id: String,
        generation: String,
        selection_revision: u64,
        keys: Vec<String>,
    ) -> Result<MicrosoftFileRecord, MicrosoftFileError> {
        let manager = self.clone();
        self.owned(async move {
            manager
                .select_owned(&agent, &id, &generation, selection_revision, keys)
                .await
        })
        .await
    }
    async fn select_owned(
        &self,
        agent: &str,
        id: &str,
        generation: &str,
        selection_revision: u64,
        mut keys: Vec<String>,
    ) -> Result<MicrosoftFileRecord, MicrosoftFileError> {
        if keys.len() > 16 || keys.iter().any(|key| !opaque(key, 128)) {
            return Err(MicrosoftFileError::Invalid);
        }
        keys.sort();
        if keys.windows(2).any(|values| values[0] == values[1]) {
            return Err(MicrosoftFileError::Invalid);
        }
        let gate = self.gate(agent).await;
        let _gate = gate.lock().await;
        let previous = self.capture(agent, id).await?;
        if previous.generation != generation {
            return Err(MicrosoftFileError::Conflict);
        }
        if previous
            .selected_files
            .iter()
            .map(|file| &file.key)
            .eq(keys.iter())
        {
            return Ok(previous);
        }
        if previous.selection_revision != selection_revision {
            return Err(MicrosoftFileError::Conflict);
        }
        let items = self.0.items.lock().await;
        let selected = keys
            .into_iter()
            .map(|key| {
                if let Some(existing) = previous.selected_files.iter().find(|file| file.key == key)
                {
                    return Ok(existing.clone());
                }
                let item = items
                    .get(&key)
                    .filter(|item| {
                        item.connector_id == id
                            && item.generation == generation
                            && item.expires_at_ms > now()
                            && !item.metadata.is_folder
                    })
                    .ok_or(MicrosoftFileError::NotFound)?;
                Ok(MicrosoftSelectedFile {
                    key,
                    drive_id: item.metadata.drive_id.clone(),
                    item_id: item.metadata.item_id.clone(),
                    name: item.metadata.name.clone(),
                    site_id: previous.approved_site_id.clone(),
                })
            })
            .collect::<Result<Vec<_>, MicrosoftFileError>>()?;
        drop(items);
        let mut next = previous.clone();
        next.selected_files = selected;
        next.selection_revision = next
            .selection_revision
            .checked_add(1)
            .ok_or(MicrosoftFileError::Capacity)?;
        next.updated_at_ms = now();
        self.edit_record(Some(previous), next.clone(), None).await?;
        Ok(next)
    }

    pub(crate) async fn read_selected(
        &self,
        agent: String,
        id: String,
        generation: String,
        key: String,
    ) -> Result<MicrosoftSelectedRead, MicrosoftFileError> {
        let manager = self.clone();
        self.owned(async move {
            let result = manager
                .read_selected_owned(&agent, &id, &generation, &key)
                .await;
            if let Err(error) = result.as_ref() {
                manager
                    .record_failure(&agent, &id, &generation, &key, *error)
                    .await;
            }
            result
        })
        .await
    }
    pub(crate) async fn metadata_selected(
        &self,
        agent: String,
        id: String,
        generation: String,
        key: String,
    ) -> Result<MicrosoftFileMetadata, MicrosoftFileError> {
        let manager = self.clone();
        self.owned(async move {
            let result = manager
                .metadata_selected_owned(&agent, &id, &generation, &key)
                .await;
            if let Err(error) = result.as_ref() {
                manager
                    .record_failure(&agent, &id, &generation, &key, *error)
                    .await;
            }
            result
        })
        .await
    }
    async fn metadata_selected_owned(
        &self,
        agent: &str,
        id: &str,
        generation: &str,
        key: &str,
    ) -> Result<MicrosoftFileMetadata, MicrosoftFileError> {
        let gate = self.gate(agent).await;
        let _gate = gate.lock().await;
        let record = self.capture(agent, id).await?;
        if record.generation != generation {
            return Err(MicrosoftFileError::Conflict);
        }
        let file = record
            .selected_files
            .iter()
            .find(|file| file.key == key)
            .ok_or(MicrosoftFileError::NotFound)?;
        let lease = self
            .0
            .oauth_apps
            .locked_config(OAuthProvider::Microsoft)
            .await
            .map_err(|_| MicrosoftFileError::Credential)?;
        let config = MicrosoftFileOAuthConfig::from_resolved(
            lease.config().ok_or(MicrosoftFileError::Unconfigured)?,
        );
        let grant = self.grant(&record, &config).await?;
        let mut metadata = self
            .0
            .transport
            .metadata(&grant.access_token, &file.drive_id, &file.item_id)
            .await?;
        self.current(&record, true).await?;
        if metadata.is_folder
            || metadata.item_id != file.item_id
            || metadata.drive_id != file.drive_id
        {
            return Err(MicrosoftFileError::Conflict);
        }
        let approved_host = record
            .approved_site_url
            .as_ref()
            .and_then(|url| reqwest::Url::parse(url).ok())
            .and_then(|url| url.host_str().map(str::to_owned));
        metadata.source_url = metadata.source_url.filter(|raw| {
            reqwest::Url::parse(raw).ok().is_some_and(|url| {
                url.scheme() == "https"
                    && url.username().is_empty()
                    && url.password().is_none()
                    && url.port().is_none()
                    && url.host_str().is_some_and(|host| {
                        host == "onedrive.live.com"
                            || host == "1drv.ms"
                            || (record.mode == MicrosoftFileMode::SharePoint
                                && approved_host.as_deref() == Some(host))
                    })
            })
        });
        Ok(metadata)
    }
    pub(crate) async fn read_for_agent(
        &self,
        agent: String,
        id: String,
        key: String,
    ) -> Result<MicrosoftSelectedRead, MicrosoftFileError> {
        let generation = self.capture(&agent, &id).await?.generation;
        self.read_selected(agent, id, generation, key).await
    }
    pub(crate) async fn verify_selected(
        &self,
        agent: &str,
        id: &str,
        key: &str,
        generation: &str,
        selection_revision: u64,
    ) -> Result<(), MicrosoftFileError> {
        let record = self.capture(agent, id).await?;
        if record.generation != generation || record.selection_revision != selection_revision {
            return Err(MicrosoftFileError::Conflict);
        }
        if record.selected_files.iter().any(|file| file.key == key) {
            Ok(())
        } else {
            Err(MicrosoftFileError::NotFound)
        }
    }

    async fn retire_source(
        &self,
        previous: &MicrosoftFileRecord,
    ) -> Result<(), MicrosoftFileError> {
        let mut next = previous.clone();
        next.status = MicrosoftFileStatus::ReauthRequired;
        next.updated_at_ms = now();
        let result = self
            .edit_record(Some(previous.clone()), next.clone(), Some(false))
            .await;
        if result == Err(MicrosoftFileError::Persistence) {
            // A failed save cannot justify serving content from a known-revoked source.
            let shared = self.shared()?;
            let _transaction = self.0.transactions.lock().await;
            let mut state = shared.write().await;
            if state.microsoft_file_records.get(&previous.id) != Some(previous) {
                return Err(MicrosoftFileError::Conflict);
            }
            state
                .microsoft_file_records
                .insert(previous.id.clone(), next);
            crate::connected_search::reconcile_all(&mut state);
        } else {
            result?;
        }
        self.0.items.lock().await.retain(|_, item| {
            item.connector_id != previous.id || item.generation != previous.generation
        });
        self.0.cursors.lock().await.retain(|_, cursor| {
            cursor.connector_id != previous.id || cursor.generation != previous.generation
        });
        result
    }

    async fn record_failure(
        &self,
        agent: &str,
        id: &str,
        generation: &str,
        key: &str,
        error: MicrosoftFileError,
    ) {
        if !matches!(
            error,
            MicrosoftFileError::Reauthorization
                | MicrosoftFileError::PermissionDenied
                | MicrosoftFileError::NotFound
        ) {
            return;
        }
        let Ok(shared) = self.shared() else {
            return;
        };
        let previous = shared
            .read()
            .await
            .microsoft_file_records
            .get(id)
            .filter(|record| record.agent_id == agent && record.generation == generation)
            .cloned();
        let Some(previous) = previous else {
            return;
        };
        if error == MicrosoftFileError::Reauthorization {
            let _ = self.retire_source(&previous).await;
            return;
        }
        let _transaction = self.0.transactions.lock().await;
        let persist = {
            let mut state = shared.write().await;
            if !state.microsoft_file_records.get(id).is_some_and(|record| {
                record.agent_id == agent
                    && record.generation == generation
                    && record.selected_files.iter().any(|file| file.key == key)
            }) {
                return;
            }
            if let Some(index) = state.connected_search.sources.get_mut(id) {
                index
                    .microsoft_denial_fences
                    .insert(key.to_owned(), uuid::Uuid::new_v4().to_string());
                let denied = index
                    .documents
                    .iter()
                    .chain(index.historical.values())
                    .filter(|document| document.external_id == key)
                    .map(|document| document.citation_id.clone())
                    .collect::<Vec<_>>();
                for citation in denied {
                    index.verified_membership.remove(&citation);
                }
                index
                    .documents
                    .retain(|document| document.external_id != key);
                index
                    .historical
                    .retain(|_, document| document.external_id != key);
                crate::connected_search::retain_enrichment(index);
                index.error = Some("A selected file is unavailable; results are incomplete".into());
            }
            state.control_plane_persist_request()
        };
        // A failed save must not undo a known denial in this running daemon.
        let _ = persist.save().await;
    }
    async fn read_selected_owned(
        &self,
        agent: &str,
        id: &str,
        generation: &str,
        key: &str,
    ) -> Result<MicrosoftSelectedRead, MicrosoftFileError> {
        let gate = self.gate(agent).await;
        let _gate = gate.lock().await;
        let record = self.capture(agent, id).await?;
        if record.generation != generation {
            return Err(MicrosoftFileError::Conflict);
        }
        let file = record
            .selected_files
            .iter()
            .find(|file| file.key == key)
            .ok_or(MicrosoftFileError::NotFound)?;
        let lease = self
            .0
            .oauth_apps
            .locked_config(OAuthProvider::Microsoft)
            .await
            .map_err(|_| MicrosoftFileError::Credential)?;
        let config = MicrosoftFileOAuthConfig::from_resolved(
            lease.config().ok_or(MicrosoftFileError::Unconfigured)?,
        );
        let grant = self.grant(&record, &config).await?;
        let mut metadata = self
            .0
            .transport
            .metadata(&grant.access_token, &file.drive_id, &file.item_id)
            .await?;
        self.current(&record, true).await?;
        if metadata.is_folder
            || metadata.item_id != file.item_id
            || metadata.drive_id != file.drive_id
        {
            return Err(MicrosoftFileError::Conflict);
        }
        let approved_host = record
            .approved_site_url
            .as_ref()
            .and_then(|url| reqwest::Url::parse(url).ok())
            .and_then(|url| url.host_str().map(str::to_owned));
        metadata.source_url = metadata.source_url.filter(|raw| {
            reqwest::Url::parse(raw).ok().is_some_and(|url| {
                url.scheme() == "https"
                    && url.username().is_empty()
                    && url.password().is_none()
                    && url.port().is_none()
                    && url.host_str().is_some_and(|host| {
                        host == "onedrive.live.com"
                            || host == "1drv.ms"
                            || (record.mode == MicrosoftFileMode::SharePoint
                                && approved_host.as_deref() == Some(host))
                    })
            })
        });
        let supported = matches!(
            metadata.mime.as_deref(),
            Some(
                "text/plain"
                    | "text/csv"
                    | "application/pdf"
                    | "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
                    | "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
            )
        );
        let (bytes, status) = if !supported {
            (None, "unsupported")
        } else if metadata.size.is_some_and(|size| size > 2 * 1024 * 1024) {
            (None, "tooLarge")
        } else {
            match self
                .0
                .transport
                .content(
                    &grant.access_token,
                    &file.drive_id,
                    &file.item_id,
                    approved_host.as_deref(),
                )
                .await
            {
                Ok(bytes) if bytes.len() <= 2 * 1024 * 1024 => (Some(bytes), "ready"),
                Err(
                    error @ (MicrosoftFileError::Reauthorization
                    | MicrosoftFileError::PermissionDenied
                    | MicrosoftFileError::NotFound),
                ) => return Err(error),
                _ => (None, "unavailable"),
            }
        };
        self.current(&record, true).await?;
        let after = self
            .0
            .transport
            .metadata(&grant.access_token, &file.drive_id, &file.item_id)
            .await?;
        if after.revision != metadata.revision
            || after.modified_at != metadata.modified_at
            || after.mime != metadata.mime
            || after.is_folder
        {
            return Err(MicrosoftFileError::Conflict);
        }
        self.current(&record, true).await?;
        Ok(MicrosoftSelectedRead {
            metadata,
            bytes,
            status: status.into(),
            generation: record.generation,
            selection_revision: record.selection_revision,
        })
    }

    async fn cleanup_record(&self, id: &str) -> Result<(), MicrosoftFileError> {
        let shared = self.shared()?;
        let record = shared
            .read()
            .await
            .microsoft_file_records
            .get(id)
            .cloned()
            .ok_or(MicrosoftFileError::NotFound)?;
        for generation in record.cleanup_generations.clone() {
            if record.status == MicrosoftFileStatus::Active && generation == record.generation {
                continue;
            }
            self.0
                .credentials
                .delete(id, &generation)
                .await
                .map_err(|_| MicrosoftFileError::Credential)?;
            let previous = shared
                .read()
                .await
                .microsoft_file_records
                .get(id)
                .cloned()
                .ok_or(MicrosoftFileError::NotFound)?;
            let mut next = previous.clone();
            next.cleanup_generations
                .retain(|value| value != &generation);
            next.updated_at_ms = now();
            self.edit_record(Some(previous), next, None).await?;
        }
        Ok(())
    }
    pub(crate) async fn start_restored(&self) {
        let Ok(shared) = self.shared() else {
            return;
        };
        let records = shared
            .read()
            .await
            .microsoft_file_records
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for record in records {
            let gate = self.gate(&record.agent_id).await;
            let _gate = gate.lock().await;
            if record.status == MicrosoftFileStatus::Pairing {
                let mut next = record.clone();
                next.status = MicrosoftFileStatus::Disconnected;
                next.updated_at_ms = now();
                if self
                    .edit_record(Some(record.clone()), next, Some(false))
                    .await
                    .is_err()
                {
                    continue;
                }
            }
            let _ = self.cleanup_record(&record.id).await;
        }
    }
    pub(crate) async fn disconnect(
        &self,
        agent: String,
        id: String,
        generation: String,
    ) -> Result<(), MicrosoftFileError> {
        let manager = self.clone();
        self.owned(async move { manager.disconnect_owned(&agent, &id, &generation).await })
            .await
    }
    async fn disconnect_owned(
        &self,
        agent: &str,
        id: &str,
        generation: &str,
    ) -> Result<(), MicrosoftFileError> {
        let gate = self.gate(agent).await;
        let _gate = gate.lock().await;
        let shared = self.shared()?;
        let previous = shared
            .read()
            .await
            .microsoft_file_records
            .get(id)
            .filter(|record| record.agent_id == agent)
            .cloned()
            .ok_or(MicrosoftFileError::NotFound)?;
        if previous.generation != generation {
            return Err(MicrosoftFileError::Conflict);
        }
        if previous.status == MicrosoftFileStatus::Disconnected {
            return Ok(());
        }
        let mut next = previous.clone();
        if !next.cleanup_generations.contains(&next.generation) {
            if next.cleanup_generations.len() >= 16 {
                return Err(MicrosoftFileError::Capacity);
            }
            next.cleanup_generations.push(next.generation.clone());
        }
        next.status = MicrosoftFileStatus::Disconnected;
        next.selected_files.clear();
        next.updated_at_ms = now();
        self.edit_record(Some(previous), next, Some(false)).await?;
        self.0
            .pending
            .lock()
            .await
            .retain(|_, pending| pending.connector_id != id);
        self.0
            .items
            .lock()
            .await
            .retain(|_, item| item.connector_id != id);
        self.0
            .cursors
            .lock()
            .await
            .retain(|_, cursor| cursor.connector_id != id);
        let _ = self.cleanup_record(id).await;
        Ok(())
    }
    pub(crate) async fn shutdown(&self) {
        self.0.pending.lock().await.clear();
        self.0.items.lock().await.clear();
        self.0.cursors.lock().await.clear();
    }
    pub(crate) async fn cleanup_deleted_agent(&self, agent: &str) {
        let gate = self.gate(agent).await;
        let _gate = gate.lock().await;
        self.0
            .pending
            .lock()
            .await
            .retain(|_, pending| pending.agent_id != agent);
        let Ok(shared) = self.shared() else {
            return;
        };
        let ids = shared
            .read()
            .await
            .microsoft_file_records
            .values()
            .filter(|record| {
                record.agent_id == agent && record.status == MicrosoftFileStatus::Disconnected
            })
            .map(|record| record.id.clone())
            .collect::<Vec<_>>();
        for id in ids {
            let _ = self.cleanup_record(&id).await;
        }
    }
}

pub(crate) fn can_tombstone_agent(state: &DaemonState, agent: &str) -> bool {
    state
        .microsoft_file_records
        .values()
        .filter(|record| {
            record.agent_id == agent && record.status != MicrosoftFileStatus::Disconnected
        })
        .all(|record| {
            record.cleanup_generations.contains(&record.generation)
                || record.cleanup_generations.len() < 16
        })
}
pub(crate) fn tombstone_agent_records(
    state: &mut DaemonState,
    agent: &str,
) -> Result<Vec<(MicrosoftFileRecord, MicrosoftFileRecord)>, MicrosoftFileError> {
    if !can_tombstone_agent(state, agent) {
        return Err(MicrosoftFileError::Capacity);
    }
    let mut changes = Vec::new();
    for record in state.microsoft_file_records.values_mut().filter(|record| {
        record.agent_id == agent && record.status != MicrosoftFileStatus::Disconnected
    }) {
        let previous = record.clone();
        if !record.cleanup_generations.contains(&record.generation) {
            record.cleanup_generations.push(record.generation.clone());
        }
        record.status = MicrosoftFileStatus::Disconnected;
        record.selected_files.clear();
        record.updated_at_ms = now();
        changes.push((previous, record.clone()));
    }
    Ok(changes)
}
pub(crate) fn rollback_agent_records(
    state: &mut DaemonState,
    changes: Vec<(MicrosoftFileRecord, MicrosoftFileRecord)>,
) {
    for (previous, changed) in changes {
        if state.microsoft_file_records.get(&changed.id) == Some(&changed) {
            state
                .microsoft_file_records
                .insert(previous.id.clone(), previous);
        }
    }
}

#[cfg(test)]
#[path = "manager_tests.rs"]
mod tests;
