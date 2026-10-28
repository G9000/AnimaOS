//! Owner document-edit route identifiers; full reviews contain only typed plain data.
use serde::Deserialize;
use utoipa::ToSchema;

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct DocumentEditOwnerQuery {
    pub(crate) agent_id: String,
    pub(crate) session_id: String,
}

// The authoritative serialized review is `document_edits::projection::DocumentEditReview`.
// Its nested OfficeInventory/OfficeChange types are shared with the portable editor.
