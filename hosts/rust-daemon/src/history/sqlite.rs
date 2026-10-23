//! SQLite history store (spec §13.1): WAL mode, FTS5 search, a versioned
//! schema, and one dedicated connection driven through `spawn_blocking`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension};

use super::{
    message_ordinal, role_name, search_tokens, searchable_text, to_i64, validate_after_query,
    validate_filtered_message_query, validate_filtered_query, ApprovalPageQuery,
    FilteredMessageSearchQuery, FilteredSessionSearchQuery, HistoryError, HistoryMessage,
    HistoryStore, MessageAfterQuery, MessagePageQuery, UsagePageQuery,
};
use crate::approvals::ApprovalRequest;
use crate::attachments::AttachmentRecord;
use crate::runs::RunRecord;
use crate::schedules::ScheduleFireRecord;
use crate::usage::UsageRecord;

/// `PRAGMA user_version` of the schema this daemon writes.
pub(crate) const SQLITE_HISTORY_SCHEMA_VERSION: i64 = 1;
const ID_CHUNK: usize = 500;

const SCHEMA_V1: &str = "
BEGIN;
CREATE TABLE messages (
    id TEXT NOT NULL PRIMARY KEY,
    agent_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    role TEXT NOT NULL,
    text TEXT NOT NULL,
    hidden INTEGER NOT NULL DEFAULT 0,
    created_at_ms INTEGER NOT NULL,
    ordinal INTEGER NOT NULL,
    record TEXT NOT NULL
);
CREATE INDEX messages_session_order ON messages (agent_id, session_id, created_at_ms, ordinal, id);
CREATE VIRTUAL TABLE messages_fts USING fts5(text, content = 'messages', content_rowid = 'rowid', tokenize = 'unicode61 remove_diacritics 0');
CREATE TRIGGER messages_fts_insert AFTER INSERT ON messages BEGIN
    INSERT INTO messages_fts (rowid, text) VALUES (new.rowid, new.text);
END;
CREATE TRIGGER messages_fts_delete AFTER DELETE ON messages BEGIN
    INSERT INTO messages_fts (messages_fts, rowid, text) VALUES ('delete', old.rowid, old.text);
END;
CREATE TRIGGER messages_fts_update AFTER UPDATE OF text ON messages BEGIN
    INSERT INTO messages_fts (messages_fts, rowid, text) VALUES ('delete', old.rowid, old.text);
    INSERT INTO messages_fts (rowid, text) VALUES (new.rowid, new.text);
END;
CREATE TABLE runs (
    id TEXT NOT NULL PRIMARY KEY,
    agent_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL,
    finished_at_ms INTEGER,
    record TEXT NOT NULL
);
CREATE INDEX runs_session ON runs (agent_id, session_id, created_at_ms);
CREATE TABLE usage (
    id TEXT NOT NULL PRIMARY KEY,
    agent_id TEXT NOT NULL,
    session_id TEXT,
    run_id TEXT,
    created_at_ms INTEGER NOT NULL,
    record TEXT NOT NULL
);
CREATE INDEX usage_created ON usage (created_at_ms);
CREATE TABLE approvals (
    id TEXT NOT NULL PRIMARY KEY,
    agent_id TEXT NOT NULL,
    session_id TEXT,
    created_at_ms INTEGER NOT NULL,
    record TEXT NOT NULL
);
CREATE INDEX approvals_created ON approvals (agent_id, created_at_ms);
CREATE TABLE schedule_runs (
    id TEXT NOT NULL PRIMARY KEY,
    schedule_id TEXT NOT NULL,
    agent_id TEXT NOT NULL,
    fired_at_ms INTEGER NOT NULL,
    record TEXT NOT NULL
);
CREATE INDEX schedule_runs_schedule ON schedule_runs (schedule_id, fired_at_ms);
CREATE TABLE attachments (
    id TEXT NOT NULL PRIMARY KEY,
    agent_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL,
    record TEXT NOT NULL
);
CREATE INDEX attachments_session ON attachments (agent_id, session_id);
PRAGMA user_version = 1;
COMMIT;
";

const UPSERT_MESSAGE: &str = "
INSERT INTO messages (id, agent_id, session_id, role, text, hidden, created_at_ms, ordinal, record)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
ON CONFLICT (id) DO UPDATE SET
    agent_id = excluded.agent_id,
    session_id = excluded.session_id,
    role = excluded.role,
    text = excluded.text,
    hidden = excluded.hidden,
    created_at_ms = excluded.created_at_ms,
    ordinal = excluded.ordinal,
    record = excluded.record";

const UPSERT_RUN: &str = "
INSERT INTO runs (id, agent_id, session_id, status, created_at_ms, finished_at_ms, record)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
ON CONFLICT (id) DO UPDATE SET
    agent_id = excluded.agent_id,
    session_id = excluded.session_id,
    status = excluded.status,
    created_at_ms = excluded.created_at_ms,
    finished_at_ms = excluded.finished_at_ms,
    record = excluded.record";

const UPSERT_APPROVAL: &str = "
INSERT INTO approvals (id, agent_id, session_id, created_at_ms, record)
VALUES (?1, ?2, ?3, ?4, ?5)
ON CONFLICT (id) DO UPDATE SET
    agent_id = excluded.agent_id,
    session_id = excluded.session_id,
    created_at_ms = excluded.created_at_ms,
    record = excluded.record";

const PAGE_APPROVALS: &str = "
SELECT record FROM approvals
WHERE (?1 IS NULL OR agent_id = ?1) AND created_at_ms >= ?2
  AND (?3 IS NULL OR created_at_ms < ?3 OR (created_at_ms = ?3 AND id < ?4))
ORDER BY created_at_ms DESC, id DESC
LIMIT ?5";

const UPSERT_SCHEDULE_RUN: &str = "
INSERT INTO schedule_runs (id, schedule_id, agent_id, fired_at_ms, record)
VALUES (?1, ?2, ?3, ?4, ?5)
ON CONFLICT (id) DO UPDATE SET
    schedule_id = excluded.schedule_id,
    agent_id = excluded.agent_id,
    fired_at_ms = excluded.fired_at_ms,
    record = excluded.record";

const PAGE_SCHEDULE_RUNS: &str = "
SELECT record FROM schedule_runs
WHERE agent_id = ?1 AND schedule_id = ?2
ORDER BY fired_at_ms DESC, id DESC
LIMIT ?3";

/// A stored row keeps its price fields when the same id is written again
/// (spec §11.1: priced when first mirrored); a legacy row without them takes
/// the new ones.
const UPSERT_USAGE: &str = "
INSERT INTO usage (id, agent_id, session_id, run_id, created_at_ms, record)
VALUES (?1, ?2, ?3, ?4, ?5, ?6)
ON CONFLICT (id) DO UPDATE SET
    agent_id = excluded.agent_id,
    session_id = excluded.session_id,
    run_id = excluded.run_id,
    created_at_ms = excluded.created_at_ms,
    record = CASE
        WHEN json_extract(usage.record, '$.pricingSource') IS NOT NULL
        THEN json_set(excluded.record,
                      '$.costMicros', json_extract(usage.record, '$.costMicros'),
                      '$.pricingSource', json_extract(usage.record, '$.pricingSource'))
        ELSE excluded.record
    END";

const PAGE_USAGE: &str = "
SELECT record FROM usage
WHERE created_at_ms >= ?1 AND created_at_ms < ?2
  AND (?3 IS NULL OR agent_id = ?3) AND (?4 IS NULL OR session_id = ?4)
  AND (?5 IS NULL OR created_at_ms < ?5 OR (created_at_ms = ?5 AND id < ?6))
ORDER BY created_at_ms DESC, id DESC
LIMIT ?7";

const PAGE_MESSAGES: &str = "
SELECT agent_id, session_id, hidden, record FROM messages
WHERE agent_id = ?1 AND session_id = ?2 AND (?3 OR hidden = 0)
  AND (?4 IS NULL OR created_at_ms < ?4
       OR (created_at_ms = ?4 AND (ordinal < ?5 OR (ordinal = ?5 AND id < ?6))))
ORDER BY created_at_ms DESC, ordinal DESC, id DESC
LIMIT ?7";

const PAGE_MESSAGES_AFTER: &str = "
SELECT agent_id, session_id, hidden, record FROM messages
WHERE agent_id = ?1 AND session_id = ?2 AND (?3 OR hidden = 0)
  AND (created_at_ms > ?4
       OR (created_at_ms = ?4 AND (ordinal > ?5 OR (ordinal = ?5 AND id > ?6))))
ORDER BY created_at_ms ASC, ordinal ASC, id ASC
LIMIT ?7";

impl From<rusqlite::Error> for HistoryError {
    fn from(error: rusqlite::Error) -> Self {
        Self::new(error)
    }
}

pub(crate) struct SqliteHistoryStore {
    connection: Arc<OwnedHistoryConnection>,
    path: PathBuf,
}

struct OwnedHistoryConnection {
    // SQLite close/WAL cleanup must finish before the final owner is dropped.
    connection: Mutex<Connection>,
    owner: Option<crate::profile_owner::ProfileOwner>,
}

impl SqliteHistoryStore {
    /// Direct test/ephemeral stores deliberately have no production authority.
    pub(crate) async fn open(path: PathBuf) -> Result<Self, HistoryError> {
        Self::open_with_owner(path, None).await
    }

    pub(crate) async fn open_with_owner(
        path: PathBuf,
        owner: Option<crate::profile_owner::ProfileOwner>,
    ) -> Result<Self, HistoryError> {
        let path = owner
            .as_ref()
            .map(|owner| owner.owned_sqlite_path(&path))
            .transpose()
            .map_err(HistoryError::new)?
            .unwrap_or(path);
        let opening = path.clone();
        // The blocking task itself owns the guard. Cancelling startup cannot
        // release a lease while SQLite still opens/migrates its payload.
        let connection = tokio::task::spawn_blocking(move || {
            if let Some(owner) = &owner {
                owner.ensure_owned().map_err(HistoryError::new)?;
            }
            let connection = open_connection(&opening)?;
            Ok::<_, HistoryError>(OwnedHistoryConnection {
                connection: Mutex::new(connection),
                owner,
            })
        })
        .await
        .map_err(|error| HistoryError::new(format!("history store worker stopped: {error}")))??;
        Ok(Self {
            connection: Arc::new(connection),
            path,
        })
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// Runs `work` on the dedicated connection off the async runtime.
    async fn run<T, F>(&self, work: F) -> Result<T, HistoryError>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T, HistoryError> + Send + 'static,
    {
        let connection = Arc::clone(&self.connection);
        tokio::task::spawn_blocking(move || {
            let mut locked = connection
                .connection
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(owner) = &connection.owner {
                owner.ensure_owned().map_err(HistoryError::new)?;
            }
            work(&mut locked)
        })
        .await
        .map_err(|error| HistoryError::new(format!("history store worker stopped: {error}")))?
    }
}

fn open_connection(path: &Path) -> Result<Connection, HistoryError> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    let connection = Connection::open(path)?;
    connection.busy_timeout(Duration::from_secs(5))?;
    let mode: String = connection.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
    if !mode.eq_ignore_ascii_case("wal") {
        return Err(HistoryError::new(format!(
            "history store could not enable WAL mode (got {mode})"
        )));
    }
    connection.pragma_update(None, "synchronous", "FULL")?;
    let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    match version {
        0 => connection.execute_batch(SCHEMA_V1)?,
        SQLITE_HISTORY_SCHEMA_VERSION => {}
        newer => {
            return Err(HistoryError::new(format!(
                "history store schema version {newer} is newer than this daemon supports ({SQLITE_HISTORY_SCHEMA_VERSION})"
            )))
        }
    }
    Ok(connection)
}

fn placeholders(first: usize, count: usize) -> String {
    (first..first + count)
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ")
}

type RawRow = (String, String, bool, String);

fn raw_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawRow> {
    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
}

fn decode(rows: Vec<RawRow>) -> Result<Vec<HistoryMessage>, HistoryError> {
    rows.into_iter()
        .map(
            |(agent_id, session_id, hidden, record)| -> Result<HistoryMessage, HistoryError> {
                Ok(HistoryMessage {
                    agent_id,
                    session_id,
                    hidden,
                    message: serde_json::from_str(&record)?,
                })
            },
        )
        .collect()
}

struct MessageRow {
    id: String,
    agent_id: String,
    session_id: String,
    role: &'static str,
    text: String,
    hidden: bool,
    created_at_ms: i64,
    ordinal: i64,
    record: String,
}

struct RunRow {
    id: String,
    agent_id: String,
    session_id: String,
    status: String,
    created_at_ms: i64,
    finished_at_ms: Option<i64>,
    record: String,
}

#[async_trait]
impl HistoryStore for SqliteHistoryStore {
    fn label(&self) -> &'static str {
        "sqlite"
    }

    async fn upsert_attachments(&self, rows: &[AttachmentRecord]) -> Result<(), HistoryError> {
        let rows = rows
            .iter()
            .map(|row| {
                Ok((
                    row.id.clone(),
                    row.agent_id.clone(),
                    row.session_id.clone(),
                    to_i64(row.created_at_ms)?,
                    serde_json::to_string(row)?,
                ))
            })
            .collect::<Result<Vec<_>, HistoryError>>()?;
        self.run(move |connection| {
            let transaction = connection.transaction()?;
            for (id, agent, session, created, record) in &rows {
                let count = transaction.execute("INSERT INTO attachments (id, agent_id, session_id, created_at_ms, record) VALUES (?1,?2,?3,?4,?5)
                    ON CONFLICT(id) DO UPDATE SET record = attachments.record
                    WHERE attachments.agent_id = excluded.agent_id AND attachments.session_id = excluded.session_id
                      AND attachments.created_at_ms = excluded.created_at_ms AND attachments.record = excluded.record",
                    params![id,agent,session,created,record])?;
                if count != 1 { return Err(HistoryError::new(super::ATTACHMENT_ID_CONFLICT)); }
            }
            transaction.commit()?;
            Ok(())
        }).await
    }
    async fn get_attachment(
        &self,
        agent_id: &str,
        attachment_id: &str,
    ) -> Result<Option<AttachmentRecord>, HistoryError> {
        let agent = agent_id.to_string();
        let id = attachment_id.to_string();
        self.run(move |connection| {
            let record: Option<String> = connection
                .query_row(
                    "SELECT record FROM attachments WHERE agent_id = ?1 AND id = ?2",
                    params![agent, id],
                    |row| row.get(0),
                )
                .optional()?;
            record
                .map(|json| serde_json::from_str(&json).map_err(HistoryError::from))
                .transpose()
        })
        .await
    }

    async fn upsert_messages(&self, messages: &[HistoryMessage]) -> Result<(), HistoryError> {
        if messages.is_empty() {
            return Ok(());
        }
        let rows = messages
            .iter()
            .map(|row| -> Result<MessageRow, HistoryError> {
                Ok(MessageRow {
                    id: row.message.id.clone(),
                    agent_id: row.agent_id.clone(),
                    session_id: row.session_id.clone(),
                    role: role_name(row.message.role),
                    // Indexed for FTS; a check-in prompt's scheduler suffix
                    // is stripped so it can't match every search (review
                    // fix, M2 fix round 1), and the text is capped to
                    // MAX_INDEXED_TEXT_BYTES so one huge message can't break
                    // FTS indexing (final fix wave item B). `record` keeps
                    // the full message.
                    text: searchable_text(&row.message).to_string(),
                    hidden: row.hidden,
                    created_at_ms: to_i64(row.message.created_at_ms)?,
                    ordinal: to_i64(message_ordinal(&row.message.id))?,
                    record: serde_json::to_string(&row.message)?,
                })
            })
            .collect::<Result<Vec<_>, HistoryError>>()?;
        self.run(move |connection| {
            let transaction = connection.transaction()?;
            {
                let mut statement = transaction.prepare_cached(UPSERT_MESSAGE)?;
                for row in &rows {
                    statement.execute(params![
                        row.id,
                        row.agent_id,
                        row.session_id,
                        row.role,
                        row.text,
                        row.hidden,
                        row.created_at_ms,
                        row.ordinal,
                        row.record
                    ])?;
                }
            }
            transaction.commit()?;
            Ok(())
        })
        .await
    }

    async fn upsert_runs(&self, runs: &[RunRecord]) -> Result<(), HistoryError> {
        if runs.is_empty() {
            return Ok(());
        }
        let rows = runs
            .iter()
            .map(|run| -> Result<RunRow, HistoryError> {
                Ok(RunRow {
                    id: run.id.clone(),
                    agent_id: run.agent_id.clone(),
                    session_id: run.session_id.clone(),
                    status: serde_json::to_value(run.status)?
                        .as_str()
                        .unwrap_or("unknown")
                        .to_string(),
                    created_at_ms: to_i64(run.created_at_ms)?,
                    finished_at_ms: run.finished_at_ms.map(to_i64).transpose()?,
                    record: serde_json::to_string(run)?,
                })
            })
            .collect::<Result<Vec<_>, HistoryError>>()?;
        self.run(move |connection| {
            let transaction = connection.transaction()?;
            {
                let mut statement = transaction.prepare_cached(UPSERT_RUN)?;
                for row in &rows {
                    statement.execute(params![
                        row.id,
                        row.agent_id,
                        row.session_id,
                        row.status,
                        row.created_at_ms,
                        row.finished_at_ms,
                        row.record
                    ])?;
                }
            }
            transaction.commit()?;
            Ok(())
        })
        .await
    }

    async fn upsert_approvals(&self, approvals: &[ApprovalRequest]) -> Result<(), HistoryError> {
        if approvals.is_empty() {
            return Ok(());
        }
        let rows = approvals
            .iter()
            .map(
                |approval| -> Result<(String, String, String, i64, String), HistoryError> {
                    Ok((
                        approval.id.clone(),
                        approval.agent_id.clone(),
                        approval.session_id.clone(),
                        to_i64(approval.created_at_ms)?,
                        serde_json::to_string(approval)?,
                    ))
                },
            )
            .collect::<Result<Vec<_>, HistoryError>>()?;
        self.run(move |connection| {
            let transaction = connection.transaction()?;
            {
                let mut statement = transaction.prepare_cached(UPSERT_APPROVAL)?;
                for (id, agent_id, session_id, created_at_ms, record) in &rows {
                    statement.execute(params![id, agent_id, session_id, created_at_ms, record])?;
                }
            }
            transaction.commit()?;
            Ok(())
        })
        .await
    }

    async fn get_approval(
        &self,
        approval_id: &str,
    ) -> Result<Option<ApprovalRequest>, HistoryError> {
        let approval_id = approval_id.to_string();
        self.run(move |connection| {
            let record = connection
                .query_row(
                    "SELECT record FROM approvals WHERE id = ?1",
                    params![approval_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            record
                .map(|record| serde_json::from_str(&record).map_err(HistoryError::from))
                .transpose()
        })
        .await
    }

    async fn page_approvals(
        &self,
        query: &ApprovalPageQuery,
    ) -> Result<Vec<ApprovalRequest>, HistoryError> {
        let query = query.clone();
        self.run(move |connection| {
            let (before_at, before_id) = match &query.before {
                Some((at_ms, id)) => (Some(to_i64(*at_ms)?), id.clone()),
                None => (None, String::new()),
            };
            let mut statement = connection.prepare_cached(PAGE_APPROVALS)?;
            let records = statement
                .query_map(
                    params![
                        query.agent_id,
                        to_i64(query.since_ms)?,
                        before_at,
                        before_id,
                        i64::try_from(query.limit).unwrap_or(i64::MAX)
                    ],
                    |row| row.get::<_, String>(0),
                )?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            records
                .iter()
                .map(|record| serde_json::from_str(record).map_err(HistoryError::from))
                .collect()
        })
        .await
    }

    async fn upsert_usage(&self, records: &[UsageRecord]) -> Result<(), HistoryError> {
        if records.is_empty() {
            return Ok(());
        }
        type Row = (String, String, Option<String>, Option<String>, i64, String);
        let rows = records
            .iter()
            .map(|record| -> Result<Row, HistoryError> {
                Ok((
                    record.id.clone(),
                    record.agent_id.clone(),
                    record.session_id.clone(),
                    record.run_id.clone(),
                    to_i64(record.created_at_ms)?,
                    serde_json::to_string(record)?,
                ))
            })
            .collect::<Result<Vec<_>, HistoryError>>()?;
        self.run(move |connection| {
            let transaction = connection.transaction()?;
            {
                let mut statement = transaction.prepare_cached(UPSERT_USAGE)?;
                for (id, agent_id, session_id, run_id, created_at_ms, record) in &rows {
                    statement.execute(params![
                        id,
                        agent_id,
                        session_id,
                        run_id,
                        created_at_ms,
                        record
                    ])?;
                }
            }
            transaction.commit()?;
            Ok(())
        })
        .await
    }

    async fn page_usage(&self, query: &UsagePageQuery) -> Result<Vec<UsageRecord>, HistoryError> {
        let query = query.clone();
        self.run(move |connection| {
            let (before_at, before_id) = match &query.before {
                Some((at_ms, id)) => (Some(to_i64(*at_ms)?), id.clone()),
                None => (None, String::new()),
            };
            let mut statement = connection.prepare_cached(PAGE_USAGE)?;
            let records = statement
                .query_map(
                    params![
                        to_i64(query.from_ms)?,
                        to_i64(query.to_ms)?,
                        query.agent_id,
                        query.session_id,
                        before_at,
                        before_id,
                        i64::try_from(query.limit).unwrap_or(i64::MAX)
                    ],
                    |row| row.get::<_, String>(0),
                )?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            records
                .iter()
                .map(|record| serde_json::from_str(record).map_err(HistoryError::from))
                .collect()
        })
        .await
    }

    async fn existing_message_ids(&self, ids: &[String]) -> Result<HashSet<String>, HistoryError> {
        let ids = ids.to_vec();
        self.run(move |connection| {
            let mut found = HashSet::new();
            for chunk in ids.chunks(ID_CHUNK) {
                let sql = format!(
                    "SELECT id FROM messages WHERE id IN ({})",
                    placeholders(1, chunk.len())
                );
                let mut statement = connection.prepare(&sql)?;
                let rows = statement.query_map(params_from_iter(chunk.iter()), |row| {
                    row.get::<_, String>(0)
                })?;
                for row in rows {
                    found.insert(row?);
                }
            }
            Ok(found)
        })
        .await
    }

    async fn get_message(
        &self,
        agent_id: &str,
        session_id: &str,
        message_id: &str,
    ) -> Result<Option<HistoryMessage>, HistoryError> {
        let (agent_id, session_id, message_id) = (
            agent_id.to_string(),
            session_id.to_string(),
            message_id.to_string(),
        );
        self.run(move |connection| {
            let row = connection
                .query_row(
                    "SELECT agent_id, session_id, hidden, record FROM messages
                     WHERE id = ?1 AND agent_id = ?2 AND session_id = ?3",
                    params![message_id, agent_id, session_id],
                    raw_row,
                )
                .optional()?;
            Ok(decode(row.into_iter().collect())?.pop())
        })
        .await
    }

    async fn get_run(&self, run_id: &str) -> Result<Option<RunRecord>, HistoryError> {
        let run_id = run_id.to_string();
        self.run(move |connection| {
            let record = connection
                .query_row(
                    "SELECT record FROM runs WHERE id = ?1",
                    params![run_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            record
                .map(|record| serde_json::from_str(&record).map_err(HistoryError::from))
                .transpose()
        })
        .await
    }

    async fn page_messages(
        &self,
        query: &MessagePageQuery,
    ) -> Result<Vec<HistoryMessage>, HistoryError> {
        let query = query.clone();
        self.run(move |connection| {
            let before = query
                .before
                .as_ref()
                .map(|order| {
                    Ok::<_, HistoryError>((
                        to_i64(order.created_at_ms)?,
                        to_i64(order.ordinal)?,
                        order.id.clone(),
                    ))
                })
                .transpose()?;
            let mut statement = connection.prepare_cached(PAGE_MESSAGES)?;
            let rows = statement
                .query_map(
                    params![
                        query.agent_id,
                        query.session_id,
                        query.include_hidden,
                        before.as_ref().map(|before| before.0),
                        before.as_ref().map(|before| before.1),
                        before.as_ref().map(|before| before.2.as_str()),
                        i64::try_from(query.limit).unwrap_or(i64::MAX)
                    ],
                    raw_row,
                )?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            decode(rows)
        })
        .await
    }

    async fn page_messages_after(
        &self,
        query: &MessageAfterQuery,
    ) -> Result<Vec<HistoryMessage>, HistoryError> {
        validate_after_query(query)?;
        if query.limit == 0 {
            return Ok(Vec::new());
        }
        let query = query.clone();
        self.run(move |connection| {
            let mut statement = connection.prepare_cached(PAGE_MESSAGES_AFTER)?;
            let rows = statement
                .query_map(
                    params![
                        query.agent_id,
                        query.session_id,
                        query.include_hidden,
                        to_i64(query.after.created_at_ms)?,
                        to_i64(query.after.ordinal)?,
                        query.after.id,
                        query.limit as i64,
                    ],
                    raw_row,
                )?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            decode(rows)
        })
        .await
    }

    async fn visible_message_counts(
        &self,
        agent_id: &str,
        session_ids: &[String],
    ) -> Result<HashMap<String, usize>, HistoryError> {
        if session_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut values = vec![agent_id.to_string()];
        values.extend(session_ids.iter().cloned());
        self.run(move |connection| {
            let sql = format!(
                "SELECT session_id, COUNT(*) FROM messages
                 WHERE agent_id = ?1 AND hidden = 0 AND session_id IN ({})
                 GROUP BY session_id",
                placeholders(2, values.len() - 1)
            );
            let mut statement = connection.prepare(&sql)?;
            let rows = statement.query_map(params_from_iter(values.iter()), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?;
            let mut counts = HashMap::new();
            for row in rows {
                let (session_id, count) = row?;
                counts.insert(session_id, usize::try_from(count).unwrap_or(0));
            }
            Ok(counts)
        })
        .await
    }

    async fn search_messages(
        &self,
        agent_ids: &[String],
        query: &str,
        limit: usize,
    ) -> Result<Vec<HistoryMessage>, HistoryError> {
        let tokens = search_tokens(query);
        if tokens.is_empty() || agent_ids.is_empty() {
            return Ok(Vec::new());
        }
        let fts = tokens
            .iter()
            .map(|token| format!("\"{}\"*", token.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(" ");
        let mut values = vec![fts];
        values.extend(agent_ids.iter().cloned());
        self.run(move |connection| {
            let sql = format!(
                "SELECT messages.agent_id, messages.session_id, messages.hidden, messages.record
                 FROM messages_fts JOIN messages ON messages.rowid = messages_fts.rowid
                 WHERE messages_fts MATCH ?1 AND messages.hidden = 0 AND messages.agent_id IN ({})
                 ORDER BY messages.created_at_ms DESC, messages.ordinal DESC, messages.id DESC
                 LIMIT {limit}",
                placeholders(2, values.len() - 1)
            );
            let mut statement = connection.prepare(&sql)?;
            let rows = statement
                .query_map(params_from_iter(values.iter()), raw_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            decode(rows)
        })
        .await
    }

    async fn search_sessions(
        &self,
        agent_ids: &[String],
        query: &str,
        limit: usize,
    ) -> Result<Vec<HistoryMessage>, HistoryError> {
        let tokens = search_tokens(query);
        if tokens.is_empty() || agent_ids.is_empty() {
            return Ok(Vec::new());
        }
        let fts = tokens
            .iter()
            .map(|token| format!("\"{}\"*", token.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(" ");
        let mut values = vec![fts];
        values.extend(agent_ids.iter().cloned());
        self.run(move |connection| {
            // The newest matching row per (agent, session), then the newest
            // `limit` sessions (Controller ruling, M2 pre-flight audit): the
            // row-number partition groups before the session limit applies.
            let sql = format!(
                "SELECT agent_id, session_id, hidden, record FROM (
                     SELECT messages.agent_id AS agent_id, messages.session_id AS session_id,
                            messages.hidden AS hidden, messages.record AS record,
                            messages.created_at_ms AS created_at_ms, messages.ordinal AS ordinal,
                            messages.id AS id,
                            ROW_NUMBER() OVER (
                                PARTITION BY messages.agent_id, messages.session_id
                                ORDER BY messages.created_at_ms DESC, messages.ordinal DESC, messages.id DESC
                            ) AS rn
                     FROM messages_fts JOIN messages ON messages.rowid = messages_fts.rowid
                     WHERE messages_fts MATCH ?1 AND messages.hidden = 0 AND messages.agent_id IN ({})
                 ) ranked
                 WHERE rn = 1
                 ORDER BY created_at_ms DESC, ordinal DESC, id DESC
                 LIMIT {limit}",
                placeholders(2, values.len() - 1)
            );
            let mut statement = connection.prepare(&sql)?;
            let rows = statement
                .query_map(params_from_iter(values.iter()), raw_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            decode(rows)
        })
        .await
    }

    async fn upsert_schedule_runs(&self, fires: &[ScheduleFireRecord]) -> Result<(), HistoryError> {
        if fires.is_empty() {
            return Ok(());
        }
        let rows = fires
            .iter()
            .map(
                |fire| -> Result<(String, String, String, i64, String), HistoryError> {
                    Ok((
                        fire.id.clone(),
                        fire.schedule_id.clone(),
                        fire.agent_id.clone(),
                        to_i64(fire.fired_at_ms)?,
                        serde_json::to_string(fire)?,
                    ))
                },
            )
            .collect::<Result<Vec<_>, HistoryError>>()?;
        self.run(move |connection| {
            let transaction = connection.transaction()?;
            {
                let mut statement = transaction.prepare_cached(UPSERT_SCHEDULE_RUN)?;
                for (id, schedule_id, agent_id, fired_at_ms, record) in &rows {
                    statement.execute(params![id, schedule_id, agent_id, fired_at_ms, record])?;
                }
            }
            transaction.commit()?;
            Ok(())
        })
        .await
    }

    async fn page_schedule_runs(
        &self,
        agent_id: &str,
        schedule_id: &str,
        limit: usize,
    ) -> Result<Vec<ScheduleFireRecord>, HistoryError> {
        let (agent_id, schedule_id) = (agent_id.to_string(), schedule_id.to_string());
        self.run(move |connection| {
            let mut statement = connection.prepare_cached(PAGE_SCHEDULE_RUNS)?;
            let records = statement
                .query_map(
                    params![
                        agent_id,
                        schedule_id,
                        i64::try_from(limit).unwrap_or(i64::MAX)
                    ],
                    |row| row.get::<_, String>(0),
                )?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            records
                .iter()
                .map(|record| serde_json::from_str(record).map_err(HistoryError::from))
                .collect()
        })
        .await
    }

    async fn search_messages_filtered(
        &self,
        query: &FilteredMessageSearchQuery,
    ) -> Result<Vec<HistoryMessage>, HistoryError> {
        validate_filtered_message_query(query)?;
        let tokens = search_tokens(&query.query);
        if tokens.is_empty() || query.session_ids.is_empty() || query.limit == 0 {
            return Ok(Vec::new());
        }
        let query = query.clone();
        let fts = tokens
            .iter()
            .map(|token| format!("\"{}\"*", token.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(" ");
        self.run(move |connection| {
            use rusqlite::types::Value;
            let after = query.session_ids.len() + query.excluded_message_ids.len() + 3;
            let before = after + 1;
            let limit = before + 1;
            let mut values = vec![Value::Text(fts), Value::Text(query.agent_id)];
            values.extend(query.session_ids.iter().cloned().map(Value::Text));
            values.extend(query.excluded_message_ids.iter().cloned().map(Value::Text));
            values.push(query.after_ms.map(to_i64).transpose()?.map_or(Value::Null, Value::Integer));
            values.push(query.before_ms.map(to_i64).transpose()?.map_or(Value::Null, Value::Integer));
            values.push(Value::Integer(query.limit as i64));
            let exclude = if query.excluded_message_ids.is_empty() { String::new() } else {
                format!("AND messages.id NOT IN ({})", placeholders(query.session_ids.len() + 3, query.excluded_message_ids.len()))
            };
            let sql = format!(
                "SELECT messages.agent_id, messages.session_id, messages.hidden, messages.record
                 FROM messages_fts JOIN messages ON messages.rowid = messages_fts.rowid
                 WHERE messages_fts MATCH ?1 AND messages.agent_id = ?2
                   AND messages.session_id IN ({}) AND messages.hidden = 0 AND messages.role <> 'system'
                   {exclude}
                   AND (?{after} IS NULL OR messages.created_at_ms >= ?{after})
                   AND (?{before} IS NULL OR messages.created_at_ms < ?{before})
                 ORDER BY messages.created_at_ms DESC, messages.ordinal DESC, messages.id DESC LIMIT ?{limit}",
                placeholders(3, query.session_ids.len())
            );
            let mut statement = connection.prepare(&sql)?;
            let rows = statement.query_map(params_from_iter(values.iter()), raw_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
            decode(rows)
        }).await
    }

    async fn search_sessions_filtered(
        &self,
        query: &FilteredSessionSearchQuery,
    ) -> Result<Vec<HistoryMessage>, HistoryError> {
        validate_filtered_query(query)?;
        let tokens = search_tokens(&query.query);
        if tokens.is_empty() || query.session_ids.is_empty() || query.limit == 0 {
            return Ok(Vec::new());
        }
        let query = query.clone();
        let fts = tokens
            .iter()
            .map(|token| format!("\"{}\"*", token.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(" ");
        self.run(move |connection| {
            use rusqlite::types::Value;
            let after = query.session_ids.len() + query.excluded_message_ids.len() + 3;
            let before = after + 1;
            let limit = before + 1;
            let mut values = vec![Value::Text(fts), Value::Text(query.agent_id)];
            values.extend(query.session_ids.iter().cloned().map(Value::Text));
            values.extend(query.excluded_message_ids.iter().cloned().map(Value::Text));
            values.push(query.after_ms.map(to_i64).transpose()?.map_or(Value::Null, Value::Integer));
            values.push(query.before_ms.map(to_i64).transpose()?.map_or(Value::Null, Value::Integer));
            values.push(Value::Integer(query.limit as i64));
            let exclude = if query.excluded_message_ids.is_empty() { String::new() } else {
                format!("AND messages.id NOT IN ({})", placeholders(query.session_ids.len() + 3, query.excluded_message_ids.len()))
            };
            let sql = format!(
                "SELECT agent_id, session_id, hidden, record FROM (
                     SELECT messages.agent_id AS agent_id, messages.session_id AS session_id,
                            messages.hidden AS hidden, messages.record AS record,
                            messages.created_at_ms AS created_at_ms, messages.ordinal AS ordinal, messages.id AS id,
                            ROW_NUMBER() OVER (
                                PARTITION BY messages.agent_id, messages.session_id
                                ORDER BY messages.created_at_ms DESC, messages.ordinal DESC, messages.id DESC
                            ) AS rn
                     FROM messages_fts JOIN messages ON messages.rowid = messages_fts.rowid
                     WHERE messages_fts MATCH ?1 AND messages.agent_id = ?2
                       AND messages.session_id IN ({}) AND messages.hidden = 0 AND messages.role <> 'system'
                       {exclude}
                       AND (?{after} IS NULL OR messages.created_at_ms >= ?{after})
                       AND (?{before} IS NULL OR messages.created_at_ms < ?{before})
                 ) ranked WHERE rn = 1
                 ORDER BY created_at_ms DESC, ordinal DESC, id DESC LIMIT ?{limit}",
                placeholders(3, query.session_ids.len())
            );
            let mut statement = connection.prepare(&sql)?;
            let rows = statement.query_map(params_from_iter(values.iter()), raw_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            decode(rows)
        }).await
    }

    async fn delete_session(&self, agent_id: &str, session_id: &str) -> Result<(), HistoryError> {
        let (agent_id, session_id) = (agent_id.to_string(), session_id.to_string());
        self.run(move |connection| {
            let transaction = connection.transaction()?;
            for table in ["messages", "runs", "attachments", "approvals"] {
                transaction.execute(
                    &format!("DELETE FROM {table} WHERE agent_id = ?1 AND session_id = ?2"),
                    params![agent_id, session_id],
                )?;
            }
            transaction.commit()?;
            Ok(())
        })
        .await
    }

    async fn delete_agent(&self, agent_id: &str) -> Result<(), HistoryError> {
        let agent_id = agent_id.to_string();
        self.run(move |connection| {
            let transaction = connection.transaction()?;
            // Usage rows stay (spec §3.3); approvals and automation fires go with their agent.
            for table in [
                "messages",
                "runs",
                "attachments",
                "approvals",
                "schedule_runs",
            ] {
                transaction.execute(
                    &format!("DELETE FROM {table} WHERE agent_id = ?1"),
                    params![agent_id],
                )?;
            }
            transaction.commit()?;
            Ok(())
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::conformance::{
        assert_history_store_approval_conformance, assert_history_store_checkin_text_conformance,
        assert_history_store_conformance, assert_history_store_diacritics_conformance,
        assert_history_store_indexed_text_cap_conformance,
        assert_history_store_schedule_run_conformance,
        assert_history_store_session_search_conformance, assert_history_store_usage_conformance,
        history_message,
    };
    use anima_core::MessageRole;

    struct TempHistory(PathBuf);

    impl TempHistory {
        fn new(label: &str) -> Self {
            Self(
                std::env::temp_dir()
                    .join(format!("anima-history-{label}-{}", uuid::Uuid::new_v4()))
                    .join("history.sqlite"),
            )
        }
    }

    impl Drop for TempHistory {
        fn drop(&mut self) {
            if let Some(directory) = self.0.parent() {
                let _ = std::fs::remove_dir_all(directory);
            }
        }
    }

    #[tokio::test]
    async fn sqlite_store_meets_the_conformance_suite() {
        let temp = TempHistory::new("conformance");
        let store = SqliteHistoryStore::open(temp.0.clone())
            .await
            .expect("the store opens and creates its directory");
        assert_history_store_conformance(&store).await;
        crate::history::conformance::assert_history_store_attachment_conformance(&store).await;
        assert_history_store_session_search_conformance(&store).await;
        crate::history::conformance::assert_history_store_m41_conformance(&store).await;
        assert_history_store_checkin_text_conformance(&store).await;
        assert_history_store_indexed_text_cap_conformance(&store).await;
        assert_history_store_diacritics_conformance(&store).await;
        assert_history_store_approval_conformance(&store).await;
        assert_history_store_schedule_run_conformance(&store).await;
        assert_history_store_usage_conformance(&store).await;
        assert_eq!(store.label(), "sqlite");
        assert!(!store.is_ephemeral());
    }

    #[tokio::test]
    async fn rows_survive_reopening_and_the_schema_is_versioned_in_wal_mode() {
        let temp = TempHistory::new("reopen");
        {
            let store = SqliteHistoryStore::open(temp.0.clone()).await.unwrap();
            store
                .upsert_messages(&[history_message(
                    "msg-5-1",
                    "agent-1",
                    "chat:a",
                    MessageRole::User,
                    "persisted words",
                    5,
                )])
                .await
                .unwrap();
        }
        let reopened = SqliteHistoryStore::open(temp.0.clone()).await.unwrap();
        assert_eq!(reopened.path(), temp.0.as_path());
        assert_eq!(
            reopened
                .search_messages(&["agent-1".to_string()], "persisted", 10)
                .await
                .unwrap()
                .len(),
            1,
            "rows and the FTS index survive a reopen"
        );
        let matches = reopened
            .search_messages_filtered(&super::super::FilteredMessageSearchQuery {
                agent_id: "agent-1".into(),
                session_ids: vec!["chat:a".into()],
                excluded_message_ids: Vec::new(),
                query: "persisted".into(),
                after_ms: None,
                before_ms: None,
                limit: 21,
            })
            .await
            .unwrap();
        assert_eq!(
            matches.len(),
            1,
            "bounded scoped message search survives reopening"
        );
        assert_eq!(matches[0].message.id, "msg-5-1");
        let connection = Connection::open(&temp.0).unwrap();
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, SQLITE_HISTORY_SCHEMA_VERSION);
        let mode: String = connection
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
    }

    #[tokio::test]
    async fn deleting_an_agent_keeps_its_usage_rows() {
        let temp = TempHistory::new("delete-agent");
        let store = SqliteHistoryStore::open(temp.0.clone()).await.unwrap();
        store
            .upsert_messages(&[history_message(
                "msg-5-1",
                "agent-1",
                "chat:a",
                MessageRole::User,
                "goodbye",
                5,
            )])
            .await
            .unwrap();
        let connection = Connection::open(&temp.0).unwrap();
        connection
            .execute(
                "INSERT INTO usage (id, agent_id, session_id, run_id, created_at_ms, record)
                 VALUES ('usage-1', 'agent-1', 'chat:a', NULL, 5, '{}')",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO attachments (id, agent_id, session_id, created_at_ms, record)
                 VALUES ('attachment-1', 'agent-1', 'chat:a', 5, '{}')",
                [],
            )
            .unwrap();

        store.delete_agent("agent-1").await.unwrap();

        let rows = |table: &str| -> i64 {
            connection
                .query_row(
                    &format!("SELECT COUNT(*) FROM {table} WHERE agent_id = 'agent-1'"),
                    [],
                    |row| row.get(0),
                )
                .unwrap()
        };
        assert_eq!(rows("messages"), 0);
        assert_eq!(rows("attachments"), 0);
        assert_eq!(
            rows("usage"),
            1,
            "usage rows outlive a deletion (spec §3.3)"
        );
    }

    #[tokio::test]
    async fn a_newer_schema_is_refused() {
        let temp = TempHistory::new("newer");
        std::fs::create_dir_all(temp.0.parent().unwrap()).unwrap();
        Connection::open(&temp.0)
            .unwrap()
            .pragma_update(None, "user_version", 99)
            .unwrap();
        let error = SqliteHistoryStore::open(temp.0.clone())
            .await
            .err()
            .expect("a newer schema must not be opened");
        assert_eq!(
            error.message(),
            "history store schema version 99 is newer than this daemon supports (1)"
        );
    }
    #[tokio::test]
    async fn attachment_records_reopen_in_existing_v1_schema() {
        let temp = TempHistory::new("attachment-reopen");
        let row = crate::history::conformance::attachment_record("id", "agent", "chat:a");
        {
            let store = SqliteHistoryStore::open(temp.0.clone()).await.unwrap();
            store
                .upsert_attachments(std::slice::from_ref(&row))
                .await
                .unwrap();
        }
        let store = SqliteHistoryStore::open(temp.0.clone()).await.unwrap();
        assert_eq!(
            store.get_attachment("agent", "id").await.unwrap(),
            Some(row)
        );
        let connection = Connection::open(&temp.0).unwrap();
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 1);
    }
}

#[cfg(test)]
mod profile_ownership_tests {
    use super::*;
    use crate::profile_owner::ProfileOwner;

    #[cfg(unix)]
    #[tokio::test]
    async fn profile_owner_renamed_sqlite_keeps_identity_custody_until_detached_job_retires() {
        use crate::profile_owner::ProfileResource;
        use std::process::{Command, Stdio};
        let fixture = tempfile::tempdir().unwrap();
        let original = fixture.path().join("history.sqlite");
        let renamed = fixture.path().join("relocated.sqlite");
        let owner =
            ProfileOwner::acquire_resources([ProfileResource::Sqlite(original.clone())]).unwrap();
        let store = Arc::new(
            SqliteHistoryStore::open_with_owner(original.clone(), Some(owner))
                .await
                .unwrap(),
        );
        let (entered, started) = tokio::sync::oneshot::channel();
        let (release, wait) = std::sync::mpsc::channel();
        let working = Arc::clone(&store);
        let task = tokio::spawn(async move {
            working
                .run(move |_| {
                    entered.send(()).unwrap();
                    let _ = wait.recv();
                    Ok(())
                })
                .await
        });
        started.await.unwrap();
        std::fs::rename(&original, &renamed).unwrap();
        task.abort();
        let _ = task.await;
        drop(store);
        let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "profile_owner::tests::ownership_child",
                "--nocapture",
            ])
            .env("ANIMA_TEST_OWNERSHIP_RESOURCE", &renamed)
            .env(
                "ANIMA_TEST_PROFILE_OWNER_REGISTRY_DIR",
                crate::profile_owner::test_registry_path().unwrap(),
            )
            .env("ANIMA_TEST_OWNERSHIP_REFUSED", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .status()
            .unwrap();
        assert!(status.success(), "a renamed database acquired a second owner while its detached blocking job retained the connection");
        release.send(()).unwrap();
        wait_until_released(&renamed).await;
    }

    async fn wait_until_released(path: &Path) {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            if ProfileOwner::acquire([path.to_path_buf()]).is_ok() {
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "finished SQLite work leaked its profile lease"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    #[test]
    fn profile_owner_cancelled_history_initialization_retains_queued_blocking_lease() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .max_blocking_threads(1)
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("history.sqlite");
            let owner =
                ProfileOwner::acquire_resources([crate::profile_owner::ProfileResource::Sqlite(
                    path.clone(),
                )])
                .unwrap();
            let (entered, started) = tokio::sync::oneshot::channel();
            let (release, wait) = std::sync::mpsc::channel();
            let busy = tokio::task::spawn_blocking(move || {
                entered.send(()).unwrap();
                let _ = wait.recv();
            });
            started.await.unwrap();
            let mut opening = Box::pin(SqliteHistoryStore::open_with_owner(
                path.clone(),
                Some(owner),
            ));
            assert!(futures::poll!(opening.as_mut()).is_pending());
            drop(opening);
            assert!(
                ProfileOwner::acquire([path.clone()]).is_err(),
                "cancelled startup released the guard still captured by SQLite initialization"
            );
            release.send(()).unwrap();
            busy.await.unwrap();
            wait_until_released(&path).await;
        });
    }

    #[tokio::test]
    async fn profile_owner_detached_history_write_keeps_lease_after_waiter_is_aborted() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("history.sqlite");
        let owner =
            ProfileOwner::acquire_resources([crate::profile_owner::ProfileResource::Sqlite(
                path.clone(),
            )])
            .unwrap();
        let store = Arc::new(
            SqliteHistoryStore::open_with_owner(path.clone(), Some(owner))
                .await
                .unwrap(),
        );
        let (entered, started) = tokio::sync::oneshot::channel();
        let (release, wait) = std::sync::mpsc::channel();
        let working = Arc::clone(&store);
        let task = tokio::spawn(async move {
            working
                .run(move |_| {
                    entered.send(()).unwrap();
                    let _ = wait.recv();
                    Ok(())
                })
                .await
        });
        started.await.unwrap();
        task.abort();
        let _ = task.await;
        drop(store);
        assert!(
            ProfileOwner::acquire([path.clone()]).is_err(),
            "aborted history waiter released an active blocking job's guard"
        );
        release.send(()).unwrap();
        wait_until_released(&path).await;
    }
}
