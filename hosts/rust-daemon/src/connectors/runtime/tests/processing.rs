use super::*;

#[tokio::test]
async fn background_processing_waits_for_global_admission_instead_of_failing() {
    let state = state_with_agent();
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    let limiter = Arc::new(Semaphore::new(0));
    let runs = AgentRunCoordinator::new(Arc::clone(&state), Arc::clone(&limiter));
    let manager = ConnectorManager::new(
        Arc::clone(&state),
        runs,
        Arc::new(InMemoryCredentialStore::default()),
        Arc::new(FakeTransport::default()),
    );
    let connector = manager
        .create(agent_id, TelegramBotToken::parse("42:token").unwrap())
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
                updates: vec![text_update(2, "101", "wait")],
                next_update_id: 3,
            },
        )
        .await
        .unwrap();

    let processing_manager = manager.clone();
    let connector_id = connector.id.clone();
    let processing =
        tokio::spawn(async move { processing_manager.process_pending_once(connector_id).await });
    tokio::time::sleep(Duration::from_millis(20)).await;
    let waited = !processing.is_finished();
    limiter.add_permits(1);
    let result = processing.await.unwrap();

    assert!(waited);
    assert_eq!(result, Ok(true));
}

/// An owner thread (no paired chat) whose room `blocker` holds without a
/// global permit, so the next owner send passes the permit pre-check and
/// then waits for that room. Its agent's model calls park on the gate.
struct BlockedOwnerThread {
    manager: ConnectorManager,
    runs: AgentRunCoordinator,
    agent_id: String,
    connector_id: String,
    blocker: crate::agent_runs::RunReservation,
    entered: Arc<Semaphore>,
    release: Arc<Semaphore>,
}

async fn owner_thread_with_blocked_room(limiter: Arc<Semaphore>) -> BlockedOwnerThread {
    let entered = Arc::new(Semaphore::new(0));
    let release = Arc::new(Semaphore::new(0));
    let mut daemon = DaemonState::with_model_adapter(Arc::new(GateModelAdapter {
        entered: Arc::clone(&entered),
        release: Arc::clone(&release),
    }));
    daemon.create_agent(test_config()).unwrap();
    let state = Arc::new(RwLock::new(daemon));
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    let runs = AgentRunCoordinator::new(Arc::clone(&state), limiter);
    let manager = ConnectorManager::new(
        Arc::clone(&state),
        runs.clone(),
        Arc::new(InMemoryCredentialStore::default()),
        Arc::new(FakeTransport::default()),
    );
    let connector = manager
        .create(
            agent_id.clone(),
            TelegramBotToken::parse("42:owner-waiting").unwrap(),
        )
        .await
        .unwrap();
    manager.stop_worker(&connector.id).await.unwrap();
    let blocker = runs
        .admit(
            &agent_id,
            &connector.room_id,
            crate::agent_runs::AdmitMode::TryNow,
        )
        .await
        .expect("the owner thread's room is free");
    BlockedOwnerThread {
        manager,
        runs,
        agent_id,
        connector_id: connector.id,
        blocker,
        entered,
        release,
    }
}

fn spawn_owner_send(
    manager: &ConnectorManager,
    agent_id: &str,
    connector_id: &str,
    key: &str,
) -> tokio::task::JoinHandle<
    Result<(crate::routes::AgentRunEnvelope, bool), super::ConnectorManagerError>,
> {
    let manager = manager.clone();
    let agent_id = agent_id.to_string();
    let connector_id = connector_id.to_string();
    let key = key.to_string();
    tokio::spawn(async move {
        manager
            .send_from_owner(agent_id, connector_id, format!("owner turn {key}"), key)
            .await
    })
}

/// Once past the permit pre-check an owner send waits for a global
/// permit instead of failing (spec §4.3), and holds one unit of its
/// agent's waiting budget only while it waits.
#[tokio::test]
async fn owner_send_waits_for_a_permit_and_holds_a_waiting_unit_only_while_waiting() {
    let limiter = Arc::new(Semaphore::new(1));
    let BlockedOwnerThread {
        manager,
        runs,
        agent_id,
        connector_id,
        blocker,
        entered,
        release,
    } = owner_thread_with_blocked_room(Arc::clone(&limiter)).await;

    let sending = spawn_owner_send(&manager, &agent_id, &connector_id, "owner-waits");
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
    assert!(!sending.is_finished(), "the send waits for its room");
    assert_eq!(
        runs.waiting_runs(&agent_id),
        1,
        "a waiting owner send holds one unit"
    );

    // The only permit is taken, then the room frees: the send now waits
    // for a permit rather than failing.
    let elsewhere = limiter
        .clone()
        .try_acquire_owned()
        .expect("the permit is free");
    drop(blocker);
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(
        !sending.is_finished(),
        "an owner send waits for a permit instead of failing"
    );
    assert_eq!(
        runs.waiting_runs(&agent_id),
        1,
        "still waiting, so it keeps its unit"
    );

    drop(elsewhere);
    tokio::time::timeout(Duration::from_secs(3), entered.acquire())
        .await
        .expect("the send takes the freed permit and enters the model")
        .unwrap()
        .forget();
    assert_eq!(
        runs.waiting_runs(&agent_id),
        0,
        "a send holding its permit no longer holds a unit"
    );
    release.add_permits(1);
    let (run, delivery_queued) = sending.await.unwrap().expect("the owner send succeeds");
    assert_eq!(run.result.status, "success");
    assert!(
        !delivery_queued,
        "an unpaired owner thread queues no delivery"
    );
    manager.shutdown().await;
}

/// Owner sends share their agent's waiting budget (spec §16): with eight
/// sends waiting, the next gets the saturation backpressure at once.
#[tokio::test]
async fn owner_sends_beyond_the_waiting_budget_get_backpressure_at_once() {
    use crate::agent_runs::MAX_QUEUED_RUNS_PER_AGENT;

    let BlockedOwnerThread {
        manager,
        runs,
        agent_id,
        connector_id,
        blocker,
        release,
        ..
    } = owner_thread_with_blocked_room(Arc::new(Semaphore::new(4))).await;
    let waiting: Vec<_> = (1..=MAX_QUEUED_RUNS_PER_AGENT)
        .map(|index| {
            spawn_owner_send(
                &manager,
                &agent_id,
                &connector_id,
                &format!("owner-{index}"),
            )
        })
        .collect();
    for _ in 0..50 {
        tokio::task::yield_now().await;
    }

    let refused = tokio::time::timeout(
        Duration::from_secs(1),
        spawn_owner_send(&manager, &agent_id, &connector_id, "owner-refused"),
    )
    .await
    .expect("the ninth waiting send is answered at once")
    .unwrap();
    assert_eq!(
        refused.unwrap_err(),
        super::ConnectorManagerError::Backpressure
    );

    drop(blocker);
    release.add_permits(MAX_QUEUED_RUNS_PER_AGENT);
    for send in waiting {
        assert!(send.await.unwrap().is_ok(), "every waiting send runs");
    }
    assert_eq!(runs.waiting_runs(&agent_id), 0);
    manager.shutdown().await;
}

#[tokio::test]
async fn processing_commits_agent_message_inbound_and_outbox_then_delivers_stored_text() {
    let state = state_with_agent();
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    let credentials = Arc::new(InMemoryCredentialStore::default());
    let transport = Arc::new(FakeTransport::default());
    let manager = manager(Arc::clone(&state), credentials, transport.clone());
    let connector = manager
        .create(
            agent_id.clone(),
            TelegramBotToken::parse("42:processing-token").unwrap(),
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
                updates: vec![text_update(2, "202", "hello from telegram")],
                next_update_id: 3,
            },
        )
        .await
        .unwrap();

    manager
        .process_pending_once(connector.id.clone())
        .await
        .unwrap();
    let outbound = {
        let state = state.read().await;
        assert_eq!(
            state.inbound[&(connector.id.clone(), 2)].processing_state,
            InboundProcessingState::Processed
        );
        let outbound = state.outbound.values().next().cloned().unwrap();
        assert_eq!(outbound.text, "operator handled task: hello from telegram");
        let snapshot = state.get_agent(&agent_id).unwrap();
        assert!(snapshot.messages.iter().any(|message| {
            message.id == outbound.assistant_message_id
                && message.room_id == connector.room_id
                && message.role == anima_core::MessageRole::Assistant
        }));
        outbound
    };

    {
        let mut state = state.write().await;
        for (id, connector_id) in [
            ("old-selected", connector.id.clone()),
            ("old-other", "other-connector".into()),
        ] {
            state.outbound.insert(
                id.into(),
                crate::connectors::TelegramOutboundRecord {
                    id: id.into(),
                    connector_id,
                    agent_id: agent_id.clone(),
                    room_id: connector.room_id.clone(),
                    assistant_message_id: format!("assistant-{id}"),
                    text: id.into(),
                    created_at_ms: 1,
                    delivered_at_ms: Some(1),
                    attempts: 1,
                    delivery_state: crate::connectors::OutboundDeliveryState::Delivered,
                    message_pruned: false,
                },
            );
        }
    }

    manager
        .deliver_pending_once(connector.id.clone())
        .await
        .unwrap();
    let delivered = state.read().await.outbound[&outbound.id].clone();
    assert_eq!(
        delivered.delivery_state,
        crate::connectors::OutboundDeliveryState::Delivered
    );
    assert_eq!(delivered.attempts, 1);
    assert!(delivered.delivered_at_ms.is_some());
    assert!(!state.read().await.outbound.contains_key("old-selected"));
    assert!(state.read().await.outbound.contains_key("old-other"));
    assert_eq!(
        transport.sent.lock().unwrap().as_slice(),
        [("202".into(), outbound.text)]
    );
    manager.shutdown().await;
}

#[tokio::test]
async fn delivery_retry_uses_outbox_without_running_the_agent_again() {
    let state = state_with_agent();
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    let transport = Arc::new(FakeTransport::default());
    transport.remaining_send_failures.store(1, Ordering::SeqCst);
    let manager = manager(
        Arc::clone(&state),
        Arc::new(InMemoryCredentialStore::default()),
        transport.clone(),
    );
    let connector = manager
        .create(
            agent_id.clone(),
            TelegramBotToken::parse("42:retry-token").unwrap(),
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
                updates: vec![text_update(2, "202", "retry me")],
                next_update_id: 3,
            },
        )
        .await
        .unwrap();
    manager
        .process_pending_once(connector.id.clone())
        .await
        .unwrap();
    let message_count = state
        .read()
        .await
        .get_agent(&agent_id)
        .unwrap()
        .message_count;

    assert_eq!(
        manager
            .deliver_pending_once(connector.id.clone())
            .await
            .unwrap_err(),
        super::ConnectorManagerError::Transport
    );
    assert_eq!(
        manager.status(&connector.id).await,
        Some(ConnectorRuntimeStatus::Error)
    );
    manager.refresh_operational_status(&connector.id).await;
    assert_eq!(
        manager.status(&connector.id).await,
        Some(ConnectorRuntimeStatus::Error),
        "a successful poll/status refresh must not mask failed delivery"
    );
    assert!(manager
        .deliver_pending_once(connector.id.clone())
        .await
        .unwrap());
    assert_eq!(
        manager.status(&connector.id).await,
        Some(ConnectorRuntimeStatus::Ready)
    );
    assert!(!manager
        .process_pending_once(connector.id.clone())
        .await
        .unwrap());
    assert_eq!(
        state
            .read()
            .await
            .get_agent(&agent_id)
            .unwrap()
            .message_count,
        message_count
    );
    assert_eq!(transport.sent.lock().unwrap().len(), 1);
    manager.shutdown().await;
}

#[tokio::test]
async fn undelivered_outbox_backpressure_preserves_inbound_without_running_agent() {
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
            TelegramBotToken::parse("42:backpressure").unwrap(),
        )
        .await
        .unwrap();
    manager.shutdown().await;
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
                updates: vec![text_update(2, "202", "do not lose me")],
                next_update_id: 3,
            },
        )
        .await
        .unwrap();
    {
        let mut state = state.write().await;
        for index in 0..super::MAX_UNDELIVERED_OUTBOUND {
            let id = format!("blocked-{index}");
            state.outbound.insert(
                id.clone(),
                crate::connectors::TelegramOutboundRecord {
                    id,
                    connector_id: connector.id.clone(),
                    agent_id: agent_id.clone(),
                    room_id: connector.room_id.clone(),
                    assistant_message_id: format!("assistant-{index}"),
                    text: "retry later".into(),
                    created_at_ms: index as u64,
                    delivered_at_ms: None,
                    attempts: 1,
                    delivery_state: crate::connectors::OutboundDeliveryState::Failed,
                    message_pruned: false,
                },
            );
        }
    }
    let message_count = state
        .read()
        .await
        .get_agent(&agent_id)
        .unwrap()
        .message_count;

    assert!(!manager
        .process_pending_once(connector.id.clone())
        .await
        .unwrap());
    assert_eq!(
        state.read().await.inbound[&(connector.id.clone(), 2)].processing_state,
        InboundProcessingState::Received
    );
    assert_eq!(
        state
            .read()
            .await
            .get_agent(&agent_id)
            .unwrap()
            .message_count,
        message_count
    );
    assert_eq!(
        manager.status(&connector.id).await,
        Some(ConnectorRuntimeStatus::Error)
    );
}

#[tokio::test]
async fn inbound_backpressure_is_bounded_and_worker_recovers_without_losing_the_batch() {
    let state = state_with_agent();
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    let delivery_attempts = Arc::new(Semaphore::new(0));
    let transport = Arc::new(InboundBackpressureTransport {
        enabled: AtomicBool::new(false),
        fail_delivery: AtomicBool::new(true),
        delivery_attempts: Arc::clone(&delivery_attempts),
        update: text_update(10_000, "202", "resume after pressure"),
    });
    let manager = manager(
        Arc::clone(&state),
        Arc::new(InMemoryCredentialStore::default()),
        transport.clone(),
    );
    let token = TelegramBotToken::parse("42:bounded-inbound").unwrap();
    let connector = manager
        .create(agent_id.clone(), token.clone())
        .await
        .unwrap();
    manager.stop_worker(&connector.id).await.unwrap();
    {
        let mut state = state.write().await;
        state
            .connectors
            .get_mut(&connector.id)
            .unwrap()
            .approved_chat = Some(text_update(0, "202", "pair").chat);
        for index in 0..super::MAX_LIVE_INBOUND {
            let update_id = index as i64 + 1;
            state.inbound.insert(
                (connector.id.clone(), update_id),
                crate::connectors::TelegramInboundRecord {
                    connector_id: connector.id.clone(),
                    update_id,
                    agent_id: agent_id.clone(),
                    room_id: connector.room_id.clone(),
                    normalized_text: format!("queued-{index}"),
                    sender: text_update(update_id, "202", "queued").sender,
                    chat: text_update(update_id, "202", "queued").chat,
                    received_at_ms: index as u64 + 1,
                    processing_state: InboundProcessingState::Received,
                    run_idempotency_key: format!("telegram:{}:update:{update_id}", connector.id),
                },
            );
        }
        state.outbound.insert(
            "pressure-blocker".into(),
            crate::connectors::TelegramOutboundRecord {
                id: "pressure-blocker".into(),
                connector_id: connector.id.clone(),
                agent_id,
                room_id: connector.room_id.clone(),
                assistant_message_id: "pressure-assistant".into(),
                text: "retry delivery".into(),
                created_at_ms: 1,
                delivered_at_ms: None,
                attempts: 1,
                delivery_state: crate::connectors::OutboundDeliveryState::Failed,
                message_pruned: false,
            },
        );
    }
    transport.enabled.store(true, Ordering::SeqCst);
    manager
        .start_worker(connector.id.clone(), token)
        .await
        .unwrap();

    tokio::time::timeout(Duration::from_secs(1), delivery_attempts.acquire())
        .await
        .expect("a pressured worker must keep retrying delivery")
        .unwrap()
        .forget();
    {
        let state = state.read().await;
        assert_eq!(state.connectors[&connector.id].next_update_id, 0);
        assert_eq!(
            state
                .inbound
                .values()
                .filter(|record| {
                    record.connector_id == connector.id
                        && matches!(
                            record.processing_state,
                            InboundProcessingState::Received | InboundProcessingState::Processing
                        )
                })
                .count(),
            super::MAX_LIVE_INBOUND
        );
    }
    assert_eq!(
        manager.status(&connector.id).await,
        Some(ConnectorRuntimeStatus::Degraded)
    );

    transport.fail_delivery.store(false, Ordering::SeqCst);
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if state.read().await.connectors[&connector.id].next_update_id == 10_001 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("delivery recovery must resume ingestion at the unchanged cursor");
    let state = state.read().await;
    assert!(state.inbound.contains_key(&(connector.id.clone(), 10_000)));
    assert!(
        state
            .inbound
            .values()
            .filter(|record| {
                record.connector_id == connector.id
                    && matches!(
                        record.processing_state,
                        InboundProcessingState::Received | InboundProcessingState::Processing
                    )
            })
            .count()
            <= super::MAX_LIVE_INBOUND
    );
    drop(state);
    manager.shutdown().await;
}

#[tokio::test]
async fn uncertain_delivery_credential_enters_reconciliation_without_sending() {
    let state = state_with_agent();
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    let setup = manager(
        Arc::clone(&state),
        Arc::new(InMemoryCredentialStore::default()),
        Arc::new(FakeTransport::default()),
    );
    let connector = setup
        .create(agent_id, TelegramBotToken::parse("42:token").unwrap())
        .await
        .unwrap();
    setup.shutdown().await;
    setup
        .accept_batch(
            connector.id.clone(),
            TelegramUpdateBatch {
                updates: vec![text_update(1, "101", "pair")],
                next_update_id: 2,
            },
        )
        .await
        .unwrap();
    setup.approve_pending(connector.id.clone()).await.unwrap();
    setup
        .accept_batch(
            connector.id.clone(),
            TelegramUpdateBatch {
                updates: vec![text_update(2, "101", "run")],
                next_update_id: 3,
            },
        )
        .await
        .unwrap();
    setup
        .process_pending_once(connector.id.clone())
        .await
        .unwrap();

    let transport = Arc::new(FakeTransport::default());
    let delivery = manager(state, Arc::new(UncertainLoadStore), transport.clone());
    assert_eq!(
        delivery
            .deliver_pending_once(connector.id.clone())
            .await
            .unwrap_err(),
        super::ConnectorManagerError::CredentialStateUncertain
    );
    assert_eq!(
        delivery.status(&connector.id).await,
        Some(super::ConnectorRuntimeStatus::Reconciling)
    );
    assert!(transport.sent.lock().unwrap().is_empty());
}
