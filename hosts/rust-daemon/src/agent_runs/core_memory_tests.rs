//! Which runs get the `core_memory` tool.

use std::sync::Arc;

use tokio::sync::{RwLock, Semaphore};

use super::test_support::{chat_request, companion_config, ScriptedModel, Step};
use super::AgentRunCoordinator;
use crate::state::DaemonState;

/// A coordinator whose one companion may use `tools`.
async fn coordinator(model: Arc<ScriptedModel>, tools: &[&str]) -> (AgentRunCoordinator, String) {
    let state = Arc::new(RwLock::new(DaemonState::with_model_adapter(model)));
    let agent_id = {
        let mut config = companion_config("companion");
        config.tools = Some(
            crate::tools::ToolRegistry::new()
                .resolve_descriptors(tools.iter().copied())
                .unwrap(),
        );
        state.write().await.create_agent(config).unwrap().state.id
    };
    (
        AgentRunCoordinator::new(state, Arc::new(Semaphore::new(8))),
        agent_id,
    )
}

#[tokio::test]
async fn a_run_gets_core_memory_exactly_when_the_agent_may_use_memory_add() {
    for (tools, expected) in [
        (&["calculate", "memory_add"][..], true),
        (&["calculate"][..], false),
        (&["calculate", "core_memory"][..], true),
    ] {
        let model = ScriptedModel::new(vec![Step::Text(vec!["ok"])]);
        let (coordinator, agent_id) = coordinator(model.clone(), tools).await;
        coordinator
            .run(chat_request(&agent_id, "chat:owner", "hello"))
            .await
            .unwrap();
        let config = &model.configs()[0];
        assert_eq!(config.allows_tool("core_memory"), expected, "{tools:?}");
        let count = config
            .tools
            .iter()
            .flatten()
            .filter(|tool| tool.name == "core_memory")
            .count();
        assert_eq!(count, usize::from(expected), "{tools:?}");
        // The canonical grant is never rewritten.
        let guard = coordinator.state.read().await;
        let canonical = guard.agents.get(&agent_id).unwrap().config().clone();
        assert_eq!(
            canonical.allows_tool("core_memory"),
            tools.contains(&"core_memory")
        );
    }
}

#[test]
fn a_helper_never_gets_core_memory() {
    let mut parent = companion_config("companion");
    parent.tools = Some(
        crate::tools::ToolRegistry::new()
            .resolve_descriptors(["memory_add", "core_memory", "calculate"])
            .unwrap(),
    );
    let mut state = DaemonState::new();
    let parent = state.create_agent(parent).unwrap().state;

    let helper = super::helper_config(&parent, "helper".into());

    let names: Vec<String> = helper
        .tools
        .unwrap_or_default()
        .into_iter()
        .map(|tool| tool.name)
        .collect();
    assert_eq!(names, ["memory_add", "calculate"]);
}

#[tokio::test]
async fn the_approval_gate_sees_core_memory_granted_through_memory_add() {
    let model = ScriptedModel::new(vec![]);
    let (with_add, agent_id) = coordinator(model.clone(), &["memory_add"]).await;
    assert_eq!(
        with_add.configured_tool(&agent_id, "core_memory").await,
        Some(true)
    );
    let (without, agent_id) = coordinator(model, &["calculate"]).await;
    assert_eq!(
        without.configured_tool(&agent_id, "core_memory").await,
        Some(false)
    );
}
