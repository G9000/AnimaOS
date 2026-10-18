//! Completion uses committed native results, settlement events and the same
//! ledger/queues as ordinary turns. No timer, provider or external service.
use super::*;
use crate::runs::HelperCompletionState;

fn chat_launch(id: &str) -> ToolCall {
    let mut call = launch(id, "Return the literal recorded evidence");
    call.args
        .insert("completion".into(), DataValue::String("chat".into()));
    call
}

async fn chat_child(harness: &Harness, id: &str) -> Value {
    success_json(harness.call(chat_launch(id)).await)
}

fn disposition(record: &RunRecord) -> &HelperCompletionState {
    &record.async_helper.as_ref().unwrap().completion
}

/// A genuine native child already committed its result. Freeze execution
/// capacity, then establish the valid pre-pump state without depending on the
/// production settlement hook being implemented yet.
async fn ready() -> (Harness, String, String, String) {
    ready_with_model(ScriptedModel::new(vec![Step::Text(vec![
        "recorded child result",
    ])]))
    .await
}

async fn ready_with_model(model: Arc<ScriptedModel>) -> (Harness, String, String, String) {
    let harness = Harness::with_model(1, model).await;
    let handle = harness.launch("committed-fixture", "Return evidence").await;
    let child = handle["runId"].as_str().unwrap().to_string();
    wait_for(&harness.coordinator, &child, RunStatus::Completed).await;
    drained(&harness, &handle).await;
    harness
        .permits
        .clone()
        .acquire_owned()
        .await
        .unwrap()
        .forget();
    let parent = harness.context.run_link.as_ref().unwrap().run_id.clone();
    let reserved = format!("run_{}", uuid::Uuid::new_v4());
    {
        let mut guard = harness.coordinator.state.write().await;
        guard
            .runs
            .get_mut(&parent)
            .unwrap()
            .finish(RunStatus::Completed, None, 3);
        let child_record = guard.runs.get_mut(&child).unwrap();
        assert!(
            child_record.reply_message_id.is_some(),
            "fixture requires committed source bytes"
        );
        child_record.async_helper.as_mut().unwrap().completion =
            HelperCompletionState::AwaitingParent {
                reserved_run_id: reserved.clone(),
            };
        // This synthetic settled fixture represents already-published source
        // outcomes. Separate publication regressions remove exactly one proof.
        child_record.terminal_published = true;
        guard.runs.get_mut(&parent).unwrap().terminal_published = true;
    }
    (harness, parent, child, reserved)
}

#[tokio::test]
async fn async_helper_completion_publication_witness_defers_provisional_parent_and_child() {
    let mut failures = Vec::new();
    for (source, drop_control) in [
        ("parent", false),
        ("parent", true),
        ("child", false),
        ("child", true),
    ] {
        let (harness, parent, child, reserved) = ready().await;
        let target = if source == "parent" { &parent } else { &child };
        let (record, hub) = {
            let mut guard = harness.coordinator.state.write().await;
            let record = guard.runs.get_mut(target).unwrap();
            record.terminal_published = false;
            (record.clone(), guard.live.clone())
        };
        let live = crate::live::LiveRun::register(hub.clone(), &record, None);
        let live = if drop_control {
            drop(live);
            assert!(hub.runs().control(target).is_none());
            None
        } else {
            Some(live)
        };
        harness.coordinator.pump_helper_completions(&child).await;
        harness.coordinator.pump_helper_completions(&parent).await;
        {
            let guard = harness.coordinator.state.read().await;
            if guard.runs.get(&reserved).is_some()
                || !matches!(
                    disposition(guard.runs.get(&child).unwrap()),
                    HelperCompletionState::AwaitingParent { .. }
                )
            {
                failures.push(format!("{source}, dropped control={drop_control}"));
            }
        }
        // A published source may still have its control registered while its
        // own settlement pump runs. That must progress without another event.
        let live =
            live.unwrap_or_else(|| crate::live::LiveRun::register(hub.clone(), &record, None));
        live.publish_record(&record);
        harness
            .coordinator
            .state
            .write()
            .await
            .runs
            .get_mut(target)
            .unwrap()
            .terminal_published = true;
        assert!(hub.runs().control(target).is_some());
        harness.coordinator.pump_helper_completions(target).await;
        harness.coordinator.pump_helper_completions(target).await;
        let guard = harness.coordinator.state.read().await;
        assert_eq!(guard.runs.get(&reserved).unwrap().status, RunStatus::Queued);
        assert_eq!(guard.runs.active_records().len(), 1);
        assert_eq!(
            harness.model.requests().len(),
            1,
            "admission must not wait for execution capacity"
        );
        drop(guard);
        harness.coordinator.request_shutdown();
    }
    assert!(failures.is_empty(), "provisional outcomes cannot authorize a callback, regardless of live control lifetime: {failures:?}");
}

#[tokio::test]
async fn async_helper_completion_actual_start_requires_both_publication_witnesses() {
    let mut failures = Vec::new();
    for source in ["parent", "child"] {
        let (harness, parent, child, _) = ready().await;
        let callback = attach_queued_completion(&harness, &child).await;
        let target = if source == "parent" { &parent } else { &child };
        let mut guard = harness.coordinator.state.write().await;
        guard.runs.get_mut(target).unwrap().terminal_published = false;
        if super::super::async_helpers::validate_accepted_completion(
            &guard,
            &callback,
            &harness.parent.id,
            "chat:owner",
        )
        .is_ok()
        {
            failures.push(source);
        }
        guard.runs.get_mut(target).unwrap().terminal_published = true;
        assert!(super::super::async_helpers::validate_accepted_completion(
            &guard,
            &callback,
            &harness.parent.id,
            "chat:owner"
        )
        .is_ok());
        drop(guard);
        harness.coordinator.request_shutdown();
    }
    assert!(
        failures.is_empty(),
        "a queued handle cannot bypass unpublished source outcomes at execution: {failures:?}"
    );
}

#[tokio::test]
async fn async_helper_completion_escaped_result_envelope_obeys_serialized_input_bound() {
    let text: &'static str = Box::leak("\0".repeat(8192).into_boxed_str());
    let (harness, parent, _, reserved) =
        ready_with_model(ScriptedModel::new(vec![Step::Text(vec![text])])).await;
    harness.coordinator.pump_helper_completions(&parent).await;
    let guard = harness.coordinator.state.read().await;
    let callback = guard
        .runs
        .get(&reserved)
        .expect("a bounded escaped result remains usable");
    assert!(
        callback.input.text.len() <= crate::runs::MAX_RUN_INPUT_TEXT_BYTES,
        "serialized JSON escaping must count toward the 32 KiB input ceiling: {}",
        callback.input.text.len()
    );
    let payload: Value = serde_json::from_str(&callback.input.text).unwrap();
    assert_eq!(payload["resultTruncated"], true);
    assert!(payload["resultText"]
        .as_str()
        .unwrap()
        .chars()
        .all(|c| c == '\0'));
    drop(guard);
    harness.coordinator.request_shutdown();
}

#[tokio::test]
async fn async_helper_management_exact_escaped_result_keeps_handle_and_provenance() {
    let text: &'static str = Box::leak("\0".repeat(8192).into_boxed_str());
    let harness = Harness::with_model(1, ScriptedModel::new(vec![Step::Text(vec![text])])).await;
    let handle = harness
        .launch("escaped-list-source", "Return bounded evidence")
        .await;
    let id = handle["runId"].as_str().unwrap();
    wait_for(&harness.coordinator, id, RunStatus::Completed).await;
    let response = harness
        .call(management(
            "escaped-exact-list",
            "list_helper_runs",
            Some(id),
            None,
        ))
        .await;
    let raw = response
        .data
        .as_ref()
        .expect("successful owned lookup")
        .text
        .clone();
    assert!(raw.len() <= 32 * 1024);
    let listed = success_json(response);
    let runs = listed["runs"].as_array().unwrap();
    assert_eq!(
        runs.len(),
        1,
        "serialized output pressure must not erase the exact owned handle"
    );
    assert_eq!(runs[0]["runId"], id);
    assert_eq!(runs[0]["agentId"], handle["agentId"]);
    assert_eq!(runs[0]["sessionId"], handle["sessionId"]);
    assert_eq!(runs[0]["status"], "completed");
    assert_eq!(runs[0]["source"], "delegation");
    assert_eq!(
        runs[0]["parentRunId"],
        harness.context.run_link.as_ref().unwrap().run_id
    );
    assert!(runs[0]["replyMessageId"].is_string());
    assert_eq!(runs[0]["resultUnavailable"], false);
    assert_eq!(runs[0]["resultTruncated"], true);
}

#[tokio::test]
async fn async_helper_completion_completed_parent_stop_suppresses_pending_callback() {
    let (harness, parent, child, reserved) = ready().await;
    harness
        .coordinator
        .stop_run(&harness.parent.id, &parent)
        .await
        .unwrap();
    harness.coordinator.pump_helper_completions(&child).await;
    assert_closed(&harness, &child, &reserved).await;
}

#[tokio::test]
async fn async_helper_completion_parent_stop_save_failure_restores_queued_callback_without_signal()
{
    let (harness, parent, child, reserved) = ready().await;
    harness.coordinator.pump_helper_completions(&parent).await;
    let control = harness
        .coordinator
        .state
        .read()
        .await
        .live
        .runs()
        .control(&reserved)
        .unwrap();
    let path =
        std::env::temp_dir().join(format!("anima-parent-stop-save-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&path).unwrap();
    harness
        .coordinator
        .state
        .write()
        .await
        .set_control_plane_store(Some(
            crate::control_plane_store::ControlPlaneStoreConfig::Json(path.clone()),
        ));
    assert!(harness
        .coordinator
        .stop_run(&harness.parent.id, &parent)
        .await
        .is_err());
    {
        let guard = harness.coordinator.state.read().await;
        assert!(
            matches!(disposition(guard.runs.get(&child).unwrap()), HelperCompletionState::Queued { run_id } if run_id == &reserved)
        );
        assert_eq!(guard.runs.get(&reserved).unwrap().status, RunStatus::Queued);
        assert!(!control.cancel.is_cancelled());
    }
    harness
        .coordinator
        .state
        .write()
        .await
        .set_control_plane_store(None);
    harness
        .coordinator
        .stop_run(&harness.parent.id, &parent)
        .await
        .unwrap();
    let guard = harness.coordinator.state.read().await;
    assert!(matches!(
        disposition(guard.runs.get(&child).unwrap()),
        HelperCompletionState::Suppressed { .. }
    ));
    assert_eq!(
        guard.runs.get(&reserved).unwrap().status,
        RunStatus::Cancelled
    );
    assert!(control.cancel.is_cancelled());
    drop(guard);
    std::fs::remove_dir(path).unwrap();
    harness.coordinator.request_shutdown();
}

#[tokio::test]
async fn async_helper_completion_queued_start_rechecks_revoked_grant_without_model_call() {
    let (harness, parent, child, reserved) = ready().await;
    harness.coordinator.pump_helper_completions(&parent).await;
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
    wait_for(&harness.coordinator, &reserved, RunStatus::Failed).await;
    assert_eq!(harness.model.requests().len(), 1);
    assert!(matches!(
        disposition(
            harness
                .coordinator
                .state
                .read()
                .await
                .runs
                .get(&child)
                .unwrap()
        ),
        HelperCompletionState::ReviewRequired { .. }
    ));
}

#[tokio::test]
async fn async_helper_completion_fresh_write_approval_belongs_to_new_run() {
    use crate::approvals::{ApprovalDecisionKind, ApprovalPolicy, PolicyAction, RiskClass};
    let model = ScriptedModel::new(vec![
        Step::Text(vec!["committed evidence"]),
        Step::Tools(vec![ToolCall {
            id: "completion-new-write".into(),
            name: "write_file".into(),
            args: BTreeMap::from([
                ("file_path".into(), DataValue::String("reviewed.txt".into())),
                (
                    "content".into(),
                    DataValue::String("approved for this continuation\n".into()),
                ),
            ]),
        }]),
        Step::Text(vec!["Write confirmed after fresh approval."]),
    ]);
    let (harness, parent, _, reserved) = ready_with_model(model.clone()).await;
    let private = tempfile::tempdir().unwrap();
    {
        let mut guard = harness.coordinator.state.write().await;
        guard.workspace = Some(crate::control_plane_store::WorkspaceConfig {
            root_path: private.path().into(),
            ..Default::default()
        });
        let mut config = harness.parent.config.clone();
        config
            .tools
            .as_mut()
            .unwrap()
            .push(ToolRegistry::new().descriptor("write_file").unwrap());
        guard.restore_agent_config(&harness.parent.id, config);
        guard.approvals.set_policy(
            &harness.parent.id,
            ApprovalPolicy::default().with(RiskClass::Write, PolicyAction::Ask),
        );
    }
    harness.coordinator.pump_helper_completions(&parent).await;
    harness.permits.add_permits(1);
    let approvals = super::super::test_support::pending_approvals(&harness.coordinator, 1).await;
    assert_eq!(approvals[0].run_id, reserved);
    assert_ne!(approvals[0].run_id, parent);
    assert_eq!(approvals[0].tool_call_id, "completion-new-write");
    assert!(!private.path().join("reviewed.txt").exists());
    harness
        .coordinator
        .decide_approval(
            &approvals[0].id,
            super::super::test_support::decision(
                ApprovalDecisionKind::AllowOnce,
                approvals[0].revision,
            ),
        )
        .await
        .unwrap();
    wait_for(&harness.coordinator, &reserved, RunStatus::Completed).await;
    assert_eq!(
        std::fs::read_to_string(private.path().join("reviewed.txt")).unwrap(),
        "approved for this continuation\n"
    );
    assert_eq!(model.requests().len(), 3);
}

#[tokio::test]
async fn async_helper_completion_failed_rollback_closes_just_settled_origin_and_preserves_suppression(
) {
    for suppressed in [false, true] {
        let (harness, _, child, _) = ready().await;
        let callback = attach_queued_completion(&harness, &child).await;
        let mut guard = harness.coordinator.state.write().await;
        if suppressed {
            guard
                .runs
                .get_mut(&child)
                .unwrap()
                .async_helper
                .as_mut()
                .unwrap()
                .completion = HelperCompletionState::Suppressed {
                reason: "Explicit owner stop remains authoritative".into(),
                run_id: Some(callback.clone()),
            };
        }
        guard
            .runs
            .get_mut(&callback)
            .unwrap()
            .finish(RunStatus::Completed, None, 4);
        guard.runs.settle_helper_origin(&callback);
        if !suppressed {
            assert!(matches!(
                disposition(guard.runs.get(&child).unwrap()),
                HelperCompletionState::Settled { .. }
            ));
        }
        // commit_run reconciles before its final persistence/source hook. Its
        // rollback must not leave a false successful child disposition.
        guard.runs.get_mut(&callback).unwrap().finish(
            RunStatus::Failed,
            Some(crate::runs::RunError::new(
                crate::runs::COMMIT_FAILED,
                "fixture final save refused",
            )),
            5,
        );
        guard.runs.settle_helper_origin(&callback);
        let closed = disposition(guard.runs.get(&child).unwrap());
        if suppressed {
            assert!(matches!(closed, HelperCompletionState::Suppressed { .. }));
        } else {
            assert!(
                matches!(closed, HelperCompletionState::ReviewRequired { .. }),
                "failed final commit cannot preserve Settled evidence: {closed:?}"
            );
        }
    }
}

#[tokio::test]
async fn async_helper_completion_unannounced_abort_closes_parent_child_and_continuation_authority()
{
    let mut failures = Vec::new();
    for variant in ["parent", "child", "continuation", "suppressed", "published"] {
        let (harness, parent, child, reserved) = ready().await;
        let target = match variant {
            "parent" => parent.clone(),
            "continuation" | "suppressed" => {
                let id = attach_queued_completion(&harness, &child).await;
                let mut guard = harness.coordinator.state.write().await;
                guard
                    .runs
                    .get_mut(&id)
                    .unwrap()
                    .finish(RunStatus::Completed, None, 4);
                guard.runs.settle_helper_origin(&id);
                if variant == "suppressed" {
                    guard
                        .runs
                        .get_mut(&child)
                        .unwrap()
                        .async_helper
                        .as_mut()
                        .unwrap()
                        .completion = HelperCompletionState::Suppressed {
                        reason: "Explicit stop".into(),
                        run_id: Some(id.clone()),
                    };
                }
                id
            }
            _ => child.clone(),
        };
        let (record, hub) = {
            let guard = harness.coordinator.state.read().await;
            (guard.runs.get(&target).unwrap().clone(), guard.live.clone())
        };
        let live = crate::live::LiveRun::register(
            hub.clone(),
            &record,
            (record.agent_id != harness.parent.id).then(|| harness.parent.id.clone()),
        );
        let mut subscription = hub.subscribe(&harness.parent.id).unwrap();
        if variant == "published" {
            live.publish_record(&record);
        }
        drop(super::super::InFlightRunGuard::new(
            harness.coordinator.state.clone(),
            harness.coordinator.control_plane_transactions(),
            target,
            live.end(),
            live.control(),
        ));
        loop {
            let event = super::super::test_support::next_event(&mut subscription)
                .await
                .to_json(0);
            if super::super::test_support::is_terminal(&event) {
                break;
            }
        }
        let guard = harness.coordinator.state.read().await;
        let state = disposition(guard.runs.get(&child).unwrap());
        let correct = match variant {
            "published" => matches!(state, HelperCompletionState::AwaitingParent { .. }),
            "suppressed" => matches!(state, HelperCompletionState::Suppressed { .. }),
            _ => matches!(state, HelperCompletionState::ReviewRequired { .. }),
        };
        if !correct {
            failures.push(format!("{variant}: {state:?}"));
        }
        assert!(guard.runs.get(&reserved).is_none());
        assert_eq!(
            harness.model.requests().len(),
            1,
            "no abort path may replay model work"
        );
    }
    assert!(
        failures.is_empty(),
        "unannounced source commit is uncertain, never successful callback authority: {failures:?}"
    );
}

async fn assert_closed(harness: &Harness, child: &str, reserved: &str) {
    let guard = harness.coordinator.state.read().await;
    assert!(
        matches!(
            disposition(guard.runs.get(child).unwrap()),
            HelperCompletionState::ReviewRequired { .. } | HelperCompletionState::Suppressed { .. }
        ),
        "an inadmissible callback must close without replay rights"
    );
    assert!(guard.runs.get(reserved).is_none());
    assert!(guard.live.runs().control(reserved).is_none());
    assert_eq!(harness.model.requests().len(), 1);
}

#[tokio::test]
async fn async_helper_completion_success_waits_for_live_parent_settlement() {
    let harness = Harness::new(1).await;
    let handle = chat_child(&harness, "child-first").await;
    let child = handle["runId"].as_str().unwrap();
    wait_for(&harness.coordinator, child, RunStatus::Completed).await;
    let guard = harness.coordinator.state.read().await;
    assert!(matches!(
        disposition(guard.runs.get(child).unwrap()),
        HelperCompletionState::AwaitingParent { .. }
    ));
    assert_eq!(harness.model.requests().len(), 1);
}

#[tokio::test]
async fn async_helper_completion_failure_closes_callback_without_result() {
    let harness = Harness::with_model(
        1,
        ScriptedModel::new(vec![Step::Fail("fixture child failed")]),
    )
    .await;
    let handle = chat_child(&harness, "failed-child").await;
    let child = handle["runId"].as_str().unwrap();
    wait_for(&harness.coordinator, child, RunStatus::Failed).await;
    let guard = harness.coordinator.state.read().await;
    assert!(matches!(
        disposition(guard.runs.get(child).unwrap()),
        HelperCompletionState::ReviewRequired { .. }
    ));
}

#[tokio::test]
async fn async_helper_completion_other_parent_kinds_require_manual_handles() {
    use crate::sessions::SessionKind;
    for kind in [
        SessionKind::Telegram,
        SessionKind::Discord,
        SessionKind::Checkin,
        SessionKind::Job,
    ] {
        let harness = Harness::new(0).await;
        harness
            .coordinator
            .state
            .write()
            .await
            .sessions
            .get_mut(&harness.parent.id, "chat:owner")
            .unwrap()
            .kind = kind;
        let refused = harness.call(chat_launch("unsupported-auto")).await;
        assert_eq!(refused.status, anima_core::TaskStatus::Error, "{kind:?}");
        assert!(refused.error.unwrap().contains("manual"));
        let accepted = harness
            .launch("supported-manual", "Return evidence through a handle")
            .await;
        assert_eq!(accepted["completion"]["state"], "manual");
        assert!(harness.model.requests().is_empty());
        harness.coordinator.request_shutdown();
    }
}

#[tokio::test]
async fn async_helper_completion_pump_reserves_exact_typed_original_chat_once() {
    let (harness, parent, child, reserved) = ready().await;
    tokio::join!(
        harness.coordinator.pump_helper_completions(&parent),
        harness.coordinator.pump_helper_completions(&child)
    );
    harness.coordinator.pump_helper_completions(&parent).await;
    let guard = harness.coordinator.state.read().await;
    let continuation = guard
        .runs
        .get(&reserved)
        .expect("one durable reserved completion");
    assert_eq!(continuation.status, RunStatus::Queued);
    assert_eq!(continuation.source, RunSource::Delegation);
    assert_eq!(continuation.agent_id, harness.parent.id);
    assert_eq!(continuation.session_id, "chat:owner");
    assert_eq!(continuation.parent_run_id.as_deref(), Some(parent.as_str()));
    let origin = continuation
        .helper_completion
        .as_ref()
        .expect("typed provenance");
    assert_eq!(origin.helper_run_id, child);
    assert_eq!(origin.launch_parent_run_id, parent);
    assert!(
        matches!(disposition(guard.runs.get(&child).unwrap()), HelperCompletionState::Queued { run_id } if run_id == &reserved)
    );
    assert_eq!(guard.runs.active_records().len(), 1);
    drop(guard);
    harness.coordinator.request_shutdown();
}

#[tokio::test]
async fn async_helper_completion_revoked_grant_closes_without_retry() {
    let (harness, parent, child, reserved) = ready().await;
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
    harness.coordinator.pump_helper_completions(&parent).await;
    assert_closed(&harness, &child, &reserved).await;
    harness
        .coordinator
        .state
        .write()
        .await
        .restore_agent_config(&harness.parent.id, harness.parent.config.clone());
    harness.coordinator.pump_helper_completions(&child).await;
    assert_closed(&harness, &child, &reserved).await;
}

#[tokio::test]
async fn async_helper_completion_archived_or_missing_original_chat_closes() {
    for missing in [false, true] {
        let (harness, parent, child, reserved) = ready().await;
        {
            let mut guard = harness.coordinator.state.write().await;
            if missing {
                guard.sessions.remove(&harness.parent.id, "chat:owner");
            } else {
                guard
                    .sessions
                    .get_mut(&harness.parent.id, "chat:owner")
                    .unwrap()
                    .archived = true;
            }
        }
        harness.coordinator.pump_helper_completions(&parent).await;
        assert_closed(&harness, &child, &reserved).await;
    }
}

#[tokio::test]
async fn async_helper_completion_failed_or_stopped_parent_closes() {
    for status in [RunStatus::Failed, RunStatus::Cancelled] {
        let (harness, parent, child, reserved) = ready().await;
        harness
            .coordinator
            .state
            .write()
            .await
            .runs
            .get_mut(&parent)
            .unwrap()
            .finish(status, None, 4);
        harness.coordinator.pump_helper_completions(&parent).await;
        assert_closed(&harness, &child, &reserved).await;
    }
}

#[tokio::test]
async fn async_helper_completion_missing_committed_result_closes() {
    let (harness, parent, child, reserved) = ready().await;
    harness
        .coordinator
        .state
        .write()
        .await
        .runs
        .get_mut(&child)
        .unwrap()
        .reply_message_id = Some("message_does_not_exist".into());
    harness.coordinator.pump_helper_completions(&parent).await;
    assert_closed(&harness, &child, &reserved).await;
}

#[tokio::test]
async fn async_helper_completion_queue_full_closes_instead_of_later_retry() {
    let (harness, parent, child, reserved) = ready().await;
    {
        let mut guard = harness.coordinator.state.write().await;
        let mut template = guard.runs.get(&parent).unwrap().clone();
        for index in 0..super::super::MAX_QUEUED_RUNS_PER_AGENT {
            template.id = format!("run_fixture_waiting_{index}");
            template.status = RunStatus::Queued;
            template.started_at_ms = None;
            template.finished_at_ms = None;
            guard.runs.insert(template.clone());
        }
    }
    harness.coordinator.pump_helper_completions(&parent).await;
    assert_closed(&harness, &child, &reserved).await;
}

#[tokio::test]
async fn async_helper_completion_save_failure_leaks_no_queued_execution() {
    let (harness, parent, child, reserved) = ready().await;
    let path = std::env::temp_dir().join(format!("anima-pump-save-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&path).unwrap();
    harness
        .coordinator
        .state
        .write()
        .await
        .set_control_plane_store(Some(
            crate::control_plane_store::ControlPlaneStoreConfig::Json(path.clone()),
        ));
    harness.coordinator.pump_helper_completions(&parent).await;
    assert_closed(&harness, &child, &reserved).await;
    harness
        .coordinator
        .state
        .write()
        .await
        .set_control_plane_store(None);
    harness.coordinator.pump_helper_completions(&parent).await;
    assert_closed(&harness, &child, &reserved).await;
    std::fs::remove_dir(path).unwrap();
}

#[tokio::test]
async fn async_helper_recovery_rejects_present_wrong_helper_session_before_mutation() {
    let harness = Harness::new(0).await;
    let child = harness.launch("snapshot-lineage", "Queued task").await;
    let mut snapshot = harness
        .coordinator
        .state
        .read()
        .await
        .control_plane_snapshot();
    let session = snapshot
        .sessions
        .iter_mut()
        .find(|session| session.agent_id == child["agentId"].as_str().unwrap())
        .unwrap();
    session.kind = crate::sessions::SessionKind::Chat;
    let model = ScriptedModel::new(vec![]);
    let mut restored = DaemonState::with_model_adapter(model.clone());
    assert!(
        restored.restore_control_plane_snapshot(snapshot).is_err(),
        "a typed helper cannot restore into an ordinary Chat"
    );
    assert!(
        restored.agents.is_empty(),
        "validation precedes restoring any agent"
    );
    assert!(model.requests().is_empty());
    harness.coordinator.request_shutdown();
}

#[tokio::test]
async fn async_helper_recovery_rejects_helper_config_owned_by_different_live_companion() {
    let harness = Harness::new(0).await;
    let child = harness
        .launch("snapshot-config-lineage", "Queued task")
        .await;
    let (foreign, _) = foreign_actor(&harness).await;
    let mut snapshot = harness
        .coordinator
        .state
        .read()
        .await
        .control_plane_snapshot();
    let helper = snapshot
        .agents
        .iter_mut()
        .find(|agent| agent.state.id == child["agentId"].as_str().unwrap())
        .unwrap();
    helper
        .state
        .config
        .settings
        .as_mut()
        .unwrap()
        .additional
        .insert("parentAgentId".into(), DataValue::String(foreign.id));
    let mut restored = DaemonState::with_model_adapter(ScriptedModel::new(vec![]));
    assert!(
        restored.restore_control_plane_snapshot(snapshot).is_err(),
        "run, Helper session and helper configuration must agree on owner"
    );
    assert!(restored.agents.is_empty());
    harness.coordinator.request_shutdown();
}

#[tokio::test]
async fn async_helper_recovery_two_fresh_restores_are_stable_without_model_replay() {
    let harness = Harness::new(0).await;
    let handle = chat_child(&harness, "restart-pending").await;
    let id = handle["runId"].as_str().unwrap();
    let snapshot = harness
        .coordinator
        .state
        .read()
        .await
        .control_plane_snapshot();
    let first_model = ScriptedModel::new(vec![Step::Panic("restart must not call the model")]);
    let mut first = DaemonState::with_model_adapter(first_model.clone());
    first.restore_control_plane_snapshot(snapshot).unwrap();
    let first_record = serde_json::to_value(first.runs.get(id).unwrap()).unwrap();
    assert_eq!(first_record["status"], "interrupted");
    assert_eq!(
        first_record["asyncHelper"]["completion"]["state"],
        "review_required"
    );
    let second_model =
        ScriptedModel::new(vec![Step::Panic("second restore must not call the model")]);
    let mut second = DaemonState::with_model_adapter(second_model.clone());
    second
        .restore_control_plane_snapshot(first.control_plane_snapshot())
        .unwrap();
    assert_eq!(
        serde_json::to_value(second.runs.get(id).unwrap()).unwrap(),
        first_record
    );
    assert!(first_model.requests().is_empty() && second_model.requests().is_empty());
    harness.coordinator.request_shutdown();
}

#[test]
fn async_helper_recovery_v20_backup_preserves_earlier_suffixes() {
    use crate::control_plane_store::{pre_upgrade_backup_path, ASYNC_HELPERS_STORE_VERSION};
    let path = std::path::Path::new("control-plane.json");
    assert_eq!(ASYNC_HELPERS_STORE_VERSION, 20);
    assert_eq!(
        pre_upgrade_backup_path(path, 19),
        std::path::PathBuf::from("control-plane.json.pre-async-helpers.bak")
    );
    assert_eq!(
        pre_upgrade_backup_path(path, 18),
        std::path::PathBuf::from("control-plane.json.pre-community-bundles.bak")
    );
    assert_eq!(
        pre_upgrade_backup_path(path, 17),
        std::path::PathBuf::from("control-plane.json.pre-mcp-stdio.bak")
    );
}

struct SixCallModel {
    total: std::sync::atomic::AtomicUsize,
    owner_calls: std::sync::atomic::AtomicUsize,
    gate: super::super::test_support::Gate,
    requests: std::sync::Mutex<
        Vec<(
            bool,
            usize,
            anima_core::AgentConfig,
            anima_core::ModelGenerateRequest,
        )>,
    >,
}

#[async_trait::async_trait]
impl anima_core::ModelAdapter for SixCallModel {
    fn provider(&self) -> &str {
        "fp05-six-recordings"
    }
    async fn generate(
        &self,
        config: &anima_core::AgentConfig,
        request: &anima_core::ModelGenerateRequest,
    ) -> Result<anima_core::ModelGenerateResponse, String> {
        let total = self.total.fetch_add(1, Ordering::SeqCst) + 1;
        assert!(
            total <= 6,
            "the acceptance fixture allows exactly six model calls"
        );
        let helper = super::super::is_helper_config(config);
        let owner_call = if helper {
            0
        } else {
            self.owner_calls.fetch_add(1, Ordering::SeqCst) + 1
        };
        self.requests
            .lock()
            .unwrap()
            .push((helper, owner_call, config.clone(), request.clone()));
        let (text, tools) = if helper {
            self.gate.entered.add_permits(1);
            self.gate.release.acquire().await.unwrap().forget();
            (format!("literal evidence from {}", config.name), None)
        } else if owner_call == 1 {
            let calls = ["Evidence A", "Evidence B"]
                .into_iter()
                .enumerate()
                .map(|(index, name)| {
                    let mut call = chat_launch(&format!("six-helper-{index}"));
                    call.args
                        .insert("name".into(), DataValue::String(name.into()));
                    call
                })
                .collect();
            (String::new(), Some(calls))
        } else if owner_call == 2 {
            ("Accepted two helpers; results are pending.".into(), None)
        } else {
            (
                "Recorded helper evidence received; it has not been independently verified.".into(),
                None,
            )
        };
        Ok(anima_core::ModelGenerateResponse {
            content: Content {
                text,
                ..Content::default()
            },
            stop_reason: if tools.is_some() {
                anima_core::ModelStopReason::ToolCall
            } else {
                anima_core::ModelStopReason::End
            },
            tool_calls: tools,
            usage: anima_core::TokenUsage {
                prompt_tokens: 10,
                completion_tokens: 2,
                total_tokens: 12,
                ..Default::default()
            },
        })
    }
}

#[tokio::test]
async fn async_helper_completion_two_helpers_two_continuations_exactly_six_recordings() {
    use crate::approvals::{ApprovalPolicy, PolicyAction, RiskClass};
    let gate = super::super::test_support::Gate::new();
    let model = Arc::new(SixCallModel {
        total: 0.into(),
        owner_calls: 0.into(),
        gate: gate.clone(),
        requests: Default::default(),
    });
    let mut state = DaemonState::with_model_adapter(model.clone());
    let mut config = lead_config("Six-call companion");
    config.tools = Some(
        ToolRegistry::new()
            .resolve_descriptors([
                "calculate",
                "spawn_helper_async",
                "spawn_helper",
                "list_helper_runs",
                "steer_helper",
                "stop_helper",
            ])
            .unwrap(),
    );
    config.settings.as_mut().unwrap().max_retries = Some(0);
    let owner = state.create_agent(config).unwrap().state.id;
    state.approvals.set_policy(
        &owner,
        ApprovalPolicy::default().with(RiskClass::Delegate, PolicyAction::Allow),
    );
    let coordinator =
        AgentRunCoordinator::new(Arc::new(RwLock::new(state)), Arc::new(Semaphore::new(3)));
    add_chat(&coordinator, &owner, "chat:six").await;
    let parent = super::super::test_support::accept_web(
        &coordinator,
        &owner,
        "chat:six",
        "Delegate two bounded evidence tasks",
    )
    .await;
    gate.entered().await;
    gate.entered().await;
    wait_for(&coordinator, &parent, RunStatus::Completed).await;
    let children = coordinator
        .state
        .read()
        .await
        .control_plane_snapshot()
        .runs
        .into_iter()
        .filter(|record| record.async_helper.is_some())
        .collect::<Vec<_>>();
    assert_eq!(children.len(), 2);
    assert_eq!(
        model.total.load(Ordering::SeqCst),
        4,
        "parent answer precedes helper output"
    );
    let reserved = children
        .iter()
        .map(|record| disposition(record).run_id().unwrap().to_string())
        .collect::<HashSet<_>>();
    assert_eq!(reserved.len(), 2);
    gate.release();
    gate.release();
    for id in &reserved {
        wait_for(&coordinator, id, RunStatus::Completed).await;
    }
    assert_eq!(model.total.load(Ordering::SeqCst), 6);
    let requests = model.requests.lock().unwrap();
    let callbacks = requests
        .iter()
        .filter(|(helper, call, _, _)| !helper && *call > 2)
        .collect::<Vec<_>>();
    assert_eq!(callbacks.len(), 2);
    for child in &children {
        let matching = callbacks
            .iter()
            .filter(|(_, _, _, request)| {
                // The original Chat legitimately retains the previous
                // completion's history. Only this turn's new host envelope
                // establishes its immutable completion origin.
                let payload: Value =
                    serde_json::from_str(&request.messages.last().unwrap().content.text).unwrap();
                payload["kind"] == "helper_completion"
                    && payload["helperRunId"] == child.id
                    && payload["launchParentRunId"] == parent
                    && payload["helperSessionId"] == child.session_id
                    && payload["resultText"]
                        .as_str()
                        .unwrap()
                        .starts_with("literal evidence from Evidence")
            })
            .count();
        assert_eq!(
            matching, 1,
            "each completion contains only its own committed source evidence"
        );
    }
    for (_, _, config, _) in callbacks {
        for tool in [
            "spawn_helper_async",
            "spawn_helper",
            "list_helper_runs",
            "steer_helper",
            "stop_helper",
        ] {
            assert!(
                !config.allows_tool(tool),
                "completion cannot coordinate recursively"
            );
        }
        assert!(config.settings.as_ref().unwrap().max_tokens.unwrap() <= 4096);
    }
    drop(requests);
    let guard = coordinator.state.read().await;
    for child in children {
        assert!(
            matches!(disposition(guard.runs.get(&child.id).unwrap()), HelperCompletionState::Settled { run_id } if reserved.contains(run_id))
        );
    }
}

#[tokio::test]
async fn async_helper_completion_filters_granted_descriptors_and_denies_native_coordination() {
    let mut spawn = chat_launch("callback-must-not-spawn");
    spawn
        .args
        .insert("completion".into(), DataValue::String("manual".into()));
    let model = ScriptedModel::new(vec![
        Step::Text(vec!["committed child evidence"]),
        Step::Tools(vec![
            ToolCall {
                id: "callback-roster".into(),
                name: "list_workspace_agents".into(),
                args: BTreeMap::new(),
            },
            spawn,
        ]),
        Step::Text(vec!["Coordination was refused; bounded inspection ended."]),
    ]);
    let harness = Harness::with_model(1, model.clone()).await;
    let handle = harness.launch("source-for-denial", "Return evidence").await;
    let child = handle["runId"].as_str().unwrap();
    wait_for(&harness.coordinator, child, RunStatus::Completed).await;
    drained(&harness, &handle).await;
    let parent = harness.context.run_link.as_ref().unwrap().clone();
    let forbidden = [
        "list_workspace_agents",
        "send_message",
        "broadcast_message",
        "delegate_to_agent",
        "spawn_helper",
        "spawn_helper_async",
        "list_helper_runs",
        "steer_helper",
        "stop_helper",
        "create_automation",
        "pause_automation",
        "update_automation",
        "resume_automation",
        "delete_automation",
        "trigger_automation",
        "bg_start",
    ];
    {
        let mut guard = harness.coordinator.state.write().await;
        guard
            .runs
            .get_mut(&parent.run_id)
            .unwrap()
            .finish(RunStatus::Completed, None, 3);
        guard
            .runs
            .get_mut(&parent.run_id)
            .unwrap()
            .terminal_published = true;
        let mut config = harness.parent.config.clone();
        config.tools = Some(
            ToolRegistry::new()
                .resolve_descriptors(forbidden.into_iter().chain([
                    "calculate",
                    "bash",
                    "bg_output",
                    "bg_stop",
                ]))
                .unwrap(),
        );
        guard.restore_agent_config(&harness.parent.id, config);
    }
    let id = attach_queued_completion(&harness, child).await;
    harness
        .coordinator
        .run_accepted(
            super::super::AgentRunRequest {
                agent_id: harness.parent.id.clone(),
                content: Content {
                    text: "Inspect committed helper evidence".into(),
                    ..Default::default()
                },
                room: super::super::RunRoom::Stable("chat:owner".into()),
                idempotency_key: None,
                source: RunSource::Delegation,
                source_ref: None,
                parent: Some(parent),
            },
            id.clone(),
        )
        .await
        .unwrap();
    let configs = model.configs();
    for tool in forbidden {
        assert!(
            !configs[1].allows_tool(tool),
            "completion must not expose {tool}"
        );
    }
    for tool in ["calculate", "bash", "bg_output", "bg_stop"] {
        assert!(
            configs[1].allows_tool(tool),
            "current ordinary grant {tool} remains bounded by normal policy"
        );
    }
    let guard = harness.coordinator.state.read().await;
    let run = guard.runs.get(&id).unwrap();
    assert_eq!(run.status, RunStatus::Failed);
    assert!(
        run.error
            .as_ref()
            .unwrap()
            .message
            .contains("not configured"),
        "the runtime rejects a batch of unconfigured calls before executing it"
    );
    let mut caller = harness.parent.clone();
    caller.config = configs[1].clone();
    let context = guard
        .tool_execution_context()
        .with_team(harness.coordinator.clone(), false)
        .with_run_link(Some(RunLink {
            run_id: id,
            agent_id: caller.id.clone(),
            session_id: "chat:owner".into(),
        }));
    drop(guard);
    for name in forbidden {
        let result = context
            .clone()
            .execute_tool(
                caller.clone(),
                input(&caller.id),
                ToolCall {
                    id: format!("native-deny-{name}"),
                    name: name.into(),
                    args: BTreeMap::new(),
                },
            )
            .await;
        assert_eq!(result.status, anima_core::TaskStatus::Error, "{name}");
        assert!(
            result.error.unwrap().contains("not configured"),
            "native configuration fence denies {name}"
        );
    }
    assert_eq!(harness.coordinator.state.read().await.agents.len(), 2);
    assert_eq!(
        model.requests().len(),
        2,
        "no further model call after the unconfigured batch"
    );
}
