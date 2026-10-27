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

#[tokio::test]
async fn tracker_route_edits_are_durable_and_require_current_revision_and_generation() {
    let path =
        std::env::temp_dir().join(format!("anima-tracker-http-{}.json", uuid::Uuid::new_v4()));
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
    state.sessions.insert(crate::sessions::SessionRecord::new(
        &agent,
        "chat:tracker",
        crate::sessions::SessionKind::Chat,
        crate::sessions::SessionOrigin::Web,
        "Tracker".into(),
        crate::sessions::TitleSource::Owner,
        1,
    ));
    let app = crate::routes::router(
        std::sync::Arc::new(tokio::sync::RwLock::new(state)),
        crate::app::DaemonConfig::default(),
    );
    let uri = format!("/api/agents/{agent}/sessions/chat:tracker/results");
    let mut create = json!({"clearGeneration":0,"result":{"expectedRevision":0,"requestKey":"tracker_create","title":"Tasks","payload":{"kind":"tracker","columns":[{"key":"task","label":"Task","kind":"text"},{"key":"done","label":"Done","kind":"checkbox"}],"rows":[{"id":"first","cells":{"task":"Draft","done":false}}]}}});
    let response = call(app.clone(), "POST", &uri, create.clone()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let saved = json_body(response).await;
    create["result"]["id"] = saved["result"]["id"].clone();
    create["result"]["expectedRevision"] = json!(1);
    create["result"]["requestKey"] = json!("tracker_edit");
    create["result"]["payload"]["rows"][0]["cells"]["done"] = json!(true);
    let edited = call(app.clone(), "POST", &uri, create.clone()).await;
    assert_eq!(edited.status(), StatusCode::OK);
    assert_eq!(json_body(edited).await["result"]["revision"], 2);
    create["result"]["requestKey"] = json!("stale_edit");
    create["result"]["payload"]["rows"][0]["cells"]["done"] = json!(false);
    assert_eq!(
        call(app.clone(), "POST", &uri, create.clone())
            .await
            .status(),
        StatusCode::CONFLICT
    );
    create["clearGeneration"] = json!(1);
    create["result"]["expectedRevision"] = json!(2);
    assert_eq!(
        call(app.clone(), "POST", &uri, create).await.status(),
        StatusCode::CONFLICT
    );
    let listed = json_body(call(app, "GET", &uri, Value::Null).await).await;
    assert_eq!(
        listed["results"][0]["payload"]["rows"][0]["cells"]["done"],
        true
    );
    let restored = crate::control_plane_store::load_control_plane_snapshot(
        &crate::control_plane_store::ControlPlaneStoreConfig::Json(path.clone()),
    )
    .await
    .unwrap()
    .unwrap();
    let saved = restored
        .sessions
        .iter()
        .find(|session| session.agent_id == agent && session.id == "chat:tracker")
        .unwrap();
    assert_eq!(
        serde_json::to_value(&saved.chat_results[0].payload).unwrap()["rows"][0]["cells"]["done"],
        true
    );
    let _ = std::fs::remove_file(path);
}
