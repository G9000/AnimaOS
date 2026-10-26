use crate::backend;
use anima_core::MemoryPortError;
use rusqlite::Connection;
use std::time::{Duration, Instant};

const APPLICATION_ID: u32 = 0x414d454d;

fn configure_wal(connection: &Connection) -> rusqlite::Result<()> {
    let timeout = Duration::from_secs(2);
    let deadline = Instant::now() + timeout;
    loop {
        // SQLite may skip its busy handler for a journal-mode lock upgrade.
        // Keep schema validation and identity creation outside this retry.
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

pub(crate) fn store_id(connection: &Connection) -> Result<String, MemoryPortError> {
    let stored: String = connection
        .query_row(
            "SELECT store_id FROM memory_store_metadata WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .map_err(backend)?;
    let id = uuid::Uuid::parse_str(&stored).map_err(backend)?;
    if id.is_nil() || id.to_string() != stored {
        return Err(backend("invalid memory store identity"));
    }
    Ok(stored)
}

fn identify(connection: &Connection) -> Result<u32, MemoryPortError> {
    let application: u32 = connection
        .query_row("PRAGMA application_id", [], |r| r.get(0))
        .map_err(backend)?;
    let version: u32 = connection
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(backend)?;
    let nonempty: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%')",[],|r|r.get(0)).map_err(backend)?;
    if version > 1
        || (application != 0 && application != APPLICATION_ID)
        || (application == 0 && (version != 0 || nonempty))
        || (application == APPLICATION_ID && version != 1)
    {
        return Err(MemoryPortError::unsupported(
            "unrecognized or unsupported memory database schema",
        ));
    }
    Ok(version)
}

pub(crate) fn initialize(connection: &mut Connection) -> Result<(), MemoryPortError> {
    connection
        .busy_timeout(std::time::Duration::from_secs(2))
        .map_err(backend)?;
    {
        // Schema/header checks must share one snapshot. A concurrent first
        // opener can commit between otherwise independent pragma reads.
        let preflight = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Deferred)
            .map_err(backend)?;
        identify(&preflight)?;
        preflight.commit().map_err(backend)?;
    }
    connection
        .pragma_update(None, "foreign_keys", true)
        .map_err(backend)?;
    configure_wal(connection).map_err(backend)?;
    connection
        .pragma_update(None, "synchronous", "FULL")
        .map_err(backend)?;
    let tx = connection
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(backend)?;
    let version = identify(&tx)?;
    if version == 0 {
        tx.execute_batch(r#"
      CREATE TABLE IF NOT EXISTS memory_identities(owner TEXT NOT NULL, individual TEXT NOT NULL, PRIMARY KEY(owner, individual));
      CREATE TABLE memory_store_metadata(singleton INTEGER PRIMARY KEY CHECK(singleton=1), store_id TEXT NOT NULL);
      CREATE TABLE memory_clock(singleton INTEGER PRIMARY KEY CHECK(singleton=1), floor_ms TEXT NOT NULL);
      INSERT INTO memory_clock VALUES(1,'00000000000000000000');
      CREATE TABLE IF NOT EXISTS memory_residents(owner TEXT NOT NULL, world TEXT NOT NULL, resident TEXT NOT NULL, individual TEXT NOT NULL, PRIMARY KEY(owner,world,resident), UNIQUE(owner,individual), FOREIGN KEY(owner,individual) REFERENCES memory_identities(owner,individual));
      CREATE TABLE IF NOT EXISTS memory_records(owner TEXT NOT NULL, id TEXT NOT NULL, writer TEXT NOT NULL, scope TEXT NOT NULL, revision TEXT NOT NULL, observed TEXT NOT NULL, deadline TEXT, forgotten INTEGER NOT NULL, original_write TEXT NOT NULL, record TEXT NOT NULL, PRIMARY KEY(owner,id), FOREIGN KEY(owner,writer) REFERENCES memory_identities(owner,individual));
      CREATE INDEX IF NOT EXISTS memory_scope_recent ON memory_records(owner,scope,observed DESC,id);
      CREATE TABLE IF NOT EXISTS memory_history(owner TEXT NOT NULL, id TEXT NOT NULL, revision TEXT NOT NULL, actor TEXT NOT NULL, mutation TEXT NOT NULL, record TEXT NOT NULL, PRIMARY KEY(owner,id,revision), FOREIGN KEY(owner,id) REFERENCES memory_records(owner,id), FOREIGN KEY(owner,actor) REFERENCES memory_identities(owner,individual));
      CREATE TABLE IF NOT EXISTS memory_supersessions(owner TEXT NOT NULL, superseder TEXT NOT NULL, target TEXT NOT NULL, PRIMARY KEY(owner,superseder,target), FOREIGN KEY(owner,superseder) REFERENCES memory_records(owner,id), FOREIGN KEY(owner,target) REFERENCES memory_records(owner,id));
      CREATE INDEX IF NOT EXISTS memory_supersession_targets ON memory_supersessions(owner,target);
      CREATE TABLE IF NOT EXISTS memory_relationships(owner TEXT NOT NULL, observer TEXT NOT NULL, id TEXT NOT NULL, scope TEXT NOT NULL, observed TEXT NOT NULL, observation TEXT NOT NULL, PRIMARY KEY(owner,observer,id), FOREIGN KEY(owner,observer) REFERENCES memory_identities(owner,individual));
      CREATE TABLE IF NOT EXISTS memory_receipts(owner TEXT NOT NULL, observer TEXT NOT NULL, namespace TEXT NOT NULL, source_id TEXT NOT NULL, payload TEXT NOT NULL, receipt TEXT NOT NULL, PRIMARY KEY(owner,observer,namespace,source_id), FOREIGN KEY(owner,observer) REFERENCES memory_identities(owner,individual));
      PRAGMA user_version=1;
      PRAGMA application_id=1095583053;
    "#).map_err(backend)?;
        tx.execute(
            "INSERT INTO memory_store_metadata(singleton,store_id) VALUES(1,?1)",
            [uuid::Uuid::new_v4().to_string()],
        )
        .map_err(backend)?;
    }
    // A recognized store's identity is never minted again, even when metadata
    // was deleted or damaged. Hosts pin this ID with accepted source outboxes.
    store_id(&tx)?;
    // A known version with missing tables is corruption, never a request to recreate them.
    for sql in [
        "SELECT floor_ms FROM memory_clock WHERE singleton=1",
        "SELECT owner,individual FROM memory_identities LIMIT 0",
        "SELECT owner,world,resident,individual FROM memory_residents LIMIT 0",
        "SELECT owner,id,writer,scope,revision,observed,deadline,forgotten,original_write,record FROM memory_records LIMIT 0",
        "SELECT owner,id,revision,actor,mutation,record FROM memory_history LIMIT 0",
        "SELECT owner,superseder,target FROM memory_supersessions LIMIT 0",
        "SELECT owner,observer,id,scope,observed,observation FROM memory_relationships LIMIT 0",
        "SELECT owner,observer,namespace,source_id,payload,receipt FROM memory_receipts LIMIT 0",
    ] { tx.prepare(sql).map_err(backend)?; }
    {
        let mut check = tx.prepare("PRAGMA foreign_key_check").map_err(backend)?;
        if check
            .query([])
            .map_err(backend)?
            .next()
            .map_err(backend)?
            .is_some()
        {
            return Err(backend("memory database foreign key violation"));
        }
    }
    tx.commit().map_err(backend)
}
