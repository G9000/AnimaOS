//! Derived local-vector storage. Embedding computation and source authority belong to hosts.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use rusqlite::{params, params_from_iter, Connection, ErrorCode, OptionalExtension};

use super::{
    checkpoint, excerpt, query_terms, read_hit, validate_id, DocumentStore, SearchHit, StoreError,
    MAX_ALLOWED_SOURCES, MAX_QUERY_CHARACTERS, MAX_SEARCH_HITS,
};

const MAX_DIMENSION: usize = 1024;
const WINDOW_BYTES: usize = 384;
const WINDOW_OVERLAP_BYTES: usize = 64;
// Raw coverage uses at most thirteen spans for a validated 4096-byte chunk.
// The remaining slots add sentence context without unbounded inference work.
const MAX_WINDOWS_PER_CHUNK: usize = 32;
const MAX_SCOPE_CHUNKS: usize = 4096;
const MAX_VECTOR_ROWS: usize = 32_768;
const MAX_VECTOR_BYTES: usize = 64 * 1024 * 1024;

/// Stable identity of passage boundaries, deduplication, and per-chunk inference bounds.
///
/// Hosts must include this identity in their embedding profile. Changing it also
/// invalidates stored vectors and refreshes persisted window counts on cache open.
pub const EMBEDDING_WINDOW_CONTRACT: &str = "bytes384-overlap64-long-sentences-pairs-cap32-v2";

/// Identity of a host's model artifacts/session, prompts, and passage-window contract.
///
/// Hosts must change the key whenever any of those inputs changes. The store cannot
/// verify model artifacts; matching dimension alone never authorizes vector reuse.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmbeddingProfile {
    key: String,
    dimension: usize,
}

impl EmbeddingProfile {
    /// Validates a nonempty profile identity and a dimension between 1 and 1024.
    ///
    /// # Errors
    /// Rejects invalid profile identities or dimensions outside the storage envelope.
    pub fn new(key: String, dimension: usize) -> Result<Self, StoreError> {
        validate_id(&key)?;
        if dimension == 0 || dimension > MAX_DIMENSION {
            return Err(StoreError::InvalidInput("embedding dimension exceeded"));
        }
        Ok(Self { key, dimension })
    }

    /// Returns the host's complete model and preprocessing identity.
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Returns the required number of finite components in each vector.
    pub fn dimension(&self) -> usize {
        self.dimension
    }
}

/// A bounded UTF-8 window, retaining its original revision-bound citation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmbeddingPassage {
    /// Exact current passage identity.
    pub citation_id: String,
    /// Zero-based deterministic window ordinal under the profile's window contract.
    pub window: usize,
    /// At most 384 UTF-8 bytes for host-side local inference.
    pub text: String,
}

/// A host-computed vector for one exact passage window.
#[derive(Clone, Debug, PartialEq)]
pub struct PassageEmbedding {
    /// Exact current passage identity, checked again during publication.
    pub citation_id: String,
    /// Window ordinal originally returned by pending enumeration.
    pub window: usize,
    /// Finite nonzero vector, normalized by the store before publication.
    pub vector: Vec<f32>,
}

/// Semantic indexing coverage of the current corpus and explicit allowed sources.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SemanticCoverage {
    /// Valid profile-matching windows actually considered during this search.
    pub indexed_windows: usize,
    /// All nonempty current passage windows within the allowed sources.
    pub total_windows: usize,
    /// Windows outside the deterministic 4096-current-chunk semantic scan envelope.
    ///
    /// Other unindexed windows equal `total_windows - omitted_windows - indexed_windows`.
    pub omitted_windows: usize,
}

/// Ranked evidence and truthful coverage; cosine similarity is not a confidence score.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HybridSearch {
    /// At most twenty current, explicitly allowed, citation-deduplicated passages.
    pub hits: Vec<SearchHit>,
    /// Coverage before result truncation, including missing and omitted windows.
    pub coverage: SemanticCoverage,
}

pub(super) fn initialize(connection: &mut Connection) -> Result<(), StoreError> {
    let has_count = {
        let mut statement = connection.prepare("PRAGMA table_info(chunks)")?;
        let columns = statement.query_map([], |row| row.get::<_, String>(1))?;
        let mut found = false;
        for column in columns {
            found |= column? == "embedding_window_count";
        }
        found
    };
    let transaction = connection.transaction()?;
    if !has_count {
        transaction.execute_batch(
            "ALTER TABLE chunks ADD COLUMN embedding_window_count INTEGER NOT NULL DEFAULT 0;",
        )?;
    }
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS passage_embeddings (
             profile TEXT NOT NULL, citation_id TEXT NOT NULL,
             window INTEGER NOT NULL CHECK(window>=0),
             dimension INTEGER NOT NULL CHECK(dimension BETWEEN 1 AND 1024),
             vector BLOB NOT NULL,
             PRIMARY KEY(profile,citation_id,window),
             FOREIGN KEY(citation_id) REFERENCES chunks(citation_id) ON DELETE CASCADE
         );
         CREATE INDEX IF NOT EXISTS passage_embeddings_citation
             ON passage_embeddings(citation_id,profile);
         CREATE TABLE IF NOT EXISTS semantic_index_metadata (
             key TEXT PRIMARY KEY, value TEXT NOT NULL
         );",
    )?;
    let prior_contract = transaction
        .query_row(
            "SELECT value FROM semantic_index_metadata WHERE key='embedding_window_contract'",
            [],
            |row| {
                Ok(match row.get_ref(0)? {
                    rusqlite::types::ValueRef::Text(bytes) => {
                        std::str::from_utf8(bytes).ok().map(str::to_owned)
                    }
                    _ => None,
                })
            },
        )
        .optional()?
        .flatten();
    if !has_count || prior_contract.as_deref() != Some(EMBEDDING_WINDOW_CONTRACT) {
        // Counts describe this store's boundaries, independent of the active host
        // profile. Refresh even nonzero old counts and invalidate every old ordinal.
        transaction.execute("DELETE FROM passage_embeddings", [])?;
        let mut after_id: Option<i64> = None;
        loop {
            let counts = {
                let mut statement = transaction.prepare(
                    "SELECT id,text FROM chunks WHERE (?1 IS NULL OR id>?1) ORDER BY id LIMIT 256",
                )?;
                let rows = statement.query_map([after_id], |row| {
                    let count = match row.get_ref(1)? {
                        rusqlite::types::ValueRef::Text(bytes) => {
                            std::str::from_utf8(bytes).map_or(0, window_count)
                        }
                        _ => 0,
                    };
                    Ok((row.get::<_, i64>(0)?, count))
                })?;
                rows.collect::<Result<Vec<_>, _>>()?
            };
            if counts.is_empty() {
                break;
            }
            let mut statement =
                transaction.prepare("UPDATE chunks SET embedding_window_count=?1 WHERE id=?2")?;
            for (id, count) in counts {
                statement.execute(params![count, id])?;
                // Empty or corrupt text is a zero-count row, never a cursor retry.
                after_id = Some(id);
            }
        }
        transaction.execute(
            "INSERT INTO semantic_index_metadata(key,value) VALUES('embedding_window_contract',?1)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            [EMBEDDING_WINDOW_CONTRACT],
        )?;
    }
    transaction.commit()?;
    Ok(())
}

pub(super) fn window_count(text: &str) -> usize {
    window_ranges(text).len()
}

fn window_ranges(text: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut start = 0;
    while start < text.len() {
        let mut end = start.saturating_add(WINDOW_BYTES).min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        ranges.push(start..end);
        if end == text.len() {
            break;
        }
        let mut next = end.saturating_sub(WINDOW_OVERLAP_BYTES);
        while !text.is_char_boundary(next) {
            next += 1;
        }
        start = next.max(start + 1);
        while !text.is_char_boundary(start) {
            start += 1;
        }
    }
    // Keep raw spans first so long unpunctuated text retains complete byte coverage.
    // Identical trimmed content needs one vector, retaining its first source span.
    let mut seen = BTreeSet::new();
    ranges.retain(|range| {
        let trimmed = text[range.clone()].trim();
        !trimmed.is_empty() && seen.insert(trimmed)
    });
    if text.len() <= WINDOW_BYTES {
        return ranges;
    }

    let sentences = sentence_ranges(text);
    let mut optional = Vec::new();
    for (index, sentence) in sentences.iter().enumerate() {
        let mut add = |range: Range<usize>| {
            if range.len() <= WINDOW_BYTES && seen.insert(&text[range.clone()]) {
                optional.push(range);
            }
        };
        add(sentence.clone());
        if let Some(next) = sentences.get(index + 1) {
            add(sentence.start..next.end);
        }
    }
    let available = MAX_WINDOWS_PER_CHUNK.saturating_sub(ranges.len());
    if optional.len() <= available {
        ranges.extend(optional);
    } else if available == 1 {
        ranges.extend(optional.pop());
    } else if available > 1 {
        // Spread scarce optional slots through the chunk while retaining the tail
        // pair and final sentence when present in the candidate sequence.
        let spread = available - 2;
        let tail = optional.len() - 2;
        for index in 0..spread {
            ranges.push(optional[index * tail / spread].clone());
        }
        ranges.extend_from_slice(&optional[tail..]);
    }
    ranges
}

fn sentence_ranges(text: &str) -> Vec<Range<usize>> {
    let mut sentences = Vec::new();
    let mut start = 0;
    let mut characters = text.char_indices().peekable();
    while let Some((index, character)) = characters.next() {
        let mut end = index + character.len_utf8();
        let cjk_terminal = matches!(character, '。' | '！' | '？');
        let terminal = cjk_terminal || matches!(character, '.' | '!' | '?' | '．');
        if terminal {
            while let Some(&(next_index, next)) = characters.peek() {
                if !matches!(
                    next,
                    '\'' | '"' | '’' | '”' | ')' | ']' | '}' | '」' | '』' | '）' | '】'
                ) {
                    break;
                }
                end = next_index + next.len_utf8();
                characters.next();
            }
        }
        let sentence_end = terminal
            && (cjk_terminal
                || characters
                    .peek()
                    .is_none_or(|(_, next)| next.is_whitespace()));
        let paragraph_end = character == '\n'
            && text[end..]
                .trim_start_matches([' ', '\t', '\r'])
                .starts_with('\n');
        if sentence_end || paragraph_end {
            if let Some(range) = trimmed_range(text, start..end) {
                sentences.push(range);
            }
            start = end;
        }
    }
    if let Some(range) = trimmed_range(text, start..text.len()) {
        sentences.push(range);
    }
    sentences
}

fn trimmed_range(text: &str, range: Range<usize>) -> Option<Range<usize>> {
    let source = &text[range.clone()];
    let trimmed = source.trim();
    if trimmed.is_empty() {
        return None;
    }
    let start = range.start + source.len() - source.trim_start().len();
    Some(start..start + trimmed.len())
}

impl DocumentStore {
    /// Enumerates missing model windows from at most 4096 current allowed chunks.
    ///
    /// The deterministic order is source identity, document identity, then chunk order.
    /// Obsolete model profiles in this corpus are retired because the current host has
    /// one active local model session. Corrupt derived vectors are considered missing.
    ///
    /// # Errors
    /// Rejects invalid scope or an unavailable derived cache. Empty scopes return no text.
    pub fn pending_embeddings(
        &self,
        corpus_id: &str,
        allowed_source_ids: &[String],
        profile: &EmbeddingProfile,
        limit: usize,
    ) -> Result<Vec<EmbeddingPassage>, StoreError> {
        validate_scope(corpus_id, allowed_source_ids)?;
        if allowed_source_ids.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let connection = self.connection()?;
        retire_profiles(&connection, corpus_id, profile)?;
        let (chunks, _) = scoped_chunks(&connection, corpus_id, allowed_source_ids)?;
        let mut output = Vec::new();
        let bounded_limit = limit.min(MAX_VECTOR_ROWS);
        let mut statement = connection.prepare(
            "SELECT window,dimension,vector FROM passage_embeddings WHERE profile=?1 AND citation_id=?2",
        )?;
        for hit in chunks {
            let stored = stored_vectors(&mut statement, &hit.citation_id, profile)?;
            for (window, range) in window_ranges(&hit.text).into_iter().enumerate() {
                if stored.contains_key(&window) {
                    continue;
                }
                output.push(EmbeddingPassage {
                    citation_id: hit.citation_id.clone(),
                    window,
                    text: hit.text[range].into(),
                });
                if output.len() == bounded_limit {
                    return Ok(output);
                }
            }
        }
        Ok(output)
    }

    /// Conditionally publishes normalized compact vectors for exact current windows.
    ///
    /// Stale citations/windows and vectors exceeding the global 32768-row/64-MiB
    /// payload budgets are skipped. A full SQLite cache skips remaining derived work;
    /// keyword evidence remains authoritative. Superseded profiles in this corpus are
    /// disposable and removed. The return value is the number of vectors published.
    ///
    /// # Errors
    /// Rejects invalid identities, oversized batches, or nonfinite, zero, wrong-sized
    /// vectors before writing anything. Other SQLite failures are reported.
    pub fn publish_embeddings(
        &self,
        corpus_id: &str,
        profile: &EmbeddingProfile,
        embeddings: &[PassageEmbedding],
    ) -> Result<usize, StoreError> {
        validate_id(corpus_id)?;
        if embeddings.len() > MAX_VECTOR_ROWS {
            return Err(StoreError::InvalidInput("embedding batch exceeded"));
        }
        let mut encoded = Vec::with_capacity(embeddings.len());
        for embedding in embeddings {
            validate_id(&embedding.citation_id)?;
            let vector = normalize(&embedding.vector, profile.dimension)?;
            encoded.push(
                vector
                    .into_iter()
                    .flat_map(f32::to_le_bytes)
                    .collect::<Vec<_>>(),
            );
        }
        let connection = self.connection()?;
        retire_profiles(&connection, corpus_id, profile)?;
        let (mut rows, mut bytes): (usize, usize) = connection.query_row(
            "SELECT count(*),coalesce(sum(length(vector)),0) FROM passage_embeddings",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let mut published = 0;
        for (embedding, blob) in embeddings.iter().zip(encoded) {
            if embedding.window > super::MAX_CHUNK_TEXT_BYTES {
                continue;
            }
            let current: bool = connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM chunks WHERE corpus_id=?1 AND citation_id=?2 AND embedding_window_count>?3)",
                params![corpus_id, embedding.citation_id, embedding.window], |row| row.get(0),
            )?;
            if !current {
                continue;
            }
            let prior: Option<usize> = connection.query_row(
                "SELECT length(vector) FROM passage_embeddings WHERE profile=?1 AND citation_id=?2 AND window=?3",
                params![profile.key, embedding.citation_id, embedding.window], |row| row.get(0),
            ).optional()?;
            let next_rows = rows + usize::from(prior.is_none());
            let next_bytes = bytes.saturating_sub(prior.unwrap_or(0)) + blob.len();
            if next_rows > MAX_VECTOR_ROWS || next_bytes > MAX_VECTOR_BYTES {
                continue;
            }
            let result = connection.execute(
                "INSERT INTO passage_embeddings(profile,citation_id,window,dimension,vector) VALUES(?1,?2,?3,?4,?5)
                 ON CONFLICT(profile,citation_id,window) DO UPDATE SET dimension=excluded.dimension,vector=excluded.vector",
                params![profile.key, embedding.citation_id, embedding.window, profile.dimension, blob],
            );
            match result {
                Ok(_) => {
                    rows = next_rows;
                    bytes = next_bytes;
                    published += 1;
                }
                Err(error) if is_full(&error) => break,
                Err(error) => return Err(error.into()),
            }
        }
        checkpoint(&connection);
        Ok(published)
    }

    /// Combines twenty semantic and twenty keyword candidates within current authority.
    ///
    /// All semantic vectors are filtered by corpus, allowed source, current citation,
    /// model profile, and the reported 4096-chunk envelope before scoring. The best
    /// window represents each citation. Verified exact phrases, short exact queries,
    /// codenames, and numeric/structured anchors precede other candidates. Remaining
    /// candidates use weighted reciprocal-rank fusion: `0.8/(1+semantic_rank)` plus
    /// `0.2/(1+keyword_rank)`, with one-based ranks and deterministic citation ties.
    /// Cosine has no invented relevance threshold or confidence interpretation.
    ///
    /// # Errors
    /// Rejects invalid scope/query bounds or invalid query vectors; reports cache errors.
    pub fn hybrid_search(
        &self,
        corpus_id: &str,
        allowed_source_ids: &[String],
        query: &str,
        limit: usize,
        profile: &EmbeddingProfile,
        query_vector: &[f32],
    ) -> Result<HybridSearch, StoreError> {
        validate_scope(corpus_id, allowed_source_ids)?;
        if query.chars().count() > MAX_QUERY_CHARACTERS {
            return Err(StoreError::InvalidInput("search envelope exceeded"));
        }
        let query_vector = normalize(query_vector, profile.dimension)?;
        if allowed_source_ids.is_empty() {
            return Ok(HybridSearch {
                hits: Vec::new(),
                coverage: SemanticCoverage::default(),
            });
        }
        let connection = self.connection()?;
        let (chunks, total_windows) = scoped_chunks(&connection, corpus_id, allowed_source_ids)?;
        let scanned_windows = chunks
            .iter()
            .map(|hit| window_count(&hit.text))
            .sum::<usize>();
        let terms = query_terms(query);
        let mut coverage = SemanticCoverage {
            indexed_windows: 0,
            total_windows,
            omitted_windows: total_windows.saturating_sub(scanned_windows),
        };
        let mut semantic = Vec::new();
        let mut statement = connection.prepare(
            "SELECT window,dimension,vector FROM passage_embeddings WHERE profile=?1 AND citation_id=?2",
        )?;
        for mut hit in chunks {
            let stored = stored_vectors(&mut statement, &hit.citation_id, profile)?;
            let mut best: Option<(f64, std::ops::Range<usize>)> = None;
            for (window, range) in window_ranges(&hit.text).into_iter().enumerate() {
                let Some(vector) = stored.get(&window) else {
                    continue;
                };
                coverage.indexed_windows += 1;
                let similarity = vector
                    .iter()
                    .zip(&query_vector)
                    .map(|(a, b)| f64::from(*a) * f64::from(*b))
                    .sum::<f64>();
                if best
                    .as_ref()
                    .is_none_or(|(previous, _)| similarity > *previous)
                {
                    best = Some((similarity, range));
                }
            }
            if let Some((similarity, range)) = best {
                hit.excerpt = excerpt(&hit.text[range], &terms);
                semantic.push((similarity, hit));
            }
        }
        if terms.is_empty() || limit == 0 {
            return Ok(HybridSearch {
                hits: Vec::new(),
                coverage,
            });
        }
        semantic.sort_by(|a, b| {
            b.0.total_cmp(&a.0)
                .then_with(|| a.1.citation_id.cmp(&b.1.citation_id))
        });
        semantic.truncate(MAX_SEARCH_HITS);
        let keyword =
            keyword_candidates(&connection, corpus_id, allowed_source_ids, query, &terms)?;
        let hits = fuse(query, keyword, semantic)
            .into_iter()
            .take(limit.min(MAX_SEARCH_HITS))
            .collect();
        Ok(HybridSearch { hits, coverage })
    }
}

fn validate_scope(corpus_id: &str, source_ids: &[String]) -> Result<(), StoreError> {
    validate_id(corpus_id)?;
    if source_ids.len() > MAX_ALLOWED_SOURCES {
        return Err(StoreError::InvalidInput(
            "semantic source envelope exceeded",
        ));
    }
    for source in source_ids {
        validate_id(source)?;
    }
    Ok(())
}

fn retire_profiles(
    connection: &Connection,
    corpus: &str,
    profile: &EmbeddingProfile,
) -> Result<(), StoreError> {
    connection.execute(
        "DELETE FROM passage_embeddings WHERE profile<>?1 AND citation_id IN (SELECT citation_id FROM chunks WHERE corpus_id=?2)",
        params![profile.key, corpus],
    )?;
    Ok(())
}

fn scoped_chunks(
    connection: &Connection,
    corpus: &str,
    sources: &[String],
) -> Result<(Vec<SearchHit>, usize), StoreError> {
    let placeholders = std::iter::repeat_n("?", sources.len())
        .collect::<Vec<_>>()
        .join(",");
    let mut bindings = vec![corpus];
    bindings.extend(sources.iter().map(String::as_str));
    let total = connection.query_row(
        &format!("SELECT coalesce(sum(embedding_window_count),0) FROM chunks WHERE corpus_id=? AND source_id IN ({placeholders})"),
        params_from_iter(bindings.iter().copied()), |row| row.get(0),
    )?;
    let mut statement = connection.prepare(&format!(
        "SELECT c.citation_id,c.source_id,c.document_id,d.relative_path,d.title,c.source_sha256,c.chunk_sha256,c.locator,c.text
         FROM chunks c JOIN documents d ON d.corpus_id=c.corpus_id AND d.source_id=c.source_id
           AND d.document_id=c.document_id AND d.source_sha256=c.source_sha256
         WHERE c.corpus_id=? AND c.source_id IN ({placeholders}) AND c.embedding_window_count>0
         ORDER BY c.source_id,c.document_id,c.id LIMIT {MAX_SCOPE_CHUNKS}"
    ))?;
    let rows = statement.query_map(params_from_iter(bindings), read_hit)?;
    Ok((rows.collect::<Result<Vec<_>, _>>()?, total))
}

fn normalize(vector: &[f32], dimension: usize) -> Result<Vec<f32>, StoreError> {
    if vector.len() != dimension || vector.iter().any(|component| !component.is_finite()) {
        return Err(StoreError::InvalidInput("invalid embedding vector"));
    }
    let norm = vector
        .iter()
        .map(|component| f64::from(*component).powi(2))
        .sum::<f64>()
        .sqrt();
    if !norm.is_finite() || norm == 0.0 {
        return Err(StoreError::InvalidInput("invalid embedding norm"));
    }
    Ok(vector
        .iter()
        .map(|component| (f64::from(*component) / norm) as f32)
        .collect())
}

fn decode(blob: &[u8], dimension: usize) -> Option<Vec<f32>> {
    if blob.len() != dimension * 4 {
        return None;
    }
    let vector = blob
        .chunks_exact(4)
        .map(|bytes| f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
        .collect::<Vec<_>>();
    let norm = vector
        .iter()
        .map(|component| f64::from(*component).powi(2))
        .sum::<f64>();
    if vector.iter().any(|component| !component.is_finite()) || (norm - 1.0).abs() > 0.002 {
        return None;
    }
    Some(vector)
}

fn stored_vectors(
    statement: &mut rusqlite::Statement<'_>,
    citation: &str,
    profile: &EmbeddingProfile,
) -> Result<BTreeMap<usize, Vec<f32>>, StoreError> {
    let rows = statement.query_map(params![profile.key, citation], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, usize>(1)?,
            row.get::<_, Vec<u8>>(2)?,
        ))
    })?;
    let mut vectors = BTreeMap::new();
    for row in rows {
        let (window, dimension, blob) = row?;
        if dimension != profile.dimension {
            continue;
        }
        if let (Ok(window), Some(vector)) = (usize::try_from(window), decode(&blob, dimension)) {
            vectors.insert(window, vector);
        }
    }
    Ok(vectors)
}

fn keyword_candidates(
    connection: &Connection,
    corpus: &str,
    sources: &[String],
    query: &str,
    terms: &[String],
) -> Result<Vec<SearchHit>, StoreError> {
    let match_query = terms
        .iter()
        .map(|term| format!("\"{term}\""))
        .collect::<Vec<_>>()
        .join(" OR ");
    let placeholders = std::iter::repeat_n("?", sources.len())
        .collect::<Vec<_>>()
        .join(",");
    let mut bindings = vec![match_query.as_str(), corpus];
    bindings.extend(sources.iter().map(String::as_str));
    let mut statement = connection.prepare(&format!(
        "SELECT c.citation_id,c.source_id,c.document_id,d.relative_path,d.title,c.source_sha256,c.chunk_sha256,c.locator,c.text
         FROM chunks_fts JOIN chunks c ON c.id=chunks_fts.rowid
         JOIN documents d ON d.corpus_id=c.corpus_id AND d.source_id=c.source_id
           AND d.document_id=c.document_id AND d.source_sha256=c.source_sha256
         WHERE chunks_fts MATCH ? AND c.corpus_id=? AND c.source_id IN ({placeholders})
         ORDER BY bm25(chunks_fts,2.0,1.0),c.citation_id"
    ))?;
    let rows = statement.query_map(params_from_iter(bindings), read_hit)?;
    let mut protected = Vec::new();
    let mut ordinary = Vec::new();
    for row in rows {
        let mut hit = row?;
        hit.excerpt = excerpt(&hit.text, terms);
        if exact_anchor(query, &hit) {
            protected.push(hit);
            if protected.len() == MAX_SEARCH_HITS {
                break;
            }
        } else if ordinary.len() < MAX_SEARCH_HITS {
            ordinary.push(hit);
        }
    }
    let remaining = MAX_SEARCH_HITS - protected.len();
    protected.extend(ordinary.into_iter().take(remaining));
    Ok(protected)
}

#[derive(Debug)]
struct FusedHit {
    hit: SearchHit,
    score: f64,
    keyword_rank: usize,
    protected: bool,
}

fn fuse(query: &str, keyword: Vec<SearchHit>, semantic: Vec<(f64, SearchHit)>) -> Vec<SearchHit> {
    let mut candidates = BTreeMap::new();
    for (index, hit) in keyword.into_iter().enumerate() {
        let rank = index + 1;
        let protected = exact_anchor(query, &hit);
        candidates.insert(
            hit.citation_id.clone(),
            FusedHit {
                hit,
                score: 0.2 / (1 + rank) as f64,
                keyword_rank: rank,
                protected,
            },
        );
    }
    for (index, (_, hit)) in semantic.into_iter().enumerate() {
        let score = 0.8 / (index + 2) as f64;
        if let Some(candidate) = candidates.get_mut(&hit.citation_id) {
            candidate.score += score;
            if !candidate.protected {
                candidate.hit.excerpt = hit.excerpt;
            }
        } else {
            let protected = exact_anchor(query, &hit);
            candidates.insert(
                hit.citation_id.clone(),
                FusedHit {
                    hit,
                    score,
                    keyword_rank: usize::MAX,
                    protected,
                },
            );
        }
    }
    let mut ranked = candidates.into_values().collect::<Vec<_>>();
    ranked.sort_by(|a, b| {
        b.protected
            .cmp(&a.protected)
            .then_with(|| {
                if a.protected && b.protected {
                    a.keyword_rank.cmp(&b.keyword_rank)
                } else {
                    b.score.total_cmp(&a.score)
                }
            })
            .then_with(|| a.hit.citation_id.cmp(&b.hit.citation_id))
    });
    ranked.into_iter().map(|candidate| candidate.hit).collect()
}

fn exact_anchor(query: &str, hit: &SearchHit) -> bool {
    let title = hit.title.to_lowercase();
    let text = hit.text.to_lowercase();
    let matches =
        |anchor: &str| contains_literal(&title, anchor) || contains_literal(&text, anchor);
    if query.contains('"') {
        // Explicit quotation requests a contiguous phrase. Token presence, numeric
        // words, or capitalized fragments must never bypass that requirement.
        return query.split('"').enumerate().any(|(index, phrase)| {
            index % 2 == 1 && !phrase.trim().is_empty() && matches(&phrase.trim().to_lowercase())
        });
    }
    let plain = query.trim().trim_matches('"').to_lowercase();
    if !plain.is_empty() && matches(&plain) {
        return true;
    }
    query.split_whitespace().any(|raw| {
        let anchor = raw
            .trim_matches(|character: char| !character.is_alphanumeric())
            .to_lowercase();
        let numeric = anchor.chars().any(|character| character.is_numeric());
        let capitalized = raw.chars().next().is_some_and(char::is_uppercase)
            && !matches!(
                anchor.as_str(),
                "how"
                    | "what"
                    | "why"
                    | "where"
                    | "when"
                    | "which"
                    | "who"
                    | "explain"
                    | "find"
                    | "show"
                    | "search"
                    | "tell"
                    | "does"
                    | "do"
                    | "is"
                    | "are"
                    | "the"
                    | "for"
                    | "project"
                    | "document"
                    | "please"
                    | "a"
                    | "an"
            );
        !anchor.is_empty() && (numeric || capitalized) && matches(&anchor)
    })
}

fn contains_literal(text: &str, anchor: &str) -> bool {
    text.match_indices(anchor).any(|(start, _)| {
        !text[..start]
            .chars()
            .next_back()
            .is_some_and(char::is_alphanumeric)
            && !text[start + anchor.len()..]
                .chars()
                .next()
                .is_some_and(char::is_alphanumeric)
    })
}

pub(super) fn is_full(error: &rusqlite::Error) -> bool {
    error.sqlite_error_code() == Some(ErrorCode::DiskFull)
}

#[cfg(test)]
mod window_tests {
    use super::*;

    #[test]
    fn long_chunks_include_the_same_two_sentence_suffix_as_short_chunks() {
        let suffix = "Access can be ended after written notice. The stored credentials are removed within thirty days.";
        let text = format!(
            "{} {suffix}",
            "Monthly attendance records are filed for administrative review. ".repeat(44)
        );
        let short = window_ranges(suffix);
        assert_eq!(short.len(), 1);
        assert_eq!(short[0], 0..suffix.len());
        assert!(
            window_ranges(&text)
                .iter()
                .any(|range| text[range.clone()] == suffix[short[0].clone()]),
            "long chunks must retain a source-contiguous window for the complete short suffix"
        );
    }

    #[test]
    fn short_chunks_keep_the_original_single_untrimmed_span() {
        for text in [
            "  First sentence. Second sentence!  ".to_owned(),
            "é".repeat(192),
            "🦀".repeat(96),
        ] {
            let ranges = window_ranges(&text);
            assert_eq!(ranges.len(), 1, "{text:?}");
            assert_eq!(ranges[0], 0..text.len(), "{text:?}");
        }
        assert!(window_ranges("").is_empty());
        assert!(window_ranges(" \t\n ").is_empty());
    }

    #[test]
    fn repeated_sentences_and_pairs_have_one_exact_trimmed_window() {
        let text = "  Forms are archived.\t\n\n Forms are archived.  ".repeat(30);
        let ranges = window_ranges(&text);
        let windows = ranges
            .iter()
            .map(|range| text[range.clone()].trim())
            .collect::<Vec<_>>();
        assert_eq!(
            windows
                .iter()
                .filter(|window| **window == "Forms are archived.")
                .count(),
            1
        );
        assert_eq!(
            windows
                .iter()
                .filter(|window| **window == "Forms are archived.\t\n\n Forms are archived.")
                .count(),
            1
        );
        let unique = windows.iter().collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            unique.len(),
            windows.len(),
            "trimmed window text must be unique"
        );
        let sentence = ranges
            .iter()
            .find(|range| &text[range.start..range.end] == "Forms are archived.")
            .unwrap();
        assert_eq!(
            sentence.start, 2,
            "the first deterministic occurrence is retained"
        );
    }

    #[test]
    fn unicode_boundaries_and_long_sentences_keep_contiguous_bounded_coverage() {
        let long = (0..130)
            .map(|index| format!("🦀{index:03}é"))
            .collect::<String>();
        let suffix = "第一条。第二条！第三条？ Third clause? Fourth clause． Final clause？";
        let text = format!("{long}\n\n{suffix}");
        let ranges = window_ranges(&text);
        let mut covered = vec![false; text.len()];
        for range in &ranges {
            assert!(text.is_char_boundary(range.start));
            assert!(text.is_char_boundary(range.end));
            assert!(range.len() <= 384, "oversized range {range:?}");
            covered[range.clone()].fill(true);
        }
        assert!(
            covered.into_iter().all(|byte| byte),
            "raw windows must cover long sentences"
        );
        for sentence in [
            "第一条。",
            "第二条！",
            "第三条？",
            "Third clause?",
            "Fourth clause．",
            "Final clause？",
        ] {
            assert!(
                ranges.iter().any(|range| text[range.clone()] == *sentence),
                "missing standalone Unicode sentence {sentence:?}"
            );
        }
        assert!(ranges
            .iter()
            .any(|range| text[range.clone()] == *"第一条。第二条！"));
        assert!(ranges
            .iter()
            .any(|range| text[range.clone()] == *"Final clause？"));
        assert_eq!(
            window_ranges(&text),
            ranges,
            "window order must be deterministic"
        );
    }

    #[test]
    fn sentence_spans_keep_closing_quotes_and_unpunctuated_paragraphs() {
        let text = format!(
            "{}\n\nUnpunctuated paragraph\r\n \r\n\"A quoted sentence.\" Another sentence!",
            "background ".repeat(70)
        );
        let ranges = window_ranges(&text);
        for expected in [
            "Unpunctuated paragraph",
            "\"A quoted sentence.\"",
            "\"A quoted sentence.\" Another sentence!",
        ] {
            assert!(
                ranges.iter().any(|range| &text[range.clone()] == expected),
                "missing {expected:?}"
            );
        }
        let decimal = "A value of 2.5 applies! End.";
        let text = format!("{} {decimal}", "Background. ".repeat(60));
        assert!(window_ranges(&text)
            .iter()
            .any(|range| &text[range.clone()] == "A value of 2.5 applies!"));
    }

    #[test]
    fn excessive_optional_spans_are_spread_with_raw_coverage_and_both_tail_spans() {
        let text = (0..100)
            .map(|index| format!("Record {index:03} is archived. "))
            .collect::<String>();
        let ranges = window_ranges(&text);
        assert_eq!(
            ranges.len(),
            32,
            "optional sentences must not amplify a chunk past its cap"
        );
        let mut covered = vec![false; text.len()];
        for range in &ranges {
            assert!(range.len() <= 384);
            covered[range.clone()].fill(true);
        }
        assert!(
            covered.into_iter().all(|byte| byte),
            "optional sampling must retain raw coverage"
        );
        for expected in [
            "Record 000 is archived.",
            "Record 098 is archived. Record 099 is archived.",
            "Record 099 is archived.",
        ] {
            assert!(
                ranges.iter().any(|range| text[range.clone()] == *expected),
                "missing {expected:?}"
            );
        }
        assert!(
            ranges.iter().any(|range| {
                range.start > text.len() / 3 && range.end < text.len() * 2 / 3 && range.len() < 100
            }),
            "optional spans should sample the middle as well as the ends"
        );
        assert_eq!(window_ranges(&text), ranges);
    }
}
