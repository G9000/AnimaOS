use super::*;

// Claims are ephemeral ownership, never a durable delivery guarantee. The mutex
// protects only selection and is released before transport and settlement.
#[derive(Default)]
pub(super) struct DeliveryClaims {
    authorities: HashMap<String, (u64, TelegramBotToken)>,
    active: HashMap<String, u64>,
    sequence: u64,
    idle: Arc<tokio::sync::Notify>,
}

struct DeliveryClaim {
    registry: Arc<StdMutex<DeliveryClaims>>,
    attempt: u64,
    generation: u64,
    connector: TelegramConnectorRecord,
    outbound: TelegramOutboundRecord,
}

impl Drop for DeliveryClaim {
    fn drop(&mut self) {
        let mut claims = self.registry.lock().unwrap_or_else(|e| e.into_inner());
        if claims.active.get(&self.connector.id) == Some(&self.attempt) {
            claims.active.remove(&self.connector.id);
            if claims.active.is_empty() {
                claims.idle.notify_waiters();
            }
        }
    }
}

impl ConnectorManager {
    pub(super) async fn drain_deliveries(&self) {
        let idle = self
            .delivery_claims
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .idle
            .clone();
        loop {
            let notified = idle.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self
                .delivery_claims
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .active
                .is_empty()
            {
                return;
            }
            notified.await;
        }
    }

    pub(super) fn remove_delivery_authority(&self, connector_id: &str) {
        self.delivery_claims
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .authorities
            .remove(connector_id);
    }

    pub(super) fn install_delivery_authority(
        &self,
        connector_id: &str,
        token: &TelegramBotToken,
        generation: u64,
    ) {
        self.delivery_claims
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .authorities
            .insert(connector_id.to_owned(), (generation, token.clone()));
    }

    async fn claim_delivery(
        &self,
        connector_id: &str,
        token: &TelegramBotToken,
        loaded_under_lifecycle: bool,
    ) -> Result<Option<DeliveryClaim>, ConnectorManagerError> {
        let _mutation = self.mutation_lock.lock().await;
        self.ensure_open()?;
        let state = self.state.read().await;
        let connector = state
            .connectors
            .get(connector_id)
            .filter(|record| record.is_active())
            .ok_or(ConnectorManagerError::ConnectorNotFound)?;
        if connector.approved_chat.is_none() {
            return Ok(None);
        }
        let Some(outbound) = state
            .outbound
            .values()
            .filter(|row| row.connector_id == connector_id && row.delivery_state.awaits_delivery())
            .min_by_key(|row| (row.created_at_ms, &row.id))
            .cloned()
        else {
            return Ok(None);
        };
        let mut claims = self
            .delivery_claims
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // A restored manager may receive a manual retry before it starts its
        // polling worker. Only the caller that loaded the vault token under
        // lifecycle authority can initialize this ephemeral committed identity.
        if loaded_under_lifecycle && !claims.authorities.contains_key(connector_id) {
            let generation = self.worker_generation.fetch_add(1, Ordering::Relaxed);
            claims
                .authorities
                .insert(connector_id.to_owned(), (generation, token.clone()));
        }
        let Some((generation, committed_token)) = claims.authorities.get(connector_id) else {
            return Err(ConnectorManagerError::Credential);
        };
        if committed_token != token {
            return Err(ConnectorManagerError::Credential);
        }
        let generation = *generation;
        if claims.active.contains_key(connector_id) {
            return Ok(None);
        }
        claims.sequence = claims.sequence.wrapping_add(1);
        let attempt = claims.sequence;
        claims.active.insert(connector_id.to_owned(), attempt);
        Ok(Some(DeliveryClaim {
            registry: Arc::clone(&self.delivery_claims),
            attempt,
            generation,
            connector: connector.clone(),
            outbound,
        }))
    }

    fn claim_is_current(&self, state: &DaemonState, claim: &DeliveryClaim) -> bool {
        let claims = self
            .delivery_claims
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        claims
            .authorities
            .get(&claim.connector.id)
            .is_some_and(|(generation, _)| *generation == claim.generation)
            && claims.active.get(&claim.connector.id) == Some(&claim.attempt)
            && state
                .connectors
                .get(&claim.connector.id)
                .is_some_and(|current| {
                    current.is_active()
                        && current.agent_id == claim.connector.agent_id
                        && current.room_id == claim.connector.room_id
                        && current.bot == claim.connector.bot
                        && current.created_at_ms == claim.connector.created_at_ms
                        && current.approved_chat == claim.connector.approved_chat
                })
            && state.outbound.get(&claim.outbound.id) == Some(&claim.outbound)
    }

    pub(crate) async fn deliver_pending_once(
        &self,
        connector_id: String,
    ) -> Result<bool, ConnectorManagerError> {
        let manager = self.clone();
        tokio::spawn(async move { manager.deliver_pending_once_owned(connector_id).await })
            .await
            .map_err(|_| ConnectorManagerError::WorkerStopped)?
    }

    async fn deliver_pending_once_owned(
        &self,
        connector_id: String,
    ) -> Result<bool, ConnectorManagerError> {
        let _lifecycle = self.lifecycle_lock.lock().await;
        self.ensure_open()?;
        let token = match self.credentials.load(&connector_id).await {
            Ok(Some(token)) => token,
            Ok(None) => {
                self.statuses
                    .lock()
                    .await
                    .insert(connector_id, ConnectorRuntimeStatus::CredentialRequired);
                return Err(ConnectorManagerError::Credential);
            }
            Err(error) => {
                let mapped = map_credential_error(error);
                self.statuses.lock().await.insert(
                    connector_id,
                    if mapped == ConnectorManagerError::CredentialStateUncertain {
                        ConnectorRuntimeStatus::Reconciling
                    } else {
                        ConnectorRuntimeStatus::Error
                    },
                );
                return Err(mapped);
            }
        };
        let claim = self.claim_delivery(&connector_id, &token, true).await?;
        drop(_lifecycle);
        self.send_delivery_claim(claim, &token).await
    }

    pub(super) async fn deliver_pending_once_with_token(
        &self,
        connector_id: String,
        token: &TelegramBotToken,
    ) -> Result<bool, ConnectorManagerError> {
        let claim = self.claim_delivery(&connector_id, token, false).await?;
        self.send_delivery_claim(claim, token).await
    }

    async fn send_delivery_claim(
        &self,
        claim: Option<DeliveryClaim>,
        token: &TelegramBotToken,
    ) -> Result<bool, ConnectorManagerError> {
        let Some(claim) = claim else {
            return Ok(false);
        };
        // Final committed-identity check immediately before transport. Once the
        // request starts, revocation cannot unsend it: settlement stays fenced.
        {
            let _mutation = self.mutation_lock.lock().await;
            let state = self.state.read().await;
            if !self.claim_is_current(&state, &claim) {
                return Ok(false);
            }
        }
        let connector_id = claim.connector.id.clone();
        let chat_id = &claim
            .connector
            .approved_chat
            .as_ref()
            .expect("claim has chat")
            .id;
        let outbound = &claim.outbound;
        let delivery = self
            .transport
            .send_message(token, chat_id, &outbound.text)
            .await;
        let _mutation = self.mutation_lock.lock().await;
        let (previous_connector_outbound, persist) = {
            let mut state = self.state.write().await;
            if !self.claim_is_current(&state, &claim) {
                if delivery.is_ok() {
                    tracing::warn!(connector_id = %connector_id, outbound_id = %outbound.id, "Telegram send succeeded after delivery authority changed; settlement is uncertain");
                }
                return Ok(false);
            }
            let previous_connector_outbound = state
                .outbound
                .iter()
                .filter(|(_, record)| record.connector_id == connector_id)
                .map(|(id, record)| (id.clone(), record.clone()))
                .collect::<Vec<_>>();
            let delivered_at_ms = now_ms();
            {
                let record = state
                    .outbound
                    .get_mut(&outbound.id)
                    .ok_or(ConnectorManagerError::ConnectorNotFound)?;
                record.attempts = record.attempts.saturating_add(1);
                match &delivery {
                    Ok(_) => {
                        record.delivery_state = OutboundDeliveryState::Delivered;
                        record.delivered_at_ms = Some(delivered_at_ms);
                    }
                    Err(_) => {
                        record.delivery_state = OutboundDeliveryState::Failed;
                        record.delivered_at_ms = None;
                    }
                }
            }
            if delivery.is_ok() {
                compact_delivered_outbox(&mut state.outbound, &connector_id, delivered_at_ms);
            }
            let persist = state.control_plane_persist_request();
            (previous_connector_outbound, persist)
        };
        if persist.save().await.is_err() {
            let mut state = self.state.write().await;
            state
                .outbound
                .retain(|_, record| record.connector_id != connector_id);
            state.outbound.extend(previous_connector_outbound);
            drop(state);
            self.statuses
                .lock()
                .await
                .insert(connector_id, ConnectorRuntimeStatus::Error);
            return Err(ConnectorManagerError::Persistence);
        }
        match delivery {
            Ok(_) => {
                self.refresh_operational_status(&connector_id).await;
                Ok(true)
            }
            Err(error) if is_revoked_credential(error) => {
                self.statuses
                    .lock()
                    .await
                    .insert(connector_id, ConnectorRuntimeStatus::CredentialRequired);
                Err(ConnectorManagerError::Credential)
            }
            Err(_) => {
                self.statuses
                    .lock()
                    .await
                    .insert(connector_id, ConnectorRuntimeStatus::Error);
                Err(ConnectorManagerError::Transport)
            }
        }
    }
}
