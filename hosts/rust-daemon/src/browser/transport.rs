//! Owned Windows Playwright transport. Lifecycle follows the maintained M44
//! Job Object owner; no MCP session or authority/catalog is copied.
use super::files::PrivateFiles;
pub(crate) use super::files::{OutputSlot, VerifiedInput, VerifiedOutput};
use super::{
    config::VerifiedBrowserInstallation,
    records::{digest_valid, valid_id},
};
pub(crate) use anima_service_adapters::browser::BrowserErrorCode;
use anima_service_adapters::browser::{
    scope_digest, validate_observation, validate_operation, validate_scope, BrowserArtifact,
    BrowserClose, BrowserControl, BrowserEffect, BrowserError, BrowserObservation,
    BrowserOperation, BrowserPort, BrowserScope, BrowserSession, MAX_BROWSER_SAFE_INTEGER,
};
use async_trait::async_trait;
use process_wrap::tokio::{ChildWrapper, CommandWrap, KillOnDrop};
use serde::{
    de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor},
    Deserialize, Serialize,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    process::Stdio,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex as StdMutex, Weak,
    },
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::Command,
    sync::{mpsc, oneshot, Notify, OwnedSemaphorePermit, Semaphore},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

pub(super) fn error(code: BrowserErrorCode) -> BrowserError {
    BrowserError {
        code,
        effect: BrowserEffect::NotDispatched,
    }
}
fn unknown(mut error: BrowserError) -> BrowserError {
    error.effect = BrowserEffect::MayHaveOccurred;
    error
}
fn lock<T>(mutex: &StdMutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
fn now() -> u64 {
    anima_core::primitives::now_millis()
}

#[derive(Clone)]
pub(crate) struct PreparedBrowserTask {
    pub(crate) scope: BrowserScope,
    pub(crate) policy_id: String,
    pub(crate) policy_digest: String,
    pub(crate) installation_id: String,
    pub(crate) installation_digest: String,
    pub(crate) expires_at_ms: u64,
    pub(crate) inputs: Vec<VerifiedInput>,
    pub(crate) outputs: Vec<OutputSlot>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BrowserDocumentWitness {
    pub(crate) document_id: String,
    pub(crate) document_generation: u64,
    pub(crate) observation_digest: String,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BrowserCounters {
    pub(crate) actions_used: u16,
    pub(crate) requests_used: u16,
    pub(crate) wire_bytes_used: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BrowserTransportReply {
    pub(crate) context_id: String,
    pub(crate) page_id: String,
    pub(crate) document: BrowserDocumentWitness,
    pub(crate) observation: BrowserObservation,
    pub(crate) counters: BrowserCounters,
}
#[derive(Clone, Debug, Default)]
pub(crate) struct BrowserTransportSnapshot {
    pub(crate) context_id: Option<String>,
    pub(crate) page_id: Option<String>,
    pub(crate) document: Option<BrowserDocumentWitness>,
    pub(crate) counters: BrowserCounters,
    pub(crate) physical_lease_held: bool,
    pub(crate) close: Option<BrowserClose>,
}
/// Private test projection for root's independent OS process inventory. These
/// timestamps bracket spawn; they are not kernel creation-time claims.
#[cfg(test)]
#[derive(Clone, Debug)]
pub(crate) struct BrowserPrivateWitness {
    pub(crate) node_pid: u32,
    pub(crate) spawn_started_at_ms: u64,
    pub(crate) spawn_returned_at_ms: u64,
    pub(crate) private_root: std::path::PathBuf,
    pub(crate) node_executable: std::path::PathBuf,
    pub(crate) browser_executable: std::path::PathBuf,
}
pub(crate) struct BoundBrowserTask {
    pub(crate) port: Arc<dyn BrowserPort>,
    pub(crate) handle: Arc<dyn BrowserTransportHandle>,
}
pub(crate) trait BrowserTransportFactory: Send + Sync {
    fn bind(&self, task: PreparedBrowserTask) -> Result<BoundBrowserTask, BrowserError>;
}
#[async_trait]
pub(crate) trait BrowserTransportHandle: Send + Sync {
    fn snapshot(&self) -> BrowserTransportSnapshot;
    #[cfg(test)]
    fn private_witness(&self) -> Option<BrowserPrivateWitness> {
        None
    }
    async fn execute_admitted(
        &self,
        operation: BrowserOperation,
        expected_witness: Option<BrowserDocumentWitness>,
        control: BrowserControl,
    ) -> Result<BrowserTransportReply, BrowserError>;
    async fn read_output(&self, artifact: &BrowserArtifact)
        -> Result<VerifiedOutput, BrowserError>;
    async fn close(&self) -> BrowserClose;
}
pub(crate) struct BrowserPhysicalRuntime {
    permit: Arc<Semaphore>,
    quarantine: StdMutex<Vec<Quarantine>>,
}
impl Default for BrowserPhysicalRuntime {
    fn default() -> Self {
        Self {
            permit: Arc::new(Semaphore::new(1)),
            quarantine: StdMutex::new(Vec::new()),
        }
    }
}
impl BrowserPhysicalRuntime {
    #[cfg(test)]
    pub(crate) fn child_count(&self) -> usize {
        1 - self.permit.available_permits()
    }
}
pub(crate) struct PlaywrightTransportFactory {
    installation: Arc<VerifiedBrowserInstallation>,
    runtime: Arc<BrowserPhysicalRuntime>,
}
impl PlaywrightTransportFactory {
    pub(crate) fn new(
        installation: VerifiedBrowserInstallation,
        runtime: Arc<BrowserPhysicalRuntime>,
    ) -> Self {
        Self {
            installation: Arc::new(installation),
            runtime,
        }
    }
}
impl BrowserTransportFactory for PlaywrightTransportFactory {
    fn bind(&self, task: PreparedBrowserTask) -> Result<BoundBrowserTask, BrowserError> {
        validate_scope(&task.scope)?;
        if !valid_id(&task.policy_id)
            || !digest_valid(&task.policy_digest)
            || task.installation_id != self.installation.id
            || task.installation_digest != self.installation.digest
            || task.expires_at_ms == 0
            || task.expires_at_ms > MAX_BROWSER_SAFE_INTEGER
        {
            return Err(error(BrowserErrorCode::Invalid));
        }
        super::files::validate_slots(&task.scope, &task.inputs, &task.outputs)?;
        let digest = scope_digest(&task.scope)?;
        let inner = Arc::new(Inner {
            task,
            digest,
            installation: self.installation.clone(),
            runtime: self.runtime.clone(),
            state: StdMutex::new(State::default()),
            opened: AtomicBool::new(false),
            stop: CancellationToken::new(),
            closed: Notify::new(),
        });
        Ok(BoundBrowserTask {
            port: Arc::new(BoundPort(inner.clone())),
            handle: Arc::new(Handle(inner)),
        })
    }
}
#[derive(Default)]
struct State {
    view: BrowserTransportSnapshot,
    sender: Option<mpsc::Sender<ActorCommand>>,
    files: Option<Arc<PrivateFiles>>,
    artifacts: Vec<BrowserArtifact>,
    operation_deadline: Option<std::time::Instant>,
    operation_cancel: Option<anima_core::CancelSignal>,
    admitted_actions: u16,
    reported: BrowserCounters,
    failure: Option<BrowserError>,
    #[cfg(test)]
    private_witness: Option<BrowserPrivateWitness>,
}
struct Inner {
    task: PreparedBrowserTask,
    digest: String,
    installation: Arc<VerifiedBrowserInstallation>,
    runtime: Arc<BrowserPhysicalRuntime>,
    state: StdMutex<State>,
    opened: AtomicBool,
    stop: CancellationToken,
    closed: Notify,
}
impl Inner {
    fn fail(&self, failure: BrowserError) {
        lock(&self.state).failure.get_or_insert(failure);
        self.stop.cancel();
    }
    fn snapshot(&self) -> BrowserTransportSnapshot {
        lock(&self.state).view.clone()
    }
    fn finish(&self, close: BrowserClose, held: bool) {
        let mut state = lock(&self.state);
        state.view.close = Some(close);
        state.view.physical_lease_held = held;
        state.sender = None;
        if close == BrowserClose::Closed {
            state.files = None;
        }
        drop(state);
        self.closed.notify_waiters();
    }
    async fn wait_closed(&self) -> BrowserClose {
        self.stop.cancel();
        if !self.opened.load(Ordering::SeqCst) {
            self.finish(BrowserClose::Closed, false);
        }
        loop {
            let notified = self.closed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if let Some(close) = lock(&self.state).view.close {
                return close;
            }
            notified.await;
        }
    }
}
struct BoundPort(Arc<Inner>);
struct Handle(Arc<Inner>);
struct Session(Arc<Inner>);
struct CancelOnDrop {
    stop: CancellationToken,
    armed: bool,
}
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if self.armed {
            self.stop.cancel();
        }
    }
}
impl Drop for Handle {
    fn drop(&mut self) {
        self.0.stop.cancel();
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        self.0.stop.cancel();
    }
}

#[async_trait]
impl BrowserPort for BoundPort {
    async fn open(
        &self,
        scope: BrowserScope,
        control: BrowserControl,
    ) -> Result<Box<dyn BrowserSession>, BrowserError> {
        check(&self.0, &control)?;
        if scope != self.0.task.scope || scope_digest(&scope)? != self.0.digest {
            return Err(error(BrowserErrorCode::Stale));
        }
        if self.0.opened.swap(true, Ordering::SeqCst) {
            return Err(error(BrowserErrorCode::Unavailable));
        }
        let inner = self.0.clone();
        let (sender, receiver) = oneshot::channel();
        let mut guard = CancelOnDrop {
            stop: inner.stop.clone(),
            armed: true,
        };
        tokio::spawn(async move {
            let result = start(inner.clone(), control).await;
            let prelaunch = {
                let state = lock(&inner.state);
                state.view.close.is_none() && !state.view.physical_lease_held
            };
            if result.is_err() && prelaunch {
                inner.finish(BrowserClose::Closed, false);
            }
            let _ =
                sender.send(result.map(|_| Box::new(Session(inner)) as Box<dyn BrowserSession>));
        });
        let result = receiver
            .await
            .map_err(|_| error(BrowserErrorCode::Unavailable))?;
        guard.armed = false;
        result
    }
}
#[async_trait]
impl BrowserSession for Session {
    async fn execute(
        &self,
        operation: BrowserOperation,
        control: BrowserControl,
    ) -> Result<BrowserObservation, BrowserError> {
        if needs_witness(&operation) {
            return Err(error(BrowserErrorCode::Denied));
        }
        Ok(execute(self.0.clone(), operation, None, control)
            .await?
            .observation)
    }
    async fn close(&self) -> BrowserClose {
        self.0.wait_closed().await
    }
}
#[async_trait]
impl BrowserTransportHandle for Handle {
    fn snapshot(&self) -> BrowserTransportSnapshot {
        self.0.snapshot()
    }
    #[cfg(test)]
    fn private_witness(&self) -> Option<BrowserPrivateWitness> {
        lock(&self.0.state).private_witness.clone()
    }
    async fn execute_admitted(
        &self,
        operation: BrowserOperation,
        witness: Option<BrowserDocumentWitness>,
        control: BrowserControl,
    ) -> Result<BrowserTransportReply, BrowserError> {
        execute(self.0.clone(), operation, witness, control).await
    }
    async fn close(&self) -> BrowserClose {
        self.0.wait_closed().await
    }
    async fn read_output(
        &self,
        artifact: &BrowserArtifact,
    ) -> Result<VerifiedOutput, BrowserError> {
        let inner = self.0.clone();
        let artifact = artifact.clone();
        tokio::task::spawn_blocking(move || {
            let state = lock(&inner.state);
            if state.view.close.is_some() || !state.artifacts.contains(&artifact) {
                return Err(error(BrowserErrorCode::Denied));
            }
            state
                .files
                .as_ref()
                .ok_or_else(|| error(BrowserErrorCode::Unavailable))?
                .read_output(&inner.task.scope, &artifact)
        })
        .await
        .map_err(|_| error(BrowserErrorCode::Unavailable))?
    }
}
async fn execute(
    inner: Arc<Inner>,
    operation: BrowserOperation,
    witness: Option<BrowserDocumentWitness>,
    control: BrowserControl,
) -> Result<BrowserTransportReply, BrowserError> {
    validate_operation(&inner.task.scope, &operation)?;
    check(&inner, &control)?;
    let sender = lock(&inner.state)
        .sender
        .clone()
        .ok_or_else(|| error(BrowserErrorCode::Unavailable))?;
    let (response, receiver) = oneshot::channel();
    sender
        .try_send(ActorCommand {
            operation,
            witness,
            control,
            response,
        })
        .map_err(|_| error(BrowserErrorCode::Unavailable))?;
    let mut guard = CancelOnDrop {
        stop: inner.stop.clone(),
        armed: true,
    };
    let result = receiver
        .await
        .map_err(|_| unknown(error(BrowserErrorCode::Unavailable)))?;
    guard.armed = false;
    result
}
fn check(inner: &Inner, control: &BrowserControl) -> Result<(), BrowserError> {
    if inner.stop.is_cancelled() || control.cancel.is_cancelled() {
        return Err(lock(&inner.state)
            .failure
            .unwrap_or_else(|| error(BrowserErrorCode::Cancelled)));
    }
    if now() >= inner.task.expires_at_ms || std::time::Instant::now() >= control.deadline {
        return Err(error(BrowserErrorCode::Deadline));
    }
    Ok(())
}
fn needs_witness(operation: &BrowserOperation) -> bool {
    !matches!(
        operation,
        BrowserOperation::Navigate { .. } | BrowserOperation::Observe { .. }
    )
}
struct ActorCommand {
    operation: BrowserOperation,
    witness: Option<BrowserDocumentWitness>,
    control: BrowserControl,
    response: oneshot::Sender<Result<BrowserTransportReply, BrowserError>>,
}

const HEADER: usize = 8;
const FRAME: usize = 64 * 1024;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    protocol: u8,
    id: u64,
    op: String,
    payload: Value,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SuccessPayload {
    ok: bool,
    result: Value,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FailurePayload {
    ok: bool,
    error: BrowserError,
    counters: BrowserCounters,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OpenReply {
    context_id: String,
    page_id: String,
    counters: BrowserCounters,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CloseReply {
    engine_closed: bool,
    counters: BrowserCounters,
}

struct Budget {
    used: AtomicUsize,
    maximum: usize,
    inner: Weak<Inner>,
}
impl Budget {
    fn consume(&self, bytes: usize) -> Result<(), BrowserError> {
        let used = self
            .used
            .fetch_add(bytes, Ordering::SeqCst)
            .saturating_add(bytes);
        if let Some(inner) = self.inner.upgrade() {
            lock(&inner.state).view.counters.wire_bytes_used = used.min(u32::MAX as usize) as u32;
        }
        if used > self.maximum {
            return Err(error(BrowserErrorCode::LimitsExceeded));
        }
        Ok(())
    }
}
fn encoded(id: u64, op: &str, payload: Value, maximum: u32) -> Result<Vec<u8>, BrowserError> {
    if id == 0 || id > MAX_BROWSER_SAFE_INTEGER || !matches!(op, "open" | "execute" | "close") {
        return Err(error(BrowserErrorCode::Protocol));
    }
    let body = serde_json::to_vec(&json!({"protocol":1,"id":id,"op":op,"payload":payload}))
        .map_err(|_| error(BrowserErrorCode::Invalid))?;
    if body.is_empty() || body.len() + HEADER > maximum as usize || body.len() + HEADER > FRAME {
        return Err(error(BrowserErrorCode::LimitsExceeded));
    }
    let mut bytes = Vec::with_capacity(body.len() + HEADER);
    bytes.extend_from_slice(b"ABW1");
    bytes.extend_from_slice(&(body.len() as u32).to_be_bytes());
    bytes.extend_from_slice(&body);
    Ok(bytes)
}
fn envelope(bytes: &[u8]) -> Result<Envelope, BrowserError> {
    let message: Envelope = strict_decode(bytes)?;
    if message.protocol != 1
        || message.id == 0
        || message.id > MAX_BROWSER_SAFE_INTEGER
        || !matches!(message.op.as_str(), "open" | "execute" | "close")
    {
        return Err(error(BrowserErrorCode::Protocol));
    }
    Ok(message)
}
async fn read_part<R: AsyncRead + Unpin>(
    reader: &mut R,
    output: &mut [u8],
    budget: &Budget,
) -> Result<bool, BrowserError> {
    let mut offset = 0;
    while offset < output.len() {
        let read = reader
            .read(&mut output[offset..])
            .await
            .map_err(|_| error(BrowserErrorCode::Unavailable))?;
        if read == 0 {
            if offset == 0 {
                return Ok(false);
            }
            return Err(error(BrowserErrorCode::Protocol));
        }
        budget.consume(read)?;
        offset += read;
    }
    Ok(true)
}
async fn stdout_pump<R: AsyncRead + Unpin>(
    mut reader: R,
    sender: mpsc::Sender<Envelope>,
    budget: Arc<Budget>,
    stop: CancellationToken,
    maximum: u32,
) {
    let result: Result<(), BrowserError> = async {
        loop {
            let mut header = [0; HEADER];
            if !read_part(&mut reader, &mut header, &budget).await? {
                return Err(error(BrowserErrorCode::Unavailable));
            }
            if &header[..4] != b"ABW1" {
                return Err(error(BrowserErrorCode::Protocol));
            }
            let size = u32::from_be_bytes(
                header[4..]
                    .try_into()
                    .map_err(|_| error(BrowserErrorCode::Protocol))?,
            ) as usize;
            if size == 0
                || size > (maximum as usize).saturating_sub(HEADER)
                || size > FRAME - HEADER
            {
                return Err(error(BrowserErrorCode::LimitsExceeded));
            }
            let mut body = vec![0; size];
            if !read_part(&mut reader, &mut body, &budget).await? {
                return Err(error(BrowserErrorCode::Protocol));
            }
            let frame = envelope(&body)?;
            sender
                .send(frame)
                .await
                .map_err(|_| error(BrowserErrorCode::Unavailable))?;
        }
    }
    .await;
    if let Err(failure) = result {
        if let Some(inner) = budget.inner.upgrade() {
            inner.fail(failure);
        }
        stop.cancel();
    }
}
async fn stderr_pump<R: AsyncRead + Unpin>(
    mut reader: R,
    stop: CancellationToken,
    inner: Weak<Inner>,
) {
    let mut buffer = [0; 4096];
    let mut total = 0usize;
    loop {
        match reader.read(&mut buffer).await {
            Ok(0) => break,
            Ok(read) => {
                total += read;
                if total > 64 * 1024 {
                    if let Some(inner) = inner.upgrade() {
                        inner.fail(error(BrowserErrorCode::LimitsExceeded));
                    }
                    stop.cancel();
                    break;
                }
            }
            Err(_) => {
                if let Some(inner) = inner.upgrade() {
                    inner.fail(error(BrowserErrorCode::Unavailable));
                }
                stop.cancel();
                break;
            }
        }
    }
}
fn validate_counters(
    scope: &BrowserScope,
    previous: BrowserCounters,
    current: BrowserCounters,
    measured: usize,
) -> Result<(), BrowserError> {
    if current.actions_used < previous.actions_used
        || current.requests_used < previous.requests_used
        || current.wire_bytes_used < previous.wire_bytes_used
        || current.actions_used > scope.limits.actions
        || current.requests_used > scope.limits.requests
        || current.wire_bytes_used > scope.limits.wire_bytes
        || current.wire_bytes_used as usize > measured
    {
        return Err(error(BrowserErrorCode::Protocol));
    }
    Ok(())
}
fn witness_valid(witness: &BrowserDocumentWitness) -> bool {
    valid_id(&witness.document_id)
        && witness.document_generation > 0
        && witness.document_generation <= MAX_BROWSER_SAFE_INTEGER
        && digest_valid(&witness.observation_digest)
}
fn admitted_witness(
    operation: &BrowserOperation,
    expected: &Option<BrowserDocumentWitness>,
    current: &Option<BrowserDocumentWitness>,
) -> Result<(), BrowserError> {
    if needs_witness(operation) {
        if expected.is_none()
            || expected != current
            || !expected.as_ref().is_some_and(witness_valid)
        {
            return Err(error(BrowserErrorCode::Stale));
        }
    } else if expected.is_some() {
        return Err(error(BrowserErrorCode::Invalid));
    }
    Ok(())
}
fn reply_payload(
    message: Envelope,
    id: u64,
    op: &str,
    scope: &BrowserScope,
    previous: BrowserCounters,
    budget: &Budget,
) -> Result<Value, BrowserError> {
    if message.id != id || message.op != op {
        return Err(error(BrowserErrorCode::Protocol));
    }
    if message.payload.get("ok") == Some(&Value::Bool(false)) {
        let payload: FailurePayload = serde_json::from_value(message.payload)
            .map_err(|_| error(BrowserErrorCode::Protocol))?;
        if payload.ok {
            return Err(error(BrowserErrorCode::Protocol));
        }
        validate_counters(
            scope,
            previous,
            payload.counters,
            budget.used.load(Ordering::SeqCst),
        )?;
        if let Some(inner) = budget.inner.upgrade() {
            record_counters(&inner, payload.counters, budget)?;
        }
        return Err(payload.error);
    }
    let payload: SuccessPayload =
        serde_json::from_value(message.payload).map_err(|_| error(BrowserErrorCode::Protocol))?;
    if !payload.ok {
        return Err(error(BrowserErrorCode::Protocol));
    }
    Ok(payload.result)
}

/// Concrete fixed-schema JCS subset: all numeric fields are JS-safe integers;
/// recursive UTF-16 object-key order also avoids serde preserve_order ambiguity.
pub(super) fn canonical_digest<T: Serialize>(value: &T) -> Result<String, BrowserError> {
    fn render(value: &Value, output: &mut String) -> Result<(), BrowserError> {
        match value {
            Value::Object(map) => {
                let mut keys = map.keys().collect::<Vec<_>>();
                keys.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
                output.push('{');
                for (index, key) in keys.into_iter().enumerate() {
                    if index > 0 {
                        output.push(',');
                    }
                    output.push_str(
                        &serde_json::to_string(key)
                            .map_err(|_| error(BrowserErrorCode::Invalid))?,
                    );
                    output.push(':');
                    render(&map[key], output)?;
                }
                output.push('}');
            }
            Value::Array(values) => {
                output.push('[');
                for (index, value) in values.iter().enumerate() {
                    if index > 0 {
                        output.push(',');
                    }
                    render(value, output)?;
                }
                output.push(']');
            }
            Value::String(value) => output.push_str(
                &serde_json::to_string(value).map_err(|_| error(BrowserErrorCode::Invalid))?,
            ),
            Value::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
            Value::Number(value) => {
                let valid = value
                    .as_u64()
                    .is_some_and(|n| n <= MAX_BROWSER_SAFE_INTEGER)
                    || value.as_i64().is_some_and(|n| {
                        n >= -(MAX_BROWSER_SAFE_INTEGER as i64)
                            && n <= MAX_BROWSER_SAFE_INTEGER as i64
                    });
                if !valid {
                    return Err(error(BrowserErrorCode::Invalid));
                }
                output.push_str(&value.to_string());
            }
            Value::Null => return Err(error(BrowserErrorCode::Invalid)),
        }
        Ok(())
    }
    let value = serde_json::to_value(value).map_err(|_| error(BrowserErrorCode::Invalid))?;
    let mut bytes = String::new();
    render(&value, &mut bytes)?;
    Ok(format!("{:x}", Sha256::digest(bytes.as_bytes())))
}
pub(super) fn strict_decode<T: de::DeserializeOwned>(bytes: &[u8]) -> Result<T, BrowserError> {
    if bytes.len() > FRAME {
        return Err(error(BrowserErrorCode::LimitsExceeded));
    }
    std::str::from_utf8(bytes).map_err(|_| error(BrowserErrorCode::Invalid))?;
    let mut bounds = JsonBounds {
        nodes: 0,
        failure: BrowserErrorCode::Invalid,
    };
    let mut decoder = serde_json::Deserializer::from_slice(bytes);
    let value = StrictJson {
        bounds: &mut bounds,
        depth: 1,
    }
    .deserialize(&mut decoder)
    .map_err(|_| error(bounds.failure))?;
    decoder
        .end()
        .map_err(|_| error(BrowserErrorCode::Invalid))?;
    serde_json::from_value(value).map_err(|_| error(BrowserErrorCode::Invalid))
}
struct JsonBounds {
    nodes: usize,
    failure: BrowserErrorCode,
}
impl JsonBounds {
    fn fail<E: de::Error>(&mut self, code: BrowserErrorCode) -> E {
        self.failure = code;
        E::custom("Invalid browser wire")
    }
}
struct StrictJson<'a> {
    bounds: &'a mut JsonBounds,
    depth: usize,
}
impl<'de> DeserializeSeed<'de> for StrictJson<'_> {
    type Value = Value;
    fn deserialize<D: de::Deserializer<'de>>(self, decoder: D) -> Result<Value, D::Error> {
        self.bounds.nodes += 1;
        if self.depth > 8 || self.bounds.nodes > 2048 {
            return Err(self.bounds.fail(BrowserErrorCode::LimitsExceeded));
        }
        decoder.deserialize_any(self)
    }
}
impl<'de> Visitor<'de> for StrictJson<'_> {
    type Value = Value;
    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("bounded browser JSON")
    }
    fn visit_unit<E: de::Error>(self) -> Result<Value, E> {
        Err(self.bounds.fail(BrowserErrorCode::Invalid))
    }
    fn visit_bool<E: de::Error>(self, v: bool) -> Result<Value, E> {
        Ok(Value::Bool(v))
    }
    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Value, E> {
        if v < -(MAX_BROWSER_SAFE_INTEGER as i64) || v > MAX_BROWSER_SAFE_INTEGER as i64 {
            return Err(self.bounds.fail(BrowserErrorCode::Invalid));
        }
        Ok(Value::Number(v.into()))
    }
    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Value, E> {
        if v > MAX_BROWSER_SAFE_INTEGER {
            return Err(self.bounds.fail(BrowserErrorCode::Invalid));
        }
        Ok(Value::Number(v.into()))
    }
    fn visit_f64<E: de::Error>(self, _v: f64) -> Result<Value, E> {
        Err(self.bounds.fail(BrowserErrorCode::Invalid))
    }
    fn visit_str<E: de::Error>(self, v: &str) -> Result<Value, E> {
        Ok(Value::String(v.into()))
    }
    fn visit_string<E: de::Error>(self, v: String) -> Result<Value, E> {
        Ok(Value::String(v))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let mut values = Vec::new();
        while let Some(value) = seq.next_element_seed(StrictJson {
            bounds: &mut *self.bounds,
            depth: self.depth + 1,
        })? {
            values.push(value);
        }
        Ok(Value::Array(values))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let mut values = serde_json::Map::new();
        while let Some(key) = map.next_key::<String>()? {
            if values.contains_key(&key) {
                return Err(self.bounds.fail(BrowserErrorCode::Invalid));
            }
            values.insert(
                key,
                map.next_value_seed(StrictJson {
                    bounds: &mut *self.bounds,
                    depth: self.depth + 1,
                })?,
            );
        }
        Ok(Value::Object(values))
    }
}

struct ActorDone(CancellationToken);
impl Drop for ActorDone {
    fn drop(&mut self) {
        self.0.cancel();
    }
}
async fn write_frame(
    stdin: &mut tokio::process::ChildStdin,
    budget: &Budget,
    bytes: Vec<u8>,
) -> Result<(), BrowserError> {
    budget.consume(bytes.len())?;
    stdin
        .write_all(&bytes)
        .await
        .map_err(|_| unknown(error(BrowserErrorCode::Unavailable)))?;
    stdin
        .flush()
        .await
        .map_err(|_| unknown(error(BrowserErrorCode::Unavailable)))
}
async fn receive(
    frames: &mut mpsc::Receiver<Envelope>,
    inner: &Inner,
) -> Result<Envelope, BrowserError> {
    tokio::select! { biased;
        _ = inner.stop.cancelled() => Err(unknown(lock(&inner.state).failure.unwrap_or_else(|| error(BrowserErrorCode::Cancelled)))),
        frame = frames.recv() => frame.ok_or_else(|| unknown(error(BrowserErrorCode::Unavailable))),
    }
}
fn record_counters(
    inner: &Inner,
    counters: BrowserCounters,
    budget: &Budget,
) -> Result<(), BrowserError> {
    let mut state = lock(&inner.state);
    validate_counters(
        &inner.task.scope,
        state.reported,
        counters,
        budget.used.load(Ordering::SeqCst),
    )?;
    let measured = budget.used.load(Ordering::SeqCst).min(u32::MAX as usize) as u32;
    state.reported = counters;
    state.view.counters = BrowserCounters {
        actions_used: state.admitted_actions.max(counters.actions_used),
        wire_bytes_used: measured,
        ..counters
    };
    Ok(())
}
fn record_reply(
    inner: &Inner,
    reply: &BrowserTransportReply,
    budget: &Budget,
) -> Result<(), BrowserError> {
    validate_observation(&inner.task.scope, &reply.observation)?;
    validate_counters(
        &inner.task.scope,
        lock(&inner.state).reported,
        reply.counters,
        budget.used.load(Ordering::SeqCst),
    )?;
    if !witness_valid(&reply.document)
        || reply.document.document_generation != reply.observation.document_generation
        || reply.document.observation_digest != canonical_digest(&reply.observation)?
    {
        return Err(error(BrowserErrorCode::Protocol));
    }
    {
        let mut state = lock(&inner.state);
        if state.view.context_id.as_ref() != Some(&reply.context_id)
            || state.view.page_id.as_ref() != Some(&reply.page_id)
            || state.view.document.as_ref().is_some_and(|old| {
                old.document_generation > reply.document.document_generation
                    || (old.document_generation == reply.document.document_generation
                        && old.document_id != reply.document.document_id)
            })
        {
            return Err(error(BrowserErrorCode::Stale));
        }
        let mut artifacts = state.artifacts.clone();
        for artifact in &reply.observation.artifacts {
            if !inner
                .task
                .outputs
                .iter()
                .any(|slot| slot.id == artifact.id && slot.kind == artifact.kind)
            {
                return Err(error(BrowserErrorCode::Protocol));
            }
            if let Some(old) = artifacts.iter().find(|old| old.id == artifact.id) {
                if old != artifact {
                    return Err(error(BrowserErrorCode::Protocol));
                }
            } else {
                if artifacts.len() >= 4 {
                    return Err(error(BrowserErrorCode::LimitsExceeded));
                }
                artifacts.push(artifact.clone());
            }
        }
        state.view.document = Some(reply.document.clone());
        state.artifacts = artifacts;
    }
    record_counters(inner, reply.counters, budget)
}
async fn close_protocol(
    inner: &Inner,
    stdin: &mut tokio::process::ChildStdin,
    frames: &mut mpsc::Receiver<Envelope>,
    budget: &Budget,
    next_id: &mut u64,
    pending: Option<(u64, &'static str)>,
) -> Result<(), BrowserError> {
    let view = inner.snapshot();
    let Some(context) = view.context_id else {
        return Ok(());
    };
    *next_id += 1;
    let close_id = *next_id;
    let bytes = encoded(
        close_id,
        "close",
        json!({
            "scopeId":inner.task.scope.id,"scopeRevision":inner.task.scope.revision,"contextId":context
        }),
        inner.task.scope.limits.frame_bytes,
    )?;
    write_frame(stdin, budget, bytes).await?;
    if let Some((id, op)) = pending {
        let message = frames
            .recv()
            .await
            .ok_or_else(|| error(BrowserErrorCode::Unavailable))?;
        if message.id != id || message.op != op {
            return Err(error(BrowserErrorCode::Protocol));
        }
        // A late ordinary outcome is discarded after authority closed, never admitted.
    }
    let message = frames
        .recv()
        .await
        .ok_or_else(|| error(BrowserErrorCode::Unavailable))?;
    let previous = lock(&inner.state).reported;
    let value = reply_payload(
        message,
        close_id,
        "close",
        &inner.task.scope,
        previous,
        budget,
    )?;
    let reply: CloseReply =
        serde_json::from_value(value).map_err(|_| error(BrowserErrorCode::Protocol))?;
    if !reply.engine_closed {
        return Err(error(BrowserErrorCode::Protocol));
    }
    record_counters(inner, reply.counters, budget)?;
    Ok(())
}
async fn actor(
    inner: Arc<Inner>,
    mut stdin: tokio::process::ChildStdin,
    mut commands: mpsc::Receiver<ActorCommand>,
    mut frames: mpsc::Receiver<Envelope>,
    budget: Arc<Budget>,
    ready: oneshot::Sender<Result<(), BrowserError>>,
) {
    let mut next_id = 1u64;
    let opening = async {
        let bytes = encoded(next_id, "open", json!({
            "scope":inner.task.scope,"policyId":inner.task.policy_id,"policyDigest":inner.task.policy_digest,
            "installationId":inner.task.installation_id,"expiresAtMs":inner.task.expires_at_ms
        }), inner.task.scope.limits.frame_bytes)?;
        write_frame(&mut stdin, &budget, bytes).await?;
        let message = receive(&mut frames, &inner).await?;
        let value = reply_payload(message, next_id, "open", &inner.task.scope, BrowserCounters::default(), &budget)?;
        let reply: OpenReply = serde_json::from_value(value).map_err(|_| error(BrowserErrorCode::Protocol))?;
        if !valid_id(&reply.context_id) || !valid_id(&reply.page_id) { return Err(error(BrowserErrorCode::Protocol)); }
        record_counters(&inner, reply.counters, &budget)?;
        let mut state = lock(&inner.state);
        state.view.context_id = Some(reply.context_id); state.view.page_id = Some(reply.page_id);
        state.operation_deadline = None;
        Ok::<(),BrowserError>(())
    }.await;
    if let Err(failure) = opening {
        let _ = ready.send(Err(unknown(failure)));
        inner.stop.cancel();
        return;
    }
    let _ = ready.send(Ok(()));
    let mut pending = None;
    loop {
        let command = tokio::select! { biased;
            _ = inner.stop.cancelled() => break,
            unexpected = frames.recv() => {
                inner.fail(error(if unexpected.is_some() { BrowserErrorCode::Protocol } else { BrowserErrorCode::Unavailable }));
                break;
            }
            command = commands.recv() => match command { Some(command) => command, None => { inner.stop.cancel(); break; } }
        };
        let admission = (|| {
            check(&inner, &command.control)?;
            let state = lock(&inner.state);
            admitted_witness(&command.operation, &command.witness, &state.view.document)?;
            if state.view.counters.actions_used >= inner.task.scope.limits.actions {
                return Err(error(BrowserErrorCode::LimitsExceeded));
            }
            Ok(())
        })();
        if let Err(failure) = admission {
            let _ = command.response.send(Err(failure));
            continue;
        }
        next_id += 1;
        let id = next_id;
        let (context, previous) = {
            let state = lock(&inner.state);
            (state.view.context_id.clone(), state.reported)
        };
        let mut dispatched = false;
        let operation_deadline = std::time::Instant::now()
            .checked_add(Duration::from_secs(10))
            .unwrap_or(command.control.deadline)
            .min(command.control.deadline);
        {
            let mut state = lock(&inner.state);
            state.operation_deadline = Some(operation_deadline);
            state.operation_cancel = Some(command.control.cancel.clone());
        }
        let result = async {
            let mut payload = json!({"scopeId":inner.task.scope.id,"scopeRevision":inner.task.scope.revision,
                "contextId":context.ok_or_else(|| error(BrowserErrorCode::Unavailable))?,"operation":command.operation});
            if let Some(witness) = &command.witness { payload["documentWitness"] = serde_json::to_value(witness).map_err(|_| error(BrowserErrorCode::Invalid))?; }
            let bytes = encoded(id, "execute", payload, inner.task.scope.limits.frame_bytes)?;
            // Recheck the captured witness and cancellation in this same actor immediately
            // before the sole write; no newer cached witness is substituted.
            check(&inner, &command.control)?;
            admitted_witness(&command.operation, &command.witness, &lock(&inner.state).view.document)?;
            {
                let mut state = lock(&inner.state);
                state.admitted_actions += 1;
                state.view.counters.actions_used = state.admitted_actions;
            }
            pending = Some((id,"execute"));
            dispatched = true;
            write_frame(&mut stdin, &budget, bytes).await?;
            let message = receive(&mut frames, &inner).await?;
            pending = None;
            let value = reply_payload(message, id, "execute", &inner.task.scope, previous, &budget)?;
            let reply: BrowserTransportReply = serde_json::from_value(value).map_err(|_| error(BrowserErrorCode::Protocol))?;
            if reply.counters.actions_used <= previous.actions_used { return Err(error(BrowserErrorCode::Protocol)); }
            record_reply(&inner, &reply, &budget)?;
            check(&inner, &command.control).map_err(unknown)?;
            Ok(reply)
        }.await;
        {
            let mut state = lock(&inner.state);
            state.operation_deadline = None;
            state.operation_cancel = None;
        }
        let retire = result.is_err();
        let result = result.map_err(|failure| {
            if dispatched {
                unknown(lock(&inner.state).failure.unwrap_or(failure))
            } else {
                failure
            }
        });
        let _ = command.response.send(result);
        if retire {
            inner.stop.cancel();
            break;
        }
    }
    let _ = close_protocol(
        &inner,
        &mut stdin,
        &mut frames,
        &budget,
        &mut next_id,
        pending,
    )
    .await;
    let _ = stdin.shutdown().await;
}

struct Reservation {
    _permit: OwnedSemaphorePermit,
    files: Arc<PrivateFiles>,
}
struct Quarantine {
    _reservation: Reservation,
    _child: Option<Box<dyn ChildWrapper>>,
    _tasks: Vec<JoinHandle<()>>,
}
struct Lease {
    runtime: Arc<BrowserPhysicalRuntime>,
    reservation: Option<Reservation>,
    inner: Weak<Inner>,
}
impl Lease {
    fn quarantine(mut self, child: Option<Box<dyn ChildWrapper>>, tasks: Vec<JoinHandle<()>>) {
        if let Some(reservation) = self.reservation.take() {
            lock(&self.runtime.quarantine).push(Quarantine {
                _reservation: reservation,
                _child: child,
                _tasks: tasks,
            });
        }
        if let Some(inner) = self.inner.upgrade() {
            inner.finish(BrowserClose::Quarantined, true);
        }
    }
    fn release(mut self) -> Result<(), BrowserError> {
        if let Some(reservation) = self.reservation.as_ref() {
            // Exact directory is the owned TempDir, never a worker/request path.
            std::fs::remove_dir_all(reservation.files.root())
                .map_err(|_| error(BrowserErrorCode::Unavailable))?;
        }
        self.reservation.take();
        Ok(())
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        // An owner-task panic/drop without a proof never releases a replacement slot.
        if let Some(reservation) = self.reservation.take() {
            lock(&self.runtime.quarantine).push(Quarantine {
                _reservation: reservation,
                _child: None,
                _tasks: Vec::new(),
            });
            if let Some(inner) = self.inner.upgrade() {
                inner.finish(BrowserClose::Quarantined, true);
            }
        }
    }
}
async fn start(inner: Arc<Inner>, control: BrowserControl) -> Result<(), BrowserError> {
    #[cfg(not(windows))]
    {
        let _ = (inner, control);
        return Err(error(BrowserErrorCode::Unavailable));
    }
    #[cfg(windows)]
    {
        start_windows(inner, control).await
    }
}
#[cfg(windows)]
fn containment(command: &mut CommandWrap) {
    use process_wrap::tokio::{CreationFlags, JobObject};
    command.wrap(KillOnDrop);
    let mut flags = CreationFlags(Default::default());
    // Documented CREATE_NO_WINDOW using the maintained public typed shim,
    // identical to M44; no additional Windows dependency, FFI or unsafe.
    flags.0 .0 = 0x08000000;
    command.wrap(flags).wrap(JobObject);
}
#[cfg(windows)]
fn contained(child: &dyn ChildWrapper) -> bool {
    child.type_id() == std::any::TypeId::of::<process_wrap::tokio::JobObjectChild>()
        && child.inner().type_id() == std::any::TypeId::of::<tokio::process::Child>()
}
#[cfg(windows)]
async fn start_windows(inner: Arc<Inner>, control: BrowserControl) -> Result<(), BrowserError> {
    check(&inner, &control)?;
    let remaining = inner
        .task
        .expires_at_ms
        .checked_sub(now())
        .filter(|value| *value <= u64::from(inner.task.scope.limits.scope_ms))
        .ok_or_else(|| error(BrowserErrorCode::Deadline))?;
    let scope_deadline = std::time::Instant::now()
        .checked_add(Duration::from_millis(remaining))
        .ok_or_else(|| error(BrowserErrorCode::Deadline))?;
    let opening_deadline = std::time::Instant::now()
        .checked_add(Duration::from_secs(10))
        .unwrap_or(control.deadline)
        .min(control.deadline);
    inner.installation.revalidate()?;
    if std::time::Instant::now() >= opening_deadline || std::time::Instant::now() >= scope_deadline
    {
        return Err(error(BrowserErrorCode::Deadline));
    }
    let permit = inner
        .runtime
        .permit
        .clone()
        .try_acquire_owned()
        .map_err(|_| error(BrowserErrorCode::Unavailable))?;
    let files = Arc::new(PrivateFiles::create(
        &inner.task.scope,
        &inner.task.inputs,
        &inner.task.outputs,
    )?);
    let lease = Lease {
        runtime: inner.runtime.clone(),
        reservation: Some(Reservation {
            _permit: permit,
            files: files.clone(),
        }),
        inner: Arc::downgrade(&inner),
    };
    {
        let mut state = lock(&inner.state);
        state.files = Some(files.clone());
        state.view.physical_lease_held = true;
        state.operation_deadline = Some(opening_deadline);
    }
    let bootstrap = json!({
        "protocol":1,"policyId":inner.task.policy_id,"policyDigest":inner.task.policy_digest,
        "scope":inner.task.scope,"expiresAtMs":inner.task.expires_at_ms,
        "installation":{"id":inner.installation.id,"browserExecutable":inner.installation.browser_executable,"browserSha256":inner.installation.browser_sha256},
        "privateRoot":super::files::path_text(files.root())?,"inputs":files.inputs,"outputs":files.outputs
    });
    let bytes = serde_json::to_vec(&bootstrap).map_err(|_| error(BrowserErrorCode::Invalid))?;
    let bootstrap_path = match files.write_bootstrap(&bytes) {
        Ok(path) => path,
        Err(failure) => {
            lock(&inner.state).files = None;
            drop(files);
            let _ = lease.release();
            return Err(failure);
        }
    };
    let mut native = Command::new(&inner.installation.node_executable);
    native
        .arg(&inner.installation.worker_script)
        .arg("--bootstrap")
        .arg(bootstrap_path)
        .env_clear()
        .current_dir(files.root())
        .env("TEMP", files.root())
        .env("TMP", files.root())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for name in ["SystemRoot", "WINDIR"] {
        if let Some(value) = std::env::var_os(name) {
            native.env(name, value);
        }
    }
    let mut command = CommandWrap::from(native);
    containment(&mut command);
    let launch_check = check(&inner, &control).and_then(|()| {
        if std::time::Instant::now() >= opening_deadline
            || std::time::Instant::now() >= scope_deadline
        {
            Err(error(BrowserErrorCode::Deadline))
        } else {
            Ok(())
        }
    });
    if let Err(failure) = launch_check {
        lock(&inner.state).files = None;
        drop(files);
        let _ = lease.release();
        return Err(failure);
    }
    #[cfg(test)]
    let spawn_started_at_ms = now();
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => {
            drop(files);
            lease.quarantine(None, Vec::new());
            return Err(error(BrowserErrorCode::Unavailable));
        }
    };
    #[cfg(test)]
    {
        let spawn_returned_at_ms = now();
        if let Some(node_pid) = child.id() {
            lock(&inner.state).private_witness = Some(BrowserPrivateWitness {
                node_pid,
                spawn_started_at_ms,
                spawn_returned_at_ms,
                private_root: files.root().to_owned(),
                node_executable: inner.installation.node_executable.clone(),
                browser_executable: inner.installation.browser_executable.clone(),
            });
        }
    }
    let stdin = child.stdin().take();
    let stdout = child.stdout().take();
    let stderr = child.stderr().take();
    if !contained(child.as_ref()) || stdin.is_none() || stdout.is_none() || stderr.is_none() {
        drop((stdin, stdout, stderr, files));
        let _ = child.start_kill();
        // Uncancelled single wait even for an unexpected composition; its result
        // cannot establish the missing containment proof, so retain quarantine.
        let _ = child.wait().await;
        lease.quarantine(Some(child), Vec::new());
        return Err(error(BrowserErrorCode::Unavailable));
    }
    let (stdin, stdout, stderr) = match (stdin, stdout, stderr) {
        (Some(stdin), Some(stdout), Some(stderr)) => (stdin, stdout, stderr),
        _ => unreachable!("checked owned pipes"),
    };
    let budget = Arc::new(Budget {
        used: AtomicUsize::new(0),
        maximum: inner.task.scope.limits.wire_bytes as usize,
        inner: Arc::downgrade(&inner),
    });
    let (frame_tx, frame_rx) = mpsc::channel(2);
    let (command_tx, command_rx) = mpsc::channel(2);
    lock(&inner.state).sender = Some(command_tx);
    let output = tokio::spawn(stdout_pump(
        stdout,
        frame_tx,
        budget.clone(),
        inner.stop.clone(),
        inner.task.scope.limits.frame_bytes,
    ));
    let errors = tokio::spawn(stderr_pump(
        stderr,
        inner.stop.clone(),
        Arc::downgrade(&inner),
    ));
    let done = CancellationToken::new();
    let actor_done = done.clone();
    let (ready_tx, ready_rx) = oneshot::channel();
    let active = inner.clone();
    let worker = tokio::spawn(async move {
        let _done = ActorDone(actor_done);
        actor(active, stdin, command_rx, frame_rx, budget, ready_tx).await;
    });
    drop(files);
    let process = inner.clone();
    tokio::spawn(async move {
        supervise(
            process,
            child,
            lease,
            vec![worker, output, errors],
            done,
            control,
            scope_deadline,
        )
        .await;
    });
    ready_rx
        .await
        .map_err(|_| unknown(error(BrowserErrorCode::Unavailable)))?
}
#[cfg(windows)]
async fn supervise(
    inner: Arc<Inner>,
    mut child: Box<dyn ChildWrapper>,
    lease: Lease,
    mut tasks: Vec<JoinHandle<()>>,
    actor_done: CancellationToken,
    control: BrowserControl,
    deadline: std::time::Instant,
) {
    loop {
        tokio::select! { biased;
            _ = inner.stop.cancelled() => break,
            _ = control.cancel.cancelled() => { inner.stop.cancel(); break; }
            _ = tokio::time::sleep(Duration::from_millis(20)) => {
                let (active_cancelled, operation_expired) = {
                    let state = lock(&inner.state);
                    (state.operation_cancel.as_ref().is_some_and(|cancel| cancel.is_cancelled()),
                        state.operation_deadline.is_some_and(|value| std::time::Instant::now() >= value))
                };
                let expired = std::time::Instant::now() >= deadline || operation_expired;
                // This verified direct native child is ONLY a leader-exit trigger.
                // Never poll outer JobObject::try_wait or infer physical disposal.
                let leader = child.inner_mut().try_wait();
                if active_cancelled || expired || !matches!(leader,Ok(None)) {
                    // The same physical owner observes distinct Execute controls;
                    // cancelling B never selects away the owned write or Job wait.
                    inner.fail(error(if active_cancelled { BrowserErrorCode::Cancelled }
                        else if expired { BrowserErrorCode::Deadline } else { BrowserErrorCode::Unavailable })); break;
                }
            }
        }
    }
    // Give owned engine Close a bounded opportunity; never cancel a writer task.
    tokio::select! { _ = actor_done.cancelled() => {}, _ = tokio::time::sleep(Duration::from_secs(2)) => {} }
    let killed = child.start_kill();
    // Exactly one owned, uncancelled maintained Job Object wait. It is never
    // timeout-selected, dropped/reinvoked, or unwrapped into the native child.
    let waited = child.wait().await;
    if killed.is_err() || waited.is_err() {
        lease.quarantine(Some(child), tasks);
        return;
    }
    while let Some(mut task) = tasks.pop() {
        match tokio::time::timeout(Duration::from_secs(2), &mut task).await {
            Ok(Ok(())) => {}
            _ => {
                tasks.push(task);
                lease.quarantine(Some(child), tasks);
                return;
            }
        }
    }
    lock(&inner.state).files = None;
    let released = lease.release();
    if released.is_ok() {
        inner.finish(BrowserClose::Closed, false);
    }
}
#[cfg(test)]
#[path = "transport_tests.rs"]
mod tests;
