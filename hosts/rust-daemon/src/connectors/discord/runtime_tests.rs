//! Native runtime regressions use injected transport and vault; never live Discord.
use super::*;
#[test]
fn uncertainty_review_retains_receipts_and_audit_and_only_suppresses_unsent_tail() {
    let connector = fixture_connector();
    let mut record = outbound_for(&connector, "reply", "hello", false);
    record.chunks[0].state = DiscordDeliveryState::Delivered;
    record.chunks[0].provider_message_id = Some("9007199254740993".into());
    let mut unknown = record.chunks[0].clone();
    unknown.index = 1;
    unknown.state = DiscordDeliveryState::Unknown;
    unknown.provider_message_id = None;
    record.chunks.push(unknown);
    let mut tail = record.chunks[0].clone();
    tail.index = 2;
    tail.state = DiscordDeliveryState::Pending;
    tail.provider_message_id = None;
    record.chunks.push(tail);
    record.delivery_state = DiscordDeliveryState::Unknown;
    assert!(suppress_unknown_record(&mut record));
    assert_eq!(record.delivery_state, DiscordDeliveryState::Suppressed);
    assert_eq!(
        record.chunks[0].provider_message_id.as_deref(),
        Some("9007199254740993")
    );
    assert_eq!(record.chunks[1].state, DiscordDeliveryState::Unknown);
    assert_eq!(record.chunks[2].state, DiscordDeliveryState::Suppressed);
}
#[test]
fn restore_sending_preserves_delivered_prefix_and_never_returns_pending() {
    let connector = fixture_connector();
    let mut record = outbound_for(&connector, "reply", "hello", false);
    record.chunks[0].state = DiscordDeliveryState::Sending;
    record.delivery_state = DiscordDeliveryState::Sending;
    recover_outbound(&mut record);
    assert_eq!(record.chunks[0].state, DiscordDeliveryState::Unknown);
    assert_eq!(record.delivery_state, DiscordDeliveryState::Unknown);
    assert!(!record.delivery_state.awaits_delivery());
}
fn fixture_connector() -> DiscordConnectorRecord {
    DiscordConnectorRecord {
        id: "discord-fixture".into(),
        agent_id: "agent".into(),
        room_id: "discord:discord-fixture".into(),
        bot: DiscordBotIdentity {
            id: "11".into(),
            username: None,
            display_name: None,
        },
        channel: DiscordChannelMetadata {
            id: "22".into(),
            guild_id: "33".into(),
            name: None,
            kind: DiscordChannelKind::GuildText,
        },
        approved_user_ids: vec!["44".into()],
        credential_generation: 1,
        cursor: "0".into(),
        activated_at_ms: Some(1),
        enabled: true,
        deleted_at_ms: None,
        created_at_ms: 1,
        updated_at_ms: 1,
    }
}

use std::collections::VecDeque;
use std::sync::atomic::AtomicU64;
use tokio::sync::{RwLock, Semaphore};
#[derive(Default)]
struct FakeTransport {
    sent_chunks: StdMutex<Vec<(String, String)>>,
    typing_calls: AtomicUsize,
    typing_inflight: AtomicUsize,
    typing_gate: AtomicBool,
    typing_entered: SemaphoreHolder,
    typing_release: SemaphoreHolder,
    typing_results: StdMutex<VecDeque<Result<(), DiscordTransportError>>>,
    latest: AtomicU64,
    baseline: StdMutex<Option<String>>,
    sends: AtomicUsize,
    gate: AtomicBool,
    poll_calls: AtomicUsize,
    poll_error: StdMutex<Option<DiscordTransportError>>,
    entered: SemaphoreHolder,
    release: SemaphoreHolder,
    results: StdMutex<VecDeque<Result<DiscordSendReceipt, DiscordTransportError>>>,
}
struct SemaphoreHolder(Semaphore);
impl Default for SemaphoreHolder {
    fn default() -> Self {
        Self(Semaphore::new(0))
    }
}
#[async_trait::async_trait]
impl DiscordTransport for FakeTransport {
    async fn send_typing(&self, _: &DiscordBotToken, _: &str) -> Result<(), DiscordTransportError> {
        struct InFlight<'a>(&'a AtomicUsize);
        impl Drop for InFlight<'_> {
            fn drop(&mut self) {
                self.0.fetch_sub(1, Ordering::SeqCst);
            }
        }
        self.typing_calls.fetch_add(1, Ordering::SeqCst);
        self.typing_inflight.fetch_add(1, Ordering::SeqCst);
        let _inflight = InFlight(&self.typing_inflight);
        self.typing_entered.0.add_permits(1);
        if self.typing_gate.load(Ordering::SeqCst) {
            self.typing_release.0.acquire().await.unwrap().forget();
        }
        self.typing_results
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Ok(()))
    }
    async fn get_me(
        &self,
        _: &DiscordBotToken,
    ) -> Result<DiscordBotIdentity, DiscordTransportError> {
        Ok(fixture_connector().bot)
    }
    async fn get_channel(
        &self,
        _: &DiscordBotToken,
        channel: &str,
    ) -> Result<DiscordChannelMetadata, DiscordTransportError> {
        Ok(DiscordChannelMetadata {
            id: channel.into(),
            ..fixture_connector().channel
        })
    }
    async fn latest_cursor(
        &self,
        _: &DiscordBotToken,
        _: &str,
    ) -> Result<String, DiscordTransportError> {
        Ok(self
            .baseline
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(|| self.latest.load(Ordering::SeqCst).to_string()))
    }
    async fn receive(
        &self,
        _: &DiscordBotToken,
        _: &str,
        cursor: &str,
    ) -> Result<DiscordMessageBatch, DiscordTransportError> {
        self.poll_calls.fetch_add(1, Ordering::SeqCst);
        if let Some(error) = self.poll_error.lock().unwrap().take() {
            return Err(error);
        }
        Ok(DiscordMessageBatch {
            messages: vec![],
            next_cursor: cursor.into(),
            boundary_proven: true,
        })
    }
    async fn send_chunk(
        &self,
        _: &DiscordBotToken,
        channel: &str,
        text: &str,
        _: &str,
    ) -> Result<DiscordSendReceipt, DiscordTransportError> {
        self.sent_chunks
            .lock()
            .unwrap()
            .push((channel.into(), text.into()));
        self.sends.fetch_add(1, Ordering::SeqCst);
        self.entered.0.add_permits(1);
        if self.gate.load(Ordering::SeqCst) {
            self.release.0.acquire().await.unwrap().forget();
        }
        self.results.lock().unwrap().pop_front().unwrap_or_else(|| {
            Ok(DiscordSendReceipt {
                message_id: "9007199254740993".into(),
                channel_id: channel.into(),
            })
        })
    }
}
async fn setup() -> (DiscordManager, Arc<FakeTransport>, DiscordConnectorRecord) {
    let mut state = DaemonState::new();
    let agent = state
        .create_agent(anima_core::AgentConfig {
            name: "Anima".into(),
            model: "deterministic".into(),
            bio: None,
            lore: None,
            knowledge: None,
            topics: None,
            adjectives: None,
            style: None,
            provider: None,
            system: None,
            tools: None,
            plugins: None,
            settings: Some(anima_core::AgentSettings::default()),
            fallbacks: Vec::new(),
        })
        .unwrap();
    let mut c = fixture_connector();
    c.agent_id = agent.state.id;
    state.discord_connectors.insert(c.id.clone(), c.clone());
    state.ensure_run_session(crate::state::RunSessionRequest {
        agent_id: &c.agent_id,
        room_id: &c.room_id,
        source: crate::runs::RunSource::Discord,
        system_followup_title: None,
        delegated_parent: None,
        peer_sender: None,
        parent: None,
        first_text: "",
        now_ms: 1,
    });
    let state = Arc::new(RwLock::new(state));
    let runs = AgentRunCoordinator::new(state.clone(), Arc::new(Semaphore::new(4)));
    let transport = Arc::new(FakeTransport::default());
    let credentials = Arc::new(credentials::InMemoryDiscordCredentialStore::default());
    credentials
        .put(
            &c.id,
            1,
            &c.bot.id,
            &DiscordBotToken::parse("test.token").unwrap(),
        )
        .await
        .unwrap();
    let manager = DiscordManager::new(
        state,
        runs,
        credentials,
        Arc::new(ConnectorRegistry::with_discord(
            Arc::new(crate::app::DeterministicTelegramTransport),
            transport.clone(),
        )),
    );
    (manager, transport, c)
}
fn incoming(c: &DiscordConnectorRecord, id: &str) -> DiscordMessage {
    DiscordMessage {
        id: id.into(),
        channel_id: c.channel.id.clone(),
        guild_id: Some(c.channel.guild_id.clone()),
        sender: DiscordSenderMetadata {
            id: c.approved_user_ids[0].clone(),
            username: None,
            display_name: None,
        },
        text: "hello Anima".into(),
        is_bot: false,
        webhook_id: None,
        message_type: 0,
    }
}
async fn within<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(std::time::Duration::from_secs(3), future)
        .await
        .expect("fixture completed within bound")
}
#[tokio::test]
async fn admission_save_failure_keeps_cursor_and_receipts_atomic_after_caller_cancellation() {
    let (manager, _, c) = setup().await;
    let gate = manager
        .state
        .write()
        .await
        .install_test_control_plane_save_gate(true);
    let job = tokio::spawn({
        let manager = manager.clone();
        let c = c.clone();
        async move {
            manager
                .accept_batch(
                    c.id.clone(),
                    DiscordMessageBatch {
                        messages: vec![incoming(&c, "9007199254740993")],
                        next_cursor: "9007199254740993".into(),
                        boundary_proven: true,
                    },
                )
                .await
        }
    });
    within(gate.entered.acquire()).await.unwrap().forget();
    job.abort();
    gate.release.add_permits(1);
    within(async {
        loop {
            if manager.owned_count.load(Ordering::SeqCst) == 0 {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    let state = manager.state.read().await;
    assert_eq!(state.discord_connectors[&c.id].cursor, "0");
    assert!(state.discord_inbound.is_empty());
    assert!(state.runs.active_records().is_empty());
}
#[tokio::test]
async fn admission_filters_native_authority_and_duplicate_replay_without_model_effects() {
    let (manager, _, c) = setup().await;
    let first = incoming(&c, "9007199254740993");
    let mut bot = incoming(&c, "9007199254740994");
    bot.is_bot = true;
    let mut webhook = incoming(&c, "9007199254740995");
    webhook.webhook_id = Some("555".into());
    let mut stranger = incoming(&c, "9007199254740996");
    stranger.sender.id = "77".into();
    let batch = DiscordMessageBatch {
        messages: vec![first.clone(), first, bot, webhook, stranger],
        next_cursor: "9007199254740996".into(),
        boundary_proven: true,
    };
    manager
        .accept_batch(c.id.clone(), batch.clone())
        .await
        .unwrap();
    manager.accept_batch(c.id.clone(), batch).await.unwrap();
    let state = manager.state.read().await;
    assert_eq!(state.discord_inbound.len(), 1);
    assert_eq!(state.discord_connectors[&c.id].cursor, "9007199254740996");
    assert!(state.runs.active_records().is_empty());
}
#[tokio::test]
async fn sending_is_durable_before_effect_and_revocation_drains_without_later_chunks() {
    let (manager, transport, c) = setup().await;
    transport.gate.store(true, Ordering::SeqCst);
    let record = outbound_for(&c, "reply", &"x".repeat(2100), false);
    let output = record.id.clone();
    manager
        .state
        .write()
        .await
        .discord_outbound
        .insert(output.clone(), record);
    let delivering = tokio::spawn({
        let manager = manager.clone();
        let id = c.id.clone();
        async move { manager.deliver_one(id).await }
    });
    within(transport.entered.0.acquire())
        .await
        .unwrap()
        .forget();
    assert_eq!(
        manager.state.read().await.discord_outbound[&output].chunks[0].state,
        DiscordDeliveryState::Sending
    );
    let disabling = tokio::spawn({
        let manager = manager.clone();
        let c = c.clone();
        async move { manager.set_enabled(c.agent_id, c.id, false).await }
    });
    within(async {
        loop {
            if manager.epoch(&c.id) > 0 {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    assert!(!disabling.is_finished());
    transport.release.0.add_permits(1);
    within(delivering).await.unwrap().unwrap();
    within(disabling).await.unwrap().unwrap();
    assert_eq!(transport.sends.load(Ordering::SeqCst), 1);
    let state = manager.state.read().await;
    assert_eq!(
        state.discord_outbound[&output].delivery_state,
        DiscordDeliveryState::Unknown
    );
    assert_eq!(
        state.discord_outbound[&output].chunks[1].state,
        DiscordDeliveryState::Pending
    );
}
#[tokio::test]
async fn unknown_never_retries_and_explicit_review_preserves_prefix_and_unblocks_new_work() {
    let (manager, transport, c) = setup().await;
    let first = outbound_for(&c, "reply", &"x".repeat(2100), false);
    let first_id = first.id.clone();
    manager
        .state
        .write()
        .await
        .discord_outbound
        .insert(first.id.clone(), first);
    transport
        .results
        .lock()
        .unwrap()
        .push_back(Ok(DiscordSendReceipt {
            message_id: "100".into(),
            channel_id: c.channel.id.clone(),
        }));
    transport
        .results
        .lock()
        .unwrap()
        .push_back(Err(DiscordTransportError::Uncertain));
    manager.deliver_one(c.id.clone()).await.unwrap();
    manager.deliver_one(c.id.clone()).await.unwrap();
    assert_eq!(
        manager
            .retry(c.agent_id.clone(), c.id.clone())
            .await
            .unwrap_err(),
        DiscordManagerError::UnknownDelivery
    );
    manager.deliver_one(c.id.clone()).await.unwrap();
    assert_eq!(transport.sends.load(Ordering::SeqCst), 2);
    manager
        .suppress_unknown(c.agent_id.clone(), c.id.clone())
        .await
        .unwrap();
    let state = manager.state.read().await;
    assert_eq!(
        state.discord_outbound[&first_id].chunks[0]
            .provider_message_id
            .as_deref(),
        Some("100")
    );
    assert_eq!(
        state.discord_outbound[&first_id].chunks[1].state,
        DiscordDeliveryState::Unknown
    );
    drop(state);
    let second = outbound_for(&c, "new-reply", "new work", false);
    manager
        .state
        .write()
        .await
        .discord_outbound
        .insert(second.id.clone(), second);
    manager.deliver_one(c.id.clone()).await.unwrap();
    assert_eq!(transport.sends.load(Ordering::SeqCst), 3);
}
#[tokio::test]
async fn rate_deadline_survives_retry_and_snapshot_restore() {
    let (manager, transport, c) = setup().await;
    let output = outbound_for(&c, "reply", "rate limited", false);
    let output_id = output.id.clone();
    manager
        .state
        .write()
        .await
        .discord_outbound
        .insert(output.id.clone(), output);
    transport
        .results
        .lock()
        .unwrap()
        .push_back(Err(DiscordTransportError::RateLimited {
            retry_after_ms: 60_000,
            global: true,
        }));
    manager.deliver_one(c.id.clone()).await.unwrap();
    let deadline = manager.state.read().await.discord_outbound[&output_id].chunks[0]
        .retry_at_ms
        .unwrap();
    manager
        .retry(c.agent_id.clone(), c.id.clone())
        .await
        .unwrap();
    assert_eq!(
        manager.state.read().await.discord_outbound[&output_id].chunks[0].retry_at_ms,
        Some(deadline)
    );
    manager.deliver_one(c.id.clone()).await.unwrap();
    assert_eq!(transport.sends.load(Ordering::SeqCst), 1);
    let snapshot = manager.state.read().await.control_plane_snapshot();
    let mut restored = DaemonState::new();
    restored.restore_control_plane_snapshot(snapshot).unwrap();
    assert_eq!(
        restored.discord_outbound[&output_id].chunks[0].retry_at_ms,
        Some(deadline)
    );
}
#[tokio::test]
async fn first_empty_activation_and_reactivation_keep_the_durable_boundary() {
    let (manager, transport, c) = setup().await;
    {
        let mut state = manager.state.write().await;
        let c = state.discord_connectors.get_mut(&c.id).unwrap();
        c.enabled = false;
        c.activated_at_ms = None;
    }
    let first = manager
        .activate(c.agent_id.clone(), c.id.clone())
        .await
        .unwrap();
    assert_eq!(first.cursor, "0");
    assert!(first.activated_at_ms.is_some());
    manager
        .set_enabled(c.agent_id.clone(), c.id.clone(), false)
        .await
        .unwrap();
    transport.latest.store(999, Ordering::SeqCst);
    let again = manager
        .activate(c.agent_id.clone(), c.id.clone())
        .await
        .unwrap();
    assert_eq!(again.cursor, "0");
    assert_eq!(again.activated_at_ms, first.activated_at_ms);
    manager.shutdown().await;
}
#[tokio::test]
async fn public_clone_drop_keeps_runtime_open_and_last_owner_closes_worker_context() {
    let (manager, _, _) = setup().await;
    let clone = manager.clone();
    let mut worker = manager.worker_context();
    drop(manager);
    assert!(!clone.is_closing());
    assert!(worker.close.has_changed().is_ok());
    drop(clone);
    assert!(worker.close.changed().await.is_err());
}

#[tokio::test]
async fn successful_post_then_receipt_save_failure_blocks_replay_even_after_restore() {
    let (manager, transport, c) = setup().await;
    transport.gate.store(true, Ordering::SeqCst);
    let record = outbound_for(&c, "reply", "receipt failure", false);
    let id = record.id.clone();
    manager
        .state
        .write()
        .await
        .discord_outbound
        .insert(id.clone(), record);
    let delivering = tokio::spawn({
        let manager = manager.clone();
        let connector = c.id.clone();
        async move { manager.deliver_one(connector).await }
    });
    within(transport.entered.0.acquire())
        .await
        .unwrap()
        .forget();
    let gate = manager
        .state
        .write()
        .await
        .install_test_control_plane_save_gate(true);
    transport.release.0.add_permits(1);
    within(gate.entered.acquire()).await.unwrap().forget();
    gate.release.add_permits(1);
    assert_eq!(
        within(delivering).await.unwrap().unwrap_err(),
        DiscordManagerError::Persistence
    );
    assert_eq!(
        manager.state.read().await.discord_outbound[&id].delivery_state,
        DiscordDeliveryState::Unknown
    );
    manager.deliver_one(c.id.clone()).await.unwrap();
    assert_eq!(transport.sends.load(Ordering::SeqCst), 1);
    let snapshot = manager.state.read().await.control_plane_snapshot();
    let mut restored = DaemonState::new();
    restored.restore_control_plane_snapshot(snapshot).unwrap();
    assert_eq!(
        restored.discord_outbound[&id].delivery_state,
        DiscordDeliveryState::Unknown
    );
}
#[tokio::test]
async fn received_turn_uses_native_coordinator_session_and_stable_run_link() {
    let (manager, _, c) = setup().await;
    manager
        .accept_batch(
            c.id.clone(),
            DiscordMessageBatch {
                messages: vec![incoming(&c, "9007199254740993")],
                next_cursor: "9007199254740993".into(),
                boundary_proven: true,
            },
        )
        .await
        .unwrap();
    manager.process_one(c.id.clone()).await.unwrap();
    within(async {
        loop {
            if manager
                .state
                .read()
                .await
                .discord_inbound
                .values()
                .all(|r| r.processing_state.is_terminal())
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    let state = manager.state.read().await;
    let receipt = state.discord_inbound.values().next().unwrap();
    assert!(receipt.run_id.is_some());
    let run = state.runs.get(receipt.run_id.as_deref().unwrap()).unwrap();
    assert_eq!(run.source, crate::runs::RunSource::Discord);
    assert_eq!(run.session_id, c.room_id);
    assert_eq!(receipt.processing_state, DiscordProcessingState::Processed);
    assert_eq!(state.discord_outbound.len(), 1);
    let session = state.sessions.get(&c.agent_id, &c.room_id).unwrap();
    assert_eq!(session.kind, crate::sessions::SessionKind::Discord);
    drop(state);
    manager.runs.shutdown().await;
}
#[tokio::test]
async fn unproven_scan_and_capacity_pressure_admit_nothing_and_do_not_move_cursor() {
    let (manager, _, c) = setup().await;
    let message = incoming(&c, "9007199254740993");
    let unproven = DiscordMessageBatch {
        messages: vec![message],
        next_cursor: "9007199254740993".into(),
        boundary_proven: false,
    };
    assert_eq!(
        manager
            .accept_batch(c.id.clone(), unproven)
            .await
            .unwrap_err(),
        DiscordManagerError::Conflict
    );
    let messages = (1..=101).map(|id| incoming(&c, &id.to_string())).collect();
    assert_eq!(
        manager
            .accept_batch(
                c.id.clone(),
                DiscordMessageBatch {
                    messages,
                    next_cursor: "101".into(),
                    boundary_proven: true
                }
            )
            .await
            .unwrap_err(),
        DiscordManagerError::Backpressure
    );
    let state = manager.state.read().await;
    assert!(state.discord_inbound.is_empty());
    assert_eq!(state.discord_connectors[&c.id].cursor, "0");
}

struct GateDeleteStore {
    inner: credentials::InMemoryDiscordCredentialStore,
    calls: AtomicUsize,
    entered: Semaphore,
    release: Semaphore,
}
#[async_trait::async_trait]
impl DiscordCredentialStore for GateDeleteStore {
    async fn load(
        &self,
        id: &str,
        generation: u64,
        bot: &str,
    ) -> Result<Option<DiscordBotToken>, credentials::DiscordCredentialError> {
        self.inner.load(id, generation, bot).await
    }
    async fn put(
        &self,
        id: &str,
        generation: u64,
        bot: &str,
        token: &DiscordBotToken,
    ) -> Result<(), credentials::DiscordCredentialError> {
        self.inner.put(id, generation, bot, token).await
    }
    async fn delete(
        &self,
        id: &str,
        generation: u64,
    ) -> Result<(), credentials::DiscordCredentialError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.entered.add_permits(1);
        self.release.acquire().await.unwrap().forget();
        self.inner.delete(id, generation).await
    }
}
async fn delete_store(c: &DiscordConnectorRecord) -> Arc<GateDeleteStore> {
    let store = Arc::new(GateDeleteStore {
        inner: credentials::InMemoryDiscordCredentialStore::default(),
        calls: AtomicUsize::new(0),
        entered: Semaphore::new(0),
        release: Semaphore::new(0),
    });
    store
        .put(
            &c.id,
            c.credential_generation,
            &c.bot.id,
            &DiscordBotToken::parse("test.token").unwrap(),
        )
        .await
        .unwrap();
    store
}
#[tokio::test]
async fn delete_journals_exact_generation_cleanup_with_tombstone_before_vault_effect() {
    let (mut manager, _, c) = setup().await;
    let store = delete_store(&c).await;
    manager.credentials = store.clone();
    let path = std::env::temp_dir().join(format!(
        "anima-discord-delete-{}.json",
        uuid::Uuid::new_v4()
    ));
    let config = crate::control_plane_store::ControlPlaneStoreConfig::Json(path.clone());
    manager
        .state
        .write()
        .await
        .set_control_plane_store(Some(config.clone()));
    let deleting = tokio::spawn({
        let manager = manager.clone();
        let c = c.clone();
        async move { manager.delete(c.agent_id, c.id).await }
    });
    within(store.entered.acquire()).await.unwrap().forget();
    let snapshot = crate::control_plane_store::load_control_plane_snapshot(&config)
        .await
        .unwrap()
        .unwrap();
    assert!(snapshot.discord_connectors[0].deleted_at_ms.is_some());
    assert!(!snapshot.discord_connectors[0].enabled);
    assert_eq!(snapshot.discord_credential_intents.len(), 1);
    assert_eq!(
        snapshot.discord_credential_intents[0].generation,
        c.credential_generation
    );
    assert_eq!(
        snapshot.discord_credential_intents[0].operation,
        DiscordCredentialOperation::Delete
    );
    let mut restored = DaemonState::new();
    restored.restore_control_plane_snapshot(snapshot).unwrap();
    assert!(restored
        .discord_credential_intents
        .contains_key(&(c.id.clone(), c.credential_generation)));
    store.release.add_permits(1);
    within(deleting).await.unwrap().unwrap();
    assert_eq!(store.calls.load(Ordering::SeqCst), 1);
    assert!(manager
        .state
        .read()
        .await
        .discord_credential_intents
        .is_empty());
    std::fs::remove_file(path).unwrap();
}
#[tokio::test]
async fn deletion_save_failure_rolls_back_tombstone_and_intent_and_never_touches_vault() {
    let (mut manager, _, c) = setup().await;
    let store = delete_store(&c).await;
    manager.credentials = store.clone();
    let gate = manager
        .state
        .write()
        .await
        .install_test_control_plane_save_gate(true);
    let deleting = tokio::spawn({
        let manager = manager.clone();
        let c = c.clone();
        async move { manager.delete(c.agent_id, c.id).await }
    });
    within(gate.entered.acquire()).await.unwrap().forget();
    gate.release.add_permits(1);
    assert_eq!(
        within(deleting).await.unwrap().unwrap_err(),
        DiscordManagerError::Persistence
    );
    assert_eq!(manager.state.read().await.discord_connectors[&c.id], c);
    assert!(manager
        .state
        .read()
        .await
        .discord_credential_intents
        .is_empty());
    assert_eq!(store.calls.load(Ordering::SeqCst), 0);
    assert!(store
        .load(&c.id, c.credential_generation, &c.bot.id)
        .await
        .unwrap()
        .is_some());
}
#[tokio::test]
async fn malformed_fake_activation_baseline_never_enables_or_changes_cursor() {
    let (manager, transport, c) = setup().await;
    {
        let mut state = manager.state.write().await;
        let c = state.discord_connectors.get_mut(&c.id).unwrap();
        c.enabled = false;
        c.activated_at_ms = None;
    }
    let previous = manager.state.read().await.discord_connectors[&c.id].clone();
    *transport.baseline.lock().unwrap() = Some("not-a-snowflake".into());
    assert_eq!(
        manager
            .activate(c.agent_id, c.id.clone())
            .await
            .unwrap_err(),
        DiscordManagerError::Conflict
    );
    assert_eq!(
        manager.state.read().await.discord_connectors[&c.id],
        previous
    );
    assert!(manager.workers.lock().await.is_empty());
}
#[tokio::test]
async fn legitimate_long_input_is_admitted_but_over_bound_input_never_consumes_cursor() {
    let (manager, _, c) = setup().await;
    let mut message = incoming(&c, "100");
    message.text = "x".repeat(4000);
    manager
        .accept_batch(
            c.id.clone(),
            DiscordMessageBatch {
                messages: vec![message],
                next_cursor: "100".into(),
                boundary_proven: true,
            },
        )
        .await
        .unwrap();
    let mut too_long = incoming(&c, "101");
    too_long.text = "x".repeat(65537);
    assert_eq!(
        manager
            .accept_batch(
                c.id.clone(),
                DiscordMessageBatch {
                    messages: vec![too_long],
                    next_cursor: "101".into(),
                    boundary_proven: true
                }
            )
            .await
            .unwrap_err(),
        DiscordManagerError::UnsupportedInput
    );
    assert_eq!(
        manager.state.read().await.discord_connectors[&c.id].cursor,
        "100"
    );
    assert_eq!(manager.state.read().await.discord_inbound.len(), 1);
}

async fn sending_run_fixture(
    manager: &DiscordManager,
    c: &DiscordConnectorRecord,
) -> (String, String) {
    let mut run = crate::runs::RunRecord::queued(
        crate::runs::RunStart {
            agent_id: c.agent_id.clone(),
            session_id: c.room_id.clone(),
            source: crate::runs::RunSource::Discord,
            source_ref: Some(c.id.clone()),
            idempotency_key: Some("owner-stop-fixture".into()),
            text: "hello".into(),
            model: "deterministic".into(),
            provider: None,
            parent_run_id: None,
        },
        1,
    );
    run.reply_message_id = Some("reply".into());
    run.finish(crate::runs::RunStatus::Completed, None, 2);
    let run_id = run.id.clone();
    let output = outbound_for(c, "reply", &"x".repeat(2100), false);
    let output_id = output.id.clone();
    let mut state = manager.state.write().await;
    state.runs.insert(run);
    state.discord_outbound.insert(output_id.clone(), output);
    (run_id, output_id)
}
#[tokio::test]
async fn owner_stop_during_sending_preserves_receipt_and_suppresses_only_unsent_tail() {
    let (manager, transport, c) = setup().await;
    transport.gate.store(true, Ordering::SeqCst);
    let (run, output) = sending_run_fixture(&manager, &c).await;
    let sending = tokio::spawn({
        let manager = manager.clone();
        let id = c.id.clone();
        async move { manager.deliver_one(id).await }
    });
    within(transport.entered.0.acquire())
        .await
        .unwrap()
        .forget();
    manager.runs.stop_run(&c.agent_id, &run).await.unwrap();
    {
        let state = manager.state.read().await;
        assert!(state.discord_outbound[&output].stop_requested);
        assert_eq!(
            state.discord_outbound[&output].chunks[0].state,
            DiscordDeliveryState::Sending
        );
        assert_eq!(
            state.discord_outbound[&output].chunks[1].state,
            DiscordDeliveryState::Suppressed
        );
    }
    transport.release.0.add_permits(1);
    within(sending).await.unwrap().unwrap();
    assert!(!manager.deliver_one(c.id.clone()).await.unwrap());
    let state = manager.state.read().await;
    assert_eq!(
        state.discord_outbound[&output].delivery_state,
        DiscordDeliveryState::Suppressed
    );
    assert_eq!(
        state.discord_outbound[&output].chunks[0]
            .provider_message_id
            .as_deref(),
        Some("9007199254740993")
    );
    assert_eq!(transport.sends.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn failed_owner_stop_save_restores_exact_inflight_attempt_and_pending_tail() {
    let (manager, transport, c) = setup().await;
    transport.gate.store(true, Ordering::SeqCst);
    let (run, output) = sending_run_fixture(&manager, &c).await;
    let sending = tokio::spawn({
        let manager = manager.clone();
        let id = c.id.clone();
        async move { manager.deliver_one(id).await }
    });
    within(transport.entered.0.acquire())
        .await
        .unwrap()
        .forget();
    let gate = manager
        .state
        .write()
        .await
        .install_test_control_plane_save_gate(true);
    let stopping = tokio::spawn({
        let runs = manager.runs.clone();
        let agent = c.agent_id.clone();
        async move { runs.stop_run(&agent, &run).await }
    });
    within(gate.entered.acquire()).await.unwrap().forget();
    gate.release.add_permits(1);
    assert!(within(stopping).await.unwrap().is_err());
    {
        let state = manager.state.read().await;
        assert!(!state.discord_outbound[&output].stop_requested);
        assert_eq!(
            state.discord_outbound[&output].chunks[0].state,
            DiscordDeliveryState::Sending
        );
        assert_eq!(
            state.discord_outbound[&output].chunks[1].state,
            DiscordDeliveryState::Pending
        );
    }
    transport.release.0.add_permits(1);
    within(sending).await.unwrap().unwrap();
    transport.gate.store(false, Ordering::SeqCst);
    manager.deliver_one(c.id.clone()).await.unwrap();
    assert_eq!(transport.sends.load(Ordering::SeqCst), 2);
}
#[tokio::test]
async fn terminal_poll_errors_pause_after_one_request_until_explicit_restart() {
    for error in [
        DiscordTransportError::Authentication,
        DiscordTransportError::PermissionDenied,
        DiscordTransportError::HttpStatus { status: 404 },
    ] {
        let (manager, transport, c) = setup().await;
        *transport.poll_error.lock().unwrap() = Some(error);
        manager.start_restored().await;
        within(async {
            loop {
                if manager
                    .workers
                    .lock()
                    .await
                    .get(&c.id)
                    .is_some_and(|worker| worker.join.is_finished())
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await;
        assert_eq!(transport.poll_calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            manager.status(&c.id).await,
            if error == DiscordTransportError::Authentication {
                DiscordRuntimeStatus::CredentialRequired
            } else {
                DiscordRuntimeStatus::Error
            }
        );
        assert_eq!(
            manager
                .accept_batch(
                    c.id.clone(),
                    DiscordMessageBatch {
                        messages: vec![incoming(&c, "100")],
                        next_cursor: "100".into(),
                        boundary_proven: true
                    }
                )
                .await
                .unwrap_err(),
            DiscordManagerError::Conflict
        );
        assert_eq!(
            manager.state.read().await.discord_connectors[&c.id].cursor,
            "0"
        );
        manager
            .restart(c.agent_id.clone(), c.id.clone())
            .await
            .unwrap();
        within(async {
            loop {
                if transport.poll_calls.load(Ordering::SeqCst) > 1 {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await;
        assert_eq!(manager.status(&c.id).await, DiscordRuntimeStatus::Ready);
        manager.shutdown().await;
    }
}
#[tokio::test]
async fn present_guild_or_channel_mismatch_rejects_entire_batch_without_receipts_or_cursor() {
    for guild in [true, false] {
        let (manager, _, c) = setup().await;
        let first = incoming(&c, "100");
        let mut mismatch = incoming(&c, "101");
        if guild {
            mismatch.guild_id = Some("88".into());
        } else {
            mismatch.channel_id = "88".into();
        }
        assert_eq!(
            manager
                .accept_batch(
                    c.id.clone(),
                    DiscordMessageBatch {
                        messages: vec![first, mismatch],
                        next_cursor: "101".into(),
                        boundary_proven: true
                    }
                )
                .await
                .unwrap_err(),
            DiscordManagerError::Conflict
        );
        assert_eq!(
            manager.state.read().await.discord_connectors[&c.id].cursor,
            "0"
        );
        assert!(manager.state.read().await.discord_inbound.is_empty());
    }
}
#[tokio::test]
async fn contradictory_delivery_snapshot_is_rejected_before_any_restore_mutation() {
    let (manager, _, c) = setup().await;
    let mut output = outbound_for(&c, "reply", "hello", false);
    output.delivery_state = DiscordDeliveryState::Delivered;
    let mut snapshot = manager.state.read().await.control_plane_snapshot();
    snapshot.discord_outbound.push(output);
    let mut restored = DaemonState::new();
    assert!(restored.restore_control_plane_snapshot(snapshot).is_err());
    assert_eq!(restored.agent_count(), 0);
    assert!(restored.discord_connectors.is_empty());
    let mut output = outbound_for(&c, "review", "hello", false);
    output.chunks[0].state = DiscordDeliveryState::Unknown;
    output.chunks[0].attempts = 1;
    output.delivery_state = DiscordDeliveryState::Suppressed;
    let mut snapshot = manager.state.read().await.control_plane_snapshot();
    snapshot.discord_outbound.push(output);
    restored.restore_control_plane_snapshot(snapshot).unwrap();
    assert_eq!(
        restored.discord_outbound.values().next().unwrap().chunks[0].state,
        DiscordDeliveryState::Unknown
    );
}
#[test]
fn setup_transport_errors_retain_safe_actionable_reasons() {
    use super::*;
    for (error, expected) in [
        (
            DiscordTransportError::PermissionDenied,
            "Discord denied access; check the bot's channel permissions",
        ),
        (
            DiscordTransportError::HttpStatus { status: 404 },
            "Discord could not find the selected channel",
        ),
        (
            DiscordTransportError::RateLimited {
                retry_after_ms: 1000,
                global: false,
            },
            "Discord is limiting requests; wait before retrying",
        ),
        (
            DiscordTransportError::InvalidResponse,
            "Discord returned an unexpected response",
        ),
        (
            DiscordTransportError::Transport,
            "The daemon could not reach Discord",
        ),
    ] {
        assert_eq!(transport_error(error).to_string(), expected);
    }
}

#[path = "gateway_tests.rs"]
mod gateway_tests;

#[path = "multi_channel_tests.rs"]
mod multi_channel_tests;
#[path = "speaker_tests.rs"]
mod speaker_tests;
#[path = "typing_tests.rs"]
mod typing_tests;
