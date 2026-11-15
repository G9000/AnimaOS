//! Durable identities established by successful remote receipts, never by local identifiers.
use serde::{Deserialize, Serialize};

/// Immutable provenance for one selected publication and its confirmed canonical result.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PublicationMapping {
    pub(crate) workspace_id: String,
    pub(crate) generation: String,
    pub(crate) agent_id: String,
    pub(crate) local_id: String,
    pub(crate) publication_id: String,
    pub(crate) source_digest: Option<String>,
    pub(crate) source_revision: Option<String>,
    pub(crate) proposal_id: Option<String>,
    pub(crate) canonical_id: String,
    pub(crate) canonical_revision: u64,
    pub(crate) receipt: serde_json::Value,
}
