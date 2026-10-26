use anima_core::{execution_limits::*, *};
use anima_execution_store_sqlite::SqliteExecutionStore;
use anima_harness::durable::*;
use async_trait::async_trait;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tokio::sync::Notify;
use uuid::Uuid;
mod common;

struct Model;
#[async_trait]
impl ModelAdapter for Model {
    fn provider(&self) -> &str {
        "fixture"
    }
    fn single_attempt_contract(&self) -> bool {
        true
    }
    fn generated_usage_is_complete(&self, _: &ModelGenerateResponse) -> bool {
        true
    }
    async fn generate(
        &self,
        _: &AgentConfig,
        request: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        let done = request
            .messages
            .iter()
            .any(|message| message.role == MessageRole::Tool);
        Ok(ModelGenerateResponse {
            content: Content {
                text: if done {
                    "recorded".into()
                } else {
                    String::new()
                },
                ..Content::default()
            },
            tool_calls: (!done).then(|| {
                vec![ToolCall {
                    id: "one-record".into(),
                    name: "fixture.record".into(),
                    args: Default::default(),
                }]
            }),
            usage: TokenUsage::default(),
            stop_reason: if done {
                ModelStopReason::End
            } else {
                ModelStopReason::ToolCall
            },
        })
    }
}
struct Effect {
    manifest: CapabilityManifest,
    calls: Arc<AtomicUsize>,
    started: Arc<Notify>,
    finish: Arc<Notify>,
    wait: bool,
}
#[async_trait]
impl CapabilityExecutor for Effect {
    fn manifest(&self) -> &CapabilityManifest {
        &self.manifest
    }
    async fn execute(
        &self,
        _: CapabilityExecutionContext,
    ) -> Result<CapabilityResult, CapabilityError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.started.notify_one();
        if self.wait {
            self.finish.notified().await;
        }
        Ok(CapabilityResult::new(serde_json::json!({"ok":true})))
    }
    async fn reconcile(
        &self,
        _: CapabilityExecutionContext,
    ) -> Result<ReconcileOutcome, CapabilityError> {
        Ok(ReconcileOutcome::RecoveryRequired)
    }
}
struct Policy {
    binding: DurableRunBinding,
    wrong_scope: bool,
    wrong_actor: bool,
    clock: Arc<ManualExecutionClock>,
}
#[async_trait]
impl CurrentPolicyResolver for Policy {
    async fn resolve(
        &self,
        request: EnginePolicyRequest,
    ) -> Result<CurrentPolicyResolution, EngineError> {
        let context = PolicyContext::new(
            self.binding.owner.to_string(),
            if self.wrong_actor {
                "other-individual"
            } else {
                self.binding.individual.individual_id().as_str()
            },
            request.definition().id.clone(),
            1,
            if self.wrong_scope {
                "other-application".into()
            } else {
                self.binding.scope.policy_workspace_id()
            },
            CapabilityReferenceId::new(self.binding.run),
            request.manifest(),
            request.invocation(),
            1,
            PolicyRestrictions::default(),
            self.clock.now_ms() as i64,
        )
        .unwrap();
        Ok(CurrentPolicyResolution::new(context, vec![], None))
    }
}
fn manifest() -> CapabilityManifest {
    CapabilityManifest::new(CapabilityManifestInput {
        id: "fixture.record".into(),
        version: 1,
        kind: CapabilityKind::Custom,
        label: "Record".into(),
        description: "bounded non-filesystem effect".into(),
        input_schema: serde_json::json!({"type":"object"}),
        output_schema: serde_json::json!({"type":"object"}),
        side_effects: true,
        risk_level: RiskLevel::Low,
        host_permissions: vec![],
        secret_references: vec![],
        environment_requirements: vec![],
        timeout_ms: 1000,
        cancellation_supported: true,
        max_retries: 0,
        idempotent: false,
        recovery_mode: RecoveryMode::NonRetryable,
        supports_streaming: false,
        supports_artifacts: false,
        supports_citations: false,
        compatibility: RuntimeCompatibility {
            minimum_runtime_schema_version: 1,
            maximum_runtime_schema_version: 1,
            manifest_schema_version: 1,
        },
    })
    .unwrap()
}
async fn scenario(wrong_scope: bool, wrong_actor: bool, stop_during_effect: bool) {
    let temp = tempfile::tempdir().unwrap();
    let clock = Arc::new(ManualExecutionClock::default());
    let manifest = manifest();
    let protection = PersistenceProtection::allow_model_payloads(
        PersistenceSnapshotSealKey::new([9; 32]).unwrap(),
        vec![PersistenceCapabilitySecretInventory::new(&manifest, vec![]).unwrap()],
    )
    .unwrap();
    let store = Arc::new(
        SqliteExecutionStore::open(
            temp.path().join("execution.sqlite"),
            protection,
            clock.clone(),
        )
        .await
        .unwrap(),
    );
    let owner = Uuid::new_v4();
    let run = Uuid::new_v4();
    let binding = DurableRunBinding::new(
        owner,
        IdentityBinding::new(owner.to_string(), "assistant").unwrap(),
        Uuid::new_v4(),
        run,
        ApplicationScope::new(owner, Uuid::new_v4()).unwrap(),
    )
    .unwrap();
    let scope = ExecutionScope::new(
        "allowance".into(),
        run.to_string(),
        ExecutionLimits::default(),
        clock.now_ms(),
    )
    .unwrap();
    let budget = store.register_budget(owner, scope.clone()).await.unwrap();
    let control = DurableControl::new();
    let execution = control
        .execution_context(&scope, run, Arc::new(budget.clone()), clock.clone())
        .unwrap();
    let mut definition = common::definition();
    definition.resolved_capabilities = vec![ResolvedCapability {
        capability_id: manifest.id.clone(),
        manifest_version: 1,
        schema_digest: manifest.schema_digest().into(),
        override_config: None,
        approval_policy_revision: 1,
    }];
    store
        .apply_authoritative_policy(
            owner,
            AuthoritativePolicyChange::create(
                AuthoritativePolicyState::active(owner, &definition.id, 1, 1, None).unwrap(),
            ),
        )
        .await
        .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let started = Arc::new(Notify::new());
    let finish = Arc::new(Notify::new());
    let mut catalog = ManifestCatalog::default();
    catalog.register_manifest(manifest.clone()).unwrap();
    let mut registry = CapabilityRegistry::new(catalog);
    registry
        .register_executor(Arc::new(Effect {
            manifest,
            calls: calls.clone(),
            started: started.clone(),
            finish: finish.clone(),
            wait: stop_during_effect,
        }))
        .unwrap();
    let harness = DurableHarness::new(
        store.clone(),
        Arc::new(Model),
        Arc::new(common::Definitions(definition.clone())),
        Arc::new(Policy {
            binding: binding.clone(),
            wrong_scope,
            wrong_actor,
            clock: clock.clone(),
        }),
        Arc::new(registry),
        clock,
        Arc::new(common::Live),
    );
    harness
        .admit(&binding, &definition, "record once", &execution)
        .await
        .unwrap();
    let result = if stop_during_effect {
        let (result, ()) = tokio::join!(harness.run(&binding, &control, &execution), async {
            started.notified().await;
            control.stop();
            finish.notify_one();
        });
        result
    } else {
        harness.run(&binding, &control, &execution).await
    };
    if wrong_scope || wrong_actor {
        assert_eq!(result.unwrap_err().code(), EngineErrorCode::Policy);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    } else if stop_during_effect {
        assert_eq!(result.unwrap(), EngineRunOutcome::Cancelled);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            budget
                .snapshot()
                .await
                .unwrap()
                .consumption()
                .unwrap()
                .inflight_operations,
            0
        );
    } else {
        assert!(matches!(
            result.unwrap(),
            EngineRunOutcome::Completed { .. }
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
    store.shutdown().await.unwrap();
}
#[tokio::test]
async fn non_filesystem_capability_has_no_directory_requirement() {
    scenario(false, false, false).await;
}
#[tokio::test]
async fn admitted_scope_and_individual_cannot_borrow_another_policy_context() {
    scenario(true, false, false).await;
    scenario(false, true, false).await;
}
#[tokio::test]
async fn stop_retains_custody_and_accounting_until_started_effect_retires() {
    scenario(false, false, true).await;
}
