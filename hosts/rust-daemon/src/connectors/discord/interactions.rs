//! Bot-wide discovery and response policy.
use super::*;

const MAX_CONVERSATIONS: usize = 1000;
const MAX_THREAD_STARTS: usize = 1000;

fn activation_cursor(c: &DiscordConnectorRecord) -> String {
    // A first channel event must not be baselined past the event that discovered it.
    let timestamp = c
        .activated_at_ms
        .unwrap_or(c.created_at_ms)
        .saturating_sub(1_420_070_400_000);
    timestamp.checked_shl(22).unwrap_or(u64::MAX).to_string()
}

fn mentioned(text: &str, bot: &str) -> bool {
    text.contains(&format!("<@{bot}>")) || text.contains(&format!("<@!{bot}>"))
}

impl DiscordManager {
    pub(super) async fn validate_destinations(
        &self,
        c: &mut DiscordConnectorRecord,
        token: &DiscordBotToken,
    ) -> Result<(), DiscordManagerError> {
        let Some(settings) = c.settings.clone() else {
            return Ok(());
        };
        if !valid_settings(&settings) {
            return Err(DiscordManagerError::InvalidInput);
        }
        let mut ids = settings.allowed_channel_ids;
        ids.extend(settings.free_response_channel_ids);
        let home = settings.home_channel_id;
        ids.extend(home.clone());
        ids.sort();
        ids.dedup();
        for id in ids {
            let channel = self
                .native()?
                .get_channel(token, &id)
                .await
                .map_err(transport_error)?;
            if channel.id != id
                || c.guild_scope
                    .as_ref()
                    .is_some_and(|guild| guild != &channel.guild_id)
                || (!c.channel_allowed(&channel) && home.as_ref() != Some(&id))
            {
                return Err(DiscordManagerError::InvalidInput);
            }
            let cursor = if c.activated_at_ms.is_some() && c.destination_for_channel(&id).is_none()
            {
                let cursor = self
                    .native()?
                    .latest_cursor(token, &id)
                    .await
                    .map_err(transport_error)?;
                if DiscordSnowflake::cursor(&cursor).is_none() {
                    return Err(DiscordManagerError::Conflict);
                }
                cursor
            } else {
                "0".into()
            };
            self.add_conversation(c, channel, cursor, false)?;
        }
        Ok(())
    }
    fn add_conversation(
        &self,
        c: &mut DiscordConnectorRecord,
        channel: DiscordChannelMetadata,
        cursor: String,
        participating: bool,
    ) -> Result<(), DiscordManagerError> {
        if !c.bot_wide && channel.id == c.channel.id {
            if !channel.routing_eq(&c.channel) {
                return Err(DiscordManagerError::Conflict);
            }
            c.channel.name = channel.name.clone();
            if let Some(conversation) = c.conversations.iter_mut().find(|v| v.room_id == c.room_id)
            {
                conversation.channel.name = channel.name.clone();
            }
            if participating
                && channel.kind == DiscordChannelKind::GuildThread
                && !c.conversations.iter().any(|v| v.room_id == c.room_id)
            {
                c.conversations.push(DiscordConversation {
                    channel,
                    room_id: c.room_id.clone(),
                    cursor,
                    participating: true,
                    auto_title: None,
                });
            } else if participating {
                if let Some(conversation) =
                    c.conversations.iter_mut().find(|v| v.room_id == c.room_id)
                {
                    conversation.participating = true;
                }
            }
            return Ok(());
        }
        if let Some(existing) = c
            .conversations
            .iter_mut()
            .find(|v| v.channel.id == channel.id)
        {
            if !existing.channel.routing_eq(&channel) {
                return Err(DiscordManagerError::Conflict);
            }
            existing.channel.name = channel.name;
            existing.participating |= participating;
            return Ok(());
        }
        if c.conversations.len() >= MAX_CONVERSATIONS {
            return Err(DiscordManagerError::Backpressure);
        }
        let room_id = format!("discord:{}:{}", c.id, channel.id);
        c.conversations.push(DiscordConversation {
            channel,
            room_id,
            cursor,
            participating,
            auto_title: None,
        });
        Ok(())
    }
    pub(crate) async fn configure(
        &self,
        agent: String,
        id: String,
        settings: DiscordSettings,
        users: Vec<String>,
    ) -> Result<DiscordConnectorRecord, DiscordManagerError> {
        let manager = self.clone();
        self.owned(async move {
            let _life = manager.lifecycle.lock().await;
            if !valid_settings(&settings) || !valid_ids(&users, 100) {
                return Err(DiscordManagerError::InvalidInput);
            }
            let previous = manager.get(&agent, &id).await?;
            let token = manager.load_token(&previous).await?;
            let users = if users.is_empty() {
                manager.check_bot(token.clone()).await?.owner_user_ids
            } else {
                users
            };
            if !valid_ids(&users, 100)
                || (users.is_empty() && settings.approved_role_ids.is_empty())
            {
                return Err(DiscordManagerError::InvalidInput);
            }
            let mut candidate = previous.clone();
            candidate.settings = Some(settings.clone());
            manager
                .validate_destinations(&mut candidate, &token)
                .await?;
            manager.stop_worker(&id).await;
            let previous = manager.get(&agent, &id).await?;
            let mut next = previous.clone();
            next.channel.name = candidate.channel.name.clone();
            next.settings = Some(settings);
            next.approved_user_ids = users;
            next.configuration_revision = next
                .configuration_revision
                .checked_add(1)
                .ok_or(DiscordManagerError::Conflict)?;
            next.updated_at_ms = now_ms();
            for conversation in candidate.conversations {
                manager.add_conversation(
                    &mut next,
                    conversation.channel,
                    conversation.cursor,
                    conversation.participating,
                )?;
            }
            let disable_schedules = next.home_destination().is_none();
            manager
                .replace_configuration(&previous, &next, true, disable_schedules)
                .await?;
            if next.enabled {
                manager.start_worker(next.clone(), token).await;
            }
            Ok(next)
        })
        .await
    }
    pub(super) async fn replace_configuration(
        &self,
        previous: &DiscordConnectorRecord,
        next: &DiscordConnectorRecord,
        revoke_work: bool,
        disable_schedules: bool,
    ) -> Result<(), DiscordManagerError> {
        let _txn = self.transaction.lock().await;
        let (inbound, outbound, schedules, persist) = {
            let mut state = self.state.write().await;
            if state.discord_connectors.get(&previous.id) != Some(previous) {
                return Err(DiscordManagerError::Conflict);
            }
            let mut inbound = Vec::new();
            let mut outbound = Vec::new();
            let mut schedules = Vec::new();
            if revoke_work {
                for record in state
                    .discord_inbound
                    .values_mut()
                    .filter(|r| r.connector_id == next.id && !r.processing_state.is_terminal())
                {
                    let before = record.clone();
                    record.processing_state = DiscordProcessingState::Stopped;
                    inbound.push((before, record.clone()));
                }
                for record in state
                    .discord_outbound
                    .values_mut()
                    .filter(|r| r.connector_id == next.id && !r.delivery_state.is_settled())
                {
                    let before = record.clone();
                    request_outbound_stop(record);
                    outbound.push((before, record.clone()));
                }
            }
            if disable_schedules {
                for schedule in state.schedules.values_mut().filter(|s|matches!(&s.target,crate::schedules::ScheduleTarget::Connector{connector_id} if connector_id==&next.id)) {
                    let before=schedule.clone();schedule.enabled=false;schedule.updated_at_ms=now_ms();schedules.push((before,schedule.clone()));
                }
            }
            state
                .discord_connectors
                .insert(next.id.clone(), next.clone());
            (
                inbound,
                outbound,
                schedules,
                state.control_plane_persist_request(),
            )
        };
        if persist.save().await.is_err() {
            let mut state = self.state.write().await;
            if state.discord_connectors.get(&next.id) == Some(next) {
                state
                    .discord_connectors
                    .insert(previous.id.clone(), previous.clone());
            }
            for (before, after) in inbound {
                let key = (after.connector_id.clone(), after.message_id.clone());
                if state.discord_inbound.get(&key) == Some(&after) {
                    state.discord_inbound.insert(key, before);
                }
            }
            for (before, after) in outbound {
                if state.discord_outbound.get(&after.id) == Some(&after) {
                    state.discord_outbound.insert(before.id.clone(), before);
                }
            }
            for (before, after) in schedules {
                if state.schedules.get(&after.id) == Some(&after) {
                    state.schedules.insert(before.id.clone(), before);
                }
            }
            return Err(DiscordManagerError::Persistence);
        }
        Ok(())
    }
    pub(super) async fn discover_channels(
        &self,
        c: &DiscordConnectorRecord,
        epoch: u64,
        token: &DiscordBotToken,
    ) -> Result<(), DiscordManagerError> {
        let queued = self
            .discovered
            .lock()
            .await
            .get(&c.id)
            .cloned()
            .unwrap_or_default();
        for notice in queued {
            let (captured, guild, id, message_id) = notice.clone();
            let result = async {
                if captured != epoch || !self.authority_current(&c.id, epoch) {
                    return Ok(());
                }
                let current = self.get(&c.agent_id, &c.id).await?;
                if current.destination_for_channel(&id).is_some() {
                    return Ok(());
                }
                let channel = match self.native()?.get_channel(token, &id).await {
                    Ok(channel) => channel,
                    Err(
                        DiscordTransportError::PermissionDenied
                        | DiscordTransportError::HttpStatus { status: 404 },
                    ) => return Ok(()),
                    Err(error) => return Err(transport_error(error)),
                };
                if channel.id != id
                    || channel.guild_id != guild
                    || !current.channel_allowed(&channel)
                {
                    return Ok(());
                }
                let mut next = current.clone();
                let cursor = message_id
                    .as_deref()
                    .and_then(DiscordSnowflake::parse)
                    .map(|v| v.value().saturating_sub(1).to_string())
                    .unwrap_or_else(|| activation_cursor(&current));
                self.add_conversation(&mut next, channel, cursor, false)?;
                if !self.authority_current(&c.id, epoch) {
                    return Err(DiscordManagerError::Conflict);
                }
                self.replace_record(&current, &next).await?;
                Ok(())
            }
            .await;
            result?;
            // Remove only a completed discovery. A transient error retains this
            // trigger and every unhandled channel for the next bounded pass.
            if let Some(pending) = self.discovered.lock().await.get_mut(&c.id) {
                pending.retain(|item| item != &notice);
            }
        }
        Ok(())
    }
    pub(super) async fn admitted_destination(
        &self,
        c: &DiscordConnectorRecord,
        epoch: u64,
        message: &DiscordMessage,
    ) -> Result<Option<DiscordConnectorRecord>, DiscordManagerError> {
        if message.is_bot
            || message.sender.id == c.bot.id
            || message.webhook_id.is_some()
            || !matches!(message.message_type, 0 | 19)
            || (message.text.trim().is_empty() && message.attachments.is_empty())
        {
            return Ok(None);
        }
        let Some(settings) = &c.settings else {
            return Ok(c
                .approved_user_ids
                .contains(&message.sender.id)
                .then(|| c.clone()));
        };
        if !c.channel_allowed(&c.channel) {
            return Ok(None);
        }
        let explicit = mentioned(&message.text, &c.bot.id);
        let free = settings
            .free_response_channel_ids
            .iter()
            .any(|id| id == &c.channel.id || c.channel.parent_channel_id.as_ref() == Some(id));
        let thread = c.channel.kind == DiscordChannelKind::GuildThread;
        let participating = c
            .conversations
            .iter()
            .any(|v| v.room_id == c.room_id && v.participating);
        let needs_mention = if thread {
            settings.thread_require_mention || !participating
        } else {
            settings.require_mention
        };
        if needs_mention && !explicit && !free {
            return Ok(None);
        }
        if !c.approved_user_ids.contains(&message.sender.id) {
            if settings.approved_role_ids.is_empty() {
                return Ok(None);
            }
            let token = self.load_token(c).await?;
            let roles = match self
                .native()?
                .member_roles(&token, &c.channel.guild_id, &message.sender.id)
                .await
            {
                Ok(roles) => roles,
                Err(
                    DiscordTransportError::PermissionDenied
                    | DiscordTransportError::HttpStatus { status: 404 },
                ) => return Ok(None),
                Err(error) => return Err(transport_error(error)),
            };
            if !roles
                .iter()
                .any(|role| settings.approved_role_ids.contains(role))
            {
                return Ok(None);
            }
        }
        if !self.authority_current(&c.id, epoch) {
            return Err(DiscordManagerError::Conflict);
        }
        if thread {
            if !participating {
                let current = self.get(&c.agent_id, &c.id).await?;
                let mut next = current.clone();
                self.add_conversation(&mut next, c.channel.clone(), c.cursor.clone(), true)?;
                self.replace_record(&current, &next).await?;
            }
            return Ok(Some(c.clone()));
        }
        if !explicit || free || !settings.auto_thread {
            return Ok(Some(c.clone()));
        }
        let current = self.get(&c.agent_id, &c.id).await?;
        if let Some(created) = current.thread_starts.get(&message.id) {
            if created.channel_id != c.channel.id {
                return Err(DiscordManagerError::Conflict);
            }
            return Ok(created
                .thread_channel_id
                .as_ref()
                .and_then(|id| current.destination_for_channel(id)));
        }
        if current.thread_starts.len() >= MAX_THREAD_STARTS
            || current.conversations.len() >= MAX_CONVERSATIONS
        {
            return Err(DiscordManagerError::Backpressure);
        }
        let mut claimed = current.clone();
        claimed.thread_starts.insert(
            message.id.clone(),
            DiscordThreadStart {
                channel_id: c.channel.id.clone(),
                thread_channel_id: None,
            },
        );
        self.replace_record(&current, &claimed).await?;
        if !self.authority_current(&c.id, epoch) {
            return Err(DiscordManagerError::Conflict);
        }
        let token = self.load_token(c).await?;
        let channel = match self
            .native()?
            .create_thread(
                &token,
                &c.channel.id,
                &message.id,
                &text::thread_title(&message.text),
            )
            .await
        {
            Ok(channel)
                if channel.guild_id == c.channel.guild_id
                    && channel.parent_channel_id.as_deref() == Some(&c.channel.id) =>
            {
                channel
            }
            _ => {
                self.set_status(&c.id,DiscordRuntimeStatus::Degraded,Some("Thread creation failed or is uncertain; this message was not redirected and creation will not be retried.")).await;
                return Ok(None);
            }
        };
        if !self.authority_current(&c.id, epoch) {
            return Err(DiscordManagerError::Conflict);
        }
        let current = self.get(&c.agent_id, &c.id).await?;
        let mut next = current.clone();
        self.add_conversation(&mut next, channel.clone(), message.id.clone(), true)?;
        if let Some(name) = &channel.name {
            if let Some(conversation) = next
                .conversations
                .iter_mut()
                .find(|v| v.channel.id == channel.id)
            {
                conversation.auto_title = Some(DiscordThreadTitle {
                    initial_name: name.clone(),
                    attempted: false,
                });
            }
        }
        next.thread_starts.insert(
            message.id.clone(),
            DiscordThreadStart {
                channel_id: c.channel.id.clone(),
                thread_channel_id: Some(channel.id.clone()),
            },
        );
        self.replace_record(&current, &next).await?;
        Ok(next.destination_for_channel(&channel.id))
    }
}
