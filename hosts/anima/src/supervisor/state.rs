//! The supervisor's state, its JSON shapes, and the pure transition function
//! that decides what a requested action does.

use std::path::PathBuf;

use anima_daemon::system::InFlight;
use serde::{Deserialize, Serialize};

/// A lifecycle action, as the control API and the console name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    Start,
    Stop,
    Restart,
    Quit,
    Update,
    CheckUpdate,
    Cancel,
}

/// What the server is doing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ServerState {
    Starting,
    Running,
    /// Waiting for the work in flight to finish before `action`.
    Draining {
        action: Action,
        in_flight: InFlight,
    },
    Stopping,
    Stopped,
    /// The server stopped on its own; it starts again in `retry_in_secs`.
    Crashed {
        message: String,
        retry_in_secs: u64,
    },
    /// Getting an update ready, installing it, or rolling it back.
    Updating {
        stage: String,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    /// The newer version that can be installed, if any.
    pub available: Option<String>,
    pub last_error: Option<String>,
}

/// `GET /v1/status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupervisorStatus {
    pub version: String,
    pub state: ServerState,
    pub server_url: String,
    pub workspace: Option<PathBuf>,
    pub workspace_problem: Option<String>,
    pub data_dir: PathBuf,
    pub autostart: bool,
    /// The server's connectors, when it could be asked.
    pub connectors: Option<u32>,
    /// The action under way: a gated one, or `cancel` until the server confirms its drain ended.
    pub pending: Option<Action>,
    pub update: UpdateStatus,
}

/// What a requested action does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transition {
    /// Nothing changes: the action is done or already under way.
    Stay,
    /// Drain the server before the gated action.
    Drain(Action),
    /// Shut the server down for the gated action without waiting for work.
    Shutdown(Action),
    /// Start the server.
    Spawn,
    /// End the drain on the server (confirmed before the state is `running` again).
    CancelDrain,
    /// No server is running: Stop becomes Stopped, Update goes to the updater.
    Settle(Action),
    /// The supervisor exits.
    Exit,
    /// Check for an update; the server is not affected.
    CheckUpdate,
}

impl Transition {
    /// The action still pending once this transition is under way.
    #[cfg(test)]
    pub fn pending(&self) -> Option<Action> {
        match self {
            Transition::Drain(action) | Transition::Shutdown(action) => Some(*action),
            Transition::Settle(Action::Update) => Some(Action::Update),
            // Until the server confirms the drain is over.
            Transition::CancelDrain => Some(Action::Cancel),
            _ => None,
        }
    }
}

/// Another action is pending; the control API answers 409.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Conflict {
    pub pending: Action,
}

/// Decides what `requested` does in `state` while `pending` is under way.
pub fn next(
    state: &ServerState,
    pending: Option<Action>,
    requested: Action,
    force: bool,
) -> Result<Transition, Conflict> {
    use Action::*;
    if requested == CheckUpdate {
        return Ok(Transition::CheckUpdate);
    }
    let draining = matches!(state, ServerState::Draining { .. });
    let pending = pending.or(match state {
        ServerState::Draining { action, .. } => Some(*action),
        _ => None,
    });
    // Ending a drain blocks nothing: a gated action simply drains again.
    if pending == Some(Cancel) {
        return Ok(match requested {
            Start | Cancel | CheckUpdate => Transition::Stay,
            gated if force => Transition::Shutdown(gated),
            gated => Transition::Drain(gated),
        });
    }
    if let Some(pending) = pending {
        return match requested {
            Cancel if draining => Ok(Transition::CancelDrain),
            same if same == pending && force && draining => Ok(Transition::Shutdown(pending)),
            same if same == pending => Ok(Transition::Stay),
            _ => Err(Conflict { pending }),
        };
    }
    let transition = match state {
        // A drain can be left on the server (one begun elsewhere, or a cancel
        // that never landed): ending it again is harmless.
        ServerState::Running if requested == Cancel => Transition::CancelDrain,
        ServerState::Starting | ServerState::Running => match requested {
            Start | Cancel | CheckUpdate => Transition::Stay,
            // A server still starting has no work to drain.
            gated if force || *state == ServerState::Starting => Transition::Shutdown(gated),
            gated => Transition::Drain(gated),
        },
        ServerState::Stopped | ServerState::Crashed { .. } => match requested {
            Start | Restart => Transition::Spawn,
            Stop if *state == ServerState::Stopped => Transition::Stay,
            Stop | Update => Transition::Settle(requested),
            Quit => Transition::Exit,
            Cancel | CheckUpdate => Transition::Stay,
        },
        // Not reachable without a pending action; nothing to change.
        ServerState::Draining { .. } | ServerState::Stopping | ServerState::Updating { .. } => {
            Transition::Stay
        }
    };
    Ok(transition)
}

/// Seconds to wait before restarting after the `crashes`-th crash in a row
/// (counting from 0): 1, 2, 4, 8, 16, then 30.
pub fn backoff_secs(crashes: u32) -> u64 {
    const DELAYS: [u64; 6] = [1, 2, 4, 8, 16, 30];
    DELAYS[(crashes as usize).min(DELAYS.len() - 1)]
}
