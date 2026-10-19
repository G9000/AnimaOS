//! `GET /api/memories/export`: the owner downloads an agent's memory (or every
//! agent's) as JSON or Markdown. Owner-only, `Cache-Control: no-store`, sent
//! as an attachment. Embedding vectors never leave the store: only the
//! records themselves are written.
//!
//! Core entries are listed once, under `core` (by block), and are left out of
//! `memories`. Facts, relationships and entities carry no agent of their own:
//! an agent's facts are those whose evidence includes one of its memories, its
//! relationships are the ones naming it, and its entities are the agent itself
//! plus everything those relationships and facts name.

use std::collections::BTreeSet;

use anima_memory::{
    AgentRelationshipOptions, Memory, MemoryEntityOptions, MemoryManager, MemoryType,
    RecentMemoryOptions, RelationshipEndpointKind, TemporalFactOptions,
};
use axum::extract::{Request, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use chrono::{SecondsFormat, Utc};
use serde::Serialize;
use utoipa::ToSchema;

use super::contracts::{
    AgentRelationshipResponse, ErrorBody, MemoryEntityResponse, MemoryFactResponse, MemoryResponse,
};
use super::http::request_query;
use super::jobs::{authorize, no_store};
use super::sessions::rejected;
use super::{ApiError, AppState};
use crate::core_memory::{self, CoreBlock};

pub(super) const EXPORT_FORMAT_INVALID: &str = "format must be json or md";
const EXPORT_VERSION: u32 = 1;
/// Longest agent id kept in the download's file name.
const FILENAME_AGENT_CHARS: usize = 64;
/// Order of the memory types in the Markdown export.
const TYPE_ORDER: [MemoryType; 4] = [
    MemoryType::Fact,
    MemoryType::Observation,
    MemoryType::TaskResult,
    MemoryType::Reflection,
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Format {
    Json,
    Markdown,
}

impl Format {
    fn extension(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Markdown => "md",
        }
    }

    fn content_type(self) -> &'static str {
        match self {
            Self::Json => "application/json; charset=utf-8",
            Self::Markdown => "text/markdown; charset=utf-8",
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExportedCoreEntry {
    pub(crate) id: String,
    pub(crate) content: String,
    pub(crate) created_at: u64,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ExportedCore {
    /// "About you", in creation order.
    pub(crate) user: Vec<ExportedCoreEntry>,
    /// "Agent notes", in creation order.
    pub(crate) notes: Vec<ExportedCoreEntry>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExportedAgent {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) core: ExportedCore,
    /// Every non-core memory of the agent, newest first.
    pub(crate) memories: Vec<MemoryResponse>,
    pub(crate) entities: Vec<MemoryEntityResponse>,
    pub(crate) relationships: Vec<AgentRelationshipResponse>,
    pub(crate) temporal_facts: Vec<MemoryFactResponse>,
}

/// Records that no agent's section holds (a fact whose evidence was deleted,
/// an entity nothing names, a relationship between two non-agents).
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UnattributedRecords {
    pub(crate) entities: Vec<MemoryEntityResponse>,
    pub(crate) relationships: Vec<AgentRelationshipResponse>,
    pub(crate) temporal_facts: Vec<MemoryFactResponse>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MemoryExport {
    pub(crate) version: u32,
    pub(crate) exported_at: String,
    pub(crate) agents: Vec<ExportedAgent>,
    /// Only in an all-agents export: every entity, relationship and fact not
    /// exported under any agent, so nothing the owner holds is left out.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) unattributed: Option<UnattributedRecords>,
}

#[utoipa::path(get, path = "/api/memories/export", tag = "memories",
    params(
        ("format" = Option<String>, Query, description = "json (default) or md"),
        ("agentId" = Option<String>, Query, description = "One agent; every agent when absent")
    ),
    responses(
        (status = 200, description = "The memory export, sent as an attachment (JSON body, or Markdown with format=md)", body = MemoryExport),
        (status = 400, description = "format is not json or md, or the query is malformed", body = ErrorBody),
        (status = 403, description = "Local owner required", body = ErrorBody),
        (status = 404, description = "No such agent", body = ErrorBody)
    ))]
pub(super) async fn export_memories(State(state): State<AppState>, request: Request) -> Response {
    if let Err(response) = authorize(&state, &request, true) {
        return response;
    }
    let query = match request_query(request.uri()) {
        Ok(query) => query,
        Err(()) => return rejected(ApiError::bad_request_static("malformed query")),
    };
    let format = match query.get("format").map(String::as_str) {
        None | Some("json") => Format::Json,
        Some("md") => Format::Markdown,
        Some(_) => return rejected(ApiError::bad_request_static(EXPORT_FORMAT_INVALID)),
    };
    let agent_filter = query.get("agentId").cloned();

    let (known_agents, memory) = {
        let guard = state.daemon.read().await;
        if let Some(agent_id) = &agent_filter {
            if !guard.agents.contains_key(agent_id) {
                return rejected(ApiError::not_found());
            }
        }
        let mut agents: Vec<(String, String)> = guard
            .agents
            .iter()
            .map(|(id, agent)| (id.clone(), agent.config().name.clone()))
            .collect();
        agents.sort();
        (agents, guard.memory_handle())
    };
    let now = Utc::now();
    let (agents, unattributed) = {
        let manager = memory.read().await;
        match &agent_filter {
            Some(agent_id) => (
                known_agents
                    .iter()
                    .filter(|(id, _)| id == agent_id)
                    .map(|(id, name)| export_agent(&manager, id, name))
                    .collect(),
                None,
            ),
            None => {
                let agents = all_agents(&manager, known_agents);
                let unattributed = unattributed(&manager, &agents);
                (agents, Some(unattributed))
            }
        }
    };
    let export = MemoryExport {
        version: EXPORT_VERSION,
        exported_at: now.to_rfc3339_opts(SecondsFormat::Secs, true),
        agents,
        unattributed,
    };
    let body = match format {
        Format::Json => match serde_json::to_string_pretty(&export) {
            Ok(body) => body,
            Err(_) => return rejected(ApiError::service_unavailable("export failed")),
        },
        Format::Markdown => markdown(&export),
    };
    let filename = format!(
        "animaos-memory-{}-{}.{}",
        agent_filter
            .as_deref()
            .map(filename_part)
            .unwrap_or_else(|| "all".to_string()),
        now.format("%Y-%m-%d"),
        format.extension()
    );
    attachment(format, &filename, body)
}

fn attachment(format: Format, filename: &str, body: String) -> Response {
    let mut response = (StatusCode::OK, body).into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(format.content_type()),
    );
    // The file name is built from safe characters only.
    if let Ok(disposition) = HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
    {
        headers.insert(header::CONTENT_DISPOSITION, disposition);
    }
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    no_store(response)
}

/// The agent id as safe file name characters (`A-Z a-z 0-9 . _ -`).
pub(super) fn filename_part(agent_id: &str) -> String {
    let part: String = agent_id
        .chars()
        .take(FILENAME_AGENT_CHARS)
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if part.is_empty() {
        "agent".to_string()
    } else {
        part
    }
}

/// Every registered agent, then any agent id that only has memories (a deleted
/// agent's records are still the owner's data).
fn all_agents(manager: &MemoryManager, known: Vec<(String, String)>) -> Vec<ExportedAgent> {
    let mut seen: BTreeSet<String> = known.iter().map(|(id, _)| id.clone()).collect();
    let mut agents: Vec<ExportedAgent> = known
        .iter()
        .map(|(id, name)| export_agent(manager, id, name))
        .collect();
    let mut orphans: Vec<(String, String)> = Vec::new();
    for memory in manager.get_recent(RecentMemoryOptions {
        limit: Some(usize::MAX),
        ..RecentMemoryOptions::default()
    }) {
        if seen.insert(memory.agent_id.clone()) {
            orphans.push((memory.agent_id.clone(), memory.agent_name.clone()));
        }
    }
    orphans.sort();
    agents.extend(
        orphans
            .iter()
            .map(|(id, name)| export_agent(manager, id, name)),
    );
    agents
}

/// Every entity, relationship and fact that no agent's section holds. Works by
/// id against what the agents already export, so no record appears twice.
fn unattributed(manager: &MemoryManager, agents: &[ExportedAgent]) -> UnattributedRecords {
    let entity_keys: BTreeSet<(&str, &str)> = agents
        .iter()
        .flat_map(|agent| &agent.entities)
        .map(|entity| (entity.kind.as_str(), entity.id.as_str()))
        .collect();
    let relationship_ids: BTreeSet<&str> = agents
        .iter()
        .flat_map(|agent| &agent.relationships)
        .map(|relationship| relationship.id.as_str())
        .collect();
    let fact_ids: BTreeSet<&str> = agents
        .iter()
        .flat_map(|agent| &agent.temporal_facts)
        .map(|fact| fact.id.as_str())
        .collect();
    UnattributedRecords {
        entities: manager
            .list_entities(MemoryEntityOptions {
                limit: Some(usize::MAX),
                ..MemoryEntityOptions::default()
            })
            .iter()
            .map(MemoryEntityResponse::from)
            .filter(|entity| !entity_keys.contains(&(entity.kind.as_str(), entity.id.as_str())))
            .collect(),
        relationships: manager
            .list_agent_relationships(AgentRelationshipOptions {
                limit: Some(usize::MAX),
                ..AgentRelationshipOptions::default()
            })
            .iter()
            .map(AgentRelationshipResponse::from)
            .filter(|relationship| !relationship_ids.contains(relationship.id.as_str()))
            .collect(),
        temporal_facts: manager
            .list_temporal_facts(TemporalFactOptions {
                include_inactive: true,
                limit: Some(usize::MAX),
                ..TemporalFactOptions::default()
            })
            .iter()
            .map(MemoryFactResponse::from)
            .filter(|fact| !fact_ids.contains(fact.id.as_str()))
            .collect(),
    }
}

fn core_entry(memory: &Memory) -> ExportedCoreEntry {
    ExportedCoreEntry {
        id: memory.id.clone(),
        content: memory.content.clone(),
        created_at: memory.created_at,
    }
}

fn export_agent(manager: &MemoryManager, agent_id: &str, name: &str) -> ExportedAgent {
    let (user, notes) = core_memory::blocks_for(manager, agent_id);
    let memories: Vec<Memory> = manager.get_recent_filtered(
        RecentMemoryOptions {
            agent_id: Some(agent_id.to_string()),
            limit: Some(usize::MAX),
            ..RecentMemoryOptions::default()
        },
        |memory| !core_memory::is_core_entry(memory),
    );
    let relationships = manager.list_agent_relationships(AgentRelationshipOptions {
        agent_id: Some(agent_id.to_string()),
        limit: Some(usize::MAX),
        ..AgentRelationshipOptions::default()
    });
    let facts: Vec<_> = manager
        .list_temporal_facts(TemporalFactOptions {
            include_inactive: true,
            limit: Some(usize::MAX),
            ..TemporalFactOptions::default()
        })
        .into_iter()
        .filter(|fact| {
            fact.evidence_memory_ids.iter().any(|id| {
                manager
                    .get(id)
                    .is_some_and(|memory| memory.agent_id == agent_id)
            })
        })
        .collect();

    // The agent itself, then whoever its relationships and facts name.
    let mut keys: BTreeSet<(&'static str, String)> = BTreeSet::new();
    keys.insert((
        RelationshipEndpointKind::Agent.as_str(),
        agent_id.to_string(),
    ));
    for relationship in &relationships {
        keys.insert((
            relationship.source_kind.as_str(),
            relationship.source_agent_id.clone(),
        ));
        keys.insert((
            relationship.target_kind.as_str(),
            relationship.target_agent_id.clone(),
        ));
    }
    for fact in &facts {
        keys.insert((fact.subject_kind.as_str(), fact.subject_id.clone()));
        if let (Some(kind), Some(id)) = (fact.object_kind, &fact.object_id) {
            keys.insert((kind.as_str(), id.clone()));
        }
    }
    let entities = keys
        .iter()
        .filter_map(|(kind, id)| {
            RelationshipEndpointKind::from_str(kind)
                .ok()
                .and_then(|kind| manager.get_entity(kind, id))
        })
        .map(|entity| MemoryEntityResponse::from(&entity))
        .collect();

    ExportedAgent {
        id: agent_id.to_string(),
        name: name.to_string(),
        core: ExportedCore {
            user: user.iter().map(core_entry).collect(),
            notes: notes.iter().map(core_entry).collect(),
        },
        memories: memories.iter().map(MemoryResponse::from).collect(),
        entities,
        relationships: relationships
            .iter()
            .map(AgentRelationshipResponse::from)
            .collect(),
        temporal_facts: facts.iter().map(MemoryFactResponse::from).collect(),
    }
}

/// One list item; extra lines are indented so they stay inside the item.
fn item(text: &str) -> String {
    format!("- {}\n", text.trim().replace('\n', "\n  "))
}

fn type_heading(memory_type: MemoryType) -> &'static str {
    match memory_type {
        MemoryType::Fact => "Facts",
        MemoryType::Observation => "Observations",
        MemoryType::TaskResult => "Task results",
        MemoryType::Reflection => "Reflections",
    }
}

fn markdown(export: &MemoryExport) -> String {
    let mut text = format!(
        "# AnimaOS memory export\n\nExported {}.\n",
        export.exported_at
    );
    for agent in &export.agents {
        text.push_str(&format!("\n## {} ({})\n", agent.name, agent.id));
        for (block, entries) in [
            (CoreBlock::User, &agent.core.user),
            (CoreBlock::Notes, &agent.core.notes),
        ] {
            text.push_str(&format!("\n### {}\n\n", block.label()));
            if entries.is_empty() {
                text.push_str("_Nothing yet._\n");
            }
            for entry in entries {
                text.push_str(&item(&entry.content));
            }
        }
        for memory_type in TYPE_ORDER {
            let kind = memory_type.as_str();
            let group: Vec<_> = agent
                .memories
                .iter()
                .filter(|memory| memory.memory_type == kind)
                .collect();
            if group.is_empty() {
                continue;
            }
            text.push_str(&format!("\n### {}\n\n", type_heading(memory_type)));
            for memory in group {
                text.push_str(&item(&memory.content));
            }
        }
        records_markdown(
            &mut text,
            &agent.entities,
            &agent.relationships,
            &agent.temporal_facts,
        );
    }
    if let Some(rest) = &export.unattributed {
        if !(rest.entities.is_empty()
            && rest.relationships.is_empty()
            && rest.temporal_facts.is_empty())
        {
            text.push_str("\n## Unattributed\n");
            records_markdown(
                &mut text,
                &rest.entities,
                &rest.relationships,
                &rest.temporal_facts,
            );
        }
    }
    text
}

/// Entities, relationships and temporal facts, each under its own heading
/// when there are any.
fn records_markdown(
    text: &mut String,
    entities: &[MemoryEntityResponse],
    relationships: &[AgentRelationshipResponse],
    facts: &[MemoryFactResponse],
) {
    if !entities.is_empty() {
        text.push_str("\n### Entities\n\n");
        for entity in entities {
            let summary = entity
                .summary
                .as_deref()
                .map(|summary| format!(": {summary}"))
                .unwrap_or_default();
            text.push_str(&item(&format!(
                "{} ({}){summary}",
                entity.name, entity.kind
            )));
        }
    }
    if !relationships.is_empty() {
        text.push_str("\n### Relationships\n\n");
        for relationship in relationships {
            let summary = relationship
                .summary
                .as_deref()
                .map(|summary| format!(": {summary}"))
                .unwrap_or_default();
            text.push_str(&item(&format!(
                "{} {} {}{summary}",
                relationship.source_agent_name,
                relationship.relationship_type,
                relationship.target_agent_name
            )));
        }
    }
    if !facts.is_empty() {
        text.push_str("\n### Temporal facts\n\n");
        for fact in facts {
            let object = fact
                .value
                .as_deref()
                .or(fact.object_name.as_deref())
                .unwrap_or("");
            text.push_str(&item(&format!(
                "{} {} {} ({})",
                fact.subject_name, fact.predicate, object, fact.status
            )));
        }
    }
}
