//! Agent/run-bound, read-only garden adapter. Publication is always an explicit tool action.
use super::binding::{self, MemoryProviderBinding};
use crate::{app::SharedDaemonState, mcp::McpManager, runs::RunLink};
use anima_core::{AgentState, CancelSignal, ToolCall};
use anima_memory::{
    ExternalMemoryError, ExternalMemoryProvider, ProviderStatus, RecallRequest, RecalledItem,
};
use serde_json::{json, Value};
use std::{collections::BTreeSet, time::Duration};

#[path = "selection.rs"]
mod selection;
use selection::RecallPlan;

const MAX_RESULT_BYTES: usize = 16 * 1024;
const MAX_RECALL_BYTES: usize = 8 * 1024;
const MAX_RECORDS: usize = 4;
/// Preview windows may be larger than the full-record fetch budget.
const MAX_CANDIDATES: usize = 8;

#[derive(Debug)]
enum ReadFailure {
    Unavailable,
    Partial,
}

struct Candidate {
    id: String,
    rank: selection::Rank,
}

struct ResolvedRecord {
    id: String,
    revision: f64,
    record: Value,
    owner_order: usize,
    rank: selection::Rank,
}

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
        self.read_complete(remote, arguments)
            .await
            .map_err(|_| unavailable())
    }

    async fn read_complete(&self, remote: &str, arguments: Value) -> Result<Value, ReadFailure> {
        // This bootstrap privilege is deliberately unavailable to mutation tools.
        if !matches!(
            remote,
            "mother_get_context" | "mother_search_context" | "mother_get_record"
        ) || !self.current().await
        {
            return Err(ReadFailure::Unavailable);
        }
        let tool = self
            .manager
            .workspace_context_tool(&self.agent.id, &self.binding.connection_id, remote)
            .await
            .ok_or(ReadFailure::Unavailable)?;
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
            .map_err(|_| ReadFailure::Unavailable)?;
        stamp.workspace_context_read = true;
        self.manager
            .revalidate_workspace_context(&stamp)
            .await
            .map_err(|_| ReadFailure::Unavailable)?;
        let content = self
            .manager
            .invoke(stamp.clone(), arguments, CancelSignal::new())
            .await
            .map_err(|_| ReadFailure::Unavailable)?;
        if !self.current().await {
            return Err(ReadFailure::Unavailable);
        }
        self.manager
            .revalidate_workspace_context(&stamp)
            .await
            .map_err(|_| ReadFailure::Unavailable)?;
        complete_envelope(&content.text)
    }

    async fn search(&self, query: &str, mode: &str) -> Result<Value, ExternalMemoryError> {
        let arguments = |limit| json!({"query":query,"limit":limit,"mode":mode});
        match self
            .read_complete("mother_search_context", arguments(MAX_CANDIDATES))
            .await
        {
            Ok(value) => Ok(value),
            // Discard the partial result entirely. A smaller read-only window
            // gets fresh binding/grant checks under the same recall deadline.
            Err(ReadFailure::Partial) => {
                self.read("mother_search_context", arguments(MAX_CANDIDATES / 2))
                    .await
            }
            Err(ReadFailure::Unavailable) => Err(unavailable()),
        }
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
        let owner_ids = ids.clone();
        let plan = RecallPlan::new(&request.query);
        let mut candidates: Vec<Candidate> = Vec::new();
        for query in &plan.literal_queries {
            let search = self.search(query, "literal").await?;
            self.authority(&search)?;
            for value in search["records"]
                .as_array()
                .ok_or_else(unavailable)?
                .iter()
                .take(MAX_CANDIDATES)
            {
                if let Some(id) = value["id"].as_str() {
                    let rank = plan.rank(value, true);
                    if let Some(existing) =
                        candidates.iter_mut().find(|candidate| candidate.id == id)
                    {
                        existing.rank = existing.rank.max(rank);
                    } else {
                        candidates.push(Candidate {
                            id: id.to_owned(),
                            rank,
                        });
                    }
                }
            }
        }
        if candidates.is_empty() && !plan.hybrid_query.is_empty() {
            let search = self.search(&plan.hybrid_query, "hybrid").await?;
            self.authority(&search)?;
            for value in search["records"]
                .as_array()
                .ok_or_else(unavailable)?
                .iter()
                .take(MAX_CANDIDATES)
            {
                if let Some(id) = value["id"].as_str() {
                    candidates.push(Candidate {
                        id: id.to_owned(),
                        rank: plan.rank(value, false),
                    });
                }
            }
        }
        candidates.sort_by(|left, right| right.rank.cmp(&left.rank));
        ids.extend(candidates.into_iter().map(|candidate| candidate.id));
        let mut seen_ids = BTreeSet::new();
        let mut resolved = Vec::new();
        for id in ids
            .into_iter()
            .filter(|id| seen_ids.insert(id.clone()))
            .take(if request.limit == 0 { 0 } else { MAX_RECORDS })
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
            record["content"].as_str().ok_or_else(unavailable)?;
            let owner_order = owner_ids
                .iter()
                .position(|owner_id| owner_id == &id)
                .unwrap_or(usize::MAX);
            if owner_order == usize::MAX && !plan.matches_current(&record) {
                continue;
            }
            resolved.push(ResolvedRecord {
                id,
                revision,
                rank: plan.rank(&record, false),
                record,
                owner_order,
            });
        }
        // Explicit owner context stays first; discovered evidence is ranked again
        // using current complete content, not a possibly stale/truncated preview.
        resolved.sort_by(|left, right| {
            left.owner_order
                .cmp(&right.owner_order)
                .then_with(|| right.rank.cmp(&left.rank))
        });
        let mut seen_text = BTreeSet::new();
        let mut items = Vec::new();
        let mut bytes = 0;
        for ResolvedRecord {
            id,
            revision,
            record,
            ..
        } in resolved
        {
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
            if items.len() == request.limit.min(MAX_RECORDS) {
                break;
            }
        }
        // A restore or grant change during record fetch discards the whole result. No cached read is reused.
        self.authority(&self.read("mother_get_context", json!({"limit":1})).await?)?;
        Ok(items)
    }
}

fn unavailable() -> ExternalMemoryError {
    ExternalMemoryError::new("Shared garden recall unavailable")
}

#[cfg(test)]
pub(crate) fn envelope(text: &str) -> Result<Value, ExternalMemoryError> {
    complete_envelope(text).map_err(|_| unavailable())
}

fn complete_envelope(text: &str) -> Result<Value, ReadFailure> {
    if text.len() > MAX_RESULT_BYTES {
        return Err(ReadFailure::Unavailable);
    }
    let value: Value = serde_json::from_str(text).map_err(|_| ReadFailure::Unavailable)?;
    let shortened = value["shortened"]
        .as_bool()
        .ok_or(ReadFailure::Unavailable)?;
    let omitted = value["omittedBlocks"]
        .as_f64()
        .filter(|v| v.is_finite() && *v >= 0.0 && v.fract() == 0.0)
        .ok_or(ReadFailure::Unavailable)?;
    if value["isError"] != false {
        return Err(ReadFailure::Unavailable);
    }
    if shortened || omitted > 0.0 {
        return Err(ReadFailure::Partial);
    }
    if value["structuredData"].is_object() {
        return Ok(value["structuredData"].clone());
    }
    let blocks = value["textBlocks"]
        .as_array()
        .filter(|v| v.len() == 1)
        .ok_or(ReadFailure::Unavailable)?;
    serde_json::from_str(blocks[0].as_str().ok_or(ReadFailure::Unavailable)?)
        .map_err(|_| ReadFailure::Unavailable)
}

#[cfg(test)]
fn bounded_query(text: &str) -> String {
    selection::bounded_query(text)
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
    fn memory_provider_mother_only_classifies_valid_partial_envelopes_as_retryable() {
        let base = json!({"isError":false,"shortened":true,"omittedBlocks":1});
        assert!(matches!(
            complete_envelope(&base.to_string()),
            Err(ReadFailure::Partial)
        ));
        for (field, invalid) in [
            ("isError", json!(true)),
            ("isError", Value::Null),
            ("shortened", Value::Null),
            ("omittedBlocks", json!(-1)),
            ("omittedBlocks", json!(0.5)),
            ("omittedBlocks", Value::Null),
        ] {
            let mut value = base.clone();
            value[field] = invalid;
            assert!(matches!(
                complete_envelope(&value.to_string()),
                Err(ReadFailure::Unavailable)
            ));
        }
    }

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
