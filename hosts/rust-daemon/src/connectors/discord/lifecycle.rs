use super::*;
impl DiscordManager {
    pub(crate) async fn create(
        &self,
        agent: String,
        token: DiscordBotToken,
        guild: String,
        channel: String,
        users: Vec<String>,
    ) -> Result<DiscordConnectorRecord, DiscordManagerError> {
        let manager = self.clone();
        self.owned(async move {
            manager
                .create_owned(agent, token, guild, channel, users)
                .await
        })
        .await
    }
    async fn create_owned(
        &self,
        agent: String,
        token: DiscordBotToken,
        guild: String,
        channel: String,
        users: Vec<String>,
    ) -> Result<DiscordConnectorRecord, DiscordManagerError> {
        self.create_connection(agent, token, Some(guild), Some(channel), users, None, None)
            .await
    }
    pub(crate) async fn check_bot(
        &self,
        token: DiscordBotToken,
    ) -> Result<DiscordBotCheck, DiscordManagerError> {
        self.ensure_open()?;
        self.native()?
            .check_application(&token)
            .await
            .map_err(|e| setup_error("check_application", e))
    }
    pub(crate) async fn create_with_settings(
        &self,
        agent: String,
        token: DiscordBotToken,
        guild: Option<String>,
        channel: Option<String>,
        users: Vec<String>,
        settings: DiscordSettings,
    ) -> Result<DiscordConnectorRecord, DiscordManagerError> {
        let manager = self.clone();
        self.owned(async move {
            if !valid_settings(&settings) || !valid_ids(&users, 100) {
                return Err(DiscordManagerError::InvalidInput);
            }
            let check = manager.check_bot(token.clone()).await?;
            let users = if users.is_empty() {
                check.owner_user_ids.clone()
            } else {
                users
            };
            if !valid_ids(&users, 100)
                || (users.is_empty() && settings.approved_role_ids.is_empty())
            {
                return Err(DiscordManagerError::InvalidInput);
            }
            manager
                .create_connection(
                    agent,
                    token,
                    guild,
                    channel,
                    users,
                    Some(settings),
                    Some(check),
                )
                .await
        })
        .await
    }
    async fn create_connection(
        &self,
        agent: String,
        token: DiscordBotToken,
        guild: Option<String>,
        channel: Option<String>,
        users: Vec<String>,
        settings: Option<DiscordSettings>,
        check: Option<DiscordBotCheck>,
    ) -> Result<DiscordConnectorRecord, DiscordManagerError> {
        let _lifecycle = self.lifecycle.lock().await;
        self.ensure_open()?;
        if guild
            .as_ref()
            .is_some_and(|id| DiscordSnowflake::parse(id).is_none())
            || channel
                .as_ref()
                .is_some_and(|id| DiscordSnowflake::parse(id).is_none())
            || (settings.is_none() && !valid_users(&users))
        {
            return Err(DiscordManagerError::InvalidInput);
        }
        {
            let state = self.state.read().await;
            if state.get_agent(&agent).is_none() {
                return Err(DiscordManagerError::NotFound);
            }
            if state.discord_connectors.values().any(|c| {
                c.agent_id == agent
                    && channel.as_ref().is_some_and(|id| &c.channel.id == id)
                    && c.deleted_at_ms.is_none()
            }) {
                return Err(DiscordManagerError::DuplicateChannel);
            }
        }
        let bot = if let Some(check) = &check {
            check.bot.clone()
        } else {
            self.native()?
                .get_me(&token)
                .await
                .map_err(|e| setup_error("identify_bot", e))?
        };
        if self
            .state
            .read()
            .await
            .discord_connectors
            .values()
            .any(|c| {
                c.agent_id == agent
                    && c.bot.id == bot.id
                    && c.deleted_at_ms.is_none()
                    && (channel.is_none() || c.bot_wide)
            })
        {
            return Err(DiscordManagerError::DuplicateChannel);
        }
        let metadata = if let Some(channel) = &channel {
            let metadata = self
                .native()?
                .get_channel(&token, channel)
                .await
                .map_err(|e| setup_error("get_channel", e))?;
            if metadata.id != *channel || guild.as_ref().is_some_and(|g| g != &metadata.guild_id) {
                return Err(DiscordManagerError::Conflict);
            }
            self.native()?
                .latest_cursor(&token, channel)
                .await
                .map_err(|e| setup_error("read_channel_history", e))?;
            metadata
        } else {
            DiscordChannelMetadata {
                id: String::new(),
                guild_id: guild.clone().unwrap_or_default(),
                name: None,
                kind: DiscordChannelKind::GuildText,
                parent_channel_id: None,
            }
        };
        let id = format!("discord-{}", uuid::Uuid::new_v4());
        let now = now_ms();
        let mut record = DiscordConnectorRecord {
            id: id.clone(),
            agent_id: agent.clone(),
            room_id: format!("discord:{id}"),
            bot,
            channel: metadata,
            bot_wide: channel.is_none(),
            guild_scope: guild,
            application_id: check.map(|c| c.application_id),
            settings,
            configuration_revision: 1,
            conversations: vec![],
            thread_starts: Default::default(),
            approved_user_ids: users,
            credential_generation: 1,
            cursor: "0".into(),
            activated_at_ms: None,
            enabled: false,
            deleted_at_ms: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        self.validate_destinations(&mut record, &token).await?;
        let intent = DiscordCredentialIntent {
            connector_id: id.clone(),
            generation: 1,
            operation: DiscordCredentialOperation::Create,
            created_at_ms: now,
        };
        {
            let _txn = self.transaction.lock().await;
            let persist = {
                let mut state = self.state.write().await;
                if state.get_agent(&agent).is_none() {
                    return Err(DiscordManagerError::NotFound);
                }
                state.discord_connectors.insert(id.clone(), record.clone());
                state
                    .discord_credential_intents
                    .insert((id.clone(), 1), intent.clone());
                state.control_plane_persist_request()
            };
            if persist.save().await.is_err() {
                let mut state = self.state.write().await;
                if state.discord_connectors.get(&id) == Some(&record) {
                    state.discord_connectors.remove(&id);
                }
                if state.discord_credential_intents.get(&(id.clone(), 1)) == Some(&intent) {
                    state.discord_credential_intents.remove(&(id.clone(), 1));
                }
                return Err(DiscordManagerError::Persistence);
            }
        }
        self.credentials
            .put(&id, 1, &record.bot.id, &token)
            .await
            .map_err(|_| DiscordManagerError::Credential)?;
        let saved = self.load_token(&record).await?;
        if saved.expose() != token.expose() {
            return Err(DiscordManagerError::Credential);
        }
        self.finish_intent(&record, &intent, false).await?;
        Ok(record)
    }
    async fn finish_intent(
        &self,
        record: &DiscordConnectorRecord,
        intent: &DiscordCredentialIntent,
        enabled: bool,
    ) -> Result<(), DiscordManagerError> {
        let _txn = self.transaction.lock().await;
        let (previous, next, schedules, persist) = {
            let mut state = self.state.write().await;
            let previous = state
                .discord_connectors
                .get(&record.id)
                .filter(|c| c.credential_generation == record.credential_generation)
                .cloned()
                .ok_or(DiscordManagerError::Conflict)?;
            let mut next = previous.clone();
            next.enabled = enabled;
            next.updated_at_ms = now_ms();
            state
                .discord_connectors
                .insert(next.id.clone(), next.clone());
            state
                .discord_credential_intents
                .remove(&(record.id.clone(), intent.generation));
            let mut schedules = Vec::new();
            if !enabled {
                for schedule in state.schedules.values_mut().filter(|s|matches!(&s.target,crate::schedules::ScheduleTarget::Connector{connector_id} if connector_id==&record.id)) {
                    let before=schedule.clone();schedule.enabled=false;schedule.updated_at_ms=now_ms();schedules.push((before,schedule.clone()));
                }
            }
            if !record.bot_wide {
                state.ensure_run_session(crate::state::RunSessionRequest {
                    agent_id: &record.agent_id,
                    room_id: &record.room_id,
                    source: crate::runs::RunSource::Discord,
                    system_followup_title: None,
                    delegated_parent: None,
                    peer_sender: None,
                    parent: None,
                    first_text: "",
                    now_ms: now_ms(),
                });
            }
            (
                previous,
                next,
                schedules,
                state.control_plane_persist_request(),
            )
        };
        if persist.save().await.is_err() {
            let mut state = self.state.write().await;
            if state.discord_connectors.get(&record.id) == Some(&next) {
                state.discord_connectors.insert(record.id.clone(), previous);
            }
            state
                .discord_credential_intents
                .entry((record.id.clone(), intent.generation))
                .or_insert_with(|| intent.clone());
            for (before, after) in schedules {
                if state.schedules.get(&after.id) == Some(&after) {
                    state.schedules.insert(before.id.clone(), before);
                }
            }
            return Err(DiscordManagerError::Persistence);
        }
        self.set_status(
            &record.id,
            if enabled {
                DiscordRuntimeStatus::Ready
            } else {
                DiscordRuntimeStatus::Disabled
            },
            None,
        )
        .await;
        Ok(())
    }
    pub(crate) async fn activate(
        &self,
        agent: String,
        id: String,
    ) -> Result<DiscordConnectorRecord, DiscordManagerError> {
        let manager = self.clone();
        self.owned(async move {
            let _life = manager.lifecycle.lock().await;
            manager.activate_owned(&agent, &id).await
        })
        .await
    }
    async fn activate_owned(
        &self,
        agent: &str,
        id: &str,
    ) -> Result<DiscordConnectorRecord, DiscordManagerError> {
        self.ensure_open()?;
        let previous = self.get(agent, id).await?;
        if self
            .state
            .read()
            .await
            .discord_credential_intents
            .keys()
            .any(|(c, _)| c == id)
        {
            return Err(DiscordManagerError::Credential);
        }
        let token = self.load_token(&previous).await?;
        let bot = self
            .registry
            .discord(&token, &previous.channel.id)
            .identify()
            .await
            .map_err(transport_error)?;
        if bot.id != previous.bot.id {
            return Err(DiscordManagerError::Conflict);
        }
        let mut next = previous.clone();
        if next.activated_at_ms.is_none() {
            if !next.bot_wide {
                let channel = self
                    .native()?
                    .get_channel(&token, &next.channel.id)
                    .await
                    .map_err(transport_error)?;
                if !channel.routing_eq(&next.channel) {
                    return Err(DiscordManagerError::Conflict);
                }
                next.channel.name = channel.name;
                next.cursor = self
                    .native()?
                    .latest_cursor(&token, &next.channel.id)
                    .await
                    .map_err(transport_error)?;
                if DiscordSnowflake::cursor(&next.cursor).is_none() {
                    return Err(DiscordManagerError::Conflict);
                }
            }
            for conversation in &mut next.conversations {
                conversation.cursor = self
                    .native()?
                    .latest_cursor(&token, &conversation.channel.id)
                    .await
                    .map_err(transport_error)?;
                if DiscordSnowflake::cursor(&conversation.cursor).is_none() {
                    return Err(DiscordManagerError::Conflict);
                }
            }
            next.activated_at_ms = Some(now_ms());
        }
        next.configuration_revision = next
            .configuration_revision
            .checked_add(1)
            .ok_or(DiscordManagerError::Conflict)?;
        next.enabled = true;
        next.updated_at_ms = now_ms();
        self.replace_record(&previous, &next).await?;
        self.start_worker(next.clone(), token).await;
        self.set_status(id, DiscordRuntimeStatus::Ready, None).await;
        Ok(next)
    }
    pub(super) async fn replace_record(
        &self,
        previous: &DiscordConnectorRecord,
        next: &DiscordConnectorRecord,
    ) -> Result<(), DiscordManagerError> {
        let _txn = self.transaction.lock().await;
        let persist = {
            let mut state = self.state.write().await;
            if state.discord_connectors.get(&previous.id) != Some(previous) {
                return Err(DiscordManagerError::Conflict);
            }
            state
                .discord_connectors
                .insert(next.id.clone(), next.clone());
            state.control_plane_persist_request()
        };
        if persist.save().await.is_err() {
            let mut state = self.state.write().await;
            if state.discord_connectors.get(&next.id) == Some(next) {
                state
                    .discord_connectors
                    .insert(previous.id.clone(), previous.clone());
            }
            return Err(DiscordManagerError::Persistence);
        }
        Ok(())
    }
    pub(crate) async fn set_enabled(
        &self,
        agent: String,
        id: String,
        enabled: bool,
    ) -> Result<DiscordConnectorRecord, DiscordManagerError> {
        let manager = self.clone();
        self.owned(async move {
            let _life = manager.lifecycle.lock().await;
            if enabled {
                return manager.activate_owned(&agent, &id).await;
            }
            let previous = manager.get(&agent, &id).await?;
            manager.stop_worker(&id).await;
            let mut next = manager.get(&agent, &id).await?;
            let before = next.clone();
            next.enabled = false;
            next.configuration_revision = next
                .configuration_revision
                .checked_add(1)
                .ok_or(DiscordManagerError::Conflict)?;
            next.updated_at_ms = now_ms();
            manager
                .replace_configuration(&before, &next, false, true)
                .await?;
            manager
                .set_status(&id, DiscordRuntimeStatus::Disabled, None)
                .await;
            let _ = previous;
            Ok(next)
        })
        .await
    }
    pub(crate) async fn replace_token(
        &self,
        agent: String,
        id: String,
        token: DiscordBotToken,
    ) -> Result<DiscordConnectorRecord, DiscordManagerError> {
        let manager = self.clone();
        self.owned(async move {
            let _life = manager.lifecycle.lock().await;
            manager.ensure_open()?;
            let previous = manager.get(&agent, &id).await?;
            let identity = manager
                .registry
                .discord(&token, &previous.channel.id)
                .identify()
                .await
                .map_err(transport_error)?;
            if identity.id != previous.bot.id {
                return Err(DiscordManagerError::Conflict);
            }
            manager.stop_worker(&id).await;
            let previous = manager.get(&agent, &id).await?;
            let enabled = previous.enabled;
            let generation = previous
                .credential_generation
                .checked_add(1)
                .ok_or(DiscordManagerError::Conflict)?;
            let mut next = previous.clone();
            next.credential_generation = generation;
            next.enabled = false;
            next.updated_at_ms = now_ms();
            let intent = DiscordCredentialIntent {
                connector_id: id.clone(),
                generation,
                operation: DiscordCredentialOperation::Replace,
                created_at_ms: now_ms(),
            };
            {
                let _txn = manager.transaction.lock().await;
                let persist = {
                    let mut state = manager.state.write().await;
                    if state.discord_connectors.get(&id) != Some(&previous) {
                        return Err(DiscordManagerError::Conflict);
                    }
                    state.discord_connectors.insert(id.clone(), next.clone());
                    state
                        .discord_credential_intents
                        .insert((id.clone(), generation), intent.clone());
                    state.control_plane_persist_request()
                };
                if persist.save().await.is_err() {
                    let mut state = manager.state.write().await;
                    if state.discord_connectors.get(&id) == Some(&next) {
                        state
                            .discord_connectors
                            .insert(id.clone(), previous.clone());
                    }
                    state
                        .discord_credential_intents
                        .remove(&(id.clone(), generation));
                    return Err(DiscordManagerError::Persistence);
                }
            }
            manager
                .credentials
                .put(&id, generation, &next.bot.id, &token)
                .await
                .map_err(|_| DiscordManagerError::Credential)?;
            let stored = manager.load_token(&next).await?;
            if stored.expose() != token.expose() {
                return Err(DiscordManagerError::Credential);
            }
            manager.finish_intent(&next, &intent, enabled).await?;
            // Retiring the exact old account cannot remove the new generation.
            manager
                .queue_cleanup(&id, previous.credential_generation)
                .await?;
            let next = manager.get(&agent, &id).await?;
            if enabled {
                manager.start_worker(next.clone(), token).await;
            }
            Ok(next)
        })
        .await
    }
    async fn queue_cleanup(&self, id: &str, generation: u64) -> Result<(), DiscordManagerError> {
        let intent = DiscordCredentialIntent {
            connector_id: id.into(),
            generation,
            operation: DiscordCredentialOperation::Delete,
            created_at_ms: now_ms(),
        };
        {
            let _txn = self.transaction.lock().await;
            let persist = {
                let mut state = self.state.write().await;
                state
                    .discord_credential_intents
                    .insert((id.into(), generation), intent.clone());
                state.control_plane_persist_request()
            };
            persist
                .save()
                .await
                .map_err(|_| DiscordManagerError::Persistence)?;
        }
        self.credentials
            .delete(id, generation)
            .await
            .map_err(|_| DiscordManagerError::Credential)?;
        let _txn = self.transaction.lock().await;
        let persist = {
            let mut state = self.state.write().await;
            state
                .discord_credential_intents
                .remove(&(id.into(), generation));
            state.control_plane_persist_request()
        };
        if persist.save().await.is_err() {
            self.state
                .write()
                .await
                .discord_credential_intents
                .entry((id.into(), generation))
                .or_insert(intent);
            return Err(DiscordManagerError::Persistence);
        }
        Ok(())
    }
    pub(crate) async fn delete(
        &self,
        agent: String,
        id: String,
    ) -> Result<(), DiscordManagerError> {
        let manager = self.clone();
        self.owned(async move {
            let _life = manager.lifecycle.lock().await;
            manager.ensure_open()?;
            manager.get(&agent, &id).await?;
            if manager.state.read().await.in_flight_runs(&agent) > 0 {
                return Err(DiscordManagerError::Busy);
            }
            manager.stop_worker(&id).await;
            let _txn = manager.transaction.lock().await;
            let (previous, next, intent, prior_intent, inbound, outbound, schedules, persist) = {
                let mut state = manager.state.write().await;
                if state.in_flight_runs(&agent) > 0 {
                    return Err(DiscordManagerError::Busy);
                }
                let previous = state
                    .discord_connectors
                    .get(&id)
                    .cloned()
                    .ok_or(DiscordManagerError::NotFound)?;
                let mut next = previous.clone();
                next.enabled = false;
                next.configuration_revision=next.configuration_revision.checked_add(1).ok_or(DiscordManagerError::Conflict)?;
                next.deleted_at_ms = Some(now_ms());
                next.updated_at_ms = now_ms();
                state.discord_connectors.insert(id.clone(), next.clone());
                let intent = DiscordCredentialIntent {
                    connector_id: id.clone(),
                    generation: previous.credential_generation,
                    operation: DiscordCredentialOperation::Delete,
                    created_at_ms: now_ms(),
                };
                let prior_intent = state
                    .discord_credential_intents
                    .insert((id.clone(), intent.generation), intent.clone());
                let mut inbound = Vec::new();
                for r in state
                    .discord_inbound
                    .values_mut()
                    .filter(|r| r.connector_id == id && !r.processing_state.is_terminal())
                {
                    let before = r.clone();
                    r.processing_state = DiscordProcessingState::Stopped;
                    inbound.push((before, r.clone()));
                }
                let mut outbound = Vec::new();
                for r in state
                    .discord_outbound
                    .values_mut()
                    .filter(|r| r.connector_id == id && !r.delivery_state.is_settled())
                {
                    let before = r.clone();
                    for ch in &mut r.chunks {
                        if ch.state.awaits_delivery() {
                            ch.state = DiscordDeliveryState::Suppressed;
                        } else if ch.state == DiscordDeliveryState::Sending {
                            ch.state = DiscordDeliveryState::Unknown;
                        }
                    }
                    r.delivery_state = DiscordDeliveryState::Suppressed;
                    outbound.push((before, r.clone()));
                }
                let mut schedules=Vec::new();
                for schedule in state.schedules.values_mut().filter(|s|matches!(&s.target,crate::schedules::ScheduleTarget::Connector{connector_id} if connector_id==&id)) {
                    let before=schedule.clone();schedule.enabled=false;schedule.updated_at_ms=now_ms();schedules.push((before,schedule.clone()));
                }
                (
                    previous,
                    next,
                    intent,
                    prior_intent,
                    inbound,
                    outbound,
                    schedules,
                    state.control_plane_persist_request(),
                )
            };
            if persist.save().await.is_err() {
                let mut state = manager.state.write().await;
                if state.discord_connectors.get(&id) == Some(&next) {
                    state.discord_connectors.insert(id.clone(), previous);
                }
                let intent_key = (id.clone(), intent.generation);
                if state.discord_credential_intents.get(&intent_key) == Some(&intent) {
                    if let Some(prior) = prior_intent {
                        state.discord_credential_intents.insert(intent_key, prior);
                    } else {
                        state.discord_credential_intents.remove(&intent_key);
                    }
                }
                for (before, after) in inbound {
                    let key = (before.connector_id.clone(), before.message_id.clone());
                    if state.discord_inbound.get(&key) == Some(&after) {
                        state.discord_inbound.insert(key, before);
                    }
                }
                for (before, after) in outbound {
                    if state.discord_outbound.get(&after.id) == Some(&after) {
                        state.discord_outbound.insert(before.id.clone(), before);
                    }
                }
                for (before,after) in schedules {if state.schedules.get(&after.id)==Some(&after){state.schedules.insert(before.id.clone(),before);}}
                return Err(DiscordManagerError::Persistence);
            }
            drop(_txn);
            manager
                .queue_cleanup(&id, previous.credential_generation)
                .await?;
            manager
                .set_status(&id, DiscordRuntimeStatus::Disabled, None)
                .await;
            Ok(())
        })
        .await
    }
    pub(crate) async fn restart(
        &self,
        agent: String,
        id: String,
    ) -> Result<DiscordConnectorRecord, DiscordManagerError> {
        let manager = self.clone();
        self.owned(async move {
            let _life = manager.lifecycle.lock().await;
            let c = manager.get(&agent, &id).await?;
            manager.stop_worker(&id).await;
            if !c.enabled {
                return Ok(c);
            }
            manager.activate_owned(&agent, &id).await
        })
        .await
    }
    pub(super) async fn stop_worker(&self, id: &str) {
        self.revoke(id);
        self.stop_typing_connector(id).await;
        let running = {
            let state = self.state.read().await;
            state
                .runs
                .active_records()
                .into_iter()
                .filter(|r| {
                    r.source == crate::runs::RunSource::Discord
                        && state
                            .discord_connectors
                            .get(id)
                            .is_some_and(|c| c.owns_room(&r.session_id))
                })
                .map(|r| (r.agent_id.clone(), r.id.clone()))
                .collect::<Vec<_>>()
        };
        for (agent, run) in running {
            let _ = self.runs.stop_run(&agent, &run).await;
        }
        let worker = self.workers.lock().await.remove(id);
        if let Some(worker) = worker {
            let _ = worker.cancel.send(true);
            let _ = worker.join.await;
        }
        self.drain(id).await;
    }
    pub(super) async fn start_worker(&self, c: DiscordConnectorRecord, token: DiscordBotToken) {
        if self.is_closing() {
            return;
        }
        {
            let mut workers = self.workers.lock().await;
            if workers.get(&c.id).is_some_and(|w| !w.join.is_finished()) {
                return;
            }
            workers.remove(&c.id);
        }
        self.paused
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&c.id);
        let epoch = self.epoch(&c.id);
        let (cancel, mut stopped) = watch::channel(false);
        let manager = self.worker_context();
        let id = c.id.clone();
        let mut close = self.close.clone();
        let gateway_record = c.clone();
        let join = tokio::spawn(async move {
            let wake = Arc::new(Notify::new());
            let gateway_stop = tokio_util::sync::CancellationToken::new();
            let gateway = manager.gateway.as_ref().map(|_| {
                let manager = manager.worker_context();
                let c = gateway_record.clone();
                let token = token.clone();
                let wake = wake.clone();
                let stop = gateway_stop.clone();
                tokio::spawn(async move {
                    manager.run_gateway(c, token, epoch, wake, stop).await;
                })
            });
            'worker: loop {
                if *stopped.borrow() || *close.borrow() || !manager.authority_current(&id, epoch) {
                    break;
                }
                let current = match manager
                    .state
                    .read()
                    .await
                    .discord_connectors
                    .get(&id)
                    .cloned()
                {
                    Some(c) if c.is_active() => c,
                    _ => break,
                };
                if current.settings.is_some() {
                    if let Err(error) = manager.discover_channels(&current, epoch, &token).await {
                        manager.set_status(&id,DiscordRuntimeStatus::Degraded,Some("Discord discovery could not complete; saved conversation cursors are retained.")).await;
                        if error == DiscordManagerError::InvalidToken {
                            break;
                        }
                    }
                }
                let latest = match manager.get(&current.agent_id, &id).await {
                    Ok(c) => c,
                    Err(_) => break,
                };
                for current in latest.destinations() {
                    let adapter = manager.registry.discord(&token, &current.channel.id);
                    let poll = adapter.receive(ConnectorReceiveRequest {
                        cursor: current.cursor.clone(),
                    });
                    let result = tokio::select! { biased; _=stopped.changed()=>break 'worker,_=close.changed()=>break 'worker,result=poll=>result };
                    match result {
                        Ok(batch) => {
                            manager
                                .set_status(&id, DiscordRuntimeStatus::Ready, None)
                                .await;
                            if let Err(error) =
                                manager.accept_batch_captured(&current, epoch, batch).await
                            {
                                if error == DiscordManagerError::Backpressure {
                                    manager.set_status(&id,DiscordRuntimeStatus::Degraded,Some("Inbound capacity is exhausted; messages have not been skipped.")).await;
                                } else if error == DiscordManagerError::UnsupportedInput {
                                    manager.set_status(&id,DiscordRuntimeStatus::Degraded,Some("Discord input exceeded the supported bound; the cursor has not advanced.")).await;
                                } else {
                                    manager.set_status(&id,DiscordRuntimeStatus::Error,Some("Discord scan admission failed; the cursor has not advanced.")).await;
                                }
                            }
                        }
                        Err(error) => {
                            if matches!(error, DiscordTransportError::Authentication)
                                || (current.settings.is_none()
                                    && matches!(
                                        error,
                                        DiscordTransportError::PermissionDenied
                                            | DiscordTransportError::HttpStatus { status: 404 }
                                    ))
                            {
                                manager
                                    .paused
                                    .lock()
                                    .unwrap_or_else(|e| e.into_inner())
                                    .insert(id.clone());
                                manager.revoke(&id);
                                let status = if error == DiscordTransportError::Authentication {
                                    DiscordRuntimeStatus::CredentialRequired
                                } else {
                                    DiscordRuntimeStatus::Error
                                };
                                manager
                                    .set_status(&id, status, Some(error.category()))
                                    .await;
                                break 'worker;
                            }
                            let status = if error == DiscordTransportError::BacklogLimit {
                                DiscordRuntimeStatus::Degraded
                            } else {
                                DiscordRuntimeStatus::Error
                            };
                            manager
                                .set_status(&id, status, Some(error.category()))
                                .await;
                        }
                    }
                }
                // Runs and sends are owned to completion: cancellation stops producers, never drops a tool-bearing child.
                if manager.authority_current(&id, epoch) {
                    let _ = manager.process_one_captured(id.clone(), epoch).await;
                    let _ = manager.deliver_captured(&current, epoch, &token).await;
                }
                tokio::select! {_=stopped.changed()=>break,_=close.changed()=>break,_=wake.notified()=>{},_=tokio::time::sleep(std::time::Duration::from_secs(5))=>{}}
            }
            gateway_stop.cancel();
            if let Some(gateway) = gateway {
                let _ = gateway.await;
            }
        });
        self.workers
            .lock()
            .await
            .insert(c.id, Worker { cancel, join });
    }
    pub(crate) async fn start_restored(&self) {
        let intents = self
            .state
            .read()
            .await
            .discord_credential_intents
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for intent in intents {
            if intent.operation == DiscordCredentialOperation::Delete {
                let _ = self
                    .queue_cleanup(&intent.connector_id, intent.generation)
                    .await;
            }
        }
        let connectors = self
            .state
            .read()
            .await
            .discord_connectors
            .values()
            .filter(|c| c.deleted_at_ms.is_none())
            .cloned()
            .collect::<Vec<_>>();
        for c in connectors {
            if self
                .state
                .read()
                .await
                .discord_credential_intents
                .keys()
                .any(|(id, _)| id == &c.id)
            {
                self.set_status(
                    &c.id,
                    DiscordRuntimeStatus::Reconciling,
                    Some("Credential operation requires owner reconciliation."),
                )
                .await;
                continue;
            }
            if !c.enabled {
                self.set_status(&c.id, DiscordRuntimeStatus::Disabled, None)
                    .await;
                continue;
            }
            match self.load_token(&c).await {
                Ok(token) => {
                    self.set_status(&c.id, DiscordRuntimeStatus::Ready, None)
                        .await;
                    self.start_worker(c, token).await;
                }
                Err(_) => {
                    self.set_status(
                        &c.id,
                        DiscordRuntimeStatus::CredentialRequired,
                        Some("Discord bot credentials are required."),
                    )
                    .await
                }
            }
        }
    }
    pub(crate) async fn request_shutdown(&self) {
        self.closing.store(true, Ordering::SeqCst);
        if let Some(close) = &self.owner_close {
            let _ = close.send(true);
        }
        let mut workers = self.workers.lock().await;
        for (id, worker) in workers.iter_mut() {
            self.revoke(id);
            let _ = worker.cancel.send(true);
        }
    }
    pub(crate) async fn shutdown(&self) {
        self.request_shutdown().await;
        self.drain_typing().await;
        let workers = std::mem::take(&mut *self.workers.lock().await);
        for (_, worker) in workers {
            let _ = worker.join.await;
        }
        loop {
            let wait = self.owned_notify.notified();
            if self.owned_count.load(Ordering::SeqCst) == 0 {
                break;
            }
            wait.await;
        }
    }
}
