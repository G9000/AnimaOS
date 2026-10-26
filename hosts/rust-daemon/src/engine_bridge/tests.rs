use super::*;
use anima_core::*;
use anima_execution_store_sqlite::SqliteExecutionStore;
use anima_memory_store_sqlite::SqliteMemoryStore;
use async_trait::async_trait;
use axum::{body::Body, http::Request, Router};
use http_body_util::BodyExt;
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;
use tower::ServiceExt;
use uuid::Uuid;

struct Model(Arc<Mutex<Vec<ModelGenerateRequest>>>);
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
        self.0.lock().unwrap().push(request.clone());
        Ok(ModelGenerateResponse {
            content: Content {
                text: "Remembered.".into(),
                ..Content::default()
            },
            tool_calls: None,
            usage: TokenUsage {
                prompt_tokens: 10,
                completion_tokens: 2,
                total_tokens: 12,
                ..TokenUsage::default()
            },
            stop_reason: ModelStopReason::End,
        })
    }
}
fn protection() -> PersistenceProtection {
    PersistenceProtection::allow_model_payloads(
        PersistenceSnapshotSealKey::new([87; 32]).unwrap(),
        vec![],
    )
    .unwrap()
}
fn profile() -> AgentConfig {
    serde_json::from_value(serde_json::json!({"name":"Assistant","model":"fixture","provider":"fixture","system":"Be kind.","bio":"Remember personal preferences.","knowledge":["The owner likes quiet mornings."]})).unwrap()
}
async fn request(
    app: Router,
    agent: &str,
    id: Uuid,
    session: Uuid,
    text: &str,
) -> (u16, serde_json::Value) {
    let req = crate::routes::owner_test_support::authenticate(
        Request::builder()
            .method("POST")
            .uri(format!("/api/agents/{agent}/run")),
    )
    .header("content-type", "application/json")
    .body(Body::from(
        serde_json::json!({"requestId":id,"roomId":session,"text":text}).to_string(),
    ))
    .unwrap();
    let response = app.oneshot(req).await.unwrap();
    let status = response.status().as_u16();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap())
}
fn app(bridge: Arc<DaemonEngineBridge>) -> Router {
    let mut state = crate::state::DaemonState::with_model_adapter(Arc::new(Model(Arc::new(
        Mutex::new(vec![]),
    ))));
    state.engine_bridge = Some(bridge);
    crate::app::app_with_state_for_owner_test(
        Arc::new(RwLock::new(state)),
        crate::DaemonConfig::default(),
    )
}

#[tokio::test]
async fn engine_bridge_http_preference_replay_and_private_recall_after_reopen() {
    let temp = tempfile::tempdir().unwrap();
    let owner = Uuid::new_v4();
    let clock = Arc::new(ManualExecutionClock::default());
    let calls = Arc::new(Mutex::new(vec![]));
    let execution = Arc::new(
        SqliteExecutionStore::open(
            temp.path().join("execution.sqlite"),
            protection(),
            clock.clone(),
        )
        .await
        .unwrap(),
    );
    let memory = SqliteMemoryStore::open(temp.path().join("memory.sqlite"), clock.clone())
        .await
        .unwrap();
    let bridge = Arc::new(
        DaemonEngineBridge::open(
            owner,
            execution.clone(),
            memory.clone(),
            temp.path().join("outbox.sqlite"),
            protection(),
            Arc::new(Model(calls.clone())),
            clock.clone(),
        )
        .await
        .unwrap(),
    );
    bridge
        .register_dialogue("assistant", profile(), 1)
        .await
        .unwrap();
    bridge
        .register_dialogue("other", profile(), 1)
        .await
        .unwrap();
    let router = app(bridge.clone());
    let run = Uuid::new_v4();
    let room = Uuid::new_v4();
    let (status, first) = request(
        router.clone(),
        "assistant",
        run,
        room,
        "I prefer jasmine tea.",
    )
    .await;
    assert_eq!(status, 200, "{first}");
    assert_eq!(first["engineFormat"], "durable-v1");
    assert!(calls.lock().unwrap()[0]
        .system
        .contains("The owner likes quiet mornings."));
    let (status, replayed) = request(
        router.clone(),
        "assistant",
        run,
        room,
        "I prefer jasmine tea.",
    )
    .await;
    assert_eq!(status, 200, "{replayed}");
    assert_eq!(first, replayed);
    assert_eq!(calls.lock().unwrap().len(), 1);
    assert_eq!(
        request(router.clone(), "assistant", run, room, "changed input")
            .await
            .0,
        409
    );
    let events = crate::routes::owner_test_support::authenticate(
        Request::builder().uri(format!("/api/agents/assistant/durable-runs/{run}/events")),
    )
    .body(Body::empty())
    .unwrap();
    let events = router.clone().oneshot(events).await.unwrap();
    assert_eq!(events.status().as_u16(), 200);
    assert!(events.headers()["content-type"]
        .to_str()
        .unwrap()
        .contains("text/event-stream"));
    let events = String::from_utf8(
        events
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap();
    assert!(events.contains("event: durable.run.event"));
    let forbidden = crate::routes::owner_test_support::authenticate(
        Request::builder().uri(format!("/api/agents/other/durable-runs/{run}/events")),
    )
    .body(Body::empty())
    .unwrap();
    assert_eq!(
        router
            .clone()
            .oneshot(forbidden)
            .await
            .unwrap()
            .status()
            .as_u16(),
        404
    );
    let resumed = crate::routes::owner_test_support::authenticate(Request::builder().uri(format!(
        "/api/agents/assistant/durable-runs/{run}/events?after={}",
        u64::MAX
    )))
    .body(Body::empty())
    .unwrap();
    let resumed = router.clone().oneshot(resumed).await.unwrap();
    assert!(resumed
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .is_empty());
    request(router, "other", Uuid::new_v4(), Uuid::new_v4(), "What tea?").await;
    assert!(!calls
        .lock()
        .unwrap()
        .last()
        .unwrap()
        .system
        .contains("jasmine"));
    bridge.shutdown().await.unwrap();
    execution.shutdown().await.unwrap();
    memory.shutdown().await.unwrap();
    let execution = Arc::new(
        SqliteExecutionStore::open(
            temp.path().join("execution.sqlite"),
            protection(),
            clock.clone(),
        )
        .await
        .unwrap(),
    );
    let memory = SqliteMemoryStore::open(temp.path().join("memory.sqlite"), clock.clone())
        .await
        .unwrap();
    let reopened = Arc::new(
        DaemonEngineBridge::open(
            owner,
            execution.clone(),
            memory.clone(),
            temp.path().join("outbox.sqlite"),
            protection(),
            Arc::new(Model(calls.clone())),
            clock,
        )
        .await
        .unwrap(),
    );
    reopened
        .register_dialogue("assistant", profile(), 1)
        .await
        .unwrap();
    let (status, value) = request(
        app(reopened.clone()),
        "assistant",
        Uuid::new_v4(),
        Uuid::new_v4(),
        "What tea do I prefer?",
    )
    .await;
    assert_eq!(status, 200, "{value}");
    assert!(calls
        .lock()
        .unwrap()
        .last()
        .unwrap()
        .system
        .contains("jasmine tea"));
    reopened.shutdown().await.unwrap();
    execution.shutdown().await.unwrap();
    memory.shutdown().await.unwrap();
}

struct PendingModel {
    entered: Arc<tokio::sync::Semaphore>,
    calls: Arc<std::sync::atomic::AtomicUsize>,
}
#[async_trait]
impl ModelAdapter for PendingModel {
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
        if self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) > 0 {
            return Err("second physical call must not dispatch".into());
        }
        self.entered.add_permits(1);
        futures::future::pending().await
    }
}
#[tokio::test]
async fn engine_bridge_holds_global_capacity_and_stop_uses_actual_http_control() {
    let temp = tempfile::tempdir().unwrap();
    let clock = Arc::new(ManualExecutionClock::default());
    let entered = Arc::new(tokio::sync::Semaphore::new(0));
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let owner = Uuid::new_v4();
    let execution = Arc::new(
        SqliteExecutionStore::open(
            temp.path().join("execution.sqlite"),
            protection(),
            clock.clone(),
        )
        .await
        .unwrap(),
    );
    let memory = SqliteMemoryStore::open(temp.path().join("memory.sqlite"), clock.clone())
        .await
        .unwrap();
    let bridge = Arc::new(
        DaemonEngineBridge::open(
            owner,
            execution.clone(),
            memory.clone(),
            temp.path().join("outbox.sqlite"),
            protection(),
            Arc::new(PendingModel {
                entered: entered.clone(),
                calls: calls.clone(),
            }),
            clock,
        )
        .await
        .unwrap(),
    );
    bridge
        .register_dialogue("first", profile(), 1)
        .await
        .unwrap();
    bridge
        .register_dialogue("second", profile(), 1)
        .await
        .unwrap();
    let mut state = crate::state::DaemonState::with_model_adapter(Arc::new(Model(Arc::new(
        Mutex::new(vec![]),
    ))));
    state.engine_bridge = Some(bridge.clone());
    let config = crate::DaemonConfig {
        max_concurrent_runs: 1,
        ..crate::DaemonConfig::default()
    };
    let router = crate::app::app_with_state_for_owner_test(Arc::new(RwLock::new(state)), config);
    let run = Uuid::new_v4();
    let running = tokio::spawn(request(
        router.clone(),
        "first",
        run,
        Uuid::new_v4(),
        "hello",
    ));
    entered.acquire().await.unwrap().forget();
    let saturated = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        request(
            router.clone(),
            "second",
            Uuid::new_v4(),
            Uuid::new_v4(),
            "must not dispatch",
        ),
    )
    .await;
    let stop = crate::routes::owner_test_support::authenticate(
        Request::builder()
            .method("POST")
            .uri(format!("/api/agents/first/durable-runs/{run}/control/stop")),
    )
    .body(Body::empty())
    .unwrap();
    assert_eq!(router.oneshot(stop).await.unwrap().status().as_u16(), 202);
    let (status, value) = tokio::time::timeout(std::time::Duration::from_secs(5), running)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status, 200, "{value}");
    assert_eq!(value["result"]["status"], "cancelled");
    assert_eq!(saturated.unwrap().0, 503);
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    bridge.shutdown().await.unwrap();
    execution.shutdown().await.unwrap();
    memory.shutdown().await.unwrap();
}

struct HttpFixture {
    temp: tempfile::TempDir,
    owner: Uuid,
    clock: Arc<ManualExecutionClock>,
    calls: Arc<Mutex<Vec<ModelGenerateRequest>>>,
    execution: Option<Arc<SqliteExecutionStore>>,
    memory: Option<SqliteMemoryStore>,
    bridge: Option<Arc<DaemonEngineBridge>>,
    router: Option<Router>,
}
impl HttpFixture {
    async fn new() -> Self {
        let mut fixture = Self {
            temp: tempfile::tempdir().unwrap(),
            owner: Uuid::new_v4(),
            clock: Arc::new(ManualExecutionClock::default()),
            calls: Arc::new(Mutex::new(vec![])),
            execution: None,
            memory: None,
            bridge: None,
            router: None,
        };
        fixture.open().await.unwrap();
        fixture
    }
    async fn open(&mut self) -> Result<(), String> {
        let execution = Arc::new(
            SqliteExecutionStore::open(
                self.temp.path().join("execution.sqlite"),
                protection(),
                self.clock.clone(),
            )
            .await
            .map_err(|e| e.to_string())?,
        );
        let memory =
            SqliteMemoryStore::open(self.temp.path().join("memory.sqlite"), self.clock.clone())
                .await
                .map_err(|e| e.to_string())?;
        let bridge = Arc::new(
            DaemonEngineBridge::open(
                self.owner,
                execution.clone(),
                memory.clone(),
                self.temp.path().join("outbox.sqlite"),
                protection(),
                Arc::new(Model(self.calls.clone())),
                self.clock.clone(),
            )
            .await
            .map_err(|e| e.to_string())?,
        );
        bridge
            .register_dialogue("assistant", profile(), 1)
            .await
            .map_err(|e| e.to_string())?;
        self.router = Some(app(bridge.clone()));
        self.execution = Some(execution);
        self.memory = Some(memory);
        self.bridge = Some(bridge);
        Ok(())
    }
    async fn close(&mut self) -> Result<(), String> {
        self.router = None;
        if let Some(bridge) = self.bridge.take() {
            bridge.shutdown().await.map_err(|e| e.to_string())?;
        }
        if let Some(execution) = self.execution.take() {
            execution.shutdown().await.map_err(|e| e.to_string())?;
        }
        if let Some(memory) = self.memory.take() {
            memory.shutdown().await.map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    fn id(value: &str) -> Uuid {
        use sha2::Digest;
        let digest = sha2::Sha256::digest(value.as_bytes());
        Uuid::from_slice(&digest[..16]).unwrap()
    }
}
#[async_trait]
impl anima_harness::conformance::PersistentHostFixture for HttpFixture {
    async fn submit(
        &mut self,
        id: &str,
        text: &str,
    ) -> Result<anima_harness::conformance::HostReply, String> {
        let (status, reply) = request(
            self.router.clone().unwrap(),
            "assistant",
            Self::id(id),
            Self::id(&format!("room:{id}")),
            text,
        )
        .await;
        if status != 200 {
            return Err(reply.to_string());
        }
        Ok(anima_harness::conformance::HostReply {
            run_id: reply["runId"].as_str().ok_or("missing run")?.into(),
            individual_id: reply["individualId"]
                .as_str()
                .ok_or("missing individual")?
                .into(),
            text: reply["result"]["data"]["text"]
                .as_str()
                .ok_or("missing text")?
                .into(),
        })
    }
    async fn accept_preference(
        &mut self,
        id: &str,
        preference: &str,
    ) -> Result<anima_harness::conformance::AcceptedPreference, String> {
        let request=crate::routes::owner_test_support::authenticate(Request::builder().method("POST").uri("/api/agents/assistant/durable-memory-events")).header("content-type","application/json").body(Body::from(serde_json::json!({"requestId":Self::id(&format!("preference:{id}")),"roomId":Self::id("memory-room"),"text":preference}).to_string())).unwrap();
        let response = self.router.clone().unwrap().oneshot(request).await.unwrap();
        let status = response.status().as_u16();
        let value: serde_json::Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        if status != 200 {
            return Err(value.to_string());
        }
        Ok(anima_harness::conformance::AcceptedPreference {
            memory_id: value["memoryId"].as_str().ok_or("missing memory")?.into(),
            source_event_id: value["sourceEventId"]
                .as_str()
                .ok_or("missing source")?
                .into(),
        })
    }
    async fn recall(&mut self, query: &str) -> Result<Vec<MemoryRecord>, String> {
        let request = crate::routes::owner_test_support::authenticate(
            Request::builder().uri(format!("/api/agents/assistant/durable-memory?q={query}")),
        )
        .body(Body::empty())
        .unwrap();
        let response = self.router.clone().unwrap().oneshot(request).await.unwrap();
        let status = response.status().as_u16();
        let value: serde_json::Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        if status != 200 {
            return Err(value.to_string());
        }
        serde_json::from_value(value["memories"].clone()).map_err(|e| e.to_string())
    }
    async fn reopen(&mut self) -> Result<(), String> {
        self.close().await?;
        self.open().await
    }
}

#[tokio::test]
async fn engine_bridge_shared_persistent_host_contract_uses_actual_authenticated_http() {
    let mut fixture = HttpFixture::new().await;
    anima_harness::conformance::assert_persistent_host_basics(&mut fixture)
        .await
        .unwrap();
    fixture.close().await.unwrap();
}

#[tokio::test]
async fn engine_bridge_rejects_mixed_execution_or_memory_databases() {
    let mut fixture = HttpFixture::new().await;
    let other_execution = Arc::new(
        SqliteExecutionStore::open(
            fixture.temp.path().join("other-execution.sqlite"),
            protection(),
            fixture.clock.clone(),
        )
        .await
        .unwrap(),
    );
    assert!(DaemonEngineBridge::open(
        fixture.owner,
        other_execution.clone(),
        fixture.memory.as_ref().unwrap().clone(),
        fixture.temp.path().join("outbox.sqlite"),
        protection(),
        Arc::new(Model(fixture.calls.clone())),
        fixture.clock.clone()
    )
    .await
    .is_err());
    other_execution.shutdown().await.unwrap();
    let other_memory = SqliteMemoryStore::open(
        fixture.temp.path().join("other-memory.sqlite"),
        fixture.clock.clone(),
    )
    .await
    .unwrap();
    assert!(DaemonEngineBridge::open(
        fixture.owner,
        fixture.execution.as_ref().unwrap().clone(),
        other_memory.clone(),
        fixture.temp.path().join("outbox.sqlite"),
        protection(),
        Arc::new(Model(fixture.calls.clone())),
        fixture.clock.clone()
    )
    .await
    .is_err());
    other_memory.shutdown().await.unwrap();
    assert!(fixture.calls.lock().unwrap().is_empty());
    fixture.close().await.unwrap();
}

#[tokio::test]
async fn engine_bridge_definition_upgrade_preserves_individual_memory_and_old_exact_retry() {
    let mut fixture = HttpFixture::new().await;
    let run = Uuid::new_v4();
    let room = Uuid::new_v4();
    let (status, accepted) = request(
        fixture.router.clone().unwrap(),
        "assistant",
        run,
        room,
        "prefers green tea",
    )
    .await;
    assert_eq!(status, 200);
    let mut changed = profile();
    changed.bio = Some("A new accepted persona".into());
    changed.model = "fixture-v2".into();
    fixture
        .bridge
        .as_ref()
        .unwrap()
        .register_dialogue("assistant", changed, 2)
        .await
        .unwrap();
    assert_eq!(
        request(
            fixture.router.clone().unwrap(),
            "assistant",
            run,
            room,
            "prefers green tea"
        )
        .await
        .1,
        accepted
    );
    let (status, next) = request(
        fixture.router.clone().unwrap(),
        "assistant",
        Uuid::new_v4(),
        Uuid::new_v4(),
        "What tea?",
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(next["individualId"], accepted["individualId"]);
    let calls = fixture.calls.lock().unwrap();
    assert!(calls
        .last()
        .unwrap()
        .system
        .contains("A new accepted persona"));
    assert!(calls.last().unwrap().system.contains("green tea"));
    drop(calls);
    fixture.close().await.unwrap();
}

#[tokio::test]
async fn engine_bridge_memory_outage_preserves_accepted_source_and_recovers_without_fallback() {
    let mut fixture = HttpFixture::new().await;
    let run = Uuid::new_v4();
    let room = Uuid::new_v4();
    fixture.memory.take().unwrap().shutdown().await.unwrap();
    let (status, error) = request(
        fixture.router.clone().unwrap(),
        "assistant",
        run,
        room,
        "I prefer black tea",
    )
    .await;
    assert_eq!(status, 503, "{error}");
    assert!(fixture.calls.lock().unwrap().is_empty());
    let raw = rusqlite::Connection::open(fixture.temp.path().join("outbox.sqlite")).unwrap();
    assert_eq!(
        raw.query_row::<usize, _, _>("SELECT COUNT(*) FROM bridge_runs", [], |r| r.get(0))
            .unwrap(),
        1
    );
    assert_eq!(
        raw.query_row::<usize, _, _>(
            "SELECT COUNT(*) FROM bridge_events WHERE pending=1",
            [],
            |r| r.get(0)
        )
        .unwrap(),
        1
    );
    drop(raw);
    use anima_harness::conformance::PersistentHostFixture;
    fixture.reopen().await.unwrap();
    let (status, value) = request(
        fixture.router.clone().unwrap(),
        "assistant",
        run,
        room,
        "I prefer black tea",
    )
    .await;
    assert_eq!(status, 200, "{value}");
    assert_eq!(fixture.calls.lock().unwrap().len(), 1);
    fixture.close().await.unwrap();
}

#[tokio::test]
async fn engine_bridge_source_and_pending_commit_together_and_ack_failure_is_retry_safe() {
    let mut fixture = HttpFixture::new().await;
    let raw = rusqlite::Connection::open(fixture.temp.path().join("outbox.sqlite")).unwrap();
    let run = Uuid::new_v4();
    let room = Uuid::new_v4();
    raw.execute_batch("CREATE TRIGGER reject_source BEFORE INSERT ON bridge_events BEGIN SELECT RAISE(ABORT,'fixture source failure'); END;").unwrap();
    assert_eq!(
        request(
            fixture.router.clone().unwrap(),
            "assistant",
            run,
            room,
            "prefers apples"
        )
        .await
        .0,
        503
    );
    assert!(fixture.calls.lock().unwrap().is_empty());
    assert_eq!(
        raw.query_row::<usize, _, _>("SELECT COUNT(*) FROM bridge_runs", [], |r| r.get(0))
            .unwrap(),
        0
    );
    raw.execute_batch("DROP TRIGGER reject_source; CREATE TRIGGER reject_ack BEFORE UPDATE ON bridge_events WHEN NEW.pending=0 BEGIN SELECT RAISE(ABORT,'fixture acknowledgement failure'); END;").unwrap();
    assert_eq!(
        request(
            fixture.router.clone().unwrap(),
            "assistant",
            run,
            room,
            "prefers apples"
        )
        .await
        .0,
        503
    );
    assert!(fixture.calls.lock().unwrap().is_empty());
    assert_eq!(
        raw.query_row::<usize, _, _>(
            "SELECT COUNT(*) FROM bridge_events WHERE pending=1",
            [],
            |r| r.get(0)
        )
        .unwrap(),
        1
    );
    let memory = rusqlite::Connection::open(fixture.temp.path().join("memory.sqlite")).unwrap();
    assert_eq!(
        memory
            .query_row::<usize, _, _>("SELECT COUNT(*) FROM memory_records", [], |r| r.get(0))
            .unwrap(),
        1
    );
    raw.execute_batch("DROP TRIGGER reject_ack;").unwrap();
    assert_eq!(
        request(
            fixture.router.clone().unwrap(),
            "assistant",
            run,
            room,
            "prefers apples"
        )
        .await
        .0,
        200
    );
    assert_eq!(
        memory
            .query_row::<usize, _, _>("SELECT COUNT(*) FROM memory_records", [], |r| r.get(0))
            .unwrap(),
        1
    );
    fixture.close().await.unwrap();
}

struct ClockModel;
#[async_trait]
impl ModelAdapter for ClockModel {
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
        config: &AgentConfig,
        request: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        assert_eq!(config.tools.as_ref().unwrap().len(), 1);
        assert_eq!(
            config.tools.as_ref().unwrap()[0].name,
            clock_capability::CLOCK_ID
        );
        let has_result = request
            .messages
            .iter()
            .any(|message| message.role == MessageRole::Tool);
        Ok(ModelGenerateResponse {
            content: Content {
                text: if has_result { "Clock read." } else { "" }.into(),
                ..Content::default()
            },
            tool_calls: if has_result {
                None
            } else {
                Some(vec![ToolCall {
                    id: "clock-call".into(),
                    name: clock_capability::CLOCK_ID.into(),
                    args: Default::default(),
                }])
            },
            usage: TokenUsage {
                prompt_tokens: 5,
                completion_tokens: 2,
                total_tokens: 7,
                ..TokenUsage::default()
            },
            stop_reason: if has_result {
                ModelStopReason::End
            } else {
                ModelStopReason::ToolCall
            },
        })
    }
}
fn clock_profile() -> AgentConfig {
    let mut config = profile();
    config.settings = Some(AgentSettings {
        max_tool_iterations: Some(1),
        max_tokens: Some(128),
        ..AgentSettings::default()
    });
    config
}
fn clock_protection() -> PersistenceProtection {
    let inventory =
        PersistenceCapabilitySecretInventory::new(&clock_manifest().unwrap(), vec![]).unwrap();
    PersistenceProtection::allow_model_payloads(
        PersistenceSnapshotSealKey::new([87; 32]).unwrap(),
        vec![inventory],
    )
    .unwrap()
}
#[tokio::test]
async fn engine_bridge_clock_profile_uses_scoped_policy_durable_receipt_and_one_shared_budget() {
    let temp = tempfile::tempdir().unwrap();
    let owner = Uuid::new_v4();
    let clock = Arc::new(ManualExecutionClock::default());
    let execution = Arc::new(
        SqliteExecutionStore::open(
            temp.path().join("execution.sqlite"),
            clock_protection(),
            clock.clone(),
        )
        .await
        .unwrap(),
    );
    let memory = SqliteMemoryStore::open(temp.path().join("memory.sqlite"), clock.clone())
        .await
        .unwrap();
    let bridge = Arc::new(
        DaemonEngineBridge::open(
            owner,
            execution.clone(),
            memory.clone(),
            temp.path().join("outbox.sqlite"),
            clock_protection(),
            Arc::new(ClockModel),
            clock,
        )
        .await
        .unwrap(),
    );
    bridge
        .register_clock("clock-assistant", clock_profile(), 1)
        .await
        .unwrap();
    let run = Uuid::new_v4();
    let room = Uuid::new_v4();
    let router = app(bridge.clone());
    let (status, response) = request(
        router.clone(),
        "clock-assistant",
        run,
        room,
        "What time is it?",
    )
    .await;
    assert_eq!(status, 200, "{response}");
    assert_eq!(response["result"]["data"]["text"], "Clock read.");
    assert_eq!(
        request(router, "clock-assistant", run, room, "What time is it?")
            .await
            .1,
        response
    );
    let raw = rusqlite::Connection::open(temp.path().join("outbox.sqlite")).unwrap();
    assert_eq!(
        raw.query_row::<usize, _, _>("SELECT COUNT(*) FROM bridge_capability_receipts", [], |r| r
            .get(0))
            .unwrap(),
        1
    );
    let usage = execution
        .bind_budget(owner, &format!("daemon-dialogue:{run}"))
        .await
        .unwrap()
        .snapshot()
        .await
        .unwrap()
        .consumption()
        .unwrap();
    assert_eq!(usage.model_attempts, 2);
    assert_eq!(usage.tool_calls, 1);
    assert!(!temp.path().join("workspace").exists());
    bridge.shutdown().await.unwrap();
    execution.shutdown().await.unwrap();
    memory.shutdown().await.unwrap();
}

#[test]
fn engine_bridge_process_peer() {
    let Ok(path) = std::env::var("ANIMA_BRIDGE_FIXTURE_PATH") else {
        return;
    };
    let mode = std::env::var("ANIMA_BRIDGE_FIXTURE_MODE").unwrap();
    let phase = std::env::var("ANIMA_BRIDGE_FIXTURE_PHASE").unwrap();
    tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap().block_on(async{
        let path=std::path::PathBuf::from(path);let owner=Uuid::from_u128(120);let run=Uuid::from_u128(121);let room=Uuid::from_u128(122);let clock=Arc::new(ManualExecutionClock::default());let protected=if phase=="clock-result"{clock_protection()}else{protection()};
        let execution=Arc::new(SqliteExecutionStore::open(path.join("execution.sqlite"),protected.clone(),clock.clone()).await.unwrap());let memory=SqliteMemoryStore::open(path.join("memory.sqlite"),clock.clone()).await.unwrap();let model:Arc<dyn ModelAdapter>=if phase=="clock-result"{Arc::new(ClockModel)}else{Arc::new(Model(Arc::new(Mutex::new(vec![]))))};let bridge=Arc::new(DaemonEngineBridge::open(owner,execution.clone(),memory.clone(),path.join("outbox.sqlite"),protected,model,clock).await.unwrap());
        if phase=="clock-result"{bridge.register_clock("assistant",clock_profile(),1).await.unwrap();}else{bridge.register_dialogue("assistant",profile(),1).await.unwrap();}
        let router=app(bridge.clone());
        if mode=="commit-and-park"{
            if phase=="clock-result"{let (status,value)=request(router,"assistant",run,room,"What time?").await;assert_eq!(status,200,"{value}");}
            else{let raw=rusqlite::Connection::open(path.join("outbox.sqlite")).unwrap();raw.execute_batch("CREATE TRIGGER pending_after_memory BEFORE UPDATE ON bridge_events WHEN NEW.pending=0 BEGIN SELECT RAISE(ABORT,'fixture acknowledgement interrupted'); END;").unwrap();assert_eq!(request(router,"assistant",run,room,"prefers jasmine tea").await.0,503);raw.execute_batch("DROP TRIGGER pending_after_memory;").unwrap();}
            use std::io::Write;println!("HOST_BRIDGE_COMMITTED");std::io::stdout().flush().unwrap();loop{std::thread::sleep(std::time::Duration::from_secs(60));}
        }else{
            if phase=="clock-result"{let (status,value)=request(router,"assistant",run,room,"What time?").await;assert_eq!(status,200,"{value}");assert_eq!(value["result"]["data"]["text"],"Clock read.");let raw=rusqlite::Connection::open(path.join("outbox.sqlite")).unwrap();assert_eq!(raw.query_row::<usize,_,_>("SELECT COUNT(*) FROM bridge_capability_receipts",[],|r|r.get(0)).unwrap(),1);let usage=execution.bind_budget(owner,&format!("daemon-dialogue:{run}")).await.unwrap().snapshot().await.unwrap().consumption().unwrap();assert_eq!(usage.model_attempts,2);assert_eq!(usage.tool_calls,1);}
            else{let records=bridge.recall("assistant","jasmine").await.unwrap();assert_eq!(records.len(),1);assert_eq!(records[0].kind(),MemoryKind::Preference);let raw=rusqlite::Connection::open(path.join("outbox.sqlite")).unwrap();assert_eq!(raw.query_row::<usize,_,_>("SELECT COUNT(*) FROM bridge_events WHERE pending=1",[],|r|r.get(0)).unwrap(),0);}
            bridge.shutdown().await.unwrap();execution.shutdown().await.unwrap();memory.shutdown().await.unwrap();println!("HOST_BRIDGE_VERIFIED");
        }
    });
}

#[test]
fn engine_bridge_actual_process_death_recovers_pending_memory_and_committed_clock_receipt() {
    use std::{
        io::{BufRead, BufReader},
        process::{Command, Stdio},
    };
    for phase in ["pending-memory", "clock-result"] {
        let temp = tempfile::tempdir().unwrap();
        let exe = std::env::current_exe().unwrap();
        let mut child = Command::new(&exe)
            .args([
                "--exact",
                "engine_bridge::tests::engine_bridge_process_peer",
                "--nocapture",
            ])
            .env("ANIMA_BRIDGE_FIXTURE_PATH", temp.path())
            .env("ANIMA_BRIDGE_FIXTURE_MODE", "commit-and-park")
            .env("ANIMA_BRIDGE_FIXTURE_PHASE", phase)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (ready, waiting) = std::sync::mpsc::channel();
        let reader = std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                if line.unwrap().contains("HOST_BRIDGE_COMMITTED") {
                    let _ = ready.send(());
                    return;
                }
            }
        });
        let accepted = waiting.recv_timeout(std::time::Duration::from_secs(30));
        let _ = child.kill();
        let output = child.wait_with_output().unwrap();
        reader.join().unwrap();
        assert!(
            accepted.is_ok(),
            "phase {phase}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let verified = Command::new(exe)
            .args([
                "--exact",
                "engine_bridge::tests::engine_bridge_process_peer",
                "--nocapture",
            ])
            .env("ANIMA_BRIDGE_FIXTURE_PATH", temp.path())
            .env("ANIMA_BRIDGE_FIXTURE_MODE", "verify")
            .env("ANIMA_BRIDGE_FIXTURE_PHASE", phase)
            .output()
            .unwrap();
        assert!(
            verified.status.success(),
            "phase {phase}: {}",
            String::from_utf8_lossy(&verified.stderr)
        );
        assert!(String::from_utf8_lossy(&verified.stdout).contains("HOST_BRIDGE_VERIFIED"));
    }
}

#[tokio::test]
async fn engine_bridge_zero_tool_round_dialogue_succeeds_and_clock_refuses_before_admission() {
    let mut fixture = HttpFixture::new().await;
    let mut text_only = profile();
    text_only.settings = Some(AgentSettings {
        max_tool_iterations: Some(0),
        ..AgentSettings::default()
    });
    fixture
        .bridge
        .as_ref()
        .unwrap()
        .register_dialogue("text-only", text_only.clone(), 1)
        .await
        .unwrap();
    assert!(fixture
        .bridge
        .as_ref()
        .unwrap()
        .register_clock("unsupported-clock", text_only, 1)
        .await
        .is_err());
    let (status, value) = request(
        fixture.router.clone().unwrap(),
        "text-only",
        Uuid::new_v4(),
        Uuid::new_v4(),
        "Hello without tools",
    )
    .await;
    assert_eq!(status, 200, "{value}");
    assert_eq!(value["result"]["data"]["text"], "Remembered.");
    let raw = rusqlite::Connection::open(fixture.temp.path().join("outbox.sqlite")).unwrap();
    assert_eq!(
        raw.query_row::<usize, _, _>(
            "SELECT COUNT(*) FROM bridge_profiles WHERE agent='unsupported-clock'",
            [],
            |r| r.get(0)
        )
        .unwrap(),
        0
    );
    fixture.close().await.unwrap();
}

#[tokio::test]
async fn engine_bridge_outage_backlog_drains_in_bounded_batches_without_memoryless_dispatch() {
    let mut fixture = HttpFixture::new().await;
    fixture.memory.take().unwrap().shutdown().await.unwrap();
    let room = Uuid::new_v4();
    for index in 0..65 {
        assert_eq!(
            request(
                fixture.router.clone().unwrap(),
                "assistant",
                Uuid::new_v4(),
                room,
                &format!("prefers tea number {index}")
            )
            .await
            .0,
            503
        );
    }
    use anima_harness::conformance::PersistentHostFixture;
    fixture.reopen().await.unwrap();
    let run = Uuid::new_v4();
    assert_eq!(
        request(
            fixture.router.clone().unwrap(),
            "assistant",
            run,
            room,
            "what tea?"
        )
        .await
        .0,
        503
    );
    assert!(fixture.calls.lock().unwrap().is_empty());
    let (status, value) = request(
        fixture.router.clone().unwrap(),
        "assistant",
        run,
        room,
        "what tea?",
    )
    .await;
    assert_eq!(status, 200, "{value}");
    assert_eq!(fixture.calls.lock().unwrap().len(), 1);
    let raw = rusqlite::Connection::open(fixture.temp.path().join("outbox.sqlite")).unwrap();
    assert_eq!(
        raw.query_row::<usize, _, _>(
            "SELECT COUNT(*) FROM bridge_events WHERE pending=1",
            [],
            |r| r.get(0)
        )
        .unwrap(),
        0
    );
    fixture.close().await.unwrap();
}

async fn source_request(
    router: Router,
    agent: &str,
    body: serde_json::Value,
) -> (u16, serde_json::Value) {
    let request = crate::routes::owner_test_support::authenticate(
        Request::builder()
            .method("POST")
            .uri(format!("/api/agents/{agent}/durable-memory-sources")),
    )
    .header("content-type", "application/json")
    .body(Body::from(body.to_string()))
    .unwrap();
    let response = router.oneshot(request).await.unwrap();
    let status = response.status().as_u16();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}
async fn relationship_request(router: Router, agent: &str) -> (u16, serde_json::Value) {
    let request = crate::routes::owner_test_support::authenticate(
        Request::builder().uri(format!("/api/agents/{agent}/durable-relationships")),
    )
    .body(Body::empty())
    .unwrap();
    let response = router.oneshot(request).await.unwrap();
    let status = response.status().as_u16();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}
#[tokio::test]
async fn engine_bridge_memory_sources_correct_forget_cas_replay_and_reopen_through_http() {
    use anima_harness::conformance::PersistentHostFixture;
    let mut fixture = HttpFixture::new().await;
    let old = fixture
        .accept_preference("old-preference", "prefers tea")
        .await
        .unwrap();
    let new = fixture
        .accept_preference("new-preference", "prefers coffee")
        .await
        .unwrap();
    let source = Uuid::new_v4();
    let correction = serde_json::json!({"sourceEventId":source,"operation":{"kind":"correction","memoryId":new.memory_id,"expectedRevision":1,"corrects":old.memory_id,"reason":"owner correction"}});
    let (status, receipt) = source_request(
        fixture.router.clone().unwrap(),
        "assistant",
        correction.clone(),
    )
    .await;
    assert_eq!(status, 200, "{receipt}");
    assert_eq!(
        source_request(
            fixture.router.clone().unwrap(),
            "assistant",
            correction.clone()
        )
        .await,
        (200, receipt.clone())
    );
    let mut conflict = correction.clone();
    conflict["operation"]["reason"] = "changed reason".into();
    assert_eq!(
        source_request(fixture.router.clone().unwrap(), "assistant", conflict)
            .await
            .0,
        409
    );
    let mut stale = correction.clone();
    stale["sourceEventId"] = Uuid::new_v4().to_string().into();
    assert_eq!(
        source_request(fixture.router.clone().unwrap(), "assistant", stale)
            .await
            .0,
        409
    );
    assert!(fixture.recall("tea").await.unwrap().is_empty());
    let corrected = fixture.recall("coffee").await.unwrap();
    assert_eq!(corrected.len(), 1);
    assert_eq!(corrected[0].revision(), 2);
    assert_eq!(corrected[0].corrects(), Some(old.memory_id.as_str()));
    let forget_superseded = serde_json::json!({"sourceEventId":Uuid::new_v4(),"operation":{"kind":"forget","memoryId":old.memory_id,"expectedRevision":1,"reason":"forget corrected history"}});
    let (status, value) = source_request(
        fixture.router.clone().unwrap(),
        "assistant",
        forget_superseded,
    )
    .await;
    assert_eq!(status, 200, "{value}");
    let identity = IdentityBinding::new(fixture.owner.to_string(), "assistant").unwrap();
    let writer = fixture
        .memory
        .as_ref()
        .unwrap()
        .bind(authority(&identity).unwrap())
        .await
        .unwrap();
    let deadline = fixture.clock.now_ms() + 1;
    writer
        .write(
            MemoryWrite::new(
                "expired-private",
                MemoryKind::Fact,
                MemoryScope::agent("assistant").unwrap(),
                "copied expired fixture",
                1.0,
                MemoryProvenance::new("copied-fixture", "expired-fixture", fixture.clock.now_ms())
                    .unwrap(),
            )
            .unwrap()
            .with_retention_deadline(deadline)
            .unwrap(),
        )
        .await
        .unwrap();
    fixture.clock.advance_ms(1).unwrap();
    let forget_expired = serde_json::json!({"sourceEventId":Uuid::new_v4(),"operation":{"kind":"forget","memoryId":"expired-private","expectedRevision":1,"reason":"forget expired history"}});
    let (status, value) =
        source_request(fixture.router.clone().unwrap(), "assistant", forget_expired).await;
    assert_eq!(status, 200, "{value}");
    assert!(writer
        .get("expired-private")
        .await
        .unwrap()
        .forgotten_at_ms()
        .is_some());
    let forget = serde_json::json!({"sourceEventId":Uuid::new_v4(),"operation":{"kind":"forget","memoryId":new.memory_id,"expectedRevision":2,"reason":"owner forgetting"}});
    let (status, forgotten) =
        source_request(fixture.router.clone().unwrap(), "assistant", forget.clone()).await;
    assert_eq!(status, 200, "{forgotten}");
    assert!(fixture.recall("coffee").await.unwrap().is_empty());
    fixture.reopen().await.unwrap();
    assert_eq!(
        source_request(fixture.router.clone().unwrap(), "assistant", forget).await,
        (200, forgotten)
    );
    assert_eq!(
        source_request(fixture.router.clone().unwrap(), "assistant", correction).await,
        (200, receipt)
    );
    assert!(fixture.recall("tea").await.unwrap().is_empty());
    assert!(fixture.calls.lock().unwrap().is_empty());
    fixture.close().await.unwrap();
}

#[tokio::test]
async fn engine_bridge_memory_sources_relationship_perspective_evidence_and_private_denial() {
    use anima_harness::conformance::PersistentHostFixture;
    let mut fixture = HttpFixture::new().await;
    fixture
        .bridge
        .as_ref()
        .unwrap()
        .register_dialogue("mira", profile(), 1)
        .await
        .unwrap();
    let evidence = fixture
        .accept_preference("relationship-evidence", "Mira helped me with tea")
        .await
        .unwrap();
    let request = serde_json::json!({"sourceEventId":Uuid::new_v4(),"operation":{"kind":"relationship","targetAgentId":"mira","relationshipType":"trust","summary":"Mira was helpful","strength":0.8,"confidence":0.9,"evidenceMemoryIds":[evidence.memory_id]}});
    let (status, receipt) = source_request(
        fixture.router.clone().unwrap(),
        "assistant",
        request.clone(),
    )
    .await;
    assert_eq!(status, 200, "{receipt}");
    assert_eq!(
        source_request(
            fixture.router.clone().unwrap(),
            "assistant",
            request.clone()
        )
        .await,
        (200, receipt)
    );
    let (status, views) = relationship_request(fixture.router.clone().unwrap(), "assistant").await;
    assert_eq!(status, 200);
    assert_eq!(views["relationships"].as_array().unwrap().len(), 1);
    assert_eq!(
        views["relationships"][0]["observer"]["individual_id"],
        "assistant"
    );
    assert_eq!(
        relationship_request(fixture.router.clone().unwrap(), "mira")
            .await
            .1["relationships"],
        serde_json::json!([])
    );
    let hostile = serde_json::json!({"sourceEventId":Uuid::new_v4(),"operation":{"kind":"forget","memoryId":evidence.memory_id,"expectedRevision":1,"reason":"foreign private memory"}});
    assert_eq!(
        source_request(fixture.router.clone().unwrap(), "mira", hostile)
            .await
            .0,
        403
    );
    let mut foreign_evidence = request.clone();
    foreign_evidence["sourceEventId"] = Uuid::new_v4().to_string().into();
    foreign_evidence["operation"]["targetAgentId"] = "assistant".into();
    assert_eq!(
        source_request(fixture.router.clone().unwrap(), "mira", foreign_evidence)
            .await
            .0,
        403
    );
    fixture.reopen().await.unwrap();
    assert_eq!(
        relationship_request(fixture.router.clone().unwrap(), "assistant")
            .await
            .1["relationships"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let forget = serde_json::json!({"sourceEventId":Uuid::new_v4(),"operation":{"kind":"forget","memoryId":evidence.memory_id,"expectedRevision":1,"reason":"withdraw evidence"}});
    assert_eq!(
        source_request(fixture.router.clone().unwrap(), "assistant", forget)
            .await
            .0,
        200
    );
    assert_eq!(
        relationship_request(fixture.router.clone().unwrap(), "assistant")
            .await
            .1["relationships"],
        serde_json::json!([])
    );
    let mut inactive = request;
    inactive["sourceEventId"] = Uuid::new_v4().to_string().into();
    assert_eq!(
        source_request(fixture.router.clone().unwrap(), "assistant", inactive)
            .await
            .0,
        409
    );
    fixture.close().await.unwrap();
}

#[tokio::test]
async fn engine_bridge_memory_sources_foreign_owner_cannot_mutate_known_memory_id() {
    use anima_harness::conformance::PersistentHostFixture;
    let mut fixture = HttpFixture::new().await;
    let evidence = fixture
        .accept_preference("private-source", "prefers private tea")
        .await
        .unwrap();
    let other = Arc::new(
        DaemonEngineBridge::open(
            Uuid::new_v4(),
            fixture.execution.as_ref().unwrap().clone(),
            fixture.memory.as_ref().unwrap().clone(),
            fixture.temp.path().join("foreign-outbox.sqlite"),
            protection(),
            Arc::new(Model(Arc::new(Mutex::new(vec![])))),
            fixture.clock.clone(),
        )
        .await
        .unwrap(),
    );
    other
        .register_dialogue("assistant", profile(), 1)
        .await
        .unwrap();
    let other_router = app(other.clone());
    let own_run = Uuid::new_v4();
    let (status, value) = request(
        other_router.clone(),
        "assistant",
        own_run,
        Uuid::new_v4(),
        "What tea?",
    )
    .await;
    assert_eq!(status, 200, "{value}");
    let hostile = serde_json::json!({"sourceEventId":Uuid::new_v4(),"operation":{"kind":"forget","memoryId":evidence.memory_id,"expectedRevision":1,"reason":"foreign owner"}});
    assert_eq!(
        source_request(other_router.clone(), "assistant", hostile)
            .await
            .0,
        404
    );
    assert_eq!(fixture.recall("private").await.unwrap().len(), 1);
    let own_forget = serde_json::json!({"sourceEventId":Uuid::new_v4(),"operation":{"kind":"forget","memoryId":format!("daemon:user:{own_run}"),"expectedRevision":1,"reason":"own memory"}});
    assert_eq!(
        source_request(other_router, "assistant", own_forget)
            .await
            .0,
        200
    );
    other.shutdown().await.unwrap();
    fixture.close().await.unwrap();
}

#[tokio::test]
async fn engine_bridge_memory_sources_ack_failure_reopen_replays_before_current_state_validation() {
    use anima_harness::conformance::PersistentHostFixture;
    let mut fixture = HttpFixture::new().await;
    let memory = fixture
        .accept_preference("forgotten-after-apply", "prefers tea")
        .await
        .unwrap();
    let source = Uuid::new_v4();
    let request = serde_json::json!({"sourceEventId":source,"operation":{"kind":"forget","memoryId":memory.memory_id,"expectedRevision":1,"reason":"owner request"}});
    let path = fixture.temp.path().join("outbox.sqlite");
    let raw = rusqlite::Connection::open(&path).unwrap();
    raw.execute_batch("CREATE TRIGGER fail_memory_source_ack BEFORE UPDATE ON bridge_memory_sources WHEN NEW.pending=0 BEGIN SELECT RAISE(ABORT,'fixture source acknowledgement failure'); END;").unwrap();
    assert_eq!(
        source_request(
            fixture.router.clone().unwrap(),
            "assistant",
            request.clone()
        )
        .await
        .0,
        503
    );
    let identity = IdentityBinding::new(fixture.owner.to_string(), "assistant").unwrap();
    let bound = fixture
        .memory
        .as_ref()
        .unwrap()
        .bind(authority(&identity).unwrap())
        .await
        .unwrap();
    assert_eq!(bound.get(&memory.memory_id).await.unwrap().revision(), 2);
    assert!(bound
        .get(&memory.memory_id)
        .await
        .unwrap()
        .forgotten_at_ms()
        .is_some());
    assert_eq!(
        raw.query_row::<usize, _, _>(
            "SELECT COUNT(*) FROM bridge_memory_sources WHERE pending=1 AND outcome IS NULL",
            [],
            |row| row.get(0)
        )
        .unwrap(),
        1
    );
    drop(raw);
    fixture.reopen().await.unwrap();
    assert_eq!(
        source_request(
            fixture.router.clone().unwrap(),
            "assistant",
            request.clone()
        )
        .await
        .0,
        503
    );
    let raw = rusqlite::Connection::open(path).unwrap();
    raw.execute_batch("DROP TRIGGER fail_memory_source_ack;")
        .unwrap();
    let (status, receipt) = source_request(
        fixture.router.clone().unwrap(),
        "assistant",
        request.clone(),
    )
    .await;
    assert_eq!(status, 200, "{receipt}");
    assert_eq!(
        source_request(fixture.router.clone().unwrap(), "assistant", request).await,
        (200, receipt)
    );
    let bound = fixture
        .memory
        .as_ref()
        .unwrap()
        .bind(authority(&identity).unwrap())
        .await
        .unwrap();
    assert_eq!(bound.get(&memory.memory_id).await.unwrap().revision(), 2);
    assert!(fixture.calls.lock().unwrap().is_empty());
    assert_eq!(
        raw.query_row::<usize, _, _>(
            "SELECT COUNT(*) FROM bridge_memory_sources WHERE pending=1",
            [],
            |row| row.get(0)
        )
        .unwrap(),
        0
    );
    fixture.close().await.unwrap();
}

#[tokio::test]
async fn engine_bridge_memory_sources_permanent_cas_loss_is_recorded_and_does_not_poison_readiness()
{
    use anima_harness::conformance::PersistentHostFixture;
    let mut fixture = HttpFixture::new().await;
    let memory = fixture
        .accept_preference("external-cas-winner", "prefers tea")
        .await
        .unwrap();
    let source = Uuid::new_v4();
    let body = serde_json::json!({"sourceEventId":source,"operation":{"kind":"forget","memoryId":memory.memory_id,"expectedRevision":1,"reason":"owner request"}});
    let identity = IdentityBinding::new(fixture.owner.to_string(), "assistant").unwrap();
    // A copied accepted checkpoint plus a second trusted writer reproduces the
    // race excluded by the normal per-profile HTTP admission gate.
    let effect = anima_memory::MemoryEffect::new(
        format!("daemon:memory:{source}:assistant"),
        identity.clone(),
        vec![],
        vec![MemoryRevision::forget(&memory.memory_id, 1, "owner request").unwrap()],
        vec![],
    )
    .unwrap();
    let accepted = memory_sources::AcceptedMemorySource {
        version: 1,
        agent_id: "assistant".into(),
        identity: identity.clone(),
        request: serde_json::from_value(body.clone()).unwrap(),
        source_kind: anima_memory::EvidenceKind::UserStatement,
        observed_at_ms: fixture.clock.now_ms(),
        effect,
    };
    fixture
        .bridge
        .as_ref()
        .unwrap()
        .outbox
        .accept_memory_source(accepted)
        .await
        .unwrap();
    let writer = fixture
        .memory
        .as_ref()
        .unwrap()
        .bind(authority(&identity).unwrap())
        .await
        .unwrap();
    writer
        .revise(MemoryRevision::forget(&memory.memory_id, 1, "external trusted winner").unwrap())
        .await
        .unwrap();
    let rejected = source_request(fixture.router.clone().unwrap(), "assistant", body.clone()).await;
    assert_eq!(rejected.0, 409);
    assert_eq!(
        source_request(fixture.router.clone().unwrap(), "assistant", body).await,
        rejected
    );
    assert_eq!(
        writer.get(&memory.memory_id).await.unwrap().forget_reason(),
        Some("external trusted winner")
    );
    let raw = rusqlite::Connection::open(fixture.temp.path().join("outbox.sqlite")).unwrap();
    assert_eq!(raw.query_row::<usize,_,_>("SELECT COUNT(*) FROM bridge_memory_sources WHERE pending=0 AND outcome IS NOT NULL AND length(payload)>0",[],|row|row.get(0)).unwrap(),1);
    assert!(fixture.recall("tea").await.unwrap().is_empty());
    let (status, value) = request(
        fixture.router.clone().unwrap(),
        "assistant",
        Uuid::new_v4(),
        Uuid::new_v4(),
        "Hello after rejected mutation",
    )
    .await;
    assert_eq!(status, 200, "{value}");
    fixture.close().await.unwrap();
}

#[tokio::test]
async fn engine_bridge_registration_cap_rejects_before_persistence_and_preserves_reopen() {
    let mut fixture = HttpFixture::new().await;
    let template = fixture
        .bridge
        .as_ref()
        .unwrap()
        .profiles
        .lock()
        .unwrap()
        .get("assistant")
        .unwrap()
        .profile
        .clone();
    let raw = rusqlite::Connection::open(fixture.temp.path().join("outbox.sqlite")).unwrap();
    for index in 1..256 {
        let mut profile = template.clone();
        profile.agent_id = format!("seed-{index:03}");
        profile.identity =
            IdentityBinding::new(fixture.owner.to_string(), &profile.agent_id).unwrap();
        profile.definition.id = format!("daemon-dialogue-{}", profile.agent_id);
        let key = Uuid::new_v4();
        let payload = protection()
            .protect_host_admission(fixture.owner, key, &serde_json::to_vec(&profile).unwrap())
            .unwrap();
        raw.execute(
            "INSERT INTO bridge_profiles VALUES(?1,?2,?3)",
            rusqlite::params![profile.agent_id, key.to_string(), payload],
        )
        .unwrap();
    }
    assert!(fixture
        .bridge
        .as_ref()
        .unwrap()
        .register_dialogue("overflow", profile(), 1)
        .await
        .is_err());
    assert_eq!(
        raw.query_row::<usize, _, _>("SELECT COUNT(*) FROM bridge_profiles", [], |row| row.get(0))
            .unwrap(),
        256
    );
    drop(raw);
    use anima_harness::conformance::PersistentHostFixture;
    fixture.reopen().await.unwrap();
    assert_eq!(
        fixture
            .bridge
            .as_ref()
            .unwrap()
            .profiles
            .lock()
            .unwrap()
            .len(),
        256
    );
    assert!(fixture.bridge.as_ref().unwrap().handles("assistant"));
    assert!(fixture.calls.lock().unwrap().is_empty());
    fixture.close().await.unwrap();
}

// Shared lifecycle conformance uses the same authenticated router as the basic
// fixture. Alternate authority is allocated by the host fixture, never the input.
impl HttpFixture {
    async fn lifecycle_scope(
        &self,
        scope: anima_harness::conformance::OtherHostScope,
    ) -> Result<(Router, &'static str, Option<Arc<DaemonEngineBridge>>), String> {
        use anima_harness::conformance::OtherHostScope;
        match scope {
            OtherHostScope::Individual => {
                self.bridge
                    .as_ref()
                    .unwrap()
                    .register_dialogue("mira", profile(), 1)
                    .await
                    .map_err(|e| e.to_string())?;
                Ok((self.router.clone().unwrap(), "mira", None))
            }
            OtherHostScope::Partition => {
                let bridge = Arc::new(
                    DaemonEngineBridge::open(
                        Self::id(&format!("lifecycle-other-owner:{}", self.owner)),
                        self.execution.as_ref().unwrap().clone(),
                        self.memory.as_ref().unwrap().clone(),
                        self.temp.path().join("lifecycle-foreign-outbox.sqlite"),
                        protection(),
                        Arc::new(Model(self.calls.clone())),
                        self.clock.clone(),
                    )
                    .await
                    .map_err(|e| e.to_string())?,
                );
                bridge
                    .register_dialogue("assistant", profile(), 1)
                    .await
                    .map_err(|e| e.to_string())?;
                Ok((app(bridge.clone()), "assistant", Some(bridge)))
            }
        }
    }
    fn lifecycle_receipt(
        memory_id: &str,
        value: serde_json::Value,
    ) -> Result<anima_harness::conformance::PreferenceMutation, String> {
        if !value["memoryIds"]
            .as_array()
            .is_some_and(|ids| ids.iter().any(|id| id.as_str() == Some(memory_id)))
        {
            return Err(format!(
                "lifecycle receipt omitted affected memory: {value}"
            ));
        }
        Ok(anima_harness::conformance::PreferenceMutation {
            memory_id: memory_id.into(),
            receipt: value.to_string(),
        })
    }
}
#[async_trait]
impl anima_harness::conformance::PersistentHostLifecycleFixture for HttpFixture {
    fn captured_requests(&self) -> Vec<ModelGenerateRequest> {
        self.calls.lock().unwrap().clone()
    }
    async fn submit_as(
        &mut self,
        scope: anima_harness::conformance::OtherHostScope,
        id: &str,
        text: &str,
    ) -> Result<anima_harness::conformance::HostReply, String> {
        let (router, agent, foreign) = self.lifecycle_scope(scope).await?;
        let (status, reply) = request(
            router,
            agent,
            Self::id(id),
            Self::id(&format!("room:{id}")),
            text,
        )
        .await;
        if let Some(foreign) = foreign {
            foreign.shutdown().await.map_err(|e| e.to_string())?;
        }
        if status != 200 {
            return Err(format!("HTTP {status}: {reply}"));
        }
        Ok(anima_harness::conformance::HostReply {
            run_id: reply["runId"].as_str().ok_or("missing run")?.into(),
            individual_id: reply["individualId"]
                .as_str()
                .ok_or("missing individual")?
                .into(),
            text: reply["result"]["data"]["text"]
                .as_str()
                .ok_or("missing text")?
                .into(),
        })
    }
    async fn correct_preference(
        &mut self,
        source: &str,
        original: &MemoryRecord,
        replacement: &str,
    ) -> Result<anima_harness::conformance::PreferenceMutation, String> {
        use anima_harness::conformance::PersistentHostFixture;
        let accepted = self
            .accept_preference(&format!("{source}-replacement"), replacement)
            .await?;
        let body = serde_json::json!({"sourceEventId":Self::id(source),"operation":{"kind":"correction","memoryId":accepted.memory_id,"expectedRevision":1,"corrects":original.id(),"reason":"shared lifecycle correction"}});
        let (status, value) = source_request(self.router.clone().unwrap(), "assistant", body).await;
        if status != 200 {
            return Err(format!("HTTP {status}: {value}"));
        }
        Self::lifecycle_receipt(&accepted.memory_id, value)
    }
    async fn forget_preference(
        &mut self,
        source: &str,
        memory_id: &str,
        revision: u64,
    ) -> Result<anima_harness::conformance::PreferenceMutation, String> {
        let body = serde_json::json!({"sourceEventId":Self::id(source),"operation":{"kind":"forget","memoryId":memory_id,"expectedRevision":revision,"reason":"shared lifecycle forgetting"}});
        let (status, value) = source_request(self.router.clone().unwrap(), "assistant", body).await;
        if status != 200 {
            return Err(format!("HTTP {status}: {value}"));
        }
        Self::lifecycle_receipt(memory_id, value)
    }
    async fn forget_as_other(
        &mut self,
        scope: anima_harness::conformance::OtherHostScope,
        memory_id: &str,
        revision: u64,
    ) -> Result<Option<anima_harness::conformance::PreferenceMutation>, String> {
        let (router, agent, foreign) = self.lifecycle_scope(scope).await?;
        let body = serde_json::json!({"sourceEventId":Uuid::new_v4(),"operation":{"kind":"forget","memoryId":memory_id,"expectedRevision":revision,"reason":"foreign lifecycle forgetting"}});
        let (status, value) = source_request(router, agent, body).await;
        if let Some(foreign) = foreign {
            foreign.shutdown().await.map_err(|e| e.to_string())?;
        }
        if status == 403 || status == 404 {
            return Ok(None);
        }
        if status != 200 {
            return Err(format!(
                "foreign forgetting failed for an unrelated reason: HTTP {status}: {value}"
            ));
        }
        Self::lifecycle_receipt(memory_id, value).map(Some)
    }
}
#[tokio::test]
async fn engine_bridge_shared_lifecycle_contract_uses_actual_authenticated_http() {
    let mut fixture = HttpFixture::new().await;
    let result = anima_harness::conformance::assert_persistent_host_lifecycle(&mut fixture).await;
    fixture.close().await.unwrap();
    result.unwrap();
}

#[tokio::test]
async fn engine_bridge_startup_draining_replays_exact_receipts_after_reopen() {
    let mut f = HttpFixture::new().await;
    let run = Uuid::new_v4();
    let room = Uuid::new_v4();
    let (status, accepted) = request(
        f.router.clone().unwrap(),
        "assistant",
        run,
        room,
        "I prefer tea",
    )
    .await;
    assert_eq!(status, 200);
    f.close().await.unwrap();
    f.open().await.unwrap();
    f.bridge
        .as_ref()
        .unwrap()
        .set_admission("assistant", false)
        .unwrap();
    let (status, replayed) = request(
        f.router.clone().unwrap(),
        "assistant",
        run,
        room,
        "I prefer tea",
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(accepted, replayed);
    assert_eq!(
        request(f.router.clone().unwrap(), "assistant", run, room, "changed")
            .await
            .0,
        409
    );
    assert_eq!(
        request(
            f.router.clone().unwrap(),
            "assistant",
            Uuid::new_v4(),
            room,
            "new"
        )
        .await
        .0,
        503
    );
    assert_eq!(f.calls.lock().unwrap().len(), 1);
    assert!(!f
        .bridge
        .as_ref()
        .unwrap()
        .recall("assistant", "tea")
        .await
        .unwrap()
        .is_empty());
    f.close().await.unwrap();
}
#[tokio::test]
async fn engine_bridge_startup_shutdown_waits_for_reserved_detached_worker() {
    let mut f = HttpFixture::new().await;
    let bridge = f.bridge.clone().unwrap();
    let permit = bridge.track().unwrap();
    bridge.close_workers();
    assert!(bridge.track().is_err());
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(20), bridge.shutdown())
            .await
            .is_err()
    );
    assert!(
        bridge.outbox.profiles().await.is_ok(),
        "deadline must not close storage under a worker"
    );
    drop(permit);
    bridge.shutdown().await.unwrap();
    f.bridge = None;
    f.close().await.unwrap();
}

#[tokio::test]
async fn engine_bridge_startup_shutdown_preserves_externally_shared_stores() {
    let mut f = HttpFixture::new().await;
    f.bridge.as_ref().unwrap().shutdown().await.unwrap();
    assert!(f
        .execution
        .as_ref()
        .unwrap()
        .load_run(f.owner, Uuid::new_v4())
        .await
        .is_ok());
    assert!(f.memory.as_ref().unwrap().store_id().await.is_ok());
    f.bridge = None;
    f.close().await.unwrap();
}
