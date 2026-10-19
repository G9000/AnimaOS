//! The owner's view and edits of an agent's core memory: the "About you"
//! (`user`) and "Agent notes" (`notes`) blocks. Every route requires the local
//! owner and answers `Cache-Control: no-store`.
//!
//! Edits go through `core_memory::apply_edit`, the agent tool's path, so the
//! budgets, the entry cap and the uniqueness rules hold. Scanner hits do not
//! refuse an owner edit: the text is saved and the ids come back as
//! `warnings`. Each edit runs in its own task (`locked`) under the memory write
//! lock, then syncs embeddings and the file copy before the lock is released.

use anima_memory::{Memory, MemoryManager};
use axum::extract::{Path, Request, State};
use axum::http::StatusCode;
use axum::response::Response;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::contracts::ErrorBody;
use super::http::{json_response, read_limited_body};
use super::jobs::{authorize, no_store};
use super::memories::{persist_memory_store, remove_memory_embeddings};
use super::memory_edits::{locked, memory_handles, sync_embedding};
use super::sessions::rejected;
use super::{parse_json_body, ApiError, AppState};
use crate::app::SharedDaemonState;
use crate::core_memory::{
    self, files, CoreAgent, CoreBlock, CoreChange, CoreEdit, CoreOutcome, ScanMode,
};
use crate::memory_store::MemoryMutation;
use crate::threat_scan::blocking_ids;

/// Request bodies are one short entry; this leaves room for JSON escapes.
const MAX_BODY_BYTES: usize = 16 * 1024;

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct CoreMemoryEntry {
    pub(crate) id: String,
    pub(crate) content: String,
    /// Scanner ids the content trips; the agent's context shows a flagged
    /// entry only as these ids.
    pub(crate) flagged: Vec<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct CoreMemoryBlock {
    /// Characters (not bytes) the entries use together.
    pub(crate) used: usize,
    pub(crate) limit: usize,
    /// In creation order.
    pub(crate) entries: Vec<CoreMemoryEntry>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct CoreMemoryView {
    pub(crate) user: CoreMemoryBlock,
    pub(crate) notes: CoreMemoryBlock,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CoreMemoryEditResponse {
    pub(crate) user: CoreMemoryBlock,
    pub(crate) notes: CoreMemoryBlock,
    /// The entry added, changed or removed (for an exact duplicate add, the
    /// entry already there).
    pub(crate) entry_id: String,
    /// False for an exact duplicate add, which changes nothing.
    pub(crate) changed: bool,
    /// Scanner ids on the new text; it was saved anyway.
    pub(crate) warnings: Vec<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct CoreMemoryText {
    pub(crate) text: String,
}

fn block_view(block: CoreBlock, entries: &[Memory]) -> CoreMemoryBlock {
    CoreMemoryBlock {
        used: entries
            .iter()
            .map(|memory| memory.content.chars().count())
            .sum(),
        limit: block.budget(),
        entries: entries
            .iter()
            .map(|memory| CoreMemoryEntry {
                id: memory.id.clone(),
                content: memory.content.clone(),
                flagged: blocking_ids(&memory.content),
            })
            .collect(),
    }
}

fn view(manager: &MemoryManager, agent_id: &str) -> CoreMemoryView {
    let (user, notes) = core_memory::blocks_for(manager, agent_id);
    CoreMemoryView {
        user: block_view(CoreBlock::User, &user),
        notes: block_view(CoreBlock::Notes, &notes),
    }
}

fn answer<T: Serialize>(result: Result<T, ApiError>) -> Response {
    match result {
        Ok(body) => no_store(json_response(StatusCode::OK, &body)),
        Err(error) => rejected(error),
    }
}

fn parse_block(block: &str) -> Result<CoreBlock, ApiError> {
    CoreBlock::parse(block).ok_or_else(ApiError::not_found)
}

async fn read_text(state: &AppState, request: Request) -> Result<String, Response> {
    let bytes = read_limited_body(request, state.config.max_request_bytes.min(MAX_BODY_BYTES))
        .await
        .map_err(no_store)?;
    parse_json_body::<CoreMemoryText>(bytes)
        .map(|input| input.text)
        .map_err(rejected)
}

#[utoipa::path(get, path = "/api/agents/{agent_id}/core-memory", tag = "memories",
    params(("agent_id" = String, Path)),
    responses(
        (status = 200, description = "Both core memory blocks with usage, entries in creation order", body = CoreMemoryView),
        (status = 403, description = "Local owner required", body = ErrorBody),
        (status = 404, description = "No such agent", body = ErrorBody)
    ))]
pub(super) async fn get_core_memory(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, true) {
        return response;
    }
    let memory = {
        let guard = state.daemon.read().await;
        if !guard.agents.contains_key(&agent_id) {
            return rejected(ApiError::not_found());
        }
        guard.memory_handle()
    };
    let manager = memory.read().await;
    answer(Ok(view(&manager, &agent_id)))
}

#[utoipa::path(post, path = "/api/agents/{agent_id}/core-memory/{block}", tag = "memories",
    params(("agent_id" = String, Path), ("block" = String, Path, description = "user or notes")),
    request_body = CoreMemoryText,
    responses(
        (status = 200, description = "Entry added (or already present); both blocks now", body = CoreMemoryEditResponse),
        (status = 400, description = "Empty, hidden or too long text, or the block is full", body = ErrorBody),
        (status = 403, description = "Local owner required", body = ErrorBody),
        (status = 404, description = "No such agent or block", body = ErrorBody),
        (status = 503, description = "The change could not be saved and was undone, or did not finish", body = ErrorBody)
    ))]
pub(super) async fn add_entry(
    State(state): State<AppState>,
    Path((agent_id, block)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let block = match parse_block(&block) {
        Ok(block) => block,
        Err(error) => return rejected(error),
    };
    let text = match read_text(&state, request).await {
        Ok(text) => text,
        Err(response) => return response,
    };
    let daemon = state.daemon.clone();
    answer(locked(edit(daemon, agent_id, block, None, CoreEdit::Add { text })).await)
}

#[utoipa::path(patch, path = "/api/agents/{agent_id}/core-memory/{block}/{entry_id}", tag = "memories",
    params(("agent_id" = String, Path), ("block" = String, Path, description = "user or notes"), ("entry_id" = String, Path)),
    request_body = CoreMemoryText,
    responses(
        (status = 200, description = "Entry text replaced in place; both blocks now", body = CoreMemoryEditResponse),
        (status = 400, description = "Empty, hidden or too long text, or the block would be over its limit", body = ErrorBody),
        (status = 403, description = "Local owner required", body = ErrorBody),
        (status = 404, description = "No such agent, block or entry", body = ErrorBody),
        (status = 503, description = "The change could not be saved and was undone, or did not finish", body = ErrorBody)
    ))]
pub(super) async fn replace_entry(
    State(state): State<AppState>,
    Path((agent_id, block, entry_id)): Path<(String, String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let block = match parse_block(&block) {
        Ok(block) => block,
        Err(error) => return rejected(error),
    };
    let text = match read_text(&state, request).await {
        Ok(text) => text,
        Err(response) => return response,
    };
    let daemon = state.daemon.clone();
    let change = CoreEdit::ReplaceEntry {
        id: entry_id.clone(),
        text,
    };
    answer(locked(edit(daemon, agent_id, block, Some(entry_id), change)).await)
}

#[utoipa::path(delete, path = "/api/agents/{agent_id}/core-memory/{block}/{entry_id}", tag = "memories",
    params(("agent_id" = String, Path), ("block" = String, Path, description = "user or notes"), ("entry_id" = String, Path)),
    responses(
        (status = 200, description = "Entry removed, flagged or not; both blocks now", body = CoreMemoryEditResponse),
        (status = 403, description = "Local owner required", body = ErrorBody),
        (status = 404, description = "No such agent, block or entry", body = ErrorBody),
        (status = 503, description = "The change could not be saved and was undone, or did not finish", body = ErrorBody)
    ))]
pub(super) async fn delete_entry(
    State(state): State<AppState>,
    Path((agent_id, block, entry_id)): Path<(String, String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let block = match parse_block(&block) {
        Ok(block) => block,
        Err(error) => return rejected(error),
    };
    let daemon = state.daemon.clone();
    let change = CoreEdit::RemoveEntry {
        id: entry_id.clone(),
    };
    answer(locked(edit(daemon, agent_id, block, Some(entry_id), change)).await)
}

/// One owner edit: apply, persist, re-index, rewrite the file copy, all under
/// the memory write lock. `entry` names the targeted entry, which must be in
/// `block` (404 otherwise).
async fn edit(
    daemon: SharedDaemonState,
    agent_id: String,
    block: CoreBlock,
    entry: Option<String>,
    change: CoreEdit,
) -> Result<CoreMemoryEditResponse, ApiError> {
    let (agent_name, workspace_root) = {
        let guard = daemon.read().await;
        let agent = guard
            .agents
            .get(&agent_id)
            .ok_or_else(ApiError::not_found)?;
        (
            agent.config().name.clone(),
            guard
                .workspace
                .as_ref()
                .map(|workspace| workspace.root_path.clone()),
        )
    };
    let (memory, embeddings, memory_store) = memory_handles(&daemon).await;
    let mut manager = memory.write().await;
    if let Some(id) = &entry {
        let (user, notes) = core_memory::blocks_for(&manager, &agent_id);
        let entries = match block {
            CoreBlock::User => user,
            CoreBlock::Notes => notes,
        };
        if !entries.iter().any(|memory| &memory.id == id) {
            return Err(ApiError::not_found());
        }
    }
    let outcome: CoreOutcome = {
        let mut mutation = MemoryMutation::new(&mut manager);
        let outcome = core_memory::apply_edit(
            &mut mutation,
            CoreAgent {
                id: &agent_id,
                name: &agent_name,
            },
            block,
            change,
            ScanMode::Warn,
        )
        .map_err(ApiError::bad_request)?;
        if outcome.changed() {
            persist_memory_store(
                memory_store.as_ref(),
                &mut mutation,
                "failed to persist core memory",
            )
            .await?;
        }
        outcome
    };
    // Still under the memory write lock (memory, then embeddings), so two
    // edits cannot leave an older vector or file copy behind.
    match &outcome.change {
        CoreChange::Added(memory)
        | CoreChange::Replaced {
            current: memory, ..
        } => {
            sync_embedding(&mut *embeddings.write().await, memory, true);
        }
        CoreChange::Removed(memory) => {
            remove_memory_embeddings(&embeddings, std::slice::from_ref(&memory.id)).await;
        }
        CoreChange::AlreadyPresent(_) => {}
    }
    if outcome.changed() {
        files::write_copies(workspace_root.as_deref(), &manager, &agent_id);
    }
    let entry_id = match &outcome.change {
        CoreChange::Added(memory)
        | CoreChange::AlreadyPresent(memory)
        | CoreChange::Removed(memory)
        | CoreChange::Replaced {
            current: memory, ..
        } => memory.id.clone(),
    };
    let CoreMemoryView { user, notes } = view(&manager, &agent_id);
    Ok(CoreMemoryEditResponse {
        user,
        notes,
        entry_id,
        changed: outcome.changed(),
        warnings: outcome.warnings,
    })
}
