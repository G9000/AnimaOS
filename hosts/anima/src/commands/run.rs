//! `anima run`: the daemon API plus the embedded console, in the foreground.

use std::collections::BTreeMap;
use std::io;

use anima_daemon::{run_from_env, ServeOptions};

use crate::config::Config;
use crate::paths::AnimaPaths;
use crate::server_env::server_env;
use crate::ui::ui_router;

pub fn run() -> io::Result<()> {
    let paths = AnimaPaths::discover()?;
    paths.ensure()?;
    let cfg = Config::load(&paths.config_file)?;
    let process_env: BTreeMap<String, String> = std::env::vars().collect();
    // The environment must be complete before the runtime spawns any other thread.
    for (key, value) in server_env(&cfg, &paths, &process_env) {
        std::env::set_var(key, value);
    }
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(run_from_env(ServeOptions {
            ui: ui_router(),
            shutdown: None,
        }))
}
