//! Controlled A7 runtime acceptance: real native tools, private files and saved
//! snapshots. Fresh-runtime recovery is not an OS crash or live-model claim.
#![cfg(windows)]

use super::test_support::{chat_request, companion_config, Gate, ScriptedModel, Step};
use super::{AgentRunCoordinator, AgentRunEnvelope};
use crate::approvals::{ApprovalPolicy, PolicyAction, RiskClass};
use crate::control_plane_store::{
    load_control_plane_snapshot, ControlPlaneSnapshot, ControlPlaneStoreConfig, WorkspaceConfig,
};
use crate::filesystem_checkpoints::{NativeCheckpointService, NativeCheckpointSummary};
use crate::runs::{ActionDurability, ActionStatus, RunLink, RunRecord, RunStatus};
use crate::state::DaemonState;
use anima_core::{DataValue, MessageRole, ToolCall};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::{RwLock, Semaphore};
use tokio::task::JoinHandle;

const SESSION: &str = "chat:a7-private-invoices";
const INPUT: &str = "customer,status,cents\nCedar,open,1250\nCedar,paid,900\nCedar,open,750\nOrchid,open,30\nOrchid,open,0\n";
const DRAFT: &str = "unverified draft\r\n";
const OUTPUT: &str = "customer,open_count,total_cents\nCedar,2,2000\nOrchid,2,30\n";
const SENTINEL: &[u8] = b"unrelated private file\r\n";

fn call(id: &str, name: &str, args: serde_json::Value) -> ToolCall {
    fn value(json: serde_json::Value) -> DataValue {
        match json {
            serde_json::Value::Null => DataValue::Null,
            serde_json::Value::Bool(value) => DataValue::Bool(value),
            serde_json::Value::Number(value) => DataValue::Number(value.as_f64().unwrap()),
            serde_json::Value::String(value) => DataValue::String(value),
            serde_json::Value::Array(values) => {
                DataValue::Array(values.into_iter().map(value).collect())
            }
            serde_json::Value::Object(values) => DataValue::Object(
                values
                    .into_iter()
                    .map(|(key, item)| (key, value(item)))
                    .collect(),
            ),
        }
    }
    let DataValue::Object(args) = value(args) else {
        panic!("native tool arguments must be an object")
    };
    ToolCall {
        id: id.into(),
        name: name.into(),
        args,
    }
}

fn read(id: &str, path: &str) -> ToolCall {
    call(id, "read_file", serde_json::json!({"file_path":path}))
}

fn write(id: &str, content: &str) -> ToolCall {
    call(
        id,
        "write_file",
        serde_json::json!({"file_path":"summary.csv","content":content}),
    )
}

fn tasks(id: &str, done: bool) -> ToolCall {
    call(
        id,
        "todo_list",
        if done {
            serde_json::json!({"merge":true,"todos":[
                {"id":"reconcile","status":"completed"},
                {"id":"verify","status":"completed"}
            ]})
        } else {
            serde_json::json!({"todos":[
                {"id":"reconcile","content":"Reconcile open invoices","activeForm":"Reconciling open invoices","status":"in_progress"},
                {"id":"verify","content":"Verify saved summary against source and arithmetic","activeForm":"Verifying saved summary","status":"pending"}
            ]})
        },
    )
}

fn arithmetic(id: &str, expression: &str) -> ToolCall {
    call(
        id,
        "calculate",
        serde_json::json!({"expression":expression}),
    )
}

struct Fixture {
    _private: tempfile::TempDir,
    root: PathBuf,
    checkpoint_root: PathBuf,
    store: ControlPlaneStoreConfig,
    coordinator: AgentRunCoordinator,
    agent: String,
}

impl Fixture {
    fn new(model: Arc<ScriptedModel>) -> Self {
        use win_custody::{current_user_sid_string, SecurityDescriptor};
        let private = tempfile::tempdir().unwrap();
        let sid = current_user_sid_string().unwrap();
        SecurityDescriptor::from_sddl(&format!("D:P(A;OICI;FA;;;{sid})(A;OICI;FA;;;SY)"))
            .unwrap()
            .apply_to_tree(private.path(), true)
            .unwrap();
        let root = private.path().join("workspace");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("invoices.csv"), INPUT).unwrap();
        std::fs::write(root.join("summary.csv"), DRAFT).unwrap();
        std::fs::write(root.join("sentinel.txt"), SENTINEL).unwrap();
        let checkpoint_root = private.path().join("checkpoint-store");
        let store = ControlPlaneStoreConfig::Json(private.path().join("control-plane.json"));
        let mut state = DaemonState::with_model_adapter(model);
        state.replace_memory_embeddings(crate::memory_embeddings::MemoryEmbeddingRuntime::disabled());
        state.workspace = Some(WorkspaceConfig {
            root_path: root.clone(),
            ..Default::default()
        });
        state.checkpoint_root = Some(checkpoint_root.clone());
        state.set_control_plane_store(Some(store.clone()));
        let mut config = companion_config("A7 private task acceptance");
        config.tools = Some(
            state
                .tool_registry
                .resolve_descriptors(["read_file", "write_file", "todo_list", "calculate"])
                .unwrap(),
        );
        let settings = config.settings.as_mut().unwrap();
        settings.max_tool_iterations = Some(8);
        settings.max_retries = Some(0);
        settings.max_tokens = Some(2048);
        settings.timeout_ms = Some(20_000);
        for name in [
            "autoTitle",
            "autoCompact",
            "agentFilesProfileEnabled",
            "completionReview",
        ] {
            settings
                .additional
                .insert(name.into(), DataValue::Bool(false));
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
            checkpoint_root,
            store,
            coordinator,
            agent,
        }
    }

    async fn start(&self, gate: &Gate) -> (RunRecord, JoinHandle<AgentRunEnvelope>) {
        let coordinator = self.coordinator.clone();
        let agent = self.agent.clone();
        let worker = tokio::spawn(async move {
            coordinator
                .run(chat_request(
                    &agent,
                    SESSION,
                    "Reconcile the private invoice summary",
                ))
                .await
                .unwrap()
        });
        gate.entered().await;
        let run = self
            .coordinator
            .state
            .read()
            .await
            .runs
            .for_session(&self.agent, SESSION)[0]
            .clone();
        (run, worker)
    }

    async fn saved(&self) -> ControlPlaneSnapshot {
        load_control_plane_snapshot(&self.store)
            .await
            .unwrap()
            .unwrap()
    }

    fn progress(&self) -> crate::tools::todo::AgentTodos {
        crate::tools::todo::read_agent_todos(Some(&self.root), &self.agent).unwrap()
    }

    fn preserved(&self, output: &str) {
        assert_eq!(
            std::fs::read(self.root.join("invoices.csv")).unwrap(),
            INPUT.as_bytes()
        );
        assert_eq!(
            std::fs::read(self.root.join("summary.csv")).unwrap(),
            output.as_bytes()
        );
        assert_eq!(
            std::fs::read(self.root.join("sentinel.txt")).unwrap(),
            SENTINEL
        );
    }

    fn checkpoints(&self, run: &RunRecord) -> Vec<NativeCheckpointSummary> {
        NativeCheckpointService::new(self.checkpoint_root.clone())
            .list(&self.root, &link(run))
            .unwrap()
    }

    fn assert_checkpoint(&self, run: &RunRecord, id: &str) -> Vec<NativeCheckpointSummary> {
        let checkpoints = self.checkpoints(run);
        assert_eq!(checkpoints.len(), 1);
        let checkpoint = &checkpoints[0];
        assert_eq!(checkpoint.path, "summary.csv");
        assert_eq!(checkpoint.status, "acknowledged");
        assert_eq!(checkpoint.native_run_id, run.id);
        assert_eq!(checkpoint.native_tool_call_id, id);
        assert!(!checkpoint.checkpoint_id.is_empty());
        // Read actual content-addressed objects independently of service output.
        let scopes = std::fs::read_dir(self.checkpoint_root.join("scopes"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        assert_eq!(scopes.len(), 1);
        for bytes in [DRAFT.as_bytes(), OUTPUT.as_bytes()] {
            let hash = anima_core::filesystem_checkpoints::checkpoint_digest(bytes);
            let path = scopes[0]
                .join("objects")
                .join(&hash[..2])
                .join(format!("{hash}.blob"));
            assert_eq!(std::fs::read(path).unwrap(), bytes);
        }
        checkpoints
    }

    fn restore(
        &self,
        snapshot: ControlPlaneSnapshot,
        model: Arc<ScriptedModel>,
    ) -> AgentRunCoordinator {
        let mut fresh = DaemonState::with_model_adapter(model);
        fresh.replace_memory_embeddings(crate::memory_embeddings::MemoryEmbeddingRuntime::disabled());
        fresh.restore_control_plane_snapshot(snapshot).unwrap();
        // Host configuration is deliberately not restored from model/profile data.
        fresh.checkpoint_root = Some(self.checkpoint_root.clone());
        fresh.set_control_plane_store(Some(self.store.clone()));
        AgentRunCoordinator::new(Arc::new(RwLock::new(fresh)), Arc::new(Semaphore::new(1)))
    }
}

fn link(run: &RunRecord) -> RunLink {
    RunLink {
        run_id: run.id.clone(),
        session_id: run.session_id.clone(),
        agent_id: run.agent_id.clone(),
    }
}

async fn join(worker: JoinHandle<AgentRunEnvelope>) -> AgentRunEnvelope {
    tokio::time::timeout(Duration::from_secs(5), worker)
        .await
        .expect("native task settles within five seconds")
        .unwrap()
}

async fn entered_save(gate: &crate::state::TestControlPlaneSaveGate) {
    tokio::time::timeout(Duration::from_secs(5), gate.entered.acquire())
        .await
        .expect("selected persistence boundary is reached")
        .unwrap()
        .forget();
}

fn assert_calls(model: &ScriptedModel, count: usize) {
    assert_eq!(model.requests().len(), count);
    assert!(model.secondary_requests().is_empty());
}

fn pending(fixture: &Fixture) -> String {
    let tasks = fixture.progress();
    assert_eq!(tasks.tasks.len(), 2);
    assert_eq!(tasks.tasks[0].id.as_deref(), Some("reconcile"));
    assert_eq!(tasks.tasks[0].status, "in_progress");
    assert_eq!(tasks.tasks[1].id.as_deref(), Some("verify"));
    assert_eq!(tasks.tasks[1].status, "pending");
    tasks.revision
}

async fn quiet(fresh: &AgentRunCoordinator, model: &ScriptedModel) {
    let before =
        serde_json::to_value(fresh.state.read().await.control_plane_snapshot().runs).unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        serde_json::to_value(fresh.state.read().await.control_plane_snapshot().runs).unwrap(),
        before
    );
    assert_calls(model, 0);
}

fn initial_reads() -> Step {
    Step::Tools(vec![
        read("a7-input", "invoices.csv"),
        call("a7-task-read", "todo_list", serde_json::json!({})),
    ])
}

#[tokio::test]
async fn a7_useful_native_task_interrupts_to_review_then_explicitly_verifies_saved_output() {
    let gate = Gate::new();
    let model = ScriptedModel::gated(
        vec![
            initial_reads(),
            Step::Tools(vec![
                tasks("a7-pending", false),
                arithmetic("a7-cedar", "1250+750"),
                arithmetic("a7-orchid", "30+0"),
            ]),
            Step::Tools(vec![write("a7-write", OUTPUT)]),
            Step::Tools(vec![tasks("a7-forbidden-done", true)]),
            Step::Panic("interrupted original task must not continue"),
        ],
        gate.clone(),
    );
    let fixture = Fixture::new(model.clone());
    let (run, worker) = fixture.start(&gate).await;
    for _ in 0..3 {
        gate.release();
        gate.entered().await;
    }
    fixture.preserved(OUTPUT);
    let task_revision = pending(&fixture);
    let checkpoints = fixture.assert_checkpoint(&run, "a7-write");
    let snapshot = fixture.saved().await;
    let saved = snapshot
        .runs
        .iter()
        .find(|saved| saved.id == run.id)
        .unwrap()
        .clone();
    assert_eq!(saved.status, RunStatus::Running);
    assert!(saved.reply_message_id.is_none());
    let write = saved
        .action_receipts
        .iter()
        .find(|receipt| receipt.tool_call_id == "a7-write")
        .unwrap();
    assert_eq!(write.status, ActionStatus::Succeeded);
    assert_eq!(write.durability, ActionDurability::Persistent);
    assert!(!saved
        .action_receipts
        .iter()
        .any(|receipt| receipt.tool_call_id == "a7-forbidden-done"));
    // Finish every original worker before a fresh runtime can write this store.
    fixture
        .coordinator
        .stop_run(&fixture.agent, &run.id)
        .await
        .unwrap();
    join(worker).await;
    fixture.coordinator.shutdown().await;
    assert_calls(&model, 4);

    let review = ScriptedModel::new(vec![
        Step::Tools(vec![read("review-input", "invoices.csv"), read("review-output", "summary.csv"), call("review-tasks", "todo_list", serde_json::json!({}))]),
        Step::Tools(vec![arithmetic("review-cedar", "1250+750"), arithmetic("review-orchid", "30+0")]),
        Step::Tools(vec![tasks("review-done", true)]),
        Step::Text(vec!["Verified saved summary: Cedar 2 open invoices, 2000 cents; Orchid 2 open invoices, 30 cents. Paid invoices excluded."]),
        Step::Panic("review has exactly four model calls"),
    ]);
    let fresh = fixture.restore(snapshot, review.clone());
    {
        let state = fresh.state.read().await;
        let restored = state.runs.get(&run.id).unwrap();
        assert_eq!(restored.status, RunStatus::Interrupted);
        assert_eq!(restored.error.as_ref().unwrap().code, "restart_during_run");
        assert!(restored.reply_message_id.is_none());
        assert_eq!(restored.action_receipts, saved.action_receipts);
        assert!(state.live.runs().control(&run.id).is_none());
    }
    quiet(&fresh, &review).await;
    assert_eq!(pending(&fixture), task_revision);
    assert_eq!(fixture.checkpoints(&run), checkpoints);
    fixture.preserved(OUTPUT);
    let outcome = tokio::time::timeout(Duration::from_secs(5), fresh.run(chat_request(&fixture.agent, SESSION, "Explicitly review the interrupted saved summary; read source, output and tasks, verify each total, then complete the saved tasks"))).await.unwrap().unwrap();
    assert_eq!(outcome.result.status, "success");
    assert_calls(&review, 4);
    let requests = review.requests();
    for (id, expected) in [("review-input", INPUT), ("review-output", OUTPUT)] {
        let actual = requests[1]
            .messages
            .iter()
            .find(|message| {
                message.role == MessageRole::Tool
                    && message
                        .content
                        .metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("toolCallId"))
                        == Some(&DataValue::String(id.into()))
            })
            .unwrap();
        for line in expected.lines() {
            assert!(actual.content.text.contains(line));
        }
    }
    for (id, expected) in [("review-cedar", "2000"), ("review-orchid", "30")] {
        let actual = requests[2]
            .messages
            .iter()
            .find(|message| {
                message.role == MessageRole::Tool
                    && message
                        .content
                        .metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("toolCallId"))
                        == Some(&DataValue::String(id.into()))
            })
            .unwrap();
        assert_eq!(actual.content.text.trim(), expected);
    }
    let tasks = fixture.progress();
    assert_eq!(tasks.tasks.len(), 2);
    assert!(tasks.tasks.iter().all(|task| task.status == "completed"));
    assert_eq!(tasks.tasks[0].id.as_deref(), Some("reconcile"));
    assert_eq!(tasks.tasks[1].id.as_deref(), Some("verify"));
    assert_ne!(tasks.revision, task_revision);
    fixture.preserved(OUTPUT);
    assert_eq!(fixture.assert_checkpoint(&run, "a7-write"), checkpoints);
    let saved = fixture.saved().await;
    let old = saved.runs.iter().find(|saved| saved.id == run.id).unwrap();
    assert_eq!(old.status, RunStatus::Interrupted);
    assert!(!old
        .action_receipts
        .iter()
        .any(|receipt| receipt.tool_call_id == "a7-forbidden-done"));
    fresh.shutdown().await;
}

#[tokio::test]
async fn a7_actual_write_with_failed_outcome_save_preserves_started_and_unfinished_review() {
    let gate = Gate::new();
    let model = ScriptedModel::gated(
        vec![
            initial_reads(),
            Step::Tools(vec![tasks("uncertain-pending", false)]),
            Step::Tools(vec![write("uncertain-write", OUTPUT)]),
            Step::Panic("uncertain native outcome must not reach another model request"),
        ],
        gate.clone(),
    );
    let fixture = Fixture::new(model.clone());
    let (run, worker) = fixture.start(&gate).await;
    for _ in 0..2 {
        gate.release();
        gate.entered().await;
    }
    let task_revision = pending(&fixture);
    let intent = fixture
        .coordinator
        .state
        .write()
        .await
        .install_test_control_plane_save_gate(false);
    gate.release();
    entered_save(&intent).await;
    let failed = fixture
        .coordinator
        .state
        .write()
        .await
        .install_test_control_plane_save_gate(true);
    intent.release.add_permits(1);
    entered_save(&failed).await;
    fixture.preserved(OUTPUT);
    let checkpoints = fixture.assert_checkpoint(&run, "uncertain-write");
    failed.release.add_permits(1);
    let outcome = join(worker).await;
    assert_eq!(outcome.result.status, "error");
    assert_eq!(
        outcome.result.error.as_deref(),
        Some(super::action_receipts::RECEIPT_NOT_SAVED)
    );
    fixture.coordinator.shutdown().await;
    assert_calls(&model, 3);
    assert_eq!(pending(&fixture), task_revision);
    let snapshot = fixture.saved().await;
    let saved = snapshot
        .runs
        .iter()
        .find(|saved| saved.id == run.id)
        .unwrap()
        .clone();
    assert_eq!(saved.status, RunStatus::Failed);
    assert!(saved.stop.is_none());
    assert!(saved.reply_message_id.is_none());
    let receipt = saved
        .action_receipts
        .iter()
        .find(|receipt| receipt.tool_call_id == "uncertain-write")
        .unwrap();
    assert_eq!(receipt.status, ActionStatus::Started);
    assert_eq!(receipt.durability, ActionDurability::Persistent);
    assert!(receipt.result_preview.is_none());
    assert!(receipt.finished_at_ms.is_none());
    assert!(receipt.duration_ms.is_none());
    let quiet_model = ScriptedModel::new(vec![Step::Panic("failed outcome restore never replays")]);
    let fresh = fixture.restore(snapshot, quiet_model.clone());
    quiet(&fresh, &quiet_model).await;
    assert_eq!(
        fresh
            .state
            .read()
            .await
            .runs
            .get(&run.id)
            .unwrap()
            .action_receipts,
        saved.action_receipts
    );
    assert_eq!(pending(&fixture), task_revision);
    assert_eq!(
        fixture.assert_checkpoint(&run, "uncertain-write"),
        checkpoints
    );
    fixture.preserved(OUTPUT);
    fresh.shutdown().await;
}

#[tokio::test]
async fn a7_owner_stop_before_later_write_preserves_progress_and_quiet_recovery() {
    let gate = Gate::new();
    let model = ScriptedModel::gated(
        vec![
            initial_reads(),
            Step::Tools(vec![
                tasks("stop-pending", false),
                write("stop-acknowledged-write", OUTPUT),
            ]),
            Step::Tools(vec![
                write(
                    "stop-forbidden-write",
                    "must never replace verified bytes\n",
                ),
                tasks("stop-forbidden-done", true),
            ]),
            Step::Panic("owner stop must not continue"),
        ],
        gate.clone(),
    );
    let fixture = Fixture::new(model.clone());
    let (run, worker) = fixture.start(&gate).await;
    for _ in 0..2 {
        gate.release();
        gate.entered().await;
    }
    let task_revision = pending(&fixture);
    fixture.preserved(OUTPUT);
    let checkpoints = fixture.assert_checkpoint(&run, "stop-acknowledged-write");
    tokio::time::timeout(Duration::from_secs(5), async {
        fixture
            .coordinator
            .stop_run(&fixture.agent, &run.id)
            .await
            .unwrap();
        join(worker).await;
        fixture.coordinator.shutdown().await;
    })
    .await
    .expect("owner Stop and all original work settle within five seconds");
    assert_calls(&model, 3);
    fixture.preserved(OUTPUT);
    assert_eq!(pending(&fixture), task_revision);
    let snapshot = fixture.saved().await;
    let saved = snapshot
        .runs
        .iter()
        .find(|saved| saved.id == run.id)
        .unwrap()
        .clone();
    assert_eq!(saved.status, RunStatus::Cancelled);
    assert!(saved.stop.is_some());
    assert_eq!(
        saved
            .action_receipts
            .iter()
            .find(|receipt| receipt.tool_call_id == "stop-acknowledged-write")
            .unwrap()
            .status,
        ActionStatus::Succeeded
    );
    assert!(!saved
        .action_receipts
        .iter()
        .any(|receipt| receipt.tool_call_id == "stop-forbidden-write"
            || receipt.tool_call_id == "stop-forbidden-done"));
    let quiet_model = ScriptedModel::new(vec![Step::Panic("stopped task never replays")]);
    let fresh = fixture.restore(snapshot, quiet_model.clone());
    quiet(&fresh, &quiet_model).await;
    assert_eq!(
        fresh
            .state
            .read()
            .await
            .runs
            .get(&run.id)
            .unwrap()
            .action_receipts,
        saved.action_receipts
    );
    assert_eq!(pending(&fixture), task_revision);
    assert_eq!(fixture.checkpoints(&run), checkpoints);
    fixture.preserved(OUTPUT);
    fresh.shutdown().await;
}
