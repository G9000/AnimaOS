use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use tower::ServiceExt;

#[tokio::test]
async fn document_edits_owner_routes_require_authentication_and_are_not_cacheable() {
    let state = std::sync::Arc::new(tokio::sync::RwLock::new(crate::state::DaemonState::new()));
    let app = crate::routes::router(state, crate::app::DaemonConfig::default());
    let path = "/api/document-edits/proposal?agent_id=agent&session_id=chat%3Aowner";
    let unauthenticated = Request::builder()
        .uri(path)
        .header("host", "127.0.0.1:8080")
        .header("origin", "http://localhost:4200")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(unauthenticated).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    for method in ["GET", "DELETE"] {
        let request = crate::routes::owner_test_support::authenticate(
            Request::builder()
                .method(method)
                .uri(path)
                .header("host", "127.0.0.1:8080")
                .header("origin", "http://localhost:4200"),
        )
        .body(Body::empty())
        .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(response.headers()["cache-control"], "no-store");
        assert!(!response.headers().contains_key("content-disposition"));
    }
}
