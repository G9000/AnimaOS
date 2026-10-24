use anima_village::{GameInput, GameState, Language};
use anima_village_game::{connection, http, Service, Store};
use std::{net::SocketAddr, path::PathBuf, time::Duration};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let connection = connection::resolve();
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--connection-status"] {
        println!(
            "{}",
            serde_json::json!({"available":connection.engine.is_some(),"runtime":connection.runtime,"reason":connection.reason})
        );
        return Ok(());
    }
    if args == ["--live-smoke"] {
        let engine = connection.engine.ok_or_else(|| {
            std::io::Error::other(
                connection
                    .reason
                    .unwrap_or("No existing model connection".into()),
            )
        })?;
        let turn = tokio::time::timeout(
            Duration::from_secs(30),
            engine.play(GameState::new(Language::Ja), GameInput::Visit),
        )
        .await?
        .map_err(std::io::Error::other)?;
        println!(
            "{}",
            serde_json::json!({"runtime":turn.runtime,"dialogue":turn.dialogue})
        );
        return Ok(());
    }
    if !args.is_empty() {
        return Err("Supported arguments: --connection-status, --live-smoke".into());
    }
    let bind: SocketAddr = std::env::var("VILLAGE_BIND")
        .unwrap_or_else(|_| "127.0.0.1:8092".into())
        .parse()?;
    if !bind.ip().is_loopback() {
        return Err("The village host must bind to a loopback address".into());
    }
    let path = std::env::var_os("VILLAGE_DB_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("USERPROFILE")
                .or_else(|| std::env::var_os("HOME"))
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".animaos-village/saves.sqlite")
        });
    let deadline = std::env::var("VILLAGE_GENERATION_TIMEOUT_MS")
        .ok()
        .map(|s| s.parse::<u64>())
        .transpose()?
        .unwrap_or(18_000);
    if !(1_000..=60_000).contains(&deadline) {
        return Err("VILLAGE_GENERATION_TIMEOUT_MS must be between 1000 and 60000".into());
    }
    let mut service = Service::new(
        Store::open(&path).map_err(|e| std::io::Error::other(e.message))?,
        connection.engine,
        Duration::from_millis(deadline),
    );
    service.runtime_info = connection.runtime;
    service.unavailable_reason = connection.reason;
    println!(
        "Village host listening on http://{bind}; mode={}, provider={}, model={}",
        service.runtime().mode,
        service.runtime().provider,
        service.runtime().model
    );
    if let Some(reason) = &service.unavailable_reason {
        println!("Reviewed dialogue: {reason}");
    }
    let listener = tokio::net::TcpListener::bind(bind).await?;
    axum::serve(listener, http::router(service))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
