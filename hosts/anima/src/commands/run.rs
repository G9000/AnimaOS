//! `anima run`: the daemon API plus the embedded console, in the foreground.

use std::collections::BTreeMap;
use std::io::{self, Read};
use std::sync::Arc;

use anima_daemon::{run_from_env, ServeOptions};
use tokio::sync::Notify;

use crate::config::Config;
use crate::paths::AnimaPaths;
use crate::server_env::server_env;
use crate::ui::ui_router;

/// Set by the supervisor that started this server.
const SUPERVISOR_URL_ENV: &str = "ANIMA_SUPERVISOR_URL";

pub fn run() -> io::Result<()> {
    let paths = AnimaPaths::discover()?;
    paths.ensure()?;
    let cfg = Config::load(&paths.config_file)?;
    let process_env: BTreeMap<String, String> = std::env::vars().collect();
    // The environment must be complete before the runtime spawns any other thread.
    for (key, value) in server_env(&cfg, &paths, &process_env) {
        std::env::set_var(key, value);
    }
    let supervised = process_env
        .get(SUPERVISOR_URL_ENV)
        .is_some_and(|url| !url.is_empty());
    let shutdown = supervised.then(shutdown_when_supervisor_goes);
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(run_from_env(ServeOptions {
            ui: ui_router(),
            shutdown,
        }))
}

/// The supervisor holds this process's stdin open for as long as it runs;
/// end of input means it is gone (even killed), so the server shuts down
/// rather than outlive it. An unsupervised server never reads stdin.
fn shutdown_when_supervisor_goes() -> Arc<Notify> {
    let shutdown = Arc::new(Notify::new());
    let notify = Arc::clone(&shutdown);
    std::thread::spawn(move || {
        let mut stdin = io::stdin().lock();
        let mut buffer = [0u8; 256];
        loop {
            match stdin.read(&mut buffer) {
                Ok(0) => break,
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
        // A permit is kept if the server is not waiting yet.
        notify.notify_one();
    });
    shutdown
}
