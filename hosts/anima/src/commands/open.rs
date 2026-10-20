//! `anima` with no subcommand: starts Anima if needed and opens the console.

use std::io::{self, IsTerminal};
use std::process::ExitCode;

use super::lifecycle::ensure_running;

pub fn open() -> io::Result<ExitCode> {
    let (paths, _) = super::load()?;
    let cfg = super::first_launch::prepare(&paths)?;
    #[cfg(windows)]
    if launched_from_explorer() {
        return relaunch_detached();
    }
    let started = ensure_running(&paths, &cfg)?;
    let server_url = started.status().server_url.clone();
    let admin_token = std::fs::read_to_string(&paths.admin_token).ok();
    let url = console_url(&server_url, admin_token.as_deref().map(str::trim));
    // The address without its launch code, which is a secret.
    println!("Opening {server_url}/");
    if std::env::var_os("ANIMA_NO_OPEN").is_some_and(|value| value == "1") {
        return Ok(ExitCode::SUCCESS);
    }
    ::open::that(&url).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("could not open {server_url}/: {error}"),
        )
    })?;
    Ok(ExitCode::SUCCESS)
}

/// Explorer/Finder/menu launches have nowhere useful to print an error.
pub fn report_startup_error(error: &io::Error) {
    if std::env::var_os("ANIMA_NO_DIALOG").is_some_and(|value| value == "1") {
        return;
    }
    let desktop = !io::stderr().is_terminal();
    #[cfg(windows)]
    let desktop = desktop || launched_from_explorer();
    if desktop {
        rfd::MessageDialog::new()
            .set_title("Anima could not start")
            .set_description(format!(
                "{error}\n\nYour saved workspace has not been reset."
            ))
            .set_level(rfd::MessageLevel::Error)
            .show();
    }
}

/// The console's address, with a one-time launch code when there is an
/// admin token to sign it: only a console opened this way can stop, restart
/// or update Anima (`anima_daemon::console_session`).
pub fn console_url(server_url: &str, admin_token: Option<&str>) -> String {
    match admin_token.filter(|token| !token.is_empty()) {
        Some(token) => format!(
            "{server_url}/?{}={}",
            anima_daemon::console_session::LAUNCH_PARAM,
            anima_daemon::console_session::launch_code(token)
        ),
        None => format!("{server_url}/"),
    }
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

#[cfg(test)]
mod tests {
    use super::console_url;

    #[test]
    fn the_console_url_carries_a_launch_code() {
        let url = console_url("http://127.0.0.1:8787", Some("admin"));
        let code = url
            .strip_prefix("http://127.0.0.1:8787/?anima-login=")
            .expect("a launch code");
        assert_eq!(code.split('.').count(), 3, "{code}");
        assert_ne!(
            url,
            console_url("http://127.0.0.1:8787", Some("admin")),
            "one-time"
        );
        assert_eq!(
            console_url("http://127.0.0.1:8787", None),
            "http://127.0.0.1:8787/"
        );
        assert_eq!(
            console_url("http://127.0.0.1:8787", Some("")),
            "http://127.0.0.1:8787/"
        );
    }
}
