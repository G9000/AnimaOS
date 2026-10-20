use super::*;

impl ConnectorManager {
    pub(crate) async fn restart(&self, connector_id: String) -> Result<(), ConnectorManagerError> {
        let manager = self.clone();
        tokio::spawn(async move { manager.restart_owned(connector_id).await })
            .await
            .map_err(|_| ConnectorManagerError::WorkerStopped)?
    }

    async fn restart_owned(&self, connector_id: String) -> Result<(), ConnectorManagerError> {
        let _lifecycle = self.lifecycle_lock.lock().await;
        self.ensure_open()?;
        self.restart_unlocked(connector_id).await
    }

    async fn restart_unlocked(&self, connector_id: String) -> Result<(), ConnectorManagerError> {
        self.stop_worker(&connector_id).await?;
        let connector = self
            .state
            .read()
            .await
            .connectors
            .get(&connector_id)
            .filter(|connector| connector.is_active())
            .cloned()
            .ok_or(ConnectorManagerError::ConnectorNotFound)?;
        let token = match self.credentials.load(&connector_id).await {
            Ok(Some(token)) => token,
            Ok(None) => {
                self.statuses
                    .lock()
                    .await
                    .insert(connector_id, ConnectorRuntimeStatus::CredentialRequired);
                return Ok(());
            }
            Err(CredentialStoreError::CredentialStateUncertain) => {
                self.statuses
                    .lock()
                    .await
                    .insert(connector_id, ConnectorRuntimeStatus::Reconciling);
                return Ok(());
            }
            Err(_) => {
                self.statuses
                    .lock()
                    .await
                    .insert(connector_id, ConnectorRuntimeStatus::Error);
                return Ok(());
            }
        };
        let status = if connector.approved_chat.is_some() {
            ConnectorRuntimeStatus::Ready
        } else {
            ConnectorRuntimeStatus::Pairing
        };
        self.statuses
            .lock()
            .await
            .insert(connector_id.clone(), status);
        self.start_worker(connector_id, token).await
    }

    pub(crate) async fn start_restored(&self) {
        let _lifecycle = self.lifecycle_lock.lock().await;
        if self.ensure_open().is_err() {
            return;
        }
        self.cleanup_restored_credentials().await;
        let connectors = self
            .state
            .read()
            .await
            .connectors
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for connector in connectors {
            if connector.deleted_at_ms.is_some() {
                match self.credentials.delete(&connector.id).await {
                    Ok(()) => {
                        self.statuses.lock().await.remove(&connector.id);
                    }
                    Err(_) => {
                        self.statuses
                            .lock()
                            .await
                            .insert(connector.id, ConnectorRuntimeStatus::Reconciling);
                    }
                }
                continue;
            }
            if connector.is_active() {
                let _ = self.restart_unlocked(connector.id).await;
            } else {
                self.statuses
                    .lock()
                    .await
                    .insert(connector.id, ConnectorRuntimeStatus::CredentialRequired);
            }
        }
    }

    async fn cleanup_restored_credentials(&self) {
        let connector_ids = self
            .state
            .read()
            .await
            .credential_cleanup
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        for connector_id in connector_ids {
            if self.credentials.delete(&connector_id).await.is_err() {
                self.statuses
                    .lock()
                    .await
                    .insert(connector_id, ConnectorRuntimeStatus::Reconciling);
                continue;
            }

            let _mutation = self.mutation_lock.lock().await;
            let (intent, persist) = {
                let mut state = self.state.write().await;
                let intent = state.credential_cleanup.remove(&connector_id);
                (intent, state.control_plane_persist_request())
            };
            if persist.save().await.is_err() {
                if let Some(intent) = intent {
                    self.state
                        .write()
                        .await
                        .credential_cleanup
                        .insert(connector_id.clone(), intent);
                }
                self.statuses
                    .lock()
                    .await
                    .insert(connector_id, ConnectorRuntimeStatus::Reconciling);
            } else {
                self.statuses.lock().await.remove(&connector_id);
            }
        }
    }

    pub(super) async fn start_worker(
        &self,
        connector_id: String,
        token: TelegramBotToken,
    ) -> Result<(), ConnectorManagerError> {
        self.stop_worker(&connector_id).await?;
        let (cancel, receiver) = watch::channel(false);
        let owner_close = self
            .owner_close
            .as_ref()
            .expect("only public manager owners start workers")
            .subscribe();
        let (start, started) = oneshot::channel();
        let generation = self.worker_generation.fetch_add(1, Ordering::Relaxed);
        self.install_delivery_authority(&connector_id, &token, generation);
        let manager = self.detached_worker_context();
        let worker_registry = Arc::downgrade(&self.workers);
        let worker_connector_id = connector_id.clone();
        let join = tokio::spawn(async move {
            if started.await.is_ok() {
                manager
                    .worker_loop(worker_connector_id.clone(), token, receiver, owner_close)
                    .await;
                if let Some(worker_registry) = worker_registry.upgrade() {
                    let mut workers = worker_registry.lock().await;
                    if workers
                        .get(&worker_connector_id)
                        .is_some_and(|handle| handle.generation == generation)
                    {
                        workers.remove(&worker_connector_id);
                    }
                }
            }
        });
        self.workers.lock().await.insert(
            connector_id,
            WorkerHandle {
                generation,
                cancel,
                join,
            },
        );
        let _ = start.send(());
        Ok(())
    }

    fn detached_worker_context(&self) -> Self {
        Self {
            state: Arc::clone(&self.state),
            runs: self.runs.clone(),
            credentials: Arc::clone(&self.credentials),
            registry: Arc::clone(&self.registry),
            lifecycle_lock: Arc::new(Mutex::new(())),
            mutation_lock: Arc::clone(&self.mutation_lock),
            statuses: Arc::clone(&self.statuses),
            workers: Arc::new(Mutex::new(HashMap::new())),
            owner_send_locks: Arc::clone(&self.owner_send_locks),
            delivery_claims: Arc::clone(&self.delivery_claims),
            worker_generation: Arc::clone(&self.worker_generation),
            closing: Arc::clone(&self.closing),
            owner_close: None,
        }
    }

    pub(super) async fn stop_worker(
        &self,
        connector_id: &str,
    ) -> Result<(), ConnectorManagerError> {
        let handle = self.workers.lock().await.remove(connector_id);
        let Some(handle) = handle else {
            return Ok(());
        };
        let _ = handle.cancel.send(true);
        handle
            .join
            .await
            .map_err(|_| ConnectorManagerError::WorkerStopped)
    }

    async fn worker_loop(
        &self,
        connector_id: String,
        token: TelegramBotToken,
        mut cancel: watch::Receiver<bool>,
        mut owner_close: watch::Receiver<bool>,
    ) {
        let mut poll_backoff = POLL_RETRY_INITIAL;
        'poll: loop {
            if receiver_stopped(&cancel) || receiver_stopped(&owner_close) {
                return;
            }
            match self
                .expire_pending_pairing_at(&connector_id, now_ms())
                .await
            {
                Ok(_) => {}
                Err(ConnectorManagerError::ConnectorNotFound) => return,
                Err(_) => {
                    if wait_or_stop(&mut cancel, &mut owner_close, Duration::from_millis(100)).await
                    {
                        return;
                    }
                    continue;
                }
            }
            let offset = match self.state.read().await.connectors.get(&connector_id) {
                Some(connector) if connector.is_active() => connector.next_update_id,
                _ => return,
            };
            let adapter = self.registry.telegram(&token);
            let batch = tokio::select! {
                changed = cancel.changed() => {
                    let _ = changed;
                    return;
                }
                changed = owner_close.changed() => {
                    let _ = changed;
                    return;
                }
                result = adapter.receive(ConnectorReceiveRequest { cursor: offset.to_string() }) => result,
            };
            let batch = match batch {
                Ok(batch) => {
                    poll_backoff = POLL_RETRY_INITIAL;
                    batch
                }
                Err(error) if is_revoked_credential(error) => {
                    self.statuses
                        .lock()
                        .await
                        .insert(connector_id, ConnectorRuntimeStatus::CredentialRequired);
                    return;
                }
                Err(error) => {
                    let status = if error == TelegramTransportError::PollConflict {
                        ConnectorRuntimeStatus::PollConflict
                    } else {
                        ConnectorRuntimeStatus::Error
                    };
                    self.statuses
                        .lock()
                        .await
                        .insert(connector_id.clone(), status);
                    if wait_or_stop(
                        &mut cancel,
                        &mut owner_close,
                        poll_retry_delay(error, poll_backoff),
                    )
                    .await
                    {
                        return;
                    }
                    if error != TelegramTransportError::PollConflict {
                        poll_backoff = next_poll_backoff(poll_backoff);
                    }
                    continue;
                }
            };
            let had_updates = !batch.updates.is_empty();
            if had_updates || batch.next_update_id != offset {
                match self.accept_batch(connector_id.clone(), batch).await {
                    Ok(()) => {}
                    Err(ConnectorManagerError::Backpressure) => {
                        loop {
                            match self
                                .deliver_pending_once_with_token(connector_id.clone(), &token)
                                .await
                            {
                                Ok(true) => continue,
                                Ok(false) => break,
                                Err(ConnectorManagerError::Transport) => {
                                    self.statuses.lock().await.insert(
                                        connector_id.clone(),
                                        ConnectorRuntimeStatus::Degraded,
                                    );
                                    if wait_or_stop(
                                        &mut cancel,
                                        &mut owner_close,
                                        Duration::from_millis(100),
                                    )
                                    .await
                                    {
                                        return;
                                    }
                                    continue 'poll;
                                }
                                Err(_) => return,
                            }
                        }
                        match self.process_pending_once(connector_id.clone()).await {
                            Ok(true) | Ok(false) => {}
                            Err(_) => return,
                        }
                        self.statuses
                            .lock()
                            .await
                            .insert(connector_id.clone(), ConnectorRuntimeStatus::Degraded);
                        if wait_or_stop(&mut cancel, &mut owner_close, Duration::from_millis(100))
                            .await
                        {
                            return;
                        }
                        continue;
                    }
                    Err(_) => {
                        self.statuses
                            .lock()
                            .await
                            .insert(connector_id, ConnectorRuntimeStatus::Error);
                        return;
                    }
                }
            }

            self.refresh_operational_status(&connector_id).await;

            let pending_chat = self
                .state
                .read()
                .await
                .connectors
                .get(&connector_id)
                .and_then(|connector| {
                    if connector.approved_chat.is_none() {
                        connector
                            .pending_pairing
                            .as_ref()
                            .map(|pairing| pairing.chat.id.clone())
                    } else {
                        None
                    }
                });
            if had_updates {
                if let Some(chat_id) = pending_chat {
                    let _ = self
                        .registry
                        .telegram(&token)
                        .send(ConnectorSendRequest {
                            destination: chat_id,
                            text: "This chat is waiting for approval in the AnimaOS web console."
                                .to_owned(),
                            // Host-generated correlation for a non-durable notice; no deduplication claim.
                            attempt_id: format!("host:pairing:{offset}"),
                        })
                        .await;
                }
            }

            loop {
                match self.process_pending_once(connector_id.clone()).await {
                    Ok(true) => continue,
                    Ok(false) => break,
                    Err(_) => return,
                }
            }
            loop {
                match self
                    .deliver_pending_once_with_token(connector_id.clone(), &token)
                    .await
                {
                    Ok(true) => continue,
                    Ok(false) => break,
                    Err(ConnectorManagerError::Transport) => break,
                    Err(_) => return,
                }
            }
            if !had_updates
                && wait_or_stop(&mut cancel, &mut owner_close, Duration::from_millis(100)).await
            {
                return;
            }
        }
    }

    pub(crate) async fn status(&self, connector_id: &str) -> Option<ConnectorRuntimeStatus> {
        self.statuses.lock().await.get(connector_id).copied()
    }

    pub(super) async fn refresh_operational_status(&self, connector_id: &str) {
        let status = {
            let state = self.state.read().await;
            state.connectors.get(connector_id).map(|connector| {
                if state.outbound.values().any(|record| {
                    record.connector_id == connector_id
                        && record.delivery_state == OutboundDeliveryState::Failed
                }) {
                    ConnectorRuntimeStatus::Error
                } else {
                    connector_operational_status(connector)
                }
            })
        };
        if let Some(status) = status {
            self.statuses
                .lock()
                .await
                .insert(connector_id.to_string(), status);
        }
    }

    pub(crate) async fn worker_count(&self) -> usize {
        self.workers.lock().await.len()
    }

    pub(crate) async fn request_shutdown(&self) {
        self.closing.store(true, Ordering::SeqCst);
        if let Some(owner_close) = &self.owner_close {
            let _ = owner_close.send(true);
        }
        for handle in self.workers.lock().await.values() {
            let _ = handle.cancel.send(true);
        }
    }

    pub(crate) async fn shutdown(&self) {
        self.request_shutdown().await;
        let _lifecycle = self.lifecycle_lock.lock().await;
        let connector_ids = self
            .workers
            .lock()
            .await
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        for connector_id in connector_ids {
            let _ = self.stop_worker(&connector_id).await;
        }
        drop(_lifecycle);
        self.drain_deliveries().await;
    }
}
