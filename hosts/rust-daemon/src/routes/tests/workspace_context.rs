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
    assert_eq!(value.get("providerBinding"), Some(&Value::Null));
    assert!(value["revision"].as_str().is_some());
    assert_eq!(value["connections"], json!([]));
}

const GARDEN_CONNECTION: &str = "76d89e5c-7b95-48e3-87d7-bbddb1c10b62";

fn additive_binding(id: &str) -> Value {
    json!({"version":1,"provider":"mother","connectionId":GARDEN_CONNECTION,"workspaceId":"workspace",
        "generation":"generation","subject":id,"publication":"selected","personaSource":"local","accessMode":"companion"})
}

fn context_value(value: Value) -> anima_core::DataValue {
    match value {
        Value::Null => anima_core::DataValue::Null,
        Value::Bool(value) => anima_core::DataValue::Bool(value),
        Value::Number(value) => anima_core::DataValue::Number(value.as_f64().unwrap()),
        Value::String(value) => anima_core::DataValue::String(value),
        Value::Array(values) => {
            anima_core::DataValue::Array(values.into_iter().map(context_value).collect())
        }
        Value::Object(values) => anima_core::DataValue::Object(
            values
                .into_iter()
                .map(|(key, value)| (key, context_value(value)))
                .collect(),
        ),
    }
}

async fn provider_fixture(label: &str) -> (WorkspaceAvatarTemp, Arc<RwLock<DaemonState>>, String) {
    let fixture = WorkspaceAvatarTemp::new(label);
    let state = fixture.state();
    let mut guard = state.write().await;
    guard.set_control_plane_store(Some(
        crate::control_plane_store::ControlPlaneStoreConfig::Json(
            fixture.root.join("control.json"),
        ),
    ));
    let id = guard
        .create_agent(test_config("provider"))
        .unwrap()
        .state
        .id;
    let descriptor = anima_core::prepare_external_tool(
        "mother_get_context",
        "Synthetic context reader",
        json!({"type":"object","properties":{},"additionalProperties":false}),
        None,
    )
    .unwrap();
    let tool = crate::mcp::ReviewedMcpTool {
        alias: crate::mcp::alias_for(GARDEN_CONNECTION, &descriptor.descriptor_digest).unwrap(),
        descriptor,
    };
    guard.mcp_registry.connections.insert(
        GARDEN_CONNECTION.into(),
        crate::mcp::McpConnectionRecord {
            id: GARDEN_CONNECTION.into(),
            agent_id: id.clone(),
            label: "Synthetic garden".into(),
            endpoint: "https://example.com/mcp".into(),
            stdio: None,
            bearer_credential_ref: None,
            oauth_credential_ref: None,
            enabled: true,
            version: 1,
            reviewed_tools: vec![tool],
        },
    );
    drop(guard);
    (fixture, state, id)
}

async fn context_view(app: &axum::Router, id: &str) -> Value {
    let response = app
        .clone()
        .oneshot(request(id, "GET", Value::Null))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap()
}

#[tokio::test]
async fn workspace_context_additive_conversion_is_atomic_and_old_callers_preserve_it() {
    let (fixture, state, id) = provider_fixture("additive-conversion").await;
    let legacy = json!({"connectionId":GARDEN_CONNECTION,"tool":"mother_get_context","mode":"companion",
        "workspaceId":"workspace","generation":"generation","subject":id});
    {
        let mut guard = state.write().await;
        let mut config = guard.get_agent(&id).unwrap().state.config;
        config
            .settings
            .get_or_insert_with(Default::default)
            .additional
            .insert("workspaceContext".into(), context_value(legacy.clone()));
        guard.restore_agent_config(&id, config);
    }
    let app = router(state.clone(), DaemonConfig::default());
    let before = context_view(&app, &id).await;
    assert_eq!(before["binding"], legacy);
    let ambiguous = app.clone().oneshot(request(&id,"PUT",json!({"expectedRevision":before["revision"],"binding":legacy,"providerBinding":additive_binding(&id)}))).await.unwrap();
    assert_eq!(ambiguous.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        context_view(&app, &id).await["revision"],
        before["revision"]
    );
    let response = app.clone().oneshot(request(&id,"PUT",json!({"expectedRevision":before["revision"],"binding":null,"providerBinding":additive_binding(&id)}))).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(
        status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&bytes)
    );
    let after: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(after["binding"], Value::Null);
    assert_eq!(after["providerBinding"], additive_binding(&id));
    assert_ne!(after["revision"], before["revision"]);
    assert!(!crate::workspace_context::primary_memory(
        &state.read().await.get_agent(&id).unwrap().state.config
    ));
    let disk = crate::control_plane_store::load_control_plane_snapshot(
        &crate::control_plane_store::ControlPlaneStoreConfig::Json(
            fixture.root.join("control.json"),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    let saved = &disk
        .agents
        .iter()
        .find(|agent| agent.state.id == id)
        .unwrap()
        .state
        .config;
    assert!(!crate::workspace_context::configured(saved));
    assert!(saved
        .settings
        .as_ref()
        .unwrap()
        .additional
        .contains_key("memoryProvider"));
    let stale = app
        .clone()
        .oneshot(request(
            &id,
            "PUT",
            json!({"expectedRevision":before["revision"],"binding":null,"providerBinding":null}),
        ))
        .await
        .unwrap();
    assert_eq!(stale.status(), StatusCode::CONFLICT);
    let old = app
        .clone()
        .oneshot(request(
            &id,
            "PUT",
            json!({"expectedRevision":after["revision"],"binding":null}),
        ))
        .await
        .unwrap();
    assert_eq!(old.status(), StatusCode::OK);
    assert_eq!(
        context_view(&app, &id).await["providerBinding"],
        additive_binding(&id)
    );
    let conflict = app
        .clone()
        .oneshot(request(
            &id,
            "PUT",
            json!({"expectedRevision":after["revision"],"binding":legacy}),
        ))
        .await
        .unwrap();
    assert_eq!(conflict.status(), StatusCode::BAD_REQUEST);
    let disabled = app
        .clone()
        .oneshot(request(
            &id,
            "PUT",
            json!({"expectedRevision":after["revision"],"binding":null,"providerBinding":null}),
        ))
        .await
        .unwrap();
    assert_eq!(disabled.status(), StatusCode::OK);
    assert_eq!(
        context_view(&app, &id).await["providerBinding"],
        Value::Null
    );
}

#[tokio::test]
async fn workspace_context_additive_rejects_invalid_authority_and_keeps_original_config() {
    let (_fixture, state, id) = provider_fixture("additive-reject").await;
    let app = router(state.clone(), DaemonConfig::default());
    let before = context_view(&app, &id).await;
    for (key, value) in [
        ("subject", json!("other")),
        ("generation", json!("")),
        ("version", json!(2)),
        ("provider", json!("other")),
        ("publication", json!("all")),
        ("connectionId", json!("foreign")),
        ("tool", json!("other")),
    ] {
        let mut binding = additive_binding(&id);
        binding[key] = value;
        let response = app.clone().oneshot(request(&id,"PUT",json!({"expectedRevision":before["revision"],"binding":null,"providerBinding":binding}))).await.unwrap();
        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "invalid {key} accepted"
        );
        assert_eq!(
            context_view(&app, &id).await["revision"],
            before["revision"]
        );
    }
    let mut shared_persona = additive_binding(&id);
    shared_persona["accessMode"] = json!("shared");
    shared_persona["personaSource"] = json!("workspace");
    let response = app.clone().oneshot(request(&id,"PUT",json!({"expectedRevision":before["revision"],"binding":null,"providerBinding":shared_persona}))).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    state
        .write()
        .await
        .mcp_registry
        .connections
        .get_mut(GARDEN_CONNECTION)
        .unwrap()
        .reviewed_tools[0]
        .descriptor
        .remote_name = "wrong_reader".into();
    let response = app.clone().oneshot(request(&id,"PUT",json!({"expectedRevision":before["revision"],"binding":null,"providerBinding":additive_binding(&id)}))).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn workspace_context_additive_rejects_helpers_and_explicit_deny() {
    let (_fixture, state, id) = provider_fixture("additive-policy").await;
    let app = router(state.clone(), DaemonConfig::default());
    state.write().await.approvals.set_policy(
        &id,
        crate::approvals::ApprovalPolicy::default().with(
            crate::approvals::RiskClass::Exec,
            crate::approvals::PolicyAction::Deny,
        ),
    );
    let before = context_view(&app, &id).await;
    let response = app.clone().oneshot(request(&id,"PUT",json!({"expectedRevision":before["revision"],"binding":null,"providerBinding":additive_binding(&id)}))).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    {
        let mut guard = state.write().await;
        guard
            .approvals
            .set_policy(&id, crate::approvals::ApprovalPolicy::default());
        let mut config = guard.get_agent(&id).unwrap().state.config;
        config
            .settings
            .get_or_insert_with(Default::default)
            .additional
            .insert(
                "workspaceRole".into(),
                anima_core::DataValue::String("helper".into()),
            );
        guard.restore_agent_config(&id, config);
    }
    let before = context_view(&app, &id).await;
    let response = app.clone().oneshot(request(&id,"PUT",json!({"expectedRevision":before["revision"],"binding":null,"providerBinding":additive_binding(&id)}))).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn workspace_context_additive_failed_persistence_restores_both_bindings() {
    let (fixture, state, id) = provider_fixture("additive-rollback").await;
    let legacy = json!({"connectionId":GARDEN_CONNECTION,"tool":"mother_get_context","mode":"companion",
        "workspaceId":"workspace","generation":"generation","subject":id});
    {
        let mut guard = state.write().await;
        let mut config = guard.get_agent(&id).unwrap().state.config;
        config
            .settings
            .get_or_insert_with(Default::default)
            .additional
            .insert("workspaceContext".into(), context_value(legacy.clone()));
        guard.restore_agent_config(&id, config);
    }
    let blocking = fixture.root.join("not-a-directory");
    std::fs::write(&blocking, "block").unwrap();
    state.write().await.set_control_plane_store(Some(
        crate::control_plane_store::ControlPlaneStoreConfig::Json(blocking.join("state.json")),
    ));
    let before = state.read().await.get_agent(&id).unwrap().state.config;
    let app = router(state.clone(), DaemonConfig::default());
    let view = context_view(&app, &id).await;
    let response = app.clone().oneshot(request(&id,"PUT",json!({"expectedRevision":view["revision"],"binding":null,"providerBinding":additive_binding(&id)}))).await.unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        state.read().await.get_agent(&id).unwrap().state.config,
        before
    );
    let after = context_view(&app, &id).await;
    assert_eq!(after["binding"], legacy);
    assert_eq!(after["providerBinding"], Value::Null);
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
