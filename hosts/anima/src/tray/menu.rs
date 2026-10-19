//! What the tray shows for a status: the menu, item by item, and which icon.
//! Pure, so it is tested without a tray.

// Only the macOS and Windows trays show it.
#![cfg_attr(not(any(target_os = "macos", windows)), allow(dead_code))]

use crate::supervisor::{Action, ServerState, SupervisorStatus};

/// The id of a separator; there is no label or state to it.
pub const SEPARATOR: &str = "sep";

/// One menu entry. `checked` is `Some` for a check item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItemModel {
    pub id: &'static str,
    pub label: String,
    pub enabled: bool,
    pub checked: Option<bool>,
}

impl MenuItemModel {
    fn new(id: &'static str, label: impl Into<String>, enabled: bool) -> Self {
        Self {
            id,
            label: label.into(),
            enabled,
            checked: None,
        }
    }

    fn separator() -> Self {
        Self::new(SEPARATOR, "", false)
    }
}

/// The icon: the Anima mark with a status dot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayIconKind {
    Running,
    /// Starting, stopping or updating.
    Starting,
    Stopped,
    /// Waiting for running work before an action.
    Waiting,
    /// Running, with an update to install.
    Update,
}

pub fn menu_model(status: &SupervisorStatus) -> Vec<MenuItemModel> {
    let up = matches!(
        status.state,
        ServerState::Running | ServerState::Draining { .. }
    );
    let down = matches!(
        status.state,
        ServerState::Stopped | ServerState::Crashed { .. }
    );
    let mut items = vec![
        MenuItemModel::new("open", "Open Console", up),
        MenuItemModel::new("status", status_line(status), false),
        MenuItemModel::separator(),
        if down {
            MenuItemModel::new("start", "Start", true)
        } else {
            MenuItemModel::new("stop", "Stop", true)
        },
        MenuItemModel::new("restart", "Restart", true),
        match &status.update.available {
            Some(version) => MenuItemModel::new("update", format!("Update to v{version}"), true),
            None => MenuItemModel::new("check", "Check for updates", true),
        },
    ];
    // Why the last update or check did not go through.
    if let Some(error) = &status.update.last_error {
        items.push(MenuItemModel::new("update-error", error.clone(), false));
    }
    // A gated action under way; a cancel being confirmed has nothing to cancel.
    if status
        .pending
        .is_some_and(|pending| pending != Action::Cancel)
    {
        let draining = matches!(status.state, ServerState::Draining { .. });
        items.push(MenuItemModel::new("cancel", "Cancel", draining));
    }
    items.push(MenuItemModel::separator());
    items.push(MenuItemModel {
        checked: Some(status.autostart),
        ..MenuItemModel::new("autostart", "Start at login", true)
    });
    items.push(MenuItemModel::new("quit", "Quit", true));
    items
}

/// The disabled line saying what Anima is doing.
fn status_line(status: &SupervisorStatus) -> String {
    if status.pending == Some(Action::Cancel) {
        return "Cancelling…".to_string();
    }
    match &status.state {
        ServerState::Running => {
            let mut line = format!("Running · v{}", status.version);
            match status.connectors {
                Some(1) => line.push_str(" · 1 connector"),
                Some(count) => line.push_str(&format!(" · {count} connectors")),
                None => {}
            }
            line
        }
        ServerState::Draining { in_flight, .. } => {
            format!("Waiting for {} tasks to finish", in_flight.total())
        }
        ServerState::Starting => "Starting…".to_string(),
        ServerState::Stopping => "Stopping…".to_string(),
        ServerState::Stopped => "Stopped".to_string(),
        ServerState::Crashed { message, .. } => message.clone(),
        ServerState::Updating { stage } => format!("Updating ({stage})"),
    }
}

pub fn icon_for(state: &ServerState, update_available: bool) -> TrayIconKind {
    match state {
        ServerState::Running if update_available => TrayIconKind::Update,
        ServerState::Running => TrayIconKind::Running,
        ServerState::Draining { .. } => TrayIconKind::Waiting,
        ServerState::Starting | ServerState::Stopping | ServerState::Updating { .. } => {
            TrayIconKind::Starting
        }
        ServerState::Stopped | ServerState::Crashed { .. } => TrayIconKind::Stopped,
    }
}
