//! Host-owned selection of descriptors advertised to the next model request.
//! Selection never authorizes execution or modifies canonical tool descriptors.
use std::collections::BTreeSet;

use crate::{AgentState, ToolDescriptor};

#[async_trait::async_trait]
pub trait ModelToolSelector: Send + Sync {
    fn visible_tool_names(&self) -> BTreeSet<String>;

    /// Select request schemas at an asynchronous model-call boundary. Hosts
    /// may wait for authority reads here instead of hiding tools on contention.
    async fn visible_tool_names_for_request(&self) -> BTreeSet<String> {
        self.visible_tool_names()
    }
}

/// A trusted host may replace a run's tool ceiling at a model-call boundary.
/// This is separate from advertisement selection: the host must revalidate
/// execution authority before publishing a replacement. Never persisted.
#[async_trait::async_trait]
pub trait RunToolRefresher: Send + Sync {
    async fn refresh_tools(
        &self,
        agent: &AgentState,
    ) -> Result<Option<Vec<ToolDescriptor>>, String>;
}
