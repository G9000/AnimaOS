//! Free / Strict / Custom over HTTP: new agents start Free, presets set the
//! class policy, a hand-made class choice reads as Custom, and the owner's
//! blocked commands are validated.
use super::*;

use serde_json::{json, Value};

const OWNER_ORIGIN: &str = "http://localhost:4200";

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
) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(request(method, uri, OWNER_ORIGIN, body))
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn new_agents_start_free_and_presets_set_the_class_policy() {
    let state = Arc::new(RwLock::new(DaemonState::new()));
    let app = router(state, DaemonConfig::default());
    let (status, created) = send(
        &app,
        "POST",
        "/api/agents",
        Some(json!({"name": "Coder", "model": "gpt-test", "provider": "openai"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let id = created["agent"]["state"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let (status, autonomy) = send(&app, "GET", &format!("/api/agents/{id}/autonomy"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(autonomy["mode"], "free");
    assert_eq!(autonomy["smartCheck"], false);
    assert_eq!(
        autonomy["policy"],
        json!({"write": "allow", "exec": "allow", "network": "allow", "delegate": "allow"})
    );

    let (status, strict) = send(
        &app,
        "PUT",
        &format!("/api/agents/{id}/autonomy"),
        Some(json!({"mode": "strict", "smartCheck": true, "denyPatterns": ["git push *"]})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{strict}");
    assert_eq!(strict["mode"], "strict");
    assert_eq!(strict["smartCheck"], true);
    assert_eq!(strict["denyPatterns"], json!(["git push *"]));
    assert_eq!(
        strict["policy"],
        json!({"write": "ask", "exec": "ask", "network": "allow", "delegate": "ask"})
    );

    // A class choice made by hand reads as Custom.
    let (status, _) = send(
        &app,
        "PUT",
        &format!("/api/agents/{id}/approval-policy"),
        Some(json!({"write": "allow", "exec": "ask", "network": "allow", "delegate": "allow"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, custom) = send(&app, "GET", &format!("/api/agents/{id}/autonomy"), None).await;
    assert_eq!(custom["mode"], "custom");
    assert_eq!(
        custom["denyPatterns"],
        json!(["git push *"]),
        "extras survive"
    );
}

#[tokio::test]
async fn custom_cannot_be_chosen_directly_and_blocked_commands_are_checked() {
    let mut daemon = DaemonState::new();
    let id = daemon
        .create_agent(crate::agent_runs::test_support::companion_config("Older"))
        .unwrap()
        .state
        .id;
    let app = router(Arc::new(RwLock::new(daemon)), DaemonConfig::default());
    let (_, older) = send(&app, "GET", &format!("/api/agents/{id}/autonomy"), None).await;
    assert_eq!(
        older["mode"], "custom",
        "an agent from before autonomy keeps its policy"
    );
    assert_eq!(older["policy"]["exec"], "ask");
    for body in [
        json!({"mode": "custom"}),
        json!({"denyPatterns": [""]}),
        json!({"denyPatterns": ["x".repeat(201)]}),
        json!({"surprise": true}),
    ] {
        let (status, _) = send(
            &app,
            "PUT",
            &format!("/api/agents/{id}/autonomy"),
            Some(body.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    }
    let (status, _) = send(&app, "GET", "/api/agents/missing/autonomy", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn an_older_agent_reads_as_the_preset_its_policy_equals() {
    let mut daemon = DaemonState::new();
    let open = daemon
        .create_agent(crate::agent_runs::test_support::companion_config("Open"))
        .unwrap()
        .state
        .id;
    let careful = daemon
        .create_agent(crate::agent_runs::test_support::companion_config("Careful"))
        .unwrap()
        .state
        .id;
    daemon.approvals.set_policy(
        &open,
        crate::approvals::autonomy::AutonomyMode::Free
            .preset()
            .unwrap(),
    );
    daemon.approvals.set_policy(
        &careful,
        crate::approvals::autonomy::AutonomyMode::Strict
            .preset()
            .unwrap(),
    );
    let app = router(Arc::new(RwLock::new(daemon)), DaemonConfig::default());
    let (_, open) = send(&app, "GET", &format!("/api/agents/{open}/autonomy"), None).await;
    assert_eq!(open["mode"], "free");
    let (_, careful) = send(
        &app,
        "GET",
        &format!("/api/agents/{careful}/autonomy"),
        None,
    )
    .await;
    assert_eq!(careful["mode"], "strict");
}

#[tokio::test]
async fn autonomy_routes_require_the_owner() {
    let mut daemon = DaemonState::new();
    let id = daemon
        .create_agent(crate::agent_runs::test_support::companion_config("Owner"))
        .unwrap()
        .state
        .id;
    let app = router(Arc::new(RwLock::new(daemon)), DaemonConfig::default());
    for (method, body) in [("GET", None), ("PUT", Some(json!({"mode": "free"})))] {
        let response = app
            .clone()
            .oneshot(request(
                method,
                &format!("/api/agents/{id}/autonomy"),
                "https://evil.test",
                body,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{method}");
    }
}
