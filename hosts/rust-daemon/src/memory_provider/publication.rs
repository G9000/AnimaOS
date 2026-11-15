//! Explicit selected memory sharing with durable, profile-owned admission and delivery.
use super::{
    binding,
    outbox::{
        Admission, OutboxError, OutboxLimits, Publication, PublicationState, PublicationStore,
        Transition,
    },
    reconcile,
};
use crate::{app::SharedDaemonState, profile_owner::ProfileOwner, state::DaemonState};
use anima_core::CancelSignal;
use anima_memory::{Memory, MemoryScope, MemoryType};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    sync::{Arc, Weak},
    time::Duration,
};
use tokio::sync::{Mutex, Notify, RwLock};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Selection {
    pub(crate) publication_id: String,
    pub(crate) local_id: String,
    pub(crate) expected_content: String,
    pub(crate) title: String,
    pub(crate) source: String,
    pub(crate) basis: Basis,
    #[serde(default)]
    pub(crate) delivery: Delivery,
    pub(crate) target: Option<Target>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Target {
    pub(crate) id: String,
    #[serde(deserialize_with = "safe_revision")]
    pub(crate) expected_revision: u64,
}
fn safe_revision<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    let n = f64::deserialize(d)?;
    if n.is_finite() && n >= 1.0 && n <= 9_007_199_254_740_991.0 && n.fract() == 0.0 {
        Ok(n as u64)
    } else {
        Err(serde::de::Error::custom(
            "expectedRevision must be a positive safe integer",
        ))
    }
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Basis {
    OwnerStatement,
    Source,
    ModelInference,
}
#[derive(Clone, Copy, Default, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Delivery {
    #[default]
    Proposal,
    Direct,
}
#[derive(Debug)]
pub(crate) enum PublicationError {
    Invalid(String),
    Conflict(String),
    Unavailable(String),
    NotFound,
}
impl std::fmt::Display for PublicationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(s) | Self::Conflict(s) | Self::Unavailable(s) => f.write_str(s),
            Self::NotFound => f.write_str("Publication not found"),
        }
    }
}
impl std::error::Error for PublicationError {}
impl From<OutboxError> for PublicationError {
    fn from(e: OutboxError) -> Self {
        match e {
            OutboxError::Invalid(_) => Self::Invalid(e.to_string()),
            OutboxError::IdempotencyConflict
            | OutboxError::StaleRevision
            | OutboxError::InvalidTransition
            | OutboxError::TargetBusy => Self::Conflict(e.to_string()),
            OutboxError::NotFound => Self::NotFound,
            _ => Self::Unavailable(e.to_string()),
        }
    }
}
fn unavailable() -> PublicationError {
    PublicationError::Unavailable("Publication storage or profile ownership unavailable".into())
}
pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}
pub(crate) fn digest(value: &str) -> String {
    hex::encode(Sha256::digest(value.as_bytes()))
}

pub(crate) struct PublicationService {
    state: Weak<RwLock<DaemonState>>,
    owner: ProfileOwner,
    store: Arc<PublicationStore>,
    cancel: CancelSignal,
    wake: Arc<Notify>,
    worker: Mutex<Option<tokio::task::JoinHandle<()>>>,
    admission: std::sync::Mutex<Option<Arc<dyn Fn() -> bool + Send + Sync>>>,
    admissions: Mutex<()>,
    storage_failed: std::sync::atomic::AtomicBool,
}
impl PublicationService {
    /// Called once after the profile lease is acquired, before any publication handlers.
    pub(crate) async fn open(
        state: &SharedDaemonState,
        owner: ProfileOwner,
        path: &Path,
    ) -> Result<Arc<Self>, PublicationError> {
        owner.verify_owned().await.map_err(|_| unavailable())?;
        let path = owner.owned_sqlite_path(path).map_err(|_| unavailable())?;
        let retained = owner.clone();
        let store = tokio::task::spawn_blocking(move || {
            retained.ensure_owned().map_err(|_| unavailable())?;
            let store = PublicationStore::open(&path, OutboxLimits::default())?;
            store.recover_interrupted(now_ms())?;
            Ok::<_, PublicationError>(store)
        })
        .await
        .map_err(|_| unavailable())??;
        Ok(Arc::new(Self {
            state: Arc::downgrade(state),
            owner,
            store: Arc::new(store),
            cancel: CancelSignal::new(),
            wake: Arc::new(Notify::new()),
            worker: Mutex::new(None),
            admission: std::sync::Mutex::new(None),
            admissions: Mutex::new(()),
            storage_failed: std::sync::atomic::AtomicBool::new(false),
        }))
    }
    pub(crate) async fn db<T: Send + 'static>(
        &self,
        f: impl FnOnce(&PublicationStore) -> Result<T, OutboxError> + Send + 'static,
    ) -> Result<T, PublicationError> {
        let owner = self.owner.clone();
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || {
            owner.ensure_owned().map_err(|_| unavailable())?;
            f(&store).map_err(Into::into)
        })
        .await
        .map_err(|_| unavailable())?
    }
    pub(crate) fn accepting(&self) -> bool {
        !self
            .storage_failed
            .load(std::sync::atomic::Ordering::SeqCst)
            && !self.cancel.is_cancelled()
            && self
                .admission
                .lock()
                .ok()
                .and_then(|v| v.clone())
                .is_none_or(|probe| probe())
    }
    async fn authorize(&self, agent: &str) -> Result<SharedDaemonState, PublicationError> {
        self.owner.verify_owned().await.map_err(|_| unavailable())?;
        let state = self.state.upgrade().ok_or_else(unavailable)?;
        {
            let guard = state.read().await;
            let runtime = guard.agents.get(agent).ok_or(PublicationError::NotFound)?;
            if crate::agent_runs::is_helper_config(runtime.config()) {
                return Err(PublicationError::Invalid(
                    "Helpers cannot share memories".into(),
                ));
            }
        }
        Ok(state)
    }
    pub(crate) async fn get(
        &self,
        agent: &str,
        id: &str,
    ) -> Result<Option<Publication>, PublicationError> {
        self.authorize(agent).await?;
        let id = id.to_owned();
        let agent = agent.to_owned();
        self.db(move |store| Ok(store.get(&id)?.filter(|p| p.admission.agent_id == agent)))
            .await
    }
    pub(crate) async fn list(&self, agent: &str) -> Result<Value, PublicationError> {
        self.authorize(agent).await?;
        let agent = agent.to_owned();
        let available = !self
            .storage_failed
            .load(std::sync::atomic::Ordering::SeqCst);
        self.db(move |store|{let rows=store.list(&agent,256)?;let counts=store.counts(&agent)?;let total:u64=counts.values().sum();Ok(json!({"available":available,"reason":if available{Value::Null}else{json!("publication_storage_unavailable")},"items":rows.iter().map(summary).collect::<Vec<_>>(),"counts":counts,"omitted":total.saturating_sub(rows.len() as u64)}))}).await
    }
    pub(crate) async fn admit(
        &self,
        agent: &str,
        selection: Selection,
    ) -> Result<Publication, PublicationError> {
        let state = self.authorize(agent).await?;
        let _admission = self.admissions.lock().await;
        validate_selection(&selection)?;
        // Retries compare only the original caller payload, before any mutable source or grant.
        if let Some(existing) = self.get(agent, &selection.publication_id).await? {
            if same_selection(&existing, &selection) {
                return Ok(existing);
            }
            return Err(PublicationError::Conflict(
                "Publication ID was reused with different selection".into(),
            ));
        }
        if !self.accepting() {
            return Err(unavailable());
        }
        let (binding, version, descriptor, memory) = {
            let guard = state.read().await;
            let runtime = guard.agents.get(agent).ok_or(PublicationError::NotFound)?;
            let binding = binding::binding(runtime.config())
                .map_err(|e| PublicationError::Invalid(e.into()))?
                .ok_or_else(|| {
                    PublicationError::Invalid("No additive memory binding configured".into())
                })?;
            binding
                .validate(agent)
                .map_err(|e| PublicationError::Invalid(e.into()))?;
            let row = guard
                .mcp_registry
                .connections
                .get(&binding.connection_id)
                .filter(|r| r.enabled && r.agent_id == agent)
                .ok_or_else(|| {
                    PublicationError::Invalid("Selected connection unavailable".into())
                })?;
            if guard
                .paired_devices
                .records
                .iter()
                .any(|d| d.mcp_connection_id == binding.connection_id)
            {
                return Err(PublicationError::Invalid(
                    "Paired-device publication is unsupported".into(),
                ));
            }
            let descriptor = row
                .reviewed_tools
                .iter()
                .find(|t| t.descriptor.remote_name == remote_tool(selection.delivery))
                .ok_or_else(|| {
                    PublicationError::Invalid("Publication tool must be reviewed".into())
                })?
                .descriptor
                .descriptor_digest
                .clone();
            (binding, row.version, descriptor, guard.memory_handle())
        };
        let source = memory
            .read()
            .await
            .get(&selection.local_id)
            .ok_or_else(|| {
                PublicationError::Conflict("Selected local memory no longer exists".into())
            })?;
        validate_source(&source, agent, &selection.expected_content)?;
        let source_digest = digest(&selection.expected_content);
        let local_id = selection.local_id.clone();
        let own = agent.to_owned();
        let ws = binding.workspace_id.clone();
        let generation = binding.generation.clone();
        let selected_digest = source_digest.clone();
        let target = selection.target.as_ref().map(|t| t.id.clone());
        if self
            .db(move |store| {
                store.has_source_mapping(
                    &ws,
                    &generation,
                    &own,
                    &local_id,
                    &selected_digest,
                    target.as_deref(),
                )
            })
            .await?
        {
            return Err(PublicationError::Conflict("This selected source is already shared; select a changed source and explicit correction target".into()));
        }
        let candidate = record(&selection);
        let mut request = json!({"workspaceId":binding.workspace_id,"generation":binding.generation,"operationId":selection.publication_id,"expectedRevision":selection.target.as_ref().map_or(0,|t|t.expected_revision),"record":candidate});
        if let Some(target) = &selection.target {
            request["id"] = json!(target.id)
        }
        if selection.delivery == Delivery::Proposal {
            request["source"] = json!(selection.source.trim());
            request["basis"] = json!(selection.basis)
        }
        if request.to_string().len() > 6 * 1024 {
            return Err(PublicationError::Invalid(
                "Selected publication exceeds the remote request budget".into(),
            ));
        }
        let mut input = Admission {
            publication_id: selection.publication_id.clone(),
            operation_id: selection.publication_id.clone(),
            agent_id: agent.into(),
            binding,
            connection_version: version,
            descriptor_digest: descriptor,
            local_id: selection.local_id.clone(),
            source_attribution: Some(selection.source.clone()),
            caller_digest: Some(digest(
                &serde_json::to_string(&selection).map_err(|_| unavailable())?,
            )),
            source_digest: Some(source_digest),
            source_revision: Some(digest(
                &serde_json::to_string(&source).map_err(|_| unavailable())?,
            )),
            candidate,
            remote_tool: remote_tool(selection.delivery).into(),
            request,
            target_key: selection.target.as_ref().map_or_else(
                || format!("source:{}", selection.local_id),
                |t| format!("record:{}", t.id),
            ),
        };
        // Recheck the exact source immediately before durable admission. No local save is implied.
        let current = memory.read().await;
        let source = current.get(&selection.local_id).ok_or_else(|| {
            PublicationError::Conflict("Selected local memory no longer exists".into())
        })?;
        validate_source(&source, agent, &selection.expected_content)?;
        input.source_revision = Some(digest(
            &serde_json::to_string(&source).map_err(|_| unavailable())?,
        ));
        let result = self.db(move |s| s.admit(input, now_ms())).await?;
        drop(current);
        self.wake.notify_one();
        Ok(result)
    }
    pub(crate) async fn control(
        &self,
        agent: &str,
        id: &str,
        expected: u64,
        action: &str,
    ) -> Result<Publication, PublicationError> {
        let item = self
            .get(agent, id)
            .await?
            .ok_or(PublicationError::NotFound)?;
        if item.revision != expected {
            return Err(PublicationError::Conflict(
                "Publication revision changed".into(),
            ));
        }
        let transition = match action {
            "cancel" => Transition::Cancelled,
            "resume" => {
                if !self.accepting() {
                    return Err(unavailable());
                }
                let state = self.authorize(agent).await?;
                let manager = state
                    .read()
                    .await
                    .mcp_manager
                    .clone()
                    .ok_or_else(unavailable)?;
                manager
                    .publication_preflight(&item, "mother_get_context", &json!({"limit":1}))
                    .await
                    .map_err(|e| PublicationError::Conflict(e.into()))?;
                Transition::Resume
            }
            _ => {
                return Err(PublicationError::Invalid(
                    "Unknown publication action".into(),
                ))
            }
        };
        let id = id.to_owned();
        let result = self
            .db(move |s| s.transition(&id, expected, transition, now_ms()))
            .await?;
        self.wake.notify_one();
        Ok(result)
    }
    pub(crate) async fn start(self: &Arc<Self>, probe: Arc<dyn Fn() -> bool + Send + Sync>) {
        if let Ok(mut p) = self.admission.lock() {
            *p = Some(probe)
        }
        let mut worker = self.worker.lock().await;
        if worker.is_some() {
            return;
        }
        let weak = Arc::downgrade(self);
        let cancel = self.cancel.clone();
        let wake = self.wake.clone();
        *worker = Some(tokio::spawn(async move {
            while let Some(service) = weak.upgrade() {
                if service.cancel.is_cancelled() {
                    break;
                }
                if service.accepting() {
                    if let Ok(rows) = service.db(|s| s.due(now_ms(), 16)).await {
                        for item in rows {
                            if !service.accepting() {
                                break;
                            }
                            service.dispatch(item).await;
                        }
                    }
                }
                drop(service);
                tokio::select! {_=cancel.cancelled()=>break,_=wake.notified()=>{},_=tokio::time::sleep(Duration::from_secs(2))=>{}}
            }
        }));
    }
    pub(crate) fn request_shutdown(&self) {
        self.cancel.cancel();
        self.wake.notify_waiters();
    }
    pub(crate) async fn shutdown(&self) {
        self.request_shutdown();
        if let Some(worker) = self.worker.lock().await.take() {
            let _ = worker.await;
        }
    }
}
fn remote_tool(delivery: Delivery) -> &'static str {
    match delivery {
        Delivery::Proposal => "mother_propose_memory",
        Delivery::Direct => "mother_write_record",
    }
}
fn record(s: &Selection) -> Value {
    json!({"kind":"memory","status":"active","title":s.title.trim(),"content":s.expected_content.trim(),"basis":s.basis,"references":[],"sources":[]})
}
fn same_selection(p: &Publication, s: &Selection) -> bool {
    if let Some(expected) = &p.admission.caller_digest {
        return serde_json::to_string(s).is_ok_and(|value| digest(&value) == *expected);
    }
    p.admission.source_digest.as_ref() == Some(&digest(&s.expected_content))
        && p.admission.local_id == s.local_id
        && p.admission.candidate == record(s)
        && p.admission.remote_tool == remote_tool(s.delivery)
        && p.admission.request.get("id").and_then(Value::as_str)
            == s.target.as_ref().map(|t| t.id.as_str())
        && p.admission.request["expectedRevision"].as_u64()
            == Some(s.target.as_ref().map_or(0, |t| t.expected_revision))
}
fn validate_selection(s: &Selection) -> Result<(), PublicationError> {
    let valid_uuid = |v: &str| uuid::Uuid::parse_str(v).is_ok_and(|u| u.to_string() == v);
    if !valid_uuid(&s.publication_id)
        || s.target
            .as_ref()
            .is_some_and(|t| !valid_uuid(&t.id) || t.expected_revision == 0)
    {
        return Err(PublicationError::Invalid(
            "Canonical publication and target UUIDs required".into(),
        ));
    }
    for (text, max) in [
        (&s.local_id, 512),
        (&s.title, 240),
        (&s.source, 200),
        (&s.expected_content, 4000),
    ] {
        if text.trim().is_empty()
            || text.encode_utf16().count() > max
            || crate::memory_text::has_hidden_text(text)
        {
            return Err(PublicationError::Invalid(
                "Selection text is empty, hidden, or exceeds its bounds".into(),
            ));
        }
    }
    if s.source.chars().any(char::is_control) {
        return Err(PublicationError::Invalid(
            "Source must be a short attribution".into(),
        ));
    }
    Ok(())
}
fn validate_source(source: &Memory, agent: &str, expected: &str) -> Result<(), PublicationError> {
    if source.agent_id != agent
        || source.memory_type != MemoryType::Fact
        || source.scope != MemoryScope::Shared
        || crate::core_memory::is_core_entry(source)
        || crate::memory_text::has_hidden_text(&source.id)
        || crate::memory_text::has_hidden_text(&source.content)
        || source.tags.as_ref().is_some_and(|t| {
            t.iter().any(|v| {
                crate::memory_text::has_hidden_text(v)
                    || v.starts_with("provider:")
                    || v.starts_with("mother:")
                    || v.starts_with("workspace:")
            })
        })
    {
        return Err(PublicationError::Invalid(
            "Only own visible shared native facts can be selected".into(),
        ));
    }
    if source.content != expected {
        return Err(PublicationError::Conflict(
            "Selected local memory changed; select its current content".into(),
        ));
    }
    Ok(())
}
pub(crate) fn summary(p: &Publication) -> Value {
    let canonical = p
        .canonical_id
        .as_ref()
        .zip(p.canonical_revision)
        .map(|(id, revision)| {
            let record = p.receipt.as_ref().map(|r| {
                if r["receipt"]["receipt"]["record"].is_object() {
                    &r["receipt"]["receipt"]["record"]
                } else {
                    &r["record"]
                }
            });
            let edited = record.is_some_and(|r| {
                ["title", "content", "kind", "status", "basis", "sources"]
                    .iter()
                    .any(|k| r[*k] != p.admission.request["record"][*k])
            });
            json!({"id":id,"revision":revision,"ownerEdited":edited})
        });
    json!({"id":p.admission.publication_id,"operationId":p.admission.operation_id,"revision":p.revision,"state":p.state,"title":p.admission.candidate["title"],"localId":p.admission.local_id,"attempts":p.attempts,"createdAtMs":p.created_at_ms,"updatedAtMs":p.updated_at_ms,"nextAttemptAtMs":p.next_attempt_at_ms,"reason":p.last_error,"proposalId":p.proposal_id,"canonical":canonical})
}

#[path = "publication_dispatch.rs"]
mod dispatch;

#[cfg(test)]
#[path = "publication_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "publication_interop_tests.rs"]
mod interop_tests;

#[cfg(test)]
#[path = "publication_crash_tests.rs"]
mod crash_tests;
