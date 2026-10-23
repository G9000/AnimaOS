use super::*;
use crate::{
    agent_runs::AgentRunCoordinator,
    app::SharedDaemonState,
    connectors::{credentials::InMemoryCredentialStore, runtime::ConnectorManager},
    control_plane_store::ControlPlaneStoreConfig,
    history::{
        conformance::{history_message, FlakyHistoryStore},
        HistoryService, HistoryStore,
    },
    routes::http::{ApiKeyPolicy, LocalOwnerPolicy},
    sessions::{
        test_support::{agent_config, message, seed_messages, within},
        SessionKind, SessionOrigin, TitleSource,
    },
    state::DaemonState,
};
use anima_core::MessageRole;
use axum::{
    body::{to_bytes, Body},
    http::Request as HttpRequest,
};
use std::sync::Arc;
use tokio::sync::{RwLock, Semaphore};
use tower::ServiceExt;

fn router(state: SharedDaemonState, keyed: bool) -> axum::Router {
    router_with_coordinator(state, keyed).0
}

fn router_with_coordinator(
    state: SharedDaemonState,
    keyed: bool,
) -> (axum::Router, AgentRunCoordinator) {
    let limiter = Arc::new(Semaphore::new(4));
    let runs = AgentRunCoordinator::new(state.clone(), limiter.clone());
    let connectors = ConnectorManager::new(
        state.clone(),
        runs.clone(),
        Arc::new(InMemoryCredentialStore::default()),
        Arc::new(crate::connectors::telegram::TelegramClient::new().unwrap()),
    );
    let scheduler =
        crate::schedules::SchedulerService::new(state.clone(), runs.clone(), connectors.clone());
    let oauth = crate::connectors::oauth_apps::OAuthAppService::in_memory();
    let calendar = crate::connectors::gcalendar::CalendarManager::new(
        &state,
        runs.clone(),
        Arc::new(crate::connectors::gcalendar::store::InMemoryGoogleCredentialStore::default()),
        Arc::new(crate::connectors::gcalendar::client::UnconfiguredGoogleTransport),
        oauth.clone(),
    );
    let mail = crate::connectors::mail::MailManager::new(
        &state,
        runs.clone(),
        Arc::new(crate::connectors::gcalendar::store::InMemoryGoogleCredentialStore::default()),
        Arc::new(crate::app::DeterministicMailTransport),
        oauth.clone(),
    );
    let jobs = crate::jobs::JobService::new(state.clone(), runs.clone());
    let app = crate::routes::router_with_services_with_policies(
        state,
        crate::app::DaemonConfig::default(),
        limiter,
        runs.clone(),
        connectors,
        calendar,
        mail,
        oauth,
        scheduler,
        jobs,
        crate::history::HistoryWorkerOwner::new(),
        crate::logs::global(),
        LocalOwnerPolicy::for_test(true, Some("local-admin")),
        ApiKeyPolicy::for_test(keyed.then_some("owner-api-key")),
        crate::system::SupervisorRelay::unsupervised(),
    );
    (app, runs)
}

fn fixture() -> (
    tempfile::TempDir,
    SharedDaemonState,
    String,
    Arc<FlakyHistoryStore>,
) {
    let temp = tempfile::tempdir().unwrap();
    let mut state = DaemonState::new();
    state.set_control_plane_store(Some(ControlPlaneStoreConfig::Json(
        temp.path().join("control.json"),
    )));
    let store = Arc::new(FlakyHistoryStore::new());
    state.set_history(HistoryService::new(store.clone()));
    let agent = state
        .create_agent(agent_config("Companion"))
        .unwrap()
        .state
        .id;
    for room in ["chat:selected", "chat:other"] {
        state.sessions.insert(SessionRecord::new(
            &agent,
            room,
            SessionKind::Chat,
            SessionOrigin::Web,
            room.into(),
            TitleSource::Owner,
            1,
        ));
    }
    let mut revised = message(
        &agent,
        "revised-draft",
        "chat:selected",
        MessageRole::Assistant,
        "Evaluator-rejected private draft",
        4,
    );
    revised.content.metadata = Some(std::collections::BTreeMap::from([(
        anima_core::REVISED_METADATA_KEY.into(),
        anima_core::DataValue::Bool(true),
    )]));
    seed_messages(
        &mut state,
        &agent,
        vec![
            revised,
            message(
                &agent,
                "user",
                "chat:selected",
                MessageRole::User,
                "Selected question",
                2,
            ),
            message(
                &agent,
                "tool",
                "chat:selected",
                MessageRole::Tool,
                "private raw effects",
                3,
            ),
            message(
                &agent,
                "system",
                "chat:selected",
                MessageRole::System,
                "private system prompt",
                4,
            ),
            message(
                &agent,
                "answer",
                "chat:selected",
                MessageRole::Assistant,
                "Selected answer",
                5,
            ),
            message(
                &agent,
                "other",
                "chat:other",
                MessageRole::User,
                "Other conversation secret",
                6,
            ),
        ],
    );
    (temp, Arc::new(RwLock::new(state)), agent, store)
}

fn owner(method: &str, uri: &str, body: serde_json::Value) -> HttpRequest<Body> {
    HttpRequest::builder()
        .method(method)
        .uri(uri)
        .header("host", "127.0.0.1:8080")
        .header("origin", "http://localhost:4200")
        .header("x-api-key", "owner-api-key")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn viewer(uri: &str, created: &serde_json::Value) -> HttpRequest<Body> {
    HttpRequest::builder()
        .uri(uri)
        .header("host", "127.0.0.1:8080")
        .header("origin", "http://localhost:4200")
        .header(
            VIEWER_ID_HEADER,
            created["grant"]["viewerId"].as_str().unwrap(),
        )
        .header(VIEWER_SECRET_HEADER, created["secret"].as_str().unwrap())
        .body(Body::empty())
        .unwrap()
}

async fn json(response: Response) -> serde_json::Value {
    assert_eq!(response.headers()["cache-control"], "no-store");
    serde_json::from_slice(&to_bytes(response.into_body(), 1_000_000).await.unwrap()).unwrap()
}

async fn grant(app: &axum::Router, agent: &str) -> serde_json::Value {
    let response = app
        .clone()
        .oneshot(owner(
            "POST",
            &format!("/api/agents/{agent}/sessions/chat%3Aselected/shares"),
            serde_json::json!({"viewerName":"Colleague","clearGeneration":0}),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    json(response).await
}

#[tokio::test]
async fn sharing_requires_a_protected_owner_api_and_separate_viewer_authentication() {
    let (_temp, state, agent, _store) = fixture();
    let app = router(state.clone(), false);
    let denied = app
        .oneshot(owner(
            "POST",
            &format!("/api/agents/{agent}/sessions/chat%3Aselected/shares"),
            serde_json::json!({"viewerName":"Viewer","clearGeneration":0}),
        ))
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(state
        .read()
        .await
        .sessions
        .get(&agent, "chat:selected")
        .unwrap()
        .conversation_shares
        .is_empty());
    let app = router(state.clone(), true);
    let created = grant(&app, &agent).await;
    let path = format!(
        "/api/shared/conversations/{}",
        created["grant"]["id"].as_str().unwrap()
    );
    let missing = app
        .clone()
        .oneshot(owner("GET", &path, serde_json::Value::Null))
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::UNAUTHORIZED);
    let valid = app.clone().oneshot(viewer(&path, &created)).await.unwrap();
    assert_eq!(valid.status(), StatusCode::OK);
    assert_eq!(json(valid).await["readOnly"], true);
    let mut link_as_secret = viewer(&path, &created);
    link_as_secret.headers_mut().insert(
        VIEWER_SECRET_HEADER,
        created["grant"]["id"].as_str().unwrap().parse().unwrap(),
    );
    assert_eq!(
        app.clone().oneshot(link_as_secret).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    // An installation that disables owner authentication also disables old grants.
    let unprotected = router(state, false)
        .oneshot(viewer(&path, &created))
        .await
        .unwrap();
    assert_eq!(unprotected.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn transcript_is_exactly_selected_text_and_persisted_grants_never_contain_the_secret() {
    let (temp, state, agent, _store) = fixture();
    let app = router(state.clone(), true);
    let created = grant(&app, &agent).await;
    let id = created["grant"]["id"].as_str().unwrap();
    let path = format!("/api/shared/conversations/{id}/messages");
    let response = app.clone().oneshot(viewer(&path, &created)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let page = json(response).await;
    let texts = page["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["text"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(texts, ["Selected question", "Selected answer"]);
    assert_eq!(page["messages"][0].as_object().unwrap().len(), 4);
    let encoded = page.to_string();
    for private in [
        "Evaluator-rejected private draft",
        "private raw effects",
        "private system prompt",
        "Other conversation secret",
        "attachments\":",
        "metadata\":",
        "toolCalls",
        "agentId",
        "roomId",
    ] {
        assert!(!encoded.contains(private), "{private}");
    }
    let persisted = std::fs::read_to_string(temp.path().join("control.json")).unwrap();
    assert!(persisted.contains(id));
    assert!(!persisted.contains(created["secret"].as_str().unwrap()));
    let list = app
        .clone()
        .oneshot(owner(
            "GET",
            &format!("/api/agents/{agent}/sessions/chat%3Aselected/shares"),
            serde_json::Value::Null,
        ))
        .await
        .unwrap();
    let listed = json(list).await.to_string();
    assert!(!listed.contains("credentialSha256"));
    assert!(!listed.contains(created["secret"].as_str().unwrap()));
    let other = format!(
        "/api/shared/conversations/share-{}/messages",
        uuid::Uuid::new_v4()
    );
    assert_eq!(
        app.oneshot(viewer(&other, &created))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn viewer_credentials_cannot_reach_owner_reads_or_writes_even_with_approved_origin() {
    let (_temp, state, agent, _store) = fixture();
    let app = router(state, true);
    let created = grant(&app, &agent).await;
    for (method, path) in [
        ("GET", "/api/agents".to_string()),
        ("GET", format!("/api/agents/{agent}")),
        ("GET", format!("/api/agents/{agent}/sessions")),
        ("GET", format!("/api/agents/{agent}/events")),
        ("GET", "/api/workspace/files".to_string()),
        ("GET", format!("/api/agents/{agent}/attachments/att-one")),
        ("GET", format!("/api/agents/{agent}/tool-results/tr_one")),
        (
            "POST",
            format!("/api/agents/{agent}/sessions/chat%3Aselected/runs"),
        ),
        ("POST", "/api/system/session".to_string()),
        (
            "DELETE",
            format!("/api/agents/{agent}/sessions/chat%3Aselected"),
        ),
    ] {
        let mut request = viewer(&path, &created);
        *request.method_mut() = method.parse().unwrap();
        // Even a request that also presents the owner API key must not promote
        // an explicitly viewer-authenticated call into an owner operation.
        request
            .headers_mut()
            .insert("x-api-key", "owner-api-key".parse().unwrap());
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{method} {path}");
        assert_eq!(response.headers()["cache-control"], "no-store");
        // Removing the viewer headers still requires the unavailable owner key.
        let no_credential = HttpRequest::builder()
            .method(method)
            .uri(&path)
            .header("host", "127.0.0.1:8080")
            .header("origin", "http://localhost:4200")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            app.clone().oneshot(no_credential).await.unwrap().status(),
            StatusCode::UNAUTHORIZED,
            "{method} {path}"
        );
    }
}

#[tokio::test]
async fn revoke_clear_and_delete_end_access_without_affecting_other_conversations() {
    for change in ["revoke", "clear", "delete"] {
        let (_temp, state, agent, _store) = fixture();
        let app = router(state.clone(), true);
        let created = grant(&app, &agent).await;
        let id = created["grant"]["id"].as_str().unwrap();
        if change == "revoke" {
            let response = app
                .clone()
                .oneshot(owner(
                    "DELETE",
                    &format!("/api/agents/{agent}/sessions/chat%3Aselected/shares/{id}"),
                    serde_json::Value::Null,
                ))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        } else {
            let mut guard = state.write().await;
            if change == "clear" {
                guard
                    .sessions
                    .get_mut(&agent, "chat:selected")
                    .unwrap()
                    .clear_generation += 1;
            } else {
                guard.sessions.remove(&agent, "chat:selected");
            }
        }
        let response = app
            .clone()
            .oneshot(viewer(
                &format!("/api/shared/conversations/{id}/messages"),
                &created,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{change}");
        assert!(state
            .read()
            .await
            .sessions
            .get(&agent, "chat:other")
            .is_some());
    }
}

#[tokio::test]
async fn a_revoke_during_a_history_read_discards_the_loaded_transcript() {
    let (_temp, state, agent, store) = fixture();
    store
        .upsert_messages(&[history_message(
            "stored-cursor",
            &agent,
            "chat:selected",
            MessageRole::User,
            "stored private after revocation",
            1,
        )])
        .await
        .unwrap();
    let app = router(state, true);
    let created = grant(&app, &agent).await;
    let id = created["grant"]["id"].as_str().unwrap();
    let gate = store.hold_message_read_after(0);
    let read = app.clone().oneshot(viewer(
        &format!("/api/shared/conversations/{id}/messages?before=stored-cursor"),
        &created,
    ));
    let read = tokio::spawn(read);
    within("shared history read", gate.entered.acquire())
        .await
        .unwrap()
        .forget();
    let response = app
        .clone()
        .oneshot(owner(
            "DELETE",
            &format!("/api/agents/{agent}/sessions/chat%3Aselected/shares/{id}"),
            serde_json::Value::Null,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    gate.release.add_permits(1);
    let response = within("revoked shared response", read)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(!json(response).await.to_string().contains("stored private"));
}

#[tokio::test]
async fn grant_auth_precedes_body_and_a_failed_save_rolls_back_the_grant() {
    let (temp, state, agent, _store) = fixture();
    let app = router(state.clone(), true);
    let path = format!("/api/agents/{agent}/sessions/chat%3Aselected/shares");
    let unauthorized = HttpRequest::builder()
        .method("POST")
        .uri(&path)
        .header("host", "127.0.0.1:8080")
        .header("origin", "https://untrusted.example")
        .header("x-api-key", "owner-api-key")
        .body(Body::from("malformed JSON"))
        .unwrap();
    let response = app.clone().oneshot(unauthorized).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    // A directory cannot be replaced by the atomic snapshot file writer.
    state
        .write()
        .await
        .set_control_plane_store(Some(ControlPlaneStoreConfig::Json(
            temp.path().to_path_buf(),
        )));
    let response = app
        .oneshot(owner(
            "POST",
            &path,
            serde_json::json!({"viewerName":"Viewer","clearGeneration":0}),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(state
        .read()
        .await
        .sessions
        .get(&agent, "chat:selected")
        .unwrap()
        .conversation_shares
        .is_empty());
}

#[tokio::test]
async fn failed_revocation_persistence_keeps_access_stopped_and_can_be_retried() {
    let (temp, state, agent, _store) = fixture();
    let app = router(state.clone(), true);
    let created = grant(&app, &agent).await;
    let id = created["grant"]["id"].as_str().unwrap();
    let path = format!("/api/agents/{agent}/sessions/chat%3Aselected/shares/{id}");
    state
        .write()
        .await
        .set_control_plane_store(Some(ControlPlaneStoreConfig::Json(
            temp.path().to_path_buf(),
        )));
    let failed = app
        .clone()
        .oneshot(owner("DELETE", &path, serde_json::Value::Null))
        .await
        .unwrap();
    assert_eq!(failed.status(), StatusCode::SERVICE_UNAVAILABLE);
    let denied = app
        .clone()
        .oneshot(viewer(&format!("/api/shared/conversations/{id}"), &created))
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
    state
        .write()
        .await
        .set_control_plane_store(Some(ControlPlaneStoreConfig::Json(
            temp.path().join("control.json"),
        )));
    let retried = app
        .oneshot(owner("DELETE", &path, serde_json::Value::Null))
        .await
        .unwrap();
    assert_eq!(retried.status(), StatusCode::OK);
}

#[tokio::test]
async fn transcript_cursors_cannot_expand_scope_or_enable_hidden_history() {
    let (_temp, state, agent, store) = fixture();
    store
        .upsert_messages(&[history_message(
            "other-stored",
            &agent,
            "chat:other",
            MessageRole::User,
            "Other stored secret",
            1,
        )])
        .await
        .unwrap();
    let app = router(state, true);
    let created = grant(&app, &agent).await;
    let id = created["grant"]["id"].as_str().unwrap();
    for suffix in [
        "before=other",
        "before=other-stored",
        "includeHidden=true",
        "anchor=other",
        "limit=200",
        "sessionId=chat%3Aother",
    ] {
        let response = app
            .clone()
            .oneshot(viewer(
                &format!("/api/shared/conversations/{id}/messages?{suffix}"),
                &created,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{suffix}");
        assert!(!json(response)
            .await
            .to_string()
            .contains("Other stored secret"));
    }
}

#[tokio::test]
async fn missing_expected_generation_never_creates_a_viewer_grant() {
    let (_temp, state, agent, _store) = fixture();
    let app = router(state.clone(), true);
    let response = app
        .oneshot(owner(
            "POST",
            &format!("/api/agents/{agent}/sessions/chat%3Aselected/shares"),
            serde_json::json!({"viewerName":"Legacy request"}),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(state
        .read()
        .await
        .sessions
        .get(&agent, "chat:selected")
        .unwrap()
        .conversation_shares
        .is_empty());
}

#[tokio::test]
async fn delayed_share_creation_cannot_grant_a_conversation_cleared_while_waiting() {
    let (_temp, state, agent, _store) = fixture();
    let (app, coordinator) = router_with_coordinator(state.clone(), true);
    let transaction = coordinator.control_plane_transaction().await;
    let parsed = Arc::new(Semaphore::new(0));
    let body_entered = parsed.clone();
    let mut request = owner(
        "POST",
        &format!("/api/agents/{agent}/sessions/chat%3Aselected/shares"),
        serde_json::Value::Null,
    );
    *request.body_mut() = Body::from_stream(futures::stream::once(async move {
        body_entered.add_permits(1);
        Ok::<_, std::convert::Infallible>(axum::body::Bytes::from_static(
            br#"{"viewerName":"Stale dialog","clearGeneration":0}"#,
        ))
    }));
    let delayed = tokio::spawn(app.clone().oneshot(request));
    within("delayed share body", parsed.acquire())
        .await
        .unwrap()
        .forget();
    assert!(!delayed.is_finished());
    state
        .write()
        .await
        .sessions
        .get_mut(&agent, "chat:selected")
        .unwrap()
        .clear_generation = 1;
    drop(transaction);
    let response = within("stale share refusal", delayed)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert!(state
        .read()
        .await
        .sessions
        .get(&agent, "chat:selected")
        .unwrap()
        .conversation_shares
        .is_empty());
    let response = app
        .oneshot(owner(
            "POST",
            &format!("/api/agents/{agent}/sessions/chat%3Aselected/shares"),
            serde_json::json!({"viewerName":"New explicit grant","clearGeneration":1}),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
}
