use super::*;

impl ConnectorManager {
    pub(crate) async fn delete(&self, connector_id: String) -> Result<(), ConnectorManagerError> {
        let manager = self.clone();
        tokio::spawn(async move { manager.delete_owned(connector_id).await })
            .await
            .map_err(|_| ConnectorManagerError::WorkerStopped)?
    }

    pub(crate) async fn delete_agent(&self, agent_id: String) -> Result<(), ConnectorManagerError> {
        let manager = self.clone();
        tokio::spawn(async move {
            let _lifecycle = manager.lifecycle_lock.lock().await;
            manager.ensure_open()?;
            // New messages are refused while the deletion runs (spec §4.2).
            let _deleting = manager.runs.begin_agent_deletion(&agent_id);
            {
                let state = manager.state.read().await;
                if state.get_agent(&agent_id).is_none() {
                    return Err(ConnectorManagerError::AgentNotFound);
                }
                // Deleting mid-run would discard that run's commit (spec §4.4 item 6).
                if state.in_flight_runs(&agent_id) > 0 {
                    return Err(ConnectorManagerError::AgentBusy);
                }
                if !crate::connectors::drive::can_tombstone_agent(&state,&agent_id){return Err(ConnectorManagerError::Persistence);}
                if state.community_bundles.iter().any(|bundle|bundle.agent_id==agent_id){return Err(ConnectorManagerError::Persistence);}
            }
            let connectors = manager
                .state
                .read()
                .await
                .connectors
                .values()
                .filter(|connector| {
                    connector.agent_id == agent_id && connector.deleted_at_ms.is_none()
                })
                .cloned()
                .collect::<Vec<_>>();
            let mut previous = Vec::with_capacity(connectors.len());
            for connector in connectors {
                let token = manager
                    .credentials
                    .load(&connector.id)
                    .await
                    .map_err(map_credential_error)?;
                let status = manager
                    .statuses
                    .lock()
                    .await
                    .get(&connector.id)
                    .copied()
                    .unwrap_or_else(|| connector_operational_status(&connector));
                previous.push((connector, token, status));
            }
            for (connector, _, _) in &previous {
                manager.stop_worker(&connector.id).await?;
            }
            for (connector, _, _) in &previous {
                if let Err(error) = manager.credentials.delete(&connector.id).await {
                    let mapped = map_credential_error(error);
                    manager.restore_agent_delete_configuration(&previous).await?;
                    return Err(mapped);
                }
            }

            let _transaction = manager.mutation_lock.lock().await;
            // A run can start between the first check and this transaction; runs
            // record themselves under this same transaction, so this check is final.
            if manager.state.read().await.in_flight_runs(&agent_id) > 0 {
                manager.restore_agent_delete_configuration(&previous).await?;
                return Err(ConnectorManagerError::AgentBusy);
            }
            // Recorded in the same save as the removal below (Controller
            // ruling 2, M2 pre-flight audit; Task 6's durable-deletion
            // protocol): a crash right after a successful save still replays
            // this deletion on restart.
            if !crate::connectors::drive::can_tombstone_agent(&*manager.state.read().await,&agent_id) {
                drop(_transaction);manager.restore_agent_delete_configuration(&previous).await?;return Err(ConnectorManagerError::Persistence);
            }
            if manager.state.read().await.community_bundles.iter().any(|bundle|bundle.agent_id==agent_id) {
                drop(_transaction);manager.restore_agent_delete_configuration(&previous).await?;return Err(ConnectorManagerError::Persistence);
            }
            let (mcp, has_mcp_rows) = {
                let state = manager.state.read().await;
                (state.mcp_manager.clone(), state.mcp_registry.connections.values().any(|row| row.agent_id == agent_id))
            };
            let oauth_cleanup = match mcp {
                Some(mcp) => mcp.prepare_agent_oauth_deletion(&agent_id).await.map_err(|_| ConnectorManagerError::Persistence),
                None if has_mcp_rows => Err(ConnectorManagerError::Persistence),
                None => Ok(()),
            };
            if let Err(error) = oauth_cleanup {
                drop(_transaction);
                manager.restore_agent_delete_configuration(&previous).await?;
                return Err(error);
            }
            let deletion = HistoryDeletion::agent(&agent_id);
            let (agent_snapshot, previous_connectors, previous_inbound, previous_outbound, previous_schedules, cancelled_queued, previous_runs, previous_sessions, previous_drive, persist) = {
                let mut state = manager.state.write().await;
                let agent_snapshot = state.get_agent(&agent_id);
                let previous_connectors = previous
                    .iter()
                    .filter_map(|(connector, _, _)| {
                        state.connectors.get(&connector.id).cloned().map(|record| (record.id.clone(), record))
                    })
                    .collect::<Vec<_>>();
                let previous_inbound = state.inbound.clone();
                let previous_outbound = state.outbound.clone();
                let previous_schedules = state.schedules.clone();
                let now = now_ms();
                for (connector, _, _) in &previous {
                    if let Some(record) = state.connectors.get_mut(&connector.id) {
                        record.enabled = false;
                        record.deleted_at_ms = Some(now);
                        record.pending_pairing = None;
                        record.updated_at_ms = now;
                    }
                    state.inbound.retain(|(record_connector_id, _), record| {
                        record_connector_id != &connector.id || record.processing_state.is_terminal()
                    });
                    state.outbound.retain(|_, record| {
                        record.connector_id != connector.id || record.delivery_state.is_settled()
                    });
                    compact_delivered_outbox(&mut state.outbound, &connector.id, now);
                    for schedule in state.schedules.values_mut() {
                        if matches!(
                            &schedule.target,
                            ScheduleTarget::Connector { connector_id } if connector_id == &connector.id
                        ) {
                            schedule.enabled = false;
                            schedule.updated_at_ms = now;
                        }
                    }
                }
                state.schedules.retain(|_, schedule| schedule.agent_id != agent_id);
                let previous_drive=crate::connectors::drive::tombstone_agent_records(&mut state,&agent_id).map_err(|_|ConnectorManagerError::Persistence)?;
                state.remove_agent(&agent_id);
                // Without this, the deleted agent's terminal runs would stay
                // in the ledger until a restart: `unmirrored_terminal` skips
                // agents that no longer exist, so they would never be
                // mirrored and never pruned (Controller ruling 2, M2
                // pre-flight audit, carried forward from Task 6).
                // Its queued messages never run (spec §4.4 item 6); cancelled
                // first so they leave the ledger with its other terminal runs.
                let cancelled_queued = state.runs.cancel_queued_for_agent(&agent_id, now);
                let previous_runs = state.runs.remove_terminal_for_agent(&agent_id);
                // Without this, a deleted agent's session records stay in the
                // registry until a restart (fix round 1, M2 review).
                let previous_sessions = state.sessions.remove_for_agent(&agent_id);
                state.record_history_deletion(deletion.clone());
                let persist = state.control_plane_persist_request();
                (
                    agent_snapshot,
                    previous_connectors,
                    previous_inbound,
                    previous_outbound,
                    previous_schedules,
                    cancelled_queued,
                    previous_runs,
                    previous_sessions,
                    previous_drive,
                    persist,
                )
            };

            if persist.save().await.is_err() {
                {
                    let mut state = manager.state.write().await;
                    for (connector_id, connector) in previous_connectors {
                        state.connectors.insert(connector_id, connector);
                    }
                    state.inbound = previous_inbound;
                    state.outbound = previous_outbound;
                    state.schedules = previous_schedules;
                    for run in previous_runs {
                        state.runs.insert(run);
                    }
                    // The queued messages wait on, as they were.
                    for (queued, _) in &cancelled_queued {
                        state.runs.insert(queued.clone());
                    }
                    for session in previous_sessions {
                        state.sessions.insert(session);
                    }
                    state.clear_history_deletion(&deletion);
                    crate::connectors::drive::rollback_agent_records(&mut state,previous_drive);
                    if let Some(agent_snapshot) = agent_snapshot {
                        state
                            .restore_removed_agent(agent_snapshot)
                            .map_err(|_| ConnectorManagerError::Persistence)?;
                    }
                }
                manager.restore_agent_delete_configuration(&previous).await?;
                drop(_transaction);
                return Err(ConnectorManagerError::Persistence);
            }
            // Durable now: the cancelled messages stop waiting and are
            // announced; their controls go with them.
            {
                let mut state = manager.state.write().await;
                state.finalize_removed_agent_mcp(&agent_id);
                for (_, cancelled) in &cancelled_queued {
                    if let Some(control) = state.live.runs().control(&cancelled.id) {
                        control.cancel.cancel();
                    }
                    state.live.runs().remove(&cancelled.id);
                    state.live.publish(run_status_event(cancelled), None);
                }
                // Its open streams end, after the events above (final fix
                // wave S2-I); none can open again, the agent being gone.
                state.live.close_agent(&agent_id);
            }
            // Durable now: the history rows may go (spec §3.3).
            let history = manager.state.read().await.history.clone();
            history.enqueue_agent_deletion(&agent_id);
            // D1 (final fix wave): forget the deleted agent's ids from
            // `HistoryService`'s mirrored set, the same as a session delete
            // does (`routes::sessions::delete_session`), or an id already
            // mirrored before the delete never leaves the set.
            if let Some(snapshot) = &agent_snapshot {
                history.forget_mirrored(snapshot.messages.iter().map(|message| message.id.as_str()));
            }
            for (connector, _, _) in &previous {
                manager.statuses.lock().await.remove(&connector.id);
                manager.remove_delivery_authority(&connector.id);
            }
            let drive=manager.state.read().await.drive_manager.as_ref().and_then(|weak|weak.upgrade());
            drop(_transaction);
            if let Some(drive)=drive {drive.cleanup_deleted_agent(&agent_id).await;}
            Ok(())
        })
        .await
        .map_err(|_| ConnectorManagerError::WorkerStopped)?
    }

    async fn restore_agent_delete_configuration(
        &self,
        previous: &[(
            TelegramConnectorRecord,
            Option<TelegramBotToken>,
            ConnectorRuntimeStatus,
        )],
    ) -> Result<(), ConnectorManagerError> {
        for (connector, token, _) in previous {
            let result = match token {
                Some(token) => self.credentials.put(&connector.id, token.clone()).await,
                None => self.credentials.delete(&connector.id).await,
            };
            if result.is_err() {
                self.statuses
                    .lock()
                    .await
                    .insert(connector.id.clone(), ConnectorRuntimeStatus::Reconciling);
                return Err(ConnectorManagerError::CredentialStateUncertain);
            }
        }
        for (connector, token, status) in previous {
            if let Some(token) = token {
                self.start_worker(connector.id.clone(), token.clone())
                    .await?;
                self.statuses
                    .lock()
                    .await
                    .insert(connector.id.clone(), *status);
            } else {
                self.statuses.lock().await.insert(
                    connector.id.clone(),
                    ConnectorRuntimeStatus::CredentialRequired,
                );
            }
        }
        Ok(())
    }

    async fn delete_owned(&self, connector_id: String) -> Result<(), ConnectorManagerError> {
        let _lifecycle = self.lifecycle_lock.lock().await;
        self.ensure_open()?;
        self.delete_unlocked(connector_id).await
    }

    async fn delete_unlocked(&self, connector_id: String) -> Result<(), ConnectorManagerError> {
        let connector = {
            let state = self.state.read().await;
            state
                .connectors
                .get(&connector_id)
                .filter(|connector| connector.deleted_at_ms.is_none())
                .cloned()
                .ok_or(ConnectorManagerError::ConnectorNotFound)?
        };
        let previous_status = self
            .statuses
            .lock()
            .await
            .get(&connector_id)
            .copied()
            .unwrap_or_else(|| connector_operational_status(&connector));
        let previous_token = match self.credentials.load(&connector_id).await {
            Ok(token) => token,
            Err(error) => {
                let mapped = map_credential_error(error);
                if mapped == ConnectorManagerError::CredentialStateUncertain {
                    self.stop_worker(&connector_id).await?;
                    self.statuses
                        .lock()
                        .await
                        .insert(connector_id, ConnectorRuntimeStatus::Reconciling);
                }
                return Err(mapped);
            }
        };
        self.stop_worker(&connector_id).await?;
        let _mutation = self.mutation_lock.lock().await;

        if let Err(error) = self.credentials.delete(&connector_id).await {
            let mapped = map_credential_error(error);
            if mapped == ConnectorManagerError::CredentialStateUncertain {
                self.statuses
                    .lock()
                    .await
                    .insert(connector_id, ConnectorRuntimeStatus::Reconciling);
            } else {
                drop(_mutation);
                if let Some(previous_token) = previous_token {
                    self.start_worker(connector_id.clone(), previous_token)
                        .await?;
                    self.statuses
                        .lock()
                        .await
                        .insert(connector_id, previous_status);
                } else {
                    self.statuses
                        .lock()
                        .await
                        .insert(connector_id, ConnectorRuntimeStatus::CredentialRequired);
                }
            }
            return Err(mapped);
        }

        let (previous_connector, previous_inbound, previous_outbound, previous_schedules, persist) = {
            let mut state = self.state.write().await;
            let previous_connector = state.connectors.get(&connector_id).cloned();
            let previous_inbound = state.inbound.clone();
            let previous_outbound = state.outbound.clone();
            let previous_schedules = state.schedules.clone();
            let now = now_ms();
            let connector = state
                .connectors
                .get_mut(&connector_id)
                .expect("active connector was prevalidated");
            connector.enabled = false;
            connector.deleted_at_ms = Some(now);
            connector.pending_pairing = None;
            connector.updated_at_ms = now;
            state.inbound.retain(|(record_connector_id, _), record| {
                record_connector_id != &connector_id || record.processing_state.is_terminal()
            });
            state.outbound.retain(|_, record| {
                record.connector_id != connector_id || record.delivery_state.is_settled()
            });
            compact_delivered_outbox(&mut state.outbound, &connector_id, now);
            for schedule in state.schedules.values_mut() {
                if matches!(
                    &schedule.target,
                    ScheduleTarget::Connector { connector_id: target } if target == &connector_id
                ) {
                    schedule.enabled = false;
                    schedule.updated_at_ms = now;
                    schedule.last_safe_outcome = Some(ScheduleSafeOutcome {
                        status: ScheduleOutcomeStatus::Failed,
                        occurred_at_ms: now,
                        error_code: Some("schedule_target_unavailable".into()),
                    });
                }
            }
            let persist = state.control_plane_persist_request();
            (
                previous_connector,
                previous_inbound,
                previous_outbound,
                previous_schedules,
                persist,
            )
        };

        if persist.save().await.is_err() {
            let mut state = self.state.write().await;
            if let Some(connector) = previous_connector {
                state.connectors.insert(connector_id.clone(), connector);
            }
            state.inbound = previous_inbound;
            state.outbound = previous_outbound;
            state.schedules = previous_schedules;
            drop(state);
            let Some(previous_token) = previous_token else {
                self.statuses
                    .lock()
                    .await
                    .insert(connector_id, ConnectorRuntimeStatus::CredentialRequired);
                return Err(ConnectorManagerError::Persistence);
            };
            match self
                .credentials
                .put(&connector_id, previous_token.clone())
                .await
            {
                Ok(()) => {
                    drop(_mutation);
                    self.start_worker(connector_id.clone(), previous_token)
                        .await?;
                    self.statuses
                        .lock()
                        .await
                        .insert(connector_id, previous_status);
                    return Err(ConnectorManagerError::Persistence);
                }
                Err(_) => {
                    self.statuses
                        .lock()
                        .await
                        .insert(connector_id, ConnectorRuntimeStatus::Reconciling);
                    return Err(ConnectorManagerError::CredentialStateUncertain);
                }
            }
        }
        self.statuses.lock().await.remove(&connector_id);
        self.remove_delivery_authority(&connector_id);
        Ok(())
    }
}
