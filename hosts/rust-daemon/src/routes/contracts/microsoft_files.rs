use crate::connectors::microsoft_files::model::{
    MicrosoftFileMode, MicrosoftFileRecord, MicrosoftFileStatus,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MicrosoftFilesConnectInput {
    pub(crate) mode: MicrosoftFileMode,
    pub(crate) approved_site_url: Option<String>,
}
#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MicrosoftFilesSelectionInput {
    pub(crate) expected_generation: String,
    pub(crate) expected_selection_revision: u64,
    pub(crate) file_keys: Vec<String>,
}
#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MicrosoftFilesDisconnectInput {
    pub(crate) expected_generation: String,
}
#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MicrosoftSelectedFileResponse {
    key: String,
    name: String,
}
#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MicrosoftFileConnectorResponse {
    id: String,
    mode: MicrosoftFileMode,
    status: MicrosoftFileStatus,
    generation: String,
    selection_revision: u64,
    account_label: Option<String>,
    approved_site_url: Option<String>,
    selected_files: Vec<MicrosoftSelectedFileResponse>,
}
impl From<&MicrosoftFileRecord> for MicrosoftFileConnectorResponse {
    fn from(record: &MicrosoftFileRecord) -> Self {
        Self {
            id: record.id.clone(),
            mode: record.mode,
            status: record.status,
            generation: record.generation.clone(),
            selection_revision: record.selection_revision,
            account_label: record.account_label.clone(),
            approved_site_url: record.approved_site_url.clone(),
            selected_files: record
                .selected_files
                .iter()
                .map(|file| MicrosoftSelectedFileResponse {
                    key: file.key.clone(),
                    name: file.name.clone(),
                })
                .collect(),
        }
    }
}
#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MicrosoftFilesStatusResponse {
    pub(crate) configured: bool,
    pub(crate) connectors: Vec<MicrosoftFileConnectorResponse>,
}
#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MicrosoftFilesSelectionResponse {
    pub(crate) connector: MicrosoftFileConnectorResponse,
}
