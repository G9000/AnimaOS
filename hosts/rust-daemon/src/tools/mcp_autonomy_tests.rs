use super::*;
use crate::agent_runs::test_support::{chat_request, ScriptedModel, Step};
use crate::approvals::autonomy::{AgentAutonomy, AutonomyMode};

#[tokio::test]
async fn free_mcp_runs_without_review_or_grant_with_and_without_tool_discovery() {
    for discovery in [false, true] {
        let f = Fixture::new(PolicyAction::Allow, false, false).await;
        let mut steps = vec![];
        if discovery {
            steps.push(Step::Tools(vec![ToolCall {
                id: "find-automatic-tool".into(),
                name: "search_tools".into(),
                args: [("query".into(), DataValue::String(f.alias.clone()))].into(),
            }]));
        }
        steps.push(Step::Tools(vec![f.call("automatic-tool-call")]));
        steps.push(Step::Text(vec!["Used the automatic MCP tool"]));
        let model = ScriptedModel::new(steps);
        let original = {
            let mut state = f.state.write().await;
            state.autonomy.set(AgentAutonomy {
                agent_id: f.agent.id.clone(),
                mode: AutonomyMode::Free,
                ..Default::default()
            });
            state.model_adapter = model.clone();
            state
                .mcp_registry
                .connections
                .get_mut(&f.connection_id)
                .unwrap()
                .reviewed_tools
                .clear();
            let mut config = state.agents[&f.agent.id].config().clone();
            config
                .tools
                .as_mut()
                .unwrap()
                .retain(|t| !t.name.starts_with("mcp_"));
            config
                .settings
                .get_or_insert_with(Default::default)
                .additional
                .insert("toolDiscovery".into(), DataValue::Bool(discovery));
            state.restore_agent_config(&f.agent.id, config.clone());
            config
        };
        // Refresh live discovery after changing the fixture's stored metadata.
        let row = f.manager.get(&f.agent.id, &f.connection_id).await.unwrap();
        f.manager
            .discover(&f.agent.id, &f.connection_id, row.record.version)
            .await
            .unwrap();
        let result = f
            .coordinator
            .run(chat_request(
                &f.agent.id,
                "chat:automatic-provider",
                "Use the enabled MCP tool",
            ))
            .await
            .unwrap();
        assert_eq!(result.result.status, "success");
        assert_eq!(f.factory.calls.load(Ordering::SeqCst), 1);
        assert!(model.configs()[usize::from(discovery)].allows_tool(&f.alias));
        let state = f.state.read().await;
        assert_eq!(state.agents[&f.agent.id].config(), &original);
        assert!(state.mcp_registry.connections[&f.connection_id]
            .reviewed_tools
            .is_empty());
        drop(state);
        assert!(pending_approvals(&f.coordinator, 0).await.is_empty());
        f.manager.shutdown().await;
    }
}
