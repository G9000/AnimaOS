//! `anima autostart on|off`.

use std::io;
use std::process::ExitCode;

use crate::autostart;
use crate::cli::OnOff;

pub fn autostart(on_off: OnOff) -> io::Result<ExitCode> {
    let (paths, mut cfg) = super::load()?;
    let platform = autostart::platform(&paths);
    // The system first: a failure leaves the config as it was.
    match on_off {
        OnOff::On => {
            platform.enable(&current_exe()?)?;
            println!("Anima will start when you log in");
        }
        OnOff::Off => {
            platform.disable()?;
            println!("Anima will no longer start when you log in");
        }
    }
    cfg.autostart = on_off == OnOff::On;
    cfg.save(&paths.config_file)?;
    Ok(ExitCode::SUCCESS)
}

/// This executable, with links resolved so a login does not depend on one.
fn current_exe() -> io::Result<std::path::PathBuf> {
    let exe = std::env::current_exe()?;
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
