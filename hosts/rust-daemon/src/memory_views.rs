//! Owner-selected references into canonical memory, never a profile content store.
use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, RwLock};

use anima_memory::{Memory, MemoryScope, MemoryType, RecentMemoryOptions};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;
use utoipa::ToSchema;

use crate::app::SharedDaemonState;
use crate::routes::ApiError;

pub(crate) type SharedMemoryViewSelections = Arc<RwLock<BTreeMap<String, MemoryViewSelection>>>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MemoryViewSelection {
    pub(crate) agent_id: String,
    pub(crate) version: u64,
    pub(crate) user_profile_ids: Vec<String>,
    pub(crate) agent_note_ids: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct MemoryViewRegistry {
    pub(crate) rows: BTreeMap<String, MemoryViewSelection>,
}
impl MemoryViewRegistry {
    pub(crate) fn snapshot(&self) -> Vec<MemoryViewSelection> {
        self.rows.values().cloned().collect()
    }
    pub(crate) fn validate(rows: &[MemoryViewSelection]) -> Result<(), String> {
        if rows.len() > 1000 {
            return Err("memory view selection limit exceeded".into());
        }
        let mut agents = HashSet::new();
        for row in rows {
            let ids: Vec<_> = row
                .user_profile_ids
                .iter()
                .chain(&row.agent_note_ids)
                .collect();
            if !valid_id(&row.agent_id)
                || !agents.insert(&row.agent_id)
                || ids.len() > 6
                || (row.version == 0 && !ids.is_empty())
                || ids.iter().any(|id| !valid_id(id))
                || ids.iter().collect::<HashSet<_>>().len() != ids.len()
            {
                return Err("invalid memory view selection".into());
            }
        }
        Ok(())
    }
    pub(crate) fn restored(rows: Vec<MemoryViewSelection>, live: &HashSet<String>) -> Self {
        Self {
            rows: rows
                .into_iter()
                .filter(|row| live.contains(&row.agent_id))
                .map(|row| (row.agent_id.clone(), row))
                .collect(),
        }
    }
    pub(crate) fn selection(&self, agent: &str) -> MemoryViewSelection {
        self.rows
            .get(agent)
            .cloned()
            .unwrap_or_else(|| MemoryViewSelection {
                agent_id: agent.into(),
                version: 0,
                user_profile_ids: Vec::new(),
                agent_note_ids: Vec::new(),
            })
    }
    pub(crate) fn published(&self) -> SharedMemoryViewSelections {
        Arc::new(RwLock::new(self.rows.clone()))
    }
    pub(crate) fn stage(&mut self, selection: MemoryViewSelection) -> Result<(), String> {
        let mut proposed = self.rows.clone();
        proposed.insert(selection.agent_id.clone(), selection);
        Self::validate(&proposed.values().cloned().collect::<Vec<_>>())?;
        self.rows = proposed;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MemoryViewSource {
    pub(crate) id: String,
    pub(crate) content: String,
    pub(crate) source_hash: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub(crate) struct MemoryViewUnavailable {
    pub(crate) id: String,
    pub(crate) reason: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MemoryViewResponse {
    pub(crate) version: u64,
    pub(crate) view_digest: String,
    pub(crate) user_profile: Vec<MemoryViewSource>,
    pub(crate) agent_notes: Vec<MemoryViewSource>,
    pub(crate) unavailable: Vec<MemoryViewUnavailable>,
}
#[derive(Clone, Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MemoryViewPick {
    pub(crate) id: String,
    pub(crate) source_hash: String,
}
#[derive(Clone, Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MemoryViewPatchRequest {
    pub(crate) expected_version: u64,
    pub(crate) user_profile: Vec<MemoryViewPick>,
    pub(crate) agent_notes: Vec<MemoryViewPick>,
}
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MemoryViewCorrectionRequest {
    pub(crate) expected_version: u64,
    pub(crate) expected_content: String,
    pub(crate) expected_source_hash: String,
    pub(crate) content: String,
}

fn valid_id(id: &str) -> bool {
    !id.trim().is_empty() && id.len() <= 256 && clean_text(id) && !id.chars().any(char::is_control)
}
fn clean_text(text: &str) -> bool {
    !crate::memory_text::has_hidden_text(text)
        && !text
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
}
pub(crate) fn eligible_source(memory: &Memory, agent: &str) -> bool {
    memory.agent_id == agent
        && memory.memory_type == MemoryType::Fact
        && memory.scope == MemoryScope::Private
        && memory.room_id.is_none()
        && memory.world_id.is_none()
        && memory.session_id.is_none()
        && valid_id(&memory.id)
        && !memory.content.trim().is_empty()
        && memory.content.len() <= 8000 * 4
        && memory.content.chars().count() <= 8000
        && clean_text(&memory.content)
        && clean_text(&memory.agent_name)
        && memory.importance.is_finite()
        && !memory.tags.as_ref().is_some_and(|tags| {
            tags.len() > 20
                || tags
                    .iter()
                    .any(|tag| !clean_text(tag) || tag.chars().count() > 40)
        })
}
pub(crate) fn source_hash(memory: &Memory) -> String {
    // Canonical Memory has no derived search score or Serialize implementation.
    // Spell every canonical field explicitly; never hash a ranked search clone.
    let canonical = serde_json::json!({"id":memory.id,"agentId":memory.agent_id,
        "agentName":memory.agent_name,"type":memory.memory_type.as_str(),"content":memory.content,
        "importance":memory.importance,"createdAt":memory.created_at,"tags":memory.tags,
        "scope":memory.scope.as_str(),"roomId":memory.room_id,"worldId":memory.world_id,
        "sessionId":memory.session_id});
    digest(&canonical)
}
fn digest(value: &serde_json::Value) -> String {
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(value).expect("canonical JSON serializes"))
    )
}
fn source(memory: &Memory) -> MemoryViewSource {
    MemoryViewSource {
        id: memory.id.clone(),
        content: memory.content.clone(),
        source_hash: source_hash(memory),
    }
}

#[derive(Clone)]
pub(crate) struct MemoryViewService {
    state: SharedDaemonState,
    transactions: Arc<Mutex<()>>,
}
impl MemoryViewService {
    pub(crate) fn new(state: SharedDaemonState, transactions: Arc<Mutex<()>>) -> Self {
        Self {
            state,
            transactions,
        }
    }

    pub(crate) async fn get(&self, agent: &str) -> Result<MemoryViewResponse, ApiError> {
        let _transaction = self.transactions.lock().await;
        let (memory, selection, durable, published) = {
            let guard = self.state.read().await;
            if !guard.agents.contains_key(agent) {
                return Err(ApiError::not_found());
            }
            (
                Arc::clone(&guard.memory),
                guard.memory_views.selection(agent),
                guard.memory_store.is_some(),
                Arc::clone(&guard.memory_view_selections),
            )
        };
        if published.read().is_err() {
            return Err(unavailable());
        }
        let guard = memory.read().await;
        Ok(render(&selection, |id| {
            guard
                .get(id)
                .filter(|memory| durable && eligible_source(memory, agent))
        }))
    }

    pub(crate) async fn candidates(
        &self,
        agent: &str,
        query: &str,
    ) -> Result<(Vec<MemoryViewSource>, bool), ApiError> {
        if query.len() > 512 * 4 || query.chars().count() > 512 || !clean_text(query) {
            return Err(ApiError::bad_request(
                "Candidate query must be at most 512 visible characters",
            ));
        }
        let memory = {
            let guard = self.state.read().await;
            if !guard.agents.contains_key(agent) {
                return Err(ApiError::not_found());
            }
            if guard.memory_store.is_none() {
                return Ok((Vec::new(), false));
            }
            Arc::clone(&guard.memory)
        };
        let query = query.trim().to_lowercase();
        let guard = memory.read().await;
        let buffer = std::cell::RefCell::new(Vec::<Memory>::new());
        // Reuse the canonical filtered scan without cloning the entire store.
        // Keep at most51 eligible rows; the core callback rejects its own result
        // collection, so unrelated facts/entities/relationships are never cloned.
        let _ = guard.get_recent_filtered(
            RecentMemoryOptions {
                agent_id: Some(agent.into()),
                limit: Some(0),
                ..RecentMemoryOptions::default()
            },
            |memory| {
                if eligible_source(memory, agent)
                    && (query.is_empty() || memory.content.to_lowercase().contains(&query))
                {
                    let mut rows = buffer.borrow_mut();
                    if rows.len() == 51
                        && rows.last().is_some_and(|last| {
                            memory.created_at < last.created_at
                                || (memory.created_at == last.created_at && memory.id >= last.id)
                        })
                    {
                        return false;
                    }
                    rows.push(memory.clone());
                    rows.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(a.id.cmp(&b.id)));
                    rows.truncate(51);
                }
                false
            },
        );
        let mut rows = buffer.into_inner();
        let has_more = rows.len() > 50;
        rows.truncate(50);
        Ok((rows.iter().map(source).collect(), has_more))
    }

    pub(crate) async fn select(
        &self,
        agent: &str,
        input: MemoryViewPatchRequest,
    ) -> Result<MemoryViewResponse, ApiError> {
        validate_picks(&input)?;
        let service = self.clone();
        let agent = agent.to_string();
        tokio::spawn(async move { service.settle(&agent, input).await })
            .await
            .map_err(|_| unavailable())?
    }

    async fn settle(
        &self,
        agent: &str,
        input: MemoryViewPatchRequest,
    ) -> Result<MemoryViewResponse, ApiError> {
        let _transaction = self.transactions.lock().await;
        let (memory, published, previous) = {
            let guard = self.state.read().await;
            if !guard.agents.contains_key(agent) {
                return Err(ApiError::not_found());
            }
            if guard.control_plane_store.is_none() || guard.memory_store.is_none() {
                return Err(unavailable());
            }
            (
                Arc::clone(&guard.memory),
                Arc::clone(&guard.memory_view_selections),
                guard.memory_views.selection(agent),
            )
        };
        if previous.version != input.expected_version {
            return Err(stale());
        }
        // Check publication access before staging. No leaf guard crosses await.
        if published.read().is_err() {
            return Err(unavailable());
        }
        let selected = MemoryViewSelection {
            agent_id: agent.into(),
            version: previous.version,
            user_profile_ids: input
                .user_profile
                .iter()
                .map(|pick| pick.id.clone())
                .collect(),
            agent_note_ids: input
                .agent_notes
                .iter()
                .map(|pick| pick.id.clone())
                .collect(),
        };
        let records = {
            let guard = memory.read().await;
            let mut records = BTreeMap::new();
            for pick in input.user_profile.iter().chain(&input.agent_notes) {
                let memory = guard
                    .get(&pick.id)
                    .filter(|memory| eligible_source(memory, agent))
                    .ok_or_else(ApiError::not_found)?;
                if source_hash(&memory) != pick.source_hash {
                    return Err(stale());
                }
                records.insert(memory.id.clone(), memory);
            }
            records
        }; // Validated canonical read is admission; release before state stage.
        let mut guard = self.state.write().await;
        if !guard.agents.contains_key(agent) {
            return Err(ApiError::not_found());
        }
        if guard.control_plane_store.is_none()
            || guard.memory_store.is_none()
            || !Arc::ptr_eq(&memory, &guard.memory)
            || !Arc::ptr_eq(&published, &guard.memory_view_selections)
        {
            return Err(unavailable());
        }
        if guard.memory_views.selection(agent) != previous {
            return Err(stale());
        }
        if previous.user_profile_ids == selected.user_profile_ids
            && previous.agent_note_ids == selected.agent_note_ids
        {
            drop(guard);
            return Ok(render(&previous, |id| records.get(id).cloned()));
        }
        let mut selected = selected;
        selected.version = selected.version.checked_add(1).ok_or_else(stale)?;
        let old_registry = guard.memory_views.clone();
        let live = guard.agents.keys().cloned().collect::<HashSet<_>>();
        let mut next_registry = MemoryViewRegistry::restored(old_registry.snapshot(), &live);
        next_registry
            .stage(selected.clone())
            .map_err(|message| ApiError::conflict(message))?;
        guard.memory_views = next_registry;
        let acknowledged_rows = guard.memory_views.rows.clone();
        let persist = guard.control_plane_persist_request();
        drop(guard);
        if persist.save().await.is_err() {
            self.state.write().await.memory_views = old_registry;
            return Err(unavailable());
        }
        // Update the existing shared handle. Creating a new Arc would leave
        // previously constructed providers on a stale published selection map.
        *published.write().map_err(|_| unavailable())? = acknowledged_rows;
        Ok(render(&selected, |id| records.get(id).cloned()))
    }
}

fn validate_picks(input: &MemoryViewPatchRequest) -> Result<(), ApiError> {
    let mut ids = HashSet::new();
    if input
        .user_profile
        .len()
        .saturating_add(input.agent_notes.len())
        > 6
    {
        return Err(ApiError::bad_request("Select at most six source facts"));
    }
    for pick in input.user_profile.iter().chain(&input.agent_notes) {
        if !valid_id(&pick.id)
            || !ids.insert(&pick.id)
            || pick.source_hash.len() != 64
            || !pick
                .source_hash
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            return Err(ApiError::bad_request(
                "Source IDs must be unique and bounded with exact canonical hashes",
            ));
        }
    }
    Ok(())
}
fn render(
    selection: &MemoryViewSelection,
    mut get: impl FnMut(&str) -> Option<Memory>,
) -> MemoryViewResponse {
    let mut unavailable = Vec::new();
    let mut collect = |ids: &[String]| {
        ids.iter()
            .filter_map(|id| match get(id) {
                Some(memory) => Some(source(&memory)),
                None => {
                    unavailable.push(MemoryViewUnavailable {
                        id: id.clone(),
                        reason: "unavailable".into(),
                    });
                    None
                }
            })
            .collect::<Vec<_>>()
    };
    let user_profile = collect(&selection.user_profile_ids);
    let agent_notes = collect(&selection.agent_note_ids);
    let view_digest = digest(
        &serde_json::json!({"agentId":selection.agent_id,"version":selection.version,
        "userProfile":user_profile,"agentNotes":agent_notes,"unavailable":unavailable}),
    );
    MemoryViewResponse {
        version: selection.version,
        view_digest,
        user_profile,
        agent_notes,
        unavailable,
    }
}
fn stale() -> ApiError {
    ApiError::conflict("Selected sources or version changed; refresh and review again")
}
fn unavailable() -> ApiError {
    ApiError::service_unavailable(
        "Memory view selection could not be acknowledged; inspect current views before retrying",
    )
}
