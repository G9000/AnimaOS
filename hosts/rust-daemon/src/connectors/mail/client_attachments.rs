//! Read a small number of attachment bodies belonging to an authorized inbox message.
use super::*;
use crate::connected_search::{AttachmentBatch, RawAttachment};

const MAX_BYTES: usize = 2 * 1024 * 1024;
const MAX_ENCODED: usize = ((MAX_BYTES + 2) / 3) * 4 + 16;

fn supported(name: &str, mime: &str) -> bool {
    let extension = name
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase());
    matches!(
        (mime, extension.as_deref()),
        ("text/plain", Some("txt"))
            | ("text/csv", Some("csv"))
            | ("application/pdf", Some("pdf"))
            | (
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
                Some("docx")
            )
            | (
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
                Some("xlsx")
            )
    )
}

fn collect_gmail_parts(
    part: &Value,
    depth: usize,
    found: &mut Vec<Value>,
) -> Result<(), MailError> {
    if depth > 12 || found.len() > 100 {
        return Err(MailError::Upstream);
    }
    if part["filename"]
        .as_str()
        .is_some_and(|name| !name.is_empty())
    {
        found.push(part.clone());
    }
    if let Some(parts) = part.get("parts") {
        let parts = parts.as_array().ok_or(MailError::Upstream)?;
        if parts.len() > 100 {
            return Err(MailError::Upstream);
        }
        for child in parts {
            collect_gmail_parts(child, depth + 1, found)?;
        }
    }
    Ok(())
}

impl MailClient {
    async fn attachment_bytes(
        &self,
        request: reqwest::RequestBuilder,
        limit: usize,
    ) -> Result<Vec<u8>, MailError> {
        let mut response = request.send().await.map_err(|_| MailError::Upstream)?;
        match response.status().as_u16() {
            200..=299 => {}
            401 | 403 => return Err(MailError::Unauthorized),
            _ => return Err(MailError::Upstream),
        }
        if response
            .content_length()
            .is_some_and(|size| size > limit as u64)
        {
            return Err(MailError::Upstream);
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| MailError::Upstream)? {
            if bytes.len().saturating_add(chunk.len()) > limit {
                return Err(MailError::Upstream);
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }

    pub(super) async fn fetch_attachment_batch(
        &self,
        provider: Provider,
        access: &str,
        message_id: &str,
    ) -> Result<AttachmentBatch, MailError> {
        if message_id.is_empty()
            || message_id.len() > 1024
            || message_id.chars().any(char::is_control)
        {
            return Err(MailError::Invalid);
        }
        match provider {
            Provider::Gmail => self.gmail_attachments(access, message_id).await,
            Provider::Outlook => self.outlook_attachments(access, message_id).await,
            Provider::Email => Err(MailError::Invalid),
        }
    }

    async fn gmail_attachments(
        &self,
        access: &str,
        message_id: &str,
    ) -> Result<AttachmentBatch, MailError> {
        if !message_id.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
            return Err(MailError::Invalid);
        }
        let detail = self
            .json(
                self.http
                    .get(format!("{}/users/me/messages/{message_id}", self.gmail_api))
                    .query(&[("format", "full")])
                    .bearer_auth(access),
                false,
            )
            .await?;
        if detail["id"].as_str() != Some(message_id) {
            return Err(MailError::Upstream);
        }
        if !detail["labelIds"]
            .as_array()
            .ok_or(MailError::Upstream)?
            .iter()
            .any(|label| label.as_str() == Some("INBOX"))
        {
            return Err(MailError::Conflict);
        }
        let mut found = Vec::new();
        collect_gmail_parts(&detail["payload"], 0, &mut found)?;
        let omitted = found.len().saturating_sub(10);
        let mut items = Vec::new();
        let mut downloads = 0;
        let mut ids = std::collections::HashSet::new();
        for part in found.into_iter().take(10) {
            let id = text(&part, "partId", 1024);
            let name = text(&part, "filename", 512);
            let mime = text(&part, "mimeType", 256).to_ascii_lowercase();
            if id.is_empty() || !ids.insert(id.clone()) {
                return Err(MailError::Upstream);
            }
            let size = part["body"]["size"].as_u64().unwrap_or(0);
            let mut entry = RawAttachment {
                id,
                name,
                mime,
                bytes: None,
                status: "unsupported".into(),
            };
            if supported(&entry.name, &entry.mime) && size > MAX_BYTES as u64 {
                entry.status = "tooLarge".into();
            } else if supported(&entry.name, &entry.mime) && downloads < 2 {
                let encoded = if let Some(inline) = part["body"]["data"].as_str() {
                    inline.to_string()
                } else if let Some(attachment_id) = part["body"]["attachmentId"].as_str() {
                    if attachment_id.is_empty()
                        || attachment_id.len() > 1024
                        || !attachment_id
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
                    {
                        return Err(MailError::Upstream);
                    }
                    let bytes = self
                        .attachment_bytes(
                            self.http
                                .get(format!(
                                    "{}/users/me/messages/{message_id}/attachments/{attachment_id}",
                                    self.gmail_api
                                ))
                                .bearer_auth(access),
                            MAX_ENCODED + 1024,
                        )
                        .await?;
                    let value: Value =
                        serde_json::from_slice(&bytes).map_err(|_| MailError::Upstream)?;
                    value["data"]
                        .as_str()
                        .ok_or(MailError::Upstream)?
                        .to_string()
                } else {
                    return Err(MailError::Upstream);
                };
                if encoded.len() > MAX_ENCODED {
                    entry.status = "tooLarge".into();
                } else {
                    let bytes = URL_SAFE_NO_PAD
                        .decode(encoded.trim_end_matches('='))
                        .map_err(|_| MailError::Upstream)?;
                    if bytes.len() > MAX_BYTES {
                        entry.status = "tooLarge".into();
                    } else {
                        entry.bytes = Some(bytes);
                        entry.status = "ready".into();
                        downloads += 1;
                    }
                }
            } else if supported(&entry.name, &entry.mime) {
                entry.status = "omitted".into();
            }
            items.push(entry);
        }
        Ok(AttachmentBatch { items, omitted })
    }

    async fn outlook_attachments(
        &self,
        access: &str,
        message_id: &str,
    ) -> Result<AttachmentBatch, MailError> {
        let mut url = reqwest::Url::parse(&format!("{}/me/messages", self.graph_api))
            .map_err(|_| MailError::Invalid)?;
        url.path_segments_mut()
            .map_err(|_| MailError::Invalid)?
            .push(message_id)
            .push("attachments");
        let page = self
            .json(
                self.http
                    .get(url.clone())
                    .query(&[
                        ("$top", "11"),
                        ("$select", "id,name,contentType,size,isInline"),
                    ])
                    .bearer_auth(access),
                false,
            )
            .await?;
        let values = page["value"].as_array().ok_or(MailError::Upstream)?;
        if values.len() > 11 {
            return Err(MailError::Upstream);
        }
        let omitted =
            values.len().saturating_sub(10) + usize::from(page["@odata.nextLink"].is_string());
        let mut downloads = 0;
        let mut items = Vec::new();
        let mut ids = std::collections::HashSet::new();
        for attachment in values.iter().take(10) {
            let id = text(attachment, "id", 1024);
            let name = text(attachment, "name", 512);
            let mime = text(attachment, "contentType", 256).to_ascii_lowercase();
            if id.is_empty() || !ids.insert(id.clone()) {
                return Err(MailError::Upstream);
            }
            let mut entry = RawAttachment {
                id: id.clone(),
                name,
                mime,
                bytes: None,
                status: "unsupported".into(),
            };
            let file =
                attachment["@odata.type"].as_str() == Some("#microsoft.graph.fileAttachment");
            if file
                && supported(&entry.name, &entry.mime)
                && attachment["size"].as_u64().unwrap_or(0) > MAX_BYTES as u64
            {
                entry.status = "tooLarge".into();
            } else if file && supported(&entry.name, &entry.mime) && downloads < 2 {
                let mut content = url.clone();
                content
                    .path_segments_mut()
                    .map_err(|_| MailError::Invalid)?
                    .push(&id)
                    .push("$value");
                entry.bytes = Some(
                    self.attachment_bytes(self.http.get(content).bearer_auth(access), MAX_BYTES)
                        .await?,
                );
                entry.status = "ready".into();
                downloads += 1;
            } else if file && supported(&entry.name, &entry.mime) {
                entry.status = "omitted".into();
            }
            items.push(entry);
        }
        Ok(AttachmentBatch { items, omitted })
    }
}

#[cfg(test)]
#[path = "client_attachments_tests.rs"]
mod tests;
