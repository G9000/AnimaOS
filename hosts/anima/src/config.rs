//! `config.toml` for the packaged Anima, plus workspace-folder validation.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::paths::AnimaPaths;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdatesConfig {
    pub check: bool,
}

impl Default for UpdatesConfig {
    fn default() -> Self {
        Self { check: true }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<PathBuf>,
    pub port: u16,
    pub control_port: u16,
    pub autostart: bool,
    pub updates: UpdatesConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            workspace: None,
            port: 8787,
            control_port: 8788,
            autostart: true,
            updates: UpdatesConfig::default(),
        }
    }
}

impl Config {
    /// A missing file yields the defaults.
    pub fn load(path: &Path) -> io::Result<Config> {
        match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string())),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(e),
        }
    }

    /// Atomic write (temp file in the same directory, then rename), mode 0600 on Unix.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        let text = toml::to_string_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
        let parent = path.parent().filter(|p| !p.as_os_str().is_empty());
        let dir = parent.unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(dir)?;
        let file_name = path.file_name().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "config path has no file name")
        })?;
        let mut tmp_name = file_name.to_os_string();
        tmp_name.push(format!(".tmp{}", std::process::id()));
        let tmp = dir.join(tmp_name);

        let result = (|| {
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create(true).truncate(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&tmp)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
            }
            file.write_all(text.as_bytes())?;
            file.sync_all()?;
            drop(file);
            std::fs::rename(&tmp, path)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        result
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceProblem {
    /// The workspace is the home folder itself.
    IsHome,
    /// The workspace contains the Anima data root.
    ContainsDataRoot,
    /// The workspace is inside the Anima data root.
    InsideDataRoot,
    /// The folder does not exist.
    Missing(PathBuf),
    /// The path exists but is not a folder.
    NotAFolder(PathBuf),
}

impl WorkspaceProblem {
    /// The problem, worded for the person who chose the folder.
    pub fn message(&self) -> String {
        match self {
            WorkspaceProblem::Missing(path) => {
                format!("Workspace folder not found: {}", path.display())
            }
            WorkspaceProblem::NotAFolder(path) => {
                format!("Workspace path is not a folder: {}", path.display())
            }
            WorkspaceProblem::IsHome => "The workspace can't be your home folder".to_string(),
            WorkspaceProblem::ContainsDataRoot => {
                "The workspace can't contain the Anima data folder".to_string()
            }
            WorkspaceProblem::InsideDataRoot => {
                "The workspace can't be inside the Anima data folder".to_string()
            }
        }
    }
}

/// Canonicalizes where the path exists, so symlinks and `..` can't dodge the rules.
fn resolve(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

pub fn validate_workspace(
    ws: &Path,
    home: &Path,
    paths: &AnimaPaths,
) -> Result<(), WorkspaceProblem> {
    if !ws.exists() {
        return Err(WorkspaceProblem::Missing(ws.to_path_buf()));
    }
    if !ws.is_dir() {
        return Err(WorkspaceProblem::NotAFolder(ws.to_path_buf()));
    }
    let ws = resolve(ws);
    if ws == resolve(home) {
        return Err(WorkspaceProblem::IsHome);
    }
    let root = resolve(&paths.root);
    if root.starts_with(&ws) {
        return Err(WorkspaceProblem::ContainsDataRoot);
    }
    if ws.starts_with(&root) {
        return Err(WorkspaceProblem::InsideDataRoot);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_when_config_missing() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config::load(&dir.path().join("nope.toml")).unwrap();
        assert_eq!(cfg.port, 8787);
        assert_eq!(cfg.control_port, 8788);
        assert!(cfg.autostart);
        assert!(cfg.updates.check);
        assert_eq!(cfg.workspace, None);
    }

    #[test]
    fn config_round_trips_and_is_private() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let cfg = Config {
            workspace: Some(PathBuf::from("/work/space")),
            port: 9001,
            control_port: 9002,
            autostart: false,
            updates: UpdatesConfig { check: false },
        };
        cfg.save(&path).unwrap();
        assert_eq!(Config::load(&path).unwrap(), cfg);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        // No temp files left behind.
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn partial_config_fills_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "port = 9100\n[updates]\ncheck = false\n").unwrap();
        let cfg = Config::load(&path).unwrap();
        assert_eq!(cfg.port, 9100);
        assert_eq!(cfg.control_port, 8788);
        assert!(!cfg.updates.check);
    }

    #[test]
    fn invalid_config_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "port = \"not a number\"").unwrap();
        let err = Config::load(&path).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn workspace_rules() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        let paths = AnimaPaths::from_home(&home);
        paths.ensure().unwrap();

        assert_eq!(
            validate_workspace(&home, &home, &paths),
            Err(WorkspaceProblem::IsHome)
        );
        assert_eq!(
            validate_workspace(tmp.path(), &home, &paths),
            Err(WorkspaceProblem::ContainsDataRoot)
        );
        assert_eq!(
            validate_workspace(&paths.data, &home, &paths),
            Err(WorkspaceProblem::InsideDataRoot)
        );
        let missing = home.join("nope");
        assert_eq!(
            validate_workspace(&missing, &home, &paths),
            Err(WorkspaceProblem::Missing(missing.clone()))
        );
        let file = home.join("afile");
        std::fs::write(&file, "x").unwrap();
        assert_eq!(
            validate_workspace(&file, &home, &paths),
            Err(WorkspaceProblem::NotAFolder(file.clone()))
        );
        let ok = home.join("anima-workspace");
        std::fs::create_dir_all(&ok).unwrap();
        assert_eq!(validate_workspace(&ok, &home, &paths), Ok(()));
    }
}
