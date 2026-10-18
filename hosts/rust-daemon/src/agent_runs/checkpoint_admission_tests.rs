//! Actual native dispatch and exact selected recovery, in private temporary roots.
use super::test_support::{companion_config, gated_context, ledger_run, tool_input};
use super::{action_receipts::RunActionJournal, AgentRunCoordinator};
use crate::{
    approvals::{ApprovalPolicy, PolicyAction, RiskClass},
    runs::RunLink,
    state::DaemonState,
};
use anima_core::{CancelSignal, DataValue, TaskStatus, ToolCall};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};
use tokio::sync::{RwLock, Semaphore};

struct Fixture {
    _temp: tempfile::TempDir,
    workspace: PathBuf,
    store: PathBuf,
    coordinator: AgentRunCoordinator,
    run: RunLink,
    context: crate::tools::ToolExecutionContext,
}
impl Fixture {
    async fn new(checkpoint_grants: bool) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let sid = win_custody::current_user_sid_string().unwrap();
        win_custody::SecurityDescriptor::from_sddl(&format!(
            "D:P(A;OICI;FA;;;{sid})(A;OICI;FA;;;SY)"
        ))
        .unwrap()
        .apply_to_tree(temp.path(), true)
        .unwrap();
        let workspace = temp.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let store = temp.path().join("checkpoints");
        let mut state = crate::skills::test_support::with_workspace(DaemonState::new(), &workspace);
        state.checkpoint_root = Some(store.clone());
        let mut config = companion_config("checkpoint-owner");
        let mut tools = vec!["write_file", "edit_file", "multi_edit"];
        if checkpoint_grants {
            tools.extend(["checkpoint_list", "checkpoint_restore"]);
        }
        config.tools = Some(
            crate::tools::ToolRegistry::new()
                .resolve_descriptors(tools)
                .unwrap(),
        );
        let owner = state.create_agent(config).unwrap().state.id;
        state.approvals.set_policy(
            &owner,
            ApprovalPolicy::default()
                .with(RiskClass::Read, PolicyAction::Allow)
                .with(RiskClass::Write, PolicyAction::Allow),
        );
        let coordinator =
            AgentRunCoordinator::new(Arc::new(RwLock::new(state)), Arc::new(Semaphore::new(1)));
        let run = ledger_run(&coordinator, &owner, "chat:a6").await;
        let cancel = CancelSignal::new();
        let journal = RunActionJournal::new(
            &coordinator.state,
            coordinator.control_plane_transactions(),
            run.clone(),
            cancel.clone(),
            None,
        );
        let context = gated_context(&coordinator, &run, cancel)
            .await
            .with_action_journal(journal)
            .with_checkpoint_root(Some(store.clone()));
        Self {
            _temp: temp,
            workspace,
            store,
            coordinator,
            run,
            context,
        }
    }
    async fn execute(&self, call: ToolCall) -> anima_core::TaskResult<anima_core::Content> {
        let agent = self
            .coordinator
            .state
            .read()
            .await
            .get_agent(&self.run.agent_id)
            .unwrap()
            .state;
        self.context
            .clone()
            .execute_tool(agent, tool_input(&self.run.agent_id, "chat:a6"), call)
            .await
    }
}
fn call(id: &str, name: &str, args: &[(&str, &str)]) -> ToolCall {
    ToolCall {
        id: id.into(),
        name: name.into(),
        args: args
            .iter()
            .map(|(k, v)| (k.to_string(), DataValue::String(v.to_string())))
            .collect(),
    }
}

#[tokio::test]
async fn a6_protected_write_preimage_must_match_exact_owner_approved_revision() {
    use super::test_support::{decision, pending_approvals};
    use crate::approvals::ApprovalDecisionKind;
    for (original, drift) in [
        (Some("owner-approved-old"), true),
        (None, true),
        (Some("owner-approved-old"), false),
        (None, false),
    ] {
        let mut fixture = Fixture::new(true).await;
        let target = fixture.workspace.join("SOUL.md");
        if let Some(original) = original {
            std::fs::write(&target, original).unwrap();
        }
        if drift {
            fixture.context = fixture
                .context
                .clone()
                .with_checkpoint_preimage_hook(move || {
                    std::fs::write(target, "owner-concurrent-edit").unwrap();
                });
        }
        let context = fixture.context.clone();
        let agent = fixture
            .coordinator
            .state
            .read()
            .await
            .get_agent(&fixture.run.agent_id)
            .unwrap()
            .state;
        let pending = tokio::spawn(async move {
            context
                .execute_tool(
                    agent.clone(),
                    tool_input(&agent.id, "chat:a6"),
                    call(
                        "protected-race",
                        "write_file",
                        &[("file_path", "SOUL.md"), ("content", "model-replacement")],
                    ),
                )
                .await
        });
        let approval = pending_approvals(&fixture.coordinator, 1).await.remove(0);
        fixture
            .coordinator
            .decide_approval(
                &approval.id,
                decision(ApprovalDecisionKind::AllowOnce, approval.revision),
            )
            .await
            .unwrap();
        let result = tokio::time::timeout(std::time::Duration::from_secs(5), pending)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            result.status,
            if drift {
                TaskStatus::Error
            } else {
                TaskStatus::Success
            },
            "exact owner revision must admit only the unchanged preimage: {:?}",
            result.error
        );
        assert_eq!(
            std::fs::read(fixture.workspace.join("SOUL.md")).unwrap(),
            if drift {
                b"owner-concurrent-edit".as_slice()
            } else {
                b"model-replacement".as_slice()
            }
        );
        let rows =
            crate::filesystem_checkpoints::NativeCheckpointService::new(fixture.store.clone())
                .list(&fixture.workspace, &fixture.run)
                .unwrap();
        assert_eq!(
            rows.len(),
            usize::from(!drift),
            "only a matching approved revision may publish a checkpoint"
        );
    }
}

#[tokio::test]
async fn a6_native_edit_can_restore_exact_pre_normalized_bytes_with_explicit_grants() {
    let fixture = Fixture::new(true).await;
    let original = b"first\r\nsecond\r\n";
    std::fs::write(fixture.workspace.join("target.txt"), original).unwrap();
    std::fs::write(fixture.workspace.join("unrelated"), b"sentinel").unwrap();
    let edit = fixture
        .execute(call(
            "edit",
            "edit_file",
            &[
                ("file_path", "target.txt"),
                ("old_string", "second"),
                ("new_string", "changed"),
            ],
        ))
        .await;
    assert_eq!(edit.status, TaskStatus::Success, "{:?}", edit.error);
    assert_eq!(
        std::fs::read(fixture.workspace.join("target.txt")).unwrap(),
        b"first\nchanged\n"
    );
    let list = fixture.execute(call("list", "checkpoint_list", &[])).await;
    assert_eq!(list.status, TaskStatus::Success, "{:?}", list.error);
    let rows: serde_json::Value = serde_json::from_str(&list.data.unwrap().text).unwrap();
    let id = rows.as_array().unwrap()[0]["checkpoint_id"]
        .as_str()
        .unwrap();
    let restored = fixture
        .execute(call(
            "restore",
            "checkpoint_restore",
            &[("checkpoint_id", id)],
        ))
        .await;
    assert_eq!(restored.status, TaskStatus::Success, "{:?}", restored.error);
    assert_eq!(
        std::fs::read(fixture.workspace.join("target.txt")).unwrap(),
        original
    );
    assert_eq!(
        std::fs::read(fixture.workspace.join("unrelated")).unwrap(),
        b"sentinel"
    );
}

#[tokio::test]
async fn a6_denied_ungranted_or_missing_journal_has_no_checkpoint_effect() {
    let fixture = Fixture::new(false).await;
    std::fs::write(fixture.workspace.join("target.txt"), b"original").unwrap();
    fixture
        .coordinator
        .state
        .write()
        .await
        .approvals
        .set_policy(
            &fixture.run.agent_id,
            ApprovalPolicy::default().with(RiskClass::Write, PolicyAction::Deny),
        );
    let denied = fixture
        .execute(call(
            "denied",
            "write_file",
            &[("file_path", "target.txt"), ("content", "forbidden")],
        ))
        .await;
    assert_eq!(denied.status, TaskStatus::Error);
    assert!(
        !fixture.store.exists(),
        "denied call must not initialize recovery storage"
    );
    assert_eq!(
        fixture
            .execute(call(
                "ungranted",
                "checkpoint_restore",
                &[("checkpoint_id", "forged")]
            ))
            .await
            .status,
        TaskStatus::Error
    );
    assert_eq!(
        std::fs::read(fixture.workspace.join("target.txt")).unwrap(),
        b"original"
    );
    let mut state =
        crate::skills::test_support::with_workspace(DaemonState::new(), &fixture.workspace);
    let mut config = companion_config("direct");
    config.tools = Some(
        crate::tools::ToolRegistry::new()
            .resolve_descriptors(["write_file"])
            .unwrap(),
    );
    let agent = state.create_agent(config).unwrap().state;
    let context = state
        .tool_execution_context()
        .with_checkpoint_root(Some(fixture.store.clone()));
    let result = context
        .execute_tool(
            agent.clone(),
            tool_input(&agent.id, "chat:direct"),
            call(
                "missing",
                "write_file",
                &[("file_path", "target.txt"), ("content", "forbidden")],
            ),
        )
        .await;
    assert_eq!(
        result.status,
        TaskStatus::Error,
        "enabled writes require authentic native journal"
    );
    assert!(!fixture.store.exists());
}

#[tokio::test]
async fn a6_native_dispatch_rechecks_revocation_stop_and_root_after_checkpoint_save() {
    for change in ["grant", "stop", "root"] {
        let mut fixture = Fixture::new(false).await;
        std::fs::write(fixture.workspace.join("target.txt"), b"original").unwrap();
        let state = fixture.coordinator.state.clone();
        let run = fixture.run.clone();
        let new_root = fixture._temp.path().join("replacement-workspace");
        std::fs::create_dir(&new_root).unwrap();
        std::fs::write(new_root.join("target.txt"), b"other workspace").unwrap();
        fixture.context = fixture
            .context
            .clone()
            .with_checkpoint_validation_hook(move || {
                let mut guard = state
                    .try_write()
                    .expect("no daemon lock spans checkpoint file I/O");
                match change {
                    "grant" => {
                        let runtime = guard.agents.get_mut(&run.agent_id).unwrap();
                        let mut config = runtime.config().clone();
                        config
                            .tools
                            .as_mut()
                            .unwrap()
                            .retain(|tool| tool.name != "write_file");
                        runtime.replace_config(config);
                    }
                    "stop" => {
                        guard.runs.get_mut(&run.run_id).unwrap().stop =
                            Some(crate::runs::RunStopRequest {
                                requested_at_ms: anima_core::primitives::now_millis(),
                            })
                    }
                    "root" => guard.workspace.as_mut().unwrap().root_path = new_root,
                    _ => unreachable!(),
                }
            });
        let result = fixture
            .execute(call(
                "raced",
                "write_file",
                &[("file_path", "target.txt"), ("content", "forbidden")],
            ))
            .await;
        assert_eq!(
            result.status,
            TaskStatus::Error,
            "{change}: {:?}",
            result.error
        );
        assert_eq!(
            std::fs::read(fixture.workspace.join("target.txt")).unwrap(),
            b"original"
        );
        assert_eq!(
            std::fs::read(
                fixture
                    ._temp
                    .path()
                    .join("replacement-workspace/target.txt")
            )
            .unwrap(),
            b"other workspace"
        );
    }
}

#[tokio::test]
async fn a6_ordinary_followup_parent_is_provenance_not_a_live_delegation_grant() {
    let fixture = Fixture::new(false).await;
    std::fs::write(fixture.workspace.join("target.txt"), b"original").unwrap();
    let parent = ledger_run(&fixture.coordinator, &fixture.run.agent_id, "chat:prior").await;
    {
        let mut state = fixture.coordinator.state.write().await;
        state.runs.get_mut(&parent.run_id).unwrap().status = crate::runs::RunStatus::Completed;
        state
            .runs
            .get_mut(&fixture.run.run_id)
            .unwrap()
            .parent_run_id = Some(parent.run_id);
    }
    let result = fixture
        .execute(call(
            "followup-write",
            "write_file",
            &[
                ("file_path", "target.txt"),
                ("content", "reviewed followup"),
            ],
        ))
        .await;
    assert_eq!(result.status, TaskStatus::Success, "{:?}", result.error);
    assert_eq!(
        std::fs::read(fixture.workspace.join("target.txt")).unwrap(),
        b"reviewed followup"
    );
}
