//! M38 owner-route checks. Reuse Anima's router, settled-run/history fixtures
//! and temporary stores; no upstream implementation is copied into these tests.

use super::*;

use anima_core::{MessageRole, ModelStreamFrame, ModelStreamSink};
use serde_json::{json, Value};
use std::collections::VecDeque;
use utoipa::OpenApi;

use crate::agent_runs::test_support::Gate;
use crate::control_plane_store::{load_control_plane_snapshot, ControlPlaneStoreConfig};
use crate::history::conformance::{history_message, terminal_run};
use crate::history::{HistoryService, SqliteHistoryStore};
use crate::runs::{ActionDurability, ActionReceipt, ActionStatus, RunRecord};
use crate::sessions::{SessionKind, SessionOrigin, SessionRecord, TitleSource};

const OWNER_ORIGIN: &str = "http://localhost:4200";
const SESSION: &str = "chat:learning";
const OBSERVATION: &str = "The accepted reply records the corrected arithmetic.";
const SUGGESTION: &str =
    "Check the arithmetic before presenting a result; verify the next task separately.";

#[derive(Default)]
struct AdviceBehavior {
    usage: Option<TokenUsage>,
    partial_usage: Option<TokenUsage>,
    text: Option<String>,
    tool_call: bool,
    panic: bool,
}

/// A single bounded advice response over the actual server-selected brief.
/// Both adapter entry points count once and stream propagates sink errors.
struct AdviceModel {
    calls: AtomicUsize,
    requests: StdMutex<Vec<(AgentConfig, ModelGenerateRequest)>>,
    gate: Option<Gate>,
    behaviors: StdMutex<VecDeque<AdviceBehavior>>,
}

impl AdviceModel {
    fn new(gate: Option<Gate>) -> Arc<Self> {
        Arc::new(Self {
            calls: AtomicUsize::new(0),
            requests: StdMutex::new(Vec::new()),
            gate,
            behaviors: StdMutex::new(VecDeque::new()),
        })
    }

    fn script(&self, behaviors: Vec<AdviceBehavior>) {
        self.behaviors.lock().unwrap().extend(behaviors);
    }

    async fn answer(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
        sink: Option<&dyn ModelStreamSink>,
    ) -> Result<ModelGenerateResponse, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.requests
            .lock()
            .unwrap()
            .push((config.clone(), request.clone()));
        let behavior = self
            .behaviors
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_default();
        if let (Some(sink), Some(usage)) = (sink, &behavior.partial_usage) {
            sink.emit(ModelStreamFrame::Usage(usage.clone())).await?;
        }
        if let Some(gate) = &self.gate {
            gate.entered.add_permits(1);
            gate.release
                .acquire()
                .await
                .map_err(|_| "fixture gate closed".to_string())?
                .forget();
        }
        if behavior.panic {
            panic!("injected learning adapter panic");
        }
        let brief: Value = serde_json::from_str(
            &request
                .messages
                .first()
                .ok_or_else(|| "the review request must contain its brief".to_string())?
                .content
                .text,
        )
        .map_err(|_| "the review brief must be JSON".to_string())?;
        let anchor = brief["anchors"]
            .as_array()
            .and_then(|anchors| anchors.first())
            .and_then(Value::as_str)
            .ok_or_else(|| "the review brief must supply an anchor".to_string())?;
        Ok(ModelGenerateResponse {
            content: Content {
                text: behavior.text.unwrap_or_else(|| {
                    json!({"proposals": [{
                        "observation": OBSERVATION,
                        "suggestion": SUGGESTION,
                        "anchors": [anchor]
                    }]})
                    .to_string()
                }),
                ..Content::default()
            },
            tool_calls: behavior.tool_call.then(|| {
                vec![anima_core::ToolCall {
                    id: "untrusted-review-tool".into(),
                    name: "memory_add".into(),
                    args: std::collections::BTreeMap::new(),
                }]
            }),
            usage: behavior.usage.unwrap_or(TokenUsage {
                prompt_tokens: 19,
                completion_tokens: 7,
                total_tokens: 26,
                ..TokenUsage::default()
            }),
            stop_reason: ModelStopReason::End,
        })
    }
}

#[async_trait]
impl ModelAdapter for AdviceModel {
    fn single_attempt_contract(&self) -> bool {
        true
    }
    fn generated_usage_is_complete(&self, response: &anima_core::ModelGenerateResponse) -> bool {
        // This fixture deliberately uses default counters to exercise missing usage.
        response.usage != anima_core::TokenUsage::default()
    }
    fn provider(&self) -> &str {
        "learning-route-fixture"
    }

    async fn generate(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        self.answer(config, request, None).await
    }

    async fn stream(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
        sink: &dyn ModelStreamSink,
    ) -> Result<(), String> {
        let response = self.answer(config, request, Some(sink)).await?;
        if self.generated_usage_is_complete(&response) {
            sink.complete_usage(response.usage.clone()).await?;
        }
        sink.emit(ModelStreamFrame::Final(response)).await
    }
}

/// Exercises the core default stream, including its ignored Final sink error.
struct FinalOnlyAdvice(Arc<AdviceModel>);

#[async_trait]
impl ModelAdapter for FinalOnlyAdvice {
    fn single_attempt_contract(&self) -> bool {
        true
    }
    fn generated_usage_is_complete(&self, response: &anima_core::ModelGenerateResponse) -> bool {
        // This fixture deliberately uses default counters to exercise missing usage.
        response.usage != anima_core::TokenUsage::default()
    }
    fn provider(&self) -> &str {
        self.0.provider()
    }
    async fn generate(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        self.0.answer(config, request, None).await
    }
}

struct Fixture {
    app: axum::Router,
    state: Arc<RwLock<DaemonState>>,
    agent: String,
    source: RunRecord,
    model: Arc<AdviceModel>,
    root: PathBuf,
}

impl Fixture {
    async fn new(durable: bool, gate: Option<Gate>) -> Self {
        let root =
            std::env::temp_dir().join(format!("anima-learning-route-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let model = AdviceModel::new(gate);
        let mut daemon = DaemonState::with_model_adapter(model.clone());
        let agent = daemon
            .create_agent(test_config("learning companion"))
            .unwrap()
            .state
            .id;
        daemon.sessions.insert(SessionRecord::new(
            &agent,
            SESSION,
            SessionKind::Chat,
            SessionOrigin::Web,
            "Arithmetic correction".into(),
            TitleSource::Owner,
            1,
        ));
        let mut source = terminal_run(&agent, SESSION);
        source.input.text = "Calculate 2 + 2 and check the answer.".into();
        source.reply_message_id = Some("learning-accepted-reply".into());
        source.tools_started = vec!["calculate".into()];
        source.action_receipts.push(ActionReceipt {
            tool_call_id: "learning-calculate".into(),
            name: "calculate".into(),
            status: ActionStatus::Succeeded,
            started_at_ms: 101,
            finished_at_ms: Some(102),
            duration_ms: Some(1),
            arguments_preview: "PRIVATE_ARGUMENTS_MUST_NOT_BE_IN_BRIEF".into(),
            arguments_truncated: false,
            result_preview: Some("4".into()),
            result_truncated: false,
            durability: ActionDurability::Persistent,
        });
        daemon.runs.insert(source.clone());
        if durable {
            let store = SqliteHistoryStore::open(root.join("history.sqlite"))
                .await
                .unwrap();
            daemon.set_history(HistoryService::new(Arc::new(store)));
            daemon.set_control_plane_store(Some(ControlPlaneStoreConfig::Json(
                root.join("control-plane.json"),
            )));
        }
        daemon
            .history
            .store()
            .upsert_runs(std::slice::from_ref(&source))
            .await
            .unwrap();
        let mut accepted_reply = history_message(
            "learning-accepted-reply",
            &agent,
            SESSION,
            MessageRole::Assistant,
            "Corrected: 2 + 2 = 4. This reply does not prove every task succeeded.",
            110,
        );
        accepted_reply.message.content.metadata = Some(std::collections::BTreeMap::from([(
            "runId".into(),
            anima_core::DataValue::String(source.id.clone()),
        )]));
        daemon
            .history
            .store()
            .upsert_messages(&[accepted_reply])
            .await
            .unwrap();
        daemon.control_plane_persist_request().save().await.unwrap();
        let state = Arc::new(RwLock::new(daemon));
        let app = router(state.clone(), DaemonConfig::default());
        Self {
            app,
            state,
            agent,
            source,
            model,
            root,
        }
    }

    fn policy_uri(&self) -> String {
        format!("/api/agents/{}/learning-policy", self.agent)
    }

    fn list_uri(&self) -> String {
        format!("/api/agents/{}/learning-reviews", self.agent)
    }

    fn request_uri(&self) -> String {
        format!(
            "/api/agents/{}/runs/{}/learning-review",
            self.agent, self.source.id
        )
    }

    fn review_uri(&self, review: &Value) -> String {
        format!("{}/{}", self.list_uri(), review["id"].as_str().unwrap())
    }

    async fn admit(&self) -> Value {
        let response = send(&self.app, "POST", &self.request_uri(), Some(json!({}))).await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        assert_no_store(&response);
        json_body(response).await["review"].clone()
    }

    async fn completed(&self, admitted: &Value) -> Value {
        let uri = self.review_uri(admitted);
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let response = send(&self.app, "GET", &uri, None).await;
                assert_eq!(response.status(), StatusCode::OK);
                assert_no_store(&response);
                let review = json_body(response).await["review"].clone();
                match review["state"].as_str().unwrap() {
                    "completed" => break review,
                    "queued" | "running" => tokio::time::sleep(Duration::from_millis(5)).await,
                    terminal => panic!("review ended as {terminal}: {review}"),
                }
            }
        })
        .await
        .expect("the mock review completes within five seconds")
    }

    async fn at_state(&self, review: &Value, expected: &str) -> Value {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let next = json_body(send(&self.app, "GET", &self.review_uri(review), None).await)
                    .await["review"]
                    .clone();
                if next["state"] == expected {
                    break next;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("the review reaches its expected state within five seconds")
    }

    async fn another_source(&self) -> RunRecord {
        let mut run = self.source.clone();
        run.id = uuid::Uuid::new_v4().to_string();
        let reply_id = format!("learning-reply-{}", run.id);
        run.reply_message_id = Some(reply_id.clone());
        let mut reply = history_message(
            &reply_id,
            &self.agent,
            SESSION,
            MessageRole::Assistant,
            "The second arithmetic result was checked.",
            110,
        );
        reply.message.content.metadata = Some(std::collections::BTreeMap::from([(
            "runId".into(),
            anima_core::DataValue::String(run.id.clone()),
        )]));
        let (history, persist) = {
            let mut guard = self.state.write().await;
            guard.runs.insert(run.clone());
            (guard.history.clone(), guard.control_plane_persist_request())
        };
        history
            .store()
            .upsert_runs(std::slice::from_ref(&run))
            .await
            .unwrap();
        history.store().upsert_messages(&[reply]).await.unwrap();
        persist.save().await.unwrap();
        run
    }

    async fn assert_source_unchanged(&self) {
        let history = self.state.read().await.history.clone();
        assert_eq!(
            history.store().get_run(&self.source.id).await.unwrap(),
            Some(self.source.clone())
        );
        assert_eq!(
            history
                .store()
                .get_message(&self.agent, SESSION, "learning-accepted-reply")
                .await
                .unwrap()
                .unwrap()
                .message
                .content
                .text,
            "Corrected: 2 + 2 = 4. This reply does not prove every task succeeded."
        );
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if self.root.starts_with(std::env::temp_dir())
            && self
                .root
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("anima-learning-route-"))
        {
            std::fs::remove_dir_all(&self.root).ok();
        }
    }
}

fn request(method: &str, uri: &str, origin: &str, body: Option<Value>) -> Request<Body> {
    let builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("host", "127.0.0.1:8080")
        .header("origin", origin);
    match body {
        Some(body) => builder
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    }
}

async fn send(
    app: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> axum::response::Response {
    app.clone()
        .oneshot(request(method, uri, OWNER_ORIGIN, body))
        .await
        .unwrap()
}

fn assert_no_store(response: &axum::response::Response) {
    assert_eq!(response.headers()["cache-control"], "no-store");
}

async fn json_body(response: axum::response::Response) -> Value {
    serde_json::from_slice(&to_bytes(response.into_body(), 128 * 1024).await.unwrap()).unwrap()
}

#[tokio::test]
async fn learning_routes_document_and_enforce_the_owner_before_body_or_source_access() {
    let fixture = Fixture::new(true, None).await;
    let operations = [
        ("GET", fixture.policy_uri()),
        ("PATCH", fixture.policy_uri()),
        ("POST", fixture.request_uri()),
        ("GET", fixture.list_uri()),
        ("GET", format!("{}/missing", fixture.list_uri())),
        ("POST", format!("{}/missing/cancel", fixture.list_uri())),
        (
            "PATCH",
            format!("{}/missing/proposals/missing", fixture.list_uri()),
        ),
        (
            "POST",
            format!("{}/missing/proposals/missing/undo", fixture.list_uri()),
        ),
    ];
    for (method, uri) in operations {
        let denied = fixture
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(&uri)
                    .header("host", "127.0.0.1:8080")
                    .header("origin", "https://foreign.example.invalid")
                    .header("content-type", "application/json")
                    .body(Body::from("{not valid JSON"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(denied.status(), StatusCode::FORBIDDEN, "{method} {uri}");
        assert_no_store(&denied);
        assert_eq!(
            json_body(denied).await["error"],
            "local owner authorization required"
        );
    }
    assert_eq!(fixture.model.calls.load(Ordering::SeqCst), 0);
    let document = serde_json::to_value(crate::routes::ApiDoc::openapi()).unwrap();
    for (path, methods) in [
        (
            "/api/agents/{agent_id}/learning-policy",
            vec!["get", "patch"],
        ),
        (
            "/api/agents/{agent_id}/runs/{run_id}/learning-review",
            vec!["post"],
        ),
        ("/api/agents/{agent_id}/learning-reviews", vec!["get"]),
        (
            "/api/agents/{agent_id}/learning-reviews/{review_id}",
            vec!["get"],
        ),
        (
            "/api/agents/{agent_id}/learning-reviews/{review_id}/cancel",
            vec!["post"],
        ),
        (
            "/api/agents/{agent_id}/learning-reviews/{review_id}/proposals/{proposal_id}",
            vec!["patch"],
        ),
        (
            "/api/agents/{agent_id}/learning-reviews/{review_id}/proposals/{proposal_id}/undo",
            vec!["post"],
        ),
    ] {
        for method in methods {
            assert!(
                document["paths"][path][method].is_object(),
                "missing {method} {path}"
            );
            assert!(document["paths"][path][method]["responses"]["403"].is_object());
        }
    }
}

#[tokio::test]
async fn learning_policy_is_default_off_and_only_the_owner_can_change_the_opt_in() {
    let fixture = Fixture::new(true, None).await;
    let defaults = send(&fixture.app, "GET", &fixture.policy_uri(), None).await;
    assert_eq!(defaults.status(), StatusCode::OK);
    assert_no_store(&defaults);
    assert_eq!(
        json_body(defaults).await,
        json!({
            "mode": "strict", "autoEnabled": false, "dailyLimit": 2, "pendingLimit": 10, "timeoutMs": 20000,
            "physicalReviewerActive": false, "windowMs": 86_400_000, "remaining": 2,
            "nextAvailableAtMs": null, "autoMinActions": 5
        })
    );
    for enabled in [true, false] {
        let saved = send(
            &fixture.app,
            "PATCH",
            &fixture.policy_uri(),
            Some(json!({"autoEnabled": enabled})),
        )
        .await;
        assert_eq!(saved.status(), StatusCode::OK);
        assert_no_store(&saved);
        let expected = json!({
            "mode": "strict", "autoEnabled": enabled, "dailyLimit": 2, "pendingLimit": 10, "timeoutMs": 20000,
            "physicalReviewerActive": false, "windowMs": 86_400_000, "remaining": 2,
            "nextAvailableAtMs": null, "autoMinActions": 5
        });
        assert_eq!(json_body(saved).await, expected);
        assert_eq!(
            json_body(send(&fixture.app, "GET", &fixture.policy_uri(), None).await).await,
            expected
        );
    }
    assert_eq!(fixture.model.calls.load(Ordering::SeqCst), 0);
    fixture.assert_source_unchanged().await;
}

#[tokio::test]
async fn ephemeral_review_admission_is_unavailable_without_a_model_call() {
    let fixture = Fixture::new(false, None).await;
    let unavailable = send(
        &fixture.app,
        "POST",
        &fixture.request_uri(),
        Some(json!({})),
    )
    .await;
    assert_eq!(unavailable.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_no_store(&unavailable);
    assert!(json_body(unavailable).await["error"].as_str().is_some());
    assert_eq!(fixture.model.calls.load(Ordering::SeqCst), 0);
    fixture.assert_source_unchanged().await;
}

#[tokio::test]
async fn completed_own_run_exposes_one_anchored_unverified_advice_record() {
    let fixture = Fixture::new(true, None).await;
    let admitted = fixture.admit().await;
    assert_eq!(admitted["agentId"], fixture.agent);
    assert_eq!(admitted["sourceRunId"], fixture.source.id);
    assert_eq!(admitted["sessionId"], SESSION);
    assert_eq!(admitted["trigger"], "manual");
    assert_eq!(admitted["policyVersion"], 1);
    assert_eq!(admitted["evidenceDigest"].as_str().unwrap().len(), 64);
    assert!(admitted["evidenceBrief"].as_str().unwrap().len() <= 12 * 1024);
    assert!(!admitted["evidenceBrief"]
        .as_str()
        .unwrap()
        .contains("PRIVATE_ARGUMENTS"));
    let completed = fixture.completed(&admitted).await;
    assert_eq!(completed["proposals"].as_array().unwrap().len(), 1);
    let proposal = &completed["proposals"][0];
    assert_eq!(proposal["observation"], OBSERVATION);
    assert_eq!(proposal["suggestion"], SUGGESTION);
    assert_eq!(proposal["decision"], "pending");
    assert_eq!(proposal["decisionVersion"], 0);
    assert!(completed["evidenceAnchors"]
        .as_array()
        .unwrap()
        .contains(&proposal["anchors"][0]));
    assert_eq!(completed["usageComplete"], true);
    assert_eq!(completed["usage"]["promptTokens"], 19);
    assert_eq!(completed["usage"]["completionTokens"], 7);
    assert_eq!(completed["usage"]["totalTokens"], 26);
    assert!(completed["startedAtMs"].as_u64().is_some());
    assert!(completed["finishedAtMs"].as_u64().is_some());
    assert_eq!(fixture.model.calls.load(Ordering::SeqCst), 1);
    let captured = fixture.model.requests.lock().unwrap();
    let (shadow, request) = &captured[0];
    assert!(shadow.tools.as_ref().is_none_or(Vec::is_empty));
    assert_eq!(shadow.settings.as_ref().unwrap().max_retries, Some(0));
    assert_eq!(request.messages.len(), 1);
    let brief: Value = serde_json::from_str(&request.messages[0].content.text).unwrap();
    assert_eq!(brief["sourceRunId"], fixture.source.id);
    assert_eq!(brief["reply"]["messageId"], "learning-accepted-reply");
    assert_eq!(brief["actions"][0]["status"], "succeeded");
    assert_eq!(brief["actions"][0]["durability"], "persistent");
    assert_eq!(brief["actions"][0]["resultContentOmitted"], true);
    drop(captured);
    let listed = send(&fixture.app, "GET", &fixture.list_uri(), None).await;
    assert_eq!(listed.status(), StatusCode::OK);
    assert_no_store(&listed);
    assert_eq!(json_body(listed).await["reviews"], json!([completed]));
    let replay = fixture.admit().await;
    assert_eq!(replay["id"], admitted["id"]);
    assert_eq!(fixture.model.calls.load(Ordering::SeqCst), 1);
    fixture.assert_source_unchanged().await;
}

#[tokio::test]
async fn learning_mutations_reject_unknown_fields_without_starting_generation() {
    let fixture = Fixture::new(true, None).await;
    for (uri, method, body) in [
        (
            fixture.request_uri(),
            "POST",
            json!({"model": "caller-model"}),
        ),
        (fixture.request_uri(), "POST", json!({"policyVersion": 9})),
        (
            fixture.request_uri(),
            "POST",
            json!({"evidenceAnchors": ["other-agent"]}),
        ),
        (
            fixture.policy_uri(),
            "PATCH",
            json!({"autoEnabled": true, "dailyLimit": 20}),
        ),
        (
            fixture.policy_uri(),
            "PATCH",
            json!({"autoEnabled": true, "extra": true}),
        ),
    ] {
        let response = send(&fixture.app, method, &uri, Some(body)).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{method} {uri}");
        assert_no_store(&response);
    }
    assert_eq!(fixture.model.calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        json_body(send(&fixture.app, "GET", &fixture.policy_uri(), None).await).await
            ["autoEnabled"],
        false
    );
    assert_eq!(
        json_body(send(&fixture.app, "GET", &fixture.list_uri(), None).await).await["reviews"],
        json!([])
    );
}

#[tokio::test]
async fn foreign_source_and_review_ids_never_cross_the_agent_boundary() {
    let fixture = Fixture::new(true, None).await;
    let other = fixture
        .state
        .write()
        .await
        .create_agent(test_config("other companion"))
        .unwrap()
        .state
        .id;
    let foreign_source = format!(
        "/api/agents/{other}/runs/{}/learning-review",
        fixture.source.id
    );
    let denied = send(&fixture.app, "POST", &foreign_source, Some(json!({}))).await;
    assert_eq!(denied.status(), StatusCode::NOT_FOUND);
    assert_no_store(&denied);
    assert_eq!(fixture.model.calls.load(Ordering::SeqCst), 0);
    let completed = fixture.completed(&fixture.admit().await).await;
    let other_list = format!("/api/agents/{other}/learning-reviews");
    assert_eq!(
        json_body(send(&fixture.app, "GET", &other_list, None).await).await["reviews"],
        json!([])
    );
    let item = format!("{other_list}/{}", completed["id"].as_str().unwrap());
    for (method, uri, body) in [
        ("GET", item.clone(), None),
        ("POST", format!("{item}/cancel"), Some(json!({}))),
        (
            "PATCH",
            format!(
                "{item}/proposals/{}",
                completed["proposals"][0]["id"].as_str().unwrap()
            ),
            Some(json!({"decision": "acknowledged", "expectedVersion": 0})),
        ),
    ] {
        let response = send(&fixture.app, method, &uri, body).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{method} {uri}");
        assert_no_store(&response);
    }
    assert_eq!(fixture.model.calls.load(Ordering::SeqCst), 1);
    fixture.assert_source_unchanged().await;
}

#[tokio::test]
async fn proposal_decisions_require_exact_versions_and_survive_reload_without_generation() {
    for decision in ["acknowledged", "dismissed"] {
        let fixture = Fixture::new(true, None).await;
        let before =
            serde_json::to_value(fixture.state.read().await.control_plane_snapshot()).unwrap();
        let completed = fixture.completed(&fixture.admit().await).await;
        let item = fixture.review_uri(&completed);
        let proposal = completed["proposals"][0]["id"].as_str().unwrap();
        let decision_uri = format!("{item}/proposals/{proposal}");
        for invalid in [
            json!({"decision": "pending", "expectedVersion": 0}),
            json!({"decision": decision, "expectedVersion": 0, "extra": true}),
        ] {
            let response = send(&fixture.app, "PATCH", &decision_uri, Some(invalid)).await;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            assert_no_store(&response);
        }
        let changed = send(
            &fixture.app,
            "PATCH",
            &decision_uri,
            Some(json!({
                "decision": decision, "expectedVersion": 0
            })),
        )
        .await;
        assert_eq!(changed.status(), StatusCode::OK);
        assert_no_store(&changed);
        let changed = json_body(changed).await["review"].clone();
        assert_eq!(changed["proposals"][0]["decision"], decision);
        assert_eq!(changed["proposals"][0]["decisionVersion"], 1);
        let stale = send(
            &fixture.app,
            "PATCH",
            &decision_uri,
            Some(json!({
                "decision": decision, "expectedVersion": 0
            })),
        )
        .await;
        assert_eq!(stale.status(), StatusCode::CONFLICT);
        assert_no_store(&stale);
        assert_eq!(
            json_body(send(&fixture.app, "GET", &item, None).await).await["review"],
            changed
        );
        let after =
            serde_json::to_value(fixture.state.read().await.control_plane_snapshot()).unwrap();
        for key in [
            "agents",
            "skills",
            "skillDrafts",
            "approvalPolicies",
            "approvalRules",
        ] {
            assert_eq!(
                after[key], before[key],
                "advice decisions must preserve {key}"
            );
        }
        let config = ControlPlaneStoreConfig::Json(fixture.root.join("control-plane.json"));
        let snapshot = load_control_plane_snapshot(&config).await.unwrap().unwrap();
        let restarted_model = AdviceModel::new(None);
        let mut restarted = DaemonState::with_model_adapter(restarted_model.clone());
        restarted.set_history(fixture.state.read().await.history.clone());
        restarted.restore_control_plane_snapshot(snapshot).unwrap();
        restarted.set_control_plane_store(Some(config));
        let restarted = router(Arc::new(RwLock::new(restarted)), DaemonConfig::default());
        let reloaded = send(&restarted, "GET", &item, None).await;
        assert_eq!(reloaded.status(), StatusCode::OK);
        assert_no_store(&reloaded);
        assert_eq!(json_body(reloaded).await["review"], changed);
        assert_eq!(restarted_model.calls.load(Ordering::SeqCst), 0);
        assert_eq!(fixture.model.calls.load(Ordering::SeqCst), 1);
        fixture.assert_source_unchanged().await;
    }
}

#[tokio::test]
async fn owner_cancellation_and_strict_empty_body_leave_the_source_task_unchanged() {
    let gate = Gate::new();
    let fixture = Fixture::new(true, Some(gate.clone())).await;
    let admitted = fixture.admit().await;
    gate.entered().await;
    let cancel_uri = format!("{}/cancel", fixture.review_uri(&admitted));
    let invalid = send(
        &fixture.app,
        "POST",
        &cancel_uri,
        Some(json!({"retry": true})),
    )
    .await;
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    assert_no_store(&invalid);
    let cancelled = send(&fixture.app, "POST", &cancel_uri, Some(json!({}))).await;
    assert_eq!(cancelled.status(), StatusCode::OK);
    assert_no_store(&cancelled);
    let cancelled = json_body(cancelled).await["review"].clone();
    assert_eq!(cancelled["state"], "cancelled");
    assert_eq!(cancelled["proposals"], json!([]));
    gate.release();
    let read = send(&fixture.app, "GET", &fixture.review_uri(&admitted), None).await;
    assert_eq!(read.status(), StatusCode::OK);
    assert_no_store(&read);
    assert_eq!(json_body(read).await["review"]["state"], "cancelled");
    assert_eq!(fixture.model.calls.load(Ordering::SeqCst), 1);
    fixture.assert_source_unchanged().await;
}

#[tokio::test]
async fn read_only_referrer_authorization_never_authorizes_learning_mutations() {
    let fixture = Fixture::new(true, None).await;
    let originless = |method: &str, uri: &str, body: Value| {
        Request::builder()
            .method(method)
            .uri(uri)
            .header("host", "127.0.0.1:8080")
            .header("referer", format!("{OWNER_ORIGIN}/settings"))
            .header("sec-fetch-site", "same-origin")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    };
    for (method, uri, body) in [
        ("POST", fixture.request_uri(), json!({})),
        ("PATCH", fixture.policy_uri(), json!({"autoEnabled": true})),
        (
            "POST",
            format!("{}/missing/cancel", fixture.list_uri()),
            json!({}),
        ),
        (
            "PATCH",
            format!("{}/missing/proposals/missing", fixture.list_uri()),
            json!({"decision": "acknowledged", "expectedVersion": 0}),
        ),
    ] {
        let response = fixture
            .app
            .clone()
            .oneshot(originless(method, &uri, body))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{method} {uri}");
        assert_no_store(&response);
    }
    let read = fixture
        .app
        .clone()
        .oneshot(originless("GET", &fixture.policy_uri(), json!({})))
        .await
        .unwrap();
    assert_eq!(
        read.status(),
        StatusCode::OK,
        "the read-only shortcut remains usable"
    );
    assert_eq!(json_body(read).await["autoEnabled"], false);
    assert_eq!(fixture.model.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn default_zero_final_usage_is_unknown_and_preserves_reported_partial_usage() {
    for partial in [false, true] {
        let fixture = Fixture::new(true, None).await;
        fixture.model.script(vec![AdviceBehavior {
            usage: Some(TokenUsage::default()),
            partial_usage: partial.then_some(TokenUsage {
                prompt_tokens: 11,
                total_tokens: 11,
                ..TokenUsage::default()
            }),
            ..AdviceBehavior::default()
        }]);
        if !partial {
            fixture.state.write().await.model_adapter =
                Arc::new(FinalOnlyAdvice(fixture.model.clone()));
        }
        let completed = fixture.completed(&fixture.admit().await).await;
        assert_eq!(completed["usageComplete"], false);
        if partial {
            assert_eq!(completed["usage"]["promptTokens"], 11);
            assert_eq!(completed["usage"]["completionTokens"], 0);
            assert_eq!(completed["usage"]["totalTokens"], 11);
        } else {
            assert_eq!(completed["usage"], Value::Null);
        }
        assert_eq!(fixture.model.calls.load(Ordering::SeqCst), 1);
        fixture.assert_source_unchanged().await;
    }
}

#[tokio::test]
async fn an_adapter_panic_settles_once_and_drains_the_already_admitted_second_review() {
    let gate = Gate::new();
    let fixture = Fixture::new(true, Some(gate.clone())).await;
    fixture.model.script(vec![AdviceBehavior {
        panic: true,
        ..AdviceBehavior::default()
    }]);
    let first = fixture.admit().await;
    gate.entered().await;
    let second_source = fixture.another_source().await;
    let uri = format!(
        "/api/agents/{}/runs/{}/learning-review",
        fixture.agent, second_source.id
    );
    let second = send(&fixture.app, "POST", &uri, Some(json!({}))).await;
    assert_eq!(second.status(), StatusCode::ACCEPTED);
    let second = json_body(second).await["review"].clone();
    assert_eq!(second["state"], "queued");
    assert_eq!(fixture.model.calls.load(Ordering::SeqCst), 1);
    gate.release();
    gate.entered().await;
    let failed = fixture.at_state(&first, "failed").await;
    assert_eq!(failed["error"]["code"], "review_generation_failed");
    assert_eq!(failed["proposals"], json!([]));
    gate.release();
    fixture.completed(&second).await;
    assert_eq!(fixture.model.calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        fixture.admit().await["id"],
        first["id"],
        "panic does not create a retry attempt"
    );
    assert_eq!(fixture.model.calls.load(Ordering::SeqCst), 2);
    fixture.assert_source_unchanged().await;
}

#[tokio::test]
async fn timeout_and_cancel_keep_the_physical_slot_until_exit_and_retain_partial_usage() {
    for cancel in [false, true] {
        let gate = Gate::new();
        let fixture = Fixture::new(true, Some(gate.clone())).await;
        fixture.model.script(vec![AdviceBehavior {
            usage: Some(TokenUsage::default()),
            partial_usage: Some(TokenUsage {
                prompt_tokens: 9,
                total_tokens: 9,
                ..TokenUsage::default()
            }),
            ..AdviceBehavior::default()
        }]);
        let service = crate::learning::LearningService::new(
            fixture.state.clone(),
            Arc::new(tokio::sync::Mutex::new(())),
        )
        .with_timeout(Duration::from_millis(250));
        let first = serde_json::to_value(
            service
                .request_review(&fixture.agent, &fixture.source.id)
                .await
                .unwrap(),
        )
        .unwrap();
        gate.entered().await;
        let second_source = fixture.another_source().await;
        let second = serde_json::to_value(
            service
                .request_review(&fixture.agent, &second_source.id)
                .await
                .unwrap(),
        )
        .unwrap();
        let expected = if cancel { "cancelled" } else { "failed" };
        if cancel {
            let response = send(
                &fixture.app,
                "POST",
                &format!("{}/cancel", fixture.review_uri(&first)),
                Some(json!({})),
            )
            .await;
            assert_eq!(response.status(), StatusCode::OK);
        }
        let settled = fixture.at_state(&first, expected).await;
        if !cancel {
            assert_eq!(settled["error"]["code"], "review_timeout");
        }
        assert!(
            service.is_active().await,
            "a facade terminal state does not free the physical request"
        );
        assert_eq!(
            json_body(send(&fixture.app, "GET", &fixture.policy_uri(), None).await).await
                ["physicalReviewerActive"],
            true
        );
        assert_eq!(fixture.model.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            json_body(send(&fixture.app, "GET", &fixture.review_uri(&second), None).await).await
                ["review"]["state"],
            "queued"
        );
        gate.release();
        gate.entered().await;
        let late = fixture.at_state(&first, expected).await;
        assert_eq!(late["proposals"], json!([]));
        assert_eq!(late["usage"]["totalTokens"], 9);
        assert_eq!(late["usageComplete"], false);
        gate.release();
        fixture.completed(&second).await;
        let third_source = fixture.another_source().await;
        let rejected = send(
            &fixture.app,
            "POST",
            &format!(
                "/api/agents/{}/runs/{}/learning-review",
                fixture.agent, third_source.id
            ),
            Some(json!({})),
        )
        .await;
        assert_eq!(
            rejected.status(),
            StatusCode::CONFLICT,
            "cancel/timeout never refunds the shared reservation"
        );
        assert_no_store(&rejected);
        assert_eq!(fixture.model.calls.load(Ordering::SeqCst), 2);
        fixture.assert_source_unchanged().await;
    }
}

#[tokio::test]
async fn default_stream_sink_rejections_and_invalid_advice_never_publish_or_change_canonical_state()
{
    let invalid = |observation: &str, anchor: &str| {
        json!({"proposals":[{
            "observation": observation, "suggestion": SUGGESTION, "anchors": [anchor]
        }]})
        .to_string()
    };
    for behavior in [
        AdviceBehavior {
            tool_call: true,
            ..AdviceBehavior::default()
        },
        AdviceBehavior {
            text: Some("x".repeat(6 * 1024 + 1)),
            ..AdviceBehavior::default()
        },
        AdviceBehavior {
            text: Some("not JSON".into()),
            ..AdviceBehavior::default()
        },
        AdviceBehavior {
            text: Some(invalid(OBSERVATION, "foreign-anchor")),
            ..AdviceBehavior::default()
        },
        AdviceBehavior {
            text: Some("{\"proposals\":[],\"extra\":true}".into()),
            ..AdviceBehavior::default()
        },
        AdviceBehavior {
            text: Some(invalid("hidden\u{202e}text", "reply")),
            ..AdviceBehavior::default()
        },
        AdviceBehavior {
            text: Some(invalid("control\u{1}text", "reply")),
            ..AdviceBehavior::default()
        },
    ] {
        let fixture = Fixture::new(true, None).await;
        let before =
            serde_json::to_value(fixture.state.read().await.control_plane_snapshot()).unwrap();
        fixture.model.script(vec![behavior]);
        fixture.state.write().await.model_adapter =
            Arc::new(FinalOnlyAdvice(fixture.model.clone()));
        let failed = fixture.at_state(&fixture.admit().await, "failed").await;
        assert_eq!(failed["error"]["code"], "review_generation_failed");
        assert_eq!(failed["proposals"], json!([]));
        assert_eq!(fixture.model.calls.load(Ordering::SeqCst), 1);
        let after =
            serde_json::to_value(fixture.state.read().await.control_plane_snapshot()).unwrap();
        for key in [
            "agents",
            "skills",
            "skillDrafts",
            "approvalPolicies",
            "approvalRules",
        ] {
            assert_eq!(after[key], before[key], "rejected review preserves {key}");
        }
        fixture.assert_source_unchanged().await;
    }
}

#[tokio::test]
async fn the_model_receives_only_byte_bounded_source_excerpts_and_eight_omitted_body_receipts() {
    let fixture = Fixture::new(true, None).await;
    let mut source = fixture.source.clone();
    source.id = uuid::Uuid::new_v4().to_string();
    source.input.text = "🦀".repeat(1000);
    let reply_id = format!("bounded-reply-{}", source.id);
    source.reply_message_id = Some(reply_id.clone());
    for index in 1..10 {
        let mut receipt = source.action_receipts[0].clone();
        receipt.tool_call_id = format!("bounded-receipt-{index}");
        receipt.arguments_preview = "SECRET_ARGUMENT_BODY".repeat(500);
        receipt.result_preview = Some("SECRET_RESULT_BODY".repeat(500));
        source.action_receipts.push(receipt);
    }
    let mut reply = history_message(
        &reply_id,
        &fixture.agent,
        SESSION,
        MessageRole::Assistant,
        &"🦀".repeat(2000),
        110,
    );
    reply.message.content.metadata = Some(std::collections::BTreeMap::from([(
        "runId".into(),
        anima_core::DataValue::String(source.id.clone()),
    )]));
    let (history, persist) = {
        let mut guard = fixture.state.write().await;
        guard.runs.insert(source.clone());
        (guard.history.clone(), guard.control_plane_persist_request())
    };
    history
        .store()
        .upsert_runs(std::slice::from_ref(&source))
        .await
        .unwrap();
    history.store().upsert_messages(&[reply]).await.unwrap();
    persist.save().await.unwrap();
    let response = send(
        &fixture.app,
        "POST",
        &format!(
            "/api/agents/{}/runs/{}/learning-review",
            fixture.agent, source.id
        ),
        Some(json!({})),
    )
    .await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let completed = fixture
        .completed(&json_body(response).await["review"])
        .await;
    let brief_text = completed["evidenceBrief"].as_str().unwrap();
    assert!(brief_text.len() <= 12 * 1024);
    assert!(
        !brief_text.contains("SECRET_ARGUMENT_BODY") && !brief_text.contains("SECRET_RESULT_BODY")
    );
    let brief: Value = serde_json::from_str(brief_text).unwrap();
    assert!(brief["input"]["text"].as_str().unwrap().len() <= 1536);
    assert!(brief["reply"]["text"].as_str().unwrap().len() <= 3072);
    assert_eq!(brief["input"]["shortened"], true);
    assert_eq!(brief["reply"]["shortened"], true);
    assert_eq!(brief["receiptsShortened"], true);
    assert_eq!(brief["actions"].as_array().unwrap().len(), 8);
    for receipt in brief["actions"].as_array().unwrap() {
        assert!(receipt.to_string().len() <= 512);
        assert_eq!(receipt["resultContentOmitted"], true);
    }
    assert_eq!(
        history.store().get_run(&source.id).await.unwrap(),
        Some(source)
    );
    assert_eq!(fixture.model.calls.load(Ordering::SeqCst), 1);
    fixture.assert_source_unchanged().await;
}

#[tokio::test]
async fn an_unacknowledged_reservation_save_starts_no_model_and_keeps_its_attempt_key() {
    let fixture = Fixture::new(true, None).await;
    let blocked = fixture.root.join("blocked-parent");
    std::fs::write(&blocked, b"a file cannot contain a snapshot").unwrap();
    fixture
        .state
        .write()
        .await
        .set_control_plane_store(Some(ControlPlaneStoreConfig::Json(
            blocked.join("control-plane.json"),
        )));
    let response = send(
        &fixture.app,
        "POST",
        &fixture.request_uri(),
        Some(json!({})),
    )
    .await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_no_store(&response);
    let rows = json_body(send(&fixture.app, "GET", &fixture.list_uri(), None).await).await;
    assert_eq!(rows["reviews"].as_array().unwrap().len(), 1);
    assert_eq!(rows["reviews"][0]["state"], "failed");
    assert_eq!(rows["reviews"][0]["error"]["code"], "review_storage_failed");
    assert_eq!(fixture.admit().await["id"], rows["reviews"][0]["id"]);
    assert_eq!(fixture.model.calls.load(Ordering::SeqCst), 0);
    fixture.assert_source_unchanged().await;
}

async fn learning_skill_fixture() -> (Fixture, Value) {
    let fixture = Fixture::new(true, None).await;
    let persist = {
        let mut guard = fixture.state.write().await;
        guard.workspace = Some(crate::control_plane_store::WorkspaceConfig {
            root_path: fixture.root.clone(),
            company_name: "Learning fixture".into(),
            mission: "Inspect advice before adopting it".into(),
            values: vec![],
        });
        guard.control_plane_persist_request()
    };
    persist.save().await.unwrap();
    let review = fixture.completed(&fixture.admit().await).await;
    (fixture, review)
}

fn learning_skill_uri(fixture: &Fixture, review: &Value) -> String {
    format!(
        "{}/proposals/{}/skill-draft",
        fixture.review_uri(review),
        review["proposals"][0]["id"].as_str().unwrap()
    )
}

fn learning_skill_input(review: &Value) -> Value {
    json!({
        "expectedVersion": review["proposals"][0]["decisionVersion"],
        "evidenceDigest": review["evidenceDigest"],
        "skill": {
            "slug": "arithmetic-check", "name": "Arithmetic check",
            "description": "Check arithmetic before presenting a result",
            "body": "Calculate the result, check it independently, and distinguish that check from broader task success.",
            "expectedHash": null
        }
    })
}

/// M39 HTTP RED uses a settled real owner/source boundary and a valid existing
/// SkillDraft contract. The owner creates a pending draft, never an active file.
#[tokio::test]
async fn m39_reviewed_advice_handoff_creates_a_pending_draft_without_writing_skill_file() {
    let (fixture, review) = learning_skill_fixture().await;
    let body = learning_skill_input(&review);
    let path = fixture.root.join("skills/arithmetic-check/SKILL.md");
    assert!(!path.exists());
    let response = send(
        &fixture.app,
        "POST",
        &learning_skill_uri(&fixture, &review),
        Some(body.clone()),
    )
    .await;
    assert_eq!(
        response.status(),
        StatusCode::CREATED,
        "a reviewed own proposal must create a pending skill draft"
    );
    assert_no_store(&response);
    let draft = json_body(response).await["draft"].clone();
    assert_eq!(draft["status"], "pending");
    assert_eq!(draft["source"], "agent");
    assert_eq!(draft["slug"], body["skill"]["slug"]);
    assert_eq!(draft["body"], body["skill"]["body"]);
    assert_eq!(draft["baseHash"], Value::Null);
    assert_eq!(draft["proposedBy"]["runId"], fixture.source.id);
    assert_eq!(
        draft["learningSource"],
        json!({
            "agentId": fixture.agent, "reviewId": review["id"],
            "proposalId": review["proposals"][0]["id"], "sourceRunId": fixture.source.id,
            "sessionId": SESSION, "evidenceDigest": review["evidenceDigest"], "decisionVersion": 0
        })
    );
    assert!(!path.exists(), "handoff must not write or approve SKILL.md");
    let current = json_body(send(&fixture.app, "GET", &fixture.review_uri(&review), None).await)
        .await["review"]
        .clone();
    assert_eq!(current["proposals"][0]["skillDraftId"], draft["id"]);
    assert_eq!(current["proposals"][0]["decision"], "pending");
    assert_eq!(current["proposals"][0]["decisionVersion"], 0);
    assert_eq!(fixture.model.calls.load(Ordering::SeqCst), 1);
    fixture.assert_source_unchanged().await;
}

async fn learning_skill_draft(fixture: &Fixture, review: &Value, input: Value) -> Value {
    let response = send(
        &fixture.app,
        "POST",
        &learning_skill_uri(fixture, review),
        Some(input),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    assert_no_store(&response);
    json_body(response).await["draft"].clone()
}

async fn change_learning_source_input(fixture: &Fixture) {
    let persist = {
        let mut guard = fixture.state.write().await;
        let mut changed = guard
            .runs
            .get(&fixture.source.id)
            .cloned()
            .unwrap_or_else(|| fixture.source.clone());
        changed
            .input
            .text
            .push_str(" Source changed after its review.");
        guard.runs.insert(changed);
        guard.control_plane_persist_request()
    };
    persist.save().await.unwrap();
}

async fn pending_skill_drafts(app: &axum::Router) -> Vec<Value> {
    let response = send(app, "GET", "/api/skill-drafts", None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_no_store(&response);
    json_body(response).await["drafts"]
        .as_array()
        .unwrap()
        .clone()
}

#[tokio::test]
async fn m39_handoff_requires_write_authority_and_strict_explicit_nullable_input() {
    let (fixture, review) = learning_skill_fixture().await;
    let uri = learning_skill_uri(&fixture, &review);
    let input = learning_skill_input(&review);
    let foreign = fixture
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&uri)
                .header("host", "127.0.0.1:8080")
                .header("origin", "https://foreign.example.invalid")
                .header("content-type", "application/json")
                .body(Body::from("{invalid JSON"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(foreign.status(), StatusCode::FORBIDDEN);
    assert_no_store(&foreign);
    let read_only = fixture
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&uri)
                .header("host", "127.0.0.1:8080")
                .header("referer", format!("{OWNER_ORIGIN}/settings"))
                .header("sec-fetch-site", "same-origin")
                .header("content-type", "application/json")
                .body(Body::from(input.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(read_only.status(), StatusCode::FORBIDDEN);
    assert_no_store(&read_only);
    let mut outer_unknown = input.clone();
    outer_unknown["autoApprove"] = json!(true);
    let mut skill_unknown = input.clone();
    skill_unknown["skill"]["grants"] = json!(["memory_add"]);
    let mut hash_missing = input.clone();
    hash_missing["skill"]
        .as_object_mut()
        .unwrap()
        .remove("expectedHash");
    let mut version_null = input.clone();
    version_null["expectedVersion"] = Value::Null;
    let mut digest_null = input.clone();
    digest_null["evidenceDigest"] = Value::Null;
    for invalid in [
        Value::Null,
        outer_unknown,
        skill_unknown,
        hash_missing,
        version_null,
        digest_null,
    ] {
        let response = send(&fixture.app, "POST", &uri, Some(invalid)).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_no_store(&response);
    }
    assert!(pending_skill_drafts(&fixture.app).await.is_empty());
    assert!(!fixture
        .root
        .join("skills/arithmetic-check/SKILL.md")
        .exists());
    assert_eq!(fixture.model.calls.load(Ordering::SeqCst), 1);
    fixture.assert_source_unchanged().await;
}

#[tokio::test]
async fn m39_stale_foreign_dismissed_and_changed_source_handoffs_create_nothing() {
    let (fixture, review) = learning_skill_fixture().await;
    let uri = learning_skill_uri(&fixture, &review);
    let input = learning_skill_input(&review);
    let mut stale = input.clone();
    stale["expectedVersion"] = json!(1);
    let mut wrong_digest = input.clone();
    wrong_digest["evidenceDigest"] = json!("0".repeat(64));
    for invalid in [stale, wrong_digest] {
        let response = send(&fixture.app, "POST", &uri, Some(invalid)).await;
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert_no_store(&response);
    }
    let other = fixture
        .state
        .write()
        .await
        .create_agent(test_config("other learning owner"))
        .unwrap()
        .state
        .id;
    let foreign_uri = format!(
        "/api/agents/{other}/learning-reviews/{}/proposals/{}/skill-draft",
        review["id"].as_str().unwrap(),
        review["proposals"][0]["id"].as_str().unwrap()
    );
    let foreign = send(&fixture.app, "POST", &foreign_uri, Some(input.clone())).await;
    assert_eq!(foreign.status(), StatusCode::NOT_FOUND);
    assert_no_store(&foreign);
    let proposal_uri = uri.strip_suffix("/skill-draft").unwrap();
    let dismissed = send(
        &fixture.app,
        "PATCH",
        proposal_uri,
        Some(json!({"decision":"dismissed","expectedVersion":0})),
    )
    .await;
    assert_eq!(dismissed.status(), StatusCode::OK);
    let dismissed_review = json_body(dismissed).await["review"].clone();
    assert_eq!(
        send(
            &fixture.app,
            "POST",
            &uri,
            Some(learning_skill_input(&dismissed_review))
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    assert!(pending_skill_drafts(&fixture.app).await.is_empty());
    assert!(!fixture
        .root
        .join("skills/arithmetic-check/SKILL.md")
        .exists());
    fixture.assert_source_unchanged().await;

    let (changed_fixture, changed_review) = learning_skill_fixture().await;
    change_learning_source_input(&changed_fixture).await;
    let changed = send(
        &changed_fixture.app,
        "POST",
        &learning_skill_uri(&changed_fixture, &changed_review),
        Some(learning_skill_input(&changed_review)),
    )
    .await;
    assert_eq!(changed.status(), StatusCode::CONFLICT);
    assert_no_store(&changed);
    assert!(pending_skill_drafts(&changed_fixture.app).await.is_empty());
    assert!(!changed_fixture
        .root
        .join("skills/arithmetic-check/SKILL.md")
        .exists());
    assert_eq!(changed_fixture.model.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn m39_concurrent_exact_handoffs_share_one_draft_and_never_recreate_a_pruned_link() {
    let (fixture, review) = learning_skill_fixture().await;
    let uri = learning_skill_uri(&fixture, &review);
    let input = learning_skill_input(&review);
    let (first, second) = tokio::join!(
        send(&fixture.app, "POST", &uri, Some(input.clone())),
        send(&fixture.app, "POST", &uri, Some(input.clone()))
    );
    let statuses = [first.status(), second.status()];
    assert!(statuses.contains(&StatusCode::CREATED) && statuses.contains(&StatusCode::OK));
    assert_no_store(&first);
    assert_no_store(&second);
    let first = json_body(first).await["draft"].clone();
    let second = json_body(second).await["draft"].clone();
    assert_eq!(first["id"], second["id"]);
    assert_eq!(pending_skill_drafts(&fixture.app).await.len(), 1);
    let mut different = input.clone();
    different["skill"]["body"] = json!("Different owner-reviewed procedure.");
    assert_eq!(
        send(&fixture.app, "POST", &uri, Some(different))
            .await
            .status(),
        StatusCode::CONFLICT
    );
    let proposal_uri = uri.strip_suffix("/skill-draft").unwrap();
    let acknowledged = send(
        &fixture.app,
        "PATCH",
        proposal_uri,
        Some(json!({"decision":"acknowledged","expectedVersion":0})),
    )
    .await;
    assert_eq!(acknowledged.status(), StatusCode::OK);
    let acknowledged_review = json_body(acknowledged).await["review"].clone();
    assert_eq!(
        send(&fixture.app, "POST", &uri, Some(input.clone()))
            .await
            .status(),
        StatusCode::CONFLICT,
        "version validation precedes duplicate lookup"
    );
    let mut fresh = input;
    fresh["expectedVersion"] = acknowledged_review["proposals"][0]["decisionVersion"].clone();
    let replay = send(&fixture.app, "POST", &uri, Some(fresh.clone())).await;
    assert_eq!(replay.status(), StatusCode::OK);
    assert_eq!(json_body(replay).await["draft"]["id"], first["id"]);
    let persist = {
        let mut guard = fixture.state.write().await;
        guard.skills.remove_draft(first["id"].as_str().unwrap());
        guard.control_plane_persist_request()
    };
    persist.save().await.unwrap();
    assert_eq!(
        send(&fixture.app, "POST", &uri, Some(fresh)).await.status(),
        StatusCode::CONFLICT
    );
    assert!(pending_skill_drafts(&fixture.app).await.is_empty());
    assert!(!fixture
        .root
        .join("skills/arithmetic-check/SKILL.md")
        .exists());
    assert_eq!(fixture.model.calls.load(Ordering::SeqCst), 1);
    fixture.assert_source_unchanged().await;
}

#[tokio::test]
async fn m39_handoff_save_failure_rolls_back_both_links_without_creating_a_skill_file() {
    let (fixture, review) = learning_skill_fixture().await;
    let blocked = fixture.root.join("handoff-blocked-parent");
    std::fs::write(&blocked, b"not a directory").unwrap();
    fixture
        .state
        .write()
        .await
        .set_control_plane_store(Some(ControlPlaneStoreConfig::Json(
            blocked.join("control-plane.json"),
        )));
    let response = send(
        &fixture.app,
        "POST",
        &learning_skill_uri(&fixture, &review),
        Some(learning_skill_input(&review)),
    )
    .await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_no_store(&response);
    let current = json_body(send(&fixture.app, "GET", &fixture.review_uri(&review), None).await)
        .await["review"]
        .clone();
    assert!(current["proposals"][0]["skillDraftId"].is_null());
    assert!(pending_skill_drafts(&fixture.app).await.is_empty());
    assert!(!fixture
        .root
        .join("skills/arithmetic-check/SKILL.md")
        .exists());
    fixture
        .state
        .write()
        .await
        .set_control_plane_store(Some(ControlPlaneStoreConfig::Json(
            fixture.root.join("control-plane.json"),
        )));
    learning_skill_draft(&fixture, &review, learning_skill_input(&review)).await;
    assert_eq!(pending_skill_drafts(&fixture.app).await.len(), 1);
    assert_eq!(fixture.model.calls.load(Ordering::SeqCst), 1);
    fixture.assert_source_unchanged().await;
}

#[tokio::test]
async fn m39_linked_approval_refuses_changed_deleted_or_dismissed_learning_sources() {
    for invalidation in ["changed", "deleted", "dismissed"] {
        let (fixture, review) = learning_skill_fixture().await;
        let draft = learning_skill_draft(&fixture, &review, learning_skill_input(&review)).await;
        match invalidation {
            "changed" => change_learning_source_input(&fixture).await,
            "deleted" => {
                let response = send(
                    &fixture.app,
                    "DELETE",
                    &format!("/api/agents/{}/sessions/chat%3Alearning", fixture.agent),
                    None,
                )
                .await;
                assert_eq!(response.status(), StatusCode::OK);
            }
            _ => {
                let uri = learning_skill_uri(&fixture, &review);
                let response = send(
                    &fixture.app,
                    "PATCH",
                    uri.strip_suffix("/skill-draft").unwrap(),
                    Some(json!({"decision":"dismissed","expectedVersion":0})),
                )
                .await;
                assert_eq!(response.status(), StatusCode::OK);
            }
        }
        let approval = send(
            &fixture.app,
            "POST",
            &format!(
                "/api/skill-drafts/{}/approve",
                draft["id"].as_str().unwrap()
            ),
            Some(json!({"body":"An explicitly reviewed edit still needs valid linked sources."})),
        )
        .await;
        assert_eq!(approval.status(), StatusCode::CONFLICT, "{invalidation}");
        assert_no_store(&approval);
        assert!(!fixture
            .root
            .join("skills/arithmetic-check/SKILL.md")
            .exists());
        assert_eq!(fixture.model.calls.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn m39_existing_skill_handoff_and_later_approval_require_the_actual_base_hash() {
    for drift in ["registry", "file"] {
        let (fixture, review) = learning_skill_fixture().await;
        let put = send(&fixture.app, "PUT", "/api/skills/arithmetic-check", Some(json!({"name":"Arithmetic check","description":"Original procedure","body":"Use the original check."}))).await;
        assert_eq!(put.status(), StatusCode::CREATED);
        let original_hash = json_body(put).await["skill"]["approvedHash"].clone();
        let path = fixture.root.join("skills/arithmetic-check/SKILL.md");
        let original_bytes = std::fs::read(&path).unwrap();
        let mut input = learning_skill_input(&review);
        assert_eq!(
            send(
                &fixture.app,
                "POST",
                &learning_skill_uri(&fixture, &review),
                Some(input.clone())
            )
            .await
            .status(),
            StatusCode::CONFLICT,
            "explicit null cannot replace an existing approved base"
        );
        input["skill"]["expectedHash"] = json!("0".repeat(64));
        assert_eq!(
            send(
                &fixture.app,
                "POST",
                &learning_skill_uri(&fixture, &review),
                Some(input.clone())
            )
            .await
            .status(),
            StatusCode::CONFLICT
        );
        input["skill"]["expectedHash"] = original_hash.clone();
        let draft = learning_skill_draft(&fixture, &review, input).await;
        assert_eq!(draft["baseHash"], original_hash);
        assert_eq!(std::fs::read(&path).unwrap(), original_bytes);
        if drift == "registry" {
            let updated = send(&fixture.app, "PUT", "/api/skills/arithmetic-check", Some(json!({"name":"Arithmetic check","description":"New procedure","body":"The owner changed the approved procedure."}))).await;
            assert_eq!(updated.status(), StatusCode::OK);
        } else {
            crate::skills::test_support::write_skill(
                &fixture.root,
                "arithmetic-check",
                &crate::skills::compose_skill_file(
                    "Arithmetic check",
                    "Unreviewed file change",
                    "The file moved without a registry approval.",
                ),
            );
        }
        let moved_bytes = std::fs::read(&path).unwrap();
        let rejected = send(
            &fixture.app,
            "POST",
            &format!(
                "/api/skill-drafts/{}/approve",
                draft["id"].as_str().unwrap()
            ),
            None,
        )
        .await;
        assert_eq!(rejected.status(), StatusCode::CONFLICT, "{drift}");
        assert_no_store(&rejected);
        assert_eq!(std::fs::read(&path).unwrap(), moved_bytes);
        assert_eq!(fixture.model.calls.load(Ordering::SeqCst), 1);
        fixture.assert_source_unchanged().await;
    }
}

#[tokio::test]
async fn m39_linked_draft_and_explicitly_edited_approval_survive_fresh_snapshot_loads() {
    let (fixture, review) = learning_skill_fixture().await;
    let input = learning_skill_input(&review);
    let draft = learning_skill_draft(&fixture, &review, input.clone()).await;
    let uri = learning_skill_uri(&fixture, &review);
    let decision = send(
        &fixture.app,
        "PATCH",
        uri.strip_suffix("/skill-draft").unwrap(),
        Some(json!({"decision":"acknowledged","expectedVersion":0})),
    )
    .await;
    assert_eq!(decision.status(), StatusCode::OK);
    let acknowledged_review = json_body(decision).await["review"].clone();
    let config = ControlPlaneStoreConfig::Json(fixture.root.join("control-plane.json"));
    let before = serde_json::to_value(fixture.state.read().await.control_plane_snapshot()).unwrap();
    let fresh_model = AdviceModel::new(None);
    let mut restored = DaemonState::with_model_adapter(fresh_model.clone());
    restored.set_history(fixture.state.read().await.history.clone());
    restored
        .restore_control_plane_snapshot(
            load_control_plane_snapshot(&config).await.unwrap().unwrap(),
        )
        .unwrap();
    restored.set_control_plane_store(Some(config.clone()));
    let restored_state = Arc::new(RwLock::new(restored));
    let restored_app = router(restored_state.clone(), DaemonConfig::default());
    let pending = pending_skill_drafts(&restored_app).await;
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0]["learningSource"], draft["learningSource"]);
    assert_eq!(pending[0]["body"], draft["body"]);
    let mut replay_input = input;
    replay_input["expectedVersion"] =
        acknowledged_review["proposals"][0]["decisionVersion"].clone();
    let replay = send(&restored_app, "POST", &uri, Some(replay_input.clone())).await;
    assert_eq!(replay.status(), StatusCode::OK);
    assert_eq!(json_body(replay).await["draft"]["id"], draft["id"]);
    let edited_body =
        "Calculate independently, record the checked result, and verify every new task separately.";
    let approved = send(
        &restored_app,
        "POST",
        &format!(
            "/api/skill-drafts/{}/approve",
            draft["id"].as_str().unwrap()
        ),
        Some(json!({"body":edited_body})),
    )
    .await;
    assert_eq!(
        approved.status(),
        StatusCode::OK,
        "acknowledgement preserves source validity"
    );
    assert_no_store(&approved);
    let approved = json_body(approved).await;
    assert_eq!(approved["draft"]["status"], "approved");
    let expected = crate::skills::compose_skill_file(
        "Arithmetic check",
        "Check arithmetic before presenting a result",
        edited_body,
    );
    let path = fixture.root.join("skills/arithmetic-check/SKILL.md");
    assert_eq!(std::fs::read(&path).unwrap(), expected.as_bytes());
    assert_eq!(
        approved["skill"]["approvedHash"],
        crate::skills::skill_hash(expected.as_bytes())
    );
    let decided_replay = send(&restored_app, "POST", &uri, Some(replay_input)).await;
    assert_eq!(decided_replay.status(), StatusCode::OK);
    assert_eq!(json_body(decided_replay).await["draft"]["id"], draft["id"]);
    let mut loaded = DaemonState::with_model_adapter(fresh_model.clone());
    loaded.set_history(fixture.state.read().await.history.clone());
    loaded
        .restore_control_plane_snapshot(
            load_control_plane_snapshot(&config).await.unwrap().unwrap(),
        )
        .unwrap();
    loaded.set_control_plane_store(Some(config));
    let after = serde_json::to_value(loaded.control_plane_snapshot()).unwrap();
    for key in ["agents", "approvalPolicies", "approvalRules"] {
        assert_eq!(after[key], before[key], "skill adoption preserves {key}");
    }
    let loaded_app = router(Arc::new(RwLock::new(loaded)), DaemonConfig::default());
    let read = send(&loaded_app, "GET", "/api/skills/arithmetic-check", None).await;
    assert_eq!(read.status(), StatusCode::OK);
    let read = json_body(read).await;
    assert_eq!(read["skill"]["status"], "active");
    assert_eq!(read["file"]["body"], edited_body);
    assert_eq!(
        read["skill"]["approvedHash"],
        approved["skill"]["approvedHash"]
    );
    assert_eq!(fresh_model.calls.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.model.calls.load(Ordering::SeqCst), 1);
    fixture.assert_source_unchanged().await;
}
