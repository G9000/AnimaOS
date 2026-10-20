//! Start at login: XDG autostart on Linux, a macOS LaunchAgent, or Windows HKCU Run.

use std::io;
use std::path::Path;

use crate::paths::AnimaPaths;

#[cfg(any(target_os = "linux", test))]
#[cfg_attr(all(test, not(target_os = "linux")), allow(dead_code))]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(windows)]
pub mod windows;

/// Test-only switch: `ANIMA_AUTOSTART_DRY_RUN=1` keeps `anima autostart` away
/// from the real launchd (macOS: the plist is still written under the home
/// folder, but `launchctl` is not run) and from the real Run key (Windows: a
/// scratch key under `HKCU\Software\AnimaOS-DryRun` is used).
pub const DRY_RUN_ENV: &str = "ANIMA_AUTOSTART_DRY_RUN";

#[cfg(any(target_os = "macos", windows))]
fn dry_run() -> bool {
    std::env::var_os(DRY_RUN_ENV).is_some_and(|value| value == "1")
}

pub trait Autostart {
    /// Makes the supervisor (`exe supervise --background`) start at login.
    fn enable(&self, exe: &Path) -> io::Result<()>;
    /// Stops it starting at login. Succeeds when it was not enabled.
    fn disable(&self) -> io::Result<()>;
    fn is_enabled(&self) -> io::Result<bool>;
}

#[cfg(target_os = "macos")]
pub fn platform(paths: &AnimaPaths) -> Box<dyn Autostart> {
    Box::new(macos::MacAutostart::for_paths(paths, dry_run()))
}

#[cfg(windows)]
pub fn platform(_paths: &AnimaPaths) -> Box<dyn Autostart> {
    Box::new(if dry_run() {
        windows::WindowsAutostart::with_key_path(windows::DRY_RUN_KEY)
    } else {
        windows::WindowsAutostart::new()
    })
}

#[cfg(target_os = "linux")]
pub fn platform(paths: &AnimaPaths) -> Box<dyn Autostart> {
    Box::new(linux::LinuxAutostart::for_paths(paths))
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub fn platform(_paths: &AnimaPaths) -> Box<dyn Autostart> {
    Box::new(Unsupported)
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
struct Unsupported;

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
impl Autostart for Unsupported {
    fn enable(&self, _exe: &Path) -> io::Result<()> {
        Err(unsupported())
    }
    fn disable(&self) -> io::Result<()> {
        Err(unsupported())
    }
    fn is_enabled(&self) -> io::Result<bool> {
        Ok(false)
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn unsupported() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "start at login is not available on this system",
    )
}
