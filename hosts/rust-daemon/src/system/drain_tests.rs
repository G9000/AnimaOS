//! The drain a supervisor starts before Stop, Restart, Quit or Update: new
//! messages are refused, the work already accepted finishes, and the counts
//! say when it has.
use std::sync::Arc;
use std::time::Duration;

use axum::http::StatusCode;

use super::{DrainController, InFlight};
use crate::agent_runs::test_support::{
    accept, accept_web, add_chat, coordinator_state, coordinator_with, wait_for, Gate,
    ScriptedModel,
};
use crate::agent_runs::AgentRunCoordinator;
use crate::connectors::credentials::InMemoryCredentialStore;
use crate::connectors::runtime::ConnectorManager;
use crate::jobs::JobService;
use crate::runs::RunStatus;
use crate::schedules::SchedulerService;

const DRAINING: &str = "Anima is restarting; send the message again in a moment";

fn drain_over(coordinator: &AgentRunCoordinator) -> DrainController {
    let state = coordinator_state(coordinator);
    let connectors = ConnectorManager::new(
        Arc::clone(&state),
        coordinator.clone(),
        Arc::new(InMemoryCredentialStore::default()),
        Arc::new(crate::app::DeterministicTelegramTransport),
    );
    let scheduler = SchedulerService::new(Arc::clone(&state), coordinator.clone(), connectors);
    let jobs = JobService::new(Arc::clone(&state), coordinator.clone());
    DrainController::new(state, coordinator.clone(), jobs, scheduler)
}

/// Waits (up to five seconds) until nothing is in flight.
async fn settled(drain: &DrainController) {
    for _ in 0..500 {
        if drain.in_flight().await.total() == 0 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("still in flight: {:?}", drain.in_flight().await);
}

#[test]
fn in_flight_totals_every_category_and_serializes_camel_case() {
    let in_flight = InFlight {
        chat_runs: 1,
        queued_runs: 2,
        swarm_runs: 3,
        helper_runs: 4,
        jobs: 5,
        background_processes: 6,
        app_sessions: 7,
        acp_sessions: 8,
        memory_reviews: 9,
        skill_evaluations: 15,
        task_regressions: 16,
        doctor_repairs: 10,
        standing_intent_changes: 11,
        execution_jobs: 12,
        mcp_operations: 13,
        browser_profile_logins: 14,
    };
    assert_eq!(in_flight.total(), 136);
    assert_eq!(InFlight::default().total(), 0);
    assert_eq!(
        serde_json::to_value(in_flight).unwrap(),
        serde_json::json!({
            "chatRuns": 1, "queuedRuns": 2, "swarmRuns": 3, "helperRuns": 4,
            "jobs": 5, "backgroundProcesses": 6, "appSessions": 7, "acpSessions": 8,
            "skillEvaluations": 15, "taskRegressions": 16, "memoryReviews": 9, "doctorRepairs": 10, "standingIntentChanges": 11, "executionJobs": 12, "mcpOperations": 13, "browserProfileLogins": 14,
        })
    );
}

#[tokio::test]
async fn drain_refuses_new_messages_but_finishes_queued() {
    let gate = Gate::new();
    let (coordinator, agent_id) =
        coordinator_with(ScriptedModel::gated(vec![], gate.clone())).await;
    add_chat(&coordinator, &agent_id, "chat:a").await;
    let going = accept_web(&coordinator, &agent_id, "chat:a", "key-a").await;
    gate.entered().await;
    let queued = accept_web(&coordinator, &agent_id, "chat:a", "key-b").await;
    wait_for(&coordinator, &queued, RunStatus::Queued).await;
    let drain = drain_over(&coordinator);

    let in_flight = drain.begin().await;
    assert!(drain.is_draining());
    assert_eq!((in_flight.chat_runs, in_flight.queued_runs), (1, 1));
    assert_eq!(in_flight.total(), 2);
    assert!(
        !coordinator.is_shutting_down(),
        "a drain never closes admission for queued runs"
    );

    let start = coordinator.web_start(
        agent_id.clone(),
        "chat:a".into(),
        "key-c".into(),
        "key-c".into(),
    );
    let refused = coordinator
        .accept_run(accept(&agent_id, "chat:a", "key-c"), start)
        .await
        .unwrap_err();
    assert_eq!(refused.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(refused.message(), DRAINING);
    assert!(
        !crate::agent_runs::is_shutting_down(&refused),
        "a drain refusal is not a shutdown"
    );

    // The going run finishes, then its queue starts the queued one.
    gate.release();
    wait_for(&coordinator, &going, RunStatus::Completed).await;
    gate.entered().await;
    gate.release();
    wait_for(&coordinator, &queued, RunStatus::Completed).await;
    settled(&drain).await;
    assert!(drain.is_draining(), "finishing work does not end the drain");
}

#[tokio::test]
async fn cancel_restores_admission() {
    let (coordinator, agent_id) = coordinator_with(ScriptedModel::new(vec![])).await;
    add_chat(&coordinator, &agent_id, "chat:a").await;
    let drain = drain_over(&coordinator);

    assert_eq!(drain.begin().await, InFlight::default());
    drain.cancel();
    assert!(!drain.is_draining());

    let accepted = accept_web(&coordinator, &agent_id, "chat:a", "key-a").await;
    wait_for(&coordinator, &accepted, RunStatus::Completed).await;
}

#[tokio::test]
async fn drain_counts_background_processes() {
    let (coordinator, _agent_id) = coordinator_with(ScriptedModel::new(vec![])).await;
    let drain = drain_over(&coordinator);
    let workspace = tempfile::tempdir().unwrap();
    let manager = coordinator_state(&coordinator)
        .read()
        .await
        .process_manager
        .clone();

    crate::tools::process::start_background_process_from_root(
        &manager,
        workspace.path(),
        "sleep 5",
        ".",
    )
    .unwrap();

    let in_flight = drain.begin().await;
    assert_eq!(in_flight.background_processes, 1);
    assert_eq!(in_flight.total(), 1);
}
