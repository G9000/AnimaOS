use crate::common::{definition, Definitions};
use anima_core::{execution_limits::*, *};
use anima_execution_store_sqlite::SqliteExecutionStore;
use anima_harness::durable::*;
use async_trait::async_trait;
use std::{
    io::{BufRead, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
use uuid::Uuid;

pub const OWNER: Uuid = Uuid::from_u128(201);
pub const RUN: Uuid = Uuid::from_u128(203);
pub const ALLOWANCE: &str = "physical-recovery-original";
pub fn barrier(point: &str) {
    if std::env::var("ANIMA_DURABLE_RECOVERY_POINT")
        .ok()
        .as_deref()
        == Some(point)
    {
        println!("READY:{point}");
        std::io::stdout().flush().unwrap();
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line).unwrap();
        panic!("parent must terminate the fixture at its barrier");
    }
}
pub fn append(path: &Path, line: &str) {
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
        .unwrap();
    writeln!(file, "{line}").unwrap();
    file.sync_all().unwrap();
}
pub fn entries(path: &Path) -> Vec<String> {
    match std::fs::read_to_string(path) {
        Ok(text) => text.lines().map(str::to_owned).collect(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => vec![],
        Err(error) => panic!("fixture ledger unavailable: {error}"),
    }
}
pub fn manifest() -> CapabilityManifest {
    CapabilityManifest::new(CapabilityManifestInput {
        id: "fixture.record".into(),
        version: 1,
        kind: CapabilityKind::Custom,
        label: "Record".into(),
        description: "durable fixture effect".into(),
        input_schema: serde_json::json!({"type":"object"}),
        output_schema: serde_json::json!({"type":"object"}),
        side_effects: true,
        risk_level: RiskLevel::Low,
        host_permissions: vec![],
        secret_references: vec![],
        environment_requirements: vec![],
        timeout_ms: 1000,
        cancellation_supported: true,
        max_retries: 1,
        idempotent: true,
        recovery_mode: RecoveryMode::KeyedIdempotent,
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
pub fn protection() -> PersistenceProtection {
    PersistenceProtection::allow_model_payloads(
        PersistenceSnapshotSealKey::new([83; 32]).unwrap(),
        vec![PersistenceCapabilitySecretInventory::new(&manifest(), vec![]).unwrap()],
    )
    .unwrap()
}
pub async fn open(path: &Path, clock: Arc<ManualExecutionClock>) -> SqliteExecutionStore {
    SqliteExecutionStore::open(path.join("execution.sqlite"), protection(), clock)
        .await
        .unwrap()
}
struct Model(PathBuf);
#[async_trait]
impl ModelAdapter for Model {
    fn provider(&self) -> &str {
        "fixture"
    }
    fn single_attempt_contract(&self) -> bool {
        true
    }
    async fn generate(
        &self,
        _: &AgentConfig,
        _: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        unreachable!()
    }
    async fn stream(
        &self,
        _: &AgentConfig,
        request: &ModelGenerateRequest,
        sink: &dyn ModelStreamSink,
    ) -> Result<(), String> {
        let done = request.messages.iter().any(|m| m.role == MessageRole::Tool);
        append(&self.0, if done { "final" } else { "initial" });
        barrier(if done {
            "provider-final-entered"
        } else {
            "provider-entered"
        });
        let usage = TokenUsage {
            prompt_tokens: 3,
            completion_tokens: 2,
            total_tokens: 5,
            ..TokenUsage::default()
        };
        sink.complete_usage(usage.clone()).await?;
        sink.emit(ModelStreamFrame::Final(ModelGenerateResponse {
            content: Content {
                text: if done {
                    "recovered dialogue".into()
                } else {
                    String::new()
                },
                ..Content::default()
            },
            tool_calls: (!done).then(|| {
                vec![ToolCall {
                    id: "record-once".into(),
                    name: "fixture.record".into(),
                    args: Default::default(),
                }]
            }),
            usage,
            stop_reason: if done {
                ModelStopReason::End
            } else {
                ModelStopReason::ToolCall
            },
        }))
        .await
    }
}
struct Effect {
    manifest: CapabilityManifest,
    path: PathBuf,
    retry: bool,
}
#[async_trait]
impl CapabilityExecutor for Effect {
    fn manifest(&self) -> &CapabilityManifest {
        &self.manifest
    }
    async fn execute(
        &self,
        context: CapabilityExecutionContext,
    ) -> Result<CapabilityResult, CapabilityError> {
        if self.retry && context.attempt().number() == 1 {
            return Err(CapabilityError::execution());
        }
        context.execution_fence().unwrap().ensure_valid().await?;
        append(&self.path, &context.invocation().id().to_string());
        barrier(if self.retry {
            "retry-after-effect"
        } else {
            "effect-before-receipt"
        });
        Ok(CapabilityResult::new(serde_json::json!({"ok":true})))
    }
    async fn reconcile(
        &self,
        context: CapabilityExecutionContext,
    ) -> Result<ReconcileOutcome, CapabilityError> {
        Ok(
            if entries(&self.path).contains(&context.invocation().id().to_string()) {
                ReconcileOutcome::Completed(CapabilityResult::new(serde_json::json!({"ok":true})))
            } else {
                ReconcileOutcome::AuthoritativeAbsence
            },
        )
    }
}
struct Policy {
    binding: DurableRunBinding,
    clock: Arc<ManualExecutionClock>,
}
#[async_trait]
impl CurrentPolicyResolver for Policy {
    async fn resolve(
        &self,
        request: EnginePolicyRequest,
    ) -> Result<CurrentPolicyResolution, EngineError> {
        Ok(CurrentPolicyResolution::new(
            PolicyContext::new(
                OWNER.to_string(),
                "fixture-agent",
                request.definition().id.clone(),
                1,
                self.binding.scope.policy_workspace_id(),
                CapabilityReferenceId::new(RUN),
                request.manifest(),
                request.invocation(),
                1,
                PolicyRestrictions::default(),
                self.clock.now_ms() as i64,
            )
            .unwrap(),
            vec![],
            None,
        ))
    }
}
struct Live {
    dispatches: AtomicUsize,
    retry: bool,
}
#[async_trait]
impl EngineLiveEventSink for Live {
    async fn emit(&self, event: EngineLiveEvent) -> Result<(), EngineError> {
        if let EngineLiveEvent::Semantic { kind, .. } = event {
            match kind {
                RuntimeEventKind::InvocationDispatchPrepared => {
                    if self.retry && self.dispatches.fetch_add(1, Ordering::SeqCst) == 1 {
                        barrier("retry-before-entry");
                    }
                }
                RuntimeEventKind::ModelCompleted => barrier("model-checkpoint"),
                RuntimeEventKind::CapabilityCompleted => barrier("result-committed"),
                RuntimeEventKind::RunCompleted => barrier("terminal-committed"),
                _ => {}
            }
        }
        Ok(())
    }
}
pub async fn execute(
    path: &Path,
    retry: bool,
    reopening: bool,
    pause_unknown_provider: bool,
) -> Result<EngineRunOutcome, EngineError> {
    let clock = Arc::new(ManualExecutionClock::new(if reopening {
        100_000
    } else {
        1_000
    }));
    let store = Arc::new(open(path, clock.clone()).await);
    let binding = DurableRunBinding::new(
        OWNER,
        IdentityBinding::new(OWNER.to_string(), "fixture-agent").unwrap(),
        Uuid::from_u128(202),
        RUN,
        ApplicationScope::new(OWNER, Uuid::from_u128(204)).unwrap(),
    )
    .unwrap();
    let mut definition = definition();
    let manifest = manifest();
    definition.resolved_capabilities = vec![ResolvedCapability {
        capability_id: manifest.id.clone(),
        manifest_version: 1,
        schema_digest: manifest.schema_digest().into(),
        override_config: None,
        approval_policy_revision: 1,
    }];
    let port = if reopening {
        store.bind_budget(OWNER, ALLOWANCE).await.unwrap()
    } else {
        let scope = ExecutionScope::new(
            ALLOWANCE.into(),
            RUN.to_string(),
            ExecutionLimits {
                max_inflight_operations: 1,
                ..ExecutionLimits::default()
            },
            clock.now_ms(),
        )
        .unwrap();
        store.register_budget(OWNER, scope).await.unwrap()
    };
    let scope = port.snapshot().await.unwrap();
    let control = DurableControl::new();
    if pause_unknown_provider {
        control.pause();
    }
    let execution = control
        .execution_context(&scope, RUN, Arc::new(port), clock.clone())
        .unwrap();
    let mut catalog = ManifestCatalog::default();
    catalog.register_manifest(manifest.clone()).unwrap();
    let mut registry = CapabilityRegistry::new(catalog);
    registry
        .register_executor(Arc::new(Effect {
            manifest,
            path: path.join("effects.log"),
            retry,
        }))
        .unwrap();
    if !reopening {
        store
            .apply_authoritative_policy(
                OWNER,
                AuthoritativePolicyChange::create(
                    AuthoritativePolicyState::active(OWNER, &definition.id, 1, 1, None).unwrap(),
                ),
            )
            .await
            .unwrap();
    }
    let harness = DurableHarness::new(
        store.clone(),
        Arc::new(Model(path.join("providers.log"))),
        Arc::new(Definitions(definition.clone())),
        Arc::new(Policy {
            binding: binding.clone(),
            clock: clock.clone(),
        }),
        Arc::new(registry),
        clock.clone(),
        Arc::new(Live {
            dispatches: AtomicUsize::new(0),
            retry,
        }),
    );
    harness
        .admit(
            &binding,
            &definition,
            "remember this conversation",
            &execution,
        )
        .await
        .unwrap();
    // Keep the test's orchestration future from embedding two large debug-build
    // engine futures on libtest's small worker stack. There is no recursive run.
    let first = Box::pin(harness.run(&binding, &control, &execution)).await;
    let outcome = if retry && !reopening {
        assert_eq!(first.unwrap_err().code(), EngineErrorCode::Capability);
        clock.advance_ms(40_000).unwrap();
        Box::pin(harness.run(&binding, &control, &execution)).await
    } else {
        first
    };
    store.shutdown().await.unwrap();
    outcome
}
