//! Explicit embedding facade. Hosts persist admission/version/identity/scope
//! mappings before dispatch and inject their durable shared allowance.
mod capabilities;
mod controls;
mod memory;
use anima_core::{execution_limits::ExecutionContext, *};
use capabilities::ControlledCapabilities;
pub use controls::DurableControl;
pub use memory::{MemoryProjectionReadiness, PersistentAgentMemory};
use std::sync::Arc;
use uuid::Uuid;

/// Opaque non-filesystem application scope. Its compatibility policy key is in
/// an owner-qualified namespace; hosts must reserve it against legacy IDs.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ApplicationScope {
    owner: Uuid,
    id: Uuid,
}
impl ApplicationScope {
    pub fn new(owner: Uuid, id: Uuid) -> Result<Self, EngineError> {
        if owner.is_nil() || id.is_nil() {
            return Err(invalid());
        }
        Ok(Self { owner, id })
    }
    pub fn owner(&self) -> Uuid {
        self.owner
    }
    pub fn id(&self) -> Uuid {
        self.id
    }
    pub fn policy_workspace_id(&self) -> String {
        format!("application:v1:{}:{}", self.owner, self.id)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DurableRunBinding {
    pub owner: Uuid,
    pub individual: IdentityBinding,
    pub session: Uuid,
    pub run: Uuid,
    pub scope: ApplicationScope,
}
impl DurableRunBinding {
    pub fn new(
        owner: Uuid,
        individual: IdentityBinding,
        session: Uuid,
        run: Uuid,
        scope: ApplicationScope,
    ) -> Result<Self, EngineError> {
        if owner.is_nil()
            || session.is_nil()
            || run.is_nil()
            || individual.owner_id().as_str() != owner.to_string()
            || scope.owner() != owner
        {
            return Err(invalid());
        }
        Ok(Self {
            owner,
            individual,
            session,
            run,
            scope,
        })
    }
}
fn invalid() -> EngineError {
    EngineError::new(EngineErrorCode::InvalidState)
}
struct NeverCrash;
impl EngineCrashInjector for NeverCrash {
    fn should_crash(&self, _: EngineCrashPoint) -> bool {
        false
    }
}
struct PinnedDefinitions {
    owner: Uuid,
    definition: AgentDefinition,
}
#[async_trait::async_trait]
impl DefinitionResolver for PinnedDefinitions {
    async fn resolve(
        &self,
        owner: Uuid,
        pin: &DefinitionPin,
    ) -> Result<AgentDefinition, EngineError> {
        if owner != self.owner
            || DefinitionPin::from_definition(&self.definition).map_err(|_| invalid())? != *pin
        {
            return Err(EngineError::new(EngineErrorCode::DefinitionUnavailable));
        }
        Ok(self.definition.clone())
    }
}
struct BoundPolicy<P> {
    inner: Arc<P>,
    binding: DurableRunBinding,
}
#[async_trait::async_trait]
impl<P: CurrentPolicyResolver> CurrentPolicyResolver for BoundPolicy<P> {
    async fn resolve(
        &self,
        request: EnginePolicyRequest,
    ) -> Result<CurrentPolicyResolution, EngineError> {
        if request.owner_id() != self.binding.owner
            || request.invocation().run_id() != self.binding.run
        {
            return Err(EngineError::new(EngineErrorCode::Policy));
        }
        let resolved = self.inner.resolve(request).await?;
        let context = resolved.context();
        if context.owner_id != self.binding.owner.to_string()
            || context.actor_id != self.binding.individual.individual_id().as_str()
            || context.workspace_id != self.binding.scope.policy_workspace_id()
        {
            return Err(EngineError::new(EngineErrorCode::Policy));
        }
        Ok(resolved)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct AcceptedRun {
    version: u32,
    binding: DurableRunBinding,
    definition: AgentDefinition,
    input: String,
    allowance: anima_core::execution_limits::ExecutionBindingHint,
    allowance_authority: anima_core::execution_limits::ExecutionBudgetAuthority,
    memory_context: Option<memory::MemoryContext>,
}

/// Delegates all model/tool orchestration to DurableAgentEngine. It does not
/// invoke AgentRuntime, allocate an ephemeral budget, or require cwd/world.
pub struct DurableHarness<S, M, D, P, R, C, L> {
    store: Arc<S>,
    model: Arc<M>,
    definitions: Arc<D>,
    policy: Arc<P>,
    capabilities: Arc<R>,
    clock: Arc<C>,
    live: Arc<L>,
}
impl<S, M, D, P, R, C, L> DurableHarness<S, M, D, P, R, C, L>
where
    S: ExecutionStore + HostRunAdmissionStore,
    M: ModelAdapter,
    D: DefinitionResolver,
    P: CurrentPolicyResolver,
    R: EngineCapabilityRuntime,
    C: ExecutionClock,
    L: EngineLiveEventSink,
{
    pub fn new(
        store: Arc<S>,
        model: Arc<M>,
        definitions: Arc<D>,
        policy: Arc<P>,
        capabilities: Arc<R>,
        clock: Arc<C>,
        live: Arc<L>,
    ) -> Self {
        Self {
            store,
            model,
            definitions,
            policy,
            capabilities,
            clock,
            live,
        }
    }
    /// Idempotent admission. Definition resolvers and hosts must retain the exact
    /// accepted definition version and the binding for all subsequent retries.
    pub async fn admit(
        &self,
        binding: &DurableRunBinding,
        definition: &AgentDefinition,
        input: &str,
        execution: &ExecutionContext,
    ) -> Result<StoredRun, EngineError> {
        if definition.memory.enabled {
            return Err(invalid());
        }
        self.admit_internal(binding, definition, input, execution, None)
            .await
    }
    pub async fn admit_persistent(
        &self,
        binding: &DurableRunBinding,
        definition: &AgentDefinition,
        input: &str,
        execution: &ExecutionContext,
        memory: &PersistentAgentMemory,
    ) -> Result<StoredRun, EngineError> {
        if !definition.memory.enabled {
            return Err(invalid());
        }
        let context = if let Some(payload) = self
            .store
            .load_host_run(binding.owner, binding.run)
            .await
            .map_err(|_| invalid())?
        {
            let accepted: AcceptedRun = serde_json::from_slice(&payload).map_err(|_| invalid())?;
            accepted.memory_context.ok_or_else(invalid)?
        } else {
            memory
                .snapshot(&binding.individual, input)
                .await
                .map_err(|_| EngineError::new(EngineErrorCode::MemoryUnavailable))?
        };
        self.admit_internal(binding, definition, input, execution, Some(context))
            .await
    }
    async fn admit_internal(
        &self,
        binding: &DurableRunBinding,
        definition: &AgentDefinition,
        input: &str,
        execution: &ExecutionContext,
        memory_context: Option<memory::MemoryContext>,
    ) -> Result<StoredRun, EngineError> {
        let expected = DurableRunBinding::new(
            binding.owner,
            binding.individual.clone(),
            binding.session,
            binding.run,
            binding.scope.clone(),
        )?;
        if &expected != binding {
            return Err(invalid());
        }
        let mapped = self
            .store
            .reserve_application_scope(binding.owner, binding.scope.id())
            .await
            .map_err(|_| EngineError::new(EngineErrorCode::Store))?;
        if mapped != binding.scope.policy_workspace_id() {
            return Err(invalid());
        }
        if execution.run_id != binding.run.to_string() {
            return Err(invalid());
        }
        let authority = execution.port.authority_binding().ok_or_else(invalid)?;
        authority.validate().map_err(|_| invalid())?;
        if authority.owner_id != binding.owner.to_string()
            || authority.scope_id != execution.scope_id
        {
            return Err(invalid());
        }
        if self
            .store
            .load_host_run(binding.owner, binding.run)
            .await
            .map_err(|_| invalid())?
            .is_none()
        {
            let resolved = self
                .definitions
                .resolve(
                    binding.owner,
                    &DefinitionPin::from_definition(definition).map_err(|_| invalid())?,
                )
                .await?;
            if &resolved != definition {
                return Err(EngineError::new(EngineErrorCode::DefinitionUnavailable));
            }
        }
        let payload = serde_json::to_vec(&AcceptedRun {
            version: 1,
            binding: binding.clone(),
            definition: definition.clone(),
            input: input.into(),
            allowance: execution.binding_hint(),
            allowance_authority: authority,
            memory_context,
        })
        .map_err(|_| invalid())?;
        self.store
            .admit_host_run(binding.owner, binding.run, payload)
            .await
            .map_err(|_| invalid())?;
        if let Some(stored) = self
            .store
            .load_run(binding.owner, binding.run)
            .await
            .map_err(|_| EngineError::new(EngineErrorCode::Store))?
        {
            if stored.run().session_id() != binding.session
                || stored.run().definition_id() != definition.id
                || stored.run().definition_version() != definition.version
                || stored.initial_input().text_value() != input
            {
                return Err(invalid());
            }
            return Ok(stored);
        }
        let session = Session::new_for_definition(
            binding.session,
            definition,
            SessionConcurrencyPolicy::Serial,
        )
        .map_err(|_| invalid())?;
        let run = Run::queued(
            binding.run,
            binding.session,
            &definition.id,
            definition.version,
        )
        .map_err(|_| invalid())?;
        let session_version = self
            .store
            .load_session_version(binding.owner, binding.session)
            .await
            .map_err(|_| EngineError::new(EngineErrorCode::Store))?
            .unwrap_or(0);
        let created = self
            .store
            .create_run(
                binding.owner,
                CreateRun::new_for_owner(
                    binding.owner,
                    session,
                    run,
                    session_version,
                    SessionConcurrencyPolicy::Serial,
                )
                .with_initial_input(
                    DurableRunInput::text(binding.owner, binding.session, binding.run, input)
                        .map_err(|_| invalid())?,
                )
                .map_err(|_| invalid())?,
            )
            .await;
        match created {
            Ok(stored) => Ok(stored),
            Err(_) => self
                .store
                .load_run(binding.owner, binding.run)
                .await
                .map_err(|_| EngineError::new(EngineErrorCode::Store))?
                .filter(|stored| {
                    stored.run().session_id() == binding.session
                        && stored.run().definition_id() == definition.id
                        && stored.run().definition_version() == definition.version
                        && stored.initial_input().text_value() == input
                })
                .ok_or_else(|| EngineError::new(EngineErrorCode::Store)),
        }
    }
    pub async fn run(
        &self,
        binding: &DurableRunBinding,
        control: &DurableControl,
        execution: &ExecutionContext,
    ) -> Result<EngineRunOutcome, EngineError> {
        self.run_internal(binding, control, execution, None).await
    }
    pub async fn run_persistent(
        &self,
        binding: &DurableRunBinding,
        control: &DurableControl,
        execution: &ExecutionContext,
        memory: &PersistentAgentMemory,
    ) -> Result<EngineRunOutcome, EngineError> {
        self.run_internal(binding, control, execution, Some(memory))
            .await
    }
    async fn run_internal(
        &self,
        binding: &DurableRunBinding,
        control: &DurableControl,
        execution: &ExecutionContext,
        memory: Option<&PersistentAgentMemory>,
    ) -> Result<EngineRunOutcome, EngineError> {
        if execution.run_id != binding.run.to_string() {
            return Err(invalid());
        }
        DurableRunBinding::new(
            binding.owner,
            binding.individual.clone(),
            binding.session,
            binding.run,
            binding.scope.clone(),
        )?;
        let payload = self
            .store
            .load_host_run(binding.owner, binding.run)
            .await
            .map_err(|_| EngineError::new(EngineErrorCode::Store))?
            .ok_or_else(invalid)?;
        let accepted: AcceptedRun = serde_json::from_slice(&payload).map_err(|_| invalid())?;
        if accepted.version != 1
            || &accepted.binding != binding
            || accepted.allowance != execution.binding_hint()
        {
            return Err(invalid());
        }
        if execution.port.authority_binding().as_ref() != Some(&accepted.allowance_authority) {
            return Err(invalid());
        }
        let definition = accepted.definition;
        if definition.memory.enabled != (memory.is_some() && accepted.memory_context.is_some()) {
            return Err(invalid());
        }
        let definitions = Arc::new(PinnedDefinitions {
            owner: binding.owner,
            definition,
        });
        let policy = Arc::new(BoundPolicy {
            inner: self.policy.clone(),
            binding: binding.clone(),
        });
        let capabilities = Arc::new(ControlledCapabilities {
            inner: self.capabilities.clone(),
            execution: execution.clone(),
        });
        let context = memory
            .zip(accepted.memory_context)
            .map(|(memory, captured)| (memory.clone(), binding.individual.clone(), captured));
        let model = Arc::new(memory::MemoryModel::new(self.model.clone(), context));
        let engine = DurableAgentEngine::new(
            self.store.clone(),
            model.clone(),
            definitions,
            policy,
            capabilities,
            self.clock.clone(),
            self.live.clone(),
            Arc::new(NeverCrash),
            DurableEngineConfig::default(),
        )?;
        let outcome = engine
            .run_controlled(binding.owner, binding.run, control, execution)
            .await;
        if model.memory_failed() {
            Err(EngineError::new(EngineErrorCode::MemoryUnavailable))
        } else {
            outcome
        }
    }
}

/// Explicit host opt-in support; no implicit filesystem configuration.
pub mod hosted;
