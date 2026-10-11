//! A1: a due scan may wait, but closure fences the subsequent durable claim.
use super::tests::{due_schedule, service_with_daemon, GatedModel};
use super::*;
use crate::control_plane_store::{load_control_plane_snapshot, ControlPlaneStoreConfig};
use crate::state::DaemonState;
use tokio::sync::{Notify, Semaphore};

async fn fixture() -> (
    SchedulerService,
    SharedDaemonState,
    String,
    std::path::PathBuf,
    Arc<Semaphore>,
    Arc<Semaphore>,
) {
    let entered = Arc::new(Semaphore::new(0));
    let release = Arc::new(Semaphore::new(0));
    let mut daemon = DaemonState::with_model_adapter(Arc::new(GatedModel {
        entered: entered.clone(),
        release: release.clone(),
    }));
    let path = std::env::temp_dir().join(format!(
        "anima-schedule-shutdown-{}.json",
        uuid::Uuid::new_v4()
    ));
    daemon.set_control_plane_store(Some(ControlPlaneStoreConfig::Json(path.clone())));
    let (service, state, agent, _) = service_with_daemon(daemon);
    (service, state, agent, path, entered, release)
}

#[tokio::test]
async fn shutdown_fences_a_due_tick_waiting_for_its_durable_claim() {
    let (service, state, agent, path, model_entered, _) = fixture().await;
    let record = due_schedule(&service, &agent).await;
    let scanned = Arc::new(Notify::new());
    let resume = Arc::new(Notify::new());
    *service.inner.claim_gate.lock().unwrap() = Some((scanned.clone(), resume.clone()));
    let tick = tokio::spawn({
        let service = service.clone();
        async move { service.tick_at(now_ms()).await }
    });
    tokio::time::timeout(Duration::from_secs(5), scanned.notified())
        .await
        .unwrap();
    let transaction = service.inner.runs.control_plane_transaction().await;
    resume.notify_one();
    service.request_shutdown().await;
    drop(transaction);
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), tick)
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
        0
    );
    assert_eq!(
        service.run_now(&agent, &record.id).await.unwrap_err(),
        ScheduleError::Conflict(SCHEDULE_PRODUCER_SHUTTING_DOWN)
    );
    assert_eq!(model_entered.available_permits(), 0);
    assert!(state.read().await.schedules[&record.id]
        .last_fired
        .is_none());
    let saved = load_control_plane_snapshot(&ControlPlaneStoreConfig::Json(path.clone()))
        .await
        .unwrap()
        .unwrap();
    assert!(saved.schedules[0].last_fired.is_none());
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn shutdown_preserves_an_already_claimed_schedule_settlement() {
    let (service, state, agent, path, entered, release) = fixture().await;
    let record = due_schedule(&service, &agent).await;
    let tick = tokio::spawn({
        let service = service.clone();
        async move { service.tick_at(now_ms()).await }
    });
    tokio::time::timeout(Duration::from_secs(5), entered.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    assert!(state.read().await.schedules[&record.id]
        .last_fired
        .is_some());
    service.request_shutdown().await;
    release.add_permits(1);
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), tick)
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
        1
    );
    assert!(state.read().await.schedules[&record.id]
        .last_safe_outcome
        .is_some());
    let saved = load_control_plane_snapshot(&ControlPlaneStoreConfig::Json(path.clone()))
        .await
        .unwrap()
        .unwrap();
    assert!(saved.schedules[0].last_safe_outcome.is_some());
    std::fs::remove_file(path).unwrap();
}
