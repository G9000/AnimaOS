//! End-to-end executor witnesses for owner-reviewed MCP tools. These use a
//! fake transport and exercise the ordinary approval and receipt boundary.

use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Mutex as StdMutex,
};

use anima_core::{
    prepare_external_tool, AgentState, CancelSignal, DataValue, ExternalToolResult, TaskStatus,
    ToolCall,
};
use futures::future::BoxFuture;
use serde_json::{json, Value};
use tokio::sync::{Mutex, RwLock, Semaphore};

use crate::agent_runs::action_receipts::RunActionJournal;
use crate::agent_runs::test_support::{
    decision, gated_context, ledger_run, pending_approvals, tool_input,
};
use crate::agent_runs::AgentRunCoordinator;
use crate::approvals::{ApprovalDecisionKind, ApprovalPolicy, PolicyAction, RiskClass};
use crate::control_plane_store::ControlPlaneStoreConfig;
use crate::mcp::transport::{
    McpCall, McpDiscovery, McpTransportError, McpTransportFactory, McpTransportSession,
    PhysicalClose, TransportControl, ValidatedMcpEndpoint,
};
use crate::mcp::{CreateMcpConnection, McpManager, ReviewMcpTool, UpdateMcpConnection};
use crate::runs::{ActionStatus, RunLink};
use crate::sessions::test_support::agent_config;
use crate::state::DaemonState;

fn descriptor() -> anima_core::ExternalToolDescriptor {
    prepare_external_tool(
        "echo_record",
        "Read one synthetic test record",
        json!({
            "type":"object",
            "properties":{"value":{"type":"string"}},
            "required":["value"],
            "additionalProperties":false
        }),
        None,
    )
    .unwrap()
}

#[derive(Default)]
struct FakeFactory {
    calls: Arc<AtomicUsize>,
    hold_calls: AtomicBool,
    is_error: AtomicBool,
    last_session: StdMutex<Option<Arc<FakeSession>>>,
}

struct FakeSession {
    calls: Arc<AtomicUsize>,
    hold_calls: bool,
    is_error: bool,
    entered: Arc<Semaphore>,
}

impl McpTransportFactory for FakeFactory {
    fn connect(
        &self,
        _endpoint: ValidatedMcpEndpoint,
        _control: TransportControl,
    ) -> BoxFuture<'static, Result<Arc<dyn McpTransportSession>, McpTransportError>> {
        let session = Arc::new(FakeSession {
            calls: self.calls.clone(),
            hold_calls: self.hold_calls.load(Ordering::SeqCst),
            is_error: self.is_error.load(Ordering::SeqCst),
            entered: Arc::new(Semaphore::new(0)),
        });
        *self.last_session.lock().unwrap() = Some(session.clone());
        Box::pin(async move { Ok(session as Arc<dyn McpTransportSession>) })
    }
}

impl McpTransportSession for FakeSession {
    fn discover(
        &self,
        _control: TransportControl,
    ) -> BoxFuture<'static, Result<McpDiscovery, McpTransportError>> {
        Box::pin(async {
            Ok(McpDiscovery {
                protocol_version: "2025-11-25".into(),
                tools: vec![descriptor()],
            })
        })
    }

    fn call_once(
        &self,
        _call: McpCall,
        control: TransportControl,
    ) -> BoxFuture<'static, Result<ExternalToolResult, McpTransportError>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.entered.add_permits(1);
        let (hold, is_error) = (self.hold_calls, self.is_error);
        Box::pin(async move {
            if hold {
                control.cancel.cancelled().await;
                return Err(McpTransportError::OutcomeUnknown);
            }
            Ok(ExternalToolResult {
                text_blocks: vec!["synthetic M42 value".into()],
                structured_data: None,
                is_error,
                omitted_blocks: 0,
                shortened: false,
            })
        })
    }

    fn close(&self) -> BoxFuture<'static, Result<PhysicalClose, McpTransportError>> {
        Box::pin(async { Ok(PhysicalClose::for_test()) })
    }
}

struct Fixture {
    state: Arc<RwLock<DaemonState>>,
    snapshot_path: std::path::PathBuf,
    coordinator: AgentRunCoordinator,
    manager: McpManager,
    factory: Arc<FakeFactory>,
    agent: AgentState,
    run: RunLink,
    alias: String,
    connection_id: String,
    cancel: CancelSignal,
    context: crate::tools::ToolExecutionContext,
}

impl Fixture {
    async fn new(policy: PolicyAction, hold_calls: bool, is_error: bool) -> Self {
        let mut daemon = DaemonState::new();
        let snapshot_path =
            std::env::temp_dir().join(format!("anima-mcp-executor-{}.json", uuid::Uuid::new_v4()));
        daemon.control_plane_store = Some(ControlPlaneStoreConfig::Json(snapshot_path.clone()));
        let agent_id = daemon
            .create_agent(agent_config("M42 MCP executor"))
            .unwrap()
            .state
            .id;
        daemon.approvals.set_policy(
            &agent_id,
            ApprovalPolicy::default().with(RiskClass::Exec, policy),
        );
        let publication = daemon.mcp_publication.clone();
        let state = Arc::new(RwLock::new(daemon));
        let factory = Arc::new(FakeFactory {
            calls: Arc::new(AtomicUsize::new(0)),
            hold_calls: AtomicBool::new(hold_calls),
            is_error: AtomicBool::new(is_error),
            last_session: StdMutex::new(None),
        });
        let manager = McpManager::new(
            Arc::downgrade(&state),
            Arc::new(Mutex::new(())),
            publication,
            factory.clone(),
        );
        state.write().await.mcp_manager = Some(manager.clone());

        let created = manager
            .create(
                &agent_id,
                CreateMcpConnection {
                    label: "M42 synthetic fixture".into(),
                    endpoint: "http://127.0.0.1:32123/mcp".into(),
                },
            )
            .await
            .unwrap();
        let enabled = manager
            .update(
                &agent_id,
                &created.record.id,
                UpdateMcpConnection {
                    expected_version: created.record.version,
                    label: None,
                    endpoint: None,
                    enabled: Some(true),
                },
            )
            .await
            .unwrap();
        let connected = manager
            .connect(&agent_id, &created.record.id, enabled.record.version)
            .await
            .unwrap();
        let reviewed = manager
            .review(
                &agent_id,
                &created.record.id,
                ReviewMcpTool {
                    expected_version: connected.record.version,
                    discovery_id: connected.discovery_id.unwrap(),
                    descriptor_digest: connected.candidates[0].descriptor_digest.clone(),
                },
            )
            .await
            .unwrap();
        let reviewed_tool = reviewed.record.reviewed_tools[0].clone();
        let alias = reviewed_tool.alias.clone();

        {
            let mut guard = state.write().await;
            guard
                .update_agent(
                    &agent_id,
                    anima_core::AgentConfigUpdate {
                        tools: Some(vec![reviewed_tool.tool_descriptor()]),
                        ..Default::default()
                    },
                )
                .unwrap();
        }
        manager.note_agent_permissions_changed(&agent_id);

        let coordinator = AgentRunCoordinator::new(state.clone(), Arc::new(Semaphore::new(4)))
            .with_mcp(manager.clone())
            .with_approval_timeouts(crate::approvals::ApprovalTimeouts {
                default: std::time::Duration::from_secs(15),
                telegram: std::time::Duration::from_secs(15),
            });
        let run = ledger_run(&coordinator, &agent_id, "chat:m42-mcp-executor").await;
        let cancel = CancelSignal::new();
        let journal = RunActionJournal::new(
            &state,
            coordinator.control_plane_transactions(),
            run.clone(),
            cancel.clone(),
            None,
        );
        let context = gated_context(&coordinator, &run, cancel.clone())
            .await
            .with_mcp(Some(manager.clone()))
            .with_action_journal(journal);
        let agent = state.read().await.get_agent(&agent_id).unwrap().state;
        Self {
            state,
            snapshot_path,
            coordinator,
            manager,
            factory,
            agent,
            run,
            alias,
            connection_id: created.record.id,
            cancel,
            context,
        }
    }

    fn call(&self, id: &str) -> ToolCall {
        ToolCall {
            id: id.into(),
            name: self.alias.clone(),
            args: [("value".into(), DataValue::String("violet".into()))].into(),
        }
    }

    async fn execute(&self, call: ToolCall) -> anima_core::TaskResult<anima_core::Content> {
        self.context
            .clone()
            .execute_tool(
                self.agent.clone(),
                tool_input(&self.agent.id, &self.run.session_id),
                call,
            )
            .await
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.snapshot_path);
    }
}

#[tokio::test]
async fn exact_reviewed_grant_dispatches_with_provenance_and_settled_receipt() {
    let fixture = Fixture::new(PolicyAction::Allow, false, false).await;
    let unreviewed = ToolCall {
        id: "m42-unreviewed".into(),
        name: "mcp_not_reviewed".into(),
        args: Default::default(),
    };
    assert_eq!(fixture.execute(unreviewed).await.status, TaskStatus::Error);
    assert_eq!(fixture.factory.calls.load(Ordering::SeqCst), 0);

    let result = fixture.execute(fixture.call("m42-allow")).await;
    assert_eq!(result.status, TaskStatus::Success, "{:?}", result.error);
    let content = result.data.expect("bounded external content");
    let envelope: Value = serde_json::from_str(&content.text).unwrap();
    assert_eq!(envelope["outcome"], "recorded");
    assert_eq!(envelope["alias"], fixture.alias);
    assert_eq!(envelope["connectionId"], fixture.connection_id);
    assert_eq!(envelope["textBlocks"][0], "synthetic M42 value");
    assert_eq!(fixture.factory.calls.load(Ordering::SeqCst), 1);

    let state = fixture.state.read().await;
    let receipt = &state.runs.get(&fixture.run.run_id).unwrap().action_receipts[0];
    assert_eq!(receipt.name, fixture.alias);
    assert_eq!(receipt.status, ActionStatus::Succeeded);
    assert!(receipt.finished_at_ms.is_some());
    let preview = receipt.result_preview.as_deref().unwrap();
    assert!(preview.contains(&fixture.alias));
    assert!(preview.contains(&fixture.connection_id));
    drop(state);
    fixture.manager.shutdown().await;
}

#[tokio::test]
async fn policy_deny_never_calls_remote_and_ask_waits_for_owner_decision() {
    let denied = Fixture::new(PolicyAction::Deny, false, false).await;
    let result = denied.execute(denied.call("m42-denied")).await;
    assert_eq!(result.status, TaskStatus::Error);
    assert_eq!(denied.factory.calls.load(Ordering::SeqCst), 0);

    let asked = Fixture::new(PolicyAction::Ask, false, false).await;
    let (context, agent, input, call) = (
        asked.context.clone(),
        asked.agent.clone(),
        tool_input(&asked.agent.id, &asked.run.session_id),
        asked.call("m42-ask"),
    );
    let pending_call = tokio::spawn(async move { context.execute_tool(agent, input, call).await });
    let request = pending_approvals(&asked.coordinator, 1).await.remove(0);
    assert_eq!(asked.factory.calls.load(Ordering::SeqCst), 0);
    asked
        .coordinator
        .decide_approval(
            &request.id,
            decision(ApprovalDecisionKind::AllowOnce, request.revision),
        )
        .await
        .unwrap();
    let result = pending_call.await.unwrap();
    assert_eq!(result.status, TaskStatus::Success, "{:?}", result.error);
    assert_eq!(asked.factory.calls.load(Ordering::SeqCst), 1);
    denied.manager.shutdown().await;
    asked.manager.shutdown().await;
}

#[tokio::test]
async fn revoked_grant_while_asking_is_rechecked_before_any_remote_call() {
    let fixture = Fixture::new(PolicyAction::Ask, false, false).await;
    let (context, agent, input, call) = (
        fixture.context.clone(),
        fixture.agent.clone(),
        tool_input(&fixture.agent.id, &fixture.run.session_id),
        fixture.call("m42-revoke"),
    );
    let pending_call = tokio::spawn(async move { context.execute_tool(agent, input, call).await });
    let request = pending_approvals(&fixture.coordinator, 1).await.remove(0);
    assert_eq!(fixture.factory.calls.load(Ordering::SeqCst), 0);

    {
        let mut state = fixture.state.write().await;
        let runtime = state.agents.get_mut(&fixture.agent.id).unwrap();
        let mut config = runtime.config().clone();
        config.tools = Some(Vec::new());
        runtime.replace_config(config);
    }
    fixture
        .manager
        .note_agent_permissions_changed(&fixture.agent.id);
    fixture
        .coordinator
        .decide_approval(
            &request.id,
            decision(ApprovalDecisionKind::AllowOnce, request.revision),
        )
        .await
        .unwrap();
    let result = pending_call.await.unwrap();
    assert_eq!(result.status, TaskStatus::Error);
    assert_eq!(fixture.factory.calls.load(Ordering::SeqCst), 0);
    let state = fixture.state.read().await;
    assert!(state
        .runs
        .get(&fixture.run.run_id)
        .unwrap()
        .action_receipts
        .iter()
        .all(|receipt| receipt.status != ActionStatus::Succeeded));
    drop(state);
    fixture.manager.shutdown().await;
}

#[tokio::test]
async fn stop_during_remote_call_records_unknown_failure_not_success() {
    let fixture = Fixture::new(PolicyAction::Allow, true, false).await;
    let session = fixture
        .factory
        .last_session
        .lock()
        .unwrap()
        .clone()
        .expect("connected fake session");
    let (context, agent, input, call) = (
        fixture.context.clone(),
        fixture.agent.clone(),
        tool_input(&fixture.agent.id, &fixture.run.session_id),
        fixture.call("m42-stop"),
    );
    let pending_call = tokio::spawn(async move { context.execute_tool(agent, input, call).await });
    tokio::time::timeout(std::time::Duration::from_secs(2), session.entered.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    fixture.cancel.cancel();
    let result = tokio::time::timeout(std::time::Duration::from_secs(2), pending_call)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.status, TaskStatus::Error);
    assert!(result
        .error
        .as_deref()
        .unwrap_or_default()
        .contains("outcome is unknown"));
    assert_eq!(fixture.factory.calls.load(Ordering::SeqCst), 1);
    let state = fixture.state.read().await;
    let receipt = &state.runs.get(&fixture.run.run_id).unwrap().action_receipts[0];
    assert_eq!(receipt.status, ActionStatus::Failed);
    assert!(!receipt
        .result_preview
        .as_deref()
        .unwrap_or_default()
        .contains("outcome\":\"recorded\""));
    drop(state);
    fixture.manager.shutdown().await;
}

#[tokio::test]
async fn remote_error_flag_is_an_error_result_while_bounded_provenance_is_retained() {
    let fixture = Fixture::new(PolicyAction::Allow, false, true).await;
    let result = fixture.execute(fixture.call("m42-remote-error")).await;
    assert_eq!(result.status, TaskStatus::Error);
    assert_eq!(
        result.error.as_deref(),
        Some("External tool reported failure")
    );
    let content = result.data.expect("bounded error envelope retained");
    let envelope: Value = serde_json::from_str(&content.text).unwrap();
    assert_eq!(envelope["isError"], true);
    assert_eq!(envelope["alias"], fixture.alias);
    let state = fixture.state.read().await;
    let receipt = &state.runs.get(&fixture.run.run_id).unwrap().action_receipts[0];
    assert_eq!(receipt.status, ActionStatus::Failed);
    assert!(receipt
        .result_preview
        .as_deref()
        .unwrap()
        .contains("External tool reported failure"));
    drop(state);
    fixture.manager.shutdown().await;
}
