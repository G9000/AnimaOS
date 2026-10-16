//! Session route bodies (spec §3.3).

use std::collections::BTreeMap;

use anima_core::{AttachmentType, MessageRole};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

use super::shared::data_value_to_json;
use super::usage::UsageTotalsResponse;
use crate::sessions::views::{MessagePage, PageMessage, SessionPage, SessionView};
use crate::sessions::SessionCapabilities;

/// Message metadata the session routes expose: spec §3.3's list plus `kind`
/// (check-in prompts) and `source` (Telegram turns), and what a historical
/// tool card or step label shows: a tool result's `toolStatus` and
/// `toolDurationMs`, `incomplete` on a model call's unfinished text, and
/// `budgetWrapup` on its accepted tool-free budget reply.
/// `taskResult` stays hidden: it repeats the whole tool result.
pub(crate) const EXPOSED_MESSAGE_METADATA: [&str; 16] = [
    "toolCalls",
    "toolCallId",
    "stepId",
    "runId",
    "toolStatus",
    "toolDurationMs",
    "incomplete",
    "stopped",
    "revised",
    "steer",
    "skill",
    "clientRequestId",
    "communication",
    "kind",
    "source",
    "budgetWrapup",
];

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionCapabilitiesResponse {
    pub(crate) send: bool,
    pub(crate) steer: bool,
    pub(crate) stop: bool,
    pub(crate) rename: bool,
    pub(crate) archive: bool,
    pub(crate) delete: bool,
    pub(crate) compact: bool,
    pub(crate) export: bool,
}

impl From<SessionCapabilities> for SessionCapabilitiesResponse {
    fn from(value: SessionCapabilities) -> Self {
        Self {
            send: value.send,
            steer: value.steer,
            stop: value.stop,
            rename: value.rename,
            archive: value.archive,
            delete: value.delete,
            compact: value.compact,
            export: value.export,
        }
    }
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionSummaryResponse {
    pub(crate) text: String,
    pub(crate) through_message_id: String,
    pub(crate) created_at_ms: u64,
    pub(crate) source_message_count: usize,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionContextTrimmedResponse {
    pub(crate) dropped_through_message_id: String,
    pub(crate) at_ms: u64,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionCompactionErrorResponse {
    pub(crate) message: String,
    pub(crate) at_ms: u64,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionMatchResponse {
    /// `null` when only the title matched.
    pub(crate) message_id: Option<String>,
    pub(crate) snippet: String,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionResponse {
    pub(crate) id: String,
    pub(crate) agent_id: String,
    /// The transcript room: the id itself except for mapped legacy rooms.
    pub(crate) room_id: String,
    pub(crate) kind: String,
    pub(crate) origin: String,
    pub(crate) title: String,
    pub(crate) title_source: String,
    pub(crate) created_at_ms: u64,
    pub(crate) last_activity_at_ms: u64,
    pub(crate) last_read_at_ms: Option<u64>,
    pub(crate) archived: bool,
    pub(crate) parent_session_id: Option<String>,
    pub(crate) parent_run_id: Option<String>,
    pub(crate) parent_agent_id: Option<String>,
    pub(crate) summary: Option<SessionSummaryResponse>,
    pub(crate) context_trimmed: Option<SessionContextTrimmedResponse>,
    /// The last failed compaction, until one succeeds (spec §5.4).
    pub(crate) compaction_error: Option<SessionCompactionErrorResponse>,
    pub(crate) message_count: usize,
    pub(crate) preview: Option<String>,
    pub(crate) active_runs: usize,
    /// Approvals waiting for the owner in this session (spec §3.2).
    pub(crate) pending_approvals: usize,
    pub(crate) unread: bool,
    pub(crate) capabilities: SessionCapabilitiesResponse,
    #[serde(rename = "match", skip_serializing_if = "Option::is_none")]
    pub(crate) matched: Option<SessionMatchResponse>,
    /// Only `GET` of one session carries it: the session's usage totals, or
    /// `null` when they could not be read. The outer `None` leaves the key out.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<UsageTotalsResponse>)]
    pub(crate) usage: Option<Option<UsageTotalsResponse>>,
}

impl From<&SessionView> for SessionResponse {
    fn from(view: &SessionView) -> Self {
        let record = &view.record;
        Self {
            id: record.id.clone(),
            agent_id: record.agent_id.clone(),
            room_id: record.room_id().to_string(),
            kind: record.kind.as_str().into(),
            origin: record.origin.as_str().into(),
            title: record.title.clone(),
            title_source: record.title_source.as_str().into(),
            created_at_ms: record.created_at_ms,
            last_activity_at_ms: record.last_activity_at_ms,
            last_read_at_ms: record.last_read_at_ms,
            archived: record.archived,
            parent_session_id: record.parent_session_id.clone(),
            parent_run_id: record.parent_run_id.clone(),
            parent_agent_id: record.parent_agent_id.clone(),
            summary: record
                .summary
                .as_ref()
                .map(|summary| SessionSummaryResponse {
                    text: summary.text.clone(),
                    through_message_id: summary.through_message_id.clone(),
                    created_at_ms: summary.created_at_ms,
                    source_message_count: summary.source_message_count,
                }),
            context_trimmed: record.context_trimmed.as_ref().map(|trimmed| {
                SessionContextTrimmedResponse {
                    dropped_through_message_id: trimmed.dropped_through_message_id.clone(),
                    at_ms: trimmed.at_ms,
                }
            }),
            compaction_error: record.compaction_error.as_ref().map(|error| {
                SessionCompactionErrorResponse {
                    message: error.message.clone(),
                    at_ms: error.at_ms,
                }
            }),
            message_count: view.message_count,
            preview: view.preview.clone(),
            active_runs: view.active_runs,
            pending_approvals: view.pending_approvals,
            unread: view.unread,
            capabilities: view.capabilities.into(),
            matched: view.matched.as_ref().map(|found| SessionMatchResponse {
                message_id: found.message_id.clone(),
                snippet: found.snippet.clone(),
            }),
            usage: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionsEnvelope {
    pub(crate) sessions: Vec<SessionResponse>,
    pub(crate) next_cursor: Option<String>,
}

impl From<&SessionPage> for SessionsEnvelope {
    fn from(page: &SessionPage) -> Self {
        Self {
            sessions: page.sessions.iter().map(SessionResponse::from).collect(),
            next_cursor: page.next_cursor.clone(),
        }
    }
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct SessionEnvelope {
    pub(crate) session: SessionResponse,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionAttachmentResponse {
    #[serde(rename = "type")]
    pub(crate) attachment_type: String,
    pub(crate) name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) mime: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) created_at_ms: Option<u64>,
    /// Evidence for this selected user message only; no raw text or source hash.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) extraction: Option<DocumentExtractionResponse>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(untagged)]
pub(crate) enum DocumentExtractionResponse {
    Docx(DocxExtractionResponse),
    Pdf(PdfExtractionResponse),
    Xlsx(XlsxExtractionResponse),
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DocxExtractionResponse {
    pub(crate) version: u32,
    /// Closed values from the core contract, kept HTTP-independent there.
    pub(crate) status: String,
    pub(crate) paragraph_count: usize,
    pub(crate) omissions: Vec<String>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PdfExtractionResponse {
    pub(crate) version: u32,
    pub(crate) format: String,
    pub(crate) status: String,
    pub(crate) page_count: Option<u32>,
    pub(crate) text_page_count: Option<u32>,
    pub(crate) included_page_count: Option<u32>,
    pub(crate) omissions: Vec<String>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct XlsxExtractionResponse {
    pub(crate) version: u32,
    pub(crate) format: String,
    pub(crate) status: String,
    pub(crate) sheet_count: Option<u32>,
    pub(crate) observed_cell_count: Option<u32>,
    pub(crate) included_cell_count: Option<u32>,
    pub(crate) hidden_sheet_count: Option<u32>,
    pub(crate) omissions: Vec<String>,
}

fn extraction_response(
    message: &anima_core::Message,
    selected: Option<&crate::attachments::AttachmentMetadata>,
) -> Option<DocumentExtractionResponse> {
    if message.role != MessageRole::User {
        return None;
    }
    let selected =
        selected.filter(|row| row.kind == crate::attachments::AttachmentKind::Document)?;
    let snapshot = anima_core::document_content::parse_document_snapshots(&message.content)
        .ok()?
        .into_iter()
        .find(|snapshot| snapshot.attachment_id == selected.id)?;
    let extraction = snapshot.extraction;
    use anima_core::document_content::DocumentFormat;
    let extension = selected.name.rsplit_once('.').map(|(_, ext)| ext);
    let expected_format = match selected.mime.as_str() {
        crate::attachments::documents::DOCX_MIME
            if extension.is_some_and(|ext| ext.eq_ignore_ascii_case("docx")) =>
        {
            DocumentFormat::Docx
        }
        crate::attachments::documents::PDF_MIME
            if extension.is_some_and(|ext| ext.eq_ignore_ascii_case("pdf")) =>
        {
            DocumentFormat::Pdf
        }
        crate::attachments::documents::XLSX_MIME
            if extension.is_some_and(|ext| ext.eq_ignore_ascii_case("xlsx")) =>
        {
            DocumentFormat::Xlsx
        }
        _ => return None,
    };
    if extraction.format != expected_format {
        return None;
    }
    let string = |value| match serde_json::to_value(value).ok()? {
        Value::String(s) => Some(s),
        _ => None,
    };
    let status = string(extraction.status)?;
    let omissions = extraction
        .omissions
        .into_iter()
        .map(|v| match serde_json::to_value(v) {
            Ok(Value::String(s)) => s,
            _ => String::new(),
        })
        .collect();
    Some(match extraction.format {
        DocumentFormat::Docx => DocumentExtractionResponse::Docx(DocxExtractionResponse {
            version: 1,
            status,
            paragraph_count: extraction.blocks.len(),
            omissions,
        }),
        DocumentFormat::Pdf => DocumentExtractionResponse::Pdf(PdfExtractionResponse {
            version: 2,
            format: "pdf".into(),
            status,
            page_count: extraction.pdf_coverage.as_ref().map(|c| c.page_count),
            text_page_count: extraction.pdf_coverage.as_ref().map(|c| c.text_page_count),
            included_page_count: extraction
                .pdf_coverage
                .as_ref()
                .map(|c| c.included_page_count),
            omissions,
        }),
        DocumentFormat::Xlsx => {
            let coverage = extraction.xlsx_coverage.as_ref();
            let hidden_sheet_count = coverage.map(|coverage| {
                coverage
                    .sheets
                    .iter()
                    .filter(|sheet| {
                        matches!(
                            sheet.visibility,
                            anima_core::document_content::XlsxSheetVisibility::Hidden
                                | anima_core::document_content::XlsxSheetVisibility::VeryHidden
                        )
                    })
                    .count() as u32
            });
            DocumentExtractionResponse::Xlsx(XlsxExtractionResponse {
                version: 2,
                format: "xlsx".into(),
                status,
                sheet_count: coverage.map(|coverage| coverage.sheet_count),
                observed_cell_count: coverage.map(|coverage| coverage.observed_cell_count),
                included_cell_count: coverage.map(|coverage| coverage.included_cell_count),
                hidden_sheet_count,
                omissions,
            })
        }
    })
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionMessageResponse {
    pub(crate) id: String,
    pub(crate) role: String,
    pub(crate) text: String,
    /// Metadata only; attachment contents are not repeated here.
    pub(crate) attachments: Vec<SessionAttachmentResponse>,
    /// Only `toolCalls`, `toolCallId`, `stepId`, `runId`, `toolStatus`
    /// (`success` or `error`), `toolDurationMs`, `incomplete`, `stopped`,
    /// `revised`, `steer`, `skill`, `clientRequestId`, `communication`,
    /// `kind`, `source`, and `budgetWrapup`; other keys are not exposed.
    pub(crate) metadata: BTreeMap<String, Value>,
    pub(crate) created_at_ms: u64,
    /// Present (true) only on silent check-in messages, with `includeHidden=true`.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub(crate) hidden: bool,
}

impl From<&PageMessage> for SessionMessageResponse {
    fn from(entry: &PageMessage) -> Self {
        let message = &entry.message;
        Self {
            id: message.id.clone(),
            role: match message.role {
                MessageRole::User => "user",
                MessageRole::Assistant => "assistant",
                MessageRole::System => "system",
                MessageRole::Tool => "tool",
            }
            .into(),
            text: message.content.text.clone(),
            attachments: message
                .content
                .attachments
                .iter()
                .flatten()
                .map(|attachment| {
                    let metadata = crate::attachments::metadata::matching_metadata(
                        &message.content,
                        attachment,
                    );
                    SessionAttachmentResponse {
                        attachment_type: match attachment.attachment_type {
                            AttachmentType::File => "file",
                            AttachmentType::Image => "image",
                            AttachmentType::Url => "url",
                        }
                        .into(),
                        name: attachment.name.clone(),
                        id: metadata.as_ref().map(|row| row.id.clone()),
                        mime: metadata.as_ref().map(|row| row.mime.clone()),
                        size: metadata.as_ref().map(|row| row.size),
                        kind: metadata.as_ref().map(|row| row.kind.as_str().into()),
                        path: metadata.as_ref().map(|row| row.path.clone()),
                        created_at_ms: metadata.as_ref().map(|row| row.created_at_ms),
                        extraction: extraction_response(message, metadata.as_ref()),
                    }
                })
                .collect(),
            metadata: message
                .content
                .metadata
                .iter()
                .flatten()
                .filter(|(key, _)| EXPOSED_MESSAGE_METADATA.contains(&key.as_str()))
                .map(|(key, value)| (key.clone(), data_value_to_json(value)))
                .collect(),
            created_at_ms: message.created_at_ms,
            hidden: entry.hidden,
        }
    }
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionMessagesEnvelope {
    pub(crate) messages: Vec<SessionMessageResponse>,
    pub(crate) next_before: Option<String>,
}

impl From<&MessagePage> for SessionMessagesEnvelope {
    fn from(page: &MessagePage) -> Self {
        Self {
            messages: page
                .messages
                .iter()
                .map(SessionMessageResponse::from)
                .collect(),
            next_before: page.next_before.clone(),
        }
    }
}

/// `POST /api/agents/{id}/sessions`; an empty body means `{}`.
#[derive(Clone, Debug, Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SessionCreateRequest {
    #[serde(default)]
    pub(crate) title: Option<String>,
}

/// `PATCH /api/agents/{id}/sessions/{sid}`; at least one field.
#[derive(Clone, Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SessionUpdateRequest {
    #[serde(default)]
    pub(crate) title: Option<String>,
    #[serde(default)]
    pub(crate) archived: Option<bool>,
    #[serde(default)]
    pub(crate) last_read_at_ms: Option<u64>,
}
