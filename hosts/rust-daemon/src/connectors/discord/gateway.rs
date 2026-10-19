//! Persistent Gateway owned independently of model runs. Hermes behavior: HERMES-NOTICE.
use super::*;
use async_trait::async_trait;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use twilight_gateway::{
    error::ReceiveMessageErrorType, CloseFrame, ConfigBuilder, Event, EventTypeFlags, Intents,
    Shard, ShardId, ShardState, StreamExt,
};
use twilight_model::gateway::{
    payload::outgoing::update_presence::UpdatePresencePayload, presence::Status,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum GatewaySignal {
    Ready {
        bot_id: String,
    },
    Resumed,
    Message {
        guild_id: Option<String>,
        channel_id: String,
    },
    Closed(Option<u16>),
    Reconnecting,
    Ignored,
}

pub(super) fn receive_error_signal(kind: &ReceiveMessageErrorType) -> GatewaySignal {
    match kind {
        // Malformed events do not disconnect an otherwise active shard. Periodic REST
        // catch-up still recovers messages without logging the rejected payload.
        ReceiveMessageErrorType::Deserializing { .. } => GatewaySignal::Ignored,
        _ => GatewaySignal::Reconnecting,
    }
}

#[async_trait]
pub(crate) trait GatewaySession: Send {
    /// Cancellation safe: polling must continue to drive the library's heartbeats.
    async fn next(&mut self) -> Option<GatewaySignal>;
    fn connected(&self) -> bool;
    async fn close(&mut self);
}

pub(crate) trait GatewayFactory: Send + Sync {
    fn session(&self, token: &DiscordBotToken, bot_id: &str) -> Box<dyn GatewaySession>;
}

#[derive(Default)]
pub(crate) struct NativeGatewayFactory {
    identify_queues: StdMutex<HashMap<String, twilight_gateway::queue::InMemoryQueue>>,
}
impl NativeGatewayFactory {
    fn config(&self, token: &DiscordBotToken, bot_id: &str) -> twilight_gateway::Config {
        // Discord's identify budget belongs to the verified bot, not a channel or
        // token generation. Reuse it across connections, restarts, and token updates.
        let queue = self
            .identify_queues
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .entry(bot_id.into())
            .or_default()
            .clone();
        // No member, presence, DM, voice, role, or arbitrary command subscriptions.
        ConfigBuilder::new(
            token.expose().to_owned(),
            Intents::GUILD_MESSAGES | Intents::MESSAGE_CONTENT,
        )
        .queue(queue)
        .presence(UpdatePresencePayload {
            activities: vec![],
            afk: false,
            since: None,
            status: Status::Online,
        })
        .build()
    }
}
impl GatewayFactory for NativeGatewayFactory {
    fn session(&self, token: &DiscordBotToken, bot_id: &str) -> Box<dyn GatewaySession> {
        Box::new(NativeSession {
            shard: Shard::with_config(ShardId::ONE, self.config(token, bot_id)),
        })
    }
}

#[cfg(test)]
mod native_tests {
    use super::*;
    use twilight_gateway::queue::Queue;

    #[tokio::test]
    async fn same_bot_sessions_share_identify_pacing_across_token_replacement() {
        let factory = NativeGatewayFactory::default();
        let first = factory.config(&DiscordBotToken::parse("test.old-token").unwrap(), "11");
        let next = factory.config(&DiscordBotToken::parse("test.new-token").unwrap(), "11");
        let other_bot = factory.config(&DiscordBotToken::parse("test.other-bot").unwrap(), "12");
        tokio::time::timeout(Duration::from_secs(1), first.queue().enqueue(0))
            .await
            .expect("first bot login may start immediately")
            .unwrap();
        let cancelled = next.queue().enqueue(0);
        drop(cancelled);
        let mut second = next.queue().enqueue(0);
        assert!(
            tokio::time::timeout(Duration::from_millis(50), &mut second)
                .await
                .is_err(),
            "another channel for the same bot must wait for the shared identify bucket"
        );
        tokio::time::timeout(Duration::from_secs(1), other_bot.queue().enqueue(0))
            .await
            .expect("other bots retain independent login budgets")
            .unwrap();
        tokio::time::timeout(Duration::from_secs(6), second)
            .await
            .expect("cancelled requests do not consume the next login bucket")
            .unwrap();
    }
}

struct NativeSession {
    shard: Shard,
}
#[async_trait]
impl GatewaySession for NativeSession {
    async fn next(&mut self) -> Option<GatewaySignal> {
        let flags =
            EventTypeFlags::READY | EventTypeFlags::RESUMED | EventTypeFlags::MESSAGE_CREATE;
        match self.shard.next_event(flags).await? {
            Ok(Event::Ready(event)) => Some(GatewaySignal::Ready {
                bot_id: event.user.id.to_string(),
            }),
            Ok(Event::Resumed) => Some(GatewaySignal::Resumed),
            Ok(Event::MessageCreate(event)) => Some(GatewaySignal::Message {
                guild_id: event.guild_id.map(|id| id.to_string()),
                channel_id: event.channel_id.to_string(),
            }),
            Ok(Event::GatewayClose(frame)) => {
                Some(GatewaySignal::Closed(frame.map(|frame| frame.code)))
            }
            // Never format library errors: a deserialization error can contain a full payload.
            Err(error) => Some(receive_error_signal(error.kind())),
            Ok(_) => Some(GatewaySignal::Ignored),
        }
    }
    fn connected(&self) -> bool {
        self.shard.state() == ShardState::Active
    }
    async fn close(&mut self) {
        self.shard.close(CloseFrame::NORMAL);
        let _ = tokio::time::timeout(Duration::from_secs(2), async {
            while let Some(event) = self.shard.next_event(EventTypeFlags::empty()).await {
                if matches!(event, Ok(Event::GatewayClose(_))) {
                    break;
                }
            }
        })
        .await;
        // Dropping the session after this bounded close retires the socket even without an ACK.
    }
}

#[derive(Clone, Copy)]
pub(super) struct GatewayHealth {
    pub status: DiscordRuntimeStatus,
    pub warning: Option<&'static str>,
}

impl DiscordManager {
    pub(crate) fn with_gateway(mut self, factory: Arc<dyn GatewayFactory>) -> Self {
        self.gateway = Some(factory);
        self
    }

    pub(super) async fn gateway_health(&self, id: &str) -> Option<GatewayHealth> {
        let states = self.gateway_states.lock().await;
        states
            .get(id)
            .filter(|(epoch, _)| self.authority_current(id, *epoch))
            .map(|(_, health)| *health)
    }

    async fn set_gateway_health(
        &self,
        id: &str,
        epoch: u64,
        status: DiscordRuntimeStatus,
        warning: Option<&'static str>,
    ) {
        let mut states = self.gateway_states.lock().await;
        if self.authority_current(id, epoch) {
            states.insert(id.into(), (epoch, GatewayHealth { status, warning }));
        }
    }

    pub(super) async fn run_gateway(
        &self,
        c: DiscordConnectorRecord,
        token: DiscordBotToken,
        epoch: u64,
        wake: Arc<Notify>,
        stop: CancellationToken,
    ) {
        let Some(factory) = &self.gateway else {
            return;
        };
        self.set_gateway_health(&c.id, epoch, DiscordRuntimeStatus::Connecting, None)
            .await;
        let mut session = factory.session(&token, &c.bot.id);
        let mut close = self.close.clone();
        let mut probe = tokio::time::interval(Duration::from_secs(1));
        let mut ready = false;
        loop {
            if !self.authority_current(&c.id, epoch) || self.is_closing() || stop.is_cancelled() {
                break;
            }
            let signal = tokio::select! {
                biased;
                _ = stop.cancelled() => break,
                _ = close.changed() => break,
                _ = probe.tick() => {
                    if ready && !session.connected() {
                        self.set_gateway_health(&c.id, epoch, DiscordRuntimeStatus::Reconnecting, Some("gatewayDisconnected")).await;
                    }
                    continue;
                }
                signal = session.next() => signal,
            };
            if !self.authority_current(&c.id, epoch) || stop.is_cancelled() {
                break;
            }
            match signal {
                Some(GatewaySignal::Ready { bot_id }) if bot_id == c.bot.id => {
                    ready = true;
                    self.set_gateway_health(&c.id, epoch, DiscordRuntimeStatus::Ready, None)
                        .await;
                    wake.notify_one();
                }
                Some(GatewaySignal::Resumed) if ready => {
                    self.set_gateway_health(&c.id, epoch, DiscordRuntimeStatus::Ready, None)
                        .await;
                    wake.notify_one();
                }
                Some(GatewaySignal::Message {
                    guild_id,
                    channel_id,
                }) if ready
                    && channel_id == c.channel.id
                    && guild_id.as_deref() == Some(&c.channel.guild_id) =>
                {
                    // Retain one wake even during catch-up; the REST cursor proves the full boundary.
                    wake.notify_one();
                }
                Some(GatewaySignal::Closed(code))
                    if code.is_some_and(|code| matches!(code, 4004 | 4010..=4014)) =>
                {
                    let (status, warning) = match code {
                        Some(4004) => (
                            DiscordRuntimeStatus::CredentialRequired,
                            "gatewayAuthentication",
                        ),
                        Some(4013 | 4014) => {
                            (DiscordRuntimeStatus::Error, "gatewayDisallowedIntents")
                        }
                        _ => (DiscordRuntimeStatus::Error, "gatewayConfiguration"),
                    };
                    self.fail_gateway(&c.id, epoch, status, warning).await;
                    break;
                }
                Some(GatewaySignal::Ready { .. }) => {
                    self.fail_gateway(
                        &c.id,
                        epoch,
                        DiscordRuntimeStatus::CredentialRequired,
                        "gatewayIdentityMismatch",
                    )
                    .await;
                    break;
                }
                Some(GatewaySignal::Closed(_)) | Some(GatewaySignal::Reconnecting) => {
                    self.set_gateway_health(
                        &c.id,
                        epoch,
                        DiscordRuntimeStatus::Reconnecting,
                        Some("gatewayDisconnected"),
                    )
                    .await;
                }
                None => {
                    self.fail_gateway(&c.id, epoch, DiscordRuntimeStatus::Error, "gatewayClosed")
                        .await;
                    break;
                }
                _ => {}
            }
        }
        let _ = tokio::time::timeout(Duration::from_secs(3), session.close()).await;
    }

    async fn fail_gateway(
        &self,
        id: &str,
        epoch: u64,
        status: DiscordRuntimeStatus,
        warning: &'static str,
    ) {
        let mut statuses = self.statuses.lock().await;
        let mut warnings = self.warnings.lock().await;
        let mut paused = self.paused.lock().unwrap_or_else(|e| e.into_inner());
        let mut authorities = self.authorities.lock().unwrap_or_else(|e| e.into_inner());
        let Some(authority) = authorities
            .get_mut(id)
            .filter(|authority| authority.epoch == epoch)
        else {
            return;
        };
        statuses.insert(id.into(), status);
        warnings.insert(id.into(), warning);
        paused.insert(id.into());
        authority.epoch = authority.epoch.wrapping_add(1);
        self.notify.notify_waiters();
        tracing::warn!(
            category = warning,
            "Discord Gateway stopped; owner action required"
        );
    }
}
