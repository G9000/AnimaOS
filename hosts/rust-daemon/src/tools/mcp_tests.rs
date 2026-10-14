//! End-to-end executor witnesses for owner-reviewed MCP tools, using both a
//! fake transport and a controlled native stdio peer at the approval boundary.

use std::path::{Path, PathBuf};
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
    DefaultMcpTransportFactory, McpCall, McpDiscovery, McpTransportError, McpTransportFactory,
    McpTransportSession, PhysicalClose, TransportControl, ValidatedMcpLaunch,
};
use crate::mcp::{
    CreateMcpConnection, McpConnectionState, McpGrantInput, McpManager, McpStdioSpec,
    ReviewMcpTool, UpdateMcpConnection,
};
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
        launch: ValidatedMcpLaunch,
        _control: TransportControl,
    ) -> BoxFuture<'static, Result<Arc<dyn McpTransportSession>, McpTransportError>> {
        assert!(matches!(launch, ValidatedMcpLaunch::Http(_)));
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

struct Fixture<F = FakeFactory> {
    state: Arc<RwLock<DaemonState>>,
    coordinator: AgentRunCoordinator,
    manager: McpManager,
    factory: Arc<F>,
    agent: AgentState,
    run: RunLink,
    alias: String,
    connection_id: String,
    descriptor_digest: String,
    cancel: CancelSignal,
    context: crate::tools::ToolExecutionContext,
    folder: tempfile::TempDir,
}

impl Fixture {
    async fn new(policy: PolicyAction, hold_calls: bool, is_error: bool) -> Self {
        let factory = Arc::new(FakeFactory {
            calls: Arc::new(AtomicUsize::new(0)),
            hold_calls: AtomicBool::new(hold_calls),
            is_error: AtomicBool::new(is_error),
            last_session: StdMutex::new(None),
        });
        Self::with_factory(policy, factory, None, |_| {}).await
    }
}

impl<F: McpTransportFactory + 'static> Fixture<F> {
    async fn with_factory(
        policy: PolicyAction,
        factory: Arc<F>,
        stdio_executable: Option<PathBuf>,
        assert_no_child: fn(&F),
    ) -> Self {
        let folder = tempfile::tempdir().unwrap();
        let marker = folder.path().join("stdio-peer.json");
        let is_stdio = stdio_executable.is_some();
        let mut daemon = DaemonState::new();
        daemon.control_plane_store = Some(ControlPlaneStoreConfig::Json(
            folder.path().join("control-plane.json"),
        ));
        let agent_id = daemon
            .create_agent(agent_config("Synthetic MCP executor"))
            .unwrap()
            .state
            .id;
        daemon.approvals.set_policy(
            &agent_id,
            ApprovalPolicy::default().with(RiskClass::Exec, policy),
        );
        let publication = daemon.mcp_publication.clone();
        let state = Arc::new(RwLock::new(daemon));
        let manager = McpManager::new(
            Arc::downgrade(&state),
            Arc::new(Mutex::new(())),
            publication,
            factory.clone(),
        );
        state.write().await.mcp_manager = Some(manager.clone());

        let created = match stdio_executable {
            Some(executable) => {
                let created = manager
                    .create_stdio(
                        &agent_id,
                        "M44 synthetic native fixture".into(),
                        McpStdioSpec {
                            executable: executable.to_str().unwrap().into(),
                            args: vec![
                                "--mode".into(),
                                "normal".into(),
                                "--marker".into(),
                                marker.to_str().unwrap().into(),
                            ],
                        },
                    )
                    .await
                    .unwrap();
                assert!(!created.record.enabled);
                assert_eq!(created.state, McpConnectionState::Disconnected);
                assert!(created.candidates.is_empty());
                assert!(!marker.exists(), "disabled save must not start the peer");
                assert_no_child(&factory);
                let listed = manager.list(&agent_id).await.unwrap();
                assert_eq!(listed.len(), 1);
                assert_eq!(listed[0].record.id, created.record.id);
                assert!(!marker.exists(), "listing must not start the peer");
                assert_no_child(&factory);
                assert!(manager
                    .review(
                        &agent_id,
                        &created.record.id,
                        ReviewMcpTool {
                            expected_version: created.record.version,
                            discovery_id: "not-discovered".into(),
                            descriptor_digest: descriptor().descriptor_digest,
                        },
                    )
                    .await
                    .is_err());
                assert!(!marker.exists(), "review must not start the peer");
                assert_no_child(&factory);
                assert!(manager
                    .connect(&agent_id, &created.record.id, created.record.version)
                    .await
                    .is_err());
                assert!(!marker.exists(), "disabled connect must not start the peer");
                assert_no_child(&factory);
                created
            }
            None => manager
                .create(
                    &agent_id,
                    CreateMcpConnection {
                        label: "M42 synthetic fixture".into(),
                        endpoint: "http://127.0.0.1:32123/mcp".into(),
                    },
                )
                .await
                .unwrap(),
        };
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
        if is_stdio {
            assert!(!marker.exists(), "enabling must not start the peer");
            assert_no_child(&factory);
        }
        let connected = manager
            .connect(&agent_id, &created.record.id, enabled.record.version)
            .await
            .unwrap();
        if is_stdio {
            assert_eq!(connected.state, McpConnectionState::NeedsReview);
            assert_eq!(connected.candidates.len(), 1);
            assert_eq!(
                connected.candidates[0].remote_name,
                "synthetic_stdio_record"
            );
            let peer = read_stdio_marker(&marker);
            assert!(peer["parentPid"].as_u64().unwrap() > 0);
            assert_eq!(peer_count(&peer, "initialize"), 1);
            assert_eq!(peer_count(&peer, "tools/list"), 1);
            assert_eq!(peer_count(&peer, "tools/call"), 0);
            for (discovery_id, descriptor_digest) in [
                (
                    "wrong-discovery".into(),
                    connected.candidates[0].descriptor_digest.clone(),
                ),
                (
                    connected.discovery_id.clone().unwrap(),
                    descriptor().descriptor_digest,
                ),
            ] {
                assert!(manager
                    .review(
                        &agent_id,
                        &created.record.id,
                        ReviewMcpTool {
                            expected_version: connected.record.version,
                            discovery_id,
                            descriptor_digest,
                        },
                    )
                    .await
                    .is_err());
                assert_eq!(read_stdio_marker(&marker), peer);
            }
        }
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
        let descriptor_digest = reviewed_tool.descriptor.descriptor_digest.clone();

        if is_stdio {
            assert_eq!(reviewed.state, McpConnectionState::Ready);
            assert_eq!(peer_count(&read_stdio_marker(&marker), "tools/call"), 0);
            let granted = manager
                .set_grant(
                    &agent_id,
                    &created.record.id,
                    &alias,
                    McpGrantInput {
                        expected_version: reviewed.record.version,
                        descriptor_digest: descriptor_digest.clone(),
                        granted: true,
                    },
                )
                .await
                .unwrap();
            assert!(granted.granted);
            assert!(granted.tool_names.contains(&alias));
            assert_eq!(peer_count(&read_stdio_marker(&marker), "tools/call"), 0);
        } else {
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
        if !is_stdio {
            manager.note_agent_permissions_changed(&agent_id);
        }

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
            folder,
            coordinator,
            manager,
            factory,
            agent,
            run,
            alias,
            connection_id: created.record.id,
            descriptor_digest,
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

fn stdio_fixture_executable() -> PathBuf {
    let executable = std::env::var_os("ANIMA_MCP_STDIO_FIXTURE_EXE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let harness = std::env::current_exe().unwrap();
            let profile = harness.parent().unwrap().parent().unwrap();
            profile.join("examples").join(if cfg!(windows) {
                "mcp_stdio_peer.exe"
            } else {
                "mcp_stdio_peer"
            })
        });
    assert!(
        executable.is_absolute(),
        "fixture executable must be absolute"
    );
    assert!(
        executable.is_file(),
        "root must build the mcp_stdio_peer example before the executor cases"
    );
    executable
}

fn read_stdio_marker(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).expect("native peer wrote its private marker"))
        .expect("native peer marker contains synthetic JSON")
}

fn peer_count(marker: &Value, method: &str) -> u64 {
    let counts = marker["counts"]
        .as_object()
        .expect("synthetic peer method counts");
    counts
        .get(method)
        .map(|value| value.as_u64().expect("synthetic peer count is an integer"))
        .unwrap_or(0)
}

impl Fixture<DefaultMcpTransportFactory> {
    async fn stdio(policy: PolicyAction) -> Self {
        let factory = Arc::new(DefaultMcpTransportFactory::default());
        assert_eq!(factory.stdio_child_count(), 0);
        let fixture = Self::with_factory(
            policy,
            factory,
            Some(stdio_fixture_executable()),
            |factory| assert_eq!(factory.stdio_child_count(), 0),
        )
        .await;
        assert_eq!(fixture.factory.stdio_child_count(), 1);
        fixture
    }

    fn peer(&self) -> Value {
        read_stdio_marker(&self.folder.path().join("stdio-peer.json"))
    }

    async fn revoke(&self) {
        let current = self
            .manager
            .get(&self.agent.id, &self.connection_id)
            .await
            .unwrap();
        let revoked = self
            .manager
            .set_grant(
                &self.agent.id,
                &self.connection_id,
                &self.alias,
                McpGrantInput {
                    expected_version: current.record.version,
                    descriptor_digest: self.descriptor_digest.clone(),
                    granted: false,
                },
            )
            .await
            .unwrap();
        assert!(!revoked.granted);
        assert!(!revoked.tool_names.contains(&self.alias));
    }

    async fn shutdown_stdio(&self) {
        tokio::time::timeout(std::time::Duration::from_secs(5), self.manager.shutdown())
            .await
            .expect("manager joins controlled child and pipe cleanup");
        // This count includes retiring/quarantined children until the transport
        // joins the group/job wait, SDK, stdout/stderr readers and owned sends.
        assert_eq!(self.factory.stdio_child_count(), 0);
    }
}

#[cfg(windows)]
#[tokio::test]
async fn m44_stdio_exec_approval_records_bounded_provenance_and_a_durable_receipt() {
    let denied = Fixture::stdio(PolicyAction::Deny).await;
    let before_deny = denied.peer();
    assert_eq!(peer_count(&before_deny, "tools/call"), 0);
    let result = denied.execute(denied.call("m44-stdio-deny")).await;
    assert_eq!(result.status, TaskStatus::Error);
    assert_eq!(denied.peer(), before_deny, "Deny sends no peer RPC");
    assert!(denied
        .state
        .read()
        .await
        .runs
        .get(&denied.run.run_id)
        .unwrap()
        .action_receipts
        .iter()
        .all(|receipt| receipt.status != ActionStatus::Succeeded));
    denied.shutdown_stdio().await;

    let asked = Fixture::stdio(PolicyAction::Ask).await;
    let before_approval = asked.peer();
    let (context, agent, input, call) = (
        asked.context.clone(),
        asked.agent.clone(),
        tool_input(&asked.agent.id, &asked.run.session_id),
        asked.call("m44-stdio-approved"),
    );
    let pending_call = tokio::spawn(async move { context.execute_tool(agent, input, call).await });
    let request = pending_approvals(&asked.coordinator, 1).await.remove(0);
    assert_eq!(request.class, RiskClass::Exec);
    assert_eq!(request.tool, asked.alias);
    assert_eq!(request.tool_call_id, "m44-stdio-approved");
    assert_eq!(request.run_id, asked.run.run_id);
    assert_eq!(
        asked.peer(),
        before_approval,
        "Ask sends no peer RPC before approval"
    );
    asked
        .coordinator
        .decide_approval(
            &request.id,
            decision(ApprovalDecisionKind::AllowOnce, request.revision),
        )
        .await
        .unwrap();
    let result = tokio::time::timeout(std::time::Duration::from_secs(5), pending_call)
        .await
        .expect("approved native call settles")
        .unwrap();
    assert_eq!(result.status, TaskStatus::Success, "{:?}", result.error);
    let content = result.data.expect("bounded native MCP content");
    assert!(content.text.len() <= 16 * 1024);
    let envelope: Value = serde_json::from_str(&content.text).unwrap();
    assert_eq!(envelope["outcome"], "recorded");
    assert_eq!(envelope["connectionId"], asked.connection_id);
    assert_eq!(envelope["alias"], asked.alias);
    assert_eq!(envelope["descriptorDigest"], asked.descriptor_digest);
    assert_eq!(envelope["isError"], false);
    assert_eq!(envelope["shortened"], false);
    assert_eq!(envelope["omittedBlocks"], 0);
    assert_eq!(
        envelope["textBlocks"],
        json!(["Synthetic stdio violet record"])
    );
    let current = asked
        .manager
        .get(&asked.agent.id, &asked.connection_id)
        .await
        .unwrap();
    assert_eq!(envelope["wireGeneration"], current.wire_generation);
    let after_call = asked.peer();
    assert_eq!(peer_count(&after_call, "initialize"), 1);
    assert_eq!(peer_count(&after_call, "tools/list"), 1);
    assert_eq!(peer_count(&after_call, "tools/call"), 1);

    let receipt = asked
        .state
        .read()
        .await
        .runs
        .get(&asked.run.run_id)
        .unwrap()
        .action_receipts
        .iter()
        .find(|receipt| receipt.tool_call_id == "m44-stdio-approved")
        .unwrap()
        .clone();
    assert_eq!(receipt.name, asked.alias);
    assert_eq!(receipt.status, ActionStatus::Succeeded);
    assert!(receipt.finished_at_ms.is_some());
    let preview = receipt.result_preview.as_deref().unwrap();
    assert!(preview.contains(&asked.alias));
    assert!(preview.contains(&asked.connection_id));
    assert!(preview.contains(&asked.descriptor_digest));
    let saved = crate::control_plane_store::load_control_plane_snapshot(
        &ControlPlaneStoreConfig::Json(asked.folder.path().join("control-plane.json")),
    )
    .await
    .unwrap()
    .unwrap();
    let persisted = saved
        .runs
        .iter()
        .find(|run| run.id == asked.run.run_id)
        .unwrap();
    assert!(persisted.action_receipts.contains(&receipt));

    asked.revoke().await;
    assert_eq!(
        asked.peer(),
        after_call,
        "grant revocation sends no peer RPC"
    );
    let rejected = asked.execute(asked.call("m44-stdio-after-revoke")).await;
    assert_eq!(rejected.status, TaskStatus::Error);
    assert_eq!(
        asked.peer(),
        after_call,
        "revoked grant sends no additional peer RPC"
    );
    asked.shutdown_stdio().await;
}

#[cfg(windows)]
#[tokio::test]
async fn m44_stdio_revoked_grant_is_rechecked_after_pending_owner_approval() {
    let fixture = Fixture::stdio(PolicyAction::Ask).await;
    let before_approval = fixture.peer();
    let (context, agent, input, call) = (
        fixture.context.clone(),
        fixture.agent.clone(),
        tool_input(&fixture.agent.id, &fixture.run.session_id),
        fixture.call("m44-stdio-revoke-pending"),
    );
    let pending_call = tokio::spawn(async move { context.execute_tool(agent, input, call).await });
    let request = pending_approvals(&fixture.coordinator, 1).await.remove(0);
    assert_eq!(request.class, RiskClass::Exec);
    assert_eq!(fixture.peer(), before_approval);
    fixture.revoke().await;
    fixture
        .coordinator
        .decide_approval(
            &request.id,
            decision(ApprovalDecisionKind::AllowOnce, request.revision),
        )
        .await
        .unwrap();
    let result = tokio::time::timeout(std::time::Duration::from_secs(5), pending_call)
        .await
        .expect("revoked pending call settles")
        .unwrap();
    assert_eq!(result.status, TaskStatus::Error);
    assert_eq!(
        fixture.peer(),
        before_approval,
        "revoked pending call sends no peer RPC"
    );
    assert_eq!(peer_count(&fixture.peer(), "tools/call"), 0);
    assert!(fixture
        .state
        .read()
        .await
        .runs
        .get(&fixture.run.run_id)
        .unwrap()
        .action_receipts
        .iter()
        .all(|receipt| receipt.status != ActionStatus::Succeeded));
    fixture.shutdown_stdio().await;
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
