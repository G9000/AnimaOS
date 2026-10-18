//! One-shot owned lifecycle. Real OS launcher acceptance is tested separately.
//! The physical owner must retain the exact process tree, transport, pipes and
//! permit after caller cancellation; interpreter isolation is not an OS jail.
use std::{
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use anima_core::{tool_rpc::ProgramRequest, CancelSignal};
use async_trait::async_trait;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    sync::{oneshot, OwnedSemaphorePermit, Semaphore},
    task::JoinHandle,
    time::Instant,
};
use tokio_util::sync::CancellationToken;

pub(super) const READY: &[u8] = b"ANIMA-TOOL-WORKER-v1\n";
pub(super) const MAX_STDOUT: usize = 50_000;
pub(super) const MAX_STDERR: usize = 10_000;

pub(super) struct WorkerConfig {
    pub(super) interpreter: Option<PathBuf>,
    pub(super) workspace: PathBuf,
}

/// Host-private bootstrap, supplied through stdin after the trusted READY.
/// Neither serde nor Debug may publish transport credentials accidentally.
pub(super) struct WorkerBootstrap {
    pub(super) endpoint: SocketAddr,
    pub(super) token: String,
    pub(super) cell_epoch: String,
}

pub(super) struct LaunchSpec {
    pub(super) interpreter: PathBuf,
    pub(super) args: Vec<String>,
    pub(super) workspace: PathBuf,
    pub(super) environment: Vec<(String, String)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WorkerFailure {
    MissingInterpreter,
    InvalidInput,
    Busy,
    Unavailable,
    Launch,
    Stdin,
    InvalidFrame,
    OutputLimit,
    TimedOut,
    Cancelled,
    CleanupUnknown,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct WorkerResult {
    pub(super) mode: &'static str,
    pub(super) exit_code: i32,
    pub(super) stdout: Vec<u8>,
    pub(super) stderr: Vec<u8>,
    pub(super) cleanup_verified: bool,
}

pub(super) struct WorkerPipes {
    pub(super) stdin: Box<dyn AsyncWrite + Send + Unpin>,
    pub(super) stdout: Box<dyn AsyncRead + Send + Unpin>,
    pub(super) stderr: Box<dyn AsyncRead + Send + Unpin>,
}

/// One owned backend instance, never a registry or a user-selected command.
/// Real Windows custody must use the already-installed process-wrap JobObject.
/// The fake peer tests exercise ordering only, not actual OS descendant proof.
#[async_trait]
pub(super) trait OwnedWorker: Send {
    /// The physical owner already retains this object before starting native
    /// creation, so an assignment/spawn failure cannot lose partial custody.
    async fn start(&mut self) -> Result<(), WorkerFailure>;
    fn take_pipes(&mut self) -> Result<WorkerPipes, WorkerFailure>;
    fn close_admission(&mut self);
    async fn wait_leader(&mut self) -> Result<i32, WorkerFailure>;
    async fn terminate_tree(&mut self) -> Result<(), WorkerFailure>;
    async fn wait_tree(&mut self) -> Result<(), WorkerFailure>;
}

#[async_trait]
pub(super) trait WorkerLauncher: Send + Sync {
    /// Prepare command/Job/transport resources without starting any user code.
    async fn prepare(&self, spec: LaunchSpec) -> Result<Box<dyn OwnedWorker>, WorkerFailure>;
}

pub(super) struct WorkerRuntime {
    state: Arc<PhysicalState>,
}

struct PhysicalState {
    slots: Arc<Semaphore>,
    capacity: usize,
    quarantine: Mutex<Vec<Quarantined>>,
    owners: Mutex<Vec<JoinHandle<()>>>,
}
struct Quarantined {
    _resources: Resources,
    // Intentional bounded lifetime pin: dropping the caller/runtime cannot
    // discard unknown native custody and silently free its physical permit.
    _runtime: Arc<PhysicalState>,
}
type PipeTask = JoinHandle<Result<Vec<u8>, WorkerFailure>>;
struct Resources {
    _permit: OwnedSemaphorePermit,
    _directory: tempfile::TempDir,
    child: Option<Box<dyn OwnedWorker>>,
    stdout: Option<PipeTask>,
    stderr: Option<PipeTask>,
}
struct Lease {
    state: Arc<PhysicalState>,
    resources: Option<Resources>,
}
impl Drop for Lease {
    fn drop(&mut self) {
        if let Some(resources) = self.resources.take() {
            self.state
                .quarantine
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .push(Quarantined {
                    _resources: resources,
                    _runtime: self.state.clone(),
                });
        }
    }
}
struct CallerDrop(CancellationToken);
impl Drop for CallerDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

impl WorkerRuntime {
    pub(super) fn new() -> Self {
        Self::with_capacity(1)
    }

    pub(super) fn shared() -> Self {
        Self::with_capacity(4)
    }

    fn with_capacity(capacity: usize) -> Self {
        Self {
            state: Arc::new(PhysicalState {
                slots: Arc::new(Semaphore::new(capacity)),
                capacity,
                quarantine: Mutex::new(Vec::new()),
                owners: Mutex::new(Vec::new()),
            }),
        }
    }

    pub(super) fn available_slots(&self) -> usize {
        self.state.slots.available_permits()
    }

    /// There is deliberately no PATH lookup or permissive physical fallback.
    /// The injected backend owns actual process-tree custody.
    pub(super) async fn run(
        &self,
        config: WorkerConfig,
        request: ProgramRequest,
        bootstrap: WorkerBootstrap,
        cancel: CancelSignal,
        deadline: Instant,
        launcher: Arc<dyn WorkerLauncher>,
    ) -> Result<WorkerResult, WorkerFailure> {
        self.run_with_descriptors(
            config,
            request,
            bootstrap,
            Vec::new(),
            cancel,
            deadline,
            launcher,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn run_with_descriptors(
        &self,
        config: WorkerConfig,
        request: ProgramRequest,
        bootstrap: WorkerBootstrap,
        descriptors: Vec<serde_json::Value>,
        cancel: CancelSignal,
        deadline: Instant,
        launcher: Arc<dyn WorkerLauncher>,
    ) -> Result<WorkerResult, WorkerFailure> {
        if cancel.is_cancelled() {
            return Err(WorkerFailure::Cancelled);
        }
        if Instant::now() >= deadline {
            return Err(WorkerFailure::TimedOut);
        }
        let interpreter = config
            .interpreter
            .filter(|path| path.is_absolute() && path.is_file())
            .ok_or(WorkerFailure::MissingInterpreter)?;
        if request.code.trim().is_empty()
            || request.code.len() > anima_core::tool_rpc::MAX_CODE_BYTES
            || !config.workspace.is_absolute()
            || !config.workspace.is_dir()
            || !bootstrap.endpoint.ip().is_loopback()
            || !bootstrap.endpoint.is_ipv4()
            || bootstrap.endpoint.port() == 0
            || [&bootstrap.token, &bootstrap.cell_epoch]
                .iter()
                .any(|value| {
                    value.trim().is_empty()
                        || value.len() > 128
                        || value.chars().any(char::is_control)
                })
        {
            return Err(WorkerFailure::InvalidInput);
        }
        let input = serde_json::to_vec(&serde_json::json!({"version":1,"code":request.code,
            "reset":request.reset,"endpoint":{"host":"127.0.0.1","port":bootstrap.endpoint.port()},
            "token":bootstrap.token,"cellEpoch":bootstrap.cell_epoch,"tools":descriptors}))
        .map_err(|_| WorkerFailure::InvalidInput)?;
        if input.len() > 524_288 {
            return Err(WorkerFailure::InvalidInput);
        }
        let permit = self
            .state
            .slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| WorkerFailure::Busy)?;
        let directory = tempfile::Builder::new()
            .prefix("anima-tool-rpc-")
            .tempdir()
            .map_err(|_| WorkerFailure::Unavailable)?;
        let runner = directory.path().join("runner.py");
        std::fs::write(&runner, include_bytes!("python_runner.py"))
            .map_err(|_| WorkerFailure::Unavailable)?;
        let mut environment: Vec<_> = ["SystemRoot", "WINDIR"]
            .into_iter()
            .filter_map(|name| {
                std::env::var(name)
                    .ok()
                    .map(|value| (name.to_owned(), value))
            })
            .collect();
        for name in ["TEMP", "TMP"] {
            environment.push((name.into(), directory.path().to_string_lossy().into_owned()));
        }
        let spec = LaunchSpec {
            interpreter,
            workspace: config.workspace,
            args: vec![
                "-I".into(),
                "-S".into(),
                "-u".into(),
                runner.to_string_lossy().into_owned(),
            ],
            environment,
        };
        let lease = Lease {
            state: self.state.clone(),
            resources: Some(Resources {
                _permit: permit,
                _directory: directory,
                child: None,
                stdout: None,
                stderr: None,
            }),
        };
        let stop = CancellationToken::new();
        let _caller = CallerDrop(stop.clone());
        let (sender, receiver) = oneshot::channel();
        {
            let mut owners = self.state.owners.lock().unwrap_or_else(|p| p.into_inner());
            owners.retain(|task| !task.is_finished());
            // Includes at most one just-finishing owner beyond the physical slots.
            // A full book refuses before any native process can be prepared.
            if owners.len() > self.state.capacity {
                let mut lease = lease;
                lease.resources.take();
                return Err(WorkerFailure::Busy);
            }
            owners.push(tokio::spawn(owner(
                lease,
                spec,
                input,
                cancel,
                stop,
                deadline.min(Instant::now() + Duration::from_secs(300)),
                launcher,
                sender,
            )));
        }
        receiver.await.unwrap_or(Err(WorkerFailure::CleanupUnknown))
    }
}

async fn capture(
    mut reader: impl AsyncRead + Unpin,
    limit: usize,
) -> Result<Vec<u8>, WorkerFailure> {
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let count = reader
            .read(&mut chunk)
            .await
            .map_err(|_| WorkerFailure::InvalidFrame)?;
        if count == 0 {
            return Ok(bytes);
        }
        if count > limit.saturating_sub(bytes.len()) {
            return Err(WorkerFailure::OutputLimit);
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
}
async fn exchange(
    mut stdin: Box<dyn AsyncWrite + Send + Unpin>,
    mut stdout: Box<dyn AsyncRead + Send + Unpin>,
    input: Vec<u8>,
) -> Result<Vec<u8>, WorkerFailure> {
    let mut ready = [0; READY.len()];
    stdout
        .read_exact(&mut ready)
        .await
        .map_err(|_| WorkerFailure::InvalidFrame)?;
    if ready != READY {
        return Err(WorkerFailure::InvalidFrame);
    }
    stdin
        .write_all(&input)
        .await
        .map_err(|_| WorkerFailure::Stdin)?;
    stdin.shutdown().await.map_err(|_| WorkerFailure::Stdin)?;
    drop(stdin);
    capture(stdout, MAX_STDOUT).await
}

async fn owner(
    mut lease: Lease,
    spec: LaunchSpec,
    input: Vec<u8>,
    cancel: CancelSignal,
    stop: CancellationToken,
    deadline: Instant,
    launcher: Arc<dyn WorkerLauncher>,
    sender: oneshot::Sender<Result<WorkerResult, WorkerFailure>>,
) {
    let prepared = tokio::select! { biased;
        _ = cancel.cancelled() => Err(WorkerFailure::Cancelled),
        _ = stop.cancelled() => Err(WorkerFailure::Cancelled),
        _ = tokio::time::sleep_until(deadline) => Err(WorkerFailure::TimedOut),
        child = launcher.prepare(spec) => child,
    };
    let resources = lease.resources.as_mut().expect("owned lease");
    let child = match prepared {
        Ok(child) => child,
        Err(failure) => {
            lease.resources.take();
            let _ = sender.send(Err(failure));
            return;
        }
    };
    resources.child = Some(child);
    let child = resources.child.as_mut().unwrap();
    let started = tokio::select! { biased;
        _ = cancel.cancelled() => Err(WorkerFailure::Cancelled),
        _ = stop.cancelled() => Err(WorkerFailure::Cancelled),
        _ = tokio::time::sleep_until(deadline) => Err(WorkerFailure::TimedOut),
        result = child.start() => result,
    };
    let mut failure = started.err();
    if failure.is_none() {
        match child.take_pipes() {
            Ok(pipes) => {
                resources.stdout = Some(tokio::spawn(exchange(pipes.stdin, pipes.stdout, input)));
                resources.stderr = Some(tokio::spawn(capture(pipes.stderr, MAX_STDERR)));
            }
            Err(error) => failure = Some(error),
        }
    }
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    while failure.is_none() && (resources.stdout.is_some() || resources.stderr.is_some()) {
        tokio::select! { biased;
            _ = cancel.cancelled() => failure = Some(WorkerFailure::Cancelled),
            _ = stop.cancelled() => failure = Some(WorkerFailure::Cancelled),
            _ = tokio::time::sleep_until(deadline) => failure = Some(WorkerFailure::TimedOut),
            value = async { resources.stdout.as_mut().unwrap().await }, if resources.stdout.is_some() => {
                resources.stdout.take(); match value { Ok(Ok(bytes)) => stdout = bytes, Ok(Err(error)) => failure=Some(error), Err(_) => failure=Some(WorkerFailure::Unavailable) }
            }
            value = async { resources.stderr.as_mut().unwrap().await }, if resources.stderr.is_some() => {
                resources.stderr.take(); match value { Ok(Ok(bytes)) => stderr = bytes, Ok(Err(error)) => failure=Some(error), Err(_) => failure=Some(WorkerFailure::Unavailable) }
            }
        }
    }
    let child = resources.child.as_mut().unwrap();
    let mut exit_code = 0;
    if failure.is_none() {
        let leader = tokio::select! { biased;
            _ = cancel.cancelled() => Err(WorkerFailure::Cancelled),
            _ = stop.cancelled() => Err(WorkerFailure::Cancelled),
            _ = tokio::time::sleep_until(deadline) => Err(WorkerFailure::TimedOut),
            result = child.wait_leader() => result,
        };
        match leader {
            Ok(code) => exit_code = code,
            Err(error) => failure = Some(error),
        }
    }
    // This path has no broker mutex or native save wait. Cancellation closes
    // admission before physical termination while durable saves retain custody.
    child.close_admission();
    if failure.is_some() {
        cancel.cancel();
    }
    // Only the logical response has a bounded grace period. This timer never
    // polls, drops, recreates or takes ownership away from the whole-tree wait.
    // PhysicalState retains this owner task, which retains its complete lease.
    let response = Arc::new(Mutex::new(Some(sender)));
    let response_done = CancellationToken::new();
    let timer_done = response_done.clone();
    let timer_response = response.clone();
    let response_timer = tokio::spawn(async move {
        tokio::select! { biased;
            _ = timer_done.cancelled() => {},
            _ = tokio::time::sleep(Duration::from_secs(2)) => {
                respond(&timer_response, Err(WorkerFailure::CleanupUnknown));
            }
        }
    });
    let killed = child.terminate_tree().await;
    let waited = child.wait_tree().await; // exactly once; never timeout/recreate
    if killed.is_err() || waited.is_err() {
        response_done.cancel();
        let _ = response_timer.await;
        respond(&response, Err(WorkerFailure::CleanupUnknown));
        return;
    }
    for task in [&mut resources.stdout, &mut resources.stderr] {
        if let Some(handle) = task.as_mut() {
            if !matches!(
                tokio::time::timeout(Duration::from_secs(2), handle).await,
                Ok(Ok(_))
            ) {
                response_done.cancel();
                let _ = response_timer.await;
                respond(&response, Err(WorkerFailure::CleanupUnknown));
                return;
            }
            task.take();
        }
    }
    let result = if let Some(error) = failure {
        Err(error)
    } else if cancel.is_cancelled() || stop.is_cancelled() {
        Err(WorkerFailure::Cancelled)
    } else if Instant::now() >= deadline {
        Err(WorkerFailure::TimedOut)
    } else {
        Ok(WorkerResult {
            mode: "one_shot",
            exit_code,
            stdout,
            stderr,
            cleanup_verified: true,
        })
    };
    response_done.cancel();
    let _ = response_timer.await;
    lease.resources.take(); // whole tree and both pipes retired before capacity
    respond(&response, result);
}

type Response = Arc<Mutex<Option<oneshot::Sender<Result<WorkerResult, WorkerFailure>>>>>;
fn respond(response: &Response, result: Result<WorkerResult, WorkerFailure>) {
    if let Some(sender) = response.lock().unwrap_or_else(|p| p.into_inner()).take() {
        let _ = sender.send(result);
    }
}

#[cfg(test)]
#[path = "worker_tests.rs"]
mod tests;
