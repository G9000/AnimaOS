//! Durable async assignments through the existing ledger and accepted-run queue.
mod completion;
mod job_origin;
pub(super) use completion::validate_accepted_completion;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use anima_core::{primitives::now_millis, AgentState, Content, DataValue, ToolCall};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::{
    helper_parent, is_workspace_manager, AgentRunCoordinator, AgentRunRequest, RunRoom,
    HELPER_MAX_RUNS, MAX_HELPERS_PER_COMPANION,
};
use crate::live::{run_status_event, LiveEventBody};
use crate::runs::{
    AsyncHelper, HelperCompletionState, RunLink, RunRecord, RunSource, RunStart, RunStatus,
    RunSteer,
};
use crate::sessions::SessionKind;
use crate::state::DaemonState;

pub(super) fn operation_key(
    operation: &str,
    parent: &RunLink,
    tool_call_id: &str,
) -> Result<String, String> {
    if tool_call_id.trim().is_empty() || tool_call_id.len() > 4096 {
        return Err("A host tool-call identity is required".into());
    }
    let bytes = serde_json::to_vec(&json!([
        "anima.async-helper.operation.v1",
        operation,
        parent.run_id,
        tool_call_id
    ]))
    .expect("string tuple serializes");
    Ok(format!(
        "helper-{operation}:{}",
        hex::encode(Sha256::digest(bytes))
    ))
}

/// A worktree launch has its own domain, so a replay with different
/// worktree arguments is refused like any other changed launch.
fn worktree_fingerprint(name: &str, task: &str, completion: &str, repo: &str) -> String {
    let bytes = serde_json::to_vec(&json!([
        "anima.async-helper.launch.worktree.v1",
        name,
        task,
        completion,
        repo
    ]))
    .expect("string tuple serializes");
    hex::encode(Sha256::digest(bytes))
}

fn fingerprint(name: &str, task: &str, completion: &str) -> String {
    let bytes = serde_json::to_vec(&json!([
        "anima.async-helper.launch.v1",
        name,
        task,
        completion
    ]))
    .expect("string tuple serializes");
    hex::encode(Sha256::digest(bytes))
}

/// Current host identity, never caller-selected owner/session IDs. A completed
/// launching parent is valid for an accepted child, but a management caller
/// itself must still be the live ordinary companion run executing the tool.
pub(super) fn active_caller(
    state: &DaemonState,
    owner: &str,
    parent: &RunLink,
    tool: &str,
) -> Result<AgentState, String> {
    let unavailable =
        || "Helper operation requires the current companion run and grant".to_string();
    let agent = state.agents.get(owner).ok_or_else(unavailable)?.state();
    let run = state.runs.get(&parent.run_id).ok_or_else(unavailable)?;
    if parent.agent_id != owner
        || run.agent_id != owner
        || run.session_id != parent.session_id
        || !run.status.is_in_flight()
        || run.stop.is_some()
        || run.async_helper.is_some()
        || run.helper_completion.is_some()
        || !is_workspace_manager(&agent)
        || !agent.config.allows_tool(tool)
        || !state
            .live
            .runs()
            .control(&run.id)
            .is_some_and(|control| !control.cancel.is_cancelled())
        || !state
            .sessions
            .get(owner, &parent.session_id)
            .is_some_and(|session| !session.archived)
    {
        return Err(unavailable());
    }
    Ok(agent)
}

pub(super) fn handle(record: &RunRecord) -> Value {
    json!({
        "runId": record.id,
        "agentId": record.agent_id,
        "sessionId": record.session_id,
        "status": record.status.as_str(),
        "completion": record.async_helper.as_ref().map(|helper| &helper.completion),
        "bounds": record.async_helper.as_ref().and_then(|helper| helper.effective_bounds.as_ref())
    })
}

impl AgentRunCoordinator {
    pub(crate) async fn manage_async_helper(
        &self,
        owner: String,
        parent: RunLink,
        call: ToolCall,
    ) -> Result<String, String> {
        let coordinator = self.clone();
        tokio::spawn(async move {
            coordinator
                .manage_async_helper_owned(&owner, &parent, &call)
                .await
        })
        .await
        .map_err(|_| "Helper management worker stopped".to_string())?
    }

    async fn owned_helper_record(&self, owner: &str, id: &str) -> Result<RunRecord, String> {
        let (known, history) = {
            let guard = self.state.read().await;
            (guard.runs.get(id).cloned(), guard.history.store())
        };
        let record = match known {
            Some(record) => Some(record),
            None => history
                .get_run(id)
                .await
                .map_err(|_| "Helper history is unavailable".to_string())?,
        };
        record
            .filter(|record| {
                record
                    .async_helper
                    .as_ref()
                    .is_some_and(|helper| helper.owner_agent_id == owner)
            })
            .ok_or_else(|| "Helper run unavailable".into())
    }

    async fn manage_async_helper_owned(
        &self,
        owner: &str,
        parent: &RunLink,
        call: &ToolCall,
    ) -> Result<String, String> {
        let transaction = self.control_plane_transaction().await;
        active_caller(&*self.state.read().await, owner, parent, &call.name)?;
        let text_arg = |name: &str| -> Result<Option<String>, String> {
            match call.args.get(name) {
                None => Ok(None),
                Some(DataValue::String(value)) if !value.trim().is_empty() => {
                    Ok(Some(value.clone()))
                }
                _ => Err(format!("{name} must be a nonblank string")),
            }
        };
        let id = text_arg("run_id")?;
        if id.as_ref().is_some_and(|id| id.len() > 200) {
            return Err("Helper run unavailable".into());
        }
        if call.name == "list_helper_runs" {
            let limit = match call.args.get("limit") {
                None => 10,
                Some(DataValue::Number(value))
                    if value.is_finite()
                        && value.fract() == 0.0
                        && (1.0..=20.0).contains(value) =>
                {
                    *value as usize
                }
                _ => return Err("limit must be between 1 and 20".into()),
            };
            let records = if let Some(id) = id {
                vec![self.owned_helper_record(owner, &id).await?]
            } else {
                self.state
                    .read()
                    .await
                    .runs
                    .async_helpers_for_owner(owner)
                    .into_iter()
                    .take(limit)
                    .cloned()
                    .collect()
            };
            let mut entries = Vec::new();
            let mut truncated = false;
            for record in records {
                let result = self.committed_helper_text(&record).await;
                let mut entry = handle(&record);
                let helper = record.async_helper.as_ref().expect("owned helper lookup");
                entry["source"] = json!(record.source.as_str());
                entry["parentRunId"] = json!(record.parent_run_id);
                entry["replyMessageId"] = json!(record.reply_message_id);
                entry["resultText"] = result
                    .as_ref()
                    .map(|text| json!(bounded_text(text, 8192)))
                    .unwrap_or(Value::Null);
                entry["resultTruncated"] =
                    json!(result.as_ref().is_some_and(|text| text.len() > 8192));
                entry["resultUnavailable"] =
                    json!(record.status == RunStatus::Completed && result.is_none());
                entry["error"] = record.error.as_ref().map(|error| json!({"code": bounded_text(&error.code, 128), "message": bounded_text(&error.message, 1024)})).unwrap_or(Value::Null);
                entry["pendingSteerCount"] = json!(record.pending_steers.len());
                entry["consumedSteerCount"] = json!(record.steered_keys.len());
                entry["unconsumedSteerCount"] = json!(helper.unconsumed_steers.len());
                if let Some(worktree) = &helper.worktree {
                    entry["worktree"] = worktree.public_json(true);
                }
                entry["unconsumedSteers"] =
                    json!(helper.unconsumed_steers.iter().map(|steer| json!({
                    "acceptanceKey": steer.idempotency_key, "acceptedAtMs": steer.accepted_at_ms,
                    "text": bounded_text(&steer.text, 512), "textTruncated": steer.text.len() > 512
                })).collect::<Vec<_>>());
                bound_list_entry(&mut entry)?;
                entries.push(entry);
                if json!({"runs": &entries, "truncated": false})
                    .to_string()
                    .len()
                    > 32 * 1024
                {
                    entries.pop();
                    truncated = true;
                    break;
                }
            }
            return Ok(json!({"runs": entries, "truncated": truncated}).to_string());
        }
        let id = id.ok_or_else(|| "run_id is required".to_string())?;
        let record = self.owned_helper_record(owner, &id).await?;
        if call.name == "stop_helper" {
            return self.stop_helper_owned(owner, record, transaction).await;
        }
        if call.name != "steer_helper" {
            return Err("Unknown helper operation".into());
        }
        let message = text_arg("message")?.ok_or_else(|| "message is required".to_string())?;
        if message.len() > 32_768 {
            return Err("message must be at most 32768 bytes".into());
        }
        let key = operation_key("steer", parent, &call.id)?;
        let (steer, control, persist) = {
            let mut guard = self.state.write().await;
            // Replay lookup precedes state and lifetime-cap checks, including a
            // retry after terminal commit. One call cannot change target/text.
            for original in guard
                .runs
                .async_helpers_for_owner(owner)
                .into_iter()
                .chain(std::iter::once(&record))
            {
                let helper = original.async_helper.as_ref().expect("typed lookup");
                let pending = original
                    .pending_steers
                    .iter()
                    .find(|steer| steer.idempotency_key == key);
                let archived = helper
                    .unconsumed_steers
                    .iter()
                    .find(|steer| steer.idempotency_key == key);
                if let Some(steer) = pending.or(archived) {
                    if original.id != id || steer.text != message {
                        return Err("This tool call was used with different steer arguments".into());
                    }
                    return Ok(
                        steer_answer(&id, steer, pending.is_some(), archived.is_some()).to_string(),
                    );
                }
                if let Some(steer) = original
                    .steered_keys
                    .iter()
                    .find(|steer| steer.idempotency_key == key)
                {
                    if original.id != id
                        || !steer
                            .input
                            .as_ref()
                            .is_some_and(|input| input.text == message)
                    {
                        return Err("This tool call was used with different steer arguments".into());
                    }
                    return Ok(json!({"runId": id, "acceptanceKey": key, "acceptedAtMs": steer.accepted_at_ms, "pending": false, "consumed": true}).to_string());
                }
            }
            let current = guard
                .runs
                .get(&id)
                .ok_or_else(|| "Helper run unavailable".to_string())?;
            let helper = current.async_helper.as_ref().expect("owned helper record");
            if !current.status.is_in_flight() || current.stop.is_some() {
                return Err(
                    "Only a Running or AwaitingApproval helper with an open inbox can be steered"
                        .into(),
                );
            }
            if current.pending_steers.len()
                + current.steered_keys.len()
                + helper.unconsumed_steers.len()
                >= 8
            {
                return Err(
                    "The eight accepted steer operations for this helper assignment have been used"
                        .into(),
                );
            }
            let control = guard
                .live
                .runs()
                .control(&id)
                .filter(|control| !control.cancel.is_cancelled() && !control.steering.is_closed())
                .ok_or_else(|| "Helper steering inbox is closed".to_string())?;
            let steer = RunSteer {
                idempotency_key: key,
                text: message,
                attachment_ids: Vec::new(),
                skill: None,
                accepted_at_ms: self.acceptance_clock.next(),
            };
            guard
                .runs
                .get_mut(&id)
                .expect("owned active helper exists")
                .pending_steers
                .push(steer.clone());
            (steer, control, guard.control_plane_persist_request())
        };
        if let Err(error) = persist.save().await {
            if let Some(run) = self.state.write().await.runs.get_mut(&id) {
                run.pending_steers
                    .retain(|saved| saved.idempotency_key != steer.idempotency_key);
            }
            return Err(format!("Could not persist helper steer: {error}"));
        }
        let delivered = control
            .steering
            .push(super::queue::steer_content(&steer, &[]))
            .is_ok();
        drop(transaction);
        Ok(steer_answer(&id, &steer, delivered, !delivered).to_string())
    }

    pub(super) async fn committed_helper_text(&self, record: &RunRecord) -> Option<String> {
        if record.status != RunStatus::Completed {
            return None;
        }
        let reply = record.reply_message_id.as_deref()?;
        let (known, history) = {
            let guard = self.state.read().await;
            let known = guard
                .agents
                .get(&record.agent_id)
                .and_then(|runtime| {
                    runtime.messages().iter().find(|message| {
                        message.id == reply
                            && message.room_id == record.session_id
                            && message.role == anima_core::MessageRole::Assistant
                    })
                })
                .map(|message| message.content.text.clone());
            (known, guard.history.store())
        };
        if known.is_some() {
            return known;
        }
        history
            .get_message(&record.agent_id, &record.session_id, reply)
            .await
            .ok()
            .flatten()
            .filter(|message| {
                message.agent_id == record.agent_id
                    && message.session_id == record.session_id
                    && message.message.role == anima_core::MessageRole::Assistant
            })
            .map(|message| message.message.content.text)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn accept_async_helper(
        &self,
        owner: String,
        parent: RunLink,
        tool_call_id: String,
        name: String,
        task: String,
        completion: String,
        starts: Arc<AtomicUsize>,
        worktree_repo: Option<String>,
    ) -> Result<String, String> {
        let coordinator = self.clone();
        // The mutation finishes even if its tool caller disconnects during save.
        tokio::spawn(async move {
            // Git runs before the transaction, never under the state lock. An
            // assignment that does not take the worktree discards it.
            let worktree = match &worktree_repo {
                Some(repo) => {
                    let (workspace, owner) = {
                        let guard = coordinator.state.read().await;
                        (
                            guard
                                .workspace
                                .as_ref()
                                .map(|workspace| workspace.root_path.clone()),
                            guard.profile_owner(),
                        )
                    };
                    Some(super::worktrees::create(workspace, repo.clone(), owner).await?)
                }
                None => None,
            };
            let result = coordinator
                .accept_async_helper_owned(
                    owner,
                    parent,
                    tool_call_id,
                    name,
                    task,
                    completion,
                    starts,
                    worktree_repo.zip(worktree.clone()),
                )
                .await;
            if let Some(worktree) = &worktree {
                if !matches!(result, Ok((_, true))) {
                    let owner = coordinator.state.read().await.profile_owner();
                    super::worktrees::discard(worktree, owner).await;
                }
            }
            result.map(|(answer, _)| answer)
        })
        .await
        .map_err(|_| "Helper acceptance worker stopped".to_string())?
    }

    #[allow(clippy::too_many_arguments)]
    async fn accept_async_helper_owned(
        &self,
        owner: String,
        parent: RunLink,
        tool_call_id: String,
        name: String,
        task: String,
        completion: String,
        starts: Arc<AtomicUsize>,
        worktree: Option<(String, super::worktrees::HelperWorktree)>,
    ) -> Result<(String, bool), String> {
        if name.trim().is_empty()
            || name.len() > 80
            || task.trim().is_empty()
            || task.len() > 32_768
            || !matches!(completion.as_str(), "chat" | "manual" | "origin")
        {
            return Err("name, task and completion must be within their supported bounds".into());
        }
        let key = operation_key("launch", &parent, &tool_call_id)?;
        let digest = match &worktree {
            None => fingerprint(&name, &task, &completion),
            Some((repo, _)) => worktree_fingerprint(&name, &task, &completion, repo),
        };
        let worktree = worktree.map(|(_, worktree)| worktree);
        let run_text = match &worktree {
            Some(worktree) => {
                let text = format!("{task}{}", super::worktrees::task_note(worktree));
                if text.len() > 32_768 {
                    return Err("The task is too long to add the worktree instructions".into());
                }
                text
            }
            None => task.clone(),
        };
        let transaction = self.control_plane_transaction().await;
        let (record, helper_id, baseline, slot, execution_candidate, persist) = {
            let mut guard = self.state.write().await;
            let parent_agent = active_caller(&guard, &owner, &parent, "spawn_helper_async")?;
            if super::main_chat_clear::clear_pending(&guard, &owner, &parent.session_id) {
                return Err(super::main_chat_clear::CLEAR_PENDING.into());
            }
            if let Some(original) = guard.runs.async_launch(&owner, &key) {
                if original
                    .async_helper
                    .as_ref()
                    .expect("typed lookup")
                    .launch_fingerprint
                    != digest
                {
                    return Err(
                        "This tool call was already used with different helper launch arguments"
                            .into(),
                    );
                }
                return Ok((handle(original).to_string(), false));
            }
            let origin = if completion == "origin" {
                let origin =
                    crate::jobs::helper_completion::derive_origin(&guard, &owner, &parent)?;
                let existing_chain =
                    guard
                        .runs
                        .async_helpers_for_owner(&owner)
                        .into_iter()
                        .any(|run| {
                            run.async_helper.as_ref().is_some_and(|helper| {
                                helper.parent_run_id == parent.run_id && helper.origin.is_some()
                            })
                        });
                if !existing_chain {
                    crate::jobs::helper_completion::ensure_origin_capacity(&guard)
                        .map_err(|error| format!("{error:?}"))?;
                }
                crate::runs::ensure_origin_evidence_capacity(
                    &crate::runs::origin_pins(&guard.control_plane_snapshot()),
                    9,
                )?;
                Some(origin)
            } else {
                None
            };
            if starts
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                    (count < 4).then_some(count + 1)
                })
                .is_err()
            {
                return Err(
                    "The four-helper start limit for this companion run has been reached".into(),
                );
            }
            if self.accepted_runs.is_closing() || self.is_being_deleted(&owner) {
                return Err("The companion is unavailable or shutting down".into());
            }
            let session = guard
                .sessions
                .get(&owner, &parent.session_id)
                .expect("active caller has a saved session");
            if completion == "chat" && session.kind != SessionKind::Chat {
                return Err("Automatic helper completion requires the saved original Chat; use completion manual here".into());
            }
            let helpers: Vec<_> = guard
                .list_agents()
                .into_iter()
                .filter(|agent| helper_parent(&agent.state) == Some(owner.as_str()))
                .collect();
            let available = helpers.iter().find_map(|helper| {
                if guard.runs.has_active_assignment(&helper.state.id) {
                    return None;
                }
                self.try_slot_lease(&helper.state.id, HELPER_MAX_RUNS)
                    .map(|slot| (helper.clone(), slot))
            });
            let mut config = super::helper_config_in(&parent_agent, name, worktree.as_ref());
            crate::chat_models::inherit_helper_model(&guard, Some(&parent), &mut config);
            let (helper, baseline, slot) = if let Some((helper, slot)) = available {
                let previous = helper.state.config;
                guard.restore_agent_config(&helper.state.id, config);
                (
                    guard
                        .get_agent(&helper.state.id)
                        .expect("reserved helper exists"),
                    Some(previous),
                    slot,
                )
            } else {
                if helpers.len() >= MAX_HELPERS_PER_COMPANION {
                    return Err(
                        "All four helpers have accepted assignments; wait for a result".into(),
                    );
                }
                let helper = guard.create_agent(config)?;
                let slot = self
                    .try_slot_lease(&helper.state.id, HELPER_MAX_RUNS)
                    .expect("new helper has a free slot");
                (helper, None, slot)
            };
            let room_id = anima_core::new_room_id();
            let now_ms = now_millis();
            let created = guard.ensure_run_session(crate::state::RunSessionRequest {
                agent_id: &helper.state.id,
                room_id: &room_id,
                source: RunSource::Delegation,
                system_followup_title: None,
                delegated_parent: Some(&owner),
                peer_sender: None,
                parent: Some(&parent),
                first_text: &task,
                now_ms,
            });
            debug_assert!(created, "accepted helper room is fresh");
            let mut record = RunRecord::queued(
                RunStart {
                    agent_id: helper.state.id.clone(),
                    session_id: crate::sessions::session_id_for_room(&room_id),
                    source: RunSource::Delegation,
                    source_ref: None,
                    idempotency_key: None,
                    text: run_text,
                    model: helper.state.config.model.clone(),
                    provider: helper.state.config.provider.clone(),
                    parent_run_id: Some(parent.run_id.clone()),
                },
                self.acceptance_clock.next(),
            );
            record.async_helper = Some(AsyncHelper {
                owner_agent_id: owner.clone(),
                owner_session_id: parent.session_id.clone(),
                parent_run_id: parent.run_id.clone(),
                launch_key: key,
                launch_fingerprint: digest,
                origin,
                effective_bounds: Some(crate::runs::AsyncHelperBounds::for_config(
                    &helper.state.config,
                )),
                completion: if completion == "manual" {
                    HelperCompletionState::Manual
                } else {
                    HelperCompletionState::AwaitingResult {
                        reserved_run_id: format!("run_{}", uuid::Uuid::new_v4()),
                    }
                },
                unconsumed_steers: Vec::new(),
                worktree: worktree.clone(),
            });
            let execution_candidate =
                match crate::execution_budgets::prepare_run_binding(&guard, &record, now_ms) {
                    Ok(candidate) => candidate,
                    Err(error) => {
                        guard.sessions.remove(&record.agent_id, &record.session_id);
                        if let Some(config) = baseline {
                            guard.restore_agent_config(&helper.state.id, config);
                        } else {
                            guard.remove_agent(&helper.state.id);
                        }
                        return Err(error);
                    }
                };
            if let Some(worktree) = &worktree {
                super::worktrees::remember_repo(&mut guard.helper_worktree_repos, worktree);
            }
            guard.execution_budget_candidate_snapshot = Some(execution_candidate.registry.clone());
            guard.runs.insert(record.clone());
            (
                record,
                helper.state.id,
                baseline,
                slot,
                execution_candidate,
                guard.control_plane_persist_request(),
            )
        };
        if let Err(error) =
            crate::execution_budgets::save_admission(&self.state, execution_candidate, persist)
                .await
        {
            let mut guard = self.state.write().await;
            guard.runs.remove(&record.id);
            guard.sessions.remove(&record.agent_id, &record.session_id);
            if let Some(config) = baseline {
                guard.restore_agent_config(&helper_id, config);
            } else {
                guard.remove_agent(&helper_id);
            }
            // A subscriber's initial in-memory snapshot may have observed the
            // provisional Queued record while its save was in flight.
            let mut refused = record.clone();
            refused.finish(
                RunStatus::Failed,
                Some(crate::runs::RunError::new(
                    crate::runs::COMMIT_FAILED,
                    error.to_string(),
                )),
                now_millis(),
            );
            guard.live.publish(run_status_event(&refused), Some(&owner));
            return Err(format!("Could not persist helper acceptance: {error}"));
        }
        // The durable ledger now owns the busy reservation. Never make an
        // accepted start wait for the same slot lease its admission owns.
        drop(slot);
        let answer = {
            let guard = self.state.read().await;
            guard.live.runs().register(&record.id);
            guard.publish_session_event(
                &record.agent_id,
                &record.session_id,
                LiveEventBody::SessionCreated,
            );
            guard.live.publish(run_status_event(&record), Some(&owner));
            handle(&record).to_string()
        };
        let room = self
            .state
            .read()
            .await
            .sessions
            .get(&record.agent_id, &record.session_id)
            .expect("saved helper session exists")
            .room_id()
            .to_string();
        let request = AgentRunRequest {
            agent_id: record.agent_id.clone(),
            content: Content {
                text: record.input.text.clone(),
                ..Content::default()
            },
            room: RunRoom::DelegatedIn {
                parent_id: owner,
                room_id: room,
            },
            idempotency_key: None,
            source: RunSource::Delegation,
            source_ref: None,
            parent: Some(parent),
        };
        self.enqueue_internal(&record, self.helper_start(request));
        drop(transaction);
        Ok((answer, worktree.is_some()))
    }
}

fn bounded_text(text: &str, max: usize) -> String {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

/// Preserve the exact immutable handle and state before spending response
/// space on result/steer previews. Raw text bounds do not bound JSON escaping.
fn bound_list_entry(entry: &mut Value) -> Result<(), String> {
    const ENTRY_BUDGET: usize = 31 * 1024;
    while entry.to_string().len() > ENTRY_BUDGET {
        if let Some(text) = entry["resultText"].as_str().filter(|text| !text.is_empty()) {
            let shorter = bounded_text(text, text.len() / 2);
            entry["resultText"] = json!(shorter);
            entry["resultTruncated"] = json!(true);
            continue;
        }
        let mut changed = false;
        if let Some(steers) = entry["unconsumedSteers"].as_array_mut() {
            for steer in steers {
                if let Some(text) = steer["text"].as_str().filter(|text| !text.is_empty()) {
                    let shorter = bounded_text(text, text.len() / 2);
                    steer["text"] = json!(shorter);
                    steer["textTruncated"] = json!(true);
                    changed = true;
                }
            }
        }
        if changed {
            continue;
        }
        if entry["unconsumedSteers"]
            .as_array()
            .is_some_and(|steers| !steers.is_empty())
        {
            // Counts and the full durable child record retain these inputs.
            // Listing may omit preview details, never silently delete steers.
            entry["unconsumedSteers"] = json!([]);
            entry["unconsumedSteersTruncated"] = json!(true);
            continue;
        }
        return Err("Saved helper identity exceeds the bounded lookup response".into());
    }
    Ok(())
}

fn steer_answer(id: &str, steer: &RunSteer, pending: bool, archived: bool) -> Value {
    json!({"runId": id, "acceptanceKey": steer.idempotency_key, "acceptedAtMs": steer.accepted_at_ms,
        "pending": pending, "archived": archived, "consumed": false, "maxAcceptedSteers": 8})
}

/// Rechecked after global capacity is obtained, in the durable start transaction.
pub(super) fn validate_accepted_helper(
    state: &DaemonState,
    run_id: Option<&str>,
    agent_id: &str,
    room_id: &str,
    parent: Option<&RunLink>,
) -> Result<(), crate::routes::ApiError> {
    let invalid = || {
        crate::routes::ApiError::conflict(
            "Accepted helper lineage is unavailable or no longer authorized",
        )
    };
    let record = run_id
        .and_then(|id| state.runs.get(id))
        .ok_or_else(invalid)?;
    let helper = record.async_helper.as_ref().ok_or_else(invalid)?;
    let parent = parent.ok_or_else(invalid)?;
    let launching = state.runs.get(&helper.parent_run_id).ok_or_else(invalid)?;
    let session = state
        .sessions
        .get(&helper.owner_agent_id, &helper.owner_session_id)
        .ok_or_else(invalid)?;
    if record.agent_id != agent_id
        || record.session_id != crate::sessions::session_id_for_room(room_id)
        || helper.parent_run_id != parent.run_id
        || helper.owner_agent_id != parent.agent_id
        || helper.owner_session_id != parent.session_id
        || launching.agent_id != parent.agent_id
        || launching.session_id != parent.session_id
        || launching.stop.is_some()
        || (launching.status.is_terminal() && launching.status != RunStatus::Completed)
        || session.archived
        || (helper.origin.is_some()
            && !crate::jobs::helper_completion::origin_is_current(state, record))
    {
        return Err(invalid());
    }
    Ok(())
}
