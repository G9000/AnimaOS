//! M47 deterministic coding acceptance. Real native file/todo/helper handlers,
//! synthetic model steps, no provider or autonomous-quality claim. The single
//! real-shell case is explicitly ignored until root's shell ownership GREEN.
//! Existing core budget, steer, Stop/helper and receipt matrices remain reused.
use super::test_support::{accept, add_chat, chat_request, lead_config, Gate, ScriptedModel, Step};
use super::{AcceptedRun, AgentRunCoordinator, SessionRunMode};
use crate::approvals::{ApprovalPolicy, PolicyAction, RiskClass};
use crate::control_plane_store::{
    load_control_plane_snapshot, ControlPlaneStoreConfig, WorkspaceConfig,
};
use crate::runs::{ActionDurability, ActionStatus, RunRecord, RunStatus};
use crate::state::DaemonState;
use anima_core::{DataValue, Message, MessageRole, ToolCall};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime},
};
use tokio::sync::{RwLock, Semaphore};

const SOURCE: &str = "src/invoice-summary.mjs";
const BUGGY: &str = include_str!(
    "../../../../packages/core-rust/acceptance/coding-m47/fixture/src/invoice-summary.mjs"
);
const REFERENCE: &str = include_str!(
    "../../../../packages/core-rust/acceptance/coding-m47/reference/invoice-summary.mjs"
);
const FILES:&[(&str,&str)]=&[
    (SOURCE,BUGGY),
    ("src/cli.mjs",include_str!("../../../../packages/core-rust/acceptance/coding-m47/fixture/src/cli.mjs")),
    ("test/invoice-summary.test.mjs",include_str!("../../../../packages/core-rust/acceptance/coding-m47/fixture/test/invoice-summary.test.mjs")),
    ("data/invoices.csv",include_str!("../../../../packages/core-rust/acceptance/coding-m47/fixture/data/invoices.csv")),
    ("README.md",include_str!("../../../../packages/core-rust/acceptance/coding-m47/fixture/README.md")),
    ("sentinel.txt",include_str!("../../../../packages/core-rust/acceptance/coding-m47/fixture/sentinel.txt")),
];
fn call(
    id: &str,
    name: &str,
    args: impl IntoIterator<Item = (&'static str, DataValue)>,
) -> ToolCall {
    ToolCall {
        id: id.into(),
        name: name.into(),
        args: args.into_iter().map(|(k, v)| (k.into(), v)).collect(),
    }
}
fn read(id: &str, path: &str) -> ToolCall {
    call(
        id,
        "read_file",
        [("file_path", DataValue::String(path.into()))],
    )
}
fn edit(id: &str, old: &str, new: &str) -> ToolCall {
    call(
        id,
        "edit_file",
        [
            ("file_path", DataValue::String(SOURCE.into())),
            ("old_string", DataValue::String(old.into())),
            ("new_string", DataValue::String(new.into())),
        ],
    )
}
fn helper() -> ToolCall {
    call("m47-helper","spawn_helper",[("name",DataValue::String("Invoice reviewer".into())),("task",DataValue::String("Inspect src/invoice-summary.mjs and tests. Explain decimal/void/sort defects and edge cases; advisory only, make no edits and run no commands.".into()))])
}
fn todos(id: &str, repaired: bool) -> ToolCall {
    let rows = [
        (
            "Repair cents and void handling",
            if repaired { "completed" } else { "in_progress" },
            "Repairing invoice aggregation",
        ),
        (
            "Run the independent CLI/oracle",
            "pending",
            "Verifying actual generated output",
        ),
    ];
    call(
        id,
        "todo_write",
        [(
            "todos",
            DataValue::Array(
                rows.into_iter()
                    .map(|(content, status, active)| {
                        DataValue::Object(BTreeMap::from([
                            ("content".into(), DataValue::String(content.into())),
                            ("status".into(), DataValue::String(status.into())),
                            ("activeForm".into(), DataValue::String(active.into())),
                        ]))
                    })
                    .collect(),
            ),
        )],
    )
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Witness {
    bytes: Vec<u8>,
    sha256: String,
    modified: SystemTime,
}
fn witness(path: &Path) -> Witness {
    let bytes = std::fs::read(path).unwrap();
    Witness {
        sha256: hex::encode(Sha256::digest(&bytes)),
        bytes,
        modified: std::fs::metadata(path).unwrap().modified().unwrap(),
    }
}
struct Fixture {
    _private: tempfile::TempDir,
    root: PathBuf,
    store: ControlPlaneStoreConfig,
    coordinator: AgentRunCoordinator,
    agent: String,
    neighbors: BTreeMap<PathBuf, Witness>,
}
impl Fixture {
    async fn new(model: Arc<ScriptedModel>, rounds: usize) -> Self {
        let private = tempfile::tempdir().unwrap();
        let root = private.path().join("workspace");
        std::fs::create_dir_all(&root).unwrap();
        for (relative, contents) in FILES {
            let path = root.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, contents).unwrap();
        }
        let neighbor = private.path().join("independent-neighbor/keep.txt");
        std::fs::create_dir_all(neighbor.parent().unwrap()).unwrap();
        std::fs::write(&neighbor, b"m47 independent neighbor violet\n").unwrap();
        let neighbors = FILES
            .iter()
            .filter(|(relative, _)| *relative != SOURCE)
            .map(|(relative, _)| root.join(relative))
            .chain(std::iter::once(neighbor))
            .map(|path| {
                let bytes = witness(&path);
                (path, bytes)
            })
            .collect();
        let store = ControlPlaneStoreConfig::Json(private.path().join("control-plane.json"));
        let mut state = DaemonState::with_model_adapter(model);
        state.workspace = Some(WorkspaceConfig {
            root_path: root.clone(),
            company_name: "M47 synthetic coding".into(),
            mission: "Bounded offline fixture".into(),
            values: vec![],
        });
        state.set_control_plane_store(Some(store.clone()));
        let mut config = lead_config("M47 coding companion");
        config.tools = Some(
            state
                .tool_registry
                .resolve_descriptors([
                    "read_file",
                    "edit_file",
                    "write_file",
                    "todo_read",
                    "todo_write",
                    "spawn_helper",
                    "bash",
                ])
                .unwrap(),
        );
        let settings = config.settings.as_mut().unwrap();
        settings.max_tool_iterations = Some(rounds);
        settings.max_retries = Some(0);
        settings.max_tokens = Some(2048);
        settings.timeout_ms = Some(20_000);
        let agent = state.create_agent(config).unwrap().state.id;
        state.approvals.set_policy(
            &agent,
            ApprovalPolicy::default()
                .with(RiskClass::Write, PolicyAction::Allow)
                .with(RiskClass::Delegate, PolicyAction::Allow)
                .with(RiskClass::Exec, PolicyAction::Ask),
        );
        let coordinator =
            AgentRunCoordinator::new(Arc::new(RwLock::new(state)), Arc::new(Semaphore::new(2)));
        Self {
            _private: private,
            root,
            store,
            coordinator,
            agent,
            neighbors,
        }
    }
    async fn start(&self, session: &str, key: &str) -> RunRecord {
        add_chat(&self.coordinator, &self.agent, session).await;
        let start =
            self.coordinator
                .web_start(self.agent.clone(), session.into(), key.into(), key.into());
        let AcceptedRun::Created(record) = self
            .coordinator
            .accept_run(accept(&self.agent, session, key), start)
            .await
            .unwrap()
        else {
            panic!("new ordinary coding run")
        };
        record
    }
    async fn terminal(&self, id: &str) -> RunRecord {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let record = self
                    .coordinator
                    .state
                    .read()
                    .await
                    .runs
                    .get(id)
                    .unwrap()
                    .clone();
                if record.status.is_terminal() {
                    break record;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap()
    }
    fn unchanged_neighbors(&self) {
        for (path, before) in &self.neighbors {
            assert_eq!(
                &witness(path),
                before,
                "neighbor changed: {}",
                path.display()
            );
        }
    }
    fn task_state(&self) -> crate::tools::todo::AgentTodos {
        crate::tools::todo::read_agent_todos(Some(&self.root), &self.agent).unwrap()
    }
    async fn only_helper(&self, parent: &str) -> (String, RunRecord) {
        let state = self.coordinator.state.read().await;
        let helpers = state
            .list_agents()
            .into_iter()
            .filter(|a| super::is_helper_config(&a.state.config))
            .collect::<Vec<_>>();
        assert_eq!(helpers.len(), 1, "exactly one observed generated helper");
        let helper = &helpers[0];
        // Advisory-only is behavioral: edit/write grants intentionally survive.
        let names = helper
            .state
            .config
            .tools
            .as_ref()
            .unwrap()
            .iter()
            .map(|t| t.name.as_str())
            .collect::<Vec<_>>();
        assert!(names.contains(&"edit_file"));
        assert!(names.contains(&"write_file"));
        for forbidden in [
            "bash",
            "bg_start",
            "spawn_helper",
            "delegate_to_agent",
            "browser_navigate",
            "browser_act",
        ] {
            assert!(
                !helper.state.config.allows_tool(forbidden),
                "helper received {forbidden}"
            );
        }
        assert!(!names.iter().any(|n| n.starts_with("mcp_")));
        let rows = state.runs.for_agent(&helper.state.id);
        assert_eq!(rows.len(), 1);
        let run = RunRecord::clone(rows[0]);
        assert_eq!(run.parent_run_id.as_deref(), Some(parent));
        let session = state
            .sessions
            .get(&helper.state.id, &run.session_id)
            .unwrap();
        assert_eq!(
            session.parent_agent_id.as_deref(),
            Some(self.agent.as_str())
        );
        assert_eq!(session.parent_run_id.as_deref(), Some(parent));
        (helper.state.id.clone(), run)
    }
}
fn paired(messages: &[Message]) {
    let requested = messages
        .iter()
        .filter_map(|m| m.content.metadata.as_ref()?.get("toolCalls"))
        .flat_map(|v| match v {
            DataValue::Array(v) => v.as_slice(),
            _ => &[],
        })
        .map(|v| match v {
            DataValue::Object(v) => match &v["id"] {
                DataValue::String(id) => id.clone(),
                _ => panic!("tool id"),
            },
            _ => panic!("tool call"),
        })
        .collect::<Vec<_>>();
    let results = messages
        .iter()
        .filter(|m| m.role == MessageRole::Tool)
        .map(|m| {
            match m
                .content
                .metadata
                .as_ref()
                .unwrap()
                .get("toolCallId")
                .unwrap()
            {
                DataValue::String(id) => id.clone(),
                _ => panic!("tool result id"),
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(requested.len(), results.len());
    assert_eq!(
        requested.iter().collect::<BTreeSet<_>>().len(),
        requested.len()
    );
    assert_eq!(
        requested.into_iter().collect::<BTreeSet<_>>(),
        results.into_iter().collect::<BTreeSet<_>>()
    );
}

#[tokio::test]
async fn m47_coding_one_helper_exact_edit_correction_budget_and_checkpoint_are_real_native_effects()
{
    let gate = Gate::new();
    let model=ScriptedModel::gated(vec![
        Step::Tools(vec![read("m47-read-source",SOURCE),todos("m47-todo-start",false),helper()]),
        Step::Tools(vec![read("m47-helper-read",SOURCE),read("m47-helper-tests","test/invoice-summary.test.mjs")]),
        Step::Text(vec!["Advisory: preserve exact cents, ignore void invoices, compare code points; no files changed."]),
        Step::Tools(vec![edit("m47-stale-edit","m47 absent stale source sentinel","unreachable"),edit("m47-exact-edit",BUGGY,REFERENCE),todos("m47-todo-repaired",true)]),
        Step::Text(vec!["Source repaired through exact edit. Independent command/oracle remains pending; tool-round budget exhausted."]),
        Step::Tools(vec![edit("m47-forbidden-later-edit",REFERENCE,"must never run")]),
    ],gate.clone());
    let f = Fixture::new(model.clone(), 2).await;
    let run = f.start("chat:m47-coding", "repair-decimals-and-void").await;
    gate.entered().await;
    gate.release(); // parent read/todo/helper batch
    gate.entered().await;
    let (_, child) = f.only_helper(&run.id).await;
    gate.release();
    gate.entered().await; // helper advice pending; steer joins before next parent call
    let correction="Preserve customer case and sort by Unicode code points; do not mark independent verification done.";
    let mut steer = accept(&f.agent, "chat:m47-coding", "m47-corrected-requirement");
    steer.text = correction.into();
    steer.mode = SessionRunMode::Steer;
    let start = f.coordinator.web_start(
        f.agent.clone(),
        steer.session_id.clone(),
        steer.text.clone(),
        steer.idempotency_key.clone(),
    );
    let AcceptedRun::Steered(active) = f.coordinator.accept_run(steer, start).await.unwrap() else {
        panic!("correction joins active run")
    };
    assert_eq!(active.id, run.id);
    gate.release();
    gate.entered().await;
    assert!(model.requests()[3]
        .messages
        .iter()
        .any(|m| m.content.text == correction));
    gate.release();
    gate.entered().await; // actual native effects saved; final reply is still held
    assert_eq!(
        std::fs::read_to_string(f.root.join(SOURCE)).unwrap(),
        REFERENCE
    );
    f.unchanged_neighbors();
    let tasks = f.task_state();
    assert_eq!(tasks.tasks[0].status, "completed");
    assert_eq!(tasks.tasks[1].status, "pending");
    let task_revision = tasks.revision.clone();
    let saved = load_control_plane_snapshot(&f.store)
        .await
        .unwrap()
        .unwrap();
    let checkpoint = saved.runs.iter().find(|r| r.id == run.id).unwrap().clone();
    assert_eq!(checkpoint.status, RunStatus::Running);
    assert!(checkpoint.reply_message_id.is_none());
    let stale = checkpoint
        .action_receipts
        .iter()
        .find(|r| r.tool_call_id == "m47-stale-edit")
        .unwrap();
    assert_eq!(stale.status, ActionStatus::Failed);
    assert!(stale.result_preview.as_ref().unwrap().contains("not found"));
    let exact = checkpoint
        .action_receipts
        .iter()
        .find(|r| r.tool_call_id == "m47-exact-edit")
        .unwrap();
    assert_eq!(exact.status, ActionStatus::Succeeded);
    assert_eq!(exact.durability, ActionDurability::Persistent);
    let restored_model = ScriptedModel::new(vec![]);
    let mut restored = DaemonState::with_model_adapter(restored_model.clone());
    restored.restore_control_plane_snapshot(saved).unwrap();
    assert_eq!(
        restored.runs.get(&run.id).unwrap().status,
        RunStatus::Interrupted
    );
    assert_eq!(
        restored.runs.get(&run.id).unwrap().action_receipts,
        checkpoint.action_receipts
    );
    assert_eq!(
        restored
            .runs
            .get(&child.id)
            .unwrap()
            .parent_run_id
            .as_deref(),
        Some(run.id.as_str())
    );
    assert!(restored_model.requests().is_empty());
    assert!(restored.live.runs().control(&run.id).is_none());
    assert_eq!(f.task_state().revision, task_revision);
    f.unchanged_neighbors();
    let requests = model.requests();
    assert_eq!(requests.len(), 5);
    assert!(requests[4].system.contains("Tool-round budget exhausted"));
    let configs = model.configs();
    assert_eq!(configs[4].tools, Some(vec![]));
    assert_eq!(configs[4].settings.as_ref().unwrap().max_retries, Some(0));
    gate.release();
    let finished = f.terminal(&run.id).await;
    assert_eq!(finished.status, RunStatus::Completed);
    assert_eq!(model.requests().len(), 5);
    assert!(model.secondary_requests().is_empty());
    let (helper_id, helper_run) = f.only_helper(&run.id).await;
    assert_eq!(helper_run.status, RunStatus::Completed);
    assert!(helper_run
        .action_receipts
        .iter()
        .all(|r| r.name == "read_file" && r.status == ActionStatus::Succeeded));
    let state = f.coordinator.state.read().await;
    paired(state.agents[&f.agent].messages());
    paired(state.agents[&helper_id].messages());
    assert!(!finished
        .action_receipts
        .iter()
        .any(|r| r.tool_call_id == "m47-forbidden-later-edit"));
    drop(state);
    assert_eq!(
        std::fs::read_to_string(f.root.join(SOURCE)).unwrap(),
        REFERENCE
    );
    f.unchanged_neighbors();
}

#[tokio::test]
async fn m47_coding_failed_started_receipt_preserves_source_and_unfinished_tasks() {
    let gate = Gate::new();
    let model = ScriptedModel::gated(
        vec![
            Step::Tools(vec![
                read("receipt-read", SOURCE),
                todos("receipt-todo", false),
            ]),
            Step::Tools(vec![edit("receipt-edit", BUGGY, REFERENCE)]),
            Step::Tools(vec![todos("receipt-false-done", true)]),
        ],
        gate.clone(),
    );
    let f = Fixture::new(model.clone(), 4).await;
    let coordinator = f.coordinator.clone();
    let agent = f.agent.clone();
    let running = tokio::spawn(async move {
        coordinator
            .run(chat_request(
                &agent,
                "chat:m47-receipt",
                "repair after checkpoint",
            ))
            .await
    });
    gate.entered().await;
    gate.release();
    gate.entered().await;
    let before = f.task_state();
    let failed = f
        .coordinator
        .state
        .write()
        .await
        .install_test_control_plane_save_gate(true);
    gate.release();
    tokio::time::timeout(Duration::from_secs(5), failed.entered.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    failed.release.add_permits(1);
    let outcome = tokio::time::timeout(Duration::from_secs(5), running)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(outcome.result.status, "error");
    assert_eq!(
        outcome.result.error.as_deref(),
        Some(super::action_receipts::RECEIPT_NOT_SAVED)
    );
    assert_eq!(model.requests().len(), 2);
    assert!(model.secondary_requests().is_empty());
    assert_eq!(std::fs::read_to_string(f.root.join(SOURCE)).unwrap(), BUGGY);
    assert_eq!(f.task_state().revision, before.revision);
    assert_eq!(f.task_state().tasks[0].status, "in_progress");
    assert_eq!(f.task_state().tasks[1].status, "pending");
    f.unchanged_neighbors();
    let state = f.coordinator.state.read().await;
    let rows = state.runs.for_session(&f.agent, "chat:m47-receipt");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].status, RunStatus::Failed);
    assert!(!rows[0]
        .action_receipts
        .iter()
        .any(|r| r.tool_call_id == "receipt-false-done"));
    paired(state.agents[&f.agent].messages());
}

#[tokio::test]
async fn m47_coding_parent_stop_reaches_linked_helper_without_later_file_or_task_effects() {
    let gate = Gate::new();
    let model = ScriptedModel::gated(
        vec![
            Step::Tools(vec![todos("stop-todo", false), helper()]),
            Step::Tools(vec![read("stop-helper-read", SOURCE)]),
            Step::Tools(vec![edit("stop-helper-write", BUGGY, REFERENCE)]),
            Step::Tools(vec![
                edit("stop-parent-write", BUGGY, REFERENCE),
                todos("stop-false-done", true),
            ]),
        ],
        gate.clone(),
    );
    let f = Fixture::new(model.clone(), 4).await;
    let run = f.start("chat:m47-stop", "inspect-before-coding").await;
    gate.entered().await;
    gate.release();
    gate.entered().await;
    gate.release();
    gate.entered().await;
    let (helper_id, child) = f.only_helper(&run.id).await;
    let tasks = f.task_state();
    let stopped = f.coordinator.stop_run(&f.agent, &run.id).await.unwrap();
    assert!(stopped.stop.is_some());
    gate.release();
    let parent = f.terminal(&run.id).await;
    let helper = f.terminal(&child.id).await;
    assert_eq!(parent.status, RunStatus::Cancelled);
    assert_eq!(helper.status, RunStatus::Cancelled);
    assert!(helper.stop.is_some());
    assert_eq!(model.requests().len(), 3);
    assert!(model.secondary_requests().is_empty());
    assert_eq!(std::fs::read_to_string(f.root.join(SOURCE)).unwrap(), BUGGY);
    assert_eq!(f.task_state().revision, tasks.revision);
    assert_eq!(f.task_state().tasks[0].status, "in_progress");
    f.unchanged_neighbors();
    assert!(!helper
        .action_receipts
        .iter()
        .any(|r| r.tool_call_id == "stop-helper-write"));
    assert!(!parent
        .action_receipts
        .iter()
        .any(|r| r.tool_call_id == "stop-parent-write" || r.tool_call_id == "stop-false-done"));
    let state = f.coordinator.state.read().await;
    paired(state.agents[&f.agent].messages());
    paired(state.agents[&helper_id].messages());
}

#[cfg(windows)]
#[tokio::test]
#[ignore = "root-only explicit installed Node after Windows shell ownership GREEN; zero providers"]
async fn m47_coding_real_shell_failure_edit_and_cli_output_have_paired_receipts() {
    use super::test_support::{decision, pending_approvals};
    use crate::approvals::ApprovalDecisionKind;
    use futures::FutureExt;
    use std::panic::AssertUnwindSafe;
    fn command_tasks(id: &str, done: bool) -> ToolCall {
        let mut result = todos(id, done);
        let DataValue::Array(items) = result.args.get_mut("todos").unwrap() else {
            unreachable!()
        };
        let DataValue::Object(verification) = &mut items[1] else {
            unreachable!()
        };
        verification.insert(
            "content".into(),
            DataValue::String("Run visible tests and CLI output verification".into()),
        );
        verification.insert(
            "status".into(),
            DataValue::String(if done { "completed" } else { "pending" }.into()),
        );
        result
    }
    fn bash(id: &str, command: &str) -> ToolCall {
        call(
            id,
            "bash",
            [
                ("command", DataValue::String(command.into())),
                ("cwd", DataValue::String(".".into())),
                ("timeout", DataValue::Number(15_000.0)),
            ],
        )
    }
    async fn allow(f: &Fixture, id: &str, command: &str) {
        let pending = pending_approvals(&f.coordinator, 1).await;
        assert_eq!(pending.len(), 1);
        let request = &pending[0];
        assert_eq!(request.tool_call_id, id);
        assert_eq!(request.tool, "bash");
        assert_eq!(request.class, RiskClass::Exec);
        assert!(!request.arguments_truncated);
        let args: serde_json::Value = serde_json::from_str(&request.arguments).unwrap();
        assert_eq!(args["command"], command);
        assert_eq!(args["cwd"], ".");
        assert_eq!(args["timeout"], 15_000.0);
        f.coordinator
            .decide_approval(
                &request.id,
                decision(ApprovalDecisionKind::AllowOnce, request.revision),
            )
            .await
            .unwrap();
    }
    let node = PathBuf::from(
        std::env::var_os("ANIMAOS_M47_TEST_NODE")
            .expect("root supplies the explicit installed absolute Node executable"),
    );
    assert!(node.is_absolute() && node.is_file());
    let node_text = node
        .to_str()
        .expect("explicit Node path is Unicode")
        .replace('\\', "/");
    assert!(!node_text.chars().any(|c| matches!(c, '\r' | '\n' | '\0')));
    // POSIX single quoting preserves the exact selected executable, including
    // spaces/apostrophes; no PATH lookup or shell substitution is authorized.
    let quoted = format!("'{}'", node_text.replace('\'', "'\"'\"'"));
    let guard = "test -n \"$BASH_VERSION\" || exit 90\n";
    let baseline = format!("{guard}exec {quoted} --test test/invoice-summary.test.mjs");
    let verified=format!("{guard}{quoted} --test test/invoice-summary.test.mjs && {quoted} src/cli.mjs --input data/invoices.csv --output out.csv");
    let gate = Gate::new();
    let model=ScriptedModel::gated(vec![
        Step::Tools(vec![read("shell-read",SOURCE),command_tasks("shell-tasks-start",false),bash("shell-visible-fails",&baseline)]),
        Step::Tools(vec![edit("shell-exact-repair",BUGGY,REFERENCE)]),
        Step::Tools(vec![bash("shell-tests-cli-pass",&verified)]),
        Step::Tools(vec![command_tasks("shell-tasks-complete",true)]),
        Step::Text(vec!["The initial visible tests failed. Exact source repair, passing visible tests and generated CLI bytes are confirmed; the separate full oracle remains root-owned."]),
        Step::Tools(vec![edit("shell-forbidden-retry",REFERENCE,"must never run")]),
    ],gate.clone());
    let f = Fixture::new(model.clone(), 4).await;
    let run = f
        .start("chat:m47-real-shell", "bounded-visible-test-repair")
        .await;
    let outcome=AssertUnwindSafe(async {
        gate.entered().await;gate.release();allow(&f,"shell-visible-fails",&baseline).await;gate.entered().await;
        let initial=model.requests();let failure=initial[1].messages.iter().find(|m|m.role==MessageRole::Tool&&m.content.metadata.as_ref().and_then(|v|v.get("toolCallId"))==Some(&DataValue::String("shell-visible-fails".into()))).unwrap();
        assert!(failure.content.text.contains("not ok"),"actual Node TAP failure must reach the next model step: {}",failure.content.text);
        assert_eq!(std::fs::read_to_string(f.root.join(SOURCE)).unwrap(),BUGGY);assert!(!f.root.join("out.csv").exists());assert_eq!(f.task_state().tasks[1].status,"pending");f.unchanged_neighbors();
        gate.release();gate.entered().await;assert_eq!(std::fs::read_to_string(f.root.join(SOURCE)).unwrap(),REFERENCE);gate.release();allow(&f,"shell-tests-cli-pass",&verified).await;gate.entered().await;
        let expected=b"customer,open_count,total\nCedar,3,20.00\nOrchid,2,0.30\n";
        let output=witness(&f.root.join("out.csv"));assert_eq!(output.bytes,expected);assert_eq!(f.task_state().tasks[1].status,"pending","completion follows actual command/output evidence");
        let current=f.coordinator.state.read().await.runs.get(&run.id).unwrap().clone();
        assert_eq!(current.action_receipts.iter().find(|r|r.tool_call_id=="shell-visible-fails").unwrap().status,ActionStatus::Failed);
        let passed=current.action_receipts.iter().find(|r|r.tool_call_id=="shell-tests-cli-pass").unwrap();assert_eq!(passed.status,ActionStatus::Succeeded);assert_eq!(passed.durability,ActionDurability::Persistent);assert!(passed.result_preview.as_ref().unwrap().contains("pass"));
        gate.release();gate.entered().await;assert!(f.task_state().tasks.iter().all(|t|t.status=="completed"));assert_eq!(model.configs()[4].tools,Some(vec![]));assert!(model.requests()[4].system.contains("Tool-round budget exhausted"));gate.release();let finished=f.terminal(&run.id).await;assert_eq!(finished.status,RunStatus::Completed);
        assert_eq!(model.requests().len(),5);assert!(model.secondary_requests().is_empty());assert_eq!(finished.action_receipts.len(),6);assert!(!finished.action_receipts.iter().any(|r|r.tool_call_id=="shell-forbidden-retry"));assert_eq!(witness(&f.root.join("out.csv")),output);f.unchanged_neighbors();
        let saved=load_control_plane_snapshot(&f.store).await.unwrap().unwrap();let persisted=saved.runs.iter().find(|r|r.id==run.id).unwrap();assert_eq!(persisted.action_receipts,finished.action_receipts);assert_eq!(saved.approvals.iter().filter(|a|a.run_id==run.id&&a.tool=="bash").count(),2);assert!(saved.approval_rules.is_empty());
        let state=f.coordinator.state.read().await;paired(state.agents[&f.agent].messages());assert_eq!(state.list_agents().len(),1);drop(state);
        println!("M47_CODING_NATIVE_ORACLE {}",serde_json::json!({"runId":run.id,"modelCalls":model.requests().len(),"secondaryCalls":model.secondary_requests().len(),"receiptCount":finished.action_receipts.len(),"execApprovals":2,"failedCommands":1,"successfulCommands":1,"outputBytes":output.bytes.len(),"outputSha256":output.sha256,"protectedFiles":f.neighbors.len()}));
    }).catch_unwind().await;
    // Even a failed assertion settles a live shell through ordinary owner Stop
    // before the isolated workspace disappears; shell Job/IO proof is separate.
    let cleanup = AssertUnwindSafe(async {
        let terminal = f
            .coordinator
            .state
            .read()
            .await
            .runs
            .get(&run.id)
            .unwrap()
            .status
            .is_terminal();
        if !terminal {
            f.coordinator.stop_run(&f.agent, &run.id).await.unwrap();
        }
        f.terminal(&run.id).await;
    })
    .catch_unwind()
    .await;
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    if let Err(panic) = cleanup {
        std::panic::resume_unwind(panic);
    }
}
