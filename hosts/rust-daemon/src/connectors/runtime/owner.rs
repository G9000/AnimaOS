use super::*;

impl ConnectorManager {
    pub(crate) async fn send_from_owner(
        &self,
        agent_id: String,
        connector_id: String,
        text: String,
        idempotency_key: String,
    ) -> Result<(crate::routes::AgentRunEnvelope, bool), ConnectorManagerError> {
        let manager = self.clone();
        tokio::spawn(async move {
            manager
                .send_from_owner_owned(agent_id, connector_id, text, idempotency_key, None)
                .await
        })
        .await
        .map_err(|_| ConnectorManagerError::WorkerStopped)?
    }

    /// A Telegram session's owner turn accepted as run `run_id` by the
    /// session runs route (spec §4.2): the same flow, without the fail-fast
    /// waiting budget (the accepted queue has its own cap) and without the
    /// transcript replay (acceptance already checked the key in the ledger).
    pub(crate) async fn send_from_owner_accepted(
        &self,
        agent_id: String,
        connector_id: String,
        text: String,
        idempotency_key: String,
        run_id: String,
    ) -> Result<(), ConnectorManagerError> {
        let manager = self.clone();
        tokio::spawn(async move {
            manager
                .send_from_owner_owned(agent_id, connector_id, text, idempotency_key, Some(run_id))
                .await
                .map(|_| ())
        })
        .await
        .map_err(|_| ConnectorManagerError::WorkerStopped)?
    }

    async fn send_from_owner_owned(
        &self,
        agent_id: String,
        connector_id: String,
        text: String,
        idempotency_key: String,
        accepted: Option<String>,
    ) -> Result<(crate::routes::AgentRunEnvelope, bool), ConnectorManagerError> {
        let owner_send_lock = self.owner_send_lock(&connector_id, &idempotency_key);
        let _owner_send_guard = owner_send_lock.lock().await;
        let (connector, connector_worker_generation, replay) = {
            let _lifecycle = self.lifecycle_lock.lock().await;
            self.ensure_open()?;
            let state = self.state.read().await;
            let connector = state
                .connectors
                .get(&connector_id)
                .filter(|connector| connector.is_active() && connector.agent_id == agent_id)
                .cloned()
                .ok_or(ConnectorManagerError::ConnectorNotFound)?;
            if connector.approved_chat.is_some()
                && state
                    .outbound
                    .values()
                    .filter(|record| {
                        record.connector_id == connector_id && !record.delivery_state.is_settled()
                    })
                    .count()
                    >= MAX_UNDELIVERED_OUTBOUND
            {
                return Err(ConnectorManagerError::Backpressure);
            }
            let worker_generation = self
                .workers
                .lock()
                .await
                .get(&connector_id)
                .map(|worker| worker.generation);
            let replay = if accepted.is_some() {
                OwnerSendReplay::New
            } else {
                owner_send_replay(&state, &connector, &text, &idempotency_key)?
            };
            (connector, worker_generation, replay)
        };
        match replay {
            OwnerSendReplay::New => {}
            OwnerSendReplay::Answered(envelope, delivery_queued) => {
                return Ok((envelope, delivery_queued));
            }
            // Read with the state and lifecycle locks released.
            OwnerSendReplay::FromHistory {
                snapshot,
                session_id,
                reply_id,
                delivery_queued,
                store,
            } => {
                let reply = match store
                    .get_message(&connector.agent_id, &session_id, &reply_id)
                    .await
                {
                    Ok(Some(reply)) => reply.message.content,
                    // Never run the turn again: the caller retries later.
                    Ok(None) | Err(_) => return Err(ConnectorManagerError::Persistence),
                };
                return Ok((
                    replay_envelope(&snapshot, TaskResult::success(reply, 0)),
                    delivery_queued,
                ));
            }
        }
        // Only a unit of the agent's waiting budget is reserved here, without
        // waiting (spec §16). The run then waits for the Telegram room, an
        // agent slot, and a global permit, in that order (spec §4.3), and
        // gives the unit back once it holds the permit.
        // An accepted send is counted by the accepted queue's cap instead.
        let waiting = if accepted.is_some() {
            None
        } else {
            if !self.runs.has_available_permit() {
                return Err(ConnectorManagerError::Backpressure);
            }
            Some(
                self.runs
                    .try_take_waiting_unit(&connector.agent_id)
                    .ok_or(ConnectorManagerError::Backpressure)?,
            )
        };
        let commit_connector_id = connector.id.clone();
        let commit_agent_id = connector.agent_id.clone();
        let commit_room_id = connector.room_id.clone();
        let commit_chat_id = connector.approved_chat.as_ref().map(|chat| chat.id.clone());
        let delivery_queued = commit_chat_id.is_some();
        let rollback_outbound = Arc::new(std::sync::Mutex::new(None::<TelegramOutboundRecord>));
        let commit_rollback_outbound = Arc::clone(&rollback_outbound);
        let commit_lifecycle_lock = Arc::clone(&self.lifecycle_lock);
        let commit_workers = Arc::clone(&self.workers);
        let request = AgentRunRequest {
            agent_id: connector.agent_id.clone(),
            content: attachments::owner_content(
                &self.state,
                &connector.agent_id,
                &connector.room_id,
                accepted.as_deref(),
                text,
                BTreeMap::from([
                    ("source".into(), DataValue::String("telegramThread".into())),
                    (
                        "connectorId".into(),
                        DataValue::String(connector.id.clone()),
                    ),
                ]),
            )
            .await?,
            room: RunRoom::Stable(connector.room_id.clone()),
            idempotency_key: Some(idempotency_key),
            source: RunSource::Telegram,
            source_ref: Some(connector.id.clone()),
            parent: None,
        };
        let commit = move |state: &mut DaemonState, outcome: &RunOutcome| {
            let _lifecycle = commit_lifecycle_lock.try_lock().map_err(|_| {
                ApiError::service_unavailable("connector lifecycle changed during run")
            })?;
            let workers = commit_workers.try_lock().map_err(|_| {
                ApiError::service_unavailable("connector worker changed during run")
            })?;
            if workers
                .get(&commit_connector_id)
                .map(|worker| worker.generation)
                != connector_worker_generation
            {
                return Err(ApiError::service_unavailable(
                    "connector worker changed during run",
                ));
            }
            let connector_unchanged =
                state
                    .connectors
                    .get(&commit_connector_id)
                    .is_some_and(|current| {
                        current.is_active()
                            && current.agent_id == commit_agent_id
                            && current.room_id == commit_room_id
                            && current.approved_chat.as_ref().map(|chat| &chat.id)
                                == commit_chat_id.as_ref()
                    });
            if !connector_unchanged {
                return Err(ApiError::not_found());
            }
            if outcome.result.status == TaskStatus::Error || commit_chat_id.is_none() {
                return Ok(());
            }
            if state
                .outbound
                .values()
                .filter(|record| {
                    record.connector_id == commit_connector_id
                        && !record.delivery_state.is_settled()
                })
                .count()
                >= MAX_UNDELIVERED_OUTBOUND
            {
                return Err(ApiError::service_unavailable(
                    "connector outbound capacity is exhausted",
                ));
            }
            let (Some(reply_id), Some(reply)) = (
                outcome.reply_message_id.clone(),
                outcome.result.data.as_ref(),
            ) else {
                return Err(ApiError::bad_request("agent produced no assistant message"));
            };
            let outbound_id = format!("telegram:{}:web:{}:outbound", commit_connector_id, reply_id);
            // A stop saved while the run finished anyway: the reply is
            // kept but never sent (spec §4.6).
            let stopped = state
                .runs
                .get(&outcome.run_id)
                .is_some_and(|record| record.stop.is_some());
            let outbound = TelegramOutboundRecord {
                id: outbound_id.clone(),
                connector_id: commit_connector_id.clone(),
                agent_id: commit_agent_id.clone(),
                room_id: commit_room_id.clone(),
                assistant_message_id: reply_id,
                text: reply.text.clone(),
                created_at_ms: now_ms(),
                delivered_at_ms: None,
                attempts: 0,
                delivery_state: if stopped {
                    OutboundDeliveryState::Suppressed
                } else {
                    OutboundDeliveryState::Pending
                },
                message_pruned: false,
            };
            if let Some(existing) = state.outbound.get(&outbound_id) {
                if existing != &outbound {
                    return Err(ApiError::bad_request(
                        "connector outbound conflicts with run",
                    ));
                }
            } else {
                state.outbound.insert(outbound_id, outbound.clone());
                *commit_rollback_outbound
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(outbound);
            }
            Ok(())
        };
        let rollback = move |state: &mut DaemonState| {
            if let Some(inserted) = rollback_outbound
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .as_ref()
            {
                if state.outbound.get(&inserted.id) == Some(inserted) {
                    state.outbound.remove(&inserted.id);
                }
            }
            Ok(())
        };
        let run = match accepted {
            Some(run_id) => {
                self.runs
                    .run_accepted_with_commit(request, run_id, commit, rollback)
                    .await
            }
            None => {
                let waiting = waiting.expect("a direct owner send holds a waiting unit");
                self.runs
                    .run_budgeted_with_commit_waiting(request, waiting, commit, rollback)
                    .await
            }
        }
        .map_err(|error| {
            // An accepted turn whose wait shutdown ended never started
            // (final fix wave S2-A): its session queue leaves it queued.
            if crate::agent_runs::is_shutting_down(&error) {
                ConnectorManagerError::WorkerStopped
            } else {
                ConnectorManagerError::Persistence
            }
        })?;
        let delivery_queued = delivery_queued && run.result.status == "success";
        Ok((run, delivery_queued))
    }
}
