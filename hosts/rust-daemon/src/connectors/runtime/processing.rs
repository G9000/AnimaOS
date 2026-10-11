use super::*;

impl ConnectorManager {
    pub(super) async fn expire_pending_pairing_at(
        &self,
        connector_id: &str,
        now: u64,
    ) -> Result<bool, ConnectorManagerError> {
        let stale = {
            let state = self.state.read().await;
            let connector = state
                .connectors
                .get(connector_id)
                .filter(|connector| connector.is_active())
                .ok_or(ConnectorManagerError::ConnectorNotFound)?;
            connector.pending_pairing.as_ref().is_some_and(|pending| {
                now.saturating_sub(pending.requested_at_ms) > PAIRING_CANDIDATE_TTL_MS
            })
        };
        if !stale {
            return Ok(false);
        }

        let _mutation = self.mutation_lock.lock().await;
        let transaction = {
            let mut state = self.state.write().await;
            let connector = state
                .connectors
                .get(connector_id)
                .filter(|connector| connector.is_active())
                .cloned()
                .ok_or(ConnectorManagerError::ConnectorNotFound)?;
            if !connector.pending_pairing.as_ref().is_some_and(|pending| {
                now.saturating_sub(pending.requested_at_ms) > PAIRING_CANDIDATE_TTL_MS
            }) {
                return Ok(false);
            }
            let target = state
                .connectors
                .get_mut(connector_id)
                .expect("connector was prevalidated");
            target.pending_pairing = None;
            target.updated_at_ms = now;
            Some((connector, state.control_plane_persist_request()))
        };
        let Some((previous, persist)) = transaction else {
            return Ok(false);
        };
        if persist.save().await.is_err() {
            self.state
                .write()
                .await
                .connectors
                .insert(connector_id.to_string(), previous);
            self.statuses
                .lock()
                .await
                .insert(connector_id.to_string(), ConnectorRuntimeStatus::Error);
            return Err(ConnectorManagerError::Persistence);
        }
        self.refresh_operational_status(connector_id).await;
        Ok(true)
    }

    pub(crate) async fn process_pending_once(
        &self,
        connector_id: String,
    ) -> Result<bool, ConnectorManagerError> {
        let manager = self.clone();
        tokio::spawn(async move { manager.process_pending_once_owned(connector_id).await })
            .await
            .map_err(|_| ConnectorManagerError::WorkerStopped)?
    }

    async fn process_pending_once_owned(
        &self,
        connector_id: String,
    ) -> Result<bool, ConnectorManagerError> {
        let inbound = {
            let _mutation = self.mutation_lock.lock().await;
            let (key, previous, persist) = {
                let mut state = self.state.write().await;
                let connector = state
                    .connectors
                    .get(&connector_id)
                    .filter(|connector| connector.is_active())
                    .ok_or(ConnectorManagerError::ConnectorNotFound)?;
                if connector.approved_chat.is_none() {
                    return Ok(false);
                }
                if state
                    .outbound
                    .values()
                    .filter(|record| {
                        record.connector_id == connector_id && !record.delivery_state.is_settled()
                    })
                    .count()
                    >= MAX_UNDELIVERED_OUTBOUND
                {
                    drop(state);
                    self.statuses
                        .lock()
                        .await
                        .insert(connector_id, ConnectorRuntimeStatus::Error);
                    return Ok(false);
                }
                let key = state
                    .inbound
                    .iter()
                    .filter(|((record_connector_id, _), record)| {
                        record_connector_id == &connector_id
                            && matches!(
                                record.processing_state,
                                InboundProcessingState::Received
                                    | InboundProcessingState::Processing
                            )
                    })
                    .min_by_key(|(_, record)| (record.received_at_ms, record.update_id))
                    .map(|(key, _)| key.clone());
                let Some(key) = key else {
                    return Ok(false);
                };
                let previous = state.inbound.get(&key).cloned().expect("key came from map");
                state
                    .inbound
                    .get_mut(&key)
                    .expect("key came from map")
                    .processing_state = InboundProcessingState::Processing;
                let persist = state.control_plane_persist_request();
                (key, previous, persist)
            };
            if persist.save().await.is_err() {
                self.state.write().await.inbound.insert(key, previous);
                self.statuses
                    .lock()
                    .await
                    .insert(connector_id, ConnectorRuntimeStatus::Error);
                return Err(ConnectorManagerError::Persistence);
            }
            self.state
                .read()
                .await
                .inbound
                .get(&key)
                .cloned()
                .expect("processing transition was persisted")
        };

        let key = (inbound.connector_id.clone(), inbound.update_id);
        let commit_connector_id = inbound.connector_id.clone();
        let commit_agent_id = inbound.agent_id.clone();
        let commit_room_id = inbound.room_id.clone();
        let commit_key = key.clone();
        let outbound_id = format!(
            "telegram:{}:update:{}:outbound",
            inbound.connector_id, inbound.update_id
        );
        let previous_outbound = {
            let state = self.state.read().await;
            state.outbound.get(&outbound_id).cloned()
        };
        let rollback_delta = Arc::new(std::sync::Mutex::new(ConnectorRunRollbackDelta::default()));
        let commit_rollback_delta = Arc::clone(&rollback_delta);
        let commit_outbound_id = outbound_id.clone();
        let rollback_target = inbound.clone();
        let rollback_key = key.clone();
        let rollback_outbound_id = outbound_id.clone();
        let request = AgentRunRequest {
            agent_id: inbound.agent_id.clone(),
            content: Content {
                text: inbound.normalized_text.clone(),
                metadata: Some(BTreeMap::from([
                    ("source".into(), DataValue::String("telegram".into())),
                    (
                        "connectorId".into(),
                        DataValue::String(inbound.connector_id.clone()),
                    ),
                    (
                        "updateId".into(),
                        DataValue::Number(inbound.update_id as f64),
                    ),
                ])),
                attachments: None,
            },
            room: RunRoom::Stable(inbound.room_id.clone()),
            idempotency_key: Some(inbound.run_idempotency_key.clone()),
            source: RunSource::Telegram,
            source_ref: Some(format!("{}:{}", inbound.connector_id, inbound.update_id)),
            parent: None,
        };

        let run = self
            .runs
            .run_with_commit_waiting(
                request,
                move |state, outcome| {
                    let current = state
                        .inbound
                        .get(&commit_key)
                        .ok_or_else(|| ApiError::bad_request("durable inbound disappeared"))?;
                    if current.agent_id != commit_agent_id || current.room_id != commit_room_id {
                        return Err(ApiError::bad_request("durable inbound changed during run"));
                    }
                    match current.processing_state {
                        // The owner stopped this turn (spec §4.6): a finish with
                        // no reply, never run again.
                        InboundProcessingState::Stopped => return Ok(()),
                        InboundProcessingState::Processing => {}
                        _ => {
                            return Err(ApiError::bad_request("durable inbound changed during run"))
                        }
                    }
                    if outcome.status == RunStatus::Cancelled {
                        let target = state
                            .inbound
                            .get_mut(&commit_key)
                            .expect("inbound was prevalidated");
                        target.processing_state = InboundProcessingState::Stopped;
                        commit_rollback_delta
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                            .committed_target = Some(target.clone());
                        return Ok(());
                    }

                    if outcome.result.status == TaskStatus::Error {
                        let target = state
                            .inbound
                            .get_mut(&commit_key)
                            .expect("inbound was prevalidated");
                        target.processing_state = InboundProcessingState::Rejected;
                        commit_rollback_delta
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                            .committed_target = Some(target.clone());
                        return Ok(());
                    }

                    let (Some(reply_id), Some(reply)) = (
                        outcome.reply_message_id.clone(),
                        outcome.result.data.as_ref(),
                    ) else {
                        return Err(ApiError::bad_request("agent produced no assistant message"));
                    };
                    let candidate = TelegramOutboundRecord {
                        id: commit_outbound_id.clone(),
                        connector_id: commit_connector_id.clone(),
                        agent_id: commit_agent_id.clone(),
                        room_id: commit_room_id.clone(),
                        assistant_message_id: reply_id,
                        text: reply.text.clone(),
                        created_at_ms: now_ms(),
                        delivered_at_ms: None,
                        attempts: 0,
                        delivery_state: OutboundDeliveryState::Pending,
                        message_pruned: false,
                    };
                    if let Some(existing) = state.outbound.get(&commit_outbound_id) {
                        if existing.connector_id != candidate.connector_id
                            || existing.agent_id != candidate.agent_id
                            || existing.room_id != candidate.room_id
                            || existing.assistant_message_id != candidate.assistant_message_id
                            || existing.text != candidate.text
                        {
                            return Err(ApiError::bad_request(
                                "durable outbound conflicts with run",
                            ));
                        }
                    }

                    let target = state
                        .inbound
                        .get_mut(&commit_key)
                        .expect("inbound was prevalidated");
                    target.processing_state = InboundProcessingState::Processed;
                    let committed_target = target.clone();
                    let inserted_outbound = if state.outbound.contains_key(&commit_outbound_id) {
                        None
                    } else {
                        state
                            .outbound
                            .insert(commit_outbound_id.clone(), candidate.clone());
                        Some(candidate)
                    };
                    let removed_terminal =
                        compact_terminal_inbound(&mut state.inbound, &commit_connector_id);
                    let mut delta = commit_rollback_delta
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    delta.committed_target = Some(committed_target);
                    delta.inserted_outbound = inserted_outbound;
                    delta.removed_terminal = removed_terminal;
                    Ok(())
                },
                move |state| {
                    let delta = rollback_delta
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    for (removed_key, removed_record) in &delta.removed_terminal {
                        state
                            .inbound
                            .entry(removed_key.clone())
                            .or_insert_with(|| removed_record.clone());
                    }
                    if delta.committed_target.as_ref().is_some_and(|committed| {
                        state.inbound.get(&rollback_key) == Some(committed)
                    }) {
                        state.inbound.insert(rollback_key.clone(), rollback_target);
                    }
                    if delta.inserted_outbound.as_ref().is_some_and(|inserted| {
                        state.outbound.get(&rollback_outbound_id) == Some(inserted)
                    }) {
                        state.outbound.remove(&rollback_outbound_id);
                    } else if let Some(previous) = previous_outbound {
                        state
                            .outbound
                            .entry(rollback_outbound_id)
                            .or_insert(previous);
                    }
                    Ok(())
                },
            )
            .await;
        if run.is_err() {
            self.statuses
                .lock()
                .await
                .insert(connector_id, ConnectorRuntimeStatus::Error);
            return Err(ConnectorManagerError::Persistence);
        }
        Ok(true)
    }
}
