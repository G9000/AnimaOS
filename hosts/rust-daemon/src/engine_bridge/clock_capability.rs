//! One read-only capability. No filesystem, process, MCP or credential authority.
use super::{outbox::Outbox, *};
pub const CLOCK_ID: &str = "daemon.clock.read";
pub fn clock_manifest() -> Result<CapabilityManifest, String> {
    CapabilityManifest::new(CapabilityManifestInput{id:CLOCK_ID.into(),version:1,kind:CapabilityKind::Knowledge,label:"Current host time".into(),description:"Read one bounded host time observation".into(),input_schema:serde_json::json!({"type":"object","properties":{},"additionalProperties":false}),output_schema:serde_json::json!({"type":"object","properties":{"unix_ms":{"type":"integer","minimum":0}},"required":["unix_ms"],"additionalProperties":false}),side_effects:false,risk_level:RiskLevel::Low,host_permissions:vec![],secret_references:vec![],environment_requirements:vec![],timeout_ms:1000,cancellation_supported:true,max_retries:0,idempotent:false,recovery_mode:RecoveryMode::KeyedIdempotent,supports_streaming:false,supports_artifacts:false,supports_citations:false,compatibility:RuntimeCompatibility{minimum_runtime_schema_version:1,maximum_runtime_schema_version:1,manifest_schema_version:1}}).map_err(|_|"invalid clock manifest".into())
}
pub(super) struct Policy {
    pub enabled: bool,
    pub binding: DurableRunBinding,
    pub clock: Arc<dyn ExecutionClock>,
}
#[async_trait]
impl CurrentPolicyResolver for Policy {
    async fn resolve(
        &self,
        request: EnginePolicyRequest,
    ) -> Result<CurrentPolicyResolution, EngineError> {
        let manifest = clock_manifest().map_err(|_| EngineError::new(EngineErrorCode::Policy))?;
        if !self.enabled
            || request.owner_id() != self.binding.owner
            || request.invocation().run_id() != self.binding.run
            || request.manifest() != &manifest
        {
            return Err(EngineError::new(EngineErrorCode::Policy));
        }
        let context = PolicyContext::new(
            self.binding.owner.to_string(),
            self.binding.individual.individual_id().as_str(),
            &request.definition().id,
            request.definition().version,
            self.binding.scope.policy_workspace_id(),
            CapabilityReferenceId::new(self.binding.scope.id()),
            &manifest,
            request.invocation(),
            request.definition().approval_policy_revision,
            PolicyRestrictions::default(),
            i64::try_from(self.clock.now_ms())
                .map_err(|_| EngineError::new(EngineErrorCode::Policy))?,
        )
        .map_err(|_| EngineError::new(EngineErrorCode::Policy))?;
        Ok(CurrentPolicyResolution::new(context, vec![], None))
    }
}
pub(super) struct Runtime {
    pub enabled: bool,
    pub binding: DurableRunBinding,
    pub outbox: Outbox,
    pub clock: Arc<dyn ExecutionClock>,
}
impl Runtime {
    fn validate(&self, context: &CapabilityExecutionContext) -> Result<(), CapabilityError> {
        let expected = CapabilityExecutionContext::for_attempt(
            context.invocation().clone(),
            context.attempt().clone(),
        )
        .map_err(|_| CapabilityError::validation())?;
        if !self.enabled
            || context.invocation().run_id() != self.binding.run
            || context.invocation().capability_id() != CLOCK_ID
            || context.invocation().manifest_version() != 1
            || context.normalized_arguments() != &serde_json::json!({})
            || context.references() != expected.references()
        {
            return Err(CapabilityError::validation());
        }
        Ok(())
    }
}
#[async_trait]
impl EngineCapabilityRuntime for Runtime {
    fn manifest(&self, id: &str, version: u32) -> Option<CapabilityManifest> {
        if self.enabled && id == CLOCK_ID && version == 1 {
            clock_manifest().ok()
        } else {
            None
        }
    }
    async fn execute(
        &self,
        context: CapabilityExecutionContext,
    ) -> Result<EngineCapabilityResult, CapabilityError> {
        self.validate(&context)?;
        // The registry installs and validates the capability-specific lease/fence
        // before invoking this single read-only executor. The host receipt remains
        // the authority for retained output and cross-process deduplication.
        let manifest = clock_manifest().map_err(|_| CapabilityError::validation())?;
        let mut catalog = ManifestCatalog::default();
        catalog
            .register_manifest(manifest.clone())
            .map_err(|_| CapabilityError::validation())?;
        let mut registry = CapabilityRegistry::new(catalog);
        registry
            .register_executor(Arc::new(ClockExecutor {
                manifest,
                outbox: self.outbox.clone(),
                clock: self.clock.clone(),
            }))
            .map_err(|_| CapabilityError::validation())?;
        registry.execute(context.clone()).await?;
        self.outbox
            .clock_result(context.invocation().clone(), None)
            .await
            .map_err(|_| CapabilityError::unavailable())?
            .ok_or_else(CapabilityError::reconciliation)
    }
    async fn completed_output(
        &self,
        context: &CapabilityExecutionContext,
        durable: &DurableCapabilityResult,
    ) -> Result<CapabilityResult, CapabilityError> {
        self.validate(context)?;
        let result = self
            .outbox
            .clock_result(context.invocation().clone(), None)
            .await
            .map_err(|_| CapabilityError::unavailable())?
            .ok_or_else(CapabilityError::reconciliation)?;
        if &result.durable != durable {
            return Err(CapabilityError::validation());
        }
        Ok(result.output)
    }
    async fn recover_dispatched(
        &self,
        context: CapabilityExecutionContext,
    ) -> Result<RecoveryAction, CapabilityError> {
        self.validate(&context)?;
        Ok(
            match self
                .outbox
                .clock_result(context.invocation().clone(), None)
                .await
                .map_err(|_| CapabilityError::unavailable())?
            {
                Some(result) => RecoveryAction::Completed(result.durable),
                None => RecoveryAction::RecoveryRequired,
            },
        )
    }
    async fn execute_retry(
        &self,
        _: CapabilityExecutionContext,
        _: CapabilityRetryAuthorization,
    ) -> Result<EngineCapabilityResult, CapabilityError> {
        Err(CapabilityError::reconciliation())
    }
}

struct ClockExecutor {
    manifest: CapabilityManifest,
    outbox: Outbox,
    clock: Arc<dyn ExecutionClock>,
}
#[async_trait]
impl CapabilityExecutor for ClockExecutor {
    fn manifest(&self) -> &CapabilityManifest {
        &self.manifest
    }
    async fn execute(
        &self,
        context: CapabilityExecutionContext,
    ) -> Result<CapabilityResult, CapabilityError> {
        context
            .execution_fence()
            .ok_or_else(CapabilityError::validation)?
            .ensure_valid()
            .await?;
        Ok(self
            .outbox
            .clock_result(context.invocation().clone(), Some(self.clock.now_ms()))
            .await
            .map_err(|_| CapabilityError::unavailable())?
            .ok_or_else(CapabilityError::reconciliation)?
            .output)
    }
    async fn reconcile(
        &self,
        context: CapabilityExecutionContext,
    ) -> Result<ReconcileOutcome, CapabilityError> {
        Ok(
            match self
                .outbox
                .clock_result(context.invocation().clone(), None)
                .await
                .map_err(|_| CapabilityError::unavailable())?
            {
                Some(result) => ReconcileOutcome::Completed(result.output),
                None => ReconcileOutcome::RecoveryRequired,
            },
        )
    }
}
