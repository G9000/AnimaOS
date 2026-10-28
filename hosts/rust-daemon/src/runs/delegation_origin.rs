//! Bounded useful-work evidence on the existing source attempt/claim. These
//! references describe work; current host lineage still establishes authority.
use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use utoipa::ToSchema;

/// Only the authentic launching run may derive this witness. A later source
/// revision is mutable; claimRevision is retained admission evidence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum DelegationOrigin {
    JobAttempt {
        #[serde(rename = "jobId")]
        job_id: String,
        attempt: u32,
        #[serde(rename = "claimRevision")]
        claim_revision: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PendingDelegations {
    pub(crate) launch_run_id: String,
    pub(crate) child_run_ids: Vec<String>,
    pub(crate) completion_run_ids: Vec<String>,
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.trim() == value
        && value.len() <= 200
        && !value.chars().any(char::is_control)
}

pub(crate) fn sha256(text: &str) -> String {
    hex::encode(Sha256::digest(text.as_bytes()))
}

pub(super) fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

impl PendingDelegations {
    pub(crate) fn validate(&self) -> Result<(), String> {
        let mut identities = HashSet::new();
        if !valid_id(&self.launch_run_id)
            || self.child_run_ids.is_empty()
            || self.child_run_ids.len() > 4
            || self.completion_run_ids.is_empty()
            || self.completion_run_ids.len() > self.child_run_ids.len()
            || !identities.insert(self.launch_run_id.as_str())
            || self
                .child_run_ids
                .iter()
                .chain(&self.completion_run_ids)
                .any(|id| !valid_id(id) || !identities.insert(id.as_str()))
        {
            return Err("Invalid bounded pending-delegation references".into());
        }
        Ok(())
    }
}

/// Retained after a pending chain releases its active slot. These exact
/// references remain evidence even when no callback may be replayed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DelegationDisposition {
    Completed,
    ReviewRequired,
    Suppressed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DelegationEvidence {
    pub(crate) launch_run_id: String,
    pub(crate) child_run_ids: Vec<String>,
    pub(crate) completion_run_ids: Vec<String>,
    pub(crate) disposition: DelegationDisposition,
    pub(crate) reason: String,
}

impl DelegationEvidence {
    pub(crate) fn references(&self) -> PendingDelegations {
        PendingDelegations {
            launch_run_id: self.launch_run_id.clone(),
            child_run_ids: self.child_run_ids.clone(),
            completion_run_ids: self.completion_run_ids.clone(),
        }
    }
    pub(crate) fn review(pending: PendingDelegations, reason: &str) -> Self {
        Self {
            launch_run_id: pending.launch_run_id,
            child_run_ids: pending.child_run_ids,
            completion_run_ids: pending.completion_run_ids,
            disposition: DelegationDisposition::ReviewRequired,
            reason: reason.into(),
        }
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        PendingDelegations {
            launch_run_id: self.launch_run_id.clone(),
            child_run_ids: self.child_run_ids.clone(),
            completion_run_ids: self.completion_run_ids.clone(),
        }
        .validate()?;
        if self.reason.trim().is_empty() || self.reason.len() > 4000 {
            return Err("Invalid delegation settlement reason".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DelegationOutput {
    pub(crate) run_id: String,
    pub(crate) reply_message_id: String,
    pub(crate) output_sha256: String,
    pub(crate) preview: String,
    pub(crate) preview_truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DelegationFinalOutput {
    pub(crate) generation: u64,
    /// Digest of the exact visible aggregate stored as the attempt result.
    pub(crate) sha256: String,
    /// Immutable launch/request order; never callback arrival order.
    pub(crate) outputs: Vec<DelegationOutput>,
}

/// Owner review input, checked against the currently displayed aggregate by
/// the native service. The values themselves grant no review authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DelegationOutputBinding {
    pub(crate) generation: u64,
    pub(crate) sha256: String,
}

impl DelegationFinalOutput {
    pub(crate) fn validate(&self, result: Option<&str>) -> Result<(), String> {
        let mut runs = HashSet::new();
        let mut replies = HashSet::new();
        if self.generation == 0
            || !valid_digest(&self.sha256)
            || result.is_none_or(|text| self.sha256 != sha256(text))
            || self.outputs.is_empty()
            || self.outputs.len() > 4
            || self.outputs.iter().any(|output| {
                !valid_id(&output.run_id)
                    || !valid_id(&output.reply_message_id)
                    || !valid_digest(&output.output_sha256)
                    || !runs.insert(output.run_id.as_str())
                    || !replies.insert(output.reply_message_id.as_str())
                    || (!output.preview_truncated
                        && output.output_sha256 != sha256(&output.preview))
                    || serde_json::to_vec(output).map_or(true, |bytes| bytes.len() > 8 * 1024)
            })
            || serde_json::to_vec(self).map_or(true, |bytes| bytes.len() > 64 * 1024)
        {
            return Err("Invalid final delegation output or aggregate digest".into());
        }
        Ok(())
    }
}

pub(crate) fn validate_origin_snapshot(
    snapshot: &crate::control_plane_store::ControlPlaneSnapshot,
) -> Result<(), String> {
    let records: HashMap<_, _> = snapshot
        .runs
        .iter()
        .map(|run| (run.id.as_str(), run))
        .collect();
    let jobs: HashMap<_, _> = snapshot
        .jobs
        .iter()
        .map(|job| (job.id.as_str(), job))
        .collect();
    for run in &snapshot.runs {
        let Some(helper) = run.async_helper.as_ref() else {
            continue;
        };
        let Some(origin) = helper.origin.as_ref() else {
            continue;
        };
        let DelegationOrigin::JobAttempt {
            job_id,
            attempt,
            claim_revision,
        } = origin;
        if !valid_id(job_id) || !(1..=3).contains(attempt) || *claim_revision == 0 {
            return Err("Invalid immutable job claim witness".into());
        }
        // Retired closed history carries no runnable rights. A retained owning
        // attempt below always requires its actual source records to remain.
        let Some(job) = jobs.get(job_id.as_str()) else {
            if run.status.is_terminal() && run.mirrored && !helper.completion.is_unresolved() {
                continue;
            }
            return Err("Origin job claim is missing".into());
        };
        let parent = records
            .get(helper.parent_run_id.as_str())
            .ok_or("Origin launch run is missing")?;
        if job.agent_id != helper.owner_agent_id
            || parent.source != super::RunSource::Job
            || parent.agent_id != job.agent_id
            || parent.session_id != helper.owner_session_id
            || parent.source_ref.as_deref() != Some(format!("{job_id}:{attempt}").as_str())
            || parent.idempotency_key.as_deref()
                != Some(format!("job:{job_id}:attempt:{attempt}").as_str())
            || parent.async_helper.is_some()
            || parent.helper_completion.is_some()
            || run.parent_run_id.as_deref() != Some(parent.id.as_str())
            || (job.attempt != *attempt
                && !job.attempts.iter().any(|saved| saved.attempt == *attempt))
        {
            return Err("Origin job source identity disagrees with the exact claim".into());
        }
    }
    for job in &snapshot.jobs {
        if let Some(pending) = &job.pending_delegations {
            validate_job_references(&records, &job.id, &job.agent_id, job.attempt, pending)?;
        }
        if let Some(evidence) = &job.delegation_evidence {
            validate_job_references(
                &records,
                &job.id,
                &job.agent_id,
                job.attempt,
                &evidence.references(),
            )?;
            if let Some(output) = &job.final_output {
                validate_final_sources(&records, output, &evidence.references())?;
            }
        }
        for attempt in &job.attempts {
            if let Some(evidence) = &attempt.delegation_evidence {
                validate_job_references(
                    &records,
                    &job.id,
                    &job.agent_id,
                    attempt.attempt,
                    &evidence.references(),
                )?;
                if let Some(output) = &attempt.final_output {
                    validate_final_sources(&records, output, &evidence.references())?;
                }
            }
        }
    }
    ensure_origin_evidence_capacity(&origin_pins(snapshot), 0)?;
    Ok(())
}

fn validate_final_sources(
    records: &HashMap<&str, &super::RunRecord>,
    output: &DelegationFinalOutput,
    refs: &PendingDelegations,
) -> Result<(), String> {
    for id in std::iter::once(&refs.launch_run_id).chain(&refs.child_run_ids) {
        let source = records
            .get(id.as_str())
            .ok_or("Required final source evidence is missing")?;
        if source.status != super::RunStatus::Completed
            || source.stop.is_some()
            || source
                .action_receipts
                .iter()
                .any(|receipt| receipt.status == super::ActionStatus::Started)
        {
            return Err(
                "Final aggregate has a failed, stopped or uncertain required source".into(),
            );
        }
    }
    if output.outputs.len() != refs.completion_run_ids.len() {
        return Err("Final output omits required callback evidence".into());
    }
    for (entry, id) in output.outputs.iter().zip(&refs.completion_run_ids) {
        let run = records
            .get(id.as_str())
            .ok_or("Final callback source is missing")?;
        if entry.run_id != *id
            || run.status != super::RunStatus::Completed
            || run.stop.is_some()
            || run.reply_message_id.as_deref() != Some(entry.reply_message_id.as_str())
            || run.reply_sha256.as_deref() != Some(entry.output_sha256.as_str())
            || run
                .action_receipts
                .iter()
                .any(|receipt| receipt.status == super::ActionStatus::Started)
        {
            return Err("Final output disagrees with committed callback evidence".into());
        }
    }
    Ok(())
}

fn validate_job_references(
    records: &HashMap<&str, &super::RunRecord>,
    job_id: &str,
    owner: &str,
    attempt: u32,
    refs: &PendingDelegations,
) -> Result<(), String> {
    refs.validate()?;
    let parent = records
        .get(refs.launch_run_id.as_str())
        .ok_or("Saved job launch evidence is missing")?;
    if parent.agent_id != owner
        || parent.source != super::RunSource::Job
        || parent.source_ref.as_deref() != Some(format!("{job_id}:{attempt}").as_str())
        || parent.idempotency_key.as_deref()
            != Some(format!("job:{job_id}:attempt:{attempt}").as_str())
        || refs.child_run_ids.len() != refs.completion_run_ids.len()
    {
        return Err("Saved job launch evidence is foreign".into());
    }
    let mut witness = None;
    for (id, completion_id) in refs.child_run_ids.iter().zip(&refs.completion_run_ids) {
        let child = records
            .get(id.as_str())
            .ok_or("Saved job child evidence is missing")?;
        let helper = child
            .async_helper
            .as_ref()
            .ok_or("Saved job child is not a helper assignment")?;
        let Some(DelegationOrigin::JobAttempt {
            job_id: source_id,
            attempt: source_attempt,
            claim_revision,
        }) = &helper.origin
        else {
            return Err("Saved job child lacks the exact immutable source witness".into());
        };
        if source_id != job_id
            || *source_attempt != attempt
            || *claim_revision == 0
            || witness.is_some_and(|known| known != *claim_revision)
            || helper.owner_agent_id != owner
            || helper.owner_session_id != parent.session_id
            || helper.parent_run_id != parent.id
            || child.parent_run_id.as_deref() != Some(parent.id.as_str())
            || helper.completion.run_id() != Some(completion_id.as_str())
        {
            return Err("Saved job assignment/callback evidence is foreign".into());
        }
        witness = Some(*claim_revision);
        if let Some(completion) = records.get(completion_id.as_str()) {
            if completion.agent_id != owner
                || completion.session_id != parent.session_id
                || completion.source != super::RunSource::Delegation
                || completion.parent_run_id.as_deref() != Some(parent.id.as_str())
                || completion.helper_completion.as_ref().is_none_or(|origin| {
                    origin.helper_run_id != *id || origin.launch_parent_run_id != parent.id
                })
            {
                return Err("Saved callback has a different source assignment".into());
            }
        } else if matches!(
            helper.completion,
            super::HelperCompletionState::Queued { .. }
                | super::HelperCompletionState::Settled { .. }
        ) {
            return Err("Saved admitted callback evidence is missing".into());
        }
    }
    Ok(())
}

pub(crate) fn origin_pins(
    snapshot: &crate::control_plane_store::ControlPlaneSnapshot,
) -> HashSet<String> {
    let mut pins = HashSet::new();
    let mut add = |refs: &PendingDelegations| {
        pins.insert(refs.launch_run_id.clone());
        pins.extend(refs.child_run_ids.iter().cloned());
        pins.extend(refs.completion_run_ids.iter().cloned());
    };
    for job in &snapshot.jobs {
        if let Some(refs) = &job.pending_delegations {
            add(refs);
        }
        if let Some(evidence) = &job.delegation_evidence {
            add(&evidence.references());
        }
        for attempt in &job.attempts {
            if let Some(evidence) = &attempt.delegation_evidence {
                add(&evidence.references());
            }
        }
    }
    // The current running source owns children admitted before its terminal
    // source commit publishes Pending. Closed orphan metadata never self-pins.
    for child in &snapshot.runs {
        let Some(helper) = &child.async_helper else {
            continue;
        };
        let Some(DelegationOrigin::JobAttempt {
            job_id, attempt, ..
        }) = &helper.origin
        else {
            continue;
        };
        if snapshot.jobs.iter().any(|job| {
            job.id == *job_id
                && job.attempt == *attempt
                && job.status == crate::jobs::AgentJobStatus::Running
        }) {
            pins.insert(helper.parent_run_id.clone());
            pins.insert(child.id.clone());
            if let Some(id) = helper.completion.run_id() {
                pins.insert(id.into());
            }
        }
    }
    // A saved question must keep its interrupted source available for an answer
    // and reviewed continuation, even after ordinary terminal-history retention.
    for question in &snapshot.clarifications {
        if question.status == crate::clarifications::ClarificationStatus::Cancelled {
            continue;
        }
        if let Some(source) = snapshot.runs.iter().find(|run| {
            run.id == question.run_id
                && run.agent_id == question.agent_id
                && run.session_id == question.session_id
        }) {
            if source.recovery.is_none()
                && source.stop.is_none()
                && !matches!(
                    source.status,
                    crate::runs::RunStatus::Completed | crate::runs::RunStatus::Cancelled
                )
            {
                pins.insert(source.id.clone());
            }
        }
    }
    // Pending document publication needs its immutable native intent for
    // read-only recovery. The registry bounds these scopes to eight drafts;
    // prepared reviews and completed publication receipts retain no run pins.
    for (run_id, agent_id, session_id) in snapshot.document_edits.pending_native_scopes() {
        if let Some(source) = snapshot.runs.iter().find(|run| {
            run.id == run_id && run.agent_id == agent_id && run.session_id == session_id
        }) {
            pins.insert(source.id.clone());
        }
    }
    pins
}

pub(crate) fn ensure_origin_evidence_capacity(
    pins: &HashSet<String>,
    additional: usize,
) -> Result<(), String> {
    if pins
        .len()
        .checked_add(additional)
        .is_none_or(|count| count > 4096)
    {
        Err("The finite 4096-run origin evidence budget is occupied".into())
    } else {
        Ok(())
    }
}
