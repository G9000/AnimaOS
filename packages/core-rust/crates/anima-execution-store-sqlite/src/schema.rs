use crate::worker::unavailable;
use anima_core::{ExecutionStoreError, ExecutionStoreErrorCode};
use rusqlite::Transaction;

pub fn migrate(tx: &Transaction<'_>) -> Result<bool, ExecutionStoreError> {
    let version: i64 = tx
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|_| unavailable())?;
    match version {
        0 => {
            tx.execute_batch("CREATE TABLE execution_store_metadata (
                singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
                store_id TEXT NOT NULL
            );
            CREATE TABLE execution_authority (
                singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
                generation INTEGER NOT NULL CHECK(generation >= 0),
                clock_floor_ms INTEGER NOT NULL CHECK(clock_floor_ms >= 0),
                snapshot BLOB NOT NULL
            );
            CREATE TABLE execution_allowances (
                owner_id TEXT NOT NULL,
                scope_id TEXT NOT NULL,
                payload TEXT NOT NULL,
                PRIMARY KEY(owner_id, scope_id)
            );
            CREATE TABLE host_run_admissions (owner_id TEXT NOT NULL, run_id TEXT NOT NULL, payload BLOB NOT NULL, PRIMARY KEY(owner_id,run_id));
            CREATE TABLE application_scopes (owner_id TEXT NOT NULL, scope_id TEXT NOT NULL, policy_id TEXT NOT NULL, PRIMARY KEY(owner_id,scope_id), UNIQUE(owner_id,policy_id));
            PRAGMA user_version = 1;").map_err(|_| unavailable())?;
        }
        1 => {}
        _ => {
            return Err(ExecutionStoreError::new(
                ExecutionStoreErrorCode::CorruptState,
            ))
        }
    }
    Ok(version == 0)
}
