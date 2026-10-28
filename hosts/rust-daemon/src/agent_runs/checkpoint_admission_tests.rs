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
        let mut tools = vec![
            "write_file",
            "edit_file",
            "multi_edit",
            "write_document",
            "read_document_source",
        ];
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

fn document_call(id: &str, title: &str, expected: Option<&str>) -> ToolCall {
    let mut call = call(id, "write_document", &[("file_path", "report.docx")]);
    call.args.insert(
        "document".into(),
        crate::tools::generated_documents::schema_data(serde_json::json!({
            "format":"docx", "title":title,
            "blocks":[{"type":"paragraph", "text":"Checkpoint body"}]
        })),
    );
    if let Some(expected) = expected {
        call.args
            .insert("expected_sha256".into(), DataValue::String(expected.into()));
    }
    call
}

/// The real final-authority guard must exclude daemon source/project/profile
/// mutations until synchronous publication and readback have both settled.
#[tokio::test]
async fn native_publication_fence_blocks_mutations_and_releases_on_errors() {
    struct Probe {
        guard: Option<tokio::sync::OwnedRwLockReadGuard<DaemonState>>,
        release: std::sync::mpsc::SyncSender<()>,
        writer: Option<std::thread::JoinHandle<()>>,
        target: PathBuf,
        expected: &'static [u8],
    }
    impl Drop for Probe {
        fn drop(&mut self) {
            assert_eq!(std::fs::read(&self.target).unwrap(), self.expected);
            self.guard.take();
            self.release.send(()).unwrap();
            self.writer.take().unwrap().join().unwrap();
        }
    }
    for change in ["source", "project", "profile"] {
        for refuse_after_authority in [false, true] {
            let fixture = Fixture::new(false).await;
            let project = {
                let mut state = fixture.coordinator.state.write().await;
                let (project, _) = state
                    .projects
                    .create(
                        crate::projects::ProjectCreateRequest {
                            request_key: "fence-project".into(),
                            title: "Fence".into(),
                            member_agent_ids: vec![fixture.run.agent_id.clone()],
                            instructions: String::new(),
                        },
                        1,
                    )
                    .unwrap();
                project.id
            };
            let call = call(
                "publication-fence",
                "write_file",
                &[("file_path", "target.txt"), ("content", "reviewed")],
            );
            let journal = RunActionJournal::new(
                &fixture.coordinator.state,
                fixture.coordinator.control_plane_transactions(),
                fixture.run.clone(),
                CancelSignal::new(),
                None,
            );
            journal.begin(&call).await.unwrap();
            let target = fixture.workspace.join("target.txt");
            let state = fixture.coordinator.state.clone();
            let run = fixture.run.clone();
            let expected: &'static [u8] = if refuse_after_authority {
                b"concurrent file"
            } else {
                b"reviewed"
            };
            let service =
                crate::filesystem_checkpoints::NativeCheckpointService::new(fixture.store.clone());
            let result = service.mutate_with_authority(
                &fixture.workspace,
                &fixture.run,
                &call,
                "target.txt",
                |_| Ok(b"reviewed".to_vec()),
                || {
                    let guard = journal.checkpoint_mutation_authority(
                        &fixture.workspace,
                        &fixture.store,
                        &call,
                        false,
                    )?;
                    let (blocked_tx, blocked_rx) = std::sync::mpsc::sync_channel(0);
                    let (release, released) = std::sync::mpsc::sync_channel(0);
                    let writer_state = state.clone();
                    let writer_run = run.clone();
                    let writer_project = project.clone();
                    let writer = std::thread::spawn(move || {
                        blocked_tx.send(writer_state.try_write().is_err()).unwrap();
                        released
                            .recv_timeout(std::time::Duration::from_secs(5))
                            .unwrap();
                        let mut state = writer_state.blocking_write();
                        match change {
                            "source" => {
                                state
                                    .sessions
                                    .remove(&writer_run.agent_id, &writer_run.session_id);
                            }
                            "project" => {
                                state.projects.delete(&writer_project, 1, 2).unwrap();
                            }
                            "profile" => state.checkpoint_root = None,
                            _ => unreachable!(),
                        }
                    });
                    assert!(
                        blocked_rx
                            .recv_timeout(std::time::Duration::from_secs(5))
                            .unwrap(),
                        "{change} crossed native publication authority"
                    );
                    if refuse_after_authority {
                        std::fs::write(&target, expected).unwrap();
                    }
                    Ok(Probe {
                        guard: Some(guard),
                        release,
                        writer: Some(writer),
                        target: target.clone(),
                        expected,
                    })
                },
            );
            assert_eq!(result.is_err(), refuse_after_authority, "{change}");
            if let Ok(receipt) = result {
                use sha2::{Digest, Sha256};
                assert_eq!(
                    receipt.observed_sha256,
                    hex::encode(Sha256::digest(expected))
                );
            }
            // Acquiring the state again also proves error unwinding released
            // the guard before any asynchronous journal settlement.
            let state = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                fixture.coordinator.state.write(),
            )
            .await
            .unwrap();
            match change {
                "source" => assert!(!state.sessions.contains(&run.agent_id, &run.session_id)),
                "project" => assert!(state.projects.records().next().is_none()),
                "profile" => assert!(state.checkpoint_root.is_none()),
                _ => unreachable!(),
            }
        }
    }
}

#[tokio::test]
async fn finished_documents_binary_create_revise_and_undo_are_checkpointed() {
    use sha2::{Digest, Sha256};
    let fixture = Fixture::new(true).await;
    let created = fixture
        .execute(document_call("document-create", "Draft", None))
        .await;
    assert_eq!(created.status, TaskStatus::Success, "{:?}", created.error);
    let original = std::fs::read(fixture.workspace.join("report.docx")).unwrap();
    assert!(original.starts_with(b"PK"));
    let hash = hex::encode(Sha256::digest(&original));
    let revised = fixture
        .execute(document_call("document-revise", "Revision", Some(&hash)))
        .await;
    assert_eq!(revised.status, TaskStatus::Success, "{:?}", revised.error);
    let list = fixture
        .execute(call("document-list", "checkpoint_list", &[]))
        .await;
    let rows: serde_json::Value = serde_json::from_str(&list.data.unwrap().text).unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 2);
    let id = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["native_tool_call_id"] == "document-revise")
        .unwrap()["checkpoint_id"]
        .as_str()
        .unwrap();
    let restored = fixture
        .execute(call(
            "document-undo",
            "checkpoint_restore",
            &[("checkpoint_id", id)],
        ))
        .await;
    assert_eq!(restored.status, TaskStatus::Success, "{:?}", restored.error);
    assert_eq!(
        std::fs::read(fixture.workspace.join("report.docx")).unwrap(),
        original
    );
    let creation = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["native_tool_call_id"] == "document-create")
        .unwrap()["checkpoint_id"]
        .as_str()
        .unwrap();
    let removed = fixture
        .execute(call(
            "document-undo-create",
            "checkpoint_restore",
            &[("checkpoint_id", creation)],
        ))
        .await;
    assert_eq!(removed.status, TaskStatus::Success, "{:?}", removed.error);
    assert!(!fixture.workspace.join("report.docx").exists());
}

#[tokio::test]
async fn finished_documents_hash_is_checked_at_checkpoint_capture() {
    use sha2::{Digest, Sha256};
    let mut fixture = Fixture::new(true).await;
    let target = fixture.workspace.join("report.docx");
    assert_eq!(
        fixture
            .execute(document_call("initial-document", "Draft", None))
            .await
            .status,
        TaskStatus::Success
    );
    let hash = hex::encode(Sha256::digest(std::fs::read(&target).unwrap()));
    let drift = b"owner concurrent replacement";
    fixture.context = fixture
        .context
        .clone()
        .with_checkpoint_preimage_hook(move || {
            std::fs::write(target, drift).unwrap();
        });
    let refused = fixture
        .execute(document_call("raced-document", "Revision", Some(&hash)))
        .await;
    assert_eq!(refused.status, TaskStatus::Error);
    assert_eq!(
        std::fs::read(fixture.workspace.join("report.docx")).unwrap(),
        drift
    );
}

#[tokio::test]
async fn finished_documents_creation_collision_at_checkpoint_capture_is_preserved() {
    let mut fixture = Fixture::new(true).await;
    let target = fixture.workspace.join("report.docx");
    fixture.context = fixture
        .context
        .clone()
        .with_checkpoint_preimage_hook(move || {
            std::fs::write(target, b"owner concurrent creation").unwrap();
        });
    let refused = fixture
        .execute(document_call("new-raced-document", "Draft", None))
        .await;
    assert_eq!(refused.status, TaskStatus::Error);
    assert_eq!(
        std::fs::read(fixture.workspace.join("report.docx")).unwrap(),
        b"owner concurrent creation"
    );
}

#[tokio::test]
async fn finished_documents_strict_write_approval_precedes_any_file_or_checkpoint() {
    use super::test_support::{decision, pending_approvals};
    use crate::approvals::ApprovalDecisionKind;
    let fixture = Fixture::new(true).await;
    fixture.coordinator.state.write().await.autonomy.set(
        crate::approvals::autonomy::AgentAutonomy {
            agent_id: fixture.run.agent_id.clone(),
            mode: crate::approvals::autonomy::AutonomyMode::Strict,
            smart_check: false,
            deny_patterns: Vec::new(),
            smart_policy: String::new(),
        },
    );
    fixture
        .coordinator
        .state
        .write()
        .await
        .approvals
        .set_policy(
            &fixture.run.agent_id,
            ApprovalPolicy::default().with(RiskClass::Write, PolicyAction::Ask),
        );
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
                document_call("strict-document", "Draft", None),
            )
            .await
    });
    let approval = pending_approvals(&fixture.coordinator, 1).await.remove(0);
    assert_eq!(approval.tool, "write_document");
    assert!(!fixture.workspace.join("report.docx").exists());
    assert!(!fixture.store.exists());
    fixture
        .coordinator
        .decide_approval(
            &approval.id,
            decision(ApprovalDecisionKind::Deny, approval.revision),
        )
        .await
        .unwrap();
    assert_eq!(pending.await.unwrap().status, TaskStatus::Error);
    assert!(!fixture.workspace.join("report.docx").exists());
}

#[tokio::test]
async fn finished_documents_default_publication_rechecks_current_run_grant_and_workspace() {
    for change in ["grant", "stop", "root"] {
        let mut fixture = Fixture::new(false).await;
        fixture.coordinator.state.write().await.checkpoint_root = None;
        fixture.context = fixture.context.clone().with_checkpoint_root(None);
        let state = fixture.coordinator.state.clone();
        let run = fixture.run.clone();
        let replacement = fixture._temp.path().join("other-workspace");
        std::fs::create_dir(&replacement).unwrap();
        let replacement_hook = replacement.clone();
        fixture.context = fixture
            .context
            .clone()
            .with_checkpoint_validation_hook(move || {
                let mut guard = state.try_write().unwrap();
                match change {
                    "grant" => {
                        let agent = guard.agents.get_mut(&run.agent_id).unwrap();
                        let mut config = agent.config().clone();
                        config
                            .tools
                            .as_mut()
                            .unwrap()
                            .retain(|tool| tool.name != "write_document");
                        agent.replace_config(config);
                    }
                    "stop" => {
                        guard.runs.get_mut(&run.run_id).unwrap().stop =
                            Some(crate::runs::RunStopRequest {
                                requested_at_ms: anima_core::primitives::now_millis(),
                            })
                    }
                    "root" => guard.workspace.as_mut().unwrap().root_path = replacement_hook,
                    _ => unreachable!(),
                }
            });
        let refused = fixture
            .execute(document_call("default-raced-document", "Draft", None))
            .await;
        assert_eq!(refused.status, TaskStatus::Error, "{change}");
        assert!(!fixture.workspace.join("report.docx").exists());
        assert!(!replacement.join("report.docx").exists());
        assert!(!fixture.store.exists());
    }
}

#[tokio::test]
async fn finished_documents_concurrent_default_revisions_cannot_both_publish_the_same_hash() {
    use sha2::{Digest, Sha256};
    let mut fixture = Fixture::new(false).await;
    fixture.coordinator.state.write().await.checkpoint_root = None;
    fixture.context = fixture.context.clone().with_checkpoint_root(None);
    let created = fixture
        .execute(document_call("default-original", "Draft", None))
        .await;
    assert_eq!(created.status, TaskStatus::Success, "{:?}", created.error);
    let hash = hex::encode(Sha256::digest(
        std::fs::read(fixture.workspace.join("report.docx")).unwrap(),
    ));
    let (first, second) = tokio::join!(
        fixture.execute(document_call("default-first", "First", Some(&hash))),
        fixture.execute(document_call("default-second", "Second", Some(&hash))),
    );
    assert_eq!(
        [first.status, second.status]
            .iter()
            .filter(|status| **status == TaskStatus::Success)
            .count(),
        1,
        "{:?} {:?}",
        first.error,
        second.error
    );
    let bytes = std::fs::read(fixture.workspace.join("report.docx")).unwrap();
    let source = anima_document_content::inspect_generated_document(
        anima_document_content::GeneratedFormat::Docx,
        &bytes,
    )
    .unwrap();
    assert!(matches!(source.title.as_str(), "First" | "Second"));
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
