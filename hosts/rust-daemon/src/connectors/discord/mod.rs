//! Native Discord channel and thread runtime. Adapted behavior: see HERMES-NOTICE.
pub(crate) mod client;
pub(crate) mod credentials;
mod delivery;
pub(crate) mod gateway;
mod inbound;
mod interactions;
mod lifecycle;
pub(crate) mod model;
mod processing;
#[cfg(test)]
mod runtime_tests;
pub(crate) mod sender;
pub(crate) mod text;
#[cfg(test)]
mod text_tests;
mod typing;
use crate::agent_runs::AgentRunCoordinator;
use crate::app::SharedDaemonState;
use crate::connectors::registry::ConnectorRegistry;
use crate::state::DaemonState;
use anima_service_adapters::connectors::{
    ConnectorReceiveRequest, ConnectorSendRequest, ConnectorTransport,
};
use client::{DiscordTransport, DiscordTransportError};
use credentials::{DiscordBotToken, DiscordCredentialStore};
pub(crate) use model::*;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use tokio::sync::{watch, Mutex, Notify};
use tokio::task::JoinHandle;

const MAX_LIVE_INBOUND: usize = 100;
pub(crate) const MAX_OUTBOUND: usize = 100;
const MAX_TERMINAL_INBOUND: usize = 1000;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DiscordRuntimeStatus {
    Disabled,
    Ready,
    Connecting,
    Reconnecting,
    CredentialRequired,
    Error,
    Degraded,
    Reconciling,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DiscordManagerError {
    NotFound,
    Busy,
    InvalidInput,
    UnsupportedInput,
    Conflict,
    DuplicateChannel,
    InvalidToken,
    Transport,
    Provider(DiscordTransportError),
    Credential,
    Persistence,
    Backpressure,
    UnknownDelivery,
    WorkerStopped,
}
impl std::fmt::Display for DiscordManagerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NotFound => "Discord connector or agent not found",
            Self::Busy => "agent has a run in progress",
            Self::InvalidInput => "invalid Discord request",
            Self::UnsupportedInput => "Discord message content exceeds the supported inbound bound",
            Self::Conflict => "Discord selection conflicts with existing state",
            Self::DuplicateChannel => "Discord channel is already connected to this agent",
            Self::InvalidToken => "Discord rejected the bot credential",
            Self::Transport => "Discord transport unavailable",
            Self::Provider(error) => match error {
                DiscordTransportError::PermissionDenied => {
                    "Discord denied access; check the bot's channel permissions"
                }
                DiscordTransportError::HttpStatus { status: 404 } => {
                    "Discord could not find the selected channel"
                }
                DiscordTransportError::RateLimited { .. } => {
                    "Discord is limiting requests; wait before retrying"
                }
                DiscordTransportError::Transport => "The daemon could not reach Discord",
                DiscordTransportError::Configuration => "Discord client configuration failed",
                DiscordTransportError::InvalidResponse
                | DiscordTransportError::ResponseTooLarge => {
                    "Discord returned an unexpected response"
                }
                _ => "Discord could not complete the request",
            },
            Self::Credential => "Discord credential requires reconciliation",
            Self::Persistence => "Discord state could not be saved",
            Self::Backpressure => "Discord backlog capacity is exhausted",
            Self::UnknownDelivery => {
                "inspect Discord and acknowledge uncertain delivery before continuing"
            }
            Self::WorkerStopped => "Discord runtime is shutting down",
        })
    }
}
impl std::error::Error for DiscordManagerError {}
#[derive(Default)]
struct Authority {
    epoch: u64,
    claimed: bool,
}
struct Claim {
    authorities: Arc<StdMutex<HashMap<String, Authority>>>,
    notify: Arc<Notify>,
    id: String,
    epoch: u64,
}
impl Drop for Claim {
    fn drop(&mut self) {
        if let Some(a) = self
            .authorities
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_mut(&self.id)
        {
            a.claimed = false;
        }
        self.notify.notify_waiters();
    }
}
struct OwnedBody {
    count: Arc<AtomicUsize>,
    notify: Arc<Notify>,
}
impl Drop for OwnedBody {
    fn drop(&mut self) {
        self.count.fetch_sub(1, Ordering::SeqCst);
        self.notify.notify_waiters();
    }
}
struct Worker {
    cancel: watch::Sender<bool>,
    join: JoinHandle<()>,
}
pub(crate) struct DiscordManager {
    state: SharedDaemonState,
    runs: AgentRunCoordinator,
    credentials: Arc<dyn DiscordCredentialStore>,
    registry: Arc<ConnectorRegistry>,
    gateway: Option<Arc<dyn gateway::GatewayFactory>>,
    gateway_states: Arc<Mutex<HashMap<String, (u64, gateway::GatewayHealth)>>>,
    typing_workers: Arc<Mutex<HashMap<String, typing::TypingWorker>>>,
    intake: Arc<Mutex<()>>,
    discovered: Arc<Mutex<HashMap<String, Vec<(u64, String, String, Option<String>)>>>>,
    lifecycle: Arc<Mutex<()>>,
    transaction: Arc<Mutex<()>>,
    statuses: Arc<Mutex<HashMap<String, DiscordRuntimeStatus>>>,
    warnings: Arc<Mutex<HashMap<String, &'static str>>>,
    workers: Arc<Mutex<HashMap<String, Worker>>>,
    authorities: Arc<StdMutex<HashMap<String, Authority>>>,
    notify: Arc<Notify>,
    paused: Arc<StdMutex<HashSet<String>>>,
    closing: Arc<AtomicBool>,
    owned_count: Arc<AtomicUsize>,
    owned_notify: Arc<Notify>,
    owner_close: Option<watch::Sender<bool>>,
    close: watch::Receiver<bool>,
}
impl Clone for DiscordManager {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            runs: self.runs.clone(),
            credentials: self.credentials.clone(),
            registry: self.registry.clone(),
            gateway: self.gateway.clone(),
            gateway_states: self.gateway_states.clone(),
            typing_workers: self.typing_workers.clone(),
            intake: self.intake.clone(),
            discovered: self.discovered.clone(),
            lifecycle: self.lifecycle.clone(),
            transaction: self.transaction.clone(),
            statuses: self.statuses.clone(),
            warnings: self.warnings.clone(),
            workers: self.workers.clone(),
            authorities: self.authorities.clone(),
            notify: self.notify.clone(),
            paused: self.paused.clone(),
            closing: self.closing.clone(),
            owned_count: self.owned_count.clone(),
            owned_notify: self.owned_notify.clone(),
            owner_close: self.owner_close.clone(),
            close: self.close.clone(),
        }
    }
}
impl DiscordManager {
    pub(crate) fn new(
        state: SharedDaemonState,
        runs: AgentRunCoordinator,
        credentials: Arc<dyn DiscordCredentialStore>,
        registry: Arc<ConnectorRegistry>,
    ) -> Self {
        let transaction = runs.control_plane_transactions();
        let (sender, close) = watch::channel(false);
        Self {
            state,
            runs,
            credentials,
            registry,
            gateway: None,
            gateway_states: Arc::new(Mutex::new(HashMap::new())),
            typing_workers: Arc::new(Mutex::new(HashMap::new())),
            intake: Arc::new(Mutex::new(())),
            discovered: Arc::new(Mutex::new(HashMap::new())),
            lifecycle: Arc::new(Mutex::new(())),
            transaction,
            statuses: Arc::new(Mutex::new(HashMap::new())),
            warnings: Arc::new(Mutex::new(HashMap::new())),
            workers: Arc::new(Mutex::new(HashMap::new())),
            authorities: Arc::new(StdMutex::new(HashMap::new())),
            notify: Arc::new(Notify::new()),
            paused: Arc::new(StdMutex::new(HashSet::new())),
            closing: Arc::new(AtomicBool::new(false)),
            owned_count: Arc::new(AtomicUsize::new(0)),
            owned_notify: Arc::new(Notify::new()),
            owner_close: Some(sender),
            close,
        }
    }
    fn worker_context(&self) -> Self {
        let mut worker = self.clone();
        worker.owner_close = None;
        worker
    }
    pub(crate) fn is_closing(&self) -> bool {
        self.closing.load(Ordering::SeqCst)
    }
    fn ensure_open(&self) -> Result<(), DiscordManagerError> {
        if self.is_closing() {
            Err(DiscordManagerError::WorkerStopped)
        } else {
            Ok(())
        }
    }
    async fn owned<T: Send + 'static>(
        &self,
        future: impl std::future::Future<Output = Result<T, DiscordManagerError>> + Send + 'static,
    ) -> Result<T, DiscordManagerError> {
        self.ensure_open()?;
        self.owned_count.fetch_add(1, Ordering::SeqCst);
        let guard = OwnedBody {
            count: self.owned_count.clone(),
            notify: self.owned_notify.clone(),
        };
        tokio::spawn(async move {
            let _guard = guard;
            future.await
        })
        .await
        .map_err(|_| DiscordManagerError::WorkerStopped)?
    }
    fn epoch(&self, id: &str) -> u64 {
        self.authorities
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(id.into())
            .or_default()
            .epoch
    }
    fn authority_current(&self, id: &str, epoch: u64) -> bool {
        !self.is_closing()
            && !self
                .paused
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .contains(id)
            && self.epoch(id) == epoch
    }
    fn revoke(&self, id: &str) {
        let mut authorities = self.authorities.lock().unwrap_or_else(|e| e.into_inner());
        let a = authorities.entry(id.into()).or_default();
        a.epoch = a.epoch.wrapping_add(1);
    }
    fn claim(&self, id: &str, epoch: u64) -> Option<Claim> {
        if self.is_closing() {
            return None;
        }
        let mut authorities = self.authorities.lock().unwrap_or_else(|e| e.into_inner());
        let a = authorities.entry(id.into()).or_default();
        if a.epoch != epoch || a.claimed {
            return None;
        }
        a.claimed = true;
        Some(Claim {
            authorities: self.authorities.clone(),
            notify: self.notify.clone(),
            id: id.into(),
            epoch,
        })
    }
    async fn drain(&self, id: &str) {
        loop {
            let notified = self.notify.notified();
            let busy = self
                .authorities
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(id)
                .is_some_and(|a| a.claimed);
            if !busy {
                return;
            }
            notified.await;
        }
    }
    pub(crate) async fn status(&self, id: &str) -> DiscordRuntimeStatus {
        if self
            .state
            .read()
            .await
            .discord_credential_intents
            .keys()
            .any(|(connector, _)| connector == id)
        {
            return DiscordRuntimeStatus::Reconciling;
        }
        let paused = self
            .paused
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(id);
        if paused {
            return self
                .statuses
                .lock()
                .await
                .get(id)
                .copied()
                .unwrap_or(DiscordRuntimeStatus::Error);
        }
        {
            let state = self.state.read().await;
            if state.discord_connectors.get(id).is_some_and(|c| !c.enabled) {
                return DiscordRuntimeStatus::Disabled;
            }
            if state.discord_outbound.values().any(|r| {
                r.connector_id == id
                    && matches!(
                        r.delivery_state,
                        DiscordDeliveryState::Unknown | DiscordDeliveryState::Sending
                    )
            }) {
                return DiscordRuntimeStatus::Degraded;
            }
        }
        if self.gateway.is_some() {
            match self.gateway_health(id).await {
                Some(health) if health.status != DiscordRuntimeStatus::Ready => {
                    return health.status
                }
                None => return DiscordRuntimeStatus::Connecting,
                _ => {}
            }
        }
        self.statuses
            .lock()
            .await
            .get(id)
            .copied()
            .unwrap_or(DiscordRuntimeStatus::Disabled)
    }
    pub(crate) async fn warning(&self, id: &str) -> Option<&'static str> {
        if self
            .state
            .read()
            .await
            .discord_outbound
            .values()
            .any(|r| r.connector_id == id && r.delivery_state == DiscordDeliveryState::Unknown)
        {
            return Some("Delivery is uncertain. Inspect Discord and acknowledge suppression before continuing. Retry cannot resend this output.");
        }
        if let Some(warning) = self
            .gateway_health(id)
            .await
            .and_then(|health| health.warning)
        {
            return Some(warning);
        }
        self.warnings.lock().await.get(id).copied()
    }
    async fn set_status(
        &self,
        id: &str,
        status: DiscordRuntimeStatus,
        warning: Option<&'static str>,
    ) {
        let mut statuses = self.statuses.lock().await;
        let mut warnings = self.warnings.lock().await;
        let paused = self.paused.lock().unwrap_or_else(|e| e.into_inner());
        // A REST success/ack racing a fatal Gateway error cannot restore online readiness.
        if status == DiscordRuntimeStatus::Ready && paused.contains(id) {
            return;
        }
        statuses.insert(id.into(), status);
        if let Some(w) = warning {
            warnings.insert(id.into(), w);
        } else {
            warnings.remove(id);
        }
    }
    pub(crate) async fn get(
        &self,
        agent: &str,
        id: &str,
    ) -> Result<DiscordConnectorRecord, DiscordManagerError> {
        self.state
            .read()
            .await
            .discord_connectors
            .get(id)
            .filter(|c| c.agent_id == agent && c.deleted_at_ms.is_none())
            .cloned()
            .ok_or(DiscordManagerError::NotFound)
    }
    fn native(&self) -> Result<&dyn DiscordTransport, DiscordManagerError> {
        self.registry
            .discord_native()
            .ok_or(DiscordManagerError::Transport)
    }
    async fn load_token(
        &self,
        c: &DiscordConnectorRecord,
    ) -> Result<DiscordBotToken, DiscordManagerError> {
        self.credentials
            .load(&c.id, c.credential_generation, &c.bot.id)
            .await
            .map_err(|_| DiscordManagerError::Credential)?
            .ok_or(DiscordManagerError::Credential)
    }
}
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}
fn transport_error(error: DiscordTransportError) -> DiscordManagerError {
    match error {
        DiscordTransportError::Authentication => DiscordManagerError::InvalidToken,
        DiscordTransportError::InvalidInput => DiscordManagerError::InvalidInput,
        _ => DiscordManagerError::Provider(error),
    }
}

fn setup_error(step: &'static str, error: DiscordTransportError) -> DiscordManagerError {
    // Keep provider bodies, request headers, and credentials out of diagnostics.
    let http_status = match error {
        DiscordTransportError::HttpStatus { status } => Some(status),
        DiscordTransportError::Authentication => Some(401),
        DiscordTransportError::PermissionDenied => Some(403),
        DiscordTransportError::RateLimited { .. } => Some(429),
        _ => None,
    };
    tracing::warn!(
        step,
        category = error.category(),
        http_status,
        "Discord setup failed"
    );
    transport_error(error)
}

pub(crate) fn request_outbound_stop(record: &mut DiscordOutboundRecord) -> bool {
    if record.delivery_state.is_settled() || record.stop_requested {
        return false;
    }
    record.stop_requested = true;
    for chunk in &mut record.chunks {
        if chunk.state.awaits_delivery() {
            chunk.state = DiscordDeliveryState::Suppressed;
        }
    }
    if record
        .chunks
        .iter()
        .any(|c| c.state == DiscordDeliveryState::Sending)
    {
        record.delivery_state = DiscordDeliveryState::Sending;
    } else if record
        .chunks
        .iter()
        .any(|c| c.state == DiscordDeliveryState::Unknown)
    {
        record.delivery_state = DiscordDeliveryState::Unknown;
    } else {
        record.delivery_state = DiscordDeliveryState::Suppressed;
    }
    true
}

pub(crate) fn recover_outbound(record: &mut DiscordOutboundRecord) {
    if record.delivery_state.is_settled() {
        return;
    }
    for chunk in &mut record.chunks {
        if chunk.state == DiscordDeliveryState::Sending {
            chunk.state = DiscordDeliveryState::Unknown;
            chunk.error_category = Some("interrupted_send".into());
        }
    }
    if record
        .chunks
        .iter()
        .any(|c| c.state == DiscordDeliveryState::Unknown)
    {
        record.delivery_state = DiscordDeliveryState::Unknown;
    }
}
fn suppress_unknown_record(record: &mut DiscordOutboundRecord) -> bool {
    if record.delivery_state != DiscordDeliveryState::Unknown
        || record
            .chunks
            .iter()
            .any(|c| c.state == DiscordDeliveryState::Sending)
    {
        return false;
    }
    // Keep the Unknown chunk as immutable uncertainty evidence; settle only the envelope and unsent tail.
    for chunk in &mut record.chunks {
        if chunk.state.awaits_delivery() {
            chunk.state = DiscordDeliveryState::Suppressed;
        }
    }
    record.delivery_state = DiscordDeliveryState::Suppressed;
    true
}
pub(crate) fn outbound_for(
    connector: &DiscordConnectorRecord,
    reply_id: &str,
    reply: &str,
    stopped: bool,
) -> DiscordOutboundRecord {
    let id = format!("discord:{}:reply:{}", connector.id, reply_id);
    let status = if stopped {
        DiscordDeliveryState::Suppressed
    } else {
        DiscordDeliveryState::Pending
    };
    let chunks = text::split_reply(reply)
        .into_iter()
        .enumerate()
        .map(|(i, text)| DiscordOutboundChunk {
            index: i as u8,
            text,
            state: status.clone(),
            attempt_id: text::stable_nonce(&format!("{id}:{i}")),
            provider_message_id: None,
            attempts: 0,
            retry_at_ms: None,
            error_category: None,
        })
        .collect();
    DiscordOutboundRecord {
        id,
        connector_id: connector.id.clone(),
        agent_id: connector.agent_id.clone(),
        room_id: connector.room_id.clone(),
        assistant_message_id: reply_id.into(),
        text: reply.into(),
        created_at_ms: now_ms(),
        delivery_state: status,
        chunks,
        message_pruned: false,
        stop_requested: stopped,
    }
}

pub(crate) fn validate_snapshot(
    snapshot: &crate::control_plane_store::ControlPlaneSnapshot,
) -> Result<(), String> {
    let agents: HashSet<_> = snapshot
        .agents
        .iter()
        .map(|a| a.state.id.as_str())
        .collect();
    let mut ids = HashSet::new();
    let mut selections = HashSet::new();
    for c in &snapshot.discord_connectors {
        if c.id.is_empty()
            || !ids.insert(c.id.as_str())
            || snapshot.connectors.iter().any(|t| t.id == c.id)
            || c.room_id != format!("discord:{}", c.id)
            || !crate::sessions::is_valid_session_id(&c.room_id)
            || c.credential_generation == 0
            || DiscordSnowflake::parse(&c.bot.id).is_none()
            || (!c.bot_wide && !valid_channel(&c.channel))
            || (c.bot_wide
                && (!c.channel.id.is_empty()
                    || c.settings.is_none()
                    || c.channel.guild_id != c.guild_scope.clone().unwrap_or_default()))
            || c.guild_scope
                .as_ref()
                .is_some_and(|id| DiscordSnowflake::parse(id).is_none())
            || c.application_id
                .as_ref()
                .is_some_and(|id| DiscordSnowflake::parse(id).is_none())
            || DiscordSnowflake::cursor(&c.cursor).is_none()
            || !valid_ids(&c.approved_user_ids, 100)
            || c.settings.as_ref().is_some_and(|s| !valid_settings(s))
            || (c.approved_user_ids.is_empty()
                && c.settings
                    .as_ref()
                    .is_none_or(|s| s.approved_role_ids.is_empty()))
            || (c.deleted_at_ms.is_some() && c.enabled)
            || (c.enabled && c.activated_at_ms.is_none())
            || (c.deleted_at_ms.is_none()
                && (!agents.contains(c.agent_id.as_str())
                    || (!c.bot_wide
                        && !selections.insert((c.agent_id.as_str(), c.channel.id.as_str())))
                    || snapshot.discord_connectors.iter().any(|other| {
                        other.id != c.id
                            && other.deleted_at_ms.is_none()
                            && other.agent_id == c.agent_id
                            && other.bot.id == c.bot.id
                            && (c.bot_wide || other.bot_wide)
                    })))
        {
            return Err("invalid native Discord connector snapshot".into());
        }
        let mut rooms = HashSet::new();
        let mut channels = HashSet::new();
        if c.conversations.len() > 1000 || c.thread_starts.len() > 1000 {
            return Err("invalid native Discord conversation bound".into());
        }
        for conversation in &c.conversations {
            let root = !c.bot_wide
                && conversation.channel.routing_eq(&c.channel)
                && conversation.room_id == c.room_id;
            if !valid_channel(&conversation.channel)
                || !rooms.insert(&conversation.room_id)
                || !channels.insert(&conversation.channel.id)
                || (!root
                    && conversation.room_id
                        != format!("discord:{}:{}", c.id, conversation.channel.id))
                || !crate::sessions::is_valid_session_id(&conversation.room_id)
                || DiscordSnowflake::cursor(&conversation.cursor).is_none()
                || c.guild_scope
                    .as_ref()
                    .is_some_and(|guild| guild != &conversation.channel.guild_id)
            {
                return Err("invalid native Discord conversation snapshot".into());
            }
        }
        for (message, claim) in &c.thread_starts {
            if DiscordSnowflake::parse(message).is_none()
                || c.destination_for_channel(&claim.channel_id).is_none()
                || claim.thread_channel_id.as_ref().is_some_and(|id| {
                    !c.conversations.iter().any(|v| {
                        v.channel.id == *id
                            && v.channel.kind == DiscordChannelKind::GuildThread
                            && v.channel.parent_channel_id.as_ref() == Some(&claim.channel_id)
                    })
                })
            {
                return Err("invalid native Discord thread claim".into());
            }
        }
    }
    let mut intents = HashSet::new();
    for i in &snapshot.discord_credential_intents {
        if i.connector_id.is_empty()
            || i.generation == 0
            || !intents.insert((&i.connector_id, i.generation))
        {
            return Err("invalid native Discord credential intent".into());
        }
    }
    let mut receipts = HashSet::new();
    for r in &snapshot.discord_inbound {
        let c = snapshot
            .discord_connectors
            .iter()
            .find(|c| c.id == r.connector_id)
            .ok_or("orphan native Discord receipt")?;
        if !receipts.insert((&r.connector_id, &r.message_id))
            || DiscordSnowflake::parse(&r.message_id).is_none()
            || DiscordSnowflake::parse(&r.message_id)
                .map(|id| id.value())
                .unwrap_or(u64::MAX)
                > c.destination_for_channel(&r.channel_id).and_then(|v|DiscordSnowflake::cursor(&v.cursor)).unwrap_or(0)
            || r.agent_id != c.agent_id
            || !c.destination_for_room(&r.room_id).is_some_and(|v|v.channel.id==r.channel_id || v.channel.parent_channel_id.as_ref()==Some(&r.channel_id))
            // Authority was proven at admission. Reauthorizing historical senders
            // against today's allowlist would break role receipts and user removal.
            || DiscordSnowflake::parse(&r.sender.id).is_none()
            || r.sender.id == c.bot.id
            || (c.deleted_at_ms.is_some() && !r.processing_state.is_terminal())
            || r.run_idempotency_key != format!("discord:{}:message:{}", c.id, r.message_id)
        {
            return Err("invalid native Discord receipt".into());
        }
    }
    let mut outputs = HashSet::new();
    for r in &snapshot.discord_outbound {
        let c = snapshot
            .discord_connectors
            .iter()
            .find(|c| c.id == r.connector_id)
            .ok_or("orphan native Discord outbox")?;
        if !outputs.insert(&r.id)
            || r.id != format!("discord:{}:reply:{}", c.id, r.assistant_message_id)
            || r.assistant_message_id.is_empty()
            || r.agent_id != c.agent_id
            || !c.owns_room(&r.room_id)
            || r.chunks.is_empty()
            || r.chunks.len() > 8
            || (c.deleted_at_ms.is_some() && !r.delivery_state.is_settled())
        {
            return Err("invalid native Discord outbox".into());
        }
        if !valid_delivery_aggregate(r) {
            return Err("contradictory native Discord delivery journal".into());
        }
        let prepared = text::split_reply(&r.text);
        if prepared.len() != r.chunks.len() {
            return Err("invalid native Discord chunk plan".into());
        }
        for (index, chunk) in r.chunks.iter().enumerate() {
            if chunk.text != prepared[index]
                || chunk.error_category.as_ref().is_some_and(|category| {
                    !matches!(
                        category.as_str(),
                        "configuration"
                            | "invalidInput"
                            | "invalidResponse"
                            | "authentication"
                            | "permissionDenied"
                            | "rateLimited"
                            | "transport"
                            | "uncertain"
                            | "backlogLimit"
                            | "httpStatus"
                            | "responseTooLarge"
                            | "interrupted_send"
                            | "authority_changed"
                            | "invalid_receipt"
                            | "receipt_persistence_failed"
                    )
                })
                || chunk.index as usize != index
                || chunk.text.encode_utf16().count() > 2000
                || chunk.text.is_empty()
                || chunk.attempt_id != text::stable_nonce(&format!("{}:{index}", r.id))
                || (matches!(
                    chunk.state,
                    DiscordDeliveryState::Sending
                        | DiscordDeliveryState::Delivered
                        | DiscordDeliveryState::Failed
                        | DiscordDeliveryState::Unknown
                ) && chunk.attempts == 0)
                || chunk
                    .provider_message_id
                    .as_ref()
                    .is_some_and(|id| DiscordSnowflake::parse(id).is_none())
                || (chunk.state == DiscordDeliveryState::Delivered
                    && chunk.provider_message_id.is_none())
            {
                return Err("invalid native Discord outbound chunk".into());
            }
        }
    }
    Ok(())
}
fn valid_delivery_aggregate(record: &DiscordOutboundRecord) -> bool {
    use DiscordDeliveryState::*;
    let chunks = &record.chunks;
    let first = chunks.iter().position(|chunk| chunk.state != Delivered);
    match record.delivery_state {
        Delivered => first.is_none(),
        Suppressed => chunks
            .iter()
            .all(|chunk| matches!(chunk.state, Delivered | Suppressed | Unknown)),
        Pending => {
            !record.stop_requested
                && first.is_some_and(|at| chunks[at..].iter().all(|chunk| chunk.state == Pending))
        }
        Failed => {
            !record.stop_requested
                && first.is_some_and(|at| {
                    chunks[at].state == Failed
                        && chunks[at + 1..].iter().all(|chunk| chunk.state == Pending)
                })
        }
        Sending | Unknown => first.is_some_and(|at| {
            chunks[at].state == record.delivery_state
                && chunks[at + 1..].iter().all(|chunk| {
                    chunk.state == Pending || (record.stop_requested && chunk.state == Suppressed)
                })
        }),
    }
}

fn valid_users(users: &[String]) -> bool {
    !users.is_empty()
        && users.len() <= 16
        && users.iter().all(|id| DiscordSnowflake::parse(id).is_some())
        && users.iter().collect::<HashSet<_>>().len() == users.len()
}

fn valid_ids(ids: &[String], max: usize) -> bool {
    ids.len() <= max
        && ids.iter().all(|id| DiscordSnowflake::parse(id).is_some())
        && ids.iter().collect::<HashSet<_>>().len() == ids.len()
}
fn valid_settings(settings: &DiscordSettings) -> bool {
    valid_ids(&settings.allowed_channel_ids, 100)
        && valid_ids(&settings.free_response_channel_ids, 100)
        && valid_ids(&settings.approved_role_ids, 100)
        && settings
            .home_channel_id
            .as_ref()
            .is_none_or(|id| DiscordSnowflake::parse(id).is_some())
}
fn valid_channel(channel: &DiscordChannelMetadata) -> bool {
    DiscordSnowflake::parse(&channel.id).is_some()
        && DiscordSnowflake::parse(&channel.guild_id).is_some()
        && match channel.kind {
            DiscordChannelKind::GuildText => channel.parent_channel_id.is_none(),
            DiscordChannelKind::GuildThread => channel
                .parent_channel_id
                .as_ref()
                .is_some_and(|id| DiscordSnowflake::parse(id).is_some() && id != &channel.id),
        }
}

/// Passive boundary for unrelated route tests/embedding: never contacts Discord.
pub(crate) struct UnconfiguredDiscordTransport;
#[async_trait::async_trait]
impl DiscordTransport for UnconfiguredDiscordTransport {
    async fn get_me(
        &self,
        _: &DiscordBotToken,
    ) -> Result<DiscordBotIdentity, DiscordTransportError> {
        Err(DiscordTransportError::Configuration)
    }
    async fn get_channel(
        &self,
        _: &DiscordBotToken,
        _: &str,
    ) -> Result<DiscordChannelMetadata, DiscordTransportError> {
        Err(DiscordTransportError::Configuration)
    }
    async fn latest_cursor(
        &self,
        _: &DiscordBotToken,
        _: &str,
    ) -> Result<String, DiscordTransportError> {
        Err(DiscordTransportError::Configuration)
    }
    async fn receive(
        &self,
        _: &DiscordBotToken,
        _: &str,
        _: &str,
    ) -> Result<DiscordMessageBatch, DiscordTransportError> {
        Err(DiscordTransportError::Configuration)
    }
    async fn send_chunk(
        &self,
        _: &DiscordBotToken,
        _: &str,
        _: &str,
        _: &str,
    ) -> Result<DiscordSendReceipt, DiscordTransportError> {
        Err(DiscordTransportError::Configuration)
    }
}
impl DiscordManager {
    pub(crate) fn unconfigured(state: SharedDaemonState, runs: AgentRunCoordinator) -> Self {
        Self::new(
            state,
            runs,
            Arc::new(credentials::InMemoryDiscordCredentialStore::default()),
            Arc::new(ConnectorRegistry::with_discord(
                Arc::new(crate::app::DeterministicTelegramTransport),
                Arc::new(UnconfiguredDiscordTransport),
            )),
        )
    }
}
