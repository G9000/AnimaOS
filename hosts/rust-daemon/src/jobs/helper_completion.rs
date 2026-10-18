//! Source settlement uses existing job claims and the daemon-wide
//! transaction. No second task registry or completion polling worker.
use super::*;

/// Called while holding the control-plane transaction, before the first
/// origin assignment is admitted. Existing-source replays bypass new admission.
pub(crate) fn ensure_origin_capacity(state: &DaemonState) -> Result<(), JobError> {
    let mut job_chains: std::collections::HashSet<_> = state
        .jobs
        .values()
        .filter(|job| job.pending_delegations.is_some())
        .map(|job| (job.id.clone(), job.attempt))
        .collect();
    // An admitted child owns capacity before its parent's source commit turns
    // Running into Pending. Two concurrent sources cannot race the last slot.
    for owner in state.agent_states() {
        for run in state.runs.async_helpers_for_owner(&owner.id) {
            if let Some(crate::runs::DelegationOrigin::JobAttempt {
                job_id, attempt, ..
            }) = run
                .async_helper
                .as_ref()
                .and_then(|helper| helper.origin.as_ref())
            {
                if state.jobs.get(job_id).is_some_and(|job| {
                    job.attempt == *attempt
                        && matches!(
                            job.status,
                            AgentJobStatus::Running | AgentJobStatus::PendingDelegations
                        )
                }) {
                    job_chains.insert((job_id.clone(), *attempt));
                }
            }
        }
    }
    if job_chains.len() >= 8 {
        return Err(JobError::Conflict(
            "The eight pending origin chains are occupied; wait for final settlement or review"
                .into(),
        ));
    }
    Ok(())
}

pub(crate) fn has_uncertain_effects(state: &DaemonState, run_id: &str) -> bool {
    state.runs.get(run_id).is_some_and(|run| {
        run.action_receipts
            .iter()
            .any(|receipt| receipt.status == crate::runs::ActionStatus::Started)
    })
}

pub(crate) fn derive_origin(
    state: &DaemonState,
    owner: &str,
    parent: &crate::runs::RunLink,
) -> Result<crate::runs::DelegationOrigin, String> {
    let source = state
        .runs
        .get(&parent.run_id)
        .ok_or("Origin launch source is unavailable")?;
    if source.source != RunSource::Job {
        return Err("Origin completion currently supports ordinary native jobs only; use chat or manual for other sources".into());
    }
    let job = state
        .jobs
        .values()
        .find(|job| {
            source.source_ref.as_deref() == Some(format!("{}:{}", job.id, job.attempt).as_str())
        })
        .ok_or("The exact origin job attempt is unavailable")?;
    if state.control_plane_store.is_none()
        || job.agent_id != owner
        || job.status != AgentJobStatus::Running
        || job.stop_requested_at_ms.is_some()
        || source.idempotency_key.as_deref()
            != Some(format!("job:{}:attempt:{}", job.id, job.attempt).as_str())
        || source.session_id != crate::sessions::session_id_for_room(&format!("job:{}", job.id))
    {
        return Err("Origin completion requires the current ordinary job claim; schedule and connector origins are not enabled".into());
    }
    Ok(crate::runs::DelegationOrigin::JobAttempt {
        job_id: job.id.clone(),
        attempt: job.attempt,
        claim_revision: job.revision,
    })
}

pub(crate) fn origin_is_current(state: &DaemonState, child: &crate::runs::RunRecord) -> bool {
    let Some(helper) = &child.async_helper else {
        return false;
    };
    let Some(crate::runs::DelegationOrigin::JobAttempt {
        job_id,
        attempt,
        claim_revision,
    }) = &helper.origin
    else {
        return false;
    };
    let Some(job) = state.jobs.get(job_id) else {
        return false;
    };
    if *claim_revision == 0
        || job.agent_id != helper.owner_agent_id
        || job.attempt != *attempt
        || job.stop_requested_at_ms.is_some()
    {
        return false;
    }
    match job.status {
        AgentJobStatus::Running => true,
        AgentJobStatus::PendingDelegations => {
            job.pending_delegations.as_ref().is_some_and(|refs| {
                refs.launch_run_id == helper.parent_run_id
                    && refs.child_run_ids.iter().any(|id| id == &child.id)
                    && helper
                        .completion
                        .run_id()
                        .is_some_and(|id| refs.completion_run_ids.iter().any(|known| known == id))
            })
        }
        _ => false,
    }
}

pub(crate) fn pending_for_parent(
    state: &DaemonState,
    owner: &str,
    parent: &str,
) -> Option<crate::runs::PendingDelegations> {
    let mut children: Vec<_> = state
        .runs
        .async_helpers_for_owner(owner)
        .into_iter()
        .filter(|run| {
            run.async_helper
                .as_ref()
                .is_some_and(|helper| helper.parent_run_id == parent && helper.origin.is_some())
        })
        .collect();
    children.sort_by(|a, b| (a.created_at_ms, &a.id).cmp(&(b.created_at_ms, &b.id)));
    if children.is_empty() {
        return None;
    }
    Some(crate::runs::PendingDelegations {
        launch_run_id: parent.into(),
        child_run_ids: children.iter().map(|child| child.id.clone()).collect(),
        completion_run_ids: children
            .iter()
            .filter_map(|child| {
                child
                    .async_helper
                    .as_ref()?
                    .completion
                    .run_id()
                    .map(str::to_owned)
            })
            .collect(),
    })
}

pub(crate) fn commit_pending(
    state: &mut DaemonState,
    id: &str,
    revision: u64,
    outcome: &crate::runs::RunOutcome,
) -> Result<bool, ApiError> {
    let owner = state
        .jobs
        .get(id)
        .ok_or_else(ApiError::not_found)?
        .agent_id
        .clone();
    let Some(refs) = pending_for_parent(state, &owner, &outcome.run_id) else {
        return Ok(false);
    };
    refs.validate().map_err(ApiError::service_unavailable)?;
    let uncertain = has_uncertain_effects(state, &outcome.run_id);
    let job = state
        .jobs
        .get_mut(id)
        .filter(|job| job.status == AgentJobStatus::Running && job.revision == revision)
        .ok_or_else(|| ApiError::conflict("Origin job claim changed"))?;
    job.pending_delegations = Some(refs);
    job.result = None;
    job.error = None;
    job.final_output = None;
    job.delegation_evidence = None;
    if uncertain
        || job.stop_requested_at_ms.is_some()
        || outcome.status != RunStatus::Completed
        || outcome.result.status != TaskStatus::Success
    {
        settle_uncertain(job,"Origin source did not commit a certain successful turn; inspect effects before retrying");
    } else {
        job.status = AgentJobStatus::PendingDelegations;
        job.finished_at_ms = None;
        advance(job);
    }
    job.validate().map_err(ApiError::service_unavailable)?;
    Ok(true)
}

pub(crate) fn attach_failed_source(state: &mut DaemonState, id: &str) {
    let Some(job) = state.jobs.get(id) else {
        return;
    };
    if job.status != AgentJobStatus::Running {
        return;
    }
    let owner = job.agent_id.clone();
    let attempt = job.attempt;
    let parent=state.runs.async_helpers_for_owner(&owner).into_iter().find_map(|child| {
        let helper=child.async_helper.as_ref()?;
        matches!(&helper.origin,Some(crate::runs::DelegationOrigin::JobAttempt {job_id,attempt:known,..}) if job_id==id && *known==attempt)
            .then(||helper.parent_run_id.clone())
    });
    if let Some(refs) = parent.and_then(|parent| pending_for_parent(state, &owner, &parent)) {
        if let Some(job) = state.jobs.get_mut(id) {
            job.pending_delegations = Some(refs);
        }
    }
}

pub(crate) fn finish_origin(
    state: &mut DaemonState,
    id: &str,
    refs: &crate::runs::PendingDelegations,
    output: Option<(String, crate::runs::DelegationFinalOutput)>,
    reason: &str,
) -> Result<bool, String> {
    let job = state.jobs.get_mut(id).filter(|job| {
        job.status == AgentJobStatus::PendingDelegations
            && job.pending_delegations.as_ref() == Some(refs)
    });
    let Some(job) = job else {
        return Ok(false);
    };
    if let Some((text, output)) = output {
        job.status = AgentJobStatus::Completed;
        job.pending_delegations = None;
        job.result = Some(text);
        job.error = None;
        job.final_output = Some(output);
        let mut evidence = crate::runs::DelegationEvidence::review(refs.clone(), reason);
        evidence.disposition = crate::runs::DelegationDisposition::Completed;
        job.delegation_evidence = Some(evidence);
        advance(job);
        job.finished_at_ms = Some(job.updated_at_ms);
        job.preserve_legacy_attempt();
    } else {
        settle_uncertain(job, reason);
        for child_id in &refs.child_run_ids {
            if let Some(child) = state.runs.get_mut(child_id) {
                if let Some(helper) = &mut child.async_helper {
                    if helper.completion.is_unresolved() {
                        helper.completion.require_review(reason);
                        child.mirrored = false;
                    }
                }
            }
        }
    }
    state.jobs[id].validate()?;
    Ok(true)
}
