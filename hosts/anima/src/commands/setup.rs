//! `anima setup`: the first-run command. Asks for the workspace folder, the
//! port and start at login, offers to copy provider API keys from the shell
//! into Anima's own key store (a login-started Anima never sees the shell's
//! environment), then starts Anima and opens the console.

use std::collections::BTreeMap;
use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use dialoguer::theme::ColorfulTheme;
use dialoguer::{Confirm, Input};
use reqwest::blocking::Client;
use reqwest::StatusCode;
use serde_json::json;

use crate::client::SupervisorClient;
use crate::config::{validate_workspace, Config};
use crate::paths::AnimaPaths;
use crate::server_env::effective_port;
use crate::supervisor::{tokens, Action, ServerState};

use super::{autostart, lifecycle, open};

/// Test-only switch: leaves the system's start-at-login alone and saves
/// `autostart = false`.
const NO_AUTOSTART_ENV: &str = "ANIMA_SETUP_NO_AUTOSTART";
/// Test-only switch: does not open the browser.
const NO_OPEN_ENV: &str = "ANIMA_SETUP_NO_OPEN";
/// What an imported key is called in the key list.
const IMPORT_LABEL: &str = "Imported from shell";
const IMPORT_TIMEOUT: Duration = Duration::from_secs(10);
/// The exit code for an answer setup can't use.
const BAD_ANSWER: u8 = 2;
/// Said once after keys were imported: these parts of the server still read
/// the shell's environment, not the key store.
const IMPORT_NOTE: &str = "Imported. Note: memory embeddings, speech-to-text and coding-agent key passthrough still read the environment variable; set it in your login environment if you use them.";

/// A provider key found in the shell's environment. No `Debug`: it holds a key.
struct Found {
    provider: &'static str,
    /// The variable that holds it; the only part ever shown.
    variable: &'static str,
    key: String,
}

/// What was chosen, from the prompts or from `--yes`.
struct Choices {
    workspace: PathBuf,
    port: u16,
    autostart: bool,
    imports: Vec<Found>,
}

pub fn setup(yes: bool, workspace: Option<PathBuf>) -> io::Result<ExitCode> {
    if !yes && !(io::stdin().is_terminal() && io::stdout().is_terminal()) {
        eprintln!(
            "anima setup asks questions, so it needs a terminal. Use --yes to accept the defaults."
        );
        return Ok(ExitCode::from(BAD_ANSWER));
    }
    let paths = AnimaPaths::discover()?;
    paths.ensure()?;
    let home = paths
        .root
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| io::Error::other("the Anima folder has no parent"))?;
    let previous = Config::load(&paths.config_file)?;
    let found = keys_in_environment(&provider_variables(), &|name| std::env::var(name).ok());

    let choices = if yes {
        match choose_defaults(workspace, &previous, &home, &paths, found) {
            Ok(choices) => choices,
            Err(message) => {
                eprintln!("{message}");
                return Ok(ExitCode::from(BAD_ANSWER));
            }
        }
    } else {
        ask(workspace, &previous, &home, &paths, found)?
    };

    let (_, admin) = tokens::ensure(&paths)?;
    let mut cfg = previous.clone();
    cfg.workspace = Some(choices.workspace.clone());
    cfg.port = choices.port;
    // Written explicitly: it stays false unless the system really has the
    // start-at-login entry.
    cfg.autostart = false;
    cfg.save(&paths.config_file)?;
    set_autostart(&paths, choices.autostart);

    // A running Anima keeps the folder and port it started with.
    if needs_restart(&paths, &previous, &cfg)? {
        let code = lifecycle::run_action(Action::Restart, false)?;
        if code != ExitCode::SUCCESS {
            return Ok(code);
        }
    }

    // An imported key must not also arrive as the server's environment key:
    // the key store would see it as a duplicate and keep nothing.
    for found in &choices.imports {
        for (provider, variable) in provider_variables() {
            if provider == found.provider {
                std::env::remove_var(variable);
            }
        }
    }
    let started = lifecycle::ensure_running(&paths, &cfg)?;
    println!("Anima is running · {}", started.status().server_url);

    import_keys(&cfg, admin.as_str(), &choices.imports);

    println!();
    println!("Workspace: {}", choices.workspace.display());
    if std::env::var_os(NO_OPEN_ENV).is_some_and(|value| value == "1") {
        return Ok(ExitCode::SUCCESS);
    }
    open::open()
}

fn provider_variables() -> Vec<(&'static str, &'static str)> {
    anima_daemon::provider_env_key_names()
}

/// For each provider with a key in the environment, the variable the server
/// would read it from (the first one set), in provider order.
fn keys_in_environment(
    variables: &[(&'static str, &'static str)],
    env: &dyn Fn(&str) -> Option<String>,
) -> Vec<Found> {
    let mut found: Vec<Found> = Vec::new();
    for &(provider, variable) in variables {
        if found.iter().any(|known| known.provider == provider) {
            continue;
        }
        if let Some(key) = env(variable)
            .map(|key| key.trim().to_string())
            .filter(|key| !key.is_empty())
        {
            found.push(Found {
                provider,
                variable,
                key,
            });
        }
    }
    found
}

/// `--yes`: the flag, else what the config has, else the suggested folder.
fn choose_defaults(
    flag: Option<PathBuf>,
    current: &Config,
    home: &Path,
    paths: &AnimaPaths,
    found: Vec<Found>,
) -> Result<Choices, String> {
    let wanted = flag
        .or_else(|| current.workspace.clone())
        .unwrap_or_else(|| default_workspace(home));
    Ok(Choices {
        workspace: prepare_workspace(&wanted.to_string_lossy(), home, paths)?,
        port: current.port,
        autostart: suggested_autostart(current),
        imports: found,
    })
}

/// Yes the first time (no workspace chosen yet); afterwards, what it is now.
fn suggested_autostart(current: &Config) -> bool {
    current.workspace.is_none() || current.autostart
}

fn ask(
    flag: Option<PathBuf>,
    current: &Config,
    home: &Path,
    paths: &AnimaPaths,
    found: Vec<Found>,
) -> io::Result<Choices> {
    let theme = ColorfulTheme::default();
    let suggested = flag
        .or_else(|| current.workspace.clone())
        .unwrap_or_else(|| default_workspace(home));

    println!("Anima works inside one folder on your computer, its workspace.");
    let workspace = loop {
        let answer: String = Input::with_theme(&theme)
            .with_prompt("Workspace folder")
            .default(suggested.to_string_lossy().into_owned())
            .interact_text()?;
        match prepare_workspace(&answer, home, paths) {
            Ok(workspace) => break workspace,
            Err(message) => eprintln!("{message}"),
        }
    };

    let control_port = current.control_port;
    let port: u16 = Input::with_theme(&theme)
        .with_prompt("Port")
        .default(current.port)
        .validate_with(move |port: &u16| {
            if *port == 0 || *port == control_port {
                Err(format!("Pick a port other than 0 and {control_port}"))
            } else {
                Ok(())
            }
        })
        .interact_text()?;

    let autostart = Confirm::with_theme(&theme)
        .with_prompt("Start Anima when you log in?")
        .default(suggested_autostart(current))
        .interact()?;

    let mut imports = Vec::new();
    for key in found {
        let import = Confirm::with_theme(&theme)
            .with_prompt(format!("Import {} into Anima?", key.variable))
            .default(true)
            .interact()?;
        if import {
            imports.push(key);
        }
    }
    Ok(Choices {
        workspace,
        port,
        autostart,
        imports,
    })
}

fn default_workspace(home: &Path) -> PathBuf {
    home.join("anima-workspace")
}

/// The folder as the person typed it, made absolute, created if it is
/// missing, and checked against the workspace rules.
pub(super) fn prepare_workspace(
    answer: &str,
    home: &Path,
    paths: &AnimaPaths,
) -> Result<PathBuf, String> {
    let answer = answer.trim();
    if answer.is_empty() {
        return Err("Enter a folder".to_string());
    }
    let path = std::path::absolute(expand_home(answer, home))
        .map_err(|error| format!("Not a usable path: {error}"))?;
    // The folder's own rules are checked against the existing data root.
    paths.ensure().map_err(|error| error.to_string())?;
    let mut created = false;
    if !path.exists() {
        std::fs::create_dir_all(&path)
            .map_err(|error| format!("Could not create {}: {error}", path.display()))?;
        created = true;
    }
    match validate_workspace(&path, home, paths) {
        Ok(()) => Ok(path),
        Err(problem) => {
            if created {
                let _ = std::fs::remove_dir(&path);
            }
            Err(problem.message())
        }
    }
}

/// `~` and `~/folder` mean the home folder.
fn expand_home(answer: &str, home: &Path) -> PathBuf {
    if answer == "~" {
        return home.to_path_buf();
    }
    match answer
        .strip_prefix("~/")
        .or_else(|| answer.strip_prefix("~\\"))
    {
        Some(rest) => home.join(rest),
        None => PathBuf::from(answer),
    }
}

/// Installs or removes start at login, and records which it is. A failure
/// is told but is not fatal: the config then says it is off.
fn set_autostart(paths: &AnimaPaths, on: bool) {
    if std::env::var_os(NO_AUTOSTART_ENV).is_some_and(|value| value == "1") {
        return;
    }
    if let Err(error) = autostart::apply(paths, on) {
        let what = if on { "turn on" } else { "turn off" };
        eprintln!("Could not {what} start at login: {error}");
    }
}

/// Whether a running Anima must restart to use the new answers.
fn needs_restart(paths: &AnimaPaths, previous: &Config, now: &Config) -> io::Result<bool> {
    if previous.workspace == now.workspace && previous.port == now.port {
        return Ok(false);
    }
    let Some(client) = SupervisorClient::connect(paths, now)? else {
        return Ok(false);
    };
    Ok(matches!(client.status()?.state, ServerState::Running))
}

/// Copies each chosen key into the key store of the running server. Says
/// which variable, never the key; a failure is a one-line warning.
fn import_keys(cfg: &Config, admin_token: &str, imports: &[Found]) {
    if imports.is_empty() {
        return;
    }
    let process_env: BTreeMap<String, String> = std::env::vars().collect();
    let base = format!("http://127.0.0.1:{}", effective_port(cfg, &process_env));
    let client = match Client::builder().timeout(IMPORT_TIMEOUT).no_proxy().build() {
        Ok(client) => client,
        Err(error) => {
            eprintln!("Could not import the provider keys: {error}");
            return;
        }
    };
    let mut imported = false;
    for found in imports {
        let sent = client
            .post(format!("{base}/api/provider-keys/{}", found.provider))
            .bearer_auth(admin_token)
            .json(&json!({ "label": IMPORT_LABEL, "key": found.key }))
            .send();
        match sent {
            Ok(response) if response.status() == StatusCode::CREATED => {
                println!("Imported {} into Anima", found.variable);
                imported = true;
            }
            // The store has this very key already (an earlier setup, or the
            // environment of a server started from this shell).
            Ok(response) if response.status() == StatusCode::CONFLICT => {
                println!("{} is already in Anima", found.variable);
            }
            Ok(response) => eprintln!(
                "Could not import {} ({}): the server answered {}",
                found.variable,
                found.provider,
                response.status()
            ),
            Err(error) => eprintln!(
                "Could not import {} ({}): {}",
                found.variable,
                found.provider,
                error.without_url()
            ),
        }
    }
    if imported {
        println!("{IMPORT_NOTE}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> (tempfile::TempDir, PathBuf, AnimaPaths) {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        let paths = AnimaPaths::from_home(&home);
        paths.ensure().unwrap();
        (dir, home, paths)
    }

    #[test]
    fn yes_without_a_workspace_uses_and_creates_the_default() {
        let (_dir, home, paths) = home();
        let choices = choose_defaults(None, &Config::default(), &home, &paths, Vec::new())
            .ok()
            .unwrap();
        assert_eq!(choices.workspace, home.join("anima-workspace"));
        assert!(choices.workspace.is_dir());
        assert_eq!(choices.port, 8787);
        // Start at login is suggested the first time, though it is off until then.
        assert!(!Config::default().autostart);
        assert!(choices.autostart);
    }

    #[test]
    fn yes_keeps_the_current_answers() {
        let (_dir, home, paths) = home();
        let current = Config {
            workspace: Some(home.join("mine")),
            port: 9100,
            autostart: false,
            ..Config::default()
        };
        let choices = choose_defaults(None, &current, &home, &paths, Vec::new())
            .ok()
            .unwrap();
        assert_eq!(choices.workspace, home.join("mine"));
        assert_eq!(choices.port, 9100);
        assert!(!choices.autostart);
        // The flag wins over the config.
        let flagged = choose_defaults(
            Some(home.join("other")),
            &current,
            &home,
            &paths,
            Vec::new(),
        )
        .ok()
        .unwrap();
        assert_eq!(flagged.workspace, home.join("other"));
    }

    #[test]
    fn a_workspace_that_breaks_the_rules_is_refused_and_not_left_behind() {
        let (_dir, home, paths) = home();
        let message = prepare_workspace(home.to_str().unwrap(), &home, &paths).unwrap_err();
        assert!(message.contains("home folder"), "{message}");
        let inside = paths.data.join("ws");
        let message = prepare_workspace(inside.to_str().unwrap(), &home, &paths).unwrap_err();
        assert!(
            message.contains("inside the Anima data folder"),
            "{message}"
        );
        assert!(!inside.exists());
        let file = home.join("afile");
        std::fs::write(&file, "x").unwrap();
        let message = prepare_workspace(file.to_str().unwrap(), &home, &paths).unwrap_err();
        assert!(message.contains("not a folder"), "{message}");
        assert!(prepare_workspace("  ", &home, &paths).is_err());
    }

    #[test]
    fn a_tilde_means_the_home_folder() {
        let (_dir, home, paths) = home();
        assert_eq!(
            prepare_workspace("~/work", &home, &paths).unwrap(),
            home.join("work")
        );
        assert!(prepare_workspace("~", &home, &paths).is_err());
    }

    #[test]
    fn the_first_variable_set_for_a_provider_is_the_one_offered() {
        let variables = [
            ("openai", "OPENAI_API_KEY"),
            ("openai", "OPENAI_KEY"),
            ("groq", "GROQ_API_KEY"),
            ("mistral", "MISTRAL_API_KEY"),
        ];
        let env = BTreeMap::from([
            ("OPENAI_KEY", "second"),
            ("GROQ_API_KEY", "  "),
            ("MISTRAL_API_KEY", " sk-m "),
        ]);
        let found = keys_in_environment(&variables, &|name| env.get(name).map(|v| v.to_string()));
        let summary: Vec<_> = found
            .iter()
            .map(|f| (f.provider, f.variable, f.key.as_str()))
            .collect();
        assert_eq!(
            summary,
            vec![
                ("openai", "OPENAI_KEY", "second"),
                ("mistral", "MISTRAL_API_KEY", "sk-m")
            ]
        );
    }
}
