//! `anima` with no subcommand: starts Anima if needed and opens the console.

use std::io;
use std::process::ExitCode;

use super::lifecycle::ensure_running;

pub fn open() -> io::Result<ExitCode> {
    let (paths, cfg) = super::load()?;
    if let Some(code) = super::refuse_unless_set_up(&cfg) {
        return Ok(code);
    }
    #[cfg(windows)]
    if launched_from_explorer() {
        return relaunch_detached();
    }
    let started = ensure_running(&paths, &cfg)?;
    let url = format!("{}/", started.status().server_url);
    println!("Opening {url}");
    ::open::that(&url)
        .map_err(|error| io::Error::new(error.kind(), format!("could not open {url}: {error}")))?;
    Ok(ExitCode::SUCCESS)
}

/// Set on the relaunched copy, so it does not relaunch itself in turn.
#[cfg(windows)]
pub const DETACHED_ENV: &str = "ANIMA_DETACHED";

/// Whether this process still runs attached to a console it should leave:
/// it is not the detached copy [`relaunch_detached`] started.
#[cfg(windows)]
pub fn needs_detaching() -> bool {
    std::env::var_os(DETACHED_ENV).is_none()
}

/// Whether a double-click in Explorer started this process, which would
/// leave a console window open for as long as it runs.
#[cfg(windows)]
fn launched_from_explorer() -> bool {
    use sysinfo::{get_current_pid, ProcessesToUpdate, System};
    if std::env::var_os(DETACHED_ENV).is_some() {
        return false;
    }
    let Ok(pid) = get_current_pid() else {
        return false;
    };
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
    let Some(parent) = system.process(pid).and_then(|process| process.parent()) else {
        return false;
    };
    system.refresh_processes(ProcessesToUpdate::Some(&[parent]), true);
    system
        .process(parent)
        .is_some_and(|process| process.name().eq_ignore_ascii_case("explorer.exe"))
}

/// Runs this executable again, with the same arguments, without a console,
/// then exits.
#[cfg(windows)]
pub fn relaunch_detached() -> io::Result<ExitCode> {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut command = Command::new(std::env::current_exe()?);
    if let Ok(paths) = crate::paths::AnimaPaths::discover() {
        if paths.data.is_dir() {
            command.current_dir(&paths.data);
        }
    }
    command
        .args(std::env::args_os().skip(1))
        .env(DETACHED_ENV, "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW)
        .spawn()?;
    Ok(ExitCode::SUCCESS)
}
