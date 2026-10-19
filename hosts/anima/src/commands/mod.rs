use std::io;

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
