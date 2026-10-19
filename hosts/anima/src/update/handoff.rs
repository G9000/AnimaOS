//! The hand-off between an old binary and the new one it installed.
//!
//! After swapping in a new binary, the old one writes `run/update-pending.json`
//! ([`install`]) and re-executes ([`restart_into`]). The new binary's supervisor
//! reads that file at start ([`on_start`]): its server must be running within
//! [`HEALTH_TIMEOUT`], or it puts the old binary back ([`roll_back`]), leaves
//! `run/update-failed.json`, and re-executes that. The old binary then reports
//! the failed update ([`HandoffOutcome::RolledBack`]).

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::UpdateError;
use crate::paths::AnimaPaths;
use crate::supervisor::tokens::write_private;

const PENDING_FILE: &str = "update-pending.json";
const FAILED_FILE: &str = "update-failed.json";

/// How long a new version gets to have its server running.
pub const HEALTH_TIMEOUT: Duration = Duration::from_secs(60);

/// `run/update-pending.json`: an installed update not yet confirmed healthy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingUpdate {
    pub from: String,
    pub to: String,
    /// The previous binary, kept for a rollback.
    pub old: PathBuf,
}

/// `run/update-failed.json`: an update that was rolled back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct FailedUpdate {
    to: String,
    error: String,
}

/// What a starting supervisor has to finish of an update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandoffOutcome {
    Nothing,
    /// This is the new version: its server must be running within
    /// [`HEALTH_TIMEOUT`], then [`confirm`]; otherwise [`roll_back`].
    AwaitHealth(PendingUpdate),
    /// This version was put back after an update failed: show `last_error`.
    RolledBack {
        last_error: String,
    },
}

/// The update error the restored version shows.
pub fn rolled_back_message(to: &str) -> String {
    format!("Update to v{to} failed to start; rolled back")
}

/// Reads (and consumes) the hand-off files for a supervisor of
/// `current_version` that holds the control port. A pending update for another
/// version, or an unreadable file, is dropped; a failure marker is shown once.
pub fn on_start(paths: &AnimaPaths, current_version: &str) -> HandoffOutcome {
    let failed: Option<FailedUpdate> = take(&failed_path(paths));
    let pending: Option<PendingUpdate> = read(&pending_path(paths));
    let outcome = decide(pending, failed, current_version);
    if !matches!(outcome, HandoffOutcome::AwaitHealth(_)) {
        let _ = remove(&pending_path(paths));
    }
    outcome
}

fn decide(
    pending: Option<PendingUpdate>,
    failed: Option<FailedUpdate>,
    current_version: &str,
) -> HandoffOutcome {
    match (pending, failed) {
        (Some(pending), _) if pending.to == current_version => HandoffOutcome::AwaitHealth(pending),
        (_, Some(failed)) => HandoffOutcome::RolledBack {
            last_error: rolled_back_message(&failed.to),
        },
        _ => HandoffOutcome::Nothing,
    }
}

/// The new version's server is running: the update is done.
pub fn confirm(paths: &AnimaPaths) -> io::Result<()> {
    remove(&pending_path(paths))
}

/// Swaps `binary` in for `exe` with `swap` (the running executable:
/// [`super::swap`]), then records the pending update. If it can't be recorded
/// the old binary is put back, since nothing would check the new one.
pub fn install(
    paths: &AnimaPaths,
    exe: &Path,
    binary: &[u8],
    from: &str,
    to: &str,
    swap: impl FnOnce(&[u8]) -> Result<PathBuf, UpdateError>,
) -> Result<PendingUpdate, UpdateError> {
    let old = swap(binary)?;
    let pending = PendingUpdate {
        from: from.to_string(),
        to: to.to_string(),
        old,
    };
    if let Err(error) = write_json(&pending_path(paths), &pending) {
        let _ = super::rollback(exe, &pending.old);
        return Err(error.into());
    }
    Ok(pending)
}

/// Undoes an [`install`] whose restart failed: the old binary back, no pending update.
pub fn undo(paths: &AnimaPaths, pending: &PendingUpdate, exe: &Path) -> Result<(), UpdateError> {
    let _ = remove(&pending_path(paths));
    super::rollback(exe, &pending.old)
}

/// The new version did not come up: puts `pending.old` back at `exe` with
/// `rollback` ([`super::rollback`]) and leaves the failure, with `error`, for the
/// old version to show once it is re-executed. The pending update is dropped
/// either way, so a failed rollback is not retried at every start.
pub fn roll_back(
    paths: &AnimaPaths,
    pending: &PendingUpdate,
    exe: &Path,
    error: &str,
    rollback: impl FnOnce(&Path, &Path) -> Result<(), UpdateError>,
) -> Result<(), UpdateError> {
    let restored = rollback(exe, &pending.old);
    let _ = remove(&pending_path(paths));
    restored?;
    let failed = FailedUpdate {
        to: pending.to.clone(),
        error: error.to_string(),
    };
    write_json(&failed_path(paths), &failed)?;
    Ok(())
}

/// Replaces this process with `exe`, run with this process's arguments and
/// environment. On Unix it is `exec`, so the process (and the launchd job
/// around it) carries on; on Windows `exe` is started detached and this
/// process exits. Returns only when that failed.
///
/// The server child must already be stopped. Its stdin lifeline is
/// close-on-exec, so even a server still running sees end of input and exits.
pub fn restart_into(exe: &Path) -> io::Error {
    let mut command = std::process::Command::new(exe);
    command.args(std::env::args_os().skip(1));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.exec()
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use std::process::Stdio;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command
            .creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        match command.spawn() {
            Ok(_) => std::process::exit(0),
            Err(error) => error,
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = command;
        io::Error::new(
            io::ErrorKind::Unsupported,
            "restarting is not supported here",
        )
    }
}

/// An update hand-off is under way: the new process may have to wait for the
/// old one to let go of the control port (Windows).
pub fn under_way(paths: &AnimaPaths) -> bool {
    pending_path(paths).exists() || failed_path(paths).exists()
}

fn pending_path(paths: &AnimaPaths) -> PathBuf {
    paths.run.join(PENDING_FILE)
}

fn failed_path(paths: &AnimaPaths) -> PathBuf {
    paths.run.join(FAILED_FILE)
}

fn write_json(path: &Path, value: &impl Serialize) -> io::Result<()> {
    let text = serde_json::to_string_pretty(value).map_err(io::Error::other)?;
    write_private(path, &text)
}

/// The file's contents; an unreadable file reads as none.
fn read<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// Reads the file and removes it.
fn take<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    let value = read(path);
    let _ = remove(path);
    value
}

fn remove(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}
