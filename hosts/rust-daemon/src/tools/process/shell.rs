use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
#[cfg(windows)]
use std::sync::Mutex;
#[cfg(windows)]
use std::thread;
use std::time::Duration;
#[cfg(windows)]
use std::time::Instant;

use anima_core::CancelSignal;

use super::super::command_binding::CommandBinding;

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

/// How the Unix polling loop ended.
#[cfg(not(windows))]
enum Waited {
    Exited(std::process::ExitStatus),
    Stopped,
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

/// Variables every shell tool child gets on top of the allowlist, so a pager
/// never waits for input that cannot come.
const SHELL_ENV_FIXED: &[(&str, &str)] = &[("PAGER", "cat"), ("GIT_PAGER", "cat")];

/// The whole environment of a `bash` or `bg_start` child; callers apply it
/// after `env_clear`.
pub(super) fn shell_env() -> Vec<(&'static str, OsString)> {
    let mut env = SHELL_ENV_ALLOWLIST
        .iter()
        .filter_map(|name| std::env::var_os(name).map(|value| (*name, value)))
        .collect::<Vec<_>>();
    env.extend(
        SHELL_ENV_FIXED
            .iter()
            .map(|(name, value)| (*name, OsString::from(value))),
    );
    env
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in super::super) struct BashCommandResult {
    pub(in super::super) status: &'static str,
    pub(in super::super) output: String,
}

struct BoundShellLaunch {
    binding: Arc<CommandBinding>,
    executable: String,
    flags: Vec<String>,
    command: String,
    cwd: PathBuf,
}

impl BoundShellLaunch {
    fn revalidate(&self) -> Result<(), String> {
        revalidate_shell_launch(
            Some(&self.binding),
            &self.executable,
            &self.flags,
            &self.command,
            &self.cwd,
        )
    }
}

/// Runs synchronously at the final spawn boundary, after any queue or async wait.
/// `None` is reserved for explicitly trusted raw embedding helpers and tests.
pub(super) fn revalidate_shell_launch(
    binding: Option<&CommandBinding>,
    executable: &str,
    flags: &[String],
    command: &str,
    cwd: &Path,
) -> Result<(), String> {
    let Some(binding) = binding else {
        return Ok(());
    };
    let (selected, selected_flags) = resolve_shell_launcher()?;
    if selected != executable || selected_flags != flags {
        return Err("Execution launcher changed while approval was pending; request approval again; the command did not run".into());
    }
    let current = CommandBinding::capture_shell(executable, flags, command, cwd)?;
    binding.ensure_unchanged(&current)
}

pub(super) fn execute_bash_command(
    configured_root: Option<&Path>,
    command: &str,
    timeout_ms: u64,
    cwd: &str,
    cancel: Option<&CancelSignal>,
) -> Result<BashCommandResult, String> {
    let workspace_root = workspace_root_path("bash", configured_root)?;
    execute_bash_command_with_requirement(
        &workspace_root,
        command,
        timeout_ms,
        cwd,
        cancel,
        true,
        None,
    )
}

pub(super) fn execute_bash_command_bound(
    configured_root: Option<&Path>,
    command: &str,
    timeout_ms: u64,
    cwd: &str,
    cancel: Option<&CancelSignal>,
    binding: Arc<CommandBinding>,
) -> Result<BashCommandResult, String> {
    let root = workspace_root_path("bash", configured_root)?;
    execute_bash_command_with_requirement(
        &root,
        command,
        timeout_ms,
        cwd,
        cancel,
        true,
        Some(binding),
    )
}

pub(in super::super) fn execute_bash_command_from_root_bound(
    workspace_root: &Path,
    command: &str,
    timeout_ms: u64,
    cwd: &str,
    cancel: Option<&CancelSignal>,
    binding: Arc<CommandBinding>,
) -> Result<BashCommandResult, String> {
    execute_bash_command_with_requirement(
        workspace_root,
        command,
        timeout_ms,
        cwd,
        cancel,
        true,
        Some(binding),
    )
}

pub(in super::super) fn execute_bash_command_from_root(
    workspace_root: &Path,
    command: &str,
    timeout_ms: u64,
    cwd: &str,
    cancel: Option<&CancelSignal>,
) -> Result<BashCommandResult, String> {
    execute_bash_command_with_requirement(
        workspace_root,
        command,
        timeout_ms,
        cwd,
        cancel,
        false,
        None,
    )
}

fn execute_bash_command_with_requirement(
    workspace_root: &Path,
    command: &str,
    timeout_ms: u64,
    cwd: &str,
    cancel: Option<&CancelSignal>,
    require_strong: bool,
    binding: Option<Arc<CommandBinding>>,
) -> Result<BashCommandResult, String> {
    #[cfg(windows)]
    {
        let _ = require_strong; // Windows always assigns a Job before resume.
        return execute_windows_from_root(
            workspace_root,
            command,
            timeout_ms,
            cwd,
            cancel.cloned(),
            None,
            binding,
        );
    }
    #[cfg(not(windows))]
    {
        let cwd = resolve_workspace_search_root(
            &canonical_workspace_root(workspace_root, "bash")?,
            cwd,
            "bash",
        )?;
        let (executable, mut args) = resolve_shell_launcher()?;
        args.push(command.to_string());
        let cancel = cancel.cloned();
        let timeout = Duration::from_millis(timeout_ms);
        super::run_on_process_runtime(run_unix_shell(
            executable,
            args,
            cwd,
            timeout,
            cancel,
            require_strong,
            binding,
        ))
        .unwrap_or_else(|| Err("bash worker stopped unexpectedly".to_string()))
    }
}

#[cfg(test)]
mod binding_tests {
    use super::*;

    #[test]
    fn queued_shell_worker_refuses_a_changed_entry_script_before_spawn() {
        let root = tempfile::tempdir().unwrap();
        let script = root.path().join("entry.sh");
        std::fs::write(&script, "echo approved\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let (shell, flags) = resolve_shell_launcher().unwrap();
        let binding =
            CommandBinding::capture_shell(&shell, &flags, "./entry.sh", root.path()).unwrap();
        std::fs::write(&script, "echo changed > should-not-exist\n").unwrap();
        let error = execute_bash_command_with_requirement(
            root.path(),
            "./entry.sh",
            1_000,
            ".",
            None,
            false,
            Some(Arc::new(binding)),
        )
        .unwrap_err();
        assert!(error.contains("identity changed"), "{error}");
        assert!(!root.path().join("should-not-exist").exists());
    }
}

/// Runs one foreground command under `Custody`, so a timeout or a stop
/// retires the whole process tree (audit M11), not only the shell.
#[cfg(not(windows))]
async fn run_unix_shell(
    executable: String,
    args: Vec<String>,
    cwd: std::path::PathBuf,
    timeout: Duration,
    cancel: Option<CancelSignal>,
    require_strong: bool,
    binding: Option<Arc<CommandBinding>>,
) -> Result<BashCommandResult, String> {
    use process_wrap::tokio::CommandWrap;

    let timed_out = || BashCommandResult {
        status: "error",
        output: format!("Command timed out after {}ms", timeout.as_millis()),
    };
    let stopped = || BashCommandResult {
        status: "error",
        output: BASH_STOPPED.to_string(),
    };
    let is_cancelled = || cancel.as_ref().is_some_and(CancelSignal::is_cancelled);

    let identity = binding.map(|binding| BoundShellLaunch {
        binding,
        executable: executable.clone(),
        flags: args[..args.len().saturating_sub(1)].to_vec(),
        command: args.last().cloned().unwrap_or_default(),
        cwd: cwd.clone(),
    });
    let (mut custody, mut native) = anima_process::Custody::prepare(Path::new(&executable), args);
    if require_strong {
        custody
            .require_strong()
            .map_err(|error| error.to_string())?;
    }
    native
        .current_dir(&cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_clear()
        .envs(shell_env());
    let mut wrapped = CommandWrap::from(native);
    custody.wrap(&mut wrapped);
    if let Some(identity) = &identity {
        identity.revalidate()?;
    }
    let mut child = wrapped
        .spawn()
        .map_err(|error| format!("bash failed to start command: {error}"))?;
    custody.attach(child.as_ref());
    let (Some(stdout), Some(stderr)) = (child.stdout().take(), child.stderr().take()) else {
        custody.retire(&mut child).await;
        return Err("bash output could not be captured".to_string());
    };
    let mut stdout_task = tokio::spawn(capture_async(stdout, false));
    let mut stderr_task = tokio::spawn(capture_async(stderr, true));

    let deadline = tokio::time::Instant::now() + timeout;
    let poll = Duration::from_millis(10);
    let waited = loop {
        match tokio::time::timeout(poll, child.inner_mut().wait()).await {
            Ok(Ok(status)) => break Waited::Exited(status),
            Ok(Err(error)) => {
                custody.retire(&mut child).await;
                return Err(format!("bash failed while waiting for command: {error}"));
            }
            Err(_) => {}
        }
        if is_cancelled() {
            break Waited::Stopped;
        }
        if tokio::time::Instant::now() >= deadline {
            break Waited::TimedOut;
        }
    };
    let status = match waited {
        Waited::Exited(status) => status,
        Waited::Stopped | Waited::TimedOut => {
            // The answer does not wait for the capture tasks: a process that
            // escaped the tree may still hold the pipes.
            custody.retire(&mut child).await;
            return Ok(if matches!(waited, Waited::Stopped) {
                stopped()
            } else {
                timed_out()
            });
        }
    };

    // The shell exited. A descendant still holding its output pipes keeps
    // the capture open only until the deadline or a stop; then the tree is
    // retired so the call cannot hang past its timeout.
    let mut captured = (None, None);
    loop {
        if captured.0.is_none() && stdout_task.is_finished() {
            captured.0 = Some((&mut stdout_task).await.unwrap_or_default());
        }
        if captured.1.is_none() && stderr_task.is_finished() {
            captured.1 = Some((&mut stderr_task).await.unwrap_or_default());
        }
        if captured.0.is_some() && captured.1.is_some() {
            break;
        }
        if is_cancelled() {
            custody.retire(&mut child).await;
            return Ok(stopped());
        }
        if tokio::time::Instant::now() >= deadline {
            custody.retire(&mut child).await;
            return Ok(timed_out());
        }
        tokio::time::sleep(poll).await;
    }
    let (stdout_text, stderr_text) = (
        captured.0.unwrap_or_default(),
        captured.1.unwrap_or_default(),
    );

    let exit_code = status.code().unwrap_or(-1);
    let produced_output = !stdout_text.is_empty() || !stderr_text.is_empty();
    if custody.join_failed(exit_code, produced_output) {
        return Err("bash could not place the command in its process container".to_string());
    }
    let combined = if stdout_text.trim().is_empty() {
        stderr_text
    } else if stderr_text.trim().is_empty() {
        stdout_text
    } else {
        format!("{}\n{}", stdout_text.trim_end(), stderr_text.trim_end())
    };

    Ok(BashCommandResult {
        status: if exit_code == 0 { "success" } else { "error" },
        output: truncate_shell_output(&combined),
    })
}

#[cfg(windows)]
pub(super) fn execute_bash_command_owned(
    configured_root: Option<&Path>,
    command: &str,
    timeout_ms: u64,
    cwd: &str,
    run_cancel: Option<CancelSignal>,
    call_cancel: CancelSignal,
    profile_owner: Option<crate::profile_owner::ProfileOwner>,
    binding: Arc<CommandBinding>,
) -> Result<BashCommandResult, String> {
    let root = workspace_root_path("bash", configured_root)?;
    execute_windows_from_root_owned(
        &root,
        command,
        timeout_ms,
        cwd,
        run_cancel,
        Some(call_cancel),
        profile_owner,
        Some(binding),
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
    identity: Option<BoundShellLaunch>,
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
    binding: Option<Arc<CommandBinding>>,
) -> ShellResult {
    execute_windows_from_root_owned(
        root,
        command,
        timeout_ms,
        cwd,
        run_cancel,
        call_cancel,
        None,
        binding,
    )
}

#[cfg(windows)]
fn execute_windows_from_root_owned(
    root: &Path,
    command: &str,
    timeout_ms: u64,
    cwd: &str,
    run_cancel: Option<CancelSignal>,
    call_cancel: Option<CancelSignal>,
    profile_owner: Option<crate::profile_owner::ProfileOwner>,
    binding: Option<Arc<CommandBinding>>,
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
    let identity = binding.map(|binding| BoundShellLaunch {
        binding,
        executable: executable.clone(),
        flags: flags.clone(),
        command: command.to_string(),
        cwd: cwd.clone(),
    });
    let mut builder = Command::new(&executable);
    builder
        .args(flags)
        .arg(command)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_clear()
        .envs(shell_env());
    use process_wrap::std::{CommandWrap, CreationFlags, JobObject};
    let mut wrapped = CommandWrap::from(builder);
    let mut creation = CreationFlags(Default::default());
    // Documented CREATE_NO_WINDOW through the same maintained public shim as
    // M44/M46. JobObject adds temporary suspension and assigns before resume.
    creation.0 .0 = 0x08000000;
    wrapped.wrap(creation).wrap(JobObject);
    owned_windows_worker(fuse, move |sender, fuse| {
        let _profile_owner = profile_owner;
        if let Some(profile_owner) = &_profile_owner {
            if let Err(error) = profile_owner.ensure_owned() {
                let _ = sender.send(Err(error.to_string()));
                return;
            }
        }
        let mut owner = WindowsShellOwner {
            command: wrapped,
            child: None,
            readers: Vec::new(),
            stdout: Arc::new(Mutex::new(String::new())),
            stderr: Arc::new(Mutex::new(String::new())),
            wait_started: false,
            identity,
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
                    std::hint::black_box((&owner, &_profile_owner));
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
        if let Some(identity) = &self.identity {
            if let Err(error) = identity.revalidate() {
                return Ok(Err(error));
            }
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

/// Captures one stream line by line, keeping at most
/// `BASH_MAX_CAPTURE_BYTES`; the rest is drained and discarded.
#[cfg(not(windows))]
async fn capture_async<R>(reader: R, stderr: bool) -> String
where
    R: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::AsyncBufReadExt;
    let mut reader = tokio::io::BufReader::new(reader);
    let mut local = String::new();
    let mut truncated = false;
    let mut line = Vec::new();
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        if truncated {
            continue;
        }
        if line.last() == Some(&b'\n') {
            line.pop();
        }
        let text = String::from_utf8_lossy(&line);
        let prefix_len = if stderr { "[stderr] ".len() } else { 0 };
        if local.len() + prefix_len + text.len() + 1 > BASH_MAX_CAPTURE_BYTES {
            local.push_str("\n[output capture limit reached; further bytes discarded]\n");
            truncated = true;
            continue;
        }
        if stderr {
            local.push_str("[stderr] ");
        }
        local.push_str(&text);
        local.push('\n');
    }
    local
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
mod env_tests {
    use super::*;

    #[test]
    fn shell_env_excludes_anima_tokens() {
        let keys = [
            "ANIMA_SUPERVISOR_TOKEN",
            "ANIMA_SUPERVISOR_URL",
            "ANIMA_LOCAL_ADMIN_TOKEN",
        ];
        for key in keys {
            std::env::set_var(key, "secret");
        }
        let env = shell_env();
        for key in keys {
            std::env::remove_var(key);
        }
        for key in keys {
            assert!(
                !env.iter().any(|(name, _)| *name == key),
                "{key} must not reach agent shells"
            );
        }
    }
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

#[cfg(all(test, unix))]
mod custody_tests {
    use super::*;

    fn marker_alive(marker: &str) -> bool {
        std::process::Command::new("pgrep")
            .args(["-f", marker])
            .output()
            .is_ok_and(|output| output.status.success())
    }

    fn unique_sleep_marker() -> String {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos();
        format!("sleep 30.{}{}", std::process::id(), nanos % 100_000)
    }

    #[test]
    fn bash_timeout_retires_a_grandchild() {
        let root = tempfile::tempdir().unwrap();
        let marker = unique_sleep_marker();
        let result = execute_bash_command_from_root(
            root.path(),
            &format!("{marker} & sleep 5"),
            400,
            ".",
            None,
        )
        .unwrap();
        assert_eq!(result.output, "Command timed out after 400ms");
        assert!(!marker_alive(&marker), "the timeout left {marker} running");
    }

    #[test]
    fn bash_stop_retires_a_grandchild_and_answers_promptly() {
        let root = tempfile::tempdir().unwrap();
        let marker = unique_sleep_marker();
        let cancel = CancelSignal::new();
        let stopper = cancel.clone();
        let canceller = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            stopper.cancel();
        });
        let started = std::time::Instant::now();
        let result = execute_bash_command_from_root(
            root.path(),
            &format!("{marker} & sleep 20"),
            60_000,
            ".",
            Some(&cancel),
        )
        .unwrap();
        canceller.join().unwrap();
        assert_eq!(result.output, BASH_STOPPED);
        assert!(started.elapsed() < Duration::from_secs(10));
        assert!(!marker_alive(&marker), "the stop left {marker} running");
    }

    #[test]
    fn bash_sees_the_allowlist_and_pager_cat_but_not_daemon_secrets() {
        std::env::set_var("ANIMA_TEST_SECRET", "do-not-leak-bash");
        let root = tempfile::tempdir().unwrap();
        let result = execute_bash_command_from_root(root.path(), "env", 10_000, ".", None).unwrap();
        assert_eq!(result.status, "success", "{}", result.output);
        assert!(!result.output.contains("ANIMA_TEST_SECRET"));
        assert!(result.output.contains("PAGER=cat") && result.output.contains("GIT_PAGER=cat"));
    }

    #[test]
    fn bash_output_after_the_shell_exits_is_still_captured() {
        let root = tempfile::tempdir().unwrap();
        let result = execute_bash_command_from_root(
            root.path(),
            "echo out; echo err >&2; exit 4",
            10_000,
            ".",
            None,
        )
        .unwrap();
        assert_eq!(result.status, "error");
        assert_eq!(result.output, "out\n[stderr] err");
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
