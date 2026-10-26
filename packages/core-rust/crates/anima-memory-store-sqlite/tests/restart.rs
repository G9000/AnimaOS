use anima_core::{
    IdentityBinding, ManualExecutionClock, MemoryKind, MemoryPortError, MemoryProvenance,
    MemoryScope, MemoryWrite, WorldResidentBinding,
};
use anima_memory::{
    CandidateEvidence, EvidenceKind, IdentityRegistry, MemoryAuthority, MemoryCandidate,
    MemoryEffect, MemoryProjectionPolicy, MemoryProjectionPort,
};
use anima_memory_store_sqlite::SqliteMemoryStore;
use std::{
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
    sync::Arc,
};

struct Accepted;
impl MemoryProjectionPolicy for Accepted {
    fn validate(&self, _: &MemoryAuthority, _: &MemoryCandidate) -> Result<(), MemoryPortError> {
        Ok(())
    }
}
fn authority() -> MemoryAuthority {
    let scope = MemoryScope::agent("tavi-world-a").unwrap();
    MemoryAuthority::trusted(
        IdentityBinding::new("owner", "tavi-world-a").unwrap(),
        vec![scope.clone()],
        vec![scope],
    )
    .unwrap()
}
fn effect(authority: &MemoryAuthority) -> anima_memory::ValidatedMemoryEffect {
    let write = MemoryWrite::new(
        "tea",
        MemoryKind::Preference,
        MemoryScope::agent("tavi-world-a").unwrap(),
        "Prefers black tea",
        1.0,
        MemoryProvenance::new("user", "accepted-event", 10).unwrap(),
    )
    .unwrap();
    let candidate = MemoryCandidate::new(
        write,
        CandidateEvidence::new(EvidenceKind::UserStatement, "accepted-event", None).unwrap(),
    )
    .unwrap();
    MemoryEffect::new(
        "accepted-event",
        authority.identity().clone(),
        vec![candidate],
        vec![],
        vec![],
    )
    .unwrap()
    .validate(authority, &Accepted)
    .unwrap()
}

/// Invoked in a physically separate process, never against real user storage.
#[test]
fn memory_process_peer() {
    let Ok(path) = std::env::var("ANIMA_MEMORY_TEST_DB") else {
        return;
    };
    let mode = std::env::var("ANIMA_MEMORY_TEST_MODE").unwrap();
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            let store = SqliteMemoryStore::open(&path, Arc::new(ManualExecutionClock::default()))
                .await
                .unwrap();
            let auth = authority();
            if mode == "commit-and-park" {
                store
                    .register_identity(auth.identity().clone())
                    .await
                    .unwrap();
                store
                    .register_resident(
                        WorldResidentBinding::new(auth.identity().clone(), "world-a", "tavi")
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                store
                    .bind(auth.clone())
                    .await
                    .unwrap()
                    .apply_effect(effect(&auth))
                    .await
                    .unwrap();
                // Simulates process death after memory commit and before source-outbox acknowledgement.
                println!("MEMORY_COMMITTED");
                std::io::stdout().flush().unwrap();
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(60));
                }
            } else {
                let bound = store.bind(auth.clone()).await.unwrap();
                let records = bound.recent(10).await.unwrap();
                assert_eq!(records.len(), 1);
                assert_eq!(records[0].content(), "Prefers black tea");
                assert!(bound.apply_effect(effect(&auth)).await.unwrap().replayed);
                assert_eq!(
                    store
                        .resident_identity("owner", "world-a", "tavi")
                        .await
                        .unwrap(),
                    Some(auth.identity().clone())
                );
                assert_eq!(
                    store
                        .resident_identity("owner", "world-b", "tavi")
                        .await
                        .unwrap(),
                    None
                );
                store.shutdown().await.unwrap();
                println!("MEMORY_VERIFIED");
            }
        });
}

#[test]
fn actual_process_death_preserves_committed_memory_and_retry_receipt() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("process.sqlite");
    let exe = std::env::current_exe().unwrap();
    let mut child = Command::new(&exe)
        .args(["--exact", "memory_process_peer", "--nocapture"])
        .env("ANIMA_MEMORY_TEST_DB", &path)
        .env("ANIMA_MEMORY_TEST_MODE", "commit-and-park")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (ready, waiting) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if line.unwrap().contains("MEMORY_COMMITTED") {
                let _ = ready.send(());
                return;
            }
        }
    });
    let result = waiting.recv_timeout(std::time::Duration::from_secs(15));
    child.kill().unwrap();
    let _ = child.wait().unwrap();
    reader.join().unwrap();
    result.unwrap();
    let reopened = Command::new(exe)
        .args(["--exact", "memory_process_peer", "--nocapture"])
        .env("ANIMA_MEMORY_TEST_DB", path)
        .env("ANIMA_MEMORY_TEST_MODE", "verify")
        .output()
        .unwrap();
    assert!(
        reopened.status.success(),
        "{}",
        String::from_utf8_lossy(&reopened.stderr)
    );
    assert!(String::from_utf8_lossy(&reopened.stdout).contains("MEMORY_VERIFIED"));
}
