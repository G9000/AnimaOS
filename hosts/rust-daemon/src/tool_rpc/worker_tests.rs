//! Synthetic duplex peers; these are not Windows Job acceptance evidence.
use super::*;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Mutex,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::Notify,
    time::{timeout, Duration},
};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy)]
enum Mode {
    Valid,
    LaunchFailure,
    StdinFailure,
    BadReady,
    StdoutFlood,
    StderrFlood,
    Stall,
    LeaderWithDescendant,
    UnknownTree,
    StalledTreeWait,
}

#[derive(Default)]
struct Trace {
    launched: AtomicBool,
    input: Mutex<Vec<u8>>,
    spec: Mutex<Option<LaunchSpec>>,
    events: Mutex<Vec<&'static str>>,
    child_alive: AtomicBool,
    pipe_alive: AtomicBool,
    launched_notify: Notify,
    wait_calls: AtomicUsize,
    wait_entered: Notify,
    wait_release: Mutex<Option<Arc<Semaphore>>>,
}
impl Trace {
    fn wait_gate(&self) -> Arc<Semaphore> {
        self.wait_release
            .lock()
            .unwrap()
            .get_or_insert_with(|| Arc::new(Semaphore::new(0)))
            .clone()
    }

    async fn wait_tree_started(&self) {
        timeout(Duration::from_secs(2), async {
            loop {
                let notified = self.wait_entered.notified();
                if self.wait_calls.load(Ordering::SeqCst) > 0 {
                    return;
                }
                notified.await;
            }
        })
        .await
        .expect("owned whole-tree wait started");
    }
    fn event(&self, event: &'static str) {
        self.events.lock().unwrap().push(event);
    }
    async fn wait_launched(&self) {
        timeout(Duration::from_secs(2), async {
            loop {
                let notified = self.launched_notify.notified();
                if self.launched.load(Ordering::SeqCst) {
                    break;
                }
                notified.await;
            }
        })
        .await
        .expect("the owned fake peer must actually launch");
    }
}

struct PeerLauncher {
    mode: Mode,
    trace: Arc<Trace>,
}
struct Peer {
    pipes: Option<WorkerPipes>,
    mode: Mode,
    trace: Arc<Trace>,
    stop: CancellationToken,
    task: Option<tokio::task::JoinHandle<()>>,
    start: Arc<Semaphore>,
}

#[async_trait]
impl WorkerLauncher for PeerLauncher {
    async fn prepare(&self, spec: LaunchSpec) -> Result<Box<dyn OwnedWorker>, WorkerFailure> {
        *self.trace.spec.lock().unwrap() = Some(spec);
        let (stdin, mut input) = tokio::io::duplex(1024);
        let (stdout, mut output) = tokio::io::duplex(1024);
        let (stderr, mut errors) = tokio::io::duplex(1024);
        let trace = self.trace.clone();
        let mode = self.mode;
        let stop = CancellationToken::new();
        let stopped = stop.clone();
        let start = Arc::new(Semaphore::new(0));
        let started = start.clone();
        let task = tokio::spawn(async move {
            let operation = async {
                let _ = started.acquire().await;
                if matches!(mode, Mode::Stall) {
                    std::future::pending::<()>().await;
                }
                let ready = if matches!(mode, Mode::BadReady) {
                    b"WRONG-TOOL-WORKER-v1\n".as_slice()
                } else {
                    READY
                };
                if output.write_all(ready).await.is_err() {
                    return;
                }
                if matches!(mode, Mode::StdinFailure) {
                    drop(input);
                    std::future::pending::<()>().await;
                    return;
                }
                let mut bytes = Vec::new();
                if input.read_to_end(&mut bytes).await.is_err() {
                    return;
                }
                *trace.input.lock().unwrap() = bytes;
                match mode {
                    Mode::StdoutFlood => {
                        let _ = output.write_all(&vec![b'x'; MAX_STDOUT + 1]).await;
                    }
                    Mode::StderrFlood => {
                        let _ = errors.write_all(&vec![b'e'; MAX_STDERR + 1]).await;
                    }
                    Mode::LeaderWithDescendant | Mode::UnknownTree | Mode::StalledTreeWait => {
                        trace.event("leader_exit");
                        std::future::pending::<()>().await;
                    }
                    _ => {
                        let _ = output.write_all(b"native-name client result\n").await;
                    }
                }
            };
            tokio::select! { _ = stopped.cancelled() => {}, _ = operation => {} }
            trace.pipe_alive.store(false, Ordering::SeqCst);
        });
        Ok(Box::new(Peer {
            pipes: Some(WorkerPipes {
                stdin: Box::new(stdin),
                stdout: Box::new(stdout),
                stderr: Box::new(stderr),
            }),
            mode,
            trace: self.trace.clone(),
            stop,
            task: Some(task),
            start,
        }))
    }
}

#[async_trait]
impl OwnedWorker for Peer {
    async fn start(&mut self) -> Result<(), WorkerFailure> {
        self.trace.launched.store(true, Ordering::SeqCst);
        self.trace.launched_notify.notify_waiters();
        if matches!(self.mode, Mode::LaunchFailure) {
            return Err(WorkerFailure::Launch);
        }
        self.trace.child_alive.store(true, Ordering::SeqCst);
        self.trace.pipe_alive.store(true, Ordering::SeqCst);
        self.start.add_permits(1);
        Ok(())
    }
    fn take_pipes(&mut self) -> Result<WorkerPipes, WorkerFailure> {
        self.pipes.take().ok_or(WorkerFailure::Unavailable)
    }
    fn close_admission(&mut self) {
        self.trace.event("admission_closed");
    }
    async fn wait_leader(&mut self) -> Result<i32, WorkerFailure> {
        if matches!(self.mode, Mode::Stall) {
            self.stop.cancelled().await;
        }
        self.trace.event("leader_wait");
        Ok(0)
    }
    async fn terminate_tree(&mut self) -> Result<(), WorkerFailure> {
        self.trace.event("tree_kill");
        self.stop.cancel();
        Ok(())
    }
    async fn wait_tree(&mut self) -> Result<(), WorkerFailure> {
        self.trace.event("tree_wait");
        self.trace.wait_calls.fetch_add(1, Ordering::SeqCst);
        self.trace.wait_entered.notify_waiters();
        if matches!(self.mode, Mode::StalledTreeWait) {
            self.trace
                .wait_gate()
                .acquire_owned()
                .await
                .unwrap()
                .forget();
        }
        if matches!(self.mode, Mode::UnknownTree) {
            return Err(WorkerFailure::CleanupUnknown);
        }
        if let Some(task) = self.task.take() {
            task.await.map_err(|_| WorkerFailure::CleanupUnknown)?;
        }
        self.trace.child_alive.store(false, Ordering::SeqCst);
        Ok(())
    }
}

struct Fixture {
    root: tempfile::TempDir,
    trace: Arc<Trace>,
    runtime: Arc<WorkerRuntime>,
}
impl Fixture {
    fn new() -> Self {
        Self {
            root: tempfile::tempdir().unwrap(),
            trace: Arc::new(Trace::default()),
            runtime: Arc::new(WorkerRuntime::new()),
        }
    }
    fn config(&self) -> WorkerConfig {
        let interpreter = self.root.path().join("explicit-python.exe");
        std::fs::write(&interpreter, b"synthetic file, never executed").unwrap();
        WorkerConfig {
            interpreter: Some(interpreter),
            workspace: self.root.path().to_path_buf(),
        }
    }
    fn launcher(&self, mode: Mode) -> Arc<dyn WorkerLauncher> {
        Arc::new(PeerLauncher {
            mode,
            trace: self.trace.clone(),
        })
    }
    async fn run(&self, mode: Mode) -> Result<WorkerResult, WorkerFailure> {
        self.runtime
            .run(
                self.config(),
                request(),
                bootstrap(),
                CancelSignal::new(),
                Instant::now() + Duration::from_millis(150),
                self.launcher(mode),
            )
            .await
    }
}
fn request() -> ProgramRequest {
    ProgramRequest {
        code: "print('owner private source 8327')".into(),
        reset: false,
    }
}
fn bootstrap() -> WorkerBootstrap {
    WorkerBootstrap {
        endpoint: "127.0.0.1:43210".parse().unwrap(),
        token: "private-cell-token-5839".into(),
        cell_epoch: "private-cell-epoch-2746".into(),
    }
}

#[tokio::test]
async fn fp06_worker_requires_explicit_absolute_interpreter_without_path_discovery() {
    let fixture = Fixture::new();
    for path in [
        None,
        Some(PathBuf::from("python")),
        Some(fixture.root.path().join("absent.exe")),
    ] {
        let mut config = fixture.config();
        config.interpreter = path;
        assert_eq!(
            fixture
                .runtime
                .run(
                    config,
                    request(),
                    bootstrap(),
                    CancelSignal::new(),
                    Instant::now() + Duration::from_secs(1),
                    fixture.launcher(Mode::Valid)
                )
                .await,
            Err(WorkerFailure::MissingInterpreter)
        );
        assert!(!fixture.trace.launched.load(Ordering::SeqCst));
    }
}

#[tokio::test]
async fn fp06_worker_rejects_oversized_source_before_launch() {
    let fixture = Fixture::new();
    let mut source = request();
    source.code = "x".repeat(anima_core::tool_rpc::MAX_CODE_BYTES + 1);
    assert_eq!(
        fixture
            .runtime
            .run(
                fixture.config(),
                source,
                bootstrap(),
                CancelSignal::new(),
                Instant::now() + Duration::from_secs(1),
                fixture.launcher(Mode::Valid)
            )
            .await,
        Err(WorkerFailure::InvalidInput)
    );
    assert!(!fixture.trace.launched.load(Ordering::SeqCst));
}

#[tokio::test]
async fn fp06_worker_already_cancelled_or_expired_request_never_starts_peer() {
    for expired in [false, true] {
        let fixture = Fixture::new();
        let cancel = CancelSignal::new();
        let deadline = if expired {
            Instant::now() - Duration::from_millis(1)
        } else {
            cancel.cancel();
            Instant::now() + Duration::from_secs(1)
        };
        assert_eq!(
            fixture
                .runtime
                .run(
                    fixture.config(),
                    request(),
                    bootstrap(),
                    cancel,
                    deadline,
                    fixture.launcher(Mode::Valid)
                )
                .await,
            Err(if expired {
                WorkerFailure::TimedOut
            } else {
                WorkerFailure::Cancelled
            })
        );
        assert!(!fixture.trace.launched.load(Ordering::SeqCst));
        assert_eq!(fixture.runtime.available_slots(), 1);
    }
}

#[tokio::test]
async fn fp06_worker_success_requires_private_stdin_and_verified_whole_tree_and_pipes() {
    let fixture = Fixture::new();
    let result = fixture
        .run(Mode::Valid)
        .await
        .expect("valid fake interpreter protocol");
    assert_eq!(result.mode, "one_shot");
    assert_eq!(result.exit_code, 0);
    assert!(result.cleanup_verified);
    assert_eq!(result.stdout, b"native-name client result\n");
    assert!(result.stderr.is_empty());
    let spec = fixture.trace.spec.lock().unwrap();
    let spec = spec.as_ref().unwrap();
    assert!(spec.interpreter.is_absolute());
    assert_eq!(spec.workspace, fixture.root.path());
    for value in spec
        .args
        .iter()
        .chain(spec.environment.iter().map(|(_, value)| value))
    {
        assert!(!value.contains("owner private source"));
        assert!(!value.contains("private-cell-token"));
        assert!(!value.contains("private-cell-epoch"));
    }
    assert!(spec.args.iter().any(|arg| arg == "-I"));
    assert!(spec.args.iter().any(|arg| arg == "-S"));
    assert!(spec.args.iter().any(|arg| arg == "-u"));
    let input = String::from_utf8(fixture.trace.input.lock().unwrap().clone()).unwrap();
    assert!(input.contains("owner private source 8327"));
    assert!(input.contains("private-cell-token-5839"));
    assert!(!fixture.trace.child_alive.load(Ordering::SeqCst));
    assert!(!fixture.trace.pipe_alive.load(Ordering::SeqCst));
    let events = fixture.trace.events.lock().unwrap();
    assert!(
        events
            .iter()
            .position(|e| *e == "admission_closed")
            .unwrap()
            < events.iter().position(|e| *e == "tree_kill").unwrap()
    );
    assert!(events.contains(&"tree_wait"));
    assert_eq!(fixture.runtime.available_slots(), 1);
}

#[tokio::test]
async fn fp06_worker_launch_stdin_and_malformed_ready_fail_without_claimed_success() {
    for (mode, expected) in [
        (Mode::LaunchFailure, WorkerFailure::Launch),
        (Mode::StdinFailure, WorkerFailure::Stdin),
        (Mode::BadReady, WorkerFailure::InvalidFrame),
    ] {
        let fixture = Fixture::new();
        assert_eq!(fixture.run(mode).await, Err(expected));
        if !matches!(mode, Mode::LaunchFailure) {
            assert!(fixture.trace.events.lock().unwrap().contains(&"tree_wait"));
            assert!(!fixture.trace.child_alive.load(Ordering::SeqCst));
        }
    }
}

#[tokio::test]
async fn fp06_worker_both_output_streams_have_finite_byte_limits() {
    for mode in [Mode::StdoutFlood, Mode::StderrFlood] {
        let fixture = Fixture::new();
        assert_eq!(fixture.run(mode).await, Err(WorkerFailure::OutputLimit));
        assert!(!fixture.trace.child_alive.load(Ordering::SeqCst));
        assert!(!fixture.trace.pipe_alive.load(Ordering::SeqCst));
    }
}

#[tokio::test]
async fn fp06_worker_deadline_covers_handshake_and_leader_exit_with_live_descendant_pipes() {
    for mode in [Mode::Stall, Mode::LeaderWithDescendant] {
        let fixture = Fixture::new();
        assert_eq!(
            timeout(Duration::from_secs(2), fixture.run(mode))
                .await
                .unwrap(),
            Err(WorkerFailure::TimedOut)
        );
        assert!(!fixture.trace.child_alive.load(Ordering::SeqCst));
        assert!(!fixture.trace.pipe_alive.load(Ordering::SeqCst));
        assert_eq!(fixture.runtime.available_slots(), 1);
    }
}

#[tokio::test]
async fn fp06_worker_dropped_caller_still_closes_admission_and_owns_tree_cleanup() {
    let fixture = Fixture::new();
    let runtime = fixture.runtime.clone();
    let config = fixture.config();
    let launcher = fixture.launcher(Mode::Stall);
    let caller = tokio::spawn(async move {
        runtime
            .run(
                config,
                request(),
                bootstrap(),
                CancelSignal::new(),
                Instant::now() + Duration::from_secs(5),
                launcher,
            )
            .await
    });
    fixture.trace.wait_launched().await;
    caller.abort();
    let _ = caller.await;
    timeout(Duration::from_secs(2), async {
        while fixture.runtime.available_slots() == 0
            || fixture.trace.child_alive.load(Ordering::SeqCst)
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("physical owner must finish after caller future is dropped");
    assert!(fixture
        .trace
        .events
        .lock()
        .unwrap()
        .contains(&"admission_closed"));
    assert!(fixture.trace.events.lock().unwrap().contains(&"tree_wait"));
    assert!(!fixture.trace.pipe_alive.load(Ordering::SeqCst));
}

#[tokio::test]
async fn fp06_worker_unproven_descendant_cleanup_quarantines_exact_capacity() {
    let fixture = Fixture::new();
    assert_eq!(
        fixture.run(Mode::UnknownTree).await,
        Err(WorkerFailure::CleanupUnknown)
    );
    assert_eq!(
        fixture.runtime.available_slots(),
        0,
        "unknown tree keeps its physical permit"
    );
    assert_eq!(fixture.run(Mode::Valid).await, Err(WorkerFailure::Busy));
    assert!(
        fixture.trace.child_alive.load(Ordering::SeqCst),
        "no fabricated whole-tree retirement"
    );
}

#[tokio::test]
async fn fp06_worker_stalled_whole_tree_wait_retains_owner_and_reports_pending_cleanup() {
    let fixture = Fixture::new();
    let runtime = fixture.runtime.clone();
    let config = fixture.config();
    let launcher = fixture.launcher(Mode::StalledTreeWait);
    let mut caller = tokio::spawn(async move {
        runtime
            .run(
                config,
                request(),
                bootstrap(),
                CancelSignal::new(),
                Instant::now() + Duration::from_millis(100),
                launcher,
            )
            .await
    });
    fixture.trace.wait_tree_started().await;
    let bounded_reply = timeout(Duration::from_secs(3), &mut caller).await;
    let held_slots = fixture.runtime.available_slots();
    let refused_replacement = fixture.run(Mode::Valid).await;
    let single_wait_before_release = fixture.trace.wait_calls.load(Ordering::SeqCst);
    let admission_closed = fixture
        .trace
        .events
        .lock()
        .unwrap()
        .contains(&"admission_closed");
    // Always unblock the synthetic wait even on RED, preserving test custody.
    fixture.trace.wait_gate().add_permits(1);
    let reply = match bounded_reply {
        Ok(result) => Some(result.unwrap()),
        Err(_) => {
            let _ = timeout(Duration::from_secs(2), caller).await;
            None
        }
    };
    timeout(Duration::from_secs(2), async {
        while fixture.runtime.available_slots() == 0
            || fixture.trace.child_alive.load(Ordering::SeqCst)
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("late proven retirement releases exact capacity");
    assert_eq!(
        reply,
        Some(Err(WorkerFailure::CleanupUnknown)),
        "logical pending-cleanup response must be bounded without cancelling the owned wait"
    );
    assert_eq!(held_slots, 0);
    assert_eq!(refused_replacement, Err(WorkerFailure::Busy));
    assert!(admission_closed);
    assert_eq!(single_wait_before_release, 1);
    assert_eq!(
        fixture.trace.wait_calls.load(Ordering::SeqCst),
        1,
        "never recreate the whole-tree wait"
    );
    assert!(!fixture.trace.pipe_alive.load(Ordering::SeqCst));
}
