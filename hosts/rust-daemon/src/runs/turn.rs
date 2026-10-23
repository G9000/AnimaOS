//! Host binding and terminal projection for portable, untrusted turn progress.
use super::{RunRecord, RunSource, RunStatus};
use crate::state::DaemonState;
use anima_core::turn_recovery::{TurnBinding, TurnCheckpoint};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TurnTerminal {
    pub(crate) status: RunStatus,
    pub(crate) reply_message_id: Option<String>,
    pub(crate) reply_sha256: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunTurn {
    pub(crate) binding: TurnBinding,
    /// Quoted recovery data only, retained even if an older root leaves retention.
    pub(crate) original_input: super::RunInput,
    pub(crate) checkpoint: Option<TurnCheckpoint>,
    pub(crate) terminal: Option<TurnTerminal>,
}

pub(crate) fn digest<T: Serialize + ?Sized>(value: &T) -> String {
    super::committed_output_sha256(&serde_json::to_string(value).expect("turn binding serializes"))
}

pub(crate) fn workspace_digest(state: &DaemonState) -> String {
    digest(&state.workspace)
}

pub(crate) fn bind(
    state: &DaemonState,
    record: &RunRecord,
    browser_selected: bool,
) -> Result<RunTurn, String> {
    let runtime = state
        .agents
        .get(&record.agent_id)
        .ok_or("Turn owner was deleted")?;
    let input = digest(&record.input);
    let source = record
        .recovery_of
        .as_ref()
        .and_then(|id| state.runs.get(id));
    let original = source.and_then(|r| r.turn.as_ref());
    let original_input = original
        .map(|t| t.original_input.clone())
        .unwrap_or_else(|| source.unwrap_or(record).input.clone());
    let binding = TurnBinding {
        root_run_id: original
            .map(|t| t.binding.root_run_id.clone())
            .unwrap_or_else(|| source.unwrap_or(record).id.clone()),
        attempt_run_id: record.id.clone(),
        agent_id: record.agent_id.clone(),
        session_id: record.session_id.clone(),
        original_input_digest: digest(&original_input),
        attempt_input_digest: input,
        workspace_digest: workspace_digest(state),
        config_digest: digest(runtime.config()),
        automatic_eligible: !browser_selected
            && record.status == RunStatus::Queued
            && record.started_at_ms.is_none()
            && record.recovery_of.is_none()
            && record.system_followup == Some(false)
            && matches!(record.source, RunSource::Web | RunSource::Api)
            && record.source_ref.is_none()
            && record.parent_run_id.is_none()
            && record.async_helper.is_none()
            && record.helper_completion.is_none()
            && !crate::agent_runs::is_helper_config(runtime.config())
            && state
                .sessions
                .get(&record.agent_id, &record.session_id)
                .is_some_and(|s| s.kind == crate::sessions::SessionKind::Chat),
    };
    binding.validate()?;
    Ok(RunTurn {
        binding,
        original_input,
        checkpoint: None,
        terminal: None,
    })
}

pub(crate) fn current(state: &DaemonState, record: &RunRecord) -> Result<(), String> {
    current_admission(state, record, true)
}

/// The browser binder owns its prepared workspace proof and specific refusal.
pub(crate) fn current_for_browser_selection(
    state: &DaemonState,
    record: &RunRecord,
) -> Result<(), String> {
    current_admission(state, record, false)
}

fn current_admission(
    state: &DaemonState,
    record: &RunRecord,
    check_workspace: bool,
) -> Result<(), String> {
    validate(record)?;
    let turn = record
        .turn
        .as_ref()
        .ok_or("Legacy run has no verified turn binding")?;
    turn.binding.validate()?;
    let runtime = state
        .agents
        .get(&record.agent_id)
        .ok_or("Turn owner was deleted")?;
    if turn.binding.attempt_run_id != record.id
        || turn.binding.agent_id != record.agent_id
        || turn.binding.session_id != record.session_id
        || turn.binding.attempt_input_digest != digest(&record.input)
        || (check_workspace && turn.binding.workspace_digest != workspace_digest(state))
        || turn.binding.config_digest != digest(runtime.config())
        || !state
            .sessions
            .contains(&record.agent_id, &record.session_id)
    {
        return Err("Turn binding changed; owner review is required".into());
    }
    Ok(())
}

/// A started runtime keeps its admitted prompt/configuration. Native tools
/// independently recheck live grants; queued admission and recovery stay strict.
pub(crate) fn active(state: &DaemonState, record: &RunRecord) -> Result<(), String> {
    validate(record)?;
    let turn = record.turn.as_ref().ok_or("Turn binding unavailable")?;
    if !state.agents.contains_key(&record.agent_id)
        || !state
            .sessions
            .contains(&record.agent_id, &record.session_id)
        || turn.binding.workspace_digest != workspace_digest(state)
    {
        return Err("Active turn owner or workspace changed".into());
    }
    Ok(())
}

pub(crate) fn automatic_candidate(state: &DaemonState, record: &RunRecord) -> bool {
    state.control_plane_store.is_some()
        && record.status == RunStatus::Interrupted
        && record
            .error
            .as_ref()
            .is_some_and(|e| e.code == super::RESTART_BEFORE_START)
        && record.started_at_ms.is_none()
        && record.stop.is_none()
        && record.recovery.is_none()
        && record.recovery_of.is_none()
        && record.recovery_attempt == 0
        && record.system_followup == Some(false)
        && matches!(record.source, RunSource::Web | RunSource::Api)
        && record.source_ref.is_none()
        && record.parent_run_id.is_none()
        && record.async_helper.is_none()
        && record.helper_completion.is_none()
        && record.action_receipts.is_empty()
        && record.tools_started.is_empty()
        && record.steps.is_empty()
        && record.pending_steers.is_empty()
        && record.turn.as_ref().is_some_and(|turn| {
            turn.binding.automatic_eligible
                && turn.checkpoint.is_none()
                && turn.binding.root_run_id == record.id
                && turn.original_input == record.input
        })
        && state
            .agents
            .get(&record.agent_id)
            .is_some_and(|runtime| !crate::agent_runs::is_helper_config(runtime.config()))
        && state
            .sessions
            .get(&record.agent_id, &record.session_id)
            .is_some_and(|s| s.kind == crate::sessions::SessionKind::Chat)
        && current(state, record).is_ok()
}

pub(crate) fn recovery_state(
    state: &DaemonState,
    record: &RunRecord,
) -> anima_core::turn_recovery::TurnRecoveryState {
    use anima_core::turn_recovery::TurnRecoveryState as R;
    if record.status == RunStatus::Completed {
        R::AlreadyCompleted
    } else if record.stop.is_some() || record.status == RunStatus::Cancelled {
        R::Stopped
    } else if automatic_candidate(state, record) {
        R::SafelyRecoverable
    } else if !super::recovery::summary(record).eligible
        || (record.turn.is_some() && current(state, record).is_err())
    {
        R::InvalidOrStale
    } else {
        R::OwnerReviewRequired
    }
}

impl RunRecord {
    pub(crate) fn seal_turn_terminal(&mut self) {
        if let Some(turn) = &mut self.turn {
            turn.terminal = self.status.is_terminal().then(|| TurnTerminal {
                status: self.status,
                reply_message_id: self.reply_message_id.clone(),
                reply_sha256: self.reply_sha256.clone(),
            });
        }
    }
}

pub(crate) fn validate(record: &RunRecord) -> Result<(), String> {
    let Some(turn) = &record.turn else {
        return Ok(());
    };
    turn.binding.validate()?;
    if turn.binding.attempt_run_id != record.id
        || turn.binding.agent_id != record.agent_id
        || turn.binding.session_id != record.session_id
        || turn.binding.attempt_input_digest != digest(&record.input)
        || turn.binding.original_input_digest != digest(&turn.original_input)
        || turn.original_input.text.len() > super::MAX_RUN_INPUT_TEXT_BYTES
        || turn.original_input.attachment_ids.len() > super::MAX_RUN_ATTACHMENTS
        || turn
            .original_input
            .attachment_ids
            .iter()
            .any(|id| id.is_empty() || id.len() > 256)
        || turn
            .original_input
            .skill
            .as_ref()
            .is_some_and(|skill| skill.len() > 1024)
        || (record.recovery_of.is_none()
            && (turn.binding.root_run_id != record.id
                || turn.binding.original_input_digest != turn.binding.attempt_input_digest))
    {
        return Err("Saved turn binding does not match its run".into());
    }
    if let Some(checkpoint) = &turn.checkpoint {
        checkpoint.validate(&turn.binding)?;
    }
    if let Some(terminal) = &turn.terminal {
        if !record.status.is_terminal()
            || terminal.status != record.status
            || terminal.reply_message_id != record.reply_message_id
            || terminal.reply_sha256 != record.reply_sha256
        {
            return Err("Saved turn terminal identity does not match the final run".into());
        }
        if record.status == RunStatus::Completed
            && (terminal.reply_message_id.is_none() || terminal.reply_sha256.is_none())
        {
            return Err("Completed turn is missing its committed reply identity".into());
        }
    } else if record.status.is_terminal() {
        return Err("Saved terminal turn is missing its final identity".into());
    }
    Ok(())
}

impl RunRecord {
    /// Preserve an acknowledged unpublished checkpoint in a newer ordinary snapshot.
    /// Terminal outcomes and current run ownership always come from that snapshot.
    pub(crate) fn merge_acknowledged_turn(&mut self, acknowledged: &RunTurn) {
        let Some(turn) = &mut self.turn else {
            return;
        };
        if turn.binding != acknowledged.binding {
            return;
        }
        let Some(checkpoint) = &acknowledged.checkpoint else {
            return;
        };
        if turn
            .checkpoint
            .as_ref()
            .is_none_or(|current| current.sequence < checkpoint.sequence)
        {
            turn.checkpoint = Some(checkpoint.clone());
            self.mirrored = false;
        }
    }
}
