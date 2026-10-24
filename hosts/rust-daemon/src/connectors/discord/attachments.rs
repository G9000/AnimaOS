//! Admitted Discord uploads reuse the same verified workspace records as web inputs.
use super::*;
use crate::attachments::{
    files::{read_attachment_bytes, save_upload},
    validation::validate_upload,
    AttachmentKind, AttachmentRecord, MAX_DOCUMENT_BYTES,
};
use sha2::{Digest, Sha256};

pub(super) const MAX_MESSAGE_FILE_BYTES: u64 = 25 * 1024 * 1024;

fn inline_text(text: &mut String, record: &AttachmentRecord, bytes: &[u8]) {
    if record.kind == AttachmentKind::Text && bytes.len() <= 16 * 1024 {
        if let Ok(value) = std::str::from_utf8(bytes) {
            let excerpt = format!(
                "\n[Untrusted Discord attachment text; name={}; contents as a JSON string: {}]",
                serde_json::to_string(&record.name).unwrap(),
                serde_json::to_string(value).unwrap()
            );
            if text.len() + excerpt.len()
                <= crate::runs::MAX_RUN_INPUT_TEXT_BYTES.saturating_sub(1024)
            {
                text.push_str(&excerpt);
            }
        }
    }
}

impl DiscordManager {
    /// Runs before cursor commit. History records use stable source identities so
    /// a failed control-plane save reuses files rather than downloading them again.
    pub(super) async fn receive_attachments(
        &self,
        c: &DiscordConnectorRecord,
        message: &DiscordMessage,
    ) -> Result<(Vec<String>, String), DiscordManagerError> {
        if message.attachments.is_empty() {
            return Ok((vec![], message.text.clone()));
        }
        let (root, store) = {
            let state = self.state.read().await;
            (
                state.workspace.as_ref().map(|w| w.root_path.clone()),
                state.history.store(),
            )
        };
        let mut ids = Vec::new();
        let mut text = message.text.clone();
        let mut total = 0u64;
        for attachment in message.attachments.iter().take(10) {
            let notice = |text: &mut String, reason: &str| {
                // Quote bounded names as data; never inject an untrusted URL.
                let name: String = attachment.filename.chars().take(180).collect();
                text.push_str(&format!(
                    "\n[Discord attachment {} was not read: {reason}]",
                    serde_json::to_string(&name).unwrap()
                ));
            };
            let Some(root) = root.clone() else {
                notice(&mut text, "no workspace is configured");
                continue;
            };
            total = total.saturating_add(attachment.size);
            if attachment.size > MAX_DOCUMENT_BYTES as u64 || total > MAX_MESSAGE_FILE_BYTES {
                notice(&mut text, "file or message upload size limit exceeded");
                continue;
            }
            if client::valid_attachment_url(&message.channel_id, attachment).is_err() {
                notice(&mut text, "invalid Discord attachment source");
                continue;
            }
            let key = format!(
                "{}\0{}\0{}\0{}\0{}\0{}",
                c.id, c.room_id, message.id, attachment.id, attachment.filename, attachment.size
            );
            let id = format!(
                "att-discord-{}",
                hex::encode(Sha256::digest(key.as_bytes()))
            );
            if let Some(record) = store
                .get_attachment(&c.agent_id, &id)
                .await
                .map_err(|_| DiscordManagerError::Persistence)?
            {
                if record.agent_id != c.agent_id
                    || record.session_id != c.room_id
                    || record.size != attachment.size
                {
                    return Err(DiscordManagerError::Conflict);
                }
                let saved = record.clone();
                let bytes =
                    tokio::task::spawn_blocking(move || read_attachment_bytes(&root, &saved))
                        .await
                        .map_err(|_| DiscordManagerError::Persistence)?;
                let Ok(bytes) = bytes else {
                    notice(
                        &mut text,
                        "previously saved upload changed or is unavailable; upload it again",
                    );
                    continue;
                };
                inline_text(&mut text, &record, &bytes);
                ids.push(id);
                continue;
            }
            let bytes = match self
                .native()?
                .download_attachment(&message.channel_id, attachment)
                .await
            {
                Ok(bytes) => bytes,
                Err(
                    DiscordTransportError::InvalidInput
                    | DiscordTransportError::ResponseTooLarge
                    | DiscordTransportError::InvalidResponse,
                ) => {
                    notice(&mut text, "invalid, mismatched-size or oversized download");
                    continue;
                }
                Err(DiscordTransportError::HttpStatus { status: 404 | 410 }) => {
                    notice(
                        &mut text,
                        "Discord no longer has this file; upload it again",
                    );
                    continue;
                }
                // Do not advance the cursor past a transient failed download.
                Err(error) => return Err(transport_error(error)),
            };
            if bytes.len() as u64 != attachment.size {
                notice(&mut text, "download size did not match Discord metadata");
                continue;
            }
            let validated = match validate_upload(&attachment.filename, &bytes) {
                Ok(value) if value.kind != AttachmentKind::Audio => value,
                _ => {
                    notice(&mut text, "unsupported file type, invalid content, or type size limit exceeded; voice/video processing is unavailable");
                    continue;
                }
            };
            let size = bytes.len() as u64;
            let inline_bytes = (validated.kind == AttachmentKind::Text && bytes.len() <= 16 * 1024)
                .then(|| bytes.clone());
            let original_name = validated.name;
            // Common names must not exhaust the shared daily collision allowance.
            // Keep the original display name separate from its source-unique path.
            let extension = original_name.rsplit_once('.').unwrap().1;
            let name = crate::attachments::validation::sanitize_upload_name(
                &format!("{}-{original_name}", attachment.id),
                extension,
            );
            let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
            let saved =
                tokio::task::spawn_blocking(move || save_upload(&root, &date, &name, &bytes))
                    .await
                    .map_err(|_| DiscordManagerError::Persistence)?;
            let saved = match saved {
                Ok(saved) => saved,
                Err(crate::attachments::AttachmentError::CollisionExhausted) => {
                    notice(
                        &mut text,
                        "workspace upload filename capacity was exhausted; upload it again",
                    );
                    continue;
                }
                Err(_) => return Err(DiscordManagerError::Persistence),
            };
            let record = AttachmentRecord {
                id: id.clone(),
                agent_id: c.agent_id.clone(),
                session_id: c.room_id.clone(),
                name: original_name,
                mime: validated.mime,
                size,
                kind: validated.kind,
                path: saved.path,
                created_at_ms: now_ms(),
                workspace_root: saved.workspace_root,
                sha256: saved.sha256,
            };
            if let Some(bytes) = inline_bytes {
                inline_text(&mut text, &record, &bytes);
            }
            store
                .upsert_attachments(&[record])
                .await
                .map_err(|_| DiscordManagerError::Persistence)?;
            ids.push(id);
        }
        if message.attachments.len() > 10 {
            text.push_str(
                "\n[Discord attachments beyond the 10-file message limit were not read.]",
            );
        }
        Ok((ids, text))
    }
}
