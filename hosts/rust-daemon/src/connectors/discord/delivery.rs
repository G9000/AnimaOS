use super::*;
impl DiscordManager {
    pub(crate) async fn deliver_one(&self, id: String) -> Result<bool, DiscordManagerError> {
        let manager = self.clone();
        self.owned(async move {
            let c = manager
                .state
                .read()
                .await
                .discord_connectors
                .get(&id)
                .filter(|c| c.is_active())
                .cloned()
                .ok_or(DiscordManagerError::NotFound)?;
            let token = manager.load_token(&c).await?;
            let epoch = manager.epoch(&id);
            manager.deliver_captured(&c, epoch, &token).await
        })
        .await
    }
    pub(super) async fn deliver_captured(
        &self,
        c: &DiscordConnectorRecord,
        epoch: u64,
        token: &DiscordBotToken,
    ) -> Result<bool, DiscordManagerError> {
        let record = {
            let state = self.state.read().await;
            if !self.authority_current(&c.id, epoch)
                || !state.discord_connectors.get(&c.id).is_some_and(|current| {
                    current.is_active() && current.credential_generation == c.credential_generation
                })
                || state
                    .discord_credential_intents
                    .keys()
                    .any(|(id, _)| id == &c.id)
            {
                return Ok(false);
            }
            state
                .discord_outbound
                .values()
                .filter(|r| r.connector_id == c.id && !r.delivery_state.is_settled())
                .min_by_key(|r| (r.created_at_ms, r.id.clone()))
                .cloned()
        };
        let Some(record) = record else {
            return Ok(false);
        };
        if matches!(
            record.delivery_state,
            DiscordDeliveryState::Unknown | DiscordDeliveryState::Sending
        ) {
            self.set_status(&c.id,DiscordRuntimeStatus::Degraded,Some("Delivery is uncertain. Inspect Discord, then explicitly suppress this output before continuing; Retry cannot resend it.")).await;
            return Ok(false);
        }
        let Some(index) = record.chunks.iter().position(|ch| !ch.state.is_settled()) else {
            return Ok(false);
        };
        let ch = &record.chunks[index];
        if ch.state == DiscordDeliveryState::Failed && ch.retry_at_ms.is_none() {
            return Ok(false);
        }
        if ch.retry_at_ms.is_some_and(|at| at > now_ms()) {
            return Ok(false);
        }
        let Some(claim) = self.claim(&c.id, epoch) else {
            return Ok(false);
        };
        let (sending, persist) = {
            let _txn = self.transaction.lock().await;
            let mut state = self.state.write().await;
            if !self.authority_current(&c.id, claim.epoch)
                || state.discord_outbound.get(&record.id) != Some(&record)
            {
                return Ok(false);
            }
            let mut sending = record.clone();
            sending.delivery_state = DiscordDeliveryState::Sending;
            sending.chunks[index].state = DiscordDeliveryState::Sending;
            sending.chunks[index].attempts = sending.chunks[index].attempts.saturating_add(1);
            sending.chunks[index].retry_at_ms = None;
            state
                .discord_outbound
                .insert(sending.id.clone(), sending.clone());
            let persist = state.control_plane_persist_request();
            drop(state);
            if persist.save().await.is_err() {
                let mut state = self.state.write().await;
                if state.discord_outbound.get(&record.id) == Some(&sending) {
                    state
                        .discord_outbound
                        .insert(record.id.clone(), record.clone());
                }
                return Err(DiscordManagerError::Persistence);
            }
            (sending, ())
        };
        let _ = persist;
        // Effect fence is checked after durable Sending and immediately before dispatch.
        let result = if self.authority_current(&c.id, claim.epoch) {
            let adapter = self.registry.discord(token, &c.channel.id);
            adapter
                .send(ConnectorSendRequest {
                    destination: c.channel.id.clone(),
                    text: sending.chunks[index].text.clone(),
                    attempt_id: sending.chunks[index].attempt_id.clone(),
                })
                .await
        } else {
            Err(DiscordTransportError::Uncertain)
        };
        let _txn = self.transaction.lock().await;
        let (settled, persist) = {
            let mut state = self.state.write().await;
            let current = state
                .discord_outbound
                .get(&record.id)
                .ok_or(DiscordManagerError::Conflict)?;
            // A durable owner stop may change only the unsent tail while this exact attempt is in flight.
            let mut expected = sending.clone();
            if current.stop_requested {
                request_outbound_stop(&mut expected);
            }
            if current != &expected {
                return Err(DiscordManagerError::Conflict);
            }
            let mut settled = current.clone();
            let chunk = &mut settled.chunks[index];
            if !self.authority_current(&c.id, claim.epoch) {
                chunk.state = DiscordDeliveryState::Unknown;
                chunk.error_category = Some("authority_changed".into());
                if let Ok(receipt) = &result {
                    chunk.provider_message_id = Some(receipt.message_id.clone());
                }
                settled.delivery_state = DiscordDeliveryState::Unknown;
            } else {
                match result {
                    Ok(receipt)
                        if receipt.channel_id == c.channel.id
                            && DiscordSnowflake::parse(&receipt.message_id).is_some() =>
                    {
                        chunk.provider_message_id = Some(receipt.message_id);
                        chunk.state = DiscordDeliveryState::Delivered;
                        chunk.error_category = None;
                        settled.delivery_state = if settled.stop_requested {
                            DiscordDeliveryState::Suppressed
                        } else if settled
                            .chunks
                            .iter()
                            .all(|ch| ch.state == DiscordDeliveryState::Delivered)
                        {
                            DiscordDeliveryState::Delivered
                        } else {
                            DiscordDeliveryState::Pending
                        };
                    }
                    Ok(_) => {
                        chunk.state = DiscordDeliveryState::Unknown;
                        chunk.error_category = Some("invalid_receipt".into());
                        settled.delivery_state = DiscordDeliveryState::Unknown;
                    }
                    Err(error) => {
                        chunk.error_category = Some(error.category().into());
                        if let Some(delay) = error.retry_after_ms() {
                            chunk.state = DiscordDeliveryState::Failed;
                            chunk.retry_at_ms = Some(now_ms().saturating_add(delay));
                            settled.delivery_state = DiscordDeliveryState::Failed;
                        } else if error.is_uncertain() {
                            chunk.state = DiscordDeliveryState::Unknown;
                            settled.delivery_state = DiscordDeliveryState::Unknown;
                        } else {
                            chunk.state = DiscordDeliveryState::Failed;
                            settled.delivery_state = DiscordDeliveryState::Failed;
                        }
                    }
                }
            }
            if settled.stop_requested && settled.delivery_state == DiscordDeliveryState::Failed {
                settled.chunks[index].state = DiscordDeliveryState::Suppressed;
                settled.delivery_state = DiscordDeliveryState::Suppressed;
            }
            state
                .discord_outbound
                .insert(settled.id.clone(), settled.clone());
            (settled, state.control_plane_persist_request())
        };
        if persist.save().await.is_err() {
            // Sending on disk is crash-safe evidence: never return it to Pending after an effect.
            let mut state = self.state.write().await;
            if state.discord_outbound.get(&record.id) == Some(&settled) {
                let mut uncertain = settled.clone();
                uncertain.chunks[index].state = DiscordDeliveryState::Unknown;
                uncertain.chunks[index].provider_message_id =
                    settled.chunks[index].provider_message_id.clone();
                uncertain.chunks[index].error_category = Some("receipt_persistence_failed".into());
                uncertain.delivery_state = DiscordDeliveryState::Unknown;
                state.discord_outbound.insert(record.id.clone(), uncertain);
            }
            drop(state);
            self.set_status(&c.id,DiscordRuntimeStatus::Degraded,Some("The send receipt could not be saved; inspect Discord before suppressing uncertain output.")).await;
            return Err(DiscordManagerError::Persistence);
        }
        match settled.delivery_state {DiscordDeliveryState::Unknown=>self.set_status(&c.id,DiscordRuntimeStatus::Degraded,Some("Delivery is uncertain. Inspect Discord, then explicitly suppress this output; it will never be retried automatically.")).await,DiscordDeliveryState::Failed=>self.set_status(&c.id,DiscordRuntimeStatus::Degraded,Some("Discord safely rejected this output; check bot permissions or the saved rate-limit deadline before retrying.")).await,_=>{}}
        Ok(true)
    }
    pub(crate) async fn retry(
        &self,
        agent: String,
        id: String,
    ) -> Result<DiscordConnectorRecord, DiscordManagerError> {
        let manager = self.clone();
        self.owned(async move{let _life=manager.lifecycle.lock().await;let c=manager.get(&agent,&id).await?;let _txn=manager.transaction.lock().await;
        let (changes,persist)={let mut state=manager.state.write().await;if state.discord_outbound.values().any(|r|r.connector_id==id && !r.delivery_state.is_settled() && matches!(r.delivery_state,DiscordDeliveryState::Unknown|DiscordDeliveryState::Sending)){return Err(DiscordManagerError::UnknownDelivery);}let mut changes=Vec::new();for record in state.discord_outbound.values_mut().filter(|r|r.connector_id==id && r.delivery_state.awaits_delivery()){let previous=record.clone();for chunk in &mut record.chunks{if chunk.state==DiscordDeliveryState::Failed{chunk.state=DiscordDeliveryState::Pending;/* keep absolute provider deadline */}}record.delivery_state=DiscordDeliveryState::Pending;changes.push((previous,record.clone()));}(changes,state.control_plane_persist_request())};
        if persist.save().await.is_err(){let mut state=manager.state.write().await;for (previous,next) in changes{if state.discord_outbound.get(&next.id)==Some(&next){state.discord_outbound.insert(previous.id.clone(),previous);}}return Err(DiscordManagerError::Persistence);}manager.set_status(&id,if c.enabled{DiscordRuntimeStatus::Ready}else{DiscordRuntimeStatus::Disabled},None).await;Ok(c)}).await
    }
    pub(crate) async fn suppress_unknown(
        &self,
        agent: String,
        id: String,
    ) -> Result<DiscordConnectorRecord, DiscordManagerError> {
        let manager = self.clone();
        self.owned(async move {
            let _life = manager.lifecycle.lock().await;
            let c = manager.get(&agent, &id).await?;
            let _txn = manager.transaction.lock().await;
            let (previous, next, persist) = {
                let mut state = manager.state.write().await;
                if state.discord_outbound.values().any(|r| {
                    r.connector_id == id && r.delivery_state == DiscordDeliveryState::Sending
                }) {
                    return Err(DiscordManagerError::UnknownDelivery);
                }
                let previous = state
                    .discord_outbound
                    .values()
                    .filter(|r| r.connector_id == id && !r.delivery_state.is_settled())
                    .min_by_key(|r| (r.created_at_ms, r.id.clone()))
                    .cloned()
                    .ok_or(DiscordManagerError::Conflict)?;
                let mut next = previous.clone();
                if !suppress_unknown_record(&mut next) {
                    return Err(DiscordManagerError::Conflict);
                }
                state.discord_outbound.insert(next.id.clone(), next.clone());
                (previous, next, state.control_plane_persist_request())
            };
            if persist.save().await.is_err() {
                let mut state = manager.state.write().await;
                if state.discord_outbound.get(&next.id) == Some(&next) {
                    state.discord_outbound.insert(previous.id.clone(), previous);
                }
                return Err(DiscordManagerError::Persistence);
            }
            manager
                .set_status(
                    &id,
                    if c.enabled {
                        DiscordRuntimeStatus::Ready
                    } else {
                        DiscordRuntimeStatus::Disabled
                    },
                    None,
                )
                .await;
            Ok(c)
        })
        .await
    }
}
