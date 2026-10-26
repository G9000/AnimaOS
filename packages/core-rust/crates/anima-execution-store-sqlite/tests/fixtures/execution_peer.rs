#![allow(dead_code)]
use anima_core::*;
use anima_execution_store_sqlite::SqliteExecutionStore;
use async_trait::async_trait;
use std::{
    io::{BufRead, Write},
    path::Path,
    sync::Arc,
};
use uuid::Uuid;

pub const OWNER: Uuid = Uuid::from_u128(101);
pub const SESSION: Uuid = Uuid::from_u128(102);
pub const RUN: Uuid = Uuid::from_u128(103);

fn barrier(point: &str) {
    if std::env::var("ANIMA_EXECUTION_TEST_POINT").ok().as_deref() == Some(point) {
        println!("READY:{point}");
        std::io::stdout().flush().unwrap();
        let mut line = String::new();
        // Parent kills us here; no timing assumptions decide where death occurs.
        std::io::stdin().lock().read_line(&mut line).unwrap();
        panic!("fixture barrier must be terminated by parent");
    }
}

pub fn manifest(manual: bool) -> CapabilityManifest {
    CapabilityManifest::new(CapabilityManifestInput {
        id: "fixture.record".into(),
        version: 1,
        kind: CapabilityKind::Custom,
        label: "Record".into(),
        description: "record a keyed fixture effect".into(),
        input_schema: serde_json::json!({"type":"object"}),
        output_schema: serde_json::json!({"type":"object"}),
        side_effects: true,
        risk_level: RiskLevel::Low,
        host_permissions: vec![],
        secret_references: vec![],
        environment_requirements: vec![],
        timeout_ms: 1_000,
        cancellation_supported: true,
        max_retries: 1,
        idempotent: !manual,
        recovery_mode: if manual {
            RecoveryMode::NonRetryable
        } else {
            RecoveryMode::KeyedIdempotent
        },
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

pub fn protection(manual: bool) -> PersistenceProtection {
    PersistenceProtection::allow_model_payloads(
        PersistenceSnapshotSealKey::new([53; 32]).unwrap(),
        vec![PersistenceCapabilitySecretInventory::new(&manifest(manual), vec![]).unwrap()],
    )
    .unwrap()
}

fn definition(manifest: &CapabilityManifest) -> AgentDefinition {
    AgentDefinition {
        schema_version: 1,
        id: "fixture-agent".into(),
        version: 1,
        name: "fixture-agent".into(),
        display_name: "Fixture".into(),
        description: "fixture".into(),
        persona: "gentle".into(),
        system: "fixture instructions".into(),
        model: ModelPolicy {
            provider: "fixture".into(),
            model: "fixture".into(),
            credential_reference: None,
            temperature: None,
        },
        source_profile: ProfileRef {
            profile_id: "fixture".into(),
            profile_version: 1,
        },
        resolved_capabilities: vec![ResolvedCapability {
            capability_id: manifest.id.clone(),
            manifest_version: 1,
            schema_digest: manifest.schema_digest().into(),
            override_config: None,
            approval_policy_revision: 1,
        }],
        memory: MemoryPolicy {
            enabled: false,
            namespace: "fixture".into(),
            retention_days: None,
        },
        approval_policy_id: "fixture".into(),
        approval_policy_revision: 1,
        approval_restrictions: vec![],
        limits: RuntimeLimits {
            max_turns: 3,
            timeout_ms: 30_000,
            max_concurrent_tasks: 1,
        },
        lifecycle: LifecyclePolicy {
            auto_start: false,
            restart_on_failure: false,
            max_restarts: 0,
            allows_concurrent_sessions: false,
        },
        host_requirements: vec![],
    }
}

struct Definitions(AgentDefinition);
#[async_trait]
impl DefinitionResolver for Definitions {
    async fn resolve(
        &self,
        owner: Uuid,
        pin: &DefinitionPin,
    ) -> Result<AgentDefinition, EngineError> {
        assert_eq!(owner, OWNER);
        assert_eq!(pin, &DefinitionPin::from_definition(&self.0).unwrap());
        Ok(self.0.clone())
    }
}

struct Model;
#[async_trait]
impl ModelAdapter for Model {
    fn provider(&self) -> &str {
        "fixture"
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
        assert_eq!(
            request
                .messages
                .iter()
                .filter(|m| m.role == MessageRole::User)
                .count(),
            1
        );
        let done = request.messages.iter().any(|m| m.role == MessageRole::Tool);
        if done {
            let assistant = request
                .messages
                .iter()
                .find(|m| m.role == MessageRole::Assistant)
                .expect("retained tool request");
            let tool = request
                .messages
                .iter()
                .find(|m| m.role == MessageRole::Tool)
                .unwrap();
            assert!(serde_json::to_string(&assistant.content)
                .unwrap()
                .contains("record-once"));
            assert!(serde_json::to_string(&tool.content)
                .unwrap()
                .contains("record-once"));
            assert!(tool.content.text.contains("true"));
        }
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
            usage: TokenUsage {
                prompt_tokens: 3,
                completion_tokens: 2,
                total_tokens: 5,
                ..TokenUsage::default()
            },
            stop_reason: if done {
                ModelStopReason::End
            } else {
                ModelStopReason::ToolCall
            },
        }))
        .await
    }
}

struct Policy;
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
                "fixture:opaque-scope",
                CapabilityReferenceId::new(RUN),
                request.manifest(),
                request.invocation(),
                1,
                PolicyRestrictions::default(),
                1_000_000,
            )
            .unwrap(),
            vec![],
            None,
        ))
    }
}

struct Effect {
    manifest: CapabilityManifest,
    path: std::path::PathBuf,
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
        let conn = rusqlite::Connection::open(&self.path).unwrap();
        conn.execute_batch("CREATE TABLE IF NOT EXISTS effects (invocation TEXT PRIMARY KEY, executions INTEGER NOT NULL);").unwrap();
        conn.execute("INSERT INTO effects VALUES (?1,1) ON CONFLICT(invocation) DO UPDATE SET executions=executions+1",[context.invocation().id().to_string()]).unwrap();
        Ok(CapabilityResult::new(serde_json::json!({"ok":true})))
    }
    async fn reconcile(
        &self,
        context: CapabilityExecutionContext,
    ) -> Result<ReconcileOutcome, CapabilityError> {
        let conn = rusqlite::Connection::open(&self.path).unwrap();
        conn.execute_batch("CREATE TABLE IF NOT EXISTS effects (invocation TEXT PRIMARY KEY, executions INTEGER NOT NULL);").unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM effects WHERE invocation=?1",
                [context.invocation().id().to_string()],
                |row| row.get(0),
            )
            .unwrap();
        Ok(if count == 1 {
            ReconcileOutcome::Completed(CapabilityResult::new(serde_json::json!({"ok":true})))
        } else {
            ReconcileOutcome::AuthoritativeAbsence
        })
    }
}

struct Live;
#[async_trait]
impl EngineLiveEventSink for Live {
    async fn emit(&self, event: EngineLiveEvent) -> Result<(), EngineError> {
        if let EngineLiveEvent::Semantic { kind, .. } = event {
            match kind {
                RuntimeEventKind::ModelCompleted => barrier("model-checkpoint"),
                RuntimeEventKind::CapabilityCompleted => barrier("result-committed"),
                RuntimeEventKind::RunCompleted => barrier("terminal-committed"),
                _ => {}
            }
        }
        Ok(())
    }
}
struct Crash;
impl EngineCrashInjector for Crash {
    fn should_crash(&self, point: EngineCrashPoint) -> bool {
        match point {
            EngineCrashPoint::AfterDispatchCommitBeforeExecutor => {
                barrier("dispatch-before-effect")
            }
            EngineCrashPoint::AfterExecutorBeforeResultCommit => barrier("effect-before-result"),
        }
        false
    }
}
struct Continue;
impl EngineControlSignal for Continue {
    fn at_boundary(&self) -> EngineBoundaryAction {
        EngineBoundaryAction::Continue
    }
}

pub async fn execute(path: &Path, manual: bool, reopening: bool) -> EngineRunOutcome {
    let clock = Arc::new(ManualExecutionClock::new(if reopening {
        1_100_000
    } else {
        1_000_000
    }));
    let store = Arc::new(
        SqliteExecutionStore::open(
            path.join("execution.sqlite"),
            protection(manual),
            clock.clone(),
        )
        .await
        .unwrap(),
    );
    let manifest = manifest(manual);
    let definition = definition(&manifest);
    if store.load_run(OWNER, RUN).await.unwrap().is_none() {
        let session =
            Session::new_for_definition(SESSION, &definition, SessionConcurrencyPolicy::Serial)
                .unwrap();
        let run = Run::queued(RUN, SESSION, &definition.id, 1).unwrap();
        store
            .create_run(
                OWNER,
                CreateRun::new_for_owner(OWNER, session, run, 0, SessionConcurrencyPolicy::Serial)
                    .with_initial_input(
                        DurableRunInput::text(OWNER, SESSION, RUN, "remember this conversation")
                            .unwrap(),
                    )
                    .unwrap(),
            )
            .await
            .unwrap();
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
    let mut catalog = ManifestCatalog::default();
    catalog.register_manifest(manifest.clone()).unwrap();
    let mut registry = CapabilityRegistry::new(catalog);
    registry
        .register_executor(Arc::new(Effect {
            manifest,
            path: path.join("effects.sqlite"),
        }))
        .unwrap();
    let engine = DurableAgentEngine::new(
        store.clone(),
        Arc::new(Model),
        Arc::new(Definitions(definition)),
        Arc::new(Policy),
        Arc::new(registry),
        clock,
        Arc::new(Live),
        Arc::new(Crash),
        DurableEngineConfig::default(),
    )
    .unwrap();
    let outcome = engine.run(OWNER, RUN, &Continue).await.unwrap();
    store.shutdown().await.unwrap();
    outcome
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("temporary fixture directory");
    futures::executor::block_on(execute(Path::new(&path), false, false));
}
