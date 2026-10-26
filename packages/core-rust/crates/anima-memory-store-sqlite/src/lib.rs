//! Durable owner-bound canonical memory on one bounded dedicated SQLite worker.
mod migration;
mod projection;
mod relationships;
mod schema;
mod store;

use anima_core::{
    ExecutionClock, IdentityBinding, KnowledgeAccessContext, MemoryHit, MemoryPort,
    MemoryPortError, MemoryPortErrorCode, MemoryQuery, MemoryQueryResult, MemoryRecord,
    MemoryRevision, MemoryWrite, WorldResidentBinding,
};
use anima_memory::{IdentityRegistry, MemoryAuthority, BM25};
use async_trait::async_trait;
pub use migration::{MemoryImportManifest, MemoryProfileRollbackExport};
pub use relationships::RelationshipPerspective;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use std::{path::Path, sync::Arc};
use tokio::sync::{mpsc, oneshot};

type Operation = Box<dyn FnOnce(&mut Worker) + Send>;
enum Job {
    Operation(Operation),
    Shutdown(oneshot::Sender<()>),
}
struct Worker {
    connection: Connection,
    clock: Arc<dyn ExecutionClock>,
}

/// Clones share a bounded queue; admitted operations retain custody if callers are cancelled.
#[derive(Clone)]
pub struct SqliteMemoryStore {
    sender: mpsc::Sender<Job>,
}

/// Only this trusted authority-bound handle implements the memory mutation port.
#[derive(Clone)]
pub struct BoundMemoryStore {
    store: SqliteMemoryStore,
    authority: MemoryAuthority,
}

impl SqliteMemoryStore {
    /// Stable identity of this canonical memory store. Hosts pin it alongside
    /// their source receipts so replacing the database cannot silently detach
    /// already-acknowledged memory effects. Reads validate it on the worker.
    pub async fn store_id(&self) -> Result<String, MemoryPortError> {
        self.call(false, |connection, _| schema::store_id(connection))
            .await
    }
    pub async fn open(
        path: impl AsRef<Path>,
        clock: Arc<dyn ExecutionClock>,
    ) -> Result<Self, MemoryPortError> {
        let path = path.as_ref().to_owned();
        let (sender, mut receiver) = mpsc::channel::<Job>(32);
        let (ready, opened) = oneshot::channel();
        std::thread::Builder::new()
            .name("anima-memory-sqlite".into())
            .spawn(move || {
                let result = Connection::open(&path)
                    .map_err(backend)
                    .and_then(|mut connection| {
                        schema::initialize(&mut connection)?;
                        Ok(Worker { connection, clock })
                    });
                let mut worker = match result {
                    Ok(worker) => worker,
                    Err(error) => {
                        let _ = ready.send(Err(error));
                        return;
                    }
                };
                if ready.send(Ok(())).is_err() {
                    return;
                }
                while let Some(job) = receiver.blocking_recv() {
                    match job {
                        Job::Operation(operation) => operation(&mut worker),
                        Job::Shutdown(done) => {
                            receiver.close();
                            let mut shutdowns = vec![done];
                            while let Some(job) = receiver.blocking_recv() {
                                match job {
                                    Job::Operation(operation) => operation(&mut worker),
                                    Job::Shutdown(other) => shutdowns.push(other),
                                }
                            }
                            drop(worker);
                            for done in shutdowns {
                                let _ = done.send(());
                            }
                            return;
                        }
                    }
                }
            })
            .map_err(backend)?;
        opened.await.map_err(backend)??;
        Ok(Self { sender })
    }
    /// Stops new admissions on every clone, drains admitted operations, and closes the DB.
    pub async fn shutdown(&self) -> Result<(), MemoryPortError> {
        let (done, closed) = oneshot::channel();
        self.sender
            .send(Job::Shutdown(done))
            .await
            .map_err(backend)?;
        closed.await.map_err(backend)
    }
    async fn call<T: Send + 'static>(
        &self,
        _write: bool,
        operation: impl FnOnce(&Connection, u64) -> Result<T, MemoryPortError> + Send + 'static,
    ) -> Result<T, MemoryPortError> {
        let (reply, response) = oneshot::channel();
        self.sender
            .send(Job::Operation(Box::new(move |worker| {
                let result = (|| {
                    let tx = worker
                        .connection
                        .transaction_with_behavior(TransactionBehavior::Immediate)
                        .map_err(backend)?;
                    let floor: String = tx
                        .query_row(
                            "SELECT floor_ms FROM memory_clock WHERE singleton=1",
                            [],
                            |row| row.get(0),
                        )
                        .map_err(backend)?;
                    let now = worker
                        .clock
                        .now_ms()
                        .max(floor.parse::<u64>().map_err(backend)?);
                    // Rejected operations retain their observed time without
                    // publishing any candidate record, relationship or receipt.
                    tx.execute_batch("SAVEPOINT memory_operation")
                        .map_err(backend)?;
                    let value = operation(&tx, now);
                    if value.is_err() {
                        tx.execute_batch("ROLLBACK TO memory_operation")
                            .map_err(backend)?;
                    }
                    tx.execute_batch("RELEASE memory_operation")
                        .map_err(backend)?;
                    tx.execute(
                        "UPDATE memory_clock SET floor_ms=?1 WHERE singleton=1",
                        [store::millis(now)],
                    )
                    .map_err(backend)?;
                    tx.commit().map_err(backend)?;
                    value
                })();
                let _ = reply.send(result);
            })))
            .await
            .map_err(backend)?;
        response.await.map_err(backend)?
    }
    pub async fn bind(
        &self,
        authority: MemoryAuthority,
    ) -> Result<BoundMemoryStore, MemoryPortError> {
        let identity = authority.identity().clone();
        self.call(false,move |connection,_| {
            let registered: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM memory_identities WHERE owner=?1 AND individual=?2)",params![identity.owner_id().as_str(),identity.individual_id().as_str()],|r|r.get(0)).map_err(backend)?;
            if registered { Ok(()) } else { Err(anima_memory::access::unauthorized()) }
        }).await?;
        Ok(BoundMemoryStore {
            store: self.clone(),
            authority,
        })
    }
}

#[async_trait]
impl IdentityRegistry for SqliteMemoryStore {
    async fn register_identity(&self, identity: IdentityBinding) -> Result<(), MemoryPortError> {
        self.call(true,move |connection,_| {
            connection.execute("INSERT INTO memory_identities(owner,individual) VALUES(?1,?2) ON CONFLICT(owner,individual) DO NOTHING",params![identity.owner_id().as_str(),identity.individual_id().as_str()]).map_err(backend)?;
            Ok(())
        }).await
    }
    async fn register_resident(
        &self,
        resident: WorldResidentBinding,
    ) -> Result<(), MemoryPortError> {
        self.call(true,move |connection,_| {
            let identity = resident.identity();
            let existing: Option<String> = connection.query_row("SELECT individual FROM memory_residents WHERE owner=?1 AND world=?2 AND resident=?3",params![identity.owner_id().as_str(),resident.world_id(),resident.resident_id()],|r|r.get(0)).optional().map_err(backend)?;
            if let Some(individual) = existing {
                return if individual == identity.individual_id().as_str() { Ok(()) } else { Err(conflict("resident is already bound to another individual")) };
            }
            let mapped: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM memory_residents WHERE owner=?1 AND individual=?2)",params![identity.owner_id().as_str(),identity.individual_id().as_str()],|r|r.get(0)).map_err(backend)?;
            if mapped { return Err(conflict("individual already has a resident mapping; a different world requires a separate identity")); }
            connection.execute("INSERT INTO memory_residents(owner,world,resident,individual) VALUES(?1,?2,?3,?4)",params![identity.owner_id().as_str(),resident.world_id(),resident.resident_id(),identity.individual_id().as_str()]).map_err(backend)?;
            Ok(())
        }).await
    }
    async fn resident_identity(
        &self,
        owner_id: &str,
        world_id: &str,
        resident_id: &str,
    ) -> Result<Option<IdentityBinding>, MemoryPortError> {
        let (owner, world, resident) = (
            owner_id.to_owned(),
            world_id.to_owned(),
            resident_id.to_owned(),
        );
        self.call(false,move |connection,_| {
            let individual: Option<String> = connection.query_row("SELECT individual FROM memory_residents WHERE owner=?1 AND world=?2 AND resident=?3",params![owner,world,resident],|r|r.get(0)).optional().map_err(backend)?;
            individual.map(|id|IdentityBinding::new(owner,id)).transpose()
        }).await
    }
}

impl BoundMemoryStore {
    pub fn authority(&self) -> &MemoryAuthority {
        &self.authority
    }
    /// Trusted lifecycle inspection with the same exact owner/read-scope boundary.
    /// Forgotten or expired records can be returned; use query/recent for active recall.
    pub async fn get(&self, id: &str) -> Result<MemoryRecord, MemoryPortError> {
        let authority = self.authority.clone();
        let id = id.to_owned();
        self.store
            .call(false, move |connection, _| {
                store::record(connection, &authority, &id)
            })
            .await
    }
    /// Exact active-record lookup for trusted mutation/evidence admission.
    /// Uses the store's durable clock floor and excludes superseded lifecycle state.
    pub async fn get_active(&self, id: &str) -> Result<MemoryRecord, MemoryPortError> {
        let authority = self.authority.clone();
        let id = id.to_owned();
        self.store
            .call(false, move |connection, now| {
                let record = store::record(connection, &authority, &id)?;
                if !store::active(connection, &authority, &id, now)? {
                    return Err(conflict("memory is no longer active"));
                }
                Ok(record)
            })
            .await
    }
    pub async fn recent(&self, limit: usize) -> Result<Vec<MemoryRecord>, MemoryPortError> {
        if limit == 0 || limit > 1000 {
            return Err(MemoryPortError::invalid(
                "recent recall limit must be between 1 and 1000",
            ));
        }
        let authority = self.authority.clone();
        self.store
            .call(false, move |connection, now| {
                store::recent(connection, &authority, authority.read_scopes(), now, limit)
            })
            .await
    }
}
#[async_trait]
impl MemoryPort for BoundMemoryStore {
    async fn get_active(
        &self,
        access: &KnowledgeAccessContext,
        id: &str,
    ) -> Result<MemoryRecord, MemoryPortError> {
        let authority = self.authority.clone();
        let access = access.clone();
        let id = id.to_owned();
        self.store
            .call(false, move |connection, now| {
                let record = store::record(connection, &authority, &id)?;
                if !access.allows(record.scope()) {
                    return Err(anima_memory::access::unauthorized());
                }
                if !store::active(connection, &authority, &id, now)? {
                    return Err(conflict("memory is no longer active"));
                }
                Ok(record)
            })
            .await
    }
    async fn write(&self, write: MemoryWrite) -> Result<MemoryRecord, MemoryPortError> {
        let authority = self.authority.clone();
        self.store
            .call(true, move |connection, _| {
                store::write(connection, &authority, write)
            })
            .await
    }
    async fn revise(&self, revision: MemoryRevision) -> Result<MemoryRecord, MemoryPortError> {
        let authority = self.authority.clone();
        self.store
            .call(true, move |connection, now| {
                store::revise(connection, &authority, revision, now)
            })
            .await
    }
    async fn query(
        &self,
        access: &KnowledgeAccessContext,
        query: MemoryQuery,
    ) -> Result<MemoryQueryResult, MemoryPortError> {
        let authority = self.authority.clone();
        let access = access.clone();
        self.store
            .call(false, move |connection, now| {
                for scope in query.requested_scopes() {
                    store::ensure_read(&authority, scope)?;
                    if !access.allows(scope) {
                        return Err(anima_memory::access::unauthorized());
                    }
                }
                let records = store::recent(
                    connection,
                    &authority,
                    query.requested_scopes(),
                    now,
                    query.limit().max(512),
                )?;
                let mut index = BM25::default();
                for record in &records {
                    index.add_document(record.id(), record.content());
                }
                let mut ranked = index.search(query.text(), records.len());
                ranked.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.id.cmp(&b.id)));
                ranked.truncate(query.limit());
                let mut hits = Vec::new();
                for result in ranked {
                    if let Some(record) = records.iter().find(|record| record.id() == result.id) {
                        hits.push(MemoryHit::new(
                            record.clone(),
                            result.score,
                            "lexical BM25 over authorized active canonical records",
                        )?);
                    }
                }
                MemoryQueryResult::new(&access, &query, hits)
            })
            .await
    }
}

pub(crate) fn backend<T>(_error: T) -> MemoryPortError {
    MemoryPortError {
        code: MemoryPortErrorCode::Backend,
        message: "required memory store is unavailable or corrupt".into(),
    }
}
pub(crate) fn conflict(message: &str) -> MemoryPortError {
    MemoryPortError {
        code: MemoryPortErrorCode::Conflict,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anima_core::{ManualExecutionClock, MemoryKind, MemoryProvenance, MemoryScope};

    #[tokio::test]
    async fn every_concurrent_shutdown_receipt_waits_for_the_entire_drain() {
        let temp = tempfile::tempdir().unwrap();
        let store = SqliteMemoryStore::open(
            temp.path().join("shutdown.sqlite"),
            Arc::new(ManualExecutionClock::default()),
        )
        .await
        .unwrap();
        let (entered, waiting) = oneshot::channel();
        let (release, released) = std::sync::mpsc::channel();
        store
            .sender
            .send(Job::Operation(Box::new(move |_| {
                let _ = entered.send(());
                released.recv().unwrap();
            })))
            .await
            .unwrap();
        waiting.await.unwrap();
        let (first_done, first) = oneshot::channel();
        let (second_done, second) = oneshot::channel();
        store.sender.send(Job::Shutdown(first_done)).await.unwrap();
        store.sender.send(Job::Shutdown(second_done)).await.unwrap();
        let (tail_entered, tail_waiting) = oneshot::channel();
        let (tail_release, tail_released) = std::sync::mpsc::channel();
        store
            .sender
            .send(Job::Operation(Box::new(move |_| {
                let _ = tail_entered.send(());
                tail_released.recv().unwrap();
            })))
            .await
            .unwrap();
        release.send(()).unwrap();
        tail_waiting.await.unwrap();
        let mut second = Box::pin(second);
        let premature = futures::poll!(&mut second).is_ready();
        tail_release.send(()).unwrap();
        first.await.unwrap();
        if !premature {
            second.await.unwrap();
        }
        assert!(!premature,"every shutdown acknowledgement must wait until tail operations finish and the connection closes");
    }

    #[tokio::test]
    async fn admitted_write_survives_cancelled_reply_and_is_drained_before_shutdown() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("cancel.sqlite");
        let clock = Arc::new(ManualExecutionClock::default());
        let store = SqliteMemoryStore::open(&path, clock.clone()).await.unwrap();
        let id = IdentityBinding::new("owner", "agent").unwrap();
        let scope = MemoryScope::agent("agent").unwrap();
        store.register_identity(id.clone()).await.unwrap();
        let auth = MemoryAuthority::trusted(id, vec![scope.clone()], vec![scope.clone()]).unwrap();
        let bound = store.bind(auth.clone()).await.unwrap();
        let (entered, waiting) = oneshot::channel();
        let (release, released) = std::sync::mpsc::channel();
        store
            .sender
            .send(Job::Operation(Box::new(move |_| {
                let _ = entered.send(());
                released.recv().unwrap();
            })))
            .await
            .unwrap();
        waiting.await.unwrap();
        let mut write = Box::pin(
            bound.write(
                MemoryWrite::new(
                    "cancelled",
                    MemoryKind::Fact,
                    scope,
                    "admitted record",
                    1.0,
                    MemoryProvenance::new("test", "event", 1).unwrap(),
                )
                .unwrap(),
            ),
        );
        assert!(futures::poll!(&mut write).is_pending());
        drop(write);
        release.send(()).unwrap();
        store.shutdown().await.unwrap();
        let reopened = SqliteMemoryStore::open(path, clock).await.unwrap();
        assert_eq!(
            reopened
                .bind(auth)
                .await
                .unwrap()
                .recent(10)
                .await
                .unwrap()
                .len(),
            1
        );
        reopened.shutdown().await.unwrap();
    }
}
