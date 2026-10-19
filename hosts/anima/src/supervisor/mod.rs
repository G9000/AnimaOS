//! `anima supervise`: runs the server (`anima run`) as a child, restarts it
//! when it crashes, and gates Stop / Restart / Quit / Update behind the
//! server's drain so running work is never cut off. The CLI, the tray and the
//! server's console relay drive it over the control API.
//!
//! One task owns the child and the state. It never waits on anything outside
//! its `select!`, so an action request is always answered promptly.

mod child;
mod control_api;
mod logs;
mod state;
#[cfg(test)]
mod state_tests;
pub mod tokens;

use std::collections::BTreeMap;
use std::future::Future;
use std::io;
use std::path::PathBuf;
use std::pin::Pin;
use std::process::ExitStatus;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use anima_daemon::system::InFlight;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, watch, Notify};

use tokio::time::Instant;

use crate::config::{validate_workspace, Config, WorkspaceProblem};
use crate::paths::AnimaPaths;
use crate::server_env::{effective_port, server_env};
use child::{ServerApi, ServerChild, ShutdownRequest};
use control_api::ActionRequest;
use logs::ServerLog;
use state::{backoff_secs, next, Action, ServerState, SupervisorStatus, Transition, UpdateStatus};
use tokens::{AdminToken, ControlToken};

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

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Runs the supervisor until it is asked to quit. A supervisor already
/// listening on the control port means there is nothing to do.
pub fn supervise() -> io::Result<()> {
    let paths = AnimaPaths::discover()?;
    paths.ensure()?;
    let cfg = Config::load(&paths.config_file)?;
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(supervise_async(paths, cfg))
}

async fn supervise_async(paths: AnimaPaths, cfg: Config) -> io::Result<()> {
    let listener = match TcpListener::bind(("127.0.0.1", cfg.control_port)).await {
        Ok(listener) => listener,
        Err(error) if error.kind() == io::ErrorKind::AddrInUse => {
            println!("Anima is already running");
            return Ok(());
        }
        Err(error) => return Err(error),
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
        update: UpdateStatus::default(),
    }));

    let (requests_tx, requests) = mpsc::channel(16);
    let router = control_api::router(control_token.clone(), Arc::clone(&status), requests_tx);
    let stop_serving = Arc::new(Notify::new());
    let serving = {
        let stop = Arc::clone(&stop_serving);
        tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async move { stop.notified().await })
                .await
        })
    };

    let (api_tx, api_rx) = watch::channel(api.clone());
    let connectors = tokio::spawn(refresh_connectors(Arc::clone(&status), api_rx));
    let lifecycle = Lifecycle {
        exe: std::env::current_exe()?,
        control_url: format!("http://127.0.0.1:{}", cfg.control_port),
        control_token,
        admin_token,
        log: ServerLog::open(&paths.logs),
        paths,
        cfg,
        api,
        api_tx,
        status,
        child: None,
        crashes: 0,
        running_since: None,
    };
    lifecycle.run(requests, termination_signals()).await;

    connectors.abort();
    // Let the answer to the last request (a quit) reach its caller.
    stop_serving.notify_one();
    let _ = tokio::time::timeout(Duration::from_secs(2), serving).await;
    Ok(())
}

fn server_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

fn workspace_problem(cfg: &Config, paths: &AnimaPaths) -> Option<String> {
    let workspace = cfg.workspace.as_ref()?;
    let home = paths.root.parent()?;
    let problem = validate_workspace(workspace, home, paths).err()?;
    Some(match problem {
        WorkspaceProblem::Missing(path) => {
            format!("Workspace folder not found: {}", path.display())
        }
        WorkspaceProblem::IsHome => "The workspace can't be your home folder".to_string(),
        WorkspaceProblem::ContainsDataRoot => {
            "The workspace can't contain the Anima data folder".to_string()
        }
        WorkspaceProblem::InsideDataRoot => {
            "The workspace can't be inside the Anima data folder".to_string()
        }
    })
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
    Exit,
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
    control_token: ControlToken,
    admin_token: AdminToken,
    api: ServerApi,
    api_tx: watch::Sender<ServerApi>,
    status: SharedStatus,
    log: ServerLog,
    child: Option<ServerChild>,
    /// Crashes in a row, for the backoff.
    crashes: u32,
    running_since: Option<Instant>,
}

impl Lifecycle {
    async fn run(
        mut self,
        mut requests: mpsc::Receiver<ActionRequest>,
        mut signals: mpsc::Receiver<()>,
    ) {
        let mut phase = self.start();
        while !matches!(phase, Phase::Exit) {
            let mut exit = self.child.as_ref().map(ServerChild::exit_watch);
            let mut work = self.work(phase);
            phase = loop {
                tokio::select! {
                    Some(request) = requests.recv() => {
                        if let Some(next) = self.handle(request) {
                            break next;
                        }
                    }
                    status = child::exited(&mut exit) => break self.on_exit(phase, status),
                    event = &mut work => break self.on_event(phase, event),
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
            Phase::Running | Phase::Stopped | Phase::Exit => Box::pin(std::future::pending()),
        }
    }

    /// Takes a requested action, answering with the status once it is under way.
    fn handle(&mut self, request: ActionRequest) -> Option<Phase> {
        let ActionRequest {
            action,
            force,
            reply,
        } = request;
        let (state, pending) = {
            let status = lock(&self.status);
            (status.state.clone(), status.pending)
        };
        let transition = match next(&state, pending, action, force) {
            Ok(transition) => transition,
            Err(conflict) => {
                let _ = reply.send(Err(conflict));
                return None;
            }
        };
        let next_phase = match transition {
            Transition::Stay | Transition::CheckUpdate => None,
            // The updater (a later task) is what makes an update available.
            Transition::Drain(Action::Update)
            | Transition::Shutdown(Action::Update)
            | Transition::Settle(Action::Update)
                if lock(&self.status).update.available.is_none() =>
            {
                lock(&self.status).update.last_error = Some("No update is available".to_string());
                None
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
            Phase::Clearing { .. } | Phase::Stopped | Phase::Crashed { .. } | Phase::Exit => phase,
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

    /// Spawns the server once its port is free.
    fn spawn_child(&mut self) -> Phase {
        let process_env: BTreeMap<String, String> = std::env::vars().collect();
        let port = effective_port(&self.cfg, &process_env);
        let mut env = server_env(&self.cfg, &self.paths, &process_env);
        // The supervisor's own calls depend on the server holding this admin
        // token, so it wins over an inherited one.
        env.insert(
            "ANIMA_LOCAL_ADMIN_TOKEN".to_string(),
            self.admin_token.as_str().to_string(),
        );
        env.insert("ANIMA_SUPERVISOR_URL".to_string(), self.control_url.clone());
        env.insert(
            "ANIMA_SUPERVISOR_TOKEN".to_string(),
            self.control_token.as_str().to_string(),
        );
        self.log.clear_tail();
        self.log
            .note(&format!("Starting the server on port {port}"));
        match ServerChild::spawn(&self.exe, &env, &self.log) {
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

    /// The updater (a later task) takes over here; until it exists the
    /// server comes back as it was.
    fn hand_off_update(&mut self) -> Phase {
        lock(&self.status).update.last_error =
            Some("Updating is not available in this build".to_string());
        self.start()
    }

    fn exit(&mut self) -> Phase {
        self.log.note("Quitting");
        self.set_state(ServerState::Stopped, None);
        Phase::Exit
    }
}
