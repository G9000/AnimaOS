//! Turn persistence and restart admission regressions, using isolated profiles.
use super::{
    test_support::{add_chat, coordinator_with, ScriptedModel, Step},
    turn::HostTurnCheckpointSink,
    AgentRunCoordinator,
};
use crate::runs::{RunRecord, RunSource, RunStart, RunStatus};
use anima_core::{
    turn_recovery::{TurnCheckpoint, TurnCheckpointSink, TurnPhase},
    CancelSignal,
};

async fn source(coordinator: &AgentRunCoordinator, agent: &str, session: &str) -> RunRecord {
    add_chat(coordinator, agent, session).await;
    let mut guard = coordinator.state.write().await;
    let mut record = RunRecord::queued(
        RunStart {
            agent_id: agent.into(),
            session_id: session.into(),
            source: RunSource::Web,
            source_ref: None,
            idempotency_key: Some("original-key".into()),
            text: "Finish the original owner request".into(),
            model: "test".into(),
            provider: None,
            parent_run_id: None,
        },
        1,
    );
    record.turn = Some(crate::runs::turn::bind(&guard, &record, false).unwrap());
    record.finish(
        RunStatus::Interrupted,
        Some(crate::runs::RunError::new(
            crate::runs::RESTART_BEFORE_START,
            "restart",
        )),
        2,
    );
    guard.runs.insert(record.clone());
    record
}

#[tokio::test]
async fn automatic_turn_recovery_requires_exact_never_started_current_binding() {
    let (coordinator, agent) = coordinator_with(ScriptedModel::new(vec![])).await;
    let dir = tempfile::tempdir().unwrap();
    coordinator
        .state
        .write()
        .await
        .set_control_plane_store(Some(
            crate::control_plane_store::ControlPlaneStoreConfig::Json(
                dir.path().join("state.json"),
            ),
        ));
    let original = source(&coordinator, &agent, "chat:turn").await;
    let guard = coordinator.state.read().await;
    assert!(crate::runs::turn::automatic_candidate(&guard, &original));
    let mut changed = original.clone();
    changed.input.text.push('!');
    assert!(!crate::runs::turn::automatic_candidate(&guard, &changed));
    changed = original.clone();
    changed.turn.as_mut().unwrap().binding.automatic_eligible = false;
    assert!(!crate::runs::turn::automatic_candidate(&guard, &changed));
    changed = original.clone();
    changed.started_at_ms = Some(1);
    assert!(!crate::runs::turn::automatic_candidate(&guard, &changed));
    changed = original.clone();
    changed.tools_started.push("send_email".into());
    assert!(!crate::runs::turn::automatic_candidate(&guard, &changed));
    changed = original.clone();
    changed.system_followup = Some(true);
    assert!(!crate::runs::turn::automatic_candidate(&guard, &changed));
    changed = original.clone();
    changed.turn = None;
    assert!(!crate::runs::turn::automatic_candidate(&guard, &changed));
    changed = original.clone();
    changed.stop = Some(crate::runs::RunStopRequest { requested_at_ms: 3 });
    assert!(!crate::runs::turn::automatic_candidate(&guard, &changed));
    changed = original.clone();
    changed.source = RunSource::Schedule;
    assert!(!crate::runs::turn::automatic_candidate(&guard, &changed));
    changed = original.clone();
    changed.error.as_mut().unwrap().code = crate::runs::RESTART_DURING_RUN.into();
    assert!(!crate::runs::turn::automatic_candidate(&guard, &changed));
    changed = original.clone();
    changed.turn.as_mut().unwrap().binding.workspace_digest = "f".repeat(64);
    assert!(!crate::runs::turn::automatic_candidate(&guard, &changed));
    changed = original.clone();
    changed.turn.as_mut().unwrap().original_input.text.push('!');
    assert!(crate::runs::RunLedger::validate(&[changed]).is_err());
    drop(guard);
    let mut guard = coordinator.state.write().await;
    let runtime = guard.agents.get_mut(&agent).unwrap();
    let mut config = runtime.config().clone();
    config.model.push_str("-changed");
    runtime.replace_config(config);
    assert!(!crate::runs::turn::automatic_candidate(&guard, &original));
}

#[tokio::test]
async fn automatic_turn_recovery_claims_once_and_keeps_original_input_lineage() {
    let model = ScriptedModel::new(vec![Step::Text(vec!["Recovered owner input"])]);
    let (coordinator, agent) = coordinator_with(model.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    coordinator
        .state
        .write()
        .await
        .set_control_plane_store(Some(
            crate::control_plane_store::ControlPlaneStoreConfig::Json(
                dir.path().join("state.json"),
            ),
        ));
    let original = source(&coordinator, &agent, "chat:auto").await;
    let ((), ()) = tokio::join!(
        coordinator.auto_recover_unstarted(),
        coordinator.auto_recover_unstarted()
    );
    coordinator.auto_recover_unstarted().await;
    let guard = coordinator.state.read().await;
    let source = guard.runs.get(&original.id).unwrap();
    let recovery = source
        .recovery
        .as_ref()
        .expect("one durable automatic claim");
    let child = guard.runs.get(&recovery.continuation_run_id).unwrap();
    assert_eq!(child.input, original.input);
    assert_eq!(child.system_followup, Some(false));
    assert_eq!(child.recovery_attempt, 1);
    assert_eq!(
        child.turn.as_ref().unwrap().binding.root_run_id,
        original.id
    );
    assert_eq!(
        child.turn.as_ref().unwrap().binding.original_input_digest,
        original
            .turn
            .as_ref()
            .unwrap()
            .binding
            .original_input_digest
    );
    assert!(!child.turn.as_ref().unwrap().binding.automatic_eligible);
    assert_eq!(guard.runs.for_session(&agent, "chat:auto").len(), 2);
}

#[tokio::test]
async fn failed_checkpoint_save_never_publishes_provisional_progress() {
    let (coordinator, agent) = coordinator_with(ScriptedModel::new(vec![])).await;
    let dir = tempfile::tempdir().unwrap();
    coordinator
        .state
        .write()
        .await
        .set_control_plane_store(Some(
            crate::control_plane_store::ControlPlaneStoreConfig::Json(
                dir.path().join("state.json"),
            ),
        ));
    let mut record = source(&coordinator, &agent, "chat:sink").await;
    record.start("test".into(), None, 3);
    record.turn.as_mut().unwrap().terminal = None;
    coordinator.state.write().await.runs.insert(record.clone());
    let binding = record.turn.as_ref().unwrap().binding.clone();
    let checkpoint = TurnCheckpoint::bounded(binding, 1, TurnPhase::BeforeModel, 1, &[]);
    let sink = HostTurnCheckpointSink::new(
        &coordinator.state,
        coordinator.control_plane_transactions(),
        crate::runs::RunLink {
            run_id: record.id.clone(),
            agent_id: agent.clone(),
            session_id: record.session_id.clone(),
        },
        CancelSignal::new(),
    );
    let gate = coordinator
        .state
        .write()
        .await
        .install_test_control_plane_save_gate(true);
    let save = tokio::spawn(async move { sink.save(checkpoint).await });
    tokio::time::timeout(std::time::Duration::from_secs(5), gate.entered.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    assert!(coordinator
        .state
        .read()
        .await
        .runs
        .get(&record.id)
        .unwrap()
        .turn
        .as_ref()
        .unwrap()
        .checkpoint
        .is_none());
    gate.release.add_permits(1);
    assert!(save.await.unwrap().is_err());
    assert!(coordinator
        .state
        .read()
        .await
        .runs
        .get(&record.id)
        .unwrap()
        .turn
        .as_ref()
        .unwrap()
        .checkpoint
        .is_none());
}

#[tokio::test]
async fn completed_turn_seals_reply_identity_and_never_recovers() {
    let (coordinator, agent) = coordinator_with(ScriptedModel::new(vec![Step::Text(vec![
        "Committed reply",
    ])]))
    .await;
    add_chat(&coordinator, &agent, "chat:terminal").await;
    coordinator
        .run(super::test_support::chat_request(
            &agent,
            "chat:terminal",
            "Reply once",
        ))
        .await
        .unwrap();
    let guard = coordinator.state.read().await;
    let record = guard
        .runs
        .for_session(&agent, "chat:terminal")
        .into_iter()
        .next()
        .unwrap();
    assert_eq!(record.status, RunStatus::Completed);
    let turn = record
        .turn
        .as_ref()
        .expect("ordinary runtime checkpoints are integrated");
    let terminal = turn
        .terminal
        .as_ref()
        .expect("same final commit seals terminal identity");
    assert_eq!(terminal.reply_message_id, record.reply_message_id);
    assert_eq!(
        terminal.reply_sha256,
        Some(crate::runs::committed_output_sha256("Committed reply"))
    );
    assert!(turn.checkpoint.as_ref().unwrap().sequence >= 2);
    assert!(!crate::runs::turn::automatic_candidate(&guard, record));
    let mut invalid = record.clone();
    invalid
        .turn
        .as_mut()
        .unwrap()
        .terminal
        .as_mut()
        .unwrap()
        .reply_sha256 = Some("f".repeat(64));
    assert!(crate::runs::RunLedger::validate(&[invalid]).is_err());
}

#[tokio::test]
async fn acknowledged_turn_checkpoint_survives_an_overtaking_snapshot_without_resurrection() {
    let (coordinator, agent) = coordinator_with(ScriptedModel::new(vec![])).await;
    let dir = tempfile::tempdir().unwrap();
    let store =
        crate::control_plane_store::ControlPlaneStoreConfig::Json(dir.path().join("state.json"));
    coordinator
        .state
        .write()
        .await
        .set_control_plane_store(Some(store.clone()));
    let mut record = source(&coordinator, &agent, "chat:ack").await;
    record.start("test".into(), None, 3);
    coordinator.state.write().await.runs.insert(record.clone());
    let checkpoint = TurnCheckpoint::bounded(
        record.turn.as_ref().unwrap().binding.clone(),
        1,
        TurnPhase::BeforeModel,
        1,
        &[],
    );
    let sink = HostTurnCheckpointSink::new(
        &coordinator.state,
        coordinator.control_plane_transactions(),
        crate::runs::RunLink {
            run_id: record.id.clone(),
            agent_id: agent.clone(),
            session_id: record.session_id.clone(),
        },
        CancelSignal::new(),
    );
    let gate = coordinator
        .state
        .write()
        .await
        .install_test_control_plane_save_gate(false);
    let saving = tokio::spawn(async move { sink.save(checkpoint).await });
    tokio::time::timeout(std::time::Duration::from_secs(5), gate.entered.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    let newer_without_publication = {
        let mut guard = coordinator.state.write().await;
        assert!(guard
            .runs
            .get(&record.id)
            .unwrap()
            .turn
            .as_ref()
            .unwrap()
            .checkpoint
            .is_none());
        guard.control_plane_persist_request()
    };
    gate.release.add_permits(1);
    saving.await.unwrap().unwrap();
    newer_without_publication.save().await.unwrap();
    let saved = crate::control_plane_store::load_control_plane_snapshot(&store)
        .await
        .unwrap()
        .unwrap();
    let saved_run = saved.runs.iter().find(|r| r.id == record.id).unwrap();
    assert_eq!(
        saved_run
            .turn
            .as_ref()
            .unwrap()
            .checkpoint
            .as_ref()
            .unwrap()
            .sequence,
        1
    );
    let acknowledged = saved_run.turn.clone().unwrap();
    let mut terminal = record.clone();
    terminal.finish(RunStatus::Failed, None, 4);
    let original_terminal = terminal.turn.as_ref().unwrap().terminal.clone();
    terminal.merge_acknowledged_turn(&acknowledged);
    assert_eq!(terminal.turn.as_ref().unwrap().terminal, original_terminal);
    let mut legacy = record.clone();
    legacy.turn = None;
    legacy.merge_acknowledged_turn(&acknowledged);
    assert!(legacy.turn.is_none());
    let deleted = {
        let mut guard = coordinator.state.write().await;
        guard.runs.remove(&record.id);
        guard.control_plane_persist_request()
    };
    deleted.save().await.unwrap();
    assert!(
        !crate::control_plane_store::load_control_plane_snapshot(&store)
            .await
            .unwrap()
            .unwrap()
            .runs
            .iter()
            .any(|r| r.id == record.id)
    );
}

#[tokio::test]
async fn turn_lineage_rejects_missing_claim_and_cross_owner_binding() {
    let (coordinator, agent) = coordinator_with(ScriptedModel::new(vec![])).await;
    let mut original = source(&coordinator, &agent, "chat:lineage").await;
    let mut child = original.clone();
    child.id = "run_lineage_child".into();
    child.recovery_of = Some(original.id.clone());
    child.recovery_attempt = 1;
    child.turn.as_mut().unwrap().binding.attempt_run_id = child.id.clone();
    child.turn.as_mut().unwrap().binding.automatic_eligible = false;
    original.recovery = Some(crate::runs::recovery::RecoveryRecord {
        fingerprint: crate::runs::recovery::summary(&original).fingerprint,
        continuation_run_id: child.id.clone(),
        reviewed_at_ms: 3,
    });
    assert!(crate::runs::RunLedger::validate(&[original.clone(), child.clone()]).is_ok());
    let mut unclaimed = original.clone();
    unclaimed.recovery = None;
    assert!(crate::runs::RunLedger::validate(&[unclaimed, child.clone()]).is_err());
    child.agent_id = "foreign-owner".into();
    child.turn.as_mut().unwrap().binding.agent_id = child.agent_id.clone();
    assert!(crate::runs::RunLedger::validate(&[original, child]).is_err());
}

#[tokio::test]
async fn active_turn_keeps_frozen_config_and_can_settle_a_response_with_orphan_approval() {
    let (coordinator, agent) = coordinator_with(ScriptedModel::new(vec![])).await;
    let mut record = source(&coordinator, &agent, "chat:frozen").await;
    record.start("test".into(), None, 3);
    let binding = record.turn.as_ref().unwrap().binding.clone();
    {
        let mut guard = coordinator.state.write().await;
        guard.runs.insert(record.clone());
        guard
            .agents
            .get_mut(&agent)
            .unwrap()
            .update_config(anima_core::AgentConfigUpdate {
                system: Some("Future turns use a new persona".into()),
                ..Default::default()
            });
        assert!(crate::runs::turn::current(&guard, &record).is_err());
        assert!(crate::runs::turn::active(&guard, &record).is_ok());
    }
    let sink = HostTurnCheckpointSink::new(
        &coordinator.state,
        coordinator.control_plane_transactions(),
        crate::runs::RunLink {
            run_id: record.id.clone(),
            agent_id: agent,
            session_id: record.session_id.clone(),
        },
        CancelSignal::new(),
    );
    sink.save(TurnCheckpoint::bounded(
        binding.clone(),
        1,
        TurnPhase::BeforeModel,
        1,
        &[],
    ))
    .await
    .unwrap();
    coordinator
        .state
        .write()
        .await
        .runs
        .get_mut(&record.id)
        .unwrap()
        .status = RunStatus::AwaitingApproval;
    sink.save(TurnCheckpoint::bounded(
        binding.clone(),
        2,
        TurnPhase::ModelResponded,
        1,
        &[],
    ))
    .await
    .unwrap();
    assert_eq!(
        coordinator
            .state
            .read()
            .await
            .runs
            .get(&record.id)
            .unwrap()
            .turn
            .as_ref()
            .unwrap()
            .binding,
        binding
    );
}
