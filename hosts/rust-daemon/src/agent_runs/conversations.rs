//! Conversation search for an agent's own runs (spec §7.1).

use super::AgentRunCoordinator;
use crate::sessions::views::{self, SessionView};

impl AgentRunCoordinator {
    pub(crate) async fn browse_conversations(
        &self,
        agent_id: &str,
        current: Option<&str>,
        request: crate::sessions::history_read::DiscoveryRequest,
        cursor: Option<crate::sessions::history_read::BrowseCursor>,
    ) -> Result<serde_json::Value, crate::sessions::history_read::HistoryReadError> {
        crate::sessions::history_read::browse_conversations(
            &self.state,
            agent_id,
            current,
            request,
            cursor,
        )
        .await
    }

    pub(crate) async fn hydrate_conversation_result(
        &self,
        agent_id: &str,
        result: &mut serde_json::Value,
    ) -> Result<(), crate::sessions::history_read::HistoryReadError> {
        crate::sessions::history_read::hydrate_top_result(&self.state, agent_id, result).await
    }

    pub(crate) async fn finalize_conversation_result(
        &self,
        agent_id: &str,
        result: &mut serde_json::Value,
        can_read: impl FnOnce(&crate::state::DaemonState) -> bool,
    ) {
        let guard = self.state.read().await;
        let reading = can_read(&guard);
        crate::sessions::history_read::finalize_discovery_current(
            &guard, agent_id, result, reading,
        );
    }

    pub(crate) async fn read_latest_conversation(
        &self,
        agent_id: &str,
        session_id: &str,
        before_count: usize,
    ) -> Result<serde_json::Value, crate::sessions::history_read::HistoryReadError> {
        crate::sessions::history_read::read_latest_conversation(
            &self.state,
            agent_id,
            session_id,
            before_count,
        )
        .await
    }

    pub(crate) async fn discover_conversations(
        &self,
        agent_id: &str,
        current: Option<&str>,
        request: crate::sessions::history_read::DiscoveryRequest,
    ) -> Result<serde_json::Value, crate::sessions::history_read::HistoryReadError> {
        crate::sessions::history_read::discover_conversations(
            &self.state,
            agent_id,
            current,
            request,
        )
        .await
    }

    pub(crate) async fn read_conversation_window(
        &self,
        agent_id: &str,
        request: crate::sessions::history_read::WindowRequest,
    ) -> Result<serde_json::Value, crate::sessions::history_read::HistoryReadError> {
        crate::sessions::history_read::read_conversation_window(&self.state, agent_id, request)
            .await
    }

    pub(crate) async fn search_conversations(
        &self,
        agent_id: &str,
        exclude_session: Option<&str>,
        query: &str,
        limit: usize,
    ) -> Option<Vec<SessionView>> {
        views::search_conversations(&self.state, agent_id, exclude_session, query, limit).await
    }
}
