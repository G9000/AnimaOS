//! Owned list inputs captured under the state lock; all text derivation is outside it.
use super::views::{automation_exists, preview_from_newest, SessionMatch};
use super::{hidden_message_ids, is_inbound_message, SessionCapabilities, SessionRecord};
use crate::agent_runs::config_helper_parent;
use crate::history::{search_snippet, searchable_text, snippet_text, text_matches, HistoryStore};
use crate::state::DaemonState;
use anima_core::{Message, MessageRole};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub(super) struct SessionListSnapshot {
    seeds: Vec<SessionSeed>,
    pub(super) store: Arc<dyn HistoryStore>,
}
struct SessionSeed {
    record: SessionRecord,
    messages: Vec<Message>,
    mirrored: HashSet<String>,
    active_runs: usize,
    pending_approvals: usize,
    automation_exists: bool,
}
pub(super) struct Candidate {
    pub(super) record: SessionRecord,
    /// Visible hot messages the store is not known to hold.
    pub(super) unmirrored_visible: usize,
    /// Every visible hot message (the count when the store cannot be read).
    pub(super) hot_visible: usize,
    pub(super) preview: Option<String>,
    pub(super) unread: bool,
    pub(super) active_runs: usize,
    pub(super) pending_approvals: usize,
    pub(super) capabilities: SessionCapabilities,
    pub(super) matched: Option<SessionMatch>,
}

pub(super) fn capture_list_snapshot(
    state: &DaemonState,
    agent_id: &str,
    include_helpers: bool,
    select: impl Fn(&SessionRecord) -> bool,
) -> Option<SessionListSnapshot> {
    if !state.agents.contains_key(agent_id) {
        return None;
    }
    let helpers = if include_helpers {
        state
            .agents
            .iter()
            .filter(|(_, runtime)| config_helper_parent(runtime.config()) == Some(agent_id))
            .map(|(id, _)| id.as_str())
            .collect::<HashSet<_>>()
    } else {
        HashSet::new()
    };
    let records = state
        .sessions
        .records()
        .filter(|record| {
            state.agents.contains_key(&record.agent_id)
                && (record.agent_id == agent_id
                    || (include_helpers
                        && (record.parent_agent_id.as_deref() == Some(agent_id)
                            || helpers.contains(record.agent_id.as_str()))))
                && select(record)
        })
        .collect::<Vec<_>>();
    let selected_rooms = records
        .iter()
        .map(|r| (r.agent_id.as_str(), r.room_id()))
        .collect::<HashSet<_>>();
    let mut rooms: HashMap<(&str, &str), Vec<Message>> = HashMap::new();
    for agent in records
        .iter()
        .map(|r| r.agent_id.as_str())
        .collect::<HashSet<_>>()
    {
        for message in state.agents[agent].messages() {
            if selected_rooms.contains(&(agent, message.room_id.as_str())) {
                // List derivation needs text and only these two metadata tags.
                // Never clone attachment payloads, tool arguments, or base64.
                let metadata = message.content.metadata.as_ref().map(|values| {
                    values
                        .iter()
                        .filter(|(key, _)| matches!(key.as_str(), "kind" | "source"))
                        .map(|(key, value)| (key.clone(), value.clone()))
                        .collect()
                });
                rooms
                    .entry((agent, message.room_id.as_str()))
                    .or_default()
                    .push(Message {
                        id: message.id.clone(),
                        agent_id: message.agent_id.clone(),
                        room_id: message.room_id.clone(),
                        content: anima_core::Content {
                            text: message.content.text.clone(),
                            metadata,
                            attachments: None,
                        },
                        role: message.role,
                        created_at_ms: message.created_at_ms,
                    });
            }
        }
    }
    let seeds = records
        .into_iter()
        .map(|record| {
            let messages = rooms
                .remove(&(record.agent_id.as_str(), record.room_id()))
                .unwrap_or_default();
            let mirrored = messages
                .iter()
                .filter(|message| state.history.is_mirrored(&message.id))
                .map(|message| message.id.clone())
                .collect();
            SessionSeed {
                record: record.clone(),
                messages,
                mirrored,
                active_runs: state
                    .runs
                    .active_count_for_session(&record.agent_id, &record.id),
                pending_approvals: state
                    .approvals
                    .pending_count_for_session(&record.agent_id, &record.id),
                automation_exists: automation_exists(state, record),
            }
        })
        .collect();
    Some(SessionListSnapshot {
        seeds,
        store: state.history.store(),
    })
}

pub(super) fn derive_candidates(
    snapshot: SessionListSnapshot,
    tokens: &[String],
) -> Vec<Candidate> {
    snapshot
        .seeds
        .into_iter()
        .map(|seed| derive_candidate(seed, tokens))
        .collect()
}
fn derive_candidate(seed: SessionSeed, tokens: &[String]) -> Candidate {
    let record = &seed.record;
    let hot = seed.messages.iter().collect::<Vec<_>>();
    let hidden_ids = hidden_message_ids(hot.iter().copied());
    let visible = hot
        .iter()
        .copied()
        .filter(|message| !hidden_ids.contains(&message.id))
        .collect::<Vec<_>>();
    let read_through = record.last_read_at_ms.unwrap_or(0);
    let schedule_exists = seed.automation_exists;
    let matched = if tokens.is_empty() {
        None
    } else {
        visible
            .iter()
            .rev()
            .find(|message| text_matches(searchable_text(message), tokens))
            .map(|message| SessionMatch {
                message_id: Some(message.id.clone()),
                snippet: search_snippet(snippet_text(message), tokens),
            })
    };
    Candidate {
        record: record.clone(),
        unmirrored_visible: visible
            .iter()
            .filter(|message| !seed.mirrored.contains(&message.id))
            .count(),
        hot_visible: visible.len(),
        preview: preview_from_newest(visible.iter().rev().copied()),
        unread: visible.iter().any(|message| {
            (message.role == MessageRole::Assistant || is_inbound_message(message))
                && message.created_at_ms > read_through
        }),
        active_runs: seed.active_runs,
        pending_approvals: seed.pending_approvals,
        capabilities: record.capabilities(schedule_exists),
        matched,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::test_support::{agent_config, message, seed_messages, within};
    use crate::sessions::{SessionKind, SessionOrigin, TitleSource};
    use tokio::sync::{oneshot, RwLock};

    #[tokio::test]
    async fn captured_list_derives_after_unlock_and_keeps_one_generation() {
        let mut daemon = DaemonState::new();
        let agent = daemon.create_agent(agent_config("owner")).unwrap().state.id;
        let foreign = daemon
            .create_agent(agent_config("foreign"))
            .unwrap()
            .state
            .id;
        daemon.sessions.insert(SessionRecord::new(
            &agent,
            "chat:one",
            SessionKind::Chat,
            SessionOrigin::Web,
            "Original".into(),
            TitleSource::System,
            1,
        ));
        daemon.sessions.insert(SessionRecord::new(
            &foreign,
            "chat:other",
            SessionKind::Chat,
            SessionOrigin::Web,
            "Foreign".into(),
            TitleSource::System,
            1,
        ));
        seed_messages(
            &mut daemon,
            &agent,
            vec![
                message(
                    &agent,
                    "one",
                    "chat:one",
                    MessageRole::Assistant,
                    "original reply",
                    2,
                ),
                message(
                    &agent,
                    "other-room",
                    "chat:unselected",
                    MessageRole::Assistant,
                    "not selected",
                    2,
                ),
            ],
        );
        seed_messages(
            &mut daemon,
            &foreign,
            vec![message(
                &foreign,
                "foreign",
                "chat:other",
                MessageRole::Assistant,
                "foreign reply",
                2,
            )],
        );
        let state = Arc::new(RwLock::new(daemon));
        let snapshot = {
            let guard = state.read().await;
            assert!(capture_list_snapshot(&guard, "missing", true, |_| true).is_none());
            capture_list_snapshot(&guard, &agent, false, |_| true).unwrap()
        };
        assert_eq!(snapshot.seeds.len(), 1);
        assert_eq!(snapshot.seeds[0].messages.len(), 1);
        assert_eq!(snapshot.seeds[0].messages[0].id, "one");
        let (release, gate) = oneshot::channel();
        let deriving = tokio::spawn(async move {
            gate.await.unwrap();
            derive_candidates(snapshot, &["original".into()])
        });
        {
            let mut guard = within("write while derivation gated", state.write()).await;
            guard.sessions.remove(&agent, "chat:one");
        }
        release.send(()).unwrap();
        let candidates = within("derive captured page", deriving).await.unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].record.title, "Original");
        assert_eq!(candidates[0].preview.as_deref(), Some("original reply"));
        assert!(candidates[0].matched.is_some());
        let guard = state.read().await;
        assert!(capture_list_snapshot(&guard, &agent, false, |_| true)
            .unwrap()
            .seeds
            .is_empty());
    }
    #[tokio::test]
    async fn list_history_calls_run_after_state_guard_release() {
        use super::lock_checking_store::LockCheckingStore;
        use crate::history::{HistoryService, MemoryHistoryStore};
        use crate::sessions::views::{list_sessions, SessionListQuery};
        let mut daemon = DaemonState::new();
        let agent = daemon.create_agent(agent_config("owner")).unwrap().state.id;
        daemon.sessions.insert(SessionRecord::new(
            &agent,
            "chat:one",
            SessionKind::Chat,
            SessionOrigin::Web,
            "Original".into(),
            TitleSource::System,
            1,
        ));
        let state = Arc::new(RwLock::new(daemon));
        let store = Arc::new(LockCheckingStore {
            state: Arc::downgrade(&state),
            inner: MemoryHistoryStore::new(),
            calls: std::sync::atomic::AtomicUsize::new(0),
        });
        state
            .write()
            .await
            .set_history(HistoryService::new(store.clone()));
        let page = list_sessions(&state, &agent, &SessionListQuery::default())
            .await
            .unwrap();
        assert_eq!(page.sessions.len(), 1);
        assert!(store.calls.load(std::sync::atomic::Ordering::SeqCst) > 0);
    }
}

#[cfg(test)]
mod lock_checking_store;
