#[path = "service_profiles.rs"]
mod service_profiles;
use super::transport::{
    BoundBrowserTask, BrowserDocumentWitness, BrowserTransportFactory, OutputSlot,
    PreparedBrowserTask, VerifiedInput,
};
use super::*;
use crate::app::SharedDaemonState;
use crate::attachments::{files::read_attachment_bytes, AttachmentKind, AttachmentRecord};
use crate::control_plane_store::WorkspaceConfig;
use crate::runs::RunSource;
use crate::sessions::SessionKind;
use anima_core::{primitives::now_millis, CancelSignal};
use anima_service_adapters::browser::{
    scope_digest, BrowserControl, BrowserEffect, BrowserError, BrowserErrorCode, BrowserMethod,
    BrowserPort, BrowserScope, BrowserSession, MAX_BROWSER_SAFE_INTEGER,
};
use anima_service_adapters::browser::{
    validate_observation, validate_operation, BrowserArtifactKind, BrowserClose, BrowserOperation,
};
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex as StdMutex, Weak,
};
use std::time::{Duration, Instant, SystemTime};
use tokio::sync::{Mutex, RwLock};

/// Metadata-stage adapter. No program lookup, spawn, context or implicit retry.
pub(crate) struct UnavailableBrowserPort;
#[async_trait]
impl BrowserPort for UnavailableBrowserPort {
    async fn open(
        &self,
        _: BrowserScope,
        _: BrowserControl,
    ) -> Result<Box<dyn BrowserSession>, BrowserError> {
        Err(BrowserError {
            code: BrowserErrorCode::Unavailable,
            effect: BrowserEffect::NotDispatched,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct WorkspaceIdentity {
    canonical_root: PathBuf,
    created: Option<SystemTime>,
}
#[derive(Clone, PartialEq, Eq)]
struct Admission {
    workspace: WorkspaceConfig,
    session_id: String,
    tool_names: Vec<String>,
    grant_digest: String,
}
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct PreparedBrowserSelection {
    profile: Option<super::profiles::BrowserProfileView>,
    agent: String,
    session: String,
    selection: BrowserSelection,
    admission: Admission,
    identity: WorkspaceIdentity,
    inputs: Vec<AttachmentRecord>,
}
#[derive(Clone)]
pub(crate) struct BrowserDispatchStamp {
    profile: Option<super::profiles::BrowserProfileView>,
    pub(crate) agent_id: String,
    pub(crate) run_id: String,
    pub(crate) session_id: String,
    pub(crate) call_id: String,
    pub(crate) tool_name: String,
    scope_id: String,
    version: u64,
    grant_digest: String,
    operation: BrowserOperation,
    expected: Option<BrowserDocumentWitness>,
}
struct ScopeRecord {
    profile: Option<super::profiles::BrowserProfileView>,
    view: BrowserScopeView,
    workspace: WorkspaceConfig,
    identity: WorkspaceIdentity,
    // Pins selected data for later action admission; caller IDs are not paths.
    _inputs: Vec<AttachmentRecord>,
    _scope: BrowserScope,
    _grant_digest: String,
    deadline: Instant,
    cancel: CancelSignal,
    task: Option<Arc<BoundBrowserTask>>,
    session: Option<Box<dyn BrowserSession>>,
    published: BTreeMap<String, BrowserArtifactView>,
    admitted_actions: u16,
    action: Arc<Mutex<()>>,
}
struct Inner {
    profile_store: super::profiles::ProfileStore,
    profile_versions: StdMutex<BTreeMap<String, super::profiles::BrowserProfileView>>,
    profile_setups: StdMutex<BTreeMap<String, service_profiles::ProfileSetup>>,
    profile_mutations: Mutex<()>,
    profile_admission_closed: AtomicBool,
    work_admission_probe: StdMutex<Option<Arc<dyn Fn() -> bool + Send + Sync>>>,
    // None is an admitted owner start preparing or waiting for its mutation gate.
    profile_work: StdMutex<BTreeMap<String, Option<Arc<BoundBrowserTask>>>>,
    state: Weak<RwLock<crate::state::DaemonState>>,
    transactions: Arc<Mutex<()>>,
    policies: InstalledBrowserPolicies,
    scopes: StdMutex<BTreeMap<String, ScopeRecord>>,
    // Deliberately not invoked by any metadata route. The worker lane is held.
    _port: Arc<dyn BrowserPort>,
    factory: Option<Arc<dyn BrowserTransportFactory>>,
    #[cfg(test)]
    publication_gate: StdMutex<Option<TestOutputPublicationGate>>,
}
#[cfg(test)]
#[derive(Clone)]
pub(crate) struct TestOutputPublicationGate {
    pub(crate) entered: Arc<tokio::sync::Semaphore>,
    pub(crate) release: Arc<tokio::sync::Semaphore>,
    pub(crate) settled: Arc<tokio::sync::Semaphore>,
    attachment_id: Arc<StdMutex<Option<String>>>,
}
#[cfg(test)]
impl TestOutputPublicationGate {
    pub(crate) fn attachment_id(&self) -> String {
        self.attachment_id
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
            .expect("publication reached the gate")
    }
}
#[cfg(test)]
struct TestOutputPublicationCompletion(Arc<tokio::sync::Semaphore>);
#[cfg(test)]
impl Drop for TestOutputPublicationCompletion {
    fn drop(&mut self) {
        self.0.add_permits(1);
    }
}
#[derive(Clone)]
pub(crate) struct BrowserService {
    inner: Arc<Inner>,
}

impl BrowserService {
    fn grants(
        state: &crate::state::DaemonState,
        agent: &str,
    ) -> Result<String, BrowserServiceError> {
        let epoch = state
            .browser_permission_epochs
            .get(agent)
            .copied()
            .unwrap_or(0);
        if epoch == u64::MAX {
            return Err(BrowserServiceError::Unavailable);
        }
        let bytes = serde_json::to_vec(&(epoch, &state.agents[agent].config().tools))
            .map_err(|_| BrowserServiceError::Unavailable)?;
        Ok(hex::encode(Sha256::digest(bytes)))
    }
    /// The owner mutation calls this only after its ordered durability ack.
    /// Closing metadata is synchronous; physical retirement runs outside guards.
    pub(crate) fn note_permissions_acknowledged(&self, agent: &str) {
        let ids = {
            let mut scopes = self.inner.scopes.lock().unwrap_or_else(|p| p.into_inner());
            scopes
                .iter_mut()
                .filter(|(_, row)| row.view.agent_id == agent)
                .map(|(id, row)| {
                    Self::close_record(row, Some("Browser tool access changed"));
                    id.clone()
                })
                .collect::<Vec<_>>()
        };
        for run in ids {
            let service = self.clone();
            tokio::spawn(async move {
                service.retire_run(&run).await;
            });
        }
    }
    fn selection_admission(
        state: &crate::state::DaemonState,
        agent: &str,
        session: &str,
    ) -> Result<Admission, BrowserServiceError> {
        let tool_names = Self::owner(state, agent)?;
        let session = state
            .sessions
            .get(agent, session)
            .filter(|s| s.kind == SessionKind::Chat && !s.archived && s.id.starts_with("chat:"))
            .ok_or(BrowserServiceError::Conflict)?;
        if state
            .pending_history_deletions
            .iter()
            .any(|d| d.agent_id == agent && d.session_id.as_ref().is_none_or(|s| s == &session.id))
        {
            return Err(BrowserServiceError::Conflict);
        }
        let workspace = state
            .workspace
            .clone()
            .filter(|w| w.root_path.is_absolute())
            .ok_or(BrowserServiceError::Conflict)?;
        Ok(Admission {
            workspace,
            session_id: session.id.clone(),
            tool_names,
            grant_digest: Self::grants(state, agent)?,
        })
    }
    pub(crate) async fn prepare_selection(
        &self,
        agent: &str,
        session: &str,
        selection: BrowserSelection,
    ) -> Result<PreparedBrowserSelection, BrowserServiceError> {
        records::validate_save(&SaveBrowserScope {
            profile_id: selection.profile_id.clone(),
            policy_id: selection.policy_id.clone(),
            selected_input_ids: selection.selected_input_ids.clone(),
            enabled: true,
            expected_version: None,
        })?;
        let policy = self
            .inner
            .policies
            .policies
            .get(&selection.policy_id)
            .ok_or(BrowserServiceError::Unavailable)?;
        if policy.template.public_ipv4_pin.is_some() && !selection.selected_input_ids.is_empty() {
            return Err(BrowserServiceError::Invalid);
        }
        let state = self.state()?;
        let (admission, store) = {
            let guard = state.read().await;
            (
                Self::selection_admission(&guard, agent, session)?,
                guard.history.store(),
            )
        };
        let identity = Self::identity(admission.workspace.root_path.clone()).await?;
        let profile = self
            .profile_for_policy(
                &admission.workspace.root_path,
                agent,
                policy,
                selection.profile_id.as_deref(),
            )
            .await?;
        let mut inputs = Vec::new();
        let mut total = 0u64;
        for id in &selection.selected_input_ids {
            let row = store
                .get_attachment(agent, id)
                .await
                .map_err(|_| BrowserServiceError::Unavailable)?
                .ok_or(BrowserServiceError::NotFound)?;
            total = total
                .checked_add(row.size)
                .filter(|n| *n <= u64::from(policy.template.limits.input_bytes))
                .ok_or(BrowserServiceError::Conflict)?;
            if row.agent_id != agent
                || row.session_id != session
                || row.kind != AttachmentKind::Text
                || row.size == 0
                || !records::digest_valid(&row.sha256)
            {
                return Err(BrowserServiceError::Conflict);
            }
            let root = admission.workspace.root_path.clone();
            let captured = row.clone();
            tokio::task::spawn_blocking(move || read_attachment_bytes(&root, &captured))
                .await
                .map_err(|_| BrowserServiceError::Unavailable)?
                .map_err(|_| BrowserServiceError::Conflict)?;
            inputs.push(row);
        }
        if Self::identity(admission.workspace.root_path.clone()).await? != identity {
            return Err(BrowserServiceError::Conflict);
        }
        Ok(PreparedBrowserSelection {
            profile,
            agent: agent.into(),
            session: session.into(),
            selection,
            admission,
            identity,
            inputs,
        })
    }
    pub(crate) fn prepared_current(
        &self,
        state: &crate::state::DaemonState,
        agent: &str,
        session: &str,
        p: &PreparedBrowserSelection,
    ) -> bool {
        self.profile_current(&p.profile)
            && p.agent == agent
            && p.session == session
            && Self::selection_admission(state, agent, session).is_ok_and(|a| a == p.admission)
    }
    pub(crate) async fn bind_prepared(
        &self,
        agent: &str,
        run: &str,
        p: PreparedBrowserSelection,
    ) -> Result<BrowserScopeView, BrowserServiceError> {
        let input = SaveBrowserScope {
            profile_id: p.selection.profile_id.clone(),
            policy_id: p.selection.policy_id.clone(),
            selected_input_ids: p.selection.selected_input_ids.clone(),
            enabled: true,
            expected_version: None,
        };
        self.save_inner(agent, run, input, Some(p)).await
    }
    pub(crate) fn new(
        state: &SharedDaemonState,
        transactions: Arc<Mutex<()>>,
        policies: InstalledBrowserPolicies,
        port: Arc<dyn BrowserPort>,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                profile_store: super::profiles::ProfileStore::default(),
                profile_versions: StdMutex::new(BTreeMap::new()),
                profile_setups: StdMutex::new(BTreeMap::new()),
                profile_mutations: Mutex::new(()),
                profile_admission_closed: AtomicBool::new(false),
                work_admission_probe: StdMutex::new(None),
                profile_work: StdMutex::new(BTreeMap::new()),
                state: Arc::downgrade(state),
                transactions,
                policies,
                scopes: StdMutex::new(BTreeMap::new()),
                _port: port,
                factory: None,
                #[cfg(test)]
                publication_gate: StdMutex::new(None),
            }),
        }
    }
    pub(crate) fn new_with_factory(
        state: &SharedDaemonState,
        transactions: Arc<Mutex<()>>,
        policies: InstalledBrowserPolicies,
        factory: Arc<dyn BrowserTransportFactory>,
    ) -> Self {
        let mut service = Self::new(
            state,
            transactions,
            policies,
            Arc::new(UnavailableBrowserPort),
        );
        Arc::get_mut(&mut service.inner)
            .expect("new service has one owner")
            .factory = Some(factory);
        service
    }
    #[cfg(test)]
    pub(crate) fn hold_next_output_publication(&self) -> TestOutputPublicationGate {
        let gate = TestOutputPublicationGate {
            entered: Arc::new(tokio::sync::Semaphore::new(0)),
            release: Arc::new(tokio::sync::Semaphore::new(0)),
            settled: Arc::new(tokio::sync::Semaphore::new(0)),
            attachment_id: Arc::new(StdMutex::new(None)),
        };
        *self
            .inner
            .publication_gate
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = Some(gate.clone());
        gate
    }
    #[cfg(test)]
    async fn pause_output_publication(&self, id: &str) -> Option<TestOutputPublicationCompletion> {
        let gate = {
            self.inner
                .publication_gate
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .take()
        };
        if let Some(gate) = gate {
            *gate.attachment_id.lock().unwrap_or_else(|p| p.into_inner()) = Some(id.into());
            gate.entered.add_permits(1);
            gate.release
                .acquire()
                .await
                .expect("publication gate remains open")
                .forget();
            Some(TestOutputPublicationCompletion(gate.settled))
        } else {
            None
        }
    }
    fn state(&self) -> Result<SharedDaemonState, BrowserServiceError> {
        self.inner
            .state
            .upgrade()
            .ok_or(BrowserServiceError::Unavailable)
    }
    fn owner(
        state: &crate::state::DaemonState,
        agent: &str,
    ) -> Result<Vec<String>, BrowserServiceError> {
        let owner = state
            .agents
            .get(agent)
            .filter(|a| !crate::agent_runs::is_helper_config(a.config()))
            .ok_or(BrowserServiceError::NotFound)?;
        let mut names = owner
            .config()
            .tools
            .iter()
            .flatten()
            .map(|t| t.name.clone())
            .collect::<Vec<_>>();
        names.sort();
        names.dedup();
        Ok(names)
    }
    fn admission(
        state: &crate::state::DaemonState,
        agent: &str,
        run: &str,
    ) -> Result<Admission, BrowserServiceError> {
        let tool_names = Self::owner(state, agent)?;
        let record = state
            .runs
            .get(run)
            .filter(|r| r.agent_id == agent)
            .ok_or(BrowserServiceError::NotFound)?;
        let session = state
            .sessions
            .get(agent, &record.session_id)
            .ok_or(BrowserServiceError::NotFound)?;
        if !record.status.is_in_flight()
            || record.stop.is_some()
            || record.parent_run_id.is_some()
            || !matches!(record.source, RunSource::Web | RunSource::Api)
            || session.kind != SessionKind::Chat
            || session.archived
            || !record.session_id.starts_with("chat:")
            || state
                .live
                .runs()
                .control(run)
                .is_none_or(|c| c.cancel.is_cancelled())
            || state.pending_history_deletions.iter().any(|d| {
                d.agent_id == agent
                    && d.session_id
                        .as_ref()
                        .is_none_or(|s| s == &record.session_id)
            })
        {
            return Err(BrowserServiceError::Conflict);
        }
        let workspace = state
            .workspace
            .clone()
            .filter(|w| w.root_path.is_absolute())
            .ok_or(BrowserServiceError::Conflict)?;
        Ok(Admission {
            workspace,
            session_id: record.session_id.clone(),
            tool_names,
            grant_digest: Self::grants(state, agent)?,
        })
    }
    async fn identity(root: PathBuf) -> Result<WorkspaceIdentity, BrowserServiceError> {
        tokio::task::spawn_blocking(move || {
            let canonical_root =
                std::fs::canonicalize(root).map_err(|_| BrowserServiceError::Conflict)?;
            let metadata =
                std::fs::metadata(&canonical_root).map_err(|_| BrowserServiceError::Conflict)?;
            if !metadata.is_dir() {
                return Err(BrowserServiceError::Conflict);
            }
            Ok(WorkspaceIdentity {
                canonical_root,
                created: metadata.created().ok(),
            })
        })
        .await
        .map_err(|_| BrowserServiceError::Unavailable)?
    }
    fn close_record(record: &mut ScopeRecord, error: Option<&str>) {
        if matches!(
            record.view.state,
            BrowserScopeState::Closed
                | BrowserScopeState::Stopping
                | BrowserScopeState::Unavailable
        ) {
            return;
        }
        record.cancel.cancel();
        record.view.enabled = false;
        record.view.state = if record.task.is_some() {
            BrowserScopeState::Stopping
        } else {
            BrowserScopeState::Closed
        };
        record.view.version = record
            .view
            .version
            .saturating_add(1)
            .min(MAX_BROWSER_SAFE_INTEGER);
        record.view.error = error.map(str::to_string);
        // Physical ownership is retained until the actor's uncancelled close.
    }
    fn expire(record: &mut ScopeRecord) {
        if Instant::now() >= record.deadline {
            Self::close_record(record, Some("Browser scope expired"));
        }
    }
    fn watch_retirement(
        &self,
        run: String,
        deadline: Instant,
        cancel: CancelSignal,
        scope_cancel: CancelSignal,
    ) {
        let weak = Arc::downgrade(&self.inner);
        tokio::spawn(async move {
            tokio::select! { _ = tokio::time::sleep_until(deadline.into()) => {}, _ = cancel.cancelled() => {}, _ = scope_cancel.cancelled() => return }
            if let Some(inner) = weak.upgrade() {
                BrowserService { inner }.retire_run(&run).await;
            }
        });
    }
    pub(crate) async fn policies(
        &self,
        agent: &str,
    ) -> Result<Vec<BrowserPolicyView>, BrowserServiceError> {
        let state = self.state()?;
        Self::owner(&*state.read().await, agent)?;
        let Some(installation) = &self.inner.policies.installation else {
            return Ok(vec![]);
        };
        self.inner
            .policies
            .policies
            .values()
            .map(|p| {
                Ok(BrowserPolicyView {
                    authenticated: p.template.authenticated,
                    id: p.id.clone(),
                    origin: p.template.origin.clone(),
                    routes: p
                        .template
                        .routes
                        .iter()
                        .map(|r| BrowserPolicyRouteView {
                            id: r.id.clone(),
                            method: match r.method {
                                BrowserMethod::Get => "GET",
                                BrowserMethod::Post => "POST",
                            }
                            .into(),
                            path: r.path.clone(),
                        })
                        .collect(),
                    policy_digest: scope_digest(&p.template)
                        .map_err(|_| BrowserServiceError::Unavailable)?,
                    installation_id: installation.id.clone(),
                    installation_digest: installation.digest.clone(),
                    limits: BrowserLimitsView::from(&p.template.limits),
                    worker_available: self.inner.factory.is_some(),
                })
            })
            .collect()
    }
    pub(crate) async fn save(
        &self,
        agent: &str,
        run: &str,
        input: SaveBrowserScope,
    ) -> Result<BrowserScopeView, BrowserServiceError> {
        self.save_inner(agent, run, input, None).await
    }
    async fn save_inner(
        &self,
        agent: &str,
        run: &str,
        input: SaveBrowserScope,
        prepared: Option<PreparedBrowserSelection>,
    ) -> Result<BrowserScopeView, BrowserServiceError> {
        records::validate_save(&input)?;
        let policy = self
            .inner
            .policies
            .policies
            .get(&input.policy_id)
            .ok_or(BrowserServiceError::Unavailable)?;
        if policy.template.public_ipv4_pin.is_some() && !input.selected_input_ids.is_empty() {
            return Err(BrowserServiceError::Invalid);
        }
        let installation = self
            .inner
            .policies
            .installation
            .as_ref()
            .ok_or(BrowserServiceError::Unavailable)?;
        let state = self.state()?;
        let (admission, store) = {
            let _transaction = self.inner.transactions.lock().await;
            let guard = state.read().await;
            (Self::admission(&guard, agent, run)?, guard.history.store())
        };
        let identity = Self::identity(admission.workspace.root_path.clone()).await?;
        let profile = self
            .profile_for_policy(
                &admission.workspace.root_path,
                agent,
                policy,
                input.profile_id.as_deref(),
            )
            .await?;
        let mut inputs = Vec::new();
        let mut input_bytes = 0u64;
        for id in &input.selected_input_ids {
            let record = store
                .get_attachment(agent, id)
                .await
                .map_err(|_| BrowserServiceError::Unavailable)?
                .ok_or(BrowserServiceError::NotFound)?;
            if record.agent_id != agent
                || record.session_id != admission.session_id
                || record.kind != AttachmentKind::Text
                || record.size == 0
                || record.size > u64::from(policy.template.limits.input_bytes)
                || !records::digest_valid(&record.sha256)
            {
                return Err(BrowserServiceError::Conflict);
            }
            input_bytes = input_bytes
                .checked_add(record.size)
                .filter(|sum| *sum <= u64::from(policy.template.limits.input_bytes))
                .ok_or(BrowserServiceError::Conflict)?;
            let root = admission.workspace.root_path.clone();
            let captured = record.clone();
            tokio::task::spawn_blocking(move || read_attachment_bytes(&root, &captured))
                .await
                .map_err(|_| BrowserServiceError::Unavailable)?
                .map_err(|_| BrowserServiceError::Conflict)?;
            inputs.push(record);
        }
        if Self::identity(admission.workspace.root_path.clone()).await? != identity {
            return Err(BrowserServiceError::Conflict);
        }
        if prepared.as_ref().is_some_and(|p| {
            p.agent != agent
                || p.session != admission.session_id
                || p.admission != admission
                || p.identity != identity
                || p.inputs != inputs
                || p.profile != profile
        }) {
            return Err(BrowserServiceError::Conflict);
        }
        // Lock order: existing ordered CP transaction -> state -> leaf scope
        // mutex. All attachment/history/workspace reads happened above.
        let _transaction = self.inner.transactions.lock().await;
        let guard = state.read().await;
        if Self::admission(&guard, agent, run)? != admission {
            return Err(BrowserServiceError::Conflict);
        }
        let mut scopes = self.inner.scopes.lock().unwrap_or_else(|p| p.into_inner());
        let now = now_millis();
        let (id, version, expires_at_ms, deadline, scope_cancel) =
            if let Some(old) = scopes.get_mut(run) {
                Self::expire(old);
                if old.view.agent_id != agent
                    || input.expected_version != Some(old.view.version)
                    || !matches!(
                        old.view.state,
                        BrowserScopeState::Disabled | BrowserScopeState::Scoped
                    )
                    || old.workspace != admission.workspace
                    || old.identity != identity
                    || old.view.physical_lease_held
                    || old.view.policy_id != input.policy_id
                {
                    return Err(BrowserServiceError::Conflict);
                }
                let version = old
                    .view
                    .version
                    .checked_add(1)
                    .filter(|v| *v <= MAX_BROWSER_SAFE_INTEGER)
                    .ok_or(BrowserServiceError::Conflict)?;
                (
                    old.view.id.clone(),
                    version,
                    old.view.expires_at_ms,
                    old.deadline,
                    old.cancel.clone(),
                )
            } else {
                if input.expected_version.is_some() {
                    return Err(BrowserServiceError::Conflict);
                }
                // Prune only finished runs without a physical lease. Live expired
                // records remain tombstones and cannot be silently renewed.
                scopes.retain(|id, row| {
                    row.view.physical_lease_held
                        || guard.runs.get(id).is_some_and(|r| !r.status.is_terminal())
                });
                if scopes.len() >= 64 {
                    return Err(BrowserServiceError::Busy);
                }
                let lifetime = Duration::from_millis(u64::from(policy.template.limits.scope_ms));
                (
                    uuid::Uuid::new_v4().to_string(),
                    1,
                    now.saturating_add(lifetime.as_millis() as u64),
                    Instant::now() + lifetime,
                    CancelSignal::new(),
                )
            };
        let mut scope = policy.template.clone();
        scope.id = id.clone();
        scope.revision = version;
        scope.input_ids = input.selected_input_ids.clone();
        anima_service_adapters::browser::validate_scope(&scope)
            .map_err(|_| BrowserServiceError::Invalid)?;
        let view = BrowserScopeView {
            profile: profile.clone(),
            id,
            agent_id: agent.into(),
            run_id: run.into(),
            session_id: admission.session_id,
            version,
            enabled: input.enabled,
            state: if input.enabled {
                BrowserScopeState::Scoped
            } else {
                BrowserScopeState::Disabled
            },
            policy_id: policy.id.clone(),
            origin: scope.origin.clone(),
            policy_digest: scope_digest(&policy.template)
                .map_err(|_| BrowserServiceError::Unavailable)?,
            installation_id: installation.id.clone(),
            installation_digest: installation.digest.clone(),
            selected_input_ids: input.selected_input_ids,
            expires_at_ms,
            actions_used: 0,
            requests_used: 0,
            wire_bytes_used: 0,
            physical_lease_held: false,
            document: None,
            artifacts: vec![],
            tool_names: admission.tool_names,
            error: None,
        };
        let run_cancel = guard
            .live
            .runs()
            .control(run)
            .ok_or(BrowserServiceError::Conflict)?
            .cancel
            .clone();
        scopes.insert(
            run.into(),
            ScopeRecord {
                profile,
                view: view.clone(),
                workspace: admission.workspace,
                identity,
                _inputs: inputs,
                _scope: scope,
                _grant_digest: admission.grant_digest,
                deadline,
                cancel: scope_cancel.clone(),
                task: None,
                session: None,
                published: BTreeMap::new(),
                admitted_actions: 0,
                action: Arc::new(Mutex::new(())),
            },
        );
        drop(scopes);
        drop(guard);
        drop(_transaction);
        if version == 1 {
            self.watch_retirement(run.into(), deadline, run_cancel, scope_cancel);
        }
        Ok(view)
    }
    pub(crate) async fn get(
        &self,
        agent: &str,
        run: &str,
    ) -> Result<Option<BrowserScopeView>, BrowserServiceError> {
        let state = self.state()?;
        // Do not retire a stop/terminal candidate before its existing ordered
        // durability acknowledgement (a failed Stop restores the prior run).
        let _transaction = self.inner.transactions.lock().await;
        let guard = state.read().await;
        let names = Self::owner(&guard, agent)?;
        let ledger = guard
            .runs
            .get(run)
            .filter(|r| r.agent_id == agent)
            .ok_or(BrowserServiceError::NotFound)?;
        let mut scopes = self.inner.scopes.lock().unwrap_or_else(|p| p.into_inner());
        let Some(row) = scopes.get_mut(run) else {
            return Ok(None);
        };
        Self::expire(row);
        if row.cancel.is_cancelled()
            || ledger.status.is_terminal()
            || ledger.stop.is_some()
            || guard
                .live
                .runs()
                .control(run)
                .is_none_or(|c| c.cancel.is_cancelled())
            || guard.workspace.as_ref() != Some(&row.workspace)
            || !guard.sessions.contains(agent, &row.view.session_id)
        {
            Self::close_record(row, Some("Browser run is no longer available"));
        }
        row.view.tool_names = names;
        if let Some(task) = row.task.clone() {
            let snapshot = task.handle.snapshot();
            Self::merge_known_counters(row, &snapshot);
            row.view.physical_lease_held = snapshot.physical_lease_held;
            if let Some(closed) = snapshot.close {
                row.view.state = if closed == BrowserClose::Closed {
                    BrowserScopeState::Closed
                } else {
                    BrowserScopeState::Unavailable
                };
            }
        }
        let view = row.view.clone();
        let closing = matches!(view.state, BrowserScopeState::Stopping);
        drop(scopes);
        drop(guard);
        drop(_transaction);
        if closing {
            let service = self.clone();
            let run = run.to_string();
            tokio::spawn(async move {
                service.retire_run(&run).await;
            });
        }
        Ok(Some(view))
    }
    pub(crate) async fn list(
        &self,
        agent: &str,
    ) -> Result<Vec<BrowserScopeView>, BrowserServiceError> {
        let state = self.state()?;
        Self::owner(&*state.read().await, agent)?;
        let ids = {
            let scopes = self.inner.scopes.lock().unwrap_or_else(|p| p.into_inner());
            scopes
                .iter()
                .filter(|(_, r)| r.view.agent_id == agent)
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>()
        };
        let mut result = Vec::new();
        for id in ids {
            match self.get(agent, &id).await {
                Ok(Some(view)) => result.push(view),
                Ok(None) | Err(BrowserServiceError::NotFound) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(result)
    }
    pub(crate) async fn stop(
        &self,
        agent: &str,
        run: &str,
        version: u64,
    ) -> Result<BrowserScopeView, BrowserServiceError> {
        if version == 0 || version > MAX_BROWSER_SAFE_INTEGER {
            return Err(BrowserServiceError::Invalid);
        }
        {
            let state = self.state()?;
            let _transaction = self.inner.transactions.lock().await;
            let guard = state.read().await;
            let names = Self::owner(&guard, agent)?;
            if !guard.runs.get(run).is_some_and(|r| r.agent_id == agent) {
                return Err(BrowserServiceError::NotFound);
            }
            let mut scopes = self.inner.scopes.lock().unwrap_or_else(|p| p.into_inner());
            let row = scopes
                .get_mut(run)
                .filter(|r| r.view.agent_id == agent)
                .ok_or(BrowserServiceError::NotFound)?;
            Self::expire(row);
            if row.view.version != version {
                return Err(BrowserServiceError::Conflict);
            }
            Self::close_record(row, None);
            row.view.tool_names = names;
        }
        self.retire_run(run).await;
        self.get(agent, run)
            .await?
            .ok_or(BrowserServiceError::NotFound)
    }
    /// Only called after the existing run Stop/terminal settlement releases
    /// its ordered CP/state guards. Scope Stop needs no new tool approval.
    pub(crate) async fn retire_run(&self, run: &str) {
        let service = self.clone();
        let run = run.to_string();
        let _ = tokio::spawn(async move {
            let task = {
                let mut scopes = service
                    .inner
                    .scopes
                    .lock()
                    .unwrap_or_else(|p| p.into_inner());
                let Some(row) = scopes.get_mut(&run) else {
                    return;
                };
                Self::close_record(row, None);
                row.task.clone()
            };
            if let Some(task) = task {
                let closed = task.handle.close().await;
                let snapshot = task.handle.snapshot();
                let mut scopes = service
                    .inner
                    .scopes
                    .lock()
                    .unwrap_or_else(|p| p.into_inner());
                if let Some(row) = scopes.get_mut(&run) {
                    Self::merge_known_counters(row, &snapshot);
                    row.view.physical_lease_held = snapshot.physical_lease_held;
                    row.view.state =
                        if closed == BrowserClose::Closed && !snapshot.physical_lease_held {
                            BrowserScopeState::Closed
                        } else {
                            BrowserScopeState::Unavailable
                        };
                }
            }
        })
        .await;
    }
    pub(crate) fn request_shutdown(&self) {
        self.inner
            .profile_admission_closed
            .store(true, Ordering::Release);
    }
    pub(crate) async fn shutdown(&self) {
        self.request_shutdown();
        // An owner sign-in may still be opening before it enters the setup table.
        // Join that mutation first so shutdown also owns its physical retirement.
        let profile_shutdown_gate = self.inner.profile_mutations.lock().await;
        let profiles = self
            .inner
            .profile_setups
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        for id in profiles {
            self.retire_profile(&id).await;
        }
        // A failed finish may remove setup metadata before physical closure.
        // Retire retained bound owners too; uncertain closes stay drain-visible.
        let tracked_owners = self
            .inner
            .profile_work
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .values()
            .filter_map(Clone::clone)
            .collect::<Vec<_>>();
        for task in tracked_owners {
            task.handle.close().await;
        }
        self.profile_in_flight_count();
        drop(profile_shutdown_gate);
        let ids = {
            self.inner
                .scopes
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .keys()
                .cloned()
                .collect::<Vec<_>>()
        };
        for id in ids {
            self.retire_run(&id).await;
        }
    }
    #[cfg(test)]
    pub(crate) fn private_witness(
        &self,
        run: &str,
    ) -> Option<super::transport::BrowserPrivateWitness> {
        self.inner
            .scopes
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(run)?
            .task
            .as_ref()?
            .handle
            .private_witness()
    }
}

fn browser_error(code: BrowserErrorCode) -> BrowserError {
    BrowserError {
        code,
        effect: BrowserEffect::NotDispatched,
    }
}
fn document_view(w: &BrowserDocumentWitness) -> BrowserDocumentView {
    BrowserDocumentView {
        document_id: w.document_id.clone(),
        document_generation: w.document_generation,
        observation_digest: w.observation_digest.clone(),
    }
}
fn observed_digest(
    observation: &anima_service_adapters::browser::BrowserObservation,
) -> Result<String, BrowserError> {
    // Validated observation objects have fixed ASCII keys and safe integer
    // numbers. Sorted serde_json maps encode their RFC8785 form exactly.
    let value =
        serde_json::to_value(observation).map_err(|_| browser_error(BrowserErrorCode::Protocol))?;
    Ok(hex::encode(Sha256::digest(
        serde_json::to_vec(&value).map_err(|_| browser_error(BrowserErrorCode::Protocol))?,
    )))
}
struct AbandonedBrowserCall(Option<CancelSignal>);
impl Drop for AbandonedBrowserCall {
    fn drop(&mut self) {
        if let Some(cancel) = self.0.take() {
            cancel.cancel();
        }
    }
}

impl BrowserService {
    fn merge_known_counters(
        row: &mut ScopeRecord,
        snapshot: &super::transport::BrowserTransportSnapshot,
    ) {
        row.view.actions_used = row.view.actions_used.max(snapshot.counters.actions_used);
        row.view.requests_used = row.view.requests_used.max(snapshot.counters.requests_used);
        row.view.wire_bytes_used = row
            .view
            .wire_bytes_used
            .max(snapshot.counters.wire_bytes_used);
    }
    pub(crate) async fn capture_dispatch(
        &self,
        run: &crate::runs::RunLink,
        call: &anima_core::ToolCall,
        operation: BrowserOperation,
        expected: Option<BrowserDocumentWitness>,
    ) -> Result<BrowserDispatchStamp, BrowserError> {
        let state = self
            .state()
            .map_err(|_| browser_error(BrowserErrorCode::Unavailable))?;
        let _transaction = self.inner.transactions.lock().await;
        let guard = state.read().await;
        let admission = Self::admission(&guard, &run.agent_id, &run.run_id)
            .map_err(|_| browser_error(BrowserErrorCode::Denied))?;
        if admission.session_id != run.session_id
            || !guard.agents[&run.agent_id].config().allows_tool(&call.name)
        {
            return Err(browser_error(BrowserErrorCode::Denied));
        }
        let scopes = self.inner.scopes.lock().unwrap_or_else(|p| p.into_inner());
        let row = scopes
            .get(&run.run_id)
            .ok_or_else(|| browser_error(BrowserErrorCode::Unavailable))?;
        validate_operation(&row._scope, &operation)?;
        if !self.profile_current(&row.profile) {
            return Err(browser_error(BrowserErrorCode::Stale));
        }
        let stamp = BrowserDispatchStamp {
            profile: row.profile.clone(),
            agent_id: run.agent_id.clone(),
            run_id: run.run_id.clone(),
            session_id: run.session_id.clone(),
            call_id: call.id.clone(),
            tool_name: call.name.clone(),
            scope_id: row.view.id.clone(),
            version: row.view.version,
            grant_digest: admission.grant_digest,
            operation,
            expected,
        };
        Self::check_row(row, &stamp)?;
        Ok(stamp)
    }
    fn check_row(row: &ScopeRecord, stamp: &BrowserDispatchStamp) -> Result<(), BrowserError> {
        if row.cancel.is_cancelled() || !row.view.enabled || Instant::now() >= row.deadline {
            return Err(browser_error(BrowserErrorCode::Cancelled));
        }
        if row.profile != stamp.profile
            || row.view.agent_id != stamp.agent_id
            || row.view.session_id != stamp.session_id
            || row.view.id != stamp.scope_id
            || row.view.version != stamp.version
            || !matches!(
                row.view.state,
                BrowserScopeState::Scoped | BrowserScopeState::Active
            )
            || stamp
                .expected
                .as_ref()
                .is_some_and(|w| row.view.document.as_ref() != Some(&document_view(w)))
        {
            return Err(browser_error(BrowserErrorCode::Stale));
        }
        if row.admitted_actions >= row._scope.limits.actions {
            return Err(browser_error(BrowserErrorCode::LimitsExceeded));
        }
        Ok(())
    }
    // Caller holds the existing CP transaction. This helper performs no I/O,
    // awaits or recursive transaction acquisition; leaf guards end on return.
    fn revalidate_dispatch_locked(
        &self,
        state: &crate::state::DaemonState,
        stamp: &BrowserDispatchStamp,
    ) -> Result<(), BrowserError> {
        let admission = Self::admission(state, &stamp.agent_id, &stamp.run_id)
            .map_err(|_| browser_error(BrowserErrorCode::Denied))?;
        if admission.grant_digest != stamp.grant_digest
            || !state.agents[&stamp.agent_id]
                .config()
                .allows_tool(&stamp.tool_name)
        {
            return Err(browser_error(BrowserErrorCode::Denied));
        }
        let scopes = self.inner.scopes.lock().unwrap_or_else(|p| p.into_inner());
        let row = scopes
            .get(&stamp.run_id)
            .ok_or_else(|| browser_error(BrowserErrorCode::Stale))?;
        if !self.profile_current(&row.profile) || admission.workspace != row.workspace {
            return Err(browser_error(BrowserErrorCode::Stale));
        }
        Self::check_row(row, stamp)
    }
    pub(crate) async fn revalidate_dispatch(
        &self,
        stamp: &BrowserDispatchStamp,
    ) -> Result<(), BrowserError> {
        let state = self
            .state()
            .map_err(|_| browser_error(BrowserErrorCode::Unavailable))?;
        let (workspace, identity, inputs, profile) = {
            let _transaction = self.inner.transactions.lock().await;
            let guard = state.read().await;
            let admission = Self::admission(&guard, &stamp.agent_id, &stamp.run_id)
                .map_err(|_| browser_error(BrowserErrorCode::Denied))?;
            if admission.grant_digest != stamp.grant_digest
                || !guard.agents[&stamp.agent_id]
                    .config()
                    .allows_tool(&stamp.tool_name)
            {
                return Err(browser_error(BrowserErrorCode::Denied));
            }
            let scopes = self.inner.scopes.lock().unwrap_or_else(|p| p.into_inner());
            let row = scopes
                .get(&stamp.run_id)
                .ok_or_else(|| browser_error(BrowserErrorCode::Stale))?;
            Self::check_row(row, stamp)?;
            if admission.workspace != row.workspace {
                return Err(browser_error(BrowserErrorCode::Stale));
            }
            (
                row.workspace.clone(),
                row.identity.clone(),
                row._inputs.clone(),
                row.profile.clone(),
            )
        };
        if Self::identity(workspace.root_path.clone())
            .await
            .map_err(|_| browser_error(BrowserErrorCode::Stale))?
            != identity
        {
            return Err(browser_error(BrowserErrorCode::Stale));
        }
        self.revalidate_profile_disk(workspace.root_path.clone(), profile)
            .await?;
        for input in inputs {
            let root = workspace.root_path.clone();
            tokio::task::spawn_blocking(move || read_attachment_bytes(&root, &input))
                .await
                .map_err(|_| browser_error(BrowserErrorCode::Unavailable))?
                .map_err(|_| browser_error(BrowserErrorCode::Stale))?;
        }
        let _transaction = self.inner.transactions.lock().await;
        let guard = state.read().await;
        self.revalidate_dispatch_locked(&guard, stamp)
    }
    pub(crate) async fn invoke(
        &self,
        stamp: BrowserDispatchStamp,
    ) -> Result<anima_core::Content, BrowserError> {
        self.revalidate_dispatch(&stamp).await?;
        let cancel = {
            let rows = self.inner.scopes.lock().unwrap_or_else(|p| p.into_inner());
            rows.get(&stamp.run_id)
                .ok_or_else(|| browser_error(BrowserErrorCode::Stale))?
                .cancel
                .clone()
        };
        let mut abandoned = AbandonedBrowserCall(Some(cancel));
        let service = self.clone();
        let result = tokio::spawn(async move { service.invoke_owned(stamp).await })
            .await
            .map_err(|_| browser_error(BrowserErrorCode::Unavailable))?;
        abandoned.0 = None;
        result
    }
    async fn invoke_owned(
        &self,
        stamp: BrowserDispatchStamp,
    ) -> Result<anima_core::Content, BrowserError> {
        let (
            action,
            scope,
            inputs,
            deadline,
            cancel,
            existing,
            view,
            workspace,
            identity,
            published,
        ) = {
            let rows = self.inner.scopes.lock().unwrap_or_else(|p| p.into_inner());
            let row = rows
                .get(&stamp.run_id)
                .ok_or_else(|| browser_error(BrowserErrorCode::Stale))?;
            Self::check_row(row, &stamp)?;
            (
                row.action.clone(),
                row._scope.clone(),
                row._inputs.clone(),
                row.deadline,
                row.cancel.clone(),
                row.task.clone(),
                row.view.clone(),
                row.workspace.clone(),
                row.identity.clone(),
                row.published.clone(),
            )
        };
        let _action = action
            .try_lock_owned()
            .map_err(|_| browser_error(BrowserErrorCode::Unavailable))?;
        self.revalidate_dispatch(&stamp).await?;
        let task = if let Some(task) = existing {
            task
        } else {
            let factory = self
                .inner
                .factory
                .as_ref()
                .ok_or_else(|| browser_error(BrowserErrorCode::Unavailable))?;
            let mut verified = Vec::new();
            for input in &inputs {
                let root = workspace.root_path.clone();
                let captured = input.clone();
                let bytes =
                    tokio::task::spawn_blocking(move || read_attachment_bytes(&root, &captured))
                        .await
                        .map_err(|_| browser_error(BrowserErrorCode::Unavailable))?
                        .map_err(|_| browser_error(BrowserErrorCode::Stale))?;
                verified.push(VerifiedInput {
                    id: input.id.clone(),
                    bytes: Arc::from(bytes),
                    sha256: input.sha256.clone(),
                });
            }
            let outputs = [
                BrowserArtifactKind::Csv,
                BrowserArtifactKind::Screenshot,
                BrowserArtifactKind::Csv,
                BrowserArtifactKind::Screenshot,
            ]
            .into_iter()
            .map(|kind| OutputSlot {
                id: format!("output-{}", uuid::Uuid::new_v4()),
                kind,
            })
            .collect();
            let profile = if let Some(view) = &view.profile {
                let store = self.inner.profile_store.clone();
                let workspace = workspace.root_path.clone();
                let captured = view.clone();
                let login_route = scope
                    .routes
                    .iter()
                    .find(|r| r.method == BrowserMethod::Get)
                    .ok_or_else(|| browser_error(BrowserErrorCode::Invalid))?
                    .id
                    .clone();
                Some(
                    tokio::task::spawn_blocking(move || {
                        store.acquire(&workspace, &captured, false, login_route)
                    })
                    .await
                    .map_err(|_| browser_error(BrowserErrorCode::Unavailable))?
                    .map_err(|_| browser_error(BrowserErrorCode::Unavailable))?,
                )
            } else {
                None
            };
            let task = Arc::new(factory.bind(PreparedBrowserTask {
                profile,
                scope: scope.clone(),
                policy_id: view.policy_id.clone(),
                policy_digest: view.policy_digest.clone(),
                installation_id: view.installation_id.clone(),
                installation_digest: view.installation_digest.clone(),
                expires_at_ms: view.expires_at_ms,
                inputs: verified,
                outputs,
            })?);
            self.revalidate_dispatch(&stamp).await?;
            {
                let mut rows = self.inner.scopes.lock().unwrap_or_else(|p| p.into_inner());
                let row = rows
                    .get_mut(&stamp.run_id)
                    .ok_or_else(|| browser_error(BrowserErrorCode::Stale))?;
                Self::check_row(row, &stamp)?;
                row.task = Some(task.clone());
                row.view.state = BrowserScopeState::Active;
                row.view.physical_lease_held = true;
            }
            let opened = task
                .port
                .open(
                    scope.clone(),
                    BrowserControl {
                        cancel: cancel.clone(),
                        deadline,
                    },
                )
                .await;
            match opened {
                Ok(session) => {
                    let mut rows = self.inner.scopes.lock().unwrap_or_else(|p| p.into_inner());
                    if let Some(row) = rows.get_mut(&stamp.run_id) {
                        row.session = Some(session);
                    }
                }
                Err(error) => {
                    self.retire_run(&stamp.run_id).await;
                    return Err(error);
                }
            }
            task
        };
        let result=async {
            self.revalidate_dispatch(&stamp).await?;
            let reply=task.handle.execute_admitted(stamp.operation.clone(),stamp.expected.clone(),BrowserControl{cancel:cancel.clone(),deadline}).await?;
            let post=async {
                validate_observation(&scope,&reply.observation)?;
                if reply.document.document_generation!=reply.observation.document_generation || observed_digest(&reply.observation)?!=reply.document.observation_digest {return Err(browser_error(BrowserErrorCode::Protocol));}
                self.revalidate_dispatch(&stamp).await?;
                let mut accepted=Vec::new();
                for artifact in &reply.observation.artifacts {
                    if let Some(existing)=published.get(&artifact.id) {let kind=if artifact.kind==BrowserArtifactKind::Csv{BrowserArtifactKindView::Csv}else{BrowserArtifactKindView::Screenshot};if existing.bytes!=artifact.bytes||existing.sha256!=artifact.sha256||existing.kind!=kind{return Err(browser_error(BrowserErrorCode::Protocol));}accepted.push(existing.clone());continue;}
                    let bytes=task.handle.read_output(artifact).await?;
                    if bytes.bytes.len() as u64!=artifact.bytes || bytes.sha256!=artifact.sha256 || hex::encode(Sha256::digest(&bytes.bytes))!=artifact.sha256 {return Err(browser_error(BrowserErrorCode::Protocol));}
                    self.revalidate_dispatch(&stamp).await?;
                    if Self::identity(workspace.root_path.clone()).await.map_err(|_|browser_error(BrowserErrorCode::Stale))?!=identity {return Err(browser_error(BrowserErrorCode::Stale));}
                    let attachment_id=format!("att-{}",uuid::Uuid::new_v4());
                    let name=format!("browser-{}.{}",attachment_id,if artifact.kind==BrowserArtifactKind::Csv{"csv"}else{"jpg"});
                    #[cfg(test)]
                    let _publication_completion=self.pause_output_publication(&attachment_id).await;
                    // Follow existing owned attachment upload ordering: Stop,
                    // grant changes and history deletion share this transaction.
                    // Actor output extraction is complete; only this CP guard
                    // crosses file/history I/O, never state/std/actor guards.
                    let state=self.state().map_err(|_|browser_error(BrowserErrorCode::Unavailable))?;
                    let publication_transaction=self.inner.transactions.lock().await;
                    let store={let guard=state.read().await;self.revalidate_dispatch_locked(&guard,&stamp)?;guard.history.store()};
                    let root=workspace.root_path.clone();let captured=bytes.bytes.clone();let filename=name.clone();let date=chrono::Utc::now().format("%Y-%m-%d").to_string();
                    let saved=tokio::task::spawn_blocking(move||crate::attachments::files::save_upload(&root,&date,&filename,&captured)).await.map_err(|_|browser_error(BrowserErrorCode::Unavailable))?.map_err(|_|browser_error(BrowserErrorCode::Unavailable))?;
                    if saved.workspace_root!=identity.canonical_root.to_str().unwrap_or("") || saved.sha256!=artifact.sha256 {return Err(browser_error(BrowserErrorCode::Stale));}
                    let row=AttachmentRecord{id:attachment_id,agent_id:stamp.agent_id.clone(),session_id:stamp.session_id.clone(),name,mime:if artifact.kind==BrowserArtifactKind::Csv{"text/csv"}else{"image/jpeg"}.into(),size:artifact.bytes,kind:if artifact.kind==BrowserArtifactKind::Csv{AttachmentKind::Text}else{AttachmentKind::Image},path:saved.path,created_at_ms:now_millis(),workspace_root:saved.workspace_root,sha256:saved.sha256};
                    store.upsert_attachments(std::slice::from_ref(&row)).await.map_err(|_|browser_error(BrowserErrorCode::Unavailable))?;
                    drop(publication_transaction);
                    self.revalidate_dispatch(&stamp).await?;
                    accepted.push(BrowserArtifactView{id:row.id,kind:if artifact.kind==BrowserArtifactKind::Csv{BrowserArtifactKindView::Csv}else{BrowserArtifactKindView::Screenshot},bytes:row.size,sha256:row.sha256});
                }
                {let mut rows=self.inner.scopes.lock().unwrap_or_else(|p|p.into_inner());let row=rows.get_mut(&stamp.run_id).ok_or_else(||browser_error(BrowserErrorCode::Stale))?;Self::check_row(row,&stamp)?;
                    if reply.counters.actions_used<=row.admitted_actions || reply.counters.actions_used>scope.limits.actions || reply.counters.requests_used>scope.limits.requests || reply.counters.wire_bytes_used>scope.limits.wire_bytes {return Err(browser_error(BrowserErrorCode::Protocol));}
                    for (private,artifact) in reply.observation.artifacts.iter().zip(&accepted) {row.published.insert(private.id.clone(),artifact.clone());if !row.view.artifacts.iter().any(|a|a.id==artifact.id){if row.view.artifacts.len()>=4{return Err(browser_error(BrowserErrorCode::LimitsExceeded));}row.view.artifacts.push(artifact.clone());}}
                    row.admitted_actions=reply.counters.actions_used;row.view.actions_used=row.view.actions_used.max(reply.counters.actions_used);row.view.requests_used=row.view.requests_used.max(reply.counters.requests_used);row.view.wire_bytes_used=row.view.wire_bytes_used.max(reply.counters.wire_bytes_used);row.view.document=Some(document_view(&reply.document));row.view.physical_lease_held=task.handle.snapshot().physical_lease_held;
                }
                let route=scope.routes.iter().find(|r|r.id==reply.observation.route_id).ok_or_else(||browser_error(BrowserErrorCode::Protocol))?;
                let provenance=serde_json::json!({"scopeId":scope.id,"scopeRevision":scope.revision,"runId":stamp.run_id,"policyId":view.policy_id,"policyDigest":view.policy_digest,"installationId":view.installation_id,"profile":view.profile,"contextId":reply.context_id,"pageId":reply.page_id,"document":reply.document,"currentUrl":format!("{}{}",scope.origin,route.path),"artifacts":accepted});
                let mut public_observation=reply.observation.clone();for (raw,public) in public_observation.artifacts.iter_mut().zip(&accepted){raw.id=public.id.clone();}
                Ok(anima_core::Content{text:format!("Browser provenance: {}\nUntrusted page evidence: {}",provenance,serde_json::to_string(&public_observation).map_err(|_|browser_error(BrowserErrorCode::Protocol))?),..Default::default()})
            }.await;
            post.map_err(|mut error:BrowserError|{error.effect=BrowserEffect::MayHaveOccurred;error})
        }.await;
        if result.is_err() {
            self.retire_run(&stamp.run_id).await;
        }
        result
    }
}
