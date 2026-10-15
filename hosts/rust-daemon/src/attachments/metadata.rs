//! Metadata projection is descriptive only. File authority always comes from
//! a scoped durable record, checked by serve and request hydration.
use super::{AttachmentKind, AttachmentMetadata, ATTACHMENT_RECORDS_KEY};
use anima_core::{Attachment, AttachmentType, Content, DataValue};

pub(crate) const MAX_ATTACHMENT_ID_BYTES: usize = 128;
pub(crate) fn valid_attachment_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ATTACHMENT_ID_BYTES
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
}
fn string<'a>(
    row: &'a std::collections::BTreeMap<String, DataValue>,
    key: &str,
) -> Option<&'a str> {
    match row.get(key)? {
        DataValue::String(value) => Some(value),
        _ => None,
    }
}
fn integer(row: &std::collections::BTreeMap<String, DataValue>, key: &str) -> Option<u64> {
    match row.get(key)? {
        DataValue::Number(value)
            if value.is_finite()
                && *value >= 0.0
                && value.fract() == 0.0
                && *value <= 9_007_199_254_740_991.0 =>
        {
            Some(*value as u64)
        }
        _ => None,
    }
}

pub(crate) fn matching_metadata(
    content: &Content,
    attachment: &Attachment,
) -> Option<AttachmentMetadata> {
    let DataValue::Array(records) = content.metadata.as_ref()?.get(ATTACHMENT_RECORDS_KEY)? else {
        return None;
    };
    records
        .iter()
        .take(crate::runs::MAX_RUN_ATTACHMENTS)
        .find_map(|value| {
            let DataValue::Object(row) = value else {
                return None;
            };
            let name = string(row, "name")?;
            let path = string(row, "path")?;
            let mime = string(row, "mime")?;
            if name != attachment.name
                || path != attachment.data
                || attachment.mime_type.as_deref() != Some(mime)
            {
                return None;
            }
            let kind = match string(row, "kind")? {
                "image" if attachment.attachment_type == AttachmentType::Image => {
                    AttachmentKind::Image
                }
                "text" if attachment.attachment_type == AttachmentType::File => {
                    AttachmentKind::Text
                }
                "document" if attachment.attachment_type == AttachmentType::File => {
                    AttachmentKind::Document
                }
                "audio" if attachment.attachment_type == AttachmentType::File => {
                    AttachmentKind::Audio
                }
                _ => return None,
            };
            let id = string(row, "id")?;
            let size = integer(row, "size")?;
            if !valid_attachment_id(id)
                || size > kind.max_bytes() as u64
                || !path.starts_with("uploads/")
                || path.contains(['\\', ':'])
                || path
                    .split('/')
                    .any(|component| component.is_empty() || component == "." || component == "..")
            {
                return None;
            }
            Some(AttachmentMetadata {
                id: id.into(),
                name: name.into(),
                mime: mime.into(),
                size,
                kind,
                path: path.into(),
                created_at_ms: integer(row, "createdAtMs")?,
            })
        })
}
