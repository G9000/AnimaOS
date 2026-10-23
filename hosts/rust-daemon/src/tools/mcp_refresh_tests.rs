use super::management_tests::management_call;
use super::*;
use crate::agent_runs::test_support::{chat_request, Gate, ScriptedModel, Step};

#[tokio::test]
async fn chat_mcp_refresh_loads_tools_without_expanding_native_permissions() {
    let f = Fixture::new(PolicyAction::Allow, false, false).await;
    let row = f.manager.get(&f.agent.id, &f.connection_id).await.unwrap();
    let row = f
        .manager
        .disconnect(&f.agent.id, &f.connection_id, row.record.version)
        .await
        .unwrap();
    let gate = Gate::new();
    let model = ScriptedModel::gated(
        vec![
            Step::Tools(vec![management_call(json!({"action":"refresh"}))]),
            Step::Tools(vec![f.call("explicit-refresh-call")]),
            Step::Text(vec!["Original request completed"]),
        ],
        gate.clone(),
    );
    f.state.write().await.model_adapter = model.clone();
    let worker = {
        let coordinator = f.coordinator.clone();
        let agent = f.agent.id.clone();
        tokio::spawn(async move {
            coordinator
                .run(chat_request(
                    &agent,
                    "chat:explicit-refresh",
                    "Refresh tools and read my project",
                ))
                .await
        })
    };
    gate.entered().await;
    assert!(!model.configs()[0].allows_tool(&f.alias));
    let native = crate::tools::ToolRegistry::new()
        .descriptor("write_file")
        .unwrap();
    {
        let mut state = f.state.write().await;
        let mut config = state.agents[&f.agent.id].config().clone();
        config.tools.get_or_insert_with(Vec::new).push(native);
        state.restore_agent_config(&f.agent.id, config);
    }
    f.manager.note_agent_permissions_changed(&f.agent.id);
    f.manager
        .connect(&f.agent.id, &f.connection_id, row.record.version)
        .await
        .unwrap();
    gate.release();
    gate.entered().await;
    assert!(model.configs()[1].allows_tool(&f.alias));
    assert!(!model.configs()[1].allows_tool("write_file"));
    let capabilities = serde_json::to_value(
        f.coordinator
            .chat_capabilities(&f.agent.id, "chat:explicit-refresh")
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(capabilities["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|tool| tool["name"] == f.alias && tool["available"] == true));
    assert_eq!(
        f.factory.calls.load(Ordering::SeqCst),
        0,
        "refresh performs no remote tool call"
    );
    gate.release();
    gate.entered().await;
    assert!(!model.configs()[2].allows_tool("write_file"));
    gate.release();
    assert_eq!(worker.await.unwrap().unwrap().result.status, "success");
    assert_eq!(f.factory.calls.load(Ordering::SeqCst), 1);
    assert!(f.state.read().await.agents[&f.agent.id]
        .config()
        .allows_tool("write_file"));
    f.manager.shutdown().await;
}

#[tokio::test]
async fn chat_mcp_grant_refreshes_reviewed_tools_but_still_requires_call_approval() {
    let f = Fixture::new(PolicyAction::Ask, false, false).await;
    let row = f.manager.get(&f.agent.id, &f.connection_id).await.unwrap();
    let row = f
        .manager
        .set_grant(
            &f.agent.id,
            &f.connection_id,
            &f.alias,
            McpGrantInput {
                expected_version: row.record.version,
                descriptor_digest: f.descriptor_digest.clone(),
                granted: false,
            },
        )
        .await
        .unwrap()
        .connection;
    let model = ScriptedModel::new(vec![
        Step::Tools(vec![management_call(
            json!({"action":"grant", "connection_id":f.connection_id,
            "expected_version":row.record.version, "alias":f.alias, "descriptor_digest":f.descriptor_digest, "granted":true}),
        )]),
        Step::Tools(vec![f.call("newly-granted-call")]),
        Step::Text(vec!["Approved project request completed"]),
    ]);
    f.state.write().await.model_adapter = model.clone();
    let worker = {
        let coordinator = f.coordinator.clone();
        let agent = f.agent.id.clone();
        tokio::spawn(async move {
            coordinator
                .run(chat_request(
                    &agent,
                    "chat:grant-refresh",
                    "Grant and use the project tool",
                ))
                .await
        })
    };
    for name in ["manage_mcp", &f.alias] {
        let approval = pending_approvals(&f.coordinator, 1).await.remove(0);
        assert_eq!(approval.tool, name);
        assert_eq!(f.factory.calls.load(Ordering::SeqCst), 0);
        f.coordinator
            .decide_approval(
                &approval.id,
                decision(ApprovalDecisionKind::AllowOnce, approval.revision),
            )
            .await
            .unwrap();
    }
    assert_eq!(worker.await.unwrap().unwrap().result.status, "success");
    assert!(!model.configs()[0].allows_tool(&f.alias));
    assert!(model.configs()[1].allows_tool(&f.alias));
    assert_eq!(f.factory.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        f.state
            .read()
            .await
            .runs
            .for_session(&f.agent.id, "chat:grant-refresh")
            .len(),
        1
    );
    f.manager.shutdown().await;
}

#[tokio::test]
async fn chat_mcp_disconnect_removes_a_hydrated_schema_in_the_same_run() {
    let f = Fixture::new(PolicyAction::Allow, false, false).await;
    {
        let mut state = f.state.write().await;
        let mut config = state.agents[&f.agent.id].config().clone();
        config
            .settings
            .get_or_insert_with(Default::default)
            .additional
            .insert("toolDiscovery".into(), DataValue::Bool(true));
        state.restore_agent_config(&f.agent.id, config);
    }
    let row = f.manager.get(&f.agent.id, &f.connection_id).await.unwrap();
    let model = ScriptedModel::new(vec![
        Step::Tools(vec![ToolCall {
            id: "hydrate-before-disconnect".into(),
            name: "search_tools".into(),
            args: [("query".into(), DataValue::String(f.alias.clone()))].into(),
        }]),
        Step::Tools(vec![management_call(
            json!({"action":"disconnect", "connection_id":f.connection_id, "expected_version":row.record.version}),
        )]),
        Step::Text(vec!["Disconnected"]),
    ]);
    f.state.write().await.model_adapter = model.clone();
    let result = f
        .coordinator
        .run(chat_request(
            &f.agent.id,
            "chat:disconnect-refresh",
            "Find the project tool then disconnect it",
        ))
        .await
        .unwrap();
    assert_eq!(result.result.status, "success");
    assert!(model.configs()[1].allows_tool(&f.alias));
    assert!(!model.configs()[2].allows_tool(&f.alias));
    assert_eq!(f.factory.calls.load(Ordering::SeqCst), 0);
    f.manager.shutdown().await;
}

#[tokio::test]
async fn chat_mcp_failed_setup_does_not_refresh_an_unrelated_connection_change() {
    let f = Fixture::new(PolicyAction::Allow, false, false).await;
    let row = f.manager.get(&f.agent.id, &f.connection_id).await.unwrap();
    let row = f
        .manager
        .disconnect(&f.agent.id, &f.connection_id, row.record.version)
        .await
        .unwrap();
    let gate = Gate::new();
    let model = ScriptedModel::gated(
        vec![
            Step::Tools(vec![management_call(
                json!({"action":"discover", "connection_id":f.connection_id, "expected_version":row.record.version + 999}),
            )]),
            Step::Text(vec!["Setup failed"]),
        ],
        gate.clone(),
    );
    f.state.write().await.model_adapter = model.clone();
    let worker = {
        let coordinator = f.coordinator.clone();
        let agent = f.agent.id.clone();
        tokio::spawn(async move {
            coordinator
                .run(chat_request(
                    &agent,
                    "chat:failed-refresh",
                    "Discover my MCP tools",
                ))
                .await
        })
    };
    gate.entered().await;
    f.manager
        .connect(&f.agent.id, &f.connection_id, row.record.version)
        .await
        .unwrap();
    gate.release();
    gate.entered().await;
    assert!(!model.configs()[1].allows_tool(&f.alias));
    gate.release();
    worker.await.unwrap().unwrap();
    assert_eq!(f.factory.calls.load(Ordering::SeqCst), 0);
    f.manager.shutdown().await;
}
