use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::process::Stdio;
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anima_process::{Containment, Custody};
use process_wrap::tokio::{ChildWrapper, CommandWrap};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::sync::{oneshot, watch};

use super::super::workspace::{
    canonical_workspace_root, resolve_workspace_search_root, workspace_root_path,
};
use super::process_runtime;
use super::shell::{resolve_shell_launcher, shell_env, truncate_shell_output};

const MAX_PROCESS_OUTPUT_LINES: usize = 2_000;
pub(crate) const DEFAULT_MAX_BACKGROUND_PROCESSES: usize = 8;
/// How long a finished leader's output readers may keep draining before its
/// exit is recorded; a descendant holding the pipes must not hide the exit.
const READER_DRAIN: Duration = Duration::from_millis(500);
/// Longer than `Custody::retire`'s own proof bound.
const RETIRE_WAIT: Duration = Duration::from_secs(65);
/// An exit the agent sees within this window (bg_wait, bg_output, bg_list)
/// does not also wake it.
const EXIT_NOTE_GRACE: Duration = Duration::from_millis(750);
const EXIT_NOTE_COMMAND_CHARS: usize = 80;

pub(crate) type SharedProcessManager = Arc<Mutex<ProcessManager>>;
/// Receives the exit note of a job that ended on its own.
/// Receives notes for the agent: a job that ended on its own, or a line that
/// matched one of its `notify_on` patterns.
pub(in super::super) type ExitNotifier = Arc<dyn Fn(String) + Send + Sync + 'static>;
/// Patterns one job may watch for; each wakes the agent at most once.
pub(in super::super) const MAX_NOTIFY_PATTERNS: usize = 4;
const MATCH_NOTE_LINE_CHARS: usize = 200;
type StopReply = SyncSender<bool>;

#[derive(Default)]
pub(crate) struct ProcessManager {
    processes: HashMap<String, ManagedProcess>,
    next_id: u64,
    max_running_processes: usize,
}

struct ManagedProcess {
    id: String,
    command: String,
    containment: Containment,
    shared: Arc<ProcessShared>,
    /// Asks the supervisor to retire the tree; dropping it does the same.
    stop: Option<oneshot::Sender<StopReply>>,
    started_at: Instant,
}

struct ProcessShared {
    output: Mutex<ManagedProcessOutput>,
    /// Patterns that wake the agent when a new line matches.
    watch: Mutex<Option<Watch>>,
    /// Bumped on every new line and on exit, for `bg_wait`.
    changed: watch::Sender<u64>,
}

#[derive(Default)]
struct ManagedProcessOutput {
    lines: VecDeque<String>,
    /// Absolute index of `lines[0]`; older lines were dropped by the cap.
    first_index: u64,
    /// Absolute index of the first line the agent has not read.
    read_cursor: u64,
    exit_code: Option<i32>,
    stopped: bool,
    /// The agent has been shown the exit, so no exit note is needed.
    exit_reported: bool,
}

impl ManagedProcessOutput {
    fn end_index(&self) -> u64 {
        self.first_index + self.lines.len() as u64
    }

    fn push(&mut self, line: String) {
        if self.lines.len() >= MAX_PROCESS_OUTPUT_LINES {
            self.lines.pop_front();
            self.first_index += 1;
        }
        self.lines.push_back(line);
    }

    fn lines_from(&self, from: u64) -> impl Iterator<Item = &String> {
        let skip = from.saturating_sub(self.first_index) as usize;
        self.lines.iter().skip(skip)
    }

    /// Unread lines (or all retained lines), advancing the read cursor.
    fn take(&mut self, all: bool) -> Vec<String> {
        let from = if all {
            self.first_index
        } else {
            self.read_cursor
        };
        let lines = self.lines_from(from).cloned().collect();
        self.read_cursor = self.end_index();
        if self.exit_code.is_some() {
            self.exit_reported = true;
        }
        lines
    }
}

impl ProcessShared {
    fn new() -> Self {
        Self {
            output: Mutex::new(ManagedProcessOutput::default()),
            watch: Mutex::new(None),
            changed: watch::channel(0).0,
        }
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, ManagedProcessOutput>, String> {
        self.output
            .lock()
            .map_err(|_| "background process output lock poisoned".to_string())
    }

    fn bump(&self) {
        self.changed
            .send_modify(|version| *version = version.wrapping_add(1));
    }

    fn finish(&self, exit_code: i32, stopped: bool) {
        if let Ok(mut output) = self.output.lock() {
            if output.exit_code.is_none() {
                output.exit_code = Some(exit_code);
            }
            output.stopped |= stopped;
        }
        self.bump();
    }

    fn exit_code(&self) -> Result<Option<i32>, String> {
        Ok(self.lock()?.exit_code)
    }
}

#[cfg(test)]
pub(crate) fn new_shared_process_manager() -> SharedProcessManager {
    new_shared_process_manager_with_limit(DEFAULT_MAX_BACKGROUND_PROCESSES)
}

pub(crate) fn new_shared_process_manager_with_limit(
    max_running_processes: usize,
) -> SharedProcessManager {
    Arc::new(Mutex::new(ProcessManager::new(max_running_processes)))
}

#[cfg(test)]
pub(crate) fn set_background_process_limit(
    process_manager: &SharedProcessManager,
    max_running_processes: usize,
) -> Result<(), String> {
    let mut manager = process_manager
        .lock()
        .map_err(|_| "background process manager lock poisoned".to_string())?;
    manager.max_running_processes = max_running_processes;
    Ok(())
}

pub(crate) fn background_process_count(
    process_manager: &SharedProcessManager,
) -> Result<usize, String> {
    let manager = process_manager
        .lock()
        .map_err(|_| "background process manager lock poisoned".to_string())?;
    running_process_count(&manager)
}

impl ProcessManager {
    fn new(max_running_processes: usize) -> Self {
        Self {
            processes: HashMap::new(),
            next_id: 1,
            max_running_processes,
        }
    }
}

impl Drop for ProcessManager {
    /// Daemon shutdown: retire every tree, asking all supervisors first so the
    /// trees are retired concurrently.
    fn drop(&mut self) {
        let replies = self
            .processes
            .values_mut()
            .filter_map(request_retire)
            .collect::<Vec<_>>();
        let deadline = Instant::now() + RETIRE_WAIT;
        for reply in replies {
            let _ = reply.recv_timeout(deadline.saturating_duration_since(Instant::now()));
        }
    }
}

fn request_retire(managed: &mut ManagedProcess) -> Option<std::sync::mpsc::Receiver<bool>> {
    let stop = managed.stop.take()?;
    let (sender, receiver) = sync_channel(1);
    stop.send(sender).ok()?;
    Some(receiver)
}

pub(super) fn start_background_process(
    process_manager: &SharedProcessManager,
    configured_root: Option<&Path>,
    command: &str,
    cwd: &str,
    exit_notifier: Option<ExitNotifier>,
    watch: Option<(ExitNotifier, Vec<(String, WaitUntil)>)>,
    profile_owner: Option<crate::profile_owner::ProfileOwner>,
) -> Result<String, String> {
    let workspace_root = workspace_root_path("bg_start", configured_root)?;
    start_background_process_watching(
        process_manager,
        &workspace_root,
        command,
        cwd,
        exit_notifier,
        watch,
        profile_owner,
    )
}

#[cfg(test)]
pub(crate) fn start_background_process_from_root(
    process_manager: &SharedProcessManager,
    workspace_root: &Path,
    command: &str,
    cwd: &str,
) -> Result<String, String> {
    start_background_process_with_notifier(process_manager, workspace_root, command, cwd, None)
}

#[cfg(test)]
fn start_background_process_with_notifier(
    process_manager: &SharedProcessManager,
    workspace_root: &Path,
    command: &str,
    cwd: &str,
    notifier: Option<ExitNotifier>,
) -> Result<String, String> {
    start_background_process_watching(
        process_manager,
        workspace_root,
        command,
        cwd,
        notifier,
        None,
        None,
    )
}

fn start_background_process_watching(
    process_manager: &SharedProcessManager,
    workspace_root: &Path,
    command: &str,
    cwd: &str,
    notifier: Option<ExitNotifier>,
    watch: Option<(ExitNotifier, Vec<(String, WaitUntil)>)>,
    profile_owner: Option<crate::profile_owner::ProfileOwner>,
) -> Result<String, String> {
    if let Some(owner) = &profile_owner {
        owner.ensure_owned().map_err(|error| error.to_string())?;
    }
    let cwd_path = resolve_workspace_search_root(
        &canonical_workspace_root(workspace_root, "bg_start")?,
        cwd,
        "bg_start",
    )?;

    let mut manager = process_manager
        .lock()
        .map_err(|_| "background process manager lock poisoned".to_string())?;
    let running_processes = running_process_count(&manager)?;
    if running_processes >= manager.max_running_processes {
        return Err(format!(
            "bg_start limit reached: {} running background processes (max {}).",
            running_processes, manager.max_running_processes
        ));
    }

    let (executable, mut args) = resolve_shell_launcher()?;
    args.push(command.to_string());
    let (mut custody, mut native) = Custody::prepare(Path::new(&executable), args);
    native
        .current_dir(&cwd_path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_clear()
        .envs(shell_env());
    let mut wrapped = CommandWrap::from(native);
    custody.wrap(&mut wrapped);
    let containment = custody.containment();

    let runtime = process_runtime();
    let mut child = {
        let _entered = runtime.enter();
        wrapped
            .spawn()
            .map_err(|error| format!("bg_start failed to start command: {error}"))?
    };
    custody.attach(child.as_ref());
    let (Some(stdout), Some(stderr)) = (child.stdout().take(), child.stderr().take()) else {
        runtime.spawn(async move {
            let _profile_owner = profile_owner;
            if !custody.retire(&mut child).await {
                futures::future::pending::<()>().await;
            }
        });
        return Err("bg_start output could not be captured".to_string());
    };

    let shared = Arc::new(ProcessShared::new());
    let id = format!("bg-{}", manager.next_id);
    if let Some((watch_notifier, patterns)) = watch.filter(|(_, patterns)| !patterns.is_empty()) {
        if let Ok(mut watch) = shared.watch.lock() {
            *watch = Some(Watch {
                id: id.clone(),
                command: command.to_string(),
                notifier: watch_notifier,
                patterns: patterns
                    .into_iter()
                    .map(|(label, until)| (label, until, false))
                    .collect(),
            });
        }
    }
    let readers = [
        runtime.spawn(read_process_output(stdout, Arc::clone(&shared), false)),
        runtime.spawn(read_process_output(stderr, Arc::clone(&shared), true)),
    ];
    let (stop, stop_requests) = oneshot::channel();

    manager.next_id += 1;
    let started_at = Instant::now();
    let exit_note = notifier.map(|notifier| ExitNote {
        notifier,
        id: id.clone(),
        command: command.to_string(),
        started_at,
    });
    runtime.spawn(supervise(
        custody,
        child,
        Arc::clone(&shared),
        readers,
        stop_requests,
        exit_note,
        profile_owner,
    ));
    manager.processes.insert(
        id.clone(),
        ManagedProcess {
            id: id.clone(),
            command: command.to_string(),
            containment,
            shared,
            stop: Some(stop),
            started_at,
        },
    );

    Ok(format!(
        "Started background process {}: {}\nUse bg_output(id: \"{}\") to read output, bg_wait(id: \"{}\") to wait for it, bg_stop(id: \"{}\") to kill.\nContainment: {}.",
        id,
        command,
        id,
        id,
        id,
        containment_name(containment)
    ))
}

struct Watch {
    id: String,
    command: String,
    notifier: ExitNotifier,
    /// Label as the agent wrote it, the matcher, and whether it fired.
    patterns: Vec<(String, WaitUntil, bool)>,
}

struct ExitNote {
    notifier: ExitNotifier,
    id: String,
    command: String,
    started_at: Instant,
}

/// Owns the tree: records the leader's exit, and retires the whole tree on
/// `bg_stop`, on removal, or when the manager is dropped.
async fn supervise(
    custody: Custody,
    mut child: Box<dyn ChildWrapper>,
    shared: Arc<ProcessShared>,
    mut readers: [tokio::task::JoinHandle<()>; 2],
    mut stop_requests: oneshot::Receiver<StopReply>,
    exit_note: Option<ExitNote>,
    profile_owner: Option<crate::profile_owner::ProfileOwner>,
) {
    let _profile_owner = profile_owner;
    let lost = _profile_owner
        .as_ref()
        .map(|owner| owner.lost_signal())
        .unwrap_or_default();
    enum First {
        Exited(std::io::Result<std::process::ExitStatus>),
        Stop(Option<StopReply>),
    }
    let first = tokio::select! {
        biased;
        _ = lost.cancelled() => First::Stop(None),
        status = child.inner_mut().wait() => First::Exited(status),
        request = &mut stop_requests => First::Stop(request.ok()),
    };
    let stop_reply = match first {
        First::Stop(reply) => {
            let retired = custody.retire(&mut child).await;
            let code = child
                .try_wait()
                .ok()
                .flatten()
                .and_then(|status| status.code())
                .unwrap_or(-1);
            shared.finish(code, true);
            if let Some(reply) = reply {
                let _ = reply.send(retired);
            }
            if !retired {
                futures::future::pending::<()>().await;
            }
            return;
        }
        First::Exited(status) => {
            let code = status.ok().and_then(|status| status.code()).unwrap_or(-1);
            let drained = futures::future::join_all(readers.iter_mut());
            let _ = tokio::time::timeout(READER_DRAIN, drained).await;
            let produced_output = shared.lock().is_ok_and(|output| output.end_index() > 0);
            if custody.join_failed(code, produced_output) {
                if let Ok(mut output) = shared.lock() {
                    output.push(
                        "[anima] The command could not join its process container and did not run."
                            .to_string(),
                    );
                }
            }
            shared.finish(code, false);
            if let Some(note) = exit_note {
                let shared = Arc::clone(&shared);
                tokio::spawn(async move {
                    tokio::time::sleep(EXIT_NOTE_GRACE).await;
                    let unseen = shared
                        .lock()
                        .is_ok_and(|output| !output.exit_reported && !output.stopped);
                    if unseen {
                        (note.notifier)(exit_note_text(
                            &note.id,
                            &note.command,
                            code,
                            note.started_at.elapsed(),
                        ));
                    }
                });
            }
            // Descendants the leader left behind stay in custody until the
            // job is stopped or removed.
            tokio::select! { biased; _ = lost.cancelled() => None, request = stop_requests => request.ok() }
        }
    };
    let retired = custody.retire(&mut child).await;
    if let Some(reply) = stop_reply {
        let _ = reply.send(retired);
    }
    if !retired {
        futures::future::pending::<()>().await;
    }
}

/// Fires each not-yet-fired pattern `line` matches. The note waits a moment
/// and is dropped when the agent has read that line by then.
fn watch_line(shared: &Arc<ProcessShared>, line: &str, index: u64) {
    let Ok(mut watch) = shared.watch.lock() else {
        return;
    };
    let Some(watch) = watch.as_mut() else {
        return;
    };
    let mut matched = Vec::new();
    for (label, until, fired) in &mut watch.patterns {
        if !*fired && until.matches(line) {
            *fired = true;
            matched.push(label.clone());
        }
    }
    if matched.is_empty() {
        return;
    }
    let note = match_note_text(&watch.id, &watch.command, line, &matched);
    let notifier = Arc::clone(&watch.notifier);
    let shared = Arc::clone(shared);
    tokio::spawn(async move {
        tokio::time::sleep(EXIT_NOTE_GRACE).await;
        let unseen = shared
            .lock()
            .is_ok_and(|output| output.read_cursor <= index && !output.stopped);
        if unseen {
            notifier(note);
        }
    });
}

fn shorten(text: &str, limit: usize) -> String {
    if text.chars().count() > limit {
        let mut shortened = text.chars().take(limit - 1).collect::<String>();
        shortened.push('…');
        shortened
    } else {
        text.to_string()
    }
}

pub(in super::super) fn match_note_text(
    id: &str,
    command: &str,
    line: &str,
    patterns: &[String],
) -> String {
    let patterns = patterns
        .iter()
        .map(|pattern| format!("`{pattern}`"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "Background job {id} `{}` printed a line matching {patterns}, and is still running:\n{}\nUse bg_output(id: \"{id}\") to read its output.",
        shorten(command, EXIT_NOTE_COMMAND_CHARS),
        shorten(line, MATCH_NOTE_LINE_CHARS),
    )
}

pub(in super::super) fn exit_note_text(
    id: &str,
    command: &str,
    exit_code: i32,
    elapsed: Duration,
) -> String {
    let command = if command.chars().count() > EXIT_NOTE_COMMAND_CHARS {
        let mut shortened = command
            .chars()
            .take(EXIT_NOTE_COMMAND_CHARS - 1)
            .collect::<String>();
        shortened.push('…');
        shortened
    } else {
        command.to_string()
    };
    format!(
        "Background job {id} `{command}` exited {exit_code} after {}. Use bg_output(id: \"{id}\") to read its output.",
        format_elapsed(elapsed)
    )
}

fn format_elapsed(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs();
    let (hours, minutes, seconds) = (seconds / 3600, (seconds / 60) % 60, seconds % 60);
    if hours > 0 {
        format!("{hours}h{minutes}m{seconds}s")
    } else if minutes > 0 {
        format!("{minutes}m{seconds}s")
    } else {
        format!("{seconds}s")
    }
}

fn containment_name(containment: Containment) -> String {
    match serde_json::to_value(containment) {
        Ok(serde_json::Value::String(name)) => name,
        _ => format!("{containment:?}"),
    }
}

fn unknown_process(id: &str) -> String {
    format!(
        "No background process with id \"{}\". Use bg_list to see active processes.",
        id
    )
}

fn status_label(exit_code: Option<i32>) -> String {
    match exit_code {
        None => "(running)".to_string(),
        Some(code) => format!("(exited: {})", code),
    }
}

pub(in super::super) fn read_background_process_output(
    process_manager: &SharedProcessManager,
    id: &str,
    all: bool,
) -> Result<String, String> {
    let shared = {
        let manager = process_manager
            .lock()
            .map_err(|_| "background process manager lock poisoned".to_string())?;
        let managed = manager
            .processes
            .get(id)
            .ok_or_else(|| unknown_process(id))?;
        Arc::clone(&managed.shared)
    };

    let mut output = shared.lock()?;
    let lines = output.take(all);
    let exit_code = output.exit_code;
    drop(output);

    if lines.is_empty() {
        return Ok(if exit_code.is_none() {
            format!("[{}] No new output. Process still running.", id)
        } else {
            format!(
                "[{}] No new output. Process exited with code {}.",
                id,
                exit_code.unwrap_or(-1)
            )
        });
    }

    Ok(format!(
        "[{}] {}\n{}",
        id,
        status_label(exit_code),
        truncate_shell_output(&lines.join("\n"))
    ))
}

/// How `bg_wait` decides it has seen what it waits for.
#[derive(Debug)]
pub(in super::super) enum WaitUntil {
    Literal(String),
    Pattern(regex::Regex),
}

impl WaitUntil {
    fn matches(&self, line: &str) -> bool {
        match self {
            Self::Literal(text) => line.contains(text.as_str()),
            Self::Pattern(pattern) => pattern.is_match(line),
        }
    }
}

/// Waits without holding any lock: until the process exits, `until` appears
/// in output the agent has not read, or `timeout` passes. Returns that unread
/// output and advances the read cursor like `bg_output`.
pub(in super::super) async fn wait_background_process(
    process_manager: &SharedProcessManager,
    id: &str,
    timeout: Duration,
    until: Option<WaitUntil>,
) -> Result<String, String> {
    let shared = {
        let manager = process_manager
            .lock()
            .map_err(|_| "background process manager lock poisoned".to_string())?;
        let managed = manager
            .processes
            .get(id)
            .ok_or_else(|| unknown_process(id))?;
        Arc::clone(&managed.shared)
    };
    let mut changes = shared.changed.subscribe();
    let deadline = tokio::time::Instant::now() + timeout;
    let mut scanned = shared.lock()?.read_cursor;
    let reason = loop {
        changes.borrow_and_update();
        {
            let output = shared.lock()?;
            if let Some(until) = &until {
                let matched = output.lines_from(scanned).any(|line| until.matches(line));
                scanned = output.end_index();
                if matched {
                    break "matched until".to_string();
                }
            }
            if output.exit_code.is_some() {
                break "process exited".to_string();
            }
        }
        match tokio::time::timeout_at(deadline, changes.changed()).await {
            Ok(Ok(())) => {}
            Ok(Err(_)) => break "process removed".to_string(),
            Err(_) => break format!("timed out after {}ms", timeout.as_millis()),
        }
    };

    let mut output = shared.lock()?;
    let lines = output.take(false);
    let exit_code = output.exit_code;
    drop(output);
    let body = if lines.is_empty() {
        "No new output.".to_string()
    } else {
        truncate_shell_output(&lines.join("\n"))
    };
    Ok(format!(
        "[{}] {} {}\n{}",
        id,
        status_label(exit_code),
        reason,
        body
    ))
}

pub(in super::super) fn stop_background_process(
    process_manager: &SharedProcessManager,
    id: &str,
) -> Result<String, String> {
    let mut managed = process_manager
        .lock()
        .map_err(|_| "background process manager lock poisoned".to_string())?
        .processes
        .remove(id)
        .ok_or_else(|| format!("No background process with id \"{}\".", id))?;

    // The manager lock is released; retiring may take a moment.
    let retired = request_retire(&mut managed)
        .map(|reply| reply.recv_timeout(RETIRE_WAIT).unwrap_or(false))
        .unwrap_or(false);
    if !retired {
        return Err(format!(
            "bg_stop could not confirm that every process of {} is gone.",
            id
        ));
    }
    Ok(format!("Stopped and removed {}.", id))
}

pub(in super::super) fn list_background_processes(
    process_manager: &SharedProcessManager,
) -> Result<String, String> {
    let manager = process_manager
        .lock()
        .map_err(|_| "background process manager lock poisoned".to_string())?;

    if manager.processes.is_empty() {
        return Ok("No background processes running.".to_string());
    }

    let mut lines = Vec::new();
    for managed in manager.processes.values() {
        let exit_code = {
            let mut output = managed.shared.lock()?;
            if output.exit_code.is_some() {
                output.exit_reported = true;
            }
            output.exit_code
        };
        let elapsed = managed.started_at.elapsed().as_secs();
        let status = if let Some(code) = exit_code {
            format!("exited({})", code)
        } else {
            "running".to_string()
        };
        lines.push(format!(
            "{}  {}  {}s  {}  {}",
            managed.id,
            status,
            elapsed,
            containment_name(managed.containment),
            managed.command
        ));
    }
    lines.sort();

    Ok(lines.join("\n"))
}

fn running_process_count(manager: &ProcessManager) -> Result<usize, String> {
    let mut running_processes = 0;
    for managed in manager.processes.values() {
        if managed.shared.exit_code()?.is_none() {
            running_processes += 1;
        }
    }
    Ok(running_processes)
}

async fn read_process_output<R>(reader: R, shared: Arc<ProcessShared>, stderr: bool)
where
    R: AsyncRead + Unpin,
{
    let mut reader = BufReader::new(reader);
    let mut buffer = Vec::new();
    loop {
        buffer.clear();
        match reader.read_until(b'\n', &mut buffer).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        while matches!(buffer.last(), Some(b'\n' | b'\r')) {
            buffer.pop();
        }
        let line = String::from_utf8_lossy(&buffer);
        let rendered = if stderr {
            format!("[stderr] {}", line)
        } else {
            line.into_owned()
        };
        let Ok(mut output) = shared.output.lock() else {
            break;
        };
        output.push(rendered.clone());
        let index = output.end_index() - 1;
        drop(output);
        shared.bump();
        watch_line(&shared, &rendered, index);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace() -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix("anima-bg-")
            .tempdir()
            .expect("temp workspace")
    }

    fn start(manager: &SharedProcessManager, root: &Path, command: &str) -> String {
        start_background_process_from_root(manager, root, command, ".").expect("bg_start")
    }

    #[tokio::test]
    async fn bg_wait_returns_when_until_appears_and_keeps_the_job_running() {
        let root = workspace();
        let manager = new_shared_process_manager();
        start(
            &manager,
            root.path(),
            "echo booting; sleep 0.2; echo READY on 4173; sleep 30",
        );
        let waited = wait_background_process(
            &manager,
            "bg-1",
            Duration::from_secs(10),
            Some(WaitUntil::Literal("READY on".into())),
        )
        .await
        .unwrap();
        assert!(
            waited.starts_with("[bg-1] (running) matched until"),
            "{waited}"
        );
        assert!(waited.contains("booting") && waited.contains("READY on 4173"));

        let waited = wait_background_process(
            &manager,
            "bg-1",
            Duration::from_millis(150),
            Some(WaitUntil::Pattern(regex::Regex::new(r"DONE \d+").unwrap())),
        )
        .await
        .unwrap();
        assert!(
            waited.contains("(running) timed out after 150ms"),
            "{waited}"
        );
        assert!(waited.ends_with("No new output."), "{waited}");
        assert_eq!(
            stop_background_process(&manager, "bg-1").unwrap(),
            "Stopped and removed bg-1."
        );
    }

    #[tokio::test]
    async fn bg_wait_returns_the_exit_code_and_tail_when_the_job_ends() {
        let root = workspace();
        let manager = new_shared_process_manager();
        start(&manager, root.path(), "echo first; echo second >&2; exit 3");
        let waited = wait_background_process(&manager, "bg-1", Duration::from_secs(10), None)
            .await
            .unwrap();
        assert!(
            waited.starts_with("[bg-1] (exited: 3) process exited"),
            "{waited}"
        );
        assert!(waited.contains("first") && waited.contains("[stderr] second"));
        let listed = list_background_processes(&manager).unwrap();
        assert!(listed.contains("bg-1  exited(3)"), "{listed}");
        assert_eq!(background_process_count(&manager).unwrap(), 0);
    }

    #[tokio::test]
    async fn bg_wait_reports_an_unknown_id() {
        let manager = new_shared_process_manager();
        let error = wait_background_process(&manager, "bg-9", Duration::from_millis(10), None)
            .await
            .unwrap_err();
        assert!(error.contains("No background process"), "{error}");
    }

    #[tokio::test]
    async fn bg_start_child_gets_the_shell_allowlist_not_the_daemon_env() {
        std::env::set_var("ANIMA_TEST_SECRET", "do-not-leak-1f2e");
        let root = workspace();
        let manager = new_shared_process_manager();
        start(
            &manager,
            root.path(),
            if cfg!(windows) { "set" } else { "env" },
        );
        let waited = wait_background_process(&manager, "bg-1", Duration::from_secs(10), None)
            .await
            .unwrap();
        assert!(waited.contains("(exited: 0)"), "{waited}");
        assert!(!waited.contains("ANIMA_TEST_SECRET"), "{waited}");
        assert!(!waited.contains("do-not-leak-1f2e"), "{waited}");
        assert!(waited.contains("PAGER=cat"), "{waited}");
        assert!(waited.contains("GIT_PAGER=cat"), "{waited}");
    }

    #[cfg(unix)]
    fn marker_alive(marker: &str) -> bool {
        std::process::Command::new("pgrep")
            .args(["-f", marker])
            .output()
            .is_ok_and(|output| output.status.success())
    }

    #[cfg(unix)]
    fn unique_sleep_marker() -> String {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos();
        format!("sleep 30.{}{}", std::process::id(), nanos % 100_000)
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn bg_stop_retires_descendants_too() {
        let root = workspace();
        let manager = new_shared_process_manager();
        let marker = unique_sleep_marker();
        start(
            &manager,
            root.path(),
            &format!("{marker} & echo started; wait"),
        );
        wait_background_process(
            &manager,
            "bg-1",
            Duration::from_secs(10),
            Some(WaitUntil::Literal("started".into())),
        )
        .await
        .unwrap();
        assert!(
            marker_alive(&marker),
            "the grandchild must be running first"
        );
        stop_background_process(&manager, "bg-1").unwrap();
        assert!(!marker_alive(&marker), "bg_stop left {marker} running");
    }

    #[tokio::test]
    async fn a_job_that_ends_unobserved_posts_one_exit_note() {
        let root = workspace();
        let manager = new_shared_process_manager();
        let (sender, mut notes) = tokio::sync::mpsc::unbounded_channel();
        start_background_process_with_notifier(
            &manager,
            root.path(),
            "echo done",
            ".",
            Some(Arc::new(move |note| {
                let _ = sender.send(note);
            })),
        )
        .unwrap();
        let note = tokio::time::timeout(Duration::from_secs(10), notes.recv())
            .await
            .expect("exit note arrives")
            .expect("one note");
        assert!(
            note.starts_with("Background job bg-1 `echo done` exited 0 after 0s."),
            "{note}"
        );
        assert!(notes.recv().await.is_none(), "only one note");
    }

    #[tokio::test]
    async fn an_exit_the_agent_already_saw_or_stopped_posts_no_note() {
        let root = workspace();
        let manager = new_shared_process_manager();
        let (sender, mut notes) = tokio::sync::mpsc::unbounded_channel::<String>();
        let observed = sender.clone();
        start_background_process_with_notifier(
            &manager,
            root.path(),
            "exit 0",
            ".",
            Some(Arc::new(move |note| {
                let _ = observed.send(note);
            })),
        )
        .unwrap();
        wait_background_process(&manager, "bg-1", Duration::from_secs(10), None)
            .await
            .unwrap();
        start_background_process_with_notifier(
            &manager,
            root.path(),
            "sleep 30",
            ".",
            Some(Arc::new(move |note| {
                let _ = sender.send(note);
            })),
        )
        .unwrap();
        stop_background_process(&manager, "bg-2").unwrap();
        tokio::time::sleep(EXIT_NOTE_GRACE + Duration::from_millis(300)).await;
        assert!(notes.try_recv().is_err(), "no note expected");
    }

    #[tokio::test]
    async fn a_watched_pattern_wakes_the_agent_once_while_the_job_runs() {
        let root = workspace();
        let manager = new_shared_process_manager();
        let (sender, mut notes) = tokio::sync::mpsc::unbounded_channel();
        start_background_process_watching(
            &manager,
            root.path(),
            "echo booting; echo 'listening on 4000'; echo 'listening on 4001'; sleep 30",
            ".",
            None,
            Some((
                Arc::new(move |note| {
                    let _ = sender.send(note);
                }),
                vec![
                    (
                        "listening".to_string(),
                        WaitUntil::Literal("listening".into()),
                    ),
                    (
                        "/ERROR/".to_string(),
                        WaitUntil::Pattern(regex::Regex::new("ERROR").unwrap()),
                    ),
                ],
            )),
            None,
        )
        .unwrap();
        let note = tokio::time::timeout(Duration::from_secs(10), notes.recv())
            .await
            .expect("match note arrives")
            .expect("one note");
        assert!(note.contains("matching `listening`"), "{note}");
        assert!(note.contains("listening on 4000"), "{note}");
        assert!(note.contains("still running"), "{note}");
        tokio::time::sleep(EXIT_NOTE_GRACE + Duration::from_millis(300)).await;
        assert!(notes.try_recv().is_err(), "a pattern fires once");
        stop_background_process(&manager, "bg-1").unwrap();
    }

    #[tokio::test]
    async fn a_matched_line_the_agent_already_read_wakes_nobody() {
        let root = workspace();
        let manager = new_shared_process_manager();
        let (sender, mut notes) = tokio::sync::mpsc::unbounded_channel::<String>();
        start_background_process_watching(
            &manager,
            root.path(),
            "echo ready; sleep 30",
            ".",
            None,
            Some((
                Arc::new(move |note| {
                    let _ = sender.send(note);
                }),
                vec![("ready".to_string(), WaitUntil::Literal("ready".into()))],
            )),
            None,
        )
        .unwrap();
        wait_background_process(
            &manager,
            "bg-1",
            Duration::from_secs(10),
            Some(WaitUntil::Literal("ready".into())),
        )
        .await
        .unwrap();
        tokio::time::sleep(EXIT_NOTE_GRACE + Duration::from_millis(300)).await;
        assert!(notes.try_recv().is_err(), "the agent already saw it");
        stop_background_process(&manager, "bg-1").unwrap();
    }

    #[test]
    fn exit_notes_shorten_long_commands_and_format_durations() {
        let command = "x".repeat(200);
        let note = exit_note_text("bg-3", &command, 0, Duration::from_secs(252));
        assert!(note.contains(&format!("`{}…`", "x".repeat(79))), "{note}");
        assert!(note.contains("exited 0 after 4m12s"), "{note}");
        assert_eq!(format_elapsed(Duration::from_secs(3725)), "1h2m5s");
    }

    #[tokio::test]
    async fn profile_owner_background_supervisor_retains_lease_until_process_retirement() {
        let root = workspace();
        let profile = root.path().join("profile.json");
        let owner = crate::profile_owner::ProfileOwner::acquire([profile.clone()]).unwrap();
        let manager = new_shared_process_manager();
        start_background_process_watching(
            &manager,
            root.path(),
            "echo ready; sleep 30",
            ".",
            None,
            None,
            Some(owner.clone()),
        )
        .unwrap();
        drop(owner);
        assert!(
            crate::profile_owner::ProfileOwner::acquire([profile.clone()]).is_err(),
            "running child released its profile lease with the caller"
        );
        // The independent process supervisor performs retirement; this wait does
        // not depend on the async test executor continuing to poll its tasks.
        drop(manager);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if crate::profile_owner::ProfileOwner::acquire([profile.clone()]).is_ok() {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "retired process did not release its profile lease"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    #[test]
    fn output_keeps_the_newest_lines_and_an_absolute_read_cursor() {
        let mut output = ManagedProcessOutput::default();
        for index in 0..(MAX_PROCESS_OUTPUT_LINES + 5) {
            output.push(index.to_string());
        }
        assert_eq!(output.lines.len(), MAX_PROCESS_OUTPUT_LINES);
        let unread = output.take(false);
        assert_eq!(unread.first().map(String::as_str), Some("5"));
        output.push("new".into());
        assert_eq!(output.take(false), vec!["new".to_string()]);
        assert!(output.take(false).is_empty());
    }
}
