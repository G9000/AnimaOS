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

#[tokio::test]
async fn persisted_unfinished_claim_recovers_in_fresh_states_without_replay() {
    use crate::connectors::credentials::InMemoryCredentialStore;
    use crate::control_plane_store::ControlPlaneSnapshot;
    use futures::FutureExt;
    use std::panic::AssertUnwindSafe;
    use tokio::sync::RwLock;

    struct FixtureFile(std::path::PathBuf);
    impl Drop for FixtureFile {
        fn drop(&mut self) {
            // Only this test's exact synthetic JSON file, including on assertion failure.
            let _ = std::fs::remove_file(&self.0);
        }
    }

    fn restore(
        snapshot: ControlPlaneSnapshot,
        store: &ControlPlaneStoreConfig,
        entered: Arc<Semaphore>,
        release: Arc<Semaphore>,
    ) -> (SchedulerService, SharedDaemonState) {
        let mut daemon = DaemonState::with_model_adapter(Arc::new(GatedModel { entered, release }));
        daemon.set_control_plane_store(Some(store.clone()));
        daemon.restore_control_plane_snapshot(snapshot).unwrap();
        // The sibling helper creates another owner. Build only its service wiring
        // here so these fresh states retain exactly the owner loaded from disk.
        let state = Arc::new(RwLock::new(daemon));
        let runs = AgentRunCoordinator::new(state.clone(), Arc::new(Semaphore::new(2)));
        let connectors = ConnectorManager::new(
            state.clone(),
            runs.clone(),
            Arc::new(InMemoryCredentialStore::default()),
            Arc::new(super::tests::NoopTelegram),
        );
        (
            SchedulerService::new(state.clone(), runs, connectors),
            state,
        )
    }

    async fn settle_owned(service: &SchedulerService, release: &Semaphore) -> bool {
        // A regression that accidentally enters the fake model must not strand
        // cleanup. No real provider exists in any of the three daemon states.
        release.add_permits(MAX_ACTIVE_SCHEDULES);
        let budget = Duration::from_secs(5);
        let mut clean = tokio::time::timeout(budget, service.request_shutdown())
            .await
            .is_ok();
        if let Ok(mut worker) = tokio::time::timeout(budget, service.worker.lock()).await {
            if let Some(mut worker) = worker.take() {
                let _ = worker.cancel.send(true);
                match tokio::time::timeout(budget, &mut worker.join).await {
                    Ok(result) => clean &= result.is_ok(),
                    Err(_) => {
                        clean = false;
                        worker.join.abort();
                        let _ = tokio::time::timeout(budget, &mut worker.join).await;
                    }
                }
            }
        } else {
            clean = false;
        }
        if let Ok(mut jobs) = tokio::time::timeout(budget, service.inner.jobs.lock()).await {
            for (_, mut job) in std::mem::take(&mut *jobs) {
                match tokio::time::timeout(budget, &mut job).await {
                    Ok(result) => clean &= result.is_ok(),
                    Err(_) => {
                        clean = false;
                        job.abort();
                        let _ = tokio::time::timeout(budget, &mut job).await;
                    }
                }
            }
        } else {
            clean = false;
        }
        clean
    }

    let (original, original_state, agent, path, original_calls, original_release) = fixture().await;
    let _file = FixtureFile(path.clone());
    let store = ControlPlaneStoreConfig::Json(path);
    let mut owned = vec![(original.clone(), original_release, original_calls.clone())];
    let result = AssertUnwindSafe(tokio::time::timeout(Duration::from_secs(15), async {
        let initial = due_schedule(&original, &agent).await;
        let declared_t = initial.next_due_at_ms;
        let expected_due = declared_t.checked_add(60_000).unwrap();
        let occurrence_id = format!("schedule:{}:{declared_t}", initial.id);
        assert_eq!(
            initial.trigger,
            ScheduleTrigger::Interval {
                interval_ms: 60_000
            }
        );
        let claimed = claim_due(&original.inner, &initial.id, declared_t)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(original_calls.available_permits(), 0);
        let unresolved = load_control_plane_snapshot(&store).await.unwrap().unwrap();
        assert_eq!(unresolved.agents.len(), 1);
        assert_eq!(unresolved.agents[0].state.id, agent);
        assert_eq!(unresolved.schedules, vec![claimed.clone()]);
        assert_eq!(claimed.agent_id, agent);
        assert_eq!(claimed.id, initial.id);
        assert!(claimed.enabled);
        assert!(claimed.last_safe_outcome.is_none());
        assert_eq!(claimed.next_due_at_ms, expected_due);
        assert_eq!(
            claimed.last_fired,
            Some(ScheduleLastFired {
                fired_at_ms: declared_t,
                run_idempotency_key: occurrence_id.clone(),
                manual: false,
            })
        );
        assert!(unresolved.runs.is_empty());
        assert!(unresolved.schedule_fires.is_empty());
        assert!(unresolved.agents[0].messages.is_empty());
        original.shutdown().await;
        assert_eq!(original_state.read().await.schedules[&initial.id], claimed);

        let restored_calls = Arc::new(Semaphore::new(0));
        let restored_release = Arc::new(Semaphore::new(0));
        let (restored, restored_state) = restore(
            unresolved,
            &store,
            restored_calls.clone(),
            restored_release.clone(),
        );
        owned.push((restored.clone(), restored_release, restored_calls.clone()));
        assert!(!Arc::ptr_eq(&original_state, &restored_state));
        assert_eq!(restored_state.read().await.schedules[&initial.id], claimed);
        restored.start().await;
        // Observe the actual startup worker, with an absolute test budget and
        // no test-driven cadence sleep or manual reconciliation shortcut.
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if !restored_state.read().await.schedules[&initial.id].enabled {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("startup must reconcile the persisted unresolved claim");
        // The state flips before its async save; joining its transaction proves
        // the durable reconciliation finished before inspecting disk.
        drop(restored.inner.runs.control_plane_transaction().await);
        let reconciled = restored_state.read().await.schedules[&initial.id].clone();
        assert!(!reconciled.enabled);
        assert_eq!(reconciled.id, initial.id);
        assert_eq!(reconciled.agent_id, agent);
        assert_eq!(reconciled.last_fired, claimed.last_fired);
        assert_eq!(reconciled.next_due_at_ms, expected_due);
        let outcome = reconciled.last_safe_outcome.as_ref().unwrap();
        assert_eq!(outcome.status, ScheduleOutcomeStatus::Failed);
        assert_eq!(
            outcome.error_code.as_deref(),
            Some("schedule_run_interrupted")
        );
        assert!(outcome.occurred_at_ms >= declared_t);
        assert_eq!(
            reconciled.counters,
            AutomationCounters {
                runs: 1,
                failures: 1,
                consecutive_failures: 1
            }
        );
        let fires = restored_state.read().await.schedule_fires.snapshot();
        assert_eq!(fires.len(), 1);
        assert_eq!(fires[0].id, occurrence_id);
        assert_eq!(fires[0].schedule_id, initial.id);
        assert_eq!(fires[0].agent_id, agent);
        assert_eq!(fires[0].fired_at_ms, declared_t);
        assert_eq!(fires[0].finished_at_ms, outcome.occurred_at_ms);
        assert_eq!(fires[0].outcome, ScheduleOutcomeStatus::Failed);
        assert_eq!(
            fires[0].error_code.as_deref(),
            Some("schedule_run_interrupted")
        );
        assert!(!fires[0].manual);
        assert!(fires[0].run_id.is_none());
        assert!(fires[0].session_id.is_none());
        assert_eq!(restored.tick_at(expected_due + 60_000).await.unwrap(), 0);
        assert_eq!(restored_calls.available_permits(), 0);
        // Keep join ownership in the service until the unconditional cleanup
        // below, even if a later assertion or the acceptance deadline fails.
        restored.request_shutdown().await;
        let memory = restored_state.read().await.control_plane_snapshot();
        let saved = load_control_plane_snapshot(&store).await.unwrap().unwrap();
        assert_eq!(saved.schedules, vec![reconciled.clone()]);
        assert_eq!(saved.schedule_fires, fires);
        assert_eq!(
            serde_json::to_value(&saved).unwrap(),
            serde_json::to_value(&memory).unwrap()
        );
        assert!(saved.runs.is_empty());
        assert!(saved.outbound.is_empty());
        assert!(saved.discord_outbound.is_empty());
        assert!(saved.agents.iter().all(|a| a.messages.is_empty()));

        let second_calls = Arc::new(Semaphore::new(0));
        let second_release = Arc::new(Semaphore::new(0));
        let (second, second_state) =
            restore(saved, &store, second_calls.clone(), second_release.clone());
        owned.push((second.clone(), second_release, second_calls.clone()));
        assert!(!Arc::ptr_eq(&restored_state, &second_state));
        assert!(!Arc::ptr_eq(&original_state, &second_state));
        second.start().await;
        assert_eq!(second.tick_at(expected_due + 120_000).await.unwrap(), 0);
        second.request_shutdown().await;
        assert_eq!(second_calls.available_permits(), 0);
        assert_eq!(original_calls.available_permits(), 0);
        let second_memory = second_state.read().await.control_plane_snapshot();
        assert_eq!(second_memory.agents.len(), 1);
        assert_eq!(second_memory.agents[0].state.id, agent);
        assert!(second_memory.agents[0].messages.is_empty());
        assert_eq!(second_memory.schedules, vec![reconciled]);
        assert_eq!(second_memory.schedule_fires, fires);
        assert!(second_memory.runs.is_empty());
        assert!(second_memory.outbound.is_empty());
        let second_disk = load_control_plane_snapshot(&store).await.unwrap().unwrap();
        assert_eq!(
            serde_json::to_value(&second_disk).unwrap(),
            serde_json::to_value(&memory).unwrap()
        );
        assert_eq!(
            serde_json::to_value(second_memory).unwrap(),
            serde_json::to_value(memory).unwrap()
        );
    }))
    .catch_unwind()
    .await;

    let mut clean = true;
    for (service, release, _) in &owned {
        clean &= settle_owned(service, release).await;
    }
    assert!(
        clean,
        "all owned scheduler tasks must settle within cleanup bounds"
    );
    // A call admitted before shutdown might enter while cleanup releases its
    // gate. Check every state's counter only after all owned tasks have joined.
    for (_, _, calls) in &owned {
        assert_eq!(
            calls.available_permits(),
            0,
            "recovery must never invoke a model, including during shutdown"
        );
    }
    match result {
        Ok(result) => result.expect("persisted recovery acceptance must finish within 15 seconds"),
        Err(panic) => std::panic::resume_unwind(panic),
    }
}
