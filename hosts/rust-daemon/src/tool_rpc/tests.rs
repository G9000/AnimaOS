//! Real native gate/journal fixtures; no Python, socket, provider or network.
#[path = "broker_reply_tests.rs"]
mod reply_tests;
#[path = "service_tests.rs"]
mod service_tests;

use super::*;
use std::{collections::BTreeMap, sync::Arc, time::Duration};

use anima_core::{
    AgentConfigUpdate, AgentState, CancelSignal, DataValue, TaskStatus, ToolCall, ToolDescriptor,
};
use serde_json::{json, Value};
use tokio::sync::{RwLock, Semaphore};

use crate::agent_runs::{
    action_receipts::RunActionJournal,
    test_support::{
        companion_config, coordinator_state, decision, gated_context, ledger_run,
        pending_approvals, tool_input,
    },
    AgentRunCoordinator,
};
use crate::approvals::{ApprovalDecisionKind, ApprovalPolicy, PolicyAction, RiskClass};
use crate::control_plane_store::ControlPlaneStoreConfig;
use crate::runs::{ActionReceipt, ActionStatus, RunLink};
use crate::skills::test_support::with_workspace;
use crate::state::{DaemonState, TestControlPlaneSaveGate};
use crate::tools::{ToolExecutionContext, ToolRegistry};

const SESSION: &str = "chat:fp06-broker";
const TARGET: &str = "rpc/summary.txt";

fn outer_call() -> ToolCall {
    ToolCall {
        id: "fp06-outer".into(),
        name: "execute_code".into(),
        args: BTreeMap::from([(
            "code".into(),
            DataValue::String("print('bounded program')".into()),
        )]),
    }
}

fn read_call(id: &str) -> ToolCall {
    ToolCall {
        id: id.into(),
        name: "read_file".into(),
        args: BTreeMap::from([("file_path".into(), DataValue::String("alpha.json".into()))]),
    }
}

struct Fixture {
    root: tempfile::TempDir,
    coordinator: AgentRunCoordinator,
    state: crate::app::SharedDaemonState,
    run: RunLink,
    agent: AgentState,
    context: ToolExecutionContext,
    bare_context: ToolExecutionContext,
    journal: RunActionJournal,
}

impl Fixture {
    async fn new(write_policy: PolicyAction) -> Self {
        Self::with_deadline(write_policy, None).await
    }

    async fn with_deadline(
        write_policy: PolicyAction,
        deadline: Option<tokio::time::Instant>,
    ) -> Self {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("alpha.json"), b"{\"value\":7}").unwrap();
        let mut state = with_workspace(DaemonState::new(), root.path());
        let mut config = companion_config("RPC owner");
        config.tools = Some(
            ToolRegistry::new()
                .resolve_descriptors(["read_file", "write_file", "bash"])
                .unwrap(),
        );
        let agent_id = state.create_agent(config).unwrap().state.id;
        // Registration is controller-owned and intentionally pending. This
        // descriptor supplies only the captured grant; no handler is installed.
        let runtime = state.agents.get_mut(&agent_id).unwrap();
        let mut tools = runtime.config().tools.clone().unwrap();
        tools.push(ToolDescriptor {
            name: "execute_code".into(),
            description: "FP06 wrapper grant fixture".into(),
            parameters_schema: BTreeMap::new(),
            examples: None,
        });
        runtime.update_config(AgentConfigUpdate {
            tools: Some(tools),
            ..Default::default()
        });
        state.approvals.set_policy(
            &agent_id,
            ApprovalPolicy::default()
                .with(RiskClass::Write, write_policy)
                .with(RiskClass::Exec, PolicyAction::Allow),
        );
        state.set_control_plane_store(Some(ControlPlaneStoreConfig::Json(
            root.path().join("control-plane.json"),
        )));
        let state = Arc::new(RwLock::new(state));
        let coordinator = AgentRunCoordinator::new(state.clone(), Arc::new(Semaphore::new(1)));
        let run = ledger_run(&coordinator, &agent_id, SESSION).await;
        let cancel = CancelSignal::new();
        let journal = RunActionJournal::new(
            &state,
            coordinator.control_plane_transactions(),
            run.clone(),
            cancel.clone(),
            deadline,
        );
        let bare_context = gated_context(&coordinator, &run, cancel).await;
        let context = bare_context.clone().with_action_journal(journal.clone());
        let agent = state.read().await.get_agent(&agent_id).unwrap().state;
        journal.begin(&outer_call()).await.unwrap();
        Self {
            root,
            coordinator,
            state,
            run,
            agent,
            context,
            bare_context,
            journal,
        }
    }

    async fn capture(&self) -> Result<Broker, BrokerError> {
        Broker::capture(
            self.context.clone(),
            self.agent.clone(),
            tool_input(&self.agent.id, SESSION),
            outer_call(),
        )
        .await
    }

    async fn receipts(&self) -> Vec<ActionReceipt> {
        self.state
            .read()
            .await
            .runs
            .get(&self.run.run_id)
            .unwrap()
            .action_receipts
            .clone()
    }

    async fn remove_grant(&self, name: &str) {
        let mut guard = self.state.write().await;
        let runtime = guard.agents.get_mut(&self.agent.id).unwrap();
        let mut tools = runtime.config().tools.clone().unwrap();
        tools.retain(|tool| tool.name != name);
        runtime.update_config(AgentConfigUpdate {
            tools: Some(tools),
            ..Default::default()
        });
    }

    async fn save_gate(&self, fail: bool) -> TestControlPlaneSaveGate {
        self.state
            .write()
            .await
            .install_test_control_plane_save_gate(fail)
    }

    fn output_exists(&self) -> bool {
        self.root.path().join(TARGET).exists()
    }
}

fn frame(credentials: &BrokerCredentials, sequence: u32, tool: &str, args: Value) -> Vec<u8> {
    serde_json::to_vec(&json!({"token":credentials.token,"request":{
        "version":1,"cellEpoch":credentials.cell_epoch,"sequence":sequence,"tool":tool,"args":args
    }}))
    .unwrap()
}

fn write_frame(credentials: &BrokerCredentials) -> Vec<u8> {
    frame(
        credentials,
        1,
        "write_file",
        json!({"file_path":TARGET,"content":"written once through native RPC"}),
    )
}

async fn entered(gate: &TestControlPlaneSaveGate) {
    tokio::time::timeout(Duration::from_secs(5), gate.entered.acquire())
        .await
        .expect("native save reached")
        .unwrap()
        .forget();
}

#[tokio::test]
async fn fp06_broker_captures_authentic_scope_and_effective_remaining_receipts() {
    let fixture = Fixture::new(PolicyAction::Allow).await;
    assert!(Arc::ptr_eq(
        &coordinator_state(&fixture.coordinator),
        &fixture.state
    ));
    let broker = fixture
        .capture()
        .await
        .expect("authentic gated run accepted");
    assert_eq!(broker.effective_call_limit(), 49);
    assert!(!broker.credentials().token.is_empty());
    for id in ["before-rpc-1", "before-rpc-2"] {
        let result = fixture
            .context
            .clone()
            .execute_tool(
                fixture.agent.clone(),
                tool_input(&fixture.agent.id, SESSION),
                read_call(id),
            )
            .await;
        assert_eq!(result.status, TaskStatus::Success);
    }
    assert_eq!(fixture.capture().await.unwrap().effective_call_limit(), 47);
}

#[tokio::test]
async fn fp06_broker_requires_both_current_wrapper_and_shell_grants() {
    for missing in ["execute_code", "bash"] {
        let fixture = Fixture::new(PolicyAction::Allow).await;
        fixture.remove_grant(missing).await;
        assert!(
            matches!(fixture.capture().await, Err(BrokerError::PermissionDenied)),
            "missing {missing}"
        );
        assert!(!fixture.output_exists());
        assert_eq!(fixture.receipts().await.len(), 1);
    }
}

#[tokio::test]
async fn fp06_broker_ordinary_native_self_route_is_not_an_upstream_peer() {
    let fixture = Fixture::new(PolicyAction::Allow).await;
    // The real coordinator installs this route on every ordinary owner run.
    // A forwarded peer route remains covered by the refusal case below.
    let context = fixture.context.clone().with_peer_route(
        anima_core::AgentCommunicationRoute::start(&fixture.agent.id),
        vec![],
    );
    let broker = Broker::capture(
        context,
        fixture.agent.clone(),
        tool_input(&fixture.agent.id, SESSION),
        outer_call(),
    )
    .await;
    assert!(
        broker.is_ok(),
        "an ordinary self route must retain native RPC admission"
    );
    let mut broker = broker.unwrap();
    let result = broker
        .request(&frame(
            &broker.credentials(),
            1,
            "read_file",
            json!({"file_path":"alpha.json"}),
        ))
        .await;
    assert_eq!(result.status, TaskStatus::Success);
    assert_eq!(fixture.receipts().await.len(), 2);
    let foreign_route = fixture.context.clone().with_peer_route(
        anima_core::AgentCommunicationRoute::start("another-owner"),
        vec![],
    );
    assert!(matches!(
        Broker::capture(
            foreign_route,
            fixture.agent.clone(),
            tool_input(&fixture.agent.id, SESSION),
            outer_call()
        )
        .await,
        Err(BrokerError::UnsupportedOrigin)
    ));
}

#[tokio::test]
async fn fp06_broker_requires_gate_journal_cancel_and_exact_host_run_scope() {
    let fixture = Fixture::new(PolicyAction::Allow).await;
    for context in [
        fixture.bare_context.clone(),
        fixture.context.clone().with_approvals(None),
        fixture.context.clone().with_cancel(None),
        fixture.context.clone().with_run_link(None),
    ] {
        let result = Broker::capture(
            context,
            fixture.agent.clone(),
            tool_input(&fixture.agent.id, SESSION),
            outer_call(),
        )
        .await;
        assert!(matches!(result, Err(BrokerError::MissingContext)));
    }
    let mut foreign = fixture.run.clone();
    foreign.session_id = "chat:foreign".into();
    let result = Broker::capture(
        fixture.context.clone().with_run_link(Some(foreign)),
        fixture.agent.clone(),
        tool_input(&fixture.agent.id, SESSION),
        outer_call(),
    )
    .await;
    assert!(matches!(result, Err(BrokerError::ForeignScope)));
    let result = Broker::capture(
        fixture.context.clone(),
        fixture.agent.clone(),
        tool_input("foreign-agent", SESSION),
        outer_call(),
    )
    .await;
    assert!(matches!(result, Err(BrokerError::ForeignScope)));

    // Both sessions and runs really exist. Relabelling only the context cannot
    // carry the original session's native journal or approval gate into either.
    let foreign = ledger_run(&fixture.coordinator, &fixture.agent.id, "chat:other-valid").await;
    let result = Broker::capture(
        fixture.context.clone().with_run_link(Some(foreign.clone())),
        fixture.agent.clone(),
        tool_input(&fixture.agent.id, &foreign.session_id),
        outer_call(),
    )
    .await;
    assert!(matches!(result, Err(BrokerError::ForeignScope)));

    let foreign_gate = gated_context(&fixture.coordinator, &foreign, CancelSignal::new())
        .await
        .with_action_journal(fixture.journal.clone())
        .with_run_link(Some(fixture.run.clone()));
    let result = Broker::capture(
        foreign_gate,
        fixture.agent.clone(),
        tool_input(&fixture.agent.id, SESSION),
        outer_call(),
    )
    .await;
    assert!(matches!(result, Err(BrokerError::ForeignScope)));
}

#[tokio::test]
async fn fp06_broker_refuses_helpers_delegated_and_peer_contexts() {
    let fixture = Fixture::new(PolicyAction::Allow).await;
    let mut helper = fixture.agent.clone();
    helper
        .config
        .settings
        .as_mut()
        .unwrap()
        .additional
        .insert("workspaceRole".into(), DataValue::String("helper".into()));
    assert!(matches!(
        Broker::capture(
            fixture.context.clone(),
            helper,
            tool_input(&fixture.agent.id, SESSION),
            outer_call()
        )
        .await,
        Err(BrokerError::UnsupportedOrigin)
    ));
    for context in [
        fixture
            .context
            .clone()
            .with_delegated_parent(Some("parent".into())),
        fixture.context.clone().with_peer_route(
            anima_core::AgentCommunicationRoute::start("source")
                .forward("source", &fixture.agent.id)
                .unwrap(),
            vec!["source".into()],
        ),
    ] {
        assert!(matches!(
            Broker::capture(
                context,
                fixture.agent.clone(),
                tool_input(&fixture.agent.id, SESSION),
                outer_call()
            )
            .await,
            Err(BrokerError::UnsupportedOrigin)
        ));
    }
}

#[tokio::test]
async fn fp06_broker_revalidates_outer_exec_policy_after_accepted_intent() {
    let fixture = Fixture::new(PolicyAction::Allow).await;
    fixture.state.write().await.approvals.set_policy(
        &fixture.agent.id,
        ApprovalPolicy::default().with(RiskClass::Exec, PolicyAction::Deny),
    );
    assert!(matches!(
        fixture.capture().await,
        Err(BrokerError::PermissionDenied)
    ));
    assert_eq!(fixture.receipts().await.len(), 1);
}

#[tokio::test]
async fn fp06_broker_authentication_epoch_and_request_scope_cannot_select_an_owner() {
    let fixture = Fixture::new(PolicyAction::Allow).await;
    let mut broker = fixture.capture().await.unwrap();
    let credentials = broker.credentials();
    for changed in [
        json!({"token":"foreign"}),
        json!({"ownerAgentId":"foreign"}),
    ] {
        let mut value: Value = serde_json::from_slice(&write_frame(&credentials)).unwrap();
        for (key, item) in changed.as_object().unwrap() {
            value[key] = item.clone();
        }
        assert_eq!(
            broker
                .request(&serde_json::to_vec(&value).unwrap())
                .await
                .status,
            TaskStatus::Error
        );
    }
    let mut stale: Value = serde_json::from_slice(&write_frame(&credentials)).unwrap();
    stale["request"]["cellEpoch"] = json!("stale-cell");
    assert_eq!(
        broker
            .request(&serde_json::to_vec(&stale).unwrap())
            .await
            .status,
        TaskStatus::Error
    );
    assert_eq!(fixture.receipts().await.len(), 1);
    assert!(!fixture.output_exists());
    assert_eq!(
        broker
            .request(&frame(
                &credentials,
                1,
                "read_file",
                json!({"file_path":"alpha.json"})
            ))
            .await
            .status,
        TaskStatus::Success
    );
}

#[tokio::test]
async fn fp06_broker_excludes_recursion_coordination_and_background_tools() {
    let fixture = Fixture::new(PolicyAction::Allow).await;
    let mut broker = fixture.capture().await.unwrap();
    let credentials = broker.credentials();
    for name in [
        "execute_code",
        "spawn_helper_async",
        "delegate_to_agent",
        "trigger_automation",
        "bg_start",
        "browser_act",
        "mcp_unreviewed",
    ] {
        assert_eq!(
            broker
                .request(&frame(&credentials, 1, name, json!({})))
                .await
                .status,
            TaskStatus::Error,
            "allowed {name}"
        );
    }
    assert_eq!(fixture.receipts().await.len(), 1);
    assert!(!fixture.output_exists());
}

#[tokio::test]
async fn fp06_broker_native_write_has_own_durable_receipt_and_no_sequence_replay() {
    let fixture = Fixture::new(PolicyAction::Allow).await;
    let mut broker = fixture.capture().await.unwrap();
    let credentials = broker.credentials();
    let request = write_frame(&credentials);
    assert_eq!(broker.request(&request).await.status, TaskStatus::Success);
    let receipts = fixture.receipts().await;
    assert_eq!(receipts.len(), 2);
    assert_eq!(receipts[1].name, "write_file");
    assert_eq!(receipts[1].status, ActionStatus::Succeeded);
    assert_ne!(receipts[1].tool_call_id, outer_call().id);
    let snapshot: Value = serde_json::from_slice(
        &std::fs::read(fixture.root.path().join("control-plane.json")).unwrap(),
    )
    .unwrap();
    let saved = snapshot["runs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|run| run["id"] == fixture.run.run_id)
        .unwrap();
    assert_eq!(saved["actionReceipts"][1]["status"], "succeeded");
    std::fs::write(
        fixture.root.path().join(TARGET),
        b"owner changed this later",
    )
    .unwrap();
    assert_eq!(broker.request(&request).await.status, TaskStatus::Success);
    assert_eq!(
        std::fs::read(fixture.root.path().join(TARGET)).unwrap(),
        b"owner changed this later"
    );
    let changed = frame(
        &credentials,
        1,
        "write_file",
        json!({"file_path":TARGET,"content":"changed duplicate"}),
    );
    assert_eq!(broker.request(&changed).await.status, TaskStatus::Error);
    assert_eq!(fixture.receipts().await.len(), 2);
}

#[tokio::test]
async fn fp06_broker_nested_write_waits_for_its_own_native_approval() {
    let fixture = Fixture::new(PolicyAction::Ask).await;
    let mut broker = fixture.capture().await.unwrap();
    let request = write_frame(&broker.credentials());
    let worker = tokio::spawn(async move { broker.request(&request).await });
    let approval = pending_approvals(&fixture.coordinator, 1).await.remove(0);
    assert_eq!(approval.tool, "write_file");
    assert_ne!(approval.tool_call_id, outer_call().id);
    assert!(!fixture.output_exists());
    assert_eq!(fixture.receipts().await.len(), 1);
    fixture
        .coordinator
        .decide_approval(
            &approval.id,
            decision(ApprovalDecisionKind::AllowOnce, approval.revision),
        )
        .await
        .unwrap();
    assert_eq!(worker.await.unwrap().status, TaskStatus::Success);
    assert!(fixture.output_exists());
    assert_eq!(fixture.receipts().await[1].status, ActionStatus::Succeeded);
}

#[tokio::test]
async fn fp06_broker_approval_save_failure_performs_no_native_effect() {
    let fixture = Fixture::new(PolicyAction::Ask).await;
    let mut broker = fixture.capture().await.unwrap();
    let request = write_frame(&broker.credentials());
    let save = fixture.save_gate(true).await;
    let worker = tokio::spawn(async move { broker.request(&request).await });
    entered(&save).await;
    assert!(!fixture.output_exists());
    save.release.add_permits(1);
    assert_eq!(worker.await.unwrap().status, TaskStatus::Error);
    assert!(!fixture.output_exists());
    assert_eq!(fixture.receipts().await.len(), 1);
}

#[tokio::test]
async fn fp06_broker_wrapper_and_inner_revocation_during_receipt_save_refuse_dispatch() {
    for revoked in ["execute_code", "bash", "write_file"] {
        let fixture = Fixture::new(PolicyAction::Allow).await;
        let mut broker = fixture.capture().await.unwrap();
        let request = write_frame(&broker.credentials());
        let save = fixture.save_gate(false).await;
        let worker = tokio::spawn(async move { broker.request(&request).await });
        entered(&save).await;
        fixture.remove_grant(revoked).await;
        save.release.add_permits(1);
        assert_eq!(
            worker.await.unwrap().status,
            TaskStatus::Error,
            "revoked {revoked}"
        );
        assert!(!fixture.output_exists());
        assert_eq!(fixture.receipts().await[1].status, ActionStatus::Failed);
    }
}

#[tokio::test]
async fn fp06_broker_current_revocation_after_capture_is_not_a_frozen_config_grant() {
    let fixture = Fixture::new(PolicyAction::Allow).await;
    let mut broker = fixture.capture().await.unwrap();
    fixture.remove_grant("write_file").await;
    let result = broker.request(&write_frame(&broker.credentials())).await;
    assert_eq!(result.status, TaskStatus::Error);
    assert!(!fixture.output_exists());
    assert_eq!(fixture.receipts().await.len(), 1);
}

#[tokio::test]
async fn fp06_broker_outer_exec_deny_after_capture_and_during_intent_save_refuses_effect() {
    for during_save in [false, true] {
        let fixture = Fixture::new(PolicyAction::Allow).await;
        let mut broker = fixture.capture().await.unwrap();
        let request = write_frame(&broker.credentials());
        let save = if during_save {
            Some(fixture.save_gate(false).await)
        } else {
            None
        };
        if let Some(save) = save {
            let worker = tokio::spawn(async move { broker.request(&request).await });
            entered(&save).await;
            fixture.state.write().await.approvals.set_policy(
                &fixture.agent.id,
                ApprovalPolicy::default().with(RiskClass::Exec, PolicyAction::Deny),
            );
            save.release.add_permits(1);
            assert_eq!(worker.await.unwrap().status, TaskStatus::Error);
            assert_eq!(fixture.receipts().await[1].status, ActionStatus::Failed);
        } else {
            fixture.state.write().await.approvals.set_policy(
                &fixture.agent.id,
                ApprovalPolicy::default().with(RiskClass::Exec, PolicyAction::Deny),
            );
            assert_eq!(broker.request(&request).await.status, TaskStatus::Error);
            assert_eq!(fixture.receipts().await.len(), 1);
        }
        assert!(!fixture.output_exists());
    }
}

#[tokio::test]
async fn fp06_broker_uncertain_outcome_save_cannot_replay_the_file_effect() {
    let fixture = Fixture::new(PolicyAction::Allow).await;
    let mut broker = fixture.capture().await.unwrap();
    let request = write_frame(&broker.credentials());
    let replay = request.clone();
    let intent = fixture.save_gate(false).await;
    let worker = tokio::spawn(async move {
        let result = broker.request(&request).await;
        (broker, result)
    });
    entered(&intent).await;
    let outcome = fixture.save_gate(true).await;
    intent.release.add_permits(1);
    entered(&outcome).await;
    assert!(
        fixture.output_exists(),
        "real effect precedes outcome acknowledgement"
    );
    outcome.release.add_permits(1);
    let (mut broker, result) = worker.await.unwrap();
    assert_eq!(result.status, TaskStatus::Error);
    std::fs::write(
        fixture.root.path().join(TARGET),
        b"owner preserved after uncertain save",
    )
    .unwrap();
    assert_eq!(broker.request(&replay).await.status, TaskStatus::Error);
    assert_eq!(
        std::fs::read(fixture.root.path().join(TARGET)).unwrap(),
        b"owner preserved after uncertain save"
    );
    assert_eq!(fixture.receipts().await.len(), 2);
    assert_eq!(fixture.receipts().await[1].status, ActionStatus::Started);
    tokio::time::timeout(Duration::from_secs(5), fixture.journal.settle())
        .await
        .unwrap();
}

async fn final_authority_lock_liveness_race(expire: bool) {
    let deadline = expire.then(|| tokio::time::Instant::now() + Duration::from_secs(5));
    let fixture = Fixture::with_deadline(PolicyAction::Allow, deadline).await;
    let mut broker = fixture.capture().await.unwrap();
    let request = write_frame(&broker.credentials());
    let gate = fixture.journal.install_test_rpc_authority_read_gate();
    let worker = tokio::spawn(async move { broker.request(&request).await });
    tokio::time::timeout(Duration::from_secs(5), gate.entered.acquire())
        .await
        .expect("final RPC authority fence was never reached")
        .unwrap()
        .forget();
    if let Some(deadline) = deadline {
        assert!(
            tokio::time::Instant::now() < deadline,
            "deadline must still be live after all earlier checks"
        );
    }
    let guard = fixture.state.write().await;
    let run = guard.runs.get(&fixture.run.run_id).unwrap();
    assert_eq!(run.status, crate::runs::RunStatus::Running);
    assert!(run.stop.is_none());
    assert_eq!(
        run.action_receipts.len(),
        2,
        "inner Started was acknowledged before the final fence"
    );
    assert_eq!(run.action_receipts[1].name, "write_file");
    assert_eq!(run.action_receipts[1].status, ActionStatus::Started);
    assert!(!fixture.output_exists());
    gate.release.add_permits(1);
    tokio::time::timeout(Duration::from_secs(5), gate.blocked.acquire())
        .await
        .expect("actual final authority read did not block on the held writer")
        .unwrap()
        .forget();
    if let Some(deadline) = deadline {
        tokio::time::sleep_until(deadline).await;
        assert!(tokio::time::Instant::now() >= deadline);
        assert!(
            !fixture.context.cancel.as_ref().unwrap().is_cancelled(),
            "expiry is tested independently of cancellation"
        );
    } else {
        fixture.context.cancel.as_ref().unwrap().cancel();
    }
    let run = guard.runs.get(&fixture.run.run_id).unwrap();
    assert_eq!(run.status, crate::runs::RunStatus::Running);
    assert!(
        run.stop.is_none(),
        "no saved stop may let check_owner mask the liveness race"
    );
    assert!(
        !fixture.output_exists(),
        "effect must still be absent while the authority read is blocked"
    );
    drop(guard);
    let result = tokio::time::timeout(Duration::from_secs(5), worker)
        .await
        .expect("RPC did not settle after releasing the authority lock")
        .unwrap();
    assert!(
        !fixture.output_exists(),
        "write_file must refuse when liveness ended during the final authority-lock await"
    );
    assert_eq!(result.status, TaskStatus::Error);
    assert_eq!(fixture.receipts().await[1].status, ActionStatus::Failed);
}

#[tokio::test]
async fn fp06_broker_cancel_during_final_authority_lock_performs_no_write() {
    final_authority_lock_liveness_race(false).await;
}

#[tokio::test]
async fn fp06_broker_expiry_during_final_authority_lock_performs_no_write() {
    final_authority_lock_liveness_race(true).await;
}
