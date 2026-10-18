//! Lifecycle effects must use the source authority current at the mutation.
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::{RwLock, Semaphore};

use super::*;
use crate::agent_runs::test_support::{
    companion_config, coordinator_state, decision, gated_context, ledger_run, pending_approvals,
    tool_input,
};
use crate::agent_runs::AgentRunCoordinator;
use crate::app::SharedDaemonState;
use crate::control_plane_store::ControlPlaneStoreConfig;
use crate::live::LiveSubscription;
use crate::runs::{RunLink, RunSource, RunStatus, RunStopRequest};
use crate::schedules::test_automation;
use crate::sessions::test_support::within;
use crate::skills::test_support::{temp_workspace, with_workspace};
use crate::state::DaemonState;

const TOOLS: [&str; 5] = [
    "create_automation",
    "pause_automation",
    "update_automation",
    "resume_automation",
    "delete_automation",
];
const SESSION: &str = "chat:fp04-lifecycle-authority";

struct Fixture {
    root: PathBuf,
    snapshot: PathBuf,
    coordinator: AgentRunCoordinator,
    state: SharedDaemonState,
    agent: AgentState,
    context: ToolExecutionContext,
    link: RunLink,
    original: ScheduledPromptRecord,
    cancel: anima_core::CancelSignal,
}

impl Fixture {
    async fn new() -> Self {
        let root = temp_workspace("fp04-lifecycle-authority");
        let snapshot = root.join("control-plane.json");
        let mut daemon = with_workspace(DaemonState::new(), &root);
        daemon.set_control_plane_store(Some(ControlPlaneStoreConfig::Json(snapshot.clone())));
        let mut config = companion_config("Lifecycle authority");
        config.tools = Some(
            crate::tools::ToolRegistry::new()
                .resolve_descriptors(TOOLS)
                .unwrap(),
        );
        let agent = daemon.create_agent(config).unwrap().state;
        let mut original = test_automation(&agent.id, "mine");
        original.enabled = false;
        original.trigger = ScheduleTrigger::Interval {
            interval_ms: 600_000,
        };
        daemon
            .schedules
            .insert(original.id.clone(), original.clone());
        let coordinator =
            AgentRunCoordinator::new(Arc::new(RwLock::new(daemon)), Arc::new(Semaphore::new(1)));
        let link = ledger_run(&coordinator, &agent.id, SESSION).await;
        let state = coordinator_state(&coordinator);
        let cancel = anima_core::CancelSignal::new();
        let context = state
            .read()
            .await
            .tool_execution_context()
            .with_team(coordinator.clone(), false)
            .with_run_link(Some(link.clone()))
            .with_cancel(Some(cancel.clone()));
        let persist = state.write().await.control_plane_persist_request();
        persist.save().await.unwrap();
        Self {
            root,
            snapshot,
            coordinator,
            state,
            agent,
            context,
            link,
            original,
            cancel,
        }
    }

    fn invoke(&self, name: &str) -> BoxFuture<'static, TaskResult<Content>> {
        let mut args = BTreeMap::from([("id".into(), DataValue::String("mine".into()))]);
        if name == "update_automation" {
            args.insert(
                "prompt".into(),
                DataValue::String("This must require current authority".into()),
            );
        }
        if name == "create_automation" {
            args = BTreeMap::from([
                (
                    "prompt".into(),
                    DataValue::String("Current authority required".into()),
                ),
                (
                    "schedule".into(),
                    DataValue::String("every 10 minutes".into()),
                ),
            ]);
        }
        let call = ToolCall {
            id: format!("{name}-authority"),
            name: name.into(),
            args,
        };
        let handler = match name {
            "create_automation" => create_automation,
            "pause_automation" => pause_automation,
            "update_automation" => update_automation,
            "resume_automation" => resume_automation,
            "delete_automation" => delete_automation,
            _ => unreachable!(),
        };
        handler(
            self.context.clone(),
            self.agent.clone(),
            tool_input(&self.agent.id, SESSION),
            call,
        )
    }

    async fn assert_unchanged(&self, bytes: &[u8], stream: &mut LiveSubscription) {
        assert_eq!(self.state.read().await.schedules.len(), 1);
        assert_eq!(self.state.read().await.schedules["mine"], self.original);
        assert_eq!(
            std::fs::read(&self.snapshot).unwrap(),
            bytes,
            "a refusal must not save any snapshot"
        );
        tokio::select! {
            biased;
            _ = stream.next() => panic!("a refused mutation must publish no automation event"),
            _ = std::future::ready(()) => {}
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

async fn authority_race(name: &str, revoke: bool) {
    authority_race_with_policy(name, revoke, false).await;
}

async fn authority_race_with_policy(name: &str, revoke: bool, deny_policy: bool) {
    authority_race_with_cancel(name, revoke, deny_policy, false).await;
}

async fn authority_race_with_cancel(name: &str, revoke: bool, deny_policy: bool, cancelled: bool) {
    let mut fixture = Fixture::new().await;
    if name == "pause_automation" {
        fixture.original.enabled = true;
        let persist = {
            let mut state = fixture.state.write().await;
            state
                .schedules
                .insert("mine".into(), fixture.original.clone());
            state.control_plane_persist_request()
        };
        persist.save().await.unwrap();
    }
    let bytes = std::fs::read(&fixture.snapshot).unwrap();
    let mut stream = fixture
        .state
        .read()
        .await
        .live
        .subscribe(&fixture.agent.id)
        .unwrap();
    let transaction = fixture.coordinator.control_plane_transaction().await;
    let mut pending = fixture.invoke(name);
    // Poll the real handler through its preflight and into the spawned mutation
    // task. Its actual transaction cannot run until the owner releases it.
    tokio::select! {
        biased;
        _ = &mut pending => panic!("the mutation cannot finish while its transaction is held"),
        _ = std::future::ready(()) => {}
    }
    {
        let mut state = fixture.state.write().await;
        if cancelled {
            fixture.cancel.cancel();
        } else if deny_policy {
            state.approvals.set_policy(
                &fixture.agent.id,
                crate::approvals::ApprovalPolicy::default().with(
                    crate::approvals::RiskClass::Write,
                    crate::approvals::PolicyAction::Deny,
                ),
            );
        } else if revoke {
            let runtime = state.agents.get_mut(&fixture.agent.id).unwrap();
            let mut config = runtime.config().clone();
            config
                .tools
                .as_mut()
                .unwrap()
                .retain(|tool| tool.name != name);
            runtime.replace_config(config);
        } else {
            state.runs.get_mut(&fixture.link.run_id).unwrap().stop = Some(RunStopRequest {
                requested_at_ms: now_millis(),
            });
        }
    }
    drop(transaction);
    let result = within("the obsolete lifecycle mutation to refuse", pending).await;
    assert!(
        result.error.is_some(),
        "{name} must refuse after {}",
        if revoke { "revocation" } else { "stop" }
    );
    fixture.assert_unchanged(&bytes, &mut stream).await;
}

#[tokio::test]
async fn a8_create_and_pause_recheck_authority_at_mutation_transaction() {
    for name in ["create_automation", "pause_automation"] {
        for revoke in [true, false] {
            authority_race(name, revoke).await;
        }
    }
}

#[tokio::test]
async fn a8_automation_lifecycle_rechecks_policy_at_mutation_transaction() {
    for name in TOOLS {
        authority_race_with_policy(name, false, true).await;
    }
}

#[tokio::test]
async fn a8_automation_lifecycle_rechecks_cancel_at_mutation_transaction() {
    for name in TOOLS {
        authority_race_with_cancel(name, false, false, true).await;
    }
}

#[tokio::test]
async fn a8_create_and_pause_real_dispatch_honors_actual_allow_once_approval() {
    use crate::approvals::{ApprovalDecisionKind, ApprovalPolicy, PolicyAction, RiskClass};
    for name in ["create_automation", "pause_automation"] {
        let mut fixture = Fixture::new().await;
        if name == "pause_automation" {
            fixture.original.enabled = true;
            fixture
                .state
                .write()
                .await
                .schedules
                .insert("mine".into(), fixture.original.clone());
        }
        fixture.state.write().await.approvals.set_policy(
            &fixture.agent.id,
            ApprovalPolicy::default().with(RiskClass::Write, PolicyAction::Ask),
        );
        let context = gated_context(&fixture.coordinator, &fixture.link, fixture.cancel.clone())
            .await
            .with_action_journal(crate::agent_runs::action_receipts::RunActionJournal::new(
                &fixture.state,
                fixture.coordinator.control_plane_transactions(),
                fixture.link.clone(),
                fixture.cancel.clone(),
                None,
            ));
        let call = lifecycle_call(name);
        let owner = fixture.agent.clone();
        let input = tool_input(&owner.id, SESSION);
        let pending = tokio::spawn(async move { context.execute_tool(owner, input, call).await });
        let approvals = pending_approvals(&fixture.coordinator, 1).await;
        assert_eq!(approvals.len(), 1);
        assert_eq!(approvals[0].tool, name);
        assert_eq!(
            fixture.state.read().await.schedules["mine"],
            fixture.original
        );
        assert_eq!(fixture.state.read().await.schedules.len(), 1);
        fixture
            .coordinator
            .decide_approval(
                &approvals[0].id,
                decision(ApprovalDecisionKind::AllowOnce, approvals[0].revision),
            )
            .await
            .unwrap();
        let result = within("actually approved lifecycle dispatch to finish", pending)
            .await
            .unwrap();
        assert_eq!(
            result.status,
            anima_core::TaskStatus::Success,
            "{name}: {result:?}"
        );
        let state = fixture.state.read().await;
        if name == "create_automation" {
            assert_eq!(state.schedules.len(), 2);
        } else {
            assert!(!state.schedules["mine"].enabled);
        }
        let receipt = state
            .runs
            .get(&fixture.link.run_id)
            .unwrap()
            .action_receipts
            .iter()
            .find(|receipt| receipt.tool_call_id == format!("{name}-authority"))
            .unwrap();
        assert_eq!(receipt.name, name);
        assert_eq!(receipt.status, crate::runs::ActionStatus::Succeeded);
        assert!(state.approvals.pending().is_empty());
    }
}

fn lifecycle_call(name: &str) -> ToolCall {
    let args = if name == "create_automation" {
        BTreeMap::from([
            (
                "prompt".into(),
                DataValue::String("Current authority required".into()),
            ),
            (
                "schedule".into(),
                DataValue::String("every 10 minutes".into()),
            ),
        ])
    } else {
        let mut args = BTreeMap::from([("id".into(), DataValue::String("mine".into()))]);
        if name == "update_automation" {
            args.insert(
                "prompt".into(),
                DataValue::String("Current authority required".into()),
            );
        }
        args
    };
    ToolCall {
        id: format!("{name}-authority"),
        name: name.into(),
        args,
    }
}

#[tokio::test]
async fn a8_lifecycle_allow_to_ask_while_queued_requires_an_actual_owner_approval() {
    use crate::approvals::{ApprovalPolicy, PolicyAction, RiskClass};
    for name in TOOLS {
        let mut fixture = Fixture::new().await;
        if name == "pause_automation" {
            fixture.original.enabled = true;
            fixture
                .state
                .write()
                .await
                .schedules
                .insert("mine".into(), fixture.original.clone());
        }
        fixture.state.write().await.approvals.set_policy(
            &fixture.agent.id,
            ApprovalPolicy::default().with(RiskClass::Write, PolicyAction::Allow),
        );
        // No journal wait intervenes: real dispatch reaches the same mutation
        // boundary after an Allow verdict, without an owner approval.
        let context =
            gated_context(&fixture.coordinator, &fixture.link, fixture.cancel.clone()).await;
        fixture
            .state
            .write()
            .await
            .control_plane_persist_request()
            .save()
            .await
            .unwrap();
        let bytes = std::fs::read(&fixture.snapshot).unwrap();
        let mut stream = fixture
            .state
            .read()
            .await
            .live
            .subscribe(&fixture.agent.id)
            .unwrap();
        let transaction = fixture.coordinator.control_plane_transaction().await;
        let owner = fixture.agent.clone();
        let input = tool_input(&owner.id, SESSION);
        let call = lifecycle_call(name);
        let mut pending = Box::pin(context.execute_tool(owner, input, call));
        tokio::select! {
            biased;
            _ = &mut pending => panic!("lifecycle transaction is held"),
            _ = std::future::ready(()) => {}
        }
        fixture.state.write().await.approvals.set_policy(
            &fixture.agent.id,
            ApprovalPolicy::default().with(RiskClass::Write, PolicyAction::Ask),
        );
        drop(transaction);
        let result = within("unapproved lifecycle dispatch to refuse", pending).await;
        assert_eq!(
            result.status,
            anima_core::TaskStatus::Error,
            "{name}: {result:?}"
        );
        assert!(fixture.state.read().await.approvals.pending().is_empty());
        fixture.assert_unchanged(&bytes, &mut stream).await;
    }
}

#[tokio::test]
async fn fp04_update_rechecks_authority_at_its_mutation_transaction() {
    for revoke in [true, false] {
        authority_race("update_automation", revoke).await;
    }
}

#[tokio::test]
async fn fp04_resume_rechecks_authority_at_its_mutation_transaction() {
    for revoke in [true, false] {
        authority_race("resume_automation", revoke).await;
    }
}

#[tokio::test]
async fn fp04_delete_rechecks_authority_at_its_mutation_transaction() {
    for revoke in [true, false] {
        authority_race("delete_automation", revoke).await;
    }
}

#[tokio::test]
async fn fp04_lifecycle_refuses_missing_foreign_terminal_and_current_helper_authority() {
    for name in TOOLS {
        for case in [
            "missing-link",
            "missing-run",
            "foreign-link",
            "foreign-run",
            "session",
            "terminal",
            "current-helper",
            "helper",
        ] {
            let mut fixture = Fixture::new().await;
            {
                let mut state = fixture.state.write().await;
                let mut link = fixture.link.clone();
                match case {
                    "missing-link" => fixture.context = fixture.context.clone().with_run_link(None),
                    "missing-run" => link.run_id = "missing".into(),
                    "foreign-link" => link.agent_id = "foreign".into(),
                    "foreign-run" => {
                        state.runs.get_mut(&link.run_id).unwrap().agent_id = "foreign".into()
                    }
                    "session" => link.session_id = "chat:foreign".into(),
                    "terminal" => {
                        state.runs.get_mut(&link.run_id).unwrap().status = RunStatus::Completed
                    }
                    "current-helper" | "helper" => {
                        let runtime = state.agents.get_mut(&fixture.agent.id).unwrap();
                        let mut config = runtime.config().clone();
                        let additional = &mut config.settings.as_mut().unwrap().additional;
                        additional
                            .insert("workspaceRole".into(), DataValue::String("helper".into()));
                        additional
                            .insert("parentAgentId".into(), DataValue::String("parent".into()));
                        runtime.replace_config(config.clone());
                        if case == "helper" {
                            fixture.agent.config = config;
                        }
                    }
                    _ => unreachable!(),
                }
                if case != "missing-link" {
                    fixture.context = fixture.context.clone().with_run_link(Some(link));
                }
            }
            let bytes = std::fs::read(&fixture.snapshot).unwrap();
            let mut stream = fixture
                .state
                .read()
                .await
                .live
                .subscribe(&fixture.agent.id)
                .unwrap();
            let result = fixture.invoke(name).await;
            assert!(result.error.is_some(), "{name} must refuse {case}");
            fixture.assert_unchanged(&bytes, &mut stream).await;
        }
    }
}

#[tokio::test]
async fn fp04_lifecycle_allows_live_granted_non_direct_sources() {
    for name in TOOLS {
        let fixture = Fixture::new().await;
        {
            let mut state = fixture.state.write().await;
            let run = state.runs.get_mut(&fixture.link.run_id).unwrap();
            run.source = RunSource::Schedule;
            run.parent_run_id = Some("parent".into());
        }
        let result = fixture.invoke(name).await;
        assert!(result.error.is_none(), "{name}: {:?}", result.error);
    }
}
