use super::*;
use crate::control_plane_store::{load_control_plane_snapshot, ControlPlaneStoreConfig};
use crate::runs::{RunSource, RunStart};
use anima_core::execution_limits::{
    ExecutionOperation, ReservationAdmission, ReservationRequest, ReservationSettlement,
    SettlementOutcome,
};

fn record(id: &str) -> RunRecord {
    let mut value = RunRecord::queued(
        RunStart {
            agent_id: "agent".into(),
            session_id: "chat:test".into(),
            source: RunSource::Web,
            source_ref: None,
            idempotency_key: None,
            text: "task".into(),
            model: "test".into(),
            provider: Some("deterministic".into()),
            parent_run_id: None,
        },
        100,
    );
    value.id = id.into();
    value
}

fn request(id: &str, run: &str) -> ReservationRequest {
    ReservationRequest {
        id: id.into(),
        run_id: run.into(),
        logical_call_id: id.into(),
        operation: ExecutionOperation::ModelAttempt,
        input_tokens: 100,
        output_tokens: 50,
    }
}

async fn fixture(label: &str) -> (SharedDaemonState, Arc<Mutex<()>>, ControlPlaneStoreConfig) {
    let path = std::env::temp_dir()
        .join(format!(
            "anima-host-budget-{label}-{}",
            uuid::Uuid::new_v4()
        ))
        .join("control.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let config = ControlPlaneStoreConfig::Json(path);
    let mut state = DaemonState::new();
    state.set_control_plane_store(Some(config.clone()));
    (
        Arc::new(tokio::sync::RwLock::new(state)),
        Arc::new(Mutex::new(())),
        config,
    )
}

#[tokio::test]
async fn host_budget_retained_child_keeps_scope_deadline_and_debits_dispatch_once() {
    let (state, transactions, config) = fixture("retained-child").await;
    let parent = standalone_context(
        &state,
        transactions.clone(),
        "retained-root".into(),
        CancelSignal::new(),
    )
    .await
    .unwrap();
    let mut child = retained_child_context(
        &state,
        transactions.clone(),
        &parent,
        "retained-child".into(),
        CancelSignal::new(),
    )
    .await
    .unwrap();
    assert_eq!(child.context.scope_id, parent.scope_id);
    assert_eq!(child.context.deadline_at_ms, parent.deadline_at_ms);
    assert_eq!(child.context.run_id, "retained-child");
    child.dispatch.revalidate().await.unwrap();
    let saved = load_control_plane_snapshot(&config).await.unwrap().unwrap();
    assert_eq!(saved.execution_budgets.scopes.len(), 1);
    let scope = &saved.execution_budgets.scopes[&parent.scope_id];
    assert_eq!(scope.consumption().unwrap().tool_calls, 1);
    let dispatch = &scope.reservations[child.dispatch.reservation_id()];
    assert_eq!(dispatch.request.operation, ExecutionOperation::Tool);
    assert_eq!(dispatch.request.logical_call_id, "retained-child-dispatch");
    assert_eq!(dispatch.request.run_id, child.context.run_id);
    child
        .dispatch
        .finish_owned(
            SettlementOutcome::Known(anima_core::TokenUsage::default()),
            true,
        )
        .await
        .unwrap();
    assert!(retained_child_context(
        &state,
        transactions,
        &parent,
        "retained-child".into(),
        CancelSignal::new(),
    )
    .await
    .is_err());
    let saved = load_control_plane_snapshot(&config).await.unwrap().unwrap();
    assert_eq!(saved.execution_budgets.scopes.len(), 1);
    assert_eq!(
        saved.execution_budgets.scopes[&parent.scope_id]
            .consumption()
            .unwrap()
            .tool_calls,
        1,
    );
}

#[tokio::test]
async fn host_budget_retained_child_and_parent_share_remaining_model_allowance() {
    let (state, transactions, _) = fixture("retained-usage").await;
    let mut limits = ExecutionLimits::default();
    limits.max_model_attempts = 2;
    limits.max_retries = 0;
    let parent = standalone_context_with_limits(
        &state,
        transactions.clone(),
        "retained-usage-root".into(),
        CancelSignal::new(),
        Some(limits),
    )
    .await
    .unwrap();
    let mut child = retained_child_context(
        &state,
        transactions,
        &parent,
        "retained-usage-child".into(),
        CancelSignal::new(),
    )
    .await
    .unwrap();
    child
        .dispatch
        .finish_owned(SettlementOutcome::ProvenNeverSent, true)
        .await
        .unwrap();
    for (context, id) in [(&parent, "parent-spent"), (&child.context, "child-spent")] {
        context
            .port
            .reserve(&context.scope_id, request(id, &context.run_id))
            .await
            .unwrap();
        context
            .port
            .submit_settlement(
                &context.scope_id,
                ReservationSettlement {
                    reservation_id: id.into(),
                    outcome: SettlementOutcome::Known(anima_core::TokenUsage {
                        prompt_tokens: 100,
                        completion_tokens: 50,
                        total_tokens: 150,
                        ..Default::default()
                    }),
                    physical_retired: true,
                    observed_usage: None,
                },
            )
            .await
            .unwrap();
    }
    let scope = state.read().await.execution_budgets.scopes[&parent.scope_id].clone();
    let usage = scope.consumption().unwrap();
    assert_eq!(usage.model_attempts, 2);
    assert_eq!(usage.charged_total_tokens, 300);
    assert!(child
        .context
        .port
        .reserve(
            &child.context.scope_id,
            request("no-reset", &child.context.run_id),
        )
        .await
        .is_err());
}

#[tokio::test]
async fn host_budget_retained_child_failed_admission_keeps_binding_and_refuses_replay() {
    let (state, transactions, _) = fixture("retained-failed").await;
    let parent = standalone_context(
        &state,
        transactions.clone(),
        "retained-failed-root".into(),
        CancelSignal::new(),
    )
    .await
    .unwrap();
    let gate = state
        .write()
        .await
        .install_test_control_plane_save_gate(true);
    gate.release.add_permits(1);
    assert!(retained_child_context(
        &state,
        transactions.clone(),
        &parent,
        "retained-failed-child".into(),
        CancelSignal::new(),
    )
    .await
    .is_err());
    let registry = state.read().await.execution_budgets.clone();
    assert_eq!(
        registry.run_scopes["retained-failed-child"],
        parent.scope_id
    );
    assert_eq!(registry.scopes.len(), 1);
    assert_eq!(
        registry.scopes[&parent.scope_id].deadline_at_ms,
        parent.deadline_at_ms,
    );
    assert!(retained_child_context(
        &state,
        transactions,
        &parent,
        "retained-failed-child".into(),
        CancelSignal::new(),
    )
    .await
    .is_err());
    assert_eq!(state.read().await.execution_budgets, registry);
}

#[tokio::test]
async fn host_budget_retained_child_dropped_caller_keeps_save_custody_and_binding() {
    let (state, transactions, config) = fixture("retained-dropped").await;
    let parent = standalone_context(
        &state,
        transactions.clone(),
        "retained-dropped-root".into(),
        CancelSignal::new(),
    )
    .await
    .unwrap();
    let gate = state
        .write()
        .await
        .install_test_control_plane_save_gate(false);
    let caller = {
        let state = state.clone();
        let transactions = transactions.clone();
        let parent = parent.clone();
        tokio::spawn(async move {
            retained_child_context(
                &state,
                transactions,
                &parent,
                "retained-dropped-child".into(),
                CancelSignal::new(),
            )
            .await
        })
    };
    gate.entered.acquire().await.unwrap().forget();
    caller.abort();
    assert!(
        transactions.try_lock().is_err(),
        "admission must retain the shared transaction through save acknowledgement",
    );
    gate.release.add_permits(1);
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if state
                .read()
                .await
                .execution_budgets
                .run_scopes
                .contains_key("retained-dropped-child")
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("owned child admission did not retain its binding");
    assert!(retained_child_context(
        &state,
        transactions,
        &parent,
        "retained-dropped-child".into(),
        CancelSignal::new(),
    )
    .await
    .is_err());
    let saved = load_control_plane_snapshot(&config).await.unwrap().unwrap();
    assert_eq!(
        saved.execution_budgets.run_scopes["retained-dropped-child"],
        parent.scope_id,
    );
}

#[tokio::test]
async fn host_budget_retained_child_unknown_dispatch_cannot_reopen_identity() {
    let (state, transactions, config) = fixture("retained-unknown").await;
    let parent = standalone_context(
        &state,
        transactions.clone(),
        "retained-unknown-root".into(),
        CancelSignal::new(),
    )
    .await
    .unwrap();
    let mut child = retained_child_context(
        &state,
        transactions.clone(),
        &parent,
        "retained-unknown-child".into(),
        CancelSignal::new(),
    )
    .await
    .unwrap();
    child
        .dispatch
        .finish_owned(SettlementOutcome::Unknown { observed: None }, false)
        .await
        .unwrap();
    assert!(retained_child_context(
        &state,
        transactions,
        &parent,
        "retained-unknown-child".into(),
        CancelSignal::new(),
    )
    .await
    .is_err());
    let saved = load_control_plane_snapshot(&config).await.unwrap().unwrap();
    let usage = saved.execution_budgets.scopes[&parent.scope_id]
        .consumption()
        .unwrap();
    assert_eq!(usage.tool_calls, 1);
    assert_eq!(usage.inflight_operations, 1);
    assert_eq!(saved.execution_budgets.scopes.len(), 1);
}

#[tokio::test]
async fn host_budget_retained_child_preserves_parent_and_caller_cancellation() {
    for cancel_parent in [true, false] {
        let (state, transactions, _) = fixture("retained-cancel").await;
        let parent = standalone_context(
            &state,
            transactions.clone(),
            "retained-cancel-root".into(),
            CancelSignal::new(),
        )
        .await
        .unwrap();
        let cancel = CancelSignal::new();
        let mut child = retained_child_context(
            &state,
            transactions,
            &parent,
            "retained-cancel-child".into(),
            cancel.clone(),
        )
        .await
        .unwrap();
        if cancel_parent {
            parent.cancel.cancel();
        } else {
            cancel.cancel();
        }
        assert!(child.context.check_live().is_err());
        assert!(child.dispatch.revalidate().await.is_err());
        child
            .dispatch
            .finish_owned(SettlementOutcome::ProvenNeverSent, true)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn host_budget_retained_child_refuses_unrepresentable_cancel_chain_without_reset() {
    let (state, transactions, _) = fixture("retained-chain").await;
    let mut parent = standalone_context(
        &state,
        transactions.clone(),
        "retained-chain-root".into(),
        CancelSignal::new(),
    )
    .await
    .unwrap();
    parent.secondary_cancel = Some(CancelSignal::new());
    let registry = state.read().await.execution_budgets.clone();
    assert!(retained_child_context(
        &state,
        transactions,
        &parent,
        "retained-chain-child".into(),
        CancelSignal::new(),
    )
    .await
    .is_err());
    assert_eq!(state.read().await.execution_budgets, registry);
}

#[tokio::test]
async fn host_budget_retained_child_refuses_changed_parent_binding_without_new_scope() {
    let (state, transactions, _) = fixture("retained-mismatch").await;
    let parent = standalone_context(
        &state,
        transactions.clone(),
        "retained-mismatch-root".into(),
        CancelSignal::new(),
    )
    .await
    .unwrap();
    let registry = state.read().await.execution_budgets.clone();
    for mismatch in ["run", "scope", "deadline"] {
        let mut changed = parent.clone();
        match mismatch {
            "run" => changed.run_id = "foreign-parent".into(),
            "scope" => changed.scope_id = "foreign-scope".into(),
            "deadline" => changed.deadline_at_ms -= 1,
            _ => unreachable!("test enumerates parent binding fields"),
        }
        assert!(retained_child_context(
            &state,
            transactions.clone(),
            &changed,
            format!("retained-mismatch-child-{mismatch}"),
            CancelSignal::new(),
        )
        .await
        .is_err());
        assert_eq!(state.read().await.execution_budgets, registry);
    }
}

#[test]
fn host_budget_root_helper_and_reviewed_recovery_keep_one_scope() {
    let root = record("root");
    let first = HostExecutionBudgets::default().prepare_run(&root, 100);
    assert!(
        first.is_ok(),
        "a new owner turn must admit one finite scope"
    );
    let first = first.unwrap();
    let mut helper = record("helper");
    helper.parent_run_id = Some(root.id.clone());
    let next = first.registry.prepare_run(&helper, 101).unwrap();
    let mut recovery = record("recovery");
    recovery.recovery_of = Some(root.id);
    let last = next.registry.prepare_run(&recovery, 102).unwrap();
    assert_eq!(first.binding.scope_id, next.binding.scope_id);
    assert_eq!(first.binding.scope_id, last.binding.scope_id);
    assert_eq!(last.registry.scopes.len(), 1);
    assert_eq!(first.binding.deadline_at_ms, last.binding.deadline_at_ms);
}

#[test]
fn host_budget_completion_inherits_source_and_rejects_disagreeing_lineage() {
    let first = HostExecutionBudgets::default()
        .prepare_run(&record("parent"), 100)
        .unwrap();
    let mut helper = record("helper");
    helper.parent_run_id = Some("parent".into());
    let next = first.registry.prepare_run(&helper, 101).unwrap();
    let mut completion = record("completion");
    completion.parent_run_id = Some("parent".into());
    completion.helper_completion = Some(crate::runs::HelperCompletion {
        helper_run_id: "helper".into(),
        launch_parent_run_id: "parent".into(),
    });
    assert_eq!(
        next.registry
            .prepare_run(&completion, 102)
            .unwrap()
            .binding
            .scope_id,
        first.binding.scope_id
    );
    let other = next.registry.prepare_run(&record("other"), 102).unwrap();
    completion.parent_run_id = Some("other".into());
    assert!(other.registry.prepare_run(&completion, 103).is_err());
}

#[test]
fn host_budget_restore_rejects_missing_scope_without_dropping_orphaned_evidence() {
    let mut value = HostExecutionBudgets::default();
    value.run_scopes.insert("old-root".into(), "missing".into());
    assert!(
        value.validate().is_err(),
        "a binding without accounting must fail closed"
    );
    let saved = HostExecutionBudgets::default()
        .prepare_operation("profile:one", 100)
        .unwrap();
    let restored: HostExecutionBudgets =
        serde_json::from_str(&serde_json::to_string(&saved.registry).unwrap()).unwrap();
    restored.validate().unwrap();
    assert!(restored.run_scopes.contains_key("profile:one"));
    assert!(restored.scopes.contains_key(&saved.binding.scope_id));
}

#[test]
fn host_budget_existing_binding_cannot_be_reparented_or_replenished() {
    let first = HostExecutionBudgets::default()
        .prepare_run(&record("root"), 100)
        .unwrap();
    let other = first.registry.prepare_run(&record("other"), 101).unwrap();
    let same = other.registry.prepare_run(&record("root"), 102).unwrap();
    assert_eq!(same.registry, other.registry);
    let mut changed = record("root");
    changed.parent_run_id = Some("other".into());
    assert!(other.registry.prepare_run(&changed, 103).is_err());
    let mut recovery = record("recovery");
    recovery.recovery_of = Some("unbound-legacy-started-run".into());
    assert!(other.registry.prepare_run(&recovery, 104).is_err());
}

#[test]
fn host_budget_legacy_recovery_only_adopts_verified_never_started_source() {
    let mut state = DaemonState::new();
    let agent = state
        .create_agent(
            serde_json::from_value(serde_json::json!({ "name": "legacy owner", "model": "test" }))
                .unwrap(),
        )
        .unwrap();
    let mut source = record("legacy");
    source.agent_id = agent.state.id.clone();
    let mut recovery = record("reviewed");
    recovery.agent_id = agent.state.id;
    recovery.recovery_of = Some(source.id.clone());
    state.runs.insert(source.clone());
    let candidate = prepare_run_binding(&state, &recovery, 100).unwrap();
    assert_eq!(
        candidate.registry.run_scopes["legacy"],
        candidate.binding.scope_id
    );
    let mut started = source;
    started.started_at_ms = Some(99);
    state.runs.insert(started);
    assert!(prepare_run_binding(&state, &recovery, 100).is_err());
}

#[test]
fn host_budget_legacy_missing_start_timestamp_cannot_erase_execution_evidence() {
    for evidence in ["step", "usage", "reply", "digest", "checkpoint"] {
        let mut state = DaemonState::new();
        let agent = state
            .create_agent(
                serde_json::from_value(serde_json::json!({ "name": "owner", "model": "test" }))
                    .unwrap(),
            )
            .unwrap();
        let mut source = record("legacy-evidence");
        source.agent_id = agent.state.id.clone();
        match evidence {
            "step" => source.steps.push(
                serde_json::from_value(serde_json::json!({ "stepId": "physical-model" })).unwrap(),
            ),
            "usage" => source.usage.prompt_tokens = 1,
            "reply" => source.reply_message_id = Some("committed-reply".into()),
            "digest" => source.reply_sha256 = Some("f".repeat(64)),
            "checkpoint" => {
                let mut turn = crate::runs::turn::bind(&state, &source, false).unwrap();
                turn.checkpoint = Some(anima_core::turn_recovery::TurnCheckpoint {
                    binding: turn.binding.clone(),
                    sequence: 1,
                    phase: anima_core::turn_recovery::TurnPhase::BeforeModel,
                    model_calls: 0,
                    progress: Vec::new(),
                    pending_tools: Vec::new(),
                });
                source.turn = Some(turn);
            }
            _ => unreachable!(),
        }
        state.runs.insert(source.clone());
        let mut recovery = record("legacy-successor");
        recovery.agent_id = agent.state.id;
        recovery.recovery_of = Some(source.id.clone());
        assert!(
            prepare_run_binding(&state, &recovery, 100).is_err(),
            "recovery ignored persisted {evidence}"
        );
        assert!(
            prepare_run_binding(&state, &source, 100).is_err(),
            "same-run adoption ignored persisted {evidence}"
        );
        assert!(state.execution_budgets.scopes.is_empty());
    }
}

#[tokio::test]
async fn host_budget_concurrent_last_slot_and_exhausted_restart_remain_bounded() {
    let (state, transactions, config) = fixture("race").await;
    let context = standalone_context(
        &state,
        transactions.clone(),
        "profile:race".into(),
        CancelSignal::new(),
    )
    .await
    .unwrap();
    {
        let mut guard = state.write().await;
        let scope = guard
            .execution_budgets
            .scopes
            .get_mut(&context.scope_id)
            .unwrap();
        scope.limits.max_model_attempts = 1;
        scope.limits.max_retries = 0;
    }
    let (left, right) = tokio::join!(
        context
            .port
            .reserve(&context.scope_id, request("left", &context.run_id)),
        context
            .port
            .reserve(&context.scope_id, request("right", &context.run_id)),
    );
    assert_eq!(usize::from(left.is_ok()) + usize::from(right.is_ok()), 1);
    let saved = load_control_plane_snapshot(&config).await.unwrap().unwrap();
    let mut restored = DaemonState::new();
    restored.set_control_plane_store(Some(config));
    restored.restore_control_plane_snapshot(saved).unwrap();
    let restored = Arc::new(tokio::sync::RwLock::new(restored));
    let reattached = context_for_run(
        &restored,
        transactions,
        &context.run_id,
        CancelSignal::new(),
    )
    .await
    .unwrap();
    assert!(reattached
        .port
        .reserve(
            &reattached.scope_id,
            request("after-restart", &reattached.run_id)
        )
        .await
        .is_err());
}

#[tokio::test]
async fn host_budget_admission_saves_run_and_binding_in_one_snapshot() {
    let (state, transactions, config) = fixture("admission").await;
    let _transaction = transactions.lock().await;
    let (candidate, persist) = {
        let mut guard = state.write().await;
        let agent = guard
            .create_agent(
                serde_json::from_value(serde_json::json!({
                    "name": "budget admission", "model": "test"
                }))
                .unwrap(),
            )
            .unwrap();
        let mut run = record("atomic-run");
        run.agent_id = agent.state.id;
        let candidate =
            prepare_run_binding(&guard, &run, anima_core::primitives::now_millis()).unwrap();
        guard.runs.insert(run);
        (candidate, guard.control_plane_persist_request())
    };
    let scope = candidate.binding.scope_id.clone();
    save_admission(&state, candidate, persist).await.unwrap();
    let saved = load_control_plane_snapshot(&config).await.unwrap().unwrap();
    assert!(saved.runs.iter().any(|run| run.id == "atomic-run"));
    assert_eq!(saved.execution_budgets.run_scopes["atomic-run"], scope);
    assert_eq!(
        state.read().await.execution_budgets,
        saved.execution_budgets
    );
}

#[tokio::test]
async fn host_budget_ordinary_writer_cannot_persist_a_provisional_unbound_run() {
    let (state, transactions, config) = fixture("provisional").await;
    let _transaction = transactions.lock().await;
    let (candidate, admission, ordinary) = {
        let mut guard = state.write().await;
        let agent = guard
            .create_agent(
                serde_json::from_value(serde_json::json!({ "name": "owner", "model": "test" }))
                    .unwrap(),
            )
            .unwrap();
        let mut run = record("provisional-run");
        run.agent_id = agent.state.id;
        let candidate =
            prepare_run_binding(&guard, &run, anima_core::primitives::now_millis()).unwrap();
        guard.execution_budget_candidate_snapshot = Some(candidate.registry.clone());
        guard.runs.insert(run);
        let admission = guard.control_plane_persist_request();
        let ordinary = guard.control_plane_persist_request();
        (candidate, admission, ordinary)
    };
    assert!(
        context_for_run(
            &state,
            transactions.clone(),
            "provisional-run",
            CancelSignal::new()
        )
        .await
        .is_err(),
        "snapshot staging must never grant runtime authority"
    );
    ordinary.save().await.unwrap();
    let saved = load_control_plane_snapshot(&config).await.unwrap().unwrap();
    assert!(saved.runs.iter().any(|run| run.id == "provisional-run"));
    assert!(saved
        .execution_budgets
        .run_scopes
        .contains_key("provisional-run"));
    assert!(
        save_admission(&state, candidate, admission).await.is_err(),
        "superseded admission must not dispatch"
    );
    assert!(state
        .read()
        .await
        .execution_budget_candidate_snapshot
        .is_none());
}

#[tokio::test]
async fn host_budget_ordinary_snapshot_cannot_erase_acknowledged_reservation() {
    let (state, transactions, config) = fixture("stale").await;
    let context = standalone_context(
        &state,
        transactions,
        "profile:stale".into(),
        CancelSignal::new(),
    )
    .await
    .unwrap();
    let gate = state
        .write()
        .await
        .install_test_control_plane_save_gate(false);
    let worker = {
        let context = context.clone();
        tokio::spawn(async move {
            context
                .port
                .reserve(&context.scope_id, request("retained", &context.run_id))
                .await
        })
    };
    gate.entered.acquire().await.unwrap().forget();
    // This ordinary snapshot has a newer persistence revision, but was
    // prepared before the acknowledged candidate acquired publication.
    let prepared = state.write().await.control_plane_persist_request();
    gate.release.add_permits(1);
    worker.await.unwrap().unwrap();
    prepared.save().await.unwrap();
    let saved = load_control_plane_snapshot(&config).await.unwrap().unwrap();
    assert!(saved.execution_budgets.scopes[&context.scope_id]
        .reservations
        .contains_key("retained"));
}

#[tokio::test]
async fn host_budget_equal_generation_conflict_is_refused_before_storage() {
    let (state, transactions, config) = fixture("conflict").await;
    let context = standalone_context(
        &state,
        transactions,
        "profile:conflict".into(),
        CancelSignal::new(),
    )
    .await
    .unwrap();
    let persist = {
        let mut guard = state.write().await;
        guard
            .execution_budgets
            .scopes
            .get_mut(&context.scope_id)
            .unwrap()
            .limits
            .max_tool_calls -= 1;
        guard.control_plane_persist_request()
    };
    assert!(persist.save().await.is_err());
    let saved = load_control_plane_snapshot(&config).await.unwrap().unwrap();
    assert_eq!(
        saved.execution_budgets.scopes[&context.scope_id]
            .limits
            .max_tool_calls,
        192
    );
}

#[tokio::test]
async fn host_budget_failed_scope_keeps_conservative_charge_without_blocking_independent_turn() {
    let (state, transactions, config) = fixture("independent").await;
    let first = standalone_context(
        &state,
        transactions.clone(),
        "profile:failed".into(),
        CancelSignal::new(),
    )
    .await
    .unwrap();
    let gate = state
        .write()
        .await
        .install_test_control_plane_save_gate(true);
    gate.release.add_permits(1);
    assert!(first
        .port
        .reserve(&first.scope_id, request("uncertain", &first.run_id))
        .await
        .is_err());
    assert!(state.read().await.execution_budgets.scopes[&first.scope_id]
        .reservations
        .contains_key("uncertain"));
    let fresh = standalone_context(
        &state,
        transactions,
        "profile:independent".into(),
        CancelSignal::new(),
    )
    .await;
    assert!(fresh.is_ok(), "a recoverable storage failure must not strand independent scopes behind an older acknowledgement");
    let fresh = fresh.unwrap();
    assert_ne!(first.scope_id, fresh.scope_id);
    let saved = load_control_plane_snapshot(&config).await.unwrap().unwrap();
    assert!(saved.execution_budgets.scopes[&first.scope_id]
        .reservations
        .contains_key("uncertain"));
    assert!(first.failure().is_some());
}

#[tokio::test]
async fn host_budget_explicit_standalone_limits_cannot_replenish_consumed_authority() {
    let (state, transactions, config) = fixture("configured").await;
    let mut limits = anima_core::execution_limits::ExecutionLimits::default();
    limits.max_model_attempts = 1;
    limits.max_retries = 0;
    let first = standalone_context_with_limits(
        &state,
        transactions.clone(),
        "profile:configured".into(),
        CancelSignal::new(),
        Some(limits),
    )
    .await
    .unwrap();
    first
        .port
        .reserve(&first.scope_id, request("spent", &first.run_id))
        .await
        .unwrap();
    let second = standalone_context_with_limits(
        &state,
        transactions,
        first.run_id.clone(),
        CancelSignal::new(),
        Some(anima_core::execution_limits::ExecutionLimits::default()),
    )
    .await
    .unwrap();
    assert_eq!(first.scope_id, second.scope_id);
    assert_eq!(first.deadline_at_ms, second.deadline_at_ms);
    assert!(second
        .port
        .reserve(&second.scope_id, request("replenished", &second.run_id))
        .await
        .is_err());
    let saved = load_control_plane_snapshot(&config).await.unwrap().unwrap();
    let scope = &saved.execution_budgets.scopes[&first.scope_id];
    assert_eq!(scope.limits.max_model_attempts, 1);
    assert_eq!(scope.consumption().unwrap().model_attempts, 1);
}

#[tokio::test]
async fn host_budget_running_helper_obeys_current_original_owner_limits() {
    let (state, transactions, config) = fixture("owner-tightening").await;
    let (root, helper) = {
        let _transaction = transactions.lock().await;
        let (root, first, persist) = {
            let mut guard = state.write().await;
            let owner = guard
                .create_agent(
                    serde_json::from_value(serde_json::json!({ "name": "owner", "model": "test" }))
                        .unwrap(),
                )
                .unwrap();
            let mut root = record("owner-root");
            root.agent_id = owner.state.id;
            let candidate =
                prepare_run_binding(&guard, &root, anima_core::primitives::now_millis()).unwrap();
            guard.runs.insert(root.clone());
            (root, candidate, guard.control_plane_persist_request())
        };
        save_admission(&state, first, persist).await.unwrap();
        let (helper, next, persist) = {
            let mut guard = state.write().await;
            let agent = guard
                .create_agent(
                    serde_json::from_value(
                        serde_json::json!({ "name": "helper", "model": "test" }),
                    )
                    .unwrap(),
                )
                .unwrap();
            let mut helper = record("owner-helper");
            helper.agent_id = agent.state.id;
            helper.parent_run_id = Some(root.id.clone());
            let candidate =
                prepare_run_binding(&guard, &helper, anima_core::primitives::now_millis()).unwrap();
            guard.runs.insert(helper.clone());
            (helper, candidate, guard.control_plane_persist_request())
        };
        save_admission(&state, next, persist).await.unwrap();
        (root, helper)
    };
    let context = context_for_run(&state, transactions, &helper.id, CancelSignal::new())
        .await
        .unwrap();
    context
        .port
        .reserve(&context.scope_id, request("before-owner-patch", &helper.id))
        .await
        .unwrap();
    let persist = {
        let mut guard = state.write().await;
        let mut limits = ExecutionLimits::default();
        limits.max_model_attempts = 1;
        limits.max_retries = 0;
        guard
            .update_agent(
                &root.agent_id,
                anima_core::AgentConfigUpdate {
                    execution_limits: Some(limits),
                    ..Default::default()
                },
            )
            .unwrap();
        guard.control_plane_persist_request()
    };
    persist.save().await.unwrap();
    context.port.allowance(&context.scope_id).await.unwrap();
    assert!(context
        .port
        .reserve(&context.scope_id, request("after-owner-patch", &helper.id))
        .await
        .is_err());
    let saved = load_control_plane_snapshot(&config).await.unwrap().unwrap();
    let scope = &saved.execution_budgets.scopes[&context.scope_id];
    assert_eq!(scope.limits.max_model_attempts, 1);
    assert_eq!(scope.consumption().unwrap().model_attempts, 1);
    assert!(scope.deadline_at_ms <= context.deadline_at_ms);
}

#[tokio::test]
async fn host_budget_reservation_is_durable_before_grant_and_rejects_foreign_run() {
    let (state, transactions, config) = fixture("grant").await;
    let context = standalone_context(
        &state,
        transactions,
        "profile:grant".into(),
        CancelSignal::new(),
    )
    .await
    .unwrap();
    assert_eq!(
        context
            .port
            .reserve(&context.scope_id, request("first", &context.run_id))
            .await
            .unwrap(),
        ReservationAdmission::NewlyGranted
    );
    let saved = load_control_plane_snapshot(&config).await.unwrap().unwrap();
    assert!(saved.execution_budgets.scopes[&context.scope_id]
        .reservations
        .contains_key("first"));
    assert!(context
        .port
        .reserve(&context.scope_id, request("foreign", "another-run"))
        .await
        .is_err());
    assert_eq!(
        context
            .port
            .reserve(&context.scope_id, request("first", &context.run_id))
            .await
            .unwrap(),
        ReservationAdmission::AlreadyRecorded
    );
}

#[tokio::test]
async fn host_budget_dropped_settlement_ack_keeps_owned_accounting_and_pending_physical_slot() {
    let (state, transactions, config) = fixture("settlement").await;
    let context = standalone_context(
        &state,
        transactions.clone(),
        "profile:settle".into(),
        CancelSignal::new(),
    )
    .await
    .unwrap();
    context
        .port
        .reserve(&context.scope_id, request("first", &context.run_id))
        .await
        .unwrap();
    let held = transactions.clone().lock_owned().await;
    drop(context.port.submit_settlement(
        &context.scope_id,
        ReservationSettlement {
            reservation_id: "first".into(),
            outcome: SettlementOutcome::Unknown { observed: None },
            physical_retired: false,
            observed_usage: None,
        },
    ));
    drop(held);
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let current = state.read().await.execution_budgets.scopes[&context.scope_id].clone();
            if current.reservations["first"].settlement.is_some() {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("owned settlement did not publish within the bounded wait");
    let saved = load_control_plane_snapshot(&config).await.unwrap().unwrap();
    let scope = &saved.execution_budgets.scopes[&context.scope_id];
    assert!(scope.reservations["first"].settlement.is_some());
    assert_eq!(scope.consumption().unwrap().inflight_operations, 1);
    assert_eq!(scope.consumption().unwrap().charged_total_tokens, 150);
}

#[tokio::test]
async fn host_budget_submitted_settlement_retains_state_before_worker_first_poll() {
    let (state, transactions, config) = fixture("custody").await;
    let context = standalone_context(
        &state,
        transactions.clone(),
        "profile:custody".into(),
        CancelSignal::new(),
    )
    .await
    .unwrap();
    context
        .port
        .reserve(&context.scope_id, request("retained", &context.run_id))
        .await
        .unwrap();
    let held = transactions.clone().lock_owned().await;
    let weak = Arc::downgrade(&state);
    drop(context.port.submit_settlement(
        &context.scope_id,
        ReservationSettlement {
            reservation_id: "retained".into(),
            outcome: SettlementOutcome::Known(anima_core::TokenUsage::default()),
            physical_retired: true,
            observed_usage: None,
        },
    ));
    // On the current-thread runtime the submitted worker has not polled yet.
    drop(state);
    assert!(
        weak.upgrade().is_some(),
        "synchronous submission failed to retain the accounting host"
    );
    drop(held);
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while weak.upgrade().is_some() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("owned accounting worker retained a state cycle");
    let saved = load_control_plane_snapshot(&config).await.unwrap().unwrap();
    let settlement = saved.execution_budgets.scopes[&context.scope_id].reservations["retained"]
        .settlement
        .as_ref()
        .unwrap();
    assert!(settlement.physical_retired);
    assert_eq!(
        settlement.outcome,
        SettlementOutcome::Known(anima_core::TokenUsage::default())
    );
}

#[tokio::test]
async fn host_budget_storage_failure_closes_sibling_context_admission() {
    let (state, transactions, _) = fixture("failure").await;
    let first = standalone_context(
        &state,
        transactions.clone(),
        "profile:failure".into(),
        CancelSignal::new(),
    )
    .await
    .unwrap();
    let sibling = context_for_run(&state, transactions, &first.run_id, CancelSignal::new())
        .await
        .unwrap();
    let gate = state
        .write()
        .await
        .install_test_control_plane_save_gate(true);
    gate.release.add_permits(1);
    assert!(first
        .port
        .reserve(&first.scope_id, request("failed", &first.run_id))
        .await
        .is_err());
    assert!(sibling.port.allowance(&sibling.scope_id).await.is_err());
    assert!(sibling.failure().is_some());
}

#[tokio::test]
async fn host_budget_known_late_overrun_is_retained_after_stop_and_closes_scope() {
    let (state, transactions, config) = fixture("late").await;
    let context = standalone_context(
        &state,
        transactions.clone(),
        "profile:late".into(),
        CancelSignal::new(),
    )
    .await
    .unwrap();
    let sibling = context_for_run(&state, transactions, &context.run_id, CancelSignal::new())
        .await
        .unwrap();
    context
        .port
        .reserve(&context.scope_id, request("late", &context.run_id))
        .await
        .unwrap();
    context.cancel.cancel();
    let usage = anima_core::TokenUsage {
        prompt_tokens: 4_000_001,
        completion_tokens: 1,
        ..Default::default()
    };
    // The write accepts known evidence even when further admission is terminal.
    let _ = context
        .port
        .submit_settlement(
            &context.scope_id,
            ReservationSettlement {
                reservation_id: "late".into(),
                outcome: SettlementOutcome::Known(usage.clone()),
                physical_retired: true,
                observed_usage: Some(usage.clone()),
            },
        )
        .await;
    let saved = load_control_plane_snapshot(&config).await.unwrap().unwrap();
    let reservation = &saved.execution_budgets.scopes[&context.scope_id].reservations["late"];
    assert_eq!(
        reservation.settlement.as_ref().unwrap().outcome,
        SettlementOutcome::Known(usage)
    );
    assert!(reservation.settlement.as_ref().unwrap().physical_retired);
    assert!(sibling.port.allowance(&sibling.scope_id).await.is_err());
    assert!(sibling.failure().is_some());
}
