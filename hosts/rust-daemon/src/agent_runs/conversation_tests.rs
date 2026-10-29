//! The companion searching its past conversations (spec §7.1).

use std::collections::BTreeMap;

use anima_core::{
    AgentStatus, Content, DataValue, Message, MessageRole, RuntimeRunDelta, TokenUsage, ToolCall,
};

use super::test_support::{chat_request, companion_config, ScriptedModel, Step};
use super::AgentRunCoordinator;
use crate::sessions::{SessionKind, SessionOrigin, SessionRecord, TitleSource};
use crate::state::DaemonState;

#[tokio::test]
async fn main_chat_clear_tool_finishes_without_restoring_the_old_transcript() {
    clear_with_discovery(false).await;
}

#[tokio::test]
async fn main_chat_clear_tool_is_available_with_discovery_enabled() {
    clear_with_discovery(true).await;
}

async fn clear_with_discovery(discovery: bool) {
    let model = ScriptedModel::new(vec![
        Step::Text(vec!["old answer"]),
        Step::Tools(vec![ToolCall {
            id: "clear-1".into(),
            name: "clear_main_chat".into(),
            args: BTreeMap::new(),
        }]),
        Step::Text(vec!["The chat will be cleared."]),
    ]);
    let (coordinator, agent) = super::test_support::coordinator_with(model.clone()).await;
    super::test_support::add_chat(&coordinator, &agent, "chat:main").await;
    coordinator
        .state
        .write()
        .await
        .sessions
        .get_mut(&agent, "chat:main")
        .unwrap()
        .main_chat = true;
    let mut config = coordinator
        .state
        .read()
        .await
        .get_agent(&agent)
        .unwrap()
        .state
        .config;
    config
        .settings
        .as_mut()
        .unwrap()
        .additional
        .insert("toolDiscovery".into(), DataValue::Bool(discovery));
    coordinator
        .state
        .write()
        .await
        .restore_agent_config(&agent, config);
    coordinator
        .run(chat_request(
            &agent,
            "chat:main",
            "old private conversation",
        ))
        .await
        .unwrap();
    let before = coordinator
        .state
        .read()
        .await
        .get_agent(&agent)
        .unwrap()
        .state
        .config;
    let memory = coordinator.state.read().await.memory_handle();
    crate::core_memory::apply_edit(
        &mut *memory.write().await,
        crate::core_memory::CoreAgent {
            id: &agent,
            name: "Companion",
        },
        crate::core_memory::CoreBlock::User,
        crate::core_memory::CoreEdit::Add {
            text: "The owner prefers jasmine tea.".into(),
        },
        crate::core_memory::ScanMode::Refuse,
    )
    .unwrap();
    let memories_before = memory.read().await.snapshot();
    coordinator
        .run(chat_request(&agent, "chat:main", "clear our conversation"))
        .await
        .unwrap();
    let guard = coordinator.state.read().await;
    assert!(guard
        .agents
        .get(&agent)
        .unwrap()
        .messages()
        .iter()
        .all(|m| m.room_id != "chat:main"));
    assert_eq!(guard.get_agent(&agent).unwrap().state.config, before);
    assert!(guard.sessions.get(&agent, "chat:main").unwrap().main_chat);
    assert!(model.configs()[1].allows_tool("clear_main_chat"));
    drop(guard);
    assert_eq!(memory.read().await.snapshot(), memories_before);
}

fn search_call(args: &[(&str, DataValue)]) -> ToolCall {
    ToolCall {
        id: "search-1".into(),
        name: "search_conversations".into(),
        args: args
            .iter()
            .map(|(key, value)| (key.to_string(), value.clone()))
            .collect::<BTreeMap<_, _>>(),
    }
}

fn read_call(args: &[(&str, DataValue)]) -> ToolCall {
    ToolCall {
        id: "read-window-1".into(),
        name: "read_conversation_window".into(),
        args: args
            .iter()
            .map(|(key, value)| (key.to_string(), value.clone()))
            .collect::<BTreeMap<_, _>>(),
    }
}

fn message(agent_id: &str, room: &str, id: &str, role: MessageRole, text: &str) -> Message {
    Message {
        id: id.into(),
        agent_id: agent_id.into(),
        room_id: room.into(),
        content: Content {
            text: text.into(),
            ..Content::default()
        },
        role,
        created_at_ms: 1,
    }
}

/// An agent allowed `search_conversations` with a past chat "Trip" about
/// Lisbon and the current chat, which mentions Lisbon too.
async fn searcher(model: std::sync::Arc<ScriptedModel>) -> (AgentRunCoordinator, String) {
    let mut config = companion_config("companion");
    config.tools = Some(
        crate::tools::ToolRegistry::new()
            .resolve_descriptors(["search_conversations", "read_conversation_window"])
            .unwrap(),
    );
    let mut state = DaemonState::with_model_adapter(model);
    let agent_id = state.create_agent(config).unwrap().state.id;
    for (id, title) in [("chat:trip", "Trip"), ("chat:now", "Now")] {
        state.sessions.insert(SessionRecord::new(
            &agent_id,
            id,
            SessionKind::Chat,
            SessionOrigin::Web,
            title.into(),
            TitleSource::Owner,
            1,
        ));
    }
    state
        .agents
        .get_mut(&agent_id)
        .unwrap()
        .apply_run_delta(&RuntimeRunDelta {
            messages: vec![
                message(
                    &agent_id,
                    "chat:trip",
                    "t1",
                    MessageRole::User,
                    "Book Lisbon for May",
                ),
                message(
                    &agent_id,
                    "chat:trip",
                    "t2",
                    MessageRole::Assistant,
                    "Booked the Lisbon flat",
                ),
                message(
                    &agent_id,
                    "chat:now",
                    "n1",
                    MessageRole::User,
                    "Lisbon again?",
                ),
                message(&agent_id, "chat:now", "n2", MessageRole::Assistant, "Maybe"),
            ],
            events: Vec::new(),
            event_total: 0,
            token_usage: TokenUsage::default(),
            step_count: 0,
            last_task: None,
            status: AgentStatus::Idle,
        });
    (
        AgentRunCoordinator::new(
            std::sync::Arc::new(tokio::sync::RwLock::new(state)),
            std::sync::Arc::new(tokio::sync::Semaphore::new(4)),
        ),
        agent_id,
    )
}

/// The text of the tool message a run recorded.
async fn tool_result(coordinator: &AgentRunCoordinator, agent_id: &str) -> String {
    coordinator.state.read().await.agents[agent_id]
        .messages()
        .iter()
        .rev()
        .find(|message| message.role == MessageRole::Tool)
        .map(|message| message.content.text.clone())
        .expect("the tool ran")
}

#[tokio::test]
async fn hermes_style_recall_browses_without_guessing_keywords_and_reads_session() {
    let model = ScriptedModel::new(vec![
        Step::Tools(vec![search_call(&[])]),
        Step::Tools(vec![read_call(&[(
            "session_id",
            DataValue::String("chat:trip".into()),
        )])]),
        Step::Text(vec!["We booked Lisbon for May."]),
    ]);
    let (coordinator, agent_id) = searcher(model).await;
    coordinator
        .run(chat_request(
            &agent_id,
            "chat:now",
            "Remember our other conversation?",
        ))
        .await
        .unwrap();
    let guard = coordinator.state.read().await;
    let results: Vec<serde_json::Value> = guard.agents[&agent_id]
        .messages()
        .iter()
        .filter(|message| message.role == MessageRole::Tool)
        .map(|message| serde_json::from_str(&message.content.text).unwrap())
        .collect();
    assert_eq!(
        results[0]["status"], "found",
        "browsing needs no guessed keywords"
    );
    assert_eq!(results[0]["results"][0]["sessionId"], "chat:trip");
    assert_eq!(
        results[1]["status"], "found",
        "a known session needs no search anchor"
    );
    assert_eq!(results[1]["messages"][0]["text"], "Book Lisbon for May");
    assert_eq!(results[1]["messages"][1]["text"], "Booked the Lisbon flat");
}

#[tokio::test]
async fn hermes_style_recall_hydrates_top_match_only_with_the_reader_grant() {
    for reading in [true, false] {
        let model = ScriptedModel::new(vec![
            Step::Tools(vec![search_call(&[(
                "query",
                DataValue::String("lisbon may".into()),
            )])]),
            Step::Text(vec!["Found the exchange."]),
        ]);
        let (coordinator, agent_id) = searcher(model).await;
        if !reading {
            let mut guard = coordinator.state.write().await;
            let mut config = guard.get_agent(&agent_id).unwrap().state.config;
            config
                .tools
                .as_mut()
                .unwrap()
                .retain(|tool| tool.name != "read_conversation_window");
            guard.restore_agent_config(&agent_id, config);
        }
        coordinator
            .run(chat_request(&agent_id, "chat:now", "What did we book?"))
            .await
            .unwrap();
        let result: serde_json::Value =
            serde_json::from_str(&tool_result(&coordinator, &agent_id).await).unwrap();
        let first = &result["results"][0];
        if reading {
            assert_eq!(first["context"]["status"], "found");
            assert_eq!(
                first["context"]["messages"][0]["text"],
                "Book Lisbon for May"
            );
            assert_eq!(
                first["context"]["messages"][1]["text"],
                "Booked the Lisbon flat"
            );
        } else {
            assert!(
                first.get("context").is_none(),
                "search-only access cannot read surrounding messages"
            );
        }
    }
}

#[tokio::test]
async fn hermes_style_recall_uses_real_catalog_and_cold_history_across_web_discord_telegram() {
    use crate::history::{HistoryService, HistoryStore, MemoryHistoryStore};
    use std::sync::Arc;
    for kind in [
        SessionKind::Chat,
        SessionKind::Discord,
        SessionKind::Telegram,
    ] {
        let session_id = format!("{}:stored", kind.as_str());
        let mut scroll = read_call(&[
            ("session_id", DataValue::String(session_id.clone())),
            ("anchor_message_id", DataValue::String("past-6".into())),
            ("before_count", DataValue::Number(19.0)),
            ("after_count", DataValue::Number(0.0)),
        ]);
        scroll.id = "scroll-earlier".into();
        let model = ScriptedModel::new(vec![
            Step::Tools(vec![ToolCall {
                id: "discover-search".into(),
                name: "search_tools".into(),
                args: BTreeMap::from([(
                    "query".into(),
                    DataValue::String("search_conversations".into()),
                )]),
            }]),
            Step::Tools(vec![search_call(&[(
                "kind",
                DataValue::String(kind.as_str().into()),
            )])]),
            Step::Tools(vec![ToolCall {
                id: "discover-read".into(),
                name: "search_tools".into(),
                args: BTreeMap::from([(
                    "query".into(),
                    DataValue::String("read_conversation_window".into()),
                )]),
            }]),
            Step::Tools(vec![read_call(&[(
                "session_id",
                DataValue::String(session_id.clone()),
            )])]),
            Step::Tools(vec![scroll]),
            Step::Text(vec!["The saved exchange records the exact agreement."]),
        ]);
        let (coordinator, agent) = searcher(model.clone()).await;
        let store = Arc::new(MemoryHistoryStore::new());
        let rows = (1..=25)
            .map(|index| {
                crate::history::conformance::history_message(
                    &format!("past-{index}"),
                    &agent,
                    &session_id,
                    MessageRole::User,
                    &format!("Exact agreement turn {index}"),
                    index,
                )
            })
            .collect::<Vec<_>>();
        store.upsert_messages(&rows).await.unwrap();
        {
            let mut guard = coordinator.state.write().await;
            let mut config = guard.get_agent(&agent).unwrap().state.config;
            config.tools.as_mut().unwrap().extend(
                crate::tools::ToolRegistry::new()
                    .resolve_descriptors(["search_tools", "describe_tool"])
                    .unwrap(),
            );
            config
                .settings
                .as_mut()
                .unwrap()
                .additional
                .insert("toolDiscovery".into(), DataValue::Bool(true));
            guard.restore_agent_config(&agent, config);
            let mut record = SessionRecord::new(
                &agent,
                &session_id,
                kind,
                SessionOrigin::Web,
                "Stored exchange".into(),
                TitleSource::Owner,
                30,
            );
            record.archived = true;
            guard.sessions.insert(record);
            guard.set_history(HistoryService::new(store));
        }
        let response = coordinator
            .run(chat_request(
                &agent,
                "chat:now",
                "Do you remember our discussion in the other chat?",
            ))
            .await
            .unwrap();
        assert_eq!(response.result.status, "success");
        let guard = coordinator.state.read().await;
        let results = guard.agents[&agent]
            .messages()
            .iter()
            .filter(|message| message.room_id == "chat:now" && message.role == MessageRole::Tool)
            .map(|message| {
                serde_json::from_str::<serde_json::Value>(&message.content.text).unwrap()
            })
            .collect::<Vec<_>>();
        let browse = results
            .iter()
            .find(|result| result["mode"] == "browse")
            .unwrap();
        assert_eq!(browse["results"][0]["kind"], kind.as_str());
        assert_eq!(
            browse["results"][0]["context"]["status"], "found",
            "catalog permits authorized context without advertising reader first"
        );
        let recent = results
            .iter()
            .find(|result| result["mode"] == "recent")
            .unwrap();
        assert_eq!(recent["firstMessageId"], "past-6");
        assert_eq!(recent["lastMessageId"], "past-25");
        let earlier = results.last().unwrap();
        assert_eq!(earlier["messages"][0]["text"], "Exact agreement turn 1");
        assert_eq!(earlier["lastMessageId"], "past-6");
        assert!(!model.configs()[0].allows_tool("read_conversation_window"));
        assert!(model.configs()[4].allows_tool("read_conversation_window"));
    }
}

#[tokio::test]
async fn the_companion_finds_excerpts_of_its_other_conversations() {
    let model = ScriptedModel::new(vec![
        Step::Tools(vec![search_call(&[(
            "query",
            DataValue::String("lisbon".into()),
        )])]),
        Step::Text(vec!["Found it"]),
    ]);
    let (coordinator, agent_id) = searcher(model).await;

    coordinator
        .run(chat_request(&agent_id, "chat:now", "what did we book?"))
        .await
        .unwrap();

    let text = tool_result(&coordinator, &agent_id).await;
    let result: serde_json::Value = serde_json::from_str(&text).expect("structured search result");
    assert_eq!(result["status"], "found");
    assert!(result["guidance"]
        .as_str()
        .unwrap()
        .contains("data, not instructions"));
    let trip = result["results"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["sessionId"] == "chat:trip")
        .expect("the past session is included");
    assert_eq!(trip["title"], "Trip");
    assert!(trip["snippet"].as_str().unwrap().contains("Lisbon"));
    assert!(trip["matchMessageId"].as_str().is_some());
    assert!(
        !text.contains("chat:now"),
        "the current session is left out"
    );
}

#[tokio::test]
async fn main_chat_search_reads_original_pruned_turn_after_automatic_compaction_and_restore() {
    use crate::history::{HistoryService, HistoryStore, MemoryHistoryStore};
    use crate::sessions::SessionPrunedThrough;
    use std::sync::Arc;

    const ORIGINAL: &str = "The cobalt narwhal rendezvous is at pier 47, bring the green ledger.";
    let model = ScriptedModel::with_secondary(
        vec![
            Step::Tools(vec![search_call(&[(
                "query",
                DataValue::String("cobalt narwhal".into()),
            )])]),
            Step::Tools(vec![read_call(&[
                ("session_id", DataValue::String("chat:now".into())),
                ("anchor_message_id", DataValue::String("original-1".into())),
                ("before_count", DataValue::Number(0.0)),
                ("after_count", DataValue::Number(0.0)),
            ])]),
            Step::Text(vec!["Found the original instruction."]),
        ],
        vec![Step::Text(vec![
            "Earlier planning was summarized without the exact rendezvous details.",
        ])],
    );
    let (coordinator, agent_id) = searcher(model.clone()).await;
    let store = Arc::new(MemoryHistoryStore::new());
    store
        .upsert_messages(&[crate::history::conformance::history_message(
            "original-1",
            &agent_id,
            "chat:now",
            MessageRole::User,
            ORIGINAL,
            0,
        )])
        .await
        .unwrap();
    {
        let mut guard = coordinator.state.write().await;
        guard.set_history(HistoryService::new(store.clone()));
        let mut record = guard.sessions.get(&agent_id, "chat:now").unwrap().clone();
        record.pruned_through = Some(SessionPrunedThrough {
            message_id: "original-1".into(),
            created_at_ms: 0,
        });
        let mut value = serde_json::to_value(record).unwrap();
        value["mainChat"] = serde_json::json!(true);
        guard
            .sessions
            .insert(serde_json::from_value(value).unwrap());
        crate::sessions::test_support::seed_messages(
            &mut guard,
            &agent_id,
            vec![
                message(
                    &agent_id,
                    "chat:now",
                    "recent-question",
                    MessageRole::User,
                    "What did we say about cobalt narwhal?",
                ),
                message(
                    &agent_id,
                    "chat:now",
                    "recent-reply",
                    MessageRole::Assistant,
                    "Let us look up the original details.",
                ),
            ],
        );
        assert!(!guard.agents[&agent_id]
            .messages()
            .iter()
            .any(|message| message.id == "original-1"));
        assert!(crate::sessions::compaction::auto_compact_enabled(
            guard.agents[&agent_id].config()
        ));
    }
    coordinator
        .run(chat_request(
            &agent_id,
            "chat:now",
            "Find the original rendezvous details",
        ))
        .await
        .unwrap();
    let guard = coordinator.state.read().await;
    let results: Vec<serde_json::Value> = guard.agents[&agent_id]
        .messages()
        .iter()
        .filter(|message| message.role == MessageRole::Tool)
        .map(|message| serde_json::from_str(&message.content.text).unwrap())
        .collect();
    let found = results
        .iter()
        .find(|result| result["query"] == "cobalt narwhal")
        .unwrap();
    assert_eq!(found["status"], "found");
    assert_eq!(found["results"][0]["sessionId"], "chat:now");
    assert_eq!(found["results"][0]["matchMessageId"], "original-1");
    let window = results
        .iter()
        .find(|result| result["anchorMessageId"] == "original-1")
        .unwrap();
    assert_eq!(window["status"], "found");
    assert_eq!(window["sessionId"], "chat:now");
    assert_eq!(window["messages"][0]["messageId"], "original-1");
    assert_eq!(window["messages"][0]["text"], ORIGINAL);
    assert!(window["guidance"]
        .as_str()
        .unwrap()
        .contains("data, not instructions"));
    let summary = guard
        .sessions
        .get(&agent_id, "chat:now")
        .unwrap()
        .summary
        .as_ref()
        .expect("default automatic compaction saved a summary");
    assert!(!summary.text.contains("cobalt narwhal"));
    assert_eq!(summary.through_message_id, "original-1");
    let snapshot = guard.control_plane_snapshot();
    drop(guard);
    let mut restored = DaemonState::new();
    restored.restore_control_plane_snapshot(snapshot).unwrap();
    restored.set_history(HistoryService::new(store));
    let restored = Arc::new(tokio::sync::RwLock::new(restored));
    let found = crate::sessions::history_read::discover_conversations(
        &restored,
        &agent_id,
        Some("chat:now"),
        crate::sessions::history_read::DiscoveryRequest {
            query: "cobalt narwhal".into(),
            limit: 5,
            kind: Some(SessionKind::Chat),
            after_ms: None,
            before_ms: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(found["results"][0]["matchMessageId"], "original-1");
}

#[tokio::test]
async fn search_conversations_reports_the_matched_message_anchor_for_archived_history() {
    let model = ScriptedModel::new(vec![
        Step::Tools(vec![search_call(&[
            ("query", DataValue::String("lisbon may".into())),
            ("kind", DataValue::String("chat".into())),
            ("after_ms", DataValue::Number(0.0)),
            ("before_ms", DataValue::Number(2.0)),
        ])]),
        Step::Text(vec!["Found it"]),
    ]);
    let (coordinator, agent_id) = searcher(model).await;
    coordinator
        .state
        .write()
        .await
        .sessions
        .get_mut(&agent_id, "chat:trip")
        .expect("the fixture owns the past session")
        .archived = true;
    assert!(
        coordinator
            .state
            .read()
            .await
            .sessions
            .get(&agent_id, "chat:trip")
            .expect("the fixture owns the archived session")
            .archived
    );

    coordinator
        .run(chat_request(&agent_id, "chat:now", "what did we book?"))
        .await
        .unwrap();

    let text = tool_result(&coordinator, &agent_id).await;
    let result: serde_json::Value = serde_json::from_str(&text).expect("structured search result");
    assert_eq!(result["status"], "found");
    let trip = result["results"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["sessionId"] == "chat:trip")
        .expect("the archived own session is included");
    assert_eq!(trip["archived"], true);
    assert_eq!(trip["matchMessageId"], "t1");
    assert_eq!(trip["matchedAtMs"], 1);
    assert!(trip["snippet"]
        .as_str()
        .unwrap()
        .contains("Book Lisbon for May"));
    assert!(result["results"]
        .as_array()
        .unwrap()
        .iter()
        .all(|item| item["sessionId"] != "chat:now"));
}

#[tokio::test]
async fn read_conversation_window_reads_the_exact_search_anchor() {
    let model = ScriptedModel::new(vec![
        Step::Tools(vec![search_call(&[(
            "query",
            DataValue::String("lisbon may".into()),
        )])]),
        Step::Tools(vec![read_call(&[
            ("session_id", DataValue::String("chat:trip".into())),
            ("anchor_message_id", DataValue::String("t1".into())),
            ("before_count", DataValue::Number(0.0)),
            ("after_count", DataValue::Number(0.0)),
        ])]),
        Step::Text(vec!["The user asked to book Lisbon for May."]),
    ]);
    let (coordinator, agent_id) = searcher(model).await;

    coordinator
        .run(chat_request(
            &agent_id,
            "chat:now",
            "what did I ask you to book?",
        ))
        .await
        .unwrap();

    let text = tool_result(&coordinator, &agent_id).await;
    let result: serde_json::Value = serde_json::from_str(&text).expect("structured window result");
    assert_eq!(result["status"], "found");
    assert_eq!(result["sessionId"], "chat:trip");
    assert_eq!(result["anchorMessageId"], "t1");
    assert_eq!(result["messages"].as_array().unwrap().len(), 1);
    assert_eq!(result["messages"][0]["messageId"], "t1");
    assert!(result["messages"][0]["text"]
        .as_str()
        .unwrap()
        .contains("Book Lisbon for May"));
}

#[tokio::test]
async fn no_match_and_bad_arguments_answer_plainly() {
    for (args, expected) in [
        (
            vec![("query", DataValue::String("zanzibar".into()))],
            "not_found",
        ),
        (
            vec![("query", DataValue::String("  ".into()))],
            "search_conversations query must be a non-empty string",
        ),
        (
            vec![
                ("query", DataValue::String("lisbon".into())),
                ("limit", DataValue::Number(11.0)),
            ],
            "search_conversations limit must be an integer from 1 to 10",
        ),
        (
            vec![("query", DataValue::String("x".repeat(201)))],
            "search_conversations query must be at most 200 characters",
        ),
    ] {
        let model = ScriptedModel::new(vec![
            Step::Tools(vec![search_call(&args)]),
            Step::Text(vec!["ok"]),
        ]);
        let (coordinator, agent_id) = searcher(model).await;
        coordinator
            .run(chat_request(&agent_id, "chat:now", "search"))
            .await
            .unwrap();
        let text = tool_result(&coordinator, &agent_id).await;
        if expected == "not_found" {
            let result: serde_json::Value =
                serde_json::from_str(&text).expect("structured search result");
            assert_eq!(result["status"], "not_found");
            assert_eq!(result["query"], "zanzibar");
        } else {
            assert!(text.contains(expected), "{expected} in {text}");
        }
    }
}

#[tokio::test]
async fn conversation_tools_reject_unknown_fields_and_oversized_windows() {
    let cases = [
        (
            search_call(&[
                ("query", DataValue::String("lisbon".into())),
                ("agent_id", DataValue::String("someone-else".into())),
            ]),
            "search_conversations received an unknown argument",
        ),
        (
            read_call(&[
                ("session_id", DataValue::String("chat:trip".into())),
                ("anchor_message_id", DataValue::String("t1".into())),
                ("cursor", DataValue::String("untrusted".into())),
            ]),
            "read_conversation_window received an unknown argument",
        ),
        (
            read_call(&[
                ("session_id", DataValue::String("chat:trip".into())),
                ("anchor_message_id", DataValue::String("t1".into())),
                ("before_count", DataValue::Number(19.0)),
                ("after_count", DataValue::Number(1.0)),
            ]),
            "read_conversation_window before_count plus after_count must be at most 19",
        ),
        (
            read_call(&[
                ("session_id", DataValue::String("x".repeat(257))),
                ("anchor_message_id", DataValue::String("t1".into())),
            ]),
            "read_conversation_window session_id must be a non-empty string up to 256 bytes",
        ),
        (
            search_call(&[
                ("query", DataValue::String("lisbon".into())),
                ("kind", DataValue::String("unsupported".into())),
            ]),
            "search_conversations kind must be chat, telegram, discord, email, checkin, job, or helper (own incoming agent conversations)",
        ),
        (
            search_call(&[
                ("query", DataValue::String("lisbon".into())),
                ("after_ms", DataValue::Number(9_007_199_254_740_992.0)),
            ]),
            "search_conversations after_ms must be a safe non-negative integer",
        ),
        (
            search_call(&[
                ("query", DataValue::String("lisbon".into())),
                ("after_ms", DataValue::Number(10.0)),
                ("before_ms", DataValue::Number(10.0)),
            ]),
            "search_conversations after_ms must be earlier than before_ms",
        ),
    ];
    for (call, expected) in cases {
        let model = ScriptedModel::new(vec![Step::Tools(vec![call]), Step::Text(vec!["ok"])]);
        let (coordinator, agent_id) = searcher(model).await;
        coordinator
            .run(chat_request(&agent_id, "chat:now", "search history"))
            .await
            .unwrap();
        let text = tool_result(&coordinator, &agent_id).await;
        assert!(text.contains(expected), "{expected} in {text}");
    }
}
