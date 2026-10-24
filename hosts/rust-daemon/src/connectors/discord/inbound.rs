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
            let destination = batch
                .messages
                .first()
                .and_then(|m| c.destination_for_channel(&m.channel_id))
                .unwrap_or(c);
            manager
                .accept_batch_captured(&destination, epoch, batch)
                .await
        })
        .await
    }
    pub(super) async fn accept_batch_captured(
        &self,
        c: &DiscordConnectorRecord,
        epoch: u64,
        batch: DiscordMessageBatch,
    ) -> Result<(), DiscordManagerError> {
        let _intake = self.intake.lock().await;
        if !batch.boundary_proven || batch.messages.len() > 1000 {
            return Err(DiscordManagerError::Conflict);
        }
        let high =
            DiscordSnowflake::cursor(&batch.next_cursor).ok_or(DiscordManagerError::Conflict)?;
        let lower = DiscordSnowflake::cursor(&c.cursor).ok_or(DiscordManagerError::Conflict)?;
        if high < lower {
            return Err(DiscordManagerError::Conflict);
        }
        let mut unique = std::collections::BTreeMap::new();
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
            if let Some(existing) = unique.insert(id, message) {
                if existing != message {
                    return Err(DiscordManagerError::Conflict);
                }
            }
        }
        let mut routed = HashMap::new();
        let mut uploads = HashMap::new();
        for message in unique.values() {
            if DiscordSnowflake::parse(&message.id)
                .expect("validated")
                .value()
                <= lower
            {
                continue;
            }
            let current = self.get(&c.agent_id, &c.id).await?;
            if !current.matches_destination(c) {
                return Err(DiscordManagerError::Conflict);
            }
            let fresh = current
                .destination_for_room(&c.room_id)
                .ok_or(DiscordManagerError::Conflict)?;
            if let Some(destination) = self.admitted_destination(&fresh, epoch, message).await? {
                uploads.insert(
                    message.id.clone(),
                    self.receive_attachments(&destination, message).await?,
                );
                routed.insert(message.id.clone(), destination);
            }
        }
        let _txn = self.transaction.lock().await;
        let (previous, next, inserted, removed, sessions, persist, content_missing) = {
            let mut state = self.state.write().await;
            if !self.authority_current(&c.id, epoch)
                || !c.is_active()
                || !state.discord_connectors.get(&c.id).is_some_and(|current| {
                    current.matches_destination(c)
                        && current
                            .destination_for_room(&c.room_id)
                            .is_some_and(|v| v.cursor == c.cursor)
                })
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
                if message.text.trim().is_empty() && message.attachments.is_empty() {
                    content_missing |= c.approved_user_ids.contains(&message.sender.id)
                        && !message.is_bot
                        && message.webhook_id.is_none();
                    continue;
                }
                let Some(destination) = routed.get(&message.id) else {
                    continue;
                };
                let key = (c.id.clone(), message.id.clone());
                let (attachment_ids, input_text) =
                    uploads.get(&message.id).expect("admitted input saved");
                let record = DiscordInboundRecord {
                    connector_id: c.id.clone(),
                    message_id: message.id.clone(),
                    agent_id: c.agent_id.clone(),
                    room_id: destination.room_id.clone(),
                    sender: message.sender.clone(),
                    channel_id: c.channel.id.clone(),
                    text: input_text.clone(),
                    attachment_ids: attachment_ids.clone(),
                    received_at_ms: now_ms(),
                    processing_state: DiscordProcessingState::Received,
                    run_idempotency_key: format!("discord:{}:message:{}", c.id, message.id),
                    run_id: None,
                };
                if let Some(existing) = state.discord_inbound.get(&key) {
                    if !existing.processing_state.is_terminal()
                        && (existing.text != record.text
                            || existing.attachment_ids != record.attachment_ids
                            || existing.sender != record.sender
                            || existing.channel_id != record.channel_id
                            || existing.room_id != record.room_id)
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
            let previous = state
                .discord_connectors
                .get(&c.id)
                .expect("validated connector")
                .clone();
            let mut next = previous.clone();
            if !next.bot_wide && next.room_id == c.room_id {
                next.cursor = batch.next_cursor;
            } else {
                let conversation = next
                    .conversations
                    .iter_mut()
                    .find(|v| v.room_id == c.room_id)
                    .ok_or(DiscordManagerError::Conflict)?;
                conversation.cursor = batch.next_cursor;
            }
            // A claim is only needed until the source cursor is durable. Remove
            // it atomically with that cursor, so failed saves preserve no-retry evidence.
            next.thread_starts.retain(|id, claim| {
                claim.channel_id != c.channel.id
                    || DiscordSnowflake::parse(id).is_none_or(|id| id.value() > high)
            });
            next.updated_at_ms = now_ms();
            state.discord_connectors.insert(c.id.clone(), next.clone());
            let mut sessions = Vec::new();
            for (key, record) in &inserted {
                state.discord_inbound.insert(key.clone(), record.clone());
                if state.ensure_run_session(crate::state::RunSessionRequest {
                    agent_id: &record.agent_id,
                    room_id: &record.room_id,
                    source: crate::runs::RunSource::Discord,
                    system_followup_title: None,
                    delegated_parent: None,
                    peer_sender: None,
                    parent: None,
                    first_text: &record.text,
                    now_ms: now_ms(),
                }) {
                    if let Some(session) = state.sessions.get(&record.agent_id, &record.room_id) {
                        sessions.push(session.clone());
                    }
                }
            }
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
                previous,
                next,
                inserted,
                removed,
                sessions,
                state.control_plane_persist_request(),
                content_missing,
            )
        };
        if persist.save().await.is_err() {
            let mut state = self.state.write().await;
            if state.discord_connectors.get(&c.id) == Some(&next) {
                state.discord_connectors.insert(c.id.clone(), previous);
            }
            for (key, r) in inserted {
                if state.discord_inbound.get(&key) == Some(&r) {
                    state.discord_inbound.remove(&key);
                }
            }
            for (key, r) in removed {
                state.discord_inbound.entry(key).or_insert(r);
            }
            for session in sessions {
                if state.sessions.get(&session.agent_id, &session.id) == Some(&session) {
                    state.sessions.remove(&session.agent_id, &session.id);
                }
            }
            return Err(DiscordManagerError::Persistence);
        }
        if content_missing {
            self.set_status(&c.id,DiscordRuntimeStatus::Degraded,Some("Allowed-user message content was empty or unavailable; check Message Content intent.")).await;
        }
        Ok(())
    }
}
