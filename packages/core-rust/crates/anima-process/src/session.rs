//! Interactive app sessions: a program on a real terminal (ConPTY on Windows,
//! a PTY elsewhere) that a caller types into and reads like a person would.
//!
//! - The screen is a `vt100` model, so a full-screen app reads as its visible
//!   rows; the transcript is the plain text that scrolled past.
//! - Unix: the app runs under [`Custody`] (its PTY child leads a new process
//!   group, inside a cgroup leaf when delegated) and Stop retires the tree.
//!   Terminal queries are answered here, since no terminal sits behind the
//!   PTY. Windows: ConPTY answers queries itself; a ConPTY child cannot join a
//!   Job Object without unsafe code, so Stop kills the tree with
//!   `taskkill /T /F` and a descendant that left the tree is not contained.
mod keys;
mod query;
mod text;

pub use keys::Key;

use crate::{Containment, Custody};
use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;
use std::{
    io::{Read, Write},
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, Instant},
};
use tokio::sync::watch;

/// Text an [`AppSession::wait`] returns on.
pub enum Until {
    Text(String),
    Pattern(regex::Regex),
}

impl Until {
    fn found(&self, haystack: &str) -> bool {
        match self {
            Self::Text(text) => haystack.contains(text.as_str()),
            Self::Pattern(pattern) => pattern.is_match(haystack),
        }
    }
}

pub struct AppSessionConfig {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    /// The whole environment; nothing is inherited from the host.
    pub env: Vec<(String, String)>,
    pub cols: u16,
    pub rows: u16,
    pub scrollback_lines: usize,
    pub transcript_bytes: usize,
    pub transcript_lines: usize,
}

impl AppSessionConfig {
    pub fn new(program: impl Into<PathBuf>, args: Vec<String>, cwd: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args,
            cwd: cwd.into(),
            env: Vec::new(),
            cols: 120,
            rows: 40,
            scrollback_lines: 2_000,
            transcript_bytes: 1024 * 1024,
            transcript_lines: 2_000,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AppExit {
    pub code: u32,
    /// The signal that ended the app, when one did.
    pub signal: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Screen {
    /// Visible rows, trailing blanks trimmed; trailing empty rows dropped.
    pub rows: Vec<String>,
    /// Zero-based (row, column).
    pub cursor: (u16, u16),
    pub size: (u16, u16),
    /// A full-screen app is showing its own screen.
    pub alternate: bool,
}

impl Screen {
    pub fn text(&self) -> String {
        self.rows.join("\n")
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Output {
    pub text: String,
    /// Pass back to [`AppSession::output_since`] for what follows.
    pub next: u64,
    /// Some of the requested output was dropped to stay within limits.
    pub truncated: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WaitOutcome {
    Matched,
    /// Output arrived and then went quiet.
    Settled,
    Exited,
    TimedOut,
}

pub struct WaitFor {
    pub until: Option<Until>,
    /// Return once output has arrived and then stayed quiet this long.
    pub settle: Option<Duration>,
    pub max: Duration,
}

/// Bound on transcript text searched for `until`, besides the screen.
const UNTIL_WINDOW: usize = 64 * 1024;
/// Bound on waiting for a killed Windows app to report its exit.
#[cfg(windows)]
const WINDOWS_EXIT_LIMIT: Duration = Duration::from_secs(10);

struct State {
    parser: vt100::Parser,
    transcript: text::Transcript,
    stripper: text::EscapeStripper,
    exit: Option<AppExit>,
    output_closed: bool,
    last_output: Instant,
    last_input: Instant,
}

struct Shared {
    state: Mutex<State>,
    changes: watch::Sender<u64>,
}

impl Shared {
    fn state(&self) -> MutexGuard<'_, State> {
        // A panicking reader thread must not take the session down with it.
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn changed(&self) {
        self.changes
            .send_modify(|count| *count = count.wrapping_add(1));
    }
}

pub struct AppSession {
    shared: Arc<Shared>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    custody: Custody,
    pid: Option<u32>,
}

impl AppSession {
    pub fn start(config: AppSessionConfig) -> std::io::Result<Self> {
        let size = PtySize {
            rows: config.rows.max(2),
            cols: config.cols.max(10),
            pixel_width: 0,
            pixel_height: 0,
        };
        let pair = portable_pty::native_pty_system()
            .openpty(size)
            .map_err(other)?;
        let (mut custody, wrapper) = Custody::prepare(&config.program, config.args.clone());
        let wrapper = wrapper.as_std();
        let mut command = CommandBuilder::new(wrapper.get_program());
        command.args(wrapper.get_args());
        command.env_clear();
        for (name, value) in &config.env {
            command.env(name, value);
        }
        if command.get_env("TERM").is_none() {
            command.env("TERM", "xterm-256color");
        }
        command.cwd(&config.cwd);
        let mut child = pair.slave.spawn_command(command).map_err(other)?;
        // The app holds the only slave handle, so reads end when it exits.
        drop(pair.slave);
        let pid = child.process_id();
        if let Some(pid) = pid {
            custody.attach_leader(pid);
        }
        let killer = child.clone_killer();
        let reader = pair.master.try_clone_reader().map_err(other)?;
        let writer = Arc::new(Mutex::new(pair.master.take_writer().map_err(other)?));

        let now = Instant::now();
        let (changes, _) = watch::channel(0);
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                parser: vt100::Parser::new(size.rows, size.cols, config.scrollback_lines),
                transcript: text::Transcript::new(config.transcript_bytes, config.transcript_lines),
                stripper: text::EscapeStripper::default(),
                exit: None,
                output_closed: false,
                last_output: now,
                last_input: now,
            }),
            changes,
        });

        let read_shared = Arc::clone(&shared);
        let reply_writer = Arc::clone(&writer);
        std::thread::Builder::new()
            .name("app-session-read".into())
            .spawn(move || read_output(reader, &read_shared, &reply_writer))?;
        let wait_shared = Arc::clone(&shared);
        std::thread::Builder::new()
            .name("app-session-wait".into())
            .spawn(move || {
                let status = child.wait();
                let exit = match status {
                    Ok(status) => AppExit {
                        code: status.exit_code(),
                        signal: status.signal().map(str::to_string),
                    },
                    Err(_) => AppExit {
                        code: u32::MAX,
                        signal: None,
                    },
                };
                wait_shared.state().exit = Some(exit);
                wait_shared.changed();
            })?;

        Ok(Self {
            shared,
            writer,
            master: Mutex::new(pair.master),
            killer: Mutex::new(killer),
            custody,
            pid,
        })
    }

    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    pub fn containment(&self) -> Containment {
        if cfg!(windows) {
            Containment::ProcessTree
        } else {
            self.custody.containment()
        }
    }

    /// Types `text`, then presses `keys`.
    pub fn send(&self, text: Option<&str>, keys: &[Key]) -> std::io::Result<()> {
        let (application_cursor, bracketed_paste) = {
            let state = self.shared.state();
            let screen = state.parser.screen();
            (screen.application_cursor(), screen.bracketed_paste())
        };
        let mut bytes = Vec::new();
        if let Some(text) = text {
            bytes.extend(keys::text_bytes(text, bracketed_paste));
        }
        for key in keys {
            bytes.extend_from_slice(key.bytes(application_cursor));
        }
        if bytes.is_empty() {
            return Ok(());
        }
        let mut writer = self.writer.lock().unwrap_or_else(|p| p.into_inner());
        writer.write_all(&bytes)?;
        writer.flush()?;
        drop(writer);
        self.shared.state().last_input = Instant::now();
        Ok(())
    }

    pub fn screen(&self) -> Screen {
        screen_of(&self.shared.state().parser)
    }

    pub fn output_since(&self, cursor: u64, max_bytes: usize) -> Output {
        let slice = self.shared.state().transcript.since(cursor, max_bytes);
        Output {
            text: slice.text,
            next: slice.next,
            truncated: slice.truncated,
        }
    }

    /// The transcript position new output will start at.
    pub fn output_end(&self) -> u64 {
        self.shared.state().transcript.end()
    }

    pub fn exit(&self) -> Option<AppExit> {
        self.shared.state().exit.clone()
    }

    /// Time since the app last printed or was typed into.
    pub fn idle_for(&self) -> Duration {
        let state = self.shared.state();
        state.last_output.max(state.last_input).elapsed()
    }

    pub fn resize(&self, cols: u16, rows: u16) -> std::io::Result<()> {
        let (rows, cols) = (rows.max(2), cols.max(10));
        self.master
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(other)?;
        self.shared.state().parser.screen_mut().set_size(rows, cols);
        Ok(())
    }

    /// Waits for `until` (searched in the screen and in output after `from`),
    /// for output to settle, for the app to exit, or for `max`.
    pub async fn wait(&self, from: u64, wait: &WaitFor) -> WaitOutcome {
        let mut changes = self.shared.changes.subscribe();
        let deadline = tokio::time::Instant::now() + wait.max;
        loop {
            let settle_at = {
                let state = self.shared.state();
                if let Some(until) = &wait.until {
                    let screen = screen_of(&state.parser).text();
                    let recent = state.transcript.since(from, UNTIL_WINDOW).text;
                    if until.found(&screen) || until.found(&recent) {
                        return WaitOutcome::Matched;
                    }
                }
                if state.exit.is_some() && state.output_closed {
                    return WaitOutcome::Exited;
                }
                match wait.settle {
                    Some(settle)
                        if state.transcript.end() > from
                            || state.last_output > state.last_input =>
                    {
                        let quiet = state.last_output.elapsed();
                        if quiet >= settle {
                            return WaitOutcome::Settled;
                        }
                        Some(tokio::time::Instant::now() + (settle - quiet))
                    }
                    _ => None,
                }
            };
            let wake = settle_at.map_or(deadline, |at| at.min(deadline));
            tokio::select! {
                changed = changes.changed() => {
                    if changed.is_err() {
                        return WaitOutcome::Exited;
                    }
                }
                () = tokio::time::sleep_until(wake) => {
                    if tokio::time::Instant::now() >= deadline {
                        return WaitOutcome::TimedOut;
                    }
                }
            }
        }
    }

    /// Ends the app and every process it started; true once they are proven
    /// gone (Unix) or the app has exited after a tree kill (Windows).
    pub async fn stop(&self) -> bool {
        #[cfg(unix)]
        {
            let mut changes = self.shared.changes.subscribe();
            let shared = Arc::clone(&self.shared);
            let reaped = async move {
                loop {
                    if shared.state().exit.is_some() {
                        return true;
                    }
                    if changes.changed().await.is_err() {
                        return false;
                    }
                }
            };
            if self.pid.is_none() {
                let _ = self.killer.lock().unwrap_or_else(|p| p.into_inner()).kill();
                return false;
            }
            self.custody.retire_group(reaped).await
        }
        #[cfg(windows)]
        {
            if let Some(pid) = self.pid {
                let _ = tokio::process::Command::new("taskkill")
                    .args(["/T", "/F", "/PID", &pid.to_string()])
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .await;
            }
            let _ = self.killer.lock().unwrap_or_else(|p| p.into_inner()).kill();
            let mut changes = self.shared.changes.subscribe();
            let exited = async {
                loop {
                    if self.shared.state().exit.is_some() {
                        return true;
                    }
                    if changes.changed().await.is_err() {
                        return false;
                    }
                }
            };
            tokio::time::timeout(WINDOWS_EXIT_LIMIT, exited)
                .await
                .unwrap_or(false)
        }
    }
}

impl Drop for AppSession {
    /// A session dropped without [`AppSession::stop`] still takes its tree down.
    fn drop(&mut self) {
        if self.shared.state().exit.is_none() {
            self.custody.kill_now();
            let _ = self.killer.lock().unwrap_or_else(|p| p.into_inner()).kill();
        }
    }
}

fn read_output(
    mut reader: Box<dyn Read + Send>,
    shared: &Shared,
    writer: &Mutex<Box<dyn Write + Send>>,
) {
    let mut scanner = query::QueryScanner::default();
    let mut buffer = [0u8; 8192];
    loop {
        let read = match reader.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        let pieces = if cfg!(windows) {
            vec![query::Piece::Output(buffer[..read].to_vec())]
        } else {
            scanner.scan(&buffer[..read])
        };
        let mut replies = Vec::new();
        {
            let mut state = shared.state();
            for piece in pieces {
                match piece {
                    query::Piece::Output(bytes) => apply_output(&mut state, &bytes),
                    query::Piece::Query(query) => {
                        let screen = state.parser.screen();
                        replies.extend(query.reply(screen.cursor_position(), screen.size()));
                    }
                }
            }
            state.last_output = Instant::now();
        }
        if !replies.is_empty() {
            let mut writer = writer.lock().unwrap_or_else(|p| p.into_inner());
            let _ = writer.write_all(&replies).and_then(|()| writer.flush());
        }
        shared.changed();
    }
    let mut state = shared.state();
    let tail = scanner.flush();
    apply_output(&mut state, &tail);
    state.output_closed = true;
    drop(state);
    shared.changed();
}

fn apply_output(state: &mut State, bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    state.parser.process(bytes);
    let text = state.stripper.strip(bytes);
    state.transcript.push(&text);
}

fn screen_of(parser: &vt100::Parser) -> Screen {
    let screen = parser.screen();
    let (rows, cols) = screen.size();
    let mut lines: Vec<String> = screen
        .rows(0, cols)
        .map(|row| row.trim_end().to_string())
        .collect();
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    Screen {
        rows: lines,
        cursor: screen.cursor_position(),
        size: (rows, cols),
        alternate: screen.alternate_screen(),
    }
}

fn other(error: impl std::fmt::Display) -> std::io::Error {
    std::io::Error::other(error.to_string())
}

#[cfg(test)]
mod tests;
