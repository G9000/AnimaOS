//! Explicit speech for one saved owner-selected reply. Payloads and audio are ephemeral.
use crate::{
    app::SharedDaemonState,
    history::{HistoryMessage, HistoryStore, SharedHistory},
};
use anima_core::{
    CancelSignal, MessageRole, SpeechAudio, SpeechAvailability, SpeechError, SpeechPort, SpeechText,
};
use axum::body::Bytes;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

const MAX_OPERATIONS: usize = 64;
const MAX_ACTIVE: usize = 2;
const TOMBSTONE_TTL: Duration = Duration::from_secs(300);
const OPERATION_BUDGET: Duration = Duration::from_secs(45);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct OperationKey {
    agent: String,
    session: String,
    operation: uuid::Uuid,
}
struct Entry {
    cancel: CancelSignal,
    terminal: Option<Instant>,
}
type Operations = Arc<Mutex<HashMap<OperationKey, Entry>>>;

#[derive(Clone)]
pub(crate) struct SpeechService {
    port: Option<Arc<dyn SpeechPort>>,
    availability: SpeechAvailability,
    operations: Operations,
    outputs: Arc<Semaphore>,
    budget: Duration,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ServiceError {
    NotFound,
    Duplicate,
    Busy,
    Unavailable,
    Cancelled,
    TimedOut,
    InvalidInput,
    Failed,
}

/// The lease remains with the actual WAV allocation through every Bytes alias.
pub(crate) struct SpeechOutput {
    audio: SpeechAudio,
    _lease: OwnedSemaphorePermit,
}
impl AsRef<[u8]> for SpeechOutput {
    fn as_ref(&self) -> &[u8] {
        self.audio.wav_bytes()
    }
}
impl SpeechOutput {
    pub(crate) fn duration_ms(&self) -> u32 {
        self.audio.duration_ms()
    }
    pub(crate) fn into_bytes(self) -> Bytes {
        Bytes::from_owner(self)
    }
}
struct ActiveOperation {
    operations: Operations,
    key: OperationKey,
    cancel: CancelSignal,
    lease: Option<OwnedSemaphorePermit>,
}
impl Drop for ActiveOperation {
    fn drop(&mut self) {
        if let Some(entry) = self
            .operations
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_mut(&self.key)
        {
            entry.terminal = Some(Instant::now());
        }
    }
}
struct AwaitOwner(CancelSignal);
impl Drop for AwaitOwner {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

struct Source {
    root: Option<PathBuf>,
    created: u64,
    history: SharedHistory,
    store: Arc<dyn HistoryStore>,
    record: HistoryMessage,
}
fn prune(map: &mut HashMap<OperationKey, Entry>, now: Instant) {
    map.retain(|_, e| {
        e.terminal
            .is_none_or(|at| now.saturating_duration_since(at) < TOMBSTONE_TTL)
    });
}
fn make_room(map: &mut HashMap<OperationKey, Entry>) -> Result<(), ServiceError> {
    if map.len() < MAX_OPERATIONS {
        return Ok(());
    }
    let oldest = map
        .iter()
        .filter_map(|(key, e)| e.terminal.map(|at| (key.clone(), at)))
        .min_by_key(|(_, at)| *at)
        .map(|(key, _)| key);
    if let Some(key) = oldest {
        map.remove(&key);
        Ok(())
    } else {
        Err(ServiceError::Busy)
    }
}
impl SpeechService {
    pub(crate) fn disabled() -> Self {
        Self::configured(SpeechAvailability::Disabled, None)
    }
    pub(crate) fn configured(
        availability: SpeechAvailability,
        port: Option<Arc<dyn SpeechPort>>,
    ) -> Self {
        Self {
            port,
            availability,
            operations: Arc::new(Mutex::new(HashMap::new())),
            outputs: Arc::new(Semaphore::new(MAX_ACTIVE)),
            budget: OPERATION_BUDGET,
        }
    }
    pub(crate) fn request_timeout(&self) -> Duration {
        self.budget.saturating_add(Duration::from_secs(1))
    }
    pub(crate) fn availability(&self) -> SpeechAvailability {
        self.port
            .as_ref()
            .map_or(self.availability, |p| p.availability())
    }
    fn key(agent: &str, session: &str, operation: uuid::Uuid) -> OperationKey {
        OperationKey {
            agent: agent.into(),
            session: session.into(),
            operation,
        }
    }
    fn admit(&self, key: OperationKey) -> Result<ActiveOperation, ServiceError> {
        let mut map = self.operations.lock().unwrap_or_else(|e| e.into_inner());
        prune(&mut map, Instant::now());
        if map.contains_key(&key) {
            return Err(ServiceError::Duplicate);
        }
        if map.values().filter(|e| e.terminal.is_none()).count() >= MAX_ACTIVE {
            return Err(ServiceError::Busy);
        }
        // Nonqueued physical capacity is reserved before any source/provider IO.
        let lease = self
            .outputs
            .clone()
            .try_acquire_owned()
            .map_err(|_| ServiceError::Busy)?;
        make_room(&mut map)?;
        let cancel = CancelSignal::new();
        map.insert(
            key.clone(),
            Entry {
                cancel: cancel.clone(),
                terminal: None,
            },
        );
        Ok(ActiveOperation {
            operations: self.operations.clone(),
            key,
            cancel,
            lease: Some(lease),
        })
    }
    pub(crate) fn cancel(
        &self,
        agent: &str,
        session: &str,
        operation: uuid::Uuid,
    ) -> Result<(), ServiceError> {
        let mut map = self.operations.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        prune(&mut map, now);
        let key = Self::key(agent, session, operation);
        if let Some(e) = map.get_mut(&key) {
            e.cancel.cancel();
            return Ok(());
        }
        make_room(&mut map)?;
        let cancel = CancelSignal::new();
        cancel.cancel();
        map.insert(
            key,
            Entry {
                cancel,
                terminal: Some(now),
            },
        );
        Ok(())
    }
    pub(crate) async fn generate(
        &self,
        state: SharedDaemonState,
        agent: String,
        session: String,
        operation: uuid::Uuid,
        message: String,
        expected: String,
    ) -> Result<SpeechOutput, ServiceError> {
        let deadline = Instant::now() + self.budget;
        let text = SpeechText::new(expected).map_err(speech_error)?;
        let mut active = self.admit(Self::key(&agent, &session, operation))?;
        let await_owner = AwaitOwner(active.cancel.clone());
        let service = self.clone();
        let task = tokio::spawn(async move {
            let cancel = active.cancel.clone();
            let work = service.perform(&state, &agent, &session, &message, text, &cancel, deadline);
            let result = tokio::select! { biased;
                () = cancel.cancelled() => Err(ServiceError::Cancelled),
                () = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)) => Err(ServiceError::TimedOut),
                result = work => result,
            };
            // DELETE and publication linearize under the same operation-map lock.
            let mut map = active.operations.lock().unwrap_or_else(|e| e.into_inner());
            let result = if cancel.is_cancelled() {
                Err(ServiceError::Cancelled)
            } else if Instant::now() >= deadline {
                Err(ServiceError::TimedOut)
            } else {
                result
            };
            if let Some(entry) = map.get_mut(&active.key) {
                entry.terminal = Some(Instant::now());
            }
            drop(map);
            result.map(|audio| SpeechOutput {
                audio,
                _lease: active.lease.take().expect("active output lease"),
            })
        });
        let result = task.await.map_err(|_| ServiceError::Failed)?;
        drop(await_owner);
        result
    }
    async fn perform(
        &self,
        state: &SharedDaemonState,
        agent: &str,
        session: &str,
        message: &str,
        text: SpeechText,
        cancel: &CancelSignal,
        deadline: Instant,
    ) -> Result<SpeechAudio, ServiceError> {
        if self.availability() != SpeechAvailability::Available {
            return Err(ServiceError::Unavailable);
        }
        let port = self.port.as_ref().ok_or(ServiceError::Unavailable)?;
        let (root, created, history, store) = {
            let guard = state.read().await;
            check_scope(&guard, agent, session)?;
            (
                guard.workspace.as_ref().map(|w| w.root_path.clone()),
                guard
                    .sessions
                    .get(agent, session)
                    .ok_or(ServiceError::NotFound)?
                    .created_at_ms,
                guard.history.clone(),
                guard.history.store(),
            )
        };
        let record = store
            .get_message(agent, session, message)
            .await
            .map_err(|_| ServiceError::Unavailable)?
            .ok_or(ServiceError::NotFound)?;
        if record.agent_id != agent
            || record.session_id != session
            || record.message.id != message
            || record.message.agent_id != agent
            || record.message.room_id != session
            || record.hidden
            || record.message.role != MessageRole::Assistant
            || record.message.content.text != text.as_str()
        {
            return Err(ServiceError::NotFound);
        }
        let source = Source {
            root,
            created,
            history,
            store,
            record,
        };
        self.revalidate(state, agent, session, message, &source, cancel, deadline)
            .await?;
        let audio = port
            .synthesize(text, cancel.clone(), deadline)
            .await
            .map_err(speech_error)?;
        self.revalidate(state, agent, session, message, &source, cancel, deadline)
            .await?;
        Ok(audio)
    }
    async fn revalidate(
        &self,
        state: &SharedDaemonState,
        agent: &str,
        session: &str,
        message: &str,
        source: &Source,
        cancel: &CancelSignal,
        deadline: Instant,
    ) -> Result<(), ServiceError> {
        if source
            .store
            .get_message(agent, session, message)
            .await
            .map_err(|_| ServiceError::Unavailable)?
            .as_ref()
            != Some(&source.record)
        {
            return Err(ServiceError::NotFound);
        }
        let guard = state.read().await;
        check_scope(&guard, agent, session)?;
        if guard.workspace.as_ref().map(|w| &w.root_path) != source.root.as_ref()
            || guard.sessions.get(agent, session).map(|s| s.created_at_ms) != Some(source.created)
            || !Arc::ptr_eq(&guard.history, &source.history)
            || !Arc::ptr_eq(&guard.history.store(), &source.store)
        {
            return Err(ServiceError::NotFound);
        }
        if cancel.is_cancelled() {
            return Err(ServiceError::Cancelled);
        }
        if Instant::now() >= deadline {
            return Err(ServiceError::TimedOut);
        }
        Ok(())
    }
}
pub(crate) fn check_scope(
    state: &crate::state::DaemonState,
    agent: &str,
    session: &str,
) -> Result<(), ServiceError> {
    crate::voice::check_scope(state, agent, session).map_err(|_| ServiceError::NotFound)
}
fn speech_error(error: SpeechError) -> ServiceError {
    match error {
        SpeechError::Busy => ServiceError::Busy,
        SpeechError::Cancelled => ServiceError::Cancelled,
        SpeechError::TimedOut => ServiceError::TimedOut,
        SpeechError::Unavailable => ServiceError::Unavailable,
        SpeechError::InvalidInput => ServiceError::InvalidInput,
        SpeechError::InvalidResponse | SpeechError::Failed => ServiceError::Failed,
    }
}
#[cfg(test)]
mod tests;
