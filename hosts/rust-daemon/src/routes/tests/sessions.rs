use super::*;
use anima_core::{DataValue, MessageRole};

use crate::history::conformance::history_message;
use crate::sessions::test_support::{message, seed_messages};
use crate::sessions::{SessionKind, SessionOrigin, SessionRecord, TitleSource};

const OWNER_ORIGIN: &str = "http://localhost:4200";

#[tokio::test]
async fn main_chat_clear_preserves_companion_and_other_conversations() {
    let (app, state, agent) = app_with_session().await;
    let response = app
        .clone()
        .oneshot(main_chat_request(&agent))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let before = state.read().await.get_agent(&agent).unwrap().state.config;
    let response = app
        .clone()
        .oneshot(send(
            "POST",
            &format!("/api/agents/{agent}/sessions/chat%3Aplans/clear"),
            OWNER_ORIGIN,
            serde_json::json!({}),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let cleared = json(response).await;
    assert_eq!(cleared["session"]["id"], "chat:plans");
    assert_eq!(cleared["session"]["mainChat"], true);
    assert_eq!(cleared["session"]["messageCount"], 0);
    let guard = state.read().await;
    assert_eq!(guard.get_agent(&agent).unwrap().state.config, before);
    assert!(guard
        .agents
        .get(&agent)
        .unwrap()
        .messages()
        .iter()
        .all(|m| m.room_id != "chat:plans"));
    assert!(guard.sessions.get(&agent, "chat:plans").unwrap().main_chat);
}

#[tokio::test]
async fn main_chat_clear_rejects_non_owner_and_non_main_sessions() {
    let (app, _, agent) = app_with_session().await;
    let path = format!("/api/agents/{agent}/sessions/chat%3Aplans/clear");
    let response = app
        .clone()
        .oneshot(send("POST", &path, OWNER_ORIGIN, serde_json::json!({})))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let response = app
        .oneshot(send(
            "POST",
            &path,
            "https://untrusted.example",
            serde_json::json!({}),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[test]
fn m83_history_projects_only_successful_tool_source_metadata() {
    use crate::routes::contracts::SessionMessageResponse;
    use crate::sessions::views::PageMessage;
    let sources =
        serde_json::json!([{"title":"Returned source","url":"https://example.com/returned"}]);
    let envelope = serde_json::json!({"version":1,"provider":"exa","mode":"anonymous","untrusted":true,
        "results":[{"title":"Returned source","url":"https://example.com/returned","excerpt":"actual result"}],"truncated":false});
    let mut message = history_message(
        "m83-tool",
        "agent",
        "chat:m83",
        MessageRole::Tool,
        &envelope.to_string(),
        1,
    )
    .message;
    message.content.metadata = Some(std::collections::BTreeMap::from([
        (
            "webSources".into(),
            DataValue::Array(vec![DataValue::Object(std::collections::BTreeMap::from([
                ("title".into(), DataValue::String("Returned source".into())),
                (
                    "url".into(),
                    DataValue::String("https://example.com/returned".into()),
                ),
            ]))]),
        ),
        ("toolStatus".into(), DataValue::String("success".into())),
        ("toolCallId".into(), DataValue::String("search-call".into())),
        (
            "taskResult".into(),
            DataValue::Object(std::collections::BTreeMap::from([(
                "status".into(),
                DataValue::String("success".into()),
            )])),
        ),
        (
            "secretLike".into(),
            DataValue::String("must stay hidden".into()),
        ),
    ]));
    let projected = serde_json::to_value(SessionMessageResponse::from(&PageMessage {
        message: message.clone(),
        hidden: false,
    }))
    .unwrap();
    assert_eq!(projected["metadata"]["webSources"], sources);
    assert!(projected["metadata"].get("taskResult").is_none());
    assert!(projected["metadata"].get("secretLike").is_none());
    for role in [
        MessageRole::User,
        MessageRole::Assistant,
        MessageRole::System,
    ] {
        message.role = role;
        let projected = serde_json::to_value(SessionMessageResponse::from(&PageMessage {
            message: message.clone(),
            hidden: false,
        }))
        .unwrap();
        assert!(projected["metadata"].get("webSources").is_none());
    }
    message.role = MessageRole::Tool;
    message
        .content
        .metadata
        .as_mut()
        .unwrap()
        .insert("toolStatus".into(), DataValue::String("error".into()));
    let projected = serde_json::to_value(SessionMessageResponse::from(&PageMessage {
        message,
        hidden: false,
    }))
    .unwrap();
    assert!(projected["metadata"].get("webSources").is_none());
}

/// A model adapter that blocks in `generate` until released, signaling entry
/// first -- lets a test hold a run's room lock open (same pattern as
/// `routes::agents::tests::PendingModelAdapter`, not reusable across those
/// two test modules).
struct PendingModelAdapter {
    entered: Arc<Semaphore>,
    release: Arc<Semaphore>,
}

#[async_trait]
impl ModelAdapter for PendingModelAdapter {
    fn provider(&self) -> &str {
        "pending"
    }

    async fn generate(
        &self,
        config: &AgentConfig,
        _request: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        self.entered.add_permits(1);
        self.release
            .acquire()
            .await
            .expect("release semaphore should remain open")
            .forget();
        Ok(ModelGenerateResponse {
            content: Content {
                text: format!("{} handled task: pending", config.name),
                attachments: None,
                metadata: None,
            },
            tool_calls: None,
            usage: TokenUsage {
                prompt_tokens: 1,
                completion_tokens: 1,
                ..TokenUsage::default()
            },
            stop_reason: ModelStopReason::End,
        })
    }
}

fn pending_adapter() -> (Arc<dyn ModelAdapter>, Arc<Semaphore>, Arc<Semaphore>) {
    let entered = Arc::new(Semaphore::new(0));
    let release = Arc::new(Semaphore::new(0));
    (
        Arc::new(PendingModelAdapter {
            entered: Arc::clone(&entered),
            release: Arc::clone(&release),
        }),
        entered,
        release,
    )
}

fn get(uri: &str, origin: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(uri)
        .header("host", "127.0.0.1:8080")
        .header("origin", origin)
        .body(Body::empty())
        .unwrap()
}

async fn json(response: axum::response::Response) -> serde_json::Value {
    serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap()
}

async fn app_with_session() -> (axum::Router, Arc<RwLock<DaemonState>>, String) {
    let mut daemon = DaemonState::new();
    let agent = daemon
        .create_agent(test_config("companion"))
        .unwrap()
        .state
        .id;
    daemon.sessions.insert(SessionRecord::new(
        &agent,
        "chat:plans",
        SessionKind::Chat,
        SessionOrigin::Web,
        "Plans".into(),
        TitleSource::FirstMessage,
        1,
    ));
    seed_messages(
        &mut daemon,
        &agent,
        vec![
            message(
                &agent,
                "m1",
                "chat:plans",
                MessageRole::User,
                "Plan the offsite",
                1,
            ),
            message(
                &agent,
                "m2",
                "chat:plans",
                MessageRole::Assistant,
                "Here is the plan",
                2,
            ),
        ],
    );
    let state = Arc::new(RwLock::new(daemon));
    (router(state.clone(), DaemonConfig::default()), state, agent)
}

#[tokio::test]
async fn conversation_context_usage_counts_selected_history_and_summary_with_native_calibration() {
    let (app, state, agent) = app_with_session().await;
    {
        let mut guard = state.write().await;
        let runtime = guard.agents.get_mut(&agent).unwrap();
        let mut config = runtime.config().clone();
        let settings = config.settings.get_or_insert_with(Default::default);
        settings
            .additional
            .insert("contextBudgetTokens".into(), DataValue::Number(100.0));
        settings.max_tokens = Some(20);
        runtime.replace_config(config);
        let record = guard.sessions.get_mut(&agent, "chat:plans").unwrap();
        record.context_calibration_permille = Some(1500);
        record.summary = Some(crate::sessions::SessionSummary {
            text: "Kept".into(),
            through_message_id: "m2".into(),
            created_at_ms: 2,
            source_message_count: 2,
        });
        let mut revised = message(
            &agent,
            "revised",
            "chat:plans",
            MessageRole::Assistant,
            &"x".repeat(400),
            5,
        );
        revised.content.metadata = Some(std::collections::BTreeMap::from([(
            "revised".into(),
            DataValue::Bool(true),
        )]));
        seed_messages(
            &mut guard,
            &agent,
            vec![
                message(
                    &agent,
                    "u",
                    "chat:plans",
                    MessageRole::User,
                    &"x".repeat(32),
                    3,
                ),
                message(
                    &agent,
                    "a",
                    "chat:plans",
                    MessageRole::Assistant,
                    &"x".repeat(32),
                    4,
                ),
                revised,
                message(
                    &agent,
                    "revision-request",
                    "chat:plans",
                    MessageRole::System,
                    &"x".repeat(400),
                    6,
                ),
                crate::sessions::test_support::checkin_prompt(
                    &agent,
                    "silent-checkin",
                    "chat:plans",
                    "s1",
                    &"x".repeat(400),
                    7,
                ),
                message(
                    &agent,
                    "silent-reply",
                    "chat:plans",
                    MessageRole::Assistant,
                    "CHECKIN_OK",
                    8,
                ),
            ],
        );
    }
    let response = app
        .clone()
        .oneshot(get(
            &format!("/api/agents/{agent}/sessions/chat%3Aplans"),
            OWNER_ORIGIN,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = json(response).await;
    assert_eq!(
        body["session"]["contextUsage"],
        serde_json::json!({"estimatedTokens":62,"budgetTokens":80,"replyReserveTokens":20})
    );
    let listed = json(
        app.oneshot(get(&format!("/api/agents/{agent}/sessions"), OWNER_ORIGIN))
            .await
            .unwrap(),
    )
    .await;
    assert!(listed["sessions"][0].get("contextUsage").is_none());
}

#[tokio::test]
async fn conversation_context_usage_accepts_empty_history_and_keeps_an_over_budget_summary_truthful(
) {
    let mut daemon = DaemonState::new();
    let mut config = test_config("empty");
    config.provider = None;
    config.model = "unknown-local-model".into();
    let agent = daemon.create_agent(config).unwrap().state.id;
    daemon.sessions.insert(SessionRecord::new(
        &agent,
        "chat:empty",
        SessionKind::Chat,
        SessionOrigin::Web,
        "Empty".into(),
        TitleSource::Owner,
        1,
    ));
    let state = Arc::new(RwLock::new(daemon));
    let app = router(state.clone(), DaemonConfig::default());
    let path = format!("/api/agents/{agent}/sessions/chat%3Aempty");
    let empty = json(app.clone().oneshot(get(&path, OWNER_ORIGIN)).await.unwrap()).await;
    assert_eq!(
        empty["session"]["contextUsage"],
        serde_json::json!({"estimatedTokens":0,"budgetTokens":27904,"replyReserveTokens":4096})
    );
    {
        let mut guard = state.write().await;
        let runtime = guard.agents.get_mut(&agent).unwrap();
        let mut config = runtime.config().clone();
        config
            .settings
            .get_or_insert_with(Default::default)
            .additional
            .insert("contextBudgetTokens".into(), DataValue::Number(10.0));
        runtime.replace_config(config);
        guard
            .sessions
            .get_mut(&agent, "chat:empty")
            .unwrap()
            .summary = Some(crate::sessions::SessionSummary {
            text: "x".repeat(100),
            through_message_id: "pruned".into(),
            created_at_ms: 2,
            source_message_count: 10,
        });
    }
    let full = json(app.oneshot(get(&path, OWNER_ORIGIN)).await.unwrap()).await;
    assert_eq!(
        full["session"]["contextUsage"],
        serde_json::json!({"estimatedTokens":33,"budgetTokens":5,"replyReserveTokens":5})
    );
}

#[tokio::test]
async fn message_search_returns_multiple_cold_and_hot_matches_without_stale_or_hidden_rows() {
    let (app, state, agent) = app_with_session().await;
    let store = {
        let mut guard = state.write().await;
        guard
            .sessions
            .get_mut(&agent, "chat:plans")
            .unwrap()
            .archived = true;
        for id in ["chat:helper", "chat:delegated", "chat:pending"] {
            let mut record = SessionRecord::new(
                &agent,
                id,
                if id == "chat:helper" {
                    SessionKind::Helper
                } else {
                    SessionKind::Chat
                },
                SessionOrigin::Web,
                "Excluded".into(),
                TitleSource::Owner,
                1,
            );
            if id == "chat:delegated" {
                record.parent_agent_id = Some(agent.clone());
            }
            guard.sessions.insert(record);
        }
        guard
            .pending_history_deletions
            .push(crate::history::HistoryDeletion::session(
                &agent,
                "chat:pending",
            ));
        seed_messages(
            &mut guard,
            &agent,
            vec![
                message(
                    &agent,
                    "hot",
                    "chat:plans",
                    MessageRole::Assistant,
                    "Nebula hot answer",
                    20,
                ),
                message(
                    &agent,
                    "shadow",
                    "chat:plans",
                    MessageRole::User,
                    "Changed subject",
                    30,
                ),
            ],
        );
        guard.history.store()
    };
    let mut rows = vec![
        history_message(
            "cold-a",
            &agent,
            "chat:plans",
            MessageRole::User,
            "Nebula first question",
            10,
        ),
        history_message(
            "cold-b",
            &agent,
            "chat:plans",
            MessageRole::Assistant,
            "Nebula older answer",
            11,
        ),
        history_message(
            "shadow",
            &agent,
            "chat:plans",
            MessageRole::User,
            "Nebula stale mirror",
            30,
        ),
        history_message(
            "foreign",
            "other-owner",
            "chat:plans",
            MessageRole::User,
            "Nebula foreign",
            99,
        ),
        history_message(
            "helper-hit",
            &agent,
            "chat:helper",
            MessageRole::User,
            "Nebula helper",
            98,
        ),
        history_message(
            "delegated-hit",
            &agent,
            "chat:delegated",
            MessageRole::User,
            "Nebula delegated",
            97,
        ),
        history_message(
            "pending-hit",
            &agent,
            "chat:pending",
            MessageRole::User,
            "Nebula pending deletion",
            96,
        ),
    ];
    for n in 40..70 {
        let mut row = history_message(
            &format!("hidden-{n}"),
            &agent,
            "chat:plans",
            MessageRole::User,
            "Nebula hidden",
            n,
        );
        row.hidden = true;
        rows.push(row);
        rows.push(history_message(
            &format!("system-{n}"),
            &agent,
            "chat:plans",
            MessageRole::System,
            "Nebula system",
            n,
        ));
        rows.push(history_message(
            &format!("unowned-{n}"),
            &agent,
            "chat:unknown",
            MessageRole::User,
            "Nebula unowned",
            n,
        ));
    }
    store.upsert_messages(&rows).await.unwrap();
    let response = app
        .clone()
        .oneshot(get(
            &format!("/api/agents/{agent}/message-search?q=Nebu&limit=2"),
            OWNER_ORIGIN,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let body = json(response).await;
    assert_eq!(
        body["results"]
            .as_array()
            .unwrap()
            .iter()
            .map(|hit| hit["messageId"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["hot", "cold-b"]
    );
    assert_eq!(body["results"][0]["sessionId"], "chat:plans");
    assert_eq!(body["results"][0]["title"], "Plans");
    assert_eq!(body["results"][0]["role"], "assistant");
    assert_eq!(body["hasMore"], true);
    let punctuation = json(
        app.clone()
            .oneshot(get(
                &format!("/api/agents/{agent}/message-search?q=%21%21"),
                OWNER_ORIGIN,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(
        punctuation,
        serde_json::json!({"results":[],"hasMore":false})
    );
    for suffix in ["", "?q=", "?q=x&limit=0", "?q=x&limit=21", "?q=%00"] {
        let response = app
            .clone()
            .oneshot(get(
                &format!("/api/agents/{agent}/message-search{suffix}"),
                OWNER_ORIGIN,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{suffix}");
    }
    assert_eq!(
        app.oneshot(get(
            &format!("/api/agents/{agent}/message-search?q=nebula"),
            "https://evil.example"
        ))
        .await
        .unwrap()
        .status(),
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn anchor_messages_return_one_native_bounded_window_and_fail_closed() {
    let (app, state, agent) = app_with_session().await;
    let store = state.read().await.history.store();
    let rows = (1..=35)
        .map(|n| {
            history_message(
                &format!("msg-100-{n}"),
                &agent,
                "chat:plans",
                MessageRole::User,
                &format!("Cold message {n}"),
                100,
            )
        })
        .collect::<Vec<_>>();
    store.upsert_messages(&rows).await.unwrap();
    let response = app
        .clone()
        .oneshot(get(
            &format!("/api/agents/{agent}/sessions/chat%3Aplans/messages?anchor=msg-100-16"),
            OWNER_ORIGIN,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = json(response).await;
    assert_eq!(body["messages"].as_array().unwrap().len(), 20);
    assert_eq!(body["messages"][0]["id"], "msg-100-7");
    assert_eq!(body["messages"][9]["id"], "msg-100-16");
    assert_eq!(body["messages"][9]["text"], "Cold message 16");
    assert_eq!(body["messages"][9]["attachments"], serde_json::json!([]));
    assert_eq!(body["messages"][19]["id"], "msg-100-26");
    assert_eq!(body["nextBefore"], "msg-100-7");
    assert_eq!(body["anchorMessageId"], "msg-100-16");
    assert_eq!(body["hasNewer"], true);
    let mut hidden = history_message(
        "hidden-anchor",
        &agent,
        "chat:plans",
        MessageRole::User,
        "Private",
        200,
    );
    hidden.hidden = true;
    store
        .upsert_messages(&[
            hidden,
            history_message(
                "foreign-anchor",
                "foreign",
                "chat:plans",
                MessageRole::User,
                "Foreign",
                201,
            ),
        ])
        .await
        .unwrap();
    for anchor in ["missing", "hidden-anchor", "foreign-anchor"] {
        let response = app
            .clone()
            .oneshot(get(
                &format!("/api/agents/{agent}/sessions/chat%3Aplans/messages?anchor={anchor}"),
                OWNER_ORIGIN,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{anchor}");
    }
    for query in [
        "anchor=m1&before=m2",
        "anchor=m1&includeHidden=true",
        "anchor=m1&limit=21",
        "anchor=",
    ] {
        let response = app
            .clone()
            .oneshot(get(
                &format!("/api/agents/{agent}/sessions/chat%3Aplans/messages?{query}"),
                OWNER_ORIGIN,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{query}");
    }
    let flaky = Arc::new(crate::history::conformance::FlakyHistoryStore::new());
    flaky.set_failing(true);
    state
        .write()
        .await
        .set_history(crate::history::HistoryService::new(flaky));
    let response = app
        .clone()
        .oneshot(get(
            &format!("/api/agents/{agent}/sessions/chat%3Aplans/messages?anchor=m1"),
            OWNER_ORIGIN,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let search = app
        .oneshot(get(
            &format!("/api/agents/{agent}/message-search?q=nebula"),
            OWNER_ORIGIN,
        ))
        .await
        .unwrap();
    assert_eq!(search.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn session_reads_require_the_owner_and_are_never_cached() {
    let (app, _, agent) = app_with_session().await;
    for path in [
        format!("/api/agents/{agent}/sessions"),
        format!("/api/agents/{agent}/sessions/chat%3Aplans"),
        format!("/api/agents/{agent}/sessions/chat%3Aplans/messages"),
    ] {
        let response = app
            .clone()
            .oneshot(get(&path, "https://untrusted.example"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{path}");
        assert_eq!(response.headers()["cache-control"], "no-store");
        let response = app.clone().oneshot(get(&path, OWNER_ORIGIN)).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert_eq!(response.headers()["cache-control"], "no-store");
    }
}

#[tokio::test]
async fn session_routes_list_one_session_and_page_its_messages() {
    let (app, _, agent) = app_with_session().await;
    let list = json(
        app.clone()
            .oneshot(get(
                &format!("/api/agents/{agent}/sessions?limit=10"),
                OWNER_ORIGIN,
            ))
            .await
            .unwrap(),
    )
    .await;
    let session = &list["sessions"][0];
    assert_eq!(session["id"], "chat:plans");
    assert_eq!(session["agentId"], agent.as_str());
    assert_eq!(session["roomId"], "chat:plans");
    assert_eq!(session["kind"], "chat");
    assert_eq!(session["origin"], "web");
    assert_eq!(session["titleSource"], "first_message");
    assert_eq!(session["messageCount"], 2);
    assert_eq!(session["preview"], "Here is the plan");
    assert_eq!(session["unread"], true);
    assert_eq!(session["activeRuns"], 0);
    assert_eq!(session["pendingApprovals"], 0);
    assert_eq!(session["capabilities"]["delete"], true);
    assert!(session.get("match").is_none());
    assert_eq!(list["nextCursor"], serde_json::Value::Null);

    let one = json(
        app.clone()
            .oneshot(get(
                &format!("/api/agents/{agent}/sessions/chat%3Aplans"),
                OWNER_ORIGIN,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(one["session"]["title"], "Plans");

    let page = json(
        app.clone()
            .oneshot(get(
                &format!("/api/agents/{agent}/sessions/chat%3Aplans/messages?limit=1"),
                OWNER_ORIGIN,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(page["messages"][0]["id"], "m2");
    assert_eq!(page["messages"][0]["role"], "assistant");
    assert_eq!(page["messages"][0]["text"], "Here is the plan");
    assert_eq!(page["nextBefore"], "m2");
    let older = json(
        app.clone()
            .oneshot(get(
                &format!("/api/agents/{agent}/sessions/chat%3Aplans/messages?limit=1&before=m2"),
                OWNER_ORIGIN,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(older["messages"][0]["id"], "m1");
    assert_eq!(older["nextBefore"], serde_json::Value::Null);

    for (path, status, error) in [
        (
            "/api/agents/missing/sessions".to_string(),
            StatusCode::NOT_FOUND,
            "not found",
        ),
        (
            format!("/api/agents/{agent}/sessions/chat%3Amissing"),
            StatusCode::NOT_FOUND,
            "not found",
        ),
        (
            format!("/api/agents/{agent}/sessions?limit=0"),
            StatusCode::BAD_REQUEST,
            "limit must be between 1 and 200",
        ),
        (
            format!("/api/agents/{agent}/sessions?kind=bogus"),
            StatusCode::BAD_REQUEST,
            "kind must be one of chat, telegram, discord, email, checkin, job, helper",
        ),
        (
            format!("/api/agents/{agent}/sessions?archived=maybe"),
            StatusCode::BAD_REQUEST,
            "archived must be true or false",
        ),
        (
            format!("/api/agents/{agent}/sessions?cursor=%21"),
            StatusCode::BAD_REQUEST,
            "cursor is invalid",
        ),
        (
            format!("/api/agents/{agent}/sessions?q={}", "a".repeat(201)),
            StatusCode::BAD_REQUEST,
            "q must be at most 200 characters",
        ),
        (
            format!("/api/agents/{agent}/sessions/chat%3Aplans/messages?before=nope"),
            StatusCode::BAD_REQUEST,
            "before message was not found",
        ),
        (
            format!("/api/agents/{agent}/sessions/chat%3Aplans/messages?limit=201"),
            StatusCode::BAD_REQUEST,
            "limit must be between 1 and 200",
        ),
    ] {
        let response = app.clone().oneshot(get(&path, OWNER_ORIGIN)).await.unwrap();
        assert_eq!(response.status(), status, "{path}");
        assert_eq!(response.headers()["cache-control"], "no-store", "{path}");
        assert_eq!(json(response).await["error"], error, "{path}");
    }
}

#[tokio::test]
async fn search_finds_a_session_whose_only_match_is_older_than_five_hundred_matches_elsewhere() {
    // Controller ruling 2 (M2 pre-flight audit): session search ranks
    // sessions by their newest matching message, so a session with more than
    // SEARCH_SESSION_LIMIT (500) matches must not crowd an older session's only
    // match out of the results.
    let (app, state, agent) = app_with_session().await;
    let store = {
        let mut daemon = state.write().await;
        daemon.sessions.insert(SessionRecord::new(
            &agent,
            "chat:busy",
            SessionKind::Chat,
            SessionOrigin::Web,
            "Busy".into(),
            TitleSource::FirstMessage,
            2_000,
        ));
        daemon.sessions.insert(SessionRecord::new(
            &agent,
            "chat:quiet",
            SessionKind::Chat,
            SessionOrigin::Web,
            "Quiet".into(),
            TitleSource::FirstMessage,
            1,
        ));
        daemon.history.store()
    };
    let busy_rows = (0..=500u64)
        .map(|n| {
            history_message(
                &format!("busy-{n}"),
                &agent,
                "chat:busy",
                MessageRole::User,
                "deploy the build",
                1_000 + n,
            )
        })
        .collect::<Vec<_>>();
    store.upsert_messages(&busy_rows).await.unwrap();
    store
        .upsert_messages(&[history_message(
            "quiet-1",
            &agent,
            "chat:quiet",
            MessageRole::User,
            "deploy notes",
            1,
        )])
        .await
        .unwrap();

    let list = json(
        app.clone()
            .oneshot(get(
                &format!("/api/agents/{agent}/sessions?q=deploy"),
                OWNER_ORIGIN,
            ))
            .await
            .unwrap(),
    )
    .await;
    let ids = list["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|session| session["id"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        ["chat:busy", "chat:quiet"],
        "a session with more than 500 matches must not crowd out an older session's only match"
    );
}

#[tokio::test]
async fn listing_agents_as_summaries_omits_their_messages() {
    let (app, _, agent) = app_with_session().await;
    let summaries = json(
        app.clone()
            .oneshot(get("/api/agents?view=summary", OWNER_ORIGIN))
            .await
            .unwrap(),
    )
    .await;
    let summary = &summaries["agents"][0];
    assert_eq!(summary["state"]["id"], agent.as_str());
    assert_eq!(summary["messageCount"], 2);
    assert!(summary.get("messages").is_none());
    let full = json(
        app.clone()
            .oneshot(get("/api/agents", OWNER_ORIGIN))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(
        full["agents"][0]["messages"].as_array().unwrap().len(),
        2,
        "the default response is unchanged"
    );
    // Controller ruling 1 (M2 pre-flight audit): an unrecognized `view` is
    // ignored, not rejected -- only `view=summary` changes the shape.
    let unrecognized = json(
        app.clone()
            .oneshot(get("/api/agents?view=full", OWNER_ORIGIN))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(
        unrecognized["agents"][0]["messages"]
            .as_array()
            .unwrap()
            .len(),
        2,
        "an unrecognized view falls back to the full response"
    );
    // Fix round 1 (M2 review): a malformed query string is not a rejection
    // either -- this route used to ignore the URI entirely.
    let malformed = json(
        app.clone()
            .oneshot(get("/api/agents?%zz=1", OWNER_ORIGIN))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(
        malformed["agents"][0]["messages"].as_array().unwrap().len(),
        2,
        "a malformed query string falls back to the full response, not a rejection"
    );
}

#[tokio::test]
async fn message_pages_expose_tool_status_duration_and_incomplete_but_not_task_results() {
    use crate::agent_runs::test_support::{
        calculate_call, chat_request, companion_config, ScriptedModel, Step,
    };

    let model = ScriptedModel::new(vec![
        Step::Tools(vec![calculate_call("call-1", "6*7")]),
        Step::Text(vec!["It is 42."]),
    ]);
    let state = Arc::new(RwLock::new(DaemonState::with_model_adapter(model)));
    let agent = state
        .write()
        .await
        .create_agent(companion_config("companion"))
        .unwrap()
        .state
        .id;
    crate::agent_runs::AgentRunCoordinator::new(state.clone(), Arc::new(Semaphore::new(8)))
        .run(chat_request(&agent, "chat:tools", "what is 6*7?"))
        .await
        .unwrap();
    // The text of a model call that failed after streaming (spec §4.5).
    let mut unfinished = message(
        &agent,
        "partial",
        "chat:tools",
        MessageRole::Assistant,
        "It is",
        anima_core::primitives::now_millis() + 1_000,
    );
    unfinished.content.metadata = Some(std::collections::BTreeMap::from([(
        anima_core::INCOMPLETE_METADATA_KEY.to_string(),
        DataValue::Bool(true),
    )]));
    seed_messages(&mut *state.write().await, &agent, vec![unfinished]);
    let app = router(state, DaemonConfig::default());

    let page = json(
        app.oneshot(get(
            &format!("/api/agents/{agent}/sessions/chat%3Atools/messages"),
            OWNER_ORIGIN,
        ))
        .await
        .unwrap(),
    )
    .await;
    let messages = page["messages"].as_array().unwrap();
    let tool = messages
        .iter()
        .find(|message| message["role"] == "tool")
        .expect("the run stored its tool result");
    let metadata = &tool["metadata"];
    assert_eq!(metadata["toolCallId"], "call-1");
    assert_eq!(metadata["toolStatus"], "success");
    assert!(metadata["toolDurationMs"].is_number(), "{metadata}");
    assert!(metadata["stepId"].is_string());
    assert!(
        metadata.get("taskResult").is_none(),
        "the whole tool result stays hidden: {metadata}"
    );
    let partial = messages
        .iter()
        .find(|message| message["id"] == "partial")
        .unwrap();
    assert_eq!(partial["metadata"]["incomplete"], true);
}

#[tokio::test]
async fn owner_message_page_exposes_budget_wrapup_provenance_without_extra_actions() {
    use crate::agent_runs::test_support::{
        calculate_call, chat_request, companion_config, ScriptedModel, Step,
    };

    const SESSION_ID: &str = "chat:budget-wrapup";
    const FINAL_TEXT: &str =
        "The calculation returned 42. I have not verified the rest of the request.";
    let model = ScriptedModel::new(vec![
        Step::Tools(vec![calculate_call("call-wrapup", "6*7")]),
        Step::Text(vec![FINAL_TEXT]),
    ]);
    let state = Arc::new(RwLock::new(DaemonState::with_model_adapter(model)));
    let mut config = companion_config("companion");
    config.settings.as_mut().unwrap().max_tool_iterations = Some(1);
    let agent = state.write().await.create_agent(config).unwrap().state.id;
    let coordinator =
        crate::agent_runs::AgentRunCoordinator::new(state.clone(), Arc::new(Semaphore::new(8)));
    coordinator
        .run(chat_request(
            &agent,
            SESSION_ID,
            "Calculate 6*7 and complete my task",
        ))
        .await
        .unwrap();

    let guard = state.read().await;
    let runtime = guard.agents.get(&agent).unwrap();
    let stored_messages = runtime.messages();
    assert_eq!(
        stored_messages.len(),
        4,
        "one user, one call/result pair, and one final reply"
    );
    assert_eq!(
        stored_messages
            .iter()
            .filter(|message| message.role == MessageRole::Tool)
            .count(),
        1,
        "the wrap-up must not fabricate another tool result"
    );
    let final_message = stored_messages.last().unwrap();
    assert_eq!(final_message.role, MessageRole::Assistant);
    assert_eq!(final_message.content.text, FINAL_TEXT);
    let final_metadata = final_message.content.metadata.as_ref().unwrap();
    assert_eq!(
        final_metadata.get("budgetWrapup"),
        Some(&DataValue::Bool(true))
    );
    assert!(final_metadata.get("runId").is_some());
    assert!(final_metadata.get("stepId").is_some());
    let DataValue::String(final_run_id) = &final_metadata["runId"] else {
        panic!("run provenance is a string");
    };
    let DataValue::String(final_step_id) = &final_metadata["stepId"] else {
        panic!("step provenance is a string");
    };
    let final_run_id = final_run_id.clone();
    let final_step_id = final_step_id.clone();

    let run = guard
        .runs
        .get(&final_run_id)
        .expect("the coordinator saved the run");
    assert_eq!(run.status, crate::runs::RunStatus::Completed);
    assert_eq!(run.tools_started, ["calculate"]);
    assert_eq!(run.action_receipts.len(), 1);
    drop(guard);

    let app = router(state, DaemonConfig::default());
    let page = json(
        app.oneshot(get(
            &format!("/api/agents/{agent}/sessions/chat%3Abudget-wrapup/messages"),
            OWNER_ORIGIN,
        ))
        .await
        .unwrap(),
    )
    .await;
    let messages = page["messages"].as_array().unwrap();
    assert_eq!(
        messages.len(),
        4,
        "the owner page must preserve the same four rows"
    );
    let final_row = messages.last().unwrap();
    assert_eq!(final_row["role"], "assistant");
    assert_eq!(final_row["text"], FINAL_TEXT);
    assert_eq!(final_row["metadata"]["budgetWrapup"], true);
    assert_eq!(final_row["metadata"]["runId"], final_run_id);
    assert_eq!(final_row["metadata"]["stepId"], final_step_id);
    let tool_row = messages
        .iter()
        .find(|message| message["role"] == "tool")
        .unwrap();
    assert_eq!(tool_row["metadata"]["toolCallId"], "call-wrapup");
    assert!(tool_row["metadata"].get("taskResult").is_none());
    assert!(tool_row["metadata"].get("budgetWrapup").is_none());
}

#[test]
fn the_openapi_document_lists_the_session_read_routes() {
    use utoipa::OpenApi;

    let paths = crate::routes::ApiDoc::openapi().paths.paths;
    for path in [
        "/api/agents/{agent_id}/sessions",
        "/api/agents/{agent_id}/sessions/{session_id}",
        "/api/agents/{agent_id}/sessions/{session_id}/messages",
    ] {
        assert!(paths.contains_key(path), "{path}");
    }
    let schema = serde_json::to_value(crate::routes::ApiDoc::openapi()).unwrap()["components"]
        ["schemas"]["SessionMessageResponse"]["properties"]["metadata"]["description"]
        .clone();
    for key in ["toolStatus", "toolDurationMs", "incomplete", "budgetWrapup"] {
        assert!(
            schema.as_str().unwrap_or_default().contains(key),
            "the metadata description names {key}: {schema}"
        );
    }
}

fn send(method: &str, uri: &str, origin: &str, body: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("host", "127.0.0.1:8080")
        .header("origin", origin)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

/// A path a JSON control-plane save cannot write to: a directory where the
/// store expects a file (same trick as `connectors::runtime::tests::
/// invalid_snapshot_directory`, not reusable across those two test modules).
fn invalid_snapshot_directory(label: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "anima-session-route-{label}-invalid-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn designated_main(record: SessionRecord) -> SessionRecord {
    let mut value = serde_json::to_value(record).unwrap();
    value["mainChat"] = serde_json::json!(true);
    serde_json::from_value(value).unwrap()
}

fn main_chat_request(agent: &str) -> Request<Body> {
    send(
        "POST",
        &format!("/api/agents/{agent}/main-chat"),
        OWNER_ORIGIN,
        serde_json::json!({}),
    )
}

#[tokio::test]
async fn main_chat_creation_is_atomic_durable_and_stable_after_restore() {
    use crate::control_plane_store::{load_control_plane_snapshot, ControlPlaneStoreConfig};

    let (app, state, agent) = app_with_session().await;
    let path = std::env::temp_dir().join(format!("anima-main-chat-{}.json", uuid::Uuid::new_v4()));
    let store = ControlPlaneStoreConfig::Json(path.clone());
    {
        let mut guard = state.write().await;
        guard.sessions.remove(&agent, "chat:plans");
        guard.set_control_plane_store(Some(store.clone()));
    }
    let (first, second) = tokio::join!(
        app.clone().oneshot(main_chat_request(&agent)),
        app.clone().oneshot(main_chat_request(&agent)),
    );
    let first = first.unwrap();
    let second = second.unwrap();
    let statuses = [first.status(), second.status()];
    assert!(statuses.contains(&StatusCode::CREATED), "{statuses:?}");
    assert!(statuses.contains(&StatusCode::OK), "{statuses:?}");
    assert_eq!(first.headers()["cache-control"], "no-store");
    assert_eq!(second.headers()["cache-control"], "no-store");
    let first = json(first).await;
    let second = json(second).await;
    assert_eq!(first["session"]["id"], second["session"]["id"]);
    assert_eq!(first["session"]["mainChat"], true);
    let id = first["session"]["id"].as_str().unwrap();
    let saved = load_control_plane_snapshot(&store).await.unwrap().unwrap();
    assert_eq!(saved.sessions.len(), 1);
    assert_eq!(
        serde_json::to_value(&saved.sessions[0]).unwrap()["mainChat"],
        true
    );
    let mut restored = DaemonState::new();
    restored.restore_control_plane_snapshot(saved).unwrap();
    let restored = Arc::new(RwLock::new(restored));
    let app = router(restored, DaemonConfig::default());
    let response = app.oneshot(main_chat_request(&agent)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json(response).await["session"]["id"], id);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn main_chat_adopts_only_the_newest_eligible_owned_web_chat_once() {
    let (app, state, agent) = app_with_session().await;
    {
        let mut guard = state.write().await;
        let other = guard.create_agent(test_config("other")).unwrap().state.id;
        for (owner, id, kind, origin, created, activity) in [
            (
                agent.as_str(),
                "chat:a",
                SessionKind::Chat,
                SessionOrigin::Web,
                5,
                10,
            ),
            (
                agent.as_str(),
                "chat:b",
                SessionKind::Chat,
                SessionOrigin::Web,
                5,
                10,
            ),
            (
                agent.as_str(),
                "chat:earlier-created",
                SessionKind::Chat,
                SessionOrigin::Web,
                4,
                10,
            ),
            (
                agent.as_str(),
                "chat:older-activity",
                SessionKind::Chat,
                SessionOrigin::Web,
                20,
                9,
            ),
            (
                agent.as_str(),
                "chat:api",
                SessionKind::Chat,
                SessionOrigin::Api,
                50,
                50,
            ),
            (
                agent.as_str(),
                "telegram:remote",
                SessionKind::Telegram,
                SessionOrigin::Telegram,
                50,
                50,
            ),
            (
                agent.as_str(),
                "discord:remote",
                SessionKind::Discord,
                SessionOrigin::Discord,
                50,
                50,
            ),
            (
                agent.as_str(),
                "schedule:checkin",
                SessionKind::Checkin,
                SessionOrigin::Schedule,
                50,
                50,
            ),
            (
                agent.as_str(),
                "job:work",
                SessionKind::Job,
                SessionOrigin::Job,
                50,
                50,
            ),
            (
                agent.as_str(),
                "chat:helper",
                SessionKind::Helper,
                SessionOrigin::Delegation,
                50,
                50,
            ),
            (
                other.as_str(),
                "chat:foreign",
                SessionKind::Chat,
                SessionOrigin::Web,
                50,
                50,
            ),
        ] {
            let mut record = SessionRecord::new(
                owner,
                id,
                kind,
                origin,
                "Owner title".into(),
                TitleSource::Owner,
                created,
            );
            record.last_activity_at_ms = activity;
            guard.sessions.insert(record);
        }
        for id in [
            "chat:archived",
            "chat:pending",
            "chat:parent-session",
            "chat:parent-run",
            "chat:parent-agent",
        ] {
            let mut record = SessionRecord::new(
                &agent,
                id,
                SessionKind::Chat,
                SessionOrigin::Web,
                "Ineligible".into(),
                TitleSource::Owner,
                100,
            );
            match id {
                "chat:archived" => record.archived = true,
                "chat:pending" => guard
                    .record_history_deletion(crate::history::HistoryDeletion::session(&agent, id)),
                "chat:parent-session" => record.parent_session_id = Some("chat:plans".into()),
                "chat:parent-run" => record.parent_run_id = Some("run:parent".into()),
                _ => record.parent_agent_id = Some(other.clone()),
            }
            guard.sessions.insert(record);
        }
        let now = anima_core::primitives::now_millis();
        while guard.session_limiter.try_acquire(&agent, now) {}
    }
    let response = app
        .clone()
        .oneshot(main_chat_request(&agent))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let adopted = json(response).await;
    assert_eq!(adopted["session"]["id"], "chat:a");
    assert_eq!(adopted["session"]["title"], "Owner title");
    assert_eq!(adopted["session"]["titleSource"], "owner");
    assert_eq!(adopted["session"]["roomId"], "chat:a");
    {
        let mut guard = state.write().await;
        guard
            .sessions
            .get_mut(&agent, "chat:b")
            .unwrap()
            .last_activity_at_ms = 1000;
    }
    let response = app.oneshot(main_chat_request(&agent)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json(response).await["session"]["id"], "chat:a");
}

#[tokio::test]
async fn main_chat_requires_owner_and_empty_contract_and_preserves_legacy_main_id() {
    let (app, state, agent) = app_with_session().await;
    let helper = {
        let mut guard = state.write().await;
        let mut config = test_config("helper");
        config
            .settings
            .as_mut()
            .unwrap()
            .additional
            .insert("workspaceRole".into(), DataValue::String("helper".into()));
        config
            .settings
            .as_mut()
            .unwrap()
            .additional
            .insert("parentAgentId".into(), DataValue::String(agent.clone()));
        let helper = guard.create_agent(config).unwrap().state.id;
        guard.sessions.insert(SessionRecord::new(
            &agent,
            "main",
            SessionKind::Chat,
            SessionOrigin::Api,
            "Legacy main".into(),
            TitleSource::Owner,
            1,
        ));
        helper
    };
    for (owner, origin, body, expected) in [
        (
            agent.as_str(),
            "https://untrusted.example",
            serde_json::json!({}),
            StatusCode::FORBIDDEN,
        ),
        (
            "missing",
            OWNER_ORIGIN,
            serde_json::json!({}),
            StatusCode::NOT_FOUND,
        ),
        (
            helper.as_str(),
            OWNER_ORIGIN,
            serde_json::json!({}),
            StatusCode::CONFLICT,
        ),
        (
            agent.as_str(),
            OWNER_ORIGIN,
            serde_json::json!({"sessionId":"chosen"}),
            StatusCode::BAD_REQUEST,
        ),
        (
            agent.as_str(),
            OWNER_ORIGIN,
            serde_json::json!(null),
            StatusCode::BAD_REQUEST,
        ),
        (
            agent.as_str(),
            OWNER_ORIGIN,
            serde_json::json!([]),
            StatusCode::BAD_REQUEST,
        ),
    ] {
        let response = app
            .clone()
            .oneshot(send(
                "POST",
                &format!("/api/agents/{owner}/main-chat"),
                origin,
                body,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
        assert_eq!(response.headers()["cache-control"], "no-store");
    }
    let empty = Request::builder()
        .method("POST")
        .uri(format!("/api/agents/{agent}/main-chat"))
        .header("host", "127.0.0.1:8080")
        .header("origin", OWNER_ORIGIN)
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.clone().oneshot(empty).await.unwrap().status(),
        StatusCode::OK
    );
    let legacy = app
        .oneshot(get(
            &format!("/api/agents/{agent}/sessions/main"),
            OWNER_ORIGIN,
        ))
        .await
        .unwrap();
    assert_eq!(legacy.status(), StatusCode::OK);
    assert_eq!(json(legacy).await["session"]["title"], "Legacy main");
}

#[tokio::test]
async fn main_chat_failed_save_rolls_back_adoption_and_creation() {
    use crate::control_plane_store::ControlPlaneStoreConfig;
    for adopt in [true, false] {
        let (app, state, agent) = app_with_session().await;
        let path = invalid_snapshot_directory("main-chat");
        let before = {
            let mut guard = state.write().await;
            if !adopt {
                guard.sessions.remove(&agent, "chat:plans");
            }
            guard.set_control_plane_store(Some(ControlPlaneStoreConfig::Json(path.clone())));
            guard
                .sessions
                .snapshot_records(&std::collections::HashSet::from([agent.clone()]))
        };
        let response = app.oneshot(main_chat_request(&agent)).await.unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(response.headers()["cache-control"], "no-store");
        assert_eq!(
            state
                .read()
                .await
                .sessions
                .snapshot_records(&std::collections::HashSet::from([agent])),
            before
        );
        std::fs::remove_dir_all(path).unwrap();
    }
}

#[tokio::test]
async fn main_chat_conflicts_if_designation_is_archived_or_pending_deletion() {
    for pending in [true, false] {
        let (app, state, agent) = app_with_session().await;
        {
            let mut guard = state.write().await;
            let mut record = guard.sessions.get(&agent, "chat:plans").unwrap().clone();
            record.archived = !pending;
            guard.sessions.insert(designated_main(record));
            if pending {
                guard.record_history_deletion(crate::history::HistoryDeletion::session(
                    &agent,
                    "chat:plans",
                ));
            }
        }
        let response = app.oneshot(main_chat_request(&agent)).await.unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert_eq!(
            state.read().await.sessions.len(),
            1,
            "never replace an unavailable main chat"
        );
    }
}

#[tokio::test]
async fn main_chat_blocks_archive_and_delete_but_keeps_rename_and_old_chat_controls() {
    let (app, state, agent) = app_with_session().await;
    let response = app
        .clone()
        .oneshot(main_chat_request(&agent))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let main = json(response).await;
    assert_eq!(main["session"]["capabilities"]["archive"], false);
    assert_eq!(main["session"]["capabilities"]["delete"], false);
    let main_path = format!("/api/agents/{agent}/sessions/chat%3Aplans");
    for (method, value) in [
        ("PATCH", serde_json::json!({"archived":true})),
        ("PATCH", serde_json::json!({"archived":false})),
        ("DELETE", serde_json::Value::Null),
    ] {
        let response = app
            .clone()
            .oneshot(send(method, &main_path, OWNER_ORIGIN, value))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);
    }
    let renamed = app
        .clone()
        .oneshot(send(
            "PATCH",
            &main_path,
            OWNER_ORIGIN,
            serde_json::json!({"title":"Personal main"}),
        ))
        .await
        .unwrap();
    assert_eq!(renamed.status(), StatusCode::OK);
    assert_eq!(json(renamed).await["session"]["title"], "Personal main");
    state.write().await.sessions.insert(SessionRecord::new(
        &agent,
        "chat:old",
        SessionKind::Chat,
        SessionOrigin::Web,
        "Old".into(),
        TitleSource::Owner,
        1,
    ));
    let old = format!("/api/agents/{agent}/sessions/chat%3Aold");
    assert_eq!(
        app.clone()
            .oneshot(send(
                "PATCH",
                &old,
                OWNER_ORIGIN,
                serde_json::json!({"archived":true})
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        app.oneshot(send("DELETE", &old, OWNER_ORIGIN, serde_json::Value::Null))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn main_chat_snapshot_defaults_and_rejects_duplicate_ineligible_and_foreign_markers() {
    let (_, state, agent) = app_with_session().await;
    let snapshot = state.read().await.control_plane_snapshot();
    let mut old = serde_json::to_value(&snapshot).unwrap();
    old["sessions"][0]
        .as_object_mut()
        .unwrap()
        .remove("mainChat");
    let mut restored = DaemonState::new();
    restored
        .restore_control_plane_snapshot(serde_json::from_value(old).unwrap())
        .unwrap();
    assert_eq!(
        serde_json::to_value(restored.sessions.get(&agent, "chat:plans").unwrap()).unwrap()
            ["mainChat"],
        false
    );
    for mutation in [
        "valid",
        "duplicate",
        "api",
        "helper-session",
        "parent",
        "foreign",
        "helper-agent",
    ] {
        let mut altered = serde_json::to_value(&snapshot).unwrap();
        altered["sessions"][0]["mainChat"] = serde_json::json!(true);
        match mutation {
            "duplicate" => {
                let mut second = altered["sessions"][0].clone();
                second["id"] = serde_json::json!("chat:second");
                altered["sessions"].as_array_mut().unwrap().push(second);
            }
            "api" => altered["sessions"][0]["origin"] = serde_json::json!("api"),
            "helper-session" => altered["sessions"][0]["kind"] = serde_json::json!("helper"),
            "parent" => {
                altered["sessions"][0]["parentSessionId"] = serde_json::json!("chat:parent")
            }
            "foreign" => altered["sessions"][0]["agentId"] = serde_json::json!("absent-owner"),
            "helper-agent" => {
                altered["agents"][0]["state"]["config"]["settings"]["additional"]
                    ["workspaceRole"] =
                    serde_json::to_value(DataValue::String("helper".into())).unwrap();
                altered["agents"][0]["state"]["config"]["settings"]["additional"]
                    ["parentAgentId"] =
                    serde_json::to_value(DataValue::String("parent".into())).unwrap();
            }
            _ => {}
        }
        let mut restored = DaemonState::new();
        let result =
            restored.restore_control_plane_snapshot(serde_json::from_value(altered).unwrap());
        if mutation == "valid" {
            result.unwrap();
            assert_eq!(
                serde_json::to_value(restored.sessions.get(&agent, "chat:plans").unwrap()).unwrap()
                    ["mainChat"],
                true
            );
        } else {
            assert!(result.is_err(), "{mutation} must not restore");
        }
    }
}

#[tokio::test]
async fn session_mutations_and_export_require_the_owner() {
    let (app, _, agent) = app_with_session().await;
    let session = format!("/api/agents/{agent}/sessions/chat%3Aplans");
    for request in [
        send(
            "POST",
            &format!("/api/agents/{agent}/sessions"),
            "https://untrusted.example",
            serde_json::json!({}),
        ),
        send(
            "PATCH",
            &session,
            "https://untrusted.example",
            serde_json::json!({"archived": true}),
        ),
        send(
            "DELETE",
            &session,
            "https://untrusted.example",
            serde_json::Value::Null,
        ),
        get(&format!("{session}/export"), "https://untrusted.example"),
    ] {
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(response.headers()["cache-control"], "no-store");
    }
}

#[tokio::test]
async fn creating_a_session_for_a_helper_agent_is_refused() {
    // Controller ruling 1 (M2 pre-flight audit): helpers have no chat of
    // their own; they only run through the companion that delegates to them.
    let (app, state, agent) = app_with_session().await;
    let helper_id = {
        let mut guard = state.write().await;
        let mut helper = test_config("helper");
        let settings = helper.settings.as_mut().unwrap();
        settings
            .additional
            .insert("workspaceRole".into(), DataValue::String("helper".into()));
        settings
            .additional
            .insert("parentAgentId".into(), DataValue::String(agent.clone()));
        guard.create_agent(helper).unwrap().state.id
    };

    let response = app
        .clone()
        .oneshot(send(
            "POST",
            &format!("/api/agents/{helper_id}/sessions"),
            OWNER_ORIGIN,
            serde_json::json!({}),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(
        json(response).await["error"],
        "Helpers must run through their owning companion"
    );
}

#[tokio::test]
async fn creating_a_chat_saves_it_returns_201_and_is_rate_limited() {
    use crate::control_plane_store::{load_control_plane_snapshot, ControlPlaneStoreConfig};

    let (app, state, agent) = app_with_session().await;
    let path = std::env::temp_dir().join(format!(
        "anima-session-create-{}.json",
        uuid::Uuid::new_v4()
    ));
    let store = ControlPlaneStoreConfig::Json(path.clone());
    state
        .write()
        .await
        .set_control_plane_store(Some(store.clone()));
    let create = format!("/api/agents/{agent}/sessions");

    let empty = Request::builder()
        .method("POST")
        .uri(&create)
        .header("host", "127.0.0.1:8080")
        .header("origin", OWNER_ORIGIN)
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(empty).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let created = json(response).await;
    let id = created["session"]["id"].as_str().unwrap().to_string();
    assert!(id.starts_with("chat:"), "{id}");
    assert_eq!(created["session"]["title"], "New chat");
    assert_eq!(created["session"]["titleSource"], "first_message");
    assert_eq!(created["session"]["kind"], "chat");
    let saved = load_control_plane_snapshot(&store).await.unwrap().unwrap();
    assert!(
        saved.sessions.iter().any(|session| session.id == id),
        "the new chat is durable"
    );

    let titled = json(
        app.clone()
            .oneshot(send(
                "POST",
                &create,
                OWNER_ORIGIN,
                serde_json::json!({"title": "  Trip   plans "}),
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(titled["session"]["title"], "Trip plans");
    assert_eq!(titled["session"]["titleSource"], "owner");

    for (uri, body, status, error) in [
        (
            create.clone(),
            serde_json::json!({"title": ""}),
            StatusCode::BAD_REQUEST,
            "title must be 1 to 120 characters",
        ),
        (
            "/api/agents/missing/sessions".to_string(),
            serde_json::json!({}),
            StatusCode::NOT_FOUND,
            "not found",
        ),
    ] {
        let response = app
            .clone()
            .oneshot(send("POST", &uri, OWNER_ORIGIN, body))
            .await
            .unwrap();
        assert_eq!(response.status(), status, "{uri}");
        assert_eq!(json(response).await["error"], error);
    }

    {
        let mut guard = state.write().await;
        let now = anima_core::primitives::now_millis();
        while guard.session_limiter.try_acquire(&agent, now) {}
    }
    let response = app
        .clone()
        .oneshot(send("POST", &create, OWNER_ORIGIN, serde_json::json!({})))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(
        json(response).await["error"],
        "Too many new chats; try again in a minute"
    );
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn a_failed_save_leaves_no_new_chat_behind() {
    // Minor 4 (fix round 1, M2 review): create's rollback was untested.
    use crate::control_plane_store::ControlPlaneStoreConfig;

    let (app, state, agent) = app_with_session().await;
    let invalid_path = invalid_snapshot_directory("session-create");
    state
        .write()
        .await
        .set_control_plane_store(Some(ControlPlaneStoreConfig::Json(invalid_path.clone())));

    let response = app
        .clone()
        .oneshot(send(
            "POST",
            &format!("/api/agents/{agent}/sessions"),
            OWNER_ORIGIN,
            serde_json::json!({}),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response.headers()["cache-control"], "no-store");

    let ids = state
        .read()
        .await
        .sessions
        .records()
        .filter(|record| record.agent_id == agent)
        .map(|record| record.id.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        ["chat:plans"],
        "the failed create's session was rolled back"
    );

    std::fs::remove_dir_all(invalid_path).unwrap();
}

#[tokio::test]
async fn patching_renames_archives_and_marks_a_session_read() {
    let (app, state, agent) = app_with_session().await;
    state.write().await.sessions.insert(SessionRecord::new(
        &agent,
        "job:job-1",
        SessionKind::Job,
        SessionOrigin::Job,
        "Job · Weekly report".into(),
        TitleSource::System,
        1,
    ));
    let chat = format!("/api/agents/{agent}/sessions/chat%3Aplans");
    let job = format!("/api/agents/{agent}/sessions/job%3Ajob-1");

    let renamed = json(
        app.clone()
            .oneshot(send(
                "PATCH",
                &chat,
                OWNER_ORIGIN,
                serde_json::json!({"title": "Offsite", "lastReadAtMs": 2}),
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(renamed["session"]["title"], "Offsite");
    assert_eq!(renamed["session"]["titleSource"], "owner");
    assert_eq!(renamed["session"]["lastReadAtMs"], 2);
    assert_eq!(renamed["session"]["unread"], false);
    let archived = json(
        app.clone()
            .oneshot(send(
                "PATCH",
                &job,
                OWNER_ORIGIN,
                serde_json::json!({"archived": true}),
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(archived["session"]["archived"], true);
    assert!(
        state
            .read()
            .await
            .sessions
            .get(&agent, "job:job-1")
            .unwrap()
            .archived
    );

    for (uri, body, status, error) in [
        (
            chat.clone(),
            serde_json::json!({}),
            StatusCode::BAD_REQUEST,
            "at least one of title, archived, or lastReadAtMs is required",
        ),
        (
            chat.clone(),
            serde_json::json!({"lastReadAtMs": u64::MAX}),
            StatusCode::BAD_REQUEST,
            "lastReadAtMs must not be in the future",
        ),
        (
            chat.clone(),
            serde_json::json!({"title": " "}),
            StatusCode::BAD_REQUEST,
            "title must be 1 to 120 characters",
        ),
        (
            job.clone(),
            serde_json::json!({"title": "Renamed"}),
            StatusCode::CONFLICT,
            "This session cannot be renamed",
        ),
        (
            format!("/api/agents/{agent}/sessions/chat%3Amissing"),
            serde_json::json!({"archived": true}),
            StatusCode::NOT_FOUND,
            "not found",
        ),
    ] {
        let response = app
            .clone()
            .oneshot(send("PATCH", &uri, OWNER_ORIGIN, body))
            .await
            .unwrap();
        assert_eq!(response.status(), status, "{uri}");
        assert_eq!(response.headers()["cache-control"], "no-store");
        assert_eq!(json(response).await["error"], error, "{uri}");
    }
}

#[tokio::test]
async fn a_failed_save_restores_the_previous_record_on_patch() {
    // Minor 4 (fix round 1, M2 review): PATCH's rollback was untested.
    use crate::control_plane_store::ControlPlaneStoreConfig;

    let (app, state, agent) = app_with_session().await;
    let invalid_path = invalid_snapshot_directory("session-patch");
    state
        .write()
        .await
        .set_control_plane_store(Some(ControlPlaneStoreConfig::Json(invalid_path.clone())));

    let response = app
        .clone()
        .oneshot(send(
            "PATCH",
            &format!("/api/agents/{agent}/sessions/chat%3Aplans"),
            OWNER_ORIGIN,
            serde_json::json!({"title": "Renamed", "archived": true}),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response.headers()["cache-control"], "no-store");

    let record = state
        .read()
        .await
        .sessions
        .get(&agent, "chat:plans")
        .unwrap()
        .clone();
    assert_eq!(
        record.title, "Plans",
        "the previous record is restored, not the failed rename"
    );
    assert_eq!(record.title_source, TitleSource::FirstMessage);
    assert!(!record.archived, "the failed archive did not stick either");

    std::fs::remove_dir_all(invalid_path).unwrap();
}

#[tokio::test]
async fn deleting_a_chat_removes_its_record_messages_and_history_rows() {
    // Important 2's success case (fix round 1, M2 review): a real JSON
    // control-plane store, so the *saved* snapshot -- not just in-memory
    // state -- is asserted to hold the session's `HistoryDeletion` and none
    // of its runs.
    use crate::control_plane_store::{load_control_plane_snapshot, ControlPlaneStoreConfig};

    let (app, state, agent) = app_with_session().await;
    seed_messages(
        &mut *state.write().await,
        &agent,
        vec![message(
            &agent,
            "k1",
            "chat:keep",
            MessageRole::User,
            "keep me",
            3,
        )],
    );
    let run_id = {
        let mut guard = state.write().await;
        let mut run = crate::runs::RunRecord::running(
            crate::runs::RunStart {
                agent_id: agent.clone(),
                session_id: "chat:plans".into(),
                source: crate::runs::RunSource::Api,
                source_ref: None,
                idempotency_key: None,
                text: "done already".into(),
                model: "gpt-5.4".into(),
                provider: None,
                parent_run_id: None,
            },
            1,
        );
        run.finish(crate::runs::RunStatus::Completed, None, 1);
        let id = run.id.clone();
        guard.runs.insert(run);
        id
    };
    let history = state.read().await.history.clone();
    history
        .flush_once(&state, &tokio::sync::Mutex::new(()), 10)
        .await
        .unwrap();
    // Minor 7 (fix round 1, M2 review): confirm `chat:plans`'s rows are
    // really mirrored before deleting, not just that they are absent
    // afterward (which would also be true if they had never been mirrored).
    let mirrored_before_delete = history
        .store()
        .page_messages(&crate::history::MessagePageQuery {
            agent_id: agent.clone(),
            session_id: "chat:plans".into(),
            before: None,
            limit: 10,
            include_hidden: true,
        })
        .await
        .unwrap();
    assert_eq!(
        mirrored_before_delete.len(),
        2,
        "chat:plans's messages are mirrored before the delete"
    );
    let session = format!("/api/agents/{agent}/sessions/chat%3Aplans");
    let path = std::env::temp_dir().join(format!(
        "anima-session-delete-durable-{}.json",
        uuid::Uuid::new_v4()
    ));
    let store = ControlPlaneStoreConfig::Json(path.clone());
    state
        .write()
        .await
        .set_control_plane_store(Some(store.clone()));

    let response = app
        .clone()
        .oneshot(send(
            "DELETE",
            &session,
            OWNER_ORIGIN,
            serde_json::Value::Null,
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(json(response).await["deleted"], true);
    {
        let guard = state.read().await;
        assert!(guard.sessions.get(&agent, "chat:plans").is_none());
        let rooms = guard
            .get_agent(&agent)
            .unwrap()
            .messages
            .into_iter()
            .map(|message| message.room_id)
            .collect::<Vec<_>>();
        assert_eq!(rooms, ["chat:keep"], "other rooms keep their messages");
        assert!(
            guard.runs.get(&run_id).is_none(),
            "the session's terminal run leaves the ledger"
        );
    }
    let saved = load_control_plane_snapshot(&store).await.unwrap().unwrap();
    assert!(
        saved.pending_history_deletions.iter().any(|deletion| {
            deletion.agent_id == agent && deletion.session_id.as_deref() == Some("chat:plans")
        }),
        "the saved snapshot records the session's deletion"
    );
    assert!(
        saved.runs.iter().all(|run| run.session_id != "chat:plans"),
        "the saved snapshot holds none of the session's runs"
    );

    history
        .flush_once(&state, &tokio::sync::Mutex::new(()), 11)
        .await
        .unwrap();
    let rows = history
        .store()
        .page_messages(&crate::history::MessagePageQuery {
            agent_id: agent.clone(),
            session_id: "chat:plans".into(),
            before: None,
            limit: 10,
            include_hidden: true,
        })
        .await
        .unwrap();
    assert!(rows.is_empty(), "the history rows are deleted too");
    assert_eq!(
        history.store().get_run(&run_id).await.unwrap(),
        None,
        "the deleted session's run is never mirrored"
    );
    assert!(state.read().await.pending_history_deletions.is_empty());
    let response = app
        .clone()
        .oneshot(get(&session, OWNER_ORIGIN))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn a_failed_delete_save_restores_the_record_messages_and_runs() {
    // Important 2 (fix round 1, M2 review): the durable protocol and its
    // rollback were untested. If `clear_history_deletion` regressed, nothing
    // here would fail and a restart would replay a deletion of a session
    // that is, in fact, still there -- data loss.
    use crate::control_plane_store::ControlPlaneStoreConfig;

    let (app, state, agent) = app_with_session().await;
    let run_id = {
        let mut guard = state.write().await;
        let mut run = crate::runs::RunRecord::running(
            crate::runs::RunStart {
                agent_id: agent.clone(),
                session_id: "chat:plans".into(),
                source: crate::runs::RunSource::Api,
                source_ref: None,
                idempotency_key: None,
                text: "done already".into(),
                model: "gpt-5.4".into(),
                provider: None,
                parent_run_id: None,
            },
            1,
        );
        run.finish(crate::runs::RunStatus::Completed, None, 1);
        let id = run.id.clone();
        guard.runs.insert(run);
        id
    };
    let invalid_path = invalid_snapshot_directory("session-delete");
    state
        .write()
        .await
        .set_control_plane_store(Some(ControlPlaneStoreConfig::Json(invalid_path.clone())));

    let response = app
        .clone()
        .oneshot(send(
            "DELETE",
            &format!("/api/agents/{agent}/sessions/chat%3Aplans"),
            OWNER_ORIGIN,
            serde_json::Value::Null,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response.headers()["cache-control"], "no-store");

    let guard = state.read().await;
    assert!(
        guard.sessions.get(&agent, "chat:plans").is_some(),
        "the record is restored"
    );
    let message_ids = guard
        .get_agent(&agent)
        .unwrap()
        .messages
        .iter()
        .map(|message| message.id.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        message_ids,
        ["m1", "m2"],
        "the room's hot messages are restored"
    );
    assert!(
        guard.runs.get(&run_id).is_some(),
        "the session's terminal run is restored"
    );
    assert!(
        guard.pending_history_deletions.is_empty(),
        "a failed save leaves no pending entry"
    );
    drop(guard);

    std::fs::remove_dir_all(invalid_path).unwrap();
}

#[tokio::test]
async fn deleting_is_refused_by_kind_and_while_a_run_is_active() {
    let (app, state, agent) = app_with_session().await;
    state.write().await.sessions.insert(SessionRecord::new(
        &agent,
        "telegram:bot",
        SessionKind::Telegram,
        SessionOrigin::Telegram,
        "Telegram · @bot".into(),
        TitleSource::System,
        1,
    ));
    state
        .write()
        .await
        .runs
        .insert(crate::runs::RunRecord::running(
            crate::runs::RunStart {
                agent_id: agent.clone(),
                session_id: "chat:plans".into(),
                source: crate::runs::RunSource::Web,
                source_ref: None,
                idempotency_key: None,
                text: "still working".into(),
                model: "gpt-5.4".into(),
                provider: None,
                parent_run_id: None,
            },
            1,
        ));

    for (uri, error) in [
        (
            format!("/api/agents/{agent}/sessions/telegram%3Abot"),
            "This session cannot be deleted",
        ),
        (
            format!("/api/agents/{agent}/sessions/chat%3Aplans"),
            "A run in this session is still in progress",
        ),
    ] {
        let response = app
            .clone()
            .oneshot(send("DELETE", &uri, OWNER_ORIGIN, serde_json::Value::Null))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT, "{uri}");
        assert_eq!(response.headers()["cache-control"], "no-store");
        assert_eq!(json(response).await["error"], error);
    }
    assert!(state
        .read()
        .await
        .sessions
        .get(&agent, "chat:plans")
        .is_some());
}

#[tokio::test]
async fn deleting_a_session_whose_room_a_run_holds_is_refused_then_succeeds() {
    // Minor 2 (fix round 1, M2 review): `try_reserve_room`'s refusal branch
    // was never exercised -- the test above only inserts a `RunRecord`
    // directly, without ever taking the room's real lock, so it only reaches
    // the ledger recheck, not `try_reserve_room` itself.
    let (adapter, entered, release) = pending_adapter();
    let mut daemon = DaemonState::with_model_adapter(adapter);
    let agent = daemon
        .create_agent(test_config("companion"))
        .unwrap()
        .state
        .id;
    daemon.sessions.insert(SessionRecord::new(
        &agent,
        "chat:held",
        SessionKind::Chat,
        SessionOrigin::Web,
        "Held".into(),
        TitleSource::FirstMessage,
        1,
    ));
    let state = Arc::new(RwLock::new(daemon));
    let runs = AgentRunCoordinator::new(Arc::clone(&state), Arc::new(Semaphore::new(4)));
    let app = custom_router(Arc::clone(&state), runs.clone(), DaemonConfig::default());

    let held = {
        let runs = runs.clone();
        let agent_id = agent.clone();
        tokio::spawn(async move {
            runs.run(crate::agent_runs::AgentRunRequest {
                agent_id,
                content: Content {
                    text: "hold the room".into(),
                    attachments: None,
                    metadata: None,
                },
                room: crate::agent_runs::RunRoom::Stable("chat:held".into()),
                idempotency_key: None,
                source: crate::runs::RunSource::Web,
                source_ref: None,
                parent: None,
            })
            .await
        })
    };
    entered.acquire().await.unwrap().forget();

    assert!(
        runs.try_reserve_room(&agent, "chat:held").is_none(),
        "the room's lock is held by the in-flight run"
    );
    let session = format!("/api/agents/{agent}/sessions/chat%3Aheld");
    let response = app
        .clone()
        .oneshot(send(
            "DELETE",
            &session,
            OWNER_ORIGIN,
            serde_json::Value::Null,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(
        json(response).await["error"],
        "A run in this session is still in progress"
    );

    release.add_permits(1);
    held.await
        .expect("the held run should join")
        .expect("the held run should finish");

    let response = app
        .oneshot(send(
            "DELETE",
            &session,
            OWNER_ORIGIN,
            serde_json::Value::Null,
        ))
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "the room is free once the run finishes"
    );
}

#[tokio::test]
async fn exporting_a_session_returns_its_full_markdown_transcript() {
    let (app, state, agent) = app_with_session().await;
    state
        .read()
        .await
        .history
        .store()
        .upsert_messages(&[crate::history::conformance::history_message(
            "m0",
            &agent,
            "chat:plans",
            MessageRole::User,
            "An archived question",
            0,
        )])
        .await
        .unwrap();

    let response = app
        .clone()
        .oneshot(get(
            &format!("/api/agents/{agent}/sessions/chat%3Aplans/export"),
            OWNER_ORIGIN,
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()["content-type"],
        "text/markdown; charset=utf-8"
    );
    assert_eq!(
        response.headers()["content-disposition"],
        "attachment; filename=\"plans.md\""
    );
    assert_eq!(response.headers()["cache-control"], "no-store");
    let body = String::from_utf8(
        to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(body.starts_with("# Plans\n"), "{body}");
    let archived = body.find("An archived question").unwrap();
    let question = body.find("Plan the offsite").unwrap();
    let answer = body.find("Here is the plan").unwrap();
    assert!(archived < question && question < answer, "{body}");
    assert!(body.contains("**You** · 1970-01-01 00:00 UTC"), "{body}");
    assert!(
        body.contains("**companion** · 1970-01-01 00:00 UTC"),
        "{body}"
    );
    let missing = app
        .clone()
        .oneshot(get(
            &format!("/api/agents/{agent}/sessions/chat%3Amissing/export"),
            OWNER_ORIGIN,
        ))
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn exporting_a_session_keeps_messages_longer_than_the_search_index_cap_whole() {
    // Residual round R1 (M2 final fix wave re-review): search indexes only the
    // first MAX_INDEXED_TEXT_BYTES of a message, but the export promises the
    // full transcript, so longer messages (kept in the store or in the hot
    // tail) must come out whole.
    let (app, state, agent) = app_with_session().await;
    let filler = "x".repeat(crate::history::MAX_INDEXED_TEXT_BYTES);
    let stored_text = format!("stored alphaneedle {filler} stored omeganeedle");
    let hot_text = format!("hot alphaneedle {filler} hot omeganeedle");
    state
        .read()
        .await
        .history
        .store()
        .upsert_messages(&[history_message(
            "m0",
            &agent,
            "chat:plans",
            MessageRole::Tool,
            &stored_text,
            0,
        )])
        .await
        .unwrap();
    seed_messages(
        &mut *state.write().await,
        &agent,
        vec![message(
            &agent,
            "m3",
            "chat:plans",
            MessageRole::Assistant,
            &hot_text,
            3,
        )],
    );

    let response = app
        .clone()
        .oneshot(get(
            &format!("/api/agents/{agent}/sessions/chat%3Aplans/export"),
            OWNER_ORIGIN,
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = String::from_utf8(
        to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(
        body.contains(&stored_text),
        "a stored message over the index cap is exported whole"
    );
    assert!(
        body.contains(&hot_text),
        "a hot message over the index cap is exported whole"
    );
}

#[tokio::test]
async fn exporting_a_session_with_an_unreadable_store_answers_service_unavailable() {
    // Minor 3 (fix round 1, M2 review): the export's 503 string was unasserted.
    use crate::history::conformance::FlakyHistoryStore;
    use crate::history::HistoryService;

    let flaky = Arc::new(FlakyHistoryStore::new());
    let mut daemon = DaemonState::new();
    daemon.set_history(HistoryService::new(flaky.clone()));
    let agent = daemon
        .create_agent(test_config("companion"))
        .unwrap()
        .state
        .id;
    daemon.sessions.insert(SessionRecord::new(
        &agent,
        "chat:plans",
        SessionKind::Chat,
        SessionOrigin::Web,
        "Plans".into(),
        TitleSource::FirstMessage,
        1,
    ));
    let state = Arc::new(RwLock::new(daemon));
    let app = router(state, DaemonConfig::default());
    flaky.set_failing(true);

    let response = app
        .oneshot(get(
            &format!("/api/agents/{agent}/sessions/chat%3Aplans/export"),
            OWNER_ORIGIN,
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(
        json(response).await["error"],
        "history store is unavailable"
    );
}

#[test]
fn the_openapi_document_lists_the_session_mutation_routes() {
    use utoipa::OpenApi;

    let paths = crate::routes::ApiDoc::openapi().paths.paths;
    assert!(paths["/api/agents/{agent_id}/sessions"].post.is_some());
    let session = &paths["/api/agents/{agent_id}/sessions/{session_id}"];
    assert!(session.patch.is_some() && session.delete.is_some());
    assert!(paths["/api/agents/{agent_id}/sessions/{session_id}/export"]
        .get
        .is_some());
}
