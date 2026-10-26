use crate::SqliteExecutionStore;
use anima_core::*;
use async_trait::async_trait;
use rusqlite::{params, OptionalExtension};
use uuid::Uuid;

fn failure() -> ExecutionStoreError {
    ExecutionStoreError::new(ExecutionStoreErrorCode::StorageUnavailable)
}
fn conflict() -> ExecutionStoreError {
    ExecutionStoreError::new(ExecutionStoreErrorCode::CommandConflict)
}

#[async_trait]
impl HostRunAdmissionStore for SqliteExecutionStore {
    async fn admit_host_run(
        &self,
        owner: Uuid,
        run: Uuid,
        payload: Vec<u8>,
    ) -> Result<(), ExecutionStoreError> {
        if owner.is_nil()
            || run.is_nil()
            || payload.is_empty()
            || payload.len() > MAX_HOST_RUN_ADMISSION_BYTES
        {
            return Err(ExecutionStoreError::new(
                ExecutionStoreErrorCode::InvalidRequest,
            ));
        }
        let identical=self.database(move |worker|{
            let protection=worker.protection.clone();
            let protected=protection.protect_host_admission(owner,run,&payload).map_err(|_|"host admission protection rejected payload")?;
            // Verify size/restore before committing; rejected policy never leaves
            // private input in an unprotected ancillary table.
            protection.restore_host_admission(owner,run,&protected).map_err(|_|"host admission protection rejected payload")?;
            worker.database(|tx,_|{
            let current:Option<Vec<u8>>=tx.query_row("SELECT payload FROM host_run_admissions WHERE owner_id=?1 AND run_id=?2",params![owner.to_string(),run.to_string()],|row|row.get(0)).optional().map_err(|_|"admission unavailable")?;
            if let Some(current)=current{return Ok(protection.restore_host_admission(owner,run,&current).map_err(|_|"host admission integrity failure")?==payload);}
            tx.execute("INSERT INTO host_run_admissions VALUES (?1,?2,?3)",params![owner.to_string(),run.to_string(),protected]).map_err(|_|"admission commit failed")?;Ok(true)
        })}).await.map_err(|_|failure())?;
        if identical {
            Ok(())
        } else {
            Err(conflict())
        }
    }
    async fn load_host_run(
        &self,
        owner: Uuid,
        run: Uuid,
    ) -> Result<Option<Vec<u8>>, ExecutionStoreError> {
        self.database(move |worker| {
            let protection = worker.protection.clone();
            worker.database(|tx, _| {
                let payload: Option<Vec<u8>> = tx
                    .query_row(
                        "SELECT payload FROM host_run_admissions WHERE owner_id=?1 AND run_id=?2",
                        params![owner.to_string(), run.to_string()],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(|_| "admission unavailable")?;
                payload
                    .map(|payload| {
                        protection
                            .restore_host_admission(owner, run, &payload)
                            .map_err(|_| "host admission integrity failure".into())
                    })
                    .transpose()
            })
        })
        .await
        .map_err(|_| failure())
    }
    async fn reserve_application_scope(
        &self,
        owner: Uuid,
        scope: Uuid,
    ) -> Result<String, ExecutionStoreError> {
        if owner.is_nil() || scope.is_nil() {
            return Err(ExecutionStoreError::new(
                ExecutionStoreErrorCode::InvalidRequest,
            ));
        }
        let policy_id = format!("application:v1:{owner}:{scope}");
        let wanted = policy_id.clone();
        let identical=self.database(move |worker|worker.database(|tx,_|{
            let current:Option<String>=tx.query_row("SELECT scope_id FROM application_scopes WHERE owner_id=?1 AND policy_id=?2",params![owner.to_string(),policy_id],|row|row.get(0)).optional().map_err(|_|"scope registry unavailable")?;
            if let Some(current)=current{return Ok(current==scope.to_string());}
            tx.execute("INSERT INTO application_scopes VALUES (?1,?2,?3)",params![owner.to_string(),scope.to_string(),policy_id]).map_err(|_|"scope registry commit failed")?;Ok(true)
        })).await.map_err(|_|failure())?;
        if identical {
            Ok(wanted)
        } else {
            Err(conflict())
        }
    }
}
impl SqliteExecutionStore {
    /// Hosts seed all existing workspace IDs before admitting non-filesystem
    /// scopes. Their old policy wire ID is preserved verbatim.
    pub async fn reserve_legacy_scope(
        &self,
        owner: Uuid,
        policy_id: &str,
    ) -> Result<(), ExecutionStoreError> {
        if owner.is_nil() || policy_id.is_empty() || policy_id.len() > 1024 {
            return Err(ExecutionStoreError::new(
                ExecutionStoreErrorCode::InvalidRequest,
            ));
        }
        let policy_id = policy_id.to_owned();
        let identical=self.database(move |worker|worker.database(|tx,_|{
            let current:Option<String>=tx.query_row("SELECT scope_id FROM application_scopes WHERE owner_id=?1 AND policy_id=?2",params![owner.to_string(),policy_id],|row|row.get(0)).optional().map_err(|_|"scope registry unavailable")?;
            let legacy=format!("legacy:{policy_id}");
            if let Some(current)=current{return Ok(current==legacy);}
            tx.execute("INSERT INTO application_scopes VALUES (?1,?2,?3)",params![owner.to_string(),legacy,policy_id]).map_err(|_|"scope registry commit failed")?;Ok(true)
        })).await.map_err(|_|failure())?;
        if identical {
            Ok(())
        } else {
            Err(conflict())
        }
    }
}
