//! `anima start`, `stop`, `restart`, `quit` and `update`.

use std::io::{self, IsTerminal, Write};
use std::path::Path;
use std::process::{Child, Command, ExitCode, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::client::{ActionOutcome, SupervisorClient};
use crate::config::Config;
use crate::paths::AnimaPaths;
use crate::supervisor::{Action, ServerState, SupervisorStatus};

use super::status::not_running;

/// How long `start` waits for the server to run.
const START_TIMEOUT: Duration = Duration::from_secs(20);
const START_POLL: Duration = Duration::from_millis(200);
/// How often the progress of a waiting action is refreshed.
const PROGRESS_INTERVAL: Duration = Duration::from_secs(1);

/// What `ensure_running` found.
pub enum Started {
    /// Anima was running already.
    Already(SupervisorStatus),
    /// Anima was started now.
    Now(SupervisorStatus),
}

impl Started {
    pub fn status(&self) -> &SupervisorStatus {
        match self {
            Started::Already(status) | Started::Now(status) => status,
        }
    }
}

pub fn start() -> io::Result<ExitCode> {
    let (paths, cfg) = super::load()?;
    match ensure_running(&paths, &cfg)? {
        Started::Already(status) => println!("Anima is already running · {}", status.server_url),
        Started::Now(status) => println!("Anima is running · {}", status.server_url),
    }
    Ok(ExitCode::SUCCESS)
}

/// Makes sure the server runs: starts the supervisor if there is none, or
/// tells a stopped one to start, then waits (up to 20 seconds) for `running`.
pub fn ensure_running(paths: &AnimaPaths, cfg: &Config) -> io::Result<Started> {
    let Some(client) = SupervisorClient::connect(paths, cfg)? else {
        let child = launch_supervisor(paths)?;
        return wait_until_running(paths, cfg, child).map(Started::Now);
    };
    let status = client.status()?;
    match status.state {
        ServerState::Running => return Ok(Started::Already(status)),
        ServerState::Starting => {}
        _ => match client.action(Action::Start, false)? {
            ActionOutcome::Accepted(_) => {}
            ActionOutcome::Conflict { message, pending } => {
                return Err(io::Error::other(conflict_text(&message, pending)))
            }
        },
    }
    wait_until_running(paths, cfg, None).map(Started::Now)
}

fn wait_until_running(
    paths: &AnimaPaths,
    cfg: &Config,
    mut child: Option<Child>,
) -> io::Result<SupervisorStatus> {
    let deadline = Instant::now() + START_TIMEOUT;
    let mut last_error = None;
    loop {
        // A supervisor still binding its port or writing its token is not
        // reachable yet: keep trying until the deadline.
        match SupervisorClient::connect(paths, cfg)
            .and_then(|client| client.map(|client| client.status()).transpose())
        {
            Ok(Some(status)) => match &status.state {
                ServerState::Running => return Ok(status),
                ServerState::Crashed { message, .. } => {
                    return Err(io::Error::other(message.clone()))
                }
                _ => {}
            },
            Ok(None) => {}
            Err(error) => last_error = Some(error),
        }
        if let Some(exited) = child
            .as_mut()
            .and_then(|child| child.try_wait().ok().flatten())
        {
            // A second supervisor that finds the first one exits cleanly.
            if !exited.success() {
                return Err(io::Error::other(format!(
                    "the supervisor exited ({exited}); see `anima logs`"
                )));
            }
            child = None;
        }
        if Instant::now() >= deadline {
            let detail = last_error.map(|e| format!(": {e}")).unwrap_or_default();
            return Err(io::Error::other(format!(
                "Anima did not start within {} seconds{detail}; see `anima logs`",
                START_TIMEOUT.as_secs()
            )));
        }
        thread::sleep(START_POLL);
    }
}

/// Starts the supervisor: through the LaunchAgent when one is installed
/// (macOS), else as a detached process of this executable.
fn launch_supervisor(paths: &AnimaPaths) -> io::Result<Option<Child>> {
    if let Some(home) = paths.root.parent() {
        if kickstart_launch_agent(paths, home)? {
            return Ok(None);
        }
    }
    let mut command = Command::new(std::env::current_exe()?);
    command
        .args(["supervise", "--background"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    detach(&mut command);
    command.spawn().map(Some)
}

/// Keeps the supervisor running after this command (and its terminal) ends.
fn detach(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW);
    }
    #[cfg(not(any(unix, windows)))]
    let _ = command;
}

/// Starts the installed LaunchAgent. `false` when there is none, or when
/// launchd would not start it (then the supervisor is spawned directly).
#[cfg(target_os = "macos")]
fn kickstart_launch_agent(paths: &AnimaPaths, home: &Path) -> io::Result<bool> {
    use crate::autostart::macos::{user_id, LABEL};
    if !crate::autostart::platform(paths).is_enabled()? {
        return Ok(false);
    }
    let uid = user_id(home)?;
    let started = Command::new("launchctl")
        .args(["kickstart", &format!("gui/{uid}/{LABEL}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    Ok(started)
}

#[cfg(not(target_os = "macos"))]
fn kickstart_launch_agent(_paths: &AnimaPaths, _home: &Path) -> io::Result<bool> {
    Ok(false)
}

/// `stop`, `restart`, `quit` and `update`: sends the action, then waits for
/// it to finish, showing how many tasks it waits for.
pub fn run_action(action: Action, now: bool) -> io::Result<ExitCode> {
    let (paths, cfg) = super::load()?;
    let Some(client) = SupervisorClient::connect(&paths, &cfg)? else {
        return Ok(not_running());
    };
    let status = match client.action(action, now)? {
        ActionOutcome::Accepted(status) => status,
        ActionOutcome::Conflict { message, pending } => {
            eprintln!("{}", conflict_text(&message, pending));
            return Ok(ExitCode::FAILURE);
        }
    };
    follow(&paths, &cfg, &client, action, status)
}

fn conflict_text(message: &str, pending: Option<Action>) -> String {
    match pending.and_then(action_name) {
        Some(name) => format!("{message}: {name}"),
        None => message.to_string(),
    }
}

fn action_name(action: Action) -> Option<String> {
    serde_json::to_value(action)
        .ok()?
        .as_str()
        .map(str::to_string)
}

fn follow(
    paths: &AnimaPaths,
    cfg: &Config,
    client: &SupervisorClient,
    action: Action,
    mut status: SupervisorStatus,
) -> io::Result<ExitCode> {
    let mut progress = Progress::new();
    loop {
        match judge(action, &status) {
            Verdict::Done(message) => {
                progress.clear();
                println!("{message}");
                return Ok(ExitCode::SUCCESS);
            }
            Verdict::Failed(message) => {
                progress.clear();
                eprintln!("{message}");
                return Ok(ExitCode::FAILURE);
            }
            Verdict::Waiting => {
                if let Some(line) = progress_line(&status.state) {
                    progress.show(&line);
                }
            }
        }
        thread::sleep(PROGRESS_INTERVAL);
        status = match client.status() {
            Ok(status) => status,
            Err(error) => {
                // Quitting (and an update, which restarts Anima) ends the
                // supervisor, and with it the control API.
                let gone = matches!(SupervisorClient::connect(paths, cfg), Ok(None));
                if gone && matches!(action, Action::Quit | Action::Update) {
                    progress.clear();
                    println!("{}", finished_text(action));
                    return Ok(ExitCode::SUCCESS);
                }
                progress.clear();
                return Err(error);
            }
        };
    }
}

/// How an action stands, judged from the status after it.
#[derive(Debug, PartialEq, Eq)]
enum Verdict {
    Waiting,
    Done(String),
    Failed(String),
}

fn judge(action: Action, status: &SupervisorStatus) -> Verdict {
    if let ServerState::Crashed { message, .. } = &status.state {
        if action != Action::Stop {
            return Verdict::Failed(message.clone());
        }
    }
    if status.pending.is_some() {
        return Verdict::Waiting;
    }
    match (action, &status.state) {
        (Action::Stop, ServerState::Stopped) => Verdict::Done(finished_text(action)),
        (Action::Restart | Action::Start, ServerState::Running) => {
            Verdict::Done(format!("Anima is running · {}", status.server_url))
        }
        // The server is running again with nothing pending: the stop was
        // cancelled (from the console, say) or the update had nothing to do.
        (Action::Update, ServerState::Running | ServerState::Stopped) => Verdict::Failed(
            status
                .update
                .last_error
                .clone()
                .unwrap_or_else(|| "No update was installed".to_string()),
        ),
        (Action::Stop | Action::Quit, ServerState::Running) => {
            Verdict::Failed("It was cancelled before the server stopped".to_string())
        }
        _ => Verdict::Waiting,
    }
}

fn finished_text(action: Action) -> String {
    match action {
        Action::Stop => "Anima is stopped",
        Action::Quit => "Anima has quit",
        Action::Update => "Anima is restarting to finish the update",
        _ => "Done",
    }
    .to_string()
}

/// The count of tasks a drain waits for, as the console words it.
fn progress_line(state: &ServerState) -> Option<String> {
    match state {
        ServerState::Draining { in_flight, .. } => Some(format!(
            "Waiting for {} tasks to finish…",
            in_flight.total()
        )),
        _ => None,
    }
}

/// A line refreshed in place on a terminal, and one new line per refresh
/// anywhere else.
struct Progress {
    terminal: bool,
    shown: bool,
}

impl Progress {
    fn new() -> Self {
        Self {
            terminal: io::stdout().is_terminal(),
            shown: false,
        }
    }

    fn show(&mut self, line: &str) {
        let mut stdout = io::stdout().lock();
        if self.terminal {
            let _ = write!(stdout, "\r\x1b[K{line}");
            self.shown = true;
        } else {
            let _ = writeln!(stdout, "{line}");
        }
        let _ = stdout.flush();
    }

    /// Wipes the line, so the next output starts clean.
    fn clear(&mut self) {
        if self.shown {
            let mut stdout = io::stdout().lock();
            let _ = write!(stdout, "\r\x1b[K");
            let _ = stdout.flush();
            self.shown = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use anima_daemon::system::InFlight;

    use super::*;
    use crate::commands::status::sample_status;

    fn with(state: ServerState, pending: Option<Action>) -> SupervisorStatus {
        let mut status = sample_status(state);
        status.pending = pending;
        status
    }

    #[test]
    fn a_drain_shows_the_task_count() {
        let draining = ServerState::Draining {
            action: Action::Stop,
            in_flight: InFlight {
                chat_runs: 2,
                swarm_runs: 1,
                ..InFlight::default()
            },
        };
        assert_eq!(
            progress_line(&draining).as_deref(),
            Some("Waiting for 3 tasks to finish…")
        );
        assert_eq!(progress_line(&ServerState::Stopping), None);
    }

    #[test]
    fn actions_finish_in_their_own_state() {
        let stopping = with(ServerState::Stopping, Some(Action::Stop));
        assert_eq!(judge(Action::Stop, &stopping), Verdict::Waiting);
        let stopped = with(ServerState::Stopped, None);
        assert_eq!(
            judge(Action::Stop, &stopped),
            Verdict::Done("Anima is stopped".into())
        );
        // A restart passes through stopped without being done.
        assert_eq!(judge(Action::Restart, &stopped), Verdict::Waiting);
        let running = with(ServerState::Running, None);
        assert!(matches!(
            judge(Action::Restart, &running),
            Verdict::Done(text) if text.starts_with("Anima is running")
        ));
    }

    #[test]
    fn an_ended_drain_or_a_crash_fails_the_wait() {
        let running = with(ServerState::Running, None);
        assert!(matches!(judge(Action::Stop, &running), Verdict::Failed(_)));
        assert!(matches!(judge(Action::Quit, &running), Verdict::Failed(_)));
        // Being cancelled is not done yet.
        let cancelling = with(ServerState::Running, Some(Action::Cancel));
        assert_eq!(judge(Action::Stop, &cancelling), Verdict::Waiting);
        let crashed = with(
            ServerState::Crashed {
                message: "Port 8787 is in use".into(),
                retry_in_secs: 4,
            },
            None,
        );
        assert_eq!(
            judge(Action::Restart, &crashed),
            Verdict::Failed("Port 8787 is in use".into())
        );
        assert!(matches!(judge(Action::Stop, &crashed), Verdict::Waiting));
    }

    #[test]
    fn an_update_without_a_release_says_why() {
        let mut running = with(ServerState::Running, None);
        running.update.last_error = Some("No update is available".into());
        assert_eq!(
            judge(Action::Update, &running),
            Verdict::Failed("No update is available".into())
        );
    }

    #[test]
    fn a_conflict_names_the_pending_action() {
        assert_eq!(
            conflict_text("Another action is in progress", Some(Action::Stop)),
            "Another action is in progress: stop"
        );
    }
}
