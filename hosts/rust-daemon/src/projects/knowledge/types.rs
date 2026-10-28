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
    pub(crate) citation_id: String,
    pub(crate) resource_id: String,
    pub(crate) document_id: String,
    pub(crate) title: String,
    pub(crate) locator: String,
    pub(crate) excerpt: String,
    #[serde(skip)]
    pub(crate) source_sha256: String,
    #[serde(skip)]
    pub(crate) chunk_sha256: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct KnowledgeSearch {
    #[serde(flatten)]
    pub(crate) binding: KnowledgeStatus,
    pub(crate) hits: Vec<KnowledgeHit>,
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
