//! Content-free source status and bounded search projections.

use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum KnowledgeSourceState {
    Indexing,
    Ready,
    Partial,
    Unavailable,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct KnowledgeSourceStatus {
    pub(crate) resource_id: String,
    pub(crate) status: KnowledgeSourceState,
    pub(crate) document_count: usize,
    pub(crate) indexed_at_ms: Option<u64>,
    pub(crate) generation: u64,
    pub(crate) omitted_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct KnowledgeStatus {
    pub(crate) project_revision: u64,
    pub(crate) access_epoch: u64,
    pub(crate) member_generation: u64,
    pub(crate) sources: Vec<KnowledgeSourceStatus>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct KnowledgeHit {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) recognition: Option<anima_document_store_sqlite::PassageOrigin>,
    pub(crate) citation_id: String,
    pub(crate) resource_id: String,
    pub(crate) document_id: String,
    pub(crate) title: String,
    pub(crate) locator: String,
    pub(crate) excerpt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) origin: Option<ConnectedCitationOrigin>,
    #[serde(skip)]
    pub(crate) source_sha256: String,
    #[serde(skip)]
    pub(crate) chunk_sha256: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConnectedCitationOrigin {
    pub(crate) owner_agent_id: String,
    pub(crate) owner_name: Option<String>,
    pub(crate) account_label: Option<String>,
    pub(crate) connector_id: String,
    pub(crate) provider: String,
    pub(crate) remote_id: String,
    pub(crate) revision_sha256: String,
    pub(crate) verified_at_ms: u64,
    pub(crate) coverage: &'static str,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct KnowledgeSearch {
    #[serde(flatten)]
    pub(crate) binding: KnowledgeStatus,
    pub(crate) hits: Vec<KnowledgeHit>,
    pub(crate) retrieval: KnowledgeSearchRetrieval,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum KnowledgeSearchMode {
    Keyword,
    Hybrid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SemanticSearchState {
    Ready,
    Partial,
    Unavailable,
}

/// Reports learned local participation separately from current text-index coverage.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct KnowledgeSearchRetrieval {
    pub(crate) mode: KnowledgeSearchMode,
    pub(crate) semantic_state: SemanticSearchState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) provider: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) model: Option<String>,
    pub(crate) indexed_windows: usize,
    pub(crate) eligible_windows: usize,
    pub(crate) omitted_windows: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) reason: Option<&'static str>,
}

impl KnowledgeSearchRetrieval {
    pub(super) fn keyword(reason: &'static str) -> Self {
        Self {
            mode: KnowledgeSearchMode::Keyword,
            semantic_state: SemanticSearchState::Unavailable,
            provider: None,
            model: None,
            indexed_windows: 0,
            eligible_windows: 0,
            omitted_windows: 0,
            reason: Some(reason),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct KnowledgeCitation {
    pub(crate) project_revision: u64,
    pub(crate) access_epoch: u64,
    pub(crate) member_generation: u64,
    #[serde(flatten)]
    pub(crate) hit: KnowledgeHit,
    pub(crate) text: String,
}
