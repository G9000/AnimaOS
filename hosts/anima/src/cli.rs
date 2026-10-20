//! Command line for the packaged Anima.

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(name = "anima", version, about = "Anima: your local agent runtime")]
pub struct Cli {
    /// No subcommand opens the console.
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub enum OnOff {
    On,
    Off,
}

#[derive(Debug, PartialEq, Eq, Subcommand)]
#[command(rename_all = "kebab-case")]
pub enum ConfigAction {
    /// Print one setting, or all of them.
    Get { key: Option<String> },
    /// Change a setting.
    Set { key: String, value: String },
}

#[derive(Debug, PartialEq, Eq, Subcommand)]
#[command(rename_all = "kebab-case")]
pub enum Command {
    /// Machine-readable packaging capabilities; never opens a profile.
    #[command(hide = true)]
    BuildInfo,
    /// Run the server in the foreground.
    Run,
    /// Run the supervisor (tray, server lifecycle).
    Supervise {
        #[arg(long)]
        background: bool,
        #[arg(long)]
        no_tray: bool,
    },
    /// Pick a workspace and finish first-time setup.
    Setup {
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        workspace: Option<PathBuf>,
    },
    /// Show whether Anima is running.
    Status,
    /// Start the server.
    Start,
    /// Stop the server.
    Stop {
        #[arg(long)]
        now: bool,
    },
    /// Restart the server.
    Restart {
        #[arg(long)]
        now: bool,
    },
    /// Stop the server and the supervisor.
    Quit {
        #[arg(long)]
        now: bool,
    },
    /// Show the server log.
    Logs {
        #[arg(long, short)]
        follow: bool,
    },
    /// Install the latest release.
    Update {
        #[arg(long)]
        now: bool,
    },
    /// Turn start-at-login on or off.
    Autostart {
        #[arg(value_enum)]
        on_off: OnOff,
    },
    /// Read or change settings.
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Option<Command> {
        let mut argv = vec!["anima"];
        argv.extend_from_slice(args);
        Cli::try_parse_from(argv).unwrap().command
    }

    #[test]
    fn no_subcommand_means_open() {
        assert_eq!(parse(&[]), None);
    }

    #[test]
    fn subcommands_and_flags_parse() {
        assert_eq!(parse(&["run"]), Some(Command::Run));
        assert_eq!(
            parse(&["supervise", "--background", "--no-tray"]),
            Some(Command::Supervise {
                background: true,
                no_tray: true
            })
        );
        assert_eq!(
            parse(&["setup", "--yes", "--workspace", "/w"]),
            Some(Command::Setup {
                yes: true,
                workspace: Some(PathBuf::from("/w"))
            })
        );
        assert_eq!(parse(&["stop", "--now"]), Some(Command::Stop { now: true }));
        assert_eq!(
            parse(&["autostart", "off"]),
            Some(Command::Autostart { on_off: OnOff::Off })
        );
        assert_eq!(
            parse(&["config", "set", "port", "9000"]),
            Some(Command::Config {
                action: ConfigAction::Set {
                    key: "port".into(),
                    value: "9000".into()
                }
            })
        );
    }
}
