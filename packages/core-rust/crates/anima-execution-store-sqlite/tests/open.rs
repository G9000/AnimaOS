use anima_core::{ManualExecutionClock, PersistenceProtection, PersistenceSnapshotSealKey};
use anima_execution_store_sqlite::SqliteExecutionStore;
use std::sync::Arc;

fn protection(key: u8) -> PersistenceProtection {
    PersistenceProtection::payload_free(PersistenceSnapshotSealKey::new([key; 32]).unwrap())
        .unwrap()
}

#[tokio::test]
async fn create_and_reopen_without_an_agent_workspace() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("execution.sqlite");
    let clock = Arc::new(ManualExecutionClock::default());
    let store = SqliteExecutionStore::open(&path, protection(1), clock.clone())
        .await
        .unwrap();
    drop(store);
    SqliteExecutionStore::open(&path, protection(1), clock)
        .await
        .unwrap();
    assert!(path.is_file());
    assert!(!temp.path().join("workspace").exists());
}

#[tokio::test]
async fn wrong_key_is_rejected_without_replacing_existing_state() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("execution.sqlite");
    let clock = Arc::new(ManualExecutionClock::default());
    drop(
        SqliteExecutionStore::open(&path, protection(1), clock.clone())
            .await
            .unwrap(),
    );
    assert!(
        SqliteExecutionStore::open(&path, protection(2), clock.clone())
            .await
            .is_err()
    );
    SqliteExecutionStore::open(&path, protection(1), clock)
        .await
        .unwrap();
}

#[tokio::test]
async fn future_schema_is_rejected_without_migration() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("execution.sqlite");
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.pragma_update(None, "user_version", 99).unwrap();
    assert!(SqliteExecutionStore::open(
        &path,
        protection(1),
        Arc::new(ManualExecutionClock::default())
    )
    .await
    .is_err());
    assert_eq!(
        conn.query_row::<i64, _, _>("PRAGMA user_version", [], |row| row.get(0))
            .unwrap(),
        99
    );
}

#[tokio::test]
async fn corrupted_snapshot_is_rejected_without_repairing_it() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("execution.sqlite");
    let store = SqliteExecutionStore::open(
        &path,
        protection(1),
        Arc::new(ManualExecutionClock::default()),
    )
    .await
    .unwrap();
    store.shutdown().await.unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute(
        "UPDATE execution_authority SET snapshot=?1",
        [b"corrupt".as_slice()],
    )
    .unwrap();
    assert!(SqliteExecutionStore::open(
        &path,
        protection(1),
        Arc::new(ManualExecutionClock::default())
    )
    .await
    .is_err());
    assert_eq!(
        conn.query_row::<Vec<u8>, _, _>("SELECT snapshot FROM execution_authority", [], |row| row
            .get(0))
            .unwrap(),
        b"corrupt"
    );
}

#[tokio::test]
async fn failed_initial_migration_rolls_back_schema_version() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("execution.sqlite");
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("CREATE TABLE execution_authority (legacy TEXT);")
        .unwrap();
    assert!(SqliteExecutionStore::open(
        &path,
        protection(1),
        Arc::new(ManualExecutionClock::default())
    )
    .await
    .is_err());
    assert_eq!(
        conn.query_row::<i64, _, _>("PRAGMA user_version", [], |row| row.get(0))
            .unwrap(),
        0
    );
    conn.execute("INSERT INTO execution_authority VALUES ('preserved')", [])
        .unwrap();
}

#[tokio::test]
async fn missing_authority_is_not_silently_replaced_by_an_empty_store() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("execution.sqlite");
    let store = SqliteExecutionStore::open(
        &path,
        protection(1),
        Arc::new(ManualExecutionClock::default()),
    )
    .await
    .unwrap();
    store.shutdown().await.unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute("DELETE FROM execution_authority", []).unwrap();
    assert!(SqliteExecutionStore::open(
        &path,
        protection(1),
        Arc::new(ManualExecutionClock::default())
    )
    .await
    .is_err());
    assert_eq!(
        conn.query_row::<i64, _, _>("SELECT COUNT(*) FROM execution_authority", [], |row| row
            .get(0))
            .unwrap(),
        0
    );
}
