use super::*;
use anima_core::DataValue;

use crate::history::{HistoryService, SqliteHistoryStore};

const OWNER_ORIGIN: &str = "http://localhost:4200";

fn request(method: &str, uri: &str, origin: &str) -> Request<Body> {
    crate::routes::owner_test_support::authenticate(
        Request::builder()
            .method(method)
            .uri(uri)
            .header("host", "127.0.0.1:8080")
            .header("origin", origin),
    )
    .body(Body::empty())
    .unwrap()
}

async fn json(response: axum::response::Response) -> serde_json::Value {
    serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap()
}

fn agent_config(name: &str, helper: bool) -> AgentConfig {
    let mut settings = AgentSettings::default();
    if helper {
        settings
            .additional
            .insert("workspaceRole".into(), DataValue::String("helper".into()));
    }
    AgentConfig {
        name: name.into(),
        model: "gpt-5.4".into(),
        bio: None,
        lore: None,
        knowledge: None,
        topics: None,
        adjectives: None,
        style: None,
        provider: Some("openai".into()),
        system: None,
        tools: None,
        plugins: None,
        settings: Some(settings),
        fallbacks: Vec::new(),
    }
}

/// A Claude Code log of 450 alternating turns: the first mentions a
/// zebrafish (older than the hot tail), the last a quokka and a secret.
fn claude_fixture() -> String {
    let mut lines =
        vec![serde_json::json!({"type": "summary", "summary": "Ship the zebra migration"})];
    for index in 0..450 {
        let (kind, text) = match index {
            0 => ("user", "Plan the zebrafish rollout".to_string()),
            448 => (
                "user",
                "Last question about api_key=supersecretvalue1".to_string(),
            ),
            449 => ("assistant", "The quokka answer".to_string()),
            _ if index % 2 == 0 => ("user", format!("question {index}")),
            _ => ("assistant", format!("answer {index}")),
        };
        lines.push(serde_json::json!({
            "type": kind,
            "cwd": "/work/app",
            "sessionId": "claude-fixture-1",
            "timestamp": format!("2026-01-02T03:{:02}:{:02}Z", index / 60, index % 60),
            "message": {"role": kind, "content": text}
        }));
    }
    lines
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Every foreign-session route in one test: the roots come from process
/// environment variables, which no other test reads.
#[tokio::test]
async fn foreign_sessions_import_into_an_agent_and_become_searchable() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp
        .path()
        .join("claude")
        .join("projects")
        .join("-work-app");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("fixture.jsonl"), claude_fixture()).unwrap();
    let day = temp.path().join("codex").join("sessions").join("2026");
    std::fs::create_dir_all(&day).unwrap();
    std::fs::write(
        day.join("rollout-1.jsonl"),
        [
            serde_json::json!({"type": "session_meta", "payload": {"id": "codex-1", "cwd": "/work/codex"}}),
            serde_json::json!({"type": "response_item", "payload": {"type": "message", "role": "user",
                "content": [{"type": "input_text", "text": "Codex question"}]}}),
        ]
        .map(|line| line.to_string())
        .join("\n"),
    )
    .unwrap();
    std::fs::File::create(project.join("huge.jsonl"))
        .unwrap()
        .set_len(crate::foreign_sessions::MAX_FILE_BYTES + 1)
        .unwrap();
    std::env::set_var("CLAUDE_CONFIG_DIR", temp.path().join("claude"));
    std::env::set_var("CODEX_HOME", temp.path().join("codex"));

    let store = Arc::new(
        SqliteHistoryStore::open(temp.path().join("history.db"))
            .await
            .unwrap(),
    );
    let mut daemon = DaemonState::new();
    daemon.set_history(HistoryService::new(store));
    let agent = daemon
        .create_agent(agent_config("companion", false))
        .unwrap()
        .state
        .id;
    let helper = daemon
        .create_agent(agent_config("helper", true))
        .unwrap()
        .state
        .id;
    let state = Arc::new(RwLock::new(daemon));
    let app = router(state.clone(), DaemonConfig::default());
    let call = |method: &'static str, uri: String| {
        let app = app.clone();
        async move {
            app.oneshot(request(method, &uri, OWNER_ORIGIN))
                .await
                .unwrap()
        }
    };

    // List.
    let response = call("GET", "/api/foreign-sessions?source=claude-code".into()).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let body = json(response).await;
    let rows = body["sessions"].as_array().unwrap();
    let row = rows
        .iter()
        .find(|row| row["foreignSessionId"] == "claude-fixture-1")
        .expect("the fixture is listed");
    assert_eq!(row["source"], "claude-code");
    assert_eq!(row["title"], "Ship the zebra migration");
    assert_eq!(row["cwd"], "/work/app");
    assert!(row["modifiedAt"].as_u64().unwrap() > 0);
    assert!(row["sizeBytes"].as_u64().unwrap() > 0);
    assert!(row.get("path").is_none());
    assert_eq!(body["nextCursor"], serde_json::Value::Null);
    let handle = row["handle"].as_str().unwrap().to_string();
    let huge = rows
        .iter()
        .find(|row| row["sizeBytes"].as_u64() == Some(crate::foreign_sessions::MAX_FILE_BYTES + 1))
        .expect("an oversized log is still listed")["handle"]
        .as_str()
        .unwrap()
        .to_string();
    let all = json(call("GET", "/api/foreign-sessions".into()).await).await;
    assert!(all["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["source"] == "codex" && row["title"] == "Codex question"));
    for bad in ["?source=bogus", "?cursor=x", "?cursor=-1"] {
        let response = call("GET", format!("/api/foreign-sessions{bad}")).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{bad}");
    }

    // Preview.
    let response = call("GET", format!("/api/foreign-sessions/{handle}/preview")).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let preview = json(response).await;
    assert_eq!(preview["messageCount"], 450);
    assert_eq!(preview["source"], "claude-code");
    assert_eq!(preview["cwd"], "/work/app");
    let messages = preview["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 40);
    assert_eq!(messages[39]["text"], "The quokka answer");
    assert_eq!(messages[39]["role"], "assistant");
    assert!(messages[39]["at"].as_u64().is_some());
    assert_eq!(
        messages[38]["text"],
        "Last question about api_key=[redacted]"
    );
    assert_eq!(
        call("GET", format!("/api/foreign-sessions/{huge}/preview"))
            .await
            .status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert_eq!(
        call(
            "GET",
            format!("/api/foreign-sessions/{}/preview", "0".repeat(64))
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );

    // Import refusals.
    let import =
        |agent: &str, handle: &str| format!("/api/agents/{agent}/foreign-sessions/{handle}/import");
    assert_eq!(
        call("POST", import(&helper, &handle)).await.status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        call("POST", import("missing", &handle)).await.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call("POST", import(&agent, &"f".repeat(64))).await.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call("POST", import(&agent, &huge)).await.status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert_eq!(
        app.clone()
            .oneshot(request(
                "POST",
                &import(&agent, &handle),
                "https://evil.example"
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );

    // Import, then again.
    let response = call("POST", import(&agent, &handle)).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let imported = json(response).await;
    assert_eq!(imported["alreadyImported"], false);
    assert_eq!(imported["messageCount"], 450);
    assert_eq!(imported["title"], "Imported · Ship the zebra migration");
    let session_id = imported["sessionId"].as_str().unwrap().to_string();
    let again = call("POST", import(&agent, &handle)).await;
    assert_eq!(again.status(), StatusCode::OK);
    let again = json(again).await;
    assert_eq!(again["alreadyImported"], true);
    assert_eq!(again["sessionId"], session_id.as_str());

    // The session is a normal chat whose newest 200 messages are hot.
    {
        let guard = state.read().await;
        let record = guard.sessions.get(&agent, &session_id).unwrap();
        assert!(record.pruned_through.is_some());
        assert_eq!(
            guard
                .agents
                .get(&agent)
                .unwrap()
                .messages()
                .iter()
                .filter(|message| message.room_id == record.room_id())
                .count(),
            200
        );
    }
    let encoded = session_id.replace(':', "%3A");
    let session = json(call("GET", format!("/api/agents/{agent}/sessions/{encoded}")).await).await;
    assert_eq!(session["session"]["origin"], "import");
    assert_eq!(session["session"]["kind"], "chat");
    assert_eq!(session["session"]["capabilities"]["send"], true);
    assert_eq!(session["session"]["importedFrom"]["tool"], "claude-code");
    assert_eq!(
        session["session"]["importedFrom"]["foreignSessionId"],
        "claude-fixture-1"
    );
    assert_eq!(
        session["session"]["importedFrom"]["pathHash"],
        handle.as_str()
    );

    // Both the cold and the hot messages are searchable.
    for query in ["zebrafish", "quokka"] {
        let response = call(
            "GET",
            format!("/api/agents/{agent}/message-search?q={query}"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK, "{query}");
        let hits = json(response).await;
        let hit = &hits["results"][0];
        assert_eq!(hit["sessionId"], session_id.as_str(), "{query}: {hits}");
        assert!(hit["snippet"].as_str().unwrap().contains(query), "{hits}");
    }
    let secret = json(
        call(
            "GET",
            format!("/api/agents/{agent}/message-search?q=supersecretvalue1"),
        )
        .await,
    )
    .await;
    assert_eq!(secret["results"], serde_json::json!([]));
}
