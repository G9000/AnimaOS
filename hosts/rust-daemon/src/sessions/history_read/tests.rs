use std::collections::BTreeMap;
use std::sync::Arc;

use anima_core::{Content, DataValue, MessageRole};
use tokio::sync::RwLock;

use super::*;
use crate::history::conformance::{history_message as stored_message, FlakyHistoryStore};
use crate::history::{HistoryService, MemoryHistoryStore};
use crate::sessions::test_support::{agent_config, message, seed_messages};
use crate::sessions::{SessionOrigin, TitleSource};

fn history_message(
    agent: &str,
    session: &str,
    id: &str,
    role: MessageRole,
    text: &str,
    at: u64,
) -> HistoryMessage {
    stored_message(id, agent, session, role, text, at)
}

fn session(agent: &str, id: &str, kind: SessionKind, title: &str) -> SessionRecord {
    SessionRecord::new(
        agent,
        id,
        kind,
        SessionOrigin::Web,
        title.into(),
        TitleSource::Owner,
        1,
    )
}
fn discovery() -> DiscoveryRequest {
    DiscoveryRequest {
        query: "nebula".into(),
        limit: 5,
        kind: Some(SessionKind::Chat),
        after_ms: Some(10),
        before_ms: Some(20),
    }
}
fn window(session: &str, anchor: &str, before: usize, after: usize) -> WindowRequest {
    WindowRequest {
        session_id: session.into(),
        anchor_message_id: anchor.into(),
        before_count: before,
        after_count: after,
    }
}
fn ids(value: &Value) -> Vec<String> {
    value["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["messageId"].as_str().unwrap().into())
        .collect()
}

#[tokio::test]
async fn hermes_style_browse_includes_own_cross_platform_and_archived_cold_history() {
    let mut daemon = DaemonState::new();
    let agent = daemon.create_agent(agent_config("owner")).unwrap().state.id;
    let other = daemon.create_agent(agent_config("other")).unwrap().state.id;
    let store = Arc::new(MemoryHistoryStore::new());
    daemon.set_history(HistoryService::new(store.clone()));
    for (index, kind) in [
        SessionKind::Chat,
        SessionKind::Telegram,
        SessionKind::Discord,
    ]
    .into_iter()
    .enumerate()
    {
        let id = format!("{}:own", kind.as_str());
        let mut record = session(&agent, &id, kind, "Saved exchange");
        record.last_activity_at_ms = 100 + index as u64;
        record.archived = true;
        daemon.sessions.insert(record);
        store
            .upsert_messages(&[history_message(
                &agent,
                &id,
                &format!("anchor-{index}"),
                MessageRole::User,
                "The exact promise survives in cold history",
                100 + index as u64,
            )])
            .await
            .unwrap();
    }
    daemon.sessions.insert(session(
        &other,
        "chat:foreign",
        SessionKind::Chat,
        "Private exchange",
    ));
    store
        .upsert_messages(&[history_message(
            &other,
            "chat:foreign",
            "foreign-anchor",
            MessageRole::User,
            "FOREIGN_SECRET",
            200,
        )])
        .await
        .unwrap();
    let state = Arc::new(RwLock::new(daemon));
    let found = discover_conversations(
        &state,
        &agent,
        Some("chat:own"),
        DiscoveryRequest {
            query: String::new(),
            limit: 5,
            kind: None,
            after_ms: None,
            before_ms: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(found["mode"], "browse");
    let results = found["results"].as_array().unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0]["kind"], "discord");
    assert_eq!(results[1]["kind"], "telegram");
    assert_eq!(results[0]["matchMessageId"], "anchor-2");
    assert!(!found.to_string().contains("FOREIGN_SECRET"));
    assert!(size(&found) <= DISCOVERY_BYTES);
}

#[tokio::test]
async fn hermes_style_recent_read_scrolls_cold_and_hot_messages_for_each_chat_platform() {
    for kind in [
        SessionKind::Chat,
        SessionKind::Telegram,
        SessionKind::Discord,
    ] {
        let mut daemon = DaemonState::new();
        let agent = daemon.create_agent(agent_config("owner")).unwrap().state.id;
        let store = Arc::new(MemoryHistoryStore::new());
        daemon.set_history(HistoryService::new(store.clone()));
        let id = format!("{}:past", kind.as_str());
        let mut record = session(&agent, &id, kind, "Past exchange");
        record.archived = true;
        daemon.sessions.insert(record);
        let rows = (1..25)
            .map(|index| {
                history_message(
                    &agent,
                    &id,
                    &format!("past-{index}"),
                    MessageRole::User,
                    &format!("Exact turn {index}"),
                    index,
                )
            })
            .collect::<Vec<_>>();
        store.upsert_messages(&rows).await.unwrap();
        seed_messages(
            &mut daemon,
            &agent,
            vec![message(
                &agent,
                "past-25",
                &id,
                MessageRole::Assistant,
                "The final agreement",
                25,
            )],
        );
        let state = Arc::new(RwLock::new(daemon));
        let recent = read_latest_conversation(&state, &agent, &id, 19)
            .await
            .unwrap();
        assert_eq!(recent["messages"].as_array().unwrap().len(), 20);
        assert_eq!(recent["firstMessageId"], "past-6");
        assert_eq!(recent["lastMessageId"], "past-25");
        assert_eq!(recent["messages"][19]["text"], "The final agreement");
        assert_eq!(recent["hasEarlier"], true);
        assert_eq!(recent["hasLater"], false);
        assert_eq!(recent["kind"], kind.as_str());
        let earlier = read_conversation_window(&state, &agent, window(&id, "past-6", 19, 0))
            .await
            .unwrap();
        assert_eq!(
            ids(&earlier),
            (1..=6)
                .map(|index| format!("past-{index}"))
                .collect::<Vec<_>>()
        );
        let later = read_conversation_window(&state, &agent, window(&id, "past-6", 0, 19))
            .await
            .unwrap();
        assert_eq!(later["lastMessageId"], "past-25");
        assert_eq!(
            later["messages"][0]["createdAt"],
            "1970-01-01T00:00:00.006Z"
        );
    }
}

#[tokio::test]
async fn hermes_style_browse_cursor_pages_equal_activity_without_duplicates() {
    let mut daemon = DaemonState::new();
    let agent = daemon.create_agent(agent_config("owner")).unwrap().state.id;
    for id in ["chat:a", "chat:b", "chat:c"] {
        let mut record = session(&agent, id, SessionKind::Chat, "Own");
        record.last_activity_at_ms = 42;
        daemon.sessions.insert(record);
        seed_messages(
            &mut daemon,
            &agent,
            vec![message(
                &agent,
                &format!("{id}-anchor"),
                id,
                MessageRole::User,
                id,
                42,
            )],
        );
    }
    let state = Arc::new(RwLock::new(daemon));
    let request = DiscoveryRequest {
        query: String::new(),
        limit: 2,
        kind: None,
        after_ms: None,
        before_ms: None,
    };
    let first = browse_conversations(&state, &agent, None, request.clone(), None)
        .await
        .unwrap();
    assert_eq!(first["results"][0]["sessionId"], "chat:c");
    assert_eq!(first["results"][1]["sessionId"], "chat:b");
    let cursor = BrowseCursor::parse(first["nextCursor"].as_str().unwrap()).unwrap();
    let second = browse_conversations(&state, &agent, None, request, Some(cursor))
        .await
        .unwrap();
    assert_eq!(second["results"].as_array().unwrap().len(), 1);
    assert_eq!(second["results"][0]["sessionId"], "chat:a");
    assert_eq!(second["hasMore"], false);
    assert_eq!(second["nextCursor"], Value::Null);
}

#[tokio::test]
async fn hermes_style_title_only_search_can_open_real_context_within_discovery_budget() {
    let mut daemon = DaemonState::new();
    let agent = daemon.create_agent(agent_config("owner")).unwrap().state.id;
    daemon.sessions.insert(session(
        &agent,
        "chat:past",
        SessionKind::Chat,
        "Nebula title",
    ));
    let messages = (1..=20)
        .map(|index| {
            message(
                &agent,
                &format!("message-{index}"),
                "chat:past",
                MessageRole::User,
                &"字".repeat(3000),
                index,
            )
        })
        .collect();
    seed_messages(&mut daemon, &agent, messages);
    let state = Arc::new(RwLock::new(daemon));
    let mut request = discovery();
    request.after_ms = None;
    request.before_ms = None;
    let mut found = discover_conversations(&state, &agent, None, request)
        .await
        .unwrap();
    assert_eq!(found["results"][0]["matchMessageId"], Value::Null);
    hydrate_top_result(&state, &agent, &mut found)
        .await
        .unwrap();
    assert_eq!(found["results"][0]["sessionId"], "chat:past");
    assert_eq!(found["results"][0]["context"]["status"], "found");
    assert!(size(&found) <= DISCOVERY_BYTES, "{} bytes", size(&found));
    let context = &found["results"][0]["context"];
    assert_eq!(context["shortened"], true);
    assert_eq!(
        context["lastMessageId"],
        context["messages"].as_array().unwrap().last().unwrap()["messageId"]
    );
}

#[tokio::test]
async fn hermes_style_browse_rechecks_every_row_after_all_store_reads_and_keeps_cursor() {
    for delete_first in [false, true] {
        let mut daemon = DaemonState::new();
        let agent = daemon.create_agent(agent_config("owner")).unwrap().state.id;
        let store = Arc::new(ExactReadStore::new(None, false));
        for id in ["chat:a", "chat:b", "chat:c"] {
            let mut record = session(&agent, id, SessionKind::Chat, "Own exchange");
            record.last_activity_at_ms = 42;
            daemon.sessions.insert(record);
            seed_messages(
                &mut daemon,
                &agent,
                vec![message(
                    &agent,
                    &format!("{id}-source"),
                    id,
                    MessageRole::User,
                    if id == "chat:c" {
                        "PRIVATE_PREVIEW_SENTINEL"
                    } else {
                        "Other visible exchange"
                    },
                    10,
                )],
            );
        }
        store.page_reads_before_gate.store(
            if delete_first { 0 } else { 1 },
            std::sync::atomic::Ordering::SeqCst,
        );
        daemon.set_history(HistoryService::new(store.clone()));
        let state = Arc::new(RwLock::new(daemon));
        let request = DiscoveryRequest {
            query: String::new(),
            limit: if delete_first { 1 } else { 2 },
            kind: None,
            after_ms: None,
            before_ms: None,
        };
        let task_state = state.clone();
        let task_agent = agent.clone();
        let task_request = request.clone();
        let task = tokio::spawn(async move {
            browse_conversations(&task_state, &task_agent, None, task_request, None).await
        });
        tokio::time::timeout(Duration::from_secs(1), store.entered.acquire())
            .await
            .unwrap()
            .unwrap()
            .forget();
        {
            let mut guard = state.write().await;
            if delete_first {
                guard.sessions.remove(&agent, "chat:c");
            } else {
                seed_messages(
                    &mut guard,
                    &agent,
                    vec![
                        crate::sessions::test_support::checkin_prompt(
                            &agent,
                            "chat:c-source",
                            "chat:c",
                            "s1",
                            "PRIVATE_PREVIEW_SENTINEL",
                            10,
                        ),
                        message(
                            &agent,
                            "silent",
                            "chat:c",
                            MessageRole::Assistant,
                            "CHECKIN_OK",
                            11,
                        ),
                    ],
                );
            }
        }
        store.release.add_permits(1);
        let first = task.await.unwrap().unwrap();
        assert!(!first.to_string().contains("PRIVATE_PREVIEW_SENTINEL"));
        assert_eq!(first["hasMore"], true);
        let cursor = BrowseCursor::parse(
            first["nextCursor"]
                .as_str()
                .expect("skipped pages retain a continuation"),
        )
        .unwrap();
        let second = browse_conversations(&state, &agent, None, request, Some(cursor))
            .await
            .unwrap();
        assert_eq!(
            second["results"][0]["sessionId"],
            if delete_first { "chat:b" } else { "chat:a" }
        );
    }
}

#[tokio::test]
async fn hermes_style_search_drops_snippets_hidden_deleted_or_replaced_during_hydration() {
    for change in 0..3 {
        let mut daemon = DaemonState::new();
        let mut config = agent_config("owner");
        config.tools = Some(
            crate::tools::ToolRegistry::new()
                .resolve_descriptors(["search_conversations", "read_conversation_window"])
                .unwrap(),
        );
        let agent = daemon.create_agent(config).unwrap().state.id;
        daemon
            .sessions
            .insert(session(&agent, "chat:past", SessionKind::Chat, "Past"));
        seed_messages(
            &mut daemon,
            &agent,
            vec![message(
                &agent,
                "source",
                "chat:past",
                MessageRole::User,
                "Nebula PRIVATE_SEARCH_SENTINEL",
                10,
            )],
        );
        let store = Arc::new(ExactReadStore::new(None, false));
        daemon.set_history(HistoryService::new(store.clone()));
        let state = Arc::new(RwLock::new(daemon));
        let mut found = discover_conversations(&state, &agent, None, discovery())
            .await
            .unwrap();
        assert_eq!(found["results"][0]["matchMessageId"], "source");
        store
            .page_reads_before_gate
            .store(0, std::sync::atomic::Ordering::SeqCst);
        let task_state = state.clone();
        let task_agent = agent.clone();
        let task = tokio::spawn(async move {
            hydrate_top_result(&task_state, &task_agent, &mut found)
                .await
                .unwrap();
            finalize_discovery(&task_state, &task_agent, &mut found).await;
            found
        });
        tokio::time::timeout(Duration::from_secs(1), store.entered.acquire())
            .await
            .unwrap()
            .unwrap()
            .forget();
        {
            let mut guard = state.write().await;
            match change {
                0 => {
                    guard.sessions.remove(&agent, "chat:past");
                }
                1 => seed_messages(
                    &mut guard,
                    &agent,
                    vec![
                        crate::sessions::test_support::checkin_prompt(
                            &agent,
                            "source",
                            "chat:past",
                            "s1",
                            "Nebula PRIVATE_SEARCH_SENTINEL",
                            10,
                        ),
                        message(
                            &agent,
                            "silent",
                            "chat:past",
                            MessageRole::Assistant,
                            "CHECKIN_OK",
                            11,
                        ),
                    ],
                ),
                _ => {
                    let adapter = guard.profile_model_adapter();
                    let mut snapshot = guard.agents[&agent].snapshot();
                    snapshot
                        .messages
                        .iter_mut()
                        .find(|message| message.id == "source")
                        .unwrap()
                        .content
                        .text = "Unrelated replacement".into();
                    guard.agents.insert(
                        agent.clone(),
                        anima_core::AgentRuntime::from_snapshot(snapshot, adapter),
                    );
                }
            }
        }
        store.release.add_permits(1);
        let result = task.await.unwrap();
        assert_eq!(result["status"], "not_found", "change {change}");
        assert!(
            !result.to_string().contains("PRIVATE_SEARCH_SENTINEL"),
            "change {change}"
        );
    }
}

#[tokio::test]
async fn hermes_style_browse_caps_large_tool_previews() {
    let mut daemon = DaemonState::new();
    let agent = daemon.create_agent(agent_config("owner")).unwrap().state.id;
    daemon.sessions.insert(session(
        &agent,
        "chat:large",
        SessionKind::Chat,
        "Large tool output",
    ));
    let text = format!(
        "Visible prefix {} TAIL_SENTINEL",
        "tool output ".repeat(300_000)
    );
    seed_messages(
        &mut daemon,
        &agent,
        vec![message(
            &agent,
            "large",
            "chat:large",
            MessageRole::Tool,
            &text,
            10,
        )],
    );
    let state = Arc::new(RwLock::new(daemon));
    let result = discover_conversations(
        &state,
        &agent,
        None,
        DiscoveryRequest {
            query: String::new(),
            limit: 5,
            kind: None,
            after_ms: None,
            before_ms: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(result["results"][0]["snippetShortened"], true);
    assert!(result["results"][0]["snippet"]
        .as_str()
        .unwrap()
        .starts_with("Visible prefix"));
    assert!(!result.to_string().contains("TAIL_SENTINEL"));
    assert!(size(&result) <= DISCOVERY_BYTES);
}

#[tokio::test]
async fn hermes_style_final_refresh_bounds_expanding_metadata_and_retains_cursor() {
    let mut daemon = DaemonState::new();
    let agent = daemon.create_agent(agent_config("owner")).unwrap().state.id;
    let mut session_ids = Vec::new();
    for index in 0..10 {
        let prefix = format!("chat:{index}");
        let id = format!(
            "{prefix}{}",
            "s".repeat(crate::sessions::MAX_SESSION_ID_BYTES - prefix.len())
        );
        let mut record = session(&agent, &id, SessionKind::Chat, "x");
        assert_eq!(record.id, id, "the fixture uses a native session ID");
        record.last_activity_at_ms = 42;
        assert!(daemon.sessions.insert(record).is_none());
        seed_messages(
            &mut daemon,
            &agent,
            vec![message(
                &agent,
                &format!("message-{index}{}", "m".repeat(220)),
                &id,
                MessageRole::User,
                "x",
                10,
            )],
        );
        session_ids.push(id);
    }
    assert_eq!(
        daemon
            .sessions
            .records()
            .filter(|record| record.agent_id == agent)
            .count(),
        10
    );
    let state = Arc::new(RwLock::new(daemon));
    let mut result = browse_conversations(
        &state,
        &agent,
        None,
        DiscoveryRequest {
            query: String::new(),
            limit: 10,
            kind: None,
            after_ms: None,
            before_ms: None,
        },
        None,
    )
    .await
    .unwrap();
    assert!(size(&result) <= DISCOVERY_BYTES);
    {
        let mut guard = state.write().await;
        for id in &session_ids {
            guard.sessions.get_mut(&agent, id).unwrap().title = "\"\\".repeat(120);
        }
        let adapter = guard.profile_model_adapter();
        let mut snapshot = guard.agents[&agent].snapshot();
        for message in &mut snapshot.messages {
            message.content.text = format!("Expanded preview {}", "\"\\".repeat(200));
        }
        guard.agents.insert(
            agent.clone(),
            anima_core::AgentRuntime::from_snapshot(snapshot, adapter),
        );
    }
    finalize_discovery(&state, &agent, &mut result).await;
    assert!(size(&result) <= DISCOVERY_BYTES, "{} bytes", size(&result));
    assert_eq!(result["hasMore"], true);
    assert_eq!(result["shortened"], true);
    assert!(BrowseCursor::parse(result["nextCursor"].as_str().unwrap()).is_some());
    assert!(result["results"][0]["snippet"]
        .as_str()
        .unwrap()
        .starts_with("Expanded preview"));
}

#[tokio::test]
async fn incoming_agent_conversations_are_searchable_and_readable_with_sender_evidence() {
    for origin in [SessionOrigin::Delegation, SessionOrigin::Peer] {
        let mut daemon = DaemonState::new();
        let sender = daemon.create_agent(agent_config("Anima")).unwrap().state.id;
        let recipient = daemon.create_agent(agent_config("Mom")).unwrap().state.id;
        let store = Arc::new(MemoryHistoryStore::new());
        daemon.set_history(HistoryService::new(store.clone()));
        let mut contact = session(
            &recipient,
            "room:contact",
            SessionKind::Helper,
            "Ask about Julio",
        );
        contact.origin = origin;
        contact.parent_agent_id = Some(sender.clone());
        contact.parent_session_id = Some("chat:private-sender".into());
        contact.parent_run_id = Some("run:parent".into());
        contact.archived = true;
        daemon.sessions.insert(contact);
        daemon.sessions.insert(session(
            &sender,
            "chat:private-sender",
            SessionKind::Chat,
            "Private",
        ));
        store
            .upsert_messages(&[
                history_message(
                    &recipient,
                    "room:contact",
                    "request",
                    MessageRole::User,
                    "What do you know about Julio?",
                    10,
                ),
                history_message(
                    &sender,
                    "chat:private-sender",
                    "private",
                    MessageRole::User,
                    "Julio private sentinel",
                    11,
                ),
            ])
            .await
            .unwrap();
        seed_messages(
            &mut daemon,
            &recipient,
            vec![message(
                &recipient,
                "reply",
                "room:contact",
                MessageRole::Assistant,
                "Julio likes warm direct replies",
                12,
            )],
        );
        let state = Arc::new(RwLock::new(daemon));
        let found = discover_conversations(
            &state,
            &recipient,
            Some("chat:main"),
            DiscoveryRequest {
                query: "Julio".into(),
                limit: 5,
                kind: Some(SessionKind::Helper),
                after_ms: None,
                before_ms: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(found["status"], "found");
        assert_eq!(found["results"].as_array().unwrap().len(), 1);
        assert_eq!(found["results"][0]["contact"]["fromAgentId"], sender);
        assert_eq!(found["results"][0]["contact"]["fromAgentName"], "Anima");
        // A peer's body need not contain its sender's name. Read the cold
        // request by host-owned sender identity when its body lacks the name.
        let by_sender = discover_conversations(
            &state,
            &recipient,
            Some("chat:main"),
            DiscoveryRequest {
                query: "Anima".into(),
                limit: 5,
                kind: Some(SessionKind::Helper),
                after_ms: None,
                before_ms: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(by_sender["results"][0]["sessionId"], "room:contact");
        assert!(by_sender["results"][0]["matchMessageId"].is_string());
        let exchange =
            read_conversation_window(&state, &recipient, window("room:contact", "request", 0, 2))
                .await
                .unwrap();
        assert_eq!(ids(&exchange), ["request", "reply"]);
        assert_eq!(exchange["contact"]["fromAgentName"], "Anima");
        assert!(!exchange.to_string().contains("private sentinel"));
        assert_eq!(
            read_conversation_window(
                &state,
                &recipient,
                window("chat:private-sender", "private", 0, 0)
            )
            .await
            .unwrap()["status"],
            "not_found"
        );
        state.write().await.pending_history_deletions.push(
            crate::history::HistoryDeletion::session(&recipient, "room:contact"),
        );
        assert_eq!(
            read_conversation_window(&state, &recipient, window("room:contact", "request", 0, 2))
                .await
                .unwrap()["status"],
            "not_found"
        );
    }
}

#[tokio::test]
async fn sender_lookup_recovers_a_real_anchor_from_cold_peer_history_without_the_name_in_the_body()
{
    let mut daemon = DaemonState::new();
    let sender = daemon.create_agent(agent_config("Anima")).unwrap().state.id;
    let recipient = daemon.create_agent(agent_config("Mom")).unwrap().state.id;
    let store = Arc::new(MemoryHistoryStore::new());
    daemon.set_history(HistoryService::new(store.clone()));
    let mut record = session(
        &recipient,
        "peer:anima:mom",
        SessionKind::Helper,
        "Question",
    );
    record.origin = SessionOrigin::Peer;
    record.parent_agent_id = Some(sender);
    daemon.sessions.insert(record);
    store
        .upsert_messages(&[history_message(
            &recipient,
            "peer:anima:mom",
            "cold-request",
            MessageRole::User,
            "What do you know about Julio?",
            12,
        )])
        .await
        .unwrap();
    let state = Arc::new(RwLock::new(daemon));
    let found = discover_conversations(
        &state,
        &recipient,
        Some("chat:main"),
        DiscoveryRequest {
            query: "Anima".into(),
            limit: 5,
            kind: Some(SessionKind::Helper),
            after_ms: None,
            before_ms: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(found["results"][0]["matchMessageId"], "cold-request");
    let timed = |before_ms| DiscoveryRequest {
        query: "Anima".into(),
        limit: 5,
        kind: Some(SessionKind::Helper),
        after_ms: Some(10),
        before_ms: Some(before_ms),
    };
    assert_eq!(
        discover_conversations(&state, &recipient, Some("chat:main"), timed(13))
            .await
            .unwrap()["results"][0]["matchMessageId"],
        "cold-request"
    );
    assert_eq!(
        discover_conversations(&state, &recipient, Some("chat:main"), timed(12))
            .await
            .unwrap()["status"],
        "not_found"
    );
    let window = read_conversation_window(
        &state,
        &recipient,
        window("peer:anima:mom", "cold-request", 0, 0),
    )
    .await
    .unwrap();
    assert_eq!(
        window["messages"][0]["text"],
        "What do you know about Julio?"
    );
    assert_eq!(window["contact"]["fromAgentName"], "Anima");
}

#[tokio::test]
async fn main_chat_discovery_falls_back_to_latest_hot_match_without_cold_evidence() {
    let mut daemon = DaemonState::new();
    let agent = daemon.create_agent(agent_config("owner")).unwrap().state.id;
    let mut record = session(&agent, "chat:main", SessionKind::Chat, "Main");
    record.main_chat = true;
    daemon.sessions.insert(record);
    seed_messages(
        &mut daemon,
        &agent,
        vec![
            message(
                &agent,
                "older",
                "chat:main",
                MessageRole::User,
                "Nebula earlier question",
                15,
            ),
            message(
                &agent,
                "newer",
                "chat:main",
                MessageRole::Assistant,
                "Nebula latest answer",
                16,
            ),
        ],
    );
    let state = Arc::new(RwLock::new(daemon));
    let found = discover_conversations(&state, &agent, Some("chat:main"), discovery())
        .await
        .unwrap();
    assert_eq!(found["status"], "found");
    assert_eq!(found["results"][0]["matchMessageId"], "newer");
}

#[tokio::test]
async fn discovery_filters_before_grouping_and_preserves_real_cold_anchor() {
    let mut daemon = DaemonState::new();
    let agent = daemon.create_agent(agent_config("owner")).unwrap().state.id;
    let store = Arc::new(MemoryHistoryStore::new());
    daemon.set_history(HistoryService::new(store.clone()));
    let mut target = session(&agent, "chat:target", SessionKind::Chat, "Archived target");
    target.archived = true;
    daemon.sessions.insert(target);
    daemon.sessions.insert(session(
        &agent,
        "chat:title",
        SessionKind::Chat,
        "Nebula title only",
    ));
    let mut rows = vec![
        history_message(
            &agent,
            "chat:target",
            "anchor",
            MessageRole::User,
            "Nebula bridge is violet",
            15,
        ),
        history_message(
            &agent,
            "chat:target",
            "too-new",
            MessageRole::Assistant,
            "Nebula bridge is orange",
            25,
        ),
    ];
    for n in 0..12 {
        let id = format!("telegram:{n}");
        daemon
            .sessions
            .insert(session(&agent, &id, SessionKind::Telegram, "Distractor"));
        rows.push(history_message(
            &agent,
            &id,
            &format!("d{n}"),
            MessageRole::User,
            "Nebula bridge",
            16 + n,
        ));
    }
    store.upsert_messages(&rows).await.unwrap();
    let state = Arc::new(RwLock::new(daemon));
    let result = discover_conversations(&state, &agent, None, discovery())
        .await
        .unwrap();
    assert_eq!(result["results"].as_array().unwrap().len(), 1);
    assert_eq!(result["results"][0]["matchMessageId"], "anchor");
    assert_eq!(result["results"][0]["matchedAtMs"], 15);
    assert_eq!(result["results"][0]["archived"], true);
    assert!(size(&result) <= DISCOVERY_BYTES);
    let mut without_time = discovery();
    without_time.after_ms = None;
    without_time.before_ms = None;
    let result = discover_conversations(&state, &agent, Some("chat:target"), without_time)
        .await
        .unwrap();
    assert_eq!(result["results"][0]["matchKind"], "title");
    assert_eq!(result["results"][0]["matchMessageId"], Value::Null);
    assert_eq!(result["results"][0]["anchorUnavailable"], true);
    // Exclude a stale mirrored match before per-session grouping: the next
    // actual cold match must survive when the hot version no longer matches.
    store
        .upsert_messages(&[history_message(
            &agent,
            "chat:target",
            "older-match",
            MessageRole::User,
            "Nebula older bridge",
            14,
        )])
        .await
        .unwrap();
    seed_messages(
        &mut *state.write().await,
        &agent,
        vec![message(
            &agent,
            "anchor",
            "chat:target",
            MessageRole::User,
            "Changed unrelated text",
            15,
        )],
    );
    let result = discover_conversations(&state, &agent, None, discovery())
        .await
        .unwrap();
    assert_eq!(result["results"][0]["matchMessageId"], "older-match");
}

#[tokio::test]
async fn cold_pruned_anchor_merges_hot_priority_and_numeric_order() {
    let mut daemon = DaemonState::new();
    let agent = daemon.create_agent(agent_config("owner")).unwrap().state.id;
    let store = Arc::new(MemoryHistoryStore::new());
    daemon.set_history(HistoryService::new(store.clone()));
    let mut record = session(&agent, "chat:old", SessionKind::Chat, "Old");
    record.pruned_through = Some(super::super::SessionPrunedThrough {
        message_id: "msg-100-10".into(),
        created_at_ms: 100,
    });
    daemon.sessions.insert(record);
    store
        .upsert_messages(&[
            history_message(
                &agent,
                "chat:old",
                "msg-100-9",
                MessageRole::User,
                "nine",
                100,
            ),
            history_message(
                &agent,
                "chat:old",
                "msg-100-10",
                MessageRole::Assistant,
                "anchor ten",
                100,
            ),
            history_message(
                &agent,
                "chat:old",
                "msg-100-11",
                MessageRole::User,
                "cold eleven",
                100,
            ),
            history_message(
                &agent,
                "chat:old",
                "msg-100-100",
                MessageRole::Assistant,
                "hundred",
                100,
            ),
        ])
        .await
        .unwrap();
    seed_messages(
        &mut daemon,
        &agent,
        vec![message(
            &agent,
            "msg-100-11",
            "chat:old",
            MessageRole::User,
            "hot eleven",
            100,
        )],
    );
    let state = Arc::new(RwLock::new(daemon));
    let result = read_conversation_window(&state, &agent, window("chat:old", "msg-100-10", 1, 2))
        .await
        .unwrap();
    assert_eq!(
        ids(&result),
        ["msg-100-9", "msg-100-10", "msg-100-11", "msg-100-100"]
    );
    assert_eq!(result["messages"][2]["text"], "hot eleven");
    assert_eq!(result["status"], "found");
    let one = read_conversation_window(&state, &agent, window("chat:old", "msg-100-10", 0, 0))
        .await
        .unwrap();
    assert_eq!(ids(&one), ["msg-100-10"]);
    assert_eq!(one["hasEarlier"], true);
    assert_eq!(one["hasLater"], true);
}

#[tokio::test]
async fn exact_own_hidden_helper_and_pending_deletion_policy_is_generic() {
    let mut daemon = DaemonState::new();
    let agent = daemon.create_agent(agent_config("owner")).unwrap().state.id;
    let other = daemon.create_agent(agent_config("other")).unwrap().state.id;
    let store = Arc::new(MemoryHistoryStore::new());
    daemon.set_history(HistoryService::new(store.clone()));
    daemon
        .sessions
        .insert(session(&agent, "chat:own", SessionKind::Chat, "Own"));
    daemon
        .sessions
        .insert(session(&other, "chat:other", SessionKind::Chat, "Other"));
    let mut helper = session(&agent, "chat:helper", SessionKind::Helper, "Helper");
    helper.parent_run_id = Some("parent".into());
    daemon.sessions.insert(helper);
    let mut hidden = history_message(
        &agent,
        "chat:own",
        "hidden",
        MessageRole::User,
        "private hidden sentinel",
        12,
    );
    hidden.hidden = true;
    store
        .upsert_messages(&[
            hidden,
            history_message(
                &agent,
                "chat:own",
                "visible",
                MessageRole::User,
                "visible",
                13,
            ),
            history_message(
                &other,
                "chat:other",
                "foreign",
                MessageRole::User,
                "foreign sentinel",
                14,
            ),
            history_message(
                &agent,
                "chat:helper",
                "child",
                MessageRole::User,
                "helper sentinel",
                15,
            ),
        ])
        .await
        .unwrap();
    let state = Arc::new(RwLock::new(daemon));
    for (sid, id) in [
        ("chat:own", "hidden"),
        ("chat:other", "foreign"),
        ("chat:helper", "child"),
        ("chat:own", "missing"),
    ] {
        let result = read_conversation_window(&state, &agent, window(sid, id, 1, 1))
            .await
            .unwrap();
        assert_eq!(result["status"], "not_found");
        assert!(!result.to_string().contains("sentinel"));
    }
    assert_eq!(
        read_conversation_window(&state, &agent, window("chat:own", "visible", 0, 0))
            .await
            .unwrap()["status"],
        "found"
    );
    state
        .write()
        .await
        .pending_history_deletions
        .push(crate::history::HistoryDeletion::session(&agent, "chat:own"));
    assert_eq!(
        read_conversation_window(&state, &agent, window("chat:own", "visible", 0, 0))
            .await
            .unwrap()["status"],
        "not_found"
    );
}

#[tokio::test]
async fn cold_page_failure_is_unavailable_even_when_anchor_is_hot() {
    let mut daemon = DaemonState::new();
    let agent = daemon.create_agent(agent_config("owner")).unwrap().state.id;
    let store = Arc::new(FlakyHistoryStore::new());
    daemon.set_history(HistoryService::new(store.clone()));
    daemon
        .sessions
        .insert(session(&agent, "chat:own", SessionKind::Chat, "Own"));
    seed_messages(
        &mut daemon,
        &agent,
        vec![message(
            &agent,
            "hot",
            "chat:own",
            MessageRole::User,
            "hot",
            1,
        )],
    );
    store.set_page_messages_failing(true);
    let state = Arc::new(RwLock::new(daemon));
    assert_eq!(
        read_conversation_window(&state, &agent, window("chat:own", "hot", 0, 0))
            .await
            .unwrap_err(),
        HistoryReadError::Unavailable
    );
    store.set_page_messages_failing(false);
    store.set_failing(true);
    assert_eq!(
        read_conversation_window(&state, &agent, window("chat:own", "cold-missing", 0, 0))
            .await
            .unwrap_err(),
        HistoryReadError::Unavailable
    );
}

#[test]
fn serialized_window_bounds_preserve_anchor_pairing_and_only_safe_metadata() {
    let mut anchor = message(
        "owner",
        "anchor",
        "chat:own",
        MessageRole::Assistant,
        &"🫖\"\\\n".repeat(1000),
        10,
    );
    anchor.content.attachments = Some(vec![anima_core::Attachment {
        attachment_type: anima_core::AttachmentType::File,
        name: "synthetic.bin".into(),
        data: "ATTACHMENT_BYTES_SENTINEL".into(),
        mime_type: None,
    }]);
    anchor.content.metadata = Some(BTreeMap::from([
        (
            "toolCalls".into(),
            DataValue::Array(vec![DataValue::Object(BTreeMap::from([
                ("id".into(), DataValue::String("call1".into())),
                ("name".into(), DataValue::String("read_file".into())),
                ("args".into(), DataValue::String("RAW_ARGS_SENTINEL".into())),
            ]))]),
        ),
        ("incomplete".into(), DataValue::Bool(true)),
        ("budgetWrapup".into(), DataValue::Bool(true)),
        (
            "taskResult".into(),
            DataValue::String("RAW_TASK_RESULT_SENTINEL".into()),
        ),
    ]));
    let mut rows = vec![shape_message(&anchor)];
    for n in 0..19 {
        rows.push(shape_message(&message(
            "owner",
            &format!("m{n}"),
            "chat:own",
            MessageRole::User,
            &"\0\"🫖".repeat(1000),
            11 + n,
        )));
    }
    let mut output = json!({"guidance":GUIDANCE,"status":"found","sessionId":"chat:own","anchorMessageId":"anchor",
        "messages":rows,"hasEarlier":false,"hasLater":false,"shortened":false,"pairingIncomplete":false});
    bound_window(&mut output);
    assert!(size(&output) <= WINDOW_BYTES);
    assert_eq!(output["messages"].as_array().unwrap().len(), 20);
    assert!(ids(&output).iter().any(|id| id == "anchor"));
    assert_eq!(output["shortened"], true);
    assert_eq!(output["messages"][0]["incomplete"], true);
    assert_eq!(output["messages"][0]["budgetWrapup"], true);
    assert_eq!(output["messages"][0]["attachmentContentOmitted"], true);
    assert!(!output.to_string().contains("ATTACHMENT_BYTES_SENTINEL"));
    assert!(!output.to_string().contains("RAW_ARGS_SENTINEL"));
    assert!(!output.to_string().contains("RAW_TASK_RESULT_SENTINEL"));
    assert!(pairing_incomplete(output["messages"].as_array().unwrap()));
    let result = json!({"role":"tool","toolCallId":"call1"});
    assert!(!pairing_incomplete(&[shape_message(&anchor), result]));
    let mut error = message(
        "owner",
        "error",
        "chat:own",
        MessageRole::Tool,
        "raw wrapper must not escape",
        20,
    );
    error.content = Content {
        text: error.content.text,
        attachments: None,
        metadata: Some(BTreeMap::from([
            ("toolCallId".into(), DataValue::String("call1".into())),
            (
                "taskResult".into(),
                DataValue::Object(BTreeMap::from([
                    ("status".into(), DataValue::String("error".into())),
                    (
                        "error".into(),
                        DataValue::String("Actual recorded failure".into()),
                    ),
                    (
                        "data".into(),
                        DataValue::String("ERROR_PRIVATE_DATA_SENTINEL".into()),
                    ),
                ])),
            ),
        ])),
    };
    let shaped = shape_message(&error);
    assert_eq!(shaped["text"], "Actual recorded failure");
    assert_eq!(shaped["toolStatus"], "error");
    assert!(!shaped.to_string().contains("ERROR_PRIVATE_DATA_SENTINEL"));
}

#[tokio::test]
async fn scope_and_request_limits_are_explicit_before_history_reads() {
    let mut daemon = DaemonState::new();
    let agent = daemon.create_agent(agent_config("owner")).unwrap().state.id;
    for n in 0..501 {
        daemon.sessions.insert(session(
            &agent,
            &format!("chat:{n}"),
            SessionKind::Chat,
            "Own",
        ));
    }
    let state = Arc::new(RwLock::new(daemon));
    assert_eq!(
        discover_conversations(&state, &agent, None, discovery())
            .await
            .unwrap_err(),
        HistoryReadError::ScopeLimit
    );
    assert_eq!(
        read_conversation_window(&state, &agent, window("chat:0", "anchor", 19, 1))
            .await
            .unwrap_err(),
        HistoryReadError::InvalidRequest
    );
    let mut invalid = discovery();
    invalid.after_ms = Some(20);
    invalid.before_ms = Some(20);
    assert_eq!(
        discover_conversations(&state, &agent, None, invalid)
            .await
            .unwrap_err(),
        HistoryReadError::InvalidRequest
    );
}

#[tokio::test]
async fn silent_turn_detection_never_crosses_session_rooms() {
    let mut daemon = DaemonState::new();
    let agent = daemon.create_agent(agent_config("owner")).unwrap().state.id;
    daemon.sessions.insert(session(
        &agent,
        "chat:literal",
        SessionKind::Chat,
        "Ordinary",
    ));
    daemon.sessions.insert(session(
        &agent,
        "schedule:pending",
        SessionKind::Checkin,
        "Pending",
    ));
    let checkin = crate::sessions::test_support::checkin_prompt(
        &agent,
        "checkin",
        "schedule:pending",
        "pending",
        "Nebula owner request",
        10,
    );
    seed_messages(
        &mut daemon,
        &agent,
        vec![
            message(
                &agent,
                "u",
                "chat:literal",
                MessageRole::User,
                "Ordinary",
                9,
            ),
            checkin,
            message(
                &agent,
                "literal",
                "chat:literal",
                MessageRole::Assistant,
                "NO_REPLY",
                11,
            ),
        ],
    );
    let state = Arc::new(RwLock::new(daemon));
    let mut query = discovery();
    query.kind = None;
    let result = discover_conversations(&state, &agent, None, query)
        .await
        .unwrap();
    assert_eq!(result["results"][0]["matchMessageId"], "checkin");
    assert!(!result["results"][0]["snippet"]
        .as_str()
        .unwrap()
        .contains("scheduler"));
}

// The exact-read seam deliberately violates source identity or waits until the
// test releases it; all unrelated ports retain the real in-memory behavior.
struct ExactReadStore {
    inner: MemoryHistoryStore,
    reply: Option<HistoryMessage>,
    wait: bool,
    page_reads_before_gate: std::sync::atomic::AtomicUsize,
    entered: tokio::sync::Semaphore,
    release: tokio::sync::Semaphore,
    dropped: Arc<std::sync::atomic::AtomicBool>,
}
impl ExactReadStore {
    fn new(reply: Option<HistoryMessage>, wait: bool) -> Self {
        Self {
            inner: MemoryHistoryStore::new(),
            reply,
            wait,
            page_reads_before_gate: std::sync::atomic::AtomicUsize::new(usize::MAX),
            entered: tokio::sync::Semaphore::new(0),
            release: tokio::sync::Semaphore::new(0),
            dropped: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }
}
struct ReadExit(Arc<std::sync::atomic::AtomicBool>);
impl Drop for ReadExit {
    fn drop(&mut self) {
        self.0.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}
use crate::history as h;
#[async_trait::async_trait]
impl HistoryStore for ExactReadStore {
    fn label(&self) -> &'static str {
        "memory"
    }
    fn is_ephemeral(&self) -> bool {
        true
    }
    async fn upsert_attachments(
        &self,
        rows: &[crate::attachments::AttachmentRecord],
    ) -> Result<(), h::HistoryError> {
        self.inner.upsert_attachments(rows).await
    }
    async fn get_attachment(
        &self,
        agent: &str,
        id: &str,
    ) -> Result<Option<crate::attachments::AttachmentRecord>, h::HistoryError> {
        self.inner.get_attachment(agent, id).await
    }
    async fn upsert_messages(&self, rows: &[HistoryMessage]) -> Result<(), h::HistoryError> {
        self.inner.upsert_messages(rows).await
    }
    async fn upsert_runs(&self, rows: &[crate::runs::RunRecord]) -> Result<(), h::HistoryError> {
        self.inner.upsert_runs(rows).await
    }
    async fn upsert_approvals(
        &self,
        rows: &[crate::approvals::ApprovalRequest],
    ) -> Result<(), h::HistoryError> {
        self.inner.upsert_approvals(rows).await
    }
    async fn get_approval(
        &self,
        id: &str,
    ) -> Result<Option<crate::approvals::ApprovalRequest>, h::HistoryError> {
        self.inner.get_approval(id).await
    }
    async fn page_approvals(
        &self,
        q: &h::ApprovalPageQuery,
    ) -> Result<Vec<crate::approvals::ApprovalRequest>, h::HistoryError> {
        self.inner.page_approvals(q).await
    }
    async fn upsert_schedule_runs(
        &self,
        rows: &[crate::schedules::ScheduleFireRecord],
    ) -> Result<(), h::HistoryError> {
        self.inner.upsert_schedule_runs(rows).await
    }
    async fn page_schedule_runs(
        &self,
        a: &str,
        s: &str,
        l: usize,
    ) -> Result<Vec<crate::schedules::ScheduleFireRecord>, h::HistoryError> {
        self.inner.page_schedule_runs(a, s, l).await
    }
    async fn upsert_usage(
        &self,
        rows: &[crate::usage::UsageRecord],
    ) -> Result<(), h::HistoryError> {
        self.inner.upsert_usage(rows).await
    }
    async fn page_usage(
        &self,
        q: &h::UsagePageQuery,
    ) -> Result<Vec<crate::usage::UsageRecord>, h::HistoryError> {
        self.inner.page_usage(q).await
    }
    async fn existing_message_ids(
        &self,
        ids: &[String],
    ) -> Result<HashSet<String>, h::HistoryError> {
        self.inner.existing_message_ids(ids).await
    }
    async fn get_message(
        &self,
        _a: &str,
        _s: &str,
        _id: &str,
    ) -> Result<Option<HistoryMessage>, h::HistoryError> {
        let _exit = ReadExit(self.dropped.clone());
        if self.wait {
            self.entered.add_permits(1);
            self.release
                .acquire()
                .await
                .map_err(|_| h::HistoryError::new("test release closed"))?
                .forget();
        }
        Ok(self.reply.clone())
    }
    async fn get_run(&self, id: &str) -> Result<Option<crate::runs::RunRecord>, h::HistoryError> {
        self.inner.get_run(id).await
    }
    async fn page_messages(
        &self,
        q: &MessagePageQuery,
    ) -> Result<Vec<HistoryMessage>, h::HistoryError> {
        use std::sync::atomic::Ordering;
        let previous =
            self.page_reads_before_gate
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |count| {
                    (count != usize::MAX).then(|| if count == 0 { usize::MAX } else { count - 1 })
                });
        if previous == Ok(0) {
            self.entered.add_permits(1);
            self.release
                .acquire()
                .await
                .map_err(|_| h::HistoryError::new("page release closed"))?
                .forget();
        }
        self.inner.page_messages(q).await
    }
    async fn page_messages_after(
        &self,
        q: &MessageAfterQuery,
    ) -> Result<Vec<HistoryMessage>, h::HistoryError> {
        self.inner.page_messages_after(q).await
    }
    async fn visible_message_counts(
        &self,
        a: &str,
        ids: &[String],
    ) -> Result<HashMap<String, usize>, h::HistoryError> {
        self.inner.visible_message_counts(a, ids).await
    }
    async fn search_messages(
        &self,
        a: &[String],
        q: &str,
        l: usize,
    ) -> Result<Vec<HistoryMessage>, h::HistoryError> {
        self.inner.search_messages(a, q, l).await
    }
    async fn search_sessions(
        &self,
        a: &[String],
        q: &str,
        l: usize,
    ) -> Result<Vec<HistoryMessage>, h::HistoryError> {
        self.inner.search_sessions(a, q, l).await
    }
    async fn search_sessions_filtered(
        &self,
        q: &FilteredSessionSearchQuery,
    ) -> Result<Vec<HistoryMessage>, h::HistoryError> {
        self.inner.search_sessions_filtered(q).await
    }
    async fn delete_session(&self, a: &str, s: &str) -> Result<(), h::HistoryError> {
        self.inner.delete_session(a, s).await
    }
    async fn delete_agent(&self, a: &str) -> Result<(), h::HistoryError> {
        self.inner.delete_agent(a).await
    }
}

#[tokio::test]
async fn malformed_exact_source_is_unavailable_but_consistent_hidden_or_missing_is_not_found() {
    let mut daemon = DaemonState::new();
    let agent = daemon.create_agent(agent_config("owner")).unwrap().state.id;
    daemon
        .sessions
        .insert(session(&agent, "chat:own", SessionKind::Chat, "Own"));
    let state = Arc::new(RwLock::new(daemon));
    let original = history_message(
        &agent,
        "chat:own",
        "anchor",
        MessageRole::User,
        "FOREIGN_SOURCE_SENTINEL",
        10,
    );
    for mismatch in 0..6 {
        let mut row = original.clone();
        match mismatch {
            0 => row.agent_id = "foreign".into(),
            1 => row.session_id = "chat:foreign".into(),
            2 => row.message.agent_id = "foreign".into(),
            3 => row.message.room_id = "chat:foreign".into(),
            4 => row.message.id = "different".into(),
            _ => row.message.id = "bad\0id".into(),
        }
        state
            .write()
            .await
            .set_history(HistoryService::new(Arc::new(ExactReadStore::new(
                Some(row),
                false,
            ))));
        let error = read_conversation_window(&state, &agent, window("chat:own", "anchor", 0, 0))
            .await
            .unwrap_err();
        assert_eq!(error, HistoryReadError::Unavailable);
        assert!(!error.to_string().contains("FOREIGN_SOURCE_SENTINEL"));
    }
    let mut hidden = original.clone();
    hidden.hidden = true;
    let mut system = original;
    system.message.role = MessageRole::System;
    for row in [Some(hidden), Some(system), None] {
        state
            .write()
            .await
            .set_history(HistoryService::new(Arc::new(ExactReadStore::new(
                row, false,
            ))));
        let value = read_conversation_window(&state, &agent, window("chat:own", "anchor", 0, 0))
            .await
            .unwrap();
        assert_eq!(value["status"], "not_found");
        assert!(!value.to_string().contains("FOREIGN_SOURCE_SENTINEL"));
    }
}

#[tokio::test]
async fn never_returning_exact_source_hits_five_second_deadline_and_drops_read_future() {
    let mut daemon = DaemonState::new();
    let agent = daemon.create_agent(agent_config("owner")).unwrap().state.id;
    daemon
        .sessions
        .insert(session(&agent, "chat:own", SessionKind::Chat, "Own"));
    let store = Arc::new(ExactReadStore::new(None, true));
    daemon.set_history(HistoryService::new(store.clone()));
    let state = Arc::new(RwLock::new(daemon));
    let start = tokio::time::Instant::now();
    let error = tokio::time::timeout(
        DEADLINE + Duration::from_secs(2),
        read_conversation_window(&state, &agent, window("chat:own", "anchor", 0, 0)),
    )
    .await
    .expect("reader must return at its own five-second deadline")
    .unwrap_err();
    assert_eq!(error, HistoryReadError::Unavailable);
    assert!(start.elapsed() >= DEADLINE);
    assert_eq!(store.entered.available_permits(), 1);
    assert_eq!(store.release.available_permits(), 0);
    assert!(store.dropped.load(std::sync::atomic::Ordering::SeqCst));
}

#[tokio::test]
async fn deleting_or_queueing_deletion_during_exact_read_removes_source_authority() {
    for pending in [false, true] {
        let mut daemon = DaemonState::new();
        let agent = daemon.create_agent(agent_config("owner")).unwrap().state.id;
        daemon
            .sessions
            .insert(session(&agent, "chat:own", SessionKind::Chat, "Own"));
        let store = Arc::new(ExactReadStore::new(
            Some(history_message(
                &agent,
                "chat:own",
                "anchor",
                MessageRole::User,
                "DELETED_SOURCE_SENTINEL",
                10,
            )),
            true,
        ));
        daemon.set_history(HistoryService::new(store.clone()));
        let state = Arc::new(RwLock::new(daemon));
        let task_state = state.clone();
        let task_agent = agent.clone();
        let task = tokio::spawn(async move {
            read_conversation_window(&task_state, &task_agent, window("chat:own", "anchor", 0, 0))
                .await
        });
        tokio::time::timeout(Duration::from_secs(1), store.entered.acquire())
            .await
            .expect("exact read reached")
            .unwrap()
            .forget();
        {
            let mut guard = state.write().await;
            if pending {
                guard
                    .pending_history_deletions
                    .push(h::HistoryDeletion::session(&agent, "chat:own"));
            } else {
                assert!(guard.sessions.remove(&agent, "chat:own").is_some());
            }
        }
        store.release.add_permits(1);
        let value = tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("read released")
            .unwrap()
            .unwrap();
        assert_eq!(value["status"], "not_found");
        assert!(!value.to_string().contains("DELETED_SOURCE_SENTINEL"));
    }
}

#[tokio::test]
async fn individual_message_search_rechecks_deletion_and_new_hot_visibility_after_store_io() {
    for change in 0..4 {
        let mut daemon = DaemonState::new();
        let agent = daemon.create_agent(agent_config("owner")).unwrap().state.id;
        daemon
            .sessions
            .insert(session(&agent, "chat:own", SessionKind::Chat, "Own"));
        let store = Arc::new(FlakyHistoryStore::new());
        store
            .upsert_messages(&[history_message(
                &agent,
                "chat:own",
                "source",
                MessageRole::User,
                "nebula private source",
                10,
            )])
            .await
            .unwrap();
        let gate = store.hold_next_message_search();
        daemon.set_history(HistoryService::new(store));
        let state = Arc::new(RwLock::new(daemon));
        let task_state = state.clone();
        let task_agent = agent.clone();
        let task = tokio::spawn(async move {
            search_messages(
                &task_state,
                &task_agent,
                MessageSearchRequest {
                    query: "nebula".into(),
                    limit: 20,
                },
            )
            .await
        });
        tokio::time::timeout(Duration::from_secs(1), gate.entered.acquire())
            .await
            .expect("search reached store without holding daemon lock")
            .unwrap()
            .forget();
        {
            let mut guard = state.write().await;
            match change {
                0 => {
                    guard.sessions.remove(&agent, "chat:own");
                }
                1 => guard
                    .pending_history_deletions
                    .push(h::HistoryDeletion::session(&agent, "chat:own")),
                2 => seed_messages(
                    &mut guard,
                    &agent,
                    vec![message(
                        &agent,
                        "source",
                        "chat:own",
                        MessageRole::User,
                        "changed subject",
                        10,
                    )],
                ),
                _ => seed_messages(
                    &mut guard,
                    &agent,
                    vec![
                        crate::sessions::test_support::checkin_prompt(
                            &agent,
                            "source",
                            "chat:own",
                            "s1",
                            "nebula private source",
                            10,
                        ),
                        message(
                            &agent,
                            "silent",
                            "chat:own",
                            MessageRole::Assistant,
                            "CHECKIN_OK",
                            11,
                        ),
                    ],
                ),
            }
        }
        gate.release.add_permits(1);
        let page = tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("search released")
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(
            page.results.is_empty(),
            "post-read authority change {change}"
        );
    }
}

#[tokio::test]
async fn custom_store_without_individual_filtered_search_is_explicitly_unavailable() {
    let store = ExactReadStore::new(None, false);
    let query = FilteredMessageSearchQuery {
        agent_id: "owner".into(),
        session_ids: vec!["chat:own".into()],
        excluded_message_ids: Vec::new(),
        query: "nebula".into(),
        after_ms: None,
        before_ms: None,
        limit: 21,
    };
    assert!(store
        .search_messages_filtered(&query)
        .await
        .unwrap_err()
        .message()
        .contains("unsupported"));
}
