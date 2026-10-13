//! The companion searching its past conversations (spec §7.1).

use std::collections::BTreeMap;

use anima_core::{
    AgentStatus, Content, DataValue, Message, MessageRole, RuntimeRunDelta, TokenUsage, ToolCall,
};

use super::test_support::{chat_request, companion_config, ScriptedModel, Step};
use super::AgentRunCoordinator;
use crate::sessions::{SessionKind, SessionOrigin, SessionRecord, TitleSource};
use crate::state::DaemonState;

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
                ("kind", DataValue::String("helper".into())),
            ]),
            "search_conversations kind must be chat, telegram, discord, checkin, or job",
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
