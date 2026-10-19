//! The two bearer tokens in the per-user folder: `control-token` guards the
//! supervisor's control API, `admin-token` is the server's local admin token.

use std::fmt::Write as _;
use std::io::{self, Write};
use std::path::Path;

use crate::paths::AnimaPaths;

/// Guards the supervisor's control API. No `Debug`: it is a secret.
#[derive(Clone)]
pub struct ControlToken(String);

/// The server's `ANIMA_LOCAL_ADMIN_TOKEN`. No `Debug`: it is a secret.
#[derive(Clone)]
pub struct AdminToken(String);

impl ControlToken {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AdminToken {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Reads both tokens, creating each missing (or empty) one: 32 random bytes,
/// hex, mode 0600 on Unix.
pub fn ensure(paths: &AnimaPaths) -> io::Result<(ControlToken, AdminToken)> {
    paths.ensure()?;
    let control = ensure_token(&paths.control_token)?;
    let admin = ensure_token(&paths.admin_token)?;
    Ok((ControlToken(control), AdminToken(admin)))
}

fn ensure_token(path: &Path) -> io::Result<String> {
    match std::fs::read_to_string(path) {
        Ok(text) if !text.trim().is_empty() => {
            restrict(path)?;
            return Ok(text.trim().to_string());
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let token = random_hex()?;
    write_private(path, &token)?;
    Ok(token)
}

fn random_hex() -> io::Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|error| io::Error::other(error.to_string()))?;
    let mut hex = String::with_capacity(64);
    for byte in bytes {
        let _ = write!(hex, "{byte:02x}");
    }
    Ok(hex)
}

/// Temp file in the same folder, then rename, so a reader never sees half a token.
pub(crate) fn write_private(path: &Path, contents: &str) -> io::Result<()> {
    let mut tmp_name = path.file_name().unwrap_or_default().to_os_string();
    tmp_name.push(format!(".tmp{}", std::process::id()));
    let tmp = path.with_file_name(tmp_name);
    let result = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&tmp)?;
        file.write_all(contents.as_bytes())?;
        file.sync_all()?;
        drop(file);
        restrict(&tmp)?;
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

fn restrict(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_created_once_and_private() {
        let home = tempfile::tempdir().unwrap();
        let paths = AnimaPaths::from_home(home.path());
        let (control, admin) = ensure(&paths).unwrap();
        for token in [control.as_str(), admin.as_str()] {
            assert_eq!(token.len(), 64);
            assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
        }
        assert_ne!(control.as_str(), admin.as_str());
        let (again, admin_again) = ensure(&paths).unwrap();
        assert_eq!(again.as_str(), control.as_str());
        assert_eq!(admin_again.as_str(), admin.as_str());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for path in [&paths.control_token, &paths.admin_token] {
                let mode = std::fs::metadata(path).unwrap().permissions().mode();
                assert_eq!(mode & 0o777, 0o600, "{path:?}");
            }
        }
    }

    #[test]
    fn an_empty_token_file_is_replaced() {
        let home = tempfile::tempdir().unwrap();
        let paths = AnimaPaths::from_home(home.path());
        paths.ensure().unwrap();
        std::fs::write(&paths.admin_token, "\n").unwrap();
        let (_, admin) = ensure(&paths).unwrap();
        assert_eq!(admin.as_str().len(), 64);
        assert_eq!(
            std::fs::read_to_string(&paths.admin_token).unwrap(),
            admin.as_str()
        );
    }
}
