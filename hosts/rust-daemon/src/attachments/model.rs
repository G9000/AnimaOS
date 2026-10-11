//! Hydrates only a session's selected, daemon-owned image references at the model boundary.
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use anima_core::{
    AgentConfig, Attachment, AttachmentType, DataValue, MessageRole, ModelAdapter,
    ModelGenerateRequest, ModelGenerateResponse, ModelStreamSink, MAX_CONTEXT_IMAGES,
};
use anima_model_adapters::{image_reference_text, model_supports_images};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine};

use super::{
    files::read_attachment_bytes, AttachmentError, AttachmentKind, AttachmentRecord,
    ATTACHMENT_INPUT_TEXT_KEY, ATTACHMENT_RECORDS_KEY, MAX_IMAGE_BYTES,
};
use crate::history::HistoryStore;

const MAX_ENCODED_IMAGE_BYTES: usize = 4 * MAX_IMAGE_BYTES.div_ceil(3);
type AttachmentReader =
    Arc<dyn Fn(&Path, &AttachmentRecord) -> Result<Vec<u8>, AttachmentError> + Send + Sync>;

pub(crate) struct AttachmentModelAdapter {
    inner: Arc<dyn ModelAdapter>,
    store: Arc<dyn HistoryStore>,
    workspace_root: Option<PathBuf>,
    agent_id: String,
    session_id: String,
}

impl AttachmentModelAdapter {
    pub(crate) fn new(
        inner: Arc<dyn ModelAdapter>,
        store: Arc<dyn HistoryStore>,
        workspace_root: Option<PathBuf>,
        agent_id: String,
        session_id: String,
    ) -> Self {
        Self {
            inner,
            store,
            workspace_root,
            agent_id,
            session_id,
        }
    }

    pub(crate) async fn prepare_request(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
    ) -> Result<ModelGenerateRequest, String> {
        self.prepare_request_using_reader(config, request, Arc::new(read_attachment_bytes))
            .await
    }

    async fn prepare_request_using_reader(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
        reader: AttachmentReader,
    ) -> Result<ModelGenerateRequest, String> {
        let mut prepared = request.clone();
        let supports_images = model_supports_images(
            config.provider.as_deref().unwrap_or(self.inner.provider()),
            &config.model,
        );
        let mut image_count = 0;
        // Match the context selector's cap: newest turns first, declared order within a turn.
        for message in prepared.messages.iter_mut().rev() {
            let references = message
                .content
                .metadata
                .as_mut()
                .and_then(|metadata| metadata.remove(ATTACHMENT_RECORDS_KEY));
            if let Some(metadata) = &mut message.content.metadata {
                metadata.remove(ATTACHMENT_INPUT_TEXT_KEY);
                if metadata.is_empty() {
                    message.content.metadata = None;
                }
            }
            let Some(attachments) = message.content.attachments.take() else {
                continue;
            };
            let mut retained = Vec::with_capacity(attachments.len());
            for mut attachment in attachments {
                if attachment.attachment_type != AttachmentType::Image {
                    retained.push(attachment);
                    continue;
                }
                // Providers serialize user images only. Other roles keep their text/tool metadata.
                if message.role != MessageRole::User {
                    continue;
                }
                let within_cap = image_count < MAX_CONTEXT_IMAGES;
                image_count += 1;
                let same_scope = message.agent_id == self.agent_id
                    && crate::sessions::session_id_for_room(&message.room_id) == self.session_id;
                if supports_images
                    && within_cap
                    && same_scope
                    && self
                        .hydrate(&mut attachment, references.as_ref(), &reader)
                        .await
                {
                    retained.push(attachment);
                    continue;
                }
                let reason = if !supports_images {
                    "the model cannot view this image"
                } else if !within_cap {
                    "image omitted by the four-image context limit"
                } else {
                    "image unavailable; the model cannot view this image"
                };
                if !message.content.text.is_empty() {
                    message.content.text.push('\n');
                }
                message.content.text.push_str(&image_reference_text(
                    &attachment.name,
                    &attachment.data,
                    reason,
                ));
            }
            message.content.attachments = (!retained.is_empty()).then_some(retained);
        }
        Ok(prepared)
    }

    async fn hydrate(
        &self,
        attachment: &mut Attachment,
        references: Option<&DataValue>,
        reader: &AttachmentReader,
    ) -> bool {
        let Some(root) = self.workspace_root.as_deref() else {
            return false;
        };
        let Some(DataValue::Array(references)) = references else {
            return false;
        };
        let Some(mime) = attachment.mime_type.as_deref().filter(|mime| {
            matches!(
                *mime,
                "image/png" | "image/jpeg" | "image/webp" | "image/gif"
            )
        }) else {
            return false;
        };
        let id = references.iter().find_map(|reference| {
            let DataValue::Object(reference) = reference else {
                return None;
            };
            let string = |key: &str| match reference.get(key) {
                Some(DataValue::String(value)) => Some(value.as_str()),
                _ => None,
            };
            (string("name") == Some(attachment.name.as_str())
                && string("path") == Some(attachment.data.as_str())
                && string("mime") == Some(mime)
                && string("kind") == Some("image"))
            .then(|| string("id"))
            .flatten()
        });
        let Some(id) = id else { return false };
        let Ok(Some(record)) = self.store.get_attachment(&self.agent_id, id).await else {
            return false;
        };
        if record.id != id
            || record.agent_id != self.agent_id
            || record.session_id != self.session_id
            || record.kind != AttachmentKind::Image
            || record.name != attachment.name
            || record.path != attachment.data
            || record.mime != mime
            || record.size > MAX_IMAGE_BYTES as u64
        {
            return false;
        }
        let root = root.to_path_buf();
        let reader = Arc::clone(reader);
        // Read/hash/validation and encoding are bounded blocking work. Only owned
        // values cross into the worker; dropping this await lets cancellation win.
        let encoded = tokio::task::spawn_blocking(move || {
            let bytes = reader(&root, &record).ok()?;
            if bytes.len() > MAX_IMAGE_BYTES || bytes.len() as u64 != record.size {
                return None;
            }
            let encoded = STANDARD.encode(bytes);
            if encoded.len() > MAX_ENCODED_IMAGE_BYTES {
                return None;
            }
            Some(format!("data:{};base64,{encoded}", record.mime))
        })
        .await;
        let Ok(Some(encoded)) = encoded else {
            return false;
        };
        attachment.data = encoded;
        true
    }
}

#[async_trait]
impl ModelAdapter for AttachmentModelAdapter {
    fn provider(&self) -> &str {
        self.inner.provider()
    }

    async fn generate(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        let prepared = self.prepare_request(config, request).await?;
        self.inner.generate(config, &prepared).await
    }

    async fn stream(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
        sink: &dyn ModelStreamSink,
    ) -> Result<(), String> {
        let prepared = self.prepare_request(config, request).await?;
        self.inner.stream(config, &prepared, sink).await
    }
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
