//! Explicit, ephemeral owner draft operations. No operation payload is persisted.
use crate::{
    app::SharedDaemonState,
    attachments::{
        documents::{
            admission::{read_source, SourceReader},
            worker::WorkerFailure,
        },
        files::read_attachment_bytes,
        AttachmentKind, AttachmentRecord,
    },
    sessions::{SessionKind, SessionOrigin},
};
use anima_core::{
    audio_content::{
        validate_transcript_text, AudioValidationError, TranscriptionAvailability,
        TranscriptionError, TranscriptionPort, WavAudio,
    },
    CancelSignal,
};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

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
pub(crate) struct VoiceService {
    port: Option<Arc<dyn TranscriptionPort>>,
    availability: TranscriptionAvailability,
    operations: Operations,
    reader: SourceReader,
    budget: Duration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum VoiceError {
    NotFound,
    Duplicate,
    Busy,
    Unavailable,
    Cancelled,
    TimedOut,
    InvalidSource,
    InvalidAudio,
    AudioTooLarge,
    Failed,
}

/// Retained by the owned operation task, never by the HTTP future alone.
struct ActiveOperation {
    operations: Operations,
    key: OperationKey,
    cancel: CancelSignal,
}
impl Drop for ActiveOperation {
    fn drop(&mut self) {
        let mut map = self.operations.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = map.get_mut(&self.key) {
            entry.terminal = Some(Instant::now());
        }
    }
}
/// A lost HTTP await requests local cancellation; it cannot release active work.
struct AwaitOwner(CancelSignal);
impl Drop for AwaitOwner {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

fn prune(map: &mut HashMap<OperationKey, Entry>, now: Instant) {
    map.retain(|_, entry| {
        entry
            .terminal
            .is_none_or(|at| now.saturating_duration_since(at) < TOMBSTONE_TTL)
    });
}
fn make_room(map: &mut HashMap<OperationKey, Entry>) -> Result<(), VoiceError> {
    if map.len() < MAX_OPERATIONS {
        return Ok(());
    }
    let oldest = map
        .iter()
        .filter_map(|(key, entry)| entry.terminal.map(|at| (key.clone(), at)))
        .min_by_key(|(_, at)| *at)
        .map(|(key, _)| key);
    match oldest {
        Some(key) => {
            map.remove(&key);
            Ok(())
        }
        None => Err(VoiceError::Busy),
    }
}

impl VoiceService {
    /// Covers body and scope waits without cutting off the45s operation budget.
    pub(crate) fn request_timeout(&self) -> Duration {
        self.budget.saturating_add(Duration::from_secs(1))
    }
    pub(crate) fn disabled() -> Self {
        Self::configured(TranscriptionAvailability::Disabled, None)
    }
    pub(crate) fn configured(
        availability: TranscriptionAvailability,
        port: Option<Arc<dyn TranscriptionPort>>,
    ) -> Self {
        Self {
            port,
            availability,
            operations: Arc::new(Mutex::new(HashMap::new())),
            reader: Arc::new(read_attachment_bytes),
            budget: OPERATION_BUDGET,
        }
    }
    pub(crate) fn availability(&self) -> TranscriptionAvailability {
        self.port
            .as_ref()
            .map_or(self.availability, |port| port.availability())
    }
    fn key(agent: &str, session: &str, operation: uuid::Uuid) -> OperationKey {
        OperationKey {
            agent: agent.into(),
            session: session.into(),
            operation,
        }
    }
    fn admit(&self, key: OperationKey) -> Result<ActiveOperation, VoiceError> {
        let mut map = self.operations.lock().unwrap_or_else(|e| e.into_inner());
        prune(&mut map, Instant::now());
        if map.contains_key(&key) {
            return Err(VoiceError::Duplicate);
        }
        if map
            .values()
            .filter(|entry| entry.terminal.is_none())
            .count()
            >= MAX_ACTIVE
        {
            return Err(VoiceError::Busy);
        }
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
            operations: Arc::clone(&self.operations),
            key,
            cancel,
        })
    }
    pub(crate) fn cancel(
        &self,
        agent: &str,
        session: &str,
        operation: uuid::Uuid,
    ) -> Result<(), VoiceError> {
        let key = Self::key(agent, session, operation);
        let mut map = self.operations.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        prune(&mut map, now);
        if let Some(entry) = map.get_mut(&key) {
            entry.cancel.cancel();
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
    pub(crate) async fn transcribe(
        &self,
        state: SharedDaemonState,
        agent: String,
        session: String,
        operation: uuid::Uuid,
        attachment: String,
    ) -> Result<String, VoiceError> {
        let deadline = Instant::now() + self.budget;
        // Admission precedes every file or provider await and survives lost clients.
        let active = self.admit(Self::key(&agent, &session, operation))?;
        let await_owner = AwaitOwner(active.cancel.clone());
        let service = self.clone();
        let task = tokio::spawn(async move {
            let cancel = active.cancel.clone();
            let work = service.perform(&state, &agent, &session, &attachment, &cancel, deadline);
            let result = tokio::select! { biased;
                () = cancel.cancelled() => Err(VoiceError::Cancelled),
                () = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)) => Err(VoiceError::TimedOut),
                result = work => result,
            };
            // Linearize known cancellation against publication under the same map lock.
            let mut map = active.operations.lock().unwrap_or_else(|e| e.into_inner());
            let result = if cancel.is_cancelled() {
                Err(VoiceError::Cancelled)
            } else if Instant::now() >= deadline {
                Err(VoiceError::TimedOut)
            } else {
                result
            };
            if let Some(entry) = map.get_mut(&active.key) {
                entry.terminal = Some(Instant::now());
            }
            drop(map);
            drop(active);
            result
        });
        let result = task.await.map_err(|_| VoiceError::Failed)?;
        drop(await_owner);
        result
    }
    async fn perform(
        &self,
        state: &SharedDaemonState,
        agent: &str,
        session: &str,
        attachment: &str,
        cancel: &CancelSignal,
        deadline: Instant,
    ) -> Result<String, VoiceError> {
        if self.availability() != TranscriptionAvailability::Available {
            return Err(VoiceError::Unavailable);
        }
        let port = self.port.as_ref().ok_or(VoiceError::Unavailable)?;
        let (root, created, store) = {
            let guard = state.read().await;
            check_scope(&guard, agent, session)?;
            let root = guard
                .workspace
                .as_ref()
                .ok_or(VoiceError::NotFound)?
                .root_path
                .clone();
            (
                root,
                guard
                    .sessions
                    .get(agent, session)
                    .ok_or(VoiceError::NotFound)?
                    .created_at_ms,
                guard.history.store(),
            )
        };
        let record = store
            .get_attachment(agent, attachment)
            .await
            .map_err(|_| VoiceError::Unavailable)?
            .filter(|r| r.id == attachment && r.agent_id == agent && r.session_id == session)
            .ok_or(VoiceError::NotFound)?;
        if record.kind != AttachmentKind::Audio || record.mime != "audio/wav" {
            return Err(VoiceError::InvalidSource);
        }
        let bytes = self
            .read(root.clone(), record.clone(), cancel, deadline)
            .await?;
        let audio = WavAudio::parse(bytes).map_err(|error| match error {
            AudioValidationError::Unsupported => VoiceError::InvalidSource,
            AudioValidationError::TooLarge => VoiceError::AudioTooLarge,
            AudioValidationError::Malformed | AudioValidationError::TooLong => {
                VoiceError::InvalidAudio
            }
        })?;
        if store
            .get_attachment(agent, attachment)
            .await
            .map_err(|_| VoiceError::Unavailable)?
            .as_ref()
            != Some(&record)
        {
            return Err(VoiceError::NotFound);
        }
        {
            let guard = state.read().await;
            check_scope(&guard, agent, session)?;
            if guard.workspace.as_ref().map(|w| &w.root_path) != Some(&root)
                || guard.sessions.get(agent, session).map(|s| s.created_at_ms) != Some(created)
            {
                return Err(VoiceError::NotFound);
            }
        }
        if cancel.is_cancelled() {
            return Err(VoiceError::Cancelled);
        }
        if Instant::now() >= deadline {
            return Err(VoiceError::TimedOut);
        }
        let text = port
            .transcribe(audio, cancel.clone(), deadline)
            .await
            .map_err(transcription_error)?;
        let text = validate_transcript_text(&text).map_err(transcription_error)?;
        // Recheck the private immutable record and current physical source.
        let current = store
            .get_attachment(agent, attachment)
            .await
            .map_err(|_| VoiceError::Unavailable)?;
        if current.as_ref() != Some(&record) {
            return Err(VoiceError::NotFound);
        }
        drop(
            self.read(root.clone(), record.clone(), cancel, deadline)
                .await?,
        );
        if store
            .get_attachment(agent, attachment)
            .await
            .map_err(|_| VoiceError::Unavailable)?
            .as_ref()
            != Some(&record)
        {
            return Err(VoiceError::NotFound);
        }
        let guard = state.read().await;
        check_scope(&guard, agent, session)?;
        if guard.workspace.as_ref().map(|w| &w.root_path) != Some(&root)
            || guard.sessions.get(agent, session).map(|s| s.created_at_ms) != Some(created)
        {
            return Err(VoiceError::NotFound);
        }
        if cancel.is_cancelled() {
            return Err(VoiceError::Cancelled);
        }
        if Instant::now() >= deadline {
            return Err(VoiceError::TimedOut);
        }
        Ok(text)
    }
    async fn read(
        &self,
        root: PathBuf,
        record: AttachmentRecord,
        cancel: &CancelSignal,
        deadline: Instant,
    ) -> Result<Vec<u8>, VoiceError> {
        read_source(
            root,
            record,
            Arc::clone(&self.reader),
            cancel,
            tokio::time::Instant::from_std(deadline),
        )
        .await
        .map_err(|error| match error {
            WorkerFailure::Busy => VoiceError::Busy,
            WorkerFailure::Cancelled => VoiceError::Cancelled,
            WorkerFailure::TimedOut => VoiceError::TimedOut,
            _ => VoiceError::NotFound,
        })
    }
}

pub(crate) fn check_scope(
    state: &crate::state::DaemonState,
    agent: &str,
    session: &str,
) -> Result<(), VoiceError> {
    let scoped = state
        .sessions
        .get(agent, session)
        .filter(|s| s.kind == SessionKind::Chat && s.origin == SessionOrigin::Web);
    if !state.agents.contains_key(agent)
        || scoped.is_none()
        || state
            .pending_history_deletions
            .iter()
            .any(|d| d.agent_id == agent && d.session_id.as_ref().is_none_or(|id| id == session))
    {
        return Err(VoiceError::NotFound);
    }
    Ok(())
}
fn transcription_error(error: TranscriptionError) -> VoiceError {
    match error {
        TranscriptionError::Busy => VoiceError::Busy,
        TranscriptionError::Unavailable => VoiceError::Unavailable,
        TranscriptionError::Cancelled => VoiceError::Cancelled,
        TranscriptionError::TimedOut => VoiceError::TimedOut,
        TranscriptionError::InvalidResponse | TranscriptionError::Failed => VoiceError::Failed,
    }
}

#[cfg(test)]
mod tests;
