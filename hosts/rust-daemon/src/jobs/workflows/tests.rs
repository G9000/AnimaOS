use super::*;
use crate::{
    agent_runs::AgentRunCoordinator,
    control_plane_store::{load_control_plane_snapshot, ControlPlaneStoreConfig},
};
use anima_core::workflows::{WorkflowResumeAction, WorkflowStepSpec};
use std::sync::Arc;
use tokio::sync::{RwLock, Semaphore};

fn setup() -> (JobService, String, std::path::PathBuf) {
    setup_with_model(Arc::new(crate::model::DeterministicModelAdapter))
}

fn setup_with_model(
    model: Arc<dyn anima_core::ModelAdapter>,
) -> (JobService, String, std::path::PathBuf) {
    let path = std::env::temp_dir().join(format!("anima-workflows-{}.json", uuid::Uuid::new_v4()));
    let mut state = DaemonState::with_model_adapter(model);
    state.set_control_plane_store(Some(ControlPlaneStoreConfig::Json(path.clone())));
    let owner = state
        .create_agent(
            serde_json::from_value(
                serde_json::json!({"name":"workflow-test","model":"deterministic"}),
            )
            .unwrap(),
        )
        .unwrap()
        .state
        .id;
    let state = Arc::new(RwLock::new(state));
    let runs = AgentRunCoordinator::new(state.clone(), Arc::new(Semaphore::new(4)));
    (JobService::new(state, runs), owner, path)
}

struct CountingModel(Arc<std::sync::atomic::AtomicUsize>);

struct TruncatedModel;

#[async_trait::async_trait]
impl anima_core::ModelAdapter for TruncatedModel {
    fn provider(&self) -> &str {
        "workflow-truncated-test"
    }
    async fn generate(
        &self,
        _config: &anima_core::AgentConfig,
        _request: &anima_core::ModelGenerateRequest,
    ) -> Result<anima_core::ModelGenerateResponse, String> {
        Ok(anima_core::ModelGenerateResponse {
            content: anima_core::Content {
                text: "Short partial answer".into(),
                ..Default::default()
            },
            tool_calls: None,
            usage: Default::default(),
            stop_reason: anima_core::ModelStopReason::MaxTokens,
        })
    }
}

#[tokio::test]
async fn workflows_provider_token_truncation_requires_review_even_for_short_output() {
    let (service, owner, path) = setup_with_model(Arc::new(TruncatedModel));
    let workflow = service.create_workflow(&owner, input()).await.unwrap();
    service.start().await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        loop {
            if service
                .get_workflow(&owner, &workflow.id)
                .await
                .unwrap()
                .status
                != WorkflowStatus::Running
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    service.shutdown().await;
    let reviewed = service.get_workflow(&owner, &workflow.id).await.unwrap();
    assert_eq!(reviewed.status, WorkflowStatus::NeedsReview);
    assert_eq!(reviewed.current_step, 0);
    let state = service.state.read().await;
    assert_eq!(state.jobs.len(), 1);
    let job = &state.jobs[reviewed.steps[0].job_id.as_ref().unwrap()];
    assert!(job.attempts.last().unwrap().result_truncated);
    assert_eq!(job.result.as_deref(), Some("Short partial answer"));
    validate_snapshot(
        &[reviewed],
        &state.jobs.values().cloned().collect::<Vec<_>>(),
    )
    .unwrap();
    drop(state);
    let _ = std::fs::remove_file(path);
}

struct WaitingModel {
    calls: Arc<std::sync::atomic::AtomicUsize>,
    entered: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
}

#[async_trait::async_trait]
impl anima_core::ModelAdapter for WaitingModel {
    fn provider(&self) -> &str {
        "workflow-waiting-test"
    }
    async fn generate(
        &self,
        config: &anima_core::AgentConfig,
        request: &anima_core::ModelGenerateRequest,
    ) -> Result<anima_core::ModelGenerateResponse, String> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.entered.notify_one();
        self.release.notified().await;
        anima_core::ModelAdapter::generate(
            &crate::model::DeterministicModelAdapter,
            config,
            request,
        )
        .await
    }
}

#[async_trait::async_trait]
impl anima_core::ModelAdapter for CountingModel {
    fn provider(&self) -> &str {
        "workflow-counting-test"
    }
    async fn generate(
        &self,
        config: &anima_core::AgentConfig,
        request: &anima_core::ModelGenerateRequest,
    ) -> Result<anima_core::ModelGenerateResponse, String> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        anima_core::ModelAdapter::generate(
            &crate::model::DeterministicModelAdapter,
            config,
            request,
        )
        .await
    }
}

async fn wait_status(
    service: &JobService,
    owner: &str,
    id: &str,
    expected: WorkflowStatus,
) -> WorkflowRun {
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        loop {
            let workflow = service.get_workflow(owner, id).await.unwrap();
            if workflow.status == expected {
                return workflow;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("workflow did not settle within 20 seconds")
}

#[tokio::test]
async fn workflows_dispatcher_restart_invokes_only_unfinished_model_steps() {
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let model = Arc::new(CountingModel(calls.clone()));
    let (service, owner, path) = setup_with_model(model.clone());
    let workflow = service.create_workflow(&owner, input()).await.unwrap();
    service.start().await.unwrap();
    let paused = wait_status(
        &service,
        &owner,
        &workflow.id,
        WorkflowStatus::AwaitingApproval,
    )
    .await;
    service.shutdown().await;
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    let first = paused.steps[0].clone();
    let snapshot = load_control_plane_snapshot(&ControlPlaneStoreConfig::Json(path.clone()))
        .await
        .unwrap()
        .unwrap();
    let mut restored = DaemonState::with_model_adapter(model);
    restored.set_control_plane_store(Some(ControlPlaneStoreConfig::Json(path.clone())));
    restored.restore_control_plane_snapshot(snapshot).unwrap();
    let state = Arc::new(RwLock::new(restored));
    let runs = AgentRunCoordinator::new(state.clone(), Arc::new(Semaphore::new(4)));
    let service = JobService::new(state, runs);
    service
        .resume_workflow(
            &owner,
            &workflow.id,
            WorkflowResumeRequest {
                revision: paused.revision,
                action: WorkflowResumeAction::Approve,
                value: None,
                acknowledge_uncertain: false,
            },
        )
        .await
        .unwrap();
    service.start().await.unwrap();
    let completed = wait_status(&service, &owner, &workflow.id, WorkflowStatus::Completed).await;
    service.shutdown().await;
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    assert_eq!(
        completed.steps[0], first,
        "saved output and first job must remain unchanged"
    );
    let state = service.state.read().await;
    assert_eq!(state.jobs.len(), 2);
    validate_snapshot(
        &[completed],
        &state.jobs.values().cloned().collect::<Vec<_>>(),
    )
    .unwrap();
    drop(state);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn workflows_cancel_after_claim_prevents_model_admission_and_replay_preserves_marker() {
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let (service, owner, path) = setup_with_model(Arc::new(CountingModel(calls.clone())));
    let workflow = service.create_workflow(&owner, input()).await.unwrap();
    service.reconcile_workflows().await.unwrap();
    let active = service.get_workflow(&owner, &workflow.id).await.unwrap();
    let id = active.steps[0].job_id.clone().unwrap();
    let ticket = service
        .runs
        .try_ticket(&owner, &format!("job:{id}"))
        .await
        .unwrap();
    let job = service
        .mutate(move |state| {
            let job = state.jobs.get_mut(&id).unwrap();
            job.status = AgentJobStatus::Running;
            job.attempt = 1;
            advance(job);
            job.started_at_ms = Some(job.updated_at_ms);
            Ok(job.clone())
        })
        .await
        .unwrap();
    service
        .cancel_workflow(&owner, &workflow.id, active.revision)
        .await
        .unwrap();
    let marker = service.state.read().await.jobs[&job.id].stop_requested_at_ms;
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    service
        .cancel_workflow(&owner, &workflow.id, active.revision)
        .await
        .unwrap();
    assert_eq!(
        service.state.read().await.jobs[&job.id].stop_requested_at_ms,
        marker
    );
    service.execute(job, ticket).await;
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert!(service.state.read().await.runs.for_agent(&owner).is_empty());
    assert_eq!(service.state.read().await.jobs.len(), 1);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn workflows_cancel_survives_dropped_request_and_stops_an_active_run() {
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let (service, owner, path) = setup_with_model(Arc::new(WaitingModel {
        calls: calls.clone(),
        entered: entered.clone(),
        release: release.clone(),
    }));
    let workflow = service.create_workflow(&owner, input()).await.unwrap();
    service.start().await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(20), entered.notified())
        .await
        .unwrap();
    let active = service.get_workflow(&owner, &workflow.id).await.unwrap();
    let transaction = service.runs.control_plane_transaction().await;
    let mut cancelling = Box::pin(service.cancel_workflow(&owner, &workflow.id, active.revision));
    assert!(futures::poll!(cancelling.as_mut()).is_pending());
    drop(cancelling);
    drop(transaction);
    wait_status(&service, &owner, &workflow.id, WorkflowStatus::Cancelled).await;
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        loop {
            if service
                .state
                .read()
                .await
                .runs
                .for_agent(&owner)
                .iter()
                .any(|run| run.stop.is_some())
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("active native run must receive a saved stop even after caller drops");
    release.notify_one();
    tokio::time::timeout(std::time::Duration::from_secs(20), service.shutdown())
        .await
        .unwrap();
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    let snapshot = service.state.read().await.control_plane_snapshot();
    assert_eq!(snapshot.jobs.len(), 1);
    assert_eq!(snapshot.workflows[0].status, WorkflowStatus::Cancelled);
    validate_snapshot(&snapshot.workflows, &snapshot.jobs).unwrap();
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn workflows_stop_reconciler_ignores_a_cancelled_snapshot_that_rolls_back() {
    use crate::runs::{RunRecord, RunStart};
    let (service, owner, path) = setup();
    let workflow = service.create_workflow(&owner, input()).await.unwrap();
    service.reconcile_workflows().await.unwrap();
    let original = service.get_workflow(&owner, &workflow.id).await.unwrap();
    let job_id = original.steps[0].job_id.clone().unwrap();
    let transaction = service.runs.control_plane_transaction().await;
    let run_id = {
        let mut state = service.state.write().await;
        let job = state.jobs.get_mut(&job_id).unwrap();
        job.status = AgentJobStatus::Running;
        job.attempt = 1;
        job.started_at_ms = Some(job.updated_at_ms);
        let record = RunRecord::running(
            RunStart {
                agent_id: owner.clone(),
                session_id: "rollback-session".into(),
                source: RunSource::Job,
                source_ref: Some(format!("{job_id}:1")),
                idempotency_key: None,
                text: "test".into(),
                model: "deterministic".into(),
                provider: None,
                parent_run_id: None,
            },
            now_ms(),
        );
        let id = record.id.clone();
        state.runs.insert(record);
        state
            .workflows
            .get_mut(&workflow.id)
            .unwrap()
            .cancel(original.revision, now_ms().max(original.updated_at_ms))
            .unwrap();
        id
    };
    // Expose exactly the provisional interval while a cancellation save holds
    // the transaction, then model its save-failure rollback before release.
    let mut reconciling = Box::pin(service.stop_cancelled_workflow_runs());
    assert!(futures::poll!(reconciling.as_mut()).is_pending());
    service
        .state
        .write()
        .await
        .workflows
        .insert(workflow.id, original);
    drop(transaction);
    reconciling.await;
    let state = service.state.read().await;
    assert!(
        state.runs.get(&run_id).unwrap().stop.is_none(),
        "an undurable cancellation must not stop the run after rollback"
    );
    assert!(state.jobs[&job_id].stop_requested_at_ms.is_none());
    drop(state);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn workflows_blank_failure_and_clock_correction_reach_review_and_owner_checkpoint() {
    let (service, owner, path) = setup();
    let workflow = service.create_workflow(&owner, input()).await.unwrap();
    service.reconcile_workflows().await.unwrap();
    let active = service.get_workflow(&owner, &workflow.id).await.unwrap();
    settle(
        &service,
        active.steps[0].job_id.as_ref().unwrap(),
        AgentJobStatus::Failed,
        "  ",
    )
    .await;
    service
        .reconcile_workflows()
        .await
        .expect("blank failure must still reach review");
    let future = now_ms() + 60_000;
    service
        .state
        .write()
        .await
        .workflows
        .get_mut(&workflow.id)
        .unwrap()
        .updated_at_ms = future;
    let reviewed = service.get_workflow(&owner, &workflow.id).await.unwrap();
    assert_eq!(reviewed.status, WorkflowStatus::NeedsReview);
    assert!(!reviewed.error.as_ref().unwrap().trim().is_empty());
    let resolved = service
        .resume_workflow(
            &owner,
            &workflow.id,
            WorkflowResumeRequest {
                revision: reviewed.revision,
                action: WorkflowResumeAction::Resolve,
                value: Some("Owner verified result".into()),
                acknowledge_uncertain: true,
            },
        )
        .await
        .expect("clock correction must not block owner resolution");
    assert!(resolved.updated_at_ms >= future);
    service
        .cancel_workflow(&owner, &workflow.id, resolved.revision)
        .await
        .expect("clock correction must not block cancellation");
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn workflows_restore_rejects_resolution_from_another_step_and_live_historical_attempts() {
    let (service, owner, path) = setup();
    let mut definition = input();
    definition.steps.remove(1);
    let workflow = service.create_workflow(&owner, definition).await.unwrap();
    service.reconcile_workflows().await.unwrap();
    let active = service.get_workflow(&owner, &workflow.id).await.unwrap();
    let first_id = active.steps[0].job_id.clone().unwrap();
    settle(
        &service,
        &first_id,
        AgentJobStatus::Failed,
        "Unknown effect",
    )
    .await;
    service.reconcile_workflows().await.unwrap();
    let reviewed = service.get_workflow(&owner, &workflow.id).await.unwrap();
    service
        .resume_workflow(
            &owner,
            &workflow.id,
            WorkflowResumeRequest {
                revision: reviewed.revision,
                action: WorkflowResumeAction::Resolve,
                value: Some("done".into()),
                acknowledge_uncertain: true,
            },
        )
        .await
        .unwrap();
    service.reconcile_workflows().await.unwrap();
    let second = service.get_workflow(&owner, &workflow.id).await.unwrap();
    let second_id = second.steps[1].job_id.clone().unwrap();
    settle(
        &service,
        &second_id,
        AgentJobStatus::Failed,
        "Unknown effect",
    )
    .await;
    let mut forged = service.get_workflow(&owner, &workflow.id).await.unwrap();
    forged
        .complete_task("done", now_ms().max(forged.updated_at_ms))
        .unwrap();
    let jobs = service
        .state
        .read()
        .await
        .jobs
        .values()
        .cloned()
        .collect::<Vec<_>>();
    assert!(
        validate_snapshot(&[forged], &jobs).is_err(),
        "step zero's resolution cannot prove step one's completion"
    );
    let mut cancelled = second;
    cancelled
        .cancel(cancelled.revision, now_ms().max(cancelled.updated_at_ms))
        .unwrap();
    let mut live_jobs = jobs.clone();
    live_jobs
        .iter_mut()
        .find(|j| j.id == second_id)
        .unwrap()
        .status = AgentJobStatus::Queued;
    assert!(
        validate_snapshot(&[cancelled], &live_jobs).is_err(),
        "cancelled workflow cannot retain queued jobs"
    );
    live_jobs
        .iter_mut()
        .find(|j| j.id == second_id)
        .unwrap()
        .status = AgentJobStatus::Failed;
    live_jobs
        .iter_mut()
        .find(|j| j.id == first_id)
        .unwrap()
        .status = AgentJobStatus::Running;
    assert!(
        validate_snapshot(
            &[service.get_workflow(&owner, &workflow.id).await.unwrap()],
            &live_jobs
        )
        .is_err(),
        "historical steps cannot remain active"
    );
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn workflows_tools_accept_granted_ordinary_sources_and_reject_system_or_stale_runs() {
    use crate::runs::{RunRecord, RunStart};
    let (service, owner, path) = setup();
    let mut state = service.state.write().await;
    state.update_agent(&owner, serde_json::from_value(serde_json::json!({"tools":[crate::tools::ToolRegistry::new().descriptor("create_workflow").unwrap()]})).unwrap()).unwrap();
    for source in [
        RunSource::Web,
        RunSource::Api,
        RunSource::Telegram,
        RunSource::Discord,
        RunSource::Job,
        RunSource::Schedule,
        RunSource::Delegation,
        RunSource::Peer,
    ] {
        let record = RunRecord::running(
            RunStart {
                agent_id: owner.clone(),
                session_id: "workflow-session".into(),
                source,
                source_ref: None,
                idempotency_key: None,
                text: "start workflow".into(),
                model: "deterministic".into(),
                provider: None,
                parent_run_id: None,
            },
            now_ms(),
        );
        let link = RunLink {
            agent_id: owner.clone(),
            session_id: record.session_id.clone(),
            run_id: record.id.clone(),
        };
        state.runs.insert(record);
        assert_eq!(
            check_caller(&state, &link, "create_workflow").is_ok(),
            matches!(
                source,
                RunSource::Web | RunSource::Api | RunSource::Telegram | RunSource::Discord
            ),
            "source {source:?}"
        );
        state.runs.get_mut(&link.run_id).unwrap().status = RunStatus::Completed;
        assert!(check_caller(&state, &link, "create_workflow").is_err());
    }
    drop(state);
    let _ = std::fs::remove_file(path);
}

fn input() -> WorkflowInput {
    WorkflowInput {
        title: "Reviewed report".into(),
        request_key: "report-key".into(),
        steps: vec![
            WorkflowStepSpec {
                id: "research".into(),
                title: "Research".into(),
                kind: WorkflowStepKind::Task,
                prompt: "Gather the report facts".into(),
            },
            WorkflowStepSpec {
                id: "review".into(),
                title: "Review".into(),
                kind: WorkflowStepKind::Approval,
                prompt: "Approve these facts".into(),
            },
            WorkflowStepSpec {
                id: "write".into(),
                title: "Write".into(),
                kind: WorkflowStepKind::Task,
                prompt: "Write the reviewed report".into(),
            },
        ],
    }
}

async fn settle(service: &JobService, id: &str, status: AgentJobStatus, text: &str) {
    let id = id.to_string();
    let text = text.to_string();
    service
        .mutate(move |state| {
            let job = state.jobs.get_mut(&id).unwrap();
            job.status = status;
            job.attempt = 1;
            job.started_at_ms = Some(job.created_at_ms);
            advance(job);
            job.finished_at_ms = Some(job.updated_at_ms);
            if status == AgentJobStatus::Completed {
                job.result = Some(text);
            } else {
                job.error = Some(text);
            }
            job.preserve_legacy_attempt();
            Ok(())
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn workflows_saved_step_is_never_repeated_after_approval_and_restart() {
    let (service, owner, path) = setup();
    let workflow = service.create_workflow(&owner, input()).await.unwrap();
    service.reconcile_workflows().await.unwrap();
    let first = service.get_workflow(&owner, &workflow.id).await.unwrap();
    let first_job = first.steps[0].job_id.clone().unwrap();
    settle(
        &service,
        &first_job,
        AgentJobStatus::Completed,
        "Exact saved research",
    )
    .await;
    service.reconcile_workflows().await.unwrap();
    let paused = service.get_workflow(&owner, &workflow.id).await.unwrap();
    assert_eq!(paused.status, WorkflowStatus::AwaitingApproval);
    assert_eq!(service.state.read().await.jobs.len(), 1);
    let snapshot = load_control_plane_snapshot(&ControlPlaneStoreConfig::Json(path.clone()))
        .await
        .unwrap()
        .unwrap();
    validate_snapshot(&snapshot.workflows, &snapshot.jobs).unwrap();
    let mut restored = DaemonState::new();
    restored.set_control_plane_store(Some(ControlPlaneStoreConfig::Json(path.clone())));
    restored.restore_control_plane_snapshot(snapshot).unwrap();
    let state = Arc::new(RwLock::new(restored));
    let runs = AgentRunCoordinator::new(state.clone(), Arc::new(Semaphore::new(4)));
    let service = JobService::new(state, runs);
    service
        .resume_workflow(
            &owner,
            &workflow.id,
            WorkflowResumeRequest {
                revision: paused.revision,
                action: WorkflowResumeAction::Approve,
                value: None,
                acknowledge_uncertain: false,
            },
        )
        .await
        .unwrap();
    service.reconcile_workflows().await.unwrap();
    service.reconcile_workflows().await.unwrap();
    let current = service.get_workflow(&owner, &workflow.id).await.unwrap();
    assert_eq!(current.current_step, 2);
    assert_eq!(current.steps[0].attempts, vec![first_job.clone()]);
    assert_eq!(
        current.steps[0].result.as_deref(),
        Some("Exact saved research")
    );
    assert_eq!(service.state.read().await.jobs.len(), 2);
    assert!(
        service.state.read().await.jobs[current.steps[2].job_id.as_ref().unwrap()]
            .prompt
            .contains("Exact saved research")
    );
    let job_revision = service.state.read().await.jobs[&first_job].revision;
    assert!(service
        .retry(&owner, &first_job, job_revision, true)
        .await
        .is_err());
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn workflows_uncertain_attempt_requires_owner_review_and_has_one_job_attempt() {
    let (service, owner, path) = setup();
    let workflow = service.create_workflow(&owner, input()).await.unwrap();
    service.reconcile_workflows().await.unwrap();
    let active = service.get_workflow(&owner, &workflow.id).await.unwrap();
    let first_job = active.steps[0].job_id.clone().unwrap();
    settle(
        &service,
        &first_job,
        AgentJobStatus::NeedsReview,
        "Effect outcome unknown",
    )
    .await;
    service.reconcile_workflows().await.unwrap();
    let reviewed = service.get_workflow(&owner, &workflow.id).await.unwrap();
    assert_eq!(reviewed.status, WorkflowStatus::NeedsReview);
    assert_eq!(service.state.read().await.jobs.len(), 1);
    assert!(service
        .resume_workflow(
            &owner,
            &workflow.id,
            WorkflowResumeRequest {
                revision: reviewed.revision,
                action: WorkflowResumeAction::Retry,
                value: None,
                acknowledge_uncertain: false
            }
        )
        .await
        .is_err());
    service
        .resume_workflow(
            &owner,
            &workflow.id,
            WorkflowResumeRequest {
                revision: reviewed.revision,
                action: WorkflowResumeAction::Retry,
                value: None,
                acknowledge_uncertain: true,
            },
        )
        .await
        .unwrap();
    service.reconcile_workflows().await.unwrap();
    let retried = service.get_workflow(&owner, &workflow.id).await.unwrap();
    assert_ne!(retried.steps[0].job_id.as_deref(), Some(first_job.as_str()));
    assert_eq!(retried.steps[0].attempts.len(), 2);
    assert!(service
        .state
        .read()
        .await
        .jobs
        .values()
        .all(|job| job.max_attempts == 1));
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn workflows_failed_save_rolls_back_step_link_and_queued_job_together() {
    let (service, owner, path) = setup();
    let workflow = service.create_workflow(&owner, input()).await.unwrap();
    let original = std::fs::read(&path).unwrap();
    let blocked = path.with_extension("blocked");
    std::fs::create_dir(&blocked).unwrap();
    service
        .state
        .write()
        .await
        .set_control_plane_store(Some(ControlPlaneStoreConfig::Json(blocked.clone())));
    assert!(service.reconcile_workflows().await.is_err());
    assert_eq!(
        service.get_workflow(&owner, &workflow.id).await.unwrap(),
        workflow
    );
    assert!(service.state.read().await.jobs.is_empty());
    assert_eq!(std::fs::read(&path).unwrap(), original);
    let _ = std::fs::remove_dir(blocked);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn workflows_cancel_queued_step_prevents_dispatch_and_preserves_history() {
    let (service, owner, path) = setup();
    let workflow = service.create_workflow(&owner, input()).await.unwrap();
    service.reconcile_workflows().await.unwrap();
    let active = service.get_workflow(&owner, &workflow.id).await.unwrap();
    let cancelled = service
        .cancel_workflow(&owner, &workflow.id, active.revision)
        .await
        .unwrap();
    assert_eq!(cancelled.status, WorkflowStatus::Cancelled);
    assert_eq!(
        service
            .cancel_workflow(&owner, &workflow.id, active.revision)
            .await
            .unwrap(),
        cancelled
    );
    let state = service.state.read().await;
    let job = &state.jobs[active.steps[0].job_id.as_ref().unwrap()];
    assert_eq!(job.status, AgentJobStatus::Cancelled);
    assert!(!dispatch_allowed(&state, job));
    validate_snapshot(
        &[cancelled],
        &state.jobs.values().cloned().collect::<Vec<_>>(),
    )
    .unwrap();
    drop(state);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn workflows_deleted_agent_history_restores_without_admitting_work() {
    let (service, owner, path) = setup();
    let workflow = service.create_workflow(&owner, input()).await.unwrap();
    service.reconcile_workflows().await.unwrap();
    service.state.write().await.remove_agent(&owner);
    let snapshot = service.state.read().await.control_plane_snapshot();
    let mut restored = DaemonState::new();
    restored.restore_control_plane_snapshot(snapshot).unwrap();
    assert!(restored.workflows.contains_key(&workflow.id));
    assert!(!needs_reconciliation(
        &restored,
        &restored.workflows[&workflow.id]
    ));
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn workflows_restore_rejects_queued_stop_but_recovers_a_running_owner_stop() {
    let (service, owner, path) = setup();
    let workflow = service.create_workflow(&owner, input()).await.unwrap();
    service.reconcile_workflows().await.unwrap();
    let mut snapshot = service.state.read().await.control_plane_snapshot();
    snapshot.jobs[0].stop_requested_at_ms = Some(snapshot.jobs[0].updated_at_ms);
    assert!(validate_snapshot(&snapshot.workflows, &snapshot.jobs).is_err());
    snapshot.jobs[0].status = AgentJobStatus::Running;
    snapshot.jobs[0].attempt = 1;
    snapshot.jobs[0].started_at_ms = Some(snapshot.jobs[0].updated_at_ms);
    validate_snapshot(&snapshot.workflows, &snapshot.jobs).unwrap();
    let job_id = snapshot.jobs[0].id.clone();
    let mut restored = DaemonState::new();
    restored.set_control_plane_store(Some(ControlPlaneStoreConfig::Json(path.clone())));
    restored
        .restore_control_plane_snapshot(snapshot)
        .expect("an owner stop saved before the run settles must restore");
    assert_eq!(restored.jobs[&job_id].status, AgentJobStatus::Running);
    let state = Arc::new(RwLock::new(restored));
    let runs = AgentRunCoordinator::new(state.clone(), Arc::new(Semaphore::new(4)));
    let service = JobService::new(state, runs);
    service.start().await.unwrap();
    wait_status(&service, &owner, &workflow.id, WorkflowStatus::NeedsReview).await;
    service.shutdown().await;
    assert_eq!(
        service.state.read().await.jobs[&job_id].status,
        AgentJobStatus::NeedsReview
    );
    let _ = std::fs::remove_file(path);
}
