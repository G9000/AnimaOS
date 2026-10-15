#[cfg(not(windows))]
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anima_core::CancelSignal;

use super::super::workspace::{
    canonical_workspace_root, resolve_workspace_search_root, workspace_root_path,
};

const BASH_MAX_OUTPUT_CHARS: usize = 30_000;
const BASH_MAX_OUTPUT_LINES: usize = 500;
/// Retained bytes per stream. Windows bounds the pending line in the same
/// fixed capture window; the unchanged non-Windows reader applies this cap
/// after each line. Final user-facing shortening remains substantially smaller.
const BASH_MAX_CAPTURE_BYTES: usize = 256 * 1024;

/// A bash command whose run was stopped (spec §4.6).
pub(in super::super) const BASH_STOPPED: &str = "Command stopped by owner";
#[cfg(windows)]
pub(super) const BASH_CLEANUP_UNKNOWN: &str = "Command cleanup is uncertain; effects may have occurred. Shell execution is disabled until daemon restart";
#[cfg(windows)]
const BASH_CLEANUP_DISABLED: &str = "Shell execution is disabled after uncertain cleanup; restart the daemon before another command";

/// How the polling loop ended, when it did not return a stop.
enum Waited {
    Exited(std::process::ExitStatus),
    TimedOut,
}

/// Environment variables that are safe and necessary to pass through to the
/// shell tool subprocess. Anything not on this list — including LLM provider
/// API keys, DATABASE_URL, etc. — is stripped via `Command::env_clear`.
const SHELL_ENV_ALLOWLIST: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "USERNAME",
    "LOGNAME",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "TERM",
    "TZ",
    "TMPDIR",
    "TEMP",
    "TMP",
    "SHELL",
    "PWD",
    "OLDPWD",
    // Windows-specific
    "SystemRoot",
    "SystemDrive",
    "ComSpec",
    "PATHEXT",
    "USERPROFILE",
    "APPDATA",
    "LOCALAPPDATA",
    "ProgramFiles",
    "ProgramFiles(x86)",
    "ProgramData",
    "windir",
    "OS",
    "PROCESSOR_ARCHITECTURE",
    "NUMBER_OF_PROCESSORS",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in super::super) struct BashCommandResult {
    pub(in super::super) status: &'static str,
    pub(in super::super) output: String,
}

pub(super) fn execute_bash_command(
    configured_root: Option<&Path>,
    command: &str,
    timeout_ms: u64,
    cwd: &str,
    cancel: Option<&CancelSignal>,
) -> Result<BashCommandResult, String> {
    let workspace_root = workspace_root_path("bash", configured_root)?;
    execute_bash_command_from_root(&workspace_root, command, timeout_ms, cwd, cancel)
}

pub(in super::super) fn execute_bash_command_from_root(
    workspace_root: &Path,
    command: &str,
    timeout_ms: u64,
    cwd: &str,
    cancel: Option<&CancelSignal>,
) -> Result<BashCommandResult, String> {
    #[cfg(windows)]
    {
        return execute_windows_from_root(
            workspace_root,
            command,
            timeout_ms,
            cwd,
            cancel.cloned(),
            None,
        );
    }
    #[cfg(not(windows))]
    {
        let cwd = resolve_workspace_search_root(
            &canonical_workspace_root(workspace_root, "bash")?,
            cwd,
            "bash",
        )?;
        let (executable, flags) = resolve_shell_launcher()?;

        let mut builder = Command::new(&executable);
        builder
            .args(&flags)
            .arg(command)
            .current_dir(&cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env_clear();
        for name in SHELL_ENV_ALLOWLIST {
            if let Ok(value) = std::env::var(name) {
                builder.env(name, value);
            }
        }
        let mut child = builder
            .spawn()
            .map_err(|error| format!("bash failed to start command: {error}"))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "bash stdout could not be captured".to_string())?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| "bash stderr could not be captured".to_string())?;

        let stdout_buffer = Arc::new(Mutex::new(String::new()));
        let stderr_buffer = Arc::new(Mutex::new(String::new()));

        let stdout_thread = spawn_output_capture(stdout, Arc::clone(&stdout_buffer), false);
        let stderr_thread = spawn_output_capture(stderr, Arc::clone(&stderr_buffer), true);

        let start = Instant::now();
        let waited = loop {
            if let Some(status) = child
                .try_wait()
                .map_err(|error| format!("bash failed while waiting for command: {error}"))?
            {
                break Waited::Exited(status);
            }

            if cancel.is_some_and(|signal| signal.is_cancelled()) {
                // Kills the direct child only (spec §4.6: "kills its child
                // process"); a process the command started keeps running, and a
                // process-group kill is later work (audit M11). The answer comes
                // at once: the capture threads are left to end on their own when
                // the last holder of the pipes closes them, so such a process
                // cannot keep the stopped run waiting (Task 8 fix round 1).
                child
                    .kill()
                    .map_err(|error| format!("bash failed to stop the command: {error}"))?;
                let _ = child.wait();
                return Ok(BashCommandResult {
                    status: "error",
                    output: BASH_STOPPED.to_string(),
                });
            }

            if start.elapsed() >= Duration::from_millis(timeout_ms) {
                child
                    .kill()
                    .map_err(|error| format!("bash failed to stop timed out command: {error}"))?;
                let _ = child.wait();
                break Waited::TimedOut;
            }

            thread::sleep(Duration::from_millis(10));
        };

        let _ = stdout_thread.join();
        let _ = stderr_thread.join();

        let stdout_text = stdout_buffer
            .lock()
            .map_err(|_| "bash stdout lock poisoned".to_string())?
            .clone();
        let stderr_text = stderr_buffer
            .lock()
            .map_err(|_| "bash stderr lock poisoned".to_string())?
            .clone();

        let exit_status = match waited {
            Waited::Exited(status) => status,
            Waited::TimedOut => {
                return Ok(BashCommandResult {
                    status: "error",
                    output: format!("Command timed out after {timeout_ms}ms"),
                });
            }
        };

        let exit_code = exit_status.code().unwrap_or(-1);
        let combined = if stdout_text.trim().is_empty() {
            stderr_text.clone()
        } else if stderr_text.trim().is_empty() {
            stdout_text.clone()
        } else {
            format!("{}\n{}", stdout_text.trim_end(), stderr_text.trim_end())
        };

        Ok(BashCommandResult {
            status: if exit_code == 0 { "success" } else { "error" },
            output: truncate_shell_output(&combined),
        })
    }
}

#[cfg(windows)]
pub(super) fn execute_bash_command_owned(
    configured_root: Option<&Path>,
    command: &str,
    timeout_ms: u64,
    cwd: &str,
    run_cancel: Option<CancelSignal>,
    call_cancel: CancelSignal,
) -> Result<BashCommandResult, String> {
    let root = workspace_root_path("bash", configured_root)?;
    execute_windows_from_root(
        &root,
        command,
        timeout_ms,
        cwd,
        run_cancel,
        Some(call_cancel),
    )
}

#[cfg(windows)]
fn cleanup_fuse() -> Arc<std::sync::atomic::AtomicBool> {
    static FUSE: std::sync::OnceLock<Arc<std::sync::atomic::AtomicBool>> =
        std::sync::OnceLock::new();
    FUSE.get_or_init(|| Arc::new(std::sync::atomic::AtomicBool::new(false)))
        .clone()
}
#[cfg(windows)]
pub(super) fn cleanup_disabled() -> Option<&'static str> {
    cleanup_fuse()
        .load(std::sync::atomic::Ordering::SeqCst)
        .then_some(BASH_CLEANUP_DISABLED)
}
#[cfg(windows)]
type ShellResult = Result<BashCommandResult, String>;
#[cfg(windows)]
fn publish_unknown(
    fuse: &std::sync::atomic::AtomicBool,
    sender: &std::sync::mpsc::SyncSender<ShellResult>,
) {
    fuse.store(true, std::sync::atomic::Ordering::SeqCst);
    let _ = sender.send(Err(BASH_CLEANUP_UNKNOWN.into()));
}
#[cfg(windows)]
fn owned_windows_worker(
    fuse: Arc<std::sync::atomic::AtomicBool>,
    work: impl FnOnce(std::sync::mpsc::SyncSender<ShellResult>, Arc<std::sync::atomic::AtomicBool>)
        + Send
        + 'static,
) -> ShellResult {
    if fuse.load(std::sync::atomic::Ordering::SeqCst) {
        return Err(BASH_CLEANUP_DISABLED.into());
    }
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let worker_fuse = fuse.clone();
    let owner = thread::Builder::new()
        .name("anima-shell-owner".into())
        .spawn(move || work(sender, worker_fuse))
        .map_err(|_| "Shell owner could not be started".to_string())?;
    let result = receiver.recv().unwrap_or_else(|_| {
        fuse.store(true, std::sync::atomic::Ordering::SeqCst);
        Err(BASH_CLEANUP_UNKNOWN.into())
    });
    if result
        .as_ref()
        .err()
        .is_some_and(|error| error == BASH_CLEANUP_UNKNOWN)
    {
        // A latched unknown owner retains its exact resources on that same
        // uncancelled thread. No retry/reaper or replacement registry exists.
        return result;
    }
    if owner.join().is_err() {
        fuse.store(true, std::sync::atomic::Ordering::SeqCst);
        return Err(BASH_CLEANUP_UNKNOWN.into());
    }
    result
}

#[cfg(windows)]
struct WindowsShellOwner {
    command: process_wrap::std::CommandWrap,
    child: Option<Box<dyn process_wrap::std::ChildWrapper>>,
    readers: Vec<thread::JoinHandle<std::io::Result<()>>>,
    stdout: Arc<Mutex<String>>,
    stderr: Arc<Mutex<String>>,
    wait_started: bool,
}
#[cfg(windows)]
enum WindowsWait {
    Exited(std::process::ExitStatus),
    Stopped,
    TimedOut,
    Failed,
}
#[cfg(windows)]
fn execute_windows_from_root(
    root: &Path,
    command: &str,
    timeout_ms: u64,
    cwd: &str,
    run_cancel: Option<CancelSignal>,
    call_cancel: Option<CancelSignal>,
) -> ShellResult {
    let fuse = cleanup_fuse();
    if fuse.load(std::sync::atomic::Ordering::SeqCst) {
        return Err(BASH_CLEANUP_DISABLED.into());
    }
    let stopped = || {
        run_cancel.as_ref().is_some_and(CancelSignal::is_cancelled)
            || call_cancel.as_ref().is_some_and(CancelSignal::is_cancelled)
    };
    if stopped() {
        return Ok(BashCommandResult {
            status: "error",
            output: BASH_STOPPED.into(),
        });
    }
    let cwd = resolve_workspace_search_root(&canonical_workspace_root(root, "bash")?, cwd, "bash")?;
    let (executable, flags) = resolve_shell_launcher()?;
    let mut builder = Command::new(&executable);
    builder
        .args(flags)
        .arg(command)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_clear();
    for name in SHELL_ENV_ALLOWLIST {
        if let Ok(value) = std::env::var(name) {
            builder.env(name, value);
        }
    }
    use process_wrap::std::{CommandWrap, CreationFlags, JobObject};
    let mut wrapped = CommandWrap::from(builder);
    let mut creation = CreationFlags(Default::default());
    // Documented CREATE_NO_WINDOW through the same maintained public shim as
    // M44/M46. JobObject adds temporary suspension and assigns before resume.
    creation.0 .0 = 0x08000000;
    wrapped.wrap(creation).wrap(JobObject);
    owned_windows_worker(fuse, move |sender, fuse| {
        let mut owner = WindowsShellOwner {
            command: wrapped,
            child: None,
            readers: Vec::new(),
            stdout: Arc::new(Mutex::new(String::new())),
            stderr: Arc::new(Mutex::new(String::new())),
            wait_started: false,
        };
        // Keep the owner outside unwinding so even a panic cannot silently
        // drop a launched Job/readers and turn missing proof into completion.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            owner.run(timeout_ms, run_cancel, call_cancel, &fuse)
        }));
        match result {
            Ok(Ok(result)) => {
                let _ = sender.send(result);
            }
            _ => {
                publish_unknown(&fuse, &sender);
                if !owner.wait_started {
                    if let Some(child) = owner.child.as_mut() {
                        let _ = child.start_kill();
                        owner.wait_started = true;
                        let _ = child.wait();
                    }
                }
                // Explicit quarantine: retain this command/Job/remaining IO
                // forever; a process-wide broken-cleanup fuse denies new bash.
                loop {
                    thread::park();
                    std::hint::black_box(&owner);
                }
            }
        }
    })
}
#[cfg(windows)]
impl WindowsShellOwner {
    fn run(
        &mut self,
        timeout_ms: u64,
        run_cancel: Option<CancelSignal>,
        call_cancel: Option<CancelSignal>,
        fuse: &std::sync::atomic::AtomicBool,
    ) -> Result<ShellResult, ()> {
        let stopped = || {
            run_cancel.as_ref().is_some_and(CancelSignal::is_cancelled)
                || call_cancel.as_ref().is_some_and(CancelSignal::is_cancelled)
        };
        if fuse.load(std::sync::atomic::Ordering::SeqCst) {
            return Ok(Err(BASH_CLEANUP_DISABLED.into()));
        }
        if stopped() {
            return Ok(Ok(BashCommandResult {
                status: "error",
                output: BASH_STOPPED.into(),
            }));
        }
        self.child = match self.command.spawn() {
            Ok(child) => Some(child),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Err("bash failed to start command".into()))
            }
            Err(_) => return Err(()), // wrapper assignment may follow native launch
        };
        let child = self.child.as_mut().ok_or(())?;
        let contained = child.as_ref().type_id()
            == std::any::TypeId::of::<process_wrap::std::JobObjectChild>()
            && child.inner().type_id() == std::any::TypeId::of::<std::process::Child>();
        let stdout = child.stdout().take();
        let stderr = child.stderr().take();
        if let Some(stdout) = stdout {
            self.readers
                .push(spawn_output_capture(stdout, self.stdout.clone(), false));
        }
        if let Some(stderr) = stderr {
            self.readers
                .push(spawn_output_capture(stderr, self.stderr.clone(), true));
        }
        if !contained || self.readers.len() != 2 {
            let _ = child.start_kill();
            self.wait_started = true;
            let _ = child.wait();
            return Err(());
        }
        let start = Instant::now();
        let outcome = loop {
            if stopped() {
                break WindowsWait::Stopped;
            }
            if start.elapsed() >= Duration::from_millis(timeout_ms) {
                break WindowsWait::TimedOut;
            }
            // Only the exact native inner leader is polled. Never disturb the
            // JobObject wait cache or infer descendant disposal from this.
            match child.inner_mut().try_wait() {
                Ok(Some(status)) => break WindowsWait::Exited(status),
                Ok(None) => {}
                Err(_) => break WindowsWait::Failed,
            }
            thread::sleep(Duration::from_millis(10));
        };
        let killed = child.start_kill();
        // Exactly one maintained whole-Job wait, never timed out/reinvoked.
        self.wait_started = true;
        let waited = child.wait();
        if killed.is_err() || waited.is_err() {
            return Err(());
        }
        while let Some(reader) = self.readers.pop() {
            if !matches!(reader.join(), Ok(Ok(()))) {
                return Err(());
            }
        }
        let stdout = self.stdout.lock().map_err(|_| ())?.clone();
        let stderr = self.stderr.lock().map_err(|_| ())?.clone();
        match outcome {
            WindowsWait::Stopped => Ok(Ok(BashCommandResult {
                status: "error",
                output: BASH_STOPPED.into(),
            })),
            WindowsWait::TimedOut => Ok(Ok(BashCommandResult {
                status: "error",
                output: format!("Command timed out after {timeout_ms}ms"),
            })),
            WindowsWait::Failed => Ok(Err("bash failed while waiting for command".into())),
            WindowsWait::Exited(status) => {
                let combined = if stdout.trim().is_empty() {
                    stderr
                } else if stderr.trim().is_empty() {
                    stdout
                } else {
                    format!("{}\n{}", stdout.trim_end(), stderr.trim_end())
                };
                Ok(Ok(BashCommandResult {
                    status: if status.code() == Some(0) {
                        "success"
                    } else {
                        "error"
                    },
                    output: truncate_shell_output(&combined),
                }))
            }
        }
    }
}

pub(in super::super) fn truncate_shell_output(text: &str) -> String {
    truncate_text_with_limits(text, BASH_MAX_OUTPUT_CHARS, BASH_MAX_OUTPUT_LINES)
}

fn truncate_text_with_limits(text: &str, max_chars: usize, max_lines: usize) -> String {
    let mut lines = text.lines().map(ToString::to_string).collect::<Vec<_>>();
    if lines.len() > max_lines {
        let head = max_lines / 2;
        let tail = max_lines - head;
        let omitted = lines.len().saturating_sub(max_lines);
        let mut selected = Vec::new();
        selected.extend(lines.iter().take(head).cloned());
        selected.push(format!("... [{} lines omitted] ...", omitted));
        selected.extend(lines.iter().skip(lines.len().saturating_sub(tail)).cloned());
        lines = selected;
    }

    let content = lines.join("\n");
    if content.chars().count() <= max_chars {
        return content;
    }

    let half = max_chars / 2;
    let start = content.chars().take(half).collect::<String>();
    let end = content
        .chars()
        .rev()
        .take(half)
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>();
    format!("{}\n... [output truncated] ...\n{}", start, end)
}

pub(in super::super) fn resolve_shell_launcher() -> Result<(String, Vec<String>), String> {
    let candidates = shell_launcher_candidates();
    let fallback = candidates
        .first()
        .cloned()
        .ok_or_else(|| "No shell candidates available".to_string())?;

    for (executable, flags) in &candidates {
        if shell_candidate_exists(executable) {
            return Ok((executable.clone(), flags.clone()));
        }
    }

    Ok(fallback)
}

fn shell_launcher_candidates() -> Vec<(String, Vec<String>)> {
    // We always invoke with `-c` (non-login, non-interactive) so the shell does
    // not source profile scripts — this keeps the env we curate via the
    // `SHELL_ENV_ALLOWLIST` from being repopulated with operator secrets.
    if cfg!(windows) {
        windows_shell_candidates()
    } else if cfg!(target_os = "macos") {
        vec![
            ("/bin/zsh".to_string(), vec!["-c".to_string()]),
            ("/bin/bash".to_string(), vec!["-c".to_string()]),
        ]
    } else {
        let mut candidates = Vec::new();
        if let Ok(user_shell) = std::env::var("SHELL") {
            if !user_shell.trim().is_empty() {
                candidates.push((user_shell.clone(), shell_flags(&user_shell)));
            }
        }
        candidates.push(("/bin/bash".to_string(), vec!["-c".to_string()]));
        candidates.push(("/usr/bin/bash".to_string(), vec!["-c".to_string()]));
        candidates.push(("/bin/zsh".to_string(), vec!["-c".to_string()]));
        candidates.push(("/bin/sh".to_string(), vec!["-c".to_string()]));
        candidates
    }
}

fn windows_shell_candidates() -> Vec<(String, Vec<String>)> {
    let mut candidates = Vec::new();
    for path in [
        "C:\\Program Files\\Git\\bin\\bash.exe",
        "C:\\Program Files (x86)\\Git\\bin\\bash.exe",
    ] {
        if Path::new(path).exists() {
            candidates.push((path.to_string(), vec!["-c".to_string()]));
        }
    }
    candidates.push(("bash".to_string(), vec!["-c".to_string()]));
    candidates.push((
        "powershell.exe".to_string(),
        vec!["-NoProfile".to_string(), "-Command".to_string()],
    ));
    candidates.push((
        "pwsh".to_string(),
        vec!["-NoProfile".to_string(), "-Command".to_string()],
    ));
    candidates.push((
        std::env::var("ComSpec").unwrap_or_else(|_| "cmd.exe".to_string()),
        vec!["/d".to_string(), "/s".to_string(), "/c".to_string()],
    ));
    candidates
}

fn shell_flags(_shell: &str) -> Vec<String> {
    // Always non-login: we explicitly do NOT want the shell sourcing rc/profile
    // files that would re-leak operator env into the subprocess.
    vec!["-c".to_string()]
}

fn shell_candidate_exists(executable: &str) -> bool {
    if executable.starts_with('/') || executable.contains(':') {
        return Path::new(executable).exists();
    }

    let probe = if cfg!(windows) { "where" } else { "which" };
    Command::new(probe)
        .arg(executable)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(not(windows))]
fn spawn_output_capture<R>(
    reader: R,
    buffer: Arc<Mutex<String>>,
    stderr: bool,
) -> thread::JoinHandle<()>
where
    R: std::io::Read + Send + 'static,
{
    thread::spawn(move || {
        let mut local = String::new();
        let mut truncated = false;
        let reader = BufReader::new(reader);
        for line in reader.lines() {
            let Ok(line) = line else {
                break;
            };
            if truncated {
                continue;
            }
            let prefix_len = if stderr { "[stderr] ".len() } else { 0 };
            if local.len() + prefix_len + line.len() + 1 > BASH_MAX_CAPTURE_BYTES {
                local.push_str("\n[output capture limit reached; further bytes discarded]\n");
                truncated = true;
                continue;
            }
            if stderr {
                local.push_str("[stderr] ");
            }
            local.push_str(&line);
            local.push('\n');
        }

        if let Ok(mut output) = buffer.lock() {
            output.push_str(&local);
        }
    })
}

#[cfg(any(windows, test))]
const CAPTURE_CHUNK_BYTES: usize = 8192;
#[cfg(any(windows, test))]
const CAPTURE_LIMIT_NOTICE: &str = "\n[output capture limit reached; further bytes discarded]\n";

#[cfg(any(windows, test))]
struct CapturedStream {
    text: String,
    #[cfg(test)]
    maximum_retained: usize,
    #[cfg(test)]
    maximum_capacity: usize,
}

/// Fixed Windows capture window. A pending line occupies the same bounded
/// byte vector as committed text; rejection rolls back that line, then drains.
#[cfg(any(windows, test))]
fn capture_stream(mut reader: impl std::io::Read, stderr: bool) -> std::io::Result<CapturedStream> {
    let mut output = Vec::with_capacity(BASH_MAX_CAPTURE_BYTES + CAPTURE_LIMIT_NOTICE.len());
    let mut chunk = [0u8; CAPTURE_CHUNK_BYTES];
    let mut discarded = false;
    let mut line_open = false;
    let mut line_start = 0usize;
    let prefix = if stderr {
        b"[stderr] ".as_slice()
    } else {
        b"".as_slice()
    };
    #[cfg(test)]
    let mut maximum_retained = 0usize;
    #[cfg(test)]
    let mut maximum_capacity = output.capacity();
    loop {
        let count = match reader.read(&mut chunk) {
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        for &byte in &chunk[..count] {
            if discarded {
                continue;
            }
            let mut limit_reached = false;
            if !line_open {
                line_open = true;
                line_start = output.len();
                if output.len() + prefix.len() <= BASH_MAX_CAPTURE_BYTES {
                    output.extend_from_slice(prefix);
                } else {
                    limit_reached = true;
                }
            }
            if !limit_reached && byte == b'\n' {
                if output.last() == Some(&b'\r') {
                    output.pop();
                }
                if std::str::from_utf8(&output[line_start..]).is_err() {
                    output.truncate(line_start);
                    discarded = true;
                } else if output.len() < BASH_MAX_CAPTURE_BYTES {
                    output.push(b'\n');
                    line_open = false;
                } else {
                    limit_reached = true;
                }
            } else if !limit_reached {
                if output.len() < BASH_MAX_CAPTURE_BYTES {
                    output.push(byte);
                } else {
                    limit_reached = true;
                }
            }
            if limit_reached {
                output.truncate(line_start);
                output.extend_from_slice(CAPTURE_LIMIT_NOTICE.as_bytes());
                discarded = true;
            }
            #[cfg(test)]
            {
                maximum_retained = maximum_retained.max(output.len());
                maximum_capacity = maximum_capacity.max(output.capacity());
            }
        }
        if count == 0 {
            break;
        }
    }
    if !discarded && line_open {
        if std::str::from_utf8(&output[line_start..]).is_err() {
            output.truncate(line_start);
        } else if output.len() < BASH_MAX_CAPTURE_BYTES {
            // As with BufRead::lines, an EOF without LF preserves trailing CR.
            output.push(b'\n');
        } else {
            output.truncate(line_start);
            output.extend_from_slice(CAPTURE_LIMIT_NOTICE.as_bytes());
        }
    }
    #[cfg(test)]
    {
        maximum_retained = maximum_retained.max(output.len());
        maximum_capacity = maximum_capacity.max(output.capacity());
    }
    let text = String::from_utf8(output)
        .map_err(|_| std::io::Error::other("shell capture unavailable"))?;
    Ok(CapturedStream {
        text,
        #[cfg(test)]
        maximum_retained,
        #[cfg(test)]
        maximum_capacity,
    })
}

#[cfg(windows)]
fn spawn_output_capture<R>(
    reader: R,
    buffer: Arc<Mutex<String>>,
    stderr: bool,
) -> thread::JoinHandle<std::io::Result<()>>
where
    R: std::io::Read + Send + 'static,
{
    thread::spawn(move || {
        let captured = capture_stream(reader, stderr)?;
        *buffer
            .lock()
            .map_err(|_| std::io::Error::other("shell capture unavailable"))? = captured.text;
        Ok(())
    })
}

#[cfg(test)]
mod capture_tests {
    use super::*;
    struct MeteredReader {
        input: std::io::Cursor<Vec<u8>>,
        maximum_request: usize,
        consumed: usize,
    }
    impl std::io::Read for MeteredReader {
        fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
            self.maximum_request = self.maximum_request.max(output.len());
            let read = std::io::Read::read(&mut self.input, output)?;
            self.consumed += read;
            Ok(read)
        }
    }
    #[test]
    fn m47_coding_chunk_capture_bounds_requests_retention_and_allocation_while_draining() {
        let input = [
            b"first\n".as_slice(),
            &vec![b'x'; BASH_MAX_CAPTURE_BYTES * 2],
            b"\nignored\n",
        ]
        .concat();
        let total = input.len();
        let mut reader = MeteredReader {
            input: std::io::Cursor::new(input),
            maximum_request: 0,
            consumed: 0,
        };
        let captured = capture_stream(&mut reader, false).unwrap();
        assert_eq!(
            reader.consumed, total,
            "discarding must still drain the actual input"
        );
        assert_eq!(reader.maximum_request, CAPTURE_CHUNK_BYTES);
        assert!(captured.maximum_retained <= BASH_MAX_CAPTURE_BYTES + CAPTURE_LIMIT_NOTICE.len());
        assert!(
            captured.maximum_retained > BASH_MAX_CAPTURE_BYTES / 2,
            "measure the actual pending long line"
        );
        assert!(captured.maximum_capacity <= BASH_MAX_CAPTURE_BYTES + CAPTURE_LIMIT_NOTICE.len());
        assert!(captured.text.starts_with("first\n"));
        assert!(captured.text.ends_with(CAPTURE_LIMIT_NOTICE));
        assert!(!captured.text.contains("ignored"));
    }
    #[test]
    fn m47_coding_chunk_capture_preserves_utf8_crlf_stderr_and_drains_invalid_text() {
        let input = format!("{}🟣\r\nlast\r", "a".repeat(CAPTURE_CHUNK_BYTES - 1));
        let captured = capture_stream(input.as_bytes(), true).unwrap();
        assert_eq!(
            captured.text,
            format!(
                "[stderr] {}🟣\n[stderr] last\r\n",
                "a".repeat(CAPTURE_CHUNK_BYTES - 1)
            )
        );
        let input = b"kept\ninvalid\xff\nignored\n".to_vec();
        let total = input.len();
        let mut reader = MeteredReader {
            input: std::io::Cursor::new(input),
            maximum_request: 0,
            consumed: 0,
        };
        assert_eq!(capture_stream(&mut reader, false).unwrap().text, "kept\n");
        assert_eq!(reader.consumed, total);
    }
    #[cfg(windows)]
    #[test]
    fn m47_coding_unknown_cleanup_retains_owner_and_fuses_later_admission() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        struct Retained(Arc<AtomicBool>);
        impl Drop for Retained {
            fn drop(&mut self) {
                self.0.store(false, Ordering::SeqCst);
            }
        }
        // An isolated fuse and a controlled resource signal test this branch;
        // no real Job/OS cleanup failure is injected or claimed here.
        let fuse = Arc::new(AtomicBool::new(false));
        let retained = Arc::new(AtomicBool::new(true));
        let finished = Arc::new(AtomicBool::new(false));
        let (release, wait) = std::sync::mpsc::channel();
        let held = retained.clone();
        let done = finished.clone();
        let first = owned_windows_worker(fuse.clone(), move |sender, fuse| {
            let resource = Retained(held);
            publish_unknown(&fuse, &sender);
            let _ = wait.recv();
            drop(resource);
            done.store(true, Ordering::SeqCst);
        });
        let latched_before_reply = fuse.load(Ordering::SeqCst);
        let retained_after_reply = retained.load(Ordering::SeqCst);
        let started = Arc::new(AtomicUsize::new(0));
        let observed = started.clone();
        let second = owned_windows_worker(fuse, move |sender, _| {
            observed.fetch_add(1, Ordering::SeqCst);
            let _ = sender.send(Ok(BashCommandResult {
                status: "success",
                output: "must not run".into(),
            }));
        });
        let _ = release.send(());
        let deadline = Instant::now() + Duration::from_secs(1);
        while !finished.load(Ordering::SeqCst) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(first, Err(BASH_CLEANUP_UNKNOWN.into()));
        assert!(latched_before_reply && retained_after_reply);
        assert_eq!(second, Err(BASH_CLEANUP_DISABLED.into()));
        assert_eq!(
            started.load(Ordering::SeqCst),
            0,
            "the fused branch must precede owner thread/work creation"
        );
        assert!(finished.load(Ordering::SeqCst) && !retained.load(Ordering::SeqCst));
    }
}

/// Root-only controlled process fixture; not a foreground process registry or
/// a production cleanup implementation. Every peer independently self-expires.
#[cfg(all(test, windows))]
pub(crate) struct M47ShellFixture {
    directory: tempfile::TempDir,
    command: String,
    shell: String,
    cleanup_deadline: Instant,
    neighbor: Option<std::process::Child>,
}
#[cfg(all(test, windows))]
impl M47ShellFixture {
    pub(crate) fn new(mode: &str) -> Self {
        assert!(matches!(mode, "live" | "leader-exit"));
        let node = std::path::PathBuf::from(
            std::env::var_os("ANIMAOS_M47_TEST_NODE")
                .expect("root supplies an explicit absolute installed Node path"),
        );
        assert!(node.is_absolute() && node.is_file());
        let (shell, flags) = resolve_shell_launcher().expect("selected shell exists");
        assert!(
            shell.to_ascii_lowercase().ends_with("\\git\\bin\\bash.exe")
                && flags.len() == 1
                && flags[0] == "-c",
            "this witness requires the actual selected Git Bash, not a substitute shell"
        );
        let directory = tempfile::Builder::new()
            .prefix("anima-m47-shell-")
            .tempdir()
            .unwrap();
        let neighbor_root = directory.path().join("neighbor");
        std::fs::create_dir(&neighbor_root).unwrap();
        let expires = anima_core::primitives::now_millis() + 8_000;
        let cleanup_deadline = Instant::now() + Duration::from_secs(9);
        let child_source = r#"const fs=require('node:fs'),path=require('node:path');
const root=process.argv[2],expires=Number(process.argv[3]);let n=0;
fs.writeFileSync(path.join(root,'child.json'),JSON.stringify({pid:process.pid,parentPid:process.ppid,image:process.execPath,startedAtMs:Date.now(),expiresAtMs:expires}));
const beat=()=>{if(fs.existsSync(path.join(root,'stop')))process.exit(0);fs.writeFileSync(path.join(root,'heartbeat'),String(++n));};beat();setInterval(beat,20);setTimeout(()=>process.exit(0),Math.max(0,expires-Date.now()));"#;
        let parent_source = r#"const fs=require('node:fs'),path=require('node:path'),{spawn}=require('node:child_process');
const root=process.argv[2],expires=Number(process.argv[3]),mode=process.argv[4];
const child=spawn(process.execPath,[path.join(__dirname,'child.cjs'),root,String(expires)],{stdio:['ignore','inherit','inherit'],windowsHide:true});
fs.writeFileSync(path.join(root,'parent.json'),JSON.stringify({pid:process.pid,parentPid:process.ppid,childPid:child.pid,image:process.execPath,startedAtMs:Date.now(),expiresAtMs:expires}));
setInterval(()=>{if(fs.existsSync(path.join(root,'stop')))process.exit(0);},20);setTimeout(()=>process.exit(0),Math.max(0,expires-Date.now()));if(mode==='leader-exit'){const ready=setInterval(()=>{if(fs.existsSync(path.join(root,'heartbeat'))){clearInterval(ready);setTimeout(()=>process.exit(0),200);}},20);}"#;
        let child_script = directory.path().join("child.cjs");
        let parent_script = directory.path().join("parent.cjs");
        std::fs::write(&child_script, child_source).unwrap();
        std::fs::write(&parent_script, parent_source).unwrap();
        let quote = |path: &Path| {
            format!(
                "'{}'",
                path.to_str()
                    .unwrap()
                    .replace('\\', "/")
                    .replace('\'', "'\\''")
            )
        };
        // A trailing fixed command prevents Bash's last-command exec shortcut;
        // the selected shell remains the owned leader while Node is live.
        let command = format!(
            "{} {} {} {} {}; printf 'm47-shell-finished\\n'",
            quote(&node),
            quote(&parent_script),
            quote(directory.path()),
            expires,
            mode
        );
        let mut builder = Command::new(&node);
        builder
            .arg(&child_script)
            .arg(&neighbor_root)
            .arg(expires.to_string())
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        for name in ["SystemRoot", "WINDIR"] {
            if let Some(value) = std::env::var_os(name) {
                builder.env(name, value);
            }
        }
        let neighbor = builder
            .spawn()
            .expect("owned self-expiring neighbor Node starts");
        Self {
            directory,
            command,
            shell,
            cleanup_deadline,
            neighbor: Some(neighbor),
        }
    }
    pub(crate) fn root(&self) -> &Path {
        self.directory.path()
    }
    pub(crate) fn command(&self) -> String {
        self.command.clone()
    }
    pub(crate) fn heartbeat(&self) -> Option<String> {
        std::fs::read_to_string(self.root().join("heartbeat")).ok()
    }
    pub(crate) fn neighbor_heartbeat(&self) -> Option<String> {
        std::fs::read_to_string(self.root().join("neighbor/heartbeat")).ok()
    }
    pub(crate) async fn wait_live(&self) -> bool {
        tokio::time::timeout(Duration::from_secs(3), async {
            while self.heartbeat().is_none()
                || self.neighbor_heartbeat().is_none()
                || !self.root().join("parent.json").is_file()
                || !self.root().join("child.json").is_file()
                || !self.root().join("neighbor/child.json").is_file()
            {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            let Some(directory) = std::env::var_os("ANIMAOS_M47_TEST_WITNESS_DIR") else {
                return true;
            };
            let directory = std::path::PathBuf::from(directory);
            if !directory.is_absolute() || !directory.is_dir() {
                return false;
            }
            let read = |name| {
                std::fs::read(self.root().join(name))
                    .ok()
                    .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            };
            let (Some(parent), Some(grandchild), Some(neighbor)) = (
                read("parent.json"),
                read("child.json"),
                read("neighbor/child.json"),
            ) else {
                return false;
            };
            let Some(basename) = self.root().file_name().and_then(|name| name.to_str()) else {
                return false;
            };
            let request = directory.join(format!("{basename}.request.json"));
            let observed = directory.join(format!("{basename}.observed.json"));
            let payload = serde_json::json!({"fixtureRoot":self.root(),"parent":parent,
                "grandchild":grandchild,"neighbor":neighbor,
                "requestedAtMs":anima_core::primitives::now_millis()});
            let Ok(bytes) = serde_json::to_vec(&payload) else {
                return false;
            };
            if std::fs::write(request, bytes).is_err() {
                return false;
            }
            // Root acknowledges only after saving independent kernel identity
            // evidence. This readiness wait never delays post-action disposal.
            while !observed.is_file() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            true
        })
        .await
        .unwrap_or(false)
    }
    pub(crate) fn witness(&self, case: &str) {
        let read = |name| {
            std::fs::read(self.root().join(name))
                .ok()
                .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        };
        println!(
            "M47_PRIVATE_PROCESS {}",
            serde_json::json!({"case":case,"fixtureRoot":self.root(),"selectedShell":self.shell,
            "parent":read("parent.json"),"grandchild":read("child.json"),"neighbor":read("neighbor/child.json")})
        );
    }
    fn request_cleanup(&self) {
        let _ = std::fs::write(self.root().join("stop"), b"stop");
        let _ = std::fs::write(self.root().join("neighbor/stop"), b"stop");
    }
    pub(crate) async fn cleanup(&mut self) {
        self.request_cleanup();
        // The RED path may have lost its child/reader handles. Wait beyond the
        // fixed peers' independent expiry before propagating any assertion.
        if let Some(remaining) = self.cleanup_deadline.checked_duration_since(Instant::now()) {
            tokio::time::sleep(remaining).await;
        }
        if let Some(mut child) = self.neighbor.take() {
            let _ = child.wait();
        }
    }
}
#[cfg(all(test, windows))]
impl Drop for M47ShellFixture {
    fn drop(&mut self) {
        self.request_cleanup();
        if let Some(remaining) = self.cleanup_deadline.checked_duration_since(Instant::now()) {
            thread::sleep(remaining);
        }
        if let Some(mut child) = self.neighbor.take() {
            let _ = child.wait();
        }
    }
}
