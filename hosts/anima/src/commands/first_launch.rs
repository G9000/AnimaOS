//! Minimal host preparation before the browser's first-run onboarding.

use std::fs::OpenOptions;
use std::io;

use crate::config::Config;
use crate::paths::AnimaPaths;
use crate::supervisor::tokens;

/// Double-clicks may race. Hold an OS lock while rereading and creating the
/// profile so neither launch overwrites a newer config or rotates its tokens.
pub(super) fn prepare(paths: &AnimaPaths) -> io::Result<Config> {
    paths.ensure()?;
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let lock = options.open(paths.root.join("first-launch.lock"))?;
    lock.lock()?;
    let mut cfg = Config::load(&paths.config_file)?;
    if cfg.workspace.is_some() {
        return Ok(cfg);
    }
    let home = paths
        .root
        .parent()
        .ok_or_else(|| io::Error::other("the Anima profile has no parent directory"))?;
    let workspace = super::setup::prepare_workspace(
        &home.join("anima-workspace").to_string_lossy(),
        home,
        paths,
    )
    .map_err(|message| io::Error::new(io::ErrorKind::InvalidInput, message))?;
    tokens::ensure(paths)?;
    cfg.workspace = Some(workspace);
    // Onboarding does not grant login startup or copy shell credentials.
    cfg.autostart = false;
    cfg.save(&paths.config_file)?;
    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simultaneous_preparation_keeps_one_config_and_token_pair() {
        let home = tempfile::tempdir().unwrap();
        let paths = AnimaPaths::from_home(home.path());
        let threads: Vec<_> = (0..4)
            .map(|_| {
                let paths = paths.clone();
                std::thread::spawn(move || {
                    let cfg = prepare(&paths).unwrap();
                    let admin = std::fs::read(&paths.admin_token).unwrap();
                    let control = std::fs::read(&paths.control_token).unwrap();
                    (cfg, admin, control)
                })
            })
            .collect();
        let results: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
        for result in &results[1..] {
            assert_eq!(result, &results[0]);
        }
    }

    #[test]
    fn an_existing_workspace_and_config_are_preserved() {
        let home = tempfile::tempdir().unwrap();
        let paths = AnimaPaths::from_home(home.path());
        let cfg = Config {
            workspace: Some(home.path().join("chosen-folder")),
            port: 9876,
            autostart: true,
            ..Config::default()
        };
        cfg.save(&paths.config_file).unwrap();
        let original = std::fs::read(&paths.config_file).unwrap();
        assert_eq!(prepare(&paths).unwrap(), cfg);
        assert_eq!(std::fs::read(&paths.config_file).unwrap(), original);
    }

    #[test]
    fn an_invalid_default_workspace_does_not_overwrite_config_or_user_files() {
        let home = tempfile::tempdir().unwrap();
        let paths = AnimaPaths::from_home(home.path());
        Config::default().save(&paths.config_file).unwrap();
        let workspace = home.path().join("anima-workspace");
        std::fs::write(&workspace, "keep this file").unwrap();
        let original = std::fs::read(&paths.config_file).unwrap();
        assert!(prepare(&paths).is_err());
        assert_eq!(std::fs::read(&paths.config_file).unwrap(), original);
        assert_eq!(
            std::fs::read_to_string(workspace).unwrap(),
            "keep this file"
        );
        assert!(!paths.admin_token.exists());
    }
}
