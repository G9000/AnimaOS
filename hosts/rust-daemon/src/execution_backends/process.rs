//! Owned children run on a service runtime and are always retired before settlement.
use anima_core::CancelSignal;
use anima_process::Custody;
use process_wrap::tokio::CommandWrap;
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};

pub(super) const CAPTURE_BYTES: usize = 8192;
fn prepare_command(executable: &Path, mut args: Vec<String>) -> (Custody, tokio::process::Command) {
    #[cfg(windows)]
    if executable
        .file_name()
        .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("cmd.exe"))
        && args.len() >= 2
        && args[args.len() - 2].eq_ignore_ascii_case("/c")
    {
        use std::os::windows::process::CommandExt;
        let command = args.pop().unwrap();
        // cmd parses its own command line rather than normal Windows argv
        // quoting rules. Keep /c command syntax intact inside one outer pair.
        let executable = PathBuf::from(executable.to_string_lossy().replace('/', "\\"));
        let (custody, mut native) = Custody::prepare(&executable, args);
        native.as_std_mut().raw_arg(format!("\"{command}\""));
        return (custody, native);
    }
    #[cfg(not(windows))]
    let _ = &mut args;
    Custody::prepare(executable, args)
}
pub(super) struct ProcessOutput {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub stop: Option<super::ExecutionStatus>,
    pub cleanup: bool,
}
pub(super) fn clean_environment(command: &mut tokio::process::Command) {
    command.env_clear();
    for name in [
        "PATH",
        "SystemRoot",
        "WINDIR",
        "TEMP",
        "TMP",
        "HOME",
        "USERPROFILE",
        "LANG",
        "LC_ALL",
    ] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    command.env("PAGER", "cat");
    #[cfg(test)]
    if let Some(value) = std::env::var_os("ANIMA_EXECUTION_FAKE_ROOT") {
        command.env("ANIMA_EXECUTION_FAKE_ROOT", value);
    }
}
pub(super) async fn capture(mut reader: impl AsyncRead + Unpin) -> String {
    let mut captured = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        match reader.read(&mut buffer).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let keep = n.min(CAPTURE_BYTES.saturating_sub(captured.len()));
                captured.extend_from_slice(&buffer[..keep]);
            }
        }
    }
    String::from_utf8_lossy(&captured).into_owned()
}
pub(super) fn local_cwd(root: &Path, cwd: &str) -> Result<PathBuf, String> {
    let path = Path::new(cwd);
    if path.is_absolute()
        || path.components().any(|c| {
            matches!(
                c,
                std::path::Component::ParentDir
                    | std::path::Component::Prefix(_)
                    | std::path::Component::RootDir
            )
        })
    {
        return Err("Execution cwd must stay inside the configured workspace".into());
    }
    let target =
        std::fs::canonicalize(root.join(path)).map_err(|_| "Execution cwd does not exist")?;
    if !target.starts_with(root) || !target.is_dir() {
        return Err("Execution cwd escapes the configured workspace".into());
    }
    Ok(target)
}
pub(super) async fn run(
    executable: &Path,
    args: Vec<String>,
    cwd: Option<&Path>,
    input: Option<Vec<u8>>,
    timeout: Duration,
    cancel: Option<&CancelSignal>,
    call: &CancelSignal,
    require_strong: bool,
) -> Result<ProcessOutput, String> {
    run_with_binding(
        executable,
        args,
        cwd,
        input,
        timeout,
        cancel,
        call,
        require_strong,
        None,
    )
    .await
}

pub(super) async fn run_bound(
    executable: &Path,
    args: Vec<String>,
    cwd: Option<&Path>,
    input: Option<Vec<u8>>,
    timeout: Duration,
    cancel: Option<&CancelSignal>,
    call: &CancelSignal,
    require_strong: bool,
    binding: &crate::tools::command_binding::CommandBinding,
) -> Result<ProcessOutput, String> {
    run_with_binding(
        executable,
        args,
        cwd,
        input,
        timeout,
        cancel,
        call,
        require_strong,
        Some(binding),
    )
    .await
}

async fn run_with_binding(
    executable: &Path,
    args: Vec<String>,
    cwd: Option<&Path>,
    input: Option<Vec<u8>>,
    timeout: Duration,
    cancel: Option<&CancelSignal>,
    call: &CancelSignal,
    require_strong: bool,
    binding: Option<&crate::tools::command_binding::CommandBinding>,
) -> Result<ProcessOutput, String> {
    if call.is_cancelled() || cancel.is_some_and(CancelSignal::is_cancelled) {
        return Ok(ProcessOutput {
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            stop: Some(super::ExecutionStatus::Cancelled),
            cleanup: true,
        });
    }
    let (mut custody, mut native) = prepare_command(executable, args);
    if require_strong {
        custody
            .require_strong()
            .map_err(|error| error.to_string())?;
    }
    native
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(cwd) = cwd {
        native.current_dir(cwd);
    }
    clean_environment(&mut native);
    let mut wrapped = CommandWrap::from(native);
    custody.wrap(&mut wrapped);
    if let Some(binding) = binding {
        binding.revalidate()?;
    }
    let mut child = match wrapped.spawn() {
        Ok(child) => child,
        Err(error) => {
            return Ok(ProcessOutput {
                exit_code: None,
                stdout: String::new(),
                stderr: format!("Configured execution process could not start: {error}"),
                stop: None,
                cleanup: false,
            })
        }
    };
    custody.attach(child.as_ref());
    let (Some(stdout), Some(stderr)) = (child.stdout().take(), child.stderr().take()) else {
        let cleanup = custody.retire(&mut child).await;
        return Ok(ProcessOutput {
            exit_code: None,
            stdout: String::new(),
            stderr: "Execution output capture unavailable".into(),
            stop: None,
            cleanup,
        });
    };
    let output = tokio::spawn(capture(stdout));
    let errors = tokio::spawn(capture(stderr));
    if let Some(input) = input {
        if let Some(mut stdin) = child.stdin().take() {
            tokio::spawn(async move {
                let _ = stdin.write_all(&input).await;
                let _ = stdin.shutdown().await;
            });
        }
    }
    let deadline = tokio::time::Instant::now() + timeout;
    let mut stop = None;
    let status = loop {
        if call.is_cancelled() || cancel.is_some_and(CancelSignal::is_cancelled) {
            stop = Some(super::ExecutionStatus::Cancelled);
            break None;
        }
        if tokio::time::Instant::now() >= deadline {
            stop = Some(super::ExecutionStatus::TimedOut);
            break None;
        }
        match child.inner_mut().try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {}
            Err(_) => break None,
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    // Even a normally exiting leader can have descendants. Retire the exact tree.
    let mut cleanup = custody.retire(&mut child).await;
    let stdout = match tokio::time::timeout(Duration::from_secs(2), output).await {
        Ok(Ok(v)) => v,
        _ => {
            cleanup = false;
            String::new()
        }
    };
    let stderr = match tokio::time::timeout(Duration::from_secs(2), errors).await {
        Ok(Ok(v)) => v,
        _ => {
            cleanup = false;
            String::new()
        }
    };
    let exit_code = status.and_then(|s| s.code());
    let cleanup = cleanup
        && !custody.join_failed(
            exit_code.unwrap_or(-1),
            !stdout.is_empty() || !stderr.is_empty(),
        );
    Ok(ProcessOutput {
        exit_code,
        stdout,
        stderr,
        stop,
        cleanup,
    })
}
