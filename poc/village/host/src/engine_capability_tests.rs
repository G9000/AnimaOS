//! Constructor-injected host capability profile, excluded from executable builds.
use crate::{
    engine_bridge::{DurableVillage, TestCapabilityProfile},
    Service, Store,
};
use anima_execution_store_sqlite::SqliteExecutionStore;
use anima_harness::{anima_core::*, durable::DurableRunBinding};
use anima_memory_store_sqlite::SqliteMemoryStore;
use async_trait::async_trait;
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

fn manifest(id: &str) -> CapabilityManifest {
    CapabilityManifest::new(CapabilityManifestInput {
        id: id.into(),
        version: 1,
        kind: if id.ends_with("filesystem") {
            CapabilityKind::Workspace
        } else {
            CapabilityKind::Knowledge
        },
        label: id.into(),
        description: "bounded host fixture".into(),
        input_schema: serde_json::json!({"type":"object","additionalProperties":false}),
        output_schema: serde_json::json!({"type":"object"}),
        side_effects: id.ends_with("filesystem"),
        risk_level: if id.ends_with("filesystem") {
            RiskLevel::High
        } else {
            RiskLevel::Low
        },
        host_permissions: vec![],
        secret_references: vec![],
        environment_requirements: vec![],
        timeout_ms: 1000,
        cancellation_supported: true,
        max_retries: 0,
        idempotent: true,
        recovery_mode: RecoveryMode::InherentlyIdempotent,
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
struct ReadClock {
    manifest: CapabilityManifest,
    calls: Arc<AtomicUsize>,
}
#[async_trait]
impl CapabilityExecutor for ReadClock {
    fn manifest(&self) -> &CapabilityManifest {
        &self.manifest
    }
    async fn execute(
        &self,
        _: CapabilityExecutionContext,
    ) -> std::result::Result<CapabilityResult, CapabilityError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(CapabilityResult::new(serde_json::json!({"villageTime":42})))
    }
    async fn reconcile(
        &self,
        _: CapabilityExecutionContext,
    ) -> std::result::Result<ReconcileOutcome, CapabilityError> {
        Ok(ReconcileOutcome::RecoveryRequired)
    }
}
struct ToolModel {
    name: String,
    calls: Arc<AtomicUsize>,
}
#[async_trait]
impl ModelAdapter for ToolModel {
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
        r: &ModelGenerateRequest,
    ) -> std::result::Result<ModelGenerateResponse, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let done = r.messages.iter().any(|m| m.role == MessageRole::Tool);
        Ok(ModelGenerateResponse {
            content: Content {
                text: if done {
                    r#"{"text":"夕食の準備をしているよ。","translation":"We are preparing supper.","reading":"Yūshoku no junbi o shite iru yo.","expression":"warm"}"#.into()
                } else {
                    String::new()
                },
                ..Default::default()
            },
            tool_calls: (!done).then(|| {
                vec![ToolCall {
                    id: "clock-observation".into(),
                    name: self.name.clone(),
                    args: Default::default(),
                }]
            }),
            usage: TokenUsage {
                prompt_tokens: 10,
                completion_tokens: 20,
                total_tokens: 30,
                ..Default::default()
            },
            stop_reason: if done {
                ModelStopReason::End
            } else {
                ModelStopReason::ToolCall
            },
        })
    }
}
struct HostPolicy {
    binding: DurableRunBinding,
    wrong_actor: bool,
    wrong_scope: bool,
    observed: Arc<Mutex<Vec<PolicyContext>>>,
}
#[async_trait]
impl CurrentPolicyResolver for HostPolicy {
    async fn resolve(
        &self,
        request: EnginePolicyRequest,
    ) -> std::result::Result<CurrentPolicyResolution, EngineError> {
        let context = PolicyContext::new(
            self.binding.owner.to_string(),
            if self.wrong_actor {
                "another-resident"
            } else {
                self.binding.individual.individual_id().as_str()
            },
            request.definition().id.clone(),
            request.definition().version,
            if self.wrong_scope {
                "another-world".into()
            } else {
                self.binding.scope.policy_workspace_id()
            },
            CapabilityReferenceId::new(self.binding.run),
            request.manifest(),
            request.invocation(),
            1,
            PolicyRestrictions {
                deny: request.manifest().id.ends_with("filesystem"),
                ..Default::default()
            },
            0,
        )
        .unwrap();
        self.observed.lock().unwrap().push(context.clone());
        Ok(CurrentPolicyResolution::new(context, vec![], None))
    }
}
async fn scenario(tool: &str, wrong_actor: bool, wrong_scope: bool) {
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;
    let temp = tempfile::tempdir().unwrap();
    let clock = Arc::new(ManualExecutionClock::default());
    let clock_manifest = manifest("fixture.clock");
    let file_manifest = manifest("fixture.filesystem");
    let inventories = [&clock_manifest, &file_manifest]
        .into_iter()
        .map(|m| PersistenceCapabilitySecretInventory::new(m, vec![]).unwrap())
        .collect();
    let execution = Arc::new(
        SqliteExecutionStore::open(
            temp.path().join("execution.sqlite"),
            PersistenceProtection::allow_model_payloads(
                PersistenceSnapshotSealKey::new([13; 32]).unwrap(),
                inventories,
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
    let clock_calls = Arc::new(AtomicUsize::new(0));
    let file_calls = Arc::new(AtomicUsize::new(0));
    let model_calls = Arc::new(AtomicUsize::new(0));
    let observed = Arc::new(Mutex::new(vec![]));
    let mut catalog = ManifestCatalog::default();
    catalog.register_manifest(clock_manifest.clone()).unwrap();
    catalog.register_manifest(file_manifest.clone()).unwrap();
    let mut registry = CapabilityRegistry::new(catalog);
    registry
        .register_executor(Arc::new(ReadClock {
            manifest: clock_manifest.clone(),
            calls: clock_calls.clone(),
        }))
        .unwrap();
    registry
        .register_executor(Arc::new(ReadClock {
            manifest: file_manifest.clone(),
            calls: file_calls.clone(),
        }))
        .unwrap();
    let capabilities = [clock_manifest, file_manifest]
        .iter()
        .map(|m| ResolvedCapability {
            capability_id: m.id.clone(),
            manifest_version: 1,
            schema_digest: m.schema_digest().into(),
            override_config: None,
            approval_policy_revision: 1,
        })
        .collect();
    let capture = observed.clone();
    let profile = TestCapabilityProfile {
        registry: Arc::new(registry),
        capabilities,
        policy: Arc::new(move |binding| {
            Arc::new(HostPolicy {
                binding: binding.clone(),
                wrong_actor,
                wrong_scope,
                observed: capture.clone(),
            })
        }),
    };
    let bridge = Arc::new(
        DurableVillage::new(
            execution.clone(),
            memory,
            clock,
            Arc::new(ToolModel {
                name: tool.into(),
                calls: model_calls.clone(),
            }),
            "fixture".into(),
            "scripted".into(),
        )
        .with_test_capabilities(profile),
    );
    let service = Service::new(
        Store::open(&temp.path().join("game.sqlite")).unwrap(),
        None,
        Duration::from_secs(30),
    )
    .with_durable(bridge)
    .await
    .unwrap();
    let save = service.create(anima_village::Language::Ja).unwrap();
    let app = crate::http::router(service.clone());
    let request = || {
        Request::builder()
            .method("POST")
            .uri(format!("/api/village/saves/{}/visit", save.save_id))
            .header("content-type", "application/json")
            .body(Body::from(r#"{"requestId":"observation"}"#))
            .unwrap()
    };
    let response = app.clone().oneshot(request()).await.unwrap();
    if tool == "fixture.clock" && !wrong_actor && !wrong_scope {
        assert_eq!(
            response.status(),
            200,
            "the read-only host capability must run through the real game request boundary"
        );
        assert_eq!(clock_calls.load(Ordering::SeqCst), 1);
        assert_eq!(model_calls.load(Ordering::SeqCst), 2);
        assert_eq!(app.oneshot(request()).await.unwrap().status(), 200);
        assert_eq!(clock_calls.load(Ordering::SeqCst), 1);
        assert_eq!(model_calls.load(Ordering::SeqCst), 2);
        let ids = service.store.resident_identities(&save.save_id).unwrap();
        {
            let contexts = observed.lock().unwrap();
            assert!(!contexts.is_empty());
            assert!(contexts
                .iter()
                .all(|c| c.actor_id == ids.tavi.individual_id().as_str()
                    && c.workspace_id == format!("application:v1:{}:{}", ids.owner, ids.world)));
        }
        let payload: String = service
            .store
            .db()
            .unwrap()
            .query_row(
                "SELECT payload FROM village_engine_requests WHERE save_id=?1",
                [&save.save_id],
                |r| r.get(0),
            )
            .unwrap();
        let binding: serde_json::Value = serde_json::from_str(&payload).unwrap();
        let scope = execution
            .bind_budget(ids.owner, binding["allowance"]["id"].as_str().unwrap())
            .await
            .unwrap()
            .snapshot()
            .await
            .unwrap();
        assert_eq!(scope.consumption().unwrap().model_attempts, 2);
        assert_eq!(scope.consumption().unwrap().tool_calls, 1);
    } else {
        assert_eq!(response.status(), 503);
        assert_eq!(clock_calls.load(Ordering::SeqCst), 0);
    }
    assert_eq!(
        file_calls.load(Ordering::SeqCst),
        0,
        "a denied filesystem proposal must never enter its executor"
    );
}
#[tokio::test]
async fn read_only_clock_capability_runs_through_http_without_filesystem_authority() {
    scenario("fixture.clock", false, false).await;
}
#[tokio::test]
async fn wrong_actor_scope_and_filesystem_proposals_never_enter_executors() {
    scenario("fixture.clock", true, false).await;
    scenario("fixture.clock", false, true).await;
    scenario("fixture.filesystem", false, false).await;
}
