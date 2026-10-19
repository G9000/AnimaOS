//! Putting a verified binary in place, keeping the previous one for rollback.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use super::UpdateError;

/// Replace the running executable with `new_binary`, keeping a copy of the
/// current one at `{exe}.old`. Returns the `.old` path.
pub fn swap(new_binary: &[u8]) -> Result<PathBuf, UpdateError> {
    let exe = running_exe()?;
    let old = keep_old(&exe)?;
    let staged = stage(new_binary, &exe)?;
    let replaced = self_replace::self_replace(&staged);
    let _ = fs::remove_file(&staged);
    replaced?;
    warn_unless_executable(&exe, ensure_executable);
    Ok(old)
}

/// [`swap`] for a binary that is not the one running (tests, or a stopped install).
#[cfg_attr(not(test), allow(dead_code))]
pub fn swap_at(new_binary: &[u8], target: &Path) -> Result<PathBuf, UpdateError> {
    replace_at(new_binary, target, ensure_executable)
}

/// [`swap_at`], with how the exec bit is set.
fn replace_at(
    new_binary: &[u8],
    target: &Path,
    make_executable: impl FnOnce(&Path) -> io::Result<()>,
) -> Result<PathBuf, UpdateError> {
    let old = keep_old(target)?;
    let staged = stage(new_binary, target)?;
    if let Err(error) = fs::rename(&staged, target) {
        let _ = fs::remove_file(&staged);
        return Err(error.into());
    }
    warn_unless_executable(target, make_executable);
    Ok(old)
}

/// Put `old` (the copy [`swap`] kept) back at `current_exe`, consuming it.
pub fn rollback(current_exe: &Path, old: &Path) -> Result<(), UpdateError> {
    let is_running = match (running_exe(), current_exe.canonicalize()) {
        (Ok(running), Ok(current)) => running == current,
        _ => false,
    };
    if is_running {
        // Windows cannot overwrite a running executable in place; self-replace can.
        self_replace::self_replace(old)?;
        fs::remove_file(old)?;
    } else {
        fs::rename(old, current_exe)?;
    }
    warn_unless_executable(current_exe, ensure_executable);
    Ok(())
}

/// The binary at `path` is already in place (staged with the exec bit, and
/// with the permissions of the one it replaced), so a failure to set the bit
/// again is a warning: answering with an error would report a swap that did
/// happen as one that did not.
fn warn_unless_executable(path: &Path, make_executable: impl FnOnce(&Path) -> io::Result<()>) {
    if let Err(error) = make_executable(path) {
        eprintln!(
            "anima: warning: {} is in place, but its exec bit could not be set: {error}",
            path.display()
        );
    }
}

fn running_exe() -> io::Result<PathBuf> {
    std::env::current_exe()?.canonicalize()
}

/// Where [`swap`] keeps the previous binary: `anima` → `anima.old`,
/// `anima.exe` → `anima.exe.old`.
pub(super) fn old_path(exe: &Path) -> PathBuf {
    let mut name = exe.file_name().unwrap_or_default().to_os_string();
    name.push(".old");
    exe.with_file_name(name)
}

fn keep_old(exe: &Path) -> io::Result<PathBuf> {
    let old = old_path(exe);
    fs::copy(exe, &old)?;
    Ok(old)
}

/// Write `bytes` to a new file next to `exe` (same file system, so the final
/// rename is atomic), with the same permissions as `exe` plus its exec bit.
fn stage(bytes: &[u8], exe: &Path) -> io::Result<PathBuf> {
    let mut name = std::ffi::OsString::from(".");
    name.push(exe.file_name().unwrap_or_default());
    name.push(format!(".new-{}", std::process::id()));
    let staged = exe.with_file_name(name);
    let written = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staged)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::set_permissions(&staged, fs::metadata(exe)?.permissions())?;
        ensure_executable(&staged)
    })();
    match written {
        Ok(()) => Ok(staged),
        Err(error) => {
            let _ = fs::remove_file(&staged);
            Err(error)
        }
    }
}

#[cfg(unix)]
fn ensure_executable(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = fs::metadata(path)?.permissions();
    if permissions.mode() & 0o100 == 0 {
        permissions.set_mode(permissions.mode() | 0o100);
        fs::set_permissions(path, permissions)?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn ensure_executable(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_exec_bit_failure_after_the_rename_is_still_installed() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("anima");
        fs::write(&exe, b"old").unwrap();
        let old = replace_at(b"new", &exe, |_| Err(io::Error::other("read-only"))).unwrap();
        assert_eq!(old, old_path(&exe));
        assert_eq!(fs::read(&exe).unwrap(), b"new");
        assert_eq!(fs::read(&old).unwrap(), b"old");
    }
}
