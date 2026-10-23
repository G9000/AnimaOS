//! Owner-requested MCP snapshot replacement between model calls in one run.
use std::collections::BTreeSet;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, RwLock,
};

use anima_core::{
    AgentConfig, AgentState, CancelSignal, ModelToolSelector, RunToolRefresher, ToolDescriptor,
};

use crate::{
    app::SharedDaemonState,
    mcp::{McpManager, McpToolAccess},
    runs::RunLink,
};

use super::catalog::RunToolCatalog;

pub(crate) struct McpToolRefresh {
    state: SharedDaemonState,
    run: RunLink,
    cancel: CancelSignal,
    original: AgentConfig,
    manager: McpManager,
    pending: AtomicBool,
    catalog: RwLock<Option<Arc<RunToolCatalog>>>,
}

impl McpToolRefresh {
    pub(crate) fn new(
        state: SharedDaemonState,
        run: RunLink,
        cancel: CancelSignal,
        original: AgentConfig,
        manager: McpManager,
        catalog: Option<Arc<RunToolCatalog>>,
    ) -> Self {
        Self {
            state,
            run,
            cancel,
            original,
            manager,
            pending: AtomicBool::new(false),
            catalog: RwLock::new(catalog),
        }
    }

    pub(super) fn request(&self) {
        self.pending.store(true, Ordering::Release);
    }

    pub(super) fn catalog(&self) -> Option<Arc<RunToolCatalog>> {
        self.catalog.read().expect("run catalog lock").clone()
    }
}

#[async_trait::async_trait]
impl RunToolRefresher for McpToolRefresh {
    async fn refresh_tools(
        &self,
        agent: &AgentState,
    ) -> Result<Option<Vec<ToolDescriptor>>, String> {
        if !self.pending.swap(false, Ordering::AcqRel) {
            return Ok(None);
        }
        let state = self.state.read().await;
        crate::agent_runs::chat_controls::validate_owner_chat(&state, &self.run)?;
        if agent.id != self.run.agent_id || self.cancel.is_cancelled() {
            return Err("MCP refresh requires the original live owner run".into());
        }
        let canonical = state.agents[&agent.id].config();
        let access = McpToolAccess::for_agent(&state, &agent.id);
        // Preserve the original run's native ceiling. Only MCP membership is
        // re-derived, including exact newly granted descriptors in review mode.
        let mut tools: Vec<_> = agent
            .config
            .tools
            .iter()
            .flatten()
            .filter(|tool| !tool.name.starts_with("mcp_"))
            .cloned()
            .collect();
        for tool in self.manager.ready_descriptors(&agent.id) {
            if access == McpToolAccess::Automatic
                || (access == McpToolAccess::Review
                    && canonical
                        .tools
                        .iter()
                        .flatten()
                        .any(|current| current == &tool))
            {
                tools.push(tool);
            }
        }
        let mut refreshed = agent.clone();
        refreshed.config.tools = Some(tools.clone());
        if let Some(previous) = self.catalog() {
            let catalog = Arc::new(RunToolCatalog::new(
                &self.state,
                self.run.clone(),
                self.cancel.clone(),
                &self.original,
                &refreshed,
                Some(self.manager.clone()),
            ));
            catalog.inherit_hydration(&previous);
            *self.catalog.write().expect("run catalog lock") = Some(catalog);
        }
        state
            .live
            .runs()
            .refresh_mcp_tool_scope(&self.run.run_id, &tools);
        Ok(Some(tools))
    }
}

impl ModelToolSelector for McpToolRefresh {
    fn visible_tool_names(&self) -> BTreeSet<String> {
        self.catalog()
            .map(|catalog| catalog.visible_tool_names())
            .unwrap_or_default()
    }
}
