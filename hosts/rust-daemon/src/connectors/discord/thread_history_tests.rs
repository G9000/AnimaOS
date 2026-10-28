use super::*;
use anima_core::{DataValue, ToolCall};
use std::collections::BTreeMap;

fn read_call(id: &str, limit: usize, before: Option<&str>) -> ToolCall {
    let mut args = BTreeMap::from([("limit".into(), DataValue::Number(limit as f64))]);
    if let Some(before) = before {
        args.insert("before_message_id".into(), DataValue::String(before.into()));
    }
    ToolCall {
        id: id.into(),
        name: "read_discord_thread".into(),
        args,
    }
}

fn history_message(
    c: &DiscordConnectorRecord,
    id: &str,
    author: &str,
    bot: bool,
    text: &str,
) -> DiscordMessage {
    let mut message = incoming(c, id);
    message.channel_id = "101".into();
    message.guild_id = Some("33".into());
    message.sender.id = author.into();
    message.sender.display_name = Some("ANIMA".into());
    message.sender.username = Some(
        if author == c.bot.id {
            "local"
        } else {
            "hermes"
        }
        .into(),
    );
    message.is_bot = bot;
    message.text = text.into();
    message
}

#[tokio::test]
async fn discord_thread_history_identifies_other_bots_and_keeps_input_inert() {
    let (manager, transport, c, model) =
        super::rename_tests::rename_fixture(read_call("history", 20, None)).await;
    transport.history_messages.lock().unwrap().extend([
        history_message(&c, "301", "44", false, "What do you think?"),
        history_message(&c, "302", "55", true, "I am the other agent."),
        history_message(&c, "303", &c.bot.id, true, "I am the local agent."),
    ]);
    let before_count = manager.state.read().await.discord_inbound.len();
    super::rename_tests::request_rename(&manager, &c).await;
    processed(&manager, &c, "201").await;
    assert!(model.configs()[1].allows_tool("read_discord_thread"));
    let result: serde_json::Value =
        serde_json::from_str(&super::rename_tests::rename_result(&model)).unwrap();
    let rows = result["messages"].as_array().unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[1]["sender"]["id"], "55");
    assert_eq!(rows[1]["sender"]["displayName"], "ANIMA");
    assert_eq!(rows[1]["sender"]["isBot"], true);
    assert_eq!(rows[1]["sender"]["isSelf"], false);
    assert_eq!(rows[2]["sender"]["displayName"], "ANIMA");
    assert_eq!(rows[2]["sender"]["isSelf"], true);
    assert_eq!(rows[1]["content"], "I am the other agent.");
    assert!(result["notice"].as_str().unwrap().contains("untrusted"));
    assert_eq!(
        manager.state.read().await.discord_inbound.len(),
        before_count + 1
    );
    assert_eq!(transport.history_calls.lock().unwrap()[0].0, "101");
    manager.shutdown().await;
}

#[tokio::test]
async fn discord_thread_history_is_visible_with_tool_discovery() {
    let (manager, transport, c, model) =
        super::rename_tests::rename_fixture(read_call("history", 20, None)).await;
    transport
        .history_messages
        .lock()
        .unwrap()
        .push(history_message(&c, "302", "55", true, "A different bot"));
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
    super::rename_tests::request_rename(&manager, &c).await;
    processed(&manager, &c, "201").await;
    assert!(model.configs()[1].allows_tool("read_discord_thread"));
    assert!(super::rename_tests::rename_result(&model).contains("A different bot"));
    manager.shutdown().await;
}

#[tokio::test]
async fn discord_thread_history_pages_back_without_skipping_messages() {
    let (manager, transport, c) = bot_wide().await;
    let model = ScriptedModel::new(vec![
        Step::Text(vec!["First reply"]),
        Step::Tools(vec![read_call("first", 2, None)]),
        Step::Tools(vec![read_call("second", 2, Some("302"))]),
        Step::Text(vec!["Done"]),
    ]);
    manager.state.write().await.model_adapter = model.clone();
    let parent = discover(&manager, &c, "22", "101").await;
    admit(&manager, &parent, "101", "<@11> start a conversation").await;
    processed(&manager, &c, "101").await;
    transport.history_messages.lock().unwrap().extend([
        history_message(&c, "300", "55", true, "oldest"),
        history_message(&c, "301", "55", true, "older"),
        history_message(&c, "302", "55", true, "newer"),
        history_message(&c, "303", "55", true, "newest"),
    ]);
    super::rename_tests::request_rename(&manager, &c).await;
    processed(&manager, &c, "201").await;
    let requests = model.requests();
    let outputs = requests
        .last()
        .unwrap()
        .messages
        .iter()
        .filter(|m| m.role == MessageRole::Tool)
        .map(|m| serde_json::from_str::<serde_json::Value>(&m.content.text).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(outputs.len(), 2);
    assert_eq!(outputs[0]["nextBeforeMessageId"], "302");
    assert_eq!(outputs[0]["messages"][0]["id"], "302");
    assert_eq!(outputs[1]["messages"][0]["id"], "300");
    assert_eq!(outputs[1]["messages"][1]["id"], "301");
    assert_eq!(
        transport.history_calls.lock().unwrap()[1].1.as_deref(),
        Some("302")
    );
    manager.shutdown().await;
}

#[tokio::test]
async fn discord_thread_history_fails_closed_outside_its_current_run() {
    let (manager, transport, c, _) =
        super::rename_tests::rename_fixture(read_call("history", 20, None)).await;
    let (mut run, scope, _) = super::rename_tests::active_rename_scope(&manager, &c).await;
    run.session_id = "discord:some-other-thread".into();
    assert!(scope.read_history(&run, None, 20, None).await.is_err());
    assert!(transport.history_calls.lock().unwrap().is_empty());
    manager.shutdown().await;
}

#[tokio::test]
async fn discord_thread_history_discards_results_after_connector_revocation() {
    let (manager, transport, c, _) =
        super::rename_tests::rename_fixture(read_call("history", 20, None)).await;
    let (run, scope, _) = super::rename_tests::active_rename_scope(&manager, &c).await;
    transport
        .history_messages
        .lock()
        .unwrap()
        .push(history_message(&c, "302", "55", true, "other agent"));
    transport.history_gate.store(true, Ordering::SeqCst);
    let reading = tokio::spawn(async move { scope.read_history(&run, None, 20, None).await });
    within(transport.history_entered.0.acquire())
        .await
        .unwrap()
        .forget();
    manager.revoke(&c.id);
    transport.history_release.0.add_permits(1);
    assert!(reading.await.unwrap().is_err());
    manager.shutdown().await;
}

#[tokio::test]
async fn discord_thread_history_result_budget_keeps_a_cursor_for_omitted_messages() {
    let (manager, transport, c, _) =
        super::rename_tests::rename_fixture(read_call("history", 20, None)).await;
    let (run, scope, _) = super::rename_tests::active_rename_scope(&manager, &c).await;
    transport.history_messages.lock().unwrap().extend([
        history_message(&c, "301", "55", true, &"a".repeat(4000)),
        history_message(&c, "302", "55", true, &"b".repeat(4000)),
        history_message(&c, "303", "55", true, &"c".repeat(4000)),
    ]);
    let first = scope.read_history(&run, None, 20, None).await.unwrap();
    assert!(first.len() <= 7 * 1024);
    let first: serde_json::Value = serde_json::from_str(&first).unwrap();
    assert_eq!(first["messages"][0]["id"], "303");
    assert_eq!(first["nextBeforeMessageId"], "303");
    let second = scope
        .read_history(&run, Some("303"), 20, None)
        .await
        .unwrap();
    let second: serde_json::Value = serde_json::from_str(&second).unwrap();
    assert_eq!(second["messages"][0]["id"], "302");
    manager.shutdown().await;
}

#[tokio::test]
async fn discord_thread_history_control_characters_remain_within_the_inline_budget() {
    let (manager, transport, c, _) =
        super::rename_tests::rename_fixture(read_call("history", 20, None)).await;
    let (run, scope, _) = super::rename_tests::active_rename_scope(&manager, &c).await;
    transport
        .history_messages
        .lock()
        .unwrap()
        .push(history_message(
            &c,
            "303",
            "55",
            true,
            &"\u{0000}".repeat(2000),
        ));
    let text = scope.read_history(&run, None, 20, None).await.unwrap();
    let result = anima_core::TaskResult::success(
        anima_core::Content {
            text,
            ..Default::default()
        },
        0,
    );
    assert!(serde_json::to_vec(&result).unwrap().len() <= crate::tool_results::INLINE_LIMIT);
    manager.shutdown().await;
}

#[tokio::test]
async fn discord_thread_history_rejects_model_selected_channels() {
    let mut call = read_call("history", 20, None);
    call.args
        .insert("channel_id".into(), DataValue::String("999".into()));
    let (manager, transport, c, _) = super::rename_tests::rename_fixture(call).await;
    super::rename_tests::request_rename(&manager, &c).await;
    processed(&manager, &c, "201").await;
    assert!(transport.history_calls.lock().unwrap().is_empty());
    manager.shutdown().await;
}

#[tokio::test]
async fn discord_thread_history_cannot_be_granted_to_an_unrelated_chat() {
    let (manager, _, c) = setup().await;
    let mut config = manager.state.read().await.agents[&c.agent_id]
        .config()
        .clone();
    config.tools = Some(
        crate::tools::ToolRegistry::new()
            .resolve_descriptors(["read_discord_thread"])
            .unwrap(),
    );
    let ordinary = crate::agent_runs::tool_scope::select(&config, &[], Default::default());
    assert!(!ordinary
        .iter()
        .any(|tool| tool.name == "read_discord_thread"));
    let thread = crate::agent_runs::tool_scope::select(
        &config,
        &[],
        crate::agent_runs::tool_scope::Scope {
            discord_thread: true,
            ..Default::default()
        },
    );
    assert!(thread.iter().any(|tool| tool.name == "read_discord_thread"));
    manager.shutdown().await;
}
