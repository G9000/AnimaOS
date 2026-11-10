use super::*;
use serde_json::{json, Value};
fn request(id: &str, method: &str, body: Value) -> Request<Body> {
    crate::routes::owner_test_support::authenticate(
        Request::builder()
            .method(method)
            .uri(format!("/api/agents/{id}/workspace-context"))
            .header("host", "127.0.0.1:8080")
            .header("origin", "http://localhost:4200"),
    )
    .header("content-type", "application/json")
    .body(if method == "GET" {
        Body::empty()
    } else {
        Body::from(body.to_string())
    })
    .unwrap()
}
#[tokio::test]
async fn workspace_context_owner_view_is_independent_and_no_store() {
    let state = Arc::new(RwLock::new(DaemonState::new()));
    let id = state
        .write()
        .await
        .create_agent(test_config("context"))
        .unwrap()
        .state
        .id;
    let app = router(state, DaemonConfig::default());
    let response = app.oneshot(request(&id, "GET", Value::Null)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let value: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(value["status"], "independent");
    assert_eq!(value["binding"], Value::Null);
    assert!(value["revision"].as_str().is_some());
    assert_eq!(value["connections"], json!([]));
}
#[tokio::test]
async fn workspace_context_disable_is_durable_and_revision_checked() {
    let fixture = WorkspaceAvatarTemp::new("workspace-context-cas");
    let state = fixture.state();
    state.write().await.set_control_plane_store(Some(
        crate::control_plane_store::ControlPlaneStoreConfig::Json(
            fixture.root.join("control.json"),
        ),
    ));
    let id = state
        .write()
        .await
        .create_agent(test_config("context"))
        .unwrap()
        .state
        .id;
    let app = router(state.clone(), DaemonConfig::default());
    let response = app
        .clone()
        .oneshot(request(&id, "GET", Value::Null))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let view: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    let response = app
        .clone()
        .oneshot(request(
            &id,
            "PUT",
            json!({"expectedRevision":"stale","binding":null}),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let response = app
        .clone()
        .oneshot(request(
            &id,
            "PUT",
            json!({"expectedRevision":view["revision"]}),
        ))
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "binding key must be explicit to clear it"
    );
    let response = app
        .clone()
        .oneshot(request(
            &id,
            "PUT",
            json!({"expectedRevision":view["revision"],"binding":null}),
        ))
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(
        status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&bytes)
    );
    assert!(fixture.root.join("control.json").exists());
    let response=app.oneshot(request(&id,"PUT",json!({"expectedRevision":view["revision"],"binding":{"connectionId":"foreign","tool":"mother_get_context","mode":"companion","workspaceId":"workspace","generation":"gen","subject":id}}))).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
