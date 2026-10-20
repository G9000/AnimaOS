//! Messages a run's commit hook adds to another session of the same agent,
//! such as an automation's card in the main chat. A side post enters the
//! transcript the way an import does (`apply_run_delta`, no run record), and
//! rides the run's own commit: saved with it, queued to the history store
//! inside its transaction, and removed again when the commit does not stand.

use anima_core::{Message, RuntimeRunDelta, RuntimeRunUndo, TokenUsage};

use super::DaemonState;
use crate::sessions::{SessionCommitUndo, SessionRecord};

/// One message staged by a commit hook, with what undoes it.
#[derive(Debug)]
pub(crate) struct SidePost {
    pub(crate) agent_id: String,
    pub(crate) session_id: String,
    pub(crate) message: Message,
    delta: RuntimeRunDelta,
    undo: RuntimeRunUndo,
    session_undo: Option<SessionCommitUndo>,
}

impl DaemonState {
    /// The agent's designated main chat: exactly one live web chat marked
    /// `mainChat`. `None` until the console has resolved it, or when the
    /// designation is ambiguous.
    pub(crate) fn main_chat_session(&self, agent_id: &str) -> Option<&SessionRecord> {
        let mut designated = self
            .sessions
            .records()
            .filter(|record| record.agent_id == agent_id && record.main_chat);
        let record = designated.next()?;
        if designated.next().is_some() || record.archived || !record.has_main_chat_identity() {
            return None;
        }
        Some(record)
    }

    /// Appends `message` to the agent's transcript in `session_id`'s room
    /// (the message names the room) and stages it for the running commit.
    /// `false`, changing nothing, when the agent or session is gone.
    pub(crate) fn stage_side_post(
        &mut self,
        agent_id: &str,
        session_id: &str,
        message: Message,
    ) -> bool {
        if self.sessions.get(agent_id, session_id).is_none() {
            return false;
        }
        let Some(runtime) = self.agents.get_mut(agent_id) else {
            return false;
        };
        let delta = RuntimeRunDelta {
            messages: vec![message.clone()],
            events: Vec::new(),
            event_total: 0,
            token_usage: TokenUsage::default(),
            step_count: 0,
            last_task: None,
            status: runtime.state().status,
        };
        let undo = runtime.apply_run_delta(&delta);
        let session_undo = self.sessions.record_commit(
            agent_id,
            session_id,
            std::slice::from_ref(&message),
            false,
        );
        self.side_posts.push(SidePost {
            agent_id: agent_id.to_string(),
            session_id: session_id.to_string(),
            message,
            delta,
            undo,
            session_undo,
        });
        true
    }

    /// The posts staged since the last take, oldest first.
    pub(crate) fn take_side_posts(&mut self) -> Vec<SidePost> {
        std::mem::take(&mut self.side_posts)
    }

    /// Removes exactly what the given posts added, newest first, so each
    /// transcript undo applies to the state it recorded.
    pub(crate) fn revert_side_posts(&mut self, posts: Vec<SidePost>) {
        for post in posts.into_iter().rev() {
            if let Some(session_undo) = post.session_undo {
                self.sessions.revert_commit(session_undo);
            }
            if let Some(runtime) = self.agents.get_mut(&post.agent_id) {
                runtime.revert_run_delta(&post.delta, post.undo);
            }
        }
    }
}
