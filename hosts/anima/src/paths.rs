//! Per-user folder layout for the packaged Anima (`~/.animaos-swarm/`).

use std::io;
use std::path::{Path, PathBuf};

const ROOT_DIR_NAME: &str = ".animaos-swarm";

/// An AppImage's mount is transient. Its launcher must re-execute the stable
/// AppImage file so a detached supervisor owns its own mount's lifetime.
pub fn launcher_executable() -> io::Result<PathBuf> {
    let executable = std::env::current_exe()?;
    #[cfg(target_os = "linux")]
    return appimage_launcher(
        std::env::var_os("APPIMAGE").map(PathBuf::from),
        std::env::var_os("APPDIR").map(PathBuf::from),
        executable,
    );
    #[cfg(not(target_os = "linux"))]
    Ok(executable)
}

#[cfg(any(target_os = "linux", test))]
fn appimage_launcher(
    appimage: Option<PathBuf>,
    appdir: Option<PathBuf>,
    executable: PathBuf,
) -> io::Result<PathBuf> {
    // Terminals launched by another AppImage may inherit its variables. Only
    // this executable's actual mount can select our outer AppImage launcher.
    let inside_mount = appdir
        .filter(|directory| directory.is_absolute())
        .and_then(|directory| directory.canonicalize().ok())
        .zip(executable.canonicalize().ok())
        .is_some_and(|(directory, binary)| binary.starts_with(directory));
    if !inside_mount {
        return Ok(executable);
    }
    match appimage {
        Some(path) if path.is_absolute() && path.is_file() => Ok(path),
        Some(_) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "APPIMAGE must name an existing absolute AppImage file",
        )),
        None => Ok(executable),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnimaPaths {
    pub root: PathBuf,
    pub config_file: PathBuf,
    pub control_token: PathBuf,
    pub admin_token: PathBuf,
    pub data: PathBuf,
    pub logs: PathBuf,
    pub run: PathBuf,
}

impl AnimaPaths {
    pub fn from_home(home: &Path) -> Self {
        let root = home.join(ROOT_DIR_NAME);
        Self {
            config_file: root.join("config.toml"),
            control_token: root.join("control-token"),
            admin_token: root.join("admin-token"),
            data: root.join("data"),
            logs: root.join("logs"),
            run: root.join("run"),
            root,
        }
    }

    pub fn discover() -> io::Result<Self> {
        // Unlike HOME/USERPROFILE, this also works with Windows Known Folders.
        // It selects an alternate profile home, including its workspace and
        // private .animaos-swarm directory; it is inherited by every child.
        if let Some(home) = std::env::var_os("ANIMA_HOME") {
            let home = PathBuf::from(home);
            if home.as_os_str().is_empty() || !home.is_absolute() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "ANIMA_HOME must be an absolute profile directory",
                ));
            }
            return Ok(Self::from_home(&home));
        }
        let dirs = directories::BaseDirs::new().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "could not determine the home directory",
            )
        })?;
        Ok(Self::from_home(dirs.home_dir()))
    }

    /// Creates the root and its subdirectories, readable only by the current user on Unix.
    pub fn ensure(&self) -> io::Result<()> {
        for dir in [&self.root, &self.data, &self.logs, &self.run] {
            create_private_dir(dir)?;
        }
        Ok(())
    }
}

fn create_private_dir(dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_inherited_appimage_does_not_replace_the_launcher() {
        let unrelated = tempfile::NamedTempFile::new().unwrap();
        let executable = PathBuf::from("/usr/bin/anima");
        assert_eq!(
            appimage_launcher(Some(unrelated.path().into()), None, executable.clone()).unwrap(),
            executable
        );
    }

    #[test]
    fn appimage_launches_keep_the_stable_package_path() {
        let image = tempfile::NamedTempFile::new().unwrap();
        let mount = tempfile::tempdir().unwrap();
        let mounted = mount.path().join("anima");
        std::fs::write(&mounted, "Anima inside the image").unwrap();
        assert_eq!(
            appimage_launcher(
                Some(image.path().into()),
                Some(mount.path().into()),
                mounted.clone()
            )
            .unwrap(),
            image.path()
        );
        assert_eq!(
            appimage_launcher(None, None, mounted.clone()).unwrap(),
            mounted
        );
        assert!(appimage_launcher(
            Some("relative.AppImage".into()),
            Some(mount.path().into()),
            mounted
        )
        .is_err());
    }

    #[test]
    fn from_home_lays_out_the_documented_folder() {
        let home = Path::new("/home/u");
        let p = AnimaPaths::from_home(home);
        assert_eq!(p.root, home.join(".animaos-swarm"));
        assert_eq!(p.config_file, p.root.join("config.toml"));
        assert_eq!(p.control_token, p.root.join("control-token"));
        assert_eq!(p.admin_token, p.root.join("admin-token"));
        assert_eq!(p.data, p.root.join("data"));
        assert_eq!(p.logs, p.root.join("logs"));
        assert_eq!(p.run, p.root.join("run"));
    }

    #[test]
    fn ensure_creates_private_directories() {
        let home = tempfile::tempdir().unwrap();
        let p = AnimaPaths::from_home(home.path());
        p.ensure().unwrap();
        p.ensure().unwrap(); // idempotent
        for dir in [&p.root, &p.data, &p.logs, &p.run] {
            assert!(dir.is_dir(), "{dir:?} missing");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = std::fs::metadata(dir).unwrap().permissions().mode() & 0o777;
                assert_eq!(mode, 0o700, "{dir:?}");
            }
        }
    }
}
