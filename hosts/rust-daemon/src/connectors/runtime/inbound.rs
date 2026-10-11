use super::*;

impl ConnectorManager {
    pub(crate) async fn accept_batch(
        &self,
        connector_id: String,
        batch: TelegramUpdateBatch,
    ) -> Result<(), ConnectorManagerError> {
        let manager = self.clone();
        tokio::spawn(async move { manager.accept_batch_owned(connector_id, batch).await })
            .await
            .map_err(|_| ConnectorManagerError::WorkerStopped)?
    }

    async fn accept_batch_owned(
        &self,
        connector_id: String,
        batch: TelegramUpdateBatch,
    ) -> Result<(), ConnectorManagerError> {
        let _mutation = self.mutation_lock.lock().await;
        let (previous_connector, previous_inbound, persist) = {
            let mut state = self.state.write().await;
            let previous_connector = state
                .connectors
                .get(&connector_id)
                .cloned()
                .ok_or(ConnectorManagerError::ConnectorNotFound)?;
            if !previous_connector.is_active() {
                return Err(ConnectorManagerError::ConnectorNotFound);
            }
            if batch.next_update_id < previous_connector.next_update_id {
                return Err(ConnectorManagerError::ConflictingUpdate);
            }
            let previous_inbound = state
                .inbound
                .iter()
                .filter(|((record_connector_id, _), _)| record_connector_id == &connector_id)
                .map(|(key, record)| (key.clone(), record.clone()))
                .collect::<Vec<_>>();
            let approved_chat_id = previous_connector
                .approved_chat
                .as_ref()
                .map(|chat| chat.id.clone());
            let now = now_ms();

            if let Some(approved_chat_id) = approved_chat_id {
                let mut new_records = Vec::new();
                for update in &batch.updates {
                    if update.chat.id != approved_chat_id {
                        continue;
                    }
                    let key = (connector_id.clone(), update.update_id);
                    let candidate = TelegramInboundRecord {
                        connector_id: connector_id.clone(),
                        update_id: update.update_id,
                        agent_id: previous_connector.agent_id.clone(),
                        room_id: previous_connector.room_id.clone(),
                        normalized_text: update.text.clone(),
                        sender: update.sender.clone(),
                        chat: update.chat.clone(),
                        received_at_ms: now,
                        processing_state: InboundProcessingState::Received,
                        run_idempotency_key: format!(
                            "telegram:{connector_id}:update:{}",
                            update.update_id
                        ),
                    };
                    if let Some(existing) = state.inbound.get(&key) {
                        if !same_inbound_identity(existing, &candidate) {
                            return Err(ConnectorManagerError::ConflictingUpdate);
                        }
                    } else if let Some((_, existing)) = new_records
                        .iter()
                        .find(|(candidate_key, _)| candidate_key == &key)
                    {
                        if !same_inbound_identity(existing, &candidate) {
                            return Err(ConnectorManagerError::ConflictingUpdate);
                        }
                    } else {
                        new_records.push((key, candidate));
                    }
                }
                let live_records = state
                    .inbound
                    .values()
                    .filter(|record| {
                        record.connector_id == connector_id
                            && matches!(
                                record.processing_state,
                                InboundProcessingState::Received
                                    | InboundProcessingState::Processing
                            )
                    })
                    .count();
                if live_records.saturating_add(new_records.len()) > MAX_LIVE_INBOUND {
                    drop(state);
                    self.statuses
                        .lock()
                        .await
                        .insert(connector_id, ConnectorRuntimeStatus::Degraded);
                    return Err(ConnectorManagerError::Backpressure);
                }
                for (key, record) in new_records {
                    state.inbound.insert(key, record);
                }
            } else if let Some(update) = batch.updates.last() {
                let connector = state
                    .connectors
                    .get_mut(&connector_id)
                    .expect("connector was prevalidated");
                connector.pending_pairing = Some(TelegramPendingPairing {
                    chat: update.chat.clone(),
                    requested_at_ms: now,
                });
            }

            let connector = state
                .connectors
                .get_mut(&connector_id)
                .expect("connector was prevalidated");
            connector.next_update_id = connector.next_update_id.max(batch.next_update_id);
            connector.updated_at_ms = now;
            let _ = compact_terminal_inbound(&mut state.inbound, &connector_id);
            let persist = state.control_plane_persist_request();
            (previous_connector, previous_inbound, persist)
        };

        if persist.save().await.is_err() {
            let mut state = self.state.write().await;
            state
                .connectors
                .insert(connector_id.clone(), previous_connector);
            state
                .inbound
                .retain(|(record_connector_id, _), _| record_connector_id != &connector_id);
            state.inbound.extend(previous_inbound);
            drop(state);
            self.statuses
                .lock()
                .await
                .insert(connector_id, ConnectorRuntimeStatus::Error);
            return Err(ConnectorManagerError::Persistence);
        }
        Ok(())
    }

    pub(crate) async fn approve_pending(
        &self,
        connector_id: String,
    ) -> Result<TelegramConnectorRecord, ConnectorManagerError> {
        self.approve_pending_chat(connector_id, None).await
    }

    pub(crate) async fn approve_pending_chat(
        &self,
        connector_id: String,
        expected_chat_id: Option<String>,
    ) -> Result<TelegramConnectorRecord, ConnectorManagerError> {
        let manager = self.clone();
        tokio::spawn(async move {
            manager
                .approve_pending_owned(connector_id, expected_chat_id)
                .await
        })
        .await
        .map_err(|_| ConnectorManagerError::WorkerStopped)?
    }

    async fn approve_pending_owned(
        &self,
        connector_id: String,
        expected_chat_id: Option<String>,
    ) -> Result<TelegramConnectorRecord, ConnectorManagerError> {
        let _mutation = self.mutation_lock.lock().await;
        let stale = {
            let state = self.state.read().await;
            state
                .connectors
                .get(&connector_id)
                .and_then(|connector| connector.pending_pairing.as_ref())
                .is_some_and(|pending| {
                    now_ms().saturating_sub(pending.requested_at_ms) > PAIRING_CANDIDATE_TTL_MS
                })
        };
        if stale {
            let (previous, persist) = {
                let mut state = self.state.write().await;
                let connector = state
                    .connectors
                    .get_mut(&connector_id)
                    .ok_or(ConnectorManagerError::ConnectorNotFound)?;
                let previous = connector.clone();
                connector.pending_pairing = None;
                connector.updated_at_ms = now_ms();
                let persist = state.control_plane_persist_request();
                (previous, persist)
            };
            if persist.save().await.is_err() {
                self.state
                    .write()
                    .await
                    .connectors
                    .insert(connector_id, previous);
                return Err(ConnectorManagerError::Persistence);
            }
            return Err(ConnectorManagerError::PendingPairingNotFound);
        }
        let (previous, updated, persist) = {
            let mut state = self.state.write().await;
            let previous = state
                .connectors
                .get(&connector_id)
                .cloned()
                .ok_or(ConnectorManagerError::ConnectorNotFound)?;
            if !previous.is_active() {
                return Err(ConnectorManagerError::ConnectorNotFound);
            }
            let pending = previous
                .pending_pairing
                .clone()
                .ok_or(ConnectorManagerError::PendingPairingNotFound)?;
            if expected_chat_id
                .as_deref()
                .is_some_and(|expected| expected != pending.chat.id)
            {
                return Err(ConnectorManagerError::PendingPairingNotFound);
            }
            let connector = state
                .connectors
                .get_mut(&connector_id)
                .expect("connector was prevalidated");
            connector.approved_chat = Some(pending.chat);
            connector.pending_pairing = None;
            connector.updated_at_ms = now_ms();
            let updated = connector.clone();
            let persist = state.control_plane_persist_request();
            (previous, updated, persist)
        };
        if persist.save().await.is_err() {
            self.state
                .write()
                .await
                .connectors
                .insert(connector_id.clone(), previous);
            self.statuses
                .lock()
                .await
                .insert(connector_id, ConnectorRuntimeStatus::Error);
            return Err(ConnectorManagerError::Persistence);
        }
        self.statuses
            .lock()
            .await
            .insert(connector_id, ConnectorRuntimeStatus::Ready);
        Ok(updated)
    }
}
