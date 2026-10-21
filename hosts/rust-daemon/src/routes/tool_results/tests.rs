use crate::{
    control_plane_store::{ControlPlaneStoreConfig, WorkspaceConfig},
    state::DaemonState,
};
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use std::sync::Arc;
use tokio::sync::RwLock;
use tower::ServiceExt;

#[tokio::test]
async fn owner_routes_retrieve_exact_payload_without_cross_agent_or_unauthorized_reads() {
    let temp = tempfile::tempdir().unwrap();
    let mut state = DaemonState::new();
    state.workspace = Some(WorkspaceConfig {
        root_path: temp.path().to_path_buf(),
        company_name: "test".into(),
        mission: "test".into(),
        values: vec![],
    });
    state.set_control_plane_store(Some(ControlPlaneStoreConfig::Json(
        temp.path().join("control.json"),
    )));
    let config = || {
        serde_json::from_value(serde_json::json!({"name":"test", "model":"deterministic"})).unwrap()
    };
    let agent = state.create_agent(config()).unwrap().state.id;
    let other = state.create_agent(config()).unwrap().state.id;
    let original = anima_core::TaskResult::success(
        anima_core::Content {
            text: "<script>untrusted result</script>".repeat(1000),
            ..Default::default()
        },
        9,
    );
    let saved = state
        .tool_result_store()
        .unwrap()
        .save(&agent, temp.path(), "run", "call", "read_file", &original)
        .unwrap();
    let app = crate::routes::router(
        Arc::new(RwLock::new(state)),
        crate::app::DaemonConfig::default(),
    );
    let uri = format!("/api/agents/{agent}/tool-results/{}", saved.id);
    let unauthenticated = app
        .clone()
        .oneshot(Request::builder().uri(&uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(unauthenticated.status(), StatusCode::FORBIDDEN);
    let request = |uri: String| {
        Request::builder()
            .uri(uri)
            .header("host", "127.0.0.1:8080")
            .header("origin", "http://localhost:4200")
            .body(Body::empty())
            .unwrap()
    };
    let page = app
        .clone()
        .oneshot(request(format!("{uri}?offset=0&limit=128")))
        .await
        .unwrap();
    assert_eq!(page.status(), StatusCode::OK);
    assert_eq!(page.headers()["cache-control"], "no-store");
    let value: serde_json::Value =
        serde_json::from_slice(&to_bytes(page.into_body(), 10000).await.unwrap()).unwrap();
    assert_eq!(value["artifact"]["id"], saved.id);
    assert_eq!(value["text"].as_str().unwrap().len(), 128);
    let download = app
        .clone()
        .oneshot(request(format!("{uri}/download")))
        .await
        .unwrap();
    assert_eq!(download.status(), StatusCode::OK);
    assert_eq!(
        download.headers()["content-type"],
        "application/octet-stream"
    );
    assert_eq!(download.headers()["x-content-type-options"], "nosniff");
    assert_eq!(
        &to_bytes(download.into_body(), 100000).await.unwrap()[..],
        serde_json::to_vec(&original).unwrap()
    );
    let cross_agent = app
        .clone()
        .oneshot(request(format!(
            "/api/agents/{other}/tool-results/{}",
            saved.id
        )))
        .await
        .unwrap();
    assert_eq!(cross_agent.status(), StatusCode::NOT_FOUND);
    let oversized_page = app
        .oneshot(request(format!("{uri}?limit=8193")))
        .await
        .unwrap();
    assert_eq!(oversized_page.status(), StatusCode::BAD_REQUEST);
}
