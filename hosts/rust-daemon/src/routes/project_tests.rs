//! Public owner/member boundary contracts; registered from the project routes.
use crate::sessions::{
    test_support::{agent_config, message, seed_messages},
    SessionKind, SessionOrigin, SessionRecord, TitleSource,
};
use crate::{
    app::{DaemonConfig, SharedDaemonState},
    state::DaemonState,
};
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::RwLock;
use tower::ServiceExt;

async fn fixture() -> (Router, SharedDaemonState, String, String, String) {
    let mut state = DaemonState::new();
    let a = state.create_agent(agent_config("Alice")).unwrap().state.id;
    let b = state.create_agent(agent_config("Bob")).unwrap().state.id;
    let c = state
        .create_agent(agent_config("Outside"))
        .unwrap()
        .state
        .id;
    for (agent, room) in [(&a, "chat:a"), (&b, "chat:b")] {
        state.sessions.insert(SessionRecord::new(
            agent,
            room,
            SessionKind::Chat,
            SessionOrigin::Web,
            "Native chat".into(),
            TitleSource::Owner,
            1,
        ));
    }
    seed_messages(
        &mut state,
        &a,
        vec![
            message(
                &a,
                "selected",
                "chat:a",
                anima_core::MessageRole::Assistant,
                "Exact pinned evidence",
                1,
            ),
            message(
                &a,
                "private",
                "chat:a",
                anima_core::MessageRole::User,
                "Unselected private transcript",
                2,
            ),
        ],
    );
    let state = Arc::new(RwLock::new(state));
    (
        crate::routes::router(state.clone(), DaemonConfig::default()),
        state,
        a,
        b,
        c,
    )
}
async fn call(
    app: &Router,
    method: &str,
    uri: &str,
    origin: &str,
    body: Value,
) -> (StatusCode, Value) {
    let request = crate::routes::owner_test_support::authenticate(
        Request::builder()
            .method(method)
            .uri(uri)
            .header("host", "127.0.0.1:8080")
            .header("origin", origin),
    )
    .header("content-type", "application/json")
    .body(Body::from(if method == "GET" {
        String::new()
    } else {
        body.to_string()
    }))
    .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    assert_eq!(response.headers()["cache-control"], "no-store");
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}
const OWNER: &str = "http://localhost:4200";

#[tokio::test]
async fn shared_project_routes_enforce_owner_and_members_and_keep_deleted_creation_receipts() {
    let (app, state, a, b, c) = fixture().await;
    let input = json!({"requestKey":"create-once","title":"Shared research","memberAgentIds":[a,b],"instructions":"Owner-only selected brief"});
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/projects",
            "https://untrusted.example",
            input.clone()
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let (status, body) = call(&app, "POST", "/api/projects", OWNER, input.clone()).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let id = body["project"]["id"].as_str().unwrap();
    let (status, replay) = call(&app, "POST", "/api/projects", OWNER, input.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(replay["project"]["id"], id);
    let (status, metadata) = call(
        &app,
        "GET",
        &format!("/api/projects?memberAgentId={a}"),
        OWNER,
        json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!metadata.to_string().contains("Owner-only selected brief"));
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/projects/{id}?asAgentId={c}"),
            OWNER,
            json!(null)
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/projects/{id}"),
            OWNER,
            json!({"expectedRevision":1})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, "POST", "/api/projects", OWNER, input).await.0,
        StatusCode::CONFLICT
    );
    let state = state.read().await;
    assert!(state.projects.snapshot().is_empty());
    assert_eq!(state.projects.creation_receipts_snapshot().len(), 1);
}

#[tokio::test]
async fn donor_pin_is_exact_and_member_removal_detaches_without_deleting_native_data() {
    let (app, state, a, b, _) = fixture().await;
    let (_, created) = call(
        &app,
        "POST",
        "/api/projects",
        OWNER,
        json!({"requestKey":"donor-pin","title":"Shared","memberAgentIds":[a,b]}),
    )
    .await;
    let id = created["project"]["id"].as_str().unwrap();
    let (status, moved) = call(
        &app,
        "PUT",
        &format!("/api/agents/{a}/sessions/chat%3Aa/project"),
        OWNER,
        json!({"projectId":id,"expectedBindingRevision":0,"expectedProjectRevision":1}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{moved}");
    let (status,pinned) = call(&app,"POST",&format!("/api/projects/{id}/resources"),OWNER,json!({"expectedRevision":1,
        "requestKey":"pin-exact","asAgentId":b,"target":{"kind":"chat_excerpt","ownerAgentId":a,"sessionId":"chat:a","clearGeneration":0,"messageIds":["selected"]},
        "role":"reference","useInContext":true})).await;
    assert_eq!(status, StatusCode::OK, "{pinned}");
    let resource = pinned["project"]["resources"][0]["id"].as_str().unwrap();
    let (status, preview) = call(
        &app,
        "GET",
        &format!("/api/projects/{id}/resources/{resource}/preview?asAgentId={b}"),
        OWNER,
        json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert!(preview["text"]
        .as_str()
        .unwrap()
        .contains("Exact pinned evidence"));
    assert!(!preview
        .to_string()
        .contains("Unselected private transcript"));
    let (status, changed) = call(
        &app,
        "PUT",
        &format!("/api/projects/{id}/members"),
        OWNER,
        json!({"expectedRevision":2,"agentIds":[b]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{changed}");
    assert_eq!(changed["detachedChats"], 1);
    let (_, view) = call(
        &app,
        "GET",
        &format!("/api/projects/{id}?asAgentId={b}"),
        OWNER,
        json!(null),
    )
    .await;
    assert_eq!(view["project"]["resources"][0]["target"], Value::Null);
    assert_eq!(view["project"]["resources"][0]["label"], Value::Null);
    let state = state.read().await;
    assert_eq!(state.sessions.get(&a, "chat:a").unwrap().project_id, None);
    assert_eq!(
        state.model_visible_history(&a, "chat:a").len(),
        2,
        "native transcript survives unsharing"
    );
}
