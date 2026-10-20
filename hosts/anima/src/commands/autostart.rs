//! `anima autostart on|off`.

use std::io;
use std::process::ExitCode;

use crate::autostart;
use crate::cli::OnOff;
use crate::config::Config;
use crate::paths::AnimaPaths;

pub fn autostart(on_off: OnOff) -> io::Result<ExitCode> {
    let (paths, _) = super::load()?;
    let on = on_off == OnOff::On;
    apply(&paths, on)?;
    if on {
        println!("Anima will start when you log in");
    } else {
        println!("Anima will no longer start when you log in");
    }
    Ok(ExitCode::SUCCESS)
}

/// Turns start at login on or off, and records it in `config.toml`. The
/// tray's "Start at login" does the same.
pub fn apply(paths: &AnimaPaths, on: bool) -> io::Result<()> {
    if std::env::var_os("ANIMA_HOME").is_some()
        && !std::env::var_os(autostart::DRY_RUN_ENV).is_some_and(|value| value == "1")
    {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Start at login cannot be changed for an alternate ANIMA_HOME profile. Launch this profile explicitly instead.",
        ));
    }
    let mut cfg = Config::load(&paths.config_file)?;
    let platform = autostart::platform(paths);
    // The system first: a failure leaves the config as it was.
    if on {
        platform.enable(&current_exe()?)?;
    } else {
        platform.disable()?;
    }
    cfg.autostart = on;
    cfg.save(&paths.config_file)
}

/// This executable, with links resolved so a login does not depend on one.
fn current_exe() -> io::Result<std::path::PathBuf> {
    let exe = crate::paths::launcher_executable()?;
    let exe = exe.canonicalize().unwrap_or(exe);
    Ok(without_verbatim_prefix(exe))
}

/// `canonicalize` gives `\\?\C:\…` on Windows; the Run value reads better
/// (and old shells cope better) without the prefix.
fn without_verbatim_prefix(path: std::path::PathBuf) -> std::path::PathBuf {
    match path.to_str().and_then(|text| text.strip_prefix(r"\\?\")) {
        Some(plain) if !plain.starts_with("UNC\\") => plain.into(),
        _ => path,
    }
}
