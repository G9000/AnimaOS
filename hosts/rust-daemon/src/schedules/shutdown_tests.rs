//! A1: a due scan may wait, but closure fences the subsequent durable claim.
use super::tests::{due_schedule, service_with_daemon, GatedModel};
use super::*;
use crate::connectors::credentials::InMemoryCredentialStore;
use crate::control_plane_store::{
    load_control_plane_snapshot, ControlPlaneSnapshot, ControlPlaneStoreConfig,
};
use crate::state::DaemonState;
use futures::FutureExt;
use std::panic::AssertUnwindSafe;
use tokio::sync::{Notify, RwLock, Semaphore};

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
    // cleanup. No real provider exists in any of the daemon states.
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

#[tokio::test]
async fn persisted_interval_outage_collapses_missed_occurrences_without_catch_up() {
    const INTERVAL_MS: u64 = 60_000;
    const OUTAGE_INTERVALS: u64 = 5;

    let (original, original_state, agent, path, original_calls, original_release) = fixture().await;
    let _file = FixtureFile(path.clone());
    let store = ControlPlaneStoreConfig::Json(path);
    let first_calls = Arc::new(Semaphore::new(0));
    let first_release = Arc::new(Semaphore::new(0));
    let second_calls = Arc::new(Semaphore::new(0));
    let second_release = Arc::new(Semaphore::new(0));
    let mut owned = vec![(original.clone(), original_release, original_calls.clone())];
    // Keep tick ownership outside the acceptance future, including if it
    // panics or times out while a model gate is held.
    let mut ticks: Vec<Option<JoinHandle<Result<usize, ScheduleError>>>> = Vec::new();
    let mut observed_calls = 0;
    let result = AssertUnwindSafe(tokio::time::timeout(Duration::from_secs(15), async {
        let initial = due_schedule(&original, &agent).await;
        let due_t = initial.next_due_at_ms;
        let outage_n = due_t
            .checked_add(INTERVAL_MS.checked_mul(OUTAGE_INTERVALS).unwrap())
            .unwrap();
        let next_due = outage_n.checked_add(INTERVAL_MS).unwrap();
        let following_due = next_due.checked_add(INTERVAL_MS).unwrap();
        assert_eq!(
            initial.trigger,
            ScheduleTrigger::Interval {
                interval_ms: INTERVAL_MS
            }
        );
        assert_eq!(initial.target, ScheduleTarget::Workspace);
        assert!(initial.enabled);
        assert!(initial.last_fired.is_none());
        assert!(initial.last_safe_outcome.is_none());
        assert_eq!(initial.counters, AutomationCounters::default());
        let seed = load_control_plane_snapshot(&store).await.unwrap().unwrap();
        assert_eq!(seed.agents.len(), 1);
        assert_eq!(seed.agents[0].state.id, agent);
        assert_eq!(seed.schedules, vec![initial.clone()]);
        assert!(seed.runs.is_empty());
        assert!(seed.schedule_fires.is_empty());
        assert!(seed.outbound.is_empty());
        assert!(seed.discord_outbound.is_empty());
        assert!(seed.agents[0].messages.is_empty());
        assert_eq!(original_calls.available_permits(), 0);
        original.shutdown().await;
        assert_eq!(original_state.read().await.schedules[&initial.id], initial);

        let (first, first_state) =
            restore(seed, &store, first_calls.clone(), first_release.clone());
        owned.push((first.clone(), first_release.clone(), first_calls.clone()));
        assert!(!Arc::ptr_eq(&original_state, &first_state));
        assert_eq!(first_state.read().await.schedules[&initial.id], initial);
        assert_eq!(first_calls.available_permits(), 0);

        let slot = ticks.len();
        ticks.push(Some(tokio::spawn({
            let first = first.clone();
            async move { first.tick_at(outage_n).await }
        })));
        tokio::time::timeout(Duration::from_secs(3), first_calls.acquire())
            .await
            .expect("the outage occurrence must enter the fake model")
            .unwrap()
            .forget();
        observed_calls += 1;
        first_release.add_permits(1);
        let admitted = tokio::time::timeout(Duration::from_secs(5), ticks[slot].as_mut().unwrap())
            .await
            .expect("the first tick must settle");
        ticks[slot] = None;
        assert_eq!(admitted.unwrap().unwrap(), 1);
        let first_record = first_state.read().await.schedules[&initial.id].clone();
        assert!(first_record.enabled);
        assert_eq!(first_record.id, initial.id);
        assert_eq!(first_record.agent_id, agent);
        assert_eq!(first_record.trigger, initial.trigger);
        assert_eq!(first_record.next_due_at_ms, next_due);
        assert_eq!(
            first_record.last_fired,
            Some(ScheduleLastFired {
                fired_at_ms: outage_n,
                run_idempotency_key: format!("schedule:{}:{outage_n}", initial.id),
                manual: false,
            })
        );
        assert_eq!(first_record.counters.runs, 1);
        assert_eq!(first_record.counters.failures, 0);
        assert_eq!(first_record.counters.consecutive_failures, 0);
        let first_outcome = first_record.last_safe_outcome.as_ref().unwrap();
        assert_eq!(first_outcome.status, ScheduleOutcomeStatus::Silent);
        assert_eq!(first_outcome.error_code, None);
        let first_snapshot = first_state.read().await.control_plane_snapshot();
        assert_eq!(first_snapshot.schedule_fires.len(), 1);
        let first_fire = &first_snapshot.schedule_fires[0];
        assert_eq!(first_fire.id, format!("schedule:{}:{outage_n}", initial.id));
        assert_eq!(first_fire.schedule_id, initial.id);
        assert_eq!(first_fire.agent_id, agent);
        assert_eq!(first_fire.fired_at_ms, outage_n);
        assert_eq!(first_fire.outcome, ScheduleOutcomeStatus::Silent);
        assert!(!first_fire.manual);
        assert_eq!(first_fire.error_code, None);
        assert!(first_snapshot.outbound.is_empty());
        assert!(first_snapshot.discord_outbound.is_empty());
        let first_disk = load_control_plane_snapshot(&store).await.unwrap().unwrap();
        assert_eq!(first_disk.schedules, vec![first_record.clone()]);
        assert_eq!(first_disk.schedule_fires, first_snapshot.schedule_fires);
        assert_eq!(
            serde_json::to_value(&first_disk).unwrap(),
            serde_json::to_value(&first_snapshot).unwrap()
        );

        let slot = ticks.len();
        ticks.push(Some(tokio::spawn({
            let first = first.clone();
            async move { first.tick_at(outage_n).await }
        })));
        let duplicate = tokio::time::timeout(Duration::from_secs(3), ticks[slot].as_mut().unwrap())
            .await
            .expect("the duplicate outage tick must settle");
        ticks[slot] = None;
        assert_eq!(duplicate.unwrap().unwrap(), 0);
        assert_eq!(first_calls.available_permits(), 0);
        assert_eq!(
            first_state.read().await.schedules[&initial.id],
            first_record
        );
        assert!(settle_owned(&first, &first_release).await);
        assert_eq!(first_calls.available_permits(), 0);
        let saved_after_retire = load_control_plane_snapshot(&store).await.unwrap().unwrap();
        assert_eq!(
            serde_json::to_value(&saved_after_retire).unwrap(),
            serde_json::to_value(&first_snapshot).unwrap()
        );

        let (second, second_state) = restore(
            saved_after_retire,
            &store,
            second_calls.clone(),
            second_release.clone(),
        );
        owned.push((second.clone(), second_release.clone(), second_calls.clone()));
        assert!(!Arc::ptr_eq(&first_state, &second_state));
        assert!(!Arc::ptr_eq(&original_state, &second_state));
        assert_eq!(
            second_state.read().await.schedules[&initial.id],
            first_record
        );
        assert_eq!(second_calls.available_permits(), 0);
        let slot = ticks.len();
        ticks.push(Some(tokio::spawn({
            let second = second.clone();
            async move { second.tick_at(outage_n).await }
        })));
        let duplicate = tokio::time::timeout(Duration::from_secs(3), ticks[slot].as_mut().unwrap())
            .await
            .expect("the fresh state must reject the old occurrence");
        ticks[slot] = None;
        assert_eq!(duplicate.unwrap().unwrap(), 0);
        assert_eq!(second_calls.available_permits(), 0);
        assert_eq!(
            second_state.read().await.schedules[&initial.id],
            first_record
        );

        let slot = ticks.len();
        ticks.push(Some(tokio::spawn({
            let second = second.clone();
            async move { second.tick_at(next_due).await }
        })));
        tokio::time::timeout(Duration::from_secs(3), second_calls.acquire())
            .await
            .expect("the next normal occurrence must enter the fake model")
            .unwrap()
            .forget();
        observed_calls += 1;
        second_release.add_permits(1);
        let admitted = tokio::time::timeout(Duration::from_secs(5), ticks[slot].as_mut().unwrap())
            .await
            .expect("the next normal tick must settle");
        ticks[slot] = None;
        assert_eq!(admitted.unwrap().unwrap(), 1);
        let second_record = second_state.read().await.schedules[&initial.id].clone();
        assert!(second_record.enabled);
        assert_eq!(second_record.id, initial.id);
        assert_eq!(second_record.agent_id, agent);
        assert_eq!(second_record.next_due_at_ms, following_due);
        assert_eq!(
            second_record.last_fired,
            Some(ScheduleLastFired {
                fired_at_ms: next_due,
                run_idempotency_key: format!("schedule:{}:{next_due}", initial.id),
                manual: false,
            })
        );
        assert_eq!(second_record.counters.runs, 2);
        assert_eq!(second_record.counters.failures, 0);
        assert_eq!(second_record.counters.consecutive_failures, 0);
        assert_eq!(
            second_record.last_safe_outcome.as_ref().unwrap().status,
            ScheduleOutcomeStatus::Silent
        );
        let final_snapshot = second_state.read().await.control_plane_snapshot();
        assert_eq!(final_snapshot.schedule_fires.len(), 2);
        assert_eq!(final_snapshot.schedule_fires[0], *first_fire);
        let second_fire = &final_snapshot.schedule_fires[1];
        assert_eq!(
            second_fire.id,
            format!("schedule:{}:{next_due}", initial.id)
        );
        assert_eq!(second_fire.schedule_id, initial.id);
        assert_eq!(second_fire.agent_id, agent);
        assert_eq!(second_fire.fired_at_ms, next_due);
        assert_eq!(second_fire.outcome, ScheduleOutcomeStatus::Silent);
        assert!(!second_fire.manual);
        assert_eq!(second_fire.error_code, None);
        assert!(final_snapshot.outbound.is_empty());
        assert!(final_snapshot.discord_outbound.is_empty());
        let final_disk = load_control_plane_snapshot(&store).await.unwrap().unwrap();
        assert_eq!(final_disk.schedules, vec![second_record]);
        assert_eq!(final_disk.schedule_fires, final_snapshot.schedule_fires);
        assert_eq!(
            serde_json::to_value(&final_disk).unwrap(),
            serde_json::to_value(&final_snapshot).unwrap()
        );
    }))
    .catch_unwind()
    .await;

    // Release every fake gate before bounded joins, even if acceptance failed.
    for (_, release, _) in &owned {
        release.add_permits(MAX_ACTIVE_SCHEDULES);
    }
    let mut clean = true;
    for tick in &mut ticks {
        if let Some(mut tick) = tick.take() {
            match tokio::time::timeout(Duration::from_secs(5), &mut tick).await {
                Ok(result) => clean &= result.is_ok(),
                Err(_) => {
                    clean = false;
                    tick.abort();
                    let _ = tokio::time::timeout(Duration::from_secs(5), &mut tick).await;
                }
            }
        }
    }
    for (service, release, _) in &owned {
        clean &= settle_owned(service, release).await;
    }
    assert!(
        clean,
        "all owned ticks and scheduler jobs must settle within bounds"
    );
    assert_eq!(observed_calls, 2, "exactly two ticks enter the fake model");
    for (_, _, calls) in &owned {
        assert_eq!(
            calls.available_permits(),
            0,
            "no extra model call may arrive through shutdown or a fresh state"
        );
    }
    match result {
        Ok(result) => result.expect("outage acceptance must finish within 15 seconds"),
        Err(panic) => std::panic::resume_unwind(panic),
    }
}
