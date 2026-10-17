use super::*;
use crate::connectors::discord::gateway::{GatewayFactory, GatewaySession, GatewaySignal};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

struct FakeGateway {
    events: StdMutex<Option<mpsc::Receiver<GatewaySignal>>>,
    connected: Arc<AtomicBool>,
    closed: Arc<AtomicUsize>,
}
struct FakeSession {
    events: mpsc::Receiver<GatewaySignal>,
    connected: Arc<AtomicBool>,
    closed: Arc<AtomicUsize>,
}
impl GatewayFactory for FakeGateway {
    fn session(&self, _: &DiscordBotToken) -> Box<dyn GatewaySession> {
        Box::new(FakeSession {
            events: self.events.lock().unwrap().take().unwrap(),
            connected: self.connected.clone(),
            closed: self.closed.clone(),
        })
    }
}
#[async_trait::async_trait]
impl GatewaySession for FakeSession {
    async fn next(&mut self) -> Option<GatewaySignal> {
        let signal = self.events.recv().await?;
        match signal {
            GatewaySignal::Ready { .. } | GatewaySignal::Resumed => {
                self.connected.store(true, Ordering::SeqCst)
            }
            GatewaySignal::Closed(_) | GatewaySignal::Reconnecting => {
                self.connected.store(false, Ordering::SeqCst)
            }
            _ => {}
        }
        Some(signal)
    }
    fn connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }
    async fn close(&mut self) {
        self.closed.fetch_add(1, Ordering::SeqCst);
    }
}
fn fake_gateway() -> (Arc<FakeGateway>, mpsc::Sender<GatewaySignal>) {
    let (tx, rx) = mpsc::channel(8);
    (
        Arc::new(FakeGateway {
            events: StdMutex::new(Some(rx)),
            connected: Arc::new(AtomicBool::new(false)),
            closed: Arc::new(AtomicUsize::new(0)),
        }),
        tx,
    )
}

#[tokio::test]
async fn recoverable_parse_error_preserves_online_and_later_message_wakes_catch_up() {
    use crate::connectors::discord::gateway::receive_error_signal;
    use twilight_gateway::error::ReceiveMessageErrorType;

    let (manager, _, c) = setup().await;
    let (factory, tx) = fake_gateway();
    let manager = manager.with_gateway(factory);
    let epoch = manager.epoch(&c.id);
    let wake = Arc::new(Notify::new());
    let stop = CancellationToken::new();
    let task = tokio::spawn({
        let manager = manager.clone();
        let c = c.clone();
        let wake = wake.clone();
        let stop = stop.clone();
        async move {
            manager
                .run_gateway(
                    c,
                    DiscordBotToken::parse("fake").unwrap(),
                    epoch,
                    wake,
                    stop,
                )
                .await;
        }
    });
    tx.send(GatewaySignal::Ready {
        bot_id: c.bot.id.clone(),
    })
    .await
    .unwrap();
    within(wake.notified()).await;
    tx.send(receive_error_signal(
        &ReceiveMessageErrorType::Deserializing {
            event: "malformed fixture payload must never be logged".into(),
        },
    ))
    .await
    .unwrap();
    tx.send(GatewaySignal::Message {
        guild_id: Some(c.channel.guild_id.clone()),
        channel_id: c.channel.id.clone(),
    })
    .await
    .unwrap();
    within(wake.notified()).await;
    assert_eq!(
        manager.gateway_health(&c.id).await.unwrap().status,
        DiscordRuntimeStatus::Ready
    );
    assert_eq!(
        receive_error_signal(&ReceiveMessageErrorType::Reconnect),
        GatewaySignal::Reconnecting
    );
    stop.cancel();
    within(task).await.unwrap();
}
async fn status_is(manager: &DiscordManager, id: &str, status: DiscordRuntimeStatus) {
    within(async {
        loop {
            if manager.status(id).await == status {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
}

#[tokio::test]
async fn rest_success_cannot_claim_online_and_gateway_reconnect_remains_independent_of_held_work() {
    let (manager, transport, c) = setup().await;
    let (factory, tx) = fake_gateway();
    let manager = manager.with_gateway(factory.clone());
    manager
        .activate(c.agent_id.clone(), c.id.clone())
        .await
        .unwrap();
    assert_eq!(
        manager.status(&c.id).await,
        DiscordRuntimeStatus::Connecting
    );
    manager
        .set_status(&c.id, DiscordRuntimeStatus::Ready, None)
        .await;
    assert_eq!(
        manager.status(&c.id).await,
        DiscordRuntimeStatus::Connecting
    );
    tx.send(GatewaySignal::Ready {
        bot_id: c.bot.id.clone(),
    })
    .await
    .unwrap();
    status_is(&manager, &c.id, DiscordRuntimeStatus::Ready).await;
    // A held outbound request cannot stall Gateway processing or its heartbeat driver.
    transport.gate.store(true, Ordering::SeqCst);
    let output = outbound_for(&c, "gateway-held-reply", "hello", false);
    manager
        .state
        .write()
        .await
        .discord_outbound
        .insert(output.id.clone(), output);
    let sending = tokio::spawn({
        let manager = manager.clone();
        let id = c.id.clone();
        async move { manager.deliver_one(id).await }
    });
    within(transport.entered.0.acquire())
        .await
        .unwrap()
        .forget();
    tx.send(GatewaySignal::Reconnecting).await.unwrap();
    status_is(&manager, &c.id, DiscordRuntimeStatus::Degraded).await; // In-flight delivery has higher priority.
    within(async {
        loop {
            if manager
                .gateway_health(&c.id)
                .await
                .is_some_and(|h| h.status == DiscordRuntimeStatus::Reconnecting)
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    manager
        .set_status(&c.id, DiscordRuntimeStatus::Ready, None)
        .await;
    tx.send(GatewaySignal::Resumed).await.unwrap();
    within(async {
        loop {
            if manager
                .gateway_health(&c.id)
                .await
                .is_some_and(|h| h.status == DiscordRuntimeStatus::Ready)
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    transport.release.0.add_permits(1);
    within(sending).await.unwrap().unwrap();
    within(manager.set_enabled(c.agent_id.clone(), c.id.clone(), false))
        .await
        .unwrap();
    assert_eq!(factory.closed.load(Ordering::SeqCst), 1);
    assert_eq!(manager.status(&c.id).await, DiscordRuntimeStatus::Disabled);
}

#[tokio::test]
async fn gateway_wakes_are_selected_coalesced_and_retained_and_old_epochs_cannot_restore_online() {
    let (manager, _, c) = setup().await;
    let (factory, tx) = fake_gateway();
    let manager = manager.with_gateway(factory.clone());
    let epoch = manager.epoch(&c.id);
    let wake = Arc::new(Notify::new());
    let stop = CancellationToken::new();
    let task = tokio::spawn({
        let manager = manager.clone();
        let c = c.clone();
        let wake = wake.clone();
        let stop = stop.clone();
        async move {
            manager
                .run_gateway(
                    c,
                    DiscordBotToken::parse("fake").unwrap(),
                    epoch,
                    wake,
                    stop,
                )
                .await;
        }
    });
    tx.send(GatewaySignal::Ready {
        bot_id: c.bot.id.clone(),
    })
    .await
    .unwrap();
    within(wake.notified()).await;
    tx.send(GatewaySignal::Message {
        guild_id: Some(c.channel.guild_id.clone()),
        channel_id: "999".into(),
    })
    .await
    .unwrap();
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(40), wake.notified())
            .await
            .is_err()
    );
    tx.send(GatewaySignal::Message {
        guild_id: Some(c.channel.guild_id.clone()),
        channel_id: c.channel.id.clone(),
    })
    .await
    .unwrap();
    tokio::task::yield_now().await;
    within(wake.notified()).await;
    manager.revoke(&c.id);
    tx.send(GatewaySignal::Ready {
        bot_id: c.bot.id.clone(),
    })
    .await
    .unwrap();
    within(task).await.unwrap();
    assert_ne!(manager.status(&c.id).await, DiscordRuntimeStatus::Ready);
    assert_eq!(factory.closed.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn gateway_rejects_wrong_bot_and_reports_missing_intent_without_reconnecting_forever() {
    for (signal, status, warning) in [
        (
            GatewaySignal::Ready {
                bot_id: "999".into(),
            },
            DiscordRuntimeStatus::CredentialRequired,
            "gatewayIdentityMismatch",
        ),
        (
            GatewaySignal::Closed(Some(4014)),
            DiscordRuntimeStatus::Error,
            "gatewayDisallowedIntents",
        ),
    ] {
        let (manager, _, c) = setup().await;
        let (factory, tx) = fake_gateway();
        let manager = manager.with_gateway(factory.clone());
        manager
            .activate(c.agent_id.clone(), c.id.clone())
            .await
            .unwrap();
        tx.send(signal).await.unwrap();
        status_is(&manager, &c.id, status).await;
        manager
            .set_status(&c.id, DiscordRuntimeStatus::Ready, None)
            .await;
        assert_eq!(manager.status(&c.id).await, status);
        assert_eq!(manager.warning(&c.id).await, Some(warning));
        within(manager.shutdown()).await;
        assert_eq!(factory.closed.load(Ordering::SeqCst), 1);
    }
}
