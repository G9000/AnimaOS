//! The lifecycle's handling of requests during an update, on a lifecycle with
//! no server child, in a temporary per-user folder. Nothing is spawned,
//! downloaded or re-executed.

use std::path::Path;

use super::*;

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn lifecycle(home: &Path) -> Lifecycle {
    let paths = AnimaPaths::from_home(home);
    paths.ensure().unwrap();
    let cfg = Config {
        port: free_port(),
        control_port: free_port(),
        autostart: false,
        ..Config::default()
    };
    cfg.save(&paths.config_file).unwrap();
    let (control_token, admin_token) = tokens::ensure(&paths).unwrap();
    let api = ServerApi::new(cfg.port, admin_token.clone(), None).unwrap();
    let status: SharedStatus = Arc::new(Mutex::new(SupervisorStatus {
        version: env!("CARGO_PKG_VERSION").to_string(),
        state: ServerState::Running,
        server_url: server_url(cfg.port),
        workspace: None,
        workspace_problem: None,
        data_dir: paths.data.clone(),
        autostart: false,
        connectors: None,
        pending: None,
        update: UpdateStatus::default(),
    }));
    let (api_tx, _) = watch::channel(api.clone());
    let (check_now, _) = mpsc::channel(1);
    let (update_events, _) = mpsc::unbounded_channel();
    Lifecycle {
        log: ServerLog::open(&paths.logs),
        exe: home.join("anima"),
        control_url: format!("http://127.0.0.1:{}", cfg.control_port),
        paths,
        cfg,
        control_token,
        admin_token,
        api,
        api_tx,
        updater: Updater::new(Arc::clone(&status)),
        status,
        child: None,
        crashes: 0,
        running_since: None,
        check_now,
        update_events,
        next_job: 0,
        job: None,
        prepared: None,
        handoff: None,
    }
}

/// Requests `action`, returning the next phase and the answer.
fn ask(
    lifecycle: &mut Lifecycle,
    phase: Phase,
    action: Action,
) -> (Option<Phase>, Result<SupervisorStatus, Conflict>) {
    let (reply, mut answer) = oneshot::channel();
    let next = lifecycle.handle(
        ActionRequest {
            action,
            force: false,
            reply,
        },
        phase,
    );
    (next, answer.try_recv().expect("answered at once"))
}

/// An update downloading (a job that never finishes), begun from `resume`.
fn preparing(lifecycle: &mut Lifecycle, resume: Resume) -> (Phase, tokio::task::AbortHandle) {
    lifecycle.set_state(
        ServerState::Updating {
            stage: "Downloading v9.9.9".to_string(),
        },
        Some(Action::Update),
    );
    let task = tokio::spawn(std::future::pending::<()>());
    let abort = task.abort_handle();
    lifecycle.job = Some(UpdateJob {
        id: 1,
        task,
        resume,
    });
    (Phase::Preparing { force: false }, abort)
}

async fn assert_aborted(abort: &tokio::task::AbortHandle) {
    for _ in 0..100 {
        if abort.is_finished() {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("the update job was not aborted");
}

#[tokio::test]
async fn cancel_while_preparing_drops_the_update() {
    let home = tempfile::tempdir().unwrap();
    let mut lifecycle = lifecycle(home.path());
    let (phase, abort) = preparing(&mut lifecycle, Resume::Stopped);

    let (next, answer) = ask(&mut lifecycle, phase, Action::Cancel);

    assert!(matches!(next, Some(Phase::Stopped)), "{next:?}");
    let status = answer.unwrap();
    assert_eq!(status.state, ServerState::Stopped);
    assert_eq!(status.pending, None);
    assert!(lifecycle.job.is_none());
    assert_aborted(&abort).await;
}

#[tokio::test]
async fn quit_while_preparing_drops_the_update_and_quits() {
    let home = tempfile::tempdir().unwrap();
    let mut lifecycle = lifecycle(home.path());
    // Begun while running; the server is gone, so there is nothing to drain.
    let (phase, abort) = preparing(&mut lifecycle, Resume::Running);

    let (next, answer) = ask(&mut lifecycle, phase, Action::Quit);

    assert!(matches!(next, Some(Phase::Exit)), "{next:?}");
    assert!(answer.is_ok());
    assert!(lifecycle.job.is_none());
    assert_aborted(&abort).await;
}

#[tokio::test]
async fn stop_while_preparing_drops_the_update_and_stops() {
    let home = tempfile::tempdir().unwrap();
    let mut lifecycle = lifecycle(home.path());
    let (phase, abort) = preparing(&mut lifecycle, Resume::Running);

    let (next, answer) = ask(&mut lifecycle, phase, Action::Stop);

    assert!(matches!(next, Some(Phase::Stopped)), "{next:?}");
    assert_eq!(answer.unwrap().state, ServerState::Stopped);
    assert_aborted(&abort).await;
}

#[tokio::test]
async fn start_while_preparing_still_conflicts() {
    let home = tempfile::tempdir().unwrap();
    let mut lifecycle = lifecycle(home.path());
    let (phase, _abort) = preparing(&mut lifecycle, Resume::Running);
    let (next, answer) = ask(&mut lifecycle, phase, Action::Start);
    assert!(next.is_none());
    assert_eq!(
        answer.unwrap_err(),
        Conflict {
            pending: Action::Update
        }
    );
    assert!(lifecycle.job.is_some());
}

fn awaiting_health(lifecycle: &mut Lifecycle) -> std::path::PathBuf {
    let pending = handoff::install(
        &lifecycle.paths,
        &lifecycle.exe,
        b"new",
        "0.0.1",
        env!("CARGO_PKG_VERSION"),
        |_| Ok(lifecycle.paths.root.join("anima.old")),
    )
    .unwrap();
    lifecycle.handoff = Some(Handoff {
        pending,
        wait: HandoffWait::Until(Instant::now() + handoff::HEALTH_TIMEOUT),
        reason: String::new(),
    });
    lifecycle.paths.run.join("update-pending.json")
}

fn wait(lifecycle: &Lifecycle) -> HandoffWait {
    lifecycle
        .handoff
        .as_ref()
        .expect("still awaiting health")
        .wait
}

#[tokio::test]
async fn stop_during_the_health_wait_pauses_it_until_the_next_start() {
    let home = tempfile::tempdir().unwrap();
    let mut lifecycle = lifecycle(home.path());
    let marker = awaiting_health(&mut lifecycle);
    lifecycle.set_state(ServerState::Starting, None);
    let starting = Phase::Starting {
        deadline: Instant::now() + READY_TIMEOUT,
    };

    let (next, _) = ask(&mut lifecycle, starting, Action::Stop);
    assert!(matches!(next, Some(Phase::Stopped)), "{next:?}");
    // No rollback (and no start) behind the owner's back.
    assert_eq!(wait(&lifecycle), HandoffWait::Paused);
    assert!(marker.exists());

    // Starting again waits the full time again.
    let (next, _) = ask(&mut lifecycle, Phase::Stopped, Action::Start);
    assert!(matches!(next, Some(Phase::Clearing { .. })), "{next:?}");
    match wait(&lifecycle) {
        HandoffWait::Until(at) => {
            assert!(at >= Instant::now() + handoff::HEALTH_TIMEOUT - Duration::from_secs(5))
        }
        other => panic!("the wait was not armed again: {other:?}"),
    }
    assert!(marker.exists());
}

#[tokio::test]
async fn quit_during_the_health_wait_keeps_the_pending_update() {
    let home = tempfile::tempdir().unwrap();
    let mut lifecycle = lifecycle(home.path());
    let marker = awaiting_health(&mut lifecycle);
    lifecycle.set_state(ServerState::Starting, None);
    let starting = Phase::Starting {
        deadline: Instant::now() + READY_TIMEOUT,
    };

    let (next, _) = ask(&mut lifecycle, starting, Action::Quit);
    assert!(matches!(next, Some(Phase::Exit)), "{next:?}");
    assert_eq!(wait(&lifecycle), HandoffWait::Paused);
    // The next supervisor of this version waits for health again.
    assert!(marker.exists());
}

#[tokio::test]
async fn a_crash_retry_does_not_extend_the_health_wait() {
    let home = tempfile::tempdir().unwrap();
    let mut lifecycle = lifecycle(home.path());
    awaiting_health(&mut lifecycle);
    let armed = wait(&lifecycle);
    let _ = lifecycle.start();
    assert_eq!(wait(&lifecycle), armed);
}
