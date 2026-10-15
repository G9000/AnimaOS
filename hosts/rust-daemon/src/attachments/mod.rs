pub(crate) mod documents;
pub(crate) mod files;
pub(crate) mod metadata;
pub(crate) mod model;
pub(crate) mod validation;
// Private durable upload identity. HTTP callers project the public metadata only.
use serde::{Deserialize, Serialize};

pub(crate) const ATTACHMENT_RECORDS_KEY: &str = "attachmentRecords";
pub(crate) const ATTACHMENT_INPUT_TEXT_KEY: &str = "attachmentInputText";

pub(crate) const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;
pub(crate) const MAX_TEXT_BYTES: usize = 1024 * 1024;
pub(crate) const MAX_DOCUMENT_BYTES: usize = 25 * 1024 * 1024;
pub(crate) const MAX_UPLOAD_HEADER_BYTES: usize = crate::routes::MAX_FORM_HEADER_BYTES;
pub(crate) const MAX_UPLOAD_PARTS: usize = 1;
pub(crate) const MAX_UPLOAD_MULTIPART_OVERHEAD: usize = MAX_UPLOAD_HEADER_BYTES + 512;
pub(crate) const MAX_UPLOAD_FORM_BYTES: usize = MAX_DOCUMENT_BYTES + MAX_UPLOAD_MULTIPART_OVERHEAD;
pub(crate) const MAX_UPLOAD_NAME_BYTES: usize = 180;
pub(crate) const MAX_UPLOAD_COLLISION_ATTEMPTS: usize = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AttachmentKind {
    Image,
    Text,
    Document,
}
impl AttachmentKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Text => "text",
            Self::Document => "document",
        }
    }
    pub(crate) fn max_bytes(self) -> usize {
        match self {
            Self::Image => MAX_IMAGE_BYTES,
            Self::Text => MAX_TEXT_BYTES,
            Self::Document => MAX_DOCUMENT_BYTES,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AttachmentRecord {
    pub(crate) id: String,
    pub(crate) agent_id: String,
    pub(crate) session_id: String,
    pub(crate) name: String,
    pub(crate) mime: String,
    pub(crate) size: u64,
    pub(crate) kind: AttachmentKind,
    pub(crate) path: String,
    pub(crate) created_at_ms: u64,
    pub(crate) workspace_root: String,
    pub(crate) sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AttachmentMetadata {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) mime: String,
    pub(crate) size: u64,
    pub(crate) kind: AttachmentKind,
    pub(crate) path: String,
    pub(crate) created_at_ms: u64,
}
impl AttachmentRecord {
    pub(crate) fn metadata(&self) -> AttachmentMetadata {
        AttachmentMetadata {
            id: self.id.clone(),
            name: self.name.clone(),
            mime: self.mime.clone(),
            size: self.size,
            kind: self.kind,
            path: self.path.clone(),
            created_at_ms: self.created_at_ms,
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn attachment_public_metadata_omits_private_identity() {
        let row = crate::history::conformance::attachment_record("id", "agent", "session");
        let private = serde_json::to_value(&row).unwrap();
        assert_eq!(private["workspaceRoot"], "private-root");
        assert_eq!(private["sha256"], "digest");
        assert!(private.get("bytes").is_none());
        let public = serde_json::to_value(row.metadata()).unwrap();
        for key in ["workspaceRoot", "sha256", "agentId", "sessionId", "bytes"] {
            assert!(public.get(key).is_none(), "{key}");
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AttachmentError {
    Invalid,
    TooLarge,
    Unavailable,
    WorkspaceChanged,
    Changed,
    CollisionExhausted,
}
impl std::fmt::Display for AttachmentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Invalid => "Attachment filename or content is not supported",
            Self::TooLarge => "Attachment exceeds its size limit",
            Self::Unavailable => "Attachment file is unavailable or unreadable",
            Self::WorkspaceChanged => "Attachment belongs to a different workspace",
            Self::Changed => "Attachment file no longer matches its upload",
            Self::CollisionExhausted => "Attachment filename collision limit reached",
        })
    }
}
impl std::error::Error for AttachmentError {}
