use super::*;

#[tokio::test]
async fn create_starts_one_worker_restart_replaces_it_and_shutdown_joins_it() {
    let state = state_with_agent();
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    let manager = manager(
        state,
        Arc::new(InMemoryCredentialStore::default()),
        Arc::new(FakeTransport::default()),
    );
    let connector = manager
        .create(
            agent_id,
            TelegramBotToken::parse("42:worker-token").unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(manager.worker_count().await, 1);

    manager.restart(connector.id.clone()).await.unwrap();
    assert_eq!(manager.worker_count().await, 1);

    manager.shutdown().await;
    assert_eq!(manager.worker_count().await, 0);
}

#[tokio::test]
async fn concurrent_restart_and_shutdown_cannot_resurrect_a_worker() {
    let state = state_with_agent();
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    let entered = Arc::new(Semaphore::new(0));
    let release = Arc::new(Semaphore::new(0));
    let credentials = Arc::new(GateLoadCredentialStore {
        inner: InMemoryCredentialStore::default(),
        gate_load: AtomicBool::new(false),
        entered: Arc::clone(&entered),
        release: Arc::clone(&release),
    });
    let manager = manager(
        state,
        credentials.clone(),
        Arc::new(FakeTransport::default()),
    );
    let connector = manager
        .create(
            agent_id,
            TelegramBotToken::parse("42:worker-token").unwrap(),
        )
        .await
        .unwrap();
    credentials.gate_load.store(true, Ordering::SeqCst);

    let restarting_manager = manager.clone();
    let connector_id = connector.id.clone();
    let restart = tokio::spawn(async move { restarting_manager.restart(connector_id).await });
    entered
        .acquire()
        .await
        .expect("restart should reach credential load")
        .forget();
    let shutdown_manager = manager.clone();
    let shutdown = tokio::spawn(async move { shutdown_manager.shutdown().await });
    tokio::time::sleep(Duration::from_millis(20)).await;
    let shutdown_waited_for_restart = !shutdown.is_finished();
    release.add_permits(1);
    let restart_result = restart.await.unwrap();
    assert!(
        restart_result.is_ok()
            || restart_result == Err(super::ConnectorManagerError::WorkerStopped)
    );
    shutdown.await.unwrap();
    let remaining_workers = manager.worker_count().await;
    manager.shutdown().await;

    assert!(shutdown_waited_for_restart);
    assert_eq!(remaining_workers, 0);
}

#[tokio::test]
async fn naturally_finished_worker_is_pruned_from_the_registry() {
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
            TelegramBotToken::parse("42:worker-token").unwrap(),
        )
        .await
        .unwrap();
    state
        .write()
        .await
        .connectors
        .get_mut(&connector.id)
        .unwrap()
        .enabled = false;

    let pruned = tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if manager.worker_count().await == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .is_ok();
    manager.shutdown().await;

    assert!(pruned);
}

#[tokio::test]
async fn shutdown_is_terminal_for_later_restart_requests() {
    let state = state_with_agent();
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    let manager = manager(
        state,
        Arc::new(InMemoryCredentialStore::default()),
        Arc::new(FakeTransport::default()),
    );
    let connector = manager
        .create(
            agent_id,
            TelegramBotToken::parse("42:shutdown-token").unwrap(),
        )
        .await
        .unwrap();

    manager.shutdown().await;

    assert_eq!(
        manager.restart(connector.id).await.unwrap_err(),
        super::ConnectorManagerError::WorkerStopped
    );
    assert_eq!(manager.worker_count().await, 0);
}

#[tokio::test]
async fn dropping_manager_does_not_leave_a_worker_registry_cycle() {
    let state = state_with_agent();
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    let manager = manager(
        state,
        Arc::new(InMemoryCredentialStore::default()),
        Arc::new(FakeTransport::default()),
    );
    manager
        .create(agent_id, TelegramBotToken::parse("42:drop-token").unwrap())
        .await
        .unwrap();
    let registry = Arc::downgrade(&manager.workers);

    drop(manager);

    tokio::time::timeout(Duration::from_secs(1), async {
        while registry.upgrade().is_some() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("worker must not strongly own its manager registry");
}

#[tokio::test]
async fn dropping_last_manager_owner_cancels_the_in_flight_poll() {
    let state = state_with_agent();
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    let entered = Arc::new(Semaphore::new(0));
    let exited = Arc::new(Semaphore::new(0));
    let manager = manager(
        state,
        Arc::new(InMemoryCredentialStore::default()),
        Arc::new(DropAwarePollingTransport {
            entered: Arc::clone(&entered),
            exited: Arc::clone(&exited),
        }),
    );
    manager
        .create(agent_id, TelegramBotToken::parse("42:drop-token").unwrap())
        .await
        .unwrap();
    entered.acquire().await.unwrap().forget();
    let registry_observer = Arc::clone(&manager.workers);

    drop(manager);

    tokio::time::timeout(Duration::from_secs(1), exited.acquire())
        .await
        .expect("dropping the last public manager owner must cancel polling")
        .unwrap()
        .forget();
    drop(registry_observer);
}

#[tokio::test]
async fn a_poll_conflict_is_reported_and_backs_off_instead_of_retrying_fast() {
    let state = state_with_agent();
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    let transport = Arc::new(FakeTransport::default());
    transport.conflict_poll.store(true, Ordering::SeqCst);
    let manager = manager(
        state,
        Arc::new(InMemoryCredentialStore::default()),
        transport.clone(),
    );
    let connector = manager
        .create(
            agent_id,
            TelegramBotToken::parse("42:conflict-token").unwrap(),
        )
        .await
        .unwrap();

    assert!(
        wait_for_status(
            &manager,
            &connector.id,
            super::ConnectorRuntimeStatus::PollConflict,
            Duration::from_secs(2),
        )
        .await
    );
    // A generic failure retries after 100 ms; a conflict must wait far longer.
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(transport.poll_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        manager.status(&connector.id).await,
        Some(super::ConnectorRuntimeStatus::PollConflict)
    );
    manager.shutdown().await;
}

#[tokio::test]
async fn restored_connector_without_a_credential_is_safe_and_does_not_block_startup() {
    let state = state_with_agent();
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    let credentials = Arc::new(InMemoryCredentialStore::default());
    let first = manager(
        Arc::clone(&state),
        credentials.clone(),
        Arc::new(FakeTransport::default()),
    );
    let connector = first
        .create(
            agent_id,
            TelegramBotToken::parse("42:missing-token").unwrap(),
        )
        .await
        .unwrap();
    first.shutdown().await;
    credentials.delete(&connector.id).await.unwrap();

    let restored = manager(state, credentials, Arc::new(FakeTransport::default()));
    restored.start_restored().await;
    assert_eq!(restored.worker_count().await, 0);
    assert_eq!(
        restored.status(&connector.id).await,
        Some(super::ConnectorRuntimeStatus::CredentialRequired)
    );
    restored.shutdown().await;
}

#[tokio::test]
async fn restored_connector_without_credential_can_be_repaired_by_replacement() {
    let state = state_with_agent();
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    let credentials = Arc::new(InMemoryCredentialStore::default());
    let first = manager(
        Arc::clone(&state),
        credentials.clone(),
        Arc::new(FakeTransport::default()),
    );
    let connector = first
        .create(agent_id, TelegramBotToken::parse("42:lost-token").unwrap())
        .await
        .unwrap();
    first.shutdown().await;
    credentials.delete(&connector.id).await.unwrap();
    let manager = manager(
        Arc::clone(&state),
        credentials.clone(),
        Arc::new(FakeTransport::default()),
    );
    manager.start_restored().await;
    assert_eq!(
        manager.status(&connector.id).await,
        Some(ConnectorRuntimeStatus::CredentialRequired)
    );

    let repaired = manager
        .replace_token(
            connector.id.clone(),
            TelegramBotToken::parse("84:repair-token").unwrap(),
        )
        .await
        .expect("a supplied verified token should repair missing credentials");

    assert_eq!(repaired.bot.id, "84");
    assert_eq!(manager.worker_count().await, 1);
    assert_eq!(
        credentials
            .load(&connector.id)
            .await
            .unwrap()
            .unwrap()
            .expose(),
        "84:repair-token"
    );
    manager.shutdown().await;
}

#[tokio::test]
async fn restored_connector_without_credential_can_be_deleted() {
    let state = state_with_agent();
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    let credentials = Arc::new(InMemoryCredentialStore::default());
    let first = manager(
        Arc::clone(&state),
        credentials.clone(),
        Arc::new(FakeTransport::default()),
    );
    let connector = first
        .create(agent_id, TelegramBotToken::parse("42:lost-token").unwrap())
        .await
        .unwrap();
    first.shutdown().await;
    credentials.delete(&connector.id).await.unwrap();
    let manager = manager(
        Arc::clone(&state),
        credentials,
        Arc::new(FakeTransport::default()),
    );
    manager.start_restored().await;

    manager
        .delete(connector.id.clone())
        .await
        .expect("verified credential absence should not block metadata deletion");

    assert!(!state.read().await.connectors[&connector.id].is_active());
    assert_eq!(manager.status(&connector.id).await, None);
}
