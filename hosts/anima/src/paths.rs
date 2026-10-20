//! Per-user folder layout for the packaged Anima (`~/.animaos-swarm/`).

use std::io;
use std::path::{Path, PathBuf};

const ROOT_DIR_NAME: &str = ".animaos-swarm";

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
