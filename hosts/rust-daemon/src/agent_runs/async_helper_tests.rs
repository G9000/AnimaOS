//! Deterministic FP-05 admission tests: actual native tool dispatch, saved
//! ledger handles and bounded recording models; no provider or network calls.
use std::collections::{BTreeMap, HashSet};
use std::sync::{atomic::Ordering, Arc};
use std::time::Duration;

use anima_core::{AgentState, Content, DataValue, Message, MessageRole, ToolCall, ToolDescriptor};
use serde_json::Value;
use tokio::sync::{RwLock, Semaphore};

use super::test_support::{add_chat, lead_config, wait_for, ScriptedModel, Step};
use super::AgentRunCoordinator;
use crate::runs::{RunLink, RunRecord, RunSource, RunStart, RunStatus};
use crate::state::DaemonState;
use crate::tools::{ToolExecutionContext, ToolRegistry};

#[path = "async_helper_origin_authority_tests.rs"]
mod origin_authority;

const TOOLS: [&str; 4] = [
    "spawn_helper_async",
    "list_helper_runs",
    "steer_helper",
    "stop_helper",
];

#[path = "async_helper_coding_tests.rs"]
mod coding;
#[path = "helper_collection_tests.rs"]
mod collection;
#[path = "async_helper_completion_tests.rs"]
mod completion;
#[path = "helper_options_tests.rs"]
mod options;
#[path = "self_clone_async_tests.rs"]
mod self_clone;

struct Harness {
    coordinator: AgentRunCoordinator,
    parent: AgentState,
    context: ToolExecutionContext,
    model: Arc<ScriptedModel>,
    permits: Arc<Semaphore>,
}

// These fixtures represent already-admitted owner turns. Use the same budget
// transaction as production rather than inventing allowance when a child asks.
async fn admit_fixture_run(coordinator: &AgentRunCoordinator, run: &RunRecord) {
    let _transaction = coordinator.control_plane_transaction().await;
    let (candidate, persist) = {
        let mut guard = coordinator.state.write().await;
        let candidate = crate::execution_budgets::prepare_run_binding(
            &guard,
            run,
            anima_core::primitives::now_millis(),
        )
        .unwrap();
        guard.execution_budget_candidate_snapshot = Some(candidate.registry.clone());
        guard.runs.insert(run.clone());
        (candidate, guard.control_plane_persist_request())
    };
    crate::execution_budgets::save_admission(&coordinator.state, candidate, persist)
        .await
        .unwrap();
}

impl Harness {
    async fn new(capacity: usize) -> Self {
        let model = ScriptedModel::new(vec![Step::Text(vec!["recorded child result"])]);
        Self::with_model(capacity, model).await
    }

    async fn with_model(capacity: usize, model: Arc<ScriptedModel>) -> Self {
        let state = Arc::new(RwLock::new(DaemonState::with_model_adapter(model.clone())));
        let permits = Arc::new(Semaphore::new(capacity));
        let coordinator = AgentRunCoordinator::new(state.clone(), permits.clone());
        let mut parent = state
            .write()
            .await
            .create_agent(lead_config("Companion"))
            .unwrap()
            .state;
        // Explicit native grants in an authenticated host context. Keeping the
        // fixture shape independent of registry lookup lets RED reach the real
        // dispatch boundary instead of failing to compile on a missing method.
        parent
            .config
            .tools
            .as_mut()
            .unwrap()
            .extend(TOOLS.map(|name| ToolDescriptor {
                name: name.into(),
                description: "FP-05 explicitly granted test operation".into(),
                parameters_schema: BTreeMap::new(),
                examples: None,
            }));
        state
            .write()
            .await
            .restore_agent_config(&parent.id, parent.config.clone());
        add_chat(&coordinator, &parent.id, "chat:owner").await;
        let run = RunRecord::running(
            RunStart {
                agent_id: parent.id.clone(),
                session_id: "chat:owner".into(),
                source: RunSource::Web,
                source_ref: None,
                idempotency_key: None,
                text: "Research this task".into(),
                model: parent.config.model.clone(),
                provider: parent.config.provider.clone(),
                parent_run_id: None,
            },
            1,
        );
        admit_fixture_run(&coordinator, &run).await;
        let context = {
            let mut guard = state.write().await;
            guard.live.runs().register(&run.id);
            guard
                .tool_execution_context()
                .with_team(coordinator.clone(), true)
                .with_run_link(Some(RunLink {
                    run_id: run.id,
                    session_id: run.session_id,
                    agent_id: parent.id.clone(),
                }))
        };
        Self {
            coordinator,
            parent,
            context,
            model,
            permits,
        }
    }

    async fn call(&self, call: ToolCall) -> anima_core::TaskResult<Content> {
        self.context
            .clone()
            .execute_tool(self.parent.clone(), input(&self.parent.id), call)
            .await
    }

    async fn launch(&self, id: &str, task: &str) -> Value {
        let result = tokio::time::timeout(Duration::from_secs(2), self.call(launch(id, task)))
            .await
            .expect("durable acceptance must not wait for execution capacity");
        assert_eq!(result.status, anima_core::TaskStatus::Success, "{result:?}");
        serde_json::from_str(&result.data.unwrap().text).expect("native handle is structured JSON")
    }
}

fn input(agent: &str) -> Message {
    Message {
        id: "owner-input".into(),
        agent_id: agent.into(),
        room_id: "chat:owner".into(),
        role: MessageRole::User,
        content: Content {
            text: "Research this task".into(),
            ..Content::default()
        },
        created_at_ms: 1,
    }
}

fn launch(id: &str, task: &str) -> ToolCall {
    ToolCall {
        id: id.into(),
        name: "spawn_helper_async".into(),
        args: BTreeMap::from([
            ("name".into(), DataValue::String("Research helper".into())),
            ("task".into(), DataValue::String(task.into())),
            ("completion".into(), DataValue::String("manual".into())),
        ]),
    }
}

#[test]
fn async_helper_admission_tools_are_separately_registered() {
    let registry = ToolRegistry::new();
    for name in TOOLS {
        assert!(
            registry.descriptor(name).is_some(),
            "missing separately granted native tool {name}"
        );
    }
}

#[tokio::test]
async fn async_helper_admission_returns_saved_queued_handle_without_global_capacity() {
    let harness = Harness::new(0).await;
    let handle = harness.launch("dispatch-1", "Find evidence").await;
    assert_eq!(handle["status"], "queued");
    assert!(
        handle.get("result").is_none(),
        "acceptance is not terminal output"
    );
    let guard = harness.coordinator.state.read().await;
    let run = guard.runs.get(handle["runId"].as_str().unwrap()).unwrap();
    assert_eq!(run.status, RunStatus::Queued);
    assert!(run.async_helper.is_some());
    assert!(run.started_at_ms.is_none());
    assert_eq!(run.source, RunSource::Delegation);
    let session = guard.sessions.get(&run.agent_id, &run.session_id).unwrap();
    assert_eq!(session.kind, crate::sessions::SessionKind::Helper);
    assert!(!session.kind.capabilities(false).send);
    assert!(!session.kind.capabilities(false).steer);
    assert!(harness.model.requests().is_empty());
    drop(guard);
    harness.coordinator.request_shutdown();
}

#[tokio::test]
async fn async_helper_admission_exact_retry_is_free_and_changed_payload_conflicts() {
    let harness = Harness::new(0).await;
    let first = harness.launch("dispatch-retry", "Find evidence").await;
    let second = harness.launch("dispatch-retry", "Find evidence").await;
    assert_eq!(first["runId"], second["runId"]);
    assert_eq!(harness.context.helper_starts.load(Ordering::Acquire), 1);
    let changed = harness
        .call(launch("dispatch-retry", "Different task"))
        .await;
    assert_eq!(changed.status, anima_core::TaskStatus::Error);
    assert!(changed.error.unwrap().contains("different"));
    assert_eq!(harness.context.helper_starts.load(Ordering::Acquire), 1);
    harness.coordinator.request_shutdown();
}

#[tokio::test]
async fn async_helper_admission_four_queued_assignments_reserve_four_distinct_helpers() {
    let harness = Harness::new(0).await;
    let mut agents = HashSet::new();
    for index in 0..4 {
        let handle = harness
            .launch(&format!("dispatch-{index}"), "Bounded task")
            .await;
        assert!(agents.insert(handle["agentId"].as_str().unwrap().to_string()));
    }
    let refused = harness.call(launch("dispatch-fifth", "Fifth task")).await;
    assert_eq!(refused.status, anima_core::TaskStatus::Error);
    assert_eq!(harness.context.helper_starts.load(Ordering::Acquire), 4);
    let mut sync = launch("sync-fifth", "Synchronous attempt shares the budget");
    sync.name = "spawn_helper".into();
    let mut caller = harness.parent.clone();
    caller
        .config
        .tools
        .as_mut()
        .unwrap()
        .push(ToolRegistry::new().descriptor("spawn_helper").unwrap());
    let refused = harness
        .context
        .clone()
        .execute_tool(caller, input(&harness.parent.id), sync)
        .await;
    assert!(refused
        .error
        .unwrap()
        .contains("configured helper start limit"));
    assert!(harness.model.requests().is_empty());
    harness.coordinator.request_shutdown();
}

#[tokio::test]
async fn async_helper_admission_requires_authentic_active_parent_run() {
    let harness = Harness::new(0).await;
    let context = harness.context.clone().with_run_link(None);
    let refused = context
        .execute_tool(
            harness.parent.clone(),
            input(&harness.parent.id),
            launch("forged", "Do not run"),
        )
        .await;
    assert_eq!(refused.status, anima_core::TaskStatus::Error);
    let guard = harness.coordinator.state.read().await;
    assert_eq!(guard.agents.len(), 1);
    assert_eq!(guard.runs.active_records().len(), 1);
    assert!(harness.model.requests().is_empty());
}

#[tokio::test]
async fn async_helper_admission_save_failure_rolls_back_helper_session_and_run() {
    let harness = Harness::new(0).await;
    let path =
        std::env::temp_dir().join(format!("anima-async-helper-save-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&path).unwrap();
    harness
        .coordinator
        .state
        .write()
        .await
        .set_control_plane_store(Some(
            crate::control_plane_store::ControlPlaneStoreConfig::Json(path.clone()),
        ));
    let refused = harness
        .call(launch("save-failure", "Do not leak records"))
        .await;
    assert_eq!(refused.status, anima_core::TaskStatus::Error);
    let guard = harness.coordinator.state.read().await;
    assert_eq!(guard.agents.len(), 1);
    assert_eq!(guard.runs.active_records().len(), 1);
    assert_eq!(
        guard
            .sessions
            .snapshot_records(&HashSet::from([harness.parent.id.clone()]))
            .len(),
        1
    );
    assert_eq!(
        harness.context.helper_starts.load(Ordering::Acquire),
        1,
        "failed non-replay attempt remains charged"
    );
    assert!(harness.model.requests().is_empty());
    drop(guard);
    std::fs::remove_dir(path).unwrap();
}

#[tokio::test]
async fn async_helper_admission_start_rechecks_revoked_parent_grant() {
    let harness = Harness::new(0).await;
    let handle = harness
        .launch("revoke-before-start", "Do not execute revoked work")
        .await;
    let mut config = harness.parent.config.clone();
    config
        .tools
        .as_mut()
        .unwrap()
        .retain(|tool| tool.name != "spawn_helper_async");
    harness
        .coordinator
        .state
        .write()
        .await
        .restore_agent_config(&harness.parent.id, config);
    harness.permits.add_permits(1);
    wait_for(
        &harness.coordinator,
        handle["runId"].as_str().unwrap(),
        RunStatus::Failed,
    )
    .await;
    assert!(harness.model.requests().is_empty());
}

#[tokio::test]
async fn async_helper_admission_reuse_keeps_immutable_run_and_session_handles() {
    let harness = Harness::new(1).await;
    let first = harness.launch("reuse-first", "First task").await;
    wait_for(
        &harness.coordinator,
        first["runId"].as_str().unwrap(),
        RunStatus::Completed,
    )
    .await;
    let second = harness.launch("reuse-second", "Second task").await;
    wait_for(
        &harness.coordinator,
        second["runId"].as_str().unwrap(),
        RunStatus::Completed,
    )
    .await;
    assert_eq!(first["agentId"], second["agentId"]);
    assert_ne!(first["runId"], second["runId"]);
    assert_ne!(first["sessionId"], second["sessionId"]);
    assert_eq!(harness.model.requests().len(), 2);
}

fn management(id: &str, name: &str, run: Option<&str>, message: Option<&str>) -> ToolCall {
    let mut args = BTreeMap::new();
    if let Some(run) = run {
        args.insert("run_id".into(), DataValue::String(run.into()));
    }
    if let Some(message) = message {
        args.insert("message".into(), DataValue::String(message.into()));
    }
    ToolCall {
        id: id.into(),
        name: name.into(),
        args,
    }
}

fn success_json(result: anima_core::TaskResult<Content>) -> Value {
    assert_eq!(result.status, anima_core::TaskStatus::Success, "{result:?}");
    serde_json::from_str(&result.data.unwrap().text).unwrap()
}

async fn drained(harness: &Harness, handle: &Value) {
    for _ in 0..500 {
        if !harness.coordinator.has_session_queue(
            handle["agentId"].as_str().unwrap(),
            handle["sessionId"].as_str().unwrap(),
        ) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the helper session queue must finish cleanup within five seconds");
}

async fn foreign_actor(harness: &Harness) -> (AgentState, ToolExecutionContext) {
    let mut config = harness.parent.config.clone();
    config.name = "Other companion".into();
    let agent = harness
        .coordinator
        .state
        .write()
        .await
        .create_agent(config)
        .unwrap()
        .state;
    add_chat(&harness.coordinator, &agent.id, "chat:owner").await;
    let mut run = {
        let guard = harness.coordinator.state.read().await;
        guard
            .runs
            .get(&harness.context.run_link.as_ref().unwrap().run_id)
            .unwrap()
            .clone()
    };
    run.id = format!("run_{}", uuid::Uuid::new_v4());
    run.agent_id = agent.id.clone();
    admit_fixture_run(&harness.coordinator, &run).await;
    let context = {
        let mut guard = harness.coordinator.state.write().await;
        guard.live.runs().register(&run.id);
        guard
            .tool_execution_context()
            .with_team(harness.coordinator.clone(), true)
            .with_run_link(Some(RunLink {
                run_id: run.id,
                agent_id: agent.id.clone(),
                session_id: run.session_id,
            }))
    };
    (agent, context)
}

#[tokio::test]
async fn async_helper_management_list_is_owned_and_exact_unknown_matches_foreign() {
    let harness = Harness::new(0).await;
    let handle = harness.launch("owned-child", "Owned task").await;
    let id = handle["runId"].as_str().unwrap();
    let listed = success_json(
        harness
            .call(management("list-own", "list_helper_runs", None, None))
            .await,
    );
    assert_eq!(listed["runs"].as_array().unwrap().len(), 1);
    assert_eq!(listed["runs"][0]["runId"], id);
    let (foreign, context) = foreign_actor(&harness).await;
    let mut failures = Vec::new();
    for target in [id, "run_unknown"] {
        let result = context
            .clone()
            .execute_tool(
                foreign.clone(),
                input(&foreign.id),
                management("foreign-list", "list_helper_runs", Some(target), None),
            )
            .await;
        assert_eq!(result.status, anima_core::TaskStatus::Error);
        failures.push(result.error.unwrap());
    }
    assert_eq!(failures[0], failures[1]);
    harness.coordinator.request_shutdown();
}

#[tokio::test]
async fn async_helper_management_list_bounds_committed_results_and_links() {
    let text: &'static str = Box::leak("x".repeat(10_000).into_boxed_str());
    let model = ScriptedModel::new(vec![Step::Text(vec![text]); 4]);
    let harness = Harness::with_model(4, model).await;
    for index in 0..4 {
        let handle = harness
            .launch(&format!("large-{index}"), "Large bounded result")
            .await;
        wait_for(
            &harness.coordinator,
            handle["runId"].as_str().unwrap(),
            RunStatus::Completed,
        )
        .await;
        drained(&harness, &handle).await;
    }
    let result = harness
        .call(management("bounded-list", "list_helper_runs", None, None))
        .await;
    let text = result
        .data
        .as_ref()
        .map(|data| data.text.clone())
        .unwrap_or_default();
    let listed = success_json(result);
    assert!(text.len() <= 32 * 1024);
    assert!(!listed["runs"].as_array().unwrap().is_empty());
    for entry in listed["runs"].as_array().unwrap() {
        assert!(entry["resultText"].as_str().unwrap().len() <= 8 * 1024);
        assert_eq!(entry["resultTruncated"], true);
        assert!(entry["runId"].as_str().is_some());
        assert!(entry["sessionId"].as_str().is_some());
        assert!(entry["replyMessageId"].as_str().is_some());
    }
}

#[tokio::test]
async fn async_helper_management_steer_is_saved_once_then_archived_on_original_child() {
    let gate = super::test_support::Gate::new();
    let model = ScriptedModel::gated(vec![Step::Text(vec!["Child result"])], gate.clone());
    let harness = Harness::with_model(1, model).await;
    let handle = harness.launch("steered-child", "Read evidence").await;
    let id = handle["runId"].as_str().unwrap();
    gate.entered().await;
    let first = success_json(
        harness
            .call(management(
                "steer-once",
                "steer_helper",
                Some(id),
                Some("Additional evidence"),
            ))
            .await,
    );
    let second = success_json(
        harness
            .call(management(
                "steer-once",
                "steer_helper",
                Some(id),
                Some("Additional evidence"),
            ))
            .await,
    );
    assert_eq!(first["acceptanceKey"], second["acceptanceKey"]);
    let changed = harness
        .call(management(
            "steer-once",
            "steer_helper",
            Some(id),
            Some("Different evidence"),
        ))
        .await;
    assert_eq!(changed.status, anima_core::TaskStatus::Error);
    {
        let guard = harness.coordinator.state.read().await;
        assert_eq!(guard.runs.get(id).unwrap().pending_steers.len(), 1);
        assert_eq!(
            guard
                .live
                .runs()
                .control(id)
                .unwrap()
                .steering
                .pending()
                .len(),
            1
        );
    }
    gate.release();
    wait_for(&harness.coordinator, id, RunStatus::Completed).await;
    let guard = harness.coordinator.state.read().await;
    let record = guard.runs.get(id).unwrap();
    assert!(record.pending_steers.is_empty());
    assert_eq!(
        record
            .async_helper
            .as_ref()
            .unwrap()
            .unconsumed_steers
            .len(),
        1
    );
    assert_eq!(
        guard.runs.for_agent(&record.agent_id).len(),
        1,
        "unread steer must not create another owner-like Web run"
    );
}

#[tokio::test]
async fn async_helper_management_eight_lifetime_steers_include_already_read_inputs() {
    let gate = super::test_support::Gate::new();
    let calculate = ToolCall {
        id: "helper-calculate".into(),
        name: "calculate".into(),
        args: BTreeMap::from([("expression".into(), DataValue::String("1+1".into()))]),
    };
    let model = ScriptedModel::gated(
        vec![Step::Tools(vec![calculate]), Step::Text(vec!["Finished"])],
        gate.clone(),
    );
    let harness = Harness::with_model(1, model).await;
    let handle = harness.launch("bounded-steers", "Bounded task").await;
    let id = handle["runId"].as_str().unwrap();
    gate.entered().await;
    success_json(
        harness
            .call(management(
                "steer-0",
                "steer_helper",
                Some(id),
                Some("Evidence 0"),
            ))
            .await,
    );
    gate.release();
    gate.entered().await;
    for index in 1..8 {
        success_json(
            harness
                .call(management(
                    &format!("steer-{index}"),
                    "steer_helper",
                    Some(id),
                    Some(&format!("Evidence {index}")),
                ))
                .await,
        );
    }
    let refused = harness
        .call(management(
            "steer-8",
            "steer_helper",
            Some(id),
            Some("Ninth input"),
        ))
        .await;
    assert_eq!(refused.status, anima_core::TaskStatus::Error);
    assert!(refused.error.unwrap().contains("eight"));
    gate.release();
    wait_for(&harness.coordinator, id, RunStatus::Completed).await;
    let guard = harness.coordinator.state.read().await;
    let child = guard.runs.get(id).unwrap();
    assert_eq!(
        child.steered_keys.len() + child.async_helper.as_ref().unwrap().unconsumed_steers.len(),
        8
    );
    assert_eq!(harness.model.requests().len(), 2);
}

#[tokio::test]
async fn async_helper_management_steer_save_failure_delivers_nothing() {
    let gate = super::test_support::Gate::new();
    let harness = Harness::with_model(1, ScriptedModel::gated(vec![], gate.clone())).await;
    let handle = harness.launch("failed-steer", "Bounded task").await;
    let id = handle["runId"].as_str().unwrap();
    gate.entered().await;
    let path = std::env::temp_dir().join(format!("anima-steer-failure-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&path).unwrap();
    harness
        .coordinator
        .state
        .write()
        .await
        .set_control_plane_store(Some(
            crate::control_plane_store::ControlPlaneStoreConfig::Json(path.clone()),
        ));
    let failed = harness
        .call(management(
            "not-delivered",
            "steer_helper",
            Some(id),
            Some("Unsaved input"),
        ))
        .await;
    assert_eq!(failed.status, anima_core::TaskStatus::Error);
    assert!(failed.error.unwrap().contains("persist"));
    {
        let guard = harness.coordinator.state.read().await;
        assert!(guard.runs.get(id).unwrap().pending_steers.is_empty());
        assert!(guard
            .live
            .runs()
            .control(id)
            .unwrap()
            .steering
            .pending()
            .is_empty());
    }
    harness
        .coordinator
        .state
        .write()
        .await
        .set_control_plane_store(None);
    gate.release();
    wait_for(&harness.coordinator, id, RunStatus::Completed).await;
    std::fs::remove_dir(path).unwrap();
}

#[tokio::test]
async fn async_helper_management_queued_terminal_and_revoked_steers_never_create_work() {
    let harness = Harness::new(0).await;
    let handle = harness.launch("refused-steer", "Bounded task").await;
    let id = handle["runId"].as_str().unwrap();
    let result = harness
        .call(management(
            "queued-steer",
            "steer_helper",
            Some(id),
            Some("Too early"),
        ))
        .await;
    assert_eq!(result.status, anima_core::TaskStatus::Error);
    assert_eq!(
        harness
            .coordinator
            .state
            .read()
            .await
            .runs
            .active_records()
            .len(),
        2
    );
    harness.permits.add_permits(1);
    wait_for(&harness.coordinator, id, RunStatus::Completed).await;
    let result = harness
        .call(management(
            "terminal-steer",
            "steer_helper",
            Some(id),
            Some("Too late"),
        ))
        .await;
    assert_eq!(result.status, anima_core::TaskStatus::Error);
    let mut config = harness.parent.config.clone();
    config
        .tools
        .as_mut()
        .unwrap()
        .retain(|tool| tool.name != "list_helper_runs");
    harness
        .coordinator
        .state
        .write()
        .await
        .restore_agent_config(&harness.parent.id, config);
    let result = harness
        .call(management(
            "revoked-list",
            "list_helper_runs",
            Some(id),
            None,
        ))
        .await;
    assert_eq!(result.status, anima_core::TaskStatus::Error);
    assert!(result.error.unwrap().contains("grant"));
    assert_eq!(harness.model.requests().len(), 1);
}

async fn attach_queued_completion(harness: &Harness, child_id: &str) -> String {
    let _transaction = harness.coordinator.control_plane_transaction().await;
    let mut guard = harness.coordinator.state.write().await;
    let parent = harness.context.run_link.as_ref().unwrap();
    let mut completion = RunRecord::queued(
        RunStart {
            agent_id: harness.parent.id.clone(),
            session_id: parent.session_id.clone(),
            source: RunSource::Delegation,
            source_ref: None,
            idempotency_key: None,
            text: "Bounded helper evidence".into(),
            model: "recording".into(),
            provider: None,
            parent_run_id: Some(parent.run_id.clone()),
        },
        2,
    );
    completion.helper_completion = Some(crate::runs::HelperCompletion {
        helper_run_id: child_id.into(),
        launch_parent_run_id: parent.run_id.clone(),
    });
    let candidate = crate::execution_budgets::prepare_run_binding(
        &guard,
        &completion,
        anima_core::primitives::now_millis(),
    )
    .unwrap();
    guard.execution_budget_candidate_snapshot = Some(candidate.registry.clone());
    let id = completion.id.clone();
    guard.live.runs().register(&id);
    guard.runs.insert(completion);
    guard
        .runs
        .get_mut(child_id)
        .unwrap()
        .async_helper
        .as_mut()
        .unwrap()
        .completion = crate::runs::HelperCompletionState::Queued { run_id: id.clone() };
    let persist = guard.control_plane_persist_request();
    drop(guard);
    crate::execution_budgets::save_admission(&harness.coordinator.state, candidate, persist)
        .await
        .unwrap();
    id
}

#[tokio::test]
async fn async_helper_management_completed_child_stop_suppresses_and_stops_completion() {
    let harness = Harness::new(1).await;
    let handle = harness.launch("stop-completed", "Bounded task").await;
    let id = handle["runId"].as_str().unwrap();
    wait_for(&harness.coordinator, id, RunStatus::Completed).await;
    let completion = attach_queued_completion(&harness, id).await;
    success_json(
        harness
            .call(management("stop-child", "stop_helper", Some(id), None))
            .await,
    );
    success_json(
        harness
            .call(management(
                "stop-child-again",
                "stop_helper",
                Some(id),
                None,
            ))
            .await,
    );
    let guard = harness.coordinator.state.read().await;
    let child = guard.runs.get(id).unwrap();
    assert!(child.stop.is_some());
    assert!(matches!(
        child.async_helper.as_ref().unwrap().completion,
        crate::runs::HelperCompletionState::Suppressed { .. }
    ));
    assert_eq!(
        guard.runs.get(&completion).unwrap().status,
        RunStatus::Cancelled
    );
    assert_eq!(harness.model.requests().len(), 1);
}

#[tokio::test]
async fn async_helper_management_stop_save_failure_restores_metadata_and_sends_no_signal() {
    let harness = Harness::new(1).await;
    let handle = harness.launch("stop-save-failure", "Bounded task").await;
    let id = handle["runId"].as_str().unwrap();
    wait_for(&harness.coordinator, id, RunStatus::Completed).await;
    let completion = attach_queued_completion(&harness, id).await;
    let control = harness
        .coordinator
        .state
        .read()
        .await
        .live
        .runs()
        .control(&completion)
        .unwrap();
    let path = std::env::temp_dir().join(format!("anima-stop-failure-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&path).unwrap();
    harness
        .coordinator
        .state
        .write()
        .await
        .set_control_plane_store(Some(
            crate::control_plane_store::ControlPlaneStoreConfig::Json(path.clone()),
        ));
    let failed = harness
        .call(management("failed-stop", "stop_helper", Some(id), None))
        .await;
    assert_eq!(failed.status, anima_core::TaskStatus::Error);
    assert!(failed.error.unwrap().contains("persist"));
    let guard = harness.coordinator.state.read().await;
    let child = guard.runs.get(id).unwrap();
    assert!(child.stop.is_none());
    assert!(matches!(
        child.async_helper.as_ref().unwrap().completion,
        crate::runs::HelperCompletionState::Queued { .. }
    ));
    assert_eq!(
        guard.runs.get(&completion).unwrap().status,
        RunStatus::Queued
    );
    assert!(!control.cancel.is_cancelled());
    drop(guard);
    std::fs::remove_dir(path).unwrap();
}

#[tokio::test]
async fn async_helper_management_running_stop_is_saved_before_cancellation() {
    let gate = super::test_support::Gate::new();
    let harness = Harness::with_model(1, ScriptedModel::gated(vec![], gate.clone())).await;
    let handle = harness.launch("stop-running", "Bounded task").await;
    let id = handle["runId"].as_str().unwrap();
    gate.entered().await;
    success_json(
        harness
            .call(management(
                "stop-running-call",
                "stop_helper",
                Some(id),
                None,
            ))
            .await,
    );
    wait_for(&harness.coordinator, id, RunStatus::Cancelled).await;
    assert!(harness
        .coordinator
        .state
        .read()
        .await
        .runs
        .get(id)
        .unwrap()
        .stop
        .is_some());
    gate.release();
}

#[tokio::test]
async fn async_helper_management_effective_bounds_survive_helper_reuse() {
    let harness = Harness::new(1).await;
    let first = harness.launch("bounds-first", "First task").await;
    let id = first["runId"].as_str().unwrap();
    wait_for(&harness.coordinator, id, RunStatus::Completed).await;
    drained(&harness, &first).await;
    let mut config = harness.parent.config.clone();
    config.settings.as_mut().unwrap().max_tokens = Some(100);
    harness
        .coordinator
        .state
        .write()
        .await
        .restore_agent_config(&harness.parent.id, config);
    let second = harness.launch("bounds-second", "Second task").await;
    assert_eq!(first["agentId"], second["agentId"]);
    let replay = harness.launch("bounds-first", "First task").await;
    assert_eq!(
        replay["bounds"]["maxOutputTokensPerResponse"],
        first["bounds"]["maxOutputTokensPerResponse"]
    );
    assert_eq!(replay["bounds"]["maxOutputTokensPerResponse"], 4096);
    assert_eq!(second["bounds"]["maxOutputTokensPerResponse"], 100);
}

#[tokio::test]
async fn async_helper_management_start_intersects_stricter_current_bounds() {
    let harness = Harness::new(0).await;
    let handle = harness.launch("bounds-at-start", "Bounded task").await;
    let mut config = harness.parent.config.clone();
    let settings = config.settings.as_mut().unwrap();
    settings.max_tokens = Some(77);
    settings.max_tool_iterations = Some(3);
    settings.timeout_ms = Some(30_000);
    harness
        .coordinator
        .state
        .write()
        .await
        .restore_agent_config(&harness.parent.id, config);
    harness.permits.add_permits(1);
    wait_for(
        &harness.coordinator,
        handle["runId"].as_str().unwrap(),
        RunStatus::Completed,
    )
    .await;
    let configs = harness.model.configs();
    let settings = configs[0].settings.as_ref().unwrap();
    assert_eq!(settings.max_tokens, Some(77));
    assert_eq!(settings.max_tool_iterations, Some(3));
    assert_eq!(settings.timeout_ms, Some(30_000));
    let guard = harness.coordinator.state.read().await;
    let bounds = guard
        .runs
        .get(handle["runId"].as_str().unwrap())
        .unwrap()
        .async_helper
        .as_ref()
        .unwrap()
        .effective_bounds
        .as_ref()
        .expect("actual effective bounds are durable");
    assert_eq!(bounds.max_output_tokens_per_response, 77);
}
