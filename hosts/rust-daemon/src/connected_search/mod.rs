//! Owner-enabled native account snapshots and cited bounded search.
mod attachments;
mod policy;
pub(crate) mod providers;
mod service;
#[cfg(test)]
mod tests;

use crate::state::DaemonState;
pub(crate) use attachments::{AttachmentBatch, RawAttachment};
pub(crate) use policy::{
    AttachmentTextMode, HistoryCoverage, HistoryDepth, HistoryPage, HistoryPageRequest,
    HistoryProgress, HistoryStatus, ImportCursor, SearchSettings,
};
use serde::{Deserialize, Serialize};
#[cfg(test)]
pub(crate) use service::SourceReader;
pub(crate) use service::{SearchService, SourceSnapshot, WeakSearchService};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};

pub(crate) const MAX_TEXT_BYTES: usize = 8 * 1024;
pub(crate) const MAX_INDEX_BYTES: usize = 16 * 1024 * 1024;
pub(crate) const MAX_SEEN_ITEMS: usize = 4096;
pub(crate) const MAX_SOURCES: usize = 128;
pub(crate) const POLL_INTERVAL_MS: u64 = 120_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) enum DocumentKind {
    File,
    Mail,
    Event,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct IndexedDocument {
    pub(crate) citation_id: String,
    pub(crate) external_id: String,
    pub(crate) collection_id: String,
    pub(crate) kind: DocumentKind,
    pub(crate) title: String,
    pub(crate) text: String,
    pub(crate) sender: Option<String>,
    pub(crate) source_url: Option<String>,
    pub(crate) source_modified_at: Option<String>,
    pub(crate) event_start: Option<String>,
    pub(crate) event_end: Option<String>,
    pub(crate) revision: String,
    pub(crate) truncated: bool,
    pub(crate) indexed_at_ms: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct SourceAuthority {
    pub(crate) agent_id: String,
    pub(crate) connector_id: String,
    pub(crate) provider: String,
    pub(crate) generation: String,
    pub(crate) enabled: bool,
    pub(crate) read_granted: bool,
    pub(crate) sync_started_at_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum SourceChangeKind {
    Observed,
    Changed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IngestionOrigin {
    LivePoll,
    HistoryBackfill,
    ContentEnrichment,
}

#[derive(Clone, Debug)]
pub(crate) struct SourceChange {
    pub(crate) id: String,
    pub(crate) connector_id: String,
    pub(crate) generation: String,
    pub(crate) provider: String,
    pub(crate) kind: SourceChangeKind,
    pub(crate) origin: IngestionOrigin,
    pub(crate) document: IndexedDocument,
    pub(crate) observed_at_ms: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SearchRegistry {
    pub(crate) sources: HashMap<String, SourceIndex>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SourceIndex {
    pub(crate) agent_id: String,
    pub(crate) generation: String,
    pub(crate) enabled: bool,
    pub(crate) grant_epoch: String,
    pub(crate) stream_epoch: Option<String>,
    pub(crate) documents: Vec<IndexedDocument>,
    /// Bounded independent history prevents retained-window reentry firing again.
    pub(crate) seen: BTreeMap<String, String>,
    #[serde(default)]
    pub(crate) settings: SearchSettings,
    #[serde(default = "policy::default_settings_revision")]
    pub(crate) settings_revision: u64,
    #[serde(default)]
    pub(crate) history: Option<HistoryProgress>,
    /// Historical text is independent of the workflow live-observation window.
    #[serde(default)]
    pub(crate) historical: BTreeMap<String, IndexedDocument>,
    /// Receipts remain after historical text is removed to prevent replay.
    #[serde(default)]
    pub(crate) imported: BTreeMap<String, String>,
    #[serde(default)]
    pub(crate) verified_membership: BTreeMap<String, String>,
    /// Runtime-only proof invalidations; restored Microsoft content starts unverified.
    #[serde(skip)]
    pub(crate) microsoft_denial_fences: BTreeMap<String, String>,
    #[serde(default)]
    pub(crate) attachments: BTreeMap<String, attachments::AttachmentEntry>,
    #[serde(default)]
    pub(crate) attachment_scanned: BTreeMap<String, String>,
    #[serde(default)]
    pub(crate) attachment_retries: BTreeMap<String, attachments::AttachmentRetry>,
    #[serde(default)]
    pub(crate) attachment_omitted: BTreeMap<String, usize>,
    #[serde(default)]
    pub(crate) attachment_error: Option<String>,
    #[serde(default)]
    pub(crate) live_event_fence_at_ms: Option<u64>,
    pub(crate) baseline_at_ms: Option<u64>,
    pub(crate) last_attempt_at_ms: Option<u64>,
    pub(crate) last_success_at_ms: Option<u64>,
    pub(crate) error: Option<String>,
}
impl SourceIndex {
    pub(crate) fn new(source: &SourceAuthority) -> Self {
        Self {
            agent_id: source.agent_id.clone(),
            generation: source.generation.clone(),
            enabled: true,
            grant_epoch: uuid::Uuid::new_v4().to_string(),
            stream_epoch: None,
            documents: vec![],
            seen: BTreeMap::new(),
            settings: SearchSettings::default(),
            settings_revision: policy::default_settings_revision(),
            history: None,
            historical: BTreeMap::new(),
            imported: BTreeMap::new(),
            verified_membership: BTreeMap::new(),
            microsoft_denial_fences: BTreeMap::new(),
            attachments: BTreeMap::new(),
            attachment_scanned: BTreeMap::new(),
            attachment_retries: BTreeMap::new(),
            attachment_omitted: BTreeMap::new(),
            attachment_error: None,
            live_event_fence_at_ms: None,
            baseline_at_ms: None,
            last_attempt_at_ms: None,
            last_success_at_ms: None,
            error: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SourceView {
    pub(crate) connector_id: String,
    pub(crate) provider: String,
    pub(crate) kind: DocumentKind,
    pub(crate) account_label: Option<String>,
    pub(crate) generation: String,
    pub(crate) available: bool,
    pub(crate) enabled: bool,
    pub(crate) status: String,
    pub(crate) scope: String,
    pub(crate) item_limit: usize,
    pub(crate) item_count: usize,
    pub(crate) last_attempt_at_ms: Option<u64>,
    pub(crate) last_success_at_ms: Option<u64>,
    pub(crate) error: Option<String>,
    pub(crate) settings_revision: u64,
    pub(crate) settings: SearchSettings,
    pub(crate) history_coverage: HistoryCoverage,
    pub(crate) capabilities: SourceCapabilities,
    pub(crate) attachment_coverage: AttachmentCoverage,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AttachmentCoverage {
    pub(crate) indexed: usize,
    pub(crate) partial: usize,
    pub(crate) omitted: usize,
    pub(crate) failed: usize,
    pub(crate) pending: usize,
    pub(crate) error: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SourceCapabilities {
    pub(crate) history: bool,
    pub(crate) attachment_formats: Vec<String>,
    pub(crate) workflow_event_kinds: Vec<String>,
}
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SourcesResponse {
    pub(crate) sources: Vec<SourceView>,
    pub(crate) poll_interval_ms: u64,
}
#[derive(Clone, Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SourceInput {
    pub(crate) enabled: bool,
    pub(crate) expected_generation: String,
    #[serde(default)]
    pub(crate) expected_settings_revision: Option<u64>,
    #[serde(default)]
    pub(crate) settings: Option<SearchSettings>,
}
#[derive(Clone, Debug, Default, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RefreshInput {
    pub(crate) connector_id: Option<String>,
    #[serde(default)]
    pub(crate) work: RefreshWork,
}
#[derive(Clone, Copy, Debug, Default, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) enum RefreshWork {
    #[default]
    Live,
    History,
    All,
}
#[derive(Clone, Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SearchQuery {
    pub(crate) query: String,
    #[serde(default)]
    pub(crate) connector_ids: Vec<String>,
    #[serde(default)]
    pub(crate) kinds: Vec<DocumentKind>,
    pub(crate) limit: Option<usize>,
}
#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SearchHit {
    pub(crate) citation_id: String,
    pub(crate) connector_id: String,
    pub(crate) provider: String,
    pub(crate) kind: DocumentKind,
    pub(crate) title: String,
    pub(crate) snippet: String,
    pub(crate) source_url: Option<String>,
    pub(crate) citation_path: String,
    pub(crate) indexed_at_ms: u64,
    pub(crate) source_modified_at: Option<String>,
    pub(crate) truncated: bool,
    #[serde(default)]
    pub(crate) content_role: Option<String>,
    #[serde(default)]
    pub(crate) parent_citation_id: Option<String>,
    #[serde(default)]
    pub(crate) extraction_status: Option<String>,
    #[serde(default)]
    pub(crate) omissions: Vec<String>,
    #[serde(default)]
    pub(crate) locators: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SearchResponse {
    pub(crate) hits: Vec<SearchHit>,
    pub(crate) sources: Vec<SourceView>,
    #[serde(default)]
    pub(crate) omitted_source_count: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SearchItem {
    #[serde(flatten)]
    pub(crate) hit: SearchHit,
    pub(crate) text: String,
    pub(crate) sender: Option<String>,
    pub(crate) event_start: Option<String>,
    pub(crate) event_end: Option<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SearchError {
    Invalid,
    NotFound,
    Conflict,
    Unavailable,
    Capacity,
    Persistence,
    UnsupportedSetting,
}
impl std::fmt::Display for SearchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Invalid => "Connected search request is invalid",
            Self::NotFound => "Connected source or citation was not found",
            Self::Conflict => "Source or search settings changed; refresh and try again",
            Self::Unavailable => "Connected source is unavailable or needs renewed permission",
            Self::Capacity => "Connected search capacity reached; reduce retained content",
            Self::Persistence => "Connected search could not be saved",
            Self::UnsupportedSetting => {
                "This account does not support the requested search setting"
            }
        })
    }
}

fn digest(parts: &[&str]) -> String {
    let mut h = Sha256::new();
    for part in parts {
        h.update((part.len() as u64).to_be_bytes());
        h.update(part.as_bytes());
    }
    hex::encode(h.finalize())
}
fn clean(value: &str, max: usize) -> String {
    value
        .chars()
        .filter(|c| !c.is_control() || matches!(*c, '\n' | '\t'))
        .take(max)
        .collect()
}
fn clip_bytes(value: &str, max: usize) -> String {
    let mut end = value.len().min(max);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}
fn opaque(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}
fn normalize(
    mut d: IndexedDocument,
    a: &SourceAuthority,
    now: u64,
) -> Result<IndexedDocument, SearchError> {
    if !opaque(&d.external_id, 1024) || !opaque(&d.collection_id, 512) {
        return Err(SearchError::Invalid);
    }
    d.title = clean(&d.title, 512);
    d.truncated |= d.text.len() > MAX_TEXT_BYTES;
    d.text = clean(&d.text, MAX_TEXT_BYTES);
    if d.text.len() > MAX_TEXT_BYTES {
        d.truncated = true;
        d.text = clip_bytes(&d.text, MAX_TEXT_BYTES);
    }
    d.sender = d.sender.map(|s| clean(&s, 512));
    for value in [&d.source_modified_at, &d.event_start, &d.event_end]
        .into_iter()
        .flatten()
    {
        if !opaque(value, 128) {
            return Err(SearchError::Invalid);
        }
    }
    if d.source_url
        .as_ref()
        .is_some_and(|url| !safe_source_url(&a.provider, url))
    {
        d.source_url = None;
    }
    d.citation_id = digest(&[
        &a.agent_id,
        &a.connector_id,
        &a.generation,
        &d.collection_id,
        &d.external_id,
    ]);
    d.revision = digest(&[
        &d.revision,
        &d.title,
        &d.text,
        d.sender.as_deref().unwrap_or(""),
        d.source_modified_at.as_deref().unwrap_or(""),
        d.event_start.as_deref().unwrap_or(""),
        d.event_end.as_deref().unwrap_or(""),
    ]);
    d.indexed_at_ms = now;
    Ok(d)
}
pub(crate) fn safe_source_url(provider: &str, value: &str) -> bool {
    if value.len() > 2048 || value.chars().any(char::is_control) {
        return false;
    }
    let Ok(url) = reqwest::Url::parse(value) else {
        return false;
    };
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && match provider {
            "gdrive" => matches!(url.host_str(), Some("drive.google.com" | "docs.google.com")),
            "gmail" => url.host_str() == Some("mail.google.com"),
            "outlook" => matches!(
                url.host_str(),
                Some("outlook.office.com" | "outlook.office365.com" | "outlook.live.com")
            ),
            "gcalendar" => matches!(
                url.host_str(),
                Some("www.google.com" | "calendar.google.com")
            ),
            "onedrive" => matches!(url.host_str(), Some("onedrive.live.com" | "1drv.ms")),
            "sharepoint" => url
                .host_str()
                .is_some_and(|host| host.ends_with(".sharepoint.com")),
            _ => false,
        }
}
fn apply_snapshot(
    source: &mut SourceIndex,
    a: &SourceAuthority,
    documents: Vec<IndexedDocument>,
    now: u64,
) -> Result<Vec<SourceChange>, SearchError> {
    let limit = match a.provider.as_str() {
        "gdrive" | "onedrive" | "sharepoint" => 16,
        "gcalendar" => 500,
        _ => 100,
    };
    if documents.len() > limit {
        return Err(SearchError::Capacity);
    }
    let mut next = source.clone();
    let mut ids = HashSet::new();
    let mut changes = vec![];
    let baseline = next.baseline_at_ms.is_none();
    next.documents = documents
        .into_iter()
        .map(|d| normalize(d, a, now))
        .collect::<Result<Vec<_>, _>>()?;
    for d in &next.documents {
        if !ids.insert(d.citation_id.clone()) {
            return Err(SearchError::Invalid);
        }
        let kind = match next.seen.get(&d.citation_id) {
            Some(rev) if rev == &d.revision => None,
            Some(_) => Some(SourceChangeKind::Changed),
            None => Some(SourceChangeKind::Observed),
        };
        let imported = next.imported.contains_key(&d.citation_id);
        let before_fence = next.live_event_fence_at_ms.is_some_and(|fence| {
            d.source_modified_at
                .as_deref()
                .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
                .and_then(|value| u64::try_from(value.timestamp_millis()).ok())
                .is_none_or(|received| received < fence)
        });
        let suppress_old_mail = d.kind == DocumentKind::Mail
            && kind == Some(SourceChangeKind::Observed)
            && (imported || before_fence);
        if !baseline
            && !suppress_old_mail
            && !matches!(a.provider.as_str(), "onedrive" | "sharepoint")
        {
            if let Some(kind) = kind {
                changes.push(SourceChange {
                    id: digest(&[
                        &d.citation_id,
                        &d.revision,
                        match kind {
                            SourceChangeKind::Observed => "observed",
                            SourceChangeKind::Changed => "changed",
                        },
                    ]),
                    connector_id: a.connector_id.clone(),
                    generation: a.generation.clone(),
                    provider: a.provider.clone(),
                    kind,
                    origin: IngestionOrigin::LivePoll,
                    document: d.clone(),
                    observed_at_ms: now,
                });
            }
        }
        next.seen.insert(d.citation_id.clone(), d.revision.clone());
    }
    if next.seen.len() > MAX_SEEN_ITEMS {
        return Err(SearchError::Capacity);
    }
    for document in &next.documents {
        if let Some(historical) = next.historical.get_mut(&document.citation_id) {
            *historical = document.clone();
        }
    }
    retain_enrichment(&mut next);
    next.baseline_at_ms = Some(now);
    next.last_attempt_at_ms = Some(a.sync_started_at_ms);
    next.last_success_at_ms = Some(now);
    next.error = None;
    *source = next;
    Ok(changes)
}

/// Merge a history page without touching the live observation or workflow state.
fn apply_history_page(
    source: &mut SourceIndex,
    authority: &SourceAuthority,
    page: HistoryPage,
    now: u64,
) -> Result<(), SearchError> {
    ensure_silent_origin(IngestionOrigin::HistoryBackfill)?;
    if page.documents.len() > policy::MAX_HISTORY_PAGE_ITEMS
        || page.exhausted != page.next_cursor.is_none()
        || page
            .next_cursor
            .as_ref()
            .is_some_and(|cursor| !cursor.valid_for(&authority.provider))
    {
        return Err(SearchError::Invalid);
    }
    let mut next = source.clone();
    let progress = next.history.as_ref().ok_or(SearchError::Conflict)?;
    let from = progress
        .coverage
        .requested_from_ms
        .ok_or(SearchError::Invalid)?;
    let before = progress
        .coverage
        .requested_before_ms
        .ok_or(SearchError::Invalid)?;
    let mut batch_ids = HashSet::new();
    let count = page.documents.len();
    for document in page.documents {
        if document.kind != DocumentKind::Mail || document.collection_id != "inbox" {
            return Err(SearchError::Invalid);
        }
        let received = document
            .source_modified_at
            .as_deref()
            .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
            .and_then(|value| u64::try_from(value.timestamp_millis()).ok())
            .ok_or(SearchError::Invalid)?;
        if received < from || received >= before {
            return Err(SearchError::Invalid);
        }
        let document = normalize(document, authority, now)?;
        if !batch_ids.insert(document.citation_id.clone()) {
            return Err(SearchError::Invalid);
        }
        if !next.imported.contains_key(&document.citation_id)
            && next.imported.len() >= policy::MAX_IMPORTED_ITEMS
        {
            return Err(SearchError::Capacity);
        }
        next.imported
            .insert(document.citation_id.clone(), document.revision.clone());
        let stored = next
            .documents
            .iter()
            .find(|live| live.citation_id == document.citation_id)
            .cloned()
            .unwrap_or(document);
        next.historical.insert(stored.citation_id.clone(), stored);
    }
    let mut retained: HashSet<&str> = next
        .documents
        .iter()
        .map(|d| d.citation_id.as_str())
        .collect();
    retained.extend(next.historical.keys().map(String::as_str));
    if retained.len() > usize::from(next.settings.retained_item_limit) {
        return Err(SearchError::Capacity);
    }
    let progress = next.history.as_mut().ok_or(SearchError::Conflict)?;
    let fingerprint = page
        .next_cursor
        .as_ref()
        .map(|cursor| {
            serde_json::to_string(cursor)
                .map(|value| digest(&["history-cursor", &value]))
                .map_err(|_| SearchError::Invalid)
        })
        .transpose()?;
    let cycle = fingerprint
        .as_ref()
        .is_some_and(|value| progress.cursor_fingerprints.contains(value));
    let limit =
        fingerprint.is_some() && progress.cursor_fingerprints.len() >= policy::MAX_HISTORY_CURSORS;
    if let Some(value) = fingerprint.filter(|_| !cycle && !limit) {
        progress.cursor_fingerprints.push(value);
    }
    progress.cursor = if cycle || limit {
        None
    } else {
        page.next_cursor
    };
    progress.coverage.state = if cycle || limit {
        HistoryStatus::Partial
    } else if page.exhausted {
        HistoryStatus::Complete
    } else {
        HistoryStatus::Pending
    };
    progress.coverage.reason = if cycle {
        Some("paginationCycle".into())
    } else if limit {
        Some("paginationLimit".into())
    } else {
        None
    };
    progress.coverage.enumerated_count = progress.coverage.enumerated_count.saturating_add(count);
    progress.coverage.retained_count = next.historical.len();
    progress.coverage.oldest_retained_at = next
        .historical
        .values()
        .filter_map(|d| d.source_modified_at.clone())
        .min();
    progress.coverage.last_attempt_at_ms = Some(now);
    progress.coverage.last_success_at_ms = Some(now);
    retain_enrichment(&mut next);
    if serde_json::to_vec(&next)
        .map_err(|_| SearchError::Invalid)?
        .len()
        > 2 * 1024 * 1024
    {
        return Err(SearchError::Capacity);
    }
    *source = next;
    Ok(())
}

fn ensure_silent_origin(origin: IngestionOrigin) -> Result<(), SearchError> {
    match origin {
        IngestionOrigin::HistoryBackfill | IngestionOrigin::ContentEnrichment => Ok(()),
        IngestionOrigin::LivePoll => Err(SearchError::Invalid),
    }
}

pub(crate) fn retain_enrichment(source: &mut SourceIndex) {
    let mut revisions: HashMap<&str, &str> = source
        .historical
        .values()
        .map(|document| (document.citation_id.as_str(), document.revision.as_str()))
        .collect();
    revisions.extend(
        source
            .documents
            .iter()
            .map(|document| (document.citation_id.as_str(), document.revision.as_str())),
    );
    source.attachment_scanned.retain(|id, revision| {
        revisions
            .get(id.as_str())
            .is_some_and(|current| *current == revision.as_str())
    });
    source.attachment_retries.retain(|id, retry| {
        revisions
            .get(id.as_str())
            .is_some_and(|current| *current == retry.revision.as_str())
    });
    let scanned = &source.attachment_scanned;
    source
        .attachment_omitted
        .retain(|id, _| scanned.contains_key(id));
    source.attachments.retain(|_, entry| {
        revisions
            .get(entry.parent_citation_id.as_str())
            .is_some_and(|revision| *revision == entry.parent_revision.as_str())
    });
    source
        .verified_membership
        .retain(|id, _| revisions.contains_key(id.as_str()));
}

fn visible_in_instance(
    source: &SourceIndex,
    citation: &str,
    instance: Option<&str>,
    microsoft: bool,
) -> bool {
    let Some(instance) = instance else {
        return !microsoft;
    };
    if microsoft
        || source.imported.contains_key(citation)
        || source.historical.contains_key(citation)
    {
        source
            .verified_membership
            .get(citation)
            .is_some_and(|verified| verified == instance)
    } else {
        true
    }
}

fn mark_instance_coverage(state: &DaemonState, views: &mut [SourceView], instance: &str) {
    for view in views {
        let Some(index) = state.connected_search.sources.get(&view.connector_id) else {
            continue;
        };
        let microsoft = matches!(view.provider.as_str(), "onedrive" | "sharepoint");
        let waiting = index
            .historical
            .keys()
            .filter(|id| !visible_in_instance(index, id, Some(instance), false))
            .count();
        if waiting > 0 {
            view.history_coverage.state = HistoryStatus::Partial;
            view.history_coverage.reason = Some("needsVerification".into());
        }
        if microsoft {
            if !view.enabled {
                view.item_count = 0;
                continue;
            }
            let visible = index
                .documents
                .iter()
                .filter(|document| {
                    visible_in_instance(index, &document.citation_id, Some(instance), true)
                })
                .count();
            let children = index
                .attachments
                .values()
                .filter(|entry| {
                    visible_in_instance(index, &entry.parent_citation_id, Some(instance), true)
                })
                .count();
            if visible < index.documents.len() {
                view.status = if index.error.is_some() {
                    "stale"
                } else {
                    "syncing"
                }
                .into();
                if view.error.is_none() {
                    view.error = Some("Checking access to selected Microsoft files".into());
                }
            }
            view.item_count = visible + children;
        }
    }
}

pub(crate) fn source_authority(s: &DaemonState, agent: &str, id: &str) -> Option<SourceAuthority> {
    let runtime = s.agents.get(agent)?;
    if s.microsoft_file_records.contains_key(id)
        && (s.drive_records.contains_key(id)
            || s.mail_records.contains_key(id)
            || s.calendar_connectors.contains_key(id))
    {
        return None;
    }
    let (provider, generation, active, tool) = if let Some(r) =
        s.drive_records.get(id).filter(|r| r.agent_id == agent)
    {
        (
            "gdrive".to_string(),
            digest(&[
                &r.generation,
                &serde_json::to_string(&r.selected_file_ids).ok()?,
            ]),
            r.status == crate::connectors::drive::model::DriveStatus::Active,
            "drive_read",
        )
    } else if let Some(r) = s
        .microsoft_file_records
        .get(id)
        .filter(|record| record.agent_id == agent)
    {
        (
            match r.mode {
                crate::connectors::microsoft_files::MicrosoftFileMode::OneDrive => "onedrive",
                crate::connectors::microsoft_files::MicrosoftFileMode::SharePoint => "sharepoint",
            }
            .to_string(),
            digest(&[&r.generation, &r.id]),
            r.status == crate::connectors::microsoft_files::MicrosoftFileStatus::Active,
            "microsoft_files_read",
        )
    } else if let Some(r) = s
        .mail_records
        .get(id)
        .filter(|r| r.connector.agent_id == agent && !r.deleted)
    {
        (
            r.connector.provider.name().into(),
            digest(&[&r.auth_generation, &r.connector.id]),
            r.connector.status == "active",
            "mail_list_messages",
        )
    } else if let Some(r) = s
        .calendar_connectors
        .get(id)
        .filter(|r| r.agent_id == agent && r.deleted_at_ms.is_none())
    {
        (
            "gcalendar".into(),
            digest(&[
                &r.authorization_generation,
                &serde_json::to_string(&r.calendar_ids).ok()?,
            ]),
            r.is_active() && !r.reauth_required,
            "calendar_list_events",
        )
    } else {
        return None;
    };
    let read_granted = active
        && runtime.config().allows_tool(tool)
        && !crate::agent_runs::is_helper_config(runtime.config());
    let enabled = read_granted
        && s.connected_search
            .sources
            .get(id)
            .is_some_and(|i| i.enabled && i.agent_id == agent && i.generation == generation);
    Some(SourceAuthority {
        agent_id: agent.into(),
        connector_id: id.into(),
        provider,
        generation,
        enabled,
        read_granted,
        sync_started_at_ms: 0,
    })
}
pub(crate) fn source_views(s: &DaemonState, agent: &str) -> Result<Vec<SourceView>, SearchError> {
    if s.get_agent(agent).is_none() {
        return Err(SearchError::NotFound);
    }
    let mut ids = s
        .drive_records
        .values()
        .filter(|r| {
            r.agent_id == agent
                && r.status != crate::connectors::drive::model::DriveStatus::Disconnected
        })
        .map(|r| r.id.clone())
        .chain(
            s.microsoft_file_records
                .values()
                .filter(|record| {
                    record.agent_id == agent
                        && record.status
                            != crate::connectors::microsoft_files::MicrosoftFileStatus::Disconnected
                })
                .map(|record| record.id.clone()),
        )
        .chain(
            s.mail_records
                .values()
                .filter(|r| r.connector.agent_id == agent && !r.deleted)
                .map(|r| r.connector.id.clone()),
        )
        .chain(
            s.calendar_connectors
                .values()
                .filter(|r| r.agent_id == agent && r.deleted_at_ms.is_none())
                .map(|r| r.id.clone()),
        )
        .collect::<Vec<_>>();
    ids.sort();
    Ok(ids.into_iter().filter_map(|id| {
        let a=source_authority(s,agent,&id)?; let index=s.connected_search.sources.get(&id).filter(|i|i.generation==a.generation&&i.agent_id==agent);
        let (kind,label,scope,limit)=match a.provider.as_str() {
            "gdrive"=>(DocumentKind::File,s.drive_records.get(&id)?.account_label.clone(),"Up to 16 owner-selected Drive files; Google Docs/plain text content, other formats metadata; 8 KiB per item",16),
            "onedrive" | "sharepoint" => (DocumentKind::File, s.microsoft_file_records.get(&id)?.account_label.clone(), "Up to 16 selected files; document text requires explicit opt-in and supported device", 16),
            "gcalendar"=>(DocumentKind::Event,s.calendar_connectors.get(&id)?.account_label.clone(),"Up to 500 events across selected calendars; 30 days past through 180 days ahead; 8 KiB per item",500),
            _=>(DocumentKind::Mail,s.mail_records.get(&id)?.connector.account_label.clone(),"Up to 100 recent inbox messages (IMAP up to 25); 8 KiB per item; polling can miss activity outside retention",100),
        };
        let error=if !a.read_granted {Some("Reconnect this account and grant its native read tool before enabling indexed search".into())}else{index.and_then(|i|i.error.clone())}; let status=if !a.read_granted {"unavailable"} else if !a.enabled {"disabled"} else if error.is_some()||index.and_then(|i|i.last_success_at_ms).is_some_and(|t|crate::connectors::gcalendar::now_ms().saturating_sub(t)>POLL_INTERVAL_MS*3){"stale"} else if index.and_then(|i|i.last_success_at_ms).is_none(){"syncing"}else{"ready"};
        let supports_history = matches!(a.provider.as_str(), "gmail" | "outlook");
        let capabilities = SourceCapabilities {
            history: supports_history,
            attachment_formats: if matches!(a.provider.as_str(), "gmail" | "outlook" | "gdrive" | "onedrive" | "sharepoint") { vec!["text".into(), "csv".into(), "pdf".into(), "docx".into(), "xlsx".into()] } else { vec![] },
            workflow_event_kinds: if matches!(a.provider.as_str(), "onedrive" | "sharepoint") { vec![] } else { vec![match kind { DocumentKind::Mail => "mailObserved", DocumentKind::File => "fileChanged", DocumentKind::Event => "calendarChanged" }.into()] },
        };
        let history_coverage = index.and_then(|i| i.history.as_ref()).map(|h| h.coverage.clone()).unwrap_or_else(|| HistoryCoverage { state: if supports_history { HistoryStatus::NotRequested } else { HistoryStatus::Unsupported }, ..Default::default() });
        let item_count = if a.enabled { index.map_or(0, |i| i.documents.len() + i.historical.keys().filter(|id| !i.documents.iter().any(|d| &d.citation_id == *id)).count() + i.attachments.len()) } else { 0 };
        let mut attachment_coverage = AttachmentCoverage::default();
        if let Some(index) = index {
            attachment_coverage.omitted = index.attachment_omitted.values().copied().sum();
            attachment_coverage.error = index.attachment_error.clone();
            for attachment in index.attachments.values() {
                match attachment.status.as_str() {
                    "ready" => attachment_coverage.indexed += 1,
                    "partial" => attachment_coverage.partial += 1,
                    "omitted" | "unsupported" => attachment_coverage.omitted += 1,
                    _ => attachment_coverage.failed += 1,
                }
            }
            if index.settings.attachment_text == AttachmentTextMode::SupportedText && matches!(a.provider.as_str(), "gmail" | "outlook" | "gdrive" | "onedrive" | "sharepoint") {
                let mut parents = HashSet::new();
                for document in index.documents.iter().chain(index.historical.values()) {
                    if (a.provider != "gdrive" || document.text.is_empty()) && parents.insert(&document.citation_id) && index.attachment_scanned.get(&document.citation_id) != Some(&document.revision) {
                        attachment_coverage.pending += 1;
                    }
                }
            }
        }
        let (scope, limit) = index.and_then(|index| index.settings.history_depth.days().map(|days| (format!("Inbox from the last {days} days; up to {} messages, subject to local storage limits; polling can miss activity", index.settings.retained_item_limit), usize::from(index.settings.retained_item_limit)))).unwrap_or_else(|| (scope.into(), limit));
        Some(SourceView{connector_id:id,provider:a.provider,kind,account_label:label,generation:a.generation,available:a.read_granted,enabled:a.enabled,status:status.into(),scope,item_limit:limit,item_count,last_attempt_at_ms:index.and_then(|i|i.last_attempt_at_ms),last_success_at_ms:index.and_then(|i|i.last_success_at_ms),error,settings_revision:index.map_or(1,|i|i.settings_revision),settings:index.map(|i|i.settings.clone()).unwrap_or_default(),history_coverage,capabilities,attachment_coverage})
    }).collect())
}
fn citation_path(agent: &str, id: &str) -> String {
    let agent = reqwest::Url::parse("http://localhost")
        .ok()
        .map(|mut u| {
            u.path_segments_mut().expect("constant URL").push(agent);
            u.path().trim_start_matches('/').to_string()
        })
        .unwrap_or_default();
    format!("/api/agents/{agent}/connected-search/items/{id}")
}
fn hit(a: &SourceAuthority, d: &IndexedDocument) -> SearchHit {
    SearchHit {
        citation_id: d.citation_id.clone(),
        connector_id: a.connector_id.clone(),
        provider: a.provider.clone(),
        kind: d.kind,
        title: d.title.clone(),
        snippet: d.text.chars().take(320).collect(),
        source_url: d.source_url.clone(),
        citation_path: citation_path(&a.agent_id, &d.citation_id),
        indexed_at_ms: d.indexed_at_ms,
        source_modified_at: d.source_modified_at.clone(),
        truncated: d.truncated,
        content_role: Some(
            match d.kind {
                DocumentKind::Mail => "body",
                DocumentKind::File => "file",
                DocumentKind::Event => "event",
            }
            .into(),
        ),
        parent_citation_id: None,
        extraction_status: None,
        omissions: vec![],
        locators: vec![],
    }
}
fn attachment_hit(a: &SourceAuthority, entry: &attachments::AttachmentEntry) -> SearchHit {
    let mut hit = hit(a, &entry.document);
    hit.content_role = Some(
        if entry.document.kind == DocumentKind::File {
            "documentText"
        } else {
            "attachment"
        }
        .into(),
    );
    hit.parent_citation_id = Some(entry.parent_citation_id.clone());
    hit.extraction_status = Some(entry.status.clone());
    hit.omissions = entry.omissions.clone();
    hit.locators = entry.locators.clone();
    hit
}
pub(crate) fn query(
    s: &DaemonState,
    agent: &str,
    q: &SearchQuery,
    instance: Option<&str>,
) -> Result<SearchResponse, SearchError> {
    let terms = q
        .query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(str::to_lowercase)
        .collect::<Vec<_>>();
    if q.query.len() > 1024
        || terms.is_empty()
        || terms.len() > 32
        || q.connector_ids.len() > MAX_SOURCES
        || q.kinds.len() > 3
        || q.limit.is_some_and(|l| l == 0 || l > 30)
    {
        return Err(SearchError::Invalid);
    }
    let mut sources = source_views(s, agent)?;
    if let Some(instance) = instance {
        mark_instance_coverage(s, &mut sources, instance);
    }
    let mut hits = vec![];
    for (id, index) in &s.connected_search.sources {
        let Some(a) = source_authority(s, agent, id).filter(|a| a.enabled) else {
            continue;
        };
        if !q.connector_ids.is_empty() && !q.connector_ids.contains(id) {
            continue;
        }
        let parent_documents = index
            .documents
            .iter()
            .chain(index.historical.values().filter(|historical| {
                !index
                    .documents
                    .iter()
                    .any(|live| live.citation_id == historical.citation_id)
            }))
            .map(|document| (document, None));
        let attachment_documents = index
            .attachments
            .values()
            .map(|entry| (&entry.document, Some(entry)));
        for (d, attachment) in parent_documents.chain(attachment_documents) {
            let parent_id = attachment.map_or(d.citation_id.as_str(), |entry| {
                entry.parent_citation_id.as_str()
            });
            if !visible_in_instance(
                index,
                parent_id,
                instance,
                matches!(a.provider.as_str(), "onedrive" | "sharepoint"),
            ) {
                continue;
            }
            if !q.kinds.is_empty() && !q.kinds.contains(&d.kind) {
                continue;
            }
            let title = d.title.to_lowercase();
            let text = format!(
                "{} {} {}",
                d.title,
                d.text,
                d.sender.as_deref().unwrap_or("")
            )
            .to_lowercase();
            let words = text
                .split(|c: char| !c.is_alphanumeric())
                .filter(|w| !w.is_empty())
                .collect::<HashSet<_>>();
            if terms.iter().all(|t| words.contains(t.as_str())) {
                let score = terms
                    .iter()
                    .map(|t| {
                        if title.split(|c: char| !c.is_alphanumeric()).any(|w| w == t) {
                            3
                        } else {
                            1
                        }
                    })
                    .sum::<usize>();
                let source_time = d
                    .source_modified_at
                    .as_deref()
                    .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
                    .map(|value| value.timestamp_millis())
                    .unwrap_or_else(|| i64::try_from(d.indexed_at_ms).unwrap_or(i64::MAX));
                hits.push((
                    score,
                    source_time,
                    attachment.map_or_else(|| hit(&a, d), |entry| attachment_hit(&a, entry)),
                ));
            }
        }
    }
    hits.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then(b.1.cmp(&a.1))
            .then(a.2.citation_id.cmp(&b.2.citation_id))
    });
    hits.truncate(q.limit.unwrap_or(20));
    Ok(SearchResponse {
        hits: hits.into_iter().map(|(_, _, h)| h).collect(),
        sources,
        omitted_source_count: 0,
    })
}
pub(crate) fn item(
    s: &DaemonState,
    agent: &str,
    id: &str,
    instance: Option<&str>,
) -> Result<SearchItem, SearchError> {
    for (connector, index) in &s.connected_search.sources {
        let Some(a) = source_authority(s, agent, connector).filter(|a| a.enabled) else {
            continue;
        };
        if let Some(d) = index
            .documents
            .iter()
            .find(|d| d.citation_id == id)
            .or_else(|| index.historical.get(id))
        {
            if !visible_in_instance(
                index,
                id,
                instance,
                matches!(a.provider.as_str(), "onedrive" | "sharepoint"),
            ) {
                return Err(SearchError::NotFound);
            }
            return Ok(SearchItem {
                hit: hit(&a, d),
                text: d.text.clone(),
                sender: d.sender.clone(),
                event_start: d.event_start.clone(),
                event_end: d.event_end.clone(),
            });
        }
        if let Some(entry) = index.attachments.get(id) {
            if !visible_in_instance(
                index,
                &entry.parent_citation_id,
                instance,
                matches!(a.provider.as_str(), "onedrive" | "sharepoint"),
            ) {
                return Err(SearchError::NotFound);
            }
            return Ok(SearchItem {
                hit: attachment_hit(&a, entry),
                text: entry.document.text.clone(),
                sender: entry.document.sender.clone(),
                event_start: None,
                event_end: None,
            });
        }
    }
    Err(SearchError::NotFound)
}
pub(crate) fn validate_snapshot(registry: &SearchRegistry) -> Result<(), String> {
    if registry.sources.len() > MAX_SOURCES
        || serde_json::to_vec(registry)
            .map_err(|_| "Invalid connected index")?
            .len()
            > MAX_INDEX_BYTES
    {
        return Err("Connected index exceeds storage bounds".into());
    }
    for (id, s) in &registry.sources {
        if !opaque(id, 256)
            || !opaque(&s.agent_id, 256)
            || !opaque(&s.generation, 128)
            || !opaque(&s.grant_epoch, 128)
            || s.stream_epoch.as_ref().is_some_and(|v| !opaque(v, 128))
            || s.seen.len() > MAX_SEEN_ITEMS
            || s.settings_revision == 0
            || !matches!(s.settings.retained_item_limit, 100 | 500 | 2_000)
            || s.imported.len() > policy::MAX_IMPORTED_ITEMS
            || s.verified_membership.len() > policy::MAX_IMPORTED_ITEMS + 100
            || s.verified_membership
                .iter()
                .any(|(id, epoch)| id.len() != 64 || epoch.is_empty() || epoch.len() > 128)
            || s.historical.len() > policy::MAX_IMPORTED_ITEMS
            || s.historical
                .iter()
                .any(|(id, document)| id != &document.citation_id || !s.imported.contains_key(id))
            || s.attachments.len() > 256
            || s.attachment_scanned.len() > policy::MAX_IMPORTED_ITEMS + 100
            || s.attachment_retries.len() > policy::MAX_IMPORTED_ITEMS + 100
            || s.attachment_retries.iter().any(|(id, retry)| {
                id.len() != 64 || retry.revision.len() != 64 || !(1..=8).contains(&retry.attempts)
            })
            || s.attachment_omitted.len() > s.attachment_scanned.len()
            || s.attachment_omitted.values().any(|count| *count > 100)
            || s.attachment_error
                .as_ref()
                .is_some_and(|error| error.len() > 128)
            || s.imported
                .iter()
                .any(|(id, revision)| id.len() != 64 || revision.len() != 64)
            || s.history.as_ref().is_some_and(|history| {
                history.pass_id.is_empty()
                    || history.pass_id.len() > 128
                    || history
                        .verification_cursor
                        .as_ref()
                        .is_some_and(|cursor| cursor.len() != 64)
                    || history.cursor_fingerprints.len() > policy::MAX_HISTORY_CURSORS
                    || history
                        .cursor_fingerprints
                        .iter()
                        .any(|fingerprint| fingerprint.len() != 64)
                    || history
                        .coverage
                        .reason
                        .as_ref()
                        .is_some_and(|reason| reason.len() > 128)
                    || history.cursor.as_ref().is_some_and(|cursor| match cursor {
                        ImportCursor::Gmail { page_token } => {
                            page_token.is_empty() || page_token.len() > policy::MAX_CURSOR_BYTES
                        }
                        ImportCursor::Outlook { next_link } => {
                            next_link.is_empty() || next_link.len() > policy::MAX_CURSOR_BYTES
                        }
                    })
            })
            || s.seen
                .iter()
                .any(|(id, revision)| id.len() != 64 || revision.len() != 64)
            || s.documents.len() > 500
            || s.error.as_ref().is_some_and(|e| e.len() > 512)
        {
            return Err("Invalid connected index source".into());
        }
        let mut ids = HashSet::new();
        for d in s.documents.iter().chain(s.historical.values()) {
            if !opaque(&d.external_id, 1024)
                || !opaque(&d.collection_id, 512)
                || d.text.len() > MAX_TEXT_BYTES
                || d.title.len() > 2048
                || d.citation_id.len() != 64
                || d.revision.len() != 64
                || (s.documents.iter().any(|live| std::ptr::eq(live, d))
                    && !ids.insert(&d.citation_id))
                || d.source_url.as_ref().is_some_and(|u| u.len() > 2048)
            {
                return Err("Invalid connected index document".into());
            }
        }
        for (id, entry) in &s.attachments {
            if id != &entry.document.citation_id
                || id.len() != 64
                || entry.document.revision.len() != 64
                || !opaque(&entry.document.external_id, 1024)
                || !opaque(&entry.document.collection_id, 512)
                || entry.document.title.len() > 2048
                || entry.parent_citation_id.len() != 64
                || entry.parent_revision.len() != 64
                || entry.document.text.len() > MAX_TEXT_BYTES
                || entry.omissions.len() > 16
                || entry.locators.len() > 16
                || entry.status.len() > 32
                || !s
                    .documents
                    .iter()
                    .chain(s.historical.values())
                    .any(|parent| {
                        parent.citation_id == entry.parent_citation_id
                            && parent.revision == entry.parent_revision
                    })
            {
                return Err("Invalid connected attachment".into());
            }
        }
    }
    Ok(())
}

/// Remove authority immediately when any connector or permission mutation is saved.
pub(crate) fn reconcile_all(state: &mut DaemonState) {
    let invalid = state
        .connected_search
        .sources
        .iter()
        .filter(|(id, index)| {
            source_authority(state, &index.agent_id, id).is_none_or(|a| !a.enabled)
        })
        .map(|(id, index)| (id.clone(), index.agent_id.clone()))
        .collect::<Vec<_>>();
    for (id, agent) in invalid {
        state.connected_search.sources.remove(&id);
        crate::account_workflows::pause_for_source(
            state,
            &agent,
            &id,
            "Account authorization or read permission changed",
        );
    }
    crate::account_workflows::reconcile_authority(state);
}

/// Restore a conservative event fence for any deeper-search record predating it.
/// Recent-only legacy records keep their original live-observation behavior until
/// the owner explicitly enables history, when configuration creates the fence.
pub(crate) fn migrate_legacy_event_fences(registry: &mut SearchRegistry, now_ms: u64) {
    for source in registry.sources.values_mut() {
        if source.settings.history_depth != HistoryDepth::Recent
            && source.live_event_fence_at_ms.is_none()
        {
            source.live_event_fence_at_ms = Some(now_ms);
        }
    }
}
