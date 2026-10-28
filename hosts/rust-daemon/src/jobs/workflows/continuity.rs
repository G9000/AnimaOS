//! Owner-only projections of retained workflow attempt evidence.
use serde::Serialize;
use utoipa::ToSchema;

use super::{owned, AgentJobStatus, JobError, JobService};
use crate::runs::{
    continuity::{project, RunContinuityEvidence},
    RunSource,
};

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkflowAttemptEvidence {
    workflow_id: String,
    workflow_revision: u64,
    step_id: String,
    job_id: String,
    step_attempt: usize,
    status: AgentJobStatus,
    started_at_ms: Option<u64>,
    finished_at_ms: Option<u64>,
    result: Option<String>,
    error: Option<String>,
    result_truncated: bool,
    evidence_state: &'static str,
    run: Option<AttemptRunEvidence>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct AttemptRunEvidence {
    id: String,
    session_id: String,
    status: String,
    evidence: RunContinuityEvidence,
}

impl JobService {
    pub(crate) async fn inspect_workflow_attempt(
        &self,
        agent_id: &str,
        workflow_id: &str,
        step_id: &str,
        job_id: &str,
    ) -> Result<WorkflowAttemptEvidence, JobError> {
        let _transaction = self.runs.control_plane_transaction().await;
        let state = self.state.read().await;
        let workflow = owned(&state, agent_id, workflow_id)?;
        let (step_index, step) = workflow
            .steps
            .iter()
            .enumerate()
            .find(|(_, step)| step.id == step_id)
            .ok_or(JobError::NotFound)?;
        let attempt = step
            .attempts
            .iter()
            .position(|id| id == job_id)
            .ok_or(JobError::NotFound)?
            + 1;
        let job = state
            .jobs
            .get(job_id)
            .filter(|job| job.agent_id == agent_id)
            .ok_or(JobError::NotFound)?;
        if !job.workflow.as_ref().is_some_and(|binding| {
            binding.workflow_id == workflow_id
                && binding.step_id == step_id
                && binding.step_index == step_index
                && binding.attempt as usize == attempt
        }) || job.max_attempts != 1
            || job.attempt > 1
        {
            return Err(JobError::Conflict(
                "Saved workflow attempt linkage is inconsistent".into(),
            ));
        }
        let source_ref = format!("{}:{}", job.id, job.attempt);
        let runs: Vec<_> = state
            .runs
            .for_agent(agent_id)
            .into_iter()
            .filter(|run| {
                run.source == RunSource::Job
                    && run.source_ref.as_deref() == Some(source_ref.as_str())
            })
            .collect();
        if runs.len() > 1 {
            return Err(JobError::Conflict(
                "More than one run matches this saved attempt".into(),
            ));
        }
        if job.attempt == 0 && !runs.is_empty() {
            return Err(JobError::Conflict(
                "An unclaimed workflow attempt cannot own a run".into(),
            ));
        }
        let run = runs.first().map(|run| AttemptRunEvidence {
            id: run.id.clone(),
            session_id: run.session_id.clone(),
            status: run.status.as_str().into(),
            evidence: project(run),
        });
        let evidence_state = if run.is_some() {
            "retained"
        } else if job.status == AgentJobStatus::Queued && job.attempt == 0 {
            "not_started"
        } else {
            "unavailable"
        };
        let result_truncated = job
            .attempts
            .last()
            .map(|attempt| attempt.result_truncated)
            .unwrap_or_else(|| {
                job.result
                    .as_ref()
                    .is_some_and(|result| result.len() >= 65533)
            });
        Ok(WorkflowAttemptEvidence {
            workflow_id: workflow.id.clone(),
            workflow_revision: workflow.revision,
            step_id: step.id.clone(),
            job_id: job.id.clone(),
            step_attempt: attempt,
            status: job.status,
            started_at_ms: job.started_at_ms,
            finished_at_ms: job.finished_at_ms,
            result: job.result.clone(),
            error: job.error.clone(),
            result_truncated,
            evidence_state,
            run,
        })
    }
}
