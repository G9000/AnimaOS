#[path = "fixtures/execution_peer.rs"]
mod peer;

use anima_core::*;
use std::{
    io::{BufRead, BufReader},
    process::{Command, Stdio},
    sync::mpsc,
    time::Duration,
};

#[tokio::test]
#[ignore = "parent-controlled process fixture"]
async fn fixture_process() {
    let path = std::env::var("ANIMA_EXECUTION_TEST_DIRECTORY").unwrap();
    let manual = std::env::var("ANIMA_EXECUTION_TEST_MANUAL").ok().as_deref() == Some("1");
    peer::execute(std::path::Path::new(&path), manual, false).await;
}

fn kill_at(temp: &std::path::Path, point: &str, manual: bool) {
    let mut child = Command::new(std::env::current_exe().unwrap())
        // Keep the readiness protocol independent of libtest's single-thread
        // progress prefix, which otherwise shares the marker's stdout line.
        .args([
            "--ignored",
            "--exact",
            "fixture_process",
            "--nocapture",
            "--quiet",
        ])
        .env("ANIMA_EXECUTION_TEST_DIRECTORY", temp)
        .env("ANIMA_EXECUTION_TEST_POINT", point)
        .env(
            "ANIMA_EXECUTION_TEST_MANUAL",
            if manual { "1" } else { "0" },
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (send, recv) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let line = line.unwrap();
            if line.starts_with("READY:") {
                let _ = send.send(line);
                return;
            }
        }
    });
    // A watchdog detects a broken fixture; it does not select the crash boundary.
    let ready = recv.recv_timeout(Duration::from_secs(30));
    child.kill().unwrap();
    child.wait().unwrap();
    assert_eq!(ready.unwrap(), format!("READY:{point}"));
}

#[tokio::test]
async fn actual_process_death_at_each_durable_boundary_recovers_one_physical_effect() {
    for point in [
        "model-checkpoint",
        "dispatch-before-effect",
        "effect-before-result",
        "result-committed",
        "terminal-committed",
    ] {
        let temp = tempfile::tempdir().unwrap();
        kill_at(temp.path(), point, false);
        let outcome = peer::execute(temp.path(), false, true).await;
        assert!(
            matches!(outcome, EngineRunOutcome::Completed { ref content } if content.text == "recovered dialogue"),
            "{point}: {outcome:?}"
        );
        let ledger = rusqlite::Connection::open(temp.path().join("effects.sqlite")).unwrap();
        assert_eq!(
            ledger
                .query_row::<i64, _, _>("SELECT SUM(executions) FROM effects", [], |row| row.get(0))
                .unwrap(),
            1,
            "{point}"
        );
    }
}

#[tokio::test]
async fn uncertain_non_idempotent_effect_is_never_replayed() {
    let temp = tempfile::tempdir().unwrap();
    kill_at(temp.path(), "effect-before-result", true);
    assert_eq!(
        peer::execute(temp.path(), true, true).await,
        EngineRunOutcome::RecoveryRequired
    );
    let ledger = rusqlite::Connection::open(temp.path().join("effects.sqlite")).unwrap();
    assert_eq!(
        ledger
            .query_row::<i64, _, _>("SELECT SUM(executions) FROM effects", [], |row| row.get(0))
            .unwrap(),
        1
    );
}
