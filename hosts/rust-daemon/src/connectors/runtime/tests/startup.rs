use super::*;

#[tokio::test]
async fn create_verifies_before_storing_and_publishes_a_stable_room() {
    let state = state_with_agent();
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    let credentials = Arc::new(InMemoryCredentialStore::default());
    let transport = Arc::new(FakeTransport::default());
    let manager = manager(Arc::clone(&state), credentials.clone(), transport.clone());

    let connector = manager
        .create(
            agent_id.clone(),
            TelegramBotToken::parse("42:test-token").unwrap(),
        )
        .await
        .expect("verified connector should be created");

    assert_eq!(transport.calls.lock().unwrap().as_slice(), ["get_me"]);
    assert_eq!(connector.agent_id, agent_id);
    assert_eq!(connector.room_id, format!("telegram:{}", connector.id));
    assert!(connector.enabled);
    assert_eq!(
        credentials
            .load(&connector.id)
            .await
            .unwrap()
            .unwrap()
            .expose(),
        "42:test-token"
    );
    assert_eq!(
        state.read().await.connectors.get(&connector.id),
        Some(&connector)
    );

    manager.shutdown().await;
}

#[tokio::test]
async fn create_persists_cleanup_intent_before_vault_write_without_publishing_connector() {
    let state = state_with_agent();
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    let entered = Arc::new(Semaphore::new(0));
    let release = Arc::new(Semaphore::new(0));
    let credentials = Arc::new(GatePutUncertainDeleteStore {
        inner: InMemoryCredentialStore::default(),
        entered: Arc::clone(&entered),
        release: Arc::clone(&release),
        fail_next_delete: AtomicBool::new(true),
    });
    let manager = manager(
        Arc::clone(&state),
        credentials,
        Arc::new(FakeTransport::default()),
    );

    let creating = {
        let manager = manager.clone();
        tokio::spawn(async move {
            manager
                .create(agent_id, TelegramBotToken::parse("42:draft-token").unwrap())
                .await
        })
    };
    entered
        .acquire()
        .await
        .expect("credential put should be reached")
        .forget();

    let state_guard = state.read().await;
    assert!(state_guard.connectors.is_empty());
    assert_eq!(state_guard.credential_cleanup.len(), 1);
    let intent_id = state_guard
        .credential_cleanup
        .keys()
        .next()
        .unwrap()
        .clone();
    drop(state_guard);
    release.add_permits(1);
    let connector = creating.await.unwrap().unwrap();
    assert_eq!(connector.id, intent_id);
    assert!(state.read().await.connectors[&connector.id].is_active());
    assert!(state.read().await.credential_cleanup.is_empty());

    manager.shutdown().await;
}

#[tokio::test]
async fn failed_active_create_publish_leaves_durable_intent_then_restart_cleans_credential() {
    let state = state_with_agent();
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    let temporary = std::env::temp_dir().join(format!(
        "anima-create-intent-restart-{}-{}",
        std::process::id(),
        super::now_ms()
    ));
    let snapshot_path = temporary.join("control-plane.json");
    state.write().await.set_control_plane_store(Some(
        crate::control_plane_store::ControlPlaneStoreConfig::Json(snapshot_path.clone()),
    ));
    let entered = Arc::new(Semaphore::new(0));
    let release = Arc::new(Semaphore::new(0));
    let credentials = Arc::new(GatePutUncertainDeleteStore {
        inner: InMemoryCredentialStore::default(),
        entered: Arc::clone(&entered),
        release: Arc::clone(&release),
        fail_next_delete: AtomicBool::new(true),
    });
    let first_manager = manager(
        Arc::clone(&state),
        credentials.clone(),
        Arc::new(FakeTransport::default()),
    );
    let creating = {
        let first_manager = first_manager.clone();
        tokio::spawn(async move {
            first_manager
                .create(
                    agent_id.clone(),
                    TelegramBotToken::parse("42:cleanup-on-restart").unwrap(),
                )
                .await
        })
    };
    entered.acquire().await.unwrap().forget();
    let gate = state
        .write()
        .await
        .install_test_control_plane_save_gate(true);
    gate.release.add_permits(1);
    release.add_permits(1);
    assert_eq!(
        creating.await.unwrap().unwrap_err(),
        super::ConnectorManagerError::CredentialStateUncertain
    );
    let intent_id = state
        .read()
        .await
        .credential_cleanup
        .keys()
        .next()
        .unwrap()
        .clone();
    assert!(state.read().await.connectors.is_empty());
    assert_eq!(
        first_manager.status(&intent_id).await,
        Some(ConnectorRuntimeStatus::Reconciling)
    );
    assert!(credentials.load(&intent_id).await.unwrap().is_some());
    let durable = crate::control_plane_store::load_control_plane_snapshot(
        &crate::control_plane_store::ControlPlaneStoreConfig::Json(snapshot_path.clone()),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(durable.credential_cleanup[0].connector_id, intent_id);

    first_manager.shutdown().await;
    let mut fresh = DaemonState::new();
    fresh.set_control_plane_store(Some(
        crate::control_plane_store::ControlPlaneStoreConfig::Json(snapshot_path),
    ));
    fresh.restore_control_plane_snapshot(durable).unwrap();
    let restored_state = Arc::new(RwLock::new(fresh));
    let restored = manager(
        Arc::clone(&restored_state),
        credentials.clone(),
        Arc::new(FakeTransport::default()),
    );
    restored.start_restored().await;
    assert!(credentials.load(&intent_id).await.unwrap().is_none());
    assert!(restored_state.read().await.credential_cleanup.is_empty());
    assert_eq!(restored.status(&intent_id).await, None);
    let restored_agent_id = restored_state.read().await.list_agents()[0]
        .state
        .id
        .clone();
    release.add_permits(1);
    let replacement = restored
        .create(
            restored_agent_id,
            TelegramBotToken::parse("84:retry-after-cleanup").unwrap(),
        )
        .await
        .expect("a completed cleanup intent must not block retry");
    assert!(replacement.is_active());
    restored.shutdown().await;
    std::fs::remove_dir_all(temporary).unwrap();
}

#[tokio::test]
async fn persistent_startup_cleanup_failure_survives_restarts_for_later_reconciliation() {
    let state = state_with_agent();
    let temporary = std::env::temp_dir().join(format!(
        "anima-persistent-cleanup-intent-{}-{}",
        std::process::id(),
        super::now_ms()
    ));
    let snapshot_path = temporary.join("control-plane.json");
    state.write().await.set_control_plane_store(Some(
        crate::control_plane_store::ControlPlaneStoreConfig::Json(snapshot_path.clone()),
    ));
    let connector_id = "telegram-orphan-persistent".to_string();
    let persist = {
        let mut state = state.write().await;
        state.credential_cleanup.insert(
            connector_id.clone(),
            crate::connectors::TelegramCredentialCleanupIntent {
                connector_id: connector_id.clone(),
                created_at_ms: 1,
            },
        );
        state.control_plane_persist_request()
    };
    persist.save().await.unwrap();
    let credentials = Arc::new(UncertainDeleteStore::default());
    credentials
        .inner
        .put(
            &connector_id,
            TelegramBotToken::parse("42:persistent-orphan").unwrap(),
        )
        .await
        .unwrap();

    let first = manager(
        Arc::clone(&state),
        credentials.clone(),
        Arc::new(FakeTransport::default()),
    );
    first.start_restored().await;
    assert!(state
        .read()
        .await
        .credential_cleanup
        .contains_key(&connector_id));
    assert!(credentials.load(&connector_id).await.unwrap().is_some());
    assert_eq!(
        first.status(&connector_id).await,
        Some(ConnectorRuntimeStatus::Reconciling)
    );
    first.shutdown().await;

    let durable = crate::control_plane_store::load_control_plane_snapshot(
        &crate::control_plane_store::ControlPlaneStoreConfig::Json(snapshot_path.clone()),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(durable.credential_cleanup.len(), 1);
    let mut fresh = DaemonState::new();
    fresh.set_control_plane_store(Some(
        crate::control_plane_store::ControlPlaneStoreConfig::Json(snapshot_path),
    ));
    fresh.restore_control_plane_snapshot(durable).unwrap();
    let restored_state = Arc::new(RwLock::new(fresh));
    let restored = manager(
        Arc::clone(&restored_state),
        credentials.clone(),
        Arc::new(FakeTransport::default()),
    );
    restored.start_restored().await;
    assert!(restored_state
        .read()
        .await
        .credential_cleanup
        .contains_key(&connector_id));
    assert!(credentials.load(&connector_id).await.unwrap().is_some());
    assert_eq!(
        restored.status(&connector_id).await,
        Some(ConnectorRuntimeStatus::Reconciling)
    );
    restored.shutdown().await;
    std::fs::remove_dir_all(temporary).unwrap();
}

#[tokio::test]
async fn startup_cleanup_harmlessly_removes_stale_intent_for_absent_credential() {
    let state = state_with_agent();
    let temporary = std::env::temp_dir().join(format!(
        "anima-stale-cleanup-intent-{}-{}",
        std::process::id(),
        super::now_ms()
    ));
    let snapshot_path = temporary.join("control-plane.json");
    state.write().await.set_control_plane_store(Some(
        crate::control_plane_store::ControlPlaneStoreConfig::Json(snapshot_path.clone()),
    ));
    let connector_id = "telegram-orphan-absent".to_string();
    let persist = {
        let mut state = state.write().await;
        state.credential_cleanup.insert(
            connector_id.clone(),
            crate::connectors::TelegramCredentialCleanupIntent {
                connector_id: connector_id.clone(),
                created_at_ms: 1,
            },
        );
        state.control_plane_persist_request()
    };
    persist.save().await.unwrap();
    let manager = manager(
        Arc::clone(&state),
        Arc::new(InMemoryCredentialStore::default()),
        Arc::new(FakeTransport::default()),
    );

    manager.start_restored().await;

    assert!(state.read().await.credential_cleanup.is_empty());
    assert_eq!(manager.status(&connector_id).await, None);
    let durable = crate::control_plane_store::load_control_plane_snapshot(
        &crate::control_plane_store::ControlPlaneStoreConfig::Json(snapshot_path),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(durable.credential_cleanup.is_empty());
    manager.shutdown().await;
    std::fs::remove_dir_all(temporary).unwrap();
}

#[tokio::test]
async fn failed_initial_cleanup_intent_save_never_mutates_the_vault() {
    let state = state_with_agent();
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    let invalid_path = std::env::temp_dir().join(format!(
        "anima-create-reconciliation-unavailable-{}-{}",
        std::process::id(),
        super::now_ms()
    ));
    std::fs::create_dir_all(&invalid_path).unwrap();
    state.write().await.set_control_plane_store(Some(
        crate::control_plane_store::ControlPlaneStoreConfig::Json(invalid_path.clone()),
    ));
    let entered = Arc::new(Semaphore::new(0));
    let release = Arc::new(Semaphore::new(0));
    let credentials = Arc::new(GatePutUncertainDeleteStore {
        inner: InMemoryCredentialStore::default(),
        entered: Arc::clone(&entered),
        release: Arc::clone(&release),
        fail_next_delete: AtomicBool::new(true),
    });
    let manager = manager(
        Arc::clone(&state),
        credentials,
        Arc::new(FakeTransport::default()),
    );
    assert_eq!(
        manager
            .create(
                agent_id,
                TelegramBotToken::parse("42:uncertain-create").unwrap(),
            )
            .await
            .unwrap_err(),
        super::ConnectorManagerError::Persistence
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(50), entered.acquire())
            .await
            .is_err()
    );
    assert!(state.read().await.connectors.is_empty());
    assert!(state.read().await.credential_cleanup.is_empty());
    drop(release);
    manager.shutdown().await;
    std::fs::remove_dir_all(invalid_path).unwrap();
}

#[tokio::test]
async fn successful_poll_recovers_status_after_transient_transport_error() {
    let state = state_with_agent();
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    let transport = Arc::new(FakeTransport::default());
    transport.remaining_poll_failures.store(1, Ordering::SeqCst);
    let manager = manager(
        Arc::clone(&state),
        Arc::new(InMemoryCredentialStore::default()),
        transport,
    );
    let connector = manager
        .create(
            agent_id,
            TelegramBotToken::parse("42:transient-token").unwrap(),
        )
        .await
        .unwrap();

    assert!(
        wait_for_status(
            &manager,
            &connector.id,
            ConnectorRuntimeStatus::Error,
            Duration::from_millis(500),
        )
        .await,
        "the failed poll should expose a safe transient error"
    );
    assert!(
        wait_for_status(
            &manager,
            &connector.id,
            ConnectorRuntimeStatus::Pairing,
            Duration::from_secs(2),
        )
        .await,
        "a later successful poll should restore the operational status"
    );

    manager.shutdown().await;
}

#[tokio::test]
async fn revoked_poll_credential_stops_worker_and_requires_replacement() {
    let state = state_with_agent();
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    let transport = Arc::new(FakeTransport::default());
    transport.revoked_poll.store(true, Ordering::SeqCst);
    let manager = manager(
        state,
        Arc::new(InMemoryCredentialStore::default()),
        transport,
    );
    let connector = manager
        .create(
            agent_id,
            TelegramBotToken::parse("42:revoked-token").unwrap(),
        )
        .await
        .unwrap();

    assert!(
        wait_for_status(
            &manager,
            &connector.id,
            ConnectorRuntimeStatus::CredentialRequired,
            Duration::from_secs(1),
        )
        .await
    );
    tokio::time::timeout(Duration::from_secs(1), async {
        while manager.worker_count().await != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("revoked credentials must terminate their poller");
    manager.shutdown().await;
}

#[tokio::test]
async fn revoked_delivery_credential_stops_worker_and_requires_replacement() {
    let state = state_with_agent();
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    let transport = Arc::new(FakeTransport::default());
    let manager = manager(
        Arc::clone(&state),
        Arc::new(InMemoryCredentialStore::default()),
        transport.clone(),
    );
    let connector = manager
        .create(
            agent_id.clone(),
            TelegramBotToken::parse("42:revoked-delivery").unwrap(),
        )
        .await
        .unwrap();
    transport.revoked_send.store(true, Ordering::SeqCst);
    {
        let mut state = state.write().await;
        state
            .connectors
            .get_mut(&connector.id)
            .unwrap()
            .approved_chat = Some(text_update(1, "202", "pair").chat);
        state.outbound.insert(
            "revoked-outbound".into(),
            crate::connectors::TelegramOutboundRecord {
                id: "revoked-outbound".into(),
                connector_id: connector.id.clone(),
                agent_id,
                room_id: connector.room_id.clone(),
                assistant_message_id: "assistant-revoked".into(),
                text: "cannot deliver".into(),
                created_at_ms: super::now_ms(),
                delivered_at_ms: None,
                attempts: 0,
                delivery_state: crate::connectors::OutboundDeliveryState::Pending,
                message_pruned: false,
            },
        );
    }

    assert!(
        wait_for_status(
            &manager,
            &connector.id,
            ConnectorRuntimeStatus::CredentialRequired,
            Duration::from_secs(1),
        )
        .await
    );
    tokio::time::timeout(Duration::from_secs(1), async {
        while manager.worker_count().await != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("revoked delivery credentials must terminate their worker");
    manager.shutdown().await;
}

#[tokio::test]
async fn only_one_active_connector_is_allowed_and_deletion_leaves_a_nonblocking_tombstone() {
    let state = state_with_agent();
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    let credentials = Arc::new(InMemoryCredentialStore::default());
    let transport = Arc::new(FakeTransport::default());
    let manager = manager(Arc::clone(&state), credentials.clone(), transport);
    let first = manager
        .create(
            agent_id.clone(),
            TelegramBotToken::parse("42:first-token").unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        manager
            .create(
                agent_id.clone(),
                TelegramBotToken::parse("42:blocked-token").unwrap(),
            )
            .await
            .unwrap_err(),
        super::ConnectorManagerError::AgentAlreadyConnected
    );

    manager.delete(first.id.clone()).await.unwrap();
    let tombstone = state
        .read()
        .await
        .connectors
        .get(&first.id)
        .cloned()
        .unwrap();
    assert!(!tombstone.enabled);
    assert!(tombstone.deleted_at_ms.is_some());
    assert!(credentials.load(&first.id).await.unwrap().is_none());

    let replacement = manager
        .create(
            agent_id,
            TelegramBotToken::parse("42:replacement-token").unwrap(),
        )
        .await
        .unwrap();
    assert_ne!(replacement.id, first.id);
    manager.shutdown().await;
}

#[tokio::test]
async fn batch_acceptance_advances_cursor_pairs_latest_then_accepts_only_approved_chat() {
    let state = state_with_agent();
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    let manager = manager(
        Arc::clone(&state),
        Arc::new(InMemoryCredentialStore::default()),
        Arc::new(FakeTransport::default()),
    );
    let connector = manager
        .create(
            agent_id,
            TelegramBotToken::parse("42:pairing-token").unwrap(),
        )
        .await
        .unwrap();

    manager
        .accept_batch(
            connector.id.clone(),
            TelegramUpdateBatch {
                updates: vec![
                    text_update(1, "101", "first"),
                    text_update(2, "202", "latest"),
                ],
                next_update_id: 3,
            },
        )
        .await
        .unwrap();
    {
        let state = state.read().await;
        let persisted = state.connectors.get(&connector.id).unwrap();
        assert_eq!(persisted.next_update_id, 3);
        assert_eq!(persisted.pending_pairing.as_ref().unwrap().chat.id, "202");
        assert!(state.inbound.is_empty());
    }

    manager.approve_pending(connector.id.clone()).await.unwrap();
    manager
        .accept_batch(
            connector.id.clone(),
            TelegramUpdateBatch {
                updates: vec![
                    text_update(3, "101", "ignored"),
                    text_update(4, "202", "accepted"),
                ],
                next_update_id: 5,
            },
        )
        .await
        .unwrap();
    let state_guard = state.read().await;
    assert_eq!(state_guard.connectors[&connector.id].next_update_id, 5);
    assert_eq!(state_guard.inbound.len(), 1);
    let inbound = state_guard.inbound.get(&(connector.id.clone(), 4)).unwrap();
    assert_eq!(inbound.normalized_text, "accepted");
    assert_eq!(inbound.processing_state, InboundProcessingState::Received);
    assert_eq!(
        inbound.run_idempotency_key,
        format!("telegram:{}:update:4", connector.id)
    );
    drop(state_guard);

    manager.shutdown().await;
}

#[tokio::test]
async fn stale_pairing_candidate_cannot_be_approved_and_a_new_chat_replaces_it() {
    let state = state_with_agent();
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    let temporary = std::env::temp_dir().join(format!(
        "anima-pairing-approval-expiry-{}-{}",
        std::process::id(),
        super::now_ms()
    ));
    let snapshot_path = temporary.join("control-plane.json");
    state.write().await.set_control_plane_store(Some(
        crate::control_plane_store::ControlPlaneStoreConfig::Json(snapshot_path.clone()),
    ));
    let manager = manager(
        Arc::clone(&state),
        Arc::new(InMemoryCredentialStore::default()),
        Arc::new(FakeTransport::default()),
    );
    let connector = manager
        .create(
            agent_id,
            TelegramBotToken::parse("42:pairing-expiry").unwrap(),
        )
        .await
        .unwrap();
    manager
        .accept_batch(
            connector.id.clone(),
            TelegramUpdateBatch {
                updates: vec![text_update(1, "101", "old")],
                next_update_id: 2,
            },
        )
        .await
        .unwrap();
    state
        .write()
        .await
        .connectors
        .get_mut(&connector.id)
        .unwrap()
        .pending_pairing
        .as_mut()
        .unwrap()
        .requested_at_ms = super::now_ms().saturating_sub(super::PAIRING_CANDIDATE_TTL_MS + 1);

    assert_eq!(
        manager
            .approve_pending(connector.id.clone())
            .await
            .unwrap_err(),
        super::ConnectorManagerError::PendingPairingNotFound
    );
    assert!(state.read().await.connectors[&connector.id]
        .pending_pairing
        .is_none());
    let persisted = crate::control_plane_store::load_control_plane_snapshot(
        &crate::control_plane_store::ControlPlaneStoreConfig::Json(snapshot_path),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(persisted.connectors[0].pending_pairing.is_none());

    manager
        .accept_batch(
            connector.id.clone(),
            TelegramUpdateBatch {
                updates: vec![text_update(2, "202", "new")],
                next_update_id: 3,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        state.read().await.connectors[&connector.id]
            .pending_pairing
            .as_ref()
            .unwrap()
            .chat
            .id,
        "202"
    );
    manager.shutdown().await;
    std::fs::remove_dir_all(temporary).unwrap();
}

#[tokio::test]
async fn worker_proactively_expires_pairing_candidate_durably_and_allows_a_new_chat() {
    let state = state_with_agent();
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    let temporary = std::env::temp_dir().join(format!(
        "anima-pairing-maintenance-{}-{}",
        std::process::id(),
        super::now_ms()
    ));
    let snapshot_path = temporary.join("control-plane.json");
    state.write().await.set_control_plane_store(Some(
        crate::control_plane_store::ControlPlaneStoreConfig::Json(snapshot_path.clone()),
    ));
    let manager = manager(
        Arc::clone(&state),
        Arc::new(InMemoryCredentialStore::default()),
        Arc::new(FakeTransport::default()),
    );
    let connector = manager
        .create(
            agent_id,
            TelegramBotToken::parse("42:pairing-maintenance").unwrap(),
        )
        .await
        .unwrap();
    manager.stop_worker(&connector.id).await.unwrap();
    manager
        .accept_batch(
            connector.id.clone(),
            TelegramUpdateBatch {
                updates: vec![text_update(1, "101", "old")],
                next_update_id: 2,
            },
        )
        .await
        .unwrap();
    let persist = {
        let mut state = state.write().await;
        state
            .connectors
            .get_mut(&connector.id)
            .unwrap()
            .pending_pairing
            .as_mut()
            .unwrap()
            .requested_at_ms = super::now_ms().saturating_sub(super::PAIRING_CANDIDATE_TTL_MS + 1);
        state.control_plane_persist_request()
    };
    persist.save().await.unwrap();

    manager
        .start_worker(
            connector.id.clone(),
            TelegramBotToken::parse("42:pairing-maintenance").unwrap(),
        )
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if state.read().await.connectors[&connector.id]
                .pending_pairing
                .is_none()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("worker maintenance must clear an expired pairing candidate");
    let durable = crate::control_plane_store::load_control_plane_snapshot(
        &crate::control_plane_store::ControlPlaneStoreConfig::Json(snapshot_path),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(durable.connectors[0].pending_pairing.is_none());

    manager.stop_worker(&connector.id).await.unwrap();
    manager
        .accept_batch(
            connector.id.clone(),
            TelegramUpdateBatch {
                updates: vec![text_update(2, "202", "new")],
                next_update_id: 3,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        state.read().await.connectors[&connector.id]
            .pending_pairing
            .as_ref()
            .unwrap()
            .chat
            .id,
        "202"
    );
    manager.shutdown().await;
    std::fs::remove_dir_all(temporary).unwrap();
}

#[tokio::test]
async fn duplicate_update_reuses_exact_record_but_rejects_conflicting_payload() {
    let state = state_with_agent();
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    let manager = manager(
        Arc::clone(&state),
        Arc::new(InMemoryCredentialStore::default()),
        Arc::new(FakeTransport::default()),
    );
    let connector = manager
        .create(
            agent_id,
            TelegramBotToken::parse("42:duplicate-token").unwrap(),
        )
        .await
        .unwrap();
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
    let accepted = TelegramUpdateBatch {
        updates: vec![text_update(2, "202", "same")],
        next_update_id: 3,
    };
    manager
        .accept_batch(connector.id.clone(), accepted.clone())
        .await
        .unwrap();
    manager
        .accept_batch(connector.id.clone(), accepted)
        .await
        .unwrap();
    assert_eq!(state.read().await.inbound.len(), 1);

    let error = manager
        .accept_batch(
            connector.id.clone(),
            TelegramUpdateBatch {
                updates: vec![text_update(2, "202", "conflict")],
                next_update_id: 3,
            },
        )
        .await
        .unwrap_err();
    assert_eq!(error, super::ConnectorManagerError::ConflictingUpdate);
    assert_eq!(state.read().await.inbound.len(), 1);
    manager.shutdown().await;
}

#[tokio::test]
async fn conflicting_batch_prevalidates_before_inserting_any_new_update() {
    let state = state_with_agent();
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    let manager = manager(
        Arc::clone(&state),
        Arc::new(InMemoryCredentialStore::default()),
        Arc::new(FakeTransport::default()),
    );
    let connector = manager
        .create(
            agent_id,
            TelegramBotToken::parse("42:atomic-batch").unwrap(),
        )
        .await
        .unwrap();
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
                updates: vec![text_update(2, "202", "original")],
                next_update_id: 3,
            },
        )
        .await
        .unwrap();

    let error = manager
        .accept_batch(
            connector.id.clone(),
            TelegramUpdateBatch {
                updates: vec![
                    text_update(3, "202", "must-not-stick"),
                    text_update(2, "202", "conflict"),
                ],
                next_update_id: 4,
            },
        )
        .await
        .unwrap_err();
    assert_eq!(error, super::ConnectorManagerError::ConflictingUpdate);
    let guard = state.read().await;
    assert!(!guard.inbound.contains_key(&(connector.id.clone(), 3)));
    assert_eq!(guard.connectors[&connector.id].next_update_id, 3);
    drop(guard);
    manager.shutdown().await;
}

#[tokio::test]
async fn failed_batch_persistence_rolls_back_cursor_and_pairing_candidate() {
    let state = state_with_agent();
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    let manager = manager(
        Arc::clone(&state),
        Arc::new(InMemoryCredentialStore::default()),
        Arc::new(FakeTransport::default()),
    );
    let connector = manager
        .create(
            agent_id,
            TelegramBotToken::parse("42:persist-failure").unwrap(),
        )
        .await
        .unwrap();
    let invalid_path = std::env::temp_dir().join(format!(
        "anima-connector-invalid-store-{}-{}",
        std::process::id(),
        super::now_ms()
    ));
    std::fs::create_dir_all(&invalid_path).unwrap();
    state.write().await.set_control_plane_store(Some(
        crate::control_plane_store::ControlPlaneStoreConfig::Json(invalid_path.clone()),
    ));

    assert_eq!(
        manager
            .accept_batch(
                connector.id.clone(),
                TelegramUpdateBatch {
                    updates: vec![text_update(1, "202", "must roll back")],
                    next_update_id: 2,
                },
            )
            .await
            .unwrap_err(),
        super::ConnectorManagerError::Persistence
    );
    let persisted = state.read().await.connectors[&connector.id].clone();
    assert_eq!(persisted.next_update_id, 0);
    assert!(persisted.pending_pairing.is_none());
    assert!(state.read().await.inbound.is_empty());
    manager.shutdown().await;
    std::fs::remove_dir_all(invalid_path).unwrap();
}
