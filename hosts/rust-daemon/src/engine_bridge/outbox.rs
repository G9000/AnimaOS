//! Accepted daemon events and pending memory effects share this host transaction.
use super::memory_sources::{AcceptedMemorySource, MemorySourceOutcome, StoredMemorySource};
use super::{BridgeError, PreparedRun, RegisteredProfile};
use anima_core::{
    CapabilityReferenceId, CapabilityResult, DurableCapabilityResult, DurableCapabilityStatus,
    EngineCapabilityResult, IdentityBinding, LogicalInvocation, PersistenceProtection,
};
use anima_harness::durable::MemoryProjectionReadiness;
use async_trait::async_trait;
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex},
};
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

type Operation = Box<dyn FnOnce(&mut Connection) + Send>;
enum Job {
    Operation(Operation),
    Shutdown(oneshot::Sender<()>),
}
#[derive(Clone)]
pub(super) struct Outbox {
    sender: mpsc::Sender<Job>,
    protection: PersistenceProtection,
    owner: Uuid,
    admissions: Arc<Mutex<HashMap<String, bool>>>,
}

impl Outbox {
    pub async fn open(
        path: impl AsRef<Path>,
        owner: Uuid,
        execution_store_id: Uuid,
        memory_store_id: String,
        protection: PersistenceProtection,
    ) -> Result<Self, BridgeError> {
        let path = path.as_ref().to_owned();
        let (sender, mut receiver) = mpsc::channel::<Job>(32);
        let (ready, opened) = oneshot::channel();
        let validation = protection.clone();
        let expected_proof = format!(
            "daemon-engine-outbox-v1:execution={execution_store_id}:memory={memory_store_id}"
        )
        .into_bytes();
        std::thread::Builder::new().name("anima-daemon-memory-outbox".into()).spawn(move||{
            let result=(||{
                let mut connection=Connection::open(path).map_err(|_|BridgeError::Unavailable)?;
                connection.busy_timeout(std::time::Duration::from_secs(5)).map_err(|_|BridgeError::Unavailable)?;
                connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;").map_err(|_|BridgeError::Unavailable)?;
                let tx=connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(|_|BridgeError::Unavailable)?;
                let version:u32=tx.query_row("PRAGMA user_version",[],|r|r.get(0)).map_err(|_|BridgeError::Unavailable)?;
                if version>1{return Err(BridgeError::Unavailable);}
                if version==0{
                    let occupied:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%')",[],|r|r.get(0)).map_err(|_|BridgeError::Unavailable)?;if occupied{return Err(BridgeError::Unavailable);}
                    tx.execute_batch("CREATE TABLE bridge_metadata(singleton INTEGER PRIMARY KEY CHECK(singleton=1),owner TEXT NOT NULL,key_id TEXT NOT NULL,proof BLOB NOT NULL); CREATE TABLE bridge_profiles(agent TEXT PRIMARY KEY,key_id TEXT NOT NULL,payload BLOB NOT NULL); CREATE TABLE bridge_runs(run TEXT PRIMARY KEY,agent TEXT NOT NULL,individual TEXT NOT NULL,session TEXT NOT NULL,payload BLOB NOT NULL,response BLOB,action TEXT NOT NULL DEFAULT 'continue'); CREATE TABLE bridge_events(event TEXT PRIMARY KEY,run TEXT NOT NULL,observer TEXT NOT NULL,pending INTEGER NOT NULL CHECK(pending IN (0,1)),FOREIGN KEY(run) REFERENCES bridge_runs(run)); CREATE TABLE bridge_capability_receipts(invocation TEXT PRIMARY KEY,run TEXT NOT NULL,payload BLOB NOT NULL,FOREIGN KEY(run) REFERENCES bridge_runs(run)); PRAGMA user_version=1;").map_err(|_|BridgeError::Unavailable)?;
                    tx.execute_batch("CREATE TABLE bridge_memory_sources(observer TEXT NOT NULL,source TEXT NOT NULL,payload BLOB NOT NULL,pending INTEGER NOT NULL CHECK(pending IN(0,1)),outcome BLOB,PRIMARY KEY(observer,source),CHECK((pending=1 AND outcome IS NULL) OR (pending=0 AND outcome IS NOT NULL)));").map_err(|_|BridgeError::Unavailable)?;
                    let key=Uuid::new_v4();let proof=validation.protect_host_admission(owner,key,&expected_proof).map_err(|_|BridgeError::Unavailable)?;
                    tx.execute("INSERT INTO bridge_metadata VALUES(1,?1,?2,?3)",params![owner.to_string(),key.to_string(),proof]).map_err(|_|BridgeError::Unavailable)?;
                }
                let (stored_owner,key,proof):(String,String,Vec<u8>)=tx.query_row("SELECT owner,key_id,proof FROM bridge_metadata WHERE singleton=1",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(|_|BridgeError::Unavailable)?;
                let key=Uuid::parse_str(&key).map_err(|_|BridgeError::Unavailable)?;
                if stored_owner!=owner.to_string()||validation.restore_host_admission(owner,key,&proof).map_err(|_|BridgeError::Unavailable)?!=expected_proof{return Err(BridgeError::Unavailable);}
                tx.prepare("SELECT observer,source,payload,pending,outcome FROM bridge_memory_sources LIMIT 0").map_err(|_|BridgeError::Unavailable)?;
                tx.commit().map_err(|_|BridgeError::Unavailable)?;Ok(connection)
            })();
            let mut connection=match result{Ok(c)=>c,Err(e)=>{let _=ready.send(Err(e));return;}};if ready.send(Ok(())).is_err(){return;}
            while let Some(job)=receiver.blocking_recv(){match job{Job::Operation(op)=>op(&mut connection),Job::Shutdown(done)=>{receiver.close();let mut receipts=vec![done];while let Some(job)=receiver.blocking_recv(){match job{Job::Operation(op)=>op(&mut connection),Job::Shutdown(done)=>receipts.push(done)}}drop(connection);for done in receipts{let _=done.send(());}return;}}}
        }).map_err(|_|BridgeError::Unavailable)?;
        opened.await.map_err(|_| BridgeError::Unavailable)??;
        Ok(Self {
            sender,
            protection,
            owner,
            admissions: Arc::new(Mutex::new(HashMap::new())),
        })
    }
    async fn call<T: Send + 'static>(
        &self,
        write: bool,
        operation: impl FnOnce(&Transaction<'_>) -> Result<T, BridgeError> + Send + 'static,
    ) -> Result<T, BridgeError> {
        let (reply, result) = oneshot::channel();
        self.sender
            .send(Job::Operation(Box::new(move |connection| {
                let value = (|| {
                    let tx = connection
                        .transaction_with_behavior(if write {
                            TransactionBehavior::Immediate
                        } else {
                            TransactionBehavior::Deferred
                        })
                        .map_err(|_| BridgeError::Unavailable)?;
                    let value = operation(&tx)?;
                    tx.commit().map_err(|_| BridgeError::Unavailable)?;
                    Ok(value)
                })();
                let _ = reply.send(value);
            })))
            .await
            .map_err(|_| BridgeError::Unavailable)?;
        result.await.map_err(|_| BridgeError::Unavailable)?
    }
    pub async fn register(
        &self,
        mut profile: RegisteredProfile,
    ) -> Result<RegisteredProfile, BridgeError> {
        let protection = self.protection.clone();
        let owner = self.owner;
        self.call(true, move |tx| {
            let existing: Option<(String, Vec<u8>)> = tx
                .query_row(
                    "SELECT key_id,payload FROM bridge_profiles WHERE agent=?1",
                    [&profile.agent_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()
                .map_err(|_| BridgeError::Unavailable)?;
            if let Some((key, bytes)) = existing {
                let key = Uuid::parse_str(&key).map_err(|_| BridgeError::Unavailable)?;
                let bytes = protection
                    .restore_host_admission(owner, key, &bytes)
                    .map_err(|_| BridgeError::Unavailable)?;
                let stored: RegisteredProfile =
                    serde_json::from_slice(&bytes).map_err(|_| BridgeError::Unavailable)?;
                if stored.agent_id != profile.agent_id
                    || stored.identity != profile.identity
                    || profile.definition.version < stored.definition.version
                {
                    return Err(BridgeError::Conflict);
                }
                if profile.definition.version == stored.definition.version {
                    if stored.config != profile.config
                        || stored.definition != profile.definition
                        || stored.limits != profile.limits
                        || stored.clock_enabled != profile.clock_enabled
                    {
                        return Err(BridgeError::Conflict);
                    }
                    return Ok(stored);
                }
                // New admissions select the explicit new definition; old runs
                // retain their complete protected profile in their source row.
                profile.scope = stored.scope;
                let payload = serde_json::to_vec(&profile).map_err(|_| BridgeError::Invalid)?;
                let bytes = protection
                    .protect_host_admission(owner, key, &payload)
                    .map_err(|_| BridgeError::Unavailable)?;
                tx.execute(
                    "UPDATE bridge_profiles SET payload=?1 WHERE agent=?2",
                    params![bytes, profile.agent_id],
                )
                .map_err(|_| BridgeError::Unavailable)?;
                return Ok(profile);
            }
            let key = Uuid::new_v4();
            let count: usize = tx
                .query_row("SELECT COUNT(*) FROM bridge_profiles", [], |row| row.get(0))
                .map_err(|_| BridgeError::Unavailable)?;
            if count >= 256 {
                return Err(BridgeError::Invalid);
            }
            let payload = serde_json::to_vec(&profile).map_err(|_| BridgeError::Invalid)?;
            let bytes = protection
                .protect_host_admission(owner, key, &payload)
                .map_err(|_| BridgeError::Unavailable)?;
            tx.execute(
                "INSERT INTO bridge_profiles VALUES(?1,?2,?3)",
                params![profile.agent_id, key.to_string(), bytes],
            )
            .map_err(|_| BridgeError::Unavailable)?;
            Ok(profile)
        })
        .await
    }
    pub async fn profiles(&self) -> Result<Vec<RegisteredProfile>, BridgeError> {
        let protection = self.protection.clone();
        let owner = self.owner;
        self.call(false, move |tx| {
            let mut statement = tx
                .prepare(
                    "SELECT agent,key_id,payload FROM bridge_profiles ORDER BY agent LIMIT 257",
                )
                .map_err(|_| BridgeError::Unavailable)?;
            let rows = statement
                .query_map([], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, Vec<u8>>(2)?,
                    ))
                })
                .map_err(|_| BridgeError::Unavailable)?;
            let mut profiles = vec![];
            for row in rows {
                if profiles.len() == 256 {
                    return Err(BridgeError::Unavailable);
                }
                let (agent, key, bytes) = row.map_err(|_| BridgeError::Unavailable)?;
                let key = Uuid::parse_str(&key).map_err(|_| BridgeError::Unavailable)?;
                let bytes = protection
                    .restore_host_admission(owner, key, &bytes)
                    .map_err(|_| BridgeError::Unavailable)?;
                let profile: RegisteredProfile =
                    serde_json::from_slice(&bytes).map_err(|_| BridgeError::Unavailable)?;
                if profile.agent_id != agent
                    || profile.identity.owner_id().as_str() != owner.to_string()
                {
                    return Err(BridgeError::Unavailable);
                }
                profiles.push(profile);
            }
            Ok(profiles)
        })
        .await
    }
    pub async fn memory_source(
        &self,
        identity: &IdentityBinding,
        source: Uuid,
    ) -> Result<Option<StoredMemorySource>, BridgeError> {
        if identity.owner_id().as_str() != self.owner.to_string() {
            return Err(BridgeError::Forbidden);
        }
        let identity = identity.clone();
        let protection = self.protection.clone();
        let owner = self.owner;
        self.call(false, move |tx| {
            load_memory_source(tx, &protection, owner, &identity, source)
        })
        .await
    }
    pub async fn accept_memory_source(
        &self,
        accepted: AcceptedMemorySource,
    ) -> Result<StoredMemorySource, BridgeError> {
        if accepted.identity.owner_id().as_str() != self.owner.to_string() {
            return Err(BridgeError::Forbidden);
        }
        let protection = self.protection.clone();
        let owner = self.owner;
        let admissions = self.admissions.clone();
        self.call(true,move|tx|{
            if let Some(stored)=load_memory_source(tx,&protection,owner,&accepted.identity,accepted.request.source_event_id)?{if stored.accepted.request!=accepted.request||stored.accepted.agent_id!=accepted.agent_id{return Err(BridgeError::Conflict);}return Ok(stored);}
            if !admissions.lock().map_err(|_|BridgeError::Unavailable)?.get(&accepted.agent_id).copied().unwrap_or(true){return Err(BridgeError::Unavailable);}
            let payload=serde_json::to_vec(&accepted).map_err(|_|BridgeError::Invalid)?;let payload=protection.protect_host_admission(owner,accepted.request.source_event_id,&payload).map_err(|_|BridgeError::Unavailable)?;
            tx.execute("INSERT INTO bridge_memory_sources(observer,source,payload,pending) VALUES(?1,?2,?3,1)",params![accepted.identity.individual_id().as_str(),accepted.request.source_event_id.to_string(),payload]).map_err(|_|BridgeError::Unavailable)?;
            Ok(StoredMemorySource{accepted,outcome:None})
        }).await
    }
    pub async fn pending_memory_sources(
        &self,
        identity: &IdentityBinding,
    ) -> Result<Vec<AcceptedMemorySource>, BridgeError> {
        if identity.owner_id().as_str() != self.owner.to_string() {
            return Err(BridgeError::Forbidden);
        }
        let identity = identity.clone();
        let protection = self.protection.clone();
        let owner = self.owner;
        self.call(false,move|tx|{let mut statement=tx.prepare("SELECT source FROM bridge_memory_sources WHERE observer=?1 AND pending=1 ORDER BY rowid LIMIT 64").map_err(|_|BridgeError::Unavailable)?;let rows=statement.query_map([identity.individual_id().as_str()],|row|row.get::<_,String>(0)).map_err(|_|BridgeError::Unavailable)?;let mut pending=vec![];for source in rows{let source=Uuid::parse_str(&source.map_err(|_|BridgeError::Unavailable)?).map_err(|_|BridgeError::Unavailable)?;pending.push(load_memory_source(tx,&protection,owner,&identity,source)?.ok_or(BridgeError::Unavailable)?.accepted);}Ok(pending)}).await
    }
    pub async fn finish_memory_source(
        &self,
        accepted: &AcceptedMemorySource,
        outcome: MemorySourceOutcome,
    ) -> Result<(), BridgeError> {
        let accepted = accepted.clone();
        let protection = self.protection.clone();
        let owner = self.owner;
        self.call(true,move|tx|{let stored=load_memory_source(tx,&protection,owner,&accepted.identity,accepted.request.source_event_id)?.ok_or(BridgeError::NotFound)?;if stored.accepted.request!=accepted.request{return Err(BridgeError::Conflict);}if let Some(previous)=stored.outcome{return if previous==outcome{Ok(())}else{Err(BridgeError::Conflict)};}
            if let MemorySourceOutcome::Applied{receipt}=&outcome{if receipt.identity!=accepted.identity||receipt.source_event_id!=accepted.effect.source_event_id(){return Err(BridgeError::Conflict);}}
            let bytes=serde_json::to_vec(&outcome).map_err(|_|BridgeError::Invalid)?;let bytes=protection.protect_host_admission(owner,accepted.request.source_event_id,&bytes).map_err(|_|BridgeError::Unavailable)?;
            tx.execute("UPDATE bridge_memory_sources SET pending=0,outcome=?1 WHERE observer=?2 AND source=?3 AND pending=1",params![bytes,accepted.identity.individual_id().as_str(),accepted.request.source_event_id.to_string()]).map_err(|_|BridgeError::Unavailable)?;Ok(())
        }).await
    }
    pub async fn exact_response(
        &self,
        agent: &str,
        request: &super::DialogueRequest,
    ) -> Result<Option<serde_json::Value>, BridgeError> {
        let agent = agent.to_owned();
        let request = request.clone();
        let protection = self.protection.clone();
        let owner = self.owner;
        self.call(false, move |tx| {
            if let Some(stored) = load(tx, &protection, owner, request.request_id)? {
                if stored.profile.agent_id != agent || stored.request != request {
                    return Err(BridgeError::Conflict);
                }
                let bytes: Option<Vec<u8>> = tx
                    .query_row(
                        "SELECT response FROM bridge_runs WHERE run=?1",
                        [request.request_id.to_string()],
                        |r| r.get(0),
                    )
                    .map_err(|_| BridgeError::Unavailable)?;
                return bytes
                    .map(|bytes| {
                        let payload = protection
                            .restore_host_admission(owner, request.request_id, &bytes)
                            .map_err(|_| BridgeError::Unavailable)?;
                        serde_json::from_slice(&payload).map_err(|_| BridgeError::Unavailable)
                    })
                    .transpose();
            }
            Ok(None)
        })
        .await
    }
    pub fn set_admission(&self, agent: &str, active: bool) -> Result<(), BridgeError> {
        self.admissions
            .lock()
            .map_err(|_| BridgeError::Unavailable)?
            .insert(agent.into(), active);
        Ok(())
    }
    pub async fn prepare(&self, proposed: PreparedRun) -> Result<PreparedRun, BridgeError> {
        let protection = self.protection.clone();
        let owner = self.owner;
        let admissions = self.admissions.clone();
        self.call(true,move|tx|{
            if let Some(stored)=load(tx,&protection,owner,proposed.binding.run)?{
                if stored.request!=proposed.request||stored.profile.agent_id!=proposed.profile.agent_id{return Err(BridgeError::Conflict);}return Ok(stored);
            }
            if !admissions.lock().map_err(|_|BridgeError::Unavailable)?.get(&proposed.profile.agent_id).copied().unwrap_or(true){return Err(BridgeError::Unavailable);}
            let payload=serde_json::to_vec(&proposed).map_err(|_|BridgeError::Invalid)?;let payload=protection.protect_host_admission(owner,proposed.binding.run,&payload).map_err(|_|BridgeError::Unavailable)?;
            tx.execute("INSERT INTO bridge_runs(run,agent,individual,session,payload) VALUES(?1,?2,?3,?4,?5)",params![proposed.binding.run.to_string(),proposed.profile.agent_id,proposed.binding.individual.individual_id().as_str(),proposed.binding.session.to_string(),payload]).map_err(|_|BridgeError::Unavailable)?;
            tx.execute("INSERT INTO bridge_events VALUES(?1,?2,?3,1)",params![proposed.effect.source_event_id(),proposed.binding.run.to_string(),proposed.binding.individual.individual_id().as_str()]).map_err(|_|BridgeError::Unavailable)?;Ok(proposed)
        }).await
    }
    pub async fn run(&self, agent: &str, run: Uuid) -> Result<PreparedRun, BridgeError> {
        let agent = agent.to_owned();
        let protection = self.protection.clone();
        let owner = self.owner;
        self.call(false, move |tx| {
            let stored = load(tx, &protection, owner, run)?.ok_or(BridgeError::NotFound)?;
            if stored.profile.agent_id != agent {
                return Err(BridgeError::NotFound);
            }
            Ok(stored)
        })
        .await
    }
    pub async fn pending(
        &self,
        identity: &IdentityBinding,
    ) -> Result<Vec<PreparedRun>, BridgeError> {
        if identity.owner_id().as_str() != self.owner.to_string() {
            return Err(BridgeError::Invalid);
        }
        let observer = identity.individual_id().as_str().to_owned();
        let protection = self.protection.clone();
        let owner = self.owner;
        self.call(false,move|tx|{let mut statement=tx.prepare("SELECT run FROM bridge_events WHERE observer=?1 AND pending=1 ORDER BY rowid LIMIT 64").map_err(|_|BridgeError::Unavailable)?;let rows=statement.query_map([observer],|r|r.get::<_,String>(0)).map_err(|_|BridgeError::Unavailable)?;let mut runs=vec![];for row in rows{let id=Uuid::parse_str(&row.map_err(|_|BridgeError::Unavailable)?).map_err(|_|BridgeError::Unavailable)?;runs.push(load(tx,&protection,owner,id)?.ok_or(BridgeError::Unavailable)?);}Ok(runs)}).await
    }
    pub async fn ack(
        &self,
        run: Uuid,
        receipt: anima_memory::MemoryEffectReceipt,
    ) -> Result<(), BridgeError> {
        let protection = self.protection.clone();
        let owner = self.owner;
        self.call(true, move |tx| {
            let accepted = load(tx, &protection, owner, run)?.ok_or(BridgeError::NotFound)?;
            if receipt.identity != accepted.binding.individual
                || receipt.source_event_id != accepted.effect.source_event_id()
            {
                return Err(BridgeError::Conflict);
            }
            tx.execute(
                "UPDATE bridge_events SET pending=0 WHERE run=?1 AND event=?2",
                params![run.to_string(), receipt.source_event_id],
            )
            .map_err(|_| BridgeError::Unavailable)?;
            Ok(())
        })
        .await
    }
    pub async fn response(&self, run: Uuid) -> Result<Option<serde_json::Value>, BridgeError> {
        let protection = self.protection.clone();
        let owner = self.owner;
        self.call(false, move |tx| {
            let bytes: Option<Vec<u8>> = tx
                .query_row(
                    "SELECT response FROM bridge_runs WHERE run=?1",
                    [run.to_string()],
                    |r| r.get(0),
                )
                .optional()
                .map_err(|_| BridgeError::Unavailable)?
                .flatten();
            bytes
                .map(|bytes| {
                    let bytes = protection
                        .restore_host_admission(owner, run, &bytes)
                        .map_err(|_| BridgeError::Unavailable)?;
                    serde_json::from_slice(&bytes).map_err(|_| BridgeError::Unavailable)
                })
                .transpose()
        })
        .await
    }
    pub async fn finish(
        &self,
        run: Uuid,
        response: serde_json::Value,
    ) -> Result<serde_json::Value, BridgeError> {
        let protection = self.protection.clone();
        let owner = self.owner;
        self.call(true, move |tx| {
            let bytes = serde_json::to_vec(&response).map_err(|_| BridgeError::Invalid)?;
            let protected = protection
                .protect_host_admission(owner, run, &bytes)
                .map_err(|_| BridgeError::Unavailable)?;
            tx.execute(
                "UPDATE bridge_runs SET response=?1 WHERE run=?2 AND response IS NULL",
                params![protected, run.to_string()],
            )
            .map_err(|_| BridgeError::Unavailable)?;
            Ok(response)
        })
        .await
    }
    pub async fn action(
        &self,
        agent: &str,
        run: Uuid,
        action: Option<&str>,
    ) -> Result<String, BridgeError> {
        let agent = agent.to_owned();
        let action = action.map(str::to_owned);
        self.call(action.is_some(), move |tx| {
            let current: Option<String> = tx
                .query_row(
                    "SELECT action FROM bridge_runs WHERE agent=?1 AND run=?2",
                    params![agent, run.to_string()],
                    |r| r.get(0),
                )
                .optional()
                .map_err(|_| BridgeError::Unavailable)?;
            let current = current.ok_or(BridgeError::NotFound)?;
            if let Some(action) = action {
                if current == "stop" && action != "stop" {
                    return Err(BridgeError::Conflict);
                }
                tx.execute(
                    "UPDATE bridge_runs SET action=?1 WHERE agent=?2 AND run=?3",
                    params![action, agent, run.to_string()],
                )
                .map_err(|_| BridgeError::Unavailable)?;
                Ok(action)
            } else {
                Ok(current)
            }
        })
        .await
    }
    pub async fn shutdown(&self) -> Result<(), BridgeError> {
        let (done, closed) = oneshot::channel();
        self.sender
            .send(Job::Shutdown(done))
            .await
            .map_err(|_| BridgeError::Unavailable)?;
        closed.await.map_err(|_| BridgeError::Unavailable)
    }
    pub async fn clock_result(
        &self,
        invocation: LogicalInvocation,
        now: Option<u64>,
    ) -> Result<Option<EngineCapabilityResult>, BridgeError> {
        let protection = self.protection.clone();
        let owner = self.owner;
        self.call(now.is_some(), move |tx| {
            let accepted =
                load(tx, &protection, owner, invocation.run_id())?.ok_or(BridgeError::NotFound)?;
            if !accepted.profile.clock_enabled
                || invocation.capability_id() != super::clock_capability::CLOCK_ID
                || invocation.manifest_version() != 1
                || invocation.normalized_arguments() != &serde_json::json!({})
            {
                return Err(BridgeError::Invalid);
            }
            let stored: Option<Vec<u8>> = tx
                .query_row(
                    "SELECT payload FROM bridge_capability_receipts WHERE invocation=?1 AND run=?2",
                    params![invocation.id().to_string(), invocation.run_id().to_string()],
                    |r| r.get(0),
                )
                .optional()
                .map_err(|_| BridgeError::Unavailable)?;
            if let Some(stored) = stored {
                let bytes = protection
                    .restore_host_admission(owner, invocation.id(), &stored)
                    .map_err(|_| BridgeError::Unavailable)?;
                let stored: ClockReceipt =
                    serde_json::from_slice(&bytes).map_err(|_| BridgeError::Unavailable)?;
                if stored.invocation != invocation {
                    return Err(BridgeError::Conflict);
                }
                return Ok(Some(EngineCapabilityResult {
                    output: stored.output,
                    durable: stored.durable,
                }));
            }
            let Some(now) = now else {
                return Ok(None);
            };
            let output = CapabilityResult::new(serde_json::json!({"unix_ms":now}));
            let bytes = serde_jcs::to_vec(&output.output).map_err(|_| BridgeError::Unavailable)?;
            use sha2::Digest;
            let digest = format!("jcs-v1:{}", hex::encode(sha2::Sha256::digest(&bytes)));
            let manifest = super::clock_manifest().map_err(|_| BridgeError::Invalid)?;
            let durable = DurableCapabilityResult::new(
                CapabilityReferenceId::new(invocation.id()),
                digest,
                manifest.schema_digest(),
                bytes.len() as u64,
                DurableCapabilityStatus::Completed,
            )
            .map_err(|_| BridgeError::Invalid)?;
            let receipt = ClockReceipt {
                invocation: invocation.clone(),
                output: output.clone(),
                durable: durable.clone(),
            };
            let bytes = serde_json::to_vec(&receipt).map_err(|_| BridgeError::Invalid)?;
            let bytes = protection
                .protect_host_admission(owner, invocation.id(), &bytes)
                .map_err(|_| BridgeError::Unavailable)?;
            tx.execute(
                "INSERT INTO bridge_capability_receipts VALUES(?1,?2,?3)",
                params![
                    invocation.id().to_string(),
                    invocation.run_id().to_string(),
                    bytes
                ],
            )
            .map_err(|_| BridgeError::Unavailable)?;
            Ok(Some(EngineCapabilityResult { output, durable }))
        })
        .await
    }
}
#[derive(serde::Serialize, serde::Deserialize)]
struct ClockReceipt {
    invocation: LogicalInvocation,
    output: CapabilityResult,
    durable: DurableCapabilityResult,
}
fn load(
    tx: &Transaction<'_>,
    protection: &PersistenceProtection,
    owner: Uuid,
    run: Uuid,
) -> Result<Option<PreparedRun>, BridgeError> {
    let bytes: Option<Vec<u8>> = tx
        .query_row(
            "SELECT payload FROM bridge_runs WHERE run=?1",
            [run.to_string()],
            |r| r.get(0),
        )
        .optional()
        .map_err(|_| BridgeError::Unavailable)?;
    bytes
        .map(|bytes| {
            let bytes = protection
                .restore_host_admission(owner, run, &bytes)
                .map_err(|_| BridgeError::Unavailable)?;
            let accepted: PreparedRun =
                serde_json::from_slice(&bytes).map_err(|_| BridgeError::Unavailable)?;
            if accepted.binding.owner != owner || accepted.binding.run != run {
                return Err(BridgeError::Unavailable);
            }
            Ok(accepted)
        })
        .transpose()
}
fn load_memory_source(
    tx: &Transaction<'_>,
    protection: &PersistenceProtection,
    owner: Uuid,
    identity: &IdentityBinding,
    source: Uuid,
) -> Result<Option<StoredMemorySource>, BridgeError> {
    let row:Option<(Vec<u8>,bool,Option<Vec<u8>>)>=tx.query_row("SELECT payload,pending,outcome FROM bridge_memory_sources WHERE observer=?1 AND source=?2",params![identity.individual_id().as_str(),source.to_string()],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))).optional().map_err(|_|BridgeError::Unavailable)?;
    row.map(|(payload, pending, outcome)| {
        let payload = protection
            .restore_host_admission(owner, source, &payload)
            .map_err(|_| BridgeError::Unavailable)?;
        let accepted: AcceptedMemorySource =
            serde_json::from_slice(&payload).map_err(|_| BridgeError::Unavailable)?;
        if accepted.version != 1
            || &accepted.identity != identity
            || accepted.identity.owner_id().as_str() != owner.to_string()
            || accepted.request.source_event_id != source
        {
            return Err(BridgeError::Unavailable);
        }
        let outcome = outcome
            .map(|bytes| {
                let bytes = protection
                    .restore_host_admission(owner, source, &bytes)
                    .map_err(|_| BridgeError::Unavailable)?;
                serde_json::from_slice::<MemorySourceOutcome>(&bytes)
                    .map_err(|_| BridgeError::Unavailable)
            })
            .transpose()?;
        if pending != outcome.is_none() {
            return Err(BridgeError::Unavailable);
        }
        if let Some(MemorySourceOutcome::Applied { receipt }) = &outcome {
            if receipt.identity != accepted.identity
                || receipt.source_event_id != accepted.effect.source_event_id()
            {
                return Err(BridgeError::Unavailable);
            }
        }
        Ok(StoredMemorySource { accepted, outcome })
    })
    .transpose()
}
#[async_trait]
impl MemoryProjectionReadiness for Outbox {
    async fn ensure_projected(
        &self,
        identity: &IdentityBinding,
    ) -> Result<(), anima_core::MemoryPortError> {
        if self
            .pending(identity)
            .await
            .map_err(|_| unavailable())?
            .is_empty()
            && self
                .pending_memory_sources(identity)
                .await
                .map_err(|_| unavailable())?
                .is_empty()
        {
            Ok(())
        } else {
            Err(unavailable())
        }
    }
}
fn unavailable() -> anima_core::MemoryPortError {
    anima_core::MemoryPortError {
        code: anima_core::MemoryPortErrorCode::Backend,
        message: "accepted daemon memory effects remain pending or unavailable".into(),
    }
}
