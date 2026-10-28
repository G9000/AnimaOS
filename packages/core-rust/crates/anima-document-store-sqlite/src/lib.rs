//! Persistent, bounded local document passages, independent of private agent memory.

use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use rusqlite::{params, params_from_iter, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Maximum number of documents published by one complete source replacement.
pub const MAX_SOURCE_DOCUMENTS: usize = 200;
/// Maximum retained UTF-8 text in one document revision.
pub const MAX_DOCUMENT_TEXT_BYTES: usize = 1024 * 1024;
/// Maximum retained UTF-8 text in a complete source revision.
pub const MAX_SOURCE_TEXT_BYTES: usize = 32 * 1024 * 1024;
/// Maximum retained passage text across all projects in this derived cache.
pub const MAX_CACHE_TEXT_BYTES: usize = 256 * 1024 * 1024;
/// Maximum main SQLite database bytes, including FTS5 and row metadata.
pub const MAX_CACHE_DATABASE_BYTES: u64 = 512 * 1024 * 1024;
/// Maximum retained chunks in one document revision, including short spreadsheet cells.
pub const MAX_DOCUMENT_CHUNKS: usize = 2048;
/// Maximum UTF-8 bytes in an independently citable passage.
pub const MAX_CHUNK_TEXT_BYTES: usize = 4096;
/// Maximum UTF-8 bytes in a search excerpt.
pub const MAX_SEARCH_EXCERPT_BYTES: usize = 1200;
/// Maximum number of passages returned by a search.
pub const MAX_SEARCH_HITS: usize = 20;
const MAX_QUERY_CHARACTERS: usize = 512;
const MAX_QUERY_TERMS: usize = 32;
const MAX_ALLOWED_SOURCES: usize = 200;
const MAX_ID_BYTES: usize = 256;
const MAX_PATH_BYTES: usize = 1024;
const MAX_TITLE_BYTES: usize = 512;
const MAX_LOCATOR_BYTES: usize = 256;

/// One bounded passage with a host-derived location inside its document.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChunkInput {
    /// Displayable page, paragraph, cell, or line location, never an access capability.
    pub locator: String,
    /// Complete literal passage text.
    pub text: String,
}

/// Complete extracted revision of a source document, supplied by an authorized host.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DocumentInput {
    /// Stable document identity within the source.
    pub document_id: String,
    /// Normalized source-relative display path, with forward slashes.
    pub relative_path: String,
    /// Human-readable document name.
    pub title: String,
    /// Lowercase SHA-256 of the original bytes, checked again by the host on citation reads.
    pub source_sha256: String,
    /// Bounded extracted passages in document order.
    pub chunks: Vec<ChunkInput>,
}

/// Exact stored provenance and text for one currently indexed document passage.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchHit {
    /// Opaque revision-bound citation identity; this conveys no source authority.
    pub citation_id: String,
    /// Source identity supplied by the host.
    pub source_id: String,
    /// Stable document identity within the source.
    pub document_id: String,
    /// Source-relative display path.
    pub relative_path: String,
    /// Document title.
    pub title: String,
    /// Exact original document revision hash.
    pub source_sha256: String,
    /// SHA-256 of the complete literal chunk text.
    pub chunk_sha256: String,
    /// Host-derived position within the document.
    pub locator: String,
    /// Bounded query-centered passage excerpt.
    pub excerpt: String,
    /// Complete bounded passage text for a reauthorized citation read.
    pub text: String,
}

/// Storage failures distinguish closed input bounds from an unavailable derived cache.
#[derive(Debug)]
pub enum StoreError {
    /// Input exceeds the documented envelope or contains an invalid identity.
    InvalidInput(&'static str),
    /// SQLite could not read or atomically publish the derived index.
    Database(String),
    /// A prior panic interrupted the connection owner.
    Poisoned,
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(message) => write!(f, "invalid document index input: {message}"),
            Self::Database(message) => write!(f, "document index unavailable: {message}"),
            Self::Poisoned => f.write_str("document index connection owner interrupted"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<rusqlite::Error> for StoreError {
    fn from(value: rusqlite::Error) -> Self {
        // A corrupt schema can put source text in database-supplied messages.
        let category = value.sqlite_error_code().map_or_else(
            || "SQLite operation failed".into(),
            |code| format!("SQLite {code:?}"),
        );
        Self::Database(category)
    }
}

/// SQLite FTS5 cache with atomic source publication and explicit corpus isolation.
///
/// Hosts own authorization and filesystem custody of both the database directory and
/// document sources. Neither citation identities nor cached paths grant read access.
#[derive(Debug)]
pub struct DocumentStore {
    connection: Mutex<Connection>,
    text_limit: usize,
}

impl DocumentStore {
    /// Opens a derived cache at a host-authorized database path and creates its schema.
    ///
    /// # Errors
    /// Returns an error if SQLite cannot open the file or initialize FTS5.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        Self::open_with_limits(path, MAX_CACHE_TEXT_BYTES, MAX_CACHE_DATABASE_BYTES)
    }

    fn open_with_limits(
        path: impl AsRef<Path>,
        text_limit: usize,
        database_bytes: u64,
    ) -> Result<Self, StoreError> {
        let connection = Connection::open(path)?;
        connection.busy_timeout(Duration::from_secs(2))?;
        let page_size: u64 = connection.pragma_query_value(None, "page_size", |row| row.get(0))?;
        let maximum_pages = database_bytes
            .checked_div(page_size)
            .filter(|pages| *pages > 0)
            .ok_or(StoreError::InvalidInput("invalid database size envelope"))?;
        let actual_pages: u64 =
            connection
                .pragma_update_and_check(None, "max_page_count", maximum_pages, |row| row.get(0))?;
        if actual_pages > maximum_pages {
            return Err(StoreError::InvalidInput(
                "existing cache exceeds database envelope",
            ));
        }
        connection.pragma_update(None, "journal_size_limit", database_bytes)?;
        connection.execute_batch(
            "PRAGMA foreign_keys=ON;
             PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS documents (
                 corpus_id TEXT NOT NULL, source_id TEXT NOT NULL,
                 document_id TEXT NOT NULL, relative_path TEXT NOT NULL,
                 title TEXT NOT NULL, source_sha256 TEXT NOT NULL,
                 PRIMARY KEY (corpus_id, source_id, document_id),
                 UNIQUE (corpus_id, source_id, document_id, source_sha256)
             );
             CREATE TABLE IF NOT EXISTS chunks (
                 id INTEGER PRIMARY KEY, corpus_id TEXT NOT NULL, source_id TEXT NOT NULL,
                 document_id TEXT NOT NULL, source_sha256 TEXT NOT NULL,
                 citation_id TEXT NOT NULL UNIQUE, chunk_sha256 TEXT NOT NULL,
                 locator TEXT NOT NULL, title TEXT NOT NULL, text TEXT NOT NULL,
                 FOREIGN KEY (corpus_id, source_id, document_id, source_sha256)
                 REFERENCES documents(corpus_id, source_id, document_id, source_sha256)
                 ON DELETE CASCADE
             );
             CREATE INDEX IF NOT EXISTS chunks_scope ON chunks(corpus_id, source_id);
             CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
                 title, text, content='chunks', content_rowid='id', tokenize='unicode61'
             );
             CREATE TRIGGER IF NOT EXISTS chunks_insert AFTER INSERT ON chunks BEGIN
                 INSERT INTO chunks_fts(rowid,title,text) VALUES(new.id,new.title,new.text);
             END;
             CREATE TRIGGER IF NOT EXISTS chunks_delete AFTER DELETE ON chunks BEGIN
                 INSERT INTO chunks_fts(chunks_fts,rowid,title,text)
                 VALUES('delete',old.id,old.title,old.text);
             END;",
        )?;
        checkpoint(&connection);
        Ok(Self {
            connection: Mutex::new(connection),
            text_limit,
        })
    }

    fn connection(&self) -> Result<MutexGuard<'_, Connection>, StoreError> {
        self.connection.lock().map_err(|_| StoreError::Poisoned)
    }

    /// Atomically replaces every document and passage belonging to one source.
    ///
    /// # Errors
    /// Rejects invalid bounds, duplicate document identities, or unsafe display paths
    /// before changing the prior revision. Database failures roll back the transaction.
    pub fn replace_source(
        &self,
        corpus_id: &str,
        source_id: &str,
        documents: &[DocumentInput],
    ) -> Result<(), StoreError> {
        validate_id(corpus_id)?;
        validate_id(source_id)?;
        validate_documents(documents)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "DELETE FROM documents WHERE corpus_id=?1 AND source_id=?2",
            params![corpus_id, source_id],
        )?;
        let existing_bytes: u64 = transaction.query_row(
            "SELECT coalesce(sum(length(CAST(text AS BLOB))),0) FROM chunks",
            [],
            |row| row.get(0),
        )?;
        let incoming_bytes = documents
            .iter()
            .flat_map(|document| &document.chunks)
            .map(|chunk| chunk.text.len() as u64)
            .sum::<u64>();
        if existing_bytes
            .checked_add(incoming_bytes)
            .is_none_or(|bytes| bytes > self.text_limit as u64)
        {
            return Err(StoreError::InvalidInput("aggregate cache text exceeded"));
        }
        {
            let mut insert_document = transaction.prepare("INSERT INTO documents(corpus_id,source_id,document_id,relative_path,title,source_sha256) VALUES(?1,?2,?3,?4,?5,?6)")?;
            let mut insert_chunk = transaction.prepare("INSERT INTO chunks(corpus_id,source_id,document_id,source_sha256,citation_id,chunk_sha256,locator,title,text) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)")?;
            for document in documents {
                insert_document.execute(params![
                    corpus_id,
                    source_id,
                    document.document_id,
                    document.relative_path,
                    document.title,
                    document.source_sha256
                ])?;
                for (index, chunk) in document.chunks.iter().enumerate() {
                    let chunk_sha256 = source_sha256(chunk.text.as_bytes());
                    let citation_id =
                        citation_identity(corpus_id, source_id, document, chunk, index);
                    insert_chunk.execute(params![
                        corpus_id,
                        source_id,
                        document.document_id,
                        document.source_sha256,
                        citation_id,
                        chunk_sha256,
                        chunk.locator,
                        document.title,
                        chunk.text
                    ])?;
                }
            }
        }
        transaction.commit()?;
        // Maintenance cannot turn a successful commit into a reported rollback.
        checkpoint(&connection);
        Ok(())
    }

    /// Searches only explicitly allowed source identities within a single corpus.
    ///
    /// Query tokens are quoted as literal FTS5 terms. The host supplies the current
    /// authorized sources on every call; an empty allowed list always returns no hits.
    ///
    /// # Errors
    /// Rejects queries over 512 characters, invalid identities, or more than 200 sources.
    pub fn search(
        &self,
        corpus_id: &str,
        allowed_source_ids: &[String],
        query: &str,
        limit: usize,
    ) -> Result<Vec<SearchHit>, StoreError> {
        validate_id(corpus_id)?;
        if query.chars().count() > MAX_QUERY_CHARACTERS
            || allowed_source_ids.len() > MAX_ALLOWED_SOURCES
        {
            return Err(StoreError::InvalidInput("search envelope exceeded"));
        }
        for source in allowed_source_ids {
            validate_id(source)?;
        }
        let terms = query_terms(query);
        if terms.is_empty() || allowed_source_ids.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let match_query = terms
            .iter()
            .map(|term| format!("\"{term}\""))
            .collect::<Vec<_>>()
            .join(" OR ");
        let placeholders = std::iter::repeat_n("?", allowed_source_ids.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT c.citation_id,c.source_id,c.document_id,d.relative_path,d.title,c.source_sha256,c.chunk_sha256,c.locator,c.text
             FROM chunks_fts JOIN chunks c ON c.id=chunks_fts.rowid
             JOIN documents d ON d.corpus_id=c.corpus_id AND d.source_id=c.source_id
               AND d.document_id=c.document_id AND d.source_sha256=c.source_sha256
             WHERE chunks_fts MATCH ? AND c.corpus_id=? AND c.source_id IN ({placeholders})
             ORDER BY bm25(chunks_fts,2.0,1.0),c.citation_id LIMIT ?"
        );
        let bounded_limit = limit.min(MAX_SEARCH_HITS).to_string();
        let mut bindings = vec![match_query.as_str(), corpus_id];
        bindings.extend(allowed_source_ids.iter().map(String::as_str));
        bindings.push(&bounded_limit);
        let connection = self.connection()?;
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(bindings), read_hit)?;
        let mut hits = Vec::new();
        for row in rows {
            let mut hit = row?;
            hit.excerpt = excerpt(&hit.text, &terms);
            hits.push(hit);
        }
        Ok(hits)
    }

    /// Resolves an opaque citation only while its exact indexed revision still exists.
    ///
    /// The caller must reauthorize source access and verify current original bytes.
    ///
    /// # Errors
    /// Returns an error for invalid identities or an unavailable SQLite cache.
    pub fn citation(
        &self,
        corpus_id: &str,
        citation_id: &str,
    ) -> Result<Option<SearchHit>, StoreError> {
        validate_id(corpus_id)?;
        validate_id(citation_id)?;
        let connection = self.connection()?;
        let hit = connection.query_row(
            "SELECT c.citation_id,c.source_id,c.document_id,d.relative_path,d.title,c.source_sha256,c.chunk_sha256,c.locator,c.text
             FROM chunks c JOIN documents d ON d.corpus_id=c.corpus_id AND d.source_id=c.source_id
               AND d.document_id=c.document_id AND d.source_sha256=c.source_sha256
             WHERE c.corpus_id=?1 AND c.citation_id=?2",
            params![corpus_id,citation_id], read_hit,
        ).optional()?;
        Ok(hit)
    }

    /// Removes a source revision and its searchable passages atomically.
    ///
    /// # Errors
    /// Returns an error for invalid identities or a database failure.
    pub fn remove_source(&self, corpus_id: &str, source_id: &str) -> Result<(), StoreError> {
        self.replace_source(corpus_id, source_id, &[])
    }

    /// Atomically discards persisted sources absent from the host's current grant list.
    ///
    /// This also reconciles stale cache rows after restart, when the process has no
    /// prior in-memory inventory. An empty list removes the complete corpus.
    ///
    /// # Errors
    /// Rejects invalid identities or more than 200 allowed sources. Database errors
    /// roll back the complete reconciliation.
    pub fn retain_sources(
        &self,
        corpus_id: &str,
        allowed_source_ids: &[String],
    ) -> Result<(), StoreError> {
        validate_id(corpus_id)?;
        if allowed_source_ids.len() > MAX_ALLOWED_SOURCES {
            return Err(StoreError::InvalidInput(
                "source reconciliation envelope exceeded",
            ));
        }
        for source in allowed_source_ids {
            validate_id(source)?;
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        if allowed_source_ids.is_empty() {
            transaction.execute("DELETE FROM documents WHERE corpus_id=?1", [corpus_id])?;
        } else {
            let placeholders = std::iter::repeat_n("?", allowed_source_ids.len())
                .collect::<Vec<_>>()
                .join(",");
            let sql = format!(
                "DELETE FROM documents WHERE corpus_id=? AND source_id NOT IN ({placeholders})"
            );
            let mut bindings = vec![corpus_id];
            bindings.extend(allowed_source_ids.iter().map(String::as_str));
            transaction.execute(&sql, params_from_iter(bindings))?;
        }
        transaction.commit()?;
        checkpoint(&connection);
        Ok(())
    }

    /// Removes every source and searchable passage from a corpus atomically.
    ///
    /// # Errors
    /// Returns an error for an invalid identity or a database failure.
    pub fn remove_corpus(&self, corpus_id: &str) -> Result<(), StoreError> {
        validate_id(corpus_id)?;
        let connection = self.connection()?;
        connection.execute("DELETE FROM documents WHERE corpus_id=?1", [corpus_id])?;
        checkpoint(&connection);
        Ok(())
    }
}

fn checkpoint(connection: &Connection) {
    // Readers share this mutex. External readers may defer truncation; later
    // publications and opens retry without misreporting the committed revision.
    let _ = connection.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
        row.get::<_, i64>(0)
    });
}

/// Computes the lowercase SHA-256 used to bind original bytes and complete passages.
pub fn source_sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn validate_id(value: &str) -> Result<(), StoreError> {
    if value.trim().is_empty() || value.len() > MAX_ID_BYTES || value.chars().any(char::is_control)
    {
        Err(StoreError::InvalidInput("invalid identity"))
    } else {
        Ok(())
    }
}

fn valid_metadata(value: &str, maximum: usize) -> bool {
    !value.trim().is_empty() && value.len() <= maximum && !value.chars().any(char::is_control)
}

fn validate_documents(documents: &[DocumentInput]) -> Result<(), StoreError> {
    if documents.len() > MAX_SOURCE_DOCUMENTS {
        return Err(StoreError::InvalidInput("too many documents"));
    }
    let mut identities = BTreeSet::new();
    let mut paths = BTreeSet::new();
    let mut source_bytes = 0usize;
    for document in documents {
        validate_id(&document.document_id)?;
        if !identities.insert(&document.document_id)
            || !paths.insert(&document.relative_path)
            || !valid_metadata(&document.relative_path, MAX_PATH_BYTES)
            || document.relative_path.contains(['\\', ':'])
            || document
                .relative_path
                .split('/')
                .any(|part| matches!(part, "" | "." | ".."))
            || !valid_metadata(&document.title, MAX_TITLE_BYTES)
            || document.source_sha256.len() != 64
            || !document
                .source_sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            || document.chunks.len() > MAX_DOCUMENT_CHUNKS
        {
            return Err(StoreError::InvalidInput("invalid document revision"));
        }
        let mut document_bytes = 0usize;
        for chunk in &document.chunks {
            if !valid_metadata(&chunk.locator, MAX_LOCATOR_BYTES)
                || chunk.text.trim().is_empty()
                || chunk.text.len() > MAX_CHUNK_TEXT_BYTES
                || chunk.text.chars().any(|character| {
                    character.is_control() && !matches!(character, '\t' | '\n' | '\r')
                })
            {
                return Err(StoreError::InvalidInput("invalid document passage"));
            }
            document_bytes = document_bytes
                .checked_add(chunk.text.len())
                .ok_or(StoreError::InvalidInput("document text overflow"))?;
        }
        if document_bytes > MAX_DOCUMENT_TEXT_BYTES {
            return Err(StoreError::InvalidInput("document text exceeded"));
        }
        source_bytes = source_bytes
            .checked_add(document_bytes)
            .ok_or(StoreError::InvalidInput("source text overflow"))?;
        if source_bytes > MAX_SOURCE_TEXT_BYTES {
            return Err(StoreError::InvalidInput("source text exceeded"));
        }
    }
    Ok(())
}

fn citation_identity(
    corpus: &str,
    source: &str,
    document: &DocumentInput,
    chunk: &ChunkInput,
    index: usize,
) -> String {
    let mut hash = Sha256::new();
    // Length framing prevents ambiguous field concatenation across host identities.
    for field in [
        "anima-document-citation-v1",
        corpus,
        source,
        &document.document_id,
        &document.relative_path,
        &document.title,
        &document.source_sha256,
        &chunk.locator,
        &chunk.text,
    ] {
        hash.update((field.len() as u64).to_le_bytes());
        hash.update(field.as_bytes());
    }
    hash.update((index as u64).to_le_bytes());
    format!("cite_{:x}", hash.finalize())
}

fn read_hit(row: &rusqlite::Row<'_>) -> rusqlite::Result<SearchHit> {
    let text: String = row.get(8)?;
    Ok(SearchHit {
        citation_id: row.get(0)?,
        source_id: row.get(1)?,
        document_id: row.get(2)?,
        relative_path: row.get(3)?,
        title: row.get(4)?,
        source_sha256: row.get(5)?,
        chunk_sha256: row.get(6)?,
        locator: row.get(7)?,
        excerpt: excerpt(&text, &[]),
        text,
    })
}

fn query_terms(query: &str) -> Vec<String> {
    query
        .split(|character: char| !character.is_alphanumeric())
        .filter(|term| !term.is_empty())
        .take(MAX_QUERY_TERMS)
        .map(str::to_lowercase)
        .collect()
}

fn excerpt(text: &str, terms: &[String]) -> String {
    excerpt_with_limit(text, terms, MAX_SEARCH_EXCERPT_BYTES)
}

/// Shortens a literal passage around a query match while preserving UTF-8 boundaries.
/// The requested byte budget is capped at the store's search-excerpt limit.
pub fn excerpt_for_query(text: &str, query: &str, max_bytes: usize) -> String {
    excerpt_with_limit(
        text,
        &query_terms(query),
        max_bytes.min(MAX_SEARCH_EXCERPT_BYTES),
    )
}

fn excerpt_with_limit(text: &str, terms: &[String], max_bytes: usize) -> String {
    if max_bytes == 0 {
        return String::new();
    }
    if text.len() <= max_bytes {
        return text.into();
    }
    // Lowercasing can expand a character (for example İ). Map every folded byte
    // back to its original byte offset rather than applying folded offsets to text.
    // Whole-string normalization also preserves contextual final sigma. Its
    // contextual replacement has the same UTF-8 length as scalar lowercasing.
    let lowercase = text.to_lowercase();
    let mut source_offsets = Vec::with_capacity(text.len());
    for (offset, character) in text.char_indices() {
        for folded in character.to_lowercase() {
            source_offsets.extend(std::iter::repeat_n(offset, folded.len_utf8()));
        }
    }
    let folded_position = terms
        .iter()
        .filter_map(|term| lowercase.find(term))
        .min()
        .unwrap_or(0);
    let position = source_offsets.get(folded_position).copied().unwrap_or(0);
    let mut start = position.saturating_sub(max_bytes / 3);
    while !text.is_char_boundary(start) {
        start -= 1;
    }
    let mut end = start.saturating_add(max_bytes).min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[start..end].into()
}

#[cfg(test)]
mod tests;
