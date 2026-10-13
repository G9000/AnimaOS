//! Conversation search for an agent's own runs (spec §7.1).

use super::AgentRunCoordinator;
use crate::sessions::views::{self, SessionView};

impl AgentRunCoordinator {
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
