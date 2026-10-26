//! Opt-in durable dialogue integration; default daemon profiles remain legacy.
mod clock_capability;
mod memory_sources;
mod model;
pub(crate) mod startup;
pub(crate) use memory_sources::MemorySourceRequest;
mod outbox;
use anima_core::{execution_limits::ExecutionLimits, *};
use anima_execution_store_sqlite::SqliteExecutionStore;
use anima_harness::durable::*;
use anima_memory::{
    CandidateEvidence, EvidenceKind, IdentityRegistry, MemoryAuthority, MemoryCandidate,
    MemoryEffect, MemoryProjectionPolicy, MemoryProjectionPort,
};
use anima_memory_store_sqlite::SqliteMemoryStore;
use async_trait::async_trait;
pub use clock_capability::clock_manifest;
use outbox::Outbox;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex},
};
use tokio::sync::{broadcast, Mutex as AsyncMutex};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BridgeError {
    Invalid,
    Conflict,
    NotFound,
    Busy,
    Unavailable,
    MemoryUnavailable,
    Forbidden,
}
impl std::fmt::Display for BridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Invalid => "unsupported durable dialogue request or profile",
            Self::Conflict => "durable request conflicts with its accepted payload",
            Self::NotFound => "durable profile or run not found",
            Self::Busy => "durable dialogue profile is already running",
            Self::Unavailable => "durable execution authority is unavailable",
            Self::MemoryUnavailable => {
                "required persistent memory or accepted-event projection is unavailable"
            }
            Self::Forbidden => "memory source is outside this observer's authority",
        })
    }
}
impl std::error::Error for BridgeError {}
impl BridgeError {
    pub(crate) fn status(self) -> axum::http::StatusCode {
        use axum::http::StatusCode;
        match self {
            Self::Invalid => StatusCode::BAD_REQUEST,
            Self::Conflict => StatusCode::CONFLICT,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::Forbidden => StatusCode::FORBIDDEN,
            _ => StatusCode::SERVICE_UNAVAILABLE,
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DialogueRequest {
    pub request_id: Uuid,
    pub room_id: Uuid,
    pub text: String,
}
impl DialogueRequest {
    fn validate(&self) -> Result<(), BridgeError> {
        if self.request_id.is_nil()
            || self.room_id.is_nil()
            || self.text.trim().is_empty()
            || self.text.len() > 32 * 1024
        {
            return Err(BridgeError::Invalid);
        }
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct RegisteredProfile {
    agent_id: String,
    identity: IdentityBinding,
    config: AgentConfig,
    definition: AgentDefinition,
    scope: ApplicationScope,
    limits: ExecutionLimits,
    clock_enabled: bool,
}
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct PreparedRun {
    version: u32,
    execution_format: String,
    memory_format: String,
    profile: RegisteredProfile,
    request: DialogueRequest,
    binding: DurableRunBinding,
    budget: anima_core::execution_limits::ExecutionScope,
    effect: MemoryEffect,
}
struct ProfileState {
    profile: RegisteredProfile,
    gate: Arc<AsyncMutex<()>>,
}

/// Explicitly injected opt-in profile service. No environment, credentials or legacy saves are read.
pub struct DaemonEngineBridge {
    owner: Uuid,
    execution: Arc<SqliteExecutionStore>,
    memory: SqliteMemoryStore,
    outbox: Outbox,
    model: Arc<dyn ModelAdapter>,
    clock: Arc<dyn ExecutionClock>,
    profiles: Mutex<HashMap<String, ProfileState>>,
    controls: AsyncMutex<HashMap<Uuid, DurableControl>>,
    live: broadcast::Sender<serde_json::Value>,
    workers: hosted::HostWorkers,
    startup_owns_stores: bool,
}
impl DaemonEngineBridge {
    pub async fn open(
        owner: Uuid,
        execution: Arc<SqliteExecutionStore>,
        memory: SqliteMemoryStore,
        outbox_path: impl AsRef<Path>,
        protection: PersistenceProtection,
        model: Arc<dyn ModelAdapter>,
        clock: Arc<dyn ExecutionClock>,
    ) -> Result<Self, BridgeError> {
        if owner.is_nil() {
            return Err(BridgeError::Invalid);
        }
        let memory_id = memory
            .store_id()
            .await
            .map_err(|_| BridgeError::MemoryUnavailable)?;
        let outbox = Outbox::open(
            outbox_path,
            owner,
            execution.store_id(),
            memory_id,
            protection,
        )
        .await?;
        let (live, _) = broadcast::channel(64);
        let mut profiles = HashMap::new();
        for profile in outbox.profiles().await? {
            memory
                .register_identity(profile.identity.clone())
                .await
                .map_err(|_| BridgeError::MemoryUnavailable)?;
            profiles.insert(
                profile.agent_id.clone(),
                ProfileState {
                    profile,
                    gate: Arc::new(AsyncMutex::new(())),
                },
            );
        }
        Ok(Self {
            owner,
            execution,
            memory,
            outbox,
            model,
            clock,
            profiles: Mutex::new(profiles),
            controls: AsyncMutex::new(HashMap::new()),
            live,
            workers: hosted::HostWorkers::default(),
            startup_owns_stores: false,
        })
    }
    /// Only the conversation-only profile is certified here. Coding/helpers/MCP/fallbacks refuse.
    pub async fn register_dialogue(
        &self,
        agent_id: &str,
        config: AgentConfig,
        version: u32,
    ) -> Result<(), BridgeError> {
        self.register_profile(agent_id, config, version, false)
            .await
    }
    pub async fn register_clock(
        &self,
        agent_id: &str,
        config: AgentConfig,
        version: u32,
    ) -> Result<(), BridgeError> {
        self.register_profile(agent_id, config, version, true).await
    }
    async fn register_profile(
        &self,
        agent_id: &str,
        config: AgentConfig,
        version: u32,
        clock_enabled: bool,
    ) -> Result<(), BridgeError> {
        {
            let profiles = self.profiles.lock().map_err(|_| BridgeError::Unavailable)?;
            if profiles.len() >= 256 && !profiles.contains_key(agent_id) {
                return Err(BridgeError::Invalid);
            }
        }
        if agent_id.is_empty()
            || agent_id.len() > 128
            || agent_id.trim() != agent_id
            || version == 0
            || config
                .provider
                .as_deref()
                .is_none_or(|provider| provider.trim().is_empty())
            || config.model.trim().is_empty()
            || (clock_enabled
                && config
                    .settings
                    .as_ref()
                    .and_then(|settings| settings.max_tool_iterations)
                    == Some(0))
            || config.tools.as_ref().is_some_and(|v| !v.is_empty())
            || config.plugins.as_ref().is_some_and(|v| !v.is_empty())
            || !config.fallbacks.is_empty()
            || crate::agent_runs::is_helper_config(&config)
        {
            return Err(BridgeError::Invalid);
        }
        let identity = IdentityBinding::new(self.owner.to_string(), agent_id)
            .map_err(|_| BridgeError::Invalid)?;
        let limits = config
            .settings
            .as_ref()
            .and_then(|s| s.execution_limits.clone())
            .unwrap_or_default();
        limits.validate().map_err(|_| BridgeError::Invalid)?;
        let mut definition = definition(agent_id, &config, version, &limits)?;
        if clock_enabled {
            let manifest = clock_manifest().map_err(|_| BridgeError::Invalid)?;
            definition.resolved_capabilities.push(ResolvedCapability {
                capability_id: manifest.id.clone(),
                manifest_version: manifest.version,
                schema_digest: manifest.schema_digest().into(),
                override_config: None,
                approval_policy_revision: definition.approval_policy_revision,
            });
            definition.validate().map_err(|_| BridgeError::Invalid)?;
        }
        let proposed = RegisteredProfile {
            agent_id: agent_id.into(),
            identity: identity.clone(),
            config,
            definition,
            scope: ApplicationScope::new(self.owner, Uuid::new_v4())
                .map_err(|_| BridgeError::Invalid)?,
            limits,
            clock_enabled,
        };
        let profile = self.outbox.register(proposed).await?;
        if profile.clock_enabled
            && self
                .execution
                .load_authoritative_policy(
                    self.owner,
                    &profile.definition.id,
                    profile.definition.version,
                )
                .await
                .map_err(|_| BridgeError::Unavailable)?
                .is_none()
        {
            let policy = AuthoritativePolicyState::active(
                self.owner,
                &profile.definition.id,
                profile.definition.version,
                profile.definition.approval_policy_revision,
                None,
            )
            .map_err(|_| BridgeError::Invalid)?;
            self.execution
                .apply_authoritative_policy(self.owner, AuthoritativePolicyChange::create(policy))
                .await
                .map_err(|_| BridgeError::Unavailable)?;
        }
        self.memory
            .register_identity(identity)
            .await
            .map_err(|_| BridgeError::MemoryUnavailable)?;
        let mut profiles = self.profiles.lock().map_err(|_| BridgeError::Unavailable)?;
        profiles
            .entry(agent_id.into())
            .and_modify(|registered| registered.profile = profile.clone())
            .or_insert_with(|| ProfileState {
                profile,
                gate: Arc::new(AsyncMutex::new(())),
            });
        Ok(())
    }
    pub(crate) fn handles(&self, agent: &str) -> bool {
        self.profiles.lock().is_ok_and(|p| p.contains_key(agent))
    }
    pub(crate) async fn submit(
        &self,
        agent: &str,
        request: DialogueRequest,
    ) -> Result<serde_json::Value, BridgeError> {
        let _work = self.track()?;
        self.submit_reserved(agent, request).await
    }
    pub(crate) async fn submit_reserved(
        &self,
        agent: &str,
        request: DialogueRequest,
    ) -> Result<serde_json::Value, BridgeError> {
        request.validate()?;
        let (profile, gate) = {
            let profiles = self.profiles.lock().map_err(|_| BridgeError::Unavailable)?;
            let registered = profiles.get(agent).ok_or(BridgeError::NotFound)?;
            (registered.profile.clone(), registered.gate.clone())
        };
        let _guard = gate.try_lock_owned().map_err(|_| BridgeError::Busy)?;
        let binding = DurableRunBinding::new(
            self.owner,
            profile.identity.clone(),
            request.room_id,
            request.request_id,
            profile.scope.clone(),
        )
        .map_err(|_| BridgeError::Invalid)?;
        let budget = anima_core::execution_limits::ExecutionScope::new(
            format!("daemon-dialogue:{}", binding.run),
            binding.run.to_string(),
            profile.limits.clone(),
            self.clock.now_ms(),
        )
        .map_err(|_| BridgeError::Invalid)?;
        let effect = user_effect(
            &binding.individual,
            binding.run,
            &request.text,
            self.clock.now_ms(),
        )?;
        let prepared = self
            .outbox
            .prepare(PreparedRun {
                version: 1,
                execution_format: "durable-v1".into(),
                memory_format: "canonical-memory-v1".into(),
                profile,
                request,
                binding,
                budget,
                effect,
            })
            .await?;
        if prepared.version != 1
            || prepared.execution_format != "durable-v1"
            || prepared.memory_format != "canonical-memory-v1"
        {
            return Err(BridgeError::Unavailable);
        }
        self.project(&prepared.binding.individual).await?;
        if let Some(response) = self.outbox.response(prepared.binding.run).await? {
            return Ok(response);
        }
        let authority = authority(&prepared.binding.individual)?;
        let bound = Arc::new(
            self.memory
                .bind(authority.clone())
                .await
                .map_err(|_| BridgeError::MemoryUnavailable)?,
        );
        let memory = PersistentAgentMemory::new(authority, bound, Arc::new(self.outbox.clone()));
        let port = self
            .execution
            .register_budget(self.owner, prepared.budget.clone())
            .await
            .map_err(|_| BridgeError::Unavailable)?;
        let control = DurableControl::new();
        {
            let mut controls = self.controls.lock().await;
            match self
                .outbox
                .action(agent, prepared.binding.run, None)
                .await?
                .as_str()
            {
                "stop" => control.stop(),
                "pause" => control.pause(),
                "resume" => control.resume(),
                _ => {}
            }
            controls.insert(prepared.binding.run, control.clone());
        }
        let execution = control
            .execution_context(
                &prepared.budget,
                prepared.binding.run,
                Arc::new(port),
                Arc::new(model::Clock(self.clock.clone())),
            )
            .map_err(|_| BridgeError::Invalid)?;
        let harness = DurableHarness::new(
            self.execution.clone(),
            Arc::new(model::ConfiguredModel {
                inner: self.model.clone(),
                config: prepared.profile.config.clone(),
            }),
            Arc::new(Definitions(prepared.profile.definition.clone())),
            Arc::new(clock_capability::Policy {
                enabled: prepared.profile.clock_enabled,
                binding: prepared.binding.clone(),
                clock: self.clock.clone(),
            }),
            Arc::new(clock_capability::Runtime {
                enabled: prepared.profile.clock_enabled,
                binding: prepared.binding.clone(),
                outbox: self.outbox.clone(),
                clock: self.clock.clone(),
            }),
            Arc::new(model::Clock(self.clock.clone())),
            Arc::new(Live(self.live.clone())),
        );
        let result = async {
            harness
                .admit_persistent(
                    &prepared.binding,
                    &prepared.profile.definition,
                    &prepared.request.text,
                    &execution,
                    &memory,
                )
                .await
                .map_err(engine_error)?;
            harness
                .run_persistent(&prepared.binding, &control, &execution, &memory)
                .await
                .map_err(engine_error)
        }
        .await;
        self.controls.lock().await.remove(&prepared.binding.run);
        let outcome = result?;
        let (status, text, terminal) = match outcome {
            EngineRunOutcome::Completed { content } => ("success", Some(content.text), true),
            EngineRunOutcome::Cancelled => ("cancelled", None, true),
            EngineRunOutcome::PausedByRequest => ("paused", None, false),
            EngineRunOutcome::PausedForBudget => ("budget_exhausted", None, false),
            EngineRunOutcome::Denied => ("denied", None, true),
            _ => ("recovery_required", None, false),
        };
        let response = serde_json::json!({"engineFormat":prepared.execution_format,"memoryFormat":prepared.memory_format,"runId":prepared.binding.run,"sessionId":prepared.binding.session,"individualId":prepared.binding.individual.individual_id().as_str(),"result":{"status":status,"data":text.map(|text|serde_json::json!({"text":text,"attachments":null,"metadata":null})),"error":null,"durationMs":0}});
        if terminal {
            self.outbox.finish(prepared.binding.run, response).await
        } else {
            Ok(response)
        }
    }
    async fn project(&self, identity: &IdentityBinding) -> Result<(), BridgeError> {
        let authority = authority(identity)?;
        let bound = self
            .memory
            .bind(authority.clone())
            .await
            .map_err(|_| BridgeError::MemoryUnavailable)?;
        for pending in self
            .outbox
            .pending(identity)
            .await
            .map_err(|_| BridgeError::MemoryUnavailable)?
        {
            let policy = UserStatementPolicy {
                accepted: pending.request.text.clone(),
                source_event: pending.effect.source_event_id().into(),
            };
            let effect = pending
                .effect
                .validate(&authority, &policy)
                .map_err(|_| BridgeError::MemoryUnavailable)?;
            let outcome = bound
                .apply_effect(effect)
                .await
                .map_err(|_| BridgeError::MemoryUnavailable)?;
            self.outbox
                .ack(pending.binding.run, outcome.receipt)
                .await
                .map_err(|_| BridgeError::MemoryUnavailable)?;
        }
        self.project_memory_sources(identity, &bound, &authority)
            .await?;
        // One request does bounded catch-up work. Remaining accepted effects
        // keep admission closed, but a retry can continue with the next batch.
        self.outbox
            .ensure_projected(identity)
            .await
            .map_err(|_| BridgeError::MemoryUnavailable)?;
        Ok(())
    }
    pub(crate) async fn recall(
        &self,
        agent: &str,
        query: &str,
    ) -> Result<Vec<MemoryRecord>, BridgeError> {
        let _work = self.track()?;
        let identity = {
            let profiles = self.profiles.lock().map_err(|_| BridgeError::Unavailable)?;
            profiles
                .get(agent)
                .ok_or(BridgeError::NotFound)?
                .profile
                .identity
                .clone()
        };
        self.project(&identity).await?;
        let authority = authority(&identity)?;
        let bound = self
            .memory
            .bind(authority.clone())
            .await
            .map_err(|_| BridgeError::MemoryUnavailable)?;
        let query = MemoryQuery::new(query, authority.read_scopes().to_vec(), 32)
            .map_err(|_| BridgeError::Invalid)?;
        Ok(bound
            .query(
                &authority
                    .access_context()
                    .map_err(|_| BridgeError::Invalid)?,
                query,
            )
            .await
            .map_err(|_| BridgeError::MemoryUnavailable)?
            .hits()
            .iter()
            .map(|hit| hit.record().clone())
            .collect())
    }
    pub(crate) async fn accept_preference(
        &self,
        agent: &str,
        request: DialogueRequest,
    ) -> Result<serde_json::Value, BridgeError> {
        let _work = self.track()?;
        self.accept_preference_reserved(agent, request).await
    }
    pub(crate) async fn accept_preference_reserved(
        &self,
        agent: &str,
        request: DialogueRequest,
    ) -> Result<serde_json::Value, BridgeError> {
        request.validate()?;
        let (profile, gate) = {
            let profiles = self.profiles.lock().map_err(|_| BridgeError::Unavailable)?;
            let registered = profiles.get(agent).ok_or(BridgeError::NotFound)?;
            (registered.profile.clone(), registered.gate.clone())
        };
        let _guard = gate.try_lock_owned().map_err(|_| BridgeError::Busy)?;
        let binding = DurableRunBinding::new(
            self.owner,
            profile.identity.clone(),
            request.room_id,
            request.request_id,
            profile.scope.clone(),
        )
        .map_err(|_| BridgeError::Invalid)?;
        let budget = anima_core::execution_limits::ExecutionScope::new(
            format!("daemon-dialogue:{}", binding.run),
            binding.run.to_string(),
            profile.limits.clone(),
            self.clock.now_ms(),
        )
        .map_err(|_| BridgeError::Invalid)?;
        let effect = user_effect(
            &binding.individual,
            binding.run,
            &request.text,
            self.clock.now_ms(),
        )?;
        let prepared = self
            .outbox
            .prepare(PreparedRun {
                version: 1,
                execution_format: "durable-v1".into(),
                memory_format: "canonical-memory-v1".into(),
                profile,
                request,
                binding,
                budget,
                effect,
            })
            .await?;
        self.project(&prepared.binding.individual).await?;
        Ok(
            serde_json::json!({"memoryId":prepared.effect.candidates()[0].write().id(),"sourceEventId":prepared.effect.source_event_id()}),
        )
    }
    pub(crate) async fn control(
        &self,
        agent: &str,
        run: Uuid,
        action: &str,
    ) -> Result<(), BridgeError> {
        let _work = self.track()?;
        if !matches!(action, "stop" | "pause" | "resume") {
            return Err(BridgeError::Invalid);
        }
        let controls = self.controls.lock().await;
        self.outbox.action(agent, run, Some(action)).await?;
        if let Some(control) = controls.get(&run) {
            match action {
                "stop" => control.stop(),
                "pause" => control.pause(),
                _ => control.resume(),
            }
        }
        Ok(())
    }
    pub(crate) async fn events(
        &self,
        agent: &str,
        run: Uuid,
    ) -> Result<Vec<RuntimeEvent>, BridgeError> {
        let _work = self.track()?;
        self.outbox.run(agent, run).await?;
        let mut request = StoreReadPage::first(256).map_err(|_| BridgeError::Invalid)?;
        let mut events = vec![];
        for _ in 0..16 {
            let page = self
                .execution
                .replay_events(self.owner, run, request)
                .await
                .map_err(|_| BridgeError::Unavailable)?;
            events.extend_from_slice(page.events());
            if let Some(next) = page.next_cursor() {
                request =
                    StoreReadPage::after(next.clone(), 256).map_err(|_| BridgeError::Invalid)?;
            } else {
                return Ok(events);
            }
        }
        Err(BridgeError::Unavailable)
    }
    pub(crate) fn subscribe(&self) -> broadcast::Receiver<serde_json::Value> {
        self.live.subscribe()
    }
    pub(crate) async fn replay_request(
        &self,
        agent: &str,
        request: &DialogueRequest,
    ) -> Result<Option<serde_json::Value>, BridgeError> {
        let _work = self.track()?;
        request.validate()?;
        self.outbox.exact_response(agent, request).await
    }
    pub(crate) fn track(&self) -> Result<hosted::HostWork, BridgeError> {
        self.workers.enter().ok_or(BridgeError::Unavailable)
    }
    pub(crate) fn close_workers(&self) {
        self.workers.close();
    }
    pub(crate) fn set_admission(&self, agent: &str, active: bool) -> Result<(), BridgeError> {
        if !self.handles(agent) {
            return Err(BridgeError::NotFound);
        }
        self.outbox.set_admission(agent, active)
    }
    pub(crate) fn drain_all_profiles(&self) -> Result<(), BridgeError> {
        for agent in self
            .profiles
            .lock()
            .map_err(|_| BridgeError::Unavailable)?
            .keys()
        {
            self.outbox.set_admission(agent, false)?;
        }
        Ok(())
    }
    pub async fn shutdown(&self) -> Result<(), BridgeError> {
        self.close_workers();
        while !self.workers.is_empty() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        self.outbox.shutdown().await
    }
    pub(crate) async fn shutdown_startup_stores(&self) -> Result<(), BridgeError> {
        self.shutdown().await?;
        if self.startup_owns_stores {
            self.memory
                .shutdown()
                .await
                .map_err(|_| BridgeError::Unavailable)?;
            self.execution
                .shutdown()
                .await
                .map_err(|_| BridgeError::Unavailable)?;
        }
        Ok(())
    }
}
fn engine_error(error: EngineError) -> BridgeError {
    if error.code() == EngineErrorCode::MemoryUnavailable {
        BridgeError::MemoryUnavailable
    } else {
        BridgeError::Unavailable
    }
}
fn authority(identity: &IdentityBinding) -> Result<MemoryAuthority, BridgeError> {
    let scope =
        MemoryScope::agent(identity.individual_id().as_str()).map_err(|_| BridgeError::Invalid)?;
    MemoryAuthority::trusted(identity.clone(), vec![scope.clone()], vec![scope])
        .map_err(|_| BridgeError::Invalid)
}
fn user_effect(
    identity: &IdentityBinding,
    event: Uuid,
    text: &str,
    now: u64,
) -> Result<MemoryEffect, BridgeError> {
    let source = format!("daemon:user:{event}");
    let scope =
        MemoryScope::agent(identity.individual_id().as_str()).map_err(|_| BridgeError::Invalid)?;
    let kind = if text.to_lowercase().contains("prefer") {
        MemoryKind::Preference
    } else {
        MemoryKind::Episode
    };
    let write = MemoryWrite::new(
        format!("daemon:user:{event}"),
        kind,
        scope,
        text,
        1.0,
        MemoryProvenance::new("daemon/accepted-user-statement", &source, now)
            .map_err(|_| BridgeError::Invalid)?,
    )
    .map_err(|_| BridgeError::Invalid)?;
    let candidate = MemoryCandidate::new(
        write,
        CandidateEvidence::new(EvidenceKind::UserStatement, &source, None)
            .map_err(|_| BridgeError::Invalid)?,
    )
    .map_err(|_| BridgeError::Invalid)?;
    MemoryEffect::new(source, identity.clone(), vec![candidate], vec![], vec![])
        .map_err(|_| BridgeError::Invalid)
}
struct UserStatementPolicy {
    accepted: String,
    source_event: String,
}
impl MemoryProjectionPolicy for UserStatementPolicy {
    fn validate(
        &self,
        _: &MemoryAuthority,
        candidate: &MemoryCandidate,
    ) -> Result<(), MemoryPortError> {
        if candidate.evidence().kind() != EvidenceKind::UserStatement
            || candidate.evidence().source_event_id() != self.source_event
            || candidate.write().content() != self.accepted.trim()
        {
            return Err(MemoryPortError::invalid(
                "candidate does not match accepted user statement",
            ));
        }
        Ok(())
    }
}
struct Definitions(AgentDefinition);
#[async_trait]
impl DefinitionResolver for Definitions {
    async fn resolve(&self, _: Uuid, pin: &DefinitionPin) -> Result<AgentDefinition, EngineError> {
        if DefinitionPin::from_definition(&self.0)
            .map_err(|_| EngineError::new(EngineErrorCode::DefinitionUnavailable))?
            != *pin
        {
            return Err(EngineError::new(EngineErrorCode::DefinitionUnavailable));
        }
        Ok(self.0.clone())
    }
}
struct Live(broadcast::Sender<serde_json::Value>);
#[async_trait]
impl EngineLiveEventSink for Live {
    async fn emit(&self, event: EngineLiveEvent) -> Result<(), EngineError> {
        let value = match event {
            EngineLiveEvent::TextDelta { run_id, text } => {
                serde_json::json!({"event":"text.delta","runId":run_id,"text":text})
            }
            EngineLiveEvent::Semantic {
                run_id,
                sequence,
                kind,
            } => {
                serde_json::json!({"event":"semantic","runId":run_id,"sequence":sequence,"kind":kind})
            }
        };
        let _ = self.0.send(value);
        Ok(())
    }
}
fn definition(
    agent: &str,
    config: &AgentConfig,
    version: u32,
    limits: &ExecutionLimits,
) -> Result<AgentDefinition, BridgeError> {
    // Legacy tool rounds do not include the first/final text-only model response.
    let max_turns = config
        .settings
        .as_ref()
        .and_then(|settings| settings.max_tool_iterations)
        .unwrap_or(4)
        .checked_add(1)
        .and_then(|turns| u32::try_from(turns).ok())
        .ok_or(BridgeError::Invalid)?;
    let knowledge = config
        .knowledge
        .as_ref()
        .filter(|items| !items.is_empty())
        .map(|items| format!("## Authored knowledge\n{}", items.join("\n")))
        .unwrap_or_default();
    serde_json::from_value(serde_json::json!({"schema_version":1,"id":format!("daemon-dialogue-{agent}"),"version":version,"name":config.name,"display_name":config.name,"description":"Opt-in persistent conversation","persona":config.legacy_personality(),"system":knowledge,"model":{"provider":config.provider.clone().unwrap_or_else(||"fixture".into()),"model":config.model,"credential_reference":null,"temperature":config.settings.as_ref().and_then(|s|s.temperature)},"source_profile":{"profile_id":"daemon-dialogue-v1","profile_version":1},"resolved_capabilities":[],"memory":{"enabled":true,"namespace":format!("daemon:{agent}"),"retention_days":null},"approval_policy_id":"daemon-dialogue-v1","approval_policy_revision":1,"approval_restrictions":[],"limits":{"max_turns":max_turns,"timeout_ms":limits.budget.max_wall_time_ms.unwrap_or(30000),"max_concurrent_tasks":1},"lifecycle":{"auto_start":false,"restart_on_failure":false,"max_restarts":0,"allows_concurrent_sessions":false},"host_requirements":[]})).map_err(|_|BridgeError::Invalid)
}
#[cfg(test)]
mod tests;
