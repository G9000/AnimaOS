use anima_core::{ManualExecutionClock, PersistenceProtection, PersistenceSnapshotSealKey};
use anima_execution_store_sqlite::SqliteExecutionStore;
use std::sync::Arc;

#[tokio::test]
async fn repeated_parallel_initial_opens_share_one_atomic_identity_and_snapshot() {
    let temp = tempfile::tempdir().unwrap();
    let protection =
        PersistenceProtection::payload_free(PersistenceSnapshotSealKey::new([101; 32]).unwrap())
            .unwrap();
    for round in 0..64 {
        let path = temp.path().join(format!("parallel-{round}.sqlite"));
        let results = futures::future::join_all((0..8).map(|_| {
            SqliteExecutionStore::open(
                &path,
                protection.clone(),
                Arc::new(ManualExecutionClock::default()),
            )
        }))
        .await;
        let mut stores = Vec::new();
        let mut failures = Vec::new();
        for (index, result) in results.into_iter().enumerate() {
            match result {
                Ok(store) => stores.push(store),
                Err(error) => failures.push((index, error.code())),
            }
        }
        let identity = stores.first().map(|store| store.store_id());
        for store in &stores {
            assert_eq!(Some(store.store_id()), identity);
        }
        for store in stores {
            store.shutdown().await.unwrap();
        }
        assert!(
            failures.is_empty(),
            "round {round} initial open failures: {failures:?}"
        );
        let connection = rusqlite::Connection::open(path).unwrap();
        let (generation,snapshots):(i64,i64)=connection.query_row("SELECT generation,(SELECT COUNT(*) FROM execution_authority) FROM execution_authority WHERE singleton=1",[],|row|Ok((row.get(0)?,row.get(1)?))).unwrap();
        assert_eq!((generation, snapshots), (0, 1));
    }
}
