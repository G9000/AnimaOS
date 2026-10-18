//! Attachment authority for accepted inputs. Records are scoped durably; bytes
//! are read only for new acceptance validation, then dropped immediately.
use super::CLIENT_REQUEST_ID_METADATA_KEY;
use crate::app::SharedDaemonState;
use crate::attachments::{
    files::read_attachment_bytes,
    metadata::{matching_metadata, valid_attachment_id},
    AttachmentKind, AttachmentRecord, ATTACHMENT_INPUT_TEXT_KEY, ATTACHMENT_RECORDS_KEY,
};
use crate::history::HistoryStore;
use crate::routes::ApiError;
use crate::runs::{RunInput, RunStatus, MAX_RUN_ATTACHMENTS};
use anima_core::{Attachment, AttachmentType, Content, DataValue};
use std::collections::{BTreeMap, HashSet};

pub(crate) const UNKNOWN_ATTACHMENT_IDS: &str = "unknown attachment ids";
pub(crate) const INVALID_ATTACHMENT_IDS: &str =
    "attachment ids must be nonempty, bounded, unique opaque identifiers";
pub(crate) const TOO_MANY_ATTACHMENTS: &str = "at most 10 attachments are allowed per message";
pub(crate) const ATTACHMENT_INPUT_UNAVAILABLE: &str =
    "Attachment file is unavailable or no longer matches its upload";
pub(crate) const ATTACHMENT_HISTORY_UNAVAILABLE: &str = "Attachment history is unavailable";

pub(crate) fn validate_attachment_ids(ids: &[String]) -> Result<(), ApiError> {
    if ids.len() > MAX_RUN_ATTACHMENTS {
        return Err(ApiError::bad_request_static(TOO_MANY_ATTACHMENTS));
    }
    let mut seen = HashSet::new();
    if ids
        .iter()
        .any(|id| !valid_attachment_id(id) || !seen.insert(id))
    {
        return Err(ApiError::bad_request_static(INVALID_ATTACHMENT_IDS));
    }
    Ok(())
}
pub(crate) async fn resolve_run_attachments(
    store: &dyn HistoryStore,
    agent_id: &str,
    session_id: &str,
    ids: &[String],
) -> Result<Vec<AttachmentRecord>, ApiError> {
    validate_attachment_ids(ids)?;
    let mut rows = Vec::with_capacity(ids.len());
    for id in ids {
        let row = store
            .get_attachment(agent_id, id)
            .await
            .map_err(|_| ApiError::service_unavailable(ATTACHMENT_HISTORY_UNAVAILABLE))?
            .filter(|row| row.id == *id && row.agent_id == agent_id && row.session_id == session_id)
            .ok_or_else(|| ApiError::bad_request_static(UNKNOWN_ATTACHMENT_IDS))?;
        rows.push(row);
    }
    Ok(rows)
}
pub(super) async fn validate_available(
    root: std::path::PathBuf,
    rows: Vec<AttachmentRecord>,
) -> Result<(), ApiError> {
    tokio::task::spawn_blocking(move || {
        for row in rows {
            drop(
                read_attachment_bytes(&root, &row)
                    .map_err(|_| ApiError::bad_request_static(ATTACHMENT_INPUT_UNAVAILABLE))?,
            );
        }
        Ok(())
    })
    .await
    .map_err(|_| ApiError::service_unavailable(ATTACHMENT_INPUT_UNAVAILABLE))?
}
fn public_value(row: &AttachmentRecord) -> DataValue {
    DataValue::Object(BTreeMap::from([
        ("id".into(), DataValue::String(row.id.clone())),
        ("name".into(), DataValue::String(row.name.clone())),
        ("mime".into(), DataValue::String(row.mime.clone())),
        ("size".into(), DataValue::Number(row.size as f64)),
        ("kind".into(), DataValue::String(row.kind.as_str().into())),
        ("path".into(), DataValue::String(row.path.clone())),
        (
            "createdAtMs".into(),
            DataValue::Number(row.created_at_ms as f64),
        ),
    ]))
}
pub(crate) fn attachment_content(
    text: &str,
    records: &[AttachmentRecord],
    existing_metadata: Option<BTreeMap<String, DataValue>>,
) -> Content {
    let mut metadata = existing_metadata.unwrap_or_default();
    metadata.remove(ATTACHMENT_RECORDS_KEY);
    metadata.remove(ATTACHMENT_INPUT_TEXT_KEY);
    metadata.remove(anima_service_adapters::document_content::DOCUMENT_EXTRACTS_METADATA_KEY);
    if records.is_empty() {
        return Content {
            text: text.into(),
            attachments: None,
            metadata: (!metadata.is_empty()).then_some(metadata),
        };
    }
    metadata.insert(
        ATTACHMENT_INPUT_TEXT_KEY.into(),
        DataValue::String(text.into()),
    );
    metadata.insert(
        ATTACHMENT_RECORDS_KEY.into(),
        DataValue::Array(records.iter().map(public_value).collect()),
    );
    let mut message = text.to_string();
    for row in records {
        if !message.is_empty() {
            message.push_str("\n\n");
        }
        message.push_str(&match row.kind {
            AttachmentKind::Text => format!(
                "Attached text: {} ({}). Use read_file to read the saved file.",
                row.name, row.path
            ),
            AttachmentKind::Document
                if crate::attachments::documents::document_format(row)
                    == Some(anima_service_adapters::document_content::DocumentFormat::Pdf) =>
            {
                format!(
                    "Attached document: {} ({}). Original saved; any admitted PDF excerpt covers the text layer only, without OCR or visual layout.",
                    row.name, row.path
                )
            }
            AttachmentKind::Document => format!(
                "Attached document: {} ({}). Original saved; any admitted text excerpt covers main-body text only.",
                row.name, row.path
            ),
            AttachmentKind::Image => format!("Attached image: {} ({}).", row.name, row.path),
            AttachmentKind::Audio => format!(
                "Attached audio: {} ({}). Original saved; audio has not been transcribed and the model cannot hear this file.",
                row.name, row.path
            ),
        });
    }
    Content {
        text: message,
        attachments: Some(
            records
                .iter()
                .map(|row| Attachment {
                    attachment_type: if row.kind == AttachmentKind::Image {
                        AttachmentType::Image
                    } else {
                        AttachmentType::File
                    },
                    name: row.name.clone(),
                    data: row.path.clone(),
                    mime_type: Some(row.mime.clone()),
                })
                .collect(),
        ),
        metadata: Some(metadata),
    }
}
pub(super) fn input_from_content(content: &Content) -> RunInput {
    let attachment_ids = content
        .attachments
        .iter()
        .flatten()
        .filter_map(|reference| matching_metadata(content, reference).map(|row| row.id))
        .collect::<Vec<_>>();
    let string = |key: &str| {
        content
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get(key))
            .and_then(|value| match value {
                DataValue::String(value) => Some(value.clone()),
                _ => None,
            })
    };
    RunInput {
        text: if attachment_ids.is_empty() {
            content.text.clone()
        } else {
            string(ATTACHMENT_INPUT_TEXT_KEY).unwrap_or_else(|| content.text.clone())
        },
        attachment_ids,
        skill: string(crate::skills::SKILL_METADATA_KEY),
    }
}
pub(crate) async fn accepted_content(
    state: &SharedDaemonState,
    agent_id: &str,
    room_id: &str,
    run_id: &str,
    mut metadata: BTreeMap<String, DataValue>,
) -> Result<Content, ApiError> {
    let (input, store) = {
        let guard = state.read().await;
        let run = guard
            .runs
            .get(run_id)
            .filter(|run| {
                run.agent_id == agent_id
                    && run.session_id == crate::sessions::session_id_for_room(room_id)
                    && run.status == RunStatus::Queued
                    && guard.sessions.get(agent_id, &run.session_id).is_some()
            })
            .ok_or_else(ApiError::not_found)?;
        if let Some(key) = &run.idempotency_key {
            metadata.insert(
                CLIENT_REQUEST_ID_METADATA_KEY.into(),
                DataValue::String(key.clone()),
            );
        }
        (run.input.clone(), guard.history.store())
    };
    let rows = resolve_run_attachments(
        store.as_ref(),
        agent_id,
        &crate::sessions::session_id_for_room(room_id),
        &input.attachment_ids,
    )
    .await?;
    if let Some(skill) = input.skill {
        metadata.insert(
            crate::skills::SKILL_METADATA_KEY.into(),
            DataValue::String(skill),
        );
    }
    Ok(attachment_content(&input.text, &rows, Some(metadata)))
}
