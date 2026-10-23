use super::*;

#[tokio::test]
async fn failed_gated_connector_publish_rolls_back_before_agent_publisher_enters() {
    let state = state_with_agent();
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    let temporary = std::env::temp_dir().join(format!(
        "anima-publish-isolation-{}-{}",
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
            agent_id.clone(),
            TelegramBotToken::parse("42:publish-gate").unwrap(),
        )
        .await
        .unwrap();
    manager.stop_worker(&connector.id).await.unwrap();
    let gate = state
        .write()
        .await
        .install_test_control_plane_save_gate(true);

    let accepting = {
        let manager = manager.clone();
        let connector_id = connector.id.clone();
        tokio::spawn(async move {
            manager
                .accept_batch(
                    connector_id,
                    TelegramUpdateBatch {
                        updates: vec![text_update(1, "202", "pair")],
                        next_update_id: 2,
                    },
                )
                .await
        })
    };
    gate.entered.acquire().await.unwrap().forget();

    let mut updating = {
        let runs = manager.runs.clone();
        let state = Arc::clone(&state);
        let agent_id = agent_id.clone();
        tokio::spawn(async move {
            let _transaction = runs.control_plane_transaction().await;
            let persist = {
                let mut state = state.write().await;
                state
                    .update_agent(
                        &agent_id,
                        AgentConfigUpdate {
                            name: Some("after rollback".into()),
                            ..AgentConfigUpdate::default()
                        },
                    )
                    .unwrap();
                state.control_plane_persist_request()
            };
            persist.save().await.unwrap();
        })
    };
    assert!(
        tokio::time::timeout(Duration::from_millis(20), &mut updating)
            .await
            .is_err()
    );

    gate.release.add_permits(1);
    assert_eq!(
        accepting.await.unwrap().unwrap_err(),
        super::ConnectorManagerError::Persistence
    );
    updating.await.unwrap();

    let state_guard = state.read().await;
    assert_eq!(state_guard.connectors[&connector.id].next_update_id, 0);
    assert_eq!(
        state_guard.get_agent(&agent_id).unwrap().state.config.name,
        "after rollback"
    );
    drop(state_guard);
    let persisted = crate::control_plane_store::load_control_plane_snapshot(
        &crate::control_plane_store::ControlPlaneStoreConfig::Json(snapshot_path),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(persisted.connectors[0].next_update_id, 0);
    assert_eq!(persisted.agents[0].state.config.name, "after rollback");
    manager.shutdown().await;
    std::fs::remove_dir_all(temporary).unwrap();
}

#[tokio::test]
async fn failed_gated_delivery_rolls_back_before_agent_publisher_enters() {
    let state = state_with_agent();
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    let temporary = std::env::temp_dir().join(format!(
        "anima-delivery-publish-isolation-{}-{}",
        std::process::id(),
        super::now_ms()
    ));
    let snapshot_path = temporary.join("control-plane.json");
    state.write().await.set_control_plane_store(Some(
        crate::control_plane_store::ControlPlaneStoreConfig::Json(snapshot_path.clone()),
    ));
    let transport = Arc::new(FakeTransport::default());
    let manager = manager(
        Arc::clone(&state),
        Arc::new(InMemoryCredentialStore::default()),
        transport.clone(),
    );
    let connector = manager
        .create(
            agent_id.clone(),
            TelegramBotToken::parse("42:delivery-publish-gate").unwrap(),
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
                updates: vec![text_update(2, "202", "deliver once")],
                next_update_id: 3,
            },
        )
        .await
        .unwrap();
    manager
        .process_pending_once(connector.id.clone())
        .await
        .unwrap();
    let outbound_id = state.read().await.outbound.keys().next().unwrap().clone();
    let gate = state
        .write()
        .await
        .install_test_control_plane_save_gate(true);

    let delivering = {
        let manager = manager.clone();
        let connector_id = connector.id.clone();
        tokio::spawn(async move { manager.deliver_pending_once(connector_id).await })
    };
    gate.entered.acquire().await.unwrap().forget();

    let mut updating = {
        let runs = manager.runs.clone();
        let state = Arc::clone(&state);
        let agent_id = agent_id.clone();
        tokio::spawn(async move {
            let _transaction = runs.control_plane_transaction().await;
            let persist = {
                let mut state = state.write().await;
                state
                    .update_agent(
                        &agent_id,
                        AgentConfigUpdate {
                            name: Some("after delivery rollback".into()),
                            ..AgentConfigUpdate::default()
                        },
                    )
                    .unwrap();
                state.control_plane_persist_request()
            };
            persist.save().await.unwrap();
        })
    };
    assert!(
        tokio::time::timeout(Duration::from_millis(20), &mut updating)
            .await
            .is_err()
    );

    gate.release.add_permits(1);
    assert_eq!(
        delivering.await.unwrap().unwrap_err(),
        super::ConnectorManagerError::Persistence
    );
    assert_eq!(transport.sent.lock().unwrap().len(), 1);
    updating.await.unwrap();

    let state_guard = state.read().await;
    let outbound = &state_guard.outbound[&outbound_id];
    assert_eq!(
        outbound.delivery_state,
        crate::connectors::OutboundDeliveryState::Pending
    );
    assert_eq!(outbound.attempts, 0);
    assert_eq!(
        state_guard.get_agent(&agent_id).unwrap().state.config.name,
        "after delivery rollback"
    );
    drop(state_guard);
    let persisted = crate::control_plane_store::load_control_plane_snapshot(
        &crate::control_plane_store::ControlPlaneStoreConfig::Json(snapshot_path),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(
        persisted.outbound[0].delivery_state,
        crate::connectors::OutboundDeliveryState::Pending
    );
    assert_eq!(persisted.outbound[0].attempts, 0);
    assert_eq!(
        persisted.agents[0].state.config.name,
        "after delivery rollback"
    );
    assert!(manager
        .deliver_pending_once(connector.id.clone())
        .await
        .unwrap());
    assert_eq!(transport.sent.lock().unwrap().len(), 2);
    let retried = state.read().await.outbound[&outbound_id].clone();
    assert_eq!(
        retried.delivery_state,
        crate::connectors::OutboundDeliveryState::Delivered
    );
    assert_eq!(retried.attempts, 1);
    manager.shutdown().await;
    std::fs::remove_dir_all(temporary).unwrap();
}

#[tokio::test]
async fn failed_final_run_snapshot_leaves_no_deliverable_outbox_and_retries_processing() {
    let entered = Arc::new(Semaphore::new(0));
    let release = Arc::new(Semaphore::new(0));
    let mut daemon = DaemonState::with_model_adapter(Arc::new(GateModelAdapter {
        entered: Arc::clone(&entered),
        release: Arc::clone(&release),
    }));
    daemon.create_agent(test_config()).unwrap();
    let state = Arc::new(RwLock::new(daemon));
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    let manager = manager(
        Arc::clone(&state),
        Arc::new(InMemoryCredentialStore::default()),
        Arc::new(FakeTransport::default()),
    );
    let connector = manager
        .create(
            agent_id.clone(),
            TelegramBotToken::parse("42:token").unwrap(),
        )
        .await
        .unwrap();
    manager.shutdown().await;
    manager
        .accept_batch(
            connector.id.clone(),
            TelegramUpdateBatch {
                updates: vec![text_update(1, "101", "pair")],
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
                updates: vec![text_update(2, "101", "run")],
                next_update_id: 3,
            },
        )
        .await
        .unwrap();
    let baseline_agent = state.read().await.get_agent(&agent_id).unwrap();

    let temporary = std::env::temp_dir().join(format!(
        "anima-connector-final-persist-{}-{}",
        std::process::id(),
        super::now_ms()
    ));
    std::fs::create_dir_all(&temporary).unwrap();
    let snapshot_path = temporary.join("control-plane.json");
    state.write().await.set_control_plane_store(Some(
        crate::control_plane_store::ControlPlaneStoreConfig::Json(snapshot_path.clone()),
    ));
    let processing_manager = manager.clone();
    let connector_id = connector.id.clone();
    let processing =
        tokio::spawn(async move { processing_manager.process_pending_once(connector_id).await });
    entered
        .acquire()
        .await
        .expect("model should enter after the running snapshot is durable")
        .forget();
    state
        .write()
        .await
        .update_agent(
            &agent_id,
            AgentConfigUpdate {
                name: Some("updated-operator".into()),
                ..AgentConfigUpdate::default()
            },
        )
        .unwrap();
    std::fs::remove_file(&snapshot_path).unwrap();
    std::fs::create_dir(&snapshot_path).unwrap();
    release.add_permits(1);

    assert_eq!(
        processing.await.unwrap().unwrap_err(),
        super::ConnectorManagerError::Persistence
    );
    let state_guard = state.read().await;
    assert_eq!(
        state_guard
            .inbound
            .get(&(connector.id.clone(), 2))
            .unwrap()
            .processing_state,
        InboundProcessingState::Processing
    );
    assert!(state_guard
        .outbound
        .values()
        .all(|record| record.connector_id != connector.id));
    drop(state_guard);

    let rolled_back = state.read().await.get_agent(&agent_id).unwrap();
    assert_eq!(rolled_back.messages, baseline_agent.messages);
    assert_eq!(rolled_back.state.name, "updated-operator");

    std::fs::remove_dir(&snapshot_path).unwrap();
    let retry_manager = manager.clone();
    let retry_connector_id = connector.id.clone();
    let retry =
        tokio::spawn(async move { retry_manager.process_pending_once(retry_connector_id).await });
    entered
        .acquire()
        .await
        .expect("retry should enter the model")
        .forget();
    release.add_permits(1);
    assert!(retry.await.unwrap().unwrap());
    let retried = state.read().await.get_agent(&agent_id).unwrap();
    assert_eq!(retried.messages.len(), baseline_agent.messages.len() + 2);
    assert_eq!(retried.state.name, "updated-operator");
    assert_eq!(
        state
            .read()
            .await
            .inbound
            .get(&(connector.id.clone(), 2))
            .unwrap()
            .processing_state,
        InboundProcessingState::Processed
    );
    assert_eq!(
        state
            .read()
            .await
            .outbound
            .values()
            .filter(|record| record.connector_id == connector.id)
            .count(),
        1
    );
    std::fs::remove_dir_all(temporary).unwrap();
}

#[tokio::test]
async fn owner_thread_idempotency_singleflights_replays_and_rejects_conflicts() {
    let entered = Arc::new(Semaphore::new(0));
    let release = Arc::new(Semaphore::new(0));
    let mut daemon = DaemonState::with_model_adapter(Arc::new(GateModelAdapter {
        entered: Arc::clone(&entered),
        release: Arc::clone(&release),
    }));
    daemon.create_agent(test_config()).unwrap();
    let state = Arc::new(RwLock::new(daemon));
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    let manager = manager(
        Arc::clone(&state),
        Arc::new(InMemoryCredentialStore::default()),
        Arc::new(FakeTransport::default()),
    );
    let connector = manager
        .create(
            agent_id.clone(),
            TelegramBotToken::parse("42:owner-idempotency").unwrap(),
        )
        .await
        .unwrap();
    manager.stop_worker(&connector.id).await.unwrap();

    let first = {
        let manager = manager.clone();
        let agent_id = agent_id.clone();
        let connector_id = connector.id.clone();
        tokio::spawn(async move {
            manager
                .send_from_owner(
                    agent_id,
                    connector_id,
                    "same owner turn".into(),
                    "owner-key".into(),
                )
                .await
        })
    };
    entered.acquire().await.unwrap().forget();
    let retry = {
        let manager = manager.clone();
        let agent_id = agent_id.clone();
        let connector_id = connector.id.clone();
        tokio::spawn(async move {
            manager
                .send_from_owner(
                    agent_id,
                    connector_id,
                    "same owner turn".into(),
                    "owner-key".into(),
                )
                .await
        })
    };
    assert!(
        tokio::time::timeout(Duration::from_millis(20), entered.acquire())
            .await
            .is_err(),
        "retry must wait on the same manager-owned singleflight"
    );
    release.add_permits(1);
    assert!(!first.await.unwrap().unwrap().1);
    assert!(!retry.await.unwrap().unwrap().1);
    assert!(
        tokio::time::timeout(Duration::from_millis(20), entered.acquire())
            .await
            .is_err(),
        "committed retry must replay without another model call"
    );

    let snapshot = state.read().await.get_agent(&agent_id).unwrap();
    let room_messages = snapshot
        .messages
        .iter()
        .filter(|message| message.room_id == connector.room_id)
        .collect::<Vec<_>>();
    assert_eq!(room_messages.len(), 2);
    assert_eq!(
        room_messages[0]
            .content
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("idempotencyKey")),
        Some(&DataValue::String("owner-key".into()))
    );

    assert_eq!(
        manager
            .send_from_owner(
                agent_id,
                connector.id,
                "different owner turn".into(),
                "owner-key".into(),
            )
            .await
            .unwrap_err(),
        super::ConnectorManagerError::IdempotencyConflict
    );
    manager.shutdown().await;
}

#[tokio::test]
async fn owner_thread_idempotency_replays_after_control_plane_restart() {
    let entered = Arc::new(Semaphore::new(0));
    let release = Arc::new(Semaphore::new(0));
    let mut daemon = DaemonState::with_model_adapter(Arc::new(GateModelAdapter {
        entered: Arc::clone(&entered),
        release: Arc::clone(&release),
    }));
    daemon.create_agent(test_config()).unwrap();
    let temporary = std::env::temp_dir().join(format!(
        "anima-owner-idempotency-restart-{}-{}",
        std::process::id(),
        super::now_ms()
    ));
    let snapshot_path = temporary.join("control-plane.json");
    daemon.set_control_plane_store(Some(
        crate::control_plane_store::ControlPlaneStoreConfig::Json(snapshot_path.clone()),
    ));
    let state = Arc::new(RwLock::new(daemon));
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    let first_manager = manager(
        Arc::clone(&state),
        Arc::new(InMemoryCredentialStore::default()),
        Arc::new(FakeTransport::default()),
    );
    let connector = first_manager
        .create(
            agent_id.clone(),
            TelegramBotToken::parse("42:owner-restart").unwrap(),
        )
        .await
        .unwrap();
    first_manager.stop_worker(&connector.id).await.unwrap();
    let first = {
        let manager = first_manager.clone();
        let agent_id = agent_id.clone();
        let connector_id = connector.id.clone();
        tokio::spawn(async move {
            manager
                .send_from_owner(
                    agent_id,
                    connector_id,
                    "restart-safe turn".into(),
                    "restart-key".into(),
                )
                .await
        })
    };
    entered.acquire().await.unwrap().forget();
    release.add_permits(1);
    assert!(!first.await.unwrap().unwrap().1);
    first_manager.shutdown().await;

    let snapshot = crate::control_plane_store::load_control_plane_snapshot(
        &crate::control_plane_store::ControlPlaneStoreConfig::Json(snapshot_path),
    )
    .await
    .unwrap()
    .unwrap();
    let retry_entered = Arc::new(Semaphore::new(0));
    let mut restored = DaemonState::with_model_adapter(Arc::new(GateModelAdapter {
        entered: Arc::clone(&retry_entered),
        release: Arc::new(Semaphore::new(0)),
    }));
    restored.restore_control_plane_snapshot(snapshot).unwrap();
    let restored = Arc::new(RwLock::new(restored));
    let retry_manager = manager(
        Arc::clone(&restored),
        Arc::new(InMemoryCredentialStore::default()),
        Arc::new(FakeTransport::default()),
    );
    let replay = retry_manager
        .send_from_owner(
            agent_id.clone(),
            connector.id.clone(),
            "restart-safe turn".into(),
            "restart-key".into(),
        )
        .await
        .unwrap();
    assert!(!replay.1);
    assert!(
        tokio::time::timeout(Duration::from_millis(20), retry_entered.acquire())
            .await
            .is_err(),
        "durable replay must not call the model after restart"
    );
    assert_eq!(
        restored
            .read()
            .await
            .get_agent(&agent_id)
            .unwrap()
            .messages
            .iter()
            .filter(|message| message.room_id == connector.room_id)
            .count(),
        2
    );
    retry_manager.shutdown().await;
    std::fs::remove_dir_all(temporary).unwrap();
}

#[tokio::test]
async fn delivery_cannot_send_outbound_from_a_failing_final_owner_thread_snapshot() {
    let entered = Arc::new(Semaphore::new(0));
    let release = Arc::new(Semaphore::new(0));
    let mut daemon = DaemonState::with_model_adapter(Arc::new(GateModelAdapter {
        entered: Arc::clone(&entered),
        release: Arc::clone(&release),
    }));
    daemon.create_agent(test_config()).unwrap();
    let state = Arc::new(RwLock::new(daemon));
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
            TelegramBotToken::parse("42:owner-final-save-gate").unwrap(),
        )
        .await
        .unwrap();
    manager.stop_worker(&connector.id).await.unwrap();
    manager
        .accept_batch(
            connector.id.clone(),
            TelegramUpdateBatch {
                updates: vec![text_update(1, "101", "pair")],
                next_update_id: 2,
            },
        )
        .await
        .unwrap();
    manager
        .approve_pending_chat(connector.id.clone(), Some("101".into()))
        .await
        .unwrap();

    let sending = {
        let manager = manager.clone();
        let agent_id = agent_id.clone();
        let connector_id = connector.id.clone();
        tokio::spawn(async move {
            manager
                .send_from_owner(
                    agent_id,
                    connector_id,
                    "owner web turn".into(),
                    "owner-final-save".into(),
                )
                .await
        })
    };
    entered.acquire().await.unwrap().forget();
    let save_gate = state
        .write()
        .await
        .install_test_control_plane_non_turn_save_gate(true);
    release.add_permits(1);
    save_gate.entered.acquire().await.unwrap().forget();

    let delivering = {
        let manager = manager.clone();
        let connector_id = connector.id.clone();
        tokio::spawn(async move { manager.deliver_pending_once(connector_id).await })
    };
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(
        transport.sent.lock().unwrap().is_empty(),
        "delivery must not observe outbound before the final snapshot commits"
    );

    save_gate.release.add_permits(1);
    assert_eq!(
        sending.await.unwrap().unwrap_err(),
        super::ConnectorManagerError::Persistence
    );
    assert!(!delivering.await.unwrap().unwrap());
    assert!(transport.sent.lock().unwrap().is_empty());
    assert!(state
        .read()
        .await
        .outbound
        .values()
        .all(|outbound| outbound.connector_id != connector.id));
    manager.shutdown().await;
}

#[tokio::test]
async fn owner_thread_run_releases_lifecycle_lock_and_rolls_back_if_deleted() {
    let entered = Arc::new(Semaphore::new(0));
    let release = Arc::new(Semaphore::new(0));
    let mut daemon = DaemonState::with_model_adapter(Arc::new(GateModelAdapter {
        entered: Arc::clone(&entered),
        release: Arc::clone(&release),
    }));
    daemon.create_agent(test_config()).unwrap();
    let state = Arc::new(RwLock::new(daemon));
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    let manager = manager(
        Arc::clone(&state),
        Arc::new(InMemoryCredentialStore::default()),
        Arc::new(FakeTransport::default()),
    );
    let connector = manager
        .create(
            agent_id.clone(),
            TelegramBotToken::parse("42:owner-thread").unwrap(),
        )
        .await
        .unwrap();
    manager
        .accept_batch(
            connector.id.clone(),
            TelegramUpdateBatch {
                updates: vec![text_update(1, "101", "pair")],
                next_update_id: 2,
            },
        )
        .await
        .unwrap();
    manager
        .approve_pending_chat(connector.id.clone(), Some("101".into()))
        .await
        .unwrap();
    let baseline = state.read().await.get_agent(&agent_id).unwrap();

    let sending_manager = manager.clone();
    let sending_agent_id = agent_id.clone();
    let sending_connector_id = connector.id.clone();
    let sending = tokio::spawn(async move {
        sending_manager
            .send_from_owner(
                sending_agent_id,
                sending_connector_id,
                "owner web turn".into(),
                "owner-delete-race".into(),
            )
            .await
    });
    entered
        .acquire()
        .await
        .expect("owner thread run should enter model")
        .forget();

    let deleting_manager = manager.clone();
    let deleting_connector_id = connector.id.clone();
    let deleting =
        tokio::spawn(async move { deleting_manager.delete(deleting_connector_id).await });
    let deleted_while_model_was_running =
        tokio::time::timeout(Duration::from_millis(200), deleting).await;
    release.add_permits(1);

    assert!(
        deleted_while_model_was_running.is_ok(),
        "connector lifecycle mutations must not wait for model/tool execution"
    );
    assert!(deleted_while_model_was_running.unwrap().unwrap().is_ok());
    assert_eq!(
        sending.await.unwrap().unwrap_err(),
        super::ConnectorManagerError::Persistence
    );
    let state = state.read().await;
    assert_eq!(
        state.get_agent(&agent_id).unwrap().messages,
        baseline.messages
    );
    assert!(state
        .outbound
        .values()
        .all(|outbound| outbound.connector_id != connector.id));
    assert!(state.connectors[&connector.id].deleted_at_ms.is_some());
}

#[tokio::test]
async fn failed_run_publish_preserves_inbound_accepted_while_model_was_running() {
    let entered = Arc::new(Semaphore::new(0));
    let release = Arc::new(Semaphore::new(0));
    let mut daemon = DaemonState::with_model_adapter(Arc::new(GateModelAdapter {
        entered: Arc::clone(&entered),
        release: Arc::clone(&release),
    }));
    daemon.create_agent(test_config()).unwrap();
    let state = Arc::new(RwLock::new(daemon));
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    let temporary = std::env::temp_dir().join(format!(
        "anima-run-delta-rollback-{}-{}",
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
            agent_id.clone(),
            TelegramBotToken::parse("42:delta-rollback").unwrap(),
        )
        .await
        .unwrap();
    manager.stop_worker(&connector.id).await.unwrap();
    manager
        .accept_batch(
            connector.id.clone(),
            TelegramUpdateBatch {
                updates: vec![text_update(1, "101", "pair")],
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
                updates: vec![text_update(2, "101", "run A")],
                next_update_id: 3,
            },
        )
        .await
        .unwrap();

    let processing = {
        let manager = manager.clone();
        let connector_id = connector.id.clone();
        tokio::spawn(async move { manager.process_pending_once(connector_id).await })
    };
    entered.acquire().await.unwrap().forget();
    manager
        .accept_batch(
            connector.id.clone(),
            TelegramUpdateBatch {
                updates: vec![text_update(3, "101", "run B")],
                next_update_id: 4,
            },
        )
        .await
        .unwrap();
    let gate = state
        .write()
        .await
        .install_test_control_plane_non_turn_save_gate(true);
    release.add_permits(1);
    gate.entered.acquire().await.unwrap().forget();
    gate.release.add_permits(1);

    assert_eq!(
        processing.await.unwrap().unwrap_err(),
        super::ConnectorManagerError::Persistence
    );
    let state_guard = state.read().await;
    assert!(state_guard.inbound.contains_key(&(connector.id.clone(), 2)));
    assert_eq!(
        state_guard.inbound[&(connector.id.clone(), 3)].normalized_text,
        "run B"
    );
    drop(state_guard);

    let _transaction = manager.runs.control_plane_transaction().await;
    let later_publish = {
        let mut state = state.write().await;
        state
            .update_agent(
                &agent_id,
                AgentConfigUpdate {
                    name: Some("published after rollback".into()),
                    ..AgentConfigUpdate::default()
                },
            )
            .unwrap();
        state.control_plane_persist_request()
    };
    later_publish.save().await.unwrap();
    drop(_transaction);
    let persisted = crate::control_plane_store::load_control_plane_snapshot(
        &crate::control_plane_store::ControlPlaneStoreConfig::Json(snapshot_path),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(persisted
        .inbound
        .iter()
        .any(|record| record.connector_id == connector.id && record.update_id == 3));
    let mut restored = DaemonState::new();
    restored.restore_control_plane_snapshot(persisted).unwrap();
    assert!(restored.inbound.contains_key(&(connector.id.clone(), 3)));
    manager.shutdown().await;
    std::fs::remove_dir_all(temporary).unwrap();
}

#[tokio::test]
async fn cross_room_rollback_preserves_a_turn_committed_while_the_connector_ran() {
    let entered = Arc::new(Semaphore::new(0));
    let release = Arc::new(Semaphore::new(0));
    let mut daemon = DaemonState::with_model_adapter(Arc::new(GateModelAdapter {
        entered: Arc::clone(&entered),
        release: Arc::clone(&release),
    }));
    daemon.create_agent(test_config()).unwrap();
    let state = Arc::new(RwLock::new(daemon));
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    let runs = AgentRunCoordinator::new(Arc::clone(&state), Arc::new(Semaphore::new(2)));
    let manager = ConnectorManager::new(
        Arc::clone(&state),
        runs.clone(),
        Arc::new(InMemoryCredentialStore::default()),
        Arc::new(FakeTransport::default()),
    );
    let connector = manager
        .create(
            agent_id.clone(),
            TelegramBotToken::parse("42:cross-room-token").unwrap(),
        )
        .await
        .unwrap();
    manager.shutdown().await;
    manager
        .accept_batch(
            connector.id.clone(),
            TelegramUpdateBatch {
                updates: vec![text_update(1, "101", "pair")],
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
                updates: vec![text_update(2, "101", "connector run")],
                next_update_id: 3,
            },
        )
        .await
        .unwrap();
    let original = state.read().await.get_agent(&agent_id).unwrap();

    let temporary = std::env::temp_dir().join(format!(
        "anima-connector-cross-room-rollback-{}-{}",
        std::process::id(),
        super::now_ms()
    ));
    std::fs::create_dir_all(&temporary).unwrap();
    let snapshot_path = temporary.join("control-plane.json");
    state.write().await.set_control_plane_store(Some(
        crate::control_plane_store::ControlPlaneStoreConfig::Json(snapshot_path.clone()),
    ));

    let intervening_runs = runs.clone();
    let intervening_agent_id = agent_id.clone();
    let intervening = tokio::spawn(async move {
        intervening_runs
            .run(AgentRunRequest {
                agent_id: intervening_agent_id,
                content: Content {
                    text: "intervening turn".into(),
                    ..Content::default()
                },
                room: RunRoom::Generated,
                idempotency_key: None,
                source: RunSource::Api,
                source_ref: None,
                parent: None,
            })
            .await
    });
    entered
        .acquire()
        .await
        .expect("the intervening turn should enter the model")
        .forget();
    let processing_manager = manager.clone();
    let connector_id = connector.id.clone();
    let processing =
        tokio::spawn(async move { processing_manager.process_pending_once(connector_id).await });
    entered
        .acquire()
        .await
        .expect("the connector turn runs concurrently in its own room")
        .forget();

    // The intervening turn waited on the gate first, so it is released first.
    release.add_permits(1);
    intervening.await.unwrap().unwrap();
    let committed_intervening = state.read().await.get_agent(&agent_id).unwrap();
    assert_eq!(
        committed_intervening.messages.len(),
        original.messages.len() + 2
    );

    std::fs::remove_file(&snapshot_path).unwrap();
    std::fs::create_dir(&snapshot_path).unwrap();
    release.add_permits(1);
    assert_eq!(
        processing.await.unwrap().unwrap_err(),
        super::ConnectorManagerError::Persistence
    );

    let rolled_back = state.read().await.get_agent(&agent_id).unwrap();
    assert_eq!(rolled_back.messages, committed_intervening.messages);
    assert_eq!(
        state
            .read()
            .await
            .inbound
            .get(&(connector.id.clone(), 2))
            .unwrap()
            .processing_state,
        InboundProcessingState::Processing
    );
    assert!(state
        .read()
        .await
        .outbound
        .values()
        .all(|record| record.connector_id != connector.id));

    std::fs::remove_dir(&snapshot_path).unwrap();
    std::fs::remove_dir_all(temporary).unwrap();
}
