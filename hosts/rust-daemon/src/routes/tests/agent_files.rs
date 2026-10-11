use super::*;
use serde_json::{json, Value};

fn preview_request(id: &str, origin: &str) -> Request<Body> {
    Request::builder()
        .uri(format!("/api/agents/{id}/agent-files"))
        .header("host", "127.0.0.1:8080")
        .header("origin", origin)
        .body(Body::empty())
        .unwrap()
}

#[tokio::test]
async fn agent_files_preview_requires_owner_and_exposes_only_current_metadata() {
    let fixture = WorkspaceAvatarTemp::new("agent-files");
    let state = fixture.state();
    let id = state
        .write()
        .await
        .create_agent(test_config("fixture"))
        .unwrap()
        .state
        .id;
    std::fs::write(
        fixture.root.join("AGENTS.md"),
        "Use clear prose. Private sentinel abcdef.",
    )
    .unwrap();
    let app = router(state, DaemonConfig::default());
    let denied = app
        .clone()
        .oneshot(preview_request(&id, "https://untrusted.example"))
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    let response = app
        .clone()
        .oneshot(preview_request(&id, "http://localhost:4200"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let raw = std::str::from_utf8(&bytes).unwrap();
    assert!(!raw.contains("Private sentinel"));
    assert!(!raw.contains(&fixture.root.to_string_lossy().to_string()));
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["mode"], "current_preview");
    assert_eq!(body["projectSourceKind"], "agents");
    assert!(body["capturedAtMs"].as_u64().unwrap() > 0);
    assert_eq!(
        body["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["relativePath"] == "AGENTS.md")
            .unwrap()["state"],
        "loaded"
    );
    let missing = app
        .oneshot(preview_request("missing", "http://localhost:4200"))
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn agent_files_preview_without_workspace_has_no_ambient_sources() {
    let mut daemon = DaemonState::new();
    let id = daemon
        .create_agent(test_config("fixture"))
        .unwrap()
        .state
        .id;
    let app = router(Arc::new(RwLock::new(daemon)), DaemonConfig::default());
    let response = app
        .oneshot(preview_request(&id, "http://localhost:4200"))
        .await
        .unwrap();
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(body["availability"], "unavailable_workspace");
    assert_eq!(body["entries"], json!([]));
}

#[test]
fn profile_setting_is_boolean_and_never_a_caller_supplied_path() {
    for value in [json!("C:/private"), json!(1), json!({"path":"outside"})] {
        let request: crate::routes::contracts::AgentConfigRequest = serde_json::from_value(json!({"name":"fixture", "model":"fixture", "settings":{"agentFilesProfileEnabled":value}})).unwrap();
        assert_eq!(
            request.into_domain().unwrap_err(),
            "agentFilesProfileEnabled must be a boolean"
        );
    }
    let request: crate::routes::contracts::AgentConfigRequest = serde_json::from_value(
        json!({"name":"fixture", "model":"fixture", "settings":{"agentFilesProfileEnabled":true}}),
    )
    .unwrap();
    assert_eq!(
        request.into_domain().unwrap().settings.unwrap().additional["agentFilesProfileEnabled"],
        anima_core::DataValue::Bool(true)
    );
}
