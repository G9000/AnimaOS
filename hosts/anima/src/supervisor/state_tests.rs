use anima_daemon::system::InFlight;

use super::state::{backoff_secs, next, Action, Conflict, ServerState, Transition};

fn draining(action: Action) -> ServerState {
    ServerState::Draining {
        action,
        in_flight: InFlight::default(),
    }
}

#[test]
fn stop_while_running_begins_drain() {
    let transition = next(&ServerState::Running, None, Action::Stop, false).unwrap();
    assert_eq!(transition, Transition::Drain(Action::Stop));
    assert_eq!(transition.pending(), Some(Action::Stop));
}

#[test]
fn second_action_while_pending_conflicts() {
    assert_eq!(
        next(
            &draining(Action::Update),
            Some(Action::Update),
            Action::Stop,
            false
        ),
        Err(Conflict {
            pending: Action::Update
        })
    );
    // Too late to take another action once the shutdown is under way.
    assert_eq!(
        next(
            &ServerState::Stopping,
            Some(Action::Restart),
            Action::Quit,
            true
        ),
        Err(Conflict {
            pending: Action::Restart
        })
    );
}

#[test]
fn cancel_clears_pending() {
    let transition = next(
        &draining(Action::Stop),
        Some(Action::Stop),
        Action::Cancel,
        false,
    )
    .unwrap();
    assert_eq!(transition, Transition::CancelDrain);
    // The stop is no longer pending; the cancel is, until the server confirms.
    assert_eq!(transition.pending(), Some(Action::Cancel));
    let cancelling = |action| next(&draining(Action::Stop), Some(Action::Cancel), action, false);
    assert_eq!(cancelling(Action::Cancel), Ok(Transition::Stay));
    // A cancel under way blocks nothing: a gated action drains again.
    assert_eq!(
        cancelling(Action::Restart),
        Ok(Transition::Drain(Action::Restart))
    );
    assert_eq!(
        next(
            &ServerState::Running,
            Some(Action::Cancel),
            Action::Quit,
            true
        ),
        Ok(Transition::Shutdown(Action::Quit))
    );
    // Nothing to cancel once the server is being stopped.
    assert_eq!(
        next(
            &ServerState::Stopping,
            Some(Action::Stop),
            Action::Cancel,
            false
        ),
        Err(Conflict {
            pending: Action::Stop
        })
    );
    // While running, a cancel still ends any drain left on the server.
    assert_eq!(
        next(&ServerState::Running, None, Action::Cancel, false),
        Ok(Transition::CancelDrain)
    );
    assert_eq!(
        next(&ServerState::Stopped, None, Action::Cancel, false),
        Ok(Transition::Stay)
    );
}

#[test]
fn force_skips_waiting() {
    assert_eq!(
        next(&ServerState::Running, None, Action::Restart, true),
        Ok(Transition::Shutdown(Action::Restart))
    );
    // Asking again with force while draining stops waiting.
    assert_eq!(
        next(
            &draining(Action::Quit),
            Some(Action::Quit),
            Action::Quit,
            true
        ),
        Ok(Transition::Shutdown(Action::Quit))
    );
    // Asking again without force changes nothing.
    assert_eq!(
        next(
            &draining(Action::Quit),
            Some(Action::Quit),
            Action::Quit,
            false
        ),
        Ok(Transition::Stay)
    );
}

#[test]
fn start_when_stopped_spawns() {
    assert_eq!(
        next(&ServerState::Stopped, None, Action::Start, false),
        Ok(Transition::Spawn)
    );
    let crashed = ServerState::Crashed {
        message: "boom".into(),
        retry_in_secs: 4,
    };
    assert_eq!(
        next(&crashed, None, Action::Start, false),
        Ok(Transition::Spawn)
    );
    assert_eq!(
        next(&crashed, None, Action::Stop, false),
        Ok(Transition::Settle(Action::Stop))
    );
    assert_eq!(
        next(&ServerState::Running, None, Action::Start, false),
        Ok(Transition::Stay)
    );
}

#[test]
fn quit_from_stopped_exits() {
    assert_eq!(
        next(&ServerState::Stopped, None, Action::Quit, false),
        Ok(Transition::Exit)
    );
    // A server that is still starting has nothing to drain.
    assert_eq!(
        next(&ServerState::Starting, None, Action::Quit, false),
        Ok(Transition::Shutdown(Action::Quit))
    );
}

#[test]
fn backoff_sequence_caps_at_30() {
    let delays: Vec<u64> = (0..8).map(backoff_secs).collect();
    assert_eq!(delays, [1, 2, 4, 8, 16, 30, 30, 30]);
    assert_eq!(backoff_secs(u32::MAX), 30);
}

#[test]
fn json_shapes_are_kebab_and_camel_case() {
    let state = ServerState::Draining {
        action: Action::CheckUpdate,
        in_flight: InFlight::default(),
    };
    let value = serde_json::to_value(&state).unwrap();
    assert_eq!(value["kind"], "draining");
    assert_eq!(value["action"], "check-update");
    assert_eq!(value["inFlight"]["chatRuns"], 0);
    let crashed = serde_json::to_value(ServerState::Crashed {
        message: "m".into(),
        retry_in_secs: 2,
    })
    .unwrap();
    assert_eq!(crashed["kind"], "crashed");
    assert_eq!(crashed["retryInSecs"], 2);
    assert_eq!(
        serde_json::to_value(ServerState::Running).unwrap()["kind"],
        "running"
    );
}
