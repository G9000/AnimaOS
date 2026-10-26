use anima_core::*;
use anima_execution_store_sqlite::SqliteExecutionStore;
use std::sync::Arc;
use uuid::Uuid;
mod common;

async fn queued(store: &SqliteExecutionStore) -> (Uuid, Uuid, StoredRun) {
    let owner = Uuid::new_v4();
    let session = Session::new(
        Uuid::new_v4(),
        "dialogue",
        1,
        SessionConcurrencyPolicy::Serial,
    )
    .unwrap();
    let run = Run::queued(Uuid::new_v4(), session.id(), "dialogue", 1).unwrap();
    let id = run.id();
    let stored = store
        .create_run(
            owner,
            CreateRun::new_for_owner(owner, session, run, 0, SessionConcurrencyPolicy::Serial),
        )
        .await
        .unwrap();
    (owner, id, stored)
}

#[tokio::test]
async fn independent_connections_compete_for_one_fence_and_never_resurrect_it() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("execution.sqlite");
    let clock = Arc::new(ManualExecutionClock::new(1_000));
    let a = SqliteExecutionStore::open(&path, common::protection(), clock.clone())
        .await
        .unwrap();
    let b = SqliteExecutionStore::open(&path, common::protection(), clock.clone())
        .await
        .unwrap();
    let (owner, id, stored) = queued(&a).await;
    let (left, right) = tokio::join!(
        a.acquire_lease(owner, id, stored.run_version(), 100),
        b.acquire_lease(owner, id, stored.run_version(), 100)
    );
    assert_ne!(left.is_ok(), right.is_ok());
    let fence = left.or(right).unwrap();
    clock.advance_ms(100).unwrap();
    assert!(a.renew_lease(owner, fence.clone(), 100).await.is_err());
    a.shutdown().await.unwrap();
    b.shutdown().await.unwrap();
    // The last observed clock floor survives a host clock rollback and process reopen.
    let reopened = SqliteExecutionStore::open(
        &path,
        common::protection(),
        Arc::new(ManualExecutionClock::new(1)),
    )
    .await
    .unwrap();
    assert!(reopened.renew_lease(owner, fence, 100).await.is_err());
    assert!(reopened
        .acquire_lease(owner, id, stored.run_version(), 100)
        .await
        .is_ok());
    assert!(reopened
        .load_run(Uuid::new_v4(), id)
        .await
        .unwrap()
        .is_none());
    reopened.shutdown().await.unwrap();
}

#[tokio::test]
async fn failed_database_write_does_not_expose_the_candidate() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("execution.sqlite");
    let store = SqliteExecutionStore::open(
        &path,
        common::protection(),
        Arc::new(ManualExecutionClock::default()),
    )
    .await
    .unwrap();
    let owner = Uuid::new_v4();
    let initial = AuthoritativePolicyState::active(owner, "assistant", 1, 1, None).unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("CREATE TRIGGER fail_commit BEFORE UPDATE ON execution_authority BEGIN SELECT RAISE(ABORT, 'fixture commit failure'); END;").unwrap();
    assert_eq!(
        store
            .apply_authoritative_policy(owner, AuthoritativePolicyChange::create(initial.clone()))
            .await
            .unwrap_err()
            .code(),
        ExecutionStoreErrorCode::StorageUnavailable
    );
    conn.execute_batch("DROP TRIGGER fail_commit;").unwrap();
    assert!(store
        .load_authoritative_policy(owner, "assistant", 1)
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        store
            .apply_authoritative_policy(owner, AuthoritativePolicyChange::create(initial.clone()))
            .await
            .unwrap(),
        initial
    );
    store.shutdown().await.unwrap();
}

#[tokio::test]
async fn simultaneous_policy_creation_has_one_winner() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("execution.sqlite");
    let clock = Arc::new(ManualExecutionClock::default());
    let a = SqliteExecutionStore::open(&path, common::protection(), clock.clone())
        .await
        .unwrap();
    let b = SqliteExecutionStore::open(&path, common::protection(), clock)
        .await
        .unwrap();
    let owner = Uuid::new_v4();
    let state = AuthoritativePolicyState::active(owner, "assistant", 1, 1, None).unwrap();
    let (left, right) = tokio::join!(
        a.apply_authoritative_policy(owner, AuthoritativePolicyChange::create(state.clone())),
        b.apply_authoritative_policy(owner, AuthoritativePolicyChange::create(state))
    );
    assert_ne!(left.is_ok(), right.is_ok());
    a.shutdown().await.unwrap();
    b.shutdown().await.unwrap();
}

async fn cancel_for_retry(store: &SqliteExecutionStore, owner: Uuid, id: Uuid, stored: &StoredRun) {
    let session = stored.run().session_id();
    let lease = store
        .acquire_lease(owner, id, stored.run_version(), 1000)
        .await
        .unwrap();
    let running = stored.run().transition(RunState::Running, None).unwrap();
    let started = store
        .commit_execution(
            owner,
            ExecutionCommit::new(
                stored.run_version(),
                0,
                lease.clone(),
                RuntimeCommand::start(Uuid::new_v4(), session, id).unwrap(),
                vec![RuntimeEvent::new(
                    Uuid::new_v4(),
                    owner,
                    session,
                    id,
                    1,
                    1,
                    RuntimeEventKind::RunStarted,
                )
                .unwrap()],
                vec![],
                vec![],
                vec![],
                None,
                running.clone(),
            ),
        )
        .await
        .unwrap();
    store
        .commit_execution(
            owner,
            ExecutionCommit::new(
                started.stored_run().run_version(),
                0,
                lease,
                RuntimeCommand::cancel(Uuid::new_v4(), session, id).unwrap(),
                vec![RuntimeEvent::new(
                    Uuid::new_v4(),
                    owner,
                    session,
                    id,
                    2,
                    2,
                    RuntimeEventKind::RunCancelled,
                )
                .unwrap()],
                vec![],
                vec![],
                vec![],
                None,
                running.transition(RunState::Cancelled, None).unwrap(),
            ),
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn concurrent_retry_receipts_replay_and_conflicting_payload_never_creates_a_run() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("retry.sqlite");
    let clock = Arc::new(ManualExecutionClock::default());
    let left = SqliteExecutionStore::open(&path, common::protection(), clock.clone())
        .await
        .unwrap();
    let right = SqliteExecutionStore::open(&path, common::protection(), clock)
        .await
        .unwrap();
    let (owner, source, stored) = queued(&left).await;
    cancel_for_retry(&left, owner, source, &stored).await;
    let command = Uuid::new_v4();
    let retry = Uuid::new_v4();
    let request = RetryRun::new(command, source, retry).unwrap();
    let (a, b) = tokio::join!(
        left.retry_run(owner, request.clone()),
        right.retry_run(owner, request)
    );
    assert_eq!(a.unwrap(), b.unwrap());
    let conflicting = Uuid::new_v4();
    assert_eq!(
        right
            .retry_run(owner, RetryRun::new(command, source, conflicting).unwrap())
            .await
            .unwrap_err()
            .code(),
        ExecutionStoreErrorCode::CommandConflict
    );
    assert!(left.load_run(owner, conflicting).await.unwrap().is_none());
    left.shutdown().await.unwrap();
    right.shutdown().await.unwrap();
}

#[tokio::test]
async fn independent_connections_cannot_admit_two_serial_session_claims() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("serial.sqlite");
    let clock = Arc::new(ManualExecutionClock::default());
    let left = SqliteExecutionStore::open(&path, common::protection(), clock.clone())
        .await
        .unwrap();
    let right = SqliteExecutionStore::open(&path, common::protection(), clock)
        .await
        .unwrap();
    let owner = Uuid::new_v4();
    let session = Session::new(
        Uuid::new_v4(),
        "assistant",
        1,
        SessionConcurrencyPolicy::Serial,
    )
    .unwrap();
    let a = CreateRun::new_for_owner(
        owner,
        session.clone(),
        Run::queued(Uuid::new_v4(), session.id(), "assistant", 1).unwrap(),
        0,
        SessionConcurrencyPolicy::Serial,
    );
    let b = CreateRun::new_for_owner(
        owner,
        session.clone(),
        Run::queued(Uuid::new_v4(), session.id(), "assistant", 1).unwrap(),
        0,
        SessionConcurrencyPolicy::Serial,
    );
    let (a, b) = tokio::join!(left.create_run(owner, a), right.create_run(owner, b));
    assert_ne!(a.is_ok(), b.is_ok());
    left.shutdown().await.unwrap();
    right.shutdown().await.unwrap();
}

#[tokio::test]
async fn actual_commit_failure_rolls_back_the_sealed_candidate_and_generation() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("commit.sqlite");
    let store = SqliteExecutionStore::open(
        &path,
        common::protection(),
        Arc::new(ManualExecutionClock::default()),
    )
    .await
    .unwrap();
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection.execute_batch("CREATE TABLE fixture_parent(id INTEGER PRIMARY KEY); CREATE TABLE fixture_child(parent_id INTEGER REFERENCES fixture_parent(id) DEFERRABLE INITIALLY DEFERRED); CREATE TRIGGER fixture_commit_failure AFTER UPDATE ON execution_authority BEGIN INSERT INTO fixture_child VALUES(999); END;").unwrap();
    let owner = Uuid::new_v4();
    let candidate = AuthoritativePolicyState::active(owner, "commit-failure", 1, 1, None).unwrap();
    assert_eq!(
        store
            .apply_authoritative_policy(owner, AuthoritativePolicyChange::create(candidate))
            .await
            .unwrap_err()
            .code(),
        ExecutionStoreErrorCode::StorageUnavailable
    );
    connection
        .execute_batch("DROP TRIGGER fixture_commit_failure;")
        .unwrap();
    assert_eq!(
        connection
            .query_row::<i64, _, _>("SELECT generation FROM execution_authority", [], |row| row
                .get(0))
            .unwrap(),
        0
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>("SELECT COUNT(*) FROM fixture_child", [], |row| row.get(0))
            .unwrap(),
        0
    );
    assert!(store
        .load_authoritative_policy(owner, "commit-failure", 1)
        .await
        .unwrap()
        .is_none());
    store.shutdown().await.unwrap();
}
