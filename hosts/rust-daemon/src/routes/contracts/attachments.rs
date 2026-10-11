use crate::attachments::{AttachmentMetadata, AttachmentRecord};
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = UploadedAttachment)]
pub(crate) struct AttachmentResponse {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) mime: String,
    pub(crate) size: u64,
    pub(crate) kind: String,
    pub(crate) path: String,
    pub(crate) created_at_ms: u64,
}
impl From<AttachmentMetadata> for AttachmentResponse {
    fn from(row: AttachmentMetadata) -> Self {
        Self {
            id: row.id,
            name: row.name,
            mime: row.mime,
            size: row.size,
            kind: row.kind.as_str().into(),
            path: row.path,
            created_at_ms: row.created_at_ms,
        }
    }
}
impl From<&AttachmentRecord> for AttachmentResponse {
    fn from(row: &AttachmentRecord) -> Self {
        row.metadata().into()
    }
}
#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct AttachmentEnvelope {
    pub(crate) attachment: AttachmentResponse,
}
