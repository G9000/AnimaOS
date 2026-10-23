//! M21 regression: a real native file effect must have an owner-visible
//! durable witness before its final model reply commits the transcript.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use anima_core::{DataValue, MessageRole, ToolCall};
use serde_json::Value;
use tokio::sync::{RwLock, Semaphore};

use super::test_support::{chat_request, companion_config, Gate, ScriptedModel, Step};
use super::AgentRunCoordinator;
use crate::approvals::{ApprovalPolicy, PolicyAction, RiskClass};
use crate::control_plane_store::{load_control_plane_snapshot, ControlPlaneStoreConfig};
use crate::routes::RunResponse;
use crate::runs::{RunSource, RunStatus};
use crate::skills::test_support::{temp_workspace, with_workspace};
use crate::state::DaemonState;

use super::action_receipts::{RunActionJournal, RECEIPT_NOT_SAVED};
use super::test_support::{gated_context, ledger_run, tool_input};
use crate::runs::{ActionStatus, RunLink};

struct ActionFixture {
    root: std::path::PathBuf,
    store: ControlPlaneStoreConfig,
    coordinator: AgentRunCoordinator,
    run: RunLink,
    journal: RunActionJournal,
    cancel: anima_core::CancelSignal,
    agent: anima_core::AgentState,
    context: crate::tools::ToolExecutionContext,
}

impl ActionFixture {
    async fn new(label: &str, persistent: bool) -> Self {
        let root = temp_workspace(label);
        let store = ControlPlaneStoreConfig::Json(root.join("control-plane.json"));
        let mut state = with_workspace(DaemonState::new(), &root);
        let mut config = companion_config("receipt-companion");
        config.tools = Some(
            crate::tools::ToolRegistry::new()
                .resolve_descriptors(["write_file"])
                .unwrap(),
        );
        let agent_id = state.create_agent(config).unwrap().state.id;
        state.approvals.set_policy(
            &agent_id,
            ApprovalPolicy::default().with(RiskClass::Write, PolicyAction::Allow),
        );
        if persistent {
            state.set_control_plane_store(Some(store.clone()));
        }
        let coordinator =
            AgentRunCoordinator::new(Arc::new(RwLock::new(state)), Arc::new(Semaphore::new(1)));
        let run = ledger_run(&coordinator, &agent_id, SESSION_ID).await;
        let cancel = anima_core::CancelSignal::new();
        let journal = RunActionJournal::new(
            &coordinator.state,
            coordinator.control_plane_transactions(),
            run.clone(),
            cancel.clone(),
            None,
        );
        let context = gated_context(&coordinator, &run, cancel.clone())
            .await
            .with_action_journal(journal.clone());
        let agent = coordinator
            .state
            .read()
            .await
            .get_agent(&agent_id)
            .unwrap()
            .state;
        coordinator
            .state
            .write()
            .await
            .control_plane_persist_request()
            .save()
            .await
            .unwrap();
        Self {
            root,
            store,
            coordinator,
            run,
            journal,
            cancel,
            agent,
            context,
        }
    }

    fn call(
        &self,
        call: ToolCall,
    ) -> tokio::task::JoinHandle<anima_core::TaskResult<anima_core::Content>> {
        let context = self.context.clone();
        let agent = self.agent.clone();
        let input = tool_input(&self.run.agent_id, SESSION_ID);
        tokio::spawn(async move { context.execute_tool(agent, input, call).await })
    }

    async fn public_run(&self) -> Value {
        let state = self.coordinator.state.read().await;
        serde_json::to_value(RunResponse::from(state.runs.get(&self.run.run_id).unwrap())).unwrap()
    }

    async fn saved_run(&self) -> Value {
        let saved = load_control_plane_snapshot(&self.store)
            .await
            .unwrap()
            .unwrap();
        serde_json::to_value(
            saved
                .runs
                .iter()
                .find(|run| run.id == self.run.run_id)
                .unwrap(),
        )
        .unwrap()
    }
}

impl Drop for ActionFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

async fn entered_save(gate: &crate::state::TestControlPlaneSaveGate) {
    tokio::time::timeout(Duration::from_secs(5), gate.entered.acquire())
        .await
        .expect("the action reaches its save")
        .unwrap()
        .forget();
}

const CALL_ID: &str = "m21-write-call";
const FILE_PATH: &str = "m21/checkpoint.txt";
const CONTENT: &str = "m21 recovery checkpoint amber-742";
const SESSION_ID: &str = "chat:m21-receipt";
const SOURCE_REF: &str = "m21-receipt-regression";

fn write_call() -> ToolCall {
    ToolCall {
        id: CALL_ID.into(),
        name: "write_file".into(),
        args: BTreeMap::from([
            ("file_path".into(), DataValue::String(FILE_PATH.into())),
            ("content".into(), DataValue::String(CONTENT.into())),
        ]),
    }
}

struct HeldWriteEvidence {
    saved_run: Value,
    public_run: Value,
    restored_owner_run: Value,
    second_restored_owner_run: Value,
}

/// Capture the Running checkpoint while the actual next model reply is
/// held, then release and join the worker before receipt assertions fail.
/// No test inserts a receipt or forces an unrelated save after the effect.
async fn write_before_final_reply(label: &str) -> HeldWriteEvidence {
    let gate = Gate::new();
    let model = ScriptedModel::gated(
        vec![
            Step::Tools(vec![write_call()]),
            Step::Text(vec!["The checkpoint was written."]),
        ],
        gate.clone(),
    );
    let root = temp_workspace(label);
    let store_path = root.join("control-plane.json");
    let store = ControlPlaneStoreConfig::Json(store_path.clone());
    let mut state = with_workspace(DaemonState::with_model_adapter(model.clone()), &root);
    let mut config = companion_config("receipt-companion");
    config.tools = Some(
        crate::tools::ToolRegistry::new()
            .resolve_descriptors(["write_file"])
            .unwrap(),
    );
    let agent_id = state.create_agent(config).unwrap().state.id;
    state.approvals.set_policy(
        &agent_id,
        ApprovalPolicy::default().with(RiskClass::Write, PolicyAction::Allow),
    );
    state.set_control_plane_store(Some(store.clone()));
    let coordinator =
        AgentRunCoordinator::new(Arc::new(RwLock::new(state)), Arc::new(Semaphore::new(1)));
    let worker = {
        let coordinator = coordinator.clone();
        let agent_id = agent_id.clone();
        tokio::spawn(async move {
            let mut request = chat_request(&agent_id, SESSION_ID, "Write the checkpoint once");
            request.source = RunSource::Schedule;
            request.source_ref = Some(SOURCE_REF.into());
            coordinator.run(request).await
        })
    };

    gate.entered().await;
    gate.release();
    gate.entered().await;
    // Reaching the second request proves the real handler returned before
    // the final model answer; the artifact proves its actual file effect.
    assert_eq!(
        std::fs::read(root.join(FILE_PATH)).unwrap(),
        CONTENT.as_bytes()
    );
    let requests = model.requests();
    assert_eq!(requests.len(), 2);
    assert!(requests[1].messages.iter().any(|message| {
        message.role == MessageRole::Tool
            && message
                .content
                .text
                .contains("Wrote 33 chars to m21/checkpoint.txt")
    }));

    let public_run = {
        let guard = coordinator.state.read().await;
        let record = guard.runs.for_session(&agent_id, SESSION_ID)[0];
        assert_eq!(record.status, RunStatus::Running);
        assert!(record.reply_message_id.is_none());
        assert!(guard.agents.get(&agent_id).unwrap().messages().is_empty());
        serde_json::to_value(RunResponse::from(record)).unwrap()
    };
    let run_id = public_run["id"].as_str().unwrap();
    let saved = load_control_plane_snapshot(&store).await.unwrap().unwrap();
    let record = saved.runs.iter().find(|run| run.id == run_id).unwrap();
    assert_eq!(record.status, RunStatus::Running);
    assert!(record.reply_message_id.is_none());
    assert!(saved
        .agents
        .iter()
        .find(|agent| agent.state.id == agent_id)
        .unwrap()
        .messages
        .is_empty());
    // Parse the actual file rather than relying on in-memory serde alone.
    let disk: Value = serde_json::from_slice(&std::fs::read(&store_path).unwrap()).unwrap();
    let saved_run = disk["runs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|run| run["id"] == public_run["id"])
        .unwrap()
        .clone();

    let mut restored = DaemonState::new();
    restored.restore_control_plane_snapshot(saved).unwrap();
    let restored_record = restored.runs.get(run_id).unwrap();
    assert_eq!(restored_record.status, RunStatus::Interrupted);
    assert_eq!(
        restored_record.error.as_ref().unwrap().code,
        "restart_during_run"
    );
    assert!(restored_record.reply_message_id.is_none());
    assert!(restored
        .agents
        .get(&agent_id)
        .unwrap()
        .messages()
        .is_empty());
    let restored_owner_run = serde_json::to_value(RunResponse::from(restored_record)).unwrap();
    let mut restored_again = DaemonState::new();
    restored_again
        .restore_control_plane_snapshot(restored.control_plane_snapshot())
        .unwrap();
    let second_restored_owner_run =
        serde_json::to_value(RunResponse::from(restored_again.runs.get(run_id).unwrap())).unwrap();

    gate.release();
    tokio::time::timeout(Duration::from_secs(5), worker)
        .await
        .expect("released final reply completes")
        .unwrap()
        .unwrap();
    assert_eq!(
        model.requests().len(),
        2,
        "restoring witnesses never reexecutes the model"
    );
    assert_eq!(
        std::fs::read(root.join(FILE_PATH)).unwrap(),
        CONTENT.as_bytes()
    );
    std::fs::remove_dir_all(&root).unwrap();
    HeldWriteEvidence {
        saved_run,
        public_run,
        restored_owner_run,
        second_restored_owner_run,
    }
}

fn assert_persistent_success(run: &Value) {
    let receipts = run.get("actionReceipts").and_then(Value::as_array)
        .expect("M21: the successful native effect needs a saved actionReceipts witness before the final model reply");
    assert_eq!(receipts.len(), 1, "one actual native call has one witness");
    let receipt = &receipts[0];
    assert_eq!(receipt["toolCallId"], CALL_ID);
    assert_eq!(receipt["name"], "write_file");
    assert_eq!(receipt["status"], "succeeded");
    assert_eq!(receipt["durability"], "persistent");
    assert!(receipt["argumentsPreview"]
        .as_str()
        .unwrap()
        .contains(FILE_PATH));
    assert_eq!(receipt["argumentsTruncated"], false);
    assert!(receipt["resultPreview"]
        .as_str()
        .unwrap()
        .contains("Wrote 33 chars"));
    assert_eq!(receipt["resultTruncated"], false);
    let started = receipt["startedAtMs"].as_u64().unwrap();
    assert!(receipt["finishedAtMs"].as_u64().unwrap() >= started);
    assert!(receipt["durationMs"].as_u64().is_some());
    assert_eq!(run["toolsStarted"], serde_json::json!(["write_file"]));
    assert!(run["replyMessageId"].is_null());
}

#[tokio::test]
async fn native_write_success_is_saved_before_the_final_reply_commits() {
    let evidence = write_before_final_reply("m21-action-before-reply").await;
    assert_eq!(evidence.saved_run["status"], "running");
    assert_persistent_success(&evidence.saved_run);
    assert_persistent_success(&evidence.public_run);
}

#[tokio::test]
async fn interrupted_owner_run_keeps_native_write_witness_without_chat_commit() {
    let evidence = write_before_final_reply("m21-action-restore").await;
    assert_eq!(evidence.restored_owner_run["status"], "interrupted");
    assert_eq!(evidence.restored_owner_run["source"], "schedule");
    assert_eq!(evidence.restored_owner_run["sourceRef"], SOURCE_REF);
    assert_eq!(
        evidence.restored_owner_run["error"]["code"],
        "restart_during_run"
    );
    assert_persistent_success(&evidence.restored_owner_run);
    assert_eq!(
        evidence.second_restored_owner_run,
        evidence.restored_owner_run
    );
}

#[tokio::test]
async fn superseded_native_intent_rebases_before_dispatch() {
    let fixture = ActionFixture::new("superseded-native-intent", true).await;
    let gate = fixture.journal.install_test_before_save_gate();
    let calling = fixture.call(write_call());
    tokio::time::timeout(Duration::from_secs(5), gate.entered.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    assert!(!fixture.root.join(FILE_PATH).exists());
    let newer = {
        let mut state = fixture.coordinator.state.write().await;
        state.runs.get_mut(&fixture.run.run_id).unwrap().model = "newer-owner-snapshot".into();
        state.control_plane_persist_request()
    };
    newer.save().await.unwrap();
    gate.release.add_permits(1);
    let result = calling.await.unwrap();
    assert_eq!(result.status, anima_core::TaskStatus::Success, "{result:?}");
    assert_persistent_success(&fixture.public_run().await);
    let saved = fixture.saved_run().await;
    assert_persistent_success(&saved);
    assert_eq!(saved["model"], "newer-owner-snapshot");
}

#[tokio::test]
async fn superseded_native_outcome_rebases_without_repeating_effect() {
    let fixture = ActionFixture::new("superseded-native-outcome", true).await;
    let intent = fixture.journal.install_test_before_save_gate();
    let calling = fixture.call(write_call());
    tokio::time::timeout(Duration::from_secs(5), intent.entered.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    let outcome = fixture.journal.install_test_before_save_gate();
    intent.release.add_permits(1);
    tokio::time::timeout(Duration::from_secs(5), outcome.entered.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    assert_eq!(
        std::fs::read_to_string(fixture.root.join(FILE_PATH)).unwrap(),
        CONTENT
    );
    assert_eq!(
        fixture.public_run().await["actionReceipts"][0]["status"],
        "started"
    );
    // A later owner edit would be overwritten if the handler ran again.
    std::fs::write(
        fixture.root.join(FILE_PATH),
        "owner edit after native effect",
    )
    .unwrap();
    let newer = {
        let mut state = fixture.coordinator.state.write().await;
        state.runs.get_mut(&fixture.run.run_id).unwrap().model = "newer-owner-snapshot".into();
        state.control_plane_persist_request()
    };
    newer.save().await.unwrap();
    outcome.release.add_permits(1);
    let result = calling.await.unwrap();
    assert_eq!(result.status, anima_core::TaskStatus::Success, "{result:?}");
    assert_eq!(
        std::fs::read_to_string(fixture.root.join(FILE_PATH)).unwrap(),
        "owner edit after native effect"
    );
    assert_persistent_success(&fixture.public_run().await);
    let saved = fixture.saved_run().await;
    assert_persistent_success(&saved);
    assert_eq!(saved["model"], "newer-owner-snapshot");
    assert_eq!(saved["actionReceipts"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn superseded_native_intent_honors_cancellation() {
    let fixture = ActionFixture::new("superseded-native-cancel-intent", true).await;
    let gate = fixture.journal.install_test_before_save_gate();
    let calling = fixture.call(write_call());
    tokio::time::timeout(Duration::from_secs(5), gate.entered.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    fixture.cancel.cancel();
    let newer = fixture
        .coordinator
        .state
        .write()
        .await
        .control_plane_persist_request();
    newer.save().await.unwrap();
    gate.release.add_permits(1);
    assert!(calling.await.unwrap().error.is_some());
    assert!(!fixture.root.join(FILE_PATH).exists());
    assert!(fixture.public_run().await["actionReceipts"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(fixture.saved_run().await.get("actionReceipts").is_none());
    assert!(fixture.journal.failure().is_none());
}

#[tokio::test]
async fn superseded_native_outcome_settles_after_stop_without_clearing_stop() {
    let fixture = ActionFixture::new("superseded-native-stop-outcome", true).await;
    let intent = fixture.journal.install_test_before_save_gate();
    let calling = fixture.call(write_call());
    tokio::time::timeout(Duration::from_secs(5), intent.entered.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    let outcome = fixture.journal.install_test_before_save_gate();
    intent.release.add_permits(1);
    tokio::time::timeout(Duration::from_secs(5), outcome.entered.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    fixture.cancel.cancel();
    // Simulate a newer authoritative Stop while the old snapshot waits.
    let stop_at = anima_core::primitives::now_millis();
    let newer = {
        let mut state = fixture.coordinator.state.write().await;
        state.runs.get_mut(&fixture.run.run_id).unwrap().stop = Some(crate::runs::RunStopRequest {
            requested_at_ms: stop_at,
        });
        state.control_plane_persist_request()
    };
    newer.save().await.unwrap();
    outcome.release.add_permits(1);
    let result = calling.await.unwrap();
    assert_eq!(result.status, anima_core::TaskStatus::Success, "{result:?}");
    let saved = fixture.saved_run().await;
    assert_persistent_success(&saved);
    assert_eq!(saved["stop"]["requestedAtMs"], stop_at);
    assert_eq!(fixture.public_run().await["stop"]["requestedAtMs"], stop_at);
    assert!(fixture.journal.failure().is_none());
}

#[tokio::test]
async fn superseded_native_intent_never_resurrects_a_removed_run() {
    let fixture = ActionFixture::new("superseded-native-deleted-run", true).await;
    let gate = fixture.journal.install_test_before_save_gate();
    let calling = fixture.call(write_call());
    tokio::time::timeout(Duration::from_secs(5), gate.entered.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    let newer = {
        let mut state = fixture.coordinator.state.write().await;
        state.runs.remove(&fixture.run.run_id).unwrap();
        state.control_plane_persist_request()
    };
    newer.save().await.unwrap();
    gate.release.add_permits(1);
    assert!(calling.await.unwrap().error.is_some());
    assert!(!fixture.root.join(FILE_PATH).exists());
    assert!(fixture
        .coordinator
        .state
        .read()
        .await
        .runs
        .get(&fixture.run.run_id)
        .is_none());
    let saved = load_control_plane_snapshot(&fixture.store)
        .await
        .unwrap()
        .unwrap();
    assert!(saved.runs.iter().all(|run| run.id != fixture.run.run_id));
}

#[tokio::test]
async fn superseded_native_outcome_never_resurrects_a_removed_run() {
    let fixture = ActionFixture::new("superseded-native-deleted-outcome", true).await;
    let intent = fixture.journal.install_test_before_save_gate();
    let calling = fixture.call(write_call());
    tokio::time::timeout(Duration::from_secs(5), intent.entered.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    let outcome = fixture.journal.install_test_before_save_gate();
    intent.release.add_permits(1);
    tokio::time::timeout(Duration::from_secs(5), outcome.entered.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    assert_eq!(
        std::fs::read_to_string(fixture.root.join(FILE_PATH)).unwrap(),
        CONTENT
    );
    std::fs::write(
        fixture.root.join(FILE_PATH),
        "owner edit after native effect",
    )
    .unwrap();
    let newer = {
        let mut state = fixture.coordinator.state.write().await;
        state.runs.remove(&fixture.run.run_id).unwrap();
        state.control_plane_persist_request()
    };
    newer.save().await.unwrap();
    outcome.release.add_permits(1);
    assert!(calling.await.unwrap().error.is_some());
    assert_eq!(
        std::fs::read_to_string(fixture.root.join(FILE_PATH)).unwrap(),
        "owner edit after native effect"
    );
    assert!(fixture
        .coordinator
        .state
        .read()
        .await
        .runs
        .get(&fixture.run.run_id)
        .is_none());
    let saved = load_control_plane_snapshot(&fixture.store)
        .await
        .unwrap()
        .unwrap();
    assert!(saved.runs.iter().all(|run| run.id != fixture.run.run_id));
}

#[tokio::test]
async fn superseded_native_intent_has_bounded_snapshot_attempts() {
    let fixture = ActionFixture::new("superseded-native-bound", true).await;
    let mut gate = fixture.journal.install_test_before_save_gate();
    let calling = fixture.call(write_call());
    for attempt in 0..8 {
        tokio::time::timeout(Duration::from_secs(5), gate.entered.acquire())
            .await
            .unwrap()
            .unwrap()
            .forget();
        let next = (attempt < 7).then(|| fixture.journal.install_test_before_save_gate());
        let newer = fixture
            .coordinator
            .state
            .write()
            .await
            .control_plane_persist_request();
        newer.save().await.unwrap();
        gate.release.add_permits(1);
        if let Some(next) = next {
            gate = next;
        }
    }
    let result = tokio::time::timeout(Duration::from_secs(5), calling)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.error.as_deref(), Some(RECEIPT_NOT_SAVED));
    assert!(fixture.journal.failure().is_some());
    assert!(!fixture.root.join(FILE_PATH).exists());
    assert!(fixture.saved_run().await.get("actionReceipts").is_none());
}

#[tokio::test]
async fn parallel_native_actions_keep_every_durable_outcome() {
    crate::init_tracing();
    let fixture = ActionFixture::new("parallel-receipt-outcomes", true).await;
    let calls: Vec<_> = (0..24)
        .map(|index| {
            let mut call = write_call();
            call.id = format!("parallel-{index}");
            call.args.insert(
                "file_path".into(),
                DataValue::String(format!("parallel/{index}.txt")),
            );
            fixture.call(call)
        })
        .collect();
    let mut errors = Vec::new();
    for call in calls {
        let result = call.await.unwrap();
        if result.status != anima_core::TaskStatus::Success {
            errors.push(result.error);
        }
    }
    fixture.journal.settle().await;
    assert!(errors.is_empty(), "parallel actions failed: {errors:?}");
    let public = fixture.public_run().await;
    let saved = fixture.saved_run().await;
    assert_eq!(public["actionReceipts"], saved["actionReceipts"]);
    let receipts = saved["actionReceipts"].as_array().unwrap();
    assert_eq!(receipts.len(), 24);
    assert!(receipts
        .iter()
        .all(|receipt| receipt["status"] == "succeeded"));
    for index in 0..24 {
        assert_eq!(
            std::fs::read_to_string(fixture.root.join(format!("parallel/{index}.txt"))).unwrap(),
            CONTENT
        );
    }
}

#[tokio::test]
async fn a_failed_intent_save_denies_parallel_effects_and_future_calls() {
    let fixture = ActionFixture::new("m21-intent-failure", true).await;
    let save = fixture
        .coordinator
        .state
        .write()
        .await
        .install_test_control_plane_save_gate(true);
    let first = fixture.call(write_call());
    entered_save(&save).await;
    let mut second_call = write_call();
    second_call.id = "second-call".into();
    second_call.args.insert(
        "file_path".into(),
        DataValue::String("m21/second.txt".into()),
    );
    let second = fixture.call(second_call);
    save.release.add_permits(1);
    assert_eq!(
        first.await.unwrap().error.as_deref(),
        Some(RECEIPT_NOT_SAVED)
    );
    assert!(second.await.unwrap().error.is_some());
    assert!(!fixture.root.join(FILE_PATH).exists());
    assert!(!fixture.root.join("m21/second.txt").exists());
    assert!(fixture.public_run().await["actionReceipts"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(fixture.saved_run().await.get("actionReceipts").is_none());
    assert!(fixture.journal.failure().is_some());
}

#[tokio::test]
async fn a_failed_outcome_save_never_exposes_or_later_saves_dirty_success() {
    let mut fixture = ActionFixture::new("m21-outcome-failure", true).await;
    let review = anima_core::CompletionReview::default();
    fixture.context = fixture
        .context
        .clone()
        .with_completion_review(Some(review.clone()));
    let intent = fixture
        .coordinator
        .state
        .write()
        .await
        .install_test_control_plane_save_gate(false);
    let calling = fixture.call(write_call());
    entered_save(&intent).await;
    let outcome = fixture
        .coordinator
        .state
        .write()
        .await
        .install_test_control_plane_save_gate(true);
    intent.release.add_permits(1);
    entered_save(&outcome).await;
    assert_eq!(
        std::fs::read(fixture.root.join(FILE_PATH)).unwrap(),
        CONTENT.as_bytes()
    );
    assert_eq!(
        fixture.public_run().await["actionReceipts"][0]["status"],
        "started"
    );
    assert_eq!(
        fixture.saved_run().await["actionReceipts"][0]["status"],
        "started"
    );
    outcome.release.add_permits(1);
    assert_eq!(
        calling.await.unwrap().error.as_deref(),
        Some(RECEIPT_NOT_SAVED)
    );
    let state = fixture.coordinator.state.read().await;
    let decision = anima_core::Evaluator::evaluate(
        &review,
        &state.agents[&fixture.run.agent_id],
        &tool_input(&fixture.run.agent_id, SESSION_ID),
        &anima_core::Content::default(),
    )
    .await
    .unwrap()
    .decision;
    assert_eq!(
        decision,
        anima_core::EvaluatorDecision::Accept,
        "an actual file effect with failed outcome persistence is never successful review evidence"
    );
    drop(state);
    let public = fixture.public_run().await;
    assert_eq!(public["actionReceipts"][0]["status"], "started");
    assert!(public["actionReceipts"][0]["resultPreview"].is_null());
    fixture
        .coordinator
        .state
        .write()
        .await
        .control_plane_persist_request()
        .save()
        .await
        .unwrap();
    assert_eq!(
        fixture.saved_run().await["actionReceipts"][0]["status"],
        "started"
    );
    assert!(fixture.call(write_call()).await.unwrap().error.is_some());
}

#[tokio::test]
async fn dropping_an_executor_during_outcome_save_still_settles_only_after_ack() {
    let fixture = ActionFixture::new("m21-dropped-outcome", true).await;
    let intent = fixture
        .coordinator
        .state
        .write()
        .await
        .install_test_control_plane_save_gate(false);
    let calling = fixture.call(write_call());
    entered_save(&intent).await;
    let outcome = fixture
        .coordinator
        .state
        .write()
        .await
        .install_test_control_plane_save_gate(false);
    intent.release.add_permits(1);
    entered_save(&outcome).await;
    assert_eq!(
        fixture.public_run().await["actionReceipts"][0]["status"],
        "started"
    );
    calling.abort();
    let _ = calling.await;
    outcome.release.add_permits(1);
    tokio::time::timeout(Duration::from_secs(5), fixture.journal.settle())
        .await
        .unwrap();
    assert_persistent_success(&fixture.public_run().await);
    assert_persistent_success(&fixture.saved_run().await);
    assert!(fixture.journal.failure().is_none());
    let _transaction = fixture.coordinator.control_plane_transaction().await;
}

#[tokio::test]
async fn configured_permission_revoked_during_intent_save_prevents_dispatch() {
    let fixture = ActionFixture::new("m21-revocation", true).await;
    let save = fixture
        .coordinator
        .state
        .write()
        .await
        .install_test_control_plane_save_gate(false);
    let calling = fixture.call(write_call());
    entered_save(&save).await;
    fixture
        .coordinator
        .state
        .write()
        .await
        .agents
        .get_mut(&fixture.run.agent_id)
        .unwrap()
        .update_config(anima_core::AgentConfigUpdate {
            tools: Some(Vec::new()),
            ..Default::default()
        });
    save.release.add_permits(1);
    assert!(calling.await.unwrap().error.is_some());
    assert!(!fixture.root.join(FILE_PATH).exists());
    assert_eq!(
        fixture.public_run().await["actionReceipts"][0]["status"],
        "failed"
    );
    assert!(fixture.journal.failure().is_none());
}

#[tokio::test]
async fn duplicate_call_identity_never_repeats_the_native_effect() {
    let fixture = ActionFixture::new("m21-duplicate", true).await;
    assert_eq!(
        fixture.call(write_call()).await.unwrap().status,
        anima_core::TaskStatus::Success
    );
    let mut duplicate = write_call();
    duplicate.args.insert(
        "content".into(),
        DataValue::String("wrong second write".into()),
    );
    assert!(fixture.call(duplicate).await.unwrap().error.is_some());
    assert_eq!(
        std::fs::read(fixture.root.join(FILE_PATH)).unwrap(),
        CONTENT.as_bytes()
    );
    assert_persistent_success(&fixture.saved_run().await);
}

#[tokio::test]
async fn memory_only_receipts_are_explicit_and_malformed_restore_is_atomic() {
    let fixture = ActionFixture::new("m21-memory-receipt", false).await;
    assert_eq!(
        fixture.call(write_call()).await.unwrap().status,
        anima_core::TaskStatus::Success
    );
    assert_eq!(
        fixture.public_run().await["actionReceipts"][0]["durability"],
        "memory"
    );
    let mut snapshot = fixture
        .coordinator
        .state
        .read()
        .await
        .control_plane_snapshot();
    let receipt = &mut snapshot.runs[0].action_receipts[0];
    receipt.status = ActionStatus::Started;
    let mut target = DaemonState::new();
    assert!(target.restore_control_plane_snapshot(snapshot).is_err());
    assert!(target.agents.is_empty());
    assert!(target.runs.active_records().is_empty());
}

#[tokio::test]
async fn a_newer_ordinary_snapshot_cannot_erase_an_acknowledged_action() {
    let fixture = ActionFixture::new("m21-ordinary-save-race", true).await;
    let intent = fixture
        .coordinator
        .state
        .write()
        .await
        .install_test_control_plane_save_gate(false);
    let calling = fixture.call(write_call());
    entered_save(&intent).await;
    let outcome = fixture
        .coordinator
        .state
        .write()
        .await
        .install_test_control_plane_save_gate(false);
    intent.release.add_permits(1);
    entered_save(&outcome).await;
    assert_eq!(
        fixture.public_run().await["actionReceipts"][0]["status"],
        "started"
    );

    // Reproduce ordinary writers such as handle_update_agent: hold the
    // daemon write lock while preparing/awaiting a newer snapshot, without
    // the run transaction. Its snapshot necessarily captures started.
    let (captured, ready) = tokio::sync::oneshot::channel();
    let ordinary = {
        let state = fixture.coordinator.state.clone();
        let run_id = fixture.run.run_id.clone();
        tokio::spawn(async move {
            let mut guard = state.write().await;
            guard.runs.get_mut(&run_id).unwrap().model = "updated-during-receipt-save".into();
            let persist = guard.control_plane_persist_request();
            captured.send(()).unwrap();
            let result = persist.save().await;
            drop(guard);
            result
        })
    };
    tokio::time::timeout(Duration::from_secs(5), ready)
        .await
        .unwrap()
        .unwrap();
    outcome.release.add_permits(1);
    ordinary.await.unwrap().unwrap();
    assert_eq!(
        calling.await.unwrap().status,
        anima_core::TaskStatus::Success
    );
    assert_persistent_success(&fixture.public_run().await);
    let saved = fixture.saved_run().await;
    assert_eq!(saved["model"], "updated-during-receipt-save");
    assert_eq!(saved["status"], "running");
    assert_persistent_success(&saved);
}

#[tokio::test]
async fn a_failed_absent_row_snapshot_keeps_the_acknowledged_patch() {
    let fixture = ActionFixture::new("m21-failed-row-removal", true).await;
    let intent = fixture
        .coordinator
        .state
        .write()
        .await
        .install_test_control_plane_save_gate(false);
    let calling = fixture.call(write_call());
    entered_save(&intent).await;
    let outcome = fixture
        .coordinator
        .state
        .write()
        .await
        .install_test_control_plane_save_gate(false);
    intent.release.add_permits(1);
    entered_save(&outcome).await;
    // A newer ordinary snapshot captures the actual public started row.
    let stale = fixture
        .coordinator
        .state
        .write()
        .await
        .control_plane_persist_request();
    outcome.release.add_permits(1);
    assert_eq!(
        calling.await.unwrap().status,
        anima_core::TaskStatus::Success
    );
    let transaction = fixture.coordinator.control_plane_transaction().await;
    let (removed, deletion, failed) = {
        let mut guard = fixture.coordinator.state.write().await;
        let removed = guard.runs.remove(&fixture.run.run_id).unwrap();
        let failed = guard.install_test_control_plane_save_gate(true);
        failed.release.add_permits(1);
        (removed, guard.control_plane_persist_request(), failed)
    };
    assert!(deletion.save().await.is_err());
    fixture.coordinator.state.write().await.runs.insert(removed);
    drop(transaction);
    drop(failed);
    // Failed newer revisions cannot retire the acknowledged overlay. This
    // older prepared writer still must carry the acknowledged success.
    stale.save().await.unwrap();
    assert_persistent_success(&fixture.saved_run().await);
}

#[tokio::test]
async fn acknowledged_action_patches_never_resurrect_removed_run_rows() {
    let fixture = ActionFixture::new("m21-acknowledged-row-removal", true).await;
    assert_eq!(
        fixture.call(write_call()).await.unwrap().status,
        anima_core::TaskStatus::Success
    );
    let transaction = fixture.coordinator.control_plane_transaction().await;
    let deletion = {
        let mut guard = fixture.coordinator.state.write().await;
        guard.runs.remove(&fixture.run.run_id).unwrap();
        guard.control_plane_persist_request()
    };
    deletion.save().await.unwrap();
    drop(transaction);
    fixture
        .coordinator
        .state
        .write()
        .await
        .control_plane_persist_request()
        .save()
        .await
        .unwrap();
    let saved = load_control_plane_snapshot(&fixture.store)
        .await
        .unwrap()
        .unwrap();
    assert!(saved.runs.iter().all(|run| run.id != fixture.run.run_id));
    assert!(fixture.journal.begin(&write_call()).await.is_err());
}

#[tokio::test]
async fn coordinator_receipt_failure_stops_model_continuation_and_finishes_failed() {
    let fixture = ActionFixture::new("m21-coordinator-fail-closed", true).await;
    let mut config = fixture.agent.config.clone();
    config
        .settings
        .get_or_insert_with(Default::default)
        .max_tool_iterations = Some(1);
    fixture
        .coordinator
        .state
        .write()
        .await
        .agents
        .get_mut(&fixture.run.agent_id)
        .unwrap()
        .replace_config(config);
    let gate = Gate::new();
    let model = ScriptedModel::gated(
        vec![
            Step::Tools(vec![write_call()]),
            Step::Text(vec!["must not run"]),
        ],
        gate.clone(),
    );
    fixture.coordinator.state.write().await.model_adapter = model.clone();
    let session_id = "chat:m21-fail-closed";
    let running = {
        let coordinator = fixture.coordinator.clone();
        let agent_id = fixture.run.agent_id.clone();
        tokio::spawn(async move {
            coordinator
                .run(chat_request(&agent_id, session_id, "write once"))
                .await
        })
    };
    gate.entered().await;
    let run_id = fixture
        .coordinator
        .state
        .read()
        .await
        .runs
        .for_session(&fixture.run.agent_id, session_id)[0]
        .id
        .clone();
    let failed = fixture
        .coordinator
        .state
        .write()
        .await
        .install_test_control_plane_non_turn_save_gate(true);
    gate.release();
    entered_save(&failed).await;
    failed.release.add_permits(1);
    let result = tokio::time::timeout(Duration::from_secs(5), running)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(result.result.status, "error");
    assert_eq!(result.result.error.as_deref(), Some(RECEIPT_NOT_SAVED));
    assert_eq!(
        model.requests().len(),
        1,
        "failed native persistence must not call the model again"
    );
    assert!(!fixture.root.join(FILE_PATH).exists());
    let guard = fixture.coordinator.state.read().await;
    let record = guard.runs.get(&run_id).unwrap();
    assert_eq!(record.status, RunStatus::Failed);
    assert!(
        record.stop.is_none(),
        "infrastructure cancellation is not an owner stop"
    );
    assert!(record.reply_message_id.is_none());
}

#[tokio::test]
async fn m47_coding_unproven_effects_keep_started_and_block_later_dispatch() {
    // Inject only the physical-unknown journal signal. This is not an OS failure
    // simulation or evidence that any real child was cleaned up.
    const UNKNOWN: &str = "Command cleanup is uncertain; effects may have occurred. Shell execution is disabled until daemon restart";
    let fixture = ActionFixture::new("m47-unproven-effects", true).await;
    let first = write_call();
    fixture.journal.begin(&first).await.unwrap();
    fixture.journal.stop_for_unproven_effects(UNKNOWN);
    assert_eq!(fixture.journal.failure().as_deref(), Some(UNKNOWN));
    assert_eq!(fixture.journal.revalidate().await, Err(UNKNOWN.to_string()));
    let outcome = anima_core::TaskResult::error(UNKNOWN, 0);
    assert_eq!(
        fixture.journal.finish(&first, &outcome, 0).await,
        Err(UNKNOWN.to_string())
    );
    let mut later = write_call();
    later.id = "m47-after-unknown".into();
    let refused = fixture.call(later).await.unwrap();
    assert_eq!(refused.status, anima_core::TaskStatus::Error);
    assert_eq!(refused.error.as_deref(), Some(UNKNOWN));
    assert!(
        !fixture.root.join(FILE_PATH).exists(),
        "latched uncertainty must prevent the later real file handler"
    );
    for run in [fixture.public_run().await, fixture.saved_run().await] {
        let receipts = run["actionReceipts"].as_array().unwrap();
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0]["status"], "started");
        assert!(receipts[0].get("finishedAtMs").is_none_or(Value::is_null));
    }
    assert!(
        fixture
            .coordinator
            .state
            .read()
            .await
            .runs
            .get(&fixture.run.run_id)
            .unwrap()
            .stop
            .is_none(),
        "physical uncertainty must not forge a saved owner Stop"
    );
}

#[cfg(windows)]
#[tokio::test]
async fn execution_current_journal_refuses_revoke_and_stop_after_waiting_for_authority_lock() {
    use crate::execution_backends::{ExecutionRequest, ExecutionScope, ExecutionService};
    for stopped in [false, true] {
        let f = ActionFixture::new("execution-current-authority", true).await;
        {
            let mut g = f.coordinator.state.write().await;
            let runtime = g.agents.get_mut(&f.agent.id).unwrap();
            let mut config = runtime.config().clone();
            config.tools = Some(
                crate::tools::ToolRegistry::new()
                    .resolve_descriptors(["execution_run"])
                    .unwrap(),
            );
            runtime.replace_config(config);
            g.approvals.set_policy(
                &f.agent.id,
                ApprovalPolicy::default().with(RiskClass::Exec, PolicyAction::Allow),
            );
        }
        let call = ToolCall {
            id: "execution-delayed-authority".into(),
            name: "execution_run".into(),
            args: BTreeMap::from([
                (
                    "targetId".into(),
                    DataValue::String("selected-local".into()),
                ),
                (
                    "command".into(),
                    DataValue::String("echo should-never-run>execution-authority.txt".into()),
                ),
            ]),
        };
        f.journal.begin(&call).await.unwrap();
        let entered = Arc::new(Semaphore::new(0));
        let journal = f.journal.clone();
        let reached = entered.clone();
        let scope = ExecutionScope {
            agent_id: f.agent.id.clone(),
            authority: Arc::new(move || {
                let journal = journal.clone();
                let call = call.clone();
                let reached = reached.clone();
                Box::pin(async move {
                    reached.add_permits(1);
                    journal.execution_authority(&call, true).await
                })
            }),
        };
        let executable = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32")
            .join("cmd.exe");
        let service=ExecutionService::from_json(&serde_json::json!({"version":1,"targets":[{"id":"selected-local","label":"Controlled local authority witness","backend":{"kind":"local","executable":executable,"arguments":["/d","/s","/c"],"workspaceRoot":f.root}}]}).to_string()).unwrap();
        let mut guard = f.coordinator.state.write().await;
        let runner = service.clone();
        let pending = tokio::spawn(async move {
            runner
                .execute_scoped(
                    ExecutionRequest {
                        target_id: "selected-local".into(),
                        command: "echo should-never-run>execution-authority.txt".into(),
                        cwd: ".".into(),
                        timeout_ms: 1000,
                    },
                    None,
                    scope,
                )
                .await
        });
        tokio::time::timeout(Duration::from_secs(2), entered.acquire())
            .await
            .unwrap()
            .unwrap()
            .forget();
        if stopped {
            guard.runs.get_mut(&f.run.run_id).unwrap().stop =
                Some(crate::runs::RunStopRequest { requested_at_ms: 1 });
        } else {
            let runtime = guard.agents.get_mut(&f.agent.id).unwrap();
            let mut config = runtime.config().clone();
            config.tools = Some(vec![]);
            runtime.replace_config(config);
        }
        drop(guard);
        assert!(tokio::time::timeout(Duration::from_secs(5), pending)
            .await
            .unwrap()
            .unwrap()
            .is_err());
        assert!(!f.root.join("execution-authority.txt").exists());
        assert_eq!(service.in_flight_count(), 0);
    }
}
