//! Typed asynchronous helper lineage. The run ledger remains the only task
//! registry; these fields grant no authority without current host validation.
use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::{RunRecord, RunSource, RunStatus, RunSteer, MAX_RUN_INPUT_TEXT_BYTES};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AsyncHelper {
    pub(crate) owner_agent_id: String,
    pub(crate) owner_session_id: String,
    pub(crate) parent_run_id: String,
    pub(crate) launch_key: String,
    /// Domain-separated digest of the immutable name/task/completion request.
    /// A reused helper's current configuration cannot establish replay equality.
    pub(crate) launch_fingerprint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) effective_bounds: Option<AsyncHelperBounds>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) origin: Option<super::DelegationOrigin>,
    pub(crate) completion: HelperCompletionState,
    #[serde(default)]
    pub(crate) unconsumed_steers: Vec<RunSteer>,
    /// The assignment's isolated git worktree and, once the run ended, its report.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) worktree: Option<crate::agent_runs::worktrees::HelperWorktree>,
}

/// Host-derived effective limits, frozen with this immutable assignment.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AsyncHelperBounds {
    pub(crate) max_tool_turns: usize,
    pub(crate) max_duration_ms: u64,
    pub(crate) max_output_tokens_per_response: u32,
    pub(crate) max_accepted_steers: usize,
}

impl AsyncHelperBounds {
    pub(crate) fn for_config(config: &anima_core::AgentConfig) -> Self {
        let settings = config.settings.as_ref();
        Self {
            max_tool_turns: settings
                .and_then(|settings| settings.max_tool_iterations)
                .unwrap_or(8)
                .min(8),
            max_duration_ms: settings
                .and_then(|settings| settings.timeout_ms)
                .unwrap_or(120_000)
                .min(120_000),
            max_output_tokens_per_response: settings
                .and_then(|settings| settings.max_tokens)
                .unwrap_or(4096)
                .min(4096),
            max_accepted_steers: 8,
        }
    }

    pub(crate) fn intersect(&self, other: &Self) -> Self {
        Self {
            max_tool_turns: self.max_tool_turns.min(other.max_tool_turns),
            max_duration_ms: self.max_duration_ms.min(other.max_duration_ms),
            max_output_tokens_per_response: self
                .max_output_tokens_per_response
                .min(other.max_output_tokens_per_response),
            max_accepted_steers: 8,
        }
    }

    pub(crate) fn apply(&self, config: &mut anima_core::AgentConfig) {
        let settings = config.settings.get_or_insert_with(Default::default);
        settings.max_tool_iterations = Some(self.max_tool_turns);
        settings.timeout_ms = Some(self.max_duration_ms);
        settings.max_tokens = Some(self.max_output_tokens_per_response);
    }

    fn valid(&self) -> bool {
        self.max_tool_turns <= 8
            && self.max_duration_ms <= 120_000
            && self.max_output_tokens_per_response <= 4096
            && self.max_accepted_steers == 8
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum HelperCompletionState {
    Manual,
    AwaitingResult {
        #[serde(rename = "reservedRunId")]
        reserved_run_id: String,
    },
    AwaitingParent {
        #[serde(rename = "reservedRunId")]
        reserved_run_id: String,
    },
    Queued {
        #[serde(rename = "runId")]
        run_id: String,
    },
    Settled {
        #[serde(rename = "runId")]
        run_id: String,
    },
    ReviewRequired {
        reason: String,
        #[serde(default, rename = "runId", skip_serializing_if = "Option::is_none")]
        run_id: Option<String>,
    },
    Suppressed {
        reason: String,
        #[serde(default, rename = "runId", skip_serializing_if = "Option::is_none")]
        run_id: Option<String>,
    },
}

impl HelperCompletionState {
    pub(crate) fn run_id(&self) -> Option<&str> {
        match self {
            Self::AwaitingResult { reserved_run_id } | Self::AwaitingParent { reserved_run_id } => {
                Some(reserved_run_id)
            }
            Self::Queued { run_id } | Self::Settled { run_id } => Some(run_id),
            Self::ReviewRequired { run_id, .. } | Self::Suppressed { run_id, .. } => {
                run_id.as_deref()
            }
            Self::Manual => None,
        }
    }

    pub(crate) fn is_unresolved(&self) -> bool {
        matches!(
            self,
            Self::AwaitingResult { .. } | Self::AwaitingParent { .. } | Self::Queued { .. }
        )
    }

    pub(crate) fn require_review(&mut self, reason: &str) {
        if self.is_unresolved() {
            *self = Self::ReviewRequired {
                reason: reason.into(),
                run_id: self.run_id().map(str::to_string),
            };
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct HelperCompletion {
    pub(crate) helper_run_id: String,
    pub(crate) launch_parent_run_id: String,
}

/// Run identities alone cannot prove persisted helper configuration and private
/// room ownership. Validate all present relationships before restore mutates state.
pub(crate) fn validate_snapshot_links(
    records: &[RunRecord],
    agents: &HashMap<String, anima_core::AgentRuntimeSnapshot>,
    sessions: &[crate::sessions::SessionRecord],
) -> Result<(), String> {
    for record in records {
        let Some(helper) = &record.async_helper else {
            continue;
        };
        let unresolved = helper.completion.is_unresolved() || !record.status.is_terminal();
        match agents.get(&record.agent_id) {
            Some(agent)
                if crate::agent_runs::config_helper_parent(&agent.state.config)
                    == Some(helper.owner_agent_id.as_str()) => {}
            None if !unresolved => {}
            _ => return Err("Async helper configuration owner is missing or inconsistent".into()),
        }
        match sessions
            .iter()
            .find(|session| session.agent_id == record.agent_id && session.id == record.session_id)
        {
            Some(session)
                if session.kind == crate::sessions::SessionKind::Helper
                    && session.parent_agent_id.as_deref()
                        == Some(helper.owner_agent_id.as_str())
                    && session.parent_session_id.as_deref()
                        == Some(helper.owner_session_id.as_str())
                    && session.parent_run_id.as_deref() == Some(helper.parent_run_id.as_str()) => {}
            None if !unresolved => {}
            _ => {
                return Err(
                    "Async helper private session lineage is missing or inconsistent".into(),
                )
            }
        }
        if unresolved
            && (!agents.contains_key(&helper.owner_agent_id)
                || !sessions.iter().any(|session| {
                    session.agent_id == helper.owner_agent_id
                        && session.id == helper.owner_session_id
                }))
        {
            return Err("Async helper owner or original session is missing".into());
        }
        if let Some(session) = sessions.iter().find(|session| {
            session.agent_id == helper.owner_agent_id && session.id == helper.owner_session_id
        }) {
            if !matches!(helper.completion, HelperCompletionState::Manual)
                && helper.completion.is_unresolved()
                && !(session.kind == crate::sessions::SessionKind::Chat && helper.origin.is_none()
                    || session.kind == crate::sessions::SessionKind::Job
                        && matches!(
                            helper.origin,
                            Some(super::DelegationOrigin::JobAttempt { .. })
                        ))
            {
                return Err("Automatic helper completion requires the original Chat".into());
            }
        }
    }
    Ok(())
}

/// Archive leftovers on their original assignment. They can be inspected but
/// can never become generic owner-authored Web runs on restart or failed commit.
pub(crate) fn archive_steers(record: &mut RunRecord) {
    record.archive_helper_steers_except(&HashSet::new());
}

impl RunRecord {
    pub(crate) fn archive_helper_steers_except(&mut self, taken: &HashSet<String>) -> bool {
        let Some(helper) = &mut self.async_helper else {
            return false;
        };
        let mut changed = false;
        let mut pending = Vec::new();
        for steer in std::mem::take(&mut self.pending_steers) {
            if taken.contains(&steer.idempotency_key) {
                pending.push(steer);
            } else if !helper
                .unconsumed_steers
                .iter()
                .any(|known| known.idempotency_key == steer.idempotency_key)
            {
                helper.unconsumed_steers.push(steer);
                changed = true;
            }
        }
        self.pending_steers = pending;
        helper
            .unconsumed_steers
            .sort_by_key(|steer| steer.accepted_at_ms);
        self.read_steer_keys.retain(|key| taken.contains(key));
        if changed {
            self.mirrored = false;
        }
        changed
    }
}

fn bounded_id(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 200
}

fn valid_steers(steers: &[RunSteer]) -> bool {
    let mut keys = HashSet::new();
    steers.len() <= 8
        && steers.iter().all(|steer| {
            !steer.idempotency_key.trim().is_empty()
                && steer.idempotency_key.len() <= 4096
                && keys.insert(steer.idempotency_key.as_str())
                && !steer.text.trim().is_empty()
                && steer.text.len() <= MAX_RUN_INPUT_TEXT_BYTES
                && steer.attachment_ids.is_empty()
                && steer.skill.is_none()
        })
}

/// Record-only validation runs before restore mutates daemon state. The state
/// layer additionally validates helper configurations and exact session kinds.
pub(crate) fn validate(records: &[RunRecord]) -> Result<(), String> {
    let by_id: HashMap<&str, &RunRecord> = records
        .iter()
        .map(|record| (record.id.as_str(), record))
        .collect();
    let mut launches = HashSet::new();
    let mut reservations = HashSet::new();
    for record in records {
        if let Some(helper) = &record.async_helper {
            if record.helper_completion.is_some()
                || record.source != RunSource::Delegation
                || record.parent_run_id.as_deref() != Some(helper.parent_run_id.as_str())
                || !bounded_id(&helper.owner_agent_id)
                || !bounded_id(&helper.owner_session_id)
                || !bounded_id(&helper.parent_run_id)
                || helper.owner_agent_id == record.agent_id
                || helper.parent_run_id == record.id
                || helper.launch_key.trim().is_empty()
                || helper.launch_key.len() > 4096
                || helper.launch_fingerprint.len() != 64
                || !helper
                    .launch_fingerprint
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                || !launches.insert((helper.owner_agent_id.as_str(), helper.launch_key.as_str()))
                || !valid_steers(&helper.unconsumed_steers)
                || !valid_steers(&record.pending_steers)
                || helper
                    .effective_bounds
                    .as_ref()
                    .is_none_or(|bounds| !bounds.valid())
                || helper.unconsumed_steers.len()
                    + record.pending_steers.len()
                    + record.steered_keys.len()
                    > 8
            {
                return Err(format!(
                    "invalid asynchronous helper identity or bounds: {}",
                    record.id
                ));
            }
            let mut steer_keys = HashSet::new();
            if !helper
                .unconsumed_steers
                .iter()
                .chain(&record.pending_steers)
                .map(|steer| steer.idempotency_key.as_str())
                .chain(
                    record
                        .steered_keys
                        .iter()
                        .map(|steer| steer.idempotency_key.as_str()),
                )
                .all(|key| !key.trim().is_empty() && key.len() <= 4096 && steer_keys.insert(key))
            {
                return Err("asynchronous helper steer identities overlap".into());
            }
            let unresolved = helper.completion.is_unresolved() || !record.status.is_terminal();
            match by_id.get(helper.parent_run_id.as_str()) {
                Some(parent)
                    if parent.agent_id == helper.owner_agent_id
                        && parent.session_id == helper.owner_session_id
                        && parent.async_helper.is_none()
                        && parent.helper_completion.is_none() => {}
                None if !unresolved => {}
                _ => {
                    return Err(format!(
                        "asynchronous helper has an invalid launch parent: {}",
                        record.id
                    ))
                }
            }
            let valid_state = match &helper.completion {
                HelperCompletionState::AwaitingResult { .. } => !record.status.is_terminal(),
                HelperCompletionState::AwaitingParent { .. }
                | HelperCompletionState::Queued { .. }
                | HelperCompletionState::Settled { .. } => {
                    record.status == RunStatus::Completed && record.reply_message_id.is_some()
                }
                HelperCompletionState::ReviewRequired { reason, .. }
                | HelperCompletionState::Suppressed { reason, .. } => {
                    !reason.trim().is_empty() && reason.len() <= 1024
                }
                HelperCompletionState::Manual => true,
            };
            if !valid_state {
                return Err(format!(
                    "asynchronous helper completion state disagrees with its run: {}",
                    record.id
                ));
            }
            if let Some(completion_id) = helper.completion.run_id() {
                if !bounded_id(completion_id)
                    || completion_id == record.id
                    || completion_id == helper.parent_run_id
                    || !reservations.insert(completion_id)
                {
                    return Err("invalid or duplicate helper completion identity".into());
                }
                match (&helper.completion, by_id.get(completion_id)) {
                    (
                        HelperCompletionState::AwaitingResult { .. }
                        | HelperCompletionState::AwaitingParent { .. },
                        Some(_),
                    ) => {
                        return Err(
                            "an unadmitted helper completion identity is already used".into()
                        );
                    }
                    (HelperCompletionState::Queued { .. }, None) => {
                        return Err("an unresolved helper completion is missing".into());
                    }
                    (state, Some(completion)) => {
                        let origin = completion.helper_completion.as_ref();
                        if origin.is_none_or(|origin| {
                            origin.helper_run_id != record.id
                                || origin.launch_parent_run_id != helper.parent_run_id
                        }) || completion.agent_id != helper.owner_agent_id
                            || completion.session_id != helper.owner_session_id
                            || (matches!(state, HelperCompletionState::Settled { .. })
                                && !completion.status.is_terminal())
                        {
                            return Err("helper completion relationship is inconsistent".into());
                        }
                    }
                    _ => {}
                }
            }
            // A malformed parent chain must not manufacture callback authority.
            let mut visited = HashSet::from([record.id.as_str()]);
            let mut next = record.parent_run_id.as_deref();
            while let Some(id) = next {
                if !visited.insert(id) {
                    return Err("asynchronous helper parent cycle".into());
                }
                next = by_id.get(id).and_then(|run| run.parent_run_id.as_deref());
            }
        }
        if let Some(origin) = &record.helper_completion {
            if record.source != RunSource::Delegation
                || !bounded_id(&origin.helper_run_id)
                || !bounded_id(&origin.launch_parent_run_id)
                || origin.helper_run_id == record.id
                || origin.launch_parent_run_id == record.id
                || record.parent_run_id.as_deref() != Some(origin.launch_parent_run_id.as_str())
            {
                return Err("invalid helper completion origin".into());
            }
            match by_id.get(origin.helper_run_id.as_str()) {
                Some(source)
                    if source.async_helper.as_ref().is_some_and(|helper| {
                        helper.owner_agent_id == record.agent_id
                            && helper.owner_session_id == record.session_id
                            && helper.parent_run_id == origin.launch_parent_run_id
                            && helper.completion.run_id() == Some(record.id.as_str())
                    }) => {}
                None if record.status.is_terminal() && record.mirrored => {}
                _ => {
                    return Err(
                        "helper completion source relationship is missing or foreign".into(),
                    )
                }
            }
        }
    }
    Ok(())
}

/// Only unresolved relationships pin local evidence. Settled/review/suppressed
/// records continue through ordinary bounded, mirrored history retention.
pub(crate) fn pinned<'a>(records: impl Iterator<Item = &'a RunRecord>) -> HashSet<String> {
    let records: Vec<_> = records.collect();
    let active: HashSet<_> = records
        .iter()
        .filter(|record| !record.status.is_terminal())
        .map(|record| record.id.as_str())
        .collect();
    let pending_evidence: HashSet<_> = records
        .iter()
        .filter(|record| !record.status.is_terminal() || !record.mirrored)
        .map(|record| record.id.as_str())
        .collect();
    let mut pinned = HashSet::new();
    for record in &records {
        if let Some(helper) = &record.async_helper {
            if helper.completion.is_unresolved()
                || !record.status.is_terminal()
                || active.contains(helper.parent_run_id.as_str())
                || helper
                    .completion
                    .run_id()
                    .is_some_and(|id| pending_evidence.contains(id))
            {
                pinned.insert(record.id.clone());
                pinned.insert(helper.parent_run_id.clone());
                if let Some(id) = helper.completion.run_id() {
                    pinned.insert(id.into());
                }
            }
        }
    }
    pinned
}

impl super::RunLedger {
    /// Only event-related assignments are considered; there is no polling or
    /// independently scheduled completion registry.
    pub(crate) fn helper_event_records(&self, event_run_id: &str) -> Vec<RunRecord> {
        let live = self.records_for_helpers();
        live.into_iter()
            .filter(|record| {
                record.async_helper.as_ref().is_some_and(|helper| {
                    record.id == event_run_id
                        || helper.parent_run_id == event_run_id
                        || helper.completion.run_id() == Some(event_run_id)
                })
            })
            .collect()
    }

    pub(crate) fn settle_helper_origin(&mut self, run_id: &str) {
        if self.get(run_id).is_some_and(|record| {
            record.stop.is_some()
                || (record.status.is_terminal() && record.status != RunStatus::Completed)
        }) {
            let affected = self
                .records_for_helpers()
                .into_iter()
                .filter(|record| {
                    record
                        .async_helper
                        .as_ref()
                        .is_some_and(|helper| helper.parent_run_id == run_id)
                })
                .map(|record| record.id)
                .collect::<Vec<_>>();
            for id in affected {
                if let Some(record) = self.get_mut(&id) {
                    let helper = record.async_helper.as_mut().expect("typed selection");
                    if helper.completion.is_unresolved() {
                        helper.completion.require_review("The launching run failed, was interrupted or was stopped; no completion retry was scheduled");
                        record.mirrored = false;
                    }
                }
            }
        }
        let Some(completion) = self
            .get(run_id)
            .filter(|record| record.status.is_terminal())
            .cloned()
        else {
            return;
        };
        let Some(origin) = completion.helper_completion else {
            return;
        };
        let Some(source) = self.get_mut(&origin.helper_run_id) else {
            return;
        };
        let Some(helper) = &mut source.async_helper else {
            return;
        };
        let successful = completion.status == RunStatus::Completed && completion.stop.is_none();
        let pending = matches!(&helper.completion, HelperCompletionState::Queued { run_id } if run_id == &completion.id);
        let rolled_back_success = !successful
            && matches!(&helper.completion, HelperCompletionState::Settled { run_id } if run_id == &completion.id);
        if pending || rolled_back_success {
            helper.completion = if completion.status == RunStatus::Completed
                && completion.stop.is_none()
            {
                HelperCompletionState::Settled {
                    run_id: completion.id,
                }
            } else {
                HelperCompletionState::ReviewRequired {
                    reason: "Helper continuation did not complete; no retry was scheduled".into(),
                    run_id: Some(completion.id),
                }
            };
            source.mirrored = false;
        }
    }
}
