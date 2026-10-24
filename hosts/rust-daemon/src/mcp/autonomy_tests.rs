use super::*;
use crate::approvals::autonomy::{AgentAutonomy, AutonomyMode};
use crate::approvals::{ApprovalPolicy, PolicyAction};

async fn policy(f: &Fixture, mode: AutonomyMode, exec: PolicyAction) {
    let mut state = f.state.write().await;
    let policy = mode.preset().unwrap_or(ApprovalPolicy {
        write: PolicyAction::Ask,
        exec,
        network: PolicyAction::Allow,
        delegate: PolicyAction::Allow,
    });
    state.approvals.set_policy(&f.agent, policy);
    state.autonomy.set(AgentAutonomy {
        agent_id: f.agent.clone(),
        mode,
        ..Default::default()
    });
    state.refresh_mcp_permissions(&f.agent).unwrap();
}

async fn connected(f: &Fixture) -> McpConnectionView {
    let row = f.create().await;
    let row = f
        .manager
        .update(
            &f.agent,
            &row.record.id,
            UpdateMcpConnection {
                expected_version: row.record.version,
                label: None,
                endpoint: None,
                enabled: Some(true),
            },
        )
        .await
        .unwrap();
    f.manager
        .connect(&f.agent, &row.record.id, row.record.version)
        .await
        .unwrap()
}

// Capture the same run-local descriptors the model receives, without adding
// anything to the persisted agent grant list.
async fn automatic_stamp(f: &Fixture) -> McpDispatchStamp {
    let tools = f.manager.ready_descriptors(&f.agent);
    assert_eq!(
        tools.len(),
        1,
        "validated automatic tools must be published"
    );
    let mut state = f.state.write().await;
    let sid = "chat:automatic-mcp";
    state.sessions.insert(SessionRecord::new(
        &f.agent,
        sid,
        SessionKind::Chat,
        SessionOrigin::Web,
        "Automatic MCP".into(),
        TitleSource::Owner,
        1,
    ));
    let run = RunRecord::running(
        RunStart {
            agent_id: f.agent.clone(),
            session_id: sid.into(),
            source: RunSource::Api,
            source_ref: None,
            idempotency_key: None,
            text: "Use an enabled MCP".into(),
            model: "mock".into(),
            provider: None,
            parent_run_id: None,
        },
        1,
    );
    let link = RunLink {
        agent_id: f.agent.clone(),
        session_id: sid.into(),
        run_id: run.id.clone(),
    };
    state.runs.insert(run);
    let mut agent = state.agents[&f.agent].state();
    agent.config.tools = Some(tools.clone());
    drop(state);
    f.manager
        .capture_dispatch(
            &agent,
            &ToolCall {
                id: uuid::Uuid::new_v4().to_string(),
                name: tools[0].name.clone(),
                args: Default::default(),
            },
            &link,
        )
        .unwrap()
}

#[tokio::test]
async fn free_and_custom_allow_invoke_without_persisting_reviews_or_grants() {
    for mode in [AutonomyMode::Free, AutonomyMode::Custom] {
        let f = Fixture::new().await;
        policy(&f, mode, PolicyAction::Allow).await;
        let original = f.state.read().await.agents[&f.agent].config().tools.clone();
        let row = connected(&f).await;
        assert_eq!(row.state, McpConnectionState::Ready);
        assert!(row.record.reviewed_tools.is_empty());
        let stamp = automatic_stamp(&f).await;
        f.manager
            .invoke(stamp, json!({"value":"violet"}), CancelSignal::new())
            .await
            .unwrap();
        assert_eq!(
            f.factory.sessions.lock().unwrap()[0]
                .calls
                .load(Ordering::SeqCst),
            1
        );
        assert_eq!(
            f.state.read().await.agents[&f.agent].config().tools,
            original
        );
        let saved = f.persisted().await;
        assert!(saved.mcp_connections[0].reviewed_tools.is_empty());
        assert_eq!(saved.agents[0].state.config.tools, original);
        assert!(saved.mcp_connections[0].enabled);
        let mut restored = DaemonState::new();
        restored.restore_control_plane_snapshot(saved).unwrap();
        restored.control_plane_store = Some(ControlPlaneStoreConfig::Json(
            f.folder.join("restored.json"),
        ));
        assert!(restored.mcp_publication.read().unwrap().live.is_empty());
        let publication = restored.mcp_publication.clone();
        let state = Arc::new(RwLock::new(restored));
        let manager = McpManager::new(
            Arc::downgrade(&state),
            Arc::new(Mutex::new(())),
            publication,
            f.factory.clone(),
        );
        let restored_view = manager.get(&f.agent, &row.record.id).await.unwrap();
        assert_eq!(restored_view.tool_access, McpToolAccess::Automatic);
        assert_eq!(restored_view.state, McpConnectionState::Disconnected);
        assert!(manager.ready_descriptors(&f.agent).is_empty());
        assert_eq!(
            manager.list(&f.agent).await.unwrap()[0].tool_access,
            McpToolAccess::Automatic
        );
        assert_eq!(
            manager
                .update(
                    &f.agent,
                    &row.record.id,
                    UpdateMcpConnection {
                        expected_version: row.record.version,
                        label: Some(row.record.label.clone()),
                        endpoint: None,
                        enabled: None,
                    }
                )
                .await
                .unwrap()
                .tool_access,
            McpToolAccess::Automatic
        );
        assert_eq!(
            manager
                .clear_bearer(&f.agent, &row.record.id, row.record.version)
                .await
                .unwrap()
                .tool_access,
            McpToolAccess::Automatic
        );
        assert_eq!(
            manager
                .disconnect(&f.agent, &row.record.id, row.record.version)
                .await
                .unwrap()
                .tool_access,
            McpToolAccess::Automatic
        );
        manager.shutdown().await;
        f.manager.shutdown().await;
    }
}

#[tokio::test]
async fn strict_and_custom_ask_require_exact_review_then_grant() {
    for mode in [AutonomyMode::Strict, AutonomyMode::Custom] {
        let f = Fixture::new().await;
        policy(&f, mode, PolicyAction::Ask).await;
        let row = connected(&f).await;
        assert_eq!(row.state, McpConnectionState::NeedsReview);
        assert!(f.manager.ready_descriptors(&f.agent).is_empty());
        let row = f
            .manager
            .review(
                &f.agent,
                &row.record.id,
                ReviewMcpTool {
                    expected_version: row.record.version,
                    discovery_id: row.discovery_id.unwrap(),
                    descriptor_digest: row.candidates[0].descriptor_digest.clone(),
                },
            )
            .await
            .unwrap();
        assert!(f.state.read().await.agents[&f.agent]
            .config()
            .tools
            .is_none());
        let ungranted = automatic_stamp(&f).await;
        assert_eq!(
            f.manager
                .invoke(ungranted, json!({"value":"violet"}), CancelSignal::new())
                .await
                .unwrap_err(),
            McpServiceError::Revoked
        );
        let stamp = f.grant_and_stamp(&row).await;
        f.manager
            .invoke(stamp, json!({"value":"violet"}), CancelSignal::new())
            .await
            .unwrap();
        assert_eq!(
            f.factory.sessions.lock().unwrap()[0]
                .calls
                .load(Ordering::SeqCst),
            1
        );
        f.manager.shutdown().await;
    }
}

#[tokio::test]
async fn switching_to_strict_cancels_queued_automatic_calls_before_transport() {
    let f = Fixture::new().await;
    policy(&f, AutonomyMode::Free, PolicyAction::Allow).await;
    f.factory.hold_calls.store(true, Ordering::SeqCst);
    connected(&f).await;
    let stamp = automatic_stamp(&f).await;
    let queued_stamp = stamp.clone();
    let session = f.factory.sessions.lock().unwrap()[0].clone();
    let manager = f.manager.clone();
    let first = tokio::spawn(async move {
        manager
            .invoke(stamp, json!({"value":"violet"}), CancelSignal::new())
            .await
    });
    timeout(Duration::from_secs(1), session.entered.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    let manager = f.manager.clone();
    let mut queued = tokio::spawn(async move {
        manager
            .invoke(queued_stamp, json!({"value":"violet"}), CancelSignal::new())
            .await
    });
    assert!(timeout(Duration::from_millis(20), &mut queued)
        .await
        .is_err());
    policy(&f, AutonomyMode::Strict, PolicyAction::Ask).await;
    assert!(timeout(Duration::from_secs(1), first)
        .await
        .unwrap()
        .unwrap()
        .is_err());
    assert_eq!(
        timeout(Duration::from_secs(1), queued)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err(),
        McpServiceError::Revoked
    );
    assert_eq!(session.calls.load(Ordering::SeqCst), 1);
    f.manager.shutdown().await;
}

#[tokio::test]
async fn automatic_tools_are_not_available_to_foreign_agents_or_helpers() {
    let f = Fixture::new().await;
    policy(&f, AutonomyMode::Free, PolicyAction::Allow).await;
    connected(&f).await;
    let stamp = automatic_stamp(&f).await;
    let descriptor = f.manager.ready_descriptors(&f.agent)[0].clone();
    let mut foreign = {
        let mut state = f.state.write().await;
        state
            .create_agent(crate::sessions::test_support::agent_config(
                "Other MCP owner",
            ))
            .unwrap()
            .state
    };
    foreign.config.tools = Some(vec![descriptor.clone()]);
    let call = ToolCall {
        id: "forged".into(),
        name: descriptor.name,
        args: Default::default(),
    };
    let mut link = RunLink {
        agent_id: foreign.id.clone(),
        session_id: stamp.session_id,
        run_id: stamp.run_id,
    };
    assert!(f.manager.ready_descriptors(&foreign.id).is_empty());
    assert!(f.manager.capture_dispatch(&foreign, &call, &link).is_err());
    foreign.id = f.agent.clone();
    link.agent_id = f.agent.clone();
    foreign
        .config
        .settings
        .get_or_insert_with(Default::default)
        .additional
        .insert(
            "workspaceRole".into(),
            anima_core::DataValue::String("helper".into()),
        );
    assert_eq!(
        f.manager
            .capture_dispatch(&foreign, &call, &link)
            .unwrap_err(),
        McpServiceError::Revoked
    );
    assert_eq!(
        f.factory.sessions.lock().unwrap()[0]
            .calls
            .load(Ordering::SeqCst),
        0
    );
    f.manager.shutdown().await;
}

#[tokio::test]
async fn custom_deny_exposes_no_reviewed_tools() {
    let f = Fixture::new().await;
    policy(&f, AutonomyMode::Custom, PolicyAction::Deny).await;
    let row = f.reviewed().await;
    assert!(f.manager.ready_descriptors(&f.agent).is_empty());
    assert!(f
        .manager
        .known_descriptor(&f.agent, &row.record.reviewed_tools[0].alias)
        .is_some());
    assert_eq!(
        f.factory.sessions.lock().unwrap()[0]
            .calls
            .load(Ordering::SeqCst),
        0
    );
    f.manager.shutdown().await;
}

async fn put(
    app: &axum::Router,
    f: &Fixture,
    route: &str,
    body: serde_json::Value,
) -> axum::http::StatusCode {
    use tower::ServiceExt;
    app.clone()
        .oneshot(
            crate::routes::owner_test_support::authenticate(
                axum::http::Request::builder()
                    .method("PUT")
                    .uri(format!("/api/agents/{}/{route}", f.agent))
                    .header("host", "127.0.0.1:8080")
                    .header("origin", "http://localhost:4200")
                    .header("content-type", "application/json"),
            )
            .body(axum::body::Body::from(body.to_string()))
            .unwrap(),
        )
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn autonomy_routes_refresh_access_and_fence_free_strict_free_dispatch() {
    let f = Fixture::new().await;
    policy(&f, AutonomyMode::Free, PolicyAction::Allow).await;
    let row = connected(&f).await;
    let stamp = automatic_stamp(&f).await;
    let app = crate::routes::router(f.state.clone(), crate::app::DaemonConfig::default());
    assert_eq!(
        put(&app, &f, "autonomy", json!({"mode":"strict"})).await,
        axum::http::StatusCode::OK
    );
    let strict = f.manager.get(&f.agent, &row.record.id).await.unwrap();
    assert_eq!(strict.state, McpConnectionState::NeedsReview);
    assert_eq!(strict.tool_access, McpToolAccess::Review);
    assert!(f.manager.ready_descriptors(&f.agent).is_empty());
    assert_eq!(
        put(&app, &f, "autonomy", json!({"mode":"free"})).await,
        axum::http::StatusCode::OK
    );
    assert_eq!(f.manager.ready_descriptors(&f.agent).len(), 1);
    assert_eq!(
        f.manager
            .invoke(stamp, json!({"value":"violet"}), CancelSignal::new())
            .await
            .unwrap_err(),
        McpServiceError::Revoked
    );
    let fresh = automatic_stamp(&f).await;
    f.manager
        .invoke(fresh, json!({"value":"violet"}), CancelSignal::new())
        .await
        .unwrap();
    assert_eq!(
        f.factory.sessions.lock().unwrap()[0]
            .calls
            .load(Ordering::SeqCst),
        1
    );
    assert!(f.persisted().await.mcp_connections[0]
        .reviewed_tools
        .is_empty());
    f.manager.shutdown().await;
}

#[tokio::test]
async fn custom_policy_route_refreshes_allow_ask_and_deny_without_reconnecting() {
    let f = Fixture::new().await;
    let row = connected(&f).await;
    let app = crate::routes::router(f.state.clone(), crate::app::DaemonConfig::default());
    for (exec, access, count) in [
        ("allow", McpToolAccess::Automatic, 1),
        ("ask", McpToolAccess::Review, 0),
        ("deny", McpToolAccess::Denied, 0),
    ] {
        assert_eq!(
            put(
                &app,
                &f,
                "approval-policy",
                json!({"write":"ask", "exec":exec,"network":"allow","delegate":"allow"})
            )
            .await,
            axum::http::StatusCode::OK
        );
        let current = f.manager.get(&f.agent, &row.record.id).await.unwrap();
        assert_eq!(current.tool_access, access);
        assert_eq!(f.manager.ready_descriptors(&f.agent).len(), count);
    }
    assert_eq!(f.factory.connects.load(Ordering::SeqCst), 1);
    f.manager.shutdown().await;
}

#[tokio::test]
async fn failed_permission_saves_restore_access_but_never_revive_old_stamps() {
    for route in ["autonomy", "approval-policy"] {
        let f = Fixture::new().await;
        policy(&f, AutonomyMode::Free, PolicyAction::Allow).await;
        let row = connected(&f).await;
        let stamp = automatic_stamp(&f).await;
        let app = crate::routes::router(f.state.clone(), crate::app::DaemonConfig::default());
        f.state.write().await.control_plane_store =
            Some(ControlPlaneStoreConfig::Json(f.folder.clone()));
        let body = if route == "autonomy" {
            json!({"mode":"strict"})
        } else {
            json!({"write":"ask","exec":"deny","network":"allow","delegate":"allow"})
        };
        assert_eq!(
            put(&app, &f, route, body).await,
            axum::http::StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(
            f.manager
                .get(&f.agent, &row.record.id)
                .await
                .unwrap()
                .tool_access,
            McpToolAccess::Automatic
        );
        assert_eq!(f.manager.ready_descriptors(&f.agent).len(), 1);
        assert_eq!(
            f.manager
                .invoke(stamp, json!({"value":"violet"}), CancelSignal::new())
                .await
                .unwrap_err(),
            McpServiceError::Revoked
        );
        assert_eq!(
            f.factory.sessions.lock().unwrap()[0]
                .calls
                .load(Ordering::SeqCst),
            0
        );
        f.manager.shutdown().await;
    }
}

#[tokio::test]
async fn automatic_discovery_replaces_old_schema_and_lifecycle_actions_revoke_access() {
    let f = Fixture::new().await;
    policy(&f, AutonomyMode::Free, PolicyAction::Allow).await;
    let row = connected(&f).await;
    let stamp = automatic_stamp(&f).await;
    let original_alias = stamp.alias.clone();
    let session = f.factory.sessions.lock().unwrap()[0].clone();
    session.changed_schema.store(true, Ordering::SeqCst);
    let row = f
        .manager
        .discover(&f.agent, &row.record.id, row.record.version)
        .await
        .unwrap();
    assert_eq!(row.state, McpConnectionState::Ready);
    assert_ne!(
        f.manager.ready_descriptors(&f.agent)[0].name,
        original_alias
    );
    assert!(f
        .manager
        .invoke(stamp, json!({"value":"violet"}), CancelSignal::new())
        .await
        .is_err());
    let stamp = automatic_stamp(&f).await;
    let row = f
        .manager
        .update(
            &f.agent,
            &row.record.id,
            UpdateMcpConnection {
                expected_version: row.record.version,
                enabled: Some(false),
                label: None,
                endpoint: None,
            },
        )
        .await
        .unwrap();
    assert!(f.manager.ready_descriptors(&f.agent).is_empty());
    assert!(f
        .manager
        .invoke(stamp, json!({"value":"violet"}), CancelSignal::new())
        .await
        .is_err());
    assert_eq!(session.calls.load(Ordering::SeqCst), 0);
    let persisted = f.persisted().await;
    assert!(persisted.mcp_connections[0].reviewed_tools.is_empty());
    assert!(!row.record.enabled);
    let mut restored = DaemonState::new();
    restored.restore_control_plane_snapshot(persisted).unwrap();
    assert!(restored.mcp_publication.read().unwrap().live.is_empty());
    assert!(restored.mcp_publication.read().unwrap().known.is_empty());
    f.manager.shutdown().await;
}
