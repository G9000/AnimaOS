//! `anima config get [key]` and `anima config set <key> <value>`.

use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::cli::ConfigAction;
use crate::client::SupervisorClient;
use crate::config::{validate_workspace, Config};
use crate::paths::AnimaPaths;

/// The exit code for a setting that does not exist or a value that is not allowed.
const INVALID: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Key {
    Workspace,
    Port,
    ControlPort,
    UpdatesCheck,
}

impl Key {
    const ALL: [Key; 4] = [
        Key::Workspace,
        Key::Port,
        Key::ControlPort,
        Key::UpdatesCheck,
    ];

    fn parse(name: &str) -> Option<Key> {
        Some(match name {
            "workspace" => Key::Workspace,
            "port" => Key::Port,
            "control-port" | "control_port" => Key::ControlPort,
            "updates.check" => Key::UpdatesCheck,
            _ => return None,
        })
    }

    fn name(self) -> &'static str {
        match self {
            Key::Workspace => "workspace",
            Key::Port => "port",
            Key::ControlPort => "control-port",
            Key::UpdatesCheck => "updates.check",
        }
    }

    fn value(self, cfg: &Config) -> String {
        match self {
            Key::Workspace => cfg
                .workspace
                .as_ref()
                .map_or_else(|| "(not set)".to_string(), |ws| ws.display().to_string()),
            Key::Port => cfg.port.to_string(),
            Key::ControlPort => cfg.control_port.to_string(),
            Key::UpdatesCheck => cfg.updates.check.to_string(),
        }
    }
}

pub fn config(action: ConfigAction) -> io::Result<ExitCode> {
    let (paths, mut cfg) = super::load()?;
    match action {
        ConfigAction::Get { key: None } => {
            for key in Key::ALL {
                println!("{} = {}", key.name(), key.value(&cfg));
            }
            Ok(ExitCode::SUCCESS)
        }
        ConfigAction::Get { key: Some(name) } => match Key::parse(&name) {
            Some(key) => {
                println!("{}", key.value(&cfg));
                Ok(ExitCode::SUCCESS)
            }
            None => Ok(invalid(unknown_key(&name))),
        },
        ConfigAction::Set { key, value } => {
            let Some(parsed) = Key::parse(&key) else {
                return Ok(invalid(unknown_key(&key)));
            };
            // Asked before the change: a new control port is not listening yet.
            let running = SupervisorClient::connect(&paths, &cfg)?.is_some();
            if let Err(problem) = apply(parsed, &value, &mut cfg, &paths) {
                return Ok(invalid(problem));
            }
            cfg.save(&paths.config_file)?;
            println!("{} = {}", parsed.name(), parsed.value(&cfg));
            if running {
                println!("Restart Anima for this to take effect");
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn unknown_key(name: &str) -> String {
    let known: Vec<&str> = Key::ALL.iter().map(|key| key.name()).collect();
    format!(
        "Unknown setting `{name}`; the settings are {}",
        known.join(", ")
    )
}

fn invalid(problem: String) -> ExitCode {
    eprintln!("{problem}");
    ExitCode::from(INVALID)
}

/// Sets `key` in `cfg`, or says what is wrong with `value`.
fn apply(key: Key, value: &str, cfg: &mut Config, paths: &AnimaPaths) -> Result<(), String> {
    match key {
        Key::Workspace => cfg.workspace = Some(workspace(value, paths)?),
        Key::Port => {
            let port = port(value)?;
            if port == cfg.control_port {
                return Err(format!("Port {port} is the control port's"));
            }
            cfg.port = port;
        }
        Key::ControlPort => {
            let port = port(value)?;
            if port == cfg.port {
                return Err(format!("Port {port} is the server's"));
            }
            cfg.control_port = port;
        }
        Key::UpdatesCheck => {
            cfg.updates.check = match value {
                "true" => true,
                "false" => false,
                _ => return Err(format!("`{value}` is not true or false")),
            }
        }
    }
    Ok(())
}

fn port(value: &str) -> Result<u16, String> {
    match value.parse::<u16>() {
        Ok(port) if port != 0 => Ok(port),
        _ => Err(format!("`{value}` is not a port number (1 to 65535)")),
    }
}

/// An existing folder the server may work in, as an absolute path.
fn workspace(value: &str, paths: &AnimaPaths) -> Result<PathBuf, String> {
    let home = paths
        .root
        .parent()
        .ok_or_else(|| "The home folder is unknown".to_string())?;
    let path = expand_home(value, home);
    let path = std::path::absolute(&path).map_err(|error| format!("{value}: {error}"))?;
    validate_workspace(&path, home, paths).map_err(|problem| problem.message())?;
    Ok(path)
}

fn expand_home(value: &str, home: &Path) -> PathBuf {
    match value
        .strip_prefix("~/")
        .or_else(|| value.strip_prefix("~\\"))
    {
        Some(rest) => home.join(rest),
        None if value == "~" => home.to_path_buf(),
        None => PathBuf::from(value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_accept_both_spellings_of_control_port() {
        assert_eq!(Key::parse("control-port"), Some(Key::ControlPort));
        assert_eq!(Key::parse("control_port"), Some(Key::ControlPort));
        assert_eq!(Key::parse("nope"), None);
    }

    #[test]
    fn ports_must_be_real_and_distinct() {
        let home = tempfile::tempdir().unwrap();
        let paths = AnimaPaths::from_home(home.path());
        let mut cfg = Config::default();
        assert!(apply(Key::Port, "0", &mut cfg, &paths).is_err());
        assert!(apply(Key::Port, "65536", &mut cfg, &paths).is_err());
        assert!(apply(Key::Port, "8788", &mut cfg, &paths).is_err());
        assert_eq!(apply(Key::Port, "9000", &mut cfg, &paths), Ok(()));
        assert_eq!(cfg.port, 9000);
        assert!(apply(Key::ControlPort, "9000", &mut cfg, &paths).is_err());
    }

    #[test]
    fn updates_check_is_a_bool() {
        let home = tempfile::tempdir().unwrap();
        let paths = AnimaPaths::from_home(home.path());
        let mut cfg = Config::default();
        assert_eq!(apply(Key::UpdatesCheck, "false", &mut cfg, &paths), Ok(()));
        assert!(!cfg.updates.check);
        assert!(apply(Key::UpdatesCheck, "yes", &mut cfg, &paths).is_err());
    }

    #[test]
    fn tilde_means_the_home_folder() {
        let home = Path::new("/h");
        assert_eq!(expand_home("~/w", home), PathBuf::from("/h/w"));
        assert_eq!(expand_home("~", home), PathBuf::from("/h"));
        assert_eq!(expand_home("/x", home), PathBuf::from("/x"));
    }
}
