use anima_core::*;
use anima_execution_store_sqlite::SqliteExecutionStore;
use std::sync::Arc;
use uuid::Uuid;
mod common;

#[tokio::test]
async fn application_scopes_are_restart_stable_and_cannot_alias_legacy_authority() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("admission.sqlite");
    let clock = Arc::new(ManualExecutionClock::default());
    let store = SqliteExecutionStore::open(&path, common::protection(), clock.clone())
        .await
        .unwrap();
    let owner = Uuid::new_v4();
    let app = Uuid::new_v4();
    let other = Uuid::new_v4();
    store
        .reserve_legacy_scope(owner, "existing-workspace-id")
        .await
        .unwrap();
    let mapped = store.reserve_application_scope(owner, app).await.unwrap();
    assert_ne!(
        mapped,
        store.reserve_application_scope(other, app).await.unwrap()
    );
    assert!(store.reserve_legacy_scope(owner, &mapped).await.is_err());
    let collision = Uuid::new_v4();
    let legacy = format!("application:v1:{owner}:{collision}");
    store.reserve_legacy_scope(owner, &legacy).await.unwrap();
    assert!(store
        .reserve_application_scope(owner, collision)
        .await
        .is_err());
    let run = Uuid::new_v4();
    store
        .admit_host_run(owner, run, b"accepted-v1".to_vec())
        .await
        .unwrap();
    assert!(store.load_host_run(other, run).await.unwrap().is_none());
    store.shutdown().await.unwrap();
    let reopened = SqliteExecutionStore::open(&path, common::protection(), clock)
        .await
        .unwrap();
    assert_eq!(
        reopened
            .reserve_application_scope(owner, app)
            .await
            .unwrap(),
        mapped
    );
    reopened
        .admit_host_run(owner, run, b"accepted-v1".to_vec())
        .await
        .unwrap();
    assert!(reopened
        .admit_host_run(owner, run, b"changed-persona-or-identity".to_vec())
        .await
        .is_err());
    reopened.shutdown().await.unwrap();
}

#[tokio::test]
async fn rejected_payload_free_admission_leaves_no_private_side_table_payload() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("admission.sqlite");
    let store = SqliteExecutionStore::open(
        &path,
        PersistenceProtection::payload_free(PersistenceSnapshotSealKey::new([7; 32]).unwrap())
            .unwrap(),
        Arc::new(ManualExecutionClock::default()),
    )
    .await
    .unwrap();
    assert!(store
        .admit_host_run(
            Uuid::new_v4(),
            Uuid::new_v4(),
            b"private input that must never be retained".to_vec()
        )
        .await
        .is_err());
    let connection = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        connection
            .query_row::<i64, _, _>("SELECT COUNT(*) FROM host_run_admissions", [], |row| row
                .get(0))
            .unwrap(),
        0
    );
    store.shutdown().await.unwrap();
}
