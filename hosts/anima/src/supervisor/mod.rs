//! `anima supervise`: runs the server (`anima run`) as a child, restarts it
//! when it crashes, and gates Stop / Restart / Quit / Update behind the
//! server's drain so running work is never cut off. The CLI and the server's
//! console relay drive it over the control API; the tray sends the same
//! requests through a `Link`.
//!
//! One task owns the child and the state. It never waits on anything outside
//! its `select!`, so an action request is always answered promptly.

mod child;
mod control_api;
mod logs;
mod state;
mod updates;
pub use logs::LOG_FILE_NAME;
pub use state::{Action, ServerState, SupervisorStatus};
#[cfg(test)]
mod lifecycle_tests;
#[cfg(test)]
mod state_tests;
pub mod tokens;

use std::collections::BTreeMap;
use std::future::Future;
use std::io;
use std::path::PathBuf;
use std::pin::Pin;
use std::process::{ExitCode, ExitStatus};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use anima_daemon::system::InFlight;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot, watch, Notify};
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::config::{validate_workspace, Config, WorkspaceProblem};
use crate::paths::AnimaPaths;
use crate::server_env::{effective_port, server_env};
use crate::update::{self, handoff};
use child::{ServerApi, ServerChild, ShutdownRequest};
use control_api::ActionRequest;
use logs::ServerLog;
use state::{backoff_secs, next, Conflict, Refusal, Transition, UpdateStatus, UNFINISHED_UPDATE};
use tokens::{AdminToken, ControlToken};
use updates::{JobProgress, Prepared, Progress, Updater};

/// The status the control API serves; the lifecycle task keeps it current.
pub type SharedStatus = Arc<Mutex<SupervisorStatus>>;

const READY_TIMEOUT: Duration = Duration::from_secs(60);
/// `/health` must still answer this long after it first did, with the
/// child alive, before the server counts as running.
const READY_CONFIRM: Duration = Duration::from_secs(1);
/// How long a server left from an earlier supervisor gets to shut down.
const CLEAR_TIMEOUT: Duration = Duration::from_secs(45);
const HEALTH_POLL: Duration = Duration::from_millis(250);
/// Retries of a cancel the server has not confirmed: 0.5, 1, 2, … capped.
const CANCEL_RETRY_FIRST: Duration = Duration::from_millis(250);
const CANCEL_RETRY_MAX: Duration = Duration::from_secs(5);
const DRAIN_POLL: Duration = Duration::from_secs(1);
/// How many `inFlight.total() == 0` readings in a row settle a drain: a
/// single zero can be read before new work has registered.
const SETTLED_READINGS: u32 = 2;
const EXIT_GRACE: Duration = Duration::from_secs(40);
/// After a kill, how long to wait for the exit before giving up on it.
const KILL_GRACE: Duration = Duration::from_secs(10);
/// Running this long forgives the earlier crashes.
const STABLE_RUN: Duration = Duration::from_secs(10 * 60);
const CONNECTORS_REFRESH: Duration = Duration::from_secs(10);
const CONNECTORS_RETRY: Duration = Duration::from_secs(1);
const PORT_IN_USE: [&str; 2] = ["Address already in use", "os error 10048"];
/// How long a supervisor started by an update waits for the one before it to
/// let go of the control port (Windows, where it is not an `exec`).
const HANDOFF_BIND_WAIT: Duration = Duration::from_secs(15);
const BIND_RETRY: Duration = Duration::from_millis(250);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Runs the supervisor until it is asked to quit, with the tray when `tray`
/// is set. A supervisor already listening on the control port means there is
/// nothing to do. Before `anima setup` has chosen a workspace it refuses.
///
/// On Windows, `background` (the start-at-login entry) runs in a console of
/// its own: the supervisor starts itself again without one, and this copy exits.
pub fn supervise(tray: bool, background: bool) -> io::Result<ExitCode> {
    let paths = AnimaPaths::discover()?;
    paths.ensure()?;
    let cfg = Config::load(&paths.config_file)?;
    if let Some(code) = crate::commands::refuse_unless_set_up(&cfg) {
        return Ok(code);
    }
    #[cfg(windows)]
    if background && crate::commands::open::needs_detaching() {
        return crate::commands::open::relaunch_detached();
    }
    #[cfg(not(windows))]
    let _ = background;
    let Some(supervisor) = Supervisor::bind(paths, cfg)? else {
        println!("Anima is already running");
        return Ok(ExitCode::SUCCESS);
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    if tray {
        crate::tray::run(runtime, supervisor)?;
    } else {
        runtime.block_on(supervisor.run())?;
    }
    Ok(ExitCode::SUCCESS)
}

/// A supervisor that holds the control port, ready to run.
pub struct Supervisor {
    paths: AnimaPaths,
    cfg: Config,
    listener: std::net::TcpListener,
    control_token: ControlToken,
    admin_token: AdminToken,
    api: ServerApi,
    status: SharedStatus,
    log: ServerLog,
    requests_tx: mpsc::Sender<ActionRequest>,
    requests: mpsc::Receiver<ActionRequest>,
    /// The update this version was installed by, until its server runs.
    handoff: Option<handoff::PendingUpdate>,
}

impl Supervisor {
    /// Takes the control port; `None` when another supervisor has it.
    fn bind(paths: AnimaPaths, cfg: Config) -> io::Result<Option<Self>> {
        let Some(listener) = bind_control_port(&paths, cfg.control_port)? else {
            return Ok(None);
        };
        listener.set_nonblocking(true)?;
        // Only the supervisor holding the port finishes an update.
        let mut update = UpdateStatus::default();
        let handoff = match handoff::on_start(&paths, env!("CARGO_PKG_VERSION")) {
            handoff::HandoffOutcome::Nothing => None,
            handoff::HandoffOutcome::AwaitHealth(pending) => Some(pending),
            handoff::HandoffOutcome::RolledBack { last_error } => {
                update.last_error = Some(last_error);
                None
            }
        };
        let (control_token, admin_token) = tokens::ensure(&paths)?;
        let process_env: BTreeMap<String, String> = std::env::vars().collect();
        let port = effective_port(&cfg, &process_env);
        let api_key = process_env
            .get("ANIMAOS_RS_API_KEY")
            .map(|key| key.trim().to_string())
            .filter(|key| !key.is_empty());
        let api = ServerApi::new(port, admin_token.clone(), api_key)?;
        let status: SharedStatus = Arc::new(Mutex::new(SupervisorStatus {
            version: env!("CARGO_PKG_VERSION").to_string(),
            state: ServerState::Starting,
            server_url: server_url(port),
            workspace: cfg.workspace.clone(),
            workspace_problem: workspace_problem(&cfg, &paths),
            data_dir: paths.data.clone(),
            autostart: cfg.autostart,
            connectors: None,
            pending: None,
            update,
        }));
        let (requests_tx, requests) = mpsc::channel(16);
        Ok(Some(Self {
            log: ServerLog::open(&paths.logs),
            paths,
            cfg,
            listener,
            control_token,
            admin_token,
            api,
            status,
            requests_tx,
            requests,
            handoff,
        }))
    }

    /// What the tray drives this supervisor with, on `runtime`.
    pub fn link(&self, runtime: tokio::runtime::Handle) -> Link {
        Link {
            requests: self.requests_tx.clone(),
            status: Arc::clone(&self.status),
            paths: self.paths.clone(),
            log: self.log.clone(),
            admin_token: self.admin_token.clone(),
            runtime,
        }
    }

    /// Runs until it is asked to quit.
    pub async fn run(self) -> io::Result<()> {
        let listener = TcpListener::from_std(self.listener)?;
        let router = control_api::router(
            self.control_token.clone(),
            Arc::clone(&self.status),
            self.requests_tx,
        );
        let stop_serving = Arc::new(Notify::new());
        let serving = {
            let stop = Arc::clone(&stop_serving);
            tokio::spawn(async move {
                axum::serve(listener, router)
                    .with_graceful_shutdown(async move { stop.notified().await })
                    .await
            })
        };

        let (api_tx, api_rx) = watch::channel(self.api.clone());
        let connectors = tokio::spawn(refresh_connectors(Arc::clone(&self.status), api_rx));
        let updater = Updater::new(Arc::clone(&self.status));
        let (check_now, check_requests) = mpsc::channel(1);
        let checks = tokio::spawn(updates::run_checks(
            updater.clone(),
            self.paths.config_file.clone(),
            check_requests,
        ));
        let (update_events, update_progress) = mpsc::unbounded_channel();
        let handoff = self.handoff.map(|pending| {
            self.log.note(&format!(
                "Updated from v{} to v{}; waiting for the server to run",
                pending.from, pending.to
            ));
            Handoff {
                pending,
                wait: HandoffWait::Until(Instant::now() + handoff::HEALTH_TIMEOUT),
                reason: String::new(),
            }
        });
        let lifecycle = Lifecycle {
            exe: std::env::current_exe()?,
            control_url: format!("http://127.0.0.1:{}", self.cfg.control_port),
            log: self.log,
            paths: self.paths,
            cfg: self.cfg,
            api: self.api,
            api_tx,
            status: self.status,
            child: None,
            crashes: 0,
            running_since: None,
            updater,
            check_now,
            update_events,
            next_job: 0,
            job: None,
            prepared: None,
            handoff,
        };
        lifecycle
            .run(self.requests, termination_signals(), update_progress)
            .await;

        connectors.abort();
        checks.abort();
        // Let the answer to the last request (a quit) reach its caller.
        stop_serving.notify_one();
        let _ = tokio::time::timeout(Duration::from_secs(2), serving).await;
        Ok(())
    }
}

/// The tray's handle on a supervisor: its status, and the same action
/// requests the control API makes.
#[derive(Clone)]
pub struct Link {
    requests: mpsc::Sender<ActionRequest>,
    status: SharedStatus,
    paths: AnimaPaths,
    log: ServerLog,
    /// Signs the console's launch codes.
    admin_token: AdminToken,
    runtime: tokio::runtime::Handle,
}

// Only the macOS and Windows trays use all of it.
#[cfg_attr(not(any(target_os = "macos", windows)), allow(dead_code))]
impl Link {
    pub fn status(&self) -> SupervisorStatus {
        lock(&self.status).clone()
    }

    /// The console's address with a one-time launch code, which lets it use
    /// the lifecycle controls.
    pub fn console_url(&self) -> String {
        crate::commands::open::console_url(
            &lock(&self.status).server_url,
            Some(self.admin_token.as_str()),
        )
    }

    pub fn runtime(&self) -> &tokio::runtime::Handle {
        &self.runtime
    }

    /// Requests `action` the way the control API does, so the same gate
    /// applies; a refusal goes to the log.
    pub fn request(&self, action: Action) {
        let requests = self.requests.clone();
        let log = self.log.clone();
        let status = Arc::clone(&self.status);
        self.runtime.spawn(async move {
            let (reply, answer) = oneshot::channel();
            let request = ActionRequest {
                action,
                force: false,
                reply,
            };
            if requests.send(request).await.is_err() {
                return;
            }
            match answer.await {
                Ok(Err(Refusal::Busy(Conflict { pending }))) => log.note(&format!(
                    "The tray asked for {action:?}, but {pending:?} is in progress"
                )),
                // The menu shows the update's last error.
                Ok(Err(Refusal::UnfinishedUpdate)) => {
                    log.note(UNFINISHED_UPDATE);
                    lock(&status).update.last_error = Some(UNFINISHED_UPDATE.to_string());
                }
                _ => {}
            }
        });
    }

    /// Turns start at login on or off, as `anima autostart on|off` does.
    pub fn set_autostart(&self, on: bool) -> io::Result<()> {
        crate::commands::autostart::apply(&self.paths, on)?;
        lock(&self.status).autostart = on;
        Ok(())
    }

    /// A line in the supervisor's log.
    pub fn note(&self, message: &str) {
        self.log.note(message);
    }
}

/// Binds the control port; `None` when another supervisor has it. A supervisor
/// started by an update hand-off on Windows waits a moment for the process
/// before it to exit and let go of the port.
fn bind_control_port(paths: &AnimaPaths, port: u16) -> io::Result<Option<std::net::TcpListener>> {
    let deadline = std::time::Instant::now() + HANDOFF_BIND_WAIT;
    loop {
        match std::net::TcpListener::bind(("127.0.0.1", port)) {
            Ok(listener) => return Ok(Some(listener)),
            Err(error) if error.kind() == io::ErrorKind::AddrInUse => {
                let waiting = cfg!(windows)
                    && handoff::under_way(paths)
                    && std::time::Instant::now() < deadline;
                if !waiting {
                    return Ok(None);
                }
                std::thread::sleep(BIND_RETRY);
            }
            Err(error) => return Err(error),
        }
    }
}

fn server_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

fn workspace_problem(cfg: &Config, paths: &AnimaPaths) -> Option<String> {
    let workspace = cfg.workspace.as_ref()?;
    let home = paths.root.parent()?;
    Some(validate_workspace(workspace, home, paths).err()?.message())
}

/// Why the server must not be started with this config: no workspace (the
/// server would fall back to its working folder), or one that holds the home
/// folder or Anima's own keys. A missing folder is not a reason: the server
/// starts with it and reports the problem.
fn unsafe_workspace(cfg: &Config, paths: &AnimaPaths) -> Option<String> {
    let Some(workspace) = cfg.workspace.as_ref() else {
        return Some(crate::commands::NOT_SET_UP.to_string());
    };
    let home = paths.root.parent()?;
    match validate_workspace(workspace, home, paths) {
        Err(
            problem @ (WorkspaceProblem::IsHome
            | WorkspaceProblem::ContainsDataRoot
            | WorkspaceProblem::InsideDataRoot),
        ) => Some(problem.message()),
        _ => None,
    }
}

/// Ctrl-C (and SIGTERM on Unix), each one sent as a `()`.
fn termination_signals() -> mpsc::Receiver<()> {
    let (tx, rx) = mpsc::channel(4);
    tokio::spawn(async move {
        #[cfg(unix)]
        let mut term =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).ok();
        loop {
            #[cfg(unix)]
            let terminated = async {
                match term.as_mut() {
                    Some(term) => {
                        term.recv().await;
                    }
                    None => std::future::pending::<()>().await,
                }
            };
            #[cfg(not(unix))]
            let terminated = std::future::pending::<()>();
            tokio::pin!(terminated);
            tokio::select! {
                result = tokio::signal::ctrl_c() => {
                    if result.is_err() {
                        // No Ctrl-C handler: keep waiting for SIGTERM alone.
                        (&mut terminated).await;
                    }
                }
                _ = &mut terminated => {}
            }
            if tx.send(()).await.is_err() {
                break;
            }
        }
    });
    rx
}

/// Keeps `connectors` current while the server is up, `None` otherwise.
async fn refresh_connectors(status: SharedStatus, api: watch::Receiver<ServerApi>) {
    loop {
        let up = matches!(
            lock(&status).state,
            ServerState::Running | ServerState::Draining { .. }
        );
        let count = if up {
            let api = api.borrow().clone();
            api.connector_count().await
        } else {
            None
        };
        lock(&status).connectors = count;
        // Until there is a count, look again soon.
        let pause = if count.is_some() {
            CONNECTORS_REFRESH
        } else {
            CONNECTORS_RETRY
        };
        tokio::time::sleep(pause).await;
    }
}

/// Where the lifecycle is; `ServerState` is what the status shows of it.
#[derive(Debug, Clone, Copy)]
enum Phase {
    /// Before spawning: a server left from an earlier supervisor that still
    /// answers on the port is asked to shut down, until `deadline`.
    Clearing {
        deadline: Instant,
    },
    Starting {
        deadline: Instant,
    },
    Running,
    /// Ending the drain on the server, `attempt` retries in.
    Cancelling {
        attempt: u32,
    },
    Draining {
        action: Action,
        /// The server took `POST /api/system/drain`.
        begun: bool,
        polls: u32,
        zeros: u32,
        in_flight: InFlight,
    },
    Stopping {
        action: Action,
        killed: bool,
    },
    Stopped,
    Crashed {
        retry_at: Instant,
    },
    /// An update is being checked, downloaded and verified (the `UpdateJob`)
    /// while the server keeps running; then it goes through the gate,
    /// without waiting for work when `force` is set.
    Preparing {
        force: bool,
    },
    /// The version this update installed did not come up: its server is
    /// killed before the previous binary is put back.
    RollingBack,
    Exit,
}

/// Where the lifecycle goes back to when an update is not installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Resume {
    Running,
    Starting,
    /// Running, with a drain on the server being ended.
    Cancelling,
    /// Stopped on purpose: it stays stopped.
    Stopped,
    /// Start the server.
    Start,
}

/// An update being got ready.
struct UpdateJob {
    id: u64,
    task: JoinHandle<()>,
    resume: Resume,
}

/// The update that installed this version, until its server runs.
struct Handoff {
    pending: handoff::PendingUpdate,
    wait: HandoffWait,
    /// Why it is being rolled back.
    reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HandoffWait {
    /// The server must be running by then, or the update is rolled back.
    Until(Instant),
    /// The owner stopped the server (or quit): no deadline until it starts again.
    Paused,
    RollingBack,
}

/// What a phase's own work came to.
enum Event {
    /// Nothing answers on the server port any more.
    PortFree,
    /// A server there would not shut down.
    PortBusy,
    Ready,
    NotReady,
    /// The server confirmed (`true`) that it is no longer draining.
    Cancelled(bool),
    Drain {
        reading: Option<child::DrainStatus>,
        began: bool,
    },
    KillNow,
    /// The server was killed and its exit never came.
    Gone,
    Retry,
}

type Work = Pin<Box<dyn Future<Output = Event> + Send>>;

struct Lifecycle {
    paths: AnimaPaths,
    cfg: Config,
    exe: PathBuf,
    control_url: String,
    api: ServerApi,
    api_tx: watch::Sender<ServerApi>,
    status: SharedStatus,
    log: ServerLog,
    child: Option<ServerChild>,
    /// Crashes in a row, for the backoff.
    crashes: u32,
    running_since: Option<Instant>,
    updater: Updater,
    /// Asks the update checks for one now.
    check_now: mpsc::Sender<()>,
    update_events: mpsc::UnboundedSender<JobProgress>,
    next_job: u64,
    job: Option<UpdateJob>,
    /// A verified binary on its way through the gate.
    prepared: Option<(Prepared, Resume)>,
    handoff: Option<Handoff>,
}

impl Lifecycle {
    async fn run(
        mut self,
        mut requests: mpsc::Receiver<ActionRequest>,
        mut signals: mpsc::Receiver<()>,
        mut update_progress: mpsc::UnboundedReceiver<JobProgress>,
    ) {
        let mut phase = self.start();
        while !matches!(phase, Phase::Exit) {
            let mut exit = self.child.as_ref().map(ServerChild::exit_watch);
            let mut work = self.work(phase);
            let handoff_due = match self.handoff.as_ref().map(|handoff| handoff.wait) {
                Some(HandoffWait::Until(at)) => Some(at),
                _ => None,
            };
            let handoff_timeout = async move {
                match handoff_due {
                    Some(at) => tokio::time::sleep_until(at).await,
                    None => std::future::pending().await,
                }
            };
            tokio::pin!(handoff_timeout);
            phase = loop {
                tokio::select! {
                    Some(request) = requests.recv() => {
                        if let Some(next) = self.handle(request, phase) {
                            break next;
                        }
                    }
                    status = child::exited(&mut exit) => break self.on_exit(phase, status),
                    event = &mut work => break self.on_event(phase, event),
                    Some((job, progress)) = update_progress.recv() => {
                        if let Some(next) = self.on_update(phase, job, progress) {
                            break next;
                        }
                    }
                    () = &mut handoff_timeout => break self.on_handoff_timeout(phase),
                    Some(()) = signals.recv() => break self.on_terminate(phase),
                }
            };
        }
    }

    fn set_state(&self, state: ServerState, pending: Option<Action>) {
        let mut status = lock(&self.status);
        status.state = state;
        status.pending = pending;
    }

    fn snapshot(&self) -> SupervisorStatus {
        lock(&self.status).clone()
    }

    /// The phase's own work, which owns everything it needs.
    fn work(&self, phase: Phase) -> Work {
        let api = self.api.clone();
        match phase {
            Phase::Clearing { deadline } => Box::pin(async move {
                if !api.healthy().await {
                    return Event::PortFree;
                }
                api.shutdown().await;
                loop {
                    tokio::time::sleep(HEALTH_POLL).await;
                    if !api.healthy().await {
                        return Event::PortFree;
                    }
                    if Instant::now() >= deadline {
                        return Event::PortBusy;
                    }
                }
            }),
            Phase::Starting { deadline } => Box::pin(async move {
                let answers = || async {
                    matches!(
                        tokio::time::timeout_at(deadline, api.healthy()).await,
                        Ok(true)
                    )
                };
                loop {
                    if Instant::now() >= deadline {
                        return Event::NotReady;
                    }
                    // A child that lost the port dies within this pause, so
                    // a server it did not start can't pass for it.
                    if answers().await {
                        tokio::time::sleep(READY_CONFIRM).await;
                        if answers().await {
                            return Event::Ready;
                        }
                    }
                    tokio::time::sleep(HEALTH_POLL).await;
                }
            }),
            Phase::Cancelling { attempt } => Box::pin(async move {
                if attempt > 0 {
                    let backoff = CANCEL_RETRY_FIRST.saturating_mul(1 << attempt.min(5));
                    tokio::time::sleep(backoff.min(CANCEL_RETRY_MAX)).await;
                }
                Event::Cancelled(api.end_drain().await)
            }),
            Phase::Draining { begun, polls, .. } => Box::pin(async move {
                if polls > 0 {
                    tokio::time::sleep(DRAIN_POLL).await;
                }
                if begun {
                    Event::Drain {
                        reading: api.drain_status().await,
                        began: false,
                    }
                } else {
                    Event::Drain {
                        reading: api.begin_drain().await,
                        began: true,
                    }
                }
            }),
            Phase::Stopping { killed: false, .. } => Box::pin(async move {
                if api.shutdown().await != ShutdownRequest::Unreachable {
                    tokio::time::sleep(EXIT_GRACE).await;
                }
                Event::KillNow
            }),
            Phase::Stopping { killed: true, .. } => Box::pin(async move {
                tokio::time::sleep(KILL_GRACE).await;
                Event::Gone
            }),
            Phase::Crashed { retry_at } => Box::pin(async move {
                tokio::time::sleep_until(retry_at).await;
                Event::Retry
            }),
            Phase::RollingBack => Box::pin(async move {
                tokio::time::sleep(KILL_GRACE).await;
                Event::Gone
            }),
            // The update job reports through its own channel.
            Phase::Running | Phase::Stopped | Phase::Preparing { .. } | Phase::Exit => {
                Box::pin(std::future::pending())
            }
        }
    }

    /// Takes a requested action, answering with the status once it is under way.
    fn handle(&mut self, request: ActionRequest, phase: Phase) -> Option<Phase> {
        // The owner's Cancel, Stop, Restart or Quit wins over an update that
        // is still downloading: it is dropped, and the action is taken as if
        // it had never begun.
        if matches!(phase, Phase::Preparing { .. })
            && matches!(
                request.action,
                Action::Cancel | Action::Stop | Action::Restart | Action::Quit
            )
        {
            let resumed = self.drop_preparation();
            if request.action == Action::Cancel {
                let _ = request.reply.send(Ok(self.snapshot()));
                return Some(resumed);
            }
            return Some(self.handle(request, resumed).unwrap_or(resumed));
        }
        let ActionRequest {
            action,
            force,
            reply,
        } = request;
        // The installed update's server must run (or be rolled back) first:
        // another one would replace the binary a rollback puts back.
        if action == Action::Update
            && self.handoff.as_ref().is_some_and(|handoff| {
                matches!(handoff.wait, HandoffWait::Until(_) | HandoffWait::Paused)
            })
        {
            let _ = reply.send(Err(Refusal::UnfinishedUpdate));
            return None;
        }
        let (state, pending) = {
            let status = lock(&self.status);
            (status.state.clone(), status.pending)
        };
        let transition = match next(&state, pending, action, force) {
            Ok(transition) => transition,
            Err(conflict) => {
                let _ = reply.send(Err(conflict.into()));
                return None;
            }
        };
        if matches!(action, Action::Stop | Action::Quit) {
            self.pause_handoff();
        }
        let next_phase = match transition {
            // `update --now` while the update downloads: once it is ready,
            // don't wait for work.
            Transition::Stay if action == Action::Update && force => match phase {
                Phase::Preparing { force: false } => Some(Phase::Preparing { force: true }),
                _ => None,
            },
            Transition::Stay => None,
            Transition::CheckUpdate => {
                // One already queued covers this one.
                let _ = self.check_now.try_send(());
                None
            }
            // The update is got ready first, with the server still running;
            // only a verified binary goes through the gate.
            Transition::Drain(Action::Update)
            | Transition::Shutdown(Action::Update)
            | Transition::Settle(Action::Update)
                if self.prepared.is_none() =>
            {
                let force = matches!(transition, Transition::Shutdown(_));
                Some(self.prepare_update(phase, force))
            }
            Transition::Drain(action) => Some(self.drain(action)),
            Transition::Shutdown(action) => Some(self.shut_down(action)),
            Transition::Spawn => {
                self.crashes = 0;
                Some(self.start())
            }
            Transition::CancelDrain => Some(self.cancel()),
            Transition::Settle(action) => Some(self.finish(action)),
            Transition::Exit => Some(self.exit()),
        };
        let _ = reply.send(Ok(self.snapshot()));
        next_phase
    }

    fn on_exit(&mut self, phase: Phase, status: Option<ExitStatus>) -> Phase {
        self.child = None;
        let status = status.map_or_else(|| "unknown status".to_string(), |s| s.to_string());
        self.log.note(&format!("The server exited ({status})"));
        match phase {
            Phase::Starting { .. } | Phase::Running | Phase::Cancelling { .. } => {
                // `anima run` reports a fatal error as its last line.
                let fatal = self
                    .log
                    .last_line()
                    .and_then(|line| line.strip_prefix("anima: ").map(str::to_string));
                let message = if self.log.tail_contains(&PORT_IN_USE) {
                    format!("Port {} is in use", self.port())
                } else if let Some(fatal) = fatal {
                    format!("The server stopped: {fatal}")
                } else {
                    format!("The server stopped unexpectedly ({status})")
                };
                self.crash(message)
            }
            Phase::Draining { action, .. } | Phase::Stopping { action, .. } => self.finish(action),
            Phase::RollingBack => self.roll_back_update(),
            // The update carries on; without a server it has nothing to drain.
            Phase::Preparing { .. }
            | Phase::Clearing { .. }
            | Phase::Stopped
            | Phase::Crashed { .. }
            | Phase::Exit => phase,
        }
    }

    fn on_event(&mut self, phase: Phase, event: Event) -> Phase {
        match (phase, event) {
            (Phase::Clearing { .. }, Event::PortFree) => self.spawn_child(),
            (Phase::Clearing { .. }, Event::PortBusy) => {
                self.log
                    .note("The server already on the port would not shut down");
                self.crash(format!("Port {} is in use", self.port()))
            }
            // The exit, if the child is gone, is handled as such next.
            (Phase::Starting { .. }, Event::Ready)
                if !self.child.as_ref().is_some_and(ServerChild::is_alive) =>
            {
                phase
            }
            (Phase::Starting { .. }, Event::Ready) => {
                self.running_since = Some(Instant::now());
                self.log.note("The server is ready");
                self.running()
            }
            (Phase::Starting { .. }, Event::NotReady) => {
                // Dropping the child kills it.
                self.child = None;
                self.log.note("The server did not become ready in time");
                self.crash(format!(
                    "The server did not start within {} seconds",
                    READY_TIMEOUT.as_secs()
                ))
            }
            (
                Phase::Draining {
                    action,
                    mut begun,
                    polls,
                    mut zeros,
                    mut in_flight,
                },
                Event::Drain { reading, began },
            ) => {
                match reading {
                    Some(reading) => {
                        if began {
                            begun = true;
                        } else if !reading.draining {
                            // The drain was ended elsewhere: begin it again.
                            begun = false;
                        }
                        in_flight = reading.in_flight;
                        zeros = if in_flight.total() == 0 { zeros + 1 } else { 0 };
                    }
                    // No answer in time: keep the last count and ask again.
                    None => zeros = 0,
                }
                if zeros >= SETTLED_READINGS {
                    return self.shut_down(action);
                }
                self.set_state(ServerState::Draining { action, in_flight }, Some(action));
                Phase::Draining {
                    action,
                    begun,
                    polls: polls.saturating_add(1),
                    zeros,
                    in_flight,
                }
            }
            (Phase::Stopping { action, .. }, Event::KillNow) => match self.child.as_mut() {
                Some(child) => {
                    self.log.note("Killing the server");
                    child.kill();
                    Phase::Stopping {
                        action,
                        killed: true,
                    }
                }
                None => self.finish(action),
            },
            (Phase::Stopping { action, .. }, Event::Gone) => self.finish(action),
            (Phase::RollingBack, Event::Gone) => self.roll_back_update(),
            (Phase::Crashed { .. }, Event::Retry) => self.start(),
            (Phase::Cancelling { .. }, Event::Cancelled(true)) => {
                self.log.note("Drain cancelled");
                self.running()
            }
            (Phase::Cancelling { attempt }, Event::Cancelled(false)) => {
                self.log
                    .note("The server has not confirmed the drain ended; trying again");
                Phase::Cancelling {
                    attempt: attempt.saturating_add(1),
                }
            }
            (phase, _) => phase,
        }
    }

    /// Ctrl-C or SIGTERM: quit without waiting for work; a second one kills.
    fn on_terminate(&mut self, phase: Phase) -> Phase {
        self.abandon_update();
        self.pause_handoff();
        match phase {
            Phase::Stopping { killed, .. } => {
                if !killed {
                    if let Some(child) = self.child.as_mut() {
                        child.kill();
                    }
                }
                self.set_state(ServerState::Stopping, Some(Action::Quit));
                Phase::Stopping {
                    action: Action::Quit,
                    killed: true,
                }
            }
            _ => self.shut_down(Action::Quit),
        }
    }

    fn port(&self) -> u16 {
        let process_env: BTreeMap<String, String> = std::env::vars().collect();
        effective_port(&self.cfg, &process_env)
    }

    /// Starts the server, with the config as it is now.
    fn start(&mut self) -> Phase {
        // A server stopped by its owner during an update's health wait gets
        // the full wait again.
        if let Some(handoff) = self.handoff.as_mut() {
            if handoff.wait == HandoffWait::Paused {
                handoff.wait = HandoffWait::Until(Instant::now() + handoff::HEALTH_TIMEOUT);
            }
        }
        match Config::load(&self.paths.config_file) {
            Ok(cfg) => self.cfg = cfg,
            Err(error) => self.log.note(&format!(
                "config.toml could not be read, keeping the last one: {error}"
            )),
        }
        let process_env: BTreeMap<String, String> = std::env::vars().collect();
        let port = effective_port(&self.cfg, &process_env);
        self.api.set_port(port);
        let _ = self.api_tx.send(self.api.clone());
        {
            let mut status = lock(&self.status);
            status.server_url = server_url(port);
            status.workspace = self.cfg.workspace.clone();
            status.workspace_problem = workspace_problem(&self.cfg, &self.paths);
            status.autostart = self.cfg.autostart;
        }
        self.running_since = None;
        self.set_state(ServerState::Starting, None);
        Phase::Clearing {
            deadline: Instant::now() + CLEAR_TIMEOUT,
        }
    }

    /// Spawns the server once its port is free, unless the workspace is
    /// one it must not work in: then it stays stopped, saying why.
    fn spawn_child(&mut self) -> Phase {
        if let Some(problem) = unsafe_workspace(&self.cfg, &self.paths) {
            self.log
                .note(&format!("Not starting the server: {problem}"));
            // Not the new version's fault: no rollback while it waits.
            self.pause_handoff();
            lock(&self.status).workspace_problem = Some(problem);
            self.set_state(ServerState::Stopped, None);
            return Phase::Stopped;
        }
        let process_env: BTreeMap<String, String> = std::env::vars().collect();
        let port = effective_port(&self.cfg, &process_env);
        let mut env = server_env(&self.cfg, &self.paths, &process_env);
        // The supervisor's own calls depend on the server holding this admin
        // token, so it wins over an inherited one. Both tokens go as files:
        // other processes of the same user can read a process's environment.
        env.insert(
            "ANIMA_LOCAL_ADMIN_TOKEN_FILE".to_string(),
            self.paths.admin_token.to_string_lossy().into_owned(),
        );
        env.insert("ANIMA_SUPERVISOR_URL".to_string(), self.control_url.clone());
        env.insert(
            "ANIMA_SUPERVISOR_TOKEN_FILE".to_string(),
            self.paths.control_token.to_string_lossy().into_owned(),
        );
        self.log.clear_tail();
        self.log
            .note(&format!("Starting the server on port {port}"));
        match ServerChild::spawn(&self.exe, &env, &self.paths.data, &self.log) {
            Ok(child) => {
                self.child = Some(child);
                Phase::Starting {
                    deadline: Instant::now() + READY_TIMEOUT,
                }
            }
            Err(error) => self.crash(format!("Could not start the server: {error}")),
        }
    }

    fn running(&mut self) -> Phase {
        if let Some(Handoff { pending, .. }) = self.handoff.take() {
            let mut status = lock(&self.status);
            if status.update.last_error.as_deref() == Some(UNFINISHED_UPDATE) {
                status.update.last_error = None;
            }
            drop(status);
            match handoff::confirm(&self.paths) {
                Ok(()) => self
                    .log
                    .note(&format!("The update to v{} is done", pending.to)),
                Err(error) => self.log.note(&format!(
                    "The update to v{} is done, but its pending marker stays: {error}",
                    pending.to
                )),
            }
        }
        self.set_state(ServerState::Running, None);
        Phase::Running
    }

    fn crash(&mut self, message: String) -> Phase {
        self.child = None;
        if self
            .running_since
            .take()
            .is_some_and(|since| since.elapsed() >= STABLE_RUN)
        {
            self.crashes = 0;
        }
        let retry_in_secs = backoff_secs(self.crashes);
        self.crashes = self.crashes.saturating_add(1);
        self.log
            .note(&format!("{message}; restarting in {retry_in_secs} s"));
        self.set_state(
            ServerState::Crashed {
                message,
                retry_in_secs,
            },
            None,
        );
        Phase::Crashed {
            retry_at: Instant::now() + Duration::from_secs(retry_in_secs),
        }
    }

    fn drain(&mut self, action: Action) -> Phase {
        self.log
            .note(&format!("Waiting for running work before {action:?}"));
        let in_flight = InFlight::default();
        self.set_state(ServerState::Draining { action, in_flight }, Some(action));
        Phase::Draining {
            action,
            begun: false,
            polls: 0,
            zeros: 0,
            in_flight,
        }
    }

    /// Ends the drain on the server. The state stays as it is (draining, or
    /// running) with `cancel` pending until the server confirms.
    fn cancel(&mut self) -> Phase {
        self.log.note("Cancelling the drain");
        // A cancelled update starts over with a fresh check.
        self.prepared = None;
        lock(&self.status).pending = Some(Action::Cancel);
        Phase::Cancelling { attempt: 0 }
    }

    /// Asks the server to shut down for `action`; the server then has
    /// `EXIT_GRACE` to exit before it is killed.
    fn shut_down(&mut self, action: Action) -> Phase {
        if self.child.is_none() {
            return self.finish(action);
        }
        self.log.note(&format!("Stopping the server ({action:?})"));
        self.set_state(ServerState::Stopping, Some(action));
        Phase::Stopping {
            action,
            killed: false,
        }
    }

    /// The server is gone: completes `action`.
    fn finish(&mut self, action: Action) -> Phase {
        self.child = None;
        self.running_since = None;
        match action {
            Action::Restart => {
                self.crashes = 0;
                self.start()
            }
            Action::Quit => self.exit(),
            Action::Update => self.hand_off_update(),
            _ => {
                self.set_state(ServerState::Stopped, None);
                Phase::Stopped
            }
        }
    }

    /// Starts getting an update ready, the server staying as it is.
    fn prepare_update(&mut self, phase: Phase, force: bool) -> Phase {
        let resume = match phase {
            Phase::Running => Resume::Running,
            Phase::Starting { .. } => Resume::Starting,
            Phase::Cancelling { .. } => Resume::Cancelling,
            Phase::Stopped => Resume::Stopped,
            _ => Resume::Start,
        };
        self.abandon_update();
        self.next_job += 1;
        let id = self.next_job;
        self.log.note("Checking for an update to install");
        self.set_state(
            ServerState::Updating {
                stage: "Checking for updates".to_string(),
            },
            Some(Action::Update),
        );
        let task = updates::prepare(self.updater.clone(), id, self.update_events.clone());
        self.job = Some(UpdateJob { id, task, resume });
        Phase::Preparing { force }
    }

    /// The update job's progress; a new phase once it is done.
    fn on_update(&mut self, phase: Phase, job: u64, progress: Progress) -> Option<Phase> {
        let Phase::Preparing { force } = phase else {
            return None;
        };
        if self.job.as_ref().map(|job| job.id) != Some(job) {
            return None;
        }
        match progress {
            Progress::Stage(stage) => {
                self.set_state(ServerState::Updating { stage }, Some(Action::Update));
                None
            }
            Progress::Done(outcome) => {
                let resume = self.job.take().map_or(Resume::Start, |job| job.resume);
                Some(match outcome {
                    Ok(prepared) => {
                        self.log
                            .note(&format!("v{} is downloaded and verified", prepared.version));
                        self.prepared = Some((prepared, resume));
                        self.gate_update(force)
                    }
                    Err(error) => self.update_failed(error, resume),
                })
            }
        }
    }

    /// A verified update goes through the gate like any other action.
    fn gate_update(&mut self, force: bool) -> Phase {
        if !self.child.as_ref().is_some_and(ServerChild::is_alive) {
            return self.finish(Action::Update);
        }
        if force {
            self.shut_down(Action::Update)
        } else {
            self.drain(Action::Update)
        }
    }

    /// The server is stopped: installs the verified binary and restarts as it.
    fn hand_off_update(&mut self) -> Phase {
        let Some((prepared, resume)) = self.prepared.take() else {
            return self.update_failed("No update is available".to_string(), Resume::Start);
        };
        let to = prepared.version;
        self.log.note(&format!("Installing v{to}"));
        self.set_state(
            ServerState::Updating {
                stage: format!("Installing v{to}"),
            },
            Some(Action::Update),
        );
        let installed = handoff::install(
            &self.paths,
            &self.exe,
            &prepared.binary,
            env!("CARGO_PKG_VERSION"),
            &to,
            update::swap,
        );
        let pending = match installed {
            Ok(pending) => pending,
            Err(error) => return self.update_failed(error.to_string(), resume),
        };
        self.log.note(&format!("Restarting as v{to}"));
        let error = handoff::restart_into(&self.exe);
        // Only reached when the restart failed: the old binary goes back.
        let _ = handoff::undo(&self.paths, &pending, &self.exe);
        self.update_failed(format!("Could not restart as v{to}: {error}"), resume)
    }

    /// The update was not installed: says why, and goes back to `resume`.
    fn update_failed(&mut self, error: String, resume: Resume) -> Phase {
        self.log
            .note(&format!("The update was not installed: {error}"));
        lock(&self.status).update.last_error = Some(error);
        self.resume(resume)
    }

    /// The owner took another action while the update was downloading: the
    /// update is dropped, and the server is as it was before.
    fn drop_preparation(&mut self) -> Phase {
        let resume = self.job.as_ref().map_or(Resume::Start, |job| job.resume);
        self.abandon_update();
        self.log.note("The update was cancelled");
        self.resume(resume)
    }

    /// Goes back to where the lifecycle was before an update began.
    fn resume(&mut self, resume: Resume) -> Phase {
        let alive = self.child.as_ref().is_some_and(ServerChild::is_alive);
        match resume {
            Resume::Running if alive => self.running(),
            Resume::Starting if alive => {
                self.set_state(ServerState::Starting, None);
                Phase::Starting {
                    deadline: Instant::now() + READY_TIMEOUT,
                }
            }
            Resume::Cancelling if alive => {
                self.set_state(ServerState::Running, None);
                self.cancel()
            }
            Resume::Stopped => {
                self.set_state(ServerState::Stopped, None);
                Phase::Stopped
            }
            _ => self.start(),
        }
    }

    /// Drops an update being got ready, or waiting at the gate.
    fn abandon_update(&mut self) {
        if let Some(job) = self.job.take() {
            job.task.abort();
        }
        self.prepared = None;
    }

    /// The owner stopped the server (or quit) before this version's server
    /// ran: no rollback behind their back. The pending update stays, and the
    /// wait starts again with the next start (or the next supervisor).
    fn pause_handoff(&mut self) {
        if let Some(handoff) = self.handoff.as_mut() {
            if matches!(handoff.wait, HandoffWait::Until(_)) {
                handoff.wait = HandoffWait::Paused;
                self.log.note(&format!(
                    "Stopped before v{} was confirmed running; the update waits for the next start",
                    handoff.pending.to
                ));
            }
        }
    }

    /// This version's server did not run in time after an update: kills it,
    /// then puts the previous binary back.
    fn on_handoff_timeout(&mut self, phase: Phase) -> Phase {
        let reason = match &lock(&self.status).state {
            ServerState::Crashed { message, .. } => message.clone(),
            _ => format!(
                "The server was not running within {} seconds",
                handoff::HEALTH_TIMEOUT.as_secs()
            ),
        };
        let Some(handoff) = self.handoff.as_mut() else {
            return phase;
        };
        handoff.wait = HandoffWait::RollingBack;
        handoff.reason = reason;
        let (from, to) = (handoff.pending.from.clone(), handoff.pending.to.clone());
        self.log.note(&format!(
            "v{to} did not start ({}); going back to v{from}",
            handoff.reason
        ));
        self.abandon_update();
        self.set_state(
            ServerState::Updating {
                stage: format!("Rolling back to v{from}"),
            },
            Some(Action::Update),
        );
        match self.child.as_mut() {
            Some(child) => {
                child.kill();
                Phase::RollingBack
            }
            None => self.roll_back_update(),
        }
    }

    /// Puts the previous binary back and restarts as it; it shows the failed update.
    fn roll_back_update(&mut self) -> Phase {
        self.child = None;
        let Some(Handoff {
            pending, reason, ..
        }) = self.handoff.take()
        else {
            return self.start();
        };
        let restored =
            handoff::roll_back(&self.paths, &pending, &self.exe, &reason, update::rollback);
        let message = match restored {
            Ok(()) => {
                self.log.note(&format!("Restarting as v{}", pending.from));
                let error = handoff::restart_into(&self.exe);
                format!(
                    "{}; restart Anima to finish ({error})",
                    handoff::rolled_back_message(&pending.to)
                )
            }
            Err(error) => format!(
                "Update to v{} failed to start, and v{} could not be restored: {error}",
                pending.to, pending.from
            ),
        };
        self.log.note(&message);
        lock(&self.status).update.last_error = Some(message);
        self.start()
    }

    fn exit(&mut self) -> Phase {
        self.abandon_update();
        self.log.note("Quitting");
        self.set_state(ServerState::Stopped, None);
        Phase::Exit
    }
}
