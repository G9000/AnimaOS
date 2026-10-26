#[path = "fixtures/execution_peer.rs"]
mod peer;

use anima_core::*;
use anima_execution_store_sqlite::SqliteExecutionStore;
use std::{
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Command, Stdio},
    sync::{mpsc, Arc},
    time::Duration,
};
use uuid::Uuid;

const LEASE_RUN: Uuid = Uuid::from_u128(301);
const LEASE_SESSION: Uuid = Uuid::from_u128(302);
const INITIAL_INPUT: &str = "stable request input survives process death";

#[tokio::test]
#[ignore = "parent-controlled writer process"]
async fn restart_writer_process() {
    let directory = std::env::var("ANIMA_EXECUTION_RESTART_DIRECTORY").unwrap();
    let path = Path::new(&directory);
    assert!(matches!(
        peer::execute(path, false, false).await,
        EngineRunOutcome::Completed { .. }
    ));
    let store = SqliteExecutionStore::open(
        path.join("execution.sqlite"),
        peer::protection(false),
        Arc::new(ManualExecutionClock::new(1_000_000)),
    )
    .await
    .unwrap();
    let session = Session::new(
        LEASE_SESSION,
        "dialogue",
        1,
        SessionConcurrencyPolicy::Serial,
    )
    .unwrap();
    let run = Run::queued(LEASE_RUN, LEASE_SESSION, "dialogue", 1).unwrap();
    let stored = store
        .create_run(
            peer::OWNER,
            CreateRun::new_for_owner(
                peer::OWNER,
                session,
                run,
                0,
                SessionConcurrencyPolicy::Serial,
            )
            .with_initial_input(
                DurableRunInput::text(peer::OWNER, LEASE_SESSION, LEASE_RUN, INITIAL_INPUT)
                    .unwrap(),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    let lease = store
        .acquire_lease(peer::OWNER, LEASE_RUN, stored.run_version(), 100)
        .await
        .unwrap();
    let acknowledged = store
        .replay_events(peer::OWNER, peer::RUN, StoreReadPage::first(2).unwrap())
        .await
        .unwrap();
    assert_eq!(acknowledged.events().len(), 2);
    let cursor = acknowledged
        .next_cursor()
        .expect("fixture must have more semantic events");
    std::fs::write(path.join("lease.json"), serde_json::to_vec(&lease).unwrap()).unwrap();
    std::fs::write(
        path.join("acknowledged.json"),
        serde_json::to_vec(&serde_json::json!({
            "cursor": cursor.as_str(),
            "sequence": acknowledged.events().last().unwrap().sequence(),
        }))
        .unwrap(),
    )
    .unwrap();
    println!("READY:committed-with-acknowledged-cursor");
    std::io::stdout().flush().unwrap();
    let mut input = String::new();
    std::io::stdin().lock().read_line(&mut input).unwrap();
    panic!("parent must kill the live writer at its handshake");
}

#[tokio::test]
#[ignore = "fresh verifier process"]
async fn restart_verifier_process() {
    let directory = std::env::var("ANIMA_EXECUTION_RESTART_DIRECTORY").unwrap();
    let path = Path::new(&directory);
    let store = SqliteExecutionStore::open(
        path.join("execution.sqlite"),
        peer::protection(false),
        Arc::new(ManualExecutionClock::new(1_100_000)),
    )
    .await
    .unwrap();
    let old: ExecutionLease =
        serde_json::from_slice(&std::fs::read(path.join("lease.json")).unwrap()).unwrap();
    let stored = store
        .load_run(peer::OWNER, LEASE_RUN)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.initial_input().text_value(), INITIAL_INPUT);
    assert_eq!(stored.run().session_id(), LEASE_SESSION);
    assert!(store
        .renew_lease(peer::OWNER, old.clone(), 100)
        .await
        .is_err());
    let replacement = store
        .acquire_lease(peer::OWNER, LEASE_RUN, stored.run_version(), 100)
        .await
        .unwrap();
    assert_ne!(replacement, old);
    assert!(store.renew_lease(peer::OWNER, old, 100).await.is_err());
    assert!(store
        .load_run(Uuid::new_v4(), LEASE_RUN)
        .await
        .unwrap()
        .is_none());
    let acknowledged: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path.join("acknowledged.json")).unwrap()).unwrap();
    let mut last = acknowledged["sequence"].as_u64().unwrap();
    let mut cursor =
        StoreReadCursor::from_opaque(acknowledged["cursor"].as_str().unwrap()).unwrap();
    let mut replayed = Vec::new();
    loop {
        let page = store
            .replay_events(
                peer::OWNER,
                peer::RUN,
                StoreReadPage::after(cursor, 2).unwrap(),
            )
            .await
            .unwrap();
        for event in page.events() {
            assert_eq!(
                event.sequence(),
                last + 1,
                "no acknowledged event may be replayed or skipped"
            );
            last = event.sequence();
            replayed.push(event.kind());
        }
        match page.next_cursor() {
            Some(next) => cursor = next.clone(),
            None => break,
        }
    }
    assert!(replayed.contains(&RuntimeEventKind::RunCompleted));
    assert!(!replayed.is_empty());
    assert_eq!(
        store
            .load_completed_output(peer::OWNER, peer::RUN)
            .await
            .unwrap()
            .unwrap()
            .text,
        "recovered dialogue"
    );
    store.shutdown().await.unwrap();
}

#[test]
fn killed_writer_reopens_in_a_fresh_process_with_fences_input_and_cursor_intact() {
    let temp = tempfile::tempdir().unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "restart_writer_process",
            "--nocapture",
            "--quiet",
        ])
        .env("ANIMA_EXECUTION_RESTART_DIRECTORY", temp.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let output = child.stdout.take().unwrap();
    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(output).lines() {
            let line = line.unwrap();
            if line.starts_with("READY:") {
                let _ = send.send(line);
                return;
            }
        }
    });
    let ready = receive.recv_timeout(Duration::from_secs(30));
    child.kill().unwrap();
    child.wait().unwrap();
    assert_eq!(ready.unwrap(), "READY:committed-with-acknowledged-cursor");
    let verifier = Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "restart_verifier_process",
            "--nocapture",
            "--quiet",
        ])
        .env("ANIMA_EXECUTION_RESTART_DIRECTORY", temp.path())
        .output()
        .unwrap();
    assert!(
        verifier.status.success(),
        "fresh verifier failed: {}\n{}",
        String::from_utf8_lossy(&verifier.stdout),
        String::from_utf8_lossy(&verifier.stderr)
    );
}
