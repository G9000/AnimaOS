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
pub(crate) use selection::RecallPlan;
#[path = "graph.rs"]
mod graph;

const MAX_RESULT_BYTES: usize = 16 * 1024;
const MAX_RECALL_BYTES: usize = 8 * 1024;
const MAX_RECORDS: usize = 4;
/// Preview windows may be larger than the full-record fetch budget.
const MAX_CANDIDATES: usize = 8;

#[derive(Debug)]
enum ReadFailure {
    Unavailable,
    Partial,
    Authority,
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

struct SelectedRecord {
    text: String,
    source_id: String,
    dedup_key: String,
    owner_order: usize,
    rank: selection::Rank,
}

pub(crate) struct SharedRecall {
    pub(crate) items: Vec<RecalledItem>,
    pub(crate) coverage: Value,
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
            "mother_get_context"
                | "mother_search_context"
                | "mother_get_record"
                | "mother_get_capabilities"
                | "mother_garden_query"
                | "mother_garden_get"
        ) {
            return Err(ReadFailure::Unavailable);
        }
        if !self.current().await {
            return Err(ReadFailure::Authority);
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
            return Err(ReadFailure::Authority);
        }
        self.manager
            .revalidate_workspace_context(&stamp)
            .await
            .map_err(|_| ReadFailure::Unavailable)?;
        complete_envelope(&content.text)
    }

    async fn search(
        &self,
        query: &str,
        mode: &str,
        kind: Option<&str>,
    ) -> Result<Value, ExternalMemoryError> {
        let arguments = |limit| {
            let mut value = json!({"query":query,"limit":limit,"mode":mode});
            if let Some(kind) = kind {
                value["kind"] = json!(kind);
            }
            value
        };
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
            Err(ReadFailure::Unavailable | ReadFailure::Authority) => Err(unavailable()),
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
        deadline: tokio::time::Instant,
        plan: &RecallPlan,
        owner_context: bool,
        include_graph: bool,
    ) -> Result<SharedRecall, ExternalMemoryError> {
        if request.agent_id != self.agent.id || !self.current().await {
            return Err(unavailable());
        }
        let context = self.read("mother_get_context", json!({"limit":1})).await?;
        self.authority(&context)?;
        let mut ids = Vec::new();
        for collection in ["pinned", "assignedToYou"]
            .into_iter()
            .filter(|_| owner_context)
        {
            for value in context[collection].as_array().into_iter().flatten() {
                if let Some(id) = value["id"].as_str() {
                    ids.push(id.to_owned());
                }
            }
        }
        let owner_ids = ids.clone();
        let mut candidates: Vec<Candidate> = Vec::new();
        let mut searches = Vec::new();
        if !plan.inventory_only() && request.limit > 0 {
            let search = self
                .search(&plan.query, &plan.mode, plan.kind.as_deref())
                .await?;
            self.authority(&search)?;
            searches.push(json!({"requestedMode":plan.mode,"kind":plan.kind,"search":search["search"],
                "selection":search["selection"],"omitted":search["omitted"],"responseBudget":search["responseBudget"]}));
            for value in search["records"]
                .as_array()
                .ok_or_else(unavailable)?
                .iter()
                .take(MAX_CANDIDATES)
            {
                if let Some(id) = value["id"].as_str() {
                    let rank = plan.rank(value, plan.mode == "literal");
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
        candidates.sort_by(|left, right| right.rank.cmp(&left.rank));
        ids.extend(candidates.into_iter().map(|candidate| candidate.id));
        let candidate_count = ids.iter().collect::<BTreeSet<_>>().len();
        let mut seen_ids = BTreeSet::new();
        let mut resolved = Vec::new();
        let mut fetched_count = 0;
        let mut rejected_count = 0;
        for id in ids
            .into_iter()
            .filter(|id| seen_ids.insert(id.clone()))
            .take(if request.limit == 0 { 0 } else { MAX_RECORDS })
        {
            if uuid::Uuid::parse_str(&id).is_err() {
                return Err(unavailable());
            }
            let record = self.read("mother_get_record", json!({"id":id})).await?;
            fetched_count += 1;
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
                rejected_count += 1;
                continue;
            }
            record["content"].as_str().ok_or_else(unavailable)?;
            let owner_order = owner_ids
                .iter()
                .position(|owner_id| owner_id == &id)
                .unwrap_or(usize::MAX);
            if owner_order == usize::MAX
                && (!plan.matches_current(&record)
                    || plan
                        .kind
                        .as_deref()
                        .is_some_and(|kind| record["kind"] != kind))
            {
                rejected_count += 1;
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
        let mut selected = Vec::new();
        for ResolvedRecord {
            id,
            revision,
            record,
            owner_order,
            rank,
        } in resolved
        {
            let content = record["content"].as_str().ok_or_else(unavailable)?;
            let normalized = content
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase();
            let text = json!({"provider":"mother","workspaceId":self.binding.workspace_id,"generation":self.binding.generation,"record":record}).to_string();
            selected.push(SelectedRecord {
                text,
                source_id: format!(
                    "mother:{}/{}/{}@{}",
                    self.binding.workspace_id, self.binding.generation, id, revision
                ),
                dedup_key: format!("v1:{normalized}"),
                owner_order,
                rank,
            });
        }
        // Optional graph latency must not consume the final v1 authority check.
        let reserve = Duration::from_millis(500);
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let mut graph_coverage = json!({"status":"not_requested","mode":"literal"});
        if include_graph && request.limit > 0 && !plan.inventory_only() {
            graph_coverage["status"] = json!("deadline_excluded");
        }
        if include_graph && request.limit > 0 && remaining > reserve && !plan.inventory_only() {
            let budget = (remaining - reserve).min(Duration::from_millis(750));
            match tokio::time::timeout(budget, graph::collect(self, plan, &mut graph_coverage))
                .await
            {
                Ok(Ok(records)) => selected.extend(records),
                Ok(Err(ReadFailure::Authority)) => return Err(unavailable()),
                Ok(Err(ReadFailure::Unavailable | ReadFailure::Partial)) => {
                    graph_coverage["status"] = json!("unavailable");
                }
                Err(_) => {
                    graph_coverage["status"] = json!("deadline_excluded");
                }
            }
        }
        selected.sort_by(|left, right| {
            left.owner_order
                .cmp(&right.owner_order)
                .then_with(|| right.rank.cmp(&left.rank))
        });
        let mut seen_text = BTreeSet::new();
        let mut items = Vec::new();
        let mut bytes = 0;
        let selected_count = selected.len();
        for record in selected {
            if items.len() >= request.limit.min(MAX_RECORDS) {
                break;
            }
            // Keep complete records and attribution; never cut a record mid-content.
            if !seen_text.insert(record.dedup_key)
                || bytes + record.text.len() > MAX_RECALL_BYTES
                || !crate::threat_scan::blocking_ids(&record.text).is_empty()
            {
                continue;
            }
            bytes += record.text.len();
            items.push(RecalledItem {
                text: record.text,
                score: 1.0,
                source_id: Some(record.source_id),
            });
        }
        // A restore or grant change during record fetch discards the whole result. No cached read is reused.
        self.authority(&self.read("mother_get_context", json!({"limit":1})).await?)?;
        Ok(SharedRecall {
            coverage: json!({"status":if items.is_empty() {"no_selected_records"} else {"selected_records"},
            "exhaustive":false,"searches":searches,"graph":graph_coverage,"candidateRecords":candidate_count,
            "fetchedRecords":fetched_count,"unresolvedCandidates":candidate_count.saturating_sub(fetched_count),
            "excludedResolvedRecords":rejected_count+selected_count.saturating_sub(items.len()),"returnedRecords":items.len(),
            "maxRecords":request.limit.min(MAX_RECORDS),"maxRecallBytes":MAX_RECALL_BYTES}),
            items,
        })
    }

    pub(crate) async fn recall_search(
        &self,
        plan: RecallPlan,
        limit: usize,
        include_graph: bool,
    ) -> Result<SharedRecall, ExternalMemoryError> {
        let request = RecallRequest {
            agent_id: self.agent.id.clone(),
            query: plan.query.clone(),
            limit,
            deadline: Duration::from_secs(3),
        };
        let deadline = tokio::time::Instant::now() + request.deadline;
        tokio::select! {
            result=tokio::time::timeout_at(deadline,self.recall_inner(&request,deadline,&plan,false,include_graph)) => result.map_err(|_|unavailable())?,
            _=self.cancel.cancelled()=>Err(unavailable()),
        }
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
        let text_error: Option<Value> = value["textBlocks"]
            .as_array()
            .filter(|blocks| blocks.len() == 1)
            .and_then(|blocks| blocks[0].as_str())
            .and_then(|text| serde_json::from_str(text).ok());
        let code = value["structuredData"]["error"]["code"]
            .as_str()
            .or_else(|| {
                text_error
                    .as_ref()
                    .and_then(|error| error["error"]["code"].as_str())
            });
        if matches!(
            code,
            Some("WORKSPACE_MISMATCH" | "GENERATION_MISMATCH" | "RESTART_REQUIRED")
        ) {
            return Err(ReadFailure::Authority);
        }
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
        let deadline = tokio::time::Instant::now() + request.deadline.min(Duration::from_secs(3));
        let plan = RecallPlan::new(&request.query);
        tokio::select! {
            result=tokio::time::timeout_at(deadline,self.recall_inner(&request,deadline,&plan,true,true)) => result.map_err(|_|unavailable())?.map(|r|r.items),
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
