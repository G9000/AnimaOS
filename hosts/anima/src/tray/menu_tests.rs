use anima_daemon::system::InFlight;

use super::menu::{icon_for, menu_model, MenuItemModel, TrayIconKind};
use crate::commands::status::sample_status;
use crate::supervisor::{Action, ServerState, SupervisorStatus};

fn ids(model: &[MenuItemModel]) -> Vec<&'static str> {
    model.iter().map(|item| item.id).collect()
}

fn item<'a>(model: &'a [MenuItemModel], id: &str) -> &'a MenuItemModel {
    model
        .iter()
        .find(|item| item.id == id)
        .unwrap_or_else(|| panic!("no `{id}` in {:?}", ids(model)))
}

fn draining(tasks: u32, action: Action) -> SupervisorStatus {
    let mut status = sample_status(ServerState::Draining {
        action,
        in_flight: InFlight {
            chat_runs: tasks,
            ..InFlight::default()
        },
    });
    status.pending = Some(action);
    status
}

#[test]
fn running_menu_items() {
    let mut status = sample_status(ServerState::Running);
    status.connectors = Some(2);
    let model = menu_model(&status);
    assert_eq!(
        ids(&model),
        [
            "open",
            "status",
            "sep",
            "stop",
            "restart",
            "check",
            "sep",
            "autostart",
            "quit"
        ]
    );
    assert!(item(&model, "open").enabled);
    assert_eq!(item(&model, "open").label, "Open Console");
    let line = item(&model, "status");
    assert_eq!(line.label, "Running · v1.2.3 · 2 connectors");
    assert!(!line.enabled);
    assert_eq!(item(&model, "check").label, "Check for updates");
    assert_eq!(item(&model, "autostart").checked, Some(true));
    assert_eq!(item(&model, "quit").label, "Quit");
}

#[test]
fn an_update_error_is_shown_under_the_update_item() {
    let mut status = sample_status(ServerState::Stopped);
    status.update.last_error = Some("Start Anima to finish the last update first".into());
    let model = menu_model(&status);
    let ids = ids(&model);
    let at = ids.iter().position(|id| *id == "update-error").unwrap();
    assert_eq!(ids[at - 1], "check");
    let line = item(&model, "update-error");
    assert_eq!(line.label, "Start Anima to finish the last update first");
    assert!(!line.enabled);
}

#[test]
fn stopped_shows_start_and_disables_open() {
    let mut status = sample_status(ServerState::Stopped);
    status.autostart = false;
    let model = menu_model(&status);
    assert_eq!(
        ids(&model),
        [
            "open",
            "status",
            "sep",
            "start",
            "restart",
            "check",
            "sep",
            "autostart",
            "quit"
        ]
    );
    assert!(!item(&model, "open").enabled);
    assert_eq!(item(&model, "status").label, "Stopped");
    assert_eq!(item(&model, "start").label, "Start");
    assert_eq!(item(&model, "autostart").checked, Some(false));
}

#[test]
fn crashed_shows_the_problem_and_start() {
    let status = sample_status(ServerState::Crashed {
        message: "Port 8787 is in use".to_string(),
        retry_in_secs: 4,
    });
    let model = menu_model(&status);
    assert_eq!(item(&model, "status").label, "Port 8787 is in use");
    assert!(ids(&model).contains(&"start"));
    assert!(!item(&model, "open").enabled);
}

#[test]
fn update_available_shows_update_label() {
    let mut status = sample_status(ServerState::Running);
    status.update.available = Some("0.2.1".to_string());
    let model = menu_model(&status);
    assert!(!ids(&model).contains(&"check"));
    assert_eq!(item(&model, "update").label, "Update to v0.2.1");
}

#[test]
fn draining_shows_waiting_and_cancel() {
    let model = menu_model(&draining(3, Action::Stop));
    assert_eq!(
        ids(&model),
        [
            "open",
            "status",
            "sep",
            "stop",
            "restart",
            "check",
            "cancel",
            "sep",
            "autostart",
            "quit"
        ]
    );
    assert_eq!(
        item(&model, "status").label,
        "Waiting for 3 tasks to finish"
    );
    assert!(item(&model, "open").enabled);
    let cancel = item(&model, "cancel");
    assert_eq!(cancel.label, "Cancel");
    assert!(cancel.enabled);
}

#[test]
fn a_cancel_being_confirmed_shows_cancelling_without_cancel() {
    let mut status = draining(3, Action::Stop);
    status.pending = Some(Action::Cancel);
    let model = menu_model(&status);
    assert_eq!(item(&model, "status").label, "Cancelling…");
    assert!(!ids(&model).contains(&"cancel"));
}

#[test]
fn icon_follows_the_state() {
    let crashed = ServerState::Crashed {
        message: "x".to_string(),
        retry_in_secs: 1,
    };
    let draining = ServerState::Draining {
        action: Action::Quit,
        in_flight: InFlight::default(),
    };
    assert_eq!(
        icon_for(&ServerState::Running, false),
        TrayIconKind::Running
    );
    assert_eq!(icon_for(&ServerState::Running, true), TrayIconKind::Update);
    assert_eq!(
        icon_for(&ServerState::Starting, true),
        TrayIconKind::Starting
    );
    assert_eq!(
        icon_for(&ServerState::Stopping, false),
        TrayIconKind::Starting
    );
    assert_eq!(icon_for(&draining, true), TrayIconKind::Waiting);
    assert_eq!(icon_for(&ServerState::Stopped, true), TrayIconKind::Stopped);
    assert_eq!(icon_for(&crashed, false), TrayIconKind::Stopped);
}
