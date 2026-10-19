//! The `AnimaOS` value under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.

use std::io;
use std::path::Path;

use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};
use winreg::RegKey;

use super::Autostart;

/// The key Windows reads at login.
pub const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
/// A scratch key for `ANIMA_AUTOSTART_DRY_RUN=1` (test-only).
pub const DRY_RUN_KEY: &str = r"Software\AnimaOS-DryRun\Run";
/// The value in the key.
pub const VALUE_NAME: &str = "AnimaOS";

pub struct WindowsAutostart {
    key_path: String,
}

impl WindowsAutostart {
    pub fn new() -> Self {
        Self::with_key_path(RUN_KEY)
    }

    /// Uses another key than the real one. For tests.
    pub fn with_key_path(key_path: &str) -> Self {
        Self {
            key_path: key_path.to_string(),
        }
    }
}

/// The command Windows runs at login.
pub fn command(exe: &Path) -> String {
    format!("\"{}\" supervise --background", exe.display())
}

fn not_found(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::NotFound
}

impl Autostart for WindowsAutostart {
    fn enable(&self, exe: &Path) -> io::Result<()> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let (key, _) = hkcu.create_subkey(&self.key_path)?;
        key.set_value(VALUE_NAME, &command(exe))
    }

    fn disable(&self) -> io::Result<()> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let key = match hkcu.open_subkey_with_flags(&self.key_path, KEY_SET_VALUE) {
            Ok(key) => key,
            Err(error) if not_found(&error) => return Ok(()),
            Err(error) => return Err(error),
        };
        match key.delete_value(VALUE_NAME) {
            Err(error) if !not_found(&error) => Err(error),
            _ => Ok(()),
        }
    }

    fn is_enabled(&self) -> io::Result<bool> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let key = match hkcu.open_subkey(&self.key_path) {
            Ok(key) => key,
            Err(error) if not_found(&error) => return Ok(false),
            Err(error) => return Err(error),
        };
        match key.get_value::<String, _>(VALUE_NAME) {
            Ok(_) => Ok(true),
            Err(error) if not_found(&error) => Ok(false),
            Err(error) => Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Removes the scratch key when the test ends, pass or fail.
    struct Scratch(String);

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(&self.0);
        }
    }

    #[test]
    fn enable_then_is_enabled_then_disable() {
        let key_path = format!(r"Software\AnimaOS-Test\{}\Run", std::process::id());
        let _scratch = Scratch(format!(r"Software\AnimaOS-Test\{}", std::process::id()));
        let autostart = WindowsAutostart::with_key_path(&key_path);
        let exe = Path::new(r"C:\Program Files\Anima\anima.exe");

        assert!(!autostart.is_enabled().unwrap());
        autostart.enable(exe).unwrap();
        assert!(autostart.is_enabled().unwrap());

        let key = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(&key_path)
            .unwrap();
        let value: String = key.get_value(VALUE_NAME).unwrap();
        assert_eq!(
            value,
            r#""C:\Program Files\Anima\anima.exe" supervise --background"#
        );

        autostart.disable().unwrap();
        assert!(!autostart.is_enabled().unwrap());
        // Disabling twice is fine.
        autostart.disable().unwrap();
    }
}
