//! A completed owner turn reaches the external memory provider, on the same
//! terms as a conversation review count: check-ins never do.
use super::test_support::{add_chat, chat_request, coordinator_with, ScriptedModel, Step};
use crate::memory_provider::test_support::{inject, FakeProvider};
use anima_core::DataValue;
use std::collections::BTreeMap;
use std::sync::Arc;

const ROOM: &str = "chat:provider";

#[tokio::test(flavor = "current_thread")]
async fn a_completed_owner_turn_sends_one_sync_turn_and_a_check_in_sends_none() {
    let fake = Arc::new(FakeProvider::new("fake"));
    let _active = inject(fake.clone());
    let model = ScriptedModel::new(vec![
        Step::Text(vec!["Noted, kilometres it is."]),
        Step::Text(vec!["All quiet here."]),
        Step::Text(vec!["Second answer."]),
    ]);
    let (coordinator, agent) = coordinator_with(model).await;
    add_chat(&coordinator, &agent, ROOM).await;

    coordinator
        .run(chat_request(&agent, ROOM, "I prefer kilometres"))
        .await
        .unwrap();
    fake.wait_until(|fake| fake.turns.lock().unwrap().len() == 1)
        .await;
    {
        let turns = fake.turns.lock().unwrap();
        assert_eq!(turns[0].agent_id, agent);
        assert_eq!(turns[0].session_id, "chat:provider");
        assert_eq!(turns[0].user_text, "I prefer kilometres");
        assert_eq!(turns[0].reply_text, "Noted, kilometres it is.");
        assert!(turns[0].at_ms > 0);
    }

    let mut checkin = chat_request(&agent, ROOM, "checkin: anything new?");
    checkin.content.metadata = Some(BTreeMap::from([(
        "kind".to_string(),
        DataValue::String("checkin".into()),
    )]));
    coordinator.run(checkin).await.unwrap();
    // A later owner turn proves the check-in's slot has passed.
    coordinator
        .run(chat_request(&agent, ROOM, "and another"))
        .await
        .unwrap();
    fake.wait_until(|fake| fake.turns.lock().unwrap().len() == 2)
        .await;
    let turns = fake.turns.lock().unwrap();
    let users: Vec<_> = turns.iter().map(|turn| turn.user_text.as_str()).collect();
    assert_eq!(users, ["I prefer kilometres", "and another"]);
}
#[tokio::test(flavor = "current_thread")]
async fn workspace_context_offline_companion_does_not_sync_or_write_local_memory() {
    let fake = Arc::new(FakeProvider::new("fake"));
    let _active = inject(fake.clone());
    let model = ScriptedModel::new(vec![Step::Text(vec!["offline answer"])]);
    let (coordinator, agent) = coordinator_with(model.clone()).await;
    {
        let mut state = coordinator.state.write().await;
        let mut config = state.agents[&agent].config().clone();
        config.settings.as_mut().unwrap().additional.insert(
            "workspaceContext".into(),
            DataValue::Object(BTreeMap::from([
                ("connectionId".into(), DataValue::String("offline".into())),
                (
                    "tool".into(),
                    DataValue::String("mother_get_context".into()),
                ),
                ("mode".into(), DataValue::String("companion".into())),
                ("workspaceId".into(), DataValue::String("workspace".into())),
                ("generation".into(), DataValue::String("generation".into())),
                ("subject".into(), DataValue::String(agent.clone())),
            ])),
        );
        state.restore_agent_config(&agent, config);
    }
    add_chat(&coordinator, &agent, ROOM).await;
    coordinator
        .run(chat_request(&agent, ROOM, "hello"))
        .await
        .unwrap();
    tokio::task::yield_now().await;
    assert!(
        fake.turns.lock().unwrap().is_empty(),
        "offline companion must not sync fallback turns"
    );
    let requests = model.requests();
    assert!(requests[0].system.contains("Workspace context unavailable"));
    assert!(!model.configs()[0].allows_tool("memory_add"));
    assert!(fake.recall_requests.lock().unwrap().is_empty());
    assert!(fake.writes.lock().unwrap().is_empty());
    let memory = coordinator.state.read().await.memory_handle();
    assert!(
        memory
            .read()
            .await
            .get_recent(anima_memory::RecentMemoryOptions {
                agent_id: Some(agent.clone()),
                ..Default::default()
            })
            .is_empty(),
        "offline fallback must not persist TaskResult memories"
    );
}

#[tokio::test]
async fn workspace_context_helpers_never_inherit_binding_or_remote_tools() {
    let model = ScriptedModel::new(vec![]);
    let (coordinator, agent) = coordinator_with(model).await;
    let mut parent = coordinator
        .state
        .read()
        .await
        .get_agent(&agent)
        .unwrap()
        .state;
    parent.config.settings.as_mut().unwrap().additional.insert(
        "workspaceContext".into(),
        DataValue::Object(BTreeMap::from([(
            "mode".into(),
            DataValue::String("companion".into()),
        )])),
    );
    let child = super::helper_config_base(&parent, "bounded helper".into());
    assert!(!crate::workspace_context::configured(&child));
    assert!(child
        .tools
        .iter()
        .flatten()
        .all(|tool| !tool.name.starts_with("mcp_")));
    let request = chat_request(
        &agent,
        "chat:frame-measurement",
        "Synthetic frame measurement",
    );
    let ticket = coordinator
        .acquire_ticket(&request, super::PermitMode::TryNow, None, None)
        .await
        .unwrap();
    let future =
        coordinator.run_locked_inner(request, ticket, |_, _| Ok(()), None, None, None, None);
    let bytes = std::mem::size_of_val(&future);
    println!("ordinary run coordinator future: {bytes} bytes; MCP manager: {} bytes; tool context: {} bytes",std::mem::size_of::<crate::mcp::McpManager>(),std::mem::size_of::<crate::tools::ToolExecutionContext>());
    assert!(bytes<2*1024*1024,"ordinary run future must fit within a default worker stack before accounting for call frames");
}
