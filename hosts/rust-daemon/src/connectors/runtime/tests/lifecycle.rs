use super::*;

#[tokio::test]
async fn replacement_load_failure_keeps_the_previous_worker_and_state() {
    let state = state_with_agent();
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    let credentials = Arc::new(FailingCredentialStore::default());
    let manager = manager(
        Arc::clone(&state),
        credentials.clone(),
        Arc::new(FakeTransport::default()),
    );
    let connector = manager
        .create(agent_id, TelegramBotToken::parse("42:old-token").unwrap())
        .await
        .unwrap();
    credentials.fail_next_load.store(true, Ordering::SeqCst);

    assert_eq!(
        manager
            .replace_token(
                connector.id.clone(),
                TelegramBotToken::parse("84:new-token").unwrap(),
            )
            .await
            .unwrap_err(),
        super::ConnectorManagerError::Credential
    );
    assert_eq!(manager.worker_count().await, 1);
    assert_eq!(
        manager.status(&connector.id).await,
        Some(super::ConnectorRuntimeStatus::Pairing)
    );
    assert_eq!(state.read().await.connectors[&connector.id], connector);
    manager.stop_worker(&connector.id).await.unwrap();
}

#[tokio::test]
async fn replacement_put_failure_restores_the_previous_worker_token_and_status() {
    let state = state_with_agent();
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    let credentials = Arc::new(FailingCredentialStore::default());
    let manager = manager(
        Arc::clone(&state),
        credentials.clone(),
        Arc::new(FakeTransport::default()),
    );
    let connector = manager
        .create(agent_id, TelegramBotToken::parse("42:old-token").unwrap())
        .await
        .unwrap();
    credentials.fail_next_put.store(true, Ordering::SeqCst);

    assert_eq!(
        manager
            .replace_token(
                connector.id.clone(),
                TelegramBotToken::parse("84:new-token").unwrap(),
            )
            .await
            .unwrap_err(),
        super::ConnectorManagerError::Credential
    );
    assert_eq!(manager.worker_count().await, 1);
    assert_eq!(
        manager.status(&connector.id).await,
        Some(super::ConnectorRuntimeStatus::Pairing)
    );
    assert_eq!(
        credentials
            .load(&connector.id)
            .await
            .unwrap()
            .unwrap()
            .expose(),
        "42:old-token"
    );
    assert_eq!(state.read().await.connectors[&connector.id], connector);
    manager.shutdown().await;
}

#[tokio::test]
async fn failed_replacement_never_lets_old_worker_deliver_with_uncommitted_new_token() {
    let state = state_with_agent();
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    let fixed_worker_delivery = Arc::new(AtomicBool::new(false));
    let worker_load_entered = Arc::new(Semaphore::new(0));
    let replacement_put_done = Arc::new(Semaphore::new(0));
    let send_entered = Arc::new(Semaphore::new(0));
    let release_send = Arc::new(Semaphore::new(0));
    let credentials = Arc::new(ReplacementRaceCredentialStore {
        inner: InMemoryCredentialStore::default(),
        fixed_worker_delivery: Arc::clone(&fixed_worker_delivery),
        worker_load_entered: Arc::clone(&worker_load_entered),
        replacement_put_done: Arc::clone(&replacement_put_done),
        load_calls: AtomicUsize::new(0),
        put_calls: AtomicUsize::new(0),
    });
    let transport = Arc::new(ReplacementRaceTransport {
        fixed_worker_delivery: Arc::clone(&fixed_worker_delivery),
        send_entered: Arc::clone(&send_entered),
        release_send: Arc::clone(&release_send),
        gate_first_send: AtomicBool::new(true),
        sent_tokens: Mutex::new(Vec::new()),
    });
    let manager = manager(Arc::clone(&state), credentials, transport.clone());
    let old_token = TelegramBotToken::parse("42:old-token").unwrap();
    let connector = manager
        .create(agent_id.clone(), old_token.clone())
        .await
        .unwrap();
    manager.stop_worker(&connector.id).await.unwrap();
    {
        let mut state = state.write().await;
        state
            .connectors
            .get_mut(&connector.id)
            .unwrap()
            .approved_chat = Some(TelegramChatMetadata {
            id: "202".into(),
            kind: TelegramChatKind::Private,
            title: None,
            username: Some("operator".into()),
        });
        state.outbound.insert(
            "replacement-race-outbound".into(),
            crate::connectors::TelegramOutboundRecord {
                id: "replacement-race-outbound".into(),
                connector_id: connector.id.clone(),
                agent_id,
                room_id: connector.room_id.clone(),
                assistant_message_id: "replacement-race-assistant".into(),
                text: "old worker response".into(),
                created_at_ms: super::now_ms(),
                delivered_at_ms: None,
                attempts: 0,
                delivery_state: crate::connectors::OutboundDeliveryState::Pending,
                message_pruned: false,
            },
        );
    }
    let temporary = std::env::temp_dir().join(format!(
        "anima-replacement-token-race-{}-{}",
        std::process::id(),
        super::now_ms()
    ));
    std::fs::create_dir_all(&temporary).unwrap();
    let invalid_path = temporary.join("cannot-replace-directory");
    std::fs::create_dir(&invalid_path).unwrap();
    state.write().await.set_control_plane_store(Some(
        crate::control_plane_store::ControlPlaneStoreConfig::Json(invalid_path),
    ));
    manager
        .start_worker(connector.id.clone(), old_token)
        .await
        .unwrap();

    tokio::select! {
        permit = worker_load_entered.acquire() => permit.unwrap().forget(),
        permit = send_entered.acquire() => permit.unwrap().forget(),
    }
    let replacing = {
        let manager = manager.clone();
        let connector_id = connector.id.clone();
        tokio::spawn(async move {
            manager
                .replace_token(
                    connector_id,
                    TelegramBotToken::parse("84:new-token").unwrap(),
                )
                .await
        })
    };
    replacement_put_done.acquire().await.unwrap().forget();
    send_entered.acquire().await.unwrap().forget();
    release_send.add_permits(1);

    assert_eq!(
        replacing.await.unwrap().unwrap_err(),
        super::ConnectorManagerError::Persistence
    );
    assert!(!transport.sent_tokens.lock().unwrap().is_empty());
    assert!(transport
        .sent_tokens
        .lock()
        .unwrap()
        .iter()
        .all(|token| token == "42:old-token"));

    manager.shutdown().await;
    std::fs::remove_dir_all(temporary).unwrap();
}

#[tokio::test]
async fn slow_failed_replacement_put_never_interrupts_the_previous_worker() {
    let state = state_with_agent();
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    let entered = Arc::new(Semaphore::new(0));
    let release = Arc::new(Semaphore::new(0));
    let credentials = Arc::new(GatePutCredentialStore {
        inner: InMemoryCredentialStore::default(),
        gate_put: AtomicBool::new(false),
        fail_put: AtomicBool::new(false),
        entered: Arc::clone(&entered),
        release: Arc::clone(&release),
    });
    let transport = Arc::new(FakeTransport::default());
    let manager = manager(Arc::clone(&state), credentials.clone(), transport.clone());
    let connector = manager
        .create(agent_id, TelegramBotToken::parse("42:old-token").unwrap())
        .await
        .unwrap();
    credentials.gate_put.store(true, Ordering::SeqCst);
    credentials.fail_put.store(true, Ordering::SeqCst);

    let replacing_manager = manager.clone();
    let connector_id = connector.id.clone();
    let replacement = tokio::spawn(async move {
        replacing_manager
            .replace_token(
                connector_id,
                TelegramBotToken::parse("84:new-token").unwrap(),
            )
            .await
    });
    entered
        .acquire()
        .await
        .expect("replacement should enter the credential write")
        .forget();

    assert_eq!(
        manager.worker_count().await,
        1,
        "the old poller must remain registered throughout credential preflight"
    );
    let observed_polls = transport.poll_calls.load(Ordering::SeqCst);
    tokio::time::timeout(Duration::from_millis(500), async {
        while transport.poll_calls.load(Ordering::SeqCst) == observed_polls {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the old poller should keep polling while the vault write is blocked");
    assert_eq!(
        manager.status(&connector.id).await,
        Some(ConnectorRuntimeStatus::Pairing)
    );

    release.add_permits(1);
    assert_eq!(
        replacement.await.unwrap().unwrap_err(),
        super::ConnectorManagerError::Credential
    );
    assert_eq!(manager.worker_count().await, 1);
    assert_eq!(
        credentials
            .load(&connector.id)
            .await
            .unwrap()
            .unwrap()
            .expose(),
        "42:old-token"
    );
    assert_eq!(state.read().await.connectors[&connector.id], connector);
    manager.shutdown().await;
}

#[tokio::test]
async fn replacement_state_failure_restores_old_metadata_credential_and_worker() {
    let state = state_with_agent();
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    let entered = Arc::new(Semaphore::new(0));
    let release = Arc::new(Semaphore::new(0));
    let credentials = Arc::new(GatePutCredentialStore {
        inner: InMemoryCredentialStore::default(),
        gate_put: AtomicBool::new(false),
        fail_put: AtomicBool::new(false),
        entered: Arc::clone(&entered),
        release: Arc::clone(&release),
    });
    let manager = manager(
        Arc::clone(&state),
        credentials.clone(),
        Arc::new(FakeTransport::default()),
    );
    let connector = manager
        .create(agent_id, TelegramBotToken::parse("42:old-token").unwrap())
        .await
        .unwrap();
    credentials.gate_put.store(true, Ordering::SeqCst);

    let replacing_manager = manager.clone();
    let connector_id = connector.id.clone();
    let replacement = tokio::spawn(async move {
        replacing_manager
            .replace_token(
                connector_id,
                TelegramBotToken::parse("84:new-token").unwrap(),
            )
            .await
    });
    entered
        .acquire()
        .await
        .expect("replacement should enter the credential write")
        .forget();
    state.write().await.connectors.remove(&connector.id);
    release.add_permits(1);

    assert_eq!(
        replacement.await.unwrap().unwrap_err(),
        super::ConnectorManagerError::ConnectorNotFound
    );
    assert_eq!(state.read().await.connectors[&connector.id], connector);
    assert_eq!(
        credentials
            .load(&connector.id)
            .await
            .unwrap()
            .unwrap()
            .expose(),
        "42:old-token"
    );
    assert_eq!(manager.worker_count().await, 1);
    assert_eq!(
        manager.status(&connector.id).await,
        Some(ConnectorRuntimeStatus::Pairing)
    );
    manager.shutdown().await;
}

#[tokio::test]
async fn replacement_persist_failure_restores_previous_vault_state_metadata_and_worker() {
    let state = state_with_agent();
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    let credentials = Arc::new(InMemoryCredentialStore::default());
    let manager = manager(
        Arc::clone(&state),
        credentials.clone(),
        Arc::new(FakeTransport::default()),
    );
    let connector = manager
        .create(agent_id, TelegramBotToken::parse("42:old-token").unwrap())
        .await
        .unwrap();
    let invalid_path = invalid_snapshot_directory("replace");
    state.write().await.set_control_plane_store(Some(
        crate::control_plane_store::ControlPlaneStoreConfig::Json(invalid_path.clone()),
    ));

    assert_eq!(
        manager
            .replace_token(
                connector.id.clone(),
                TelegramBotToken::parse("84:new-token").unwrap(),
            )
            .await
            .unwrap_err(),
        super::ConnectorManagerError::Persistence
    );
    assert_eq!(manager.worker_count().await, 1);
    assert_eq!(
        manager.status(&connector.id).await,
        Some(super::ConnectorRuntimeStatus::Pairing)
    );
    assert_eq!(
        credentials
            .load(&connector.id)
            .await
            .unwrap()
            .unwrap()
            .expose(),
        "42:old-token"
    );
    assert_eq!(state.read().await.connectors[&connector.id], connector);
    manager.shutdown().await;
    std::fs::remove_dir_all(invalid_path).unwrap();
}

#[tokio::test]
async fn replace_token_verifies_and_updates_bot_identity_without_changing_room() {
    let state = state_with_agent();
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    let credentials = Arc::new(InMemoryCredentialStore::default());
    let manager = manager(
        Arc::clone(&state),
        credentials.clone(),
        Arc::new(FakeTransport::default()),
    );
    let connector = manager
        .create(agent_id, TelegramBotToken::parse("42:old-token").unwrap())
        .await
        .unwrap();

    let replaced = manager
        .replace_token(
            connector.id.clone(),
            TelegramBotToken::parse("99:new-token").unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(replaced.id, connector.id);
    assert_eq!(replaced.room_id, connector.room_id);
    assert_eq!(replaced.bot.id, "99");
    assert_eq!(
        credentials
            .load(&connector.id)
            .await
            .unwrap()
            .unwrap()
            .expose(),
        "99:new-token"
    );
    manager.shutdown().await;
}

#[tokio::test]
async fn agent_cleanup_tombstones_connectors_and_removes_agent_in_one_transaction() {
    let state = state_with_agent();
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    let manager = manager(
        Arc::clone(&state),
        Arc::new(InMemoryCredentialStore::default()),
        Arc::new(FakeTransport::default()),
    );
    let connector = manager
        .create(
            agent_id.clone(),
            TelegramBotToken::parse("42:cleanup-token").unwrap(),
        )
        .await
        .unwrap();

    manager.delete_agent(agent_id.clone()).await.unwrap();
    assert!(!state.read().await.connectors[&connector.id].is_active());
    assert!(state.read().await.get_agent(&agent_id).is_none());
    // Controller ruling 2 (M2 pre-flight audit): the deletion is recorded
    // durably in the same save as the agent's removal.
    assert!(
        state
            .read()
            .await
            .pending_history_deletions
            .iter()
            .any(|deletion| deletion.agent_id == agent_id && deletion.session_id.is_none()),
        "the agent's history deletion is recorded in the saved snapshot"
    );
    manager.shutdown().await;
}

#[tokio::test]
async fn agent_cleanup_final_save_failure_restores_agent_connector_credential_and_worker() {
    let state = state_with_agent();
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    let credentials = Arc::new(InMemoryCredentialStore::default());
    let manager = manager(
        Arc::clone(&state),
        credentials.clone(),
        Arc::new(FakeTransport::default()),
    );
    let connector = manager
        .create(
            agent_id.clone(),
            TelegramBotToken::parse("42:agent-delete-token").unwrap(),
        )
        .await
        .unwrap();
    let invalid_path = invalid_snapshot_directory("agent-delete");
    state.write().await.set_control_plane_store(Some(
        crate::control_plane_store::ControlPlaneStoreConfig::Json(invalid_path.clone()),
    ));

    assert_eq!(
        manager.delete_agent(agent_id.clone()).await.unwrap_err(),
        super::ConnectorManagerError::Persistence
    );

    assert!(state.read().await.get_agent(&agent_id).is_some());
    assert_eq!(state.read().await.connectors[&connector.id], connector);
    assert_eq!(
        credentials
            .load(&connector.id)
            .await
            .unwrap()
            .unwrap()
            .expose(),
        "42:agent-delete-token"
    );
    assert_eq!(manager.worker_count().await, 1);
    // Controller ruling 2 (M2 pre-flight audit): a failed save leaves no
    // pending history deletion behind, matching the restored agent.
    assert!(
        state.read().await.pending_history_deletions.is_empty(),
        "a failed save leaves no pending entry"
    );
    manager.shutdown().await;
    std::fs::remove_dir_all(invalid_path).unwrap();
}

#[tokio::test]
async fn deleting_an_agent_through_the_manager_is_durable_and_a_flush_clears_its_rows() {
    // Fix round 1 (M2 review): the two tests above never exercise a real
    // control-plane store or a flush, so deleting `remove_terminal_for_agent`
    // or `enqueue_agent_deletion`, or reordering `record_history_deletion`
    // after the persist request is built, would leave every test green.
    // This test goes through `ConnectorManager::delete_agent` itself, with a
    // real JSON store, a mirrored message, a terminal run, and a session
    // record, then flushes with the coordinator's own transaction mutex.
    use crate::control_plane_store::{load_control_plane_snapshot, ControlPlaneStoreConfig};
    use crate::history::MessagePageQuery;
    use crate::runs::{RunRecord, RunSource as RunLedgerSource, RunStart, RunStatus};
    use crate::sessions::test_support::{message, seed_messages};
    use crate::sessions::{SessionKind, SessionOrigin, SessionRecord, TitleSource};
    use anima_core::MessageRole;

    let state = state_with_agent();
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    let survivor_id = state
        .write()
        .await
        .create_agent(AgentConfig {
            name: "survivor".into(),
            ..test_config()
        })
        .unwrap()
        .state
        .id;
    let runs = AgentRunCoordinator::new(Arc::clone(&state), Arc::new(Semaphore::new(4)));
    let manager = ConnectorManager::new(
        Arc::clone(&state),
        runs.clone(),
        Arc::new(InMemoryCredentialStore::default()),
        Arc::new(FakeTransport::default()),
    );
    let transactions = runs.control_plane_transactions();

    state.write().await.sessions.insert(SessionRecord::new(
        &agent_id,
        "chat:one",
        SessionKind::Chat,
        SessionOrigin::Web,
        "Plans".into(),
        TitleSource::FirstMessage,
        1,
    ));
    seed_messages(
        &mut *state.write().await,
        &agent_id,
        vec![message(
            &agent_id,
            "m1",
            "chat:one",
            MessageRole::User,
            "hello",
            1,
        )],
    );
    seed_messages(
        &mut *state.write().await,
        &survivor_id,
        vec![message(
            &survivor_id,
            "s1",
            "chat:one",
            MessageRole::User,
            "unrelated",
            1,
        )],
    );
    let history = state.read().await.history.clone();
    history.flush_once(&state, &transactions, 1).await.unwrap();
    let mirrored = history
        .store()
        .page_messages(&MessagePageQuery {
            agent_id: agent_id.clone(),
            session_id: "chat:one".into(),
            before: None,
            limit: 10,
            include_hidden: true,
        })
        .await
        .unwrap();
    assert_eq!(mirrored.len(), 1, "the message is mirrored before deleting");
    assert!(
        history.is_mirrored("m1"),
        "the message is mirrored before deleting"
    );

    let run_id = {
        let mut guard = state.write().await;
        let mut run = RunRecord::running(
            RunStart {
                agent_id: agent_id.clone(),
                session_id: "chat:one".into(),
                source: RunLedgerSource::Api,
                source_ref: None,
                idempotency_key: None,
                text: "hi".into(),
                model: "gpt-5.4".into(),
                provider: None,
                parent_run_id: None,
            },
            1,
        );
        run.finish(RunStatus::Completed, None, 1);
        let id = run.id.clone();
        guard.runs.insert(run);
        id
    };

    let path = std::env::temp_dir().join(format!(
        "anima-agent-delete-durable-{}.json",
        uuid::Uuid::new_v4()
    ));
    let store = ControlPlaneStoreConfig::Json(path.clone());
    state
        .write()
        .await
        .set_control_plane_store(Some(store.clone()));

    manager.delete_agent(agent_id.clone()).await.unwrap();

    // D1 (mirrored-set leak, final fix wave): `delete_agent` must forget
    // the deleted agent's ids from `HistoryService`'s mirrored set itself,
    // the same as a session delete does, or they never leave it (the
    // rows are gone from the store immediately below, before a restart
    // could ever repopulate the set from scratch).
    assert!(
        !history.is_mirrored("m1"),
        "delete_agent forgets the deleted agent's ids from the mirrored set"
    );

    // The saved snapshot (not just in-memory state) holds the deletion and
    // excludes the agent's runs and session records.
    let saved = load_control_plane_snapshot(&store).await.unwrap().unwrap();
    assert!(
        saved
            .pending_history_deletions
            .iter()
            .any(|deletion| deletion.agent_id == agent_id && deletion.session_id.is_none()),
        "the saved snapshot records the deletion"
    );
    assert!(
        saved.runs.iter().all(|run| run.agent_id != agent_id),
        "the saved snapshot holds none of the agent's runs"
    );
    assert!(
        saved
            .sessions
            .iter()
            .all(|session| session.agent_id != agent_id),
        "the saved snapshot holds none of the agent's sessions"
    );
    assert!(
        state
            .read()
            .await
            .sessions
            .get(&agent_id, "chat:one")
            .is_none(),
        "the agent's session records leave the registry immediately, not just the snapshot"
    );
    // Neither the saved snapshot nor a flush would fail to exclude a
    // lingering run of a deleted agent -- both independently filter to
    // live agents already -- so this is the only assertion that would
    // catch a regressed (skipped) `remove_terminal_for_agent` call.
    assert!(
        state.read().await.runs.get(&run_id).is_none(),
        "the agent's terminal run leaves the ledger immediately, not just the snapshot"
    );

    let report = history.flush_once(&state, &transactions, 2).await.unwrap();
    assert_eq!(report.deletions, 1);

    let after = history
        .store()
        .page_messages(&MessagePageQuery {
            agent_id: agent_id.clone(),
            session_id: "chat:one".into(),
            before: None,
            limit: 10,
            include_hidden: true,
        })
        .await
        .unwrap();
    assert!(after.is_empty(), "the flush removes the agent's rows");
    assert_eq!(
        history.store().get_run(&run_id).await.unwrap(),
        None,
        "the flush never mirrors the deleted agent's run"
    );
    assert!(
        state.read().await.pending_history_deletions.is_empty(),
        "the flush clears the pending entry once applied"
    );
    let survivor_rows = history
        .store()
        .page_messages(&MessagePageQuery {
            agent_id: survivor_id.clone(),
            session_id: "chat:one".into(),
            before: None,
            limit: 10,
            include_hidden: true,
        })
        .await
        .unwrap();
    assert_eq!(
        survivor_rows.len(),
        1,
        "an unrelated agent's rows are untouched"
    );

    manager.shutdown().await;
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn delete_persist_failure_restores_credential_metadata_status_and_worker() {
    let state = state_with_agent();
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    let credentials = Arc::new(InMemoryCredentialStore::default());
    let manager = manager(
        Arc::clone(&state),
        credentials.clone(),
        Arc::new(FakeTransport::default()),
    );
    let connector = manager
        .create(agent_id, TelegramBotToken::parse("42:old-token").unwrap())
        .await
        .unwrap();
    let invalid_path = invalid_snapshot_directory("delete");
    state.write().await.set_control_plane_store(Some(
        crate::control_plane_store::ControlPlaneStoreConfig::Json(invalid_path.clone()),
    ));

    assert_eq!(
        manager.delete(connector.id.clone()).await.unwrap_err(),
        super::ConnectorManagerError::Persistence
    );
    assert_eq!(state.read().await.connectors[&connector.id], connector);
    assert_eq!(
        credentials
            .load(&connector.id)
            .await
            .unwrap()
            .unwrap()
            .expose(),
        "42:old-token"
    );
    assert_eq!(manager.worker_count().await, 1);
    assert_eq!(
        manager.status(&connector.id).await,
        Some(super::ConnectorRuntimeStatus::Pairing)
    );
    manager.shutdown().await;
    std::fs::remove_dir_all(invalid_path).unwrap();
}

#[tokio::test]
async fn uncertain_credential_delete_stops_worker_and_preserves_metadata_for_reconciliation() {
    let state = state_with_agent();
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    let manager = manager(
        Arc::clone(&state),
        Arc::new(UncertainDeleteStore::default()),
        Arc::new(FakeTransport::default()),
    );
    let connector = manager
        .create(
            agent_id,
            TelegramBotToken::parse("42:uncertain-delete").unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        manager.delete(connector.id.clone()).await.unwrap_err(),
        super::ConnectorManagerError::CredentialStateUncertain
    );
    assert!(state.read().await.connectors[&connector.id].is_active());
    assert_eq!(manager.worker_count().await, 0);
    assert_eq!(
        manager.status(&connector.id).await,
        Some(super::ConnectorRuntimeStatus::Reconciling)
    );
    manager.shutdown().await;
}

#[tokio::test]
async fn deletion_archives_completed_history_purges_pending_work_and_disables_schedules() {
    use crate::connectors::{OutboundDeliveryState, TelegramOutboundRecord};
    use crate::schedules::{ScheduleTarget, ScheduleTrigger, ScheduledPromptRecord};

    let state = state_with_agent();
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    let manager = manager(
        Arc::clone(&state),
        Arc::new(InMemoryCredentialStore::default()),
        Arc::new(FakeTransport::default()),
    );
    let connector = manager
        .create(
            agent_id.clone(),
            TelegramBotToken::parse("42:archive-token").unwrap(),
        )
        .await
        .unwrap();
    manager.stop_worker(&connector.id).await.unwrap();
    manager
        .accept_batch(
            connector.id.clone(),
            TelegramUpdateBatch {
                updates: vec![text_update(1, "202", "pair")],
                next_update_id: 2,
            },
        )
        .await
        .unwrap();
    manager.approve_pending(connector.id.clone()).await.unwrap();
    manager
        .accept_batch(
            connector.id.clone(),
            TelegramUpdateBatch {
                updates: (2..=5)
                    .map(|id| text_update(id, "202", &format!("message-{id}")))
                    .collect(),
                next_update_id: 6,
            },
        )
        .await
        .unwrap();
    let now = super::now_ms();
    {
        let mut guard = state.write().await;
        for (update_id, processing_state) in [
            (2, InboundProcessingState::Processed),
            (3, InboundProcessingState::Rejected),
            (4, InboundProcessingState::Received),
            (5, InboundProcessingState::Processing),
        ] {
            guard
                .inbound
                .get_mut(&(connector.id.clone(), update_id))
                .unwrap()
                .processing_state = processing_state;
        }
        for (id, delivery_state, delivered_at_ms) in [
            ("delivered", OutboundDeliveryState::Delivered, Some(now)),
            ("pending", OutboundDeliveryState::Pending, None),
            ("failed", OutboundDeliveryState::Failed, None),
        ] {
            guard.outbound.insert(
                id.into(),
                TelegramOutboundRecord {
                    id: id.into(),
                    connector_id: connector.id.clone(),
                    agent_id: agent_id.clone(),
                    room_id: connector.room_id.clone(),
                    assistant_message_id: format!("assistant-{id}"),
                    text: id.into(),
                    created_at_ms: now,
                    delivered_at_ms,
                    attempts: 1,
                    delivery_state,
                    message_pruned: false,
                },
            );
        }
        guard.schedules.insert(
            "telegram-schedule".into(),
            ScheduledPromptRecord {
                id: "telegram-schedule".into(),
                import_idempotency_key: None,
                agent_id,
                prompt: "check in".into(),
                trigger: ScheduleTrigger::Interval {
                    interval_ms: 60_000,
                },
                enabled: true,
                target: ScheduleTarget::Connector {
                    connector_id: connector.id.clone(),
                },
                next_due_at_ms: now + 60_000,
                last_fired: None,
                last_safe_outcome: None,
                created_at_ms: now,
                updated_at_ms: now,
                name: "check in".into(),
                active_hours: None,
                created_by: crate::schedules::AutomationCreator::Owner,
                preset: None,
                counters: Default::default(),
                delivery_events: Vec::new(),
                verification: None,
                verification_for: None,
                done_at_ms: None,
            },
        );
    }

    manager.delete(connector.id.clone()).await.unwrap();
    let guard = state.read().await;
    assert!(guard.inbound.contains_key(&(connector.id.clone(), 2)));
    assert!(guard.inbound.contains_key(&(connector.id.clone(), 3)));
    assert!(!guard.inbound.contains_key(&(connector.id.clone(), 4)));
    assert!(!guard.inbound.contains_key(&(connector.id.clone(), 5)));
    assert!(guard.outbound.contains_key("delivered"));
    assert!(!guard.outbound.contains_key("pending"));
    assert!(!guard.outbound.contains_key("failed"));
    assert!(!guard.schedules["telegram-schedule"].enabled);
    drop(guard);
    manager.shutdown().await;
}

#[test]
fn outbox_compaction_removes_old_delivered_but_never_pending_or_failed() {
    use crate::connectors::{OutboundDeliveryState, TelegramOutboundRecord};
    use std::collections::HashMap;

    let now = 10 * 24 * 60 * 60 * 1000;
    let mut outbox = HashMap::new();
    for (id, state, delivered_at_ms) in [
        ("old", OutboundDeliveryState::Delivered, Some(1)),
        ("recent", OutboundDeliveryState::Delivered, Some(now - 1)),
        ("pending", OutboundDeliveryState::Pending, None),
        ("failed", OutboundDeliveryState::Failed, None),
    ] {
        outbox.insert(
            id.to_string(),
            TelegramOutboundRecord {
                id: id.into(),
                connector_id: "connector".into(),
                agent_id: "agent".into(),
                room_id: "room".into(),
                assistant_message_id: format!("message-{id}"),
                text: id.into(),
                created_at_ms: 1,
                delivered_at_ms,
                attempts: 1,
                delivery_state: state,
                message_pruned: false,
            },
        );
    }

    super::compact_delivered_outbox(&mut outbox, "connector", now);
    assert!(!outbox.contains_key("old"));
    assert!(outbox.contains_key("recent"));
    assert!(outbox.contains_key("pending"));
    assert!(outbox.contains_key("failed"));
}

#[test]
fn outbox_compaction_caps_only_the_selected_connector() {
    use crate::connectors::{OutboundDeliveryState, TelegramOutboundRecord};
    use std::collections::HashMap;

    let now = super::DELIVERED_RETENTION_MS + 10_000;
    let mut outbox = HashMap::new();
    for connector_id in ["selected", "other"] {
        for index in 0..1_002_u64 {
            let id = format!("{connector_id}-{index}");
            outbox.insert(
                id.clone(),
                TelegramOutboundRecord {
                    id: id.clone(),
                    connector_id: connector_id.into(),
                    agent_id: format!("agent-{connector_id}"),
                    room_id: format!("room-{connector_id}"),
                    assistant_message_id: format!("message-{id}"),
                    text: id,
                    created_at_ms: now - 1_000 + index,
                    delivered_at_ms: Some(now - 1_000 + index),
                    attempts: 1,
                    delivery_state: OutboundDeliveryState::Delivered,
                    message_pruned: false,
                },
            );
        }
    }
    for state in [
        OutboundDeliveryState::Pending,
        OutboundDeliveryState::Failed,
    ] {
        let id = format!("selected-{state:?}");
        outbox.insert(
            id.clone(),
            TelegramOutboundRecord {
                id: id.clone(),
                connector_id: "selected".into(),
                agent_id: "agent-selected".into(),
                room_id: "room-selected".into(),
                assistant_message_id: format!("message-{id}"),
                text: id,
                created_at_ms: now,
                delivered_at_ms: None,
                attempts: 1,
                delivery_state: state,
                message_pruned: false,
            },
        );
    }

    super::compact_delivered_outbox(&mut outbox, "selected", now);

    assert_eq!(
        outbox
            .values()
            .filter(|record| {
                record.connector_id == "selected"
                    && record.delivery_state == OutboundDeliveryState::Delivered
            })
            .count(),
        1_000
    );
    assert_eq!(
        outbox
            .values()
            .filter(|record| record.connector_id == "other")
            .count(),
        1_002
    );
    assert_eq!(
        outbox
            .values()
            .filter(|record| {
                record.connector_id == "selected"
                    && record.delivery_state != OutboundDeliveryState::Delivered
            })
            .count(),
        2
    );
}

#[test]
fn terminal_inbound_compaction_is_bounded_per_connector_and_keeps_live_records() {
    use std::collections::HashMap;

    let sender = TelegramSenderMetadata {
        id: "sender".into(),
        username: None,
        display_name: None,
    };
    let chat = TelegramChatMetadata {
        id: "chat".into(),
        kind: TelegramChatKind::Private,
        title: None,
        username: None,
    };
    let mut inbound = HashMap::new();
    for connector_id in ["selected", "other"] {
        for update_id in 0..1_002_i64 {
            inbound.insert(
                (connector_id.to_string(), update_id),
                crate::connectors::TelegramInboundRecord {
                    connector_id: connector_id.into(),
                    update_id,
                    agent_id: format!("agent-{connector_id}"),
                    room_id: format!("room-{connector_id}"),
                    normalized_text: "done".into(),
                    sender: sender.clone(),
                    chat: chat.clone(),
                    received_at_ms: update_id as u64,
                    processing_state: InboundProcessingState::Processed,
                    run_idempotency_key: format!("{connector_id}-{update_id}"),
                },
            );
        }
    }
    inbound.insert(
        ("selected".into(), 2_000),
        crate::connectors::TelegramInboundRecord {
            connector_id: "selected".into(),
            update_id: 2_000,
            agent_id: "agent-selected".into(),
            room_id: "room-selected".into(),
            normalized_text: "live".into(),
            sender,
            chat,
            received_at_ms: 2_000,
            processing_state: InboundProcessingState::Processing,
            run_idempotency_key: "selected-live".into(),
        },
    );

    super::compact_terminal_inbound(&mut inbound, "selected");

    assert_eq!(
        inbound
            .values()
            .filter(|record| {
                record.connector_id == "selected"
                    && matches!(
                        record.processing_state,
                        InboundProcessingState::Processed | InboundProcessingState::Rejected
                    )
            })
            .count(),
        super::MAX_RETAINED_TERMINAL_INBOUND
    );
    assert_eq!(
        inbound
            .values()
            .filter(|record| record.connector_id == "other")
            .count(),
        1_002
    );
    assert_eq!(
        inbound[&("selected".into(), 2_000)].processing_state,
        InboundProcessingState::Processing
    );
}

#[test]
fn poll_backoff_is_exponential_and_capped() {
    let mut delay = super::POLL_RETRY_INITIAL;
    assert_eq!(delay, Duration::from_millis(100));
    delay = super::next_poll_backoff(delay);
    assert_eq!(delay, Duration::from_millis(200));
    for _ in 0..20 {
        delay = super::next_poll_backoff(delay);
    }
    assert_eq!(delay, super::POLL_RETRY_MAX);
    assert_eq!(super::next_poll_backoff(delay), super::POLL_RETRY_MAX);
}

#[test]
fn poll_retry_delay_waits_a_minute_for_a_conflict_only() {
    let current = Duration::from_millis(400);
    assert_eq!(
        super::poll_retry_delay(TelegramTransportError::PollConflict, current),
        Duration::from_secs(60)
    );
    for error in [
        TelegramTransportError::Transport,
        TelegramTransportError::UpstreamApi { code: Some(409) },
    ] {
        assert_eq!(super::poll_retry_delay(error, current), current);
    }
}

#[tokio::test]
async fn agent_deletion_is_rejected_while_a_run_is_in_flight() {
    let entered = Arc::new(Semaphore::new(0));
    let release = Arc::new(Semaphore::new(0));
    let mut daemon = DaemonState::with_model_adapter(Arc::new(GateModelAdapter {
        entered: Arc::clone(&entered),
        release: Arc::clone(&release),
    }));
    daemon.create_agent(test_config()).unwrap();
    let state = Arc::new(RwLock::new(daemon));
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    let credentials = Arc::new(InMemoryCredentialStore::default());
    let manager = manager(
        Arc::clone(&state),
        credentials.clone(),
        Arc::new(FakeTransport::default()),
    );
    let connector = manager
        .create(
            agent_id.clone(),
            TelegramBotToken::parse("42:busy-delete").unwrap(),
        )
        .await
        .unwrap();
    let running = {
        let runs = manager.runs.clone();
        let agent_id = agent_id.clone();
        tokio::spawn(async move {
            runs.run(AgentRunRequest {
                agent_id,
                content: Content {
                    text: "stay busy".into(),
                    ..Content::default()
                },
                room: RunRoom::Stable("direct:busy".into()),
                idempotency_key: None,
                source: RunSource::Api,
                source_ref: None,
                parent: None,
            })
            .await
        })
    };
    entered
        .acquire()
        .await
        .expect("the run should enter the model")
        .forget();

    assert_eq!(
        manager.delete_agent(agent_id.clone()).await.unwrap_err(),
        super::ConnectorManagerError::AgentBusy
    );
    assert!(state.read().await.get_agent(&agent_id).is_some());
    assert_eq!(state.read().await.connectors[&connector.id], connector);
    assert_eq!(
        credentials
            .load(&connector.id)
            .await
            .unwrap()
            .unwrap()
            .expose(),
        "42:busy-delete"
    );
    assert_eq!(manager.worker_count().await, 1);

    release.add_permits(1);
    running.await.unwrap().expect("the run commits normally");
    manager
        .delete_agent(agent_id.clone())
        .await
        .expect("deletion succeeds once no run is in flight");
    assert!(state.read().await.get_agent(&agent_id).is_none());
    manager.shutdown().await;
}
