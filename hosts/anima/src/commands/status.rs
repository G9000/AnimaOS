//! `anima status`.

use std::io;
use std::process::ExitCode;

use crate::client::SupervisorClient;
use crate::supervisor::{ServerState, SupervisorStatus};

/// The exit code when Anima is not running.
const NOT_RUNNING: u8 = 3;

pub fn status() -> io::Result<ExitCode> {
    let (paths, cfg) = super::load()?;
    let Some(client) = SupervisorClient::connect(&paths, &cfg)? else {
        return Ok(not_running());
    };
    for line in describe(&client.status()?) {
        println!("{line}");
    }
    Ok(ExitCode::SUCCESS)
}

pub fn not_running() -> ExitCode {
    println!("Anima is not running");
    ExitCode::from(NOT_RUNNING)
}

/// The lines `status` prints.
pub fn describe(status: &SupervisorStatus) -> Vec<String> {
    let mut first = format!(
        "{} · v{} · {}",
        state_label(&status.state),
        status.version,
        status.server_url
    );
    if let Some(connectors) = status.connectors {
        first.push_str(&format!(" · {connectors} connectors"));
    }
    let mut lines = vec![first];
    if let Some(workspace) = &status.workspace {
        lines.push(format!("Workspace: {}", workspace.display()));
    }
    if let Some(problem) = &status.workspace_problem {
        lines.push(format!("Workspace problem: {problem}"));
    }
    if let Some(version) = &status.update.available {
        lines.push(format!("Update available: v{version}"));
    }
    lines
}

fn state_label(state: &ServerState) -> String {
    match state {
        ServerState::Starting => "Starting".to_string(),
        ServerState::Running => "Running".to_string(),
        ServerState::Draining { in_flight, .. } => {
            format!("Waiting for {} tasks to finish", in_flight.total())
        }
        ServerState::Stopping => "Stopping".to_string(),
        ServerState::Stopped => "Stopped".to_string(),
        ServerState::Crashed {
            message,
            retry_in_secs,
        } => format!("Crashed ({message}; retrying in {retry_in_secs}s)"),
        ServerState::Updating { stage } => format!("Updating ({stage})"),
    }
}

#[cfg(test)]
pub fn sample_status(state: ServerState) -> SupervisorStatus {
    serde_json::from_value(serde_json::json!({
        "version": "1.2.3",
        "state": serde_json::to_value(state).unwrap(),
        "serverUrl": "http://127.0.0.1:8787",
        "workspace": null,
        "workspaceProblem": null,
        "dataDir": "/d",
        "autostart": true,
        "connectors": null,
        "pending": null,
        "update": {"available": null, "lastError": null},
    }))
    .unwrap()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use anima_daemon::system::InFlight;

    use super::*;
    use crate::supervisor::Action;

    #[test]
    fn running_line_has_version_url_and_connectors() {
        let mut running = sample_status(ServerState::Running);
        assert_eq!(
            describe(&running),
            ["Running · v1.2.3 · http://127.0.0.1:8787"]
        );
        running.connectors = Some(2);
        running.workspace = Some(PathBuf::from("/w"));
        running.update.available = Some("1.3.0".into());
        assert_eq!(
            describe(&running),
            [
                "Running · v1.2.3 · http://127.0.0.1:8787 · 2 connectors",
                "Workspace: /w",
                "Update available: v1.3.0"
            ]
        );
    }

    #[test]
    fn other_states_are_named() {
        let draining = sample_status(ServerState::Draining {
            action: Action::Stop,
            in_flight: InFlight {
                chat_runs: 2,
                jobs: 1,
                ..InFlight::default()
            },
        });
        assert!(describe(&draining)[0].starts_with("Waiting for 3 tasks to finish · v1.2.3"));
        assert!(describe(&sample_status(ServerState::Stopped))[0].starts_with("Stopped · v"));
    }
}
