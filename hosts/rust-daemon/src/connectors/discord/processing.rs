use super::*;
use crate::agent_runs::{
    AcceptRun, AcceptedRun, AgentRunRequest, QueuedRunStart, QueuedStartError, RunRoom,
    SessionRunMode,
};
use crate::routes::ApiError;
use crate::runs::{RunOutcome, RunSource, RunStatus};
use anima_core::{DataValue, TaskStatus};
use futures::future::BoxFuture;
use std::collections::BTreeMap;
impl DiscordManager {
    pub(crate) async fn process_one(&self, id: String) -> Result<bool, DiscordManagerError> {
        let manager = self.clone();
        self.owned(async move { manager.process_one_owned(id, None).await })
            .await
    }
    pub(super) async fn process_one_captured(
        &self,
        id: String,
        epoch: u64,
    ) -> Result<bool, DiscordManagerError> {
        let manager = self.clone();
        self.owned(async move { manager.process_one_owned(id, Some(epoch)).await })
            .await
    }
    async fn process_one_owned(
        &self,
        id: String,
        captured: Option<u64>,
    ) -> Result<bool, DiscordManagerError> {
        // A drain before a restart leaves received messages for later.
        if self.runs.is_draining() {
            return Ok(false);
        }
        let epoch = captured.unwrap_or_else(|| self.epoch(&id));
        let (c, inbound) = {
            let _txn = self.transaction.lock().await;
            let (c, key, previous, next, persist) = {
                let mut state = self.state.write().await;
                let c = state
                    .discord_connectors
                    .get(&id)
                    .filter(|c| c.is_active())
                    .cloned()
                    .ok_or(DiscordManagerError::NotFound)?;
                if !self.authority_current(&id, epoch)
                    || state
                        .discord_outbound
                        .values()
                        .filter(|r| r.connector_id == id && !r.delivery_state.is_settled())
                        .count()
                        >= MAX_OUTBOUND
                    || state.discord_outbound.values().any(|r| {
                        r.connector_id == id && r.delivery_state == DiscordDeliveryState::Unknown
                    })
                {
                    return Ok(false);
                }
                let key = state
                    .discord_inbound
                    .iter()
                    .filter(|(_, r)| {
                        r.connector_id == id
                            && r.processing_state == DiscordProcessingState::Received
                    })
                    .min_by_key(|(_, r)| DiscordSnowflake::parse(&r.message_id).map(|i| i.value()))
                    .map(|(key, _)| key.clone());
                let Some(key) = key else {
                    return Ok(false);
                };
                let previous = state
                    .discord_inbound
                    .get(&key)
                    .cloned()
                    .expect("selected receipt");
                let c = c
                    .destination_for_room(&previous.room_id)
                    .filter(|destination| c.channel_allowed(&destination.channel))
                    .ok_or(DiscordManagerError::Conflict)?;
                let mut next = previous.clone();
                next.processing_state = DiscordProcessingState::Processing;
                state.discord_inbound.insert(key.clone(), next.clone());
                (
                    c,
                    key,
                    previous,
                    next,
                    state.control_plane_persist_request(),
                )
            };
            if persist.save().await.is_err() {
                let mut state = self.state.write().await;
                if state.discord_inbound.get(&key) == Some(&next) {
                    state.discord_inbound.insert(key, previous);
                }
                return Err(DiscordManagerError::Persistence);
            }
            (c, next)
        };
        let manager = self.clone();
        let run_c = c.clone();
        let run_inbound = inbound.clone();
        let agent = c.agent_id.clone();
        let session = c.room_id.clone();
        let text = inbound.text.clone();
        let key = inbound.run_idempotency_key.clone();
        let start = QueuedRunStart::new(
            move |run_id| -> BoxFuture<'static, Result<(), QueuedStartError>> {
                Box::pin(async move {
                    manager
                        .run_native(run_c, text, key, run_id, Some(run_inbound), epoch)
                        .await
                        .map_err(|e| {
                            if manager.is_closing() {
                                QueuedStartError::ShuttingDown
                            } else {
                                QueuedStartError::Failed(e.to_string())
                            }
                        })
                })
            },
        );
        let accepted = self
            .runs
            .accept_run(
                AcceptRun {
                    agent_id: agent,
                    session_id: session,
                    text: inbound.text.clone(),
                    attachment_ids: inbound.attachment_ids.clone(),
                    idempotency_key: inbound.run_idempotency_key.clone(),
                    mode: SessionRunMode::Queue,
                    source: RunSource::Discord,
                    source_ref: Some(format!("{}:{}", c.id, inbound.message_id)),
                    skill: None,
                },
                start,
            )
            .await;
        if let Err(error) = &accepted {
            // A drain that began after the pickup accepted nothing: the
            // receipt goes back to received, for the restart or a cancel.
            let deferred = crate::agent_runs::is_draining_refusal(error);
            let _txn = self.transaction.lock().await;
            let persist = {
                let mut state = self.state.write().await;
                let key = (c.id.clone(), inbound.message_id.clone());
                if let Some(r) = state.discord_inbound.get_mut(&key) {
                    if r == &inbound {
                        r.processing_state = if deferred {
                            DiscordProcessingState::Received
                        } else {
                            DiscordProcessingState::Stopped
                        };
                    }
                }
                state.control_plane_persist_request()
            };
            persist
                .save()
                .await
                .map_err(|_| DiscordManagerError::Persistence)?;
            if deferred {
                return Ok(false);
            }
            return Err(DiscordManagerError::Persistence);
        }
        if let Ok(AcceptedRun::Replayed(run)) = accepted {
            if run.status.is_terminal() {
                let _txn = self.transaction.lock().await;
                let persist = {
                    let mut state = self.state.write().await;
                    if let Some(r) = state.discord_inbound.get_mut(&(id, inbound.message_id)) {
                        r.processing_state = DiscordProcessingState::Stopped;
                        r.run_id = Some(run.id);
                    }
                    state.control_plane_persist_request()
                };
                persist
                    .save()
                    .await
                    .map_err(|_| DiscordManagerError::Persistence)?;
            }
        }
        Ok(true)
    }
    pub(crate) async fn send_from_owner_accepted(
        &self,
        agent: String,
        id: String,
        text: String,
        key: String,
        run_id: String,
    ) -> Result<(), DiscordManagerError> {
        let c = self.get(&agent, &id).await?;
        let room = if c.bot_wide {
            c.home_destination()
                .ok_or(DiscordManagerError::InvalidInput)?
                .room_id
        } else {
            c.room_id
        };
        self.send_from_owner_in_room_accepted(agent, id, room, text, key, run_id)
            .await
    }
    pub(crate) async fn send_from_owner_in_room_accepted(
        &self,
        agent: String,
        id: String,
        room: String,
        text: String,
        key: String,
        run_id: String,
    ) -> Result<(), DiscordManagerError> {
        let manager = self.clone();
        self.owned(async move {
            let (c, epoch) = {
                let _life = manager.lifecycle.lock().await;
                let root = manager.get(&agent, &id).await?;
                let c = root
                    .destination_for_room(&room)
                    .filter(|destination| root.can_send_to_channel(&destination.channel))
                    .ok_or(DiscordManagerError::InvalidInput)?;
                let epoch = manager.epoch(&id);
                (c, epoch)
            };
            if !c.is_active() {
                return Err(DiscordManagerError::Conflict);
            }
            manager.run_native(c, text, key, run_id, None, epoch).await
        })
        .await
    }
    async fn run_native(
        &self,
        c: DiscordConnectorRecord,
        text: String,
        key: String,
        run_id: String,
        inbound: Option<DiscordInboundRecord>,
        epoch: u64,
    ) -> Result<(), DiscordManagerError> {
        self.ensure_open()?;
        {
            let state = self.state.read().await;
            if !self.authority_current(&c.id, epoch)
                || !state
                    .discord_connectors
                    .get(&c.id)
                    .is_some_and(|current| current.matches_destination(&c))
            {
                return Err(DiscordManagerError::Conflict);
            }
        }

        // Attach acceptance's stable run ID before any model/tool effect.
        if let Some(r) = &inbound {
            let _txn = self.transaction.lock().await;
            let persist = {
                let mut state = self.state.write().await;
                if !self.authority_current(&c.id, epoch) {
                    return Err(DiscordManagerError::Conflict);
                }
                let target = state
                    .discord_inbound
                    .get_mut(&(c.id.clone(), r.message_id.clone()))
                    .filter(|r| r.processing_state == DiscordProcessingState::Processing)
                    .ok_or(DiscordManagerError::Conflict)?;
                target.run_id = Some(run_id.clone());
                state.control_plane_persist_request()
            };
            persist
                .save()
                .await
                .map_err(|_| DiscordManagerError::Persistence)?;
        }
        let manager = self.clone();
        let commit_c = c.clone();
        let commit_inbound = inbound.clone();
        let delta = Arc::new(StdMutex::new((
            None::<DiscordInboundRecord>,
            None::<DiscordOutboundRecord>,
            None::<(String, String, String)>,
        )));
        let commit_delta = delta.clone();
        let title_delta = delta.clone();
        let mut metadata = BTreeMap::from([
            (
                "source".into(),
                DataValue::String(
                    if inbound.is_some() {
                        "discord"
                    } else {
                        "discordThread"
                    }
                    .into(),
                ),
            ),
            ("connectorId".into(), DataValue::String(c.id.clone())),
            ("channelId".into(), DataValue::String(c.channel.id.clone())),
        ]);
        if let Some(inbound) = &inbound {
            metadata.insert(
                "messageId".into(),
                DataValue::String(inbound.message_id.clone()),
            );
            metadata.insert(
                "senderId".into(),
                DataValue::String(inbound.sender.id.clone()),
            );
        }
        let title_input = inbound
            .as_ref()
            .map_or_else(|| text.clone(), |r| r.text.clone());
        let text = inbound.as_ref().map_or(text, sender::attributed_text);
        let attachment_ids = inbound
            .as_ref()
            .map(|r| r.attachment_ids.as_slice())
            .unwrap_or_default();
        let store = self.state.read().await.history.store();
        let records = crate::agent_runs::attachments::resolve_run_attachments(
            store.as_ref(),
            &c.agent_id,
            &c.room_id,
            attachment_ids,
        )
        .await
        .map_err(|_| DiscordManagerError::Persistence)?;
        let request = AgentRunRequest {
            agent_id: c.agent_id.clone(),
            content: crate::agent_runs::attachments::attachment_content(
                &text,
                &records,
                Some(metadata),
            ),
            room: if c.channel.kind == DiscordChannelKind::GuildThread {
                RunRoom::DiscordThread {
                    room_id: c.room_id.clone(),
                    scope: self.thread_scope(&c, epoch),
                }
            } else {
                RunRoom::Stable(c.room_id.clone())
            },
            idempotency_key: Some(key),
            source: RunSource::Discord,
            source_ref: Some(if let Some(r) = &inbound {
                format!("{}:{}", c.id, r.message_id)
            } else {
                c.id.clone()
            }),
            parent: None,
        };
        let commit = move |state: &mut DaemonState, outcome: &RunOutcome| {
            if !manager.authority_current(&commit_c.id, epoch)
                || !state
                    .discord_connectors
                    .get(&commit_c.id)
                    .is_some_and(|current| current.matches_destination(&commit_c))
            {
                return Err(ApiError::service_unavailable(
                    "Discord authority changed during run",
                ));
            }
            let stopped = outcome.status == RunStatus::Cancelled
                || state
                    .runs
                    .get(&outcome.run_id)
                    .is_some_and(|r| r.stop.is_some());
            let mut delta = commit_delta.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(inbound) = &commit_inbound {
                let target = state
                    .discord_inbound
                    .get_mut(&(commit_c.id.clone(), inbound.message_id.clone()))
                    .ok_or_else(ApiError::not_found)?;
                if target.processing_state == DiscordProcessingState::Stopped {
                    return Ok(());
                }
                if target.processing_state != DiscordProcessingState::Processing {
                    return Err(ApiError::conflict("Discord receipt changed"));
                }
                delta.0 = Some(target.clone());
                target.processing_state = if stopped {
                    DiscordProcessingState::Stopped
                } else if outcome.result.status == TaskStatus::Error {
                    DiscordProcessingState::Rejected
                } else {
                    DiscordProcessingState::Processed
                };
            }
            if outcome.result.status == TaskStatus::Error {
                return Ok(());
            }
            let (Some(reply_id), Some(reply)) = (
                outcome.reply_message_id.as_deref(),
                outcome.result.data.as_ref(),
            ) else {
                return Err(ApiError::bad_request_static(
                    "agent produced no assistant message",
                ));
            };
            if reply.text.is_empty() {
                return Ok(());
            }
            if state
                .discord_outbound
                .values()
                .filter(|r| r.connector_id == commit_c.id && !r.delivery_state.is_settled())
                .count()
                >= MAX_OUTBOUND
            {
                return Err(ApiError::service_unavailable(
                    "Discord outbox capacity is exhausted",
                ));
            }
            let record = outbound_for(&commit_c, reply_id, &reply.text, stopped);
            if let Some(old) = state.discord_outbound.get(&record.id) {
                if old.text != record.text
                    || old.assistant_message_id != record.assistant_message_id
                    || old.room_id != record.room_id
                {
                    return Err(ApiError::conflict(
                        "Discord reply conflicts with durable output",
                    ));
                }
            } else {
                state
                    .discord_outbound
                    .insert(record.id.clone(), record.clone());
                delta.1 = Some(record);
            }
            // Consume naming eligibility in the same durable commit as the first
            // completed reply. A restart cannot let a later reply claim the title.
            if outcome.status == RunStatus::Completed && !stopped {
                if let Some(title) = state
                    .discord_connectors
                    .get_mut(&commit_c.id)
                    .and_then(|c| {
                        c.conversations
                            .iter_mut()
                            .find(|v| v.room_id == commit_c.room_id)
                    })
                    .and_then(|v| v.auto_title.as_mut())
                    .filter(|v| !v.attempted)
                {
                    title.attempted = true;
                    delta.2 = Some((
                        commit_c.id.clone(),
                        commit_c.room_id.clone(),
                        title.initial_name.clone(),
                    ));
                }
            }
            Ok(())
        };
        let rollback = move |state: &mut DaemonState| {
            let delta = delta.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(previous) = &delta.0 {
                let key = (previous.connector_id.clone(), previous.message_id.clone());
                if let Some(current) = state.discord_inbound.get_mut(&key) {
                    if current.run_id == previous.run_id && current.processing_state.is_terminal() {
                        *current = previous.clone();
                    }
                }
            }
            if let Some(inserted) = &delta.1 {
                if state.discord_outbound.get(&inserted.id) == Some(inserted) {
                    state.discord_outbound.remove(&inserted.id);
                }
            }
            if let Some((connector, room, initial_name)) = &delta.2 {
                if let Some(title) = state
                    .discord_connectors
                    .get_mut(connector)
                    .and_then(|c| c.conversations.iter_mut().find(|v| &v.room_id == room))
                    .and_then(|v| v.auto_title.as_mut())
                    .filter(|v| &v.initial_name == initial_name)
                {
                    title.attempted = false;
                }
            }
            Ok(())
        };
        let typing = self.start_typing(&c, &run_id, epoch).await;
        let result = self
            .runs
            .run_accepted_with_commit(request, run_id.clone(), commit, rollback)
            .await
            .map_err(|e| {
                if crate::agent_runs::is_shutting_down(&e) {
                    DiscordManagerError::WorkerStopped
                } else {
                    DiscordManagerError::Persistence
                }
            });
        drop(typing);
        self.stop_typing_run(&c.id, &run_id).await;
        let result = result?;
        let initial_name = title_delta
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .2
            .clone();
        if let (Some(reply), Some((_, _, initial_name))) = (result.result.data, initial_name) {
            if !reply.text.trim().is_empty() {
                if let Err(error) = self.schedule_thread_title(
                    &c,
                    &run_id,
                    title_input,
                    reply.text,
                    initial_name,
                    epoch,
                ) {
                    tracing::warn!(connector_id = %c.id, error = %error, "could not schedule Discord thread title");
                }
            }
        }
        Ok(())
    }
}
