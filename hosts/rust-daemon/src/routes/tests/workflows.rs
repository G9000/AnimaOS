use super::*;
use axum::Router;

async fn response_json(response: axum::response::Response) -> serde_json::Value {
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

async fn workflow_request(
    app: Router,
    method: &str,
    uri: &str,
    input: serde_json::Value,
) -> axum::response::Response {
    app.oneshot(
        crate::routes::owner_test_support::authenticate(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("host", "127.0.0.1:8080")
                .header("origin", "http://localhost:4200"),
        )
        .header("content-type", "application/json")
        .body(Body::from(input.to_string()))
        .unwrap(),
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn workflows_http_checkpoint_decisions_are_durable_idempotent_and_owned() {
    use crate::control_plane_store::{load_control_plane_snapshot, ControlPlaneStoreConfig};
    let file = std::env::temp_dir().join(format!(
        "anima-workflows-http-{}.json",
        uuid::Uuid::new_v4()
    ));
    let mut daemon = DaemonState::new();
    daemon.set_control_plane_store(Some(ControlPlaneStoreConfig::Json(file.clone())));
    let owner = daemon
        .create_agent(test_config("workflow-owner"))
        .unwrap()
        .state
        .id;
    let other = daemon
        .create_agent(test_config("workflow-other"))
        .unwrap()
        .state
        .id;
    let state = Arc::new(RwLock::new(daemon));
    let app = router(state.clone(), DaemonConfig::default());
    let uri = format!("/api/agents/{owner}/workflows");
    let definition = serde_json::json!({"title":"Reviewed report", "requestKey":"weekly-report", "steps":[
        {"id":"review", "title":"Review scope", "kind":"approval", "prompt":"Approve the report scope"},
        {"id":"audience", "title":"Choose audience", "kind":"input", "prompt":"Who is the report for?"}
    ]});
    let response = workflow_request(app.clone(), "POST", &uri, definition.clone()).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let created = response_json(response).await;
    assert_eq!(created["status"], "awaiting_approval");
    let id = created["id"].as_str().unwrap();
    let revision = created["revision"].as_u64().unwrap();
    let retry =
        response_json(workflow_request(app.clone(), "POST", &uri, definition.clone()).await).await;
    assert_eq!(retry["id"], id);
    let mut changed = definition;
    changed["title"] = serde_json::json!("Different report");
    assert_eq!(
        workflow_request(app.clone(), "POST", &uri, changed)
            .await
            .status(),
        StatusCode::CONFLICT
    );
    let resume_uri = format!("{uri}/{id}/resume");
    for invalid in [
        serde_json::json!({"revision":revision,"action":"answer"}),
        serde_json::json!({"revision":revision,"action":"answer","value":"   "}),
        serde_json::json!({"revision":revision,"action":"approve","value":"unexpected"}),
        serde_json::json!({"revision":0,"action":"approve"}),
        serde_json::json!({"revision":revision,"action":"retry"}),
        serde_json::json!({"revision":revision,"action":"resolve","acknowledgeUncertain":true}),
    ] {
        assert_eq!(
            workflow_request(app.clone(), "POST", &resume_uri, invalid)
                .await
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
    let decision = serde_json::json!({"revision":revision,"action":"approve"});
    let approved =
        response_json(workflow_request(app.clone(), "POST", &resume_uri, decision.clone()).await)
            .await;
    assert_eq!(approved["status"], "awaiting_input");
    assert_eq!(approved["steps"][0]["status"], "completed");
    let replay =
        response_json(workflow_request(app.clone(), "POST", &resume_uri, decision).await).await;
    assert_eq!(
        replay, approved,
        "A repeated decision must not advance another checkpoint"
    );
    assert_eq!(
        workflow_request(
            app.clone(),
            "POST",
            &resume_uri,
            serde_json::json!({"revision":revision,"action":"answer","value":"Changed decision"})
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        workflow_request(
            app.clone(),
            "GET",
            &format!("/api/agents/{other}/workflows/{id}"),
            serde_json::Value::Null
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    let snapshot = load_control_plane_snapshot(&ControlPlaneStoreConfig::Json(file.clone()))
        .await
        .unwrap()
        .unwrap();
    let mut restored = DaemonState::new();
    restored.set_control_plane_store(Some(ControlPlaneStoreConfig::Json(file.clone())));
    restored.restore_control_plane_snapshot(snapshot).unwrap();
    let app = router(Arc::new(RwLock::new(restored)), DaemonConfig::default());
    let after_restart = response_json(
        workflow_request(
            app.clone(),
            "GET",
            &format!("{uri}/{id}"),
            serde_json::Value::Null,
        )
        .await,
    )
    .await;
    assert_eq!(after_restart, approved);
    let answered = response_json(workflow_request(app, "POST", &resume_uri,
        serde_json::json!({"revision":approved["revision"],"action":"answer","value":"The operations team"})).await).await;
    assert_eq!(answered["status"], "completed");
    assert_eq!(answered["steps"][1]["result"], "The operations team");
    assert!(
        state.read().await.jobs.is_empty(),
        "Checkpoints do not invoke a model or tool"
    );
    std::fs::remove_file(file).unwrap();
}

#[tokio::test]
async fn workflows_http_guard_precedes_body_parsing_and_lookup() {
    let app = router(
        Arc::new(RwLock::new(DaemonState::new())),
        DaemonConfig::default(),
    );
    for (method, uri) in [
        ("GET", "/api/agents/missing/workflows"),
        ("POST", "/api/agents/missing/workflows"),
        ("GET", "/api/agents/missing/workflows/missing"),
        ("POST", "/api/agents/missing/workflows/missing/resume"),
        ("POST", "/api/agents/missing/workflows/missing/cancel"),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .header("host", "127.0.0.1:8080")
                    .header("origin", "https://untrusted.example")
                    .body(Body::from("not json"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{method} {uri}");
        assert_eq!(response.headers()["cache-control"], "no-store");
    }
}
