//! M52 real-handler wiring with a scripted model. No provider-quality claim.
//! Existing M47/M3 rejection, Stop, budget and Windows shell ownership matrices
//! remain authoritative; only the distinct fixture/Ask integration is added here.
use super::test_support::{
    accept, add_chat, decision, lead_config, pending_approvals, Gate, ScriptedModel, Step,
};
use super::{AcceptedRun, AgentRunCoordinator};
use crate::approvals::{ApprovalDecisionKind, ApprovalPolicy, PolicyAction, RiskClass};
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

const SOURCE: &str = "src/log-summary.mjs";
const BUGGY: &str = include_str!(
    "../../../../packages/core-rust/acceptance/provider-task-m52/fixture/src/log-summary.mjs"
);
const SKIP: &str = "    if (event.status === 'error') continue;\n";
const OVERWRITE: &str = "    group.durationMs = event.durationMs;";
const ACCUMULATE: &str = "    group.durationMs += event.durationMs;";
const EXPECTED:&[u8]=b"{\"services\":[{\"service\":\"api\",\"count\":3,\"errors\":1,\"durationMs\":210},{\"service\":\"worker\",\"count\":2,\"errors\":1,\"durationMs\":15}]}\n";
const FILES:&[(&str,&str)]=&[
    (SOURCE,BUGGY),
    ("src/cli.mjs",include_str!("../../../../packages/core-rust/acceptance/provider-task-m52/fixture/src/cli.mjs")),
    ("test/log-summary.test.mjs",include_str!("../../../../packages/core-rust/acceptance/provider-task-m52/fixture/test/log-summary.test.mjs")),
    ("data/events.jsonl",include_str!("../../../../packages/core-rust/acceptance/provider-task-m52/fixture/data/events.jsonl")),
    ("README.md",include_str!("../../../../packages/core-rust/acceptance/provider-task-m52/fixture/README.md")),
    ("sentinel.txt",include_str!("../../../../packages/core-rust/acceptance/provider-task-m52/fixture/sentinel.txt")),
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
    call("m52-helper","spawn_helper",[("name",DataValue::String("Log reviewer".into())),("task",DataValue::String("Inspect src/log-summary.mjs and test/log-summary.test.mjs only. Explain both aggregation defects and one input edge case with paths. Advisory only: do not edit files or execute commands.".into()))])
}
fn todos(id: &str, phase: usize) -> ToolCall {
    let rows = [
        (
            "Inspect and consult one advisory helper",
            if phase >= 1 {
                "completed"
            } else {
                "in_progress"
            },
        ),
        (
            "Repair only the two aggregation lines",
            if phase >= 2 { "completed" } else { "pending" },
        ),
        (
            "Verify tests and exact CLI output",
            if phase >= 3 { "completed" } else { "pending" },
        ),
    ];
    call(
        id,
        "todo_write",
        [(
            "todos",
            DataValue::Array(
                rows.into_iter()
                    .map(|(content, status)| {
                        DataValue::Object(BTreeMap::from([
                            ("content".into(), DataValue::String(content.into())),
                            ("status".into(), DataValue::String(status.into())),
                            ("activeForm".into(), DataValue::String(content.into())),
                        ]))
                    })
                    .collect(),
            ),
        )],
    )
}
fn bash(id: &str, command: &str) -> ToolCall {
    call(
        id,
        "bash",
        [
            ("command", DataValue::String(command.into())),
            ("cwd", DataValue::String(".".into())),
            ("timeout", DataValue::Number(10_000.0)),
        ],
    )
}
fn repaired() -> String {
    assert_eq!(BUGGY.matches(SKIP).count(), 1);
    assert_eq!(BUGGY.matches(OVERWRITE).count(), 1);
    BUGGY.replace(SKIP, "").replace(OVERWRITE, ACCUMULATE)
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
        sha256: format!("{:x}", Sha256::digest(&bytes)),
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
    protected: BTreeMap<PathBuf, Witness>,
}
impl Fixture {
    async fn new(model: Arc<ScriptedModel>) -> Self {
        let private = tempfile::tempdir().unwrap();
        let root = private.path().join("candidate");
        for (relative, contents) in FILES {
            let path = root.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, contents).unwrap();
        }
        let neighbor = private.path().join("independent-neighbor/keep.txt");
        std::fs::create_dir_all(neighbor.parent().unwrap()).unwrap();
        std::fs::write(&neighbor, b"m52 independent neighbor cobalt\n").unwrap();
        let protected = FILES
            .iter()
            .filter(|(path, _)| *path != SOURCE)
            .map(|(path, _)| root.join(path))
            .chain(std::iter::once(neighbor))
            .map(|path| {
                let before = witness(&path);
                (path, before)
            })
            .collect();
        let store = ControlPlaneStoreConfig::Json(private.path().join("control-plane.json"));
        let mut state = DaemonState::with_model_adapter(model);
        state.replace_memory_embeddings(crate::memory_embeddings::MemoryEmbeddingRuntime::disabled());
        state.workspace = Some(WorkspaceConfig {
            root_path: root.clone(),
            company_name: "M52 synthetic provider task".into(),
            mission: "One bounded offline repair".into(),
            values: vec![],
        });
        state.set_control_plane_store(Some(store.clone()));
        let mut config = lead_config("M52 task companion");
        config.provider = Some("chatgpt".into());
        config.model = "synthetic-provider-task-model".into();
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
        settings.max_tool_iterations = Some(4);
        settings.max_retries = Some(0);
        settings.max_tokens = Some(2048);
        settings.timeout_ms = Some(30_000);
        settings
            .additional
            .insert("autoTitle".into(), DataValue::Bool(false));
        settings
            .additional
            .insert("autoCompact".into(), DataValue::Bool(false));
        settings
            .additional
            .insert("agentFilesProfileEnabled".into(), DataValue::Bool(false));
        let agent = state.create_agent(config).unwrap().state.id;
        state.approvals.set_policy(
            &agent,
            ApprovalPolicy::default()
                .with(RiskClass::Read, PolicyAction::Allow)
                .with(RiskClass::Write, PolicyAction::Allow)
                .with(RiskClass::Delegate, PolicyAction::Ask)
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
            protected,
        }
    }
    async fn start(&self) -> RunRecord {
        add_chat(&self.coordinator, &self.agent, "chat:m52-task").await;
        let mut input = accept(&self.agent, "chat:m52-task", "m52-single-admission");
        input.text="Inspect the JSONL fixture, consult exactly one advisory helper, repair only the two aggregation defects, and verify only the declared commands. No extra delegation or providers.".into();
        let start = self.coordinator.web_start(
            self.agent.clone(),
            input.session_id.clone(),
            input.text.clone(),
            input.idempotency_key.clone(),
        );
        let AcceptedRun::Created(run) = self.coordinator.accept_run(input, start).await.unwrap()
        else {
            panic!("one new owner task")
        };
        run
    }
    async fn terminal(&self, id: &str) -> RunRecord {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let run = self
                    .coordinator
                    .state
                    .read()
                    .await
                    .runs
                    .get(id)
                    .unwrap()
                    .clone();
                if run.status.is_terminal() {
                    break run;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap()
    }
    async fn decide(&self, expected: &ToolCall, class: RiskClass, kind: ApprovalDecisionKind) {
        self.decide_exact_batch(expected, class, kind, 1).await;
    }
    async fn decide_exact_batch(
        &self,
        expected: &ToolCall,
        class: RiskClass,
        kind: ApprovalDecisionKind,
        expected_count: usize,
    ) {
        let pending = pending_approvals(&self.coordinator, expected_count).await;
        assert_eq!(pending.len(), expected_count);
        let request = pending
            .iter()
            .find(|request| request.tool_call_id == expected.id)
            .expect("the exact pending batch contains the frozen call ID");
        assert_eq!(request.tool_call_id, expected.id);
        assert_eq!(request.tool, expected.name);
        assert_eq!(request.class, class);
        assert!(!request.arguments_truncated);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&request.arguments).unwrap(),
            crate::routes::data_value_to_json(&DataValue::Object(expected.args.clone()))
        );
        self.coordinator
            .decide_approval(&request.id, decision(kind, request.revision))
            .await
            .unwrap();
    }
    fn protected_unchanged(&self) {
        for (path, before) in &self.protected {
            assert_eq!(
                &witness(path),
                before,
                "protected witness {}",
                path.display()
            );
        }
    }
    fn tasks(&self) -> crate::tools::todo::AgentTodos {
        crate::tools::todo::read_agent_todos(Some(&self.root), &self.agent).unwrap()
    }
    async fn helper_record(&self, parent: &str) -> (String, RunRecord) {
        let state = self.coordinator.state.read().await;
        let helpers = state
            .list_agents()
            .into_iter()
            .filter(|agent| super::is_helper_config(&agent.state.config))
            .collect::<Vec<_>>();
        assert_eq!(helpers.len(), 1);
        let helper = &helpers[0];
        let config = &helper.state.config;
        assert_eq!(config.provider, Some("chatgpt".into()));
        assert_eq!(config.model, "synthetic-provider-task-model");
        let settings = config.settings.as_ref().unwrap();
        assert_eq!(settings.max_tool_iterations, Some(4));
        assert_eq!(settings.max_retries, Some(0));
        assert_eq!(settings.max_tokens, Some(2048));
        assert_eq!(settings.timeout_ms, Some(30_000));
        for flag in ["autoTitle", "autoCompact", "agentFilesProfileEnabled"] {
            assert!(
                settings.additional.get(flag).is_none(),
                "helper additions are absent, not inherited false flags"
            );
        }
        assert!(
            config.allows_tool("edit_file") && config.allows_tool("write_file"),
            "observed advisory behavior is not an enforced read-only grant"
        );
        for denied in [
            "bash",
            "bg_start",
            "spawn_helper",
            "delegate_to_agent",
            "send_message",
            "broadcast_message",
            "browser_navigate",
        ] {
            assert!(!config.allows_tool(denied));
        }
        assert!(!config
            .tools
            .as_ref()
            .unwrap()
            .iter()
            .any(|tool| tool.name.starts_with("mcp_")));
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
        assert_eq!(session.kind, crate::sessions::SessionKind::Helper);
        assert!(session.summary.is_none() && session.context_trimmed.is_none());
        (helper.state.id.clone(), run)
    }
    async fn verify_and_quiet_reload(
        &self,
        model: &ScriptedModel,
        parent: &RunRecord,
        exec_count: usize,
    ) {
        let (helper, child) = self.helper_record(&parent.id).await;
        assert_eq!(child.status, RunStatus::Completed);
        assert!(child.action_receipts.iter().all(
            |receipt| receipt.name == "read_file" && receipt.status == ActionStatus::Succeeded
        ));
        assert_eq!(child.action_receipts.len(), 2);
        assert_eq!(
            parent.action_receipts.len(),
            // Owner-denied calls never enter the native action journal.
            if exec_count == 0 { 9 } else { 11 }
        );
        for id in ["m52-remove-skip", "m52-accumulate"] {
            let receipt = parent
                .action_receipts
                .iter()
                .find(|receipt| receipt.tool_call_id == id)
                .unwrap();
            assert_eq!(receipt.status, ActionStatus::Succeeded);
            assert_eq!(receipt.durability, ActionDurability::Persistent);
        }
        let guard = self.coordinator.state.read().await;
        assert_eq!(guard.list_agents().len(), 2);
        assert_eq!(guard.runs.for_agent(&self.agent).len(), 1);
        assert_eq!(guard.runs.for_agent(&helper).len(), 1);
        assert_eq!(
            guard.approvals.policy(&self.agent).delegate,
            PolicyAction::Ask
        );
        assert_eq!(guard.approvals.policy(&self.agent).exec, PolicyAction::Ask);
        assert!(guard.approvals.rules_for(&self.agent).is_empty());
        let embedding_status = guard.memory_embeddings.read().await.status();
        assert!(!embedding_status.enabled);
        assert_eq!(embedding_status.vector_count, 0);
        for agent in [&self.agent, &helper] {
            paired(guard.agents[agent].messages());
        }
        drop(guard);
        for agent in [&self.agent, &helper] {
            assert!(
                !self
                    .coordinator
                    .learning()
                    .policy(agent)
                    .await
                    .unwrap()
                    .auto_enabled
            );
        }
        let configs = model.configs();
        assert_eq!(configs.len(), 7);
        assert_eq!(
            configs
                .iter()
                .filter(|config| super::is_helper_config(config))
                .count(),
            2
        );
        assert!(model.secondary_requests().is_empty());
        assert!(configs.len() <= 10);
        for config in &configs {
            assert_eq!(config.provider, Some("chatgpt".into()));
            assert_eq!(config.model, "synthetic-provider-task-model");
            let settings = config.settings.as_ref().unwrap();
            assert_eq!(settings.max_retries, Some(0));
            assert_eq!(settings.max_tool_iterations, Some(4));
            assert_eq!(settings.timeout_ms, Some(30_000));
            if !super::is_helper_config(config) {
                for flag in ["autoTitle", "autoCompact", "agentFilesProfileEnabled"] {
                    assert_eq!(settings.additional.get(flag), Some(&DataValue::Bool(false)));
                }
            }
        }
        assert_eq!(configs[6].tools, Some(vec![]));
        assert!(model.requests()[6]
            .system
            .contains("Tool-round budget exhausted"));
        let saved = load_control_plane_snapshot(&self.store)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(saved.runs.len(), 2);
        assert!(
            saved.jobs.is_empty()
                && saved.schedules.is_empty()
                && saved.connectors.is_empty()
                && saved.mcp_connections.is_empty()
                && saved.learning_reviews.is_empty()
        );
        assert!(saved.approval_rules.is_empty());
        assert_eq!(
            saved
                .approvals
                .iter()
                .filter(|request| request.class == RiskClass::Exec)
                .count(),
            exec_count
        );
        assert_eq!(saved.approvals.len(), if exec_count == 0 { 5 } else { 3 });
        assert_eq!(
            saved
                .approvals
                .iter()
                .filter(|request| request.class == RiskClass::Delegate
                    && request
                        .resolution
                        .as_ref()
                        .is_some_and(|resolution| resolution.decision
                            == Some(ApprovalDecisionKind::AllowOnce)))
                .count(),
            1
        );
        assert!(saved
            .approvals
            .iter()
            .all(|request| request
                .resolution
                .as_ref()
                .is_some_and(
                    |resolution| resolution.matcher.is_none() && resolution.rule_id.is_none()
                )));
        let before = serde_json::to_value(&saved).unwrap();
        let source = witness(&self.root.join(SOURCE));
        let output = self
            .root
            .join("out.json")
            .exists()
            .then(|| witness(&self.root.join("out.json")));
        let tasks = self.tasks();
        let quiet_model = ScriptedModel::new(vec![]);
        let mut restored = DaemonState::with_model_adapter(quiet_model.clone());
        restored.replace_memory_embeddings(crate::memory_embeddings::MemoryEmbeddingRuntime::disabled());
        restored.restore_control_plane_snapshot(saved).unwrap();
        let quiet =
            AgentRunCoordinator::new(Arc::new(RwLock::new(restored)), Arc::new(Semaphore::new(2)));
        tokio::time::sleep(Duration::from_millis(100)).await;
        let after =
            serde_json::to_value(quiet.state.read().await.control_plane_snapshot()).unwrap();
        for key in [
            "agents",
            "sessions",
            "runs",
            "approvals",
            "approvalPolicies",
            "approvalRules",
            "workspace",
            "toolGrantsApplied",
            "learningReviews",
        ] {
            if before[key].is_array() {
                // Registry iteration order is not canonical; every record and
                // each record's internal transcript/receipt order remain exact.
                let canonical = |value: &serde_json::Value| {
                    let mut records = value
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>();
                    records.sort();
                    records
                };
                assert_eq!(
                    canonical(&after[key]),
                    canonical(&before[key]),
                    "quiet reload retains {key}"
                );
            } else {
                assert_eq!(after[key], before[key], "quiet reload retains {key}");
            }
        }
        assert!(quiet_model.requests().is_empty() && quiet_model.secondary_requests().is_empty());
        assert_eq!(model.requests().len(), 7);
        assert!(model.secondary_requests().is_empty());
        assert!(quiet
            .state
            .read()
            .await
            .live
            .runs()
            .control(&parent.id)
            .is_none());
        assert!(quiet
            .state
            .read()
            .await
            .live
            .runs()
            .control(&child.id)
            .is_none());
        assert_eq!(witness(&self.root.join(SOURCE)), source);
        assert_eq!(
            self.root
                .join("out.json")
                .exists()
                .then(|| witness(&self.root.join("out.json"))),
            output
        );
        assert_eq!(self.tasks().revision, tasks.revision);
        assert_eq!(self.tasks().tasks, tasks.tasks);
        self.protected_unchanged();
    }
}
fn paired(messages: &[Message]) {
    let calls = messages
        .iter()
        .filter_map(|message| message.content.metadata.as_ref()?.get("toolCalls"))
        .flat_map(|value| match value {
            DataValue::Array(items) => items.as_slice(),
            _ => &[],
        })
        .map(|value| match value {
            DataValue::Object(row) => match &row["id"] {
                DataValue::String(id) => id.clone(),
                _ => panic!("tool ID"),
            },
            _ => panic!("tool call"),
        })
        .collect::<Vec<_>>();
    let results = messages
        .iter()
        .filter(|message| message.role == MessageRole::Tool)
        .map(
            |message| match &message.content.metadata.as_ref().unwrap()["toolCallId"] {
                DataValue::String(id) => id.clone(),
                _ => panic!("result ID"),
            },
        )
        .collect::<Vec<_>>();
    assert_eq!(calls.len(), results.len());
    assert_eq!(calls.iter().collect::<BTreeSet<_>>().len(), calls.len());
    assert_eq!(
        calls.into_iter().collect::<BTreeSet<_>>(),
        results.into_iter().collect::<BTreeSet<_>>()
    );
}
fn denied_delegation() -> Vec<ToolCall> {
    vec![
        call(
            "m52-extra-helper",
            "spawn_helper",
            [
                ("name", DataValue::String("Forbidden helper".into())),
                (
                    "task",
                    DataValue::String("No additional admission authorized".into()),
                ),
            ],
        ),
        call(
            "m52-peer-delegate",
            "delegate_to_agent",
            [
                ("agent_id", DataValue::String("nonexistent-peer".into())),
                (
                    "task",
                    DataValue::String("No peer admission authorized".into()),
                ),
            ],
        ),
        call(
            "m52-peer-send",
            "send_message",
            [
                ("to_agent_id", DataValue::String("nonexistent-peer".into())),
                (
                    "message",
                    DataValue::String("No peer admission authorized".into()),
                ),
            ],
        ),
        call(
            "m52-peer-broadcast",
            "broadcast_message",
            [(
                "message",
                DataValue::String("No broadcast authorized".into()),
            )],
        ),
    ]
}

#[tokio::test]
async fn m52_provider_task_one_helper_allow_once_denies_extra_admissions_and_reloads_quietly() {
    let gate = Gate::new();
    let denied = denied_delegation();
    let model=ScriptedModel::gated(vec![
        Step::Tools(vec![read("m52-inspect",SOURCE),read("m52-tests","test/log-summary.test.mjs"),todos("m52-todos-start",0),helper()]),
        Step::Tools(vec![read("m52-helper-source",SOURCE),read("m52-helper-tests","test/log-summary.test.mjs")]),
        Step::Text(vec!["src/log-summary.mjs skips error rows and overwrites duration totals; preserve empty input and exact service case. Advisory only, no edits or commands."]),
        Step::Tools(vec![edit("m52-remove-skip",SKIP,""),edit("m52-accumulate",OVERWRITE,ACCUMULATE),todos("m52-todos-repaired",2)]),
        Step::Tools(vec![read("m52-read-repair",SOURCE),call("m52-read-todos","todo_read",[])]),
        Step::Tools(denied.clone()),
        Step::Text(vec!["Both exact source edits are saved. Additional delegation was denied. Command verification remains pending; no provider-quality result is claimed."]),
        Step::Tools(vec![helper()]),
    ],gate.clone());
    let f = Fixture::new(model.clone()).await;
    let run = f.start().await;
    gate.entered().await;
    gate.release();
    f.decide(
        &helper(),
        RiskClass::Delegate,
        ApprovalDecisionKind::AllowOnce,
    )
    .await;
    gate.entered().await;
    f.helper_record(&run.id).await;
    assert_eq!(std::fs::read_to_string(f.root.join(SOURCE)).unwrap(), BUGGY);
    gate.release();
    gate.entered().await;
    f.protected_unchanged();
    gate.release();
    gate.entered().await;
    assert_eq!(std::fs::read_to_string(f.root.join(SOURCE)).unwrap(), BUGGY);
    gate.release();
    gate.entered().await;
    assert_eq!(
        std::fs::read_to_string(f.root.join(SOURCE)).unwrap(),
        repaired()
    );
    gate.release();
    gate.entered().await;
    gate.release();
    for (index, request) in denied.iter().enumerate() {
        f.decide_exact_batch(
            request,
            RiskClass::Delegate,
            ApprovalDecisionKind::Deny,
            denied.len() - index,
        )
        .await;
    }
    gate.entered().await;
    assert_eq!(f.tasks().tasks[2].status, "pending");
    gate.release();
    let terminal = f.terminal(&run.id).await;
    assert_eq!(terminal.status, RunStatus::Completed);
    assert!(!f.root.join("out.json").exists());
    for denied in &denied {
        // Approval refusal precedes journal.begin, so no native action starts.
        assert!(!terminal
            .action_receipts
            .iter()
            .any(|receipt| receipt.tool_call_id == denied.id));
        let requests = model.requests();
        let result = requests[6]
            .messages
            .iter()
            .find(|message| {
                message.role == MessageRole::Tool
                    && message
                        .content
                        .metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("toolCallId"))
                        == Some(&DataValue::String(denied.id.clone()))
            })
            .expect("every denied call has its paired tool result");
        assert!(result
            .content
            .text
            .contains(crate::approvals::DENIED_BY_OWNER));
    }
    f.verify_and_quiet_reload(&model, &terminal, 0).await;
}

#[tokio::test]
#[ignore = "HELD: root explicitly releases installed Node and owned shell for this synthetic M52 fixture; no provider"]
async fn m52_provider_task_real_shell_helper_baseline_two_line_repair_cli_and_quiet_reload() {
    use futures::FutureExt;
    use std::panic::AssertUnwindSafe;
    // Checked before fixture/model construction or command admission. This is a
    // synthetic local command release, separate from the held actual task.
    assert_eq!(
        std::env::var_os("ANIMAOS_M52_RUN_SYNTHETIC_SHELL").as_deref(),
        Some(std::ffi::OsStr::new("1")),
        "exact root synthetic shell opt-in required"
    );
    let node = PathBuf::from(
        std::env::var_os("ANIMAOS_M52_TEST_NODE").expect("root supplies installed absolute Node"),
    );
    assert!(node.is_absolute() && node.is_file());
    let node = node.to_str().unwrap().replace('\\', "/");
    assert!(!node.chars().any(|c| matches!(c, '\r' | '\n' | '\0')));
    let quoted = format!("'{}'", node.replace('\'', "'\"'\"'"));
    let guard = "test -n \"$BASH_VERSION\" || exit 90\n";
    let baseline = format!("{guard}exec {quoted} --test test/log-summary.test.mjs");
    let final_command=format!("{guard}{quoted} --test test/log-summary.test.mjs && {quoted} src/cli.mjs --input data/events.jsonl --output out.json");
    let baseline_call = bash("m52-baseline-fail", &baseline);
    let final_call = bash("m52-final-tests-cli", &final_command);
    let gate = Gate::new();
    let model=ScriptedModel::gated(vec![
        Step::Tools(vec![read("m52-inspect",SOURCE),read("m52-tests","test/log-summary.test.mjs"),todos("m52-todos-start",0),helper(),baseline_call.clone()]),
        Step::Tools(vec![read("m52-helper-source",SOURCE),read("m52-helper-tests","test/log-summary.test.mjs")]),
        Step::Text(vec!["src/log-summary.mjs skips error events and overwrites totals; empty input is a relevant edge case. Advisory only, no writes or commands."]),
        Step::Tools(vec![edit("m52-remove-skip",SKIP,""),edit("m52-accumulate",OVERWRITE,ACCUMULATE),todos("m52-todos-repaired",2)]),
        Step::Tools(vec![final_call.clone()]),
        Step::Tools(vec![todos("m52-todos-verified",3),call("m52-final-todo-read","todo_read",[])]),
        Step::Text(vec!["Initial visible tests failed. One advisory helper, two exact source edits, passing visible tests, and exact CLI output are observed. The independent oracle remains root-owned; synthetic wiring does not establish provider quality."]),
        Step::Tools(vec![bash("m52-forbidden-third-exec","must never execute")]),
    ],gate.clone());
    let f = Fixture::new(model.clone()).await;
    let run = f.start().await;
    let outcome = AssertUnwindSafe(async {
        gate.entered().await;
        gate.release();
        f.decide_exact_batch(
            &helper(),
            RiskClass::Delegate,
            ApprovalDecisionKind::AllowOnce,
            2, // The baseline Exec is pending concurrently in this tool round.
        )
        .await;
        gate.entered().await;
        f.helper_record(&run.id).await;
        gate.release();
        gate.entered().await;
        assert_eq!(std::fs::read_to_string(f.root.join(SOURCE)).unwrap(), BUGGY);
        f.protected_unchanged();
        gate.release();
        f.decide(
            &baseline_call,
            RiskClass::Exec,
            ApprovalDecisionKind::AllowOnce,
        )
        .await;
        gate.entered().await;
        let requests = model.requests();
        let failed = requests[3]
            .messages
            .iter()
            .find(|message| {
                message.role == MessageRole::Tool
                    && message
                        .content
                        .metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("toolCallId"))
                        == Some(&DataValue::String("m52-baseline-fail".into()))
            })
            .unwrap();
        assert!(failed.content.text.contains("not ok"));
        assert_eq!(std::fs::read_to_string(f.root.join(SOURCE)).unwrap(), BUGGY);
        assert!(!f.root.join("out.json").exists());
        gate.release();
        gate.entered().await;
        assert_eq!(
            std::fs::read_to_string(f.root.join(SOURCE)).unwrap(),
            repaired()
        );
        f.protected_unchanged();
        assert_eq!(f.tasks().tasks[2].status, "pending");
        gate.release();
        f.decide(
            &final_call,
            RiskClass::Exec,
            ApprovalDecisionKind::AllowOnce,
        )
        .await;
        gate.entered().await;
        assert_eq!(EXPECTED.len(), 129);
        assert_eq!(std::fs::read(f.root.join("out.json")).unwrap(), EXPECTED);
        assert_eq!(
            f.tasks().tasks[2].status,
            "pending",
            "completion follows actual command and CLI output"
        );
        let current = f
            .coordinator
            .state
            .read()
            .await
            .runs
            .get(&run.id)
            .unwrap()
            .clone();
        let failed = current
            .action_receipts
            .iter()
            .find(|receipt| receipt.tool_call_id == baseline_call.id)
            .unwrap();
        assert_eq!(failed.status, ActionStatus::Failed);
        let passed = current
            .action_receipts
            .iter()
            .find(|receipt| receipt.tool_call_id == final_call.id)
            .unwrap();
        assert_eq!(passed.status, ActionStatus::Succeeded);
        assert_eq!(passed.durability, ActionDurability::Persistent);
        assert!(passed.result_preview.as_ref().unwrap().contains("pass"));
        gate.release();
        gate.entered().await;
        assert!(f
            .tasks()
            .tasks
            .iter()
            .all(|todo| todo.status == "completed"));
        gate.release();
        let terminal = f.terminal(&run.id).await;
        assert_eq!(terminal.status, RunStatus::Completed);
        assert!(!terminal
            .action_receipts
            .iter()
            .any(|receipt| receipt.tool_call_id == "m52-forbidden-third-exec"));
        f.verify_and_quiet_reload(&model, &terminal, 2).await;
    })
    .catch_unwind()
    .await;
    let cleanup = AssertUnwindSafe(async {
        if !f
            .coordinator
            .state
            .read()
            .await
            .runs
            .get(&run.id)
            .unwrap()
            .status
            .is_terminal()
        {
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
