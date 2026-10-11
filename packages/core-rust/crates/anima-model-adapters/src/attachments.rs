//! Request-only image serialization. Filesystem authority and byte validation belong to the host.
use anima_core::{Attachment, AttachmentType, Content, MessageRole, ModelGenerateRequest};

const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;
const MAX_ENCODED_IMAGE_BYTES: usize = 4 * MAX_IMAGE_BYTES.div_ceil(3);

/// Missing model metadata is deliberately treated as no image support.
pub fn model_supports_images(provider: &str, model: &str) -> bool {
    crate::model_info(provider, model).is_some_and(|row| row.vision)
}

pub(crate) struct PreparedImage<'a> {
    pub(crate) mime: &'a str,
    pub(crate) base64: &'a str,
    pub(crate) data_uri: &'a str,
}

/// Only host-prepared, bounded data URIs become image parts. A path stays a reference.
pub(crate) fn prepared_image(attachment: &Attachment) -> Result<Option<PreparedImage<'_>>, String> {
    if attachment.attachment_type != AttachmentType::Image || !attachment.data.starts_with("data:")
    {
        return Ok(None);
    }
    let invalid = || "invalid prepared image attachment".to_owned();
    let mime = attachment.mime_type.as_deref().ok_or_else(invalid)?;
    if !matches!(
        mime,
        "image/png" | "image/jpeg" | "image/webp" | "image/gif"
    ) {
        return Err(invalid());
    }
    let prefix = format!("data:{mime};base64,");
    let base64 = attachment.data.strip_prefix(&prefix).ok_or_else(invalid)?;
    if base64.is_empty() || base64.len() > MAX_ENCODED_IMAGE_BYTES || base64.len() % 4 != 0 {
        return Err(invalid());
    }
    let encoded = base64.trim_end_matches('=');
    let padding = base64.len() - encoded.len();
    if padding > 2
        || !encoded
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/'))
        || base64.len() / 4 * 3 - padding > MAX_IMAGE_BYTES
    {
        return Err(invalid());
    }
    Ok(Some(PreparedImage {
        mime,
        base64,
        data_uri: &attachment.data,
    }))
}

/// Safe, bounded notes never interpolate an inline payload or an absolute workspace path.
pub fn image_reference_text(name: &str, path: &str, reason: &str) -> String {
    fn label(value: &str, limit: usize) -> String {
        if value
            .trim_start()
            .get(..5)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("data:"))
        {
            return "[inline image]".into();
        }
        value
            .chars()
            .filter(|character| !character.is_control())
            .take(limit)
            .collect()
    }
    let path = if path.starts_with(['/', '\\'])
        || path.as_bytes().get(1) == Some(&b':')
        || path.contains("://")
    {
        "[untrusted image reference]".into()
    } else {
        label(path, 1024)
    };
    format!(
        "[Image {} at {}: {}]",
        label(name, 256),
        path,
        label(reason, 160)
    )
}

pub(crate) struct PreparedUserContent<'a> {
    pub(crate) text: String,
    pub(crate) images: Vec<PreparedImage<'a>>,
}

pub(crate) fn prepare_user_content(
    content: &Content,
    supports_images: bool,
) -> Result<PreparedUserContent<'_>, String> {
    let mut prepared = PreparedUserContent {
        text: content.text.clone(),
        images: Vec::new(),
    };
    for attachment in content
        .attachments
        .iter()
        .flatten()
        .filter(|attachment| attachment.attachment_type == AttachmentType::Image)
    {
        if supports_images {
            if let Some(image) = prepared_image(attachment)? {
                prepared.images.push(image);
                continue;
            }
        }
        if !prepared.text.is_empty() {
            prepared.text.push('\n');
        }
        prepared.text.push_str(&image_reference_text(
            &attachment.name,
            &attachment.data,
            if supports_images {
                "image unavailable; the model cannot view this image"
            } else {
                "the model cannot view this image"
            },
        ));
    }
    Ok(prepared)
}

/// Deterministic adapters prepare the same references while retaining their fixed response.
pub(crate) fn text_fallback_request(
    request: &ModelGenerateRequest,
) -> Result<ModelGenerateRequest, String> {
    let mut request = request.clone();
    for message in &mut request.messages {
        if message.role == MessageRole::User {
            message.content.text = prepare_user_content(&message.content, false)?.text;
        }
        if let Some(attachments) = &mut message.content.attachments {
            attachments.retain(|attachment| attachment.attachment_type != AttachmentType::Image);
            if attachments.is_empty() {
                message.content.attachments = None;
            }
        }
    }
    Ok(request)
}

/// Upstream error bodies may echo any provider's full image request. Discard the
/// whole suspicious body before truncation, including native base64 field shapes.
pub(crate) fn image_payload_error(body: &str) -> bool {
    let body = body.to_ascii_lowercase();
    [
        "data:",
        "base64",
        "inline_data",
        "inlinedata",
        "\"images\"",
        "'images'",
    ]
    .iter()
    .any(|marker| body.contains(marker))
}
