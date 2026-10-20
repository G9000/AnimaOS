//! Per-user XDG desktop autostart, without a system service or shell command.

use std::io;
use std::path::{Path, PathBuf};

use super::Autostart;
use crate::paths::AnimaPaths;

const ENTRY_NAME: &str = "dev.animaos.anima.desktop";

pub struct LinuxAutostart {
    config_home: PathBuf,
}

impl LinuxAutostart {
    pub fn for_paths(paths: &AnimaPaths) -> Self {
        // AnimaPaths also reflects an explicit ANIMA_HOME profile, so the
        // fallback stays within that profile rather than the real home.
        let home = paths.root.parent().unwrap_or(&paths.root);
        let xdg = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from);
        Self::new(config_home(home, xdg.as_deref()))
    }

    fn new(config_home: PathBuf) -> Self {
        Self { config_home }
    }

    fn entry_path(&self) -> io::Result<PathBuf> {
        if !self.config_home.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the autostart configuration directory must be absolute",
            ));
        }
        Ok(self.config_home.join("autostart").join(ENTRY_NAME))
    }
}

fn config_home(home: &Path, xdg: Option<&Path>) -> PathBuf {
    xdg.filter(|path| path.is_absolute())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| home.join(".config"))
}

fn desktop_entry(exe: &Path) -> io::Result<String> {
    let executable = exe.to_str().filter(|value| {
        exe.is_absolute() && !value.contains('=') && !value.chars().any(char::is_control)
    });
    let executable = executable.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "the executable must be an absolute UTF-8 path without control characters or '='",
        )
    })?;
    // TryExec is a string path, not an Exec command line: it has no surrounding
    // quotes or field codes. The desktop ignores the entry after uninstall.
    let try_exec = executable.replace('\\', "\\\\");
    Ok(format!(
        "[Desktop Entry]\nType=Application\nName=Anima\n\
         Exec={} supervise --background\nTryExec={try_exec}\nTerminal=false\n",
        quoted_exec(executable)
    ))
}

fn quoted_exec(exe: &str) -> String {
    // Desktop Entry Exec has two escaping passes: string unescaping, then
    // argument unquoting. No shell interprets this command. A literal percent
    // must also be doubled to keep it out of desktop field-code expansion.
    let mut argument = String::from("\"");
    for character in exe.chars() {
        match character {
            '\\' => argument.push_str(r"\\\\"),
            '"' | '$' | '`' => {
                argument.push_str(r"\\");
                argument.push(character);
            }
            '%' => argument.push_str("%%"),
            _ => argument.push(character),
        }
    }
    argument.push('"');
    argument
}

impl Autostart for LinuxAutostart {
    fn enable(&self, exe: &Path) -> io::Result<()> {
        let entry = desktop_entry(exe)?;
        let path = self.entry_path()?;
        std::fs::create_dir_all(path.parent().expect("entry has an autostart directory"))?;
        std::fs::write(path, entry)
    }

    fn disable(&self) -> io::Result<()> {
        match std::fs::remove_file(self.entry_path()?) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        }
    }

    fn is_enabled(&self) -> io::Result<bool> {
        match std::fs::metadata(self.entry_path()?) {
            Ok(metadata) => Ok(metadata.is_file()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_config_uses_only_absolute_xdg_override() {
        let home = tempfile::tempdir().unwrap();
        let xdg = home.path().join("custom-config");
        assert_eq!(config_home(home.path(), Some(&xdg)), xdg);
        assert_eq!(
            config_home(home.path(), Some(Path::new("relative-config"))),
            home.path().join(".config")
        );
        assert_eq!(config_home(home.path(), None), home.path().join(".config"));
    }

    #[test]
    fn linux_anima_home_profile_keeps_fallback_config_within_profile() {
        let home = tempfile::tempdir().unwrap();
        let paths = AnimaPaths::from_home(home.path());
        assert_eq!(
            config_home(paths.root.parent().unwrap(), None),
            home.path().join(".config")
        );
    }

    #[test]
    fn linux_exec_quotes_reserved_characters_without_shell_expansion() {
        assert_eq!(
            quoted_exec("/opt/Anima OS/anima"),
            r#""/opt/Anima OS/anima""#
        );
        assert_eq!(
            quoted_exec("/opt/Anima \"beta\"/$`%/anima\\"),
            r#""/opt/Anima \\"beta\\"/\\$\\`%%/anima\\\\""#
        );
    }

    #[test]
    fn linux_enable_writes_user_entry_then_disable_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config");
        let autostart = LinuxAutostart::new(config.clone());
        let exe = dir.path().join("Anima OS").join("anima");
        assert!(!autostart.is_enabled().unwrap());
        autostart.enable(&exe).unwrap();
        assert!(autostart.is_enabled().unwrap());
        let entry = std::fs::read_to_string(autostart.entry_path().unwrap()).unwrap();
        assert!(entry.starts_with("[Desktop Entry]\nType=Application\n"));
        assert!(entry.contains("\nName=Anima\n"));
        assert!(entry.contains(" supervise --background\n"));
        assert!(entry.contains("\nTerminal=false\n"));
        assert_eq!(entry.matches("\nExec=").count(), 1);
        assert!(entry.lines().any(|line| line.starts_with("TryExec=")));
        autostart.enable(&exe).unwrap();
        assert_eq!(
            std::fs::read_to_string(autostart.entry_path().unwrap()).unwrap(),
            entry
        );
        autostart.disable().unwrap();
        autostart.disable().unwrap();
        assert!(!autostart.is_enabled().unwrap());
        assert!(config.is_dir());
    }

    #[test]
    fn linux_tryexec_is_an_unquoted_literal_path() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("Anima \"beta\" $`% executable");
        let entry = desktop_entry(&exe).unwrap();
        let expected = format!("TryExec={}", exe.to_str().unwrap().replace('\\', "\\\\"));
        assert!(entry.lines().any(|line| line == expected));
    }

    #[test]
    fn linux_rejects_unrepresentable_executable_paths_before_creating_autostart() {
        let dir = tempfile::tempdir().unwrap();
        let autostart = LinuxAutostart::new(dir.path().join("config"));
        for exe in [
            PathBuf::from("relative/anima"),
            dir.path().join("anima\nHidden=false"),
            dir.path().join("anima\rExec=unexpected"),
            dir.path().join("anima=unexpected"),
        ] {
            let error = autostart.enable(&exe).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{exe:?}");
        }
        assert!(!autostart.config_home.exists());
    }

    #[test]
    fn linux_refuses_relative_config_directory() {
        let autostart = LinuxAutostart::new(PathBuf::from("relative-config"));
        let exe = std::env::current_exe().unwrap();
        assert_eq!(
            autostart.enable(&exe).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(
            autostart.disable().unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(
            autostart.is_enabled().unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }
}
