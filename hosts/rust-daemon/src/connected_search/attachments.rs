//! Bounded attachment content is kept separate from live workflow observations.
use super::*;
use crate::attachments::documents::worker::{DocumentExtractor, DocumentWorker};
use anima_service_adapters::document_content::{
    DocumentExtractionStatus, DocumentFormat, DocumentLocator,
};

#[derive(Clone)]
pub(crate) struct RawAttachment {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) mime: String,
    pub(crate) bytes: Option<Vec<u8>>,
    pub(crate) status: String,
}

impl std::fmt::Debug for RawAttachment {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RawAttachment")
            .field("id", &"[REDACTED]")
            .field("name", &"[REDACTED]")
            .field("mime", &self.mime)
            .field("bytes", &self.bytes.as_ref().map(Vec::len))
            .field("status", &self.status)
            .finish()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct AttachmentBatch {
    pub(crate) items: Vec<RawAttachment>,
    pub(crate) omitted: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AttachmentEntry {
    pub(crate) parent_citation_id: String,
    pub(crate) parent_revision: String,
    pub(crate) document: IndexedDocument,
    pub(crate) status: String,
    pub(crate) omissions: Vec<String>,
    pub(crate) locators: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AttachmentRetry {
    pub(crate) revision: String,
    pub(crate) attempts: u8,
    pub(crate) retry_at_ms: u64,
}

pub(super) fn schedule_retry(source: &mut SourceIndex, parent: &IndexedDocument, now: u64) {
    let attempts = source
        .attachment_retries
        .get(&parent.citation_id)
        .filter(|retry| retry.revision == parent.revision)
        .map_or(1, |retry| retry.attempts.saturating_add(1).min(8));
    let delay = (30_000_u64.saturating_mul(1_u64 << (attempts - 1))).min(3_600_000);
    source.attachment_retries.insert(
        parent.citation_id.clone(),
        AttachmentRetry {
            revision: parent.revision.clone(),
            attempts,
            retry_at_ms: now.saturating_add(delay),
        },
    );
    source.attachment_scanned.remove(&parent.citation_id);
    source.attachment_omitted.remove(&parent.citation_id);
    source.attachment_error =
        Some("Attachment text is temporarily unavailable; retry scheduled".into());
}

pub(super) fn retry_due(source: &SourceIndex, parent: &IndexedDocument, now: u64) -> bool {
    source
        .attachment_retries
        .get(&parent.citation_id)
        .is_none_or(|retry| retry.revision != parent.revision || retry.retry_at_ms <= now)
}

pub(super) async fn extract_attachment(
    authority: &SourceAuthority,
    parent: &IndexedDocument,
    raw: RawAttachment,
    now: u64,
) -> Result<AttachmentEntry, SearchError> {
    if !opaque(&raw.id, 1024) || raw.name.len() > 2048 || raw.mime.len() > 256 {
        return Err(SearchError::Invalid);
    }
    let mut status = raw.status;
    let mut text = String::new();
    let mut omissions = Vec::new();
    let mut locators = Vec::new();
    if let Some(bytes) = raw.bytes {
        if bytes.len() > 2 * 1024 * 1024 {
            status = "tooLarge".into();
        } else if matches!(raw.mime.as_str(), "text/plain" | "text/csv") {
            match String::from_utf8(bytes) {
                Ok(value) => {
                    text = value;
                    status = "ready".into();
                }
                Err(_) => status = "unsupported".into(),
            }
        } else {
            let format = match raw.mime.as_str() {
                "application/pdf" => Some(DocumentFormat::Pdf),
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => {
                    Some(DocumentFormat::Docx)
                }
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => {
                    Some(DocumentFormat::Xlsx)
                }
                _ => None,
            };
            if let Some(format) = format {
                match DocumentWorker
                    .extract(
                        format,
                        bytes,
                        anima_core::CancelSignal::new(),
                        Some(tokio::time::Instant::now() + std::time::Duration::from_secs(5)),
                    )
                    .await
                {
                    Ok(extracted) => {
                        status = match extracted.status {
                            DocumentExtractionStatus::Ready => "ready",
                            DocumentExtractionStatus::Partial => "partial",
                            DocumentExtractionStatus::Unsupported => "unsupported",
                            DocumentExtractionStatus::Encrypted => "encrypted",
                            DocumentExtractionStatus::Malformed => "malformed",
                            DocumentExtractionStatus::TooLarge => "tooLarge",
                            DocumentExtractionStatus::NoText => "noText",
                            DocumentExtractionStatus::Unavailable
                            | DocumentExtractionStatus::Cancelled
                            | DocumentExtractionStatus::ContextOmitted => "unavailable",
                        }
                        .into();
                        omissions = extracted
                            .omissions
                            .iter()
                            .filter_map(|value| {
                                serde_json::to_value(value)
                                    .ok()?
                                    .as_str()
                                    .map(str::to_owned)
                            })
                            .take(16)
                            .collect();
                        for block in extracted.blocks {
                            let locator = match block.locator {
                                DocumentLocator::PdfPage { page } => format!("Page {page}"),
                                DocumentLocator::DocxParagraph { paragraph } => {
                                    format!("Paragraph {paragraph}")
                                }
                                DocumentLocator::XlsxCell { sheet, row, column } => {
                                    format!("Sheet {sheet}, row {row}, column {column}")
                                }
                            };
                            if locators.len() < 16 && !locators.contains(&locator) {
                                locators.push(locator.clone());
                            }
                            text.push_str(&format!("[{locator}] {}\n", block.text));
                            if text.len() > MAX_TEXT_BYTES {
                                break;
                            }
                        }
                    }
                    Err(_) => status = "unavailable".into(),
                }
            } else {
                status = "unsupported".into();
            }
        }
    }
    let mut document = providers::document(
        parent.kind,
        digest(&["connected-content", &parent.external_id, &raw.id]),
        format!("attachment:{}", parent.citation_id),
        raw.name,
        text,
    );
    document.source_url = parent.source_url.clone();
    document.source_modified_at = parent.source_modified_at.clone();
    document.sender = parent.sender.clone();
    let document = normalize(document, authority, now)?;
    Ok(AttachmentEntry {
        parent_citation_id: parent.citation_id.clone(),
        parent_revision: parent.revision.clone(),
        document,
        status,
        omissions,
        locators,
    })
}
