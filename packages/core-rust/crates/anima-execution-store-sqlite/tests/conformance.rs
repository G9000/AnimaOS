use anima_core::*;
use anima_execution_store_sqlite::SqliteExecutionStore;
use std::sync::Arc;

struct Factory {
    clock: ManualExecutionClock,
    directory: tempfile::TempDir,
}

#[async_trait::async_trait]
impl ExecutionStoreFactory for Factory {
    type Store = SqliteExecutionStore;
    async fn create_execution_store(&self) -> Result<Self::Store, ExecutionStoreError> {
        SqliteExecutionStore::open(
            self.directory
                .path()
                .join(format!("{}.sqlite", uuid::Uuid::new_v4())),
            common::protection(),
            Arc::new(self.clock.clone()),
        )
        .await
    }
    fn advance_clock(&self, duration_ms: u64) -> Result<(), ExecutionStoreError> {
        self.clock.advance_ms(duration_ms)
    }
}

mod common;

#[tokio::test]
async fn complete_public_store_conformance_against_real_sqlite() {
    let factory = Factory {
        clock: ManualExecutionClock::default(),
        directory: tempfile::tempdir().unwrap(),
    };
    assert_execution_store_conformance(&factory).await.unwrap();
}
