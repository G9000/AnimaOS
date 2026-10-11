mod attachments;
mod configuration;
mod delivery;
mod inbound;
mod lifecycle;
mod owner;
mod processing;
mod workers;

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex, Weak};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anima_core::connectors::{
    ConnectorReceiveRequest, ConnectorSendRequest, ConnectorTransport as AsyncConnectorTransport,
};
use anima_core::{Content, DataValue, MessageRole, TaskResult, TaskStatus};
use async_trait::async_trait;
use tokio::sync::{oneshot, watch, Mutex};
use tokio::task::JoinHandle;

use super::credentials::{ConnectorCredentialStore, CredentialStoreError, TelegramBotToken};
use super::registry::ConnectorRegistry;
use super::telegram::{
    TelegramClient, TelegramSentMessage, TelegramTransportError, TelegramUpdateBatch,
};
use super::{
    TelegramBotIdentity, TelegramConnectorRecord, TelegramCredentialCleanupIntent,
    TelegramInboundRecord, TelegramPendingPairing,
};
use crate::agent_runs::{AgentRunCoordinator, AgentRunRequest, RunRoom};
use crate::app::SharedDaemonState;
use crate::connectors::{InboundProcessingState, OutboundDeliveryState, TelegramOutboundRecord};
use crate::history::HistoryDeletion;
use crate::live::run_status_event;
use crate::routes::{AgentRunEnvelope, AgentRuntimeSnapshotResponse, ApiError, TaskResultResponse};
use crate::runs::{RunOutcome, RunSource, RunStatus};
use crate::schedules::{ScheduleOutcomeStatus, ScheduleSafeOutcome, ScheduleTarget};
use crate::state::DaemonState;

static CONNECTOR_ID_SEQUENCE: AtomicU64 = AtomicU64::new(1);
const DELIVERED_RETENTION_MS: u64 = 7 * 24 * 60 * 60 * 1000;
const MAX_RETAINED_DELIVERED: usize = 1000;
const POLL_RETRY_INITIAL: Duration = Duration::from_millis(100);
const POLL_RETRY_MAX: Duration = Duration::from_secs(30);
const PAIRING_CANDIDATE_TTL_MS: u64 = 10 * 60 * 1000;
const MAX_RETAINED_TERMINAL_INBOUND: usize = 1000;
const MAX_LIVE_INBOUND: usize = 100;
const MAX_UNDELIVERED_OUTBOUND: usize = 100;

#[async_trait]
pub(crate) trait TelegramTransport: Send + Sync {
    async fn get_me(
        &self,
        token: &TelegramBotToken,
    ) -> Result<TelegramBotIdentity, TelegramTransportError>;
    async fn get_updates(
        &self,
        token: &TelegramBotToken,
        offset: i64,
    ) -> Result<TelegramUpdateBatch, TelegramTransportError>;
    async fn send_message(
        &self,
        token: &TelegramBotToken,
        chat_id: &str,
        text: &str,
    ) -> Result<Vec<TelegramSentMessage>, TelegramTransportError>;
}

#[async_trait]
impl TelegramTransport for TelegramClient {
    async fn get_me(
        &self,
        token: &TelegramBotToken,
    ) -> Result<TelegramBotIdentity, TelegramTransportError> {
        TelegramClient::get_me(self, token).await
    }

    async fn get_updates(
        &self,
        token: &TelegramBotToken,
        offset: i64,
    ) -> Result<TelegramUpdateBatch, TelegramTransportError> {
        TelegramClient::get_updates(self, token, offset).await
    }

    async fn send_message(
        &self,
        token: &TelegramBotToken,
        chat_id: &str,
        text: &str,
    ) -> Result<Vec<TelegramSentMessage>, TelegramTransportError> {
        TelegramClient::send_message(self, token, chat_id, text).await
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConnectorRuntimeStatus {
    Ready,
    Pairing,
    CredentialRequired,
    Error,
    Degraded,
    Reconciling,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConnectorManagerError {
    AgentNotFound,
    AgentBusy,
    ConnectorNotFound,
    AgentAlreadyConnected,
    PendingPairingNotFound,
    InvalidToken,
    Transport,
    Credential,
    CredentialStateUncertain,
    Persistence,
    Backpressure,
    ConflictingUpdate,
    IdempotencyConflict,
    WorkerStopped,
}

impl fmt::Display for ConnectorManagerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::AgentNotFound => "agent not found",
            Self::AgentBusy => "agent has a run in progress",
            Self::ConnectorNotFound => "connector not found",
            Self::AgentAlreadyConnected => "agent already has an active Telegram connector",
            Self::PendingPairingNotFound => "Telegram pairing candidate was not found",
            Self::InvalidToken => "Telegram bot token is invalid",
            Self::Transport => "Telegram transport failed",
            Self::Credential => "credential vault operation failed",
            Self::CredentialStateUncertain => "credential vault state requires reconciliation",
            Self::Persistence => "connector state persistence failed",
            Self::Backpressure => "connector inbound capacity is exhausted",
            Self::ConflictingUpdate => "Telegram update conflicts with durable history",
            Self::IdempotencyConflict => "connector idempotency key conflicts with committed text",
            Self::WorkerStopped => "connector worker stopped unexpectedly",
        })
    }
}

impl std::error::Error for ConnectorManagerError {}

#[derive(Clone)]
pub(crate) struct ConnectorManager {
    state: SharedDaemonState,
    runs: AgentRunCoordinator,
    credentials: Arc<dyn ConnectorCredentialStore>,
    registry: Arc<ConnectorRegistry>,
    lifecycle_lock: Arc<Mutex<()>>,
    mutation_lock: Arc<Mutex<()>>,
    statuses: Arc<Mutex<HashMap<String, ConnectorRuntimeStatus>>>,
    workers: Arc<Mutex<HashMap<String, WorkerHandle>>>,
    owner_send_locks: Arc<StdMutex<HashMap<(String, String), Weak<Mutex<()>>>>>,
    delivery_claims: Arc<StdMutex<delivery::DeliveryClaims>>,
    worker_generation: Arc<AtomicU64>,
    closing: Arc<AtomicBool>,
    owner_close: Option<watch::Sender<bool>>,
}

struct WorkerHandle {
    generation: u64,
    cancel: watch::Sender<bool>,
    join: JoinHandle<()>,
}

#[derive(Default)]
struct ConnectorRunRollbackDelta {
    committed_target: Option<TelegramInboundRecord>,
    inserted_outbound: Option<TelegramOutboundRecord>,
    removed_terminal: Vec<((String, i64), TelegramInboundRecord)>,
}

impl ConnectorManager {
    pub(crate) fn new(
        state: SharedDaemonState,
        runs: AgentRunCoordinator,
        credentials: Arc<dyn ConnectorCredentialStore>,
        transport: Arc<dyn TelegramTransport>,
    ) -> Self {
        let mutation_lock = runs.control_plane_transactions();
        let (owner_close, _) = watch::channel(false);
        Self {
            state,
            runs,
            credentials,
            registry: Arc::new(ConnectorRegistry::new(transport)),
            lifecycle_lock: Arc::new(Mutex::new(())),
            mutation_lock,
            statuses: Arc::new(Mutex::new(HashMap::new())),
            workers: Arc::new(Mutex::new(HashMap::new())),
            owner_send_locks: Arc::new(StdMutex::new(HashMap::new())),
            delivery_claims: Arc::new(StdMutex::new(delivery::DeliveryClaims::default())),
            worker_generation: Arc::new(AtomicU64::new(1)),
            closing: Arc::new(AtomicBool::new(false)),
            owner_close: Some(owner_close),
        }
    }

    /// Shutdown has begun: no new work starts (see `ensure_open`).
    pub(crate) fn is_closing(&self) -> bool {
        self.closing.load(Ordering::SeqCst)
    }

    fn ensure_open(&self) -> Result<(), ConnectorManagerError> {
        if self.closing.load(Ordering::SeqCst) {
            Err(ConnectorManagerError::WorkerStopped)
        } else {
            Ok(())
        }
    }

    fn owner_send_lock(&self, connector_id: &str, idempotency_key: &str) -> Arc<Mutex<()>> {
        let mut locks = self
            .owner_send_locks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        locks.retain(|_, lock| lock.strong_count() > 0);
        let key = (connector_id.to_string(), idempotency_key.to_string());
        if let Some(lock) = locks.get(&key).and_then(Weak::upgrade) {
            return lock;
        }
        let lock = Arc::new(Mutex::new(()));
        locks.insert(key, Arc::downgrade(&lock));
        lock
    }
}

/// What an owner send's key already answers (M1 F16; final fix wave S2-D).
enum OwnerSendReplay {
    /// No earlier send has this key: the send runs.
    New,
    /// The earlier send's answer, and whether its reply is queued for delivery.
    Answered(AgentRunEnvelope, bool),
    /// The earlier send completed, but its reply left the hot tail: read from
    /// the history store once the locks are released.
    FromHistory {
        snapshot: anima_core::AgentRuntimeSnapshot,
        session_id: String,
        reply_id: String,
        delivery_queued: bool,
        store: Arc<dyn crate::history::HistoryStore>,
    },
}

fn replay_envelope(
    snapshot: &anima_core::AgentRuntimeSnapshot,
    result: TaskResult<Content>,
) -> AgentRunEnvelope {
    AgentRunEnvelope {
        agent: AgentRuntimeSnapshotResponse::from(snapshot),
        result: TaskResultResponse::from(&result),
    }
}

/// Whether the connector still has `reply_id` to deliver, or delivered it; a
/// reply a stop suppressed is never sent (spec §4.6).
fn reply_delivery_queued(
    state: &DaemonState,
    connector: &TelegramConnectorRecord,
    reply_id: &str,
) -> bool {
    state.outbound.values().any(|outbound| {
        outbound.connector_id == connector.id
            && outbound.assistant_message_id == reply_id
            && outbound.delivery_state != OutboundDeliveryState::Suppressed
    })
}

/// The answer to an owner send whose key was already used: its run's ledger
/// record when the ledger holds one, from its status and error (a stopped or
/// failed run is not a success, and a completed run's reply is read from the
/// history store once it left the hot tail); otherwise, for sends from
/// before the ledger, the turn found in the transcript.
fn owner_send_replay(
    state: &DaemonState,
    connector: &TelegramConnectorRecord,
    text: &str,
    idempotency_key: &str,
) -> Result<OwnerSendReplay, ConnectorManagerError> {
    let snapshot = state
        .get_agent(&connector.agent_id)
        .ok_or(ConnectorManagerError::AgentNotFound)?;
    let session_id = crate::sessions::session_id_for_room(&connector.room_id);
    let record = state
        .runs
        .for_session(&connector.agent_id, &session_id)
        .into_iter()
        .find(|record| record.idempotency_key.as_deref() == Some(idempotency_key));
    let Some(record) = record else {
        return transcript_owner_send_replay(state, connector, snapshot, text, idempotency_key);
    };
    if record.input.text != text {
        return Err(ConnectorManagerError::IdempotencyConflict);
    }
    if !record.status.is_terminal() {
        // Still queued or running: its answer is not known yet.
        return Err(ConnectorManagerError::AgentBusy);
    }
    if record.status != RunStatus::Completed {
        let message = record
            .error
            .as_ref()
            .map(|error| error.message.clone())
            .unwrap_or_else(|| format!("The run ended {}", record.status.as_str()));
        return Ok(OwnerSendReplay::Answered(
            replay_envelope(&snapshot, TaskResult::error(message, 0)),
            false,
        ));
    }
    let Some(reply_id) = record.reply_message_id.clone() else {
        // A run completed before replies were saved with their runs.
        return transcript_owner_send_replay(state, connector, snapshot, text, idempotency_key);
    };
    let delivery_queued = reply_delivery_queued(state, connector, &reply_id);
    let reply = snapshot
        .messages
        .iter()
        .find(|message| message.id == reply_id)
        .map(|message| message.content.clone());
    Ok(match reply {
        Some(reply) => OwnerSendReplay::Answered(
            replay_envelope(&snapshot, TaskResult::success(reply, 0)),
            delivery_queued,
        ),
        None => OwnerSendReplay::FromHistory {
            snapshot,
            session_id,
            reply_id,
            delivery_queued,
            store: state.history.store(),
        },
    })
}

/// A send from before the ledger: its turn in the transcript, and the first
/// assistant message after it.
fn transcript_owner_send_replay(
    state: &DaemonState,
    connector: &TelegramConnectorRecord,
    snapshot: anima_core::AgentRuntimeSnapshot,
    text: &str,
    idempotency_key: &str,
) -> Result<OwnerSendReplay, ConnectorManagerError> {
    let Some((user_index, user)) = snapshot.messages.iter().enumerate().find(|(_, message)| {
        message.room_id == connector.room_id
            && message.role == MessageRole::User
            && message.content.metadata.as_ref().is_some_and(|metadata| {
                matches!(
                    metadata.get("idempotencyKey"),
                    Some(DataValue::String(value)) if value == idempotency_key
                ) && matches!(
                    metadata.get("connectorId"),
                    Some(DataValue::String(value)) if value == &connector.id
                )
            })
    }) else {
        return Ok(OwnerSendReplay::New);
    };
    if user.content.text != text {
        return Err(ConnectorManagerError::IdempotencyConflict);
    }
    let Some(assistant) = snapshot
        .messages
        .iter()
        .skip(user_index + 1)
        .find(|message| {
            message.room_id == connector.room_id && message.role == MessageRole::Assistant
        })
    else {
        return Ok(OwnerSendReplay::New);
    };
    let delivery_queued = reply_delivery_queued(state, connector, &assistant.id);
    let result = TaskResult::success(assistant.content.clone(), 0);
    Ok(OwnerSendReplay::Answered(
        replay_envelope(&snapshot, result),
        delivery_queued,
    ))
}

fn map_credential_error(error: CredentialStoreError) -> ConnectorManagerError {
    match error {
        CredentialStoreError::CredentialStateUncertain => {
            ConnectorManagerError::CredentialStateUncertain
        }
        _ => ConnectorManagerError::Credential,
    }
}

fn map_token_validation_error(error: TelegramTransportError) -> ConnectorManagerError {
    if is_invalid_token(error) {
        ConnectorManagerError::InvalidToken
    } else {
        ConnectorManagerError::Transport
    }
}

fn is_invalid_token(error: TelegramTransportError) -> bool {
    matches!(
        error,
        TelegramTransportError::HttpStatus {
            status: 400 | 401 | 404
        } | TelegramTransportError::UpstreamApi {
            code: Some(400 | 401 | 404)
        }
    )
}

fn is_revoked_credential(error: TelegramTransportError) -> bool {
    matches!(
        error,
        TelegramTransportError::HttpStatus { status: 401 }
            | TelegramTransportError::UpstreamApi { code: Some(401) }
    )
}

fn next_poll_backoff(current: Duration) -> Duration {
    current.saturating_mul(2).min(POLL_RETRY_MAX)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn next_connector_id(now: u64) -> String {
    let sequence = CONNECTOR_ID_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("telegram-{now}-{sequence}")
}

fn same_inbound_identity(
    existing: &TelegramInboundRecord,
    candidate: &TelegramInboundRecord,
) -> bool {
    existing.connector_id == candidate.connector_id
        && existing.update_id == candidate.update_id
        && existing.agent_id == candidate.agent_id
        && existing.room_id == candidate.room_id
        && existing.normalized_text == candidate.normalized_text
        && existing.sender == candidate.sender
        && existing.chat == candidate.chat
        && existing.run_idempotency_key == candidate.run_idempotency_key
}

fn connector_operational_status(connector: &TelegramConnectorRecord) -> ConnectorRuntimeStatus {
    if connector.approved_chat.is_some() {
        ConnectorRuntimeStatus::Ready
    } else {
        ConnectorRuntimeStatus::Pairing
    }
}

fn compact_delivered_outbox(
    outbox: &mut HashMap<String, TelegramOutboundRecord>,
    connector_id: &str,
    now: u64,
) {
    let cutoff = now.saturating_sub(DELIVERED_RETENTION_MS);
    // Suppressed replies (spec §4.6) age out like delivered ones, by when
    // they were committed.
    let settled_at = |record: &TelegramOutboundRecord| match record.delivery_state {
        OutboundDeliveryState::Delivered => record.delivered_at_ms,
        OutboundDeliveryState::Suppressed => Some(record.created_at_ms),
        OutboundDeliveryState::Pending | OutboundDeliveryState::Failed => None,
    };
    outbox.retain(|_, record| {
        record.connector_id != connector_id
            || !record.delivery_state.is_settled()
            || settled_at(record).is_none_or(|at| at >= cutoff)
    });
    let mut settled = outbox
        .values()
        .filter(|record| record.connector_id == connector_id && record.delivery_state.is_settled())
        .map(|record| {
            (
                settled_at(record).unwrap_or(record.created_at_ms),
                record.id.clone(),
            )
        })
        .collect::<Vec<_>>();
    let excess = settled.len().saturating_sub(MAX_RETAINED_DELIVERED);
    if excess == 0 {
        return;
    }
    settled.sort();
    for (_, id) in settled.into_iter().take(excess) {
        outbox.remove(&id);
    }
}

fn compact_terminal_inbound(
    inbound: &mut HashMap<(String, i64), TelegramInboundRecord>,
    connector_id: &str,
) -> Vec<((String, i64), TelegramInboundRecord)> {
    let mut terminal = inbound
        .iter()
        .filter(|((record_connector_id, _), record)| {
            record_connector_id == connector_id && record.processing_state.is_terminal()
        })
        .map(|(key, record)| (record.received_at_ms, record.update_id, key.clone()))
        .collect::<Vec<_>>();
    let excess = terminal.len().saturating_sub(MAX_RETAINED_TERMINAL_INBOUND);
    if excess == 0 {
        return Vec::new();
    }
    terminal.sort();
    let mut removed = Vec::with_capacity(excess);
    for (_, _, key) in terminal.into_iter().take(excess) {
        if let Some(record) = inbound.remove(&key) {
            removed.push((key, record));
        }
    }
    removed
}

fn receiver_stopped(receiver: &watch::Receiver<bool>) -> bool {
    *receiver.borrow() || receiver.has_changed().is_err()
}

async fn wait_or_stop(
    cancel: &mut watch::Receiver<bool>,
    owner_close: &mut watch::Receiver<bool>,
    duration: Duration,
) -> bool {
    tokio::select! {
        _ = tokio::time::sleep(duration) => false,
        changed = cancel.changed() => {
            let _ = changed;
            true
        }
        changed = owner_close.changed() => {
            let _ = changed;
            true
        }
    }
}

#[cfg(test)]
mod stop_tests;

#[cfg(test)]
mod tests;
