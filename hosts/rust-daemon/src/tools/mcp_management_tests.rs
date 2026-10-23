use super::*;
use crate::agent_runs::test_support::{chat_request, ScriptedModel, Step};

#[tokio::test]
async fn chat_mcp_setup_is_visible_before_a_server_is_configured() {
    let f = Fixture::new(PolicyAction::Allow, false, false).await;
    f.manager
        .delete(
            &f.agent.id,
            &f.connection_id,
            f.manager
                .get(&f.agent.id, &f.connection_id)
                .await
                .unwrap()
                .record
                .version,
        )
        .await
        .unwrap();
    assert!(f.manager.list(&f.agent.id).await.unwrap().is_empty());
    let model = ScriptedModel::new(vec![Step::Text(vec!["Send the MCP URL or configuration."])]);
    f.state.write().await.model_adapter = model.clone();
    let result = f
        .coordinator
        .run(chat_request(
            &f.agent.id,
            "chat:mcp-question",
            "Can you connect to MCP?",
        ))
        .await
        .unwrap();
    assert_eq!(result.result.status, "success");
    assert!(
        model.configs()[0].allows_tool("manage_mcp"),
        "The model needs a real MCP setup tool before it can offer to configure a server"
    );
    f.manager.shutdown().await;
}

fn management_call(args: Value) -> ToolCall {
    ToolCall {
        id: uuid::Uuid::new_v4().to_string(),
        name: "manage_mcp".into(),
        args: args
            .as_object()
            .unwrap()
            .iter()
            .map(|(key, value)| {
                (
                    key.clone(),
                    match value {
                        Value::String(text) => DataValue::String(text.clone()),
                        Value::Bool(value) => DataValue::Bool(*value),
                        Value::Number(value) => DataValue::Number(value.as_f64().unwrap()),
                        _ => panic!("test management arguments are primitive"),
                    },
                )
            })
            .collect(),
    }
}

async fn manage(f: &Fixture, args: Value) -> Arc<ScriptedModel> {
    let model = ScriptedModel::new(vec![
        Step::Tools(vec![management_call(args)]),
        Step::Text(vec!["setup result"]),
    ]);
    f.state.write().await.model_adapter = model.clone();
    f.coordinator
        .run(chat_request(
            &f.agent.id,
            &format!("chat:mcp-{}", uuid::Uuid::new_v4()),
            "Set up the MCP server I supplied",
        ))
        .await
        .unwrap();
    model
}

#[tokio::test]
async fn chat_mcp_setup_is_visible_with_tool_discovery_enabled() {
    let f = Fixture::new(PolicyAction::Allow, false, false).await;
    {
        let mut state = f.state.write().await;
        let mut config = state.get_agent(&f.agent.id).unwrap().state.config;
        config
            .settings
            .get_or_insert_with(Default::default)
            .additional
            .insert("toolDiscovery".into(), DataValue::Bool(true));
        state.restore_agent_config(&f.agent.id, config);
    }
    let model = manage(&f, json!({"action":"list"})).await;
    assert!(
        model.configs()[0].allows_tool("manage_mcp"),
        "actual provider configuration must advertise setup even with discovery enabled"
    );
    assert!(model
        .requests()
        .iter()
        .flat_map(|r| &r.messages)
        .any(|m| m.role == anima_core::MessageRole::Tool
            && m.content.text.contains("canConfigureMcp")));
    f.manager.shutdown().await;
}

#[tokio::test]
async fn chat_mcp_setup_configures_tests_grants_and_uses_tools_in_a_later_chat() {
    let f = Fixture::new(PolicyAction::Allow, false, false).await;
    manage(&f, json!({"action":"configure","label":"Project demo","configuration":
        json!({"mcpServers":{"project":{"url":"http://127.0.0.1:32124/mcp","type":"http"}}}).to_string()})).await;
    let row = f
        .manager
        .list(&f.agent.id)
        .await
        .unwrap()
        .into_iter()
        .find(|r| r.record.label == "Project demo")
        .expect("chat saved the connection");
    let id = row.record.id;
    assert!(!row.record.enabled);
    assert!(row.candidates.is_empty());
    manage(&f, json!({"action":"update","connection_id":id,"expected_version":row.record.version,"enabled":true})).await;
    let row = f.manager.get(&f.agent.id, &id).await.unwrap();
    assert!(row.record.enabled);
    manage(
        &f,
        json!({"action":"connect","connection_id":id,"expected_version":row.record.version}),
    )
    .await;
    let row = f.manager.get(&f.agent.id, &id).await.unwrap();
    assert_eq!(row.state, McpConnectionState::Ready);
    assert_eq!(row.candidates.len(), 1);
    assert_eq!(
        f.factory.calls.load(Ordering::SeqCst),
        0,
        "setup makes no remote tool calls"
    );
    let digest = row.candidates[0].descriptor_digest.clone();
    manage(
        &f,
        json!({"action":"review","connection_id":id,"expected_version":row.record.version,
        "discovery_id":row.discovery_id.unwrap(),"descriptor_digest":digest}),
    )
    .await;
    let row = f.manager.get(&f.agent.id, &id).await.unwrap();
    let alias = row.record.reviewed_tools[0].alias.clone();
    assert!(!f.state.read().await.agents[&f.agent.id]
        .config()
        .allows_tool(&alias));
    let before_grant = f.state.read().await.get_agent(&f.agent.id).unwrap().state;
    manage(
        &f,
        json!({"action":"grant","connection_id":id,"expected_version":row.record.version,
        "descriptor_digest":digest,"alias":alias,"granted":true}),
    )
    .await;
    assert!(f.state.read().await.agents[&f.agent.id]
        .config()
        .allows_tool(&alias));
    let call = ToolCall {
        id: "read-project".into(),
        name: alias.clone(),
        args: [("value".into(), DataValue::String("demo".into()))].into(),
    };
    assert!(
        f.manager
            .capture_dispatch(&before_grant, &call, &f.run)
            .is_err(),
        "an old run cannot acquire the new grant"
    );
    let model = ScriptedModel::new(vec![
        Step::Tools(vec![call]),
        Step::Text(vec!["project accessed"]),
    ]);
    f.state.write().await.model_adapter = model.clone();
    f.coordinator
        .run(chat_request(
            &f.agent.id,
            "chat:project-access",
            "Read my project record",
        ))
        .await
        .unwrap();
    assert!(model.configs()[0].allows_tool(&alias));
    assert_eq!(f.factory.calls.load(Ordering::SeqCst), 1);
    assert!(model
        .requests()
        .iter()
        .flat_map(|r| &r.messages)
        .any(|m| m.role == anima_core::MessageRole::Tool
            && m.content.text.contains("synthetic M42 value")));

    let config = f.state.read().await.control_plane_store.clone().unwrap();
    let saved = crate::control_plane_store::load_control_plane_snapshot(&config)
        .await
        .unwrap()
        .unwrap();
    f.manager.shutdown().await;
    let mut restarted = DaemonState::new();
    restarted.control_plane_store = Some(config);
    restarted.restore_control_plane_snapshot(saved).unwrap();
    assert!(restarted.agents[&f.agent.id].config().allows_tool(&alias));
    assert_eq!(
        restarted.mcp_registry.connections[&id].reviewed_tools[0].alias,
        alias
    );
    assert!(restarted.mcp_publication.read().unwrap().live.is_empty());
    let publication = restarted.mcp_publication.clone();
    let state = Arc::new(RwLock::new(restarted));
    let manager = McpManager::new(
        Arc::downgrade(&state),
        Arc::new(Mutex::new(())),
        publication,
        f.factory.clone(),
    );
    let row = manager.get(&f.agent.id, &id).await.unwrap();
    assert_eq!(row.state, McpConnectionState::Disconnected);
    assert!(manager.ready_descriptors(&f.agent.id).is_empty());
    manager
        .connect(&f.agent.id, &id, row.record.version)
        .await
        .unwrap();
    assert!(manager
        .ready_descriptors(&f.agent.id)
        .iter()
        .any(|tool| tool.name == alias));
    manager.shutdown().await;
}

#[tokio::test]
async fn chat_mcp_setup_rejects_secrets_before_approval_or_receipt_admission() {
    let f = Fixture::new(PolicyAction::Ask, false, false).await;
    let before = f.manager.list(&f.agent.id).await.unwrap().len();
    for config in [
        json!({"url":"https://example.com/mcp","headers":{"Authorization":"Bearer synthetic-mcp-secret"}}),
        json!({"command":"C:\\demo.exe","env":{"TOKEN":"synthetic-mcp-secret"}}),
    ] {
        let model = manage(&f, json!({"action":"configure","label":"Rejected demo","configuration":config.to_string()})).await;
        assert!(model
            .requests()
            .iter()
            .flat_map(|r| &r.messages)
            .any(|m| m.role == anima_core::MessageRole::Tool
                && m.content.text.contains("Invalid MCP setup arguments")));
    }
    let state = f.state.read().await;
    assert!(state.approvals.pending().is_empty());
    assert_eq!(
        state
            .runs
            .for_agent(&f.agent.id)
            .iter()
            .flat_map(|r| &r.action_receipts)
            .filter(|r| r.name == "manage_mcp")
            .count(),
        0
    );
    drop(state);
    assert_eq!(f.manager.list(&f.agent.id).await.unwrap().len(), before);
    f.manager.shutdown().await;
}

#[tokio::test]
async fn chat_mcp_setup_waiting_mutation_rechecks_cancelled_or_archived_owner_scope() {
    for archive in [false, true] {
        let f = Fixture::new(PolicyAction::Allow, false, false).await;
        f.state.read().await.live.runs().register(&f.run.run_id);
        let scoped = f.manager.for_owner_chat(f.run.clone());
        scoped.list(&f.agent.id).await.unwrap();
        let transactions = f.manager.control_plane_transactions();
        let hold = transactions.lock().await;
        let worker = {
            let manager = scoped.clone();
            let agent = f.agent.id.clone();
            tokio::spawn(async move {
                manager
                    .create(
                        &agent,
                        CreateMcpConnection {
                            label: "Must not save".into(),
                            endpoint: "https://example.com/mcp".into(),
                        },
                    )
                    .await
            })
        };
        for _ in 0..100 {
            if f.manager.pending_work_count() > 0 {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert!(f.manager.pending_work_count() > 0);
        if archive {
            f.state
                .write()
                .await
                .sessions
                .get_mut(&f.agent.id, &f.run.session_id)
                .unwrap()
                .archived = true;
        } else {
            f.state
                .read()
                .await
                .live
                .runs()
                .control(&f.run.run_id)
                .unwrap()
                .cancel
                .cancel();
        }
        drop(hold);
        assert!(worker.await.unwrap().is_err());
        assert_eq!(f.manager.list(&f.agent.id).await.unwrap().len(), 1);
        f.manager.shutdown().await;
    }
}

#[tokio::test]
async fn chat_mcp_setup_does_not_cross_agent_or_peer_scope() {
    let f = Fixture::new(PolicyAction::Allow, false, false).await;
    f.state.read().await.live.runs().register(&f.run.run_id);
    let other = f
        .state
        .write()
        .await
        .create_agent(agent_config("Other owner"))
        .unwrap()
        .state
        .id;
    assert!(f
        .manager
        .for_owner_chat(f.run.clone())
        .list(&other)
        .await
        .is_err());
    f.state
        .write()
        .await
        .runs
        .get_mut(&f.run.run_id)
        .unwrap()
        .source = crate::runs::RunSource::Peer;
    assert!(f
        .manager
        .for_owner_chat(f.run.clone())
        .list(&f.agent.id)
        .await
        .is_err());
    f.manager.shutdown().await;
}

#[tokio::test]
async fn chat_mcp_setup_stale_versions_cannot_disable_a_connection() {
    let f = Fixture::new(PolicyAction::Allow, false, false).await;
    let row = f.manager.get(&f.agent.id, &f.connection_id).await.unwrap();
    let stale = row.record.version - 1;
    let model = manage(&f, json!({"action":"update","connection_id":f.connection_id,"expected_version":stale,"enabled":false})).await;
    assert!(model
        .requests()
        .iter()
        .flat_map(|r| &r.messages)
        .any(|m| m.role == anima_core::MessageRole::Tool && m.content.text.contains("changed")));
    let current = f.manager.get(&f.agent.id, &f.connection_id).await.unwrap();
    assert_eq!(current.record.version, row.record.version);
    assert!(current.record.enabled);
    f.manager.shutdown().await;
}

#[tokio::test]
async fn chat_mcp_setup_cancelled_approval_never_creates_a_connection() {
    let f = Fixture::new(PolicyAction::Ask, false, false).await;
    let model = ScriptedModel::new(vec![
        Step::Tools(vec![management_call(
            json!({"action":"configure","label":"Cancelled setup",
        "configuration":json!({"url":"https://example.com/mcp"}).to_string()}),
        )]),
        Step::Text(vec!["done"]),
    ]);
    f.state.write().await.model_adapter = model;
    let worker = {
        let coordinator = f.coordinator.clone();
        let agent = f.agent.id.clone();
        tokio::spawn(async move {
            coordinator
                .run(chat_request(
                    &agent,
                    "chat:mcp-approval",
                    "Configure my MCP server",
                ))
                .await
        })
    };
    let approvals = pending_approvals(&f.coordinator, 1).await;
    let run = f
        .state
        .read()
        .await
        .runs
        .for_session(&f.agent.id, "chat:mcp-approval")[0]
        .id
        .clone();
    assert_eq!(approvals[0].tool, "manage_mcp");
    f.state
        .read()
        .await
        .live
        .runs()
        .control(&run)
        .unwrap()
        .cancel
        .cancel();
    tokio::time::timeout(std::time::Duration::from_secs(5), worker)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(f.manager.list(&f.agent.id).await.unwrap().len(), 1);
    f.manager.shutdown().await;
}

#[tokio::test]
async fn chat_mcp_setup_admitted_save_still_settles_after_owner_cancellation() {
    let f = Fixture::new(PolicyAction::Allow, false, false).await;
    f.state.read().await.live.runs().register(&f.run.run_id);
    let gate = f
        .state
        .write()
        .await
        .install_test_control_plane_save_gate(false);
    let worker = {
        let manager = f.manager.for_owner_chat(f.run.clone());
        let agent = f.agent.id.clone();
        tokio::spawn(async move {
            manager
                .create(
                    &agent,
                    CreateMcpConnection {
                        label: "Already admitted".into(),
                        endpoint: "https://example.com/mcp".into(),
                    },
                )
                .await
        })
    };
    gate.entered.acquire().await.unwrap().forget();
    f.state
        .read()
        .await
        .live
        .runs()
        .control(&f.run.run_id)
        .unwrap()
        .cancel
        .cancel();
    gate.release.add_permits(1);
    let saved = worker.await.unwrap().unwrap();
    assert_eq!(
        f.manager
            .get(&f.agent.id, &saved.record.id)
            .await
            .unwrap()
            .record
            .label,
        "Already admitted"
    );
    f.manager.shutdown().await;
}

#[tokio::test]
async fn chat_mcp_setup_can_refresh_disconnect_and_remove_its_connection() {
    let f = Fixture::new(PolicyAction::Allow, false, false).await;
    let row = f.manager.get(&f.agent.id, &f.connection_id).await.unwrap();
    let discovery = row.discovery_id;
    manage(&f,json!({"action":"discover","connection_id":f.connection_id,"expected_version":row.record.version})).await;
    let row = f.manager.get(&f.agent.id, &f.connection_id).await.unwrap();
    assert_ne!(row.discovery_id, discovery);
    assert_eq!(f.factory.calls.load(Ordering::SeqCst), 0);
    manage(&f,json!({"action":"disconnect","connection_id":f.connection_id,"expected_version":row.record.version})).await;
    assert!(f.manager.ready_descriptors(&f.agent.id).is_empty());
    let row = f.manager.get(&f.agent.id, &f.connection_id).await.unwrap();
    manage(&f,json!({"action":"remove","connection_id":f.connection_id,"expected_version":row.record.version})).await;
    assert!(f.manager.list(&f.agent.id).await.unwrap().is_empty());
    assert!(!f.state.read().await.agents[&f.agent.id]
        .config()
        .allows_tool(&f.alias));
    f.manager.shutdown().await;
}

#[tokio::test]
async fn chat_mcp_setup_live_cards_omit_rejected_credentials() {
    use crate::agent_runs::test_support::Gate;
    let f = Fixture::new(PolicyAction::Ask, false, false).await;
    let gate = Gate::new();
    let call = management_call(json!({"action":"configure","label":"Secret-free setup",
        "configuration":json!({"url":"https://example.com/mcp","headers":{"Authorization":"Bearer synthetic-live-secret"}}).to_string()}));
    let model = ScriptedModel::gated(
        vec![
            Step::Tools(vec![call]),
            Step::Text(vec!["Use the credential control."]),
        ],
        gate.clone(),
    );
    f.state.write().await.model_adapter = model;
    let worker = {
        let coordinator = f.coordinator.clone();
        let agent = f.agent.id.clone();
        tokio::spawn(async move {
            coordinator
                .run(chat_request(
                    &agent,
                    "chat:mcp-redacted",
                    "Configure my project MCP",
                ))
                .await
        })
    };
    gate.entered().await;
    gate.release();
    gate.entered().await;
    let state = f.state.read().await;
    let run = state.runs.for_session(&f.agent.id, "chat:mcp-redacted")[0]
        .id
        .clone();
    let live = state.live.runs().view(&run).unwrap();
    assert_eq!(live.tools.len(), 1);
    assert!(!format!("{live:?}").contains("synthetic-live-secret"));
    assert!(live.tools[0].arguments_preview.contains("omitted"));
    assert!(state.approvals.pending().is_empty());
    assert!(state.runs.get(&run).unwrap().action_receipts.is_empty());
    drop(state);
    gate.release();
    worker.await.unwrap().unwrap();
    f.manager.shutdown().await;
}
