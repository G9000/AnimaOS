use super::*;
use crate::agent_runs::AgentRunCoordinator;
use crate::control_plane_store::ControlPlaneStoreConfig;
use anima_core::workflows::{WorkflowResumeAction, WorkflowStepSpec};
use std::sync::Arc;
use tokio::sync::{RwLock, Semaphore};

fn setup() -> (JobService, String, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let mut state = DaemonState::new();
    state.set_control_plane_store(Some(ControlPlaneStoreConfig::Json(
        dir.path().join("state.json"),
    )));
    let owner = state
        .create_agent(
            serde_json::from_value(serde_json::json!({
                "name": "continuity-test", "model": "deterministic"
            }))
            .unwrap(),
        )
        .unwrap()
        .state
        .id;
    let state = Arc::new(RwLock::new(state));
    let runs = AgentRunCoordinator::new(state.clone(), Arc::new(Semaphore::new(4)));
    (JobService::new(state, runs), owner, dir)
}

fn input() -> WorkflowInput {
    WorkflowInput {
        title: "Saved work".into(),
        request_key: "continuity-test".into(),
        steps: vec![WorkflowStepSpec {
            id: "first".into(),
            title: "First".into(),
            kind: WorkflowStepKind::Task,
            prompt: "Do one bounded step".into(),
        }],
    }
}

fn decision(revision: u64, action: WorkflowResumeAction) -> WorkflowResumeRequest {
    WorkflowResumeRequest {
        revision,
        action,
        value: None,
        acknowledge_uncertain: false,
    }
}

#[tokio::test]
async fn continuity_pause_holds_queued_job_and_continue_reuses_it() {
    let (service, owner, _dir) = setup();
    let created = service.create_workflow(&owner, input()).await.unwrap();
    service.reconcile_workflows().await.unwrap();
    let queued = service.get_workflow(&owner, &created.id).await.unwrap();
    let job_id = queued.steps[0].job_id.clone().unwrap();
    let paused = service
        .resume_workflow(
            &owner,
            &created.id,
            decision(queued.revision, WorkflowResumeAction::Pause),
        )
        .await
        .unwrap();
    {
        let state = service.state.read().await;
        let job = &state.jobs[&job_id];
        assert_eq!(job.status, AgentJobStatus::Queued);
        assert!(!dispatch_allowed(&state, job));
    }
    service.reconcile_workflows().await.unwrap();
    let held = service.get_workflow(&owner, &created.id).await.unwrap();
    assert_eq!(held.steps[0].job_id.as_deref(), Some(job_id.as_str()));
    assert_eq!(held.steps[0].attempt, 1);
    let resumed = service
        .resume_workflow(
            &owner,
            &created.id,
            decision(paused.revision, WorkflowResumeAction::Continue),
        )
        .await
        .unwrap();
    assert_eq!(resumed.steps[0].job_id.as_deref(), Some(job_id.as_str()));
    let state = service.state.read().await;
    assert!(dispatch_allowed(&state, &state.jobs[&job_id]));
    assert_eq!(state.jobs.len(), 1);
}

#[tokio::test]
async fn continuity_pause_before_attachment_survives_restore_without_new_job() {
    let (service, owner, _dir) = setup();
    let created = service.create_workflow(&owner, input()).await.unwrap();
    let paused = service
        .resume_workflow(
            &owner,
            &created.id,
            decision(created.revision, WorkflowResumeAction::Pause),
        )
        .await
        .unwrap();
    service.reconcile_workflows().await.unwrap();
    let snapshot = {
        let state = service.state.read().await;
        assert!(state.jobs.is_empty());
        state.control_plane_snapshot()
    };
    let mut restored = DaemonState::new();
    restored.restore_control_plane_snapshot(snapshot).unwrap();
    let saved = &restored.workflows[&created.id];
    assert!(saved.paused);
    assert_eq!(saved.revision, paused.revision);
    assert!(!needs_reconciliation(&restored, saved));
}

#[tokio::test]
async fn continuity_attempt_inspection_checks_owner_and_exact_step_binding() {
    let (service, owner, _dir) = setup();
    let created = service.create_workflow(&owner, input()).await.unwrap();
    service.reconcile_workflows().await.unwrap();
    let queued = service.get_workflow(&owner, &created.id).await.unwrap();
    let job = queued.steps[0].job_id.as_ref().unwrap();
    let view = service
        .inspect_workflow_attempt(&owner, &created.id, "first", job)
        .await
        .unwrap();
    let view = serde_json::to_value(view).unwrap();
    assert_eq!(view["evidenceState"], "not_started");
    assert_eq!(view["workflowRevision"], queued.revision);
    assert!(view["run"].is_null());
    assert!(matches!(
        service
            .inspect_workflow_attempt("other", &created.id, "first", job)
            .await,
        Err(JobError::NotFound)
    ));
    assert!(matches!(
        service
            .inspect_workflow_attempt(&owner, &created.id, "other", job)
            .await,
        Err(JobError::NotFound)
    ));
    assert!(matches!(
        service
            .inspect_workflow_attempt(&owner, &created.id, "first", "other")
            .await,
        Err(JobError::NotFound)
    ));
    {
        let mut state = service.state.write().await;
        state
            .jobs
            .get_mut(job)
            .unwrap()
            .workflow
            .as_mut()
            .unwrap()
            .step_id = "other".into();
    }
    assert!(matches!(
        service
            .inspect_workflow_attempt(&owner, &created.id, "first", job)
            .await,
        Err(JobError::Conflict(_))
    ));
}

#[tokio::test]
async fn continuity_attempt_inspection_reads_only_the_exact_job_run() {
    use crate::runs::{
        ActionDurability, ActionReceipt, ActionStatus, RunRecord, RunSource, RunStart,
    };
    let (service, owner, _dir) = setup();
    let created = service.create_workflow(&owner, input()).await.unwrap();
    service.reconcile_workflows().await.unwrap();
    let queued = service.get_workflow(&owner, &created.id).await.unwrap();
    let job_id = queued.steps[0].job_id.clone().unwrap();
    {
        let mut state = service.state.write().await;
        let job = state.jobs.get_mut(&job_id).unwrap();
        job.status = AgentJobStatus::Running;
        job.attempt = 1;
        job.started_at_ms = Some(job.updated_at_ms);
        let mut run = RunRecord::running(
            RunStart {
                agent_id: owner.clone(),
                session_id: "job-session".into(),
                source: RunSource::Job,
                source_ref: Some(format!("{job_id}:1")),
                idempotency_key: None,
                text: "Task".into(),
                model: "test".into(),
                provider: None,
                parent_run_id: None,
            },
            1,
        );
        run.action_receipts.push(ActionReceipt {
            tool_call_id: "call".into(),
            name: "send".into(),
            status: ActionStatus::Started,
            started_at_ms: 1,
            finished_at_ms: None,
            duration_ms: None,
            arguments_preview: String::new(),
            arguments_truncated: false,
            result_preview: None,
            result_truncated: false,
            durability: ActionDurability::Persistent,
        });
        state.runs.insert(run);
    }
    let view = service
        .inspect_workflow_attempt(&owner, &created.id, "first", &job_id)
        .await
        .unwrap();
    let view = serde_json::to_value(view).unwrap();
    assert_eq!(view["evidenceState"], "retained");
    assert_eq!(
        view["run"]["evidence"]["actionReceipts"][0]["status"],
        "started"
    );
}

#[tokio::test]
async fn continuity_claimed_job_may_start_while_pause_blocks_later_admission() {
    let (service, owner, _dir) = setup();
    let created = service.create_workflow(&owner, input()).await.unwrap();
    service.reconcile_workflows().await.unwrap();
    let queued = service.get_workflow(&owner, &created.id).await.unwrap();
    let job_id = queued.steps[0].job_id.clone().unwrap();
    let claimed = job_id.clone();
    service
        .mutate(move |state| {
            let job = state.jobs.get_mut(&claimed).unwrap();
            job.status = AgentJobStatus::Running;
            job.attempt = 1;
            advance(job);
            job.started_at_ms = Some(job.updated_at_ms);
            Ok(())
        })
        .await
        .unwrap();
    service
        .resume_workflow(
            &owner,
            &created.id,
            decision(queued.revision, WorkflowResumeAction::Pause),
        )
        .await
        .unwrap();
    let state = service.state.read().await;
    assert!(
        dispatch_allowed(&state, &state.jobs[&job_id]),
        "the already claimed task retains its admission"
    );
    assert!(!needs_reconciliation(&state, &state.workflows[&created.id]));
    assert_eq!(state.jobs.len(), 1);
}

#[tokio::test]
async fn continuity_pause_survives_a_dropped_waiter_and_replay_is_idempotent() {
    let (service, owner, _dir) = setup();
    let created = service.create_workflow(&owner, input()).await.unwrap();
    let transaction = service.runs.control_plane_transaction().await;
    let request = decision(created.revision, WorkflowResumeAction::Pause);
    let mut pending = Box::pin(service.resume_workflow(&owner, &created.id, request.clone()));
    assert!(futures::poll!(pending.as_mut()).is_pending());
    drop(pending);
    drop(transaction);
    let held = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let saved = service.get_workflow(&owner, &created.id).await.unwrap();
            if saved.paused {
                break saved;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let replay = service
        .resume_workflow(&owner, &created.id, request)
        .await
        .unwrap();
    assert_eq!(replay.revision, held.revision);
    assert_eq!(replay.decisions.len(), 1);
    assert!(service.state.read().await.jobs.is_empty());
}
