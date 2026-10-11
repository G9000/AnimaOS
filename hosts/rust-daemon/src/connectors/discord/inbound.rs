use super::*;
impl DiscordManager {
    pub(crate) async fn accept_batch(
        &self,
        id: String,
        batch: DiscordMessageBatch,
    ) -> Result<(), DiscordManagerError> {
        let manager = self.clone();
        self.owned(async move {
            let c = manager
                .state
                .read()
                .await
                .discord_connectors
                .get(&id)
                .cloned()
                .ok_or(DiscordManagerError::NotFound)?;
            let epoch = manager.epoch(&id);
            manager.accept_batch_captured(&c, epoch, batch).await
        })
        .await
    }
    pub(super) async fn accept_batch_captured(
        &self,
        c: &DiscordConnectorRecord,
        epoch: u64,
        batch: DiscordMessageBatch,
    ) -> Result<(), DiscordManagerError> {
        if !batch.boundary_proven || batch.messages.len() > 1000 {
            return Err(DiscordManagerError::Conflict);
        }
        let high =
            DiscordSnowflake::cursor(&batch.next_cursor).ok_or(DiscordManagerError::Conflict)?;
        let lower = DiscordSnowflake::cursor(&c.cursor).ok_or(DiscordManagerError::Conflict)?;
        if high < lower {
            return Err(DiscordManagerError::Conflict);
        }
        let mut unique = HashMap::new();
        for message in &batch.messages {
            let id = DiscordSnowflake::parse(&message.id)
                .ok_or(DiscordManagerError::Conflict)?
                .value();
            if message.text.chars().count() > client::MAX_INBOUND_TEXT_CHARS {
                return Err(DiscordManagerError::UnsupportedInput);
            }
            if id > high
                || DiscordSnowflake::parse(&message.channel_id).is_none()
                || message
                    .guild_id
                    .as_ref()
                    .is_some_and(|id| DiscordSnowflake::parse(id).is_none())
                || message
                    .webhook_id
                    .as_ref()
                    .is_some_and(|id| DiscordSnowflake::parse(id).is_none())
                || DiscordSnowflake::parse(&message.sender.id).is_none()
            {
                return Err(DiscordManagerError::Conflict);
            }
            if message.channel_id != c.channel.id
                || message
                    .guild_id
                    .as_ref()
                    .is_some_and(|guild| guild != &c.channel.guild_id)
            {
                return Err(DiscordManagerError::Conflict);
            }
            if let Some(existing) = unique.insert(message.id.clone(), message) {
                if existing != message {
                    return Err(DiscordManagerError::Conflict);
                }
            }
        }
        let _txn = self.transaction.lock().await;
        let (next, inserted, removed, persist, content_missing) = {
            let mut state = self.state.write().await;
            if !self.authority_current(&c.id, epoch)
                || !c.is_active()
                || state.discord_connectors.get(&c.id) != Some(c)
                || state
                    .discord_credential_intents
                    .keys()
                    .any(|(id, _)| id == &c.id)
            {
                return Err(DiscordManagerError::Conflict);
            }
            let mut inserted = Vec::new();
            let mut content_missing = false;
            for message in unique.values() {
                let number = DiscordSnowflake::parse(&message.id)
                    .expect("validated id")
                    .value();
                if number <= lower {
                    continue;
                }
                if message.is_bot
                    || message.sender.id == c.bot.id
                    || message.webhook_id.is_some()
                    || !c.approved_user_ids.contains(&message.sender.id)
                    || !matches!(message.message_type, 0 | 19)
                {
                    continue;
                }
                if message.text.trim().is_empty() {
                    content_missing = true;
                    continue;
                }
                let key = (c.id.clone(), message.id.clone());
                let record = DiscordInboundRecord {
                    connector_id: c.id.clone(),
                    message_id: message.id.clone(),
                    agent_id: c.agent_id.clone(),
                    room_id: c.room_id.clone(),
                    sender: message.sender.clone(),
                    channel_id: c.channel.id.clone(),
                    text: message.text.clone(),
                    received_at_ms: now_ms(),
                    processing_state: DiscordProcessingState::Received,
                    run_idempotency_key: format!("discord:{}:message:{}", c.id, message.id),
                    run_id: None,
                };
                if let Some(existing) = state.discord_inbound.get(&key) {
                    if !existing.processing_state.is_terminal()
                        && (existing.text != record.text
                            || existing.sender != record.sender
                            || existing.channel_id != record.channel_id)
                    {
                        return Err(DiscordManagerError::Conflict);
                    }
                } else {
                    inserted.push((key, record));
                }
            }
            let live = state
                .discord_inbound
                .values()
                .filter(|r| r.connector_id == c.id && !r.processing_state.is_terminal())
                .count();
            if live + inserted.len() > MAX_LIVE_INBOUND {
                return Err(DiscordManagerError::Backpressure);
            }
            let mut next = c.clone();
            next.cursor = batch.next_cursor;
            next.updated_at_ms = now_ms();
            for (key, record) in &inserted {
                state.discord_inbound.insert(key.clone(), record.clone());
            }
            state.discord_connectors.insert(c.id.clone(), next.clone());
            let mut terminals = state
                .discord_inbound
                .iter()
                .filter(|(_, r)| r.connector_id == c.id && r.processing_state.is_terminal())
                .map(|(key, r)| (key.clone(), r.received_at_ms))
                .collect::<Vec<_>>();
            terminals.sort_by_key(|(_, at)| *at);
            let overflow = terminals.len().saturating_sub(MAX_TERMINAL_INBOUND);
            let mut removed = Vec::new();
            for (key, _) in terminals.into_iter().take(overflow) {
                if let Some(r) = state.discord_inbound.remove(&key) {
                    removed.push((key, r));
                }
            }
            (
                next,
                inserted,
                removed,
                state.control_plane_persist_request(),
                content_missing,
            )
        };
        if persist.save().await.is_err() {
            let mut state = self.state.write().await;
            if state.discord_connectors.get(&c.id) == Some(&next) {
                state.discord_connectors.insert(c.id.clone(), c.clone());
            }
            for (key, r) in inserted {
                if state.discord_inbound.get(&key) == Some(&r) {
                    state.discord_inbound.remove(&key);
                }
            }
            for (key, r) in removed {
                state.discord_inbound.entry(key).or_insert(r);
            }
            return Err(DiscordManagerError::Persistence);
        }
        if content_missing {
            self.set_status(&c.id,DiscordRuntimeStatus::Degraded,Some("Allowed-user message content was empty or unavailable; check Message Content intent. Attachments-only messages are unsupported.")).await;
        }
        Ok(())
    }
}
