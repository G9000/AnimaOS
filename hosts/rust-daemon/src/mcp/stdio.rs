//! Owned local-process pipes; rmcp owns JSON-RPC framing and the client session.
//! Resource leases survive dropped startup futures and uncertain retirement.
use std::{
    collections::HashSet,
    io,
    pin::Pin,
    process::Stdio,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex as StdMutex,
    },
    task::{Context, Poll},
    time::Duration,
};

use anima_core::{external_tools, ExternalToolResult};
use futures::future::BoxFuture;
use process_wrap::tokio::{ChildWrapper, CommandWrap, KillOnDrop};
use rmcp::{
    model::{
        ClientConfig, ClientJsonRpcMessage, ClientRequest, ProtocolVersion, ServerJsonRpcMessage,
    },
    service::{serve_client_with_lifecycle_and_ct, ClientLifecycleMode, RunningService},
    transport::{async_rw::AsyncRwTransport, Transport},
    RoleClient,
};
use tempfile::TempDir;
use tokio::{
    io::{AsyncRead, AsyncReadExt, ReadBuf},
    process::{ChildStdin, Command},
    sync::{mpsc, Mutex, Notify, OwnedSemaphorePermit, Semaphore},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

use super::transport::{
    bound_sdk_result, check_initialize, discover_sdk, lock, owned, request_sdk, Activity,
    JoinedTransport, McpCall, McpDiscovery, McpTransportError, McpTransportSession, PhysicalClose,
    TransportControl, PROTOCOL, RAW_EVENT, RAW_TOTAL,
};
use super::{McpStdioSpec, MAX_CONNECTIONS};

struct Reservation {
    connection_id: String,
    _permit: OwnedSemaphorePermit,
    directory: TempDir,
}
struct Quarantine {
    _reservation: Reservation,
    _child: Option<Box<dyn ChildWrapper>>,
}
pub(super) struct StdioRuntime {
    children: Arc<Semaphore>,
    leases: StdMutex<HashSet<String>>,
    quarantine: StdMutex<Vec<Quarantine>>,
}
impl Default for StdioRuntime {
    fn default() -> Self {
        Self {
            children: Arc::new(Semaphore::new(MAX_CONNECTIONS)),
            leases: StdMutex::new(HashSet::new()),
            quarantine: StdMutex::new(Vec::new()),
        }
    }
}
impl StdioRuntime {
    fn reserve(self: &Arc<Self>, connection_id: String) -> Result<Lease, McpTransportError> {
        if connection_id.is_empty() {
            return Err(McpTransportError::Unavailable);
        }
        let permit = self
            .children
            .clone()
            .try_acquire_owned()
            .map_err(|_| McpTransportError::Unavailable)?;
        let directory = tempfile::Builder::new()
            .prefix("anima-mcp-")
            .tempdir()
            .map_err(|_| McpTransportError::Unavailable)?;
        if !lock(&self.leases).insert(connection_id.clone()) {
            return Err(McpTransportError::Unavailable);
        }
        Ok(Lease {
            runtime: self.clone(),
            reservation: Some(Reservation {
                connection_id,
                _permit: permit,
                directory,
            }),
        })
    }
    #[cfg(test)]
    pub(super) fn child_count(&self) -> usize {
        MAX_CONNECTIONS - self.children.available_permits()
    }
}
struct Lease {
    runtime: Arc<StdioRuntime>,
    reservation: Option<Reservation>,
}
impl Lease {
    fn quarantine(mut self, child: Option<Box<dyn ChildWrapper>>) {
        let reservation = self.reservation.take().expect("owned reservation");
        lock(&self.runtime.quarantine).push(Quarantine {
            _reservation: reservation,
            _child: child,
        });
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        if let Some(reservation) = self.reservation.take() {
            lock(&self.runtime.leases).remove(&reservation.connection_id);
            // Both the private cwd and child permit are released only here.
            drop(reservation);
        }
    }
}

struct InputState {
    control: Option<TransportControl>,
    idle_bytes: usize,
    error: Option<McpTransportError>,
}
#[derive(Clone)]
struct Pipes {
    state: Arc<StdMutex<InputState>>,
    stop: CancellationToken,
    workers: Arc<Activity>,
}
impl Pipes {
    fn new(control: TransportControl) -> Self {
        Self {
            state: Arc::new(StdMutex::new(InputState {
                control: Some(control),
                idle_bytes: 0,
                error: None,
            })),
            stop: CancellationToken::new(),
            workers: Arc::new(Activity::default()),
        }
    }
    fn error(&self, fallback: McpTransportError) -> McpTransportError {
        lock(&self.state).error.unwrap_or(fallback)
    }
    fn fail(&self, error: McpTransportError) {
        lock(&self.state).error.get_or_insert(error);
        self.stop.cancel();
    }
    fn begin(&self, control: TransportControl) -> Result<Operation, McpTransportError> {
        if self.stop.is_cancelled() {
            return Err(self.error(McpTransportError::Unavailable));
        }
        let mut state = lock(&self.state);
        state.control = Some(control);
        // Only an explicit owned operation resets the unsolicited-byte allowance.
        state.idle_bytes = 0;
        Ok(Operation(self.clone()))
    }
    fn consume(&self, bytes: usize) -> Result<(), McpTransportError> {
        let mut state = lock(&self.state);
        if let Some(control) = &state.control {
            control.wire_budget.consume(bytes)
        } else {
            state.idle_bytes = state
                .idle_bytes
                .checked_add(bytes)
                .filter(|size| *size <= RAW_TOTAL)
                .ok_or(McpTransportError::WireLimit)?;
            Ok(())
        }
    }
}
struct Operation(Pipes);
impl Drop for Operation {
    fn drop(&mut self) {
        lock(&self.0.state).control = None;
    }
}

// A finite channel contains complete, bounded, prevalidated frames. poll_read
// retains each partially consumed frame when SDK receive is cancelled.
struct FrameRead {
    receiver: mpsc::Receiver<Vec<u8>>,
    frame: Vec<u8>,
    offset: usize,
}
impl AsyncRead for FrameRead {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        output: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if output.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        while this.offset == this.frame.len() {
            match this.receiver.poll_recv(cx) {
                Poll::Ready(Some(frame)) => {
                    this.frame = frame;
                    this.offset = 0;
                }
                Poll::Ready(None) => return Poll::Ready(Ok(())),
                Poll::Pending => return Poll::Pending,
            }
        }
        let length = output.remaining().min(this.frame.len() - this.offset);
        output.put_slice(&this.frame[this.offset..this.offset + length]);
        this.offset += length;
        Poll::Ready(Ok(()))
    }
}

async fn stdout_pump(
    mut stdout: impl AsyncRead + Unpin,
    sender: mpsc::Sender<Vec<u8>>,
    pipes: Pipes,
) -> Result<(), McpTransportError> {
    let mut chunk = [0u8; 8192];
    let mut frame = Vec::with_capacity(8192);
    loop {
        let length = tokio::select! { biased;
            _ = pipes.stop.cancelled() => return Ok(()),
            result = stdout.read(&mut chunk) => result.map_err(|_| McpTransportError::Unavailable)?,
        };
        if length == 0 {
            if !frame.is_empty() {
                return Err(McpTransportError::Malformed);
            }
            // A process can close stdout and remain alive. EOF must retire the
            // session and process lease even when the native leader has not exited.
            pipes.fail(McpTransportError::Unavailable);
            return Ok(());
        }
        pipes.consume(length)?;
        for &byte in &chunk[..length] {
            if frame.len() == RAW_EVENT {
                return Err(McpTransportError::WireLimit);
            }
            frame.push(byte);
            if byte != b'\n' {
                continue;
            }
            let line = frame.strip_suffix(b"\n").unwrap_or(&frame);
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            let line = line.strip_prefix(b"\xef\xbb\xbf").unwrap_or(line);
            if !line.iter().all(u8::is_ascii_whitespace) {
                // SDK malformed-frame diagnostics contain raw input. Reject it
                // here, without formatting serde errors or untrusted bytes.
                let message: ServerJsonRpcMessage =
                    serde_json::from_slice(line).map_err(|_| McpTransportError::Malformed)?;
                check_initialize(&message)?;
                tokio::select! { biased;
                    _ = pipes.stop.cancelled() => return Ok(()),
                    result = sender.send(std::mem::take(&mut frame)) =>
                        result.map_err(|_| McpTransportError::Unavailable)?,
                }
            } else {
                frame.clear();
            }
        }
    }
}
async fn stderr_pump(
    mut stderr: impl AsyncRead + Unpin,
    stop: CancellationToken,
) -> Result<(), McpTransportError> {
    let mut discard = [0u8; 8192];
    loop {
        let length = tokio::select! { biased;
            _ = stop.cancelled() => return Ok(()),
            result = stderr.read(&mut discard) => result.map_err(|_| McpTransportError::Unavailable)?,
        };
        if length == 0 {
            return Ok(());
        }
    }
}

type SdkPipes = AsyncRwTransport<RoleClient, FrameRead, ChildStdin>;
struct ControlledTransport {
    sdk: SdkPipes,
    pipes: Pipes,
}
impl Transport<RoleClient> for ControlledTransport {
    type Error = io::Error;
    fn send(
        &mut self,
        message: ClientJsonRpcMessage,
    ) -> impl std::future::Future<Output = io::Result<()>> + Send + 'static {
        let is_call = matches!(&message, ClientJsonRpcMessage::Request(request)
            if matches!(request.request, ClientRequest::CallToolRequest(_)));
        let send = self.sdk.send(message);
        let pipes = self.pipes.clone();
        async move {
            let control = lock(&pipes.state).control.clone();
            if pipes.stop.is_cancelled() {
                return Err(io::Error::other("External connection is unavailable"));
            }
            if let Some(control) = control {
                if let Err(error) = control.check() {
                    pipes.fail(error);
                    return Err(io::Error::other("External operation is unavailable"));
                }
                if is_call {
                    control.dispatched.store(true, Ordering::SeqCst);
                }
            } else if is_call {
                pipes.fail(McpTransportError::Unavailable);
                return Err(io::Error::other("External operation is unavailable"));
            }
            // An attempted write may have caused an effect. Its entire future
            // is joined by JoinedTransport before any physical-close witness.
            send.await.map_err(|_| {
                pipes.fail(McpTransportError::Unavailable);
                io::Error::other("External connection is unavailable")
            })
        }
    }
    async fn receive(&mut self) -> Option<ServerJsonRpcMessage> {
        self.sdk.receive().await
    }
    async fn close(&mut self) -> io::Result<()> {
        self.sdk.close().await
    }
}

struct ChildOutcome {
    child: Box<dyn ChildWrapper>,
    result: Result<(), McpTransportError>,
}
async fn own_child(
    mut child: Box<dyn ChildWrapper>,
    stdout: JoinHandle<Result<(), McpTransportError>>,
    stderr: JoinHandle<Result<(), McpTransportError>>,
    pipes: Pipes,
) -> ChildOutcome {
    loop {
        tokio::select! { biased;
            _ = pipes.stop.cancelled() => break,
            _ = tokio::time::sleep(Duration::from_millis(20)) => {
                // Never call the group/job try_wait: JobObject wait caches the
                // leader status before it finishes waiting for all descendants.
                // contained() verified this direct inner is the native Tokio
                // child. The safe trait method forwards to Child::try_wait;
                // the outer Job Object cache and ownership remain untouched.
                let leader = child.inner_mut().try_wait();
                if !matches!(leader, Ok(None)) {
                    pipes.fail(McpTransportError::Unavailable);
                    break;
                }
            }
        }
    }
    let killed = child
        .start_kill()
        .map_err(|_| McpTransportError::Unavailable);
    // Exactly one owned, uncancelled wrapper wait. Do not timeout/drop/reinvoke
    // it or unwrap the native child and discard Job Object/group ownership.
    let waited = child
        .wait()
        .await
        .map(|_| ())
        .map_err(|_| McpTransportError::Unavailable);
    // The read result is an operation failure, already captured by Pipes.
    // Successful task joins prove pipe retirement even after malformed input.
    let out = stdout
        .await
        .map(|_| ())
        .map_err(|_| McpTransportError::Unavailable);
    let err = stderr
        .await
        .map(|_| ())
        .map_err(|_| McpTransportError::Unavailable);
    let readers_joined = out.and(err);
    ChildOutcome {
        child,
        result: killed.and(waited).and(readers_joined),
    }
}

struct Inner {
    pipes: Pipes,
    sdk: Mutex<Option<RunningService<RoleClient, ClientConfig>>>,
    operation: Mutex<()>,
    shutdown: CancellationToken,
    closing: AtomicBool,
    closed: Notify,
    close_result: StdMutex<Option<Result<(), McpTransportError>>>,
}
impl Inner {
    fn begin_close(&self) {
        self.closing.store(true, Ordering::SeqCst);
        self.pipes.stop.cancel();
        self.shutdown.cancel();
    }
    async fn wait_closed(&self) -> Result<PhysicalClose, McpTransportError> {
        self.begin_close();
        loop {
            let notice = self.closed.notified();
            tokio::pin!(notice);
            notice.as_mut().enable();
            if let Some(result) = *lock(&self.close_result) {
                return result.map(|_| PhysicalClose::joined());
            }
            notice.await;
        }
    }
}
struct StdioSession(Arc<Inner>);
impl Drop for StdioSession {
    fn drop(&mut self) {
        self.0.begin_close();
    }
}
impl McpTransportSession for StdioSession {
    fn discover(
        &self,
        control: TransportControl,
    ) -> BoxFuture<'static, Result<McpDiscovery, McpTransportError>> {
        let inner = self.0.clone();
        owned(control.clone(), async move {
            let result = tokio::select! { biased;
                _ = inner.pipes.stop.cancelled() => Err(inner.pipes.error(McpTransportError::Unavailable)),
                result = control.run(async {
                    let _serial = inner.operation.lock().await;
                    if inner.closing.load(Ordering::SeqCst) { return Err(McpTransportError::Unavailable); }
                    let _operation = inner.pipes.begin(control.clone())?;
                    let sdk = inner.sdk.lock().await;
                    let sdk = sdk.as_ref().ok_or(McpTransportError::Unavailable)?;
                    discover_sdk(sdk, &control, |fallback| inner.pipes.error(fallback),
                        || async { Ok(()) }).await
                }) => result,
            };
            if result.is_err() {
                inner.begin_close();
            }
            result
        })
    }
    fn call_once(
        &self,
        call: McpCall,
        control: TransportControl,
    ) -> BoxFuture<'static, Result<ExternalToolResult, McpTransportError>> {
        let inner = self.0.clone();
        owned(control.clone(), async move {
            let result = tokio::select! { biased;
                _ = inner.pipes.stop.cancelled() => Err(inner.pipes.error(McpTransportError::Unavailable)),
                result = control.run(async {
                    let _serial = inner.operation.lock().await;
                    if inner.closing.load(Ordering::SeqCst) || call.remote_name != call.descriptor.remote_name {
                        return Err(McpTransportError::Unavailable);
                    }
                    external_tools::validate_external_arguments(&call.descriptor, &call.arguments)
                        .map_err(|_| McpTransportError::SchemaRejected)?;
                    let _operation = inner.pipes.begin(control.clone())?;
                    let sdk = inner.sdk.lock().await;
                    let sdk = sdk.as_ref().ok_or(McpTransportError::Unavailable)?;
                    let result = request_sdk(sdk, call.remote_name, call.arguments, &control,
                        |fallback| inner.pipes.error(fallback)).await?;
                    if let Some(error) = lock(&inner.pipes.state).error { return Err(error); }
                    bound_sdk_result(result, &call.descriptor)
                }) => result,
            };
            if result.is_err() {
                inner.begin_close();
            }
            match result {
                Err(
                    McpTransportError::Cancelled
                    | McpTransportError::TimedOut
                    | McpTransportError::Unavailable,
                ) if control.dispatched.load(Ordering::SeqCst) => {
                    Err(McpTransportError::OutcomeUnknown)
                }
                other => other,
            }
        })
    }
    fn close(&self) -> BoxFuture<'static, Result<PhysicalClose, McpTransportError>> {
        self.0.begin_close();
        let inner = self.0.clone();
        Box::pin(async move { inner.wait_closed().await })
    }
}

fn containment(command: &mut CommandWrap) -> Result<(), McpTransportError> {
    command.wrap(KillOnDrop);
    #[cfg(windows)]
    {
        use process_wrap::tokio::{CreationFlags, JobObject};
        let mut flags = CreationFlags(Default::default());
        // Documented CREATE_NO_WINDOW; keep the maintained wrapper's public
        // typed flags without an extra Windows dependency or handwritten FFI.
        flags.0 .0 = 0x08000000;
        command.wrap(flags).wrap(JobObject);
        Ok(())
    }
    #[cfg(unix)]
    {
        command.wrap(process_wrap::tokio::ProcessGroup::leader());
        Ok(())
    }
    #[cfg(not(any(windows, unix)))]
    {
        Err(McpTransportError::Unavailable)
    }
}
fn contained(child: &dyn ChildWrapper) -> bool {
    #[cfg(windows)]
    {
        child.type_id() == std::any::TypeId::of::<process_wrap::tokio::JobObjectChild>()
            && child.inner().type_id() == std::any::TypeId::of::<tokio::process::Child>()
    }
    #[cfg(unix)]
    {
        child.type_id() == std::any::TypeId::of::<process_wrap::tokio::ProcessGroupChild>()
            && child.inner().type_id() == std::any::TypeId::of::<tokio::process::Child>()
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = child;
        false
    }
}

pub(super) fn connect(
    runtime: Arc<StdioRuntime>,
    spec: McpStdioSpec,
    connection_id: String,
    control: TransportControl,
) -> BoxFuture<'static, Result<Arc<dyn McpTransportSession>, McpTransportError>> {
    #[cfg(not(windows))]
    {
        // Unix ECHILD/group wait does not establish whole-descendant exit.
        // Fail before validation, filesystem lookup, resource reservation or
        // spawn; HTTP remains portable through its independent adapter.
        let _ = (runtime, spec, connection_id);
        Box::pin(async move {
            control.check()?;
            Err(McpTransportError::Unavailable)
        })
    }
    #[cfg(windows)]
    {
        connect_windows(runtime, spec, connection_id, control)
    }
}

#[cfg(windows)]
fn connect_windows(
    runtime: Arc<StdioRuntime>,
    spec: McpStdioSpec,
    connection_id: String,
    control: TransportControl,
) -> BoxFuture<'static, Result<Arc<dyn McpTransportSession>, McpTransportError>> {
    owned(control.clone(), async move {
        control.check()?;
        spec.validate()
            .map_err(|_| McpTransportError::Unavailable)?;
        // Definite pre-launch failures free resources; no existence lookup on
        // save/list/restore. No PATH search, shell, installer, or owner cwd.
        if !std::fs::metadata(&spec.executable).is_ok_and(|metadata| metadata.is_file()) {
            return Err(McpTransportError::Unavailable);
        }
        let lease = runtime.reserve(connection_id)?;
        let mut native = Command::new(&spec.executable);
        native
            .args(&spec.args)
            .env_clear()
            .current_dir(
                lease
                    .reservation
                    .as_ref()
                    .expect("owned cwd")
                    .directory
                    .path(),
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        for name in ["SystemRoot", "WINDIR"] {
            if let Some(value) = std::env::var_os(name) {
                native.env(name, value);
            }
        }
        let mut command = CommandWrap::from(native);
        containment(&mut command)?;
        control.check()?;
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(_) => {
                // process-wrap may fail after launching but before returning
                // the handle. Preserve permit/id/cwd rather than assume exit.
                lease.quarantine(None);
                return Err(McpTransportError::Unavailable);
            }
        };
        let stdin = child.stdin().take();
        let stdout = child.stdout().take();
        let stderr = child.stderr().take();
        if !contained(child.as_ref()) || stdin.is_none() || stdout.is_none() || stderr.is_none() {
            drop((stdin, stdout, stderr));
            let killed = child.start_kill();
            let waited = child.wait().await;
            if killed.is_err() || waited.is_err() {
                lease.quarantine(Some(child));
            }
            return Err(McpTransportError::Unavailable);
        }
        let pipes = Pipes::new(control.clone());
        let (sender, receiver) = mpsc::channel(4);
        let stdout_pipes = pipes.clone();
        let stdout_task = tokio::spawn(async move {
            let result = stdout_pump(
                stdout.expect("checked stdout"),
                sender,
                stdout_pipes.clone(),
            )
            .await;
            if let Err(error) = result {
                stdout_pipes.fail(error);
            }
            result
        });
        let stderr_pipes = pipes.clone();
        let stderr_task = tokio::spawn(async move {
            let result =
                stderr_pump(stderr.expect("checked stderr"), stderr_pipes.stop.clone()).await;
            if let Err(error) = result {
                stderr_pipes.fail(error);
            }
            result
        });
        let process = tokio::spawn(own_child(child, stdout_task, stderr_task, pipes.clone()));
        let inner = Arc::new(Inner {
            pipes: pipes.clone(),
            sdk: Mutex::new(None),
            operation: Mutex::new(()),
            shutdown: CancellationToken::new(),
            closing: AtomicBool::new(false),
            closed: Notify::new(),
            close_result: StdMutex::new(None),
        });
        // This coordinator is owned and never aborted by a caller. Even startup
        // cancellation retains all resources until the same cleanup completes.
        let retirement = inner.clone();
        tokio::spawn(async move {
            retirement.pipes.stop.cancelled().await;
            retirement.begin_close();
            let sdk_result = match retirement.sdk.lock().await.take() {
                Some(mut sdk) => sdk
                    .close()
                    .await
                    .map(|_| ())
                    .map_err(|_| McpTransportError::Unavailable),
                None => Ok(()),
            };
            retirement.pipes.workers.idle().await;
            let result = match process.await {
                Ok(outcome) => {
                    let result = sdk_result.and(outcome.result);
                    if result.is_err() {
                        lease.quarantine(Some(outcome.child));
                    } else {
                        drop(outcome.child);
                        drop(lease);
                    }
                    result
                }
                Err(_) => {
                    lease.quarantine(None);
                    Err(McpTransportError::Unavailable)
                }
            };
            *lock(&retirement.close_result) = Some(result);
            retirement.closed.notify_waiters();
        });
        let session = Arc::new(StdioSession(inner.clone()));
        let startup = async {
            // Hold the slot through startup so retirement cannot race publication.
            // stop is also selected below: shutdown never waits on this mutex to
            // cancel initialization when the leader exits or a raw frame fails.
            let mut slot = inner.sdk.lock().await;
            let read = FrameRead {
                receiver,
                frame: Vec::new(),
                offset: 0,
            };
            let sdk_transport = ControlledTransport {
                sdk: AsyncRwTransport::new_client(read, stdin.expect("checked stdin")),
                pipes: pipes.clone(),
            };
            let transport = JoinedTransport::new(sdk_transport, &pipes.workers);
            let mut info = ClientConfig::default();
            info.protocol_version = ProtocolVersion::V_2025_11_25;
            let sdk = tokio::select! { biased;
                _ = pipes.stop.cancelled() => return Err(pipes.error(McpTransportError::Unavailable)),
                sdk = serve_client_with_lifecycle_and_ct(info, transport,
                    ClientLifecycleMode::Initialize, inner.shutdown.clone()) =>
                    sdk.map_err(|_| pipes.error(McpTransportError::Malformed))?,
            };
            *slot = Some(sdk);
            // Publish before the next await so cancelled post-handshake work
            // still has a retained RunningService that retirement can join.
            let sdk = slot.as_ref().expect("fresh SDK");
            sdk.set_response_cache_config(rmcp::ClientCacheConfig::disabled())
                .await;
            let valid = sdk
                .peer_info()
                .is_some_and(|info| info.protocol_version.as_str() == PROTOCOL);
            if !valid {
                return Err(McpTransportError::UnsupportedProtocol);
            }
            Ok(())
        };
        let result = control.run(startup).await;
        lock(&pipes.state).control = None;
        if let Err(error) = result {
            inner.begin_close();
            inner.wait_closed().await?;
            return Err(error);
        }
        if pipes.stop.is_cancelled() {
            let error = pipes.error(McpTransportError::Unavailable);
            inner.wait_closed().await?;
            return Err(error);
        }
        Ok(session as Arc<dyn McpTransportSession>)
    })
}

#[cfg(test)]
mod pipe_tests {
    use super::*;
    use tokio::time::Instant;

    #[tokio::test]
    async fn m44_complete_stdout_eof_requests_retirement_even_before_leader_exit() {
        // Typed pipe evidence only: this test does not claim native process
        // exit. The Windows descendant test establishes actual job cleanup.
        let control = TransportControl::new(
            Instant::now() + Duration::from_secs(5),
            CancellationToken::new(),
        );
        let pipes = Pipes::new(control);
        let (read, writer) = tokio::io::duplex(64);
        let (sender, _receiver) = mpsc::channel(1);
        drop(writer);
        tokio::time::timeout(
            Duration::from_secs(1),
            stdout_pump(read, sender, pipes.clone()),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(pipes.stop.is_cancelled());
        assert_eq!(
            pipes.error(McpTransportError::Malformed),
            McpTransportError::Unavailable
        );
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn m44_non_windows_stdio_rejects_before_reserving_or_starting() {
        let temporary = tempfile::tempdir().unwrap();
        let executable = temporary.path().join("synthetic-peer.exe");
        std::fs::write(&executable, b"synthetic existing file; never executable").unwrap();
        let runtime = Arc::new(StdioRuntime::default());
        let result = connect(
            runtime.clone(),
            McpStdioSpec {
                executable: executable.to_string_lossy().into_owned(),
                args: Vec::new(),
            },
            "unsupported-host".into(),
            TransportControl::new(
                Instant::now() + Duration::from_secs(5),
                CancellationToken::new(),
            ),
        )
        .await;
        assert!(matches!(result, Err(McpTransportError::Unavailable)));
        assert_eq!(runtime.child_count(), 0);
        assert!(lock(&runtime.leases).is_empty());
        assert!(lock(&runtime.quarantine).is_empty());
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::super::transport::{
        DefaultMcpTransportFactory, McpTransportFactory, ValidatedMcpLaunch,
    };
    use super::*;
    use serde_json::{json, Value};
    use std::path::{Path, PathBuf};
    use tokio::time::Instant;

    fn control() -> TransportControl {
        TransportControl::new(
            Instant::now() + Duration::from_secs(5),
            CancellationToken::new(),
        )
    }
    fn fixture_path() -> PathBuf {
        let path = std::env::var_os("ANIMA_MCP_STDIO_FIXTURE_EXE")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../target/debug/examples")
                    .join(if cfg!(windows) {
                        "mcp_stdio_peer.exe"
                    } else {
                        "mcp_stdio_peer"
                    })
            });
        path.canonicalize()
            .expect("root must build the native mcp_stdio_peer example before stdio tests")
    }
    fn launch(mode: &str, marker: &Path, connection_id: &str) -> ValidatedMcpLaunch {
        ValidatedMcpLaunch::Stdio {
            spec: McpStdioSpec {
                executable: fixture_path().to_string_lossy().into_owned(),
                args: vec![
                    "--mode".into(),
                    mode.into(),
                    "--marker".into(),
                    marker.to_string_lossy().into_owned(),
                ],
            },
            connection_id: connection_id.into(),
        }
    }
    async fn wait_marker(path: &Path, method: Option<&str>) -> Value {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if let Ok(bytes) = std::fs::read(path) {
                    if let Ok(value) = serde_json::from_slice::<Value>(&bytes) {
                        if method.is_none_or(|method| {
                            value["counts"][method].as_u64().is_some_and(|n| n > 0)
                        }) {
                            return value;
                        }
                    }
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("native peer must reach the bounded fixture marker")
    }
    async fn wait_count(factory: &DefaultMcpTransportFactory, expected: usize) {
        tokio::time::timeout(Duration::from_secs(3), async {
            while factory.stdio_child_count() != expected {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("owned child cleanup must finish before its slot is reused");
    }
    async fn wait_heartbeat(path: &Path, after: Option<u64>) -> u64 {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                // The fixture's truncating write briefly exposes empty text.
                if let Ok(text) = std::fs::read_to_string(path) {
                    if let Ok(ticks) = text.parse::<u64>() {
                        if ticks > 0 && after.is_none_or(|before| ticks > before) {
                            return ticks;
                        }
                    }
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("actual descendant heartbeat must reach a valid advancing count")
    }
    async fn joined(session: &Arc<dyn McpTransportSession>) {
        tokio::time::timeout(Duration::from_secs(3), session.close())
            .await
            .expect("cleanup must be bounded for the synthetic peer")
            .unwrap();
    }

    #[tokio::test]
    async fn m44_native_debug_spans_never_record_launch_arguments() {
        use tracing::instrument::WithSubscriber;
        #[derive(Clone, Default)]
        struct Captured(Arc<StdMutex<Vec<u8>>>);
        impl std::io::Write for Captured {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                lock(&self.0).extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let temporary = tempfile::tempdir().unwrap();
        let sentinel = "m44-private-argv-sentinel";
        let marker = temporary.path().join(sentinel);
        let captured = Captured::default();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_max_level(tracing::Level::DEBUG)
            .with_span_events(tracing_subscriber::fmt::format::FmtSpan::NEW)
            .with_writer({
                let captured = captured.clone();
                move || captured.clone()
            })
            .finish();
        let factory = DefaultMcpTransportFactory::default();
        async {
            tracing::debug!("m44 capture active");
            let session = factory
                .connect(launch("normal", &marker, "debug-privacy"), control())
                .await
                .unwrap();
            session.discover(control()).await.unwrap();
            joined(&session).await;
        }
        .with_subscriber(subscriber)
        .await;
        assert_eq!(factory.stdio_child_count(), 0);
        let logged = String::from_utf8(lock(&captured.0).clone()).unwrap();
        assert!(
            logged.contains("m44 capture active"),
            "debug capture must be active"
        );
        assert!(
            !logged.contains(sentinel),
            "launch argv must stay out of debug logs"
        );
    }

    #[tokio::test]
    async fn m44_native_handshake_discovery_calls_and_stderr_discard_use_one_sdk_session() {
        for mode in ["normal", "stderr-flood", "split-frames"] {
            let temporary = tempfile::tempdir().unwrap();
            let marker = temporary.path().join("peer.json");
            let factory = DefaultMcpTransportFactory::default();
            let ctl = control();
            let session = factory
                .connect(launch(mode, &marker, "native-roundtrip"), ctl.clone())
                .await
                .unwrap();
            assert_eq!(factory.stdio_child_count(), 1);
            let discovery = session.discover(ctl).await.unwrap();
            assert_eq!(discovery.protocol_version, PROTOCOL);
            assert_eq!(discovery.tools.len(), 1);
            let again = session.discover(control()).await.unwrap();
            assert_eq!(discovery.tools, again.tools);
            let descriptor = discovery.tools.into_iter().next().unwrap();
            let result = session
                .call_once(
                    McpCall {
                        remote_name: descriptor.remote_name.clone(),
                        arguments: json!({"value":"violet"}),
                        descriptor,
                    },
                    control(),
                )
                .await
                .unwrap();
            assert_eq!(result.text_blocks, ["Synthetic stdio violet record"]);
            assert!(!result.is_error);
            let peer = wait_marker(&marker, Some("tools/call")).await;
            assert_eq!(peer["counts"]["initialize"], 1);
            assert_eq!(peer["counts"]["tools/list"], 2);
            assert_eq!(peer["counts"]["tools/call"], 1);
            assert_eq!(peer["ambientEnvironmentPresent"], false);
            assert_eq!(peer["inheritedCwd"], false);
            joined(&session).await;
            joined(&session).await;
            assert_eq!(factory.stdio_child_count(), 0);
        }
    }

    #[tokio::test]
    async fn m44_native_predecode_malformed_line_no_newline_and_protocol_failures_are_static() {
        for (mode, expected) in [
            ("malformed", McpTransportError::Malformed),
            ("oversized-line", McpTransportError::WireLimit),
            ("no-newline", McpTransportError::WireLimit),
            ("wrong-protocol", McpTransportError::UnsupportedProtocol),
            ("exit-before-init", McpTransportError::Unavailable),
        ] {
            let temporary = tempfile::tempdir().unwrap();
            let marker = temporary.path().join("peer.json");
            let factory = DefaultMcpTransportFactory::default();
            let error = match factory
                .connect(launch(mode, &marker, "native-invalid"), control())
                .await
            {
                Ok(session) => {
                    joined(&session).await;
                    panic!("invalid native peer unexpectedly connected");
                }
                Err(error) => error,
            };
            if mode == "exit-before-init" {
                assert!(matches!(
                    error,
                    McpTransportError::Unavailable | McpTransportError::Malformed
                ));
            } else {
                assert_eq!(error, expected);
            }
            assert!(!error.to_string().contains("synthetic-sensitive"));
            assert!(!error
                .to_string()
                .contains(&marker.to_string_lossy().to_string()));
            assert_eq!(factory.stdio_child_count(), 0);
        }
    }

    #[tokio::test]
    async fn m44_native_idle_and_shared_discovery_wire_budgets_retire_the_child() {
        for mode in ["idle-flood", "aggregate-discovery"] {
            let temporary = tempfile::tempdir().unwrap();
            let marker = temporary.path().join("peer.json");
            let factory = DefaultMcpTransportFactory::default();
            let session = factory
                .connect(launch(mode, &marker, "native-flood"), control())
                .await
                .unwrap();
            let discovery = session.discover(control()).await;
            if mode == "aggregate-discovery" {
                assert_eq!(discovery.unwrap_err(), McpTransportError::WireLimit);
            }
            // Idle unsolicited data has its own fixed allowance even after an
            // operation has returned; it cannot accumulate unbounded frames.
            wait_count(&factory, 0).await;
            assert_eq!(
                session.discover(control()).await.unwrap_err(),
                McpTransportError::WireLimit
            );
            joined(&session).await;
        }
    }

    #[tokio::test]
    async fn m44_native_root_exit_kills_live_descendant_and_joins_held_output_pipes() {
        let temporary = tempfile::tempdir().unwrap();
        let marker = temporary.path().join("peer.json");
        let heartbeat = marker.with_extension("heartbeat");
        let factory = DefaultMcpTransportFactory::default();
        let session = factory
            .connect(
                launch("descendant-held-pipes", &marker, "native-tree"),
                control(),
            )
            .await
            .unwrap();
        let peer = wait_marker(&marker, Some("initialize")).await;
        assert!(peer["childPid"].as_u64().is_some_and(|pid| pid > 0));
        let before = wait_heartbeat(&heartbeat, None).await;
        tokio::time::sleep(Duration::from_millis(80)).await;
        let live = wait_heartbeat(&heartbeat, Some(before)).await;
        assert!(
            live > before,
            "the actual descendant must be alive before the leader exits"
        );
        let _ = session.discover(control()).await;
        wait_marker(&marker, Some("tools/list")).await;
        joined(&session).await;
        assert_eq!(factory.stdio_child_count(), 0);
        let stopped = std::fs::read_to_string(&heartbeat).unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(std::fs::read_to_string(&heartbeat).unwrap(), stopped);
        joined(&session).await;
    }

    #[tokio::test]
    async fn m44_native_cancelled_and_dropped_initialization_keep_lease_until_join() {
        for dropped in [false, true] {
            let temporary = tempfile::tempdir().unwrap();
            let marker = temporary.path().join("peer.json");
            let factory = Arc::new(DefaultMcpTransportFactory::default());
            let ctl = control();
            let connect = factory.connect(
                launch("blocked-init", &marker, "native-startup"),
                ctl.clone(),
            );
            let task = tokio::spawn(connect);
            wait_marker(&marker, None).await;
            assert_eq!(factory.stdio_child_count(), 1);
            assert!(factory
                .connect(launch("normal", &marker, "native-startup"), control())
                .await
                .is_err());
            if dropped {
                task.abort();
                let _ = task.await;
            } else {
                ctl.cancel.cancel();
                assert!(matches!(
                    task.await.unwrap(),
                    Err(McpTransportError::Cancelled)
                ));
            }
            wait_count(&factory, 0).await;
            let replacement = factory
                .connect(launch("normal", &marker, "native-startup"), control())
                .await
                .unwrap();
            joined(&replacement).await;
            assert_eq!(factory.stdio_child_count(), 0);
        }
    }

    #[tokio::test]
    async fn m44_native_cancelled_and_dropped_dispatched_calls_join_and_leave_outcome_unknown() {
        for dropped in [false, true] {
            let temporary = tempfile::tempdir().unwrap();
            let marker = temporary.path().join("peer.json");
            let factory = DefaultMcpTransportFactory::default();
            let session = factory
                .connect(launch("blocked-call", &marker, "native-call"), control())
                .await
                .unwrap();
            let descriptor = session.discover(control()).await.unwrap().tools.remove(0);
            let ctl = control();
            let task = tokio::spawn(session.call_once(
                McpCall {
                    remote_name: descriptor.remote_name.clone(),
                    arguments: json!({"value":"violet"}),
                    descriptor,
                },
                ctl.clone(),
            ));
            wait_marker(&marker, Some("tools/call")).await;
            assert!(ctl.dispatched.load(Ordering::SeqCst));
            if dropped {
                task.abort();
                let _ = task.await;
            } else {
                ctl.cancel.cancel();
                assert_eq!(
                    task.await.unwrap().unwrap_err(),
                    McpTransportError::OutcomeUnknown
                );
            }
            joined(&session).await;
            joined(&session).await;
            assert_eq!(factory.stdio_child_count(), 0);
        }
    }

    #[tokio::test]
    async fn m44_native_shared_four_child_cap_and_session_drop_block_replacements() {
        let temporary = tempfile::tempdir().unwrap();
        let factory = DefaultMcpTransportFactory::default();
        let mut sessions = Vec::new();
        for index in 0..MAX_CONNECTIONS {
            sessions.push(
                factory
                    .connect(
                        launch(
                            "normal",
                            &temporary.path().join(format!("peer-{index}.json")),
                            &format!("native-{index}"),
                        ),
                        control(),
                    )
                    .await
                    .unwrap(),
            );
        }
        assert_eq!(factory.stdio_child_count(), MAX_CONNECTIONS);
        assert!(factory
            .connect(
                launch("normal", &temporary.path().join("extra.json"), "extra"),
                control()
            )
            .await
            .is_err());
        assert!(factory
            .connect(
                launch(
                    "normal",
                    &temporary.path().join("duplicate.json"),
                    "native-0"
                ),
                control()
            )
            .await
            .is_err());
        drop(sessions.pop());
        wait_count(&factory, MAX_CONNECTIONS - 1).await;
        let extra = factory
            .connect(
                launch("normal", &temporary.path().join("extra.json"), "extra"),
                control(),
            )
            .await
            .unwrap();
        joined(&extra).await;
        for session in sessions {
            joined(&session).await;
        }
        assert_eq!(factory.stdio_child_count(), 0);
    }

    #[tokio::test]
    async fn m44_missing_executable_is_prelaunch_but_deleted_quarantines_keep_slots_and_ids() {
        let temporary = tempfile::tempdir().unwrap();
        let factory = DefaultMcpTransportFactory::default();
        let missing = ValidatedMcpLaunch::Stdio {
            spec: McpStdioSpec {
                executable: temporary
                    .path()
                    .join("nonexistent-synthetic.exe")
                    .to_string_lossy()
                    .into_owned(),
                args: Vec::new(),
            },
            connection_id: "missing".into(),
        };
        assert!(factory.connect(missing, control()).await.is_err());
        assert_eq!(factory.stdio_child_count(), 0);
        // Simulate retained uncertainty after rows are deleted. Runtime leases
        // are independent of durable records and remain counted at the factory.
        let runtime = Arc::new(StdioRuntime::default());
        for index in 0..MAX_CONNECTIONS {
            runtime
                .reserve(format!("deleted-{index}"))
                .unwrap()
                .quarantine(None);
        }
        assert_eq!(runtime.child_count(), MAX_CONNECTIONS);
        assert!(runtime.reserve("deleted-0".into()).is_err());
        assert!(runtime.reserve("replacement".into()).is_err());
        assert_eq!(lock(&runtime.quarantine).len(), MAX_CONNECTIONS);
        assert_eq!(lock(&runtime.leases).len(), MAX_CONNECTIONS);
    }

    #[tokio::test]
    async fn m44_bounded_frame_reader_retains_partial_bytes_across_cancelled_reads() {
        let (sender, receiver) = mpsc::channel(1);
        let mut reader = FrameRead {
            receiver,
            frame: Vec::new(),
            offset: 0,
        };
        let frame = b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/tools/list_changed\"}\n";
        sender.send(frame.to_vec()).await.unwrap();
        let mut first = [0u8; 7];
        reader.read_exact(&mut first).await.unwrap();
        let mut rest = vec![0; frame.len()];
        // read_exact receives the remainder, then waits for more; cancellation
        // retains those bytes in the caller buffer and advances the frame once.
        let pending = reader.read_exact(&mut rest);
        assert!(tokio::time::timeout(Duration::from_millis(10), pending)
            .await
            .is_err());
        drop(sender);
        assert_eq!(&first, &frame[..7]);
        assert_eq!(&rest[..frame.len() - 7], &frame[7..]);
        assert_eq!(reader.read(&mut [0u8; 1]).await.unwrap(), 0);
    }
}
