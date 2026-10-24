use super::*;
use axum::{
    body::{to_bytes, Body},
    http::Request as HttpRequest,
    Router,
};
use serde_json::{json, Value};
use tower::ServiceExt;

async fn call(app: Router, method: &str, path: &str, body: Value) -> Response {
    app.oneshot(
        crate::routes::owner_test_support::authenticate(
            HttpRequest::builder()
                .method(method)
                .uri(path)
                .header("host", "127.0.0.1:8080")
                .header("origin", "http://localhost:4200"),
        )
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap(),
    )
    .await
    .unwrap()
}
async fn json_body(response: Response) -> Value {
    serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap()).unwrap()
}

#[tokio::test]
async fn result_routes_scope_pins_and_reject_untrusted_origins_before_parsing() {
    let path =
        std::env::temp_dir().join(format!("anima-result-http-{}.json", uuid::Uuid::new_v4()));
    let mut state = crate::state::DaemonState::new();
    state.set_control_plane_store(Some(
        crate::control_plane_store::ControlPlaneStoreConfig::Json(path.clone()),
    ));
    let agent = state
        .create_agent(
            serde_json::from_value(json!({"name":"Owner","model":"deterministic"})).unwrap(),
        )
        .unwrap()
        .state
        .id;
    for id in ["chat:results", "chat:other"] {
        state.sessions.insert(crate::sessions::SessionRecord::new(
            &agent,
            id,
            crate::sessions::SessionKind::Chat,
            crate::sessions::SessionOrigin::Web,
            "Chat".into(),
            crate::sessions::TitleSource::Owner,
            1,
        ));
    }
    let app = crate::routes::router(
        std::sync::Arc::new(tokio::sync::RwLock::new(state)),
        crate::app::DaemonConfig::default(),
    );
    let uri = format!("/api/agents/{agent}/sessions/chat:results/results");
    let request = json!({"clearGeneration":0,"result":{"expectedRevision":0,"requestKey":"create","title":"Inline","payload":{"kind":"document","format":"markdown","text":"# Report"}}});
    let created = call(app.clone(), "POST", &uri, request).await;
    assert_eq!(created.status(), StatusCode::OK);
    assert_eq!(created.headers()["cache-control"], "no-store");
    let saved = json_body(created).await;
    let id = saved["result"]["id"].as_str().unwrap();
    let result_uri = format!("{uri}/{id}");
    assert_eq!(
        call(
            app.clone(),
            "PATCH",
            &result_uri,
            json!({"clearGeneration":1,"expectedRevision":1,"pinned":true})
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(
            app.clone(),
            "PATCH",
            &format!("/api/agents/{agent}/sessions/chat:other/results/{id}"),
            json!({"clearGeneration":0,"expectedRevision":1,"pinned":true})
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    let pinned = call(
        app.clone(),
        "PATCH",
        &result_uri,
        json!({"clearGeneration":0,"expectedRevision":1,"pinned":true}),
    )
    .await;
    assert_eq!(pinned.status(), StatusCode::OK);
    assert_eq!(json_body(pinned).await["result"]["revision"], 2);
    let listed = json_body(call(app.clone(), "GET", &uri, Value::Null).await).await;
    assert_eq!(listed["results"][0]["pinned"], true);
    let other = json_body(
        call(
            app.clone(),
            "GET",
            &format!("/api/agents/{agent}/sessions/chat:other/results"),
            Value::Null,
        )
        .await,
    )
    .await;
    assert_eq!(other["results"], json!([]));
    for (method, target) in [
        ("GET", uri.as_str()),
        ("POST", uri.as_str()),
        ("PATCH", result_uri.as_str()),
    ] {
        let denied = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(method)
                    .uri(target)
                    .header("host", "127.0.0.1:8080")
                    .header("origin", "https://untrusted.example")
                    .body(Body::from("not json"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(denied.status(), StatusCode::FORBIDDEN);
        assert_eq!(denied.headers()["cache-control"], "no-store");
    }
    let _ = std::fs::remove_file(path);
}
