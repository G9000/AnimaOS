use crate::agent_files::AgentFilesManifest;
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub(crate) struct AgentFilesPreview {
    pub(crate) mode: PreviewMode,
    #[serde(flatten)]
    pub(crate) manifest: AgentFilesManifest,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PreviewMode {
    CurrentPreview,
}
