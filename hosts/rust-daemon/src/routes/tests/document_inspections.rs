use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use tower::ServiceExt;

#[tokio::test]
async fn inspection_pages_and_reports_require_current_owner_authentication() {
    let state = std::sync::Arc::new(tokio::sync::RwLock::new(crate::state::DaemonState::new()));
    let app = crate::routes::router(state, crate::app::DaemonConfig::default());
    for suffix in ["", "/pages/1"] {
        let path = format!(
            "/api/document-inspections/{}{}?agent_id=agent&session_id=chat%3Aowner",
            "a".repeat(64),
            suffix
        );
        let request = Request::builder()
            .uri(&path)
            .header("host", "127.0.0.1:8080")
            .header("origin", "http://localhost:4200")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            app.clone().oneshot(request).await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
        let request = crate::routes::owner_test_support::authenticate(
            Request::builder()
                .uri(&path)
                .header("host", "127.0.0.1:8080")
                .header("origin", "http://localhost:4200"),
        )
        .body(Body::empty())
        .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(response.headers()["cache-control"], "no-store");
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["kind"], "inconclusive");
        assert_eq!(body["error"], body["reason"]);
        assert!(body["reason"]
            .as_str()
            .is_some_and(|reason| !reason.is_empty()));
    }
}

#[tokio::test]
async fn inspection_creation_rejects_executable_settings_instead_of_forwarding_them() {
    let state = std::sync::Arc::new(tokio::sync::RwLock::new(crate::state::DaemonState::new()));
    let app = crate::routes::router(state, crate::app::DaemonConfig::default());
    let request = crate::routes::owner_test_support::authenticate(Request::builder().method("POST").uri("/api/document-inspections?agent_id=agent&session_id=chat%3Aowner").header("host", "127.0.0.1:8080").header("origin", "http://localhost:4200").header("content-type", "application/json"))
        .body(Body::from(serde_json::json!({"subject":{"kind":"workspace","file_path":"report.docx","expected_sha256":"a".repeat(64)},"executable":"arbitrary.exe"}).to_string())).unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
