use std::io;
use std::process::ExitCode;

use crate::config::Config;
use crate::paths::AnimaPaths;

pub mod autostart;
pub mod config_cmd;
pub mod lifecycle;
pub mod logs;
pub mod open;
pub mod run;
pub mod setup;
pub mod status;

/// The per-user folder (created if it is missing) and the config in it.
pub fn load() -> io::Result<(AnimaPaths, Config)> {
    let paths = AnimaPaths::discover()?;
    paths.ensure()?;
    let cfg = Config::load(&paths.config_file)?;
    Ok((paths, cfg))
}

/// What `anima`, `anima start` and `anima supervise` say before `anima setup`
/// has chosen a workspace: without one the server would work in whatever
/// folder it was started from (often the home folder, with Anima's own keys).
pub const NOT_SET_UP: &str = "Anima isn't set up yet. Run `anima setup` first.";
/// The exit code for a command refused until setup has run.
const NOT_SET_UP_CODE: u8 = 2;

/// `Some(exit code)`, after saying so, when no workspace is configured yet.
pub fn refuse_unless_set_up(cfg: &Config) -> Option<ExitCode> {
    if cfg.workspace.is_some() {
        return None;
    }
    eprintln!("{NOT_SET_UP}");
    Some(ExitCode::from(NOT_SET_UP_CODE))
}
