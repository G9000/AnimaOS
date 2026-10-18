//! Ordinary native task execution and completed-run restoration.
use super::test_support::{chat_request, companion_config, ScriptedModel, Step};
use super::AgentRunCoordinator;
use crate::approvals::{ApprovalPolicy, PolicyAction, RiskClass};
use crate::control_plane_store::{
    load_control_plane_snapshot, ControlPlaneStoreConfig, WorkspaceConfig,
};
use crate::state::DaemonState;
use anima_core::{DataValue, MessageRole, ModelAdapter, ToolCall};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::{RwLock, Semaphore};

const INPUT: &str = "customer,status,cents\nCedar,open,1250\nCedar,paid,900\nCedar,open,750\nOrchid,open,30\nOrchid,open,0\n";
const OUTPUT: &str = "customer,open_count,total_cents\nCedar,2,2000\nOrchid,2,30\n";

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
fn read(id: &str, path: &str) -> ToolCall {
    call(
        id,
        "read_file",
        [("file_path", DataValue::String(path.into()))],
    )
}
fn write(id: &str, text: &str) -> ToolCall {
    call(
        id,
        "write_file",
        [
            ("file_path", DataValue::String("summary.csv".into())),
            ("content", DataValue::String(text.into())),
        ],
    )
}
fn tasks(id: &str, done: bool) -> ToolCall {
    call(
        id,
        "todo_write",
        [(
            "todos",
            DataValue::Array(
                [
                    "Reconcile open invoices",
                    "Read back and verify the saved summary",
                ]
                .into_iter()
                .map(|text| {
                    DataValue::Object(
                        [
                            ("content".into(), DataValue::String(text.into())),
                            (
                                "activeForm".into(),
                                DataValue::String(format!("Working: {text}")),
                            ),
                            (
                                "status".into(),
                                DataValue::String(
                                    if done { "completed" } else { "in_progress" }.into(),
                                ),
                            ),
                        ]
                        .into(),
                    )
                })
                .collect(),
            ),
        )],
    )
}

struct Fixture {
    _private: tempfile::TempDir,
    root: PathBuf,
    store: ControlPlaneStoreConfig,
    coordinator: AgentRunCoordinator,
    agent: String,
}
impl Fixture {
    fn new(model: Arc<dyn ModelAdapter>, enabled: bool, rounds: usize, live: bool) -> Self {
        let private = tempfile::tempdir().unwrap();
        let root = private.path().join("workspace");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("invoices.csv"), INPUT).unwrap();
        std::fs::write(root.join("summary.csv"), "incorrect draft\n").unwrap();
        let store = ControlPlaneStoreConfig::Json(private.path().join("control-plane.json"));
        let mut state = DaemonState::with_model_adapter(model);
        state.replace_memory_embeddings(crate::memory_embeddings::MemoryEmbeddingRuntime::disabled());
        state.workspace = Some(WorkspaceConfig {
            root_path: root.clone(),
            company_name: "Completion acceptance".into(),
            mission: "Reconcile an isolated invoice summary".into(),
            values: vec![],
        });
        state.set_control_plane_store(Some(store.clone()));
        let mut config = companion_config("Completion acceptance");
        if live {
            config.provider = Some("chatgpt".into());
            config.model = "gpt-6-luna".into();
        }
        config.tools = Some(
            state
                .tool_registry
                .resolve_descriptors([
                    "read_file",
                    "write_file",
                    "edit_file",
                    "todo_read",
                    "todo_write",
                    "calculate",
                ])
                .unwrap(),
        );
        let settings = config.settings.as_mut().unwrap();
        settings.max_tool_iterations = Some(rounds);
        settings.max_retries = Some(0);
        settings.max_tokens = Some(2048);
        settings.timeout_ms = Some(30_000);
        for key in ["autoTitle", "autoCompact", "agentFilesProfileEnabled"] {
            settings
                .additional
                .insert(key.into(), DataValue::Bool(false));
        }
        settings
            .additional
            .insert("completionReview".into(), DataValue::Bool(enabled));
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
    async fn run(&self, key: &str) -> super::AgentRunEnvelope {
        self.coordinator
            .run(chat_request(&self.agent, "chat:completion", key))
            .await
            .unwrap()
    }
    async fn assert_quiet_restore(&self) {
        let saved = load_control_plane_snapshot(&self.store)
            .await
            .unwrap()
            .unwrap();
        let before_runs = serde_json::to_value(&saved.runs).unwrap();
        let before_messages = saved
            .agents
            .iter()
            .find(|agent| agent.state.id == self.agent)
            .unwrap()
            .messages
            .clone();
        let tasks = crate::tools::todo::read_agent_todos(Some(&self.root), &self.agent).unwrap();
        assert_eq!(tasks.tasks.len(), 2);
        assert!(tasks.tasks.iter().all(|task| task.status == "completed"));
        let quiet_model = ScriptedModel::new(vec![]);
        let mut fresh = DaemonState::with_model_adapter(quiet_model.clone());
        fresh.replace_memory_embeddings(crate::memory_embeddings::MemoryEmbeddingRuntime::disabled());
        fresh.restore_control_plane_snapshot(saved).unwrap();
        let fresh =
            AgentRunCoordinator::new(Arc::new(RwLock::new(fresh)), Arc::new(Semaphore::new(1)));
        tokio::time::sleep(Duration::from_millis(100)).await;
        let guard = fresh.state.read().await;
        assert_eq!(
            serde_json::to_value(&guard.control_plane_snapshot().runs).unwrap(),
            before_runs
        );
        assert_eq!(guard.agents[&self.agent].messages(), before_messages);
        assert_task_evidence(guard.agents[&self.agent].messages());
        assert!(quiet_model.requests().is_empty());
        assert!(quiet_model.secondary_requests().is_empty());
        assert_eq!(
            std::fs::read_to_string(self.root.join("summary.csv")).unwrap(),
            OUTPUT
        );
        assert_eq!(
            std::fs::read_to_string(self.root.join("invoices.csv")).unwrap(),
            INPUT
        );
        assert_eq!(
            serde_json::to_value(
                crate::tools::todo::read_agent_todos(Some(&self.root), &self.agent).unwrap()
            )
            .unwrap(),
            serde_json::to_value(tasks).unwrap()
        );
        assert_paired(guard.agents[&self.agent].messages());
    }
}

fn assert_paired(messages: &[anima_core::Message]) {
    let mut pending = std::collections::BTreeSet::new();
    for message in messages {
        let metadata = message.content.metadata.as_ref();
        if message.role == MessageRole::Assistant {
            if let Some(DataValue::Array(calls)) = metadata.and_then(|data| data.get("toolCalls")) {
                for call in calls {
                    let DataValue::Object(call) = call else {
                        panic!("call object")
                    };
                    let DataValue::String(id) = &call["id"] else {
                        panic!("call id")
                    };
                    assert!(pending.insert(id.clone()));
                }
            }
        } else if message.role == MessageRole::Tool {
            let Some(DataValue::String(id)) = metadata.and_then(|data| data.get("toolCallId"))
            else {
                panic!("result id")
            };
            assert!(pending.remove(id));
        }
    }
    assert!(pending.is_empty());
}

/// Inspect actual settled results and causal model rounds independently of
/// the final prose. A same-round read/save cannot prove it saw a prior result.
fn assert_task_evidence(messages: &[anima_core::Message]) {
    let mut calls = std::collections::BTreeMap::new();
    let (mut input_read, mut last_write, mut read_back, mut cedar, mut orchid, mut complete) =
        (None, None, None, None, None, None);
    for (index, message) in messages.iter().enumerate() {
        let Some(metadata) = &message.content.metadata else {
            continue;
        };
        if message.role == MessageRole::Assistant {
            if let Some(DataValue::Array(tool_calls)) = metadata.get("toolCalls") {
                for value in tool_calls {
                    let DataValue::Object(fields) = value else {
                        panic!("call object")
                    };
                    let (DataValue::String(id), DataValue::String(name), DataValue::Object(args)) =
                        (&fields["id"], &fields["name"], &fields["args"])
                    else {
                        panic!("native call")
                    };
                    calls.insert(id.clone(), (index, name.clone(), args.clone()));
                }
            }
        } else if message.role == MessageRole::Tool {
            let Some(DataValue::Object(result)) = metadata.get("taskResult") else {
                continue;
            };
            if result.get("status") != Some(&DataValue::String("success".into())) {
                continue;
            }
            let DataValue::String(id) = &metadata["toolCallId"] else {
                panic!("native result")
            };
            let (issued, name, args) = &calls[id];
            let file = args.get("file_path").and_then(|value| match value {
                DataValue::String(path) => Some(path.trim_start_matches("./")),
                _ => None,
            });
            match name.as_str() {
                "read_file" if file == Some("invoices.csv") => {
                    input_read = Some(index);
                }
                "write_file" | "edit_file" if file == Some("summary.csv") => {
                    assert!(input_read.is_some_and(|read| read < *issued));
                    last_write = Some(index);
                    read_back = None;
                    complete = None;
                }
                "read_file"
                    if file == Some("summary.csv")
                        && last_write.is_some_and(|write| write < *issued) =>
                {
                    for row in OUTPUT.lines() {
                        assert!(
                            message.content.text.contains(row),
                            "read-back must contain the actual saved rows"
                        );
                    }
                    read_back = Some(index);
                }
                "calculate" if input_read.is_some_and(|read| read < *issued) => {
                    if message.content.text.trim() == "2000" {
                        cedar = Some(index);
                    }
                    if message.content.text.trim() == "30" {
                        orchid = Some(index);
                    }
                }
                "todo_write" => {
                    let Some(DataValue::Array(tasks)) = args.get("todos") else {
                        panic!("native task list")
                    };
                    let done = tasks.len() == 2 && tasks.iter().all(|value| matches!(value, DataValue::Object(fields) if fields.get("status") == Some(&DataValue::String("completed".into()))));
                    if done {
                        assert!(
                            read_back.is_some_and(|read| read < *issued),
                            "completion must follow observed read-back"
                        );
                        assert!(
                            cedar.is_some_and(|check| check < *issued)
                                && orchid.is_some_and(|check| check < *issued),
                            "completion must follow both arithmetic results"
                        );
                        complete = Some(index);
                    }
                }
                _ => {}
            }
        }
    }
    assert!(
        last_write.is_some() && read_back.is_some() && complete.is_some(),
        "all completion stages must have native evidence"
    );
}

#[tokio::test]
async fn completion_review_native_task_corrects_premature_finish_and_restores_completed_result() {
    let model = ScriptedModel::new(vec![
        Step::Tools(vec![read("read-input", "invoices.csv"), call("read-tasks", "todo_read", [])]),
        Step::Tools(vec![write("repair-summary", OUTPUT), tasks("pending", false)]),
        Step::Text(vec!["The summary is saved."]),
        Step::Tools(vec![read("read-output", "summary.csv"), call("check-total", "calculate", [("expression", DataValue::String("1250+750".into()))]), call("check-orchid", "calculate", [("expression", DataValue::String("30+0".into()))])]),
        Step::Tools(vec![tasks("done", true)]),
        Step::Text(vec!["Verified the saved summary: Cedar has 2 open invoices totaling 2000 cents; Orchid has 2 totaling 30 cents. Paid invoices are excluded."]),
    ]);
    let fixture = Fixture::new(model.clone(), true, 8, false);
    let result = fixture.run("reconcile invoices").await;
    assert_eq!(result.result.status, "success");
    assert_eq!(model.requests().len(), 6);
    assert!(model.requests()[3]
        .messages
        .iter()
        .any(|message| message.content.text.contains("Completion review:")));
    assert!(model.secondary_requests().is_empty());
    fixture.assert_quiet_restore().await;
}

#[tokio::test]
async fn completion_review_run_local_opt_in_and_budget_preserve_existing_behavior() {
    for (enabled, rounds, expected) in [(false, 8, 2), (true, 1, 2), (true, 8, 3)] {
        let model = ScriptedModel::new(vec![
            Step::Tools(vec![write("edit", OUTPUT)]),
            Step::Text(vec!["Saved; read-back remains unverified."]),
            Step::Text(vec![
                "Read-back remains unverified; no further permission is available.",
            ]),
        ]);
        let fixture = Fixture::new(model.clone(), enabled, rounds, false);
        assert_eq!(fixture.run("save summary").await.result.status, "success");
        assert_eq!(model.requests().len(), expected);
    }
    let model = ScriptedModel::new(vec![
        Step::Tools(vec![write("edit-1", OUTPUT)]),
        Step::Text(vec!["saved"]),
        Step::Tools(vec![read("read-1", "summary.csv")]),
        Step::Text(vec!["read back"]),
        Step::Tools(vec![write("edit-2", OUTPUT)]),
        Step::Text(vec!["saved"]),
        Step::Tools(vec![read("read-2", "summary.csv")]),
        Step::Text(vec!["read back"]),
    ]);
    let fixture = Fixture::new(model.clone(), true, 8, false);
    assert_eq!(fixture.run("first").await.result.status, "success");
    assert_eq!(fixture.run("second").await.result.status, "success");
    assert_eq!(model.requests().len(), 8, "each run owns a fresh review");
}

#[tokio::test]
#[ignore = "explicit opt-in: one bounded real subscription task in an isolated file-only workspace"]
async fn completion_review_live_invoice_reconciliation_and_quiet_restart() {
    let adapter = Arc::new(crate::runtime_model::RuntimeModelAdapter::from_env(
        crate::chatgpt_auth::ChatGptAuth::new(),
    ));
    let fixture = Fixture::new(adapter, true, 12, true);
    let input = "Reconcile invoices.csv against the incorrect summary.csv. Create exactly two saved todos: Reconcile open invoices; Read back and verify the saved summary. Read the input and existing report, calculate each customer's open-invoice total separately with calculate, and repair summary.csv. Group only open invoices by customer, count zero-cent rows, exclude paid rows, sort customer names, and sum integer cents. CSV header is customer,open_count,total_cents; use LF newlines and a trailing newline. Read back the saved summary and compare it with the source and arithmetic before marking both todos completed. End with a fenced CSV block copied exactly from the verified saved summary, then one short sentence describing limitations. Preserve invoices.csv; only summary.csv and your native todo list may change. Use native tools only; no delegation, network, shell or further task creation.";
    let outcome = tokio::time::timeout(
        Duration::from_secs(180),
        fixture
            .coordinator
            .run(chat_request(&fixture.agent, "chat:completion", input)),
    )
    .await
    .expect("bounded live task")
    .unwrap();
    // Retain the reply even if any later acceptance assertion fails. Case v1
    // failed a literal prose check after all file/progress/reload checks passed;
    // its unretained reply cannot be diagnosed or retroactively accepted.
    println!(
        "COMPLETION_LIVE_RESULT {}",
        serde_json::json!({"caseVersion":2,"status":outcome.result.status,"error":outcome.result.error,"reply":outcome.result.data.as_ref().map(|data| &data.text)})
    );
    assert_eq!(
        outcome.result.status, "success",
        "{:?}",
        outcome.result.error
    );
    fixture.assert_quiet_restore().await;
    let guard = fixture.coordinator.state.read().await;
    let messages = guard.agents[&fixture.agent].messages();
    assert!(
        messages
            .iter()
            .filter(|message| message.role == MessageRole::Tool)
            .count()
            >= 6
    );
    let runs = guard.runs.for_agent(&fixture.agent);
    assert_eq!(runs.len(), 1);
    assert!(
        runs[0].steps.len() <= 14,
        "bounded tools, one review and one wrap-up"
    );
    let reply = &outcome.result.data.as_ref().unwrap().text;
    assert!(
        exact_csv_report(reply),
        "the declared final CSV block must exactly match the independently checked saved output"
    );
    println!(
        "COMPLETION_LIVE_ACCEPTED {}",
        serde_json::json!({"model":"gpt-6-luna","steps":runs[0].steps.len(),"reportedTokens":guard.agents[&fixture.agent].state().token_usage.total_tokens,"toolReceipts":runs[0].action_receipts.len(),"outputBytes":OUTPUT.len(),"completedTodos":2,"restartCalls":0,"finalReply":reply})
    );
}

fn exact_csv_report(reply: &str) -> bool {
    let mut blocks = reply.match_indices("```csv\n");
    let Some((start, marker)) = blocks.next() else {
        return false;
    };
    if blocks.next().is_some() {
        return false;
    }
    let Some((body, _)) = reply[start + marker.len()..].split_once("```") else {
        return false;
    };
    body == OUTPUT
}

#[test]
fn completion_review_report_oracle_rejects_extra_changed_and_unfenced_rows() {
    let report = format!("```csv\n{OUTPUT}```\nSynthetic source only.");
    assert!(exact_csv_report(&report));
    assert!(!exact_csv_report(
        &report.replace("Orchid,2,30", "Orchid,2,300")
    ));
    assert!(!exact_csv_report(&format!(
        "```csv\n{OUTPUT}Other,1,99\n```"
    )));
    assert!(!exact_csv_report(&format!("{report}\n{report}")));
    assert!(!exact_csv_report(OUTPUT));
}
