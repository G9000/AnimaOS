//! Appending a transcript without a run: an imported conversation enters
//! its agent the way a run commit does (`commit_run` merges a run delta),
//! with no run record, usage, events, or status change.

use anima_core::{Message, RuntimeRunDelta, RuntimeRunUndo, TokenUsage};

use super::DaemonState;
use crate::sessions::{ImportedFrom, SessionPrunedThrough, SessionRecord};

/// What `commit_imported_session` changed, so a failed save can put it back.
#[derive(Debug)]
pub(crate) struct SessionImportUndo {
    agent_id: String,
    session_id: String,
    delta: RuntimeRunDelta,
    undo: RuntimeRunUndo,
}

impl DaemonState {
    /// The session an earlier import of the same foreign conversation
    /// created for this agent, if it still exists.
    pub(crate) fn find_imported_session(
        &self,
        agent_id: &str,
        provenance: &ImportedFrom,
    ) -> Option<&SessionRecord> {
        self.sessions.records().find(|record| {
            record.agent_id == agent_id
                && record
                    .imported_from
                    .as_ref()
                    .is_some_and(|imported| imported.same_conversation(provenance))
        })
    }

    /// Records a new session and appends `hot` -- its newest messages, in
    /// transcript order -- to the agent's canonical transcript. `cold`
    /// names the newest message already written straight to the history
    /// store, which the record keeps as `prunedThrough`, exactly as if
    /// hot-tail pruning had removed the older turns. The caller saves the
    /// control plane, then queues `hot` with
    /// `HistoryService::enqueue_committed` inside the same transaction, as
    /// a run commit does. `None` when the agent is gone.
    pub(crate) fn commit_imported_session(
        &mut self,
        mut record: SessionRecord,
        hot: Vec<Message>,
        newest_cold: Option<&Message>,
    ) -> Option<SessionImportUndo> {
        let runtime = self.agents.get_mut(&record.agent_id)?;
        let delta = RuntimeRunDelta {
            messages: hot,
            events: Vec::new(),
            event_total: 0,
            token_usage: TokenUsage::default(),
            step_count: 0,
            last_task: None,
            status: runtime.state().status,
        };
        let undo = runtime.apply_run_delta(&delta);
        record.pruned_through = newest_cold.map(SessionPrunedThrough::of);
        let (agent_id, session_id) = (record.agent_id.clone(), record.id.clone());
        self.sessions.insert(record);
        Some(SessionImportUndo {
            agent_id,
            session_id,
            delta,
            undo,
        })
    }

    /// Removes exactly what `commit_imported_session` added.
    pub(crate) fn revert_imported_session(&mut self, undo: SessionImportUndo) {
        if let Some(runtime) = self.agents.get_mut(&undo.agent_id) {
            runtime.revert_run_delta(&undo.delta, undo.undo);
        }
        self.sessions.remove(&undo.agent_id, &undo.session_id);
    }
}
