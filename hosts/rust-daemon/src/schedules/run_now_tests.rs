//! Run now, active hours and `once` at claim, and outcomes with counters,
//! fire records, and `automation.updated` (spec §9.1, §9.2).

use std::sync::Arc;

use tokio::sync::Semaphore;

use super::tests::{due_schedule, service, service_with_daemon, GatedModel};
use super::*;
use crate::agent_runs::test_support::next_event;
use crate::sessions::test_support::within;
use crate::state::DaemonState;

type Gated = (
    SchedulerService,
    SharedDaemonState,
    String,
    Arc<Semaphore>,
    Arc<Semaphore>,
);

/// A scheduler whose agent's model call waits for a `release` permit.
fn gated() -> Gated {
    let entered = Arc::new(Semaphore::new(0));
    let release = Arc::new(Semaphore::new(0));
    let daemon = DaemonState::with_model_adapter(Arc::new(GatedModel {
        entered: entered.clone(),
        release: release.clone(),
    }));
    let (service, state, agent_id, _) = service_with_daemon(daemon);
    (service, state, agent_id, entered, release)
}

/// An automation of `agent_id` next due in an hour.
async fn later(service: &SchedulerService, agent_id: &str) -> ScheduledPromptRecord {
    service
        .create(
            agent_id.into(),
            "Check status".into(),
            ScheduleTrigger::Interval {
                interval_ms: 60_000,
            },
            ScheduleTarget::Workspace,
            true,
            None,
            Some(now_ms() + 3_600_000),
            None,
        )
        .await
        .unwrap()
        .0
}

async fn entered_run(entered: &Semaphore) {
    within("the automation's run to reach its model", entered.acquire())
        .await
        .unwrap()
        .forget();
}

#[test]
fn the_run_now_strings_are_the_specs() {
    assert_eq!(
        AUTOMATION_ALREADY_RUNNING,
        "This automation is already running"
    );
    assert_eq!(
        TOO_MANY_RUNNING_AUTOMATIONS,
        "Too many automations are running; try again shortly"
    );
}

#[tokio::test]
async fn run_now_fires_without_moving_the_due_time_and_is_recorded_as_manual() {
    let (service, state, agent_id, entered, release) = gated();
    let record = later(&service, &agent_id).await;
    let paused = service
        .update(&agent_id, &record.id, None, None, None, Some(false))
        .await
        .unwrap();
    release.add_permits(1);

    let claimed = service.run_now(&agent_id, &record.id).await.unwrap();
    entered_run(&entered).await;
    service.drain().await;

    assert!(claimed.last_fired.as_ref().unwrap().manual);
    let after = state.read().await.schedules[&record.id].clone();
    assert_eq!(
        after.next_due_at_ms, paused.next_due_at_ms,
        "the due time stays"
    );
    assert!(!after.enabled, "a paused automation stays paused");
    assert_eq!(
        after.last_safe_outcome.as_ref().unwrap().status,
        ScheduleOutcomeStatus::Silent
    );
    assert_eq!(after.counters.runs, 1);
    let fires = state.read().await.schedule_fires.for_schedule(&record.id);
    assert_eq!(fires.len(), 1);
    assert!(fires[0].manual);
    assert!(fires[0]
        .id
        .starts_with(&format!("schedule:{}:manual:", record.id)));
    assert!(fires[0].run_id.is_some());
    assert_eq!(
        fires[0].session_id.as_deref(),
        Some(crate::sessions::schedule_room_id(&record.id).as_str())
    );
}

#[tokio::test]
async fn run_now_refuses_while_the_automation_runs() {
    let (service, _state, agent_id, entered, release) = gated();
    let record = later(&service, &agent_id).await;
    service.run_now(&agent_id, &record.id).await.unwrap();
    entered_run(&entered).await;

    assert_eq!(
        service.run_now(&agent_id, &record.id).await,
        Err(ScheduleError::Conflict(AUTOMATION_ALREADY_RUNNING))
    );

    release.add_permits(2);
    service.drain().await;
    service
        .run_now(&agent_id, &record.id)
        .await
        .expect("free again once its run finished");
    entered_run(&entered).await;
    service.drain().await;
}

#[tokio::test]
async fn run_now_keeps_ownership_and_the_admission_cap() {
    let (service, _state, agent_id, _, _) = gated();
    let record = later(&service, &agent_id).await;
    assert_eq!(
        service.run_now("someone-else", &record.id).await,
        Err(ScheduleError::NotFound)
    );
    assert_eq!(
        service.run_now(&agent_id, "missing").await,
        Err(ScheduleError::NotFound)
    );
    {
        let mut jobs = service.inner.jobs.lock().await;
        for n in 0..MAX_ACTIVE_SCHEDULES {
            jobs.insert(
                format!("busy-{n}"),
                tokio::spawn(std::future::pending::<()>()),
            );
        }
    }
    assert_eq!(
        service.run_now(&agent_id, &record.id).await,
        Err(ScheduleError::Busy(TOO_MANY_RUNNING_AUTOMATIONS))
    );
    for (_, job) in std::mem::take(&mut *service.inner.jobs.lock().await) {
        job.abort();
    }
}

#[tokio::test]
async fn a_once_automation_fires_once_and_turns_itself_off() {
    let (service, state, agent_id, entered, release) = gated();
    let at = now_ms() + 3_600_000;
    let (record, _) = service
        .automations()
        .create(
            AutomationInput::owner(
                agent_id.clone(),
                "Remind me".into(),
                ScheduleTrigger::Once { at_ms: at },
                ScheduleTarget::Workspace,
            ),
            now_ms(),
        )
        .await
        .unwrap();
    release.add_permits(1);

    let ticking = service.clone();
    let tick = tokio::spawn(async move { ticking.tick_at(at).await });
    entered_run(&entered).await;
    assert_eq!(tick.await.unwrap().unwrap(), 1);

    let fired = state.read().await.schedules[&record.id].clone();
    assert!(!fired.enabled, "the claim turned it off");
    assert_eq!(fired.next_due_at_ms, at);
    assert_eq!(fired.counters.runs, 1);
    assert_eq!(
        service.tick_at(at + 3_600_000).await.unwrap(),
        0,
        "it never fires again"
    );
}

#[tokio::test]
async fn active_hours_move_the_next_due_time_at_claim() {
    let (service, state, agent_id, entered, release) = gated();
    // 2026-01-05 21:30 UTC; the window closes at 22:00.
    let due = 1_767_648_600_000;
    let mut record = crate::schedules::test_automation(&agent_id, "windowed");
    record.trigger = ScheduleTrigger::Interval {
        interval_ms: 30 * 60_000,
    };
    record.active_hours = Some(ActiveHours {
        start: "08:00".into(),
        end: "22:00".into(),
        days: (0..=6).collect(),
        time_zone: "UTC".into(),
    });
    record.next_due_at_ms = due;
    state
        .write()
        .await
        .schedules
        .insert(record.id.clone(), record);
    release.add_permits(1);

    let ticking = service.clone();
    let tick = tokio::spawn(async move { ticking.tick_at(due + 60_000).await });
    entered_run(&entered).await;
    tick.await.unwrap().unwrap();

    assert_eq!(
        state.read().await.schedules["windowed"].next_due_at_ms,
        due + (10 * 60 + 30) * 60_000,
        "the next day's 08:00, not 22:00"
    );
}

#[tokio::test]
async fn outcomes_count_record_a_fire_and_announce() {
    let (service, state, agent_id, entered, release) = gated();
    let mut stream = state.read().await.live.subscribe(&agent_id).unwrap();
    let record = due_schedule(&service, &agent_id).await;
    release.add_permits(1);

    let ticking = service.clone();
    let tick = tokio::spawn(async move { ticking.tick_at(now_ms()).await });
    entered_run(&entered).await;
    tick.await.unwrap().unwrap();

    let after = state.read().await.schedules[&record.id].clone();
    assert_eq!(
        after.counters,
        AutomationCounters {
            runs: 1,
            failures: 0,
            consecutive_failures: 0
        }
    );
    let fires = state.read().await.schedule_fires.for_schedule(&record.id);
    assert_eq!(fires.len(), 1);
    assert_eq!(fires[0].outcome, ScheduleOutcomeStatus::Silent);
    assert!(!fires[0].manual);
    // Created, claimed, and finished.
    let mut announced = 0;
    while announced < 3 {
        let event = next_event(&mut stream).await.to_json(1);
        if event["type"] == "automation.updated" && event["scheduleId"] == record.id.as_str() {
            announced += 1;
        }
    }
}

#[tokio::test]
async fn a_failed_commit_save_rolls_back_the_outcome_counters_and_fire() {
    let (service, state, agent_id, entered, release) = gated();
    let record = due_schedule(&service, &agent_id).await;
    let ticking = service.clone();
    let tick = tokio::spawn(async move { ticking.tick_at(now_ms()).await });
    entered_run(&entered).await;
    let directory =
        std::env::temp_dir().join(format!("anima-fire-rollback-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    state.write().await.set_control_plane_store(Some(
        crate::control_plane_store::ControlPlaneStoreConfig::Json(directory.clone()),
    ));
    release.add_permits(1);
    tick.await.unwrap().unwrap();

    let after = state.read().await.schedules[&record.id].clone();
    assert!(after.last_safe_outcome.is_none());
    assert_eq!(after.counters, AutomationCounters::default());
    assert!(state
        .read()
        .await
        .schedule_fires
        .for_schedule(&record.id)
        .is_empty());
    let _ = std::fs::remove_dir_all(&directory);
}

#[tokio::test]
async fn restart_reconciliation_records_a_failed_fire() {
    let (service, state, agent_id, _) = service();
    let record = due_schedule(&service, &agent_id).await;
    claim_due(&service.inner, &record.id, now_ms())
        .await
        .unwrap()
        .unwrap();

    assert_eq!(service.tick_at(now_ms()).await.unwrap(), 0);

    let after = state.read().await.schedules[&record.id].clone();
    assert!(!after.enabled);
    assert_eq!(
        after.counters,
        AutomationCounters {
            runs: 1,
            failures: 1,
            consecutive_failures: 1
        }
    );
    let fires = state.read().await.schedule_fires.for_schedule(&record.id);
    assert_eq!(fires.len(), 1);
    assert_eq!(fires[0].outcome, ScheduleOutcomeStatus::Failed);
    assert_eq!(
        fires[0].error_code.as_deref(),
        Some("schedule_run_interrupted")
    );
    assert_eq!(fires[0].run_id, None, "no run started before the restart");
}

/// A JSON store at a directory: every save fails.
fn broken_store(state: &mut DaemonState) -> std::path::PathBuf {
    let directory =
        std::env::temp_dir().join(format!("anima-broken-store-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    state.set_control_plane_store(Some(
        crate::control_plane_store::ControlPlaneStoreConfig::Json(directory.clone()),
    ));
    directory
}

/// Counters and fires an earlier occurrence left, so a rollback that reset
/// them instead of restoring them would show.
async fn with_history(state: &SharedDaemonState, schedule_id: &str, agent_id: &str) {
    let mut guard = state.write().await;
    guard.schedules.get_mut(schedule_id).unwrap().counters = AutomationCounters {
        runs: 4,
        failures: 2,
        consecutive_failures: 1,
    };
    for n in 1..=2 {
        let mut fire = crate::schedules::history::tests_support_fire(agent_id);
        fire.id = format!("schedule:{schedule_id}:{n}");
        fire.schedule_id = schedule_id.to_string();
        guard.schedule_fires.record(fire);
    }
}

/// Controller ruling 3: a failed save restores exactly the previous
/// counters and fire records.
#[tokio::test]
async fn a_failed_outcome_save_restores_the_counters_and_fires_exactly() {
    let (service, state, agent_id, _) = service();
    let record = due_schedule(&service, &agent_id).await;
    with_history(&state, &record.id, &agent_id).await;
    claim_due(&service.inner, &record.id, now_ms())
        .await
        .unwrap()
        .unwrap();
    let before = state.read().await.schedules[&record.id].clone();
    let fires_before = state.read().await.schedule_fires.snapshot();
    let directory = broken_store(&mut *state.write().await);

    assert_eq!(
        record_outcome(
            &service.inner,
            &record.id,
            ScheduleOutcomeStatus::Failed,
            Some("schedule_run_failed"),
            now_ms(),
            None,
        )
        .await,
        Err(ScheduleError::Persistence)
    );

    let after = state.read().await.schedules[&record.id].clone();
    assert_eq!(after.counters, before.counters);
    assert_eq!(after.last_safe_outcome, before.last_safe_outcome);
    assert_eq!(state.read().await.schedule_fires.snapshot(), fires_before);
    let _ = std::fs::remove_dir_all(&directory);
}

/// Controller ruling 3, during restart reconciliation.
#[tokio::test]
async fn a_failed_reconciliation_save_restores_the_counters_and_fires_exactly() {
    let (service, state, agent_id, _) = service();
    let record = due_schedule(&service, &agent_id).await;
    with_history(&state, &record.id, &agent_id).await;
    claim_due(&service.inner, &record.id, now_ms())
        .await
        .unwrap()
        .unwrap();
    let before = state.read().await.schedules.clone();
    let fires_before = state.read().await.schedule_fires.snapshot();
    let directory = broken_store(&mut *state.write().await);

    assert_eq!(
        reconcile_interrupted(&service.inner, now_ms(), &BTreeSet::new()).await,
        Err(ScheduleError::Persistence)
    );

    assert_eq!(state.read().await.schedules, before);
    assert_eq!(state.read().await.schedule_fires.snapshot(), fires_before);
    let _ = std::fs::remove_dir_all(&directory);
}

#[tokio::test]
async fn run_now_refuses_an_occurrence_still_waiting_for_its_outcome() {
    let (service, _state, agent_id, _) = service();
    let record = later(&service, &agent_id).await;
    claim_manual(&service.inner, &agent_id, &record.id, now_ms(), None)
        .await
        .unwrap();
    assert_eq!(
        service.run_now(&agent_id, &record.id).await,
        Err(ScheduleError::Conflict(AUTOMATION_ALREADY_RUNNING))
    );
}

#[tokio::test]
async fn a_restart_during_run_now_disables_the_automation_and_records_a_manual_fire() {
    let (service, state, agent_id, _) = service();
    let record = later(&service, &agent_id).await;
    claim_manual(&service.inner, &agent_id, &record.id, now_ms(), None)
        .await
        .unwrap();

    reconcile_interrupted(&service.inner, now_ms(), &BTreeSet::new())
        .await
        .unwrap();

    let after = state.read().await.schedules[&record.id].clone();
    assert!(!after.enabled);
    assert_eq!(after.next_due_at_ms, record.next_due_at_ms);
    let fires = state.read().await.schedule_fires.for_schedule(&record.id);
    assert_eq!(fires.len(), 1);
    assert!(fires[0].manual);
    assert_eq!(fires[0].outcome, ScheduleOutcomeStatus::Failed);
}

fn m79_at(utc: &str) -> u64 {
    chrono::DateTime::parse_from_rfc3339(utc)
        .unwrap()
        .timestamp_millis() as u64
}

fn m79_hours(start: &str, end: &str, days: &[u8], zone: &str) -> ActiveHours {
    ActiveHours {
        start: start.into(),
        end: end.into(),
        days: days.to_vec(),
        time_zone: zone.into(),
    }
}

fn m79_record(agent: &str, id: &str, due: u64, hours: ActiveHours) -> ScheduledPromptRecord {
    let mut record = test_automation(agent, id);
    record.trigger = ScheduleTrigger::Interval {
        interval_ms: 30 * 60_000,
    };
    record.next_due_at_ms = due;
    record.active_hours = Some(hours);
    record
}

// Own each join until it settles. Expected RED never waits for a model-entry
// notification: permits are supplied before admission, including zero-work cases.
async fn m79_settle_jobs(service: &SchedulerService) {
    let jobs = std::mem::take(&mut *service.inner.jobs.lock().await);
    let mut settled = true;
    for (_, mut job) in jobs {
        match tokio::time::timeout(std::time::Duration::from_secs(10), &mut job).await {
            Ok(joined) => settled &= joined.is_ok(),
            Err(_) => {
                settled = false;
                let _ = tokio::time::timeout(
                    std::time::Duration::from_secs(5),
                    service.request_shutdown(),
                )
                .await;
                job.abort();
                let _ = tokio::time::timeout(std::time::Duration::from_secs(5), &mut job).await;
            }
        }
    }
    assert!(
        settled,
        "M79 owned jobs must settle before inspecting effects"
    );
}

async fn m79_tick(
    service: &SchedulerService,
    release: &Semaphore,
    now: u64,
) -> Result<usize, ScheduleError> {
    release.add_permits(MAX_ACTIVE_SCHEDULES);
    let ticking = service.clone();
    let mut tick =
        tokio::spawn(async move { SchedulerService::tick_inner(&ticking.inner, now).await });
    let result = tokio::time::timeout(std::time::Duration::from_secs(10), &mut tick).await;
    if result.is_err() {
        tick.abort();
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), &mut tick).await;
        let _ = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            service.request_shutdown(),
        )
        .await;
    }
    m79_settle_jobs(service).await;
    result
        .expect("M79 tick must finish within its bound")
        .expect("M79 tick task joined")
}

#[tokio::test]
async fn m79_overdue_occurrence_waits_unchanged_until_next_opening_then_runs_once() {
    let (service, state, agent, calls, release) = gated();
    let due = m79_at("2026-01-05T21:30:00Z");
    let opening = m79_at("2026-01-06T08:00:00Z");
    let before = m79_record(
        &agent,
        "m79-outage",
        due,
        m79_hours("08:00", "22:00", &[0, 1, 2, 3, 4, 5, 6], "UTC"),
    );
    state
        .write()
        .await
        .schedules
        .insert(before.id.clone(), before.clone());
    for at in [
        m79_at("2026-01-05T23:00:00Z"),
        m79_at("2026-01-06T07:59:00Z"),
    ] {
        let claimed = m79_tick(&service, &release, at).await;
        let snapshot = state.read().await.control_plane_snapshot();
        assert_eq!(
            claimed.unwrap(),
            0,
            "outside-window automatic claim is deferred"
        );
        assert_eq!(
            snapshot.schedules,
            vec![before.clone()],
            "every stored field is unchanged"
        );
        assert!(snapshot.runs.is_empty());
        assert!(snapshot.schedule_fires.is_empty());
        assert!(snapshot
            .agents
            .iter()
            .all(|entry| entry.messages.is_empty()));
        assert_eq!(calls.available_permits(), 0);
    }
    let claimed = m79_tick(&service, &release, opening).await;
    assert_eq!(claimed.unwrap(), 1);
    let after = state.read().await.schedules[&before.id].clone();
    assert_eq!(after.next_due_at_ms, m79_at("2026-01-06T08:30:00Z"));
    assert_eq!(
        after.counters,
        AutomationCounters {
            runs: 1,
            failures: 0,
            consecutive_failures: 0
        }
    );
    let fires = state.read().await.schedule_fires.for_schedule(&before.id);
    assert_eq!(fires.len(), 1);
    assert_eq!(fires[0].id, format!("schedule:{}:{opening}", before.id));
    assert_eq!(fires[0].fired_at_ms, opening);
    assert_eq!(fires[0].outcome, ScheduleOutcomeStatus::Silent);
    assert!(!fires[0].manual);
    assert!(fires[0].run_id.is_some());
    assert_eq!(calls.available_permits(), 1);
    let repeat = m79_tick(&service, &release, opening).await;
    assert_eq!(repeat.unwrap(), 0);
    assert_eq!(state.read().await.schedules[&before.id], after);
    assert_eq!(
        state.read().await.schedule_fires.for_schedule(&before.id),
        fires
    );
    assert_eq!(calls.available_permits(), 1);
}

#[tokio::test]
async fn m79_interval_daily_and_cron_defer_at_exclusive_end_and_admit_at_inclusive_start() {
    let due = m79_at("2026-01-05T21:30:00Z");
    let close = m79_at("2026-01-05T22:00:00Z");
    let opening = m79_at("2026-01-06T08:00:00Z");
    for (trigger, expected_next) in [
        (
            ScheduleTrigger::Interval {
                interval_ms: 30 * 60_000,
            },
            m79_at("2026-01-06T08:30:00Z"),
        ),
        (
            ScheduleTrigger::Daily {
                hour: 21,
                minute: 30,
                time_zone: "UTC".into(),
            },
            m79_at("2026-01-06T21:30:00Z"),
        ),
        (
            ScheduleTrigger::Cron {
                expression: "30 21 * * *".into(),
                time_zone: "UTC".into(),
            },
            m79_at("2026-01-06T21:30:00Z"),
        ),
    ] {
        let (service, state, agent, calls, release) = gated();
        let mut before = m79_record(
            &agent,
            "m79-boundary",
            due,
            m79_hours("08:00", "22:00", &[0, 1, 2, 3, 4, 5, 6], "UTC"),
        );
        before.trigger = trigger;
        state
            .write()
            .await
            .schedules
            .insert(before.id.clone(), before.clone());
        let closed_claim = m79_tick(&service, &release, close).await;
        assert_eq!(closed_claim.unwrap(), 0, "{:?}", before.trigger);
        assert_eq!(state.read().await.schedules[&before.id], before);
        assert_eq!(calls.available_permits(), 0);
        let open_claim = m79_tick(&service, &release, opening).await;
        assert_eq!(open_claim.unwrap(), 1, "{:?}", before.trigger);
        assert_eq!(
            state.read().await.schedules[&before.id].next_due_at_ms,
            expected_next
        );
        assert_eq!(
            state
                .read()
                .await
                .schedule_fires
                .for_schedule(&before.id)
                .len(),
            1
        );
        let repeated = m79_tick(&service, &release, opening).await;
        assert_eq!(repeated.unwrap(), 0);
        assert_eq!(calls.available_permits(), 1);
    }
}

#[tokio::test]
async fn m79_automatic_admission_uses_existing_overnight_weekday_and_dst_windows() {
    let overnight = m79_hours("22:00", "06:00", &[5], "UTC");
    let weekdays = m79_hours("08:00", "22:00", &[1, 2, 3, 4, 5], "UTC");
    let new_york = m79_hours("01:00", "04:00", &[0, 1, 2, 3, 4, 5, 6], "America/New_York");
    for (hours, instant, expected) in [
        (overnight.clone(), "2026-01-09T22:00:00Z", true),
        (overnight.clone(), "2026-01-10T05:59:00Z", true),
        (overnight.clone(), "2026-01-10T06:00:00Z", false),
        (overnight, "2026-01-10T22:00:00Z", false),
        (weekdays.clone(), "2026-01-10T08:00:00Z", false),
        (weekdays.clone(), "2026-01-11T08:00:00Z", false),
        (weekdays, "2026-01-12T08:00:00Z", true),
        (new_york.clone(), "2026-03-08T06:59:00Z", true),
        (new_york.clone(), "2026-03-08T07:00:00Z", true),
        (new_york, "2026-03-08T08:00:00Z", false),
    ] {
        let at = m79_at(instant);
        assert_eq!(
            timing::ActiveWindow::parse(&hours).unwrap().contains(at),
            expected,
            "declared existing wall-clock rule: {instant}"
        );
        let (service, state, agent, calls, release) = gated();
        let before = m79_record(&agent, "m79-local-clock", at - 60_000, hours);
        state
            .write()
            .await
            .schedules
            .insert(before.id.clone(), before.clone());
        let claimed = m79_tick(&service, &release, at).await;
        assert_eq!(claimed.unwrap(), usize::from(expected), "{instant}");
        assert_eq!(
            calls.available_permits(),
            usize::from(expected),
            "{instant}"
        );
        let snapshot = state.read().await.control_plane_snapshot();
        assert_eq!(
            snapshot.schedule_fires.len(),
            usize::from(expected),
            "{instant}"
        );
        if !expected {
            assert_eq!(snapshot.schedules, vec![before], "{instant}");
            assert!(snapshot.runs.is_empty());
            assert!(snapshot
                .agents
                .iter()
                .all(|entry| entry.messages.is_empty()));
        }
    }
}

#[tokio::test]
async fn m79_skipped_and_malformed_windows_do_not_starve_an_eligible_peer() {
    for malformed in [false, true] {
        let (service, state, agent, calls, release) = gated();
        let at = m79_at("2026-01-05T23:00:00Z");
        let mut blocked = m79_record(
            &agent,
            "m79-first",
            at - 90 * 60_000,
            m79_hours("08:00", "22:00", &[0, 1, 2, 3, 4, 5, 6], "UTC"),
        );
        if malformed {
            blocked.active_hours.as_mut().unwrap().start = "invalid".into();
        }
        let mut peer = test_automation(&agent, "m79-peer");
        peer.next_due_at_ms = blocked.next_due_at_ms + 1;
        {
            let mut guard = state.write().await;
            guard.schedules.insert(blocked.id.clone(), blocked.clone());
            guard.schedules.insert(peer.id.clone(), peer.clone());
        }
        let claimed = m79_tick(&service, &release, at).await;
        assert_eq!(claimed.unwrap(), 1, "malformed={malformed}");
        assert_eq!(calls.available_permits(), 1);
        let guard = state.read().await;
        let mut expected = blocked.clone();
        if malformed {
            expected.enabled = false;
            expected.updated_at_ms = at;
        }
        assert_eq!(guard.schedules[&blocked.id], expected);
        assert!(guard.schedule_fires.for_schedule(&blocked.id).is_empty());
        let fires = guard.schedule_fires.for_schedule(&peer.id);
        assert_eq!(fires.len(), 1);
        assert_eq!(fires[0].outcome, ScheduleOutcomeStatus::Silent);
        assert_eq!(guard.schedules[&peer.id].counters.runs, 1);
    }
}

#[tokio::test]
async fn m79_explicit_manual_run_now_still_overrides_active_hours_without_moving_due() {
    let (service, state, agent, calls, release) = gated();
    let at = m79_at("2026-01-05T23:00:00Z");
    let before = m79_record(
        &agent,
        "m79-manual",
        m79_at("2026-01-06T08:00:00Z"),
        m79_hours("08:00", "22:00", &[0, 1, 2, 3, 4, 5, 6], "UTC"),
    );
    state
        .write()
        .await
        .schedules
        .insert(before.id.clone(), before.clone());
    release.add_permits(1);
    let claimed = run_now_inner(&service.inner, &agent, &before.id, at, None).await;
    m79_settle_jobs(&service).await;
    assert!(claimed.unwrap().last_fired.unwrap().manual);
    assert_eq!(calls.available_permits(), 1);
    let guard = state.read().await;
    let after = &guard.schedules[&before.id];
    assert_eq!(after.next_due_at_ms, before.next_due_at_ms);
    assert_eq!(after.enabled, before.enabled);
    assert_eq!(after.counters.runs, 1);
    let fires = guard.schedule_fires.for_schedule(&before.id);
    assert_eq!(fires.len(), 1);
    assert!(fires[0].manual);
    assert_eq!(fires[0].fired_at_ms, at);
    assert_eq!(fires[0].outcome, ScheduleOutcomeStatus::Silent);
}
