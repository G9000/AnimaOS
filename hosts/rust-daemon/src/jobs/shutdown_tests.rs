//! A1: scans blocked on the transaction cannot claim new work after closure.
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

struct ObservedModel {
    calls: Arc<AtomicUsize>,
    entered: Arc<Notify>,
    release: Arc<Notify>,
}
#[async_trait::async_trait]
impl anima_core::ModelAdapter for ObservedModel {
    fn single_attempt_contract(&self) -> bool {
        true
    }
    fn generated_usage_is_complete(&self, _: &anima_core::ModelGenerateResponse) -> bool {
        true
    }
    fn provider(&self) -> &str {
        "shutdown-test"
    }
    async fn generate(
        &self,
        config: &anima_core::AgentConfig,
        request: &anima_core::ModelGenerateRequest,
    ) -> Result<anima_core::ModelGenerateResponse, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
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

fn observed_service() -> (
    JobService,
    String,
    std::path::PathBuf,
    Arc<AtomicUsize>,
    Arc<Notify>,
    Arc<Notify>,
) {
    let calls = Arc::new(AtomicUsize::new(0));
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let path =
        std::env::temp_dir().join(format!("anima-job-shutdown-{}.json", uuid::Uuid::new_v4()));
    let mut state = DaemonState::with_model_adapter(Arc::new(ObservedModel {
        calls: calls.clone(),
        entered: entered.clone(),
        release: release.clone(),
    }));
    state.set_control_plane_store(Some(ControlPlaneStoreConfig::Json(path.clone())));
    let agent = state
        .create_agent(
            serde_json::from_value(serde_json::json!({"name":"worker","model":"deterministic"}))
                .unwrap(),
        )
        .unwrap()
        .state
        .id;
    let state = Arc::new(RwLock::new(state));
    let runs = AgentRunCoordinator::new(state.clone(), Arc::new(Semaphore::new(4)));
    (
        JobService::new(state, runs),
        agent,
        path,
        calls,
        entered,
        release,
    )
}

#[tokio::test]
async fn shutdown_fences_a_scanned_job_waiting_for_its_durable_claim() {
    let (service, agent, path, calls, _, _) = observed_service();
    let job = service
        .create(&agent, "queued", "prompt", "queued")
        .await
        .unwrap();
    let transaction = service.runs.control_plane_transaction().await;
    let (_stop, receiver) = watch::channel(false);
    let dispatch = tokio::spawn({
        let service = service.clone();
        async move { service.dispatch(receiver).await }
    });
    tokio::time::timeout(Duration::from_secs(5), service.claim_ready.notified())
        .await
        .unwrap();
    service.request_shutdown().await;
    assert!(
        matches!(service.start().await, Err(JobError::Unavailable(message)) if message == JOB_PRODUCER_SHUTTING_DOWN)
    );
    drop(transaction);
    tokio::time::timeout(Duration::from_secs(5), dispatch)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let queued = service.state.read().await.jobs[&job.id].clone();
    assert_eq!(queued.status, AgentJobStatus::Queued);
    assert_eq!(queued.attempt, 0);
    assert!(queued.attempts.is_empty());
    let saved = load_control_plane_snapshot(&ControlPlaneStoreConfig::Json(path.clone()))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.jobs[0].attempt, 0);
    assert_eq!(saved.jobs[0].status, AgentJobStatus::Queued);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn shutdown_preserves_an_already_claimed_job_settlement() {
    let (service, agent, path, calls, entered, release) = observed_service();
    let job = service
        .create(&agent, "admitted", "prompt", "admitted")
        .await
        .unwrap();
    service.start().await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), entered.notified())
        .await
        .unwrap();
    assert_eq!(service.state.read().await.jobs[&job.id].attempt, 1);
    service.request_shutdown().await;
    release.notify_one();
    tokio::time::timeout(Duration::from_secs(5), service.shutdown())
        .await
        .unwrap();
    let done = service.state.read().await.jobs[&job.id].clone();
    assert_eq!(done.status, AgentJobStatus::Completed);
    assert_eq!(done.attempt, 1);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let saved = load_control_plane_snapshot(&ControlPlaneStoreConfig::Json(path.clone()))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.jobs[0].status, AgentJobStatus::Completed);
    std::fs::remove_file(path).unwrap();
}
