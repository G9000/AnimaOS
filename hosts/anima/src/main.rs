//! Packaged Anima entry point.

mod autostart;
mod cli;
mod client;
mod commands;
mod config;
mod paths;
mod server_env;
mod supervisor;
mod tray;
mod ui;

use std::io;
use std::process::ExitCode;

use clap::Parser;

use cli::{Cli, Command};
use supervisor::Action;

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
        Some(Command::Supervise {
            background,
            no_tray,
        }) => report(supervisor::supervise(tray::wanted(no_tray, background))),
        None => report_code(commands::open::open()),
        Some(Command::Status) => report_code(commands::status::status()),
        Some(Command::Start) => report_code(commands::lifecycle::start()),
        Some(Command::Stop { now }) => report_action(Action::Stop, now),
        Some(Command::Restart { now }) => report_action(Action::Restart, now),
        Some(Command::Quit { now }) => report_action(Action::Quit, now),
        Some(Command::Update { now }) => report_action(Action::Update, now),
        Some(Command::Logs { follow }) => report_code(commands::logs::logs(follow)),
        Some(Command::Config { action }) => report_code(commands::config_cmd::config(action)),
        Some(Command::Autostart { on_off }) => report_code(commands::autostart::autostart(on_off)),
        // Setup lands with its own task.
        Some(Command::Setup { .. }) => {
            eprintln!("not yet available");
            ExitCode::from(2)
        }
    }
}

fn report_action(action: Action, now: bool) -> ExitCode {
    report_code(commands::lifecycle::run_action(action, now))
}

fn report_code(result: io::Result<ExitCode>) -> ExitCode {
    result.unwrap_or_else(|error| {
        eprintln!("anima: {error}");
        ExitCode::FAILURE
    })
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
