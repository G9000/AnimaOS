//! Bounded CDN reads and native multipart uploads. CDN requests never carry bot auth.
use super::*;
pub(super) const MAX_SEND_FILE_BYTES: usize = 10 * 1024 * 1024;

pub(in crate::connectors::discord) fn valid_attachment_url(
    channel: &str,
    attachment: &DiscordAttachment,
) -> Result<Url, DiscordTransportError> {
    validate_id(channel)?;
    validate_id(&attachment.id)?;
    if attachment.url.len() > 8192 || attachment.filename.len() > 1024 {
        return Err(DiscordTransportError::InvalidInput);
    }
    let url = Url::parse(&attachment.url).map_err(|_| DiscordTransportError::InvalidInput)?;
    let parts = url
        .path_segments()
        .map(|v| v.collect::<Vec<_>>())
        .unwrap_or_default();
    if url.scheme() != "https"
        || !matches!(
            url.host_str(),
            Some("cdn.discordapp.com" | "media.discordapp.net")
        )
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || parts.len() != 4
        || parts[0] != "attachments"
        || parts[1] != channel
        || parts[2] != attachment.id
        || parts[3].is_empty()
    {
        return Err(DiscordTransportError::InvalidInput);
    }
    Ok(url)
}

impl DiscordClient {
    pub(super) async fn download_file(
        &self,
        channel: &str,
        attachment: &DiscordAttachment,
    ) -> Result<Vec<u8>, DiscordTransportError> {
        let url = valid_attachment_url(channel, attachment)?;
        let max = crate::attachments::MAX_DOCUMENT_BYTES;
        if attachment.size > max as u64 {
            return Err(DiscordTransportError::ResponseTooLarge);
        }
        // Same no-redirect, time-bounded client; Authorization is added only by REST request_body.
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|_| DiscordTransportError::Transport)?;
        Self::download_response(response, attachment.size).await
    }

    pub(super) async fn download_response(
        response: reqwest::Response,
        expected: u64,
    ) -> Result<Vec<u8>, DiscordTransportError> {
        let max = crate::attachments::MAX_DOCUMENT_BYTES;
        if matches!(response.status().as_u16(), 404 | 410) {
            return Err(DiscordTransportError::HttpStatus {
                status: response.status().as_u16(),
            });
        }
        if !response.status().is_success() {
            return Err(DiscordTransportError::Transport);
        }
        if response
            .content_length()
            .is_some_and(|v| v > expected || v > max as u64)
        {
            return Err(DiscordTransportError::ResponseTooLarge);
        }
        let mut bytes = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(part) = stream.next().await {
            let part = part.map_err(|_| DiscordTransportError::Transport)?;
            if bytes.len().saturating_add(part.len()) > max
                || bytes.len().saturating_add(part.len()) as u64 > expected
            {
                return Err(DiscordTransportError::ResponseTooLarge);
            }
            bytes.extend_from_slice(&part);
        }
        if bytes.len() as u64 != expected {
            return Err(DiscordTransportError::InvalidResponse);
        }
        Ok(bytes)
    }

    pub(super) async fn upload_file(
        &self,
        token: &DiscordBotToken,
        channel: &str,
        file: &DiscordFileUpload,
        caption: &str,
        attempt_id: &str,
    ) -> Result<DiscordFileReceipt, DiscordTransportError> {
        validate_id(channel)?;
        if file.bytes.len() > MAX_SEND_FILE_BYTES
            || caption.encode_utf16().count() > 2000
            || attempt_id.is_empty()
            || attempt_id.len() > 256
        {
            return Err(DiscordTransportError::InvalidInput);
        }
        let validated =
            crate::attachments::validation::validate_upload(&file.filename, &file.bytes)
                .map_err(|_| DiscordTransportError::InvalidInput)?;
        if validated.name != file.filename
            || validated.mime != file.mime
            || validated.kind == crate::attachments::AttachmentKind::Audio
        {
            return Err(DiscordTransportError::InvalidInput);
        }
        let payload = serde_json::json!({"content":caption,"allowed_mentions":{"parse":[],"replied_user":false},"nonce":stable_nonce(attempt_id),"enforce_nonce":true,"attachments":[{"id":0,"filename":file.filename}]}).to_string();
        let boundary = format!("anima-{}", uuid::Uuid::new_v4());
        let mut body = format!("--{boundary}\r\nContent-Disposition: form-data; name=\"payload_json\"\r\nContent-Type: application/json\r\n\r\n{payload}\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"files[0]\"; filename=\"{}\"\r\nContent-Type: {}\r\n\r\n", file.filename, file.mime).into_bytes();
        body.extend_from_slice(&file.bytes);
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
        let value: serde_json::Value = self
            .request_body(
                token,
                Method::POST,
                &format!("channels/{channel}/messages"),
                None,
                Some((body, format!("multipart/form-data; boundary={boundary}"))),
            )
            .await?;
        file_receipt(value, channel, file)
    }
}

fn file_receipt(
    value: serde_json::Value,
    channel: &str,
    file: &DiscordFileUpload,
) -> Result<DiscordFileReceipt, DiscordTransportError> {
    let id = value["id"]
        .as_str()
        .ok_or(DiscordTransportError::Uncertain)?;
    let attachments = value["attachments"]
        .as_array()
        .ok_or(DiscordTransportError::Uncertain)?;
    let attachment = attachments
        .first()
        .filter(|_| attachments.len() == 1)
        .ok_or(DiscordTransportError::Uncertain)?;
    let attachment_id = attachment["id"]
        .as_str()
        .ok_or(DiscordTransportError::Uncertain)?;
    if provider_id(id).is_err()
        || provider_id(attachment_id).is_err()
        || value["channel_id"].as_str() != Some(channel)
        || attachment["filename"].as_str() != Some(&file.filename)
        || attachment["size"].as_u64() != Some(file.bytes.len() as u64)
    {
        return Err(DiscordTransportError::Uncertain);
    }
    Ok(DiscordFileReceipt {
        sent: true,
        message_id: id.into(),
        channel_id: channel.into(),
        attachment_id: attachment_id.into(),
        filename: file.filename.clone(),
        size: file.bytes.len() as u64,
    })
}
