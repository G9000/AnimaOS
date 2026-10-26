use anima_core::{
    ExecutionClock, ExecutionStoreError, ExecutionStoreErrorCode, ExecutionStoreSnapshot,
    InMemoryExecutionStore, ManualExecutionClock, PersistenceProtection,
};
use futures::executor::block_on;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use std::{
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::oneshot;
use uuid::Uuid;

pub(crate) enum Job {
    Operation(Box<dyn FnOnce(&mut Worker) + Send>),
    Shutdown(oneshot::Sender<()>),
}

pub(crate) fn unavailable() -> ExecutionStoreError {
    ExecutionStoreError::new(ExecutionStoreErrorCode::StorageUnavailable)
}

fn corrupt() -> ExecutionStoreError {
    ExecutionStoreError::new(ExecutionStoreErrorCode::CorruptState)
}

fn configure_wal(connection: &Connection) -> rusqlite::Result<()> {
    let timeout = Duration::from_secs(5);
    let deadline = Instant::now() + timeout;
    loop {
        // WAL lock upgrades can return BUSY without invoking SQLite's busy
        // handler. Retry only that pragma and only within the original budget.
        connection.busy_timeout(deadline.saturating_duration_since(Instant::now()))?;
        match connection.pragma_update(None, "journal_mode", "WAL") {
            Err(error)
                if error.sqlite_error().is_some_and(|error| {
                    matches!(
                        error.code,
                        rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
                    )
                }) && Instant::now() < deadline =>
            {
                std::thread::sleep(
                    Duration::from_millis(10)
                        .min(deadline.saturating_duration_since(Instant::now())),
                );
            }
            result => {
                connection.busy_timeout(timeout)?;
                return result;
            }
        }
    }
}

pub(crate) struct Worker {
    connection: Connection,
    pub(crate) protection: PersistenceProtection,
    pub(crate) store_id: Uuid,
    clock: Arc<dyn ExecutionClock>,
}

impl Worker {
    pub(crate) fn database<T>(
        &mut self,
        operation: impl FnOnce(&rusqlite::Transaction<'_>, u64) -> Result<T, String>,
    ) -> Result<T, String> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| "execution storage unavailable")?;
        let floor: i64 = tx
            .query_row(
                "SELECT clock_floor_ms FROM execution_authority WHERE singleton=1",
                [],
                |row| row.get(0),
            )
            .map_err(|_| "execution clock unavailable")?;
        let now = self
            .clock
            .now_ms()
            .max(u64::try_from(floor).map_err(|_| "execution clock corrupt")?);
        let result = operation(&tx, now);
        if result.is_err() {
            return result;
        }
        tx.execute(
            "UPDATE execution_authority SET clock_floor_ms=?1 WHERE singleton=1",
            [i64::try_from(now).map_err(|_| "execution clock overflow")?],
        )
        .map_err(|_| "execution storage unavailable")?;
        tx.commit().map_err(|_| "execution storage unavailable")?;
        result
    }

    pub fn open(
        path: &Path,
        protection: PersistenceProtection,
        clock: Arc<dyn ExecutionClock>,
    ) -> Result<Self, ExecutionStoreError> {
        let mut connection = Connection::open(path).map_err(|_| unavailable())?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(|_| unavailable())?;
        configure_wal(&connection).map_err(|_| unavailable())?;
        connection
            .pragma_update(None, "synchronous", "FULL")
            .map_err(|_| unavailable())?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| unavailable())?;
        let created = crate::schema::migrate(&tx)?;
        let stored_id: Option<String> = tx
            .query_row(
                "SELECT store_id FROM execution_store_metadata WHERE singleton=1",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(|_| corrupt())?;
        let bytes: Option<Vec<u8>> = tx
            .query_row(
                "SELECT snapshot FROM execution_authority WHERE singleton = 1",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(|_| unavailable())?;
        let store_id = match (bytes, stored_id) {
            (Some(bytes), Some(id)) => {
                ExecutionStoreSnapshot::from_bytes(bytes, &protection)?;
                let id = Uuid::parse_str(&id).map_err(|_| corrupt())?;
                if id.is_nil() {
                    return Err(corrupt());
                }
                id
            }
            (None, None) => {
                if !created {
                    return Err(corrupt());
                }
                let now = i64::try_from(clock.now_ms()).map_err(|_| corrupt())?;
                let snapshot = block_on(
                    InMemoryExecutionStore::with_clock(clock.clone()).export_snapshot(&protection),
                )?;
                let id = Uuid::new_v4();
                tx.execute(
                    "INSERT INTO execution_store_metadata(singleton,store_id) VALUES(1,?1)",
                    [id.to_string()],
                )
                .map_err(|_| unavailable())?;
                tx.execute(
                    "INSERT INTO execution_authority VALUES (1, 0, ?1, ?2)",
                    rusqlite::params![now, snapshot.as_bytes()],
                )
                .map_err(|_| unavailable())?;
                id
            }
            _ => return Err(corrupt()),
        };
        tx.commit().map_err(|_| unavailable())?;
        Ok(Self {
            connection,
            protection,
            store_id,
            clock,
        })
    }

    pub fn transaction<T>(
        &mut self,
        write: bool,
        operation: impl FnOnce(&InMemoryExecutionStore) -> Result<T, ExecutionStoreError>,
    ) -> Result<T, ExecutionStoreError> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| unavailable())?;
        let (generation, floor, bytes): (i64, i64, Vec<u8>) = tx.query_row("SELECT generation, clock_floor_ms, snapshot FROM execution_authority WHERE singleton = 1", [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).map_err(|_| corrupt())?;
        if generation < 0 || floor < 0 {
            return Err(corrupt());
        }
        let now = self.clock.now_ms().max(floor as u64);
        let floor = i64::try_from(now).map_err(|_| corrupt())?;
        let snapshot = ExecutionStoreSnapshot::from_bytes(bytes, &self.protection)?;
        let reference = InMemoryExecutionStore::from_snapshot(
            &snapshot,
            &self.protection,
            Arc::new(ManualExecutionClock::new(now)),
        )?;
        let mut result = operation(&reference);
        let candidate = if write && result.is_ok() {
            match block_on(reference.export_snapshot(&self.protection)).and_then(|candidate| {
                generation
                    .checked_add(1)
                    .ok_or_else(corrupt)
                    .map(|generation| (generation, candidate))
            }) {
                Ok(candidate) => Some(candidate),
                Err(error) => {
                    // A rejected export has no durable candidate, but the
                    // authoritative time it observed must not be rolled back.
                    result = Err(error);
                    None
                }
            }
        } else {
            None
        };
        if let Some((generation, candidate)) = candidate {
            tx.execute("UPDATE execution_authority SET generation=?1, clock_floor_ms=?2, snapshot=?3 WHERE singleton=1", rusqlite::params![generation, floor, candidate.as_bytes()]).map_err(|_| unavailable())?;
        } else {
            // Observing expiry is durable even when the requested mutation is rejected.
            tx.execute(
                "UPDATE execution_authority SET clock_floor_ms=?1 WHERE singleton=1",
                [floor],
            )
            .map_err(|_| unavailable())?;
        }
        tx.commit().map_err(|_| unavailable())?;
        result
    }
}
