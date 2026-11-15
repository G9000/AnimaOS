//! Agent/run-bound, read-only garden adapter. Publication is always an explicit tool action.
use super::binding::{self, MemoryProviderBinding};
use crate::{app::SharedDaemonState, mcp::McpManager, runs::RunLink};
use anima_core::{AgentState, CancelSignal, ToolCall};
use anima_memory::{
    ExternalMemoryError, ExternalMemoryProvider, ProviderStatus, RecallRequest, RecalledItem,
};
use serde_json::{json, Value};
use std::{collections::BTreeSet, time::Duration};

const MAX_RESULT_BYTES: usize = 16 * 1024;
const MAX_RECALL_BYTES: usize = 8 * 1024;
const MAX_RECORDS: usize = 4;

pub(crate) struct MotherProvider {
    state: SharedDaemonState,
    manager: McpManager,
    binding: MemoryProviderBinding,
    agent: AgentState,
    run: RunLink,
    cancel: CancelSignal,
}

impl MotherProvider {
    pub(crate) fn new(
        state: SharedDaemonState,
        manager: McpManager,
        binding: MemoryProviderBinding,
        agent: AgentState,
        run: RunLink,
        cancel: CancelSignal,
    ) -> Self {
        Self {
            state,
            manager,
            binding,
            agent,
            run,
            cancel,
        }
    }

    async fn current(&self) -> bool {
        !self.cancel.is_cancelled()
            && self
                .state
                .read()
                .await
                .agents
                .get(&self.agent.id)
                .is_some_and(|agent| {
                    binding::binding(agent.config()).ok().flatten().as_ref() == Some(&self.binding)
                })
    }

    async fn read(&self, remote: &str, arguments: Value) -> Result<Value, ExternalMemoryError> {
        // This bootstrap privilege is deliberately unavailable to mutation tools.
        if !matches!(
            remote,
            "mother_get_context" | "mother_search_context" | "mother_get_record"
        ) || !self.current().await
        {
            return Err(unavailable());
        }
        let tool = self
            .manager
            .workspace_context_tool(&self.agent.id, &self.binding.connection_id, remote)
            .await
            .ok_or_else(unavailable)?;
        let call = ToolCall {
            id: format!("garden-read-{}", uuid::Uuid::new_v4()),
            name: tool.name.clone(),
            args: Default::default(),
        };
        let mut agent = self.agent.clone();
        if self.manager.tool_access(&agent.id) == crate::mcp::McpToolAccess::Automatic {
            agent.config.tools.get_or_insert_with(Vec::new).push(tool);
        }
        let mut stamp = self
            .manager
            .capture_dispatch(&agent, &call, &self.run)
            .map_err(|_| unavailable())?;
        stamp.workspace_context_read = true;
        self.manager
            .revalidate_workspace_context(&stamp)
            .await
            .map_err(|_| unavailable())?;
        let content = self
            .manager
            .invoke(stamp.clone(), arguments, CancelSignal::new())
            .await
            .map_err(|_| unavailable())?;
        if !self.current().await {
            return Err(unavailable());
        }
        self.manager
            .revalidate_workspace_context(&stamp)
            .await
            .map_err(|_| unavailable())?;
        envelope(&content.text)
    }

    fn authority(&self, value: &Value) -> Result<(), ExternalMemoryError> {
        let mode = match self.binding.access_mode {
            crate::workspace_context::ContextMode::Companion => "companion",
            crate::workspace_context::ContextMode::Shared => "shared",
        };
        if value["access"]["version"].as_f64() == Some(1.0)
            && value["access"]["mode"] == mode
            && value["access"]["subject"] == self.agent.id
            && value["workspace"]["id"] == self.binding.workspace_id
            && value["workspace"]["generation"] == self.binding.generation
        {
            Ok(())
        } else {
            Err(unavailable())
        }
    }

    async fn recall_inner(
        &self,
        request: &RecallRequest,
    ) -> Result<Vec<RecalledItem>, ExternalMemoryError> {
        if request.agent_id != self.agent.id || !self.current().await {
            return Err(unavailable());
        }
        let context = self.read("mother_get_context", json!({"limit":1})).await?;
        self.authority(&context)?;
        let mut ids = Vec::new();
        for collection in ["pinned", "assignedToYou"] {
            for value in context[collection].as_array().into_iter().flatten() {
                if let Some(id) = value["id"].as_str() {
                    ids.push(id.to_owned());
                }
            }
        }
        let query = bounded_query(&request.query);
        if !query.is_empty() {
            let search = self
                .read(
                    "mother_search_context",
                    json!({"query":query,"limit":MAX_RECORDS,"mode":"hybrid"}),
                )
                .await?;
            self.authority(&search)?;
            for value in search["records"].as_array().ok_or_else(unavailable)? {
                if let Some(id) = value["id"].as_str() {
                    ids.push(id.to_owned());
                }
            }
        }
        let mut seen_ids = BTreeSet::new();
        let mut seen_text = BTreeSet::new();
        let mut items = Vec::new();
        let mut bytes = 0;
        for id in ids
            .into_iter()
            .filter(|id| seen_ids.insert(id.clone()))
            .take(request.limit.min(MAX_RECORDS))
        {
            if uuid::Uuid::parse_str(&id).is_err() {
                return Err(unavailable());
            }
            let record = self.read("mother_get_record", json!({"id":id})).await?;
            let revision = record["revision"]
                .as_f64()
                .filter(|v| v.is_finite() && *v >= 1.0 && v.fract() == 0.0)
                .ok_or_else(unavailable)?;
            if record["id"] != id || record["updatedAt"].as_str().is_none() {
                return Err(unavailable());
            }
            if matches!(
                record["status"].as_str(),
                Some("retracted" | "superseded" | "cancelled")
            ) {
                continue;
            }
            let content = record["content"].as_str().ok_or_else(unavailable)?;
            let normalized = content
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase();
            if !seen_text.insert(normalized) {
                continue;
            }
            // Preserve complete records, provenance and contradictory assertions; never cut a record mid-content.
            let text = json!({"provider":"mother","workspaceId":self.binding.workspace_id,"generation":self.binding.generation,"record":record}).to_string();
            if bytes + text.len() > MAX_RECALL_BYTES
                || !crate::threat_scan::blocking_ids(&text).is_empty()
            {
                continue;
            }
            bytes += text.len();
            items.push(RecalledItem {
                text,
                score: 1.0,
                source_id: Some(format!(
                    "mother:{}/{}/{}@{}",
                    self.binding.workspace_id, self.binding.generation, id, revision
                )),
            });
        }
        // A restore or grant change during record fetch discards the whole result. No cached read is reused.
        self.authority(&self.read("mother_get_context", json!({"limit":1})).await?)?;
        Ok(items)
    }
}

fn unavailable() -> ExternalMemoryError {
    ExternalMemoryError::new("Shared garden recall unavailable")
}

pub(crate) fn envelope(text: &str) -> Result<Value, ExternalMemoryError> {
    if text.len() > MAX_RESULT_BYTES {
        return Err(unavailable());
    }
    let value: Value = serde_json::from_str(text).map_err(|_| unavailable())?;
    if value["isError"] != false || value["shortened"] != false || value["omittedBlocks"] != 0 {
        return Err(unavailable());
    }
    if value["structuredData"].is_object() {
        return Ok(value["structuredData"].clone());
    }
    let blocks = value["textBlocks"]
        .as_array()
        .filter(|v| v.len() == 1)
        .ok_or_else(unavailable)?;
    serde_json::from_str(blocks[0].as_str().ok_or_else(unavailable)?).map_err(|_| unavailable())
}

fn bounded_query(text: &str) -> String {
    let mut units = 0;
    text.trim()
        .chars()
        .take_while(|c| {
            units += c.len_utf16();
            units <= 200
        })
        .collect()
}

#[async_trait::async_trait]
impl ExternalMemoryProvider for MotherProvider {
    fn id(&self) -> &str {
        "mother"
    }
    async fn recall(
        &self,
        request: RecallRequest,
    ) -> Result<Vec<RecalledItem>, ExternalMemoryError> {
        tokio::select! {
            result=tokio::time::timeout(request.deadline.min(Duration::from_secs(3)),self.recall_inner(&request)) => result.map_err(|_|unavailable())?,
            _=self.cancel.cancelled()=>Err(unavailable()),
        }
    }
    // The trait's default sync_turn/on_memory_write are intentionally no-ops.
    async fn status(&self) -> ProviderStatus {
        if self.current().await {
            ProviderStatus::Ready
        } else {
            ProviderStatus::Unavailable("Shared garden unavailable".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn memory_provider_mother_rejects_partial_envelopes_and_bounds_utf16_query() {
        assert_eq!(bounded_query(&"🪴".repeat(110)).encode_utf16().count(), 200);
        assert_eq!(bounded_query("  facts  "), "facts");
        for field in ["shortened", "isError"] {
            let mut value = json!({"isError":false,"shortened":false,"omittedBlocks":0,"structuredData":{"id":"record"}});
            value[field] = json!(true);
            assert!(envelope(&value.to_string()).is_err());
        }
        assert!(envelope(&"x".repeat(MAX_RESULT_BYTES + 1)).is_err());
    }
}
