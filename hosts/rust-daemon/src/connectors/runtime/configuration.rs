use super::*;

impl ConnectorManager {
    pub(crate) async fn create(
        &self,
        agent_id: String,
        token: TelegramBotToken,
    ) -> Result<TelegramConnectorRecord, ConnectorManagerError> {
        let manager = self.clone();
        tokio::spawn(async move { manager.create_owned(agent_id, token).await })
            .await
            .map_err(|_| ConnectorManagerError::WorkerStopped)?
    }

    async fn create_owned(
        &self,
        agent_id: String,
        token: TelegramBotToken,
    ) -> Result<TelegramConnectorRecord, ConnectorManagerError> {
        let _lifecycle = self.lifecycle_lock.lock().await;
        self.ensure_open()?;
        {
            let state = self.state.read().await;
            if state.get_agent(&agent_id).is_none() {
                return Err(ConnectorManagerError::AgentNotFound);
            }
            if state.connectors.values().any(|connector| {
                connector.agent_id == agent_id && connector.deleted_at_ms.is_none()
            }) {
                return Err(ConnectorManagerError::AgentAlreadyConnected);
            }
        }

        let bot = self
            .registry
            .telegram(&token)
            .identify()
            .await
            .map_err(map_token_validation_error)?;
        let now = now_ms();
        let id = next_connector_id(now);
        let connector = TelegramConnectorRecord {
            id: id.clone(),
            agent_id,
            room_id: format!("telegram:{id}"),
            bot,
            approved_chat: None,
            pending_pairing: None,
            next_update_id: 0,
            enabled: true,
            deleted_at_ms: None,
            created_at_ms: now,
            updated_at_ms: now,
        };

        let _mutation = self.mutation_lock.lock().await;
        let intent = TelegramCredentialCleanupIntent {
            connector_id: id.clone(),
            created_at_ms: now,
        };
        let intent_persist = {
            let mut state = self.state.write().await;
            if state.get_agent(&connector.agent_id).is_none() {
                return Err(ConnectorManagerError::AgentNotFound);
            }
            if state.connectors.values().any(|candidate| {
                candidate.agent_id == connector.agent_id && candidate.deleted_at_ms.is_none()
            }) {
                return Err(ConnectorManagerError::AgentAlreadyConnected);
            }
            state.credential_cleanup.insert(id.clone(), intent.clone());
            state.control_plane_persist_request()
        };
        if intent_persist.save().await.is_err() {
            self.state.write().await.credential_cleanup.remove(&id);
            return Err(ConnectorManagerError::Persistence);
        }
        drop(_mutation);

        let worker_token = token.clone();
        if let Err(error) = self.credentials.put(&id, token).await {
            let mapped = map_credential_error(error);
            return self.cleanup_failed_create_vault(&id, mapped).await;
        }

        let _mutation = self.mutation_lock.lock().await;
        let persist = {
            let mut state = self.state.write().await;
            if state.get_agent(&connector.agent_id).is_none() {
                drop(state);
                drop(_mutation);
                return self
                    .cleanup_failed_create_vault(&id, ConnectorManagerError::AgentNotFound)
                    .await;
            }
            if state.connectors.values().any(|candidate| {
                candidate.agent_id == connector.agent_id && candidate.deleted_at_ms.is_none()
            }) {
                drop(state);
                drop(_mutation);
                return self
                    .cleanup_failed_create_vault(&id, ConnectorManagerError::AgentAlreadyConnected)
                    .await;
            }
            state.connectors.insert(id.clone(), connector.clone());
            state.credential_cleanup.remove(&id);
            state.control_plane_persist_request()
        };
        if persist.save().await.is_err() {
            let mut state = self.state.write().await;
            state.connectors.remove(&id);
            state.credential_cleanup.insert(id.clone(), intent);
            drop(state);
            drop(_mutation);
            return self
                .cleanup_failed_create_vault(&id, ConnectorManagerError::Persistence)
                .await;
        }
        self.statuses
            .lock()
            .await
            .insert(id.clone(), ConnectorRuntimeStatus::Pairing);
        drop(_mutation);
        self.start_worker(id, worker_token).await?;
        Ok(connector)
    }

    async fn cleanup_failed_create_vault(
        &self,
        connector_id: &str,
        original_error: ConnectorManagerError,
    ) -> Result<TelegramConnectorRecord, ConnectorManagerError> {
        if self.credentials.delete(connector_id).await.is_err() {
            self.statuses.lock().await.insert(
                connector_id.to_string(),
                ConnectorRuntimeStatus::Reconciling,
            );
            return Err(ConnectorManagerError::CredentialStateUncertain);
        }
        let _mutation = self.mutation_lock.lock().await;
        let (intent, persist) = {
            let mut state = self.state.write().await;
            let intent = state.credential_cleanup.remove(connector_id);
            (intent, state.control_plane_persist_request())
        };
        if persist.save().await.is_err() {
            if let Some(intent) = intent {
                self.state
                    .write()
                    .await
                    .credential_cleanup
                    .insert(connector_id.to_string(), intent);
            }
            self.statuses.lock().await.insert(
                connector_id.to_string(),
                ConnectorRuntimeStatus::Reconciling,
            );
            return Err(ConnectorManagerError::Persistence);
        }
        self.statuses.lock().await.remove(connector_id);
        Err(original_error)
    }

    pub(crate) async fn replace_token(
        &self,
        connector_id: String,
        token: TelegramBotToken,
    ) -> Result<TelegramConnectorRecord, ConnectorManagerError> {
        let manager = self.clone();
        tokio::spawn(async move { manager.replace_token_owned(connector_id, token).await })
            .await
            .map_err(|_| ConnectorManagerError::WorkerStopped)?
    }

    async fn replace_token_owned(
        &self,
        connector_id: String,
        token: TelegramBotToken,
    ) -> Result<TelegramConnectorRecord, ConnectorManagerError> {
        let _lifecycle = self.lifecycle_lock.lock().await;
        self.ensure_open()?;
        let preflight_connector = {
            let state = self.state.read().await;
            state
                .connectors
                .get(&connector_id)
                .filter(|connector| connector.deleted_at_ms.is_none())
                .cloned()
                .ok_or(ConnectorManagerError::ConnectorNotFound)?
        };
        let previous_status = self
            .statuses
            .lock()
            .await
            .get(&connector_id)
            .copied()
            .unwrap_or_else(|| connector_operational_status(&preflight_connector));
        let bot = self
            .registry
            .telegram(&token)
            .identify()
            .await
            .map_err(map_token_validation_error)?;
        let previous_token = match self.credentials.load(&connector_id).await {
            Ok(token) => token,
            Err(error) => {
                let mapped = map_credential_error(error);
                if mapped == ConnectorManagerError::CredentialStateUncertain {
                    self.stop_worker(&connector_id).await?;
                    self.statuses
                        .lock()
                        .await
                        .insert(connector_id, ConnectorRuntimeStatus::Reconciling);
                }
                return Err(mapped);
            }
        };
        let worker_token = token.clone();
        if let Err(error) = self.credentials.put(&connector_id, token).await {
            let mapped = map_credential_error(error);
            if mapped == ConnectorManagerError::CredentialStateUncertain {
                self.stop_worker(&connector_id).await?;
                self.statuses
                    .lock()
                    .await
                    .insert(connector_id, ConnectorRuntimeStatus::Reconciling);
            }
            return Err(mapped);
        }

        if let Err(error) = self.stop_worker(&connector_id).await {
            return match self
                .restore_previous_worker(connector_id, previous_token, previous_status)
                .await
            {
                Ok(()) => Err(error),
                Err(compensation_error) => Err(compensation_error),
            };
        }
        let _mutation = self.mutation_lock.lock().await;

        let transaction = {
            let mut state = self.state.write().await;
            state
                .connectors
                .get(&connector_id)
                .cloned()
                .map(|previous| {
                    let connector = state
                        .connectors
                        .get_mut(&connector_id)
                        .expect("connector was just found");
                    connector.bot = bot;
                    connector.enabled = true;
                    connector.updated_at_ms = now_ms();
                    let updated = connector.clone();
                    let persist = state.control_plane_persist_request();
                    (previous, updated, persist)
                })
        };
        let Some((previous, updated, persist)) = transaction else {
            let rollback = {
                let mut state = self.state.write().await;
                state
                    .connectors
                    .insert(connector_id.clone(), preflight_connector);
                state.control_plane_persist_request()
            };
            if rollback.save().await.is_err() {
                drop(_mutation);
                let _ = self
                    .restore_previous_worker(connector_id.clone(), previous_token, previous_status)
                    .await;
                self.statuses
                    .lock()
                    .await
                    .insert(connector_id, ConnectorRuntimeStatus::Reconciling);
                return Err(ConnectorManagerError::Persistence);
            }
            drop(_mutation);
            self.restore_previous_worker(connector_id, previous_token, previous_status)
                .await?;
            return Err(ConnectorManagerError::ConnectorNotFound);
        };
        if persist.save().await.is_err() {
            self.state
                .write()
                .await
                .connectors
                .insert(connector_id.clone(), previous.clone());
            drop(_mutation);
            self.restore_previous_worker(connector_id, previous_token, previous_status)
                .await?;
            return Err(ConnectorManagerError::Persistence);
        }
        // Publish credential identity before releasing the commit fence, even
        // when replacement keeps the same bot identity and timestamp.
        let generation = self.worker_generation.fetch_add(1, Ordering::Relaxed);
        self.install_delivery_authority(&connector_id, &worker_token, generation);
        drop(_mutation);
        if let Err(error) = self.start_worker(connector_id.clone(), worker_token).await {
            let _mutation = self.mutation_lock.lock().await;
            let rollback = {
                let mut state = self.state.write().await;
                state
                    .connectors
                    .insert(connector_id.clone(), previous.clone());
                state.control_plane_persist_request()
            };
            if rollback.save().await.is_err() {
                drop(_mutation);
                let _ = self
                    .restore_previous_worker(connector_id.clone(), previous_token, previous_status)
                    .await;
                self.statuses
                    .lock()
                    .await
                    .insert(connector_id, ConnectorRuntimeStatus::Reconciling);
                return Err(ConnectorManagerError::Persistence);
            }
            drop(_mutation);
            return match self
                .restore_previous_worker(connector_id, previous_token, previous_status)
                .await
            {
                Ok(()) => Err(error),
                Err(compensation_error) => Err(compensation_error),
            };
        }
        self.statuses
            .lock()
            .await
            .insert(connector_id, connector_operational_status(&updated));
        Ok(updated)
    }

    async fn restore_previous_worker(
        &self,
        connector_id: String,
        previous_token: Option<TelegramBotToken>,
        previous_status: ConnectorRuntimeStatus,
    ) -> Result<(), ConnectorManagerError> {
        let Some(previous_token) = previous_token else {
            if self.credentials.delete(&connector_id).await.is_err() {
                self.statuses
                    .lock()
                    .await
                    .insert(connector_id, ConnectorRuntimeStatus::Reconciling);
                return Err(ConnectorManagerError::CredentialStateUncertain);
            }
            self.statuses
                .lock()
                .await
                .insert(connector_id, ConnectorRuntimeStatus::CredentialRequired);
            return Ok(());
        };
        if self
            .credentials
            .put(&connector_id, previous_token.clone())
            .await
            .is_err()
        {
            self.statuses
                .lock()
                .await
                .insert(connector_id, ConnectorRuntimeStatus::Reconciling);
            return Err(ConnectorManagerError::CredentialStateUncertain);
        }
        if self
            .start_worker(connector_id.clone(), previous_token)
            .await
            .is_err()
        {
            self.statuses
                .lock()
                .await
                .insert(connector_id, ConnectorRuntimeStatus::Reconciling);
            return Err(ConnectorManagerError::WorkerStopped);
        }
        self.statuses
            .lock()
            .await
            .insert(connector_id, previous_status);
        Ok(())
    }
}
