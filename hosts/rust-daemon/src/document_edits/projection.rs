//! Plain-data owner projections, with complete reviews and bounded inventory pages.
use super::{DocumentEditRecord, ExpectedDestination};
use anima_document_content::{
    OfficeChange, OfficeFormat, OfficeInventory, OfficePreservation, OfficeTarget,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

// Owner source inspection adds its fixed-size source scope after pagination.
const PAGE_BYTES: usize = 12 * 1024 - 128;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceLabel {
    pub(crate) kind: String,
    pub(crate) display_name: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct InventoryCoverage {
    pub(crate) total_targets: usize,
    pub(crate) returned_targets: usize,
    pub(crate) complete: bool,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct OfficeInventoryPage {
    pub(crate) version: u8,
    pub(crate) format: OfficeFormat,
    pub(crate) source_sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) source_scope: Option<String>,
    pub(crate) source: SourceLabel,
    pub(crate) targets: Vec<OfficeTarget>,
    pub(crate) coverage: InventoryCoverage,
    pub(crate) retained_hyperlink_count: usize,
    pub(crate) limitations: Vec<String>,
    pub(crate) next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct DocumentEditReview {
    pub(crate) version: u8,
    pub(crate) proposal_id: String,
    pub(crate) review_sha256: String,
    pub(crate) source: SourceLabel,
    pub(crate) source_sha256: String,
    pub(crate) file_path: String,
    pub(crate) expected_destination: ExpectedDestination,
    pub(crate) expires_at_ms: u64,
    pub(crate) preview_quality: &'static str,
    pub(crate) before: OfficeInventory,
    pub(crate) after: OfficeInventory,
    pub(crate) changes: Vec<OfficeChange>,
    pub(crate) preservation: OfficePreservation,
}
impl DocumentEditReview {
    pub(crate) fn from_record(record: &DocumentEditRecord, source: SourceLabel) -> Self {
        let draft = &record.draft;
        Self {
            version: 1,
            proposal_id: draft.id.clone(),
            review_sha256: record.review_sha256.clone(),
            source,
            source_sha256: draft.source.sha256().into(),
            file_path: draft.file_path.clone(),
            expected_destination: draft.expected_destination.clone(),
            expires_at_ms: draft.expires_at_ms,
            preview_quality: "structural_content",
            before: draft.content_review.before.clone(),
            after: draft.content_review.after.clone(),
            changes: draft.content_review.changes.clone(),
            preservation: draft.content_review.preservation.clone(),
        }
    }
}

fn cursor_seal(hash: &str, scope: &str, offset: usize) -> String {
    hex::encode(Sha256::digest(
        format!("anima-office-page-v1\0{hash}\0{scope}\0{offset}").as_bytes(),
    ))
}
fn cursor(hash: &str, scope: &str, offset: usize) -> String {
    format!("{offset}:{}", cursor_seal(hash, scope, offset))
}

pub(crate) fn inventory_page(
    inventory: &OfficeInventory,
    hash: &str,
    source: SourceLabel,
    scope: &str,
    input: Option<&str>,
) -> Result<OfficeInventoryPage, String> {
    let start = if let Some(value) = input {
        if value.len() > 96 {
            return Err("Document inventory cursor is invalid".into());
        }
        let (number, seal) = value
            .split_once(':')
            .ok_or("Document inventory cursor is invalid")?;
        let offset: usize = number
            .parse()
            .map_err(|_| "Document inventory cursor is invalid")?;
        if seal != cursor_seal(hash, scope, offset) || offset >= inventory.targets.len() {
            return Err("Document inventory source or scope changed".into());
        }
        offset
    } else {
        0
    };
    let mut page = OfficeInventoryPage {
        version: 1,
        format: inventory.format,
        source_sha256: hash.into(),
        source_scope: None,
        source,
        targets: Vec::new(),
        coverage: InventoryCoverage {
            total_targets: inventory.targets.len(),
            returned_targets: 0,
            complete: false,
        },
        retained_hyperlink_count: inventory.retained_hyperlinks.len(),
        limitations: inventory.limitations.clone(),
        next_cursor: None,
    };
    for target in &inventory.targets[start..] {
        page.targets.push(target.clone());
        let end = start + page.targets.len();
        page.coverage.returned_targets = page.targets.len();
        page.coverage.complete = end == inventory.targets.len();
        page.next_cursor = (!page.coverage.complete).then(|| cursor(hash, scope, end));
        if serde_json::to_vec(&page)
            .map_err(|_| "Document inventory cannot be encoded")?
            .len()
            > PAGE_BYTES
        {
            page.targets.pop();
            if page.targets.is_empty() {
                return Err(
                    "Document target exceeds the inventory page limit; use a smaller document"
                        .into(),
                );
            }
            page.coverage.returned_targets = page.targets.len();
            page.coverage.complete = false;
            page.next_cursor = Some(cursor(hash, scope, start + page.targets.len()));
            break;
        }
    }
    if inventory.targets.is_empty() {
        page.coverage.complete = true;
    }
    if serde_json::to_vec(&page)
        .map_err(|_| "Document inventory cannot be encoded")?
        .len()
        > PAGE_BYTES
    {
        return Err("Document inventory exceeds the page limit".into());
    }
    Ok(page)
}
