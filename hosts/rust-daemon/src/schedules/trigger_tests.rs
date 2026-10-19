//! FP04: tool-triggered work shares the scheduler's durable admission boundary.

use std::collections::BTreeMap;
use std::sync::Arc;

use anima_core::{DataValue, MessageRole, ToolCall};
use tokio::sync::{RwLock, Semaphore};

use super::tests::NoopTelegram;
use super::*;
use crate::agent_runs::test_support::{
    chat_request, companion_config, decision, gated_context, ledger_run, pending_approvals,
    tool_input, Gate, ScriptedModel, Step,
};
use crate::connectors::credentials::InMemoryCredentialStore;
use crate::control_plane_store::{ControlPlaneSnapshot, ControlPlaneStoreConfig};
use crate::runs::RunStopRequest;
use crate::sessions::test_support::within;
use crate::state::DaemonState;

fn fixture(
    model: Arc<ScriptedModel>,
) -> (SchedulerService, SharedDaemonState, String, Arc<Semaphore>) {
    let mut daemon = DaemonState::with_model_adapter(model);
    let mut config = companion_config("manual-trigger");
    config.tools = Some(
        crate::tools::ToolRegistry::new()
            .resolve_descriptors(["trigger_automation"])
            .unwrap(),
    );
    let agent_id = daemon.create_agent(config).unwrap().state.id;
    let state = Arc::new(RwLock::new(daemon));
    // The direct caller owns the entire global budget until it finishes.
    let limiter = Arc::new(Semaphore::new(1));
    let runs = AgentRunCoordinator::new(state.clone(), limiter.clone()).with_max_runs_per_agent(1);
    let connectors = ConnectorManager::new(
        state.clone(),
        runs.clone(),
        Arc::new(InMemoryCredentialStore::default()),
        Arc::new(NoopTelegram),
    );
    (
        SchedulerService::new(state.clone(), runs, connectors),
        state,
        agent_id,
        limiter,
    )
}

fn trigger_call(id: &str) -> ToolCall {
    ToolCall {
        id: "fp04-trigger".into(),
        name: "trigger_automation".into(),
        args: BTreeMap::from([("id".into(), DataValue::String(id.into()))]),
    }
}

fn trigger_authority(link: &crate::runs::RunLink, id: &str) -> automations::AutomationRunAuthority {
    automations::AutomationRunAuthority::new(
        link.clone(),
        automations::AutomationRunTool::Trigger,
        trigger_call(id),
        false,
        Some(anima_core::CancelSignal::new()),
    )
}

struct SnapshotFile(std::path::PathBuf);

impl SnapshotFile {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("anima-fp04-trigger-{}.json", uuid::Uuid::new_v4())))
    }
}

impl Drop for SnapshotFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[tokio::test]
async fn fp04_trigger_tool_returns_a_durable_claim_before_execution_with_one_global_and_agent_slot()
{
    let gate = Gate::new();
    let model = ScriptedModel::gated(
        vec![
            Step::Tools(vec![trigger_call("mine")]),
            Step::Text(vec!["The manual occurrence was admitted."]),
            Step::Text(vec![CHECKIN_SENTINEL]),
        ],
        gate.clone(),
    );
    let (service, state, agent, limiter) = fixture(model.clone());
    let file = SnapshotFile::new();
    let mut original = test_automation(&agent, "mine");
    original.enabled = false;
    original.next_due_at_ms = now_ms() + 3_600_000;
    {
        let mut guard = state.write().await;
        guard
            .schedules
            .insert(original.id.clone(), original.clone());
        guard.set_control_plane_store(Some(ControlPlaneStoreConfig::Json(file.0.clone())));
    }

    let caller = service.inner.runs.clone();
    let agent_for_run = agent.clone();
    let parent = tokio::spawn(async move {
        caller
            .run(chat_request(
                &agent_for_run,
                "chat:fp04-trigger",
                "Run my saved automation once",
            ))
            .await
    });
    gate.entered().await;
    assert_eq!(limiter.available_permits(), 0);
    gate.release();
    // This second model call can only start once the trigger tool has returned.
    // Keep the caller here so its global and per-agent permits stay occupied.
    gate.entered().await;
    let requests = model.requests();
    assert_eq!(
        requests.len(),
        2,
        "the scheduled model cannot run while the caller holds the only permit"
    );
    let reply = requests[1]
        .messages
        .iter()
        .find(|message| message.role == MessageRole::Tool)
        .unwrap();
    assert!(
        reply
            .content
            .text
            .contains("Admitted one manual occurrence"),
        "{}",
        reply.content.text
    );
    assert!(reply
        .content
        .text
        .contains("final outcome is not yet known"));
    let saved: ControlPlaneSnapshot =
        serde_json::from_slice(&std::fs::read(&file.0).unwrap()).unwrap();
    let claimed = saved
        .schedules
        .iter()
        .find(|record| record.id == original.id)
        .unwrap();
    assert!(claimed.last_fired.as_ref().unwrap().manual);
    assert!(claimed.last_safe_outcome.is_none());
    assert_eq!(claimed.counters, original.counters);
    assert_eq!(claimed.next_due_at_ms, original.next_due_at_ms);
    assert_eq!(claimed.enabled, original.enabled);
    assert!(saved.schedule_fires.is_empty());
    assert_eq!(limiter.available_permits(), 0);

    gate.release();
    within(
        "the direct caller to finish without waiting for its automation",
        parent,
    )
    .await
    .unwrap()
    .unwrap();
    gate.entered().await;
    gate.release();
    within(
        "the admitted automation to settle after the caller releases its permits",
        service.drain(),
    )
    .await;
    let guard = state.read().await;
    let after = &guard.schedules[&original.id];
    assert_eq!(after.next_due_at_ms, original.next_due_at_ms);
    assert_eq!(after.enabled, original.enabled);
    assert_eq!(after.counters.runs, 1);
    assert_eq!(
        after.last_safe_outcome.as_ref().unwrap().status,
        ScheduleOutcomeStatus::Silent
    );
    let fires = guard.schedule_fires.for_schedule(&original.id);
    assert_eq!(fires.len(), 1);
    assert!(fires[0].manual);
    assert!(fires[0].run_id.is_some());
    assert_eq!(limiter.available_permits(), 1);
}

#[tokio::test]
async fn fp04_trigger_rechecks_authoritative_source_and_current_grant_without_changing_occurrences()
{
    for case in [
        "schedule",
        "job",
        "delegation",
        "peer",
        "parent",
        "foreign",
        "session",
        "missing",
        "stopped",
        "terminal",
        "revoked",
    ] {
        let model = ScriptedModel::new(vec![]);
        let (service, state, agent, _) = fixture(model.clone());
        let mut original = test_automation(&agent, "mine");
        original.counters = AutomationCounters {
            runs: 4,
            failures: 2,
            consecutive_failures: 1,
        };
        state
            .write()
            .await
            .schedules
            .insert(original.id.clone(), original.clone());
        let mut link = ledger_run(&service.inner.runs, &agent, "chat:fp04-source").await;
        {
            let mut guard = state.write().await;
            let record = guard.runs.get_mut(&link.run_id).unwrap();
            match case {
                "schedule" => record.source = RunSource::Schedule,
                "job" => record.source = RunSource::Job,
                "delegation" => record.source = RunSource::Delegation,
                "peer" => record.source = RunSource::Peer,
                "parent" => record.parent_run_id = Some("parent-run".into()),
                "foreign" => record.agent_id = "foreign-agent".into(),
                "session" => link.session_id = "chat:forged".into(),
                "missing" => link.run_id = "missing-run".into(),
                "stopped" => {
                    record.stop = Some(RunStopRequest {
                        requested_at_ms: now_ms(),
                    })
                }
                "terminal" => record.status = RunStatus::Completed,
                "revoked" => {
                    let runtime = guard.agents.get_mut(&agent).unwrap();
                    let mut config = runtime.config().clone();
                    config.tools = None;
                    runtime.replace_config(config);
                }
                _ => unreachable!(),
            }
        }
        let fires = state.read().await.schedule_fires.snapshot();
        let result = service
            .inner
            .runs
            .trigger_automation_from_run(
                &agent,
                trigger_authority(&link, &original.id),
                &original.id,
            )
            .await;
        assert!(
            matches!(result, Err(ScheduleError::Rejected(_))),
            "{case}: {result:?}"
        );
        assert_eq!(
            state.read().await.schedules[&original.id],
            original,
            "{case}"
        );
        assert_eq!(
            state.read().await.schedule_fires.snapshot(),
            fires,
            "{case}"
        );
        assert!(service.inner.jobs.lock().await.is_empty(), "{case}");
        assert!(model.requests().is_empty(), "{case}");
    }
}

#[tokio::test]
async fn fp04_trigger_rechecks_a_grant_revoked_while_waiting_for_the_claim_transaction() {
    let model = ScriptedModel::new(vec![]);
    let (service, state, agent, _) = fixture(model.clone());
    let original = test_automation(&agent, "mine");
    state
        .write()
        .await
        .schedules
        .insert(original.id.clone(), original.clone());
    let link = ledger_run(&service.inner.runs, &agent, "chat:fp04-revocation").await;
    let transaction = service.inner.runs.control_plane_transaction().await;
    let caller = service.inner.runs.clone();
    let source_agent = agent.clone();
    let pending = tokio::spawn(async move {
        caller
            .trigger_automation_from_run(&source_agent, trigger_authority(&link, "mine"), "mine")
            .await
    });
    within(
        "trigger admission to wait at the claim transaction",
        async {
            // Admission holds the scheduler jobs lock before acquiring this
            // transaction. Observing that lock occupied proves the call reached
            // the claim boundary while its source grant was still present.
            loop {
                if service.inner.jobs.try_lock().is_err() {
                    break;
                }
                tokio::task::yield_now().await;
            }
        },
    )
    .await;
    {
        let mut guard = state.write().await;
        let runtime = guard.agents.get_mut(&agent).unwrap();
        let mut config = runtime.config().clone();
        config.tools = None;
        runtime.replace_config(config);
    }
    drop(transaction);
    let result = within("the revoked trigger admission to settle", pending)
        .await
        .unwrap();
    assert!(
        matches!(result, Err(ScheduleError::Rejected(_))),
        "{result:?}"
    );
    assert_eq!(state.read().await.schedules[&original.id], original);
    assert!(service.inner.jobs.lock().await.is_empty());
    assert!(model.requests().is_empty());
}

#[tokio::test]
async fn a8_trigger_rechecks_policy_and_cancel_after_waiting_for_claim_transaction() {
    use crate::approvals::{ApprovalPolicy, PolicyAction, RiskClass};
    for case in ["deny", "ask", "cancel"] {
        let model = ScriptedModel::new(vec![Step::Text(vec![CHECKIN_SENTINEL])]);
        let (service, state, agent, _) = fixture(model.clone());
        let file = SnapshotFile::new();
        let original = test_automation(&agent, "mine");
        {
            let mut guard = state.write().await;
            guard
                .schedules
                .insert(original.id.clone(), original.clone());
            guard.approvals.set_policy(
                &agent,
                ApprovalPolicy::default().with(RiskClass::Delegate, PolicyAction::Allow),
            );
            guard.set_control_plane_store(Some(ControlPlaneStoreConfig::Json(file.0.clone())));
        }
        let source = ledger_run(&service.inner.runs, &agent, "chat:a8-trigger-authority").await;
        let cancel = anima_core::CancelSignal::new();
        let context = gated_context(&service.inner.runs, &source, cancel.clone()).await;
        let owner = state.read().await.get_agent(&agent).unwrap().state;
        state
            .write()
            .await
            .control_plane_persist_request()
            .save()
            .await
            .unwrap();
        let bytes = std::fs::read(&file.0).unwrap();
        let transaction = service.inner.runs.control_plane_transaction().await;
        let input = tool_input(&agent, &source.session_id);
        let pending = tokio::spawn(async move {
            context
                .execute_tool(owner, input, trigger_call("mine"))
                .await
        });
        within("trigger dispatch to reach the claim transaction", async {
            loop {
                if service.inner.jobs.try_lock().is_err() {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await;
        if case == "cancel" {
            cancel.cancel();
        } else {
            state.write().await.approvals.set_policy(
                &agent,
                ApprovalPolicy::default().with(
                    RiskClass::Delegate,
                    if case == "deny" {
                        PolicyAction::Deny
                    } else {
                        PolicyAction::Ask
                    },
                ),
            );
        }
        drop(transaction);
        let result = within("obsolete trigger dispatch to settle", pending)
            .await
            .unwrap();
        // Settle even an erroneously admitted finite child before an assertion
        // fails; the regression never leaves a scheduler task behind.
        within("any admitted trigger work to settle", service.drain()).await;
        assert_eq!(
            result.status,
            anima_core::TaskStatus::Error,
            "{case}: {result:?}"
        );
        assert_eq!(
            state.read().await.schedules[&original.id],
            original,
            "{case}"
        );
        assert_eq!(std::fs::read(&file.0).unwrap(), bytes, "{case}");
        assert!(
            state.read().await.schedule_fires.snapshot().is_empty(),
            "{case}"
        );
        assert!(service.inner.jobs.lock().await.is_empty(), "{case}");
        assert!(model.requests().is_empty(), "{case}");
        assert!(model.secondary_requests().is_empty(), "{case}");
    }
}

#[tokio::test]
async fn a8_trigger_actual_allow_once_approval_reaches_the_shared_claim_boundary() {
    use crate::approvals::{ApprovalDecisionKind, ApprovalPolicy, PolicyAction, RiskClass};
    let model = ScriptedModel::new(vec![
        Step::Text(vec![CHECKIN_SENTINEL]),
        Step::Panic("only one approved automation may run"),
    ]);
    let (service, state, agent, _) = fixture(model.clone());
    let original = test_automation(&agent, "mine");
    {
        let mut guard = state.write().await;
        guard
            .schedules
            .insert(original.id.clone(), original.clone());
        guard.approvals.set_policy(
            &agent,
            ApprovalPolicy::default().with(RiskClass::Delegate, PolicyAction::Ask),
        );
    }
    let source = ledger_run(&service.inner.runs, &agent, "chat:a8-trigger-approved").await;
    let context = gated_context(
        &service.inner.runs,
        &source,
        anima_core::CancelSignal::new(),
    )
    .await;
    let owner = state.read().await.get_agent(&agent).unwrap().state;
    let input = tool_input(&agent, &source.session_id);
    let pending = tokio::spawn(async move {
        context
            .execute_tool(owner, input, trigger_call("mine"))
            .await
    });
    let approvals = pending_approvals(&service.inner.runs, 1).await;
    assert_eq!(approvals[0].tool, "trigger_automation");
    assert_eq!(state.read().await.schedules["mine"], original);
    assert!(model.requests().is_empty());
    service
        .inner
        .runs
        .decide_approval(
            &approvals[0].id,
            decision(ApprovalDecisionKind::AllowOnce, approvals[0].revision),
        )
        .await
        .unwrap();
    let result = within("approved trigger to return its claim", pending)
        .await
        .unwrap();
    within("approved automation to settle", service.drain()).await;
    assert_eq!(result.status, anima_core::TaskStatus::Success, "{result:?}");
    assert_eq!(state.read().await.schedules["mine"].counters.runs, 1);
    assert_eq!(model.requests().len(), 1);
    assert!(model.secondary_requests().is_empty());
}

#[tokio::test]
async fn fp04_trigger_uses_the_same_occurrence_ownership_and_admission_cap_as_run_now() {
    let gate = Gate::new();
    let model = ScriptedModel::gated(vec![Step::Text(vec![CHECKIN_SENTINEL])], gate.clone());
    let (service, state, agent, _) = fixture(model);
    let mut original = test_automation(&agent, "mine");
    original.next_due_at_ms = now_ms() - 1;
    state
        .write()
        .await
        .schedules
        .insert(original.id.clone(), original.clone());
    let link = ledger_run(&service.inner.runs, &agent, "chat:fp04-shared").await;
    service.run_now(&agent, &original.id).await.unwrap();
    gate.entered().await;
    assert_eq!(
        service
            .inner
            .runs
            .trigger_automation_from_run(
                &agent,
                trigger_authority(&link, &original.id),
                &original.id
            )
            .await,
        Err(ScheduleError::Conflict(AUTOMATION_ALREADY_RUNNING))
    );
    assert_eq!(
        SchedulerService::tick_inner(&service.inner, now_ms())
            .await
            .unwrap(),
        0,
        "automatic tick shares ownership of the same occurrence"
    );
    gate.release();
    within("the shared manual occurrence to finish", service.drain()).await;
    assert_eq!(state.read().await.schedules[&original.id].counters.runs, 1);
    assert_eq!(
        state
            .read()
            .await
            .schedule_fires
            .for_schedule(&original.id)
            .len(),
        1
    );

    let before = state.read().await.schedules[&original.id].clone();
    {
        let mut jobs = service.inner.jobs.lock().await;
        for n in 0..MAX_ACTIVE_SCHEDULES {
            jobs.insert(
                format!("fp04-busy-{n}"),
                tokio::spawn(std::future::pending::<()>()),
            );
        }
    }
    let result = service
        .inner
        .runs
        .trigger_automation_from_run(&agent, trigger_authority(&link, &original.id), &original.id)
        .await;
    // Own and settle these synthetic cap occupiers before asserting the result.
    for (_, job) in std::mem::take(&mut *service.inner.jobs.lock().await) {
        job.abort();
        let _ = job.await;
    }
    assert_eq!(
        result,
        Err(ScheduleError::Busy(TOO_MANY_RUNNING_AUTOMATIONS))
    );
    assert_eq!(state.read().await.schedules[&original.id], before);
}

#[tokio::test]
async fn fp04_failed_trigger_admission_save_restores_the_occurrence_and_starts_no_job() {
    let model = ScriptedModel::new(vec![]);
    let (service, state, agent, _) = fixture(model.clone());
    let original = test_automation(&agent, "mine");
    state
        .write()
        .await
        .schedules
        .insert(original.id.clone(), original.clone());
    let link = ledger_run(&service.inner.runs, &agent, "chat:fp04-unsaved").await;
    let directory =
        std::env::temp_dir().join(format!("anima-fp04-broken-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    state
        .write()
        .await
        .set_control_plane_store(Some(ControlPlaneStoreConfig::Json(directory.clone())));
    let result = service
        .inner
        .runs
        .trigger_automation_from_run(&agent, trigger_authority(&link, &original.id), &original.id)
        .await;
    std::fs::remove_dir(&directory).unwrap();
    assert_eq!(result, Err(ScheduleError::Persistence));
    assert_eq!(state.read().await.schedules[&original.id], original);
    assert!(state.read().await.schedule_fires.snapshot().is_empty());
    assert!(service.inner.jobs.lock().await.is_empty());
    assert!(model.requests().is_empty());
}

#[tokio::test]
async fn a_tool_trigger_is_refused_while_draining_and_claims_nothing() {
    let model = ScriptedModel::new(vec![]);
    let (service, state, agent, _) = fixture(model.clone());
    let original = test_automation(&agent, "mine");
    state
        .write()
        .await
        .schedules
        .insert(original.id.clone(), original.clone());
    let link = ledger_run(&service.inner.runs, &agent, "chat:drain-trigger").await;
    service.inner.runs.begin_drain();
    let result = service
        .inner
        .runs
        .trigger_automation_from_run(&agent, trigger_authority(&link, &original.id), &original.id)
        .await;
    assert_eq!(result, Err(ScheduleError::Draining));
    assert_eq!(state.read().await.schedules[&original.id], original);
    assert!(service.inner.jobs.lock().await.is_empty());
    assert!(model.requests().is_empty());
}
