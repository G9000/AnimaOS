//! Conversation reviews: every `CONVERSATION_EVERY` owner messages the recent
//! conversation is reviewed, and the review may propose core-memory edits.
//! Free mode applies them at once (each with Undo); Strict mode applies one
//! when the owner accepts it. Every edit goes through `core_memory::apply_edit`
//! with the scanner refusing, as the agent's own `core_memory` tool does.

use anima_core::{AgentConfig, Message, MessageRole};
use anima_memory::{Memory, MemoryManager, MemoryScope, MemoryType};

use super::records::*;
use super::service::LearningService;
use crate::core_memory::commit::{commit, reindex_for, CommitError, CoreStores, Reindex};
use crate::core_memory::{self, CoreAgent, CoreChange, CoreEdit, ScanMode};
use crate::routes::ApiError;
use crate::runs::{RunRecord, RunSource, RunStatus};

/// Whether a finished run is one owner chat turn: a completed, top-level run
/// the owner started from chat (web, API, Telegram or Discord) on the agent
/// itself, whose input is not a scheduled check-in and whose reply is not
/// silent. Helpers, delegated and peer runs, helper completions, daemon
/// follow-ups and swarm members (which never run through here) do not count.
/// `owner_message` is the run's first user message; `reply` its reply text.
/// A Telegram or Discord run counts only when `owner_only_chat` (see
/// `owner_only_connector_chat`) or when `owner_message` is a turn the owner
/// sent from the web console into the thread (`sessions::is_owner_web_turn`):
/// other people's messages must not rewrite About you.
///
/// Conversation reviews count these turns; the external memory provider's
/// turn sync uses exactly the same filter.
pub(crate) fn counts_as_owner_turn(
    run: &RunRecord,
    agent: &AgentConfig,
    owner_message: Option<&Message>,
    reply: Option<&str>,
    owner_only_chat: bool,
) -> bool {
    let source_counts = match run.source {
        RunSource::Web | RunSource::Api => true,
        // A turn the owner typed from the console into the thread carries
        // daemon-set metadata no inbound message can, so it counts even in a
        // shared chat.
        RunSource::Telegram | RunSource::Discord => {
            owner_only_chat || owner_message.is_some_and(crate::sessions::is_owner_web_turn)
        }
        _ => false,
    };
    source_counts
        && run.status == RunStatus::Completed
        && run.error.is_none()
        && run.stop.is_none()
        && run.parent_run_id.is_none()
        && run.async_helper.is_none()
        && run.helper_completion.is_none()
        && run.system_followup != Some(true)
        && !crate::agent_runs::is_helper_config(agent)
        && owner_message.is_some_and(|message| {
            message.role == MessageRole::User && !crate::sessions::is_checkin_message(message)
        })
        && reply.is_some_and(|text| {
            !text.trim().is_empty() && !crate::schedules::is_silent_checkin_reply(text)
        })
}

/// Whether a Telegram or Discord run provably came from a chat that holds the
/// owner alone, looked up from the connector state when the run commits (the
/// inbound record is still there then). Telegram: the run's update arrived in
/// the approved chat and that chat is a private chat; groups admit every
/// member. Discord: only a direct message would prove it, and a connector binds
/// one guild text channel shared by its approved users, so no Discord run does
/// today. A run whose source reference names nothing known proves nothing.
pub(crate) fn owner_only_connector_chat(
    state: &crate::state::DaemonState,
    run: &RunRecord,
) -> bool {
    let Some((connector_id, message_id)) = run
        .source_ref
        .as_deref()
        .and_then(|source| source.rsplit_once(':'))
    else {
        return false;
    };
    match run.source {
        RunSource::Telegram => {
            let Ok(update_id) = message_id.parse::<i64>() else {
                return false;
            };
            let private = crate::connectors::TelegramChatKind::Private;
            state
                .inbound
                .get(&(connector_id.to_owned(), update_id))
                .is_some_and(|inbound| {
                    inbound.chat.kind == private
                        && state
                            .connectors
                            .get(connector_id)
                            .and_then(|connector| connector.approved_chat.as_ref())
                            .is_some_and(|chat| chat.id == inbound.chat.id && chat.kind == private)
                })
        }
        RunSource::Discord => state
            .discord_connectors
            .get(connector_id)
            .is_some_and(|connector| connector.channel.kind.is_direct_message()),
        _ => false,
    }
}

/// What applying one core proposal changed, for the proposal's record.
struct Applied {
    memory_id: Option<String>,
    previous_text: Option<String>,
    removed: Option<RemovedCoreEntry>,
}

fn core_edit(proposal: &CoreProposal) -> CoreEdit {
    let text = proposal.text.clone().unwrap_or_default();
    let old_text = proposal.old_text.clone().unwrap_or_default();
    match proposal.action {
        CoreProposalAction::Add => CoreEdit::Add { text },
        CoreProposalAction::Replace => CoreEdit::Replace { old_text, text },
        CoreProposalAction::Remove => CoreEdit::Remove { old_text },
    }
}

fn removed_entry(memory: &Memory) -> RemovedCoreEntry {
    RemovedCoreEntry {
        id: memory.id.clone(),
        content: memory.content.clone(),
        created_at: memory.created_at,
        tags: memory.tags.clone().unwrap_or_default(),
        importance: serde_json::Number::from_f64(memory.importance)
            .unwrap_or_else(|| serde_json::Number::from(0)),
    }
}

fn restored_entry(entry: &RemovedCoreEntry, agent_id: &str, agent_name: &str) -> Memory {
    Memory {
        id: entry.id.clone(),
        agent_id: agent_id.into(),
        agent_name: agent_name.into(),
        memory_type: MemoryType::Fact,
        content: entry.content.clone(),
        importance: entry.importance.as_f64().unwrap_or_default(),
        created_at: entry.created_at,
        tags: Some(entry.tags.clone()),
        scope: MemoryScope::Private,
        room_id: None,
        world_id: None,
        session_id: None,
    }
}

/// Applies `proposal` and commits it. `Err(Ok(reason))` is a rule's refusal;
/// `Err(Err(()))` a failed save, which leaves the store unchanged.
async fn apply(
    stores: &CoreStores,
    agent: CoreAgent<'_>,
    proposal: &CoreProposal,
) -> Result<Applied, Result<String, ()>> {
    let block = proposal.block.block();
    let edit = core_edit(proposal);
    let outcome = commit(stores, agent.id, |manager| {
        let outcome = core_memory::apply_edit(manager, agent, block, edit, ScanMode::Refuse)?;
        let reindex = reindex_for(&outcome.change);
        Ok((outcome, reindex))
    })
    .await
    .map_err(|error| match error {
        CommitError::Refused(reason) => Ok(reason),
        CommitError::Persist(error) => {
            tracing::warn!(%error, "a review's core memory edit could not be saved");
            Err(())
        }
    })?;
    Ok(match outcome.change {
        CoreChange::Added(memory) => Applied {
            memory_id: Some(memory.id),
            previous_text: None,
            removed: None,
        },
        // Nothing changed, so there is nothing to undo.
        CoreChange::AlreadyPresent(_) => Applied {
            memory_id: None,
            previous_text: None,
            removed: None,
        },
        CoreChange::Replaced { previous, current } => Applied {
            memory_id: Some(current.id),
            previous_text: Some(previous.content),
            removed: None,
        },
        CoreChange::Removed(memory) => Applied {
            memory_id: Some(memory.id.clone()),
            previous_text: None,
            removed: Some(removed_entry(&memory)),
        },
    })
}

/// Settles a pending core proposal with what applying it did.
fn record(proposal: &mut LearningProposal, result: Result<Applied, String>) {
    let Some(edit) = proposal.core_edit.as_mut() else {
        return;
    };
    match result {
        Ok(applied) => {
            proposal.decision = LearningDecision::Acknowledged;
            proposal.memory_id = applied.memory_id;
            edit.previous_text = applied.previous_text;
            edit.removed = applied.removed;
        }
        Err(reason) => {
            proposal.decision = LearningDecision::Failed;
            edit.error = Some(reason);
        }
    }
    proposal.decision_version = proposal.decision_version.saturating_add(1);
}

fn stores(guard: &crate::state::DaemonState) -> CoreStores {
    CoreStores {
        memory: guard.memory_handle(),
        embeddings: guard.memory_embeddings_handle(),
        store: guard.memory_store_config(),
        files_root: guard
            .workspace
            .as_ref()
            .map(|workspace| workspace.root_path.clone()),
    }
}

/// Undo never overwrites or deletes text the owner changed after the review.
const CHANGED_AFTER_REVIEW: &str =
    "This entry changed after the review; edit it in the console instead";

/// Undoes an applied core edit in `manager`. An entry that is already gone
/// (or back) leaves nothing to do.
fn undo_edit(
    manager: &mut MemoryManager,
    agent: CoreAgent<'_>,
    memory_id: &str,
    edit: &CoreProposal,
) -> Result<((), Reindex), String> {
    match edit.action {
        CoreProposalAction::Add => {
            let Some(current) = manager
                .get(memory_id)
                .filter(|memory| core_memory::is_core_entry(memory))
            else {
                return Ok(((), Reindex::Unchanged));
            };
            if Some(current.content.as_str()) != edit.text.as_deref() {
                return Err(CHANGED_AFTER_REVIEW.into());
            }
            manager.delete_memory(memory_id);
            Ok(((), Reindex::Remove(memory_id.into())))
        }
        CoreProposalAction::Replace => {
            let Some(current) = manager.get(memory_id) else {
                return Ok(((), Reindex::Unchanged));
            };
            let previous = edit
                .previous_text
                .clone()
                .ok_or("This edit has nothing to restore")?;
            if Some(current.content.as_str()) != edit.text.as_deref() {
                return Err(CHANGED_AFTER_REVIEW.into());
            }
            // The previous text was already in the block; restoring it only
            // warns, as an owner edit does.
            core_memory::apply_edit(
                manager,
                agent,
                edit.block.block(),
                CoreEdit::ReplaceEntry {
                    id: memory_id.into(),
                    text: previous,
                },
                ScanMode::Warn,
            )?;
            Ok(((), Reindex::Upsert(memory_id.into())))
        }
        CoreProposalAction::Remove => {
            let entry = edit
                .removed
                .as_ref()
                .filter(|entry| entry.id == memory_id)
                .ok_or("This edit has nothing to restore")?;
            if manager.get(memory_id).is_some() {
                return Ok(((), Reindex::Unchanged));
            }
            core_memory::restore_entry(manager, &restored_entry(entry, agent.id, agent.name))?;
            Ok(((), Reindex::Upsert(memory_id.into())))
        }
    }
}

impl LearningService {
    /// After a counted owner turn: admits a conversation review once enough
    /// turns are counted and learning may run by itself. A refusal (the daily
    /// limit, a run already reviewed) keeps the count, so the next owner
    /// message tries again.
    pub(crate) async fn consider_conversation(&self, agent_id: &str, run_id: &str) {
        let due = {
            let guard = self.state.read().await;
            guard.learning_auto_allowed(agent_id) && guard.learning.conversation_due(agent_id)
        };
        if due {
            if let Err(error) = self
                .admit(agent_id, run_id, LearningTrigger::Conversation)
                .await
            {
                tracing::debug!(
                    agent_id,
                    message = error.message(),
                    "conversation review not admitted"
                );
            }
        }
    }

    /// Free mode: a conversation review's core edits are applied at once, in
    /// order, each recorded on its proposal (applied, with what Undo needs, or
    /// failed, with the rule's reason).
    pub(super) async fn keep_core_edits(&self, review_id: &str) {
        let _transaction = self.transactions.lock().await;
        let (agent_id, agent_name, edits, stores) = {
            let guard = self.state.read().await;
            let Some(review) = guard
                .learning
                .reviews
                .get(review_id)
                .filter(|r| r.state == LearningReviewState::Completed)
            else {
                return;
            };
            if guard.learning_mode(&review.agent_id) != LearningMode::Free {
                return;
            }
            let Some(runtime) = guard.agents.get(&review.agent_id) else {
                return;
            };
            let edits: Vec<(String, CoreProposal)> = review
                .proposals
                .iter()
                .filter(|p| p.decision == LearningDecision::Pending)
                .filter_map(|p| Some((p.id.clone(), p.core_edit.clone()?)))
                .collect();
            (
                review.agent_id.clone(),
                runtime.config().name.clone(),
                edits,
                stores(&guard),
            )
        };
        if edits.is_empty() {
            return;
        }
        let agent = CoreAgent {
            id: &agent_id,
            name: &agent_name,
        };
        let mut results = Vec::new();
        for (proposal_id, edit) in edits {
            match apply(&stores, agent, &edit).await {
                Ok(applied) => results.push((proposal_id, Ok(applied))),
                Err(Ok(reason)) => results.push((proposal_id, Err(reason))),
                // An unsaved edit stays pending for the owner.
                Err(Err(())) => {}
            }
        }
        let mut guard = self.state.write().await;
        let Some(review) = guard.learning.reviews.get_mut(review_id) else {
            return;
        };
        for (proposal_id, result) in results {
            if let Some(proposal) = review
                .proposals
                .iter_mut()
                .find(|p| p.id == proposal_id && p.decision == LearningDecision::Pending)
            {
                record(proposal, result);
            }
        }
        let persist = guard.control_plane_persist_request();
        drop(guard);
        // An unsaved record leaves the edit in core memory, where the console
        // changes it.
        let _ = persist.save().await;
    }

    /// Strict mode: the owner accepted a pending core proposal; apply it now.
    /// A rule's refusal settles it as failed with the reason. The caller
    /// holds the learning transaction.
    pub(super) async fn accept_core_edit(
        &self,
        agent_id: &str,
        review_id: &str,
        proposal_id: &str,
        edit: CoreProposal,
    ) -> Result<LearningReviewRecord, ApiError> {
        let (agent_name, stores) = {
            let guard = self.state.read().await;
            let runtime = guard.agents.get(agent_id).ok_or_else(ApiError::not_found)?;
            (runtime.config().name.clone(), stores(&guard))
        };
        let agent = CoreAgent {
            id: agent_id,
            name: &agent_name,
        };
        let result = match apply(&stores, agent, &edit).await {
            Ok(applied) => Ok(applied),
            Err(Ok(reason)) => Err(reason),
            Err(Err(())) => return Err(super::service::unavailable()),
        };
        let mut guard = self.state.write().await;
        let review = guard
            .learning
            .reviews
            .get_mut(review_id)
            .filter(|r| r.agent_id == agent_id)
            .ok_or_else(ApiError::not_found)?;
        let proposal = review
            .proposals
            .iter_mut()
            .find(|p| p.id == proposal_id && p.decision == LearningDecision::Pending)
            .ok_or_else(|| {
                ApiError::conflict("Proposal decision changed; refresh before deciding")
            })?;
        record(proposal, result);
        let result = review.clone();
        let persist = guard.control_plane_persist_request();
        drop(guard);
        // The edit is already in core memory. An unsaved decision is lost on
        // restart; accepting again then finds the edit applied (a duplicate add
        // changes nothing) or refused.
        persist
            .save()
            .await
            .map_err(|_| super::service::unavailable())?;
        Ok(result)
    }

    /// Undoes an applied core edit: an added entry is deleted, a replaced one
    /// gets its previous text back, a removed one is restored in place. The
    /// caller holds the learning transaction.
    pub(super) async fn undo_core_edit(
        &self,
        agent_id: &str,
        memory_id: &str,
        edit: &CoreProposal,
    ) -> Result<(), ApiError> {
        let (agent_name, stores) = {
            let guard = self.state.read().await;
            let runtime = guard.agents.get(agent_id).ok_or_else(ApiError::not_found)?;
            (runtime.config().name.clone(), stores(&guard))
        };
        let agent = CoreAgent {
            id: agent_id,
            name: &agent_name,
        };
        commit(&stores, agent_id, |manager| {
            undo_edit(manager, agent, memory_id, edit)
        })
        .await
        .map_err(|error| match error {
            CommitError::Refused(reason) => ApiError::conflict(reason),
            CommitError::Persist(_) => super::service::unavailable(),
        })
    }
}
