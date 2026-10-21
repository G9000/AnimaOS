use super::test_support::{chat_request, coordinator_with, Gate, ScriptedModel, Step};
use super::*;
use anima_core::ToolCall;
use std::collections::BTreeMap;
fn call(id: &str, name: &str, key: &str, value: &str) -> ToolCall {
    ToolCall {
        id: id.into(),
        name: name.into(),
        args: BTreeMap::from([(key.into(), DataValue::String(value.into()))]),
    }
}
async fn configure(coordinator: &AgentRunCoordinator, id: &str, enabled: bool) -> AgentConfig {
    let mut config = coordinator
        .state
        .read()
        .await
        .get_agent(id)
        .unwrap()
        .state
        .config;
    let registry = crate::tools::ToolRegistry::new();
    config.tools = Some(registry.resolve_descriptors(registry.tool_names()).unwrap());
    config
        .settings
        .as_mut()
        .unwrap()
        .additional
        .insert("toolDiscovery".into(), DataValue::Bool(enabled));
    coordinator
        .state
        .write()
        .await
        .restore_agent_config(id, config.clone());
    config
}
fn bytes(config: &AgentConfig) -> usize {
    serde_json::to_vec(&config.tools).unwrap().len()
}
#[tokio::test]
async fn tool_discovery_provider_request_is_compact_then_exact_native_schema_and_receipts() {
    let model = ScriptedModel::new(vec![
        Step::Tools(vec![call("search", "search_tools", "query", "memory_add")]),
        Step::Tools(vec![call(
            "save",
            "memory_add",
            "content",
            "The owner prefers compact discovery catalogs.",
        )]),
        Step::Text(vec!["saved"]),
    ]);
    let (coordinator, id) = coordinator_with(model.clone()).await;
    let original = configure(&coordinator, &id, true).await;
    let result = coordinator
        .run(chat_request(&id, "chat:discovery", "save preference"))
        .await
        .unwrap();
    assert_eq!(result.result.status, "success");
    let configs = model.configs();
    assert!(configs[0].allows_tool("search_tools"));
    assert!(!configs[0].allows_tool("memory_add"));
    assert!(bytes(&configs[0]) * 3 < bytes(&original));
    let exact = original
        .tools
        .as_ref()
        .unwrap()
        .iter()
        .find(|t| t.name == "memory_add")
        .unwrap();
    assert!(configs[1].tools.as_ref().unwrap().contains(exact));
    let state = coordinator.state.read().await;
    assert_eq!(state.get_agent(&id).unwrap().state.config, original);
    let run = state
        .runs
        .for_agent(&id)
        .into_iter()
        .find(|r| r.session_id == "chat:discovery")
        .unwrap();
    assert_eq!(run.action_receipts.len(), 2);
    assert!(run
        .action_receipts
        .iter()
        .all(|r| r.status == crate::runs::ActionStatus::Succeeded));
    assert!(state
        .memory
        .read()
        .await
        .snapshot()
        .memories
        .iter()
        .any(|m| m.content == "The owner prefers compact discovery catalogs."));
    eprintln!(
        "tool_discovery schema bytes: full={}, first={}, hydrated={}",
        bytes(&original),
        bytes(&configs[0]),
        bytes(&configs[1])
    );
}
#[tokio::test]
async fn tool_discovery_default_keeps_full_provider_catalog_and_no_discovery_tools() {
    let model = ScriptedModel::new(vec![Step::Text(vec!["done"])]);
    let (coordinator, id) = coordinator_with(model.clone()).await;
    let mut config = coordinator
        .state
        .read()
        .await
        .get_agent(&id)
        .unwrap()
        .state
        .config;
    config
        .settings
        .as_mut()
        .unwrap()
        .additional
        .remove("toolDiscovery");
    coordinator
        .state
        .write()
        .await
        .restore_agent_config(&id, config.clone());
    coordinator
        .run(chat_request(&id, "chat:legacy", "hello"))
        .await
        .unwrap();
    let sent = model.configs();
    assert!(sent[0].allows_tool("calculate"));
    assert!(!sent[0].allows_tool("search_tools"));
    assert!(!sent[0].allows_tool("describe_tool"));
}
#[tokio::test]
async fn tool_discovery_revoked_after_search_before_target_has_no_native_effect() {
    let gate = Gate::new();
    let model = ScriptedModel::gated(
        vec![
            Step::Tools(vec![call("search", "search_tools", "query", "memory_add")]),
            Step::Tools(vec![call(
                "save",
                "memory_add",
                "content",
                "revoked discovery must never save",
            )]),
            Step::Text(vec!["refused"]),
        ],
        gate.clone(),
    );
    let (coordinator, id) = coordinator_with(model).await;
    let mut original = configure(&coordinator, &id, true).await;
    let task = {
        let coordinator = coordinator.clone();
        let id = id.clone();
        tokio::spawn(async move {
            coordinator
                .run(chat_request(&id, "chat:revoke", "save preference"))
                .await
        })
    };
    gate.entered().await;
    gate.release();
    gate.entered().await;
    original
        .tools
        .as_mut()
        .unwrap()
        .retain(|t| t.name != "memory_add");
    coordinator
        .state
        .write()
        .await
        .restore_agent_config(&id, original);
    gate.release();
    gate.entered().await;
    gate.release();
    task.await.unwrap().unwrap();
    assert!(!coordinator
        .state
        .read()
        .await
        .memory
        .read()
        .await
        .snapshot()
        .memories
        .iter()
        .any(|m| m.content == "revoked discovery must never save"));
}
#[tokio::test]
async fn tool_discovery_reminder_verification_never_gets_selector_or_tools() {
    let model = ScriptedModel::new(vec![Step::Text(vec!["status only"])]);
    let (coordinator, id) = coordinator_with(model.clone()).await;
    configure(&coordinator, &id, true).await;
    let mut request = chat_request(&id, "schedule:discovery", "explain supplied status");
    request.source = RunSource::Schedule;
    request.content.metadata = Some(BTreeMap::from([(
        "reminderVerification".into(),
        DataValue::Bool(true),
    )]));
    coordinator.run(request).await.unwrap();
    assert!(model.configs()[0].tools.as_ref().unwrap().is_empty());
}

#[tokio::test]
async fn tool_discovery_helpers_never_reintroduce_filtered_parent_capabilities() {
    use anima_core::{CancelSignal, ModelToolSelector};
    let model = ScriptedModel::new(vec![]);
    let (coordinator, id) = coordinator_with(model).await;
    configure(&coordinator, &id, true).await;
    let parent = coordinator.state.read().await.get_agent(&id).unwrap().state;
    let mut helper = super::helper_config(&parent, "bounded helper".into());
    helper
        .settings
        .as_mut()
        .unwrap()
        .additional
        .insert("toolDiscovery".into(), DataValue::Bool(true));
    let mut helper = coordinator
        .state
        .write()
        .await
        .create_agent(helper)
        .unwrap()
        .state;
    let original = helper.config.clone();
    for name in ["search_tools", "describe_tool"] {
        if !helper.config.allows_tool(name) {
            helper
                .config
                .tools
                .as_mut()
                .unwrap()
                .push(crate::tools::ToolRegistry::new().descriptor(name).unwrap());
        }
    }
    let run = test_support::ledger_run(&coordinator, &helper.id, "helper:discovery").await;
    let catalog = crate::tools::catalog::RunToolCatalog::new(
        &coordinator.state,
        run.clone(),
        CancelSignal::default(),
        &original,
        &helper,
        None,
    );
    for forbidden in [
        "bash",
        "bg_start",
        "spawn_helper",
        "delegate_to_agent",
        "send_message",
        "browser_navigate",
        "core_memory",
        "search_conversations",
    ] {
        assert!(
            catalog.describe(&helper, &run, forbidden).await.is_err(),
            "helper discovered {forbidden}"
        );
        assert!(!catalog.visible_tool_names().contains(forbidden));
    }
    assert!(catalog.describe(&helper, &run, "calculate").await.is_ok());
}

async fn origin_discovery_run(helper: bool) {
    use test_support::{calculate_call, companion_config, lead_config};
    let model = ScriptedModel::new(vec![
        Step::Tools(vec![call("find", "search_tools", "query", "calculate")]),
        Step::Tools(vec![calculate_call("effect", "2+2")]),
        Step::Text(vec!["settled"]),
    ]);
    let (coordinator, source) = coordinator_with(model.clone()).await;
    let source_config = if helper {
        lead_config("source")
    } else {
        companion_config("source")
    };
    coordinator
        .state
        .write()
        .await
        .restore_agent_config(&source, source_config);
    let sender = coordinator
        .state
        .read()
        .await
        .get_agent(&source)
        .unwrap()
        .state;
    let mut target_config = if helper {
        super::helper_config(&sender, "target".into())
    } else {
        companion_config("target")
    };
    target_config
        .settings
        .as_mut()
        .unwrap()
        .additional
        .insert("toolDiscovery".into(), DataValue::Bool(true));
    let target = coordinator
        .state
        .write()
        .await
        .create_agent(target_config.clone())
        .unwrap()
        .state
        .id;
    assert!(!sender.config.allows_tool("search_tools"));
    assert!(!target_config.allows_tool("search_tools"));
    let mut request = chat_request(&target, "chat:origin-discovery", "calculate the result");
    request.source = if helper {
        RunSource::Delegation
    } else {
        RunSource::Peer
    };
    request.room = if helper {
        RunRoom::Delegated {
            parent_id: source.clone(),
        }
    } else {
        RunRoom::Peer {
            route: anima_core::AgentCommunicationRoute::start(&source)
                .forward(&source, &target)
                .unwrap(),
        }
    };
    coordinator.run(request).await.unwrap();
    let configs = model.configs();
    assert!(configs[0].allows_tool("search_tools"));
    assert!(!configs[0].allows_tool("calculate"));
    assert!(
        configs[1].allows_tool("calculate"),
        "ordinary discovery must hydrate an upstream-permitted tool"
    );
    let guard = coordinator.state.read().await;
    let run = guard.runs.for_agent(&target).into_iter().next().unwrap();
    assert_eq!(run.action_receipts.len(), 2);
    assert!(
        run.action_receipts
            .iter()
            .all(|r| r.status == crate::runs::ActionStatus::Succeeded),
        "discovery controls and native target use the ordinary receipt lane"
    );
    assert_eq!(
        guard.get_agent(&target).unwrap().state.config,
        target_config
    );
}
#[tokio::test]
async fn tool_discovery_helper_executes_settings_derived_controls_and_granted_target() {
    origin_discovery_run(true).await;
}
#[tokio::test]
async fn tool_discovery_peer_executes_settings_derived_controls_and_granted_target() {
    origin_discovery_run(false).await;
}

#[tokio::test]
async fn tool_discovery_peer_revocation_filters_search_and_blocks_hidden_target() {
    use test_support::{calculate_call, companion_config};
    let gate = Gate::new();
    let model = ScriptedModel::gated(
        vec![
            Step::Tools(vec![call("find", "search_tools", "query", "calculate")]),
            Step::Tools(vec![calculate_call("effect", "2+2")]),
            Step::Text(vec!["refused"]),
        ],
        gate.clone(),
    );
    let (coordinator, source) = coordinator_with(model.clone()).await;
    let mut target_config = companion_config("target");
    target_config
        .settings
        .as_mut()
        .unwrap()
        .additional
        .insert("toolDiscovery".into(), DataValue::Bool(true));
    let target = coordinator
        .state
        .write()
        .await
        .create_agent(target_config)
        .unwrap()
        .state
        .id;
    let mut request = chat_request(&target, "chat:peer-revoke", "calculate");
    request.source = RunSource::Peer;
    request.room = RunRoom::Peer {
        route: anima_core::AgentCommunicationRoute::start(&source)
            .forward(&source, &target)
            .unwrap(),
    };
    let run = {
        let coordinator = coordinator.clone();
        tokio::spawn(async move { coordinator.run(request).await })
    };
    gate.entered().await;
    let mut source_config = coordinator
        .state
        .read()
        .await
        .get_agent(&source)
        .unwrap()
        .state
        .config;
    source_config.tools = Some(vec![]);
    coordinator
        .state
        .write()
        .await
        .restore_agent_config(&source, source_config);
    gate.release();
    gate.entered().await;
    assert!(!model.configs()[1].allows_tool("calculate"));
    gate.release();
    gate.entered().await;
    gate.release();
    run.await.unwrap().unwrap();
    let guard = coordinator.state.read().await;
    let record = guard.runs.for_agent(&target).into_iter().next().unwrap();
    assert_eq!(record.action_receipts.len(), 1);
    assert_eq!(record.action_receipts[0].name, "search_tools");
    assert_eq!(
        record.action_receipts[0].status,
        crate::runs::ActionStatus::Succeeded
    );
    let snapshot = guard.get_agent(&target).unwrap();
    let search = snapshot
        .messages
        .iter()
        .find(|m| {
            m.role == anima_core::MessageRole::Tool && m.content.text.contains("schemaVisibility")
        })
        .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&search.content.text).unwrap()["tools"],
        serde_json::json!([])
    );
}
