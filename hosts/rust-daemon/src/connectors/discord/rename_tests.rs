use super::*;
use anima_core::{DataValue, ToolCall};
use std::collections::BTreeMap;

const RENAME: &str = "rename_discord_thread";

fn rename_call(title: &str) -> ToolCall {
    ToolCall {
        id: "rename-current-thread".into(),
        name: RENAME.into(),
        args: BTreeMap::from([("title".into(), DataValue::String(title.into()))]),
    }
}

#[test]
fn discord_rename_tool_has_only_a_title_parameter() {
    let descriptor = crate::tools::ToolRegistry::new()
        .descriptor(RENAME)
        .expect("Discord thread rename is registered");
    let json = serde_json::to_value(descriptor).unwrap();
    assert!(json.to_string().contains("title"));
    assert!(!json.to_string().contains("channel_id"));
}

#[tokio::test]
async fn discord_rename_tool_executes_in_the_current_thread_and_returns_acceptance() {
    let (manager, transport, c) = bot_wide().await;
    let model = ScriptedModel::new(vec![
        Step::Text(vec!["First reply"]),
        Step::Tools(vec![rename_call("Discord Title Test — Take Two")]),
        Step::Text(vec!["Renamed."]),
    ]);
    manager.state.write().await.model_adapter = model.clone();
    let parent = discover(&manager, &c, "22", "101").await;
    admit(&manager, &parent, "101", "<@11> start a conversation").await;
    processed(&manager, &c, "101").await;
    let thread = manager
        .get(&c.agent_id, &c.id)
        .await
        .unwrap()
        .destination_for_channel("101")
        .unwrap();
    admit(
        &manager,
        &thread,
        "201",
        "Rename this thread to Discord Title Test — Take Two",
    )
    .await;
    processed(&manager, &c, "201").await;
    assert_eq!(
        transport.channels.lock().unwrap()["101"].name.as_deref(),
        Some("Discord Title Test — Take Two")
    );
    assert_eq!(transport.rename_calls.load(Ordering::SeqCst), 1);
    assert!(model.configs()[1].allows_tool(RENAME));
    let requests = model.requests();
    let tool_result = requests
        .last()
        .unwrap()
        .messages
        .iter()
        .find(|m| m.role == MessageRole::Tool)
        .expect("accepted rename reaches the model");
    assert!(tool_result.content.text.contains("renamed"));
    assert!(tool_result
        .content
        .text
        .contains("Discord Title Test — Take Two"));
    manager.shutdown().await;
}

pub(super) async fn rename_fixture(
    call: ToolCall,
) -> (
    DiscordManager,
    Arc<FakeTransport>,
    DiscordConnectorRecord,
    Arc<ScriptedModel>,
) {
    let (manager, transport, c) = bot_wide().await;
    let model = ScriptedModel::new(vec![
        Step::Text(vec!["First reply"]),
        Step::Tools(vec![call]),
        Step::Text(vec!["Result received."]),
    ]);
    manager.state.write().await.model_adapter = model.clone();
    let parent = discover(&manager, &c, "22", "101").await;
    admit(&manager, &parent, "101", "<@11> start a conversation").await;
    processed(&manager, &c, "101").await;
    (manager, transport, c, model)
}

pub(super) async fn request_rename(manager: &DiscordManager, c: &DiscordConnectorRecord) {
    let thread = manager
        .get(&c.agent_id, &c.id)
        .await
        .unwrap()
        .destination_for_channel("101")
        .unwrap();
    admit(manager, &thread, "201", "Rename this thread to Take Two").await;
}

pub(super) fn rename_result(model: &ScriptedModel) -> String {
    model
        .requests()
        .last()
        .unwrap()
        .messages
        .iter()
        .filter(|m| m.role == MessageRole::Tool)
        .map(|m| m.content.text.clone())
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test]
async fn discord_rename_tool_waits_for_discord_acceptance_before_the_model_continues() {
    let (manager, transport, c, model) = rename_fixture(rename_call("Take Two")).await;
    transport.rename_gate.store(true, Ordering::SeqCst);
    request_rename(&manager, &c).await;
    assert!(manager.process_one(c.id.clone()).await.unwrap());
    within(transport.rename_entered.0.acquire())
        .await
        .unwrap()
        .forget();
    assert_eq!(
        model.requests().len(),
        2,
        "the final response cannot start before Discord accepts"
    );
    assert_eq!(
        transport.channels.lock().unwrap()["101"].name.as_deref(),
        Some("start a conversation")
    );
    transport.rename_release.0.add_permits(1);
    within(async {
        while !manager.state.read().await.discord_inbound[&(c.id.clone(), "201".into())]
            .processing_state
            .is_terminal()
        {
            tokio::task::yield_now().await;
        }
    })
    .await;
    assert!(rename_result(&model).contains("\"renamed\":true"));
    manager.shutdown().await;
}

#[tokio::test]
async fn discord_rename_tool_never_reports_success_for_denied_rate_limited_or_uncertain_effects() {
    for error in [
        DiscordTransportError::PermissionDenied,
        DiscordTransportError::RateLimited {
            retry_after_ms: 1000,
            global: false,
        },
        DiscordTransportError::Uncertain,
        DiscordTransportError::HttpStatus { status: 500 },
    ] {
        let (manager, transport, c, model) = rename_fixture(rename_call("Take Two")).await;
        *transport.rename_error.lock().unwrap() = Some(error);
        request_rename(&manager, &c).await;
        processed(&manager, &c, "201").await;
        let result = rename_result(&model);
        assert!(!result.contains("\"renamed\":true"), "{result}");
        if matches!(
            error,
            DiscordTransportError::Uncertain | DiscordTransportError::HttpStatus { status: 500 }
        ) {
            assert!(
                result.contains("uncertain") && result.contains("before retrying"),
                "{result}"
            );
        }
        assert_eq!(transport.rename_calls.load(Ordering::SeqCst), 1);
        manager.shutdown().await;
    }
}

#[tokio::test]
async fn discord_rename_tool_rejects_invalid_titles_and_target_arguments_before_effects() {
    let mut targeted = rename_call("Take Two");
    targeted
        .args
        .insert("channel_id".into(), DataValue::String("999".into()));
    for call in [
        rename_call(" "),
        rename_call("bad\nname"),
        rename_call(&"🦀".repeat(41)),
        targeted,
    ] {
        let (manager, transport, c, model) = rename_fixture(call).await;
        request_rename(&manager, &c).await;
        processed(&manager, &c, "201").await;
        assert_eq!(transport.rename_calls.load(Ordering::SeqCst), 0);
        assert!(!rename_result(&model).contains("\"renamed\":true"));
        manager.shutdown().await;
    }
}

#[tokio::test]
async fn discord_rename_tool_rejects_mismatched_discord_acknowledgement() {
    for mismatch in ["id", "guild", "parent", "name"] {
        let (manager, transport, c, model) = rename_fixture(rename_call("Take Two")).await;
        let mut response = transport.channels.lock().unwrap()["101"].clone();
        response.name = Some("Take Two".into());
        match mismatch {
            "id" => response.id = "999".into(),
            "guild" => response.guild_id = "999".into(),
            "parent" => response.parent_channel_id = Some("999".into()),
            _ => response.name = Some("Wrong title".into()),
        }
        *transport.rename_response.lock().unwrap() = Some(response);
        request_rename(&manager, &c).await;
        processed(&manager, &c, "201").await;
        assert!(!rename_result(&model).contains("\"renamed\":true"));
        assert!(rename_result(&model).contains("before retrying"));
        manager.shutdown().await;
    }
}

#[tokio::test]
async fn discord_rename_tool_updates_a_primary_selected_thread() {
    let (manager, transport, mut c) = setup().await;
    c.channel.kind = DiscordChannelKind::GuildThread;
    c.channel.parent_channel_id = Some("21".into());
    c.channel.name = Some("My chosen title".into());
    transport
        .channels
        .lock()
        .unwrap()
        .insert(c.channel.id.clone(), c.channel.clone());
    let model = ScriptedModel::new(vec![
        Step::Tools(vec![rename_call("Take Two")]),
        Step::Text(vec!["Result received."]),
    ]);
    {
        let mut state = manager.state.write().await;
        state.model_adapter = model.clone();
        state.discord_connectors.insert(c.id.clone(), c.clone());
    }
    admit(&manager, &c, "101", "<@11> rename this thread to Take Two").await;
    processed(&manager, &c, "101").await;
    assert!(rename_result(&model).contains("\"renamed\":true"));
    let saved = manager.get(&c.agent_id, &c.id).await.unwrap();
    assert_eq!(saved.channel.name.as_deref(), Some("Take Two"));
    validate_snapshot(&manager.state.read().await.control_plane_snapshot()).unwrap();
    manager.shutdown().await;
}

#[tokio::test]
async fn discord_discovers_granted_web_search_from_profile_controls() {
    let call = ToolCall {
        id: "find-web-search".into(),
        name: "search_tools".into(),
        args: BTreeMap::from([("query".into(), DataValue::String("web_search".into()))]),
    };
    let (manager, _, c, model) = rename_fixture(call).await;
    {
        let mut state = manager.state.write().await;
        let mut config = state.agents[&c.agent_id].config().clone();
        config
            .settings
            .as_mut()
            .unwrap()
            .additional
            .remove("toolDiscovery");
        config.tools = Some(
            crate::tools::ToolRegistry::new()
                .resolve_descriptors(["web_search", "search_tools", "describe_tool"])
                .unwrap(),
        );
        state.restore_agent_config(&c.agent_id, config);
    }
    request_rename(&manager, &c).await;
    processed(&manager, &c, "201").await;
    assert!(model.configs()[1].allows_tool("search_tools"));
    assert!(model.configs()[2].allows_tool("web_search"));
    assert!(rename_result(&model).contains("web_search"));
    manager.shutdown().await;
}

#[tokio::test]
async fn discord_rename_tool_is_available_with_tool_discovery_enabled() {
    let (manager, transport, c, model) = rename_fixture(rename_call("Take Two")).await;
    {
        let mut state = manager.state.write().await;
        let mut config = state.agents[&c.agent_id].config().clone();
        config
            .settings
            .as_mut()
            .unwrap()
            .additional
            .insert("toolDiscovery".into(), DataValue::Bool(true));
        state.restore_agent_config(&c.agent_id, config);
    }
    request_rename(&manager, &c).await;
    processed(&manager, &c, "201").await;
    assert_eq!(transport.rename_calls.load(Ordering::SeqCst), 1);
    assert!(model.configs()[1].allows_tool(RENAME));
    assert!(rename_result(&model).contains("\"renamed\":true"));
    manager.shutdown().await;
}

pub(super) async fn active_rename_scope(
    manager: &DiscordManager,
    c: &DiscordConnectorRecord,
) -> (
    crate::runs::RunLink,
    crate::connectors::discord::thread_tools::DiscordThreadScope,
    anima_core::AgentState,
) {
    let thread = manager
        .get(&c.agent_id, &c.id)
        .await
        .unwrap()
        .destination_for_channel("101")
        .unwrap();
    let scope = manager.thread_scope(&thread, manager.epoch(&c.id));
    let mut state = manager.state.write().await;
    let run_id = state.runs.for_session(&c.agent_id, &thread.room_id)[0]
        .id
        .clone();
    let run = state.runs.get_mut(&run_id).unwrap();
    run.status = crate::runs::RunStatus::Running;
    run.stop = None;
    state.live.runs().register(&run_id);
    let agent = state.get_agent(&c.agent_id).unwrap().state;
    (
        crate::runs::RunLink {
            agent_id: c.agent_id.clone(),
            session_id: thread.room_id,
            run_id,
        },
        scope,
        agent,
    )
}

#[tokio::test]
async fn discord_rename_tool_catalog_withdraws_the_exact_reconfigured_capability() {
    use anima_core::ModelToolSelector;
    let (manager, _, c, _) = rename_fixture(rename_call("Take Two")).await;
    let (link, scope, mut agent) = active_rename_scope(&manager, &c).await;
    let mut original = agent.config.clone();
    original
        .settings
        .as_mut()
        .unwrap()
        .additional
        .insert("toolDiscovery".into(), DataValue::Bool(true));
    agent.config.tools = Some(
        crate::tools::ToolRegistry::new()
            .resolve_descriptors([RENAME])
            .unwrap(),
    );
    let catalog = crate::tools::catalog::RunToolCatalog::new(
        &manager.state,
        link.clone(),
        anima_core::CancelSignal::default(),
        &original,
        &agent,
        None,
    )
    .with_discord_thread(Some(scope));
    assert!(catalog.visible_tool_names().contains(RENAME));
    assert!(catalog.describe(&agent, &link, RENAME).await.is_ok());
    manager
        .state
        .write()
        .await
        .discord_connectors
        .get_mut(&c.id)
        .unwrap()
        .configuration_revision += 1;
    assert!(!catalog.visible_tool_names().contains(RENAME));
    assert!(catalog.check(&agent, &link, RENAME).await.is_err());
    assert!(catalog.describe(&agent, &link, RENAME).await.is_err());
    assert!(!catalog
        .search(&agent, &link, "rename discord", 5)
        .await
        .unwrap()
        .contains(RENAME));
    manager.shutdown().await;
}

#[tokio::test]
async fn discord_rename_tool_refuses_revoked_and_cancelled_live_scopes() {
    for revoked in [true, false] {
        let (manager, transport, c, _) = rename_fixture(rename_call("Take Two")).await;
        let (link, scope, _) = active_rename_scope(&manager, &c).await;
        if revoked {
            manager.revoke(&c.id);
        } else {
            manager
                .state
                .read()
                .await
                .live
                .runs()
                .control(&link.run_id)
                .unwrap()
                .cancel
                .cancel();
        }
        assert!(scope.rename(&link, "Take Two", None).await.is_err());
        assert_eq!(transport.rename_calls.load(Ordering::SeqCst), 0);
        manager.shutdown().await;
    }
}

#[tokio::test]
async fn discord_rename_tool_failed_intent_save_sends_no_patch() {
    let (manager, transport, c, _) = rename_fixture(rename_call("Take Two")).await;
    let (link, scope, _) = active_rename_scope(&manager, &c).await;
    let gate = manager
        .state
        .write()
        .await
        .install_test_control_plane_save_gate(true);
    let task = tokio::spawn(async move { scope.rename(&link, "Take Two", None).await });
    within(gate.entered.acquire()).await.unwrap().forget();
    gate.release.add_permits(1);
    assert!(within(task).await.unwrap().is_err());
    assert_eq!(transport.rename_calls.load(Ordering::SeqCst), 0);
    manager.shutdown().await;
}

#[tokio::test]
async fn discord_rename_tool_explicit_rename_suppresses_pending_automatic_title() {
    let (manager, transport, c, _) = rename_fixture(rename_call("Take Two")).await;
    let (link, scope, _) = active_rename_scope(&manager, &c).await;
    let thread = manager
        .get(&c.agent_id, &c.id)
        .await
        .unwrap()
        .destination_for_channel("101")
        .unwrap();
    let model = ScriptedModel::with_secondary(vec![], vec![Step::Text(vec!["Automatic Title"])]);
    {
        let mut state = manager.state.write().await;
        state.model_adapter = model.clone();
        state.set_generated_titles(true);
    }
    transport.rename_gate.store(true, Ordering::SeqCst);
    let explicit = tokio::spawn(async move { scope.rename(&link, "Take Two", None).await });
    within(transport.rename_entered.0.acquire())
        .await
        .unwrap()
        .forget();
    manager
        .schedule_thread_title(
            &thread,
            &manager
                .state
                .read()
                .await
                .runs
                .for_session(&c.agent_id, &thread.room_id)[0]
                .id
                .clone(),
            "start".into(),
            "reply".into(),
            "start a conversation".into(),
            manager.epoch(&c.id),
        )
        .unwrap();
    within(async {
        while model.secondary_requests().is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await;
    transport.rename_release.0.add_permits(1);
    assert!(within(explicit).await.unwrap().is_ok());
    within(async {
        while manager.owned_count.load(Ordering::SeqCst) != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await;
    assert_eq!(transport.rename_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        transport.channels.lock().unwrap()["101"].name.as_deref(),
        Some("Take Two")
    );
    manager.shutdown().await;
}

#[tokio::test]
async fn discord_rename_tool_failed_metadata_save_does_not_claim_success_or_reenable_auto_title() {
    let (manager, transport, c, _) = rename_fixture(rename_call("Take Two")).await;
    let (link, scope, _) = active_rename_scope(&manager, &c).await;
    transport.rename_gate.store(true, Ordering::SeqCst);
    let task = tokio::spawn(async move { scope.rename(&link, "Take Two", None).await });
    within(transport.rename_entered.0.acquire())
        .await
        .unwrap()
        .forget();
    let gate = manager
        .state
        .write()
        .await
        .install_test_control_plane_save_gate(true);
    transport.rename_release.0.add_permits(1);
    within(gate.entered.acquire()).await.unwrap().forget();
    gate.release.add_permits(1);
    let error = within(task)
        .await
        .unwrap()
        .err()
        .expect("metadata failure cannot confirm success");
    assert!(error.contains("Discord accepted") && error.contains("before retrying"));
    assert_eq!(
        transport.channels.lock().unwrap()["101"].name.as_deref(),
        Some("Take Two")
    );
    let current = manager.get(&c.agent_id, &c.id).await.unwrap();
    let saved = current
        .conversations
        .iter()
        .find(|v| v.channel.id == "101")
        .unwrap();
    assert!(saved.auto_title.is_none());
    assert_eq!(saved.channel.name.as_deref(), Some("start a conversation"));
    manager.shutdown().await;
}

#[tokio::test]
async fn discord_rename_tool_cannot_be_granted_to_a_plain_channel_or_web_chat() {
    for web in [true, false] {
        let (manager, transport, c) = setup().await;
        let model = ScriptedModel::new(vec![
            Step::Tools(vec![rename_call("Take Two")]),
            Step::Text(vec!["No current Discord thread."]),
        ]);
        {
            let mut state = manager.state.write().await;
            state.model_adapter = model.clone();
            let mut config = state.agents[&c.agent_id].config().clone();
            config.tools = Some(state.tool_registry.resolve_descriptors([RENAME]).unwrap());
            state.restore_agent_config(&c.agent_id, config);
        }
        if web {
            let mut request = crate::agent_runs::test_support::chat_request(
                &c.agent_id,
                "chat:spoof",
                "Rename the current thread",
            );
            request.content.metadata = Some(BTreeMap::from([
                ("source".into(), DataValue::String("discord".into())),
                ("connectorId".into(), DataValue::String(c.id.clone())),
                ("channelId".into(), DataValue::String("101".into())),
            ]));
            manager.runs.run(request).await.unwrap();
        } else {
            admit(&manager, &c, "101", "Rename the current thread").await;
            processed(&manager, &c, "101").await;
        }
        assert!(!model.configs()[0].allows_tool(RENAME));
        assert_eq!(transport.rename_calls.load(Ordering::SeqCst), 0);
        manager.shutdown().await;
    }
}
