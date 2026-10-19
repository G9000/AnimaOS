//! Library entry points for the daemon process: environment parsing and
//! `run_from_env`, shared by the `anima-daemon` binary and other hosts.
use std::io;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use tokio::net::TcpListener;
use tokio::sync::Notify;
use tracing::info;

use super::{serve_with, DaemonConfig, PersistenceMode};

/// Host-supplied additions to [`serve_with`].
#[derive(Default)]
pub struct ServeOptions {
    /// Installed as the router's fallback, so every daemon route wins over it.
    pub ui: Option<Router>,
    /// Starts a graceful shutdown when notified (`notify_one`), in addition
    /// to Ctrl+C and SIGTERM.
    pub shutdown: Option<Arc<Notify>>,
}

/// The listen address: `ANIMAOS_RS_HOST` (default `127.0.0.1`) and
/// `ANIMAOS_RS_PORT` (default `8080`).
pub fn bind_addr_from_env() -> String {
    let host = std::env::var("ANIMAOS_RS_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port = std::env::var("ANIMAOS_RS_PORT").unwrap_or_else(|_| "8080".to_string());
    format!("{host}:{port}")
}

/// Reads the daemon's operating limits from `ANIMAOS_RS_*` variables.
pub fn config_from_env() -> io::Result<DaemonConfig> {
    let default_config = DaemonConfig::default();
    Ok(DaemonConfig {
        max_request_bytes: parse_env_usize(
            "ANIMAOS_RS_MAX_REQUEST_BYTES",
            default_config.max_request_bytes,
        )?,
        request_timeout: Duration::from_secs(parse_env_u64(
            "ANIMAOS_RS_REQUEST_TIMEOUT_SECS",
            default_config.request_timeout.as_secs(),
        )?),
        persistence_mode: parse_persistence_mode(default_config.persistence_mode)?,
        run_request_timeout: Duration::from_secs(parse_env_u64(
            "ANIMAOS_RS_RUN_REQUEST_TIMEOUT_SECS",
            default_config.run_request_timeout.as_secs(),
        )?),
        max_concurrent_runs: parse_env_usize(
            "ANIMAOS_RS_MAX_CONCURRENT_RUNS",
            default_config.max_concurrent_runs,
        )?,
        max_runs_per_agent: validate_max_runs_per_agent(parse_env_usize(
            "ANIMAOS_RS_MAX_RUNS_PER_AGENT",
            default_config.max_runs_per_agent,
        )?)?,
        max_background_processes: parse_env_usize(
            "ANIMAOS_RS_MAX_BACKGROUND_PROCESSES",
            default_config.max_background_processes,
        )?,
        db_max_connections: parse_env_u32(
            "ANIMAOS_RS_DB_MAX_CONNECTIONS",
            default_config.db_max_connections,
        )?,
        event_buffer: parse_env_usize("ANIMAOS_RS_EVENT_BUFFER", default_config.event_buffer)?,
        session_event_buffer: validate_session_event_buffer(parse_env_usize(
            "ANIMAOS_RS_SESSION_EVENT_BUFFER",
            default_config.session_event_buffer,
        )?)?,
    })
}

/// Initialises tracing, binds the configured address, logs the startup line
/// and serves until shutdown.
pub async fn run_from_env(options: ServeOptions) -> io::Result<()> {
    crate::keychain::validate_namespace_env()
        .map_err(|message| io::Error::new(io::ErrorKind::InvalidInput, message))?;
    crate::init_tracing();

    let bind_addr = bind_addr_from_env();
    let config = config_from_env()?;
    let listener = TcpListener::bind(bind_addr.as_str()).await?;
    let local_addr = listener.local_addr()?;
    info!(
        address = %local_addr,
        timeout_secs = config.request_timeout.as_secs(),
        run_timeout_secs = config.run_request_timeout.as_secs(),
        persistence_mode = config.persistence_mode.as_str(),
        max_concurrent_runs = config.max_concurrent_runs,
        max_runs_per_agent = config.max_runs_per_agent,
        max_background_processes = config.max_background_processes,
        runtime_memory_store = runtime_memory_store_label(),
        control_plane_durability = control_plane_store_label(),
        "anima-daemon listening"
    );

    serve_with(listener, config, options).await
}

fn control_plane_store_label() -> String {
    if let Ok(path) = std::env::var("ANIMAOS_RS_CONTROL_PLANE_FILE") {
        return format!("json:{path}");
    }
    "ephemeral".to_string()
}

fn runtime_memory_store_label() -> String {
    if let Ok(path) = std::env::var("ANIMAOS_RS_MEMORY_SQLITE_FILE") {
        return format!("sqlite:{path}");
    }
    if let Ok(path) = std::env::var("ANIMAOS_RS_MEMORY_FILE") {
        return format!("json:{path}");
    }
    "memory-only".to_string()
}

fn parse_env_usize(name: &str, default: usize) -> io::Result<usize> {
    match std::env::var(name) {
        Ok(value) => {
            let parsed = value.parse::<usize>().map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("{name} must be a positive integer"),
                )
            })?;
            if parsed == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("{name} must be a positive integer"),
                ));
            }
            Ok(parsed)
        }
        Err(std::env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("failed to read {name}: {error}"),
        )),
    }
}

/// Keeps `ANIMAOS_RS_MAX_RUNS_PER_AGENT` within the same hard limit that
/// `AgentRunCoordinator::with_max_runs_per_agent` clamps to
/// (`anima_daemon::agent_runs::MAX_RUNS_PER_AGENT_LIMIT`), so a misconfigured
/// value is rejected at startup instead of being silently clamped.
const MAX_RUNS_PER_AGENT_UPPER_BOUND: usize = 64;

fn validate_max_runs_per_agent(parsed: usize) -> io::Result<usize> {
    if parsed > MAX_RUNS_PER_AGENT_UPPER_BOUND {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "ANIMAOS_RS_MAX_RUNS_PER_AGENT must be between 1 and 64",
        ));
    }
    Ok(parsed)
}

/// Caps `ANIMAOS_RS_SESSION_EVENT_BUFFER`. Tokio allocates every slot of an
/// agent's event channel when its first stream opens, under the daemon's
/// locks, so an absurd value would exhaust memory or panic then; it is
/// refused here, at startup, instead.
const MAX_SESSION_EVENT_BUFFER: usize = 65_536;

fn validate_session_event_buffer(parsed: usize) -> io::Result<usize> {
    if !(1..=MAX_SESSION_EVENT_BUFFER).contains(&parsed) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "ANIMAOS_RS_SESSION_EVENT_BUFFER must be between 1 and {MAX_SESSION_EVENT_BUFFER}"
            ),
        ));
    }
    Ok(parsed)
}

fn parse_env_u32(name: &str, default: u32) -> io::Result<u32> {
    match std::env::var(name) {
        Ok(value) => {
            let parsed = value.parse::<u32>().map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("{name} must be a positive integer"),
                )
            })?;
            if parsed == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("{name} must be a positive integer"),
                ));
            }
            Ok(parsed)
        }
        Err(std::env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("failed to read {name}: {error}"),
        )),
    }
}

fn parse_env_u64(name: &str, default: u64) -> io::Result<u64> {
    match std::env::var(name) {
        Ok(value) => {
            let parsed = value.parse::<u64>().map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("{name} must be a positive integer"),
                )
            })?;
            if parsed == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("{name} must be a positive integer"),
                ));
            }
            Ok(parsed)
        }
        Err(std::env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("failed to read {name}: {error}"),
        )),
    }
}

fn parse_persistence_mode(default: PersistenceMode) -> io::Result<PersistenceMode> {
    match std::env::var("ANIMAOS_RS_PERSISTENCE_MODE") {
        Ok(value) => match value.to_ascii_lowercase().as_str() {
            "memory" => Ok(PersistenceMode::Memory),
            "postgres" => Ok(PersistenceMode::Postgres),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "ANIMAOS_RS_PERSISTENCE_MODE must be either 'memory' or 'postgres'",
            )),
        },
        Err(std::env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("failed to read ANIMAOS_RS_PERSISTENCE_MODE: {error}"),
        )),
    }
}

#[cfg(test)]
#[path = "entry_tests.rs"]
mod entry_tests;
