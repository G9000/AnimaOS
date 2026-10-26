use anima_core::{execution_limits::*, *};
use anima_execution_store_sqlite::SqliteExecutionStore;
use anima_harness::durable::*;
use anima_memory::{IdentityRegistry, MemoryAuthority};
use anima_memory_store_sqlite::{BoundMemoryStore, SqliteMemoryStore};
use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use uuid::Uuid;
mod common;
use common::{definition, Definitions, Live, Model};

const SECRET: &str = "private shared secret: lavender tea";
const INPUT: &str = "what lavender tea do I like?";

struct Ready(bool);
#[async_trait]
impl MemoryProjectionReadiness for Ready {
    async fn ensure_projected(&self, _: &IdentityBinding) -> Result<(), MemoryPortError> {
        if self.0 {
            Ok(())
        } else {
            Err(MemoryPortError::unsupported(
                "accepted effects still pending",
            ))
        }
    }
}

struct Fixture {
    _temp: tempfile::TempDir,
    clock: Arc<ManualExecutionClock>,
    store: Arc<SqliteExecutionStore>,
    memory: SqliteMemoryStore,
    bound: Arc<BoundMemoryStore>,
    authority: MemoryAuthority,
    binding: DurableRunBinding,
    definition: AgentDefinition,
    control: DurableControl,
    execution: ExecutionContext,
}
impl Fixture {
    async fn new(expiring: bool, with_tool: bool) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let clock = Arc::new(ManualExecutionClock::new(10));
        let manifest = manifest();
        let store = Arc::new(
            SqliteExecutionStore::open(
                temp.path().join("execution.sqlite"),
                PersistenceProtection::allow_model_payloads(
                    PersistenceSnapshotSealKey::new([4; 32]).unwrap(),
                    vec![PersistenceCapabilitySecretInventory::new(&manifest, vec![]).unwrap()],
                )
                .unwrap(),
                clock.clone(),
            )
            .await
            .unwrap(),
        );
        let memory = SqliteMemoryStore::open(temp.path().join("memory.sqlite"), clock.clone())
            .await
            .unwrap();
        let owner = Uuid::new_v4();
        let identity = IdentityBinding::new(owner.to_string(), "assistant").unwrap();
        memory.register_identity(identity.clone()).await.unwrap();
        let private = MemoryScope::agent("assistant").unwrap();
        let shared = MemoryScope::workspace("shared-room").unwrap();
        let authority = MemoryAuthority::trusted(
            identity.clone(),
            vec![private.clone(), shared.clone()],
            vec![private, shared.clone()],
        )
        .unwrap();
        let bound = Arc::new(memory.bind(authority.clone()).await.unwrap());
        let mut write = MemoryWrite::new(
            "shared-secret",
            MemoryKind::Preference,
            shared,
            SECRET,
            1.0,
            MemoryProvenance::new("user-statement", "accepted-shared-event", 10).unwrap(),
        )
        .unwrap();
        if expiring {
            write = write.with_retention_deadline(20).unwrap();
        }
        bound.write(write).await.unwrap();
        let mut definition = definition();
        definition.memory.enabled = true;
        if with_tool {
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
                        AuthoritativePolicyState::active(owner, &definition.id, 1, 1, None)
                            .unwrap(),
                    ),
                )
                .await
                .unwrap();
        }
        let run = Uuid::new_v4();
        let binding = DurableRunBinding::new(
            owner,
            identity,
            Uuid::new_v4(),
            run,
            ApplicationScope::new(owner, Uuid::new_v4()).unwrap(),
        )
        .unwrap();
        let budget = ExecutionScope::new(
            "memory-revalidation".into(),
            run.to_string(),
            ExecutionLimits::default(),
            clock.now_ms(),
        )
        .unwrap();
        let budget_port = store.register_budget(owner, budget.clone()).await.unwrap();
        let control = DurableControl::new();
        let execution = control
            .execution_context(&budget, run, Arc::new(budget_port), clock.clone())
            .unwrap();
        Self {
            _temp: temp,
            clock,
            store,
            memory,
            bound,
            authority,
            binding,
            definition,
            control,
            execution,
        }
    }
    fn harness<M: ModelAdapter>(
        &self,
        model: Arc<M>,
    ) -> DurableHarness<
        SqliteExecutionStore,
        M,
        Definitions,
        Policy,
        CapabilityRegistry,
        ManualExecutionClock,
        Live,
    > {
        let mut catalog = ManifestCatalog::default();
        catalog.register_manifest(manifest()).unwrap();
        let mut registry = CapabilityRegistry::new(catalog);
        registry
            .register_executor(Arc::new(ForgetEffect {
                manifest: manifest(),
                memory: self.bound.clone(),
            }))
            .unwrap();
        DurableHarness::new(
            self.store.clone(),
            model,
            Arc::new(Definitions(self.definition.clone())),
            Arc::new(Policy {
                binding: self.binding.clone(),
                clock: self.clock.clone(),
            }),
            Arc::new(registry),
            self.clock.clone(),
            Arc::new(Live),
        )
    }
    async fn persistent(&self, authority: MemoryAuthority, ready: bool) -> PersistentAgentMemory {
        PersistentAgentMemory::new(
            authority.clone(),
            Arc::new(self.memory.bind(authority).await.unwrap()),
            Arc::new(Ready(ready)),
        )
    }
    async fn close(self) {
        // Some outage scenarios deliberately closed memory before dispatch.
        let _ = self.memory.shutdown().await;
        self.store.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn revoked_shared_scope_must_not_reach_model_from_admission_snapshot() {
    let fixture = Fixture::new(false, false).await;
    let calls = Arc::new(Mutex::new(vec![]));
    let harness = fixture.harness(Arc::new(Model(calls.clone())));
    let broad = fixture.persistent(fixture.authority.clone(), true).await;
    assert_eq!(
        fixture
            .bound
            .query(
                &fixture.authority.access_context().unwrap(),
                MemoryQuery::new(
                    "lavender tea",
                    vec![MemoryScope::workspace("shared-room").unwrap()],
                    16
                )
                .unwrap()
            )
            .await
            .unwrap()
            .hits()
            .len(),
        1
    );
    harness
        .admit_persistent(
            &fixture.binding,
            &fixture.definition,
            INPUT,
            &fixture.execution,
            &broad,
        )
        .await
        .unwrap();
    assert!(
        calls.lock().unwrap().is_empty(),
        "revoke before any model dispatch"
    );
    let private = MemoryScope::agent("assistant").unwrap();
    let narrow_authority = MemoryAuthority::trusted(
        fixture.binding.individual.clone(),
        vec![private.clone()],
        vec![private],
    )
    .unwrap();
    let narrow_port = Arc::new(fixture.memory.bind(narrow_authority.clone()).await.unwrap());
    assert_eq!(
        narrow_port
            .query(
                &narrow_authority.access_context().unwrap(),
                MemoryQuery::new(
                    "lavender tea",
                    vec![MemoryScope::workspace("shared-room").unwrap()],
                    16
                )
                .unwrap()
            )
            .await
            .unwrap_err()
            .code,
        MemoryPortErrorCode::UnauthorizedScope
    );
    let narrow = PersistentAgentMemory::new(narrow_authority, narrow_port, Arc::new(Ready(true)));
    let result = harness
        .run_persistent(
            &fixture.binding,
            &fixture.control,
            &fixture.execution,
            &narrow,
        )
        .await;
    let emitted_secret = calls
        .lock()
        .unwrap()
        .iter()
        .any(|request| request.system.contains(SECRET));
    fixture.close().await;
    assert!(
        !emitted_secret,
        "revoked shared memory reached the model from the admission snapshot"
    );
    assert_eq!(
        result.unwrap_err().code(),
        EngineErrorCode::MemoryUnavailable
    );
}

#[tokio::test]
async fn forgotten_corrected_and_expired_captured_records_block_first_dispatch() {
    for mutation in ["forget", "correct", "expire"] {
        let fixture = Fixture::new(mutation == "expire", false).await;
        let calls = Arc::new(Mutex::new(vec![]));
        let harness = fixture.harness(Arc::new(Model(calls.clone())));
        let persistent = fixture.persistent(fixture.authority.clone(), true).await;
        harness
            .admit_persistent(
                &fixture.binding,
                &fixture.definition,
                INPUT,
                &fixture.execution,
                &persistent,
            )
            .await
            .unwrap();
        match mutation {
            "forget" => {
                fixture
                    .bound
                    .revise(MemoryRevision::forget("shared-secret", 1, "withdrawn").unwrap())
                    .await
                    .unwrap();
            }
            "correct" => {
                fixture
                    .bound
                    .write(
                        MemoryWrite::new(
                            "replacement",
                            MemoryKind::Preference,
                            MemoryScope::workspace("shared-room").unwrap(),
                            "now likes coffee",
                            1.0,
                            MemoryProvenance::new("user", "correction", 10).unwrap(),
                        )
                        .unwrap(),
                    )
                    .await
                    .unwrap();
                fixture
                    .bound
                    .revise(
                        MemoryRevision::correction(
                            "replacement",
                            1,
                            "shared-secret",
                            "corrected preference",
                        )
                        .unwrap(),
                    )
                    .await
                    .unwrap();
            }
            _ => {
                fixture.clock.advance_ms(11).unwrap();
            }
        }
        let result = harness
            .run_persistent(
                &fixture.binding,
                &fixture.control,
                &fixture.execution,
                &persistent,
            )
            .await;
        fixture.close().await;
        assert_eq!(
            result.unwrap_err().code(),
            EngineErrorCode::MemoryUnavailable,
            "{mutation}"
        );
        assert!(
            calls.lock().unwrap().is_empty(),
            "{mutation} must prevent the first provider call"
        );
    }
}

#[tokio::test]
async fn changed_revision_of_still_active_memory_blocks_dispatch() {
    let fixture = Fixture::new(false, false).await;
    let calls = Arc::new(Mutex::new(vec![]));
    let harness = fixture.harness(Arc::new(Model(calls.clone())));
    let persistent = fixture.persistent(fixture.authority.clone(), true).await;
    harness
        .admit_persistent(
            &fixture.binding,
            &fixture.definition,
            INPUT,
            &fixture.execution,
            &persistent,
        )
        .await
        .unwrap();
    fixture
        .bound
        .write(
            MemoryWrite::new(
                "older",
                MemoryKind::Fact,
                MemoryScope::workspace("shared-room").unwrap(),
                "obsolete",
                1.0,
                MemoryProvenance::new("user", "older", 1).unwrap(),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    fixture
        .bound
        .revise(MemoryRevision::correction("shared-secret", 1, "older", "new lineage").unwrap())
        .await
        .unwrap();
    assert_eq!(
        fixture
            .bound
            .get_active("shared-secret")
            .await
            .unwrap()
            .revision(),
        2
    );
    let result = harness
        .run_persistent(
            &fixture.binding,
            &fixture.control,
            &fixture.execution,
            &persistent,
        )
        .await;
    fixture.close().await;
    assert_eq!(
        result.unwrap_err().code(),
        EngineErrorCode::MemoryUnavailable
    );
    assert!(calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn resume_revalidates_captured_memory_before_dispatch() {
    let fixture = Fixture::new(false, false).await;
    let calls = Arc::new(Mutex::new(vec![]));
    let harness = fixture.harness(Arc::new(Model(calls.clone())));
    let persistent = fixture.persistent(fixture.authority.clone(), true).await;
    harness
        .admit_persistent(
            &fixture.binding,
            &fixture.definition,
            INPUT,
            &fixture.execution,
            &persistent,
        )
        .await
        .unwrap();
    fixture.control.pause();
    assert_eq!(
        harness
            .run_persistent(
                &fixture.binding,
                &fixture.control,
                &fixture.execution,
                &persistent
            )
            .await
            .unwrap(),
        EngineRunOutcome::PausedByRequest
    );
    fixture
        .bound
        .revise(MemoryRevision::forget("shared-secret", 1, "withdrawn during pause").unwrap())
        .await
        .unwrap();
    fixture.control.resume();
    let result = harness
        .run_persistent(
            &fixture.binding,
            &fixture.control,
            &fixture.execution,
            &persistent,
        )
        .await;
    fixture.close().await;
    assert_eq!(
        result.unwrap_err().code(),
        EngineErrorCode::MemoryUnavailable
    );
    assert!(calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn readiness_and_storage_failure_are_explicit_before_new_dispatch() {
    for unavailable in ["readiness", "storage"] {
        let fixture = Fixture::new(false, false).await;
        let calls = Arc::new(Mutex::new(vec![]));
        let harness = fixture.harness(Arc::new(Model(calls.clone())));
        let persistent = fixture.persistent(fixture.authority.clone(), true).await;
        harness
            .admit_persistent(
                &fixture.binding,
                &fixture.definition,
                INPUT,
                &fixture.execution,
                &persistent,
            )
            .await
            .unwrap();
        let current = if unavailable == "readiness" {
            fixture.persistent(fixture.authority.clone(), false).await
        } else {
            fixture.memory.shutdown().await.unwrap();
            persistent
        };
        let result = harness
            .run_persistent(
                &fixture.binding,
                &fixture.control,
                &fixture.execution,
                &current,
            )
            .await;
        fixture.close().await;
        assert_eq!(
            result.unwrap_err().code(),
            EngineErrorCode::MemoryUnavailable,
            "{unavailable}"
        );
        assert!(calls.lock().unwrap().is_empty(), "{unavailable}");
    }
}

#[tokio::test]
async fn completed_exact_replay_needs_no_new_memory_read_or_model_call() {
    let fixture = Fixture::new(false, false).await;
    let calls = Arc::new(Mutex::new(vec![]));
    let harness = fixture.harness(Arc::new(Model(calls.clone())));
    let persistent = fixture.persistent(fixture.authority.clone(), true).await;
    harness
        .admit_persistent(
            &fixture.binding,
            &fixture.definition,
            INPUT,
            &fixture.execution,
            &persistent,
        )
        .await
        .unwrap();
    let completed = harness
        .run_persistent(
            &fixture.binding,
            &fixture.control,
            &fixture.execution,
            &persistent,
        )
        .await
        .unwrap();
    fixture.memory.shutdown().await.unwrap();
    harness
        .admit_persistent(
            &fixture.binding,
            &fixture.definition,
            INPUT,
            &fixture.execution,
            &persistent,
        )
        .await
        .unwrap();
    assert_eq!(
        harness
            .run_persistent(
                &fixture.binding,
                &fixture.control,
                &fixture.execution,
                &persistent
            )
            .await
            .unwrap(),
        completed
    );
    assert_eq!(calls.lock().unwrap().len(), 1);
    let mut new_binding = fixture.binding.clone();
    new_binding.run = Uuid::new_v4();
    assert_eq!(
        harness
            .admit_persistent(
                &new_binding,
                &fixture.definition,
                INPUT,
                &fixture.execution,
                &persistent
            )
            .await
            .unwrap_err()
            .code(),
        EngineErrorCode::MemoryUnavailable
    );
    fixture.close().await;
}

fn manifest() -> CapabilityManifest {
    CapabilityManifest::new(CapabilityManifestInput {
        id: "fixture.forget".into(),
        version: 1,
        kind: CapabilityKind::Custom,
        label: "Forget".into(),
        description: "host-authorized memory lifecycle change".into(),
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
                self.binding.owner.to_string(),
                self.binding.individual.individual_id().as_str(),
                request.definition().id.clone(),
                1,
                self.binding.scope.policy_workspace_id(),
                CapabilityReferenceId::new(self.binding.run),
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
struct ForgetEffect {
    manifest: CapabilityManifest,
    memory: Arc<BoundMemoryStore>,
}
#[async_trait]
impl CapabilityExecutor for ForgetEffect {
    fn manifest(&self) -> &CapabilityManifest {
        &self.manifest
    }
    async fn execute(
        &self,
        _: CapabilityExecutionContext,
    ) -> Result<CapabilityResult, CapabilityError> {
        self.memory
            .revise(
                MemoryRevision::forget("shared-secret", 1, "host withdrawal between calls")
                    .unwrap(),
            )
            .await
            .unwrap();
        Ok(CapabilityResult::new(serde_json::json!({"forgotten":true})))
    }
    async fn reconcile(
        &self,
        _: CapabilityExecutionContext,
    ) -> Result<ReconcileOutcome, CapabilityError> {
        Ok(ReconcileOutcome::RecoveryRequired)
    }
}
struct TwoStepModel(Arc<Mutex<Vec<ModelGenerateRequest>>>);
#[async_trait]
impl ModelAdapter for TwoStepModel {
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
        self.0.lock().unwrap().push(request.clone());
        let first = self.0.lock().unwrap().len() == 1;
        Ok(ModelGenerateResponse {
            content: Content::default(),
            tool_calls: first.then(|| {
                vec![ToolCall {
                    id: "withdraw".into(),
                    name: "fixture.forget".into(),
                    args: Default::default(),
                }]
            }),
            usage: TokenUsage::default(),
            stop_reason: if first {
                ModelStopReason::ToolCall
            } else {
                ModelStopReason::End
            },
        })
    }
}
#[tokio::test]
async fn second_model_step_revalidates_after_a_committed_memory_change() {
    let fixture = Fixture::new(false, true).await;
    let calls = Arc::new(Mutex::new(vec![]));
    let harness = fixture.harness(Arc::new(TwoStepModel(calls.clone())));
    let persistent = fixture.persistent(fixture.authority.clone(), true).await;
    harness
        .admit_persistent(
            &fixture.binding,
            &fixture.definition,
            INPUT,
            &fixture.execution,
            &persistent,
        )
        .await
        .unwrap();
    let result = harness
        .run_persistent(
            &fixture.binding,
            &fixture.control,
            &fixture.execution,
            &persistent,
        )
        .await;
    assert!(
        fixture
            .bound
            .get("shared-secret")
            .await
            .unwrap()
            .forgotten_at_ms()
            .is_some(),
        "tool must have committed withdrawal between provider steps"
    );
    fixture.close().await;
    assert_eq!(
        result.unwrap_err().code(),
        EngineErrorCode::MemoryUnavailable
    );
    assert_eq!(
        calls.lock().unwrap().len(),
        1,
        "the second provider call must be blocked"
    );
}

struct RetryInsideAdapter {
    model: Model,
    memory: Arc<BoundMemoryStore>,
}
#[async_trait]
impl ModelAdapter for RetryInsideAdapter {
    fn provider(&self) -> &str {
        "retry-fixture"
    }
    async fn generate(
        &self,
        _: &AgentConfig,
        _: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        panic!("controlled path required")
    }
    async fn stream_controlled(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
        _: &dyn ModelStreamSink,
        context: &ModelCallContext,
    ) -> Result<(), String> {
        self.model
            .generate_controlled(config, request, context)
            .await?;
        self.memory
            .revise(
                MemoryRevision::forget("shared-secret", 1, "withdraw before internal retry")
                    .unwrap(),
            )
            .await
            .unwrap();
        // This new physical attempt stays inside the same adapter invocation.
        self.model
            .generate_controlled(config, request, &context.clone())
            .await?;
        Err("revoked retry unexpectedly dispatched".into())
    }
}
#[tokio::test]
async fn canonical_memory_revalidation_guards_each_internal_physical_retry() {
    let fixture = Fixture::new(false, false).await;
    let calls = Arc::new(Mutex::new(vec![]));
    let harness = fixture.harness(Arc::new(RetryInsideAdapter {
        model: Model(calls.clone()),
        memory: fixture.bound.clone(),
    }));
    let persistent = fixture.persistent(fixture.authority.clone(), true).await;
    harness
        .admit_persistent(
            &fixture.binding,
            &fixture.definition,
            INPUT,
            &fixture.execution,
            &persistent,
        )
        .await
        .unwrap();
    let result = harness
        .run_persistent(
            &fixture.binding,
            &fixture.control,
            &fixture.execution,
            &persistent,
        )
        .await;
    fixture.close().await;
    assert_eq!(
        result.unwrap_err().code(),
        EngineErrorCode::MemoryUnavailable
    );
    assert_eq!(
        calls.lock().unwrap().len(),
        1,
        "canonical forgetting must block the internal physical retry"
    );
}
