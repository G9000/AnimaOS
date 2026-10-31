//! Public outcome criteria exercised through the durable native run coordinator.

use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use anima_core::outcomes::{
    OutcomeAcceptance, OutcomeCriterion, OutcomeDefinition, OutcomeStatus, OutcomeVerdict,
};
use anima_core::{Content, DataValue, Message, MessageRole, ModelAdapter, TaskStatus, ToolCall};
use tokio::sync::{RwLock, Semaphore};

use crate::agent_runs::test_support::{
    accept, add_chat, companion_config, coordinator_state, ScriptedModel, Step,
};
use crate::agent_runs::{AcceptedRun, AgentRunCoordinator, QueuedRunStart};
use crate::approvals::{ApprovalPolicy, PolicyAction, RiskClass};
use crate::control_plane_store::{
    load_control_plane_snapshot, ControlPlaneSnapshot, ControlPlaneStoreConfig, WorkspaceConfig,
};
use crate::runs::{
    ActionDurability, ActionReceipt, ActionStatus, RunRecord, RunSource, RunStart, RunStatus,
};
use crate::state::DaemonState;

const SESSION: &str = "chat:outcome-regression";
const WRONG: &str = "Total:41\n";
const CORRECT: &str = "Total:42\n";

fn call(
    id: &str,
    name: &str,
    args: impl IntoIterator<Item = (&'static str, DataValue)>,
) -> ToolCall {
    ToolCall {
        id: id.into(),
        name: name.into(),
        args: args
            .into_iter()
            .map(|(key, value)| (key.into(), value))
            .collect(),
    }
}

fn write(id: &str, text: &str) -> ToolCall {
    call(
        id,
        "write_file",
        [
            ("file_path", DataValue::String("report.txt".into())),
            ("content", DataValue::String(text.into())),
        ],
    )
}

fn file_definition(acceptance: OutcomeAcceptance) -> OutcomeDefinition {
    OutcomeDefinition {
        criteria: vec![OutcomeCriterion::FileText {
            id: "correct-total".into(),
            path: "report.txt".into(),
            contains: "Total:42".into(),
        }],
        max_corrections: 2,
        acceptance,
    }
}

struct Fixture {
    _private: tempfile::TempDir,
    root: PathBuf,
    store: ControlPlaneStoreConfig,
    coordinator: AgentRunCoordinator,
    agent: String,
}

impl Fixture {
    fn new(model: Arc<dyn ModelAdapter>) -> Self {
        let private = tempfile::tempdir().unwrap();
        #[cfg(windows)]
        {
            let sid = win_custody::current_user_sid_string().unwrap();
            win_custody::SecurityDescriptor::from_sddl(&format!(
                "D:P(A;OICI;FA;;;{sid})(A;OICI;FA;;;SY)"
            ))
            .unwrap()
            .apply_to_tree(private.path(), true)
            .unwrap();
        }
        let root = private.path().join("workspace");
        std::fs::create_dir_all(&root).unwrap();
        let store = ControlPlaneStoreConfig::Json(private.path().join("control-plane.json"));
        let mut state = DaemonState::with_model_adapter(model);
        state.replace_memory_embeddings(crate::memory_embeddings::MemoryEmbeddingRuntime::disabled());
        state.workspace = Some(WorkspaceConfig {
            root_path: root.clone(),
            company_name: "Synthetic outcome regression".into(),
            mission: "Check isolated saved bytes".into(),
            values: vec![],
        });
        state.set_control_plane_store(Some(store.clone()));
        state.checkpoint_root = Some(private.path().join("checkpoints"));
        let mut config = companion_config("Outcome regression");
        config.tools = Some(
            state
                .tool_registry
                .resolve_descriptors([
                    "read_file",
                    "write_file",
                    "edit_file",
                    "todo_read",
                    "todo_write",
                ])
                .unwrap(),
        );
        let settings = config.settings.as_mut().unwrap();
        settings.max_tool_iterations = Some(12);
        settings.max_retries = Some(0);
        settings.max_tokens = Some(2048);
        settings.timeout_ms = Some(30_000);
        for key in [
            "autoTitle",
            "autoCompact",
            "agentFilesProfileEnabled",
            "completionReview",
        ] {
            settings
                .additional
                .insert(key.into(), DataValue::Bool(false));
        }
        let agent = state.create_agent(config).unwrap().state.id;
        state.approvals.set_policy(
            &agent,
            ApprovalPolicy::default()
                .with(RiskClass::Read, PolicyAction::Allow)
                .with(RiskClass::Write, PolicyAction::Allow),
        );
        let coordinator =
            AgentRunCoordinator::new(Arc::new(RwLock::new(state)), Arc::new(Semaphore::new(1)));
        Self {
            _private: private,
            root,
            store,
            coordinator,
            agent,
        }
    }

    async fn run(&self, definition: OutcomeDefinition) -> RunRecord {
        add_chat(&self.coordinator, &self.agent, SESSION).await;
        let mut request = accept(&self.agent, SESSION, "make-synthetic-report");
        request.outcome = Some(definition);
        let start = self.coordinator.web_start(
            self.agent.clone(),
            SESSION.into(),
            request.text.clone(),
            request.idempotency_key.clone(),
        );
        let run_id = match self.coordinator.accept_run(request, start).await.unwrap() {
            AcceptedRun::Created(record) => record.id,
            other => panic!("expected a newly accepted outcome run, got {other:?}"),
        };
        let state = coordinator_state(&self.coordinator);
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let Some(record) = state
                    .read()
                    .await
                    .runs
                    .get(&run_id)
                    .filter(|record| record.status.is_terminal() && record.terminal_published)
                    .cloned()
                {
                    return record;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the bounded scripted outcome run must publish a terminal result")
    }

    async fn saved(&self) -> ControlPlaneSnapshot {
        load_control_plane_snapshot(&self.store)
            .await
            .unwrap()
            .unwrap()
    }

    async fn failure_details(&self, record: &RunRecord) -> String {
        let state = coordinator_state(&self.coordinator);
        let guard = state.read().await;
        let tools = guard.agents[&self.agent]
            .messages()
            .iter()
            .rev()
            .filter(|message| message.role == MessageRole::Tool)
            .take(8)
            .map(|message| message.content.text.chars().take(256).collect::<String>())
            .collect::<Vec<_>>();
        let checks = record
            .outcome
            .as_ref()
            .into_iter()
            .flat_map(|outcome| outcome.iterations.iter().rev().take(3))
            .flat_map(|iteration| &iteration.observations)
            .take(8)
            .map(|observation| {
                (
                    observation.verdict,
                    observation.detail.chars().take(256).collect::<String>(),
                )
            })
            .collect::<Vec<_>>();
        let receipts = record
            .action_receipts
            .iter()
            .rev()
            .take(8)
            .map(|receipt| (&receipt.name, receipt.status, receipt.durability))
            .collect::<Vec<_>>();
        format!("status={:?}; error={:?}; outcome={:?}; file_bytes={:?}; checks={checks:?}; tools={tools:?}; receipts={receipts:?}",
            record.status, record.error.as_ref().map(|error|
                (&error.code, error.message.chars().take(256).collect::<String>())),
            record.outcome.as_ref().map(|outcome| (outcome.status, outcome.consumed_corrections)),
            std::fs::metadata(self.root.join("report.txt")).ok().map(|metadata| metadata.len()))
    }

    async fn assert_quiet_restore(&self, record: &RunRecord) {
        let saved = self.saved().await;
        let stored = saved.runs.iter().find(|run| run.id == record.id).unwrap();
        assert_eq!(
            stored.outcome, record.outcome,
            "native outcome must survive the terminal save"
        );
        assert_eq!(stored.action_receipts, record.action_receipts);
        let before_messages = saved
            .agents
            .iter()
            .find(|agent| agent.state.id == self.agent)
            .unwrap()
            .messages
            .clone();
        let before_budgets = saved.execution_budgets.clone();
        let model = ScriptedModel::new(vec![]);
        let mut restored = DaemonState::with_model_adapter(model.clone());
        restored.replace_memory_embeddings(crate::memory_embeddings::MemoryEmbeddingRuntime::disabled());
        restored.restore_control_plane_snapshot(saved).unwrap();
        let restored =
            AgentRunCoordinator::new(Arc::new(RwLock::new(restored)), Arc::new(Semaphore::new(1)));
        tokio::time::sleep(Duration::from_millis(40)).await;
        let state = coordinator_state(&restored);
        let guard = state.read().await;
        let restored_outcome = guard
            .runs
            .get(&record.id)
            .unwrap()
            .outcome
            .as_ref()
            .unwrap();
        let prior = record.outcome.as_ref().unwrap();
        assert_eq!(restored_outcome.iterations, prior.iterations);
        assert_eq!(
            restored_outcome.consumed_corrections,
            prior.consumed_corrections
        );
        if matches!(
            prior.status,
            OutcomeStatus::CriteriaMet | OutcomeStatus::AwaitingOwner | OutcomeStatus::Accepted
        ) {
            assert_eq!(restored_outcome.status, OutcomeStatus::NeedsReview);
            assert!(restored_outcome.accepted_report_sha256.is_none());
        } else {
            assert_eq!(restored_outcome, prior);
        }
        assert_eq!(guard.agents[&self.agent].messages(), before_messages);
        assert_eq!(guard.execution_budgets, before_budgets);
        assert!(
            model.requests().is_empty(),
            "restoration must not replay producer work"
        );
        assert!(model.secondary_requests().is_empty());
    }
}

#[tokio::test]
async fn outcome_retry_at_retained_capacity_replays_without_dispatch_and_rejects_new_or_changed_requests(
) {
    let model = ScriptedModel::new(vec![]);
    let fixture = Fixture::new(model.clone());
    add_chat(&fixture.coordinator, &fixture.agent, SESSION).await;
    let room = fixture
        .coordinator
        .try_reserve_room(&fixture.agent, SESSION)
        .unwrap();
    let definition = OutcomeDefinition {
        criteria: vec![OutcomeCriterion::ResponseText {
            id: "result".into(),
            contains: "Ready".into(),
        }],
        max_corrections: 0,
        acceptance: OutcomeAcceptance::ReportOnly,
    };
    let state = coordinator_state(&fixture.coordinator);
    let live_agents = std::collections::HashSet::from([fixture.agent.clone()]);
    {
        let mut guard = state.write().await;
        let now = anima_core::primitives::now_millis();
        for index in 0..15 {
            let key = format!("retained-outcome-{index}");
            let mut retained = RunRecord::running(
                RunStart {
                    agent_id: fixture.agent.clone(),
                    session_id: SESSION.into(),
                    source: RunSource::Web,
                    source_ref: None,
                    idempotency_key: Some(key.clone()),
                    text: key,
                    model: "gpt-5.4".into(),
                    provider: Some("openai".into()),
                    parent_run_id: None,
                },
                now,
            );
            super::attach(&mut retained, definition.clone(), Some(&fixture.root)).unwrap();
            retained
                .outcome
                .as_mut()
                .unwrap()
                .hold(OutcomeStatus::NeedsReview)
                .unwrap();
            retained.finish(
                RunStatus::Failed,
                Some(crate::runs::RunError {
                    code: crate::runs::RUN_FAILED.into(),
                    message: "Retained for owner review".into(),
                }),
                now,
            );
            guard.runs.insert(retained);
        }
        let retained = guard.runs.snapshot_records(&live_agents);
        assert_eq!(retained.len(), 15);
        crate::runs::RunLedger::validate(&retained).unwrap();
    }
    let mut original = accept(&fixture.agent, SESSION, "capacity-original");
    original.outcome = Some(definition.clone());
    let start = fixture.coordinator.web_start(
        fixture.agent.clone(),
        SESSION.into(),
        original.text.clone(),
        original.idempotency_key.clone(),
    );
    let accepted = match fixture
        .coordinator
        .accept_run(original.clone(), start)
        .await
        .unwrap()
    {
        AcceptedRun::Created(record) => record,
        other => panic!("the sixteenth outcome must be newly admitted, got {other:?}"),
    };
    assert_eq!(accepted.status, RunStatus::Queued);
    let before = fixture.saved().await;
    assert_eq!(
        before.runs.len(),
        16,
        "the actual admission must durably reach the exact cap"
    );
    let dispatches = Arc::new(AtomicUsize::new(0));
    let unexpected_start = || {
        let dispatches = Arc::clone(&dispatches);
        QueuedRunStart::new(move |_| {
            dispatches.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Ok(()) })
        })
    };
    match fixture
        .coordinator
        .accept_run(original.clone(), unexpected_start())
        .await
        .unwrap()
    {
        AcceptedRun::Replayed(record) => {
            assert_eq!(record.id, accepted.id);
            assert_eq!(record.outcome.as_ref().unwrap().definition, definition);
        }
        other => panic!("an exact retry at capacity must rejoin its accepted run, got {other:?}"),
    }
    let mut seventeenth = accept(&fixture.agent, SESSION, "capacity-seventeenth");
    seventeenth.outcome = Some(definition.clone());
    let refused = fixture
        .coordinator
        .accept_run(seventeenth, unexpected_start())
        .await
        .unwrap_err();
    assert_eq!(refused.status(), axum::http::StatusCode::CONFLICT);
    assert!(refused
        .message()
        .contains("Outcome review capacity reached"));
    let mut conflicting = original;
    conflicting.outcome.as_mut().unwrap().criteria[0] = OutcomeCriterion::ResponseText {
        id: "result".into(),
        contains: "Changed requirement".into(),
    };
    let reused = fixture
        .coordinator
        .accept_run(conflicting, unexpected_start())
        .await
        .unwrap_err();
    assert_eq!(reused.status(), axum::http::StatusCode::CONFLICT);
    assert_eq!(reused.message(), crate::agent_runs::IDEMPOTENCY_KEY_REUSED);
    {
        let guard = state.read().await;
        assert_eq!(guard.runs.snapshot_records(&live_agents).len(), 16);
        assert_eq!(guard.execution_budgets, before.execution_budgets);
        assert_eq!(
            guard.agents[&fixture.agent].messages(),
            before
                .agents
                .iter()
                .find(|agent| agent.state.id == fixture.agent)
                .unwrap()
                .messages
        );
    }
    assert_eq!(
        serde_json::to_value(fixture.saved().await).unwrap(),
        serde_json::to_value(before).unwrap(),
        "replays and rejected admissions must leave the saved control plane unchanged"
    );
    fixture
        .coordinator
        .stop_run(&fixture.agent, &accepted.id)
        .await
        .unwrap();
    drop(room);
    tokio::time::timeout(Duration::from_secs(5), async {
        while fixture
            .coordinator
            .has_session_queue(&fixture.agent, SESSION)
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the cancelled original queue must drain");
    assert_eq!(dispatches.load(Ordering::SeqCst), 0);
    assert!(model.requests().is_empty());
    assert!(model.secondary_requests().is_empty());
}

#[tokio::test]
async fn outcome_native_file_correction_checks_saved_bytes_and_keeps_one_execution_scope() {
    let model = ScriptedModel::new(vec![
        Step::Tools(vec![write("draft", WRONG)]),
        Step::Text(vec!["done"]),
        Step::Tools(vec![call(
            "repair",
            "edit_file",
            [
                ("file_path", DataValue::String("report.txt".into())),
                ("old_string", DataValue::String(WRONG.into())),
                ("new_string", DataValue::String(CORRECT.into())),
            ],
        )]),
        Step::Text(vec!["done"]),
    ]);
    let fixture = Fixture::new(model.clone());
    let record = fixture
        .run(file_definition(OutcomeAcceptance::ReportOnly))
        .await;
    assert_eq!(
        record.status,
        RunStatus::Completed,
        "{}",
        fixture.failure_details(&record).await
    );
    assert_eq!(
        std::fs::read(fixture.root.join("report.txt")).unwrap(),
        CORRECT.as_bytes()
    );
    let outcome = record.outcome.as_ref().unwrap();
    assert_eq!(outcome.status, OutcomeStatus::CriteriaMet);
    assert_eq!(outcome.consumed_corrections, 1);
    assert_eq!(outcome.iterations.len(), 2);
    assert_eq!(
        outcome.iterations[0].observations[0].verdict,
        OutcomeVerdict::NotMet
    );
    assert_eq!(
        outcome.iterations[1].observations[0].verdict,
        OutcomeVerdict::Met
    );
    assert_ne!(
        outcome.iterations[0].observations[0].subject_sha256,
        outcome.iterations[1].observations[0].subject_sha256
    );
    assert_eq!(
        outcome.iterations[1].observations[0].subject_sha256,
        Some(super::digest(CORRECT.as_bytes()))
    );
    assert_eq!(outcome.root_run_id, record.id);
    assert!(outcome.accepted_report_sha256.is_none());
    assert_eq!(model.requests().len(), 4);
    assert!(model.requests()[2].messages.iter().any(|message| message
        .content
        .text
        .contains("Public outcome checks require correction:")));
    assert!(
        model.secondary_requests().is_empty(),
        "deterministic checks need no judge provider"
    );
    assert_eq!(record.action_receipts.len(), 2);
    assert!(record
        .action_receipts
        .iter()
        .all(|receipt| receipt.status == ActionStatus::Succeeded
            && receipt.durability == ActionDurability::Persistent));
    let saved = fixture.saved().await;
    assert_eq!(saved.execution_budgets.scopes.len(), 1);
    assert_eq!(saved.execution_budgets.run_scopes.len(), 1);
    let scope_id = &saved.execution_budgets.run_scopes[&record.id];
    let scope = &saved.execution_budgets.scopes[scope_id];
    assert_eq!(scope.root_run_id, record.id);
    assert!(scope
        .reservations
        .values()
        .all(|reservation| reservation.request.run_id == record.id));
    assert_eq!(
        scope
            .reservations
            .values()
            .filter(|reservation| reservation
                .request
                .logical_call_id
                .starts_with("outcome-check:"))
            .count(),
        2,
        "independent checks must debit the producer's canonical scope"
    );
    assert_eq!(scope.consumption().unwrap().model_attempts, 4);
    fixture.assert_quiet_restore(&record).await;
}

fn todos(id: &str, content: &str) -> ToolCall {
    call(
        id,
        "todo_write",
        [(
            "todos",
            DataValue::Array(vec![DataValue::Object(
                [
                    ("content".into(), DataValue::String(content.into())),
                    (
                        "activeForm".into(),
                        DataValue::String("Recording synthetic effect".into()),
                    ),
                    ("status".into(), DataValue::String("pending".into())),
                ]
                .into(),
            )]),
        )],
    )
}

#[tokio::test]
async fn outcome_correction_denies_a_second_native_effect_even_with_a_different_call_key() {
    // todo_write is an inert local witness: no connector or external account is contacted.
    let model = ScriptedModel::new(vec![
        Step::Tools(vec![call("read-before-effect", "todo_read", [])]),
        Step::Tools(vec![todos(
            "confirmed-effect",
            "Initial effect happened once",
        )]),
        Step::Text(vec!["done"]),
        Step::Tools(vec![todos(
            "new-key-same-effect",
            "Repeated effect must never appear",
        )]),
        Step::Text(vec!["Delivery acknowledged"]),
    ]);
    let fixture = Fixture::new(model.clone());
    let record = fixture
        .run(OutcomeDefinition {
            criteria: vec![OutcomeCriterion::ResponseText {
                id: "acknowledgement".into(),
                contains: "Delivery acknowledged".into(),
            }],
            max_corrections: 2,
            acceptance: OutcomeAcceptance::ReportOnly,
        })
        .await;
    assert_eq!(record.status, RunStatus::Completed, "{:?}", record.error);
    let outcome = record.outcome.as_ref().unwrap();
    assert_eq!(outcome.status, OutcomeStatus::CriteriaMet);
    assert_eq!(outcome.consumed_corrections, 1);
    let saved_todos =
        crate::tools::todo::read_agent_todos(Some(&fixture.root), &fixture.agent).unwrap();
    assert_eq!(saved_todos.tasks.len(), 1);
    assert_eq!(saved_todos.tasks[0].content, "Initial effect happened once");
    let effects: Vec<_> = record
        .action_receipts
        .iter()
        .filter(|receipt| receipt.name == "todo_write")
        .collect();
    assert_eq!(
        effects.len(),
        1,
        "denied corrections must never enter the native handler"
    );
    assert_eq!(effects[0].tool_call_id, "confirmed-effect");
    assert_eq!(effects[0].status, ActionStatus::Succeeded);
    let state = coordinator_state(&fixture.coordinator);
    let guard = state.read().await;
    assert!(
        guard.agents[&fixture.agent]
            .messages()
            .iter()
            .any(|message| {
                message.role == MessageRole::Tool
                    && message
                        .content
                        .text
                        .contains("Outcome correction permits only")
                    && message
                        .content
                        .metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("toolCallId"))
                        == Some(&DataValue::String("new-key-same-effect".into()))
            }),
        "the actual native dispatch boundary must return the correction denial"
    );
    drop(guard);
    assert_eq!(model.requests().len(), 5);
    fixture.assert_quiet_restore(&record).await;
}

#[tokio::test]
async fn outcome_unmet_saved_bytes_exhaust_exactly_two_corrections_without_success() {
    let model = ScriptedModel::new(vec![
        Step::Tools(vec![write("wrong-initial", WRONG)]),
        Step::Text(vec!["done"]),
        Step::Tools(vec![write("wrong-correction-one", "Total:40\n")]),
        Step::Text(vec!["done"]),
        Step::Tools(vec![write("wrong-correction-two", "Total:39\n")]),
        Step::Text(vec!["done"]),
        Step::Tools(vec![write("must-never-run", CORRECT)]),
    ]);
    let fixture = Fixture::new(model.clone());
    let record = fixture
        .run(file_definition(OutcomeAcceptance::ReportOnly))
        .await;
    assert_eq!(record.status, RunStatus::Failed);
    assert!(
        record.reply_message_id.is_none(),
        "exhaustion cannot publish a successful reply: {}",
        fixture.failure_details(&record).await
    );
    assert!(!record.allows_outcome_review_delivery());
    let outcome = record.outcome.as_ref().unwrap();
    assert_eq!(outcome.status, OutcomeStatus::Exhausted);
    assert_eq!(outcome.consumed_corrections, 2);
    assert_eq!(outcome.iterations.len(), 3);
    assert!(outcome
        .iterations
        .iter()
        .all(|iteration| iteration.observations[0].verdict == OutcomeVerdict::NotMet));
    assert!(outcome.accepted_report_sha256.is_none());
    assert_eq!(
        model.requests().len(),
        6,
        "no fourth check or third correction is admitted"
    );
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("report.txt")).unwrap(),
        "Total:39\n"
    );
    assert_eq!(record.action_receipts.len(), 3);
    fixture.assert_quiet_restore(&record).await;
}

#[tokio::test]
async fn outcome_owner_required_report_stays_unaccepted_and_refuses_changed_saved_bytes() {
    let model = ScriptedModel::new(vec![
        Step::Tools(vec![write("correct-first-time", CORRECT)]),
        Step::Text(vec!["done"]),
    ]);
    let fixture = Fixture::new(model.clone());
    let record = fixture
        .run(file_definition(OutcomeAcceptance::OwnerRequired))
        .await;
    assert_ne!(
        record.status,
        RunStatus::Completed,
        "owner acceptance must gate downstream success"
    );
    let outcome = record.outcome.as_ref().unwrap();
    assert_eq!(
        outcome.status,
        OutcomeStatus::AwaitingOwner,
        "{}",
        fixture.failure_details(&record).await
    );
    assert!(record.reply_message_id.is_some());
    assert!(record.allows_outcome_review_delivery());
    assert_eq!(outcome.consumed_corrections, 0);
    assert!(outcome.accepted_report_sha256.is_none());
    fixture.assert_quiet_restore(&record).await;
    let state = coordinator_state(&fixture.coordinator);
    let report = super::owner_report(
        &state,
        fixture.coordinator.control_plane_transactions(),
        &fixture.agent,
        &record.id,
        None,
    )
    .await
    .unwrap();
    assert_eq!(report.status, OutcomeStatus::AwaitingOwner);
    assert!(report.accepted_report_sha256.is_none());
    std::fs::write(fixture.root.join("report.txt"), WRONG).unwrap();
    let acceptance = super::owner_report(
        &state,
        fixture.coordinator.control_plane_transactions(),
        &fixture.agent,
        &record.id,
        Some((report.revision, report.report_sha256.unwrap())),
    )
    .await;
    assert!(
        acceptance.is_err(),
        "a previously good report cannot accept changed source bytes"
    );
    let after = state.read().await.runs.get(&record.id).unwrap().clone();
    let outcome = after.outcome.as_ref().unwrap();
    assert_eq!(outcome.status, OutcomeStatus::NeedsReview);
    assert!(outcome.accepted_report_sha256.is_none());
    assert_eq!(after.reply_message_id, record.reply_message_id);
    assert_eq!(after.reply_sha256, record.reply_sha256);
    assert_eq!(
        outcome.iterations,
        record.outcome.as_ref().unwrap().iterations
    );
    assert!(!after.allows_outcome_review_delivery());
    fixture.assert_quiet_restore(&after).await;
    let closed = super::close_report(
        &state,
        fixture.coordinator.control_plane_transactions(),
        &fixture.agent,
        &record.id,
        outcome.revision,
    )
    .await
    .unwrap();
    assert_eq!(
        closed.status,
        OutcomeStatus::Stopped,
        "owner can release a retained held task without replay"
    );
    assert_eq!(
        model.requests().len(),
        2,
        "owner inspection must not resume the producer"
    );
    let after = state.read().await.runs.get(&record.id).unwrap().clone();
    assert_eq!(after.reply_message_id, record.reply_message_id);
    assert_eq!(after.reply_sha256, record.reply_sha256);
    assert_eq!(
        after.outcome.as_ref().unwrap().iterations,
        record.outcome.as_ref().unwrap().iterations
    );
    assert!(!after.allows_outcome_review_delivery());
    fixture.assert_quiet_restore(&after).await;
}

#[tokio::test]
async fn outcome_restart_retains_unknown_action_evidence_and_never_replays_it() {
    let fixture = Fixture::new(ScriptedModel::new(vec![]));
    add_chat(&fixture.coordinator, &fixture.agent, SESSION).await;
    let state = coordinator_state(&fixture.coordinator);
    let mut source = state.write().await;
    let mut record = RunRecord::running(
        RunStart {
            agent_id: fixture.agent.clone(),
            session_id: SESSION.into(),
            source: RunSource::Web,
            source_ref: None,
            idempotency_key: Some("interrupted-effect".into()),
            text: "Create the synthetic report".into(),
            model: "gpt-5.4".into(),
            provider: Some("openai".into()),
            parent_run_id: None,
        },
        anima_core::primitives::now_millis(),
    );
    super::attach(
        &mut record,
        file_definition(OutcomeAcceptance::ReportOnly),
        Some(&fixture.root),
    )
    .unwrap();
    record.tools_started.push("send_email".into());
    record.action_receipts.push(ActionReceipt {
        tool_call_id: "unknown-send".into(),
        name: "send_email".into(),
        status: ActionStatus::Started,
        started_at_ms: record.started_at_ms.unwrap(),
        finished_at_ms: None,
        duration_ms: None,
        arguments_preview: "synthetic unavailable effect".into(),
        arguments_truncated: false,
        result_preview: None,
        result_truncated: false,
        durability: ActionDurability::Persistent,
    });
    let receipts = record.action_receipts.clone();
    source.runs.insert(record.clone());
    let snapshot = source.control_plane_snapshot();
    drop(source);
    // Round-trip the crash image, including a start receipt with no success acknowledgement.
    let snapshot: ControlPlaneSnapshot =
        serde_json::from_slice(&serde_json::to_vec(&snapshot).unwrap()).unwrap();
    let quiet = ScriptedModel::new(vec![]);
    let mut restored = DaemonState::with_model_adapter(quiet.clone());
    restored
        .replace_memory_embeddings(crate::memory_embeddings::MemoryEmbeddingRuntime::disabled());
    restored.restore_control_plane_snapshot(snapshot).unwrap();
    let restored =
        AgentRunCoordinator::new(Arc::new(RwLock::new(restored)), Arc::new(Semaphore::new(1)));
    tokio::time::sleep(Duration::from_millis(40)).await;
    let state = coordinator_state(&restored);
    let guard = state.read().await;
    let interrupted = guard.runs.get(&record.id).unwrap();
    assert_eq!(interrupted.status, RunStatus::Interrupted);
    assert_eq!(
        interrupted.outcome.as_ref().unwrap().status,
        OutcomeStatus::NeedsReview
    );
    assert_eq!(
        interrupted.action_receipts, receipts,
        "restart cannot invent an effect acknowledgement"
    );
    let recovery = crate::runs::recovery::summary(interrupted);
    assert!(recovery.review_required);
    assert_eq!(recovery.unknown_action_count, 1);
    let agent = guard.agents[&fixture.agent].state();
    let context = guard
        .tool_execution_context()
        .with_team(restored.clone(), false)
        .with_run_link(Some(crate::runs::RunLink {
            run_id: record.id.clone(),
            agent_id: fixture.agent.clone(),
            session_id: SESSION.into(),
        }));
    drop(guard);
    let denied = context
        .execute_tool(
            agent,
            Message {
                id: "synthetic-restart-message".into(),
                agent_id: fixture.agent.clone(),
                room_id: SESSION.into(),
                role: MessageRole::User,
                content: Content {
                    text: "Inspect the interrupted report".into(),
                    ..Content::default()
                },
                created_at_ms: anima_core::primitives::now_millis(),
            },
            call("different-key-for-unknown-send", "send_email", []),
        )
        .await;
    assert_eq!(denied.status, TaskStatus::Error);
    assert!(
        denied
            .error
            .as_deref()
            .unwrap()
            .contains("Outcome verification has stopped"),
        "the restored outcome fence must deny native dispatch before any connector handler"
    );
    assert_eq!(
        state
            .read()
            .await
            .runs
            .get(&record.id)
            .unwrap()
            .action_receipts,
        receipts
    );
    assert!(quiet.requests().is_empty());
    assert!(quiet.secondary_requests().is_empty());
    assert!(!fixture.root.join("report.txt").exists());
}
