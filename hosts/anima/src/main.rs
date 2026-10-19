//! Packaged Anima entry point.

mod cli;
mod commands;
mod config;
mod paths;
mod server_env;
mod supervisor;
mod ui;

use std::io;
use std::process::ExitCode;

use clap::Parser;

use cli::{Cli, Command};

fn main() -> ExitCode {
    // Worker re-exec must be recognised before clap sees the arguments.
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if arguments
        .iter()
        .any(|argument| argument.to_string_lossy().starts_with("--document-worker-"))
    {
        if arguments.len() != 1 || arguments[0] != "--document-worker-v2" {
            return report(Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "document worker requires exactly --document-worker-v2",
            )));
        }
        return report(anima_daemon::document_worker_entry());
    }

    match Cli::parse().command {
        Some(Command::Run) => report(commands::run::run()),
        // There is no tray yet: with or without `--no-tray` it runs headless.
        Some(Command::Supervise { .. }) => report(supervisor::supervise()),
        // `open` and the other commands land with the supervisor.
        _ => {
            eprintln!("not yet available");
            ExitCode::from(2)
        }
    }
}

fn report(result: io::Result<()>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("anima: {error}");
            ExitCode::FAILURE
        }
    }
}
