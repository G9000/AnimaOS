//! Host-admitted transient context, with authority checked before model publication.
use crate::AgentState;

/// One request's identity and attributed reference material; never part of snapshots.
#[derive(Clone, Debug, Default)]
pub struct PromptContext {
    /// Replaces only the request's personality; absence preserves the complete local source.
    pub identity: Option<String>,
    /// Attributed context/status, separate from identity and harness instructions.
    pub reference: String,
    /// Exact tool aliases withheld on this request; dispatch remains host-authorized separately.
    pub withheld_tools: std::collections::BTreeSet<String>,
}

/// The host owns admission and revalidates current authority for every request.
#[async_trait::async_trait]
pub trait PromptContextSource: Send + Sync {
    /// Resolve current admission just before model publication, including offline status.
    async fn current(&self, agent: &AgentState) -> PromptContext;
}
