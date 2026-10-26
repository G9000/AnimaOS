use anima_core::*;
use anima_execution_store_sqlite::SqliteExecutionStore;
use std::{path::Path, sync::Arc};
use uuid::Uuid;

fn request(owner: Uuid, input: &str) -> CreateRun {
    let session = Session::new(
        Uuid::new_v4(),
        "capacity",
        1,
        SessionConcurrencyPolicy::Serial,
    )
    .unwrap();
    let run = Run::queued(Uuid::new_v4(), session.id(), "capacity", 1).unwrap();
    let input = DurableRunInput::text(owner, session.id(), run.id(), input).unwrap();
    CreateRun::new_for_owner(owner, session, run, 0, SessionConcurrencyPolicy::Serial)
        .with_initial_input(input)
        .unwrap()
}

fn authority(connection: &rusqlite::Connection) -> (i64, Vec<u8>) {
    connection
        .query_row(
            "SELECT generation,snapshot FROM execution_authority WHERE singleton=1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap()
}

async fn stale_commit_is_rejected_after_reopen(
    path: &Path,
    protection: PersistenceProtection,
    owner: Uuid,
    stored: StoredRun,
    lease: ExecutionLease,
) {
    let store =
        SqliteExecutionStore::open(path, protection, Arc::new(ManualExecutionClock::new(1)))
            .await
            .unwrap();
    let session = stored.run().session_id();
    let run = stored.run().id();
    let commit = ExecutionCommit::new(
        stored.run_version(),
        0,
        lease.clone(),
        RuntimeCommand::start(Uuid::new_v4(), session, run).unwrap(),
        vec![RuntimeEvent::new(
            Uuid::new_v4(),
            owner,
            session,
            run,
            1,
            1,
            RuntimeEventKind::RunStarted,
        )
        .unwrap()],
        vec![],
        vec![],
        vec![],
        None,
        stored.run().transition(RunState::Running, None).unwrap(),
    );
    assert_eq!(
        store
            .commit_execution(owner, commit)
            .await
            .unwrap_err()
            .code(),
        ExecutionStoreErrorCode::LeaseExpired,
        "export rejection must not let an observed expired fence commit after clock rollback"
    );
    assert_eq!(
        store
            .renew_lease(owner, lease, 100)
            .await
            .unwrap_err()
            .code(),
        ExecutionStoreErrorCode::LeaseExpired
    );
    assert_eq!(
        store
            .load_run(owner, run)
            .await
            .unwrap()
            .unwrap()
            .run()
            .state(),
        RunState::Queued
    );
    store.shutdown().await.unwrap();
}

#[tokio::test]
async fn snapshot_capacity_rejection_persists_only_clock_floor_and_never_revives_a_fence() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("capacity-clock.sqlite");
    let clock = Arc::new(ManualExecutionClock::new(1_000));
    let protection = PersistenceProtection::allow_model_payloads(
        PersistenceSnapshotSealKey::new([97; 32]).unwrap(),
        vec![],
    )
    .unwrap();
    let store = SqliteExecutionStore::open(&path, protection.clone(), clock.clone())
        .await
        .unwrap();
    let owner = Uuid::new_v4();
    let reference = InMemoryExecutionStore::with_clock(clock.clone());
    // Keep this run payload-free so a start commit would fit even at capacity:
    // the regression must prove fence rejection rather than another size error.
    let initial = request(owner, "");
    let stored = reference.create_run(owner, initial).await.unwrap();
    let lease = reference
        .acquire_lease(owner, stored.run().id(), stored.run_version(), 100)
        .await
        .unwrap();
    let base = reference.export_snapshot(&protection).await.unwrap();
    let input = "capacity-fixture".repeat(2048);
    reference
        .create_run(owner, request(owner, &input))
        .await
        .unwrap();
    let first = reference.export_snapshot(&protection).await.unwrap();
    let per_run = first.as_bytes().len() - base.as_bytes().len();
    let seed_count =
        ((ExecutionStoreSnapshot::maximum_bytes() - base.as_bytes().len()) * 97 / 100 / per_run)
            .clamp(1, 900);
    for _ in 1..seed_count {
        reference
            .create_run(owner, request(owner, &input))
            .await
            .unwrap();
    }
    let mut snapshot = reference.export_snapshot(&protection).await.unwrap();
    let mut count = seed_count;
    let mut rejected = None;
    for _ in 0..1000 {
        let next = request(owner, &input);
        reference.create_run(owner, next.clone()).await.unwrap();
        match reference.export_snapshot(&protection).await {
            Ok(next_snapshot) => {
                snapshot = next_snapshot;
                count += 1;
            }
            Err(error) => {
                assert_eq!(error.code(), ExecutionStoreErrorCode::InvalidRequest);
                rejected = Some(next);
                break;
            }
        }
    }
    let rejected = rejected.expect("fixture must reach the snapshot capacity bound");
    assert!(snapshot.as_bytes().len() > ExecutionStoreSnapshot::maximum_bytes() * 99 / 100);
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute(
            "UPDATE execution_authority SET generation=?1,snapshot=?2 WHERE singleton=1",
            rusqlite::params![count as i64 + 2, snapshot.as_bytes()],
        )
        .unwrap();
    let before = authority(&connection);
    clock.advance_ms(100).unwrap();
    assert_eq!(
        store
            .create_run(owner, rejected.clone())
            .await
            .unwrap_err()
            .code(),
        ExecutionStoreErrorCode::InvalidRequest
    );
    assert_eq!(
        authority(&connection),
        before,
        "failed export must not publish its candidate or generation"
    );
    store.shutdown().await.unwrap();
    stale_commit_is_rejected_after_reopen(&path, protection.clone(), owner, stored, lease).await;
    let reopened =
        SqliteExecutionStore::open(&path, protection, Arc::new(ManualExecutionClock::new(1)))
            .await
            .unwrap();
    assert!(reopened
        .load_run(owner, rejected.run().id())
        .await
        .unwrap()
        .is_none());
    assert_eq!(authority(&connection), before);
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT clock_floor_ms FROM execution_authority",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        1_100
    );
    reopened.shutdown().await.unwrap();
}

#[tokio::test]
async fn protection_rejection_persists_only_clock_floor_and_never_revives_a_fence() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("protection-clock.sqlite");
    let clock = Arc::new(ManualExecutionClock::new(1_000));
    let protection =
        PersistenceProtection::payload_free(PersistenceSnapshotSealKey::new([98; 32]).unwrap())
            .unwrap();
    let store = SqliteExecutionStore::open(&path, protection.clone(), clock.clone())
        .await
        .unwrap();
    let owner = Uuid::new_v4();
    let initial = request(owner, "");
    let stored = store.create_run(owner, initial).await.unwrap();
    let lease = store
        .acquire_lease(owner, stored.run().id(), stored.run_version(), 100)
        .await
        .unwrap();
    let connection = rusqlite::Connection::open(&path).unwrap();
    let before = authority(&connection);
    clock.advance_ms(100).unwrap();
    let rejected = request(owner, "payload-free protection must reject this input");
    assert_eq!(
        store
            .create_run(owner, rejected.clone())
            .await
            .unwrap_err()
            .code(),
        ExecutionStoreErrorCode::InvalidRequest
    );
    assert_eq!(authority(&connection), before);
    store.shutdown().await.unwrap();
    stale_commit_is_rejected_after_reopen(&path, protection, owner, stored, lease).await;
    assert_eq!(authority(&connection), before);
}

#[tokio::test]
async fn rejected_export_with_failed_floor_commit_reports_unavailable_without_publication() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("floor-commit.sqlite");
    let clock = Arc::new(ManualExecutionClock::new(1_000));
    let protection =
        PersistenceProtection::payload_free(PersistenceSnapshotSealKey::new([99; 32]).unwrap())
            .unwrap();
    let store = SqliteExecutionStore::open(&path, protection, clock.clone())
        .await
        .unwrap();
    let connection = rusqlite::Connection::open(&path).unwrap();
    let before = authority(&connection);
    connection.execute_batch("CREATE TABLE floor_parent(id INTEGER PRIMARY KEY); CREATE TABLE floor_child(parent_id INTEGER REFERENCES floor_parent(id) DEFERRABLE INITIALLY DEFERRED); CREATE TRIGGER fail_floor_commit AFTER UPDATE ON execution_authority BEGIN INSERT INTO floor_child VALUES(999); END;").unwrap();
    clock.advance_ms(100).unwrap();
    let owner = Uuid::new_v4();
    let rejected = request(owner, "blocked payload");
    assert_eq!(
        store
            .create_run(owner, rejected.clone())
            .await
            .unwrap_err()
            .code(),
        ExecutionStoreErrorCode::StorageUnavailable,
        "a real floor COMMIT failure must override the export error"
    );
    assert_eq!(authority(&connection), before);
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT clock_floor_ms FROM execution_authority",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        1_000
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>("SELECT COUNT(*) FROM floor_child", [], |row| row.get(0))
            .unwrap(),
        0
    );
    connection
        .execute_batch("DROP TRIGGER fail_floor_commit")
        .unwrap();
    assert!(store
        .load_run(owner, rejected.run().id())
        .await
        .unwrap()
        .is_none());
    store.shutdown().await.unwrap();
}
