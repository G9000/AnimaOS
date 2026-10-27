//! Owner-enabled native account snapshots and cited bounded search.
pub(crate) mod providers;
mod service;
#[cfg(test)]
mod tests;

use crate::state::DaemonState;
use serde::{Deserialize, Serialize};
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

#[derive(Clone, Debug)]
pub(crate) struct SourceChange {
    pub(crate) id: String,
    pub(crate) connector_id: String,
    pub(crate) generation: String,
    pub(crate) provider: String,
    pub(crate) kind: SourceChangeKind,
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
}
#[derive(Clone, Debug, Default, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RefreshInput {
    pub(crate) connector_id: Option<String>,
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
}
#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SearchResponse {
    pub(crate) hits: Vec<SearchHit>,
    pub(crate) sources: Vec<SourceView>,
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
}
impl std::fmt::Display for SearchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
        Self::Invalid=>"Connected search request is invalid", Self::NotFound=>"Connected source or citation was not found", Self::Conflict=>"Source authorization changed; refresh and enable the source again", Self::Unavailable=>"Connected source is unavailable or needs renewed permission", Self::Capacity=>"Connected search capacity reached; disable and re-enable the source to establish a fresh baseline", Self::Persistence=>"Connected search could not be saved",
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
        "gdrive" => 16,
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
        if !baseline {
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
    next.baseline_at_ms = Some(now);
    next.last_attempt_at_ms = Some(a.sync_started_at_ms);
    next.last_success_at_ms = Some(now);
    next.error = None;
    *source = next;
    Ok(changes)
}

pub(crate) fn source_authority(s: &DaemonState, agent: &str, id: &str) -> Option<SourceAuthority> {
    let runtime = s.agents.get(agent)?;
    let (provider, generation, active, tool) =
        if let Some(r) = s.drive_records.get(id).filter(|r| r.agent_id == agent) {
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
            "gcalendar"=>(DocumentKind::Event,s.calendar_connectors.get(&id)?.account_label.clone(),"Up to 500 events across selected calendars; 30 days past through 180 days ahead; 8 KiB per item",500),
            _=>(DocumentKind::Mail,s.mail_records.get(&id)?.connector.account_label.clone(),"Up to 100 recent inbox messages (IMAP up to 25); 8 KiB per item; polling can miss activity outside retention",100),
        };
        let error=if !a.read_granted {Some("Reconnect this account and grant its native read tool before enabling indexed search".into())}else{index.and_then(|i|i.error.clone())}; let status=if !a.read_granted {"unavailable"} else if !a.enabled {"disabled"} else if error.is_some()||index.and_then(|i|i.last_success_at_ms).is_some_and(|t|crate::connectors::gcalendar::now_ms().saturating_sub(t)>POLL_INTERVAL_MS*3){"stale"} else if index.and_then(|i|i.last_success_at_ms).is_none(){"syncing"}else{"ready"};
        Some(SourceView{connector_id:id,provider:a.provider,kind,account_label:label,generation:a.generation,available:a.read_granted,enabled:a.enabled,status:status.into(),scope:scope.into(),item_limit:limit,item_count:if a.enabled {index.map_or(0,|i|i.documents.len())}else{0},last_attempt_at_ms:index.and_then(|i|i.last_attempt_at_ms),last_success_at_ms:index.and_then(|i|i.last_success_at_ms),error})
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
    }
}
pub(crate) fn query(
    s: &DaemonState,
    agent: &str,
    q: &SearchQuery,
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
    let sources = source_views(s, agent)?;
    let mut hits = vec![];
    for (id, index) in &s.connected_search.sources {
        let Some(a) = source_authority(s, agent, id).filter(|a| a.enabled) else {
            continue;
        };
        if !q.connector_ids.is_empty() && !q.connector_ids.contains(id) {
            continue;
        }
        for d in &index.documents {
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
                hits.push((score, d.indexed_at_ms, hit(&a, d)));
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
    })
}
pub(crate) fn item(s: &DaemonState, agent: &str, id: &str) -> Result<SearchItem, SearchError> {
    for (connector, index) in &s.connected_search.sources {
        let Some(a) = source_authority(s, agent, connector).filter(|a| a.enabled) else {
            continue;
        };
        if let Some(d) = index.documents.iter().find(|d| d.citation_id == id) {
            return Ok(SearchItem {
                hit: hit(&a, d),
                text: d.text.clone(),
                sender: d.sender.clone(),
                event_start: d.event_start.clone(),
                event_end: d.event_end.clone(),
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
            || s.seen
                .iter()
                .any(|(id, revision)| id.len() != 64 || revision.len() != 64)
            || s.documents.len() > 500
            || s.error.as_ref().is_some_and(|e| e.len() > 512)
        {
            return Err("Invalid connected index source".into());
        }
        let mut ids = HashSet::new();
        for d in &s.documents {
            if !opaque(&d.external_id, 1024)
                || !opaque(&d.collection_id, 512)
                || d.text.len() > MAX_TEXT_BYTES
                || d.title.len() > 2048
                || d.citation_id.len() != 64
                || d.revision.len() != 64
                || !ids.insert(&d.citation_id)
                || d.source_url.as_ref().is_some_and(|u| u.len() > 2048)
            {
                return Err("Invalid connected index document".into());
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
