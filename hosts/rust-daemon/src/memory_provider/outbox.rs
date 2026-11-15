//! Transactional selected-publication journal, independent of the native memory snapshot.
use super::{binding::MemoryProviderBinding, mapping::PublicationMapping};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{path::Path, sync::Mutex};

// Reserve room before admission for bounded IDs, errors, counters and the largest receipt.
const MUTABLE_METADATA_HEADROOM: usize = 8 * 1024;
const MAX_RECEIPT_BYTES: usize = 64 * 1024;

/// One explicit selection, including the original source and frozen remote request identity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Admission {
    pub(crate) publication_id: String,
    pub(crate) operation_id: String,
    pub(crate) agent_id: String,
    pub(crate) binding: MemoryProviderBinding,
    pub(crate) connection_version: u64,
    pub(crate) descriptor_digest: String,
    pub(crate) local_id: String,
    pub(crate) source_digest: Option<String>,
    pub(crate) source_revision: Option<String>,
    #[serde(default)]
    pub(crate) caller_digest: Option<String>,
    #[serde(default)]
    pub(crate) source_attribution: Option<String>,
    pub(crate) candidate: Value,
    pub(crate) remote_tool: String,
    pub(crate) request: Value,
    pub(crate) target_key: String,
}

/// Durable outcomes distinguish an accepted proposal from an approved canonical record.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PublicationState {
    Queued,
    Sending,
    Unknown,
    SubmittedForReview,
    Saved,
    Conflict,
    Blocked,
    Dismissed,
    Cancelled,
}

/// The immutable selection plus CAS-controlled dispatch and reconciliation evidence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Publication {
    pub(crate) admission: Admission,
    pub(crate) payload_hash: String,
    pub(crate) state: PublicationState,
    pub(crate) resume_state: Option<PublicationState>,
    pub(crate) revision: u64,
    pub(crate) attempts: u32,
    pub(crate) next_attempt_at_ms: i64,
    pub(crate) created_at_ms: i64,
    pub(crate) updated_at_ms: i64,
    pub(crate) last_error: Option<String>,
    pub(crate) proposal_id: Option<String>,
    pub(crate) canonical_id: Option<String>,
    pub(crate) canonical_revision: Option<u64>,
    pub(crate) receipt: Option<Value>,
}

/// Caller-validated evidence; this store performs no remote calls or receipt authentication.
#[derive(Clone, Debug)]
pub(crate) enum Transition {
    Unknown {
        error: String,
        next_attempt_at_ms: i64,
    },
    /// A definite not-sent failure; never use this for a timeout or lost response.
    Retry {
        error: String,
        next_attempt_at_ms: i64,
    },
    /// A receipt lookup definitively established absence in the same pinned generation.
    ReconciledAbsent {
        next_attempt_at_ms: i64,
    },
    Submitted {
        proposal_id: String,
        receipt: Value,
        next_attempt_at_ms: i64,
    },
    Saved {
        canonical_id: String,
        revision: u64,
        receipt: Value,
    },
    Approved {
        proposal_id: String,
        canonical_id: String,
        revision: u64,
        receipt: Value,
    },
    /// Without a known proposal, the caller must have validated a definitive remote
    /// rejection with no effect (such as CONFLICT or INVALID_INPUT). That operation
    /// is terminal; timeouts and ambiguous failures remain Unknown.
    Conflict {
        error: String,
        receipt: Value,
    },
    Blocked {
        error: String,
    },
    Resume,
    Dismissed {
        receipt: Value,
    },
    Cancelled,
}

/// Hard retention limits reject new writes without discarding publication history.
#[derive(Clone, Copy, Debug)]
pub(crate) struct OutboxLimits {
    pub(crate) max_records: u64,
    pub(crate) max_record_bytes: usize,
    /// Includes the main database and a bounded rollback journal allowance.
    pub(crate) max_disk_bytes: u64,
}
impl Default for OutboxLimits {
    fn default() -> Self {
        Self {
            max_records: 10_000,
            max_record_bytes: 256 * 1024,
            max_disk_bytes: 128 * 1024 * 1024,
        }
    }
}

/// Storage and state-machine failures expose no selected memory text through Display.
#[derive(Debug)]
pub(crate) enum OutboxError {
    Storage(rusqlite::Error),
    Encoding(serde_json::Error),
    Invalid(&'static str),
    Limit,
    IdempotencyConflict,
    StaleRevision,
    InvalidTransition,
    TargetBusy,
    NotFound,
    Poisoned,
}
impl std::fmt::Display for OutboxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Storage(_) => "publication storage failed",
            Self::Encoding(_) => "invalid publication encoding",
            Self::Invalid(message) => message,
            Self::Limit => "publication storage limit exceeded",
            Self::IdempotencyConflict => {
                "publication identity or selected source conflicts with a prior publication"
            }
            Self::StaleRevision => "publication revision changed",
            Self::InvalidTransition => "invalid publication transition",
            Self::TargetBusy => "an earlier publication holds this target",
            Self::NotFound => "publication not found",
            Self::Poisoned => "publication storage lock poisoned",
        })
    }
}
impl std::error::Error for OutboxError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Storage(error) => Some(error),
            Self::Encoding(error) => Some(error),
            _ => None,
        }
    }
}
impl From<rusqlite::Error> for OutboxError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Storage(error)
    }
}
impl From<serde_json::Error> for OutboxError {
    fn from(error: serde_json::Error) -> Self {
        Self::Encoding(error)
    }
}

/// A synchronous SQLite writer; async callers should use their blocking-I/O boundary.
#[derive(Debug)]
pub(crate) struct PublicationStore {
    connection: Mutex<Connection>,
    limits: OutboxLimits,
}
impl PublicationStore {
    /// Opens only the selected-publication database; the caller owns the enclosing profile lease.
    /// Recovery is explicit and must run once at startup, before starting dispatch handlers.
    pub(crate) fn open(path: &Path, limits: OutboxLimits) -> Result<Self, OutboxError> {
        if limits.max_records == 0
            || limits.max_record_bytes < 1024
            || limits.max_disk_bytes < 256 * 1024
        {
            return Err(OutboxError::Limit);
        }
        let mut connection = Connection::open(path)?;
        connection.busy_timeout(std::time::Duration::from_secs(2))?;
        // DELETE journals cannot accumulate an uncheckpointed WAL. Reserve twice the
        // database ceiling for rollback pages, per-page journal framing and headers.
        connection.pragma_update(None, "journal_mode", "DELETE")?;
        connection.pragma_update(None, "synchronous", "FULL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        let page_size: u64 = connection.query_row("PRAGMA page_size", [], |r| r.get(0))?;
        let max_pages = limits.max_disk_bytes / 3 / page_size;
        let pages: u64 = connection.query_row("PRAGMA page_count", [], |r| r.get(0))?;
        if pages > max_pages {
            return Err(OutboxError::Limit);
        }
        connection.pragma_update(None, "max_page_count", max_pages)?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let version: u32 = tx.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version > 1 {
            return Err(OutboxError::Invalid(
                "unsupported publication store version",
            ));
        }
        tx.execute_batch("CREATE TABLE IF NOT EXISTS publications (
            sequence INTEGER PRIMARY KEY AUTOINCREMENT,
            publication_id TEXT NOT NULL UNIQUE,
            operation_id TEXT NOT NULL,
            workspace_id TEXT NOT NULL,
            generation TEXT NOT NULL,
            agent_id TEXT NOT NULL,
            target_key TEXT NOT NULL,
            state TEXT NOT NULL,
            next_attempt_at_ms INTEGER NOT NULL,
            revision INTEGER NOT NULL,
            body TEXT NOT NULL,
            UNIQUE(workspace_id, generation, operation_id)
        );
        CREATE INDEX IF NOT EXISTS publications_target ON publications(workspace_id, generation, agent_id, target_key, sequence);
        CREATE INDEX IF NOT EXISTS publications_due ON publications(state, next_attempt_at_ms, sequence);
        CREATE TABLE IF NOT EXISTS publication_mappings (
            workspace_id TEXT NOT NULL, generation TEXT NOT NULL, agent_id TEXT NOT NULL,
            local_id TEXT NOT NULL, publication_id TEXT NOT NULL UNIQUE REFERENCES publications(publication_id),
            body TEXT NOT NULL,
            PRIMARY KEY(workspace_id, generation, agent_id, local_id, publication_id)
        ); PRAGMA user_version = 1;")?;
        tx.commit()?;
        Ok(Self {
            connection: Mutex::new(connection),
            limits,
        })
    }

    /// Atomically persists the selected candidate and exact immutable wire request before dispatch.
    pub(crate) fn admit(&self, input: Admission, now_ms: i64) -> Result<Publication, OutboxError> {
        input.validate()?;
        let bytes = serde_json::to_vec(&input)?;
        if bytes.len() > self.limits.max_record_bytes {
            return Err(OutboxError::Limit);
        }
        let payload_hash = hex::encode(Sha256::digest(&bytes));
        let mut conn = self.connection.lock().map_err(|_| OutboxError::Poisoned)?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(existing) = read_publication(&tx, &input.publication_id, self.limits)? {
            if existing.payload_hash != payload_hash || existing.admission != input {
                return Err(OutboxError::IdempotencyConflict);
            }
            return Ok(existing);
        }
        let operation_exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM publications WHERE workspace_id=? AND generation=? AND operation_id=?)",
            params![input.binding.workspace_id, input.binding.generation, input.operation_id], |r| r.get(0))?;
        if operation_exists {
            return Err(OutboxError::IdempotencyConflict);
        }
        if let Some(source_digest) = &input.source_digest {
            if source_mapping_exists(
                &tx,
                &input.binding.workspace_id,
                &input.binding.generation,
                &input.agent_id,
                &input.local_id,
                source_digest,
                input.request.get("id").and_then(Value::as_str),
            )? {
                return Err(OutboxError::IdempotencyConflict);
            }
        }
        let count: u64 = tx.query_row("SELECT COUNT(*) FROM publications", [], |r| r.get(0))?;
        if count >= self.limits.max_records {
            return Err(OutboxError::Limit);
        }
        let publication = Publication {
            admission: input,
            payload_hash,
            state: PublicationState::Queued,
            resume_state: None,
            revision: 1,
            attempts: 0,
            next_attempt_at_ms: now_ms,
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
            last_error: None,
            proposal_id: None,
            canonical_id: None,
            canonical_revision: None,
            receipt: None,
        };
        let body = bounded_json(&publication, self.limits)?;
        let admission_limit = self
            .limits
            .max_record_bytes
            .checked_sub(MUTABLE_METADATA_HEADROOM + receipt_limit(self.limits))
            .ok_or(OutboxError::Limit)?;
        if body.len() > admission_limit {
            return Err(OutboxError::Limit);
        }
        let a = &publication.admission;
        tx.execute("INSERT INTO publications(publication_id,operation_id,workspace_id,generation,agent_id,target_key,state,next_attempt_at_ms,revision,body) VALUES(?,?,?,?,?,?,?,?,?,?)",
            params![a.publication_id,a.operation_id,a.binding.workspace_id,a.binding.generation,a.agent_id,a.target_key,publication.state.as_str(),now_ms,publication.revision,body])?;
        reserve_outcome_capacity(&tx, self.limits)?;
        tx.commit()?;
        Ok(publication)
    }

    pub(crate) fn get(&self, id: &str) -> Result<Option<Publication>, OutboxError> {
        let conn = self.connection.lock().map_err(|_| OutboxError::Poisoned)?;
        read_publication(&conn, id, self.limits)
    }

    /// Bounded status history, newest selections first, including terminal evidence.
    pub(crate) fn list(
        &self,
        agent_id: &str,
        limit: usize,
    ) -> Result<Vec<Publication>, OutboxError> {
        let conn = self.connection.lock().map_err(|_| OutboxError::Poisoned)?;
        let mut statement = conn.prepare(
            "SELECT body FROM publications WHERE agent_id=? ORDER BY sequence DESC LIMIT ?",
        )?;
        let rows = statement.query_map(params![agent_id, bounded_limit(limit)], |r| {
            r.get::<_, String>(0)
        })?;
        rows.map(|row| decode(&row?, self.limits)).collect()
    }

    /// Counts retained status rows without loading any selected text or receipt payloads.
    pub(crate) fn counts(
        &self,
        agent_id: &str,
    ) -> Result<std::collections::BTreeMap<String, u64>, OutboxError> {
        let conn = self.connection.lock().map_err(|_| OutboxError::Poisoned)?;
        let mut statement = conn
            .prepare("SELECT state,COUNT(*) FROM publications WHERE agent_id=? GROUP BY state")?;
        let rows = statement.query_map([agent_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect::<Result<_, _>>().map_err(OutboxError::from)
    }

    /// Only the earliest unfinished publication of each target is dispatchable.
    pub(crate) fn due(&self, now_ms: i64, limit: usize) -> Result<Vec<Publication>, OutboxError> {
        let conn = self.connection.lock().map_err(|_| OutboxError::Poisoned)?;
        let mut statement = conn.prepare("SELECT p.body FROM publications p
            WHERE p.state IN ('queued','unknown','submitted_for_review') AND p.next_attempt_at_ms<=?
            AND NOT EXISTS (SELECT 1 FROM publications older WHERE older.workspace_id=p.workspace_id
                AND older.generation=p.generation AND older.agent_id=p.agent_id AND older.target_key=p.target_key
                AND older.sequence<p.sequence AND older.state NOT IN ('saved','dismissed','cancelled')
                AND NOT (older.state='conflict' AND json_extract(older.body,'$.proposal_id') IS NULL))
            ORDER BY p.sequence LIMIT ?")?;
        let rows = statement.query_map(params![now_ms, bounded_limit(limit)], |r| {
            r.get::<_, String>(0)
        })?;
        rows.map(|row| decode(&row?, self.limits)).collect()
    }

    /// Claims a target under CAS. Only a Queued resume phase permits a remote write;
    /// Unknown and SubmittedForReview claims permit receipt/proposal reads only.
    pub(crate) fn claim(
        &self,
        id: &str,
        expected_revision: u64,
        now_ms: i64,
    ) -> Result<Publication, OutboxError> {
        let mut conn = self.connection.lock().map_err(|_| OutboxError::Poisoned)?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut publication = current(&tx, id, expected_revision, self.limits)?;
        if !matches!(
            publication.state,
            PublicationState::Queued
                | PublicationState::Unknown
                | PublicationState::SubmittedForReview
        ) || publication.next_attempt_at_ms > now_ms
        {
            return Err(OutboxError::InvalidTransition);
        }
        let busy: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM publications older JOIN publications p ON p.publication_id=?
            WHERE older.workspace_id=p.workspace_id AND older.generation=p.generation AND older.agent_id=p.agent_id
            AND older.target_key=p.target_key AND older.sequence<p.sequence AND older.state NOT IN ('saved','dismissed','cancelled')
            AND NOT (older.state='conflict' AND json_extract(older.body,'$.proposal_id') IS NULL))",
            [id], |r| r.get(0))?;
        if busy {
            return Err(OutboxError::TargetBusy);
        }
        publication.resume_state = Some(publication.state);
        publication.state = PublicationState::Sending;
        publication.attempts = publication
            .attempts
            .checked_add(1)
            .ok_or(OutboxError::Limit)?;
        persist(
            &tx,
            &mut publication,
            expected_revision,
            now_ms,
            self.limits,
        )?;
        tx.commit()?;
        Ok(publication)
    }

    /// Applies a validated outcome with CAS; canonical mappings and state commit together.
    /// The caller must validate remote receipts against the pinned request before calling.
    pub(crate) fn transition(
        &self,
        id: &str,
        expected_revision: u64,
        transition: Transition,
        now_ms: i64,
    ) -> Result<Publication, OutboxError> {
        let mut conn = self.connection.lock().map_err(|_| OutboxError::Poisoned)?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut publication = current(&tx, id, expected_revision, self.limits)?;
        apply_transition(&mut publication, transition)?;
        if publication
            .receipt
            .as_ref()
            .map(serde_json::to_vec)
            .transpose()?
            .is_some_and(|bytes| bytes.len() > receipt_limit(self.limits))
        {
            return Err(OutboxError::Limit);
        }
        persist(
            &tx,
            &mut publication,
            expected_revision,
            now_ms,
            self.limits,
        )?;
        if publication.state == PublicationState::Saved {
            let a = &publication.admission;
            let mapping = PublicationMapping {
                workspace_id: a.binding.workspace_id.clone(),
                generation: a.binding.generation.clone(),
                agent_id: a.agent_id.clone(),
                local_id: a.local_id.clone(),
                publication_id: a.publication_id.clone(),
                source_digest: a.source_digest.clone(),
                source_revision: a.source_revision.clone(),
                proposal_id: publication.proposal_id.clone(),
                canonical_id: publication
                    .canonical_id
                    .clone()
                    .ok_or(OutboxError::InvalidTransition)?,
                canonical_revision: publication
                    .canonical_revision
                    .ok_or(OutboxError::InvalidTransition)?,
                receipt: publication
                    .receipt
                    .clone()
                    .ok_or(OutboxError::InvalidTransition)?,
            };
            tx.execute("INSERT INTO publication_mappings(workspace_id,generation,agent_id,local_id,publication_id,body) VALUES(?,?,?,?,?,?)",
                params![mapping.workspace_id,mapping.generation,mapping.agent_id,mapping.local_id,mapping.publication_id,bounded_json(&mapping,self.limits)?])?;
        }
        tx.commit()?;
        Ok(publication)
    }

    /// Startup-only recovery preserves every request and marks interrupted sends uncertain.
    pub(crate) fn recover_interrupted(&self, now_ms: i64) -> Result<usize, OutboxError> {
        let mut conn = self.connection.lock().map_err(|_| OutboxError::Poisoned)?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let interrupted: Vec<Publication> = {
            let mut statement = tx
                .prepare("SELECT body FROM publications WHERE state='sending' ORDER BY sequence")?;
            let rows = statement.query_map([], |r| r.get::<_, String>(0))?;
            rows.map(|row| decode(&row?, self.limits))
                .collect::<Result<_, OutboxError>>()?
        };
        let count = interrupted.len();
        for mut publication in interrupted {
            let revision = publication.revision;
            publication.state =
                if publication.resume_state == Some(PublicationState::SubmittedForReview) {
                    PublicationState::SubmittedForReview
                } else {
                    PublicationState::Unknown
                };
            publication.resume_state = None;
            publication.last_error = Some("interrupted dispatch requires reconciliation".into());
            persist(&tx, &mut publication, revision, now_ms, self.limits)?;
        }
        tx.commit()?;
        Ok(count)
    }

    pub(crate) fn mapping(
        &self,
        publication_id: &str,
    ) -> Result<Option<PublicationMapping>, OutboxError> {
        let conn = self.connection.lock().map_err(|_| OutboxError::Poisoned)?;
        let body: Option<String> = conn
            .query_row(
                "SELECT body FROM publication_mappings WHERE publication_id=?",
                [publication_id],
                |r| r.get(0),
            )
            .optional()?;
        body.map(|body| decode(&body, self.limits)).transpose()
    }

    /// Checks the entire saved source history, independently of bounded history projections.
    pub(crate) fn has_source_mapping(
        &self,
        workspace_id: &str,
        generation: &str,
        agent_id: &str,
        local_id: &str,
        source_digest: &str,
        canonical_id: Option<&str>,
    ) -> Result<bool, OutboxError> {
        let conn = self.connection.lock().map_err(|_| OutboxError::Poisoned)?;
        source_mapping_exists(
            &conn,
            workspace_id,
            generation,
            agent_id,
            local_id,
            source_digest,
            canonical_id,
        )
    }

    pub(crate) fn mappings(
        &self,
        agent_id: &str,
        local_id: &str,
        limit: usize,
    ) -> Result<Vec<PublicationMapping>, OutboxError> {
        let conn = self.connection.lock().map_err(|_| OutboxError::Poisoned)?;
        let mut statement = conn.prepare("SELECT m.body FROM publication_mappings m JOIN publications p ON p.publication_id=m.publication_id WHERE m.agent_id=? AND m.local_id=? ORDER BY p.sequence DESC LIMIT ?")?;
        let rows = statement.query_map(params![agent_id, local_id, bounded_limit(limit)], |r| {
            r.get::<_, String>(0)
        })?;
        rows.map(|row| decode(&row?, self.limits)).collect()
    }
}

impl Admission {
    fn validate(&self) -> Result<(), OutboxError> {
        self.binding
            .validate(&self.agent_id)
            .map_err(OutboxError::Invalid)?;
        for id in [&self.publication_id, &self.operation_id] {
            if uuid::Uuid::parse_str(id)
                .ok()
                .is_none_or(|uuid| uuid.to_string() != *id)
            {
                return Err(OutboxError::Invalid(
                    "publication and operation IDs must be canonical UUIDs",
                ));
            }
        }
        for value in [&self.local_id, &self.target_key, &self.descriptor_digest] {
            validate_identifier(value)?;
        }
        for value in [
            &self.source_digest,
            &self.source_revision,
            &self.caller_digest,
        ]
        .into_iter()
        .flatten()
        {
            validate_identifier(value)?;
        }
        if !matches!(
            self.remote_tool.as_str(),
            "mother_propose_memory" | "mother_write_record"
        ) || !self.candidate.is_object()
            || !self.request.is_object()
            || self.request["operationId"] != self.operation_id
            || self.request["workspaceId"] != self.binding.workspace_id
            || self.request["generation"] != self.binding.generation
        {
            return Err(OutboxError::Invalid(
                "publication request does not match its pinned identity",
            ));
        }
        Ok(())
    }
}

impl PublicationState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Sending => "sending",
            Self::Unknown => "unknown",
            Self::SubmittedForReview => "submitted_for_review",
            Self::Saved => "saved",
            Self::Conflict => "conflict",
            Self::Blocked => "blocked",
            Self::Dismissed => "dismissed",
            Self::Cancelled => "cancelled",
        }
    }
    fn terminal(self) -> bool {
        matches!(self, Self::Saved | Self::Dismissed | Self::Cancelled)
    }
}

fn validate_identifier(value: &str) -> Result<(), OutboxError> {
    if value.trim().is_empty() || value.len() > 512 || value.chars().any(char::is_control) {
        Err(OutboxError::Invalid(
            "invalid bounded publication identifier",
        ))
    } else {
        Ok(())
    }
}
fn bounded_limit(limit: usize) -> i64 {
    limit.min(256) as i64
}
fn receipt_limit(limits: OutboxLimits) -> usize {
    (limits.max_record_bytes / 2).min(MAX_RECEIPT_BYTES)
}

fn source_mapping_exists(
    conn: &Connection,
    workspace_id: &str,
    generation: &str,
    agent_id: &str,
    local_id: &str,
    source_digest: &str,
    canonical_id: Option<&str>,
) -> Result<bool, OutboxError> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM publication_mappings
            WHERE workspace_id=?1 AND generation=?2 AND agent_id=?3 AND local_id=?4
                AND json_extract(body,'$.source_digest')=?5
                AND (?6 IS NULL OR json_extract(body,'$.canonical_id')=?6))",
        params![
            workspace_id,
            generation,
            agent_id,
            local_id,
            source_digest,
            canonical_id
        ],
        |r| r.get(0),
    )?)
}

/// Admission must leave database pages available for already accepted remote outcomes.
/// Keep the full liability even after partial growth; only definite terminal outcomes
/// release it. This is intentionally conservative and never removes retained evidence.
fn reserve_outcome_capacity(conn: &Connection, limits: OutboxLimits) -> Result<(), OutboxError> {
    let pages: u64 = conn.query_row("PRAGMA page_count", [], |r| r.get(0))?;
    let page_size: u64 = conn.query_row("PRAGMA page_size", [], |r| r.get(0))?;
    let max_pages: u64 = conn.query_row("PRAGMA max_page_count", [], |r| r.get(0))?;
    let outstanding: u64 = conn.query_row(
        "SELECT COUNT(*) FROM publications WHERE state NOT IN ('saved','dismissed','cancelled')
            AND NOT (state='conflict' AND json_extract(body,'$.proposal_id') IS NULL)",
        [],
        |r| r.get(0),
    )?;
    // A row can grow to its full cap and add a mapping containing a full receipt.
    // Extra pages cover B-tree/index splits, overflow framing and mapping metadata.
    let growth = limits.max_record_bytes as u128
        + receipt_limit(limits) as u128
        + 16 * u128::from(page_size)
        + MUTABLE_METADATA_HEADROOM as u128;
    let reserved = u128::from(pages) * u128::from(page_size) + u128::from(outstanding) * growth;
    if reserved > u128::from(max_pages) * u128::from(page_size) {
        return Err(OutboxError::Limit);
    }
    Ok(())
}
fn bounded_json(value: &impl Serialize, limits: OutboxLimits) -> Result<String, OutboxError> {
    let body = serde_json::to_string(value)?;
    if body.len() > limits.max_record_bytes {
        return Err(OutboxError::Limit);
    }
    Ok(body)
}
fn decode<T: serde::de::DeserializeOwned>(
    body: &str,
    limits: OutboxLimits,
) -> Result<T, OutboxError> {
    if body.len() > limits.max_record_bytes {
        return Err(OutboxError::Limit);
    }
    Ok(serde_json::from_str(body)?)
}
fn read_publication(
    conn: &Connection,
    id: &str,
    limits: OutboxLimits,
) -> Result<Option<Publication>, OutboxError> {
    let body: Option<String> = conn
        .query_row(
            "SELECT body FROM publications WHERE publication_id=?",
            [id],
            |r| r.get(0),
        )
        .optional()?;
    body.map(|body| decode(&body, limits)).transpose()
}
fn current(
    conn: &Connection,
    id: &str,
    revision: u64,
    limits: OutboxLimits,
) -> Result<Publication, OutboxError> {
    let p = read_publication(conn, id, limits)?.ok_or(OutboxError::NotFound)?;
    if p.revision != revision {
        return Err(OutboxError::StaleRevision);
    }
    Ok(p)
}
fn persist(
    conn: &Connection,
    p: &mut Publication,
    expected_revision: u64,
    now_ms: i64,
    limits: OutboxLimits,
) -> Result<(), OutboxError> {
    p.revision = p
        .revision
        .checked_add(1)
        .filter(|r| *r <= i64::MAX as u64)
        .ok_or(OutboxError::Limit)?;
    p.updated_at_ms = now_ms;
    let changed=conn.execute("UPDATE publications SET state=?,next_attempt_at_ms=?,revision=?,body=? WHERE publication_id=? AND revision=?",
        params![p.state.as_str(),p.next_attempt_at_ms,p.revision,bounded_json(p,limits)?,p.admission.publication_id,expected_revision])?;
    if changed != 1 {
        return Err(OutboxError::StaleRevision);
    }
    Ok(())
}

fn apply_transition(p: &mut Publication, transition: Transition) -> Result<(), OutboxError> {
    use PublicationState::*;
    if p.state.terminal() || (p.state == Conflict && p.proposal_id.is_none()) {
        return Err(OutboxError::InvalidTransition);
    }
    let phase = if matches!(p.state, Sending | Blocked) {
        p.resume_state.unwrap_or(Unknown)
    } else {
        p.state
    };
    match transition {
        Transition::Unknown {
            error,
            next_attempt_at_ms,
        } if matches!(p.state, Sending | Unknown) => {
            p.state = if phase == SubmittedForReview {
                SubmittedForReview
            } else {
                Unknown
            };
            p.resume_state = None;
            p.last_error = Some(error);
            p.next_attempt_at_ms = next_attempt_at_ms;
        }
        Transition::Retry {
            error,
            next_attempt_at_ms,
        } if p.state == Sending && phase == Queued => {
            p.state = Queued;
            p.resume_state = None;
            p.last_error = Some(error);
            p.next_attempt_at_ms = next_attempt_at_ms;
        }
        Transition::ReconciledAbsent { next_attempt_at_ms }
            if phase == Unknown
                && matches!(p.state, Unknown | Sending)
                && p.proposal_id.is_none() =>
        {
            p.state = Queued;
            p.resume_state = None;
            p.last_error = None;
            p.next_attempt_at_ms = next_attempt_at_ms;
        }
        Transition::Submitted {
            proposal_id,
            receipt,
            next_attempt_at_ms,
        } if p.admission.remote_tool == "mother_propose_memory"
            && matches!(p.state, Sending | Unknown | SubmittedForReview) =>
        {
            validate_identifier(&proposal_id)?;
            if p.proposal_id.as_ref().is_some_and(|id| *id != proposal_id) {
                return Err(OutboxError::InvalidTransition);
            }
            p.state = SubmittedForReview;
            p.resume_state = None;
            p.proposal_id = Some(proposal_id);
            p.receipt = Some(receipt);
            p.next_attempt_at_ms = next_attempt_at_ms;
            p.last_error = None;
        }
        Transition::Saved {
            canonical_id,
            revision,
            receipt,
        } if p.admission.remote_tool == "mother_write_record"
            && matches!(p.state, Sending | Unknown) =>
        {
            save_result(p, canonical_id, revision, receipt)?;
        }
        Transition::Approved {
            proposal_id,
            canonical_id,
            revision,
            receipt,
        } if p.admission.remote_tool == "mother_propose_memory"
            && matches!(p.state, SubmittedForReview | Sending | Unknown)
            && p.proposal_id.as_ref() == Some(&proposal_id) =>
        {
            save_result(p, canonical_id, revision, receipt)?;
        }
        Transition::Conflict { error, receipt }
            if matches!(p.state, Sending | Unknown | SubmittedForReview) =>
        {
            p.state = Conflict;
            p.resume_state = None;
            p.last_error = Some(error);
            p.receipt = Some(receipt);
        }
        Transition::Blocked { error } => {
            p.resume_state = Some(if p.state == Sending && phase == Queued {
                Unknown
            } else {
                phase
            });
            p.state = Blocked;
            p.last_error = Some(error);
        }
        Transition::Resume if p.state == Blocked => {
            p.state = p
                .resume_state
                .take()
                .ok_or(OutboxError::InvalidTransition)?;
            p.last_error = None;
        }
        Transition::Resume if p.state == Conflict && p.proposal_id.is_some() => {
            p.state = SubmittedForReview;
            p.resume_state = None;
            p.last_error = None;
        }
        Transition::Dismissed { receipt }
            if p.proposal_id.is_some()
                && matches!(p.state, SubmittedForReview | Sending | Unknown | Conflict) =>
        {
            p.state = Dismissed;
            p.resume_state = None;
            p.receipt = Some(receipt);
            p.last_error = None;
        }
        Transition::Cancelled if p.attempts == 0 && matches!(p.state, Queued | Blocked) => {
            p.state = Cancelled;
            p.resume_state = None;
            p.last_error = None;
        }
        _ => return Err(OutboxError::InvalidTransition),
    }
    if p.last_error
        .as_ref()
        .is_some_and(|error| error.len() > 1024)
    {
        return Err(OutboxError::Limit);
    }
    Ok(())
}

fn save_result(
    p: &mut Publication,
    canonical_id: String,
    revision: u64,
    receipt: Value,
) -> Result<(), OutboxError> {
    validate_identifier(&canonical_id)?;
    if revision == 0 {
        return Err(OutboxError::Invalid("canonical revision must be positive"));
    }
    p.state = PublicationState::Saved;
    p.resume_state = None;
    p.canonical_id = Some(canonical_id);
    p.canonical_revision = Some(revision);
    p.receipt = Some(receipt);
    p.last_error = None;
    Ok(())
}

#[cfg(test)]
#[path = "outbox_tests.rs"]
mod tests;
