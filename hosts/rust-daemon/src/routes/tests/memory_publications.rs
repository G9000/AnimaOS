use super::*;
use serde_json::{json, Value};

fn request(agent: &str, method: &str, suffix: &str, input: Value) -> Request<Body> {
    crate::routes::owner_test_support::authenticate(
        Request::builder()
            .method(method)
            .uri(format!("/api/agents/{agent}/memory-publications{suffix}"))
            .header("host", "127.0.0.1:8080")
            .header("origin", "http://localhost:4200"),
    )
    .header("content-type", "application/json")
    .body(if method == "GET" {
        Body::empty()
    } else {
        Body::from(input.to_string())
    })
    .unwrap()
}

#[tokio::test]
async fn memory_publications_owner_read_distinguishes_unconfigured_storage() {
    let state = Arc::new(RwLock::new(DaemonState::new()));
    let id = state
        .write()
        .await
        .create_agent(test_config("share"))
        .unwrap()
        .state
        .id;
    let app = router(state, DaemonConfig::default());
    let response = app
        .oneshot(request(&id, "GET", "", Value::Null))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let value: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(value["available"], false);
    assert_eq!(value["reason"], "persistent_storage_required");
    assert_eq!(value["items"], json!([]));
}

#[tokio::test]
async fn memory_publications_require_owner_and_never_acknowledge_volatile_admission() {
    let state = Arc::new(RwLock::new(DaemonState::new()));
    let id = state
        .write()
        .await
        .create_agent(test_config("share"))
        .unwrap()
        .state
        .id;
    let app = router(state, DaemonConfig::default());
    let unauthenticated = Request::builder()
        .uri(format!("/api/agents/{id}/memory-publications"))
        .header("host", "127.0.0.1:8080")
        .header("origin", "http://localhost:4200")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.clone().oneshot(unauthenticated).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    let response=app.clone().oneshot(request(&id,"POST","",json!({"publicationId":uuid::Uuid::new_v4().to_string(),"localId":"local","expectedContent":"synthetic","title":"Synthetic","source":"Explicit selection","basis":"owner_statement"}))).await.unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(
        app.oneshot(request("missing", "GET", "", Value::Null))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn memory_publications_report_unreadable_storage_without_claiming_empty_queue() {
    let mut daemon = DaemonState::new();
    let id = daemon.create_agent(test_config("share")).unwrap().state.id;
    daemon.memory_publications_unavailable = Some("publication_storage_unavailable".into());
    let app = router(Arc::new(RwLock::new(daemon)), DaemonConfig::default());
    let response = app
        .oneshot(request(&id, "GET", "", Value::Null))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let value: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(value["available"], false);
    assert_eq!(value["reason"], "publication_storage_unavailable");
}

#[tokio::test]
async fn memory_publications_durable_ack_reread_and_revision_checked_cancellation() {
    use crate::mcp::{McpConnectionRecord, ReviewedMcpTool};
    use crate::memory_provider::{binding, publication::PublicationService};
    use crate::profile_owner::{ProfileOwner, ProfileResource};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("selected.sqlite");
    let owner = ProfileOwner::acquire_resources([ProfileResource::Sqlite(path.clone())]).unwrap();
    let mut daemon = DaemonState::new();
    daemon.profile_owner = Some(owner.clone());
    let id = daemon.create_agent(test_config("share")).unwrap().state.id;
    let other = daemon.create_agent(test_config("other")).unwrap().state.id;
    let connection = uuid::Uuid::new_v4().to_string();
    let binding: binding::MemoryProviderBinding = serde_json::from_value(json!({"version":1,"provider":"mother","connectionId":connection,"workspaceId":"00000000-0000-4000-8000-000000000001","generation":"00000000-0000-4000-8000-000000000002","subject":id,"publication":"selected","personaSource":"local","accessMode":"companion"})).unwrap();
    let mut config = daemon.agents[&id].config().clone();
    config
        .settings
        .get_or_insert_with(Default::default)
        .additional
        .insert(binding::SETTING.into(), binding.portable());
    daemon.restore_agent_config(&id, config);
    let tool = anima_core::prepare_external_tool(
        "mother_propose_memory",
        "Synthetic",
        json!({"type":"object"}),
        None,
    )
    .unwrap();
    daemon.mcp_registry.connections.insert(
        connection.clone(),
        McpConnectionRecord {
            id: connection,
            agent_id: id.clone(),
            label: "Synthetic".into(),
            endpoint: "http://127.0.0.1:9999/mcp".into(),
            stdio: None,
            bearer_credential_ref: None,
            oauth_credential_ref: None,
            enabled: true,
            version: 1,
            reviewed_tools: vec![ReviewedMcpTool {
                alias: "mcp_synthetic_propose".into(),
                descriptor: tool,
            }],
        },
    );
    let source = daemon
        .memory_handle()
        .write()
        .await
        .add(anima_memory::NewMemory {
            agent_id: id.clone(),
            agent_name: "Synthetic".into(),
            memory_type: anima_memory::MemoryType::Fact,
            content: "Synthetic selected fact".into(),
            importance: 0.5,
            tags: None,
            scope: Some(anima_memory::MemoryScope::Shared),
            room_id: None,
            world_id: None,
            session_id: None,
        })
        .unwrap();
    let state = Arc::new(RwLock::new(daemon));
    let service = PublicationService::open(&state, owner, &path)
        .await
        .unwrap();
    state.write().await.memory_publications = Some(service);
    let app = router(state, DaemonConfig::default());
    let publication = uuid::Uuid::new_v4().to_string();
    let selection = json!({"publicationId":publication,"localId":source.id,"expectedContent":source.content,"title":"Selected synthetic fact","source":"Explicit test selection","basis":"owner_statement"});
    // Ignore the first response body, then repeat the identical admission as a lost-ack retry.
    assert_eq!(
        app.clone()
            .oneshot(request(&id, "POST", "", selection.clone()))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let response = app
        .clone()
        .oneshot(request(&id, "POST", "", selection.clone()))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let queued: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(queued["state"], "queued");
    assert_eq!(queued["canonical"], Value::Null);
    assert_eq!(queued["attempts"], 0);
    let suffix = format!("/{publication}");
    assert_eq!(
        app.clone()
            .oneshot(request(&other, "GET", &suffix, Value::Null))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    let mut changed = selection;
    changed["title"] = json!("Changed selection");
    assert_eq!(
        app.clone()
            .oneshot(request(&id, "POST", "", changed))
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    let control = format!("/{publication}/control");
    assert_eq!(
        app.clone()
            .oneshot(request(
                &id,
                "POST",
                &control,
                json!({"expectedRevision":0,"action":"cancel"})
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        app.clone()
            .oneshot(request(
                &id,
                "POST",
                &control,
                json!({"expectedRevision":queued["revision"],"action":"cancel"})
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let response = app
        .oneshot(request(&id, "GET", &suffix, Value::Null))
        .await
        .unwrap();
    let final_state: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(final_state["state"], "cancelled");
    assert_eq!(final_state["attempts"], 0);
}
