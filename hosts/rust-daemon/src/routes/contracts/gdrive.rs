use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DriveConnectInput {
    pub(crate) selected_file_ids: Vec<String>,
}
#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DriveConnectorResponse {
    pub(crate) id: String,
    pub(crate) agent_id: String,
    #[serde(rename = "type")]
    pub(crate) kind: String,
    pub(crate) status: String,
    pub(crate) account_label: Option<String>,
    pub(crate) selected_file_ids: Vec<String>,
    pub(crate) requested_scope: String,
    pub(crate) enforcement: String,
    pub(crate) cleanup_pending: bool,
    pub(crate) created_at_ms: u64,
    pub(crate) updated_at_ms: u64,
}
impl From<&crate::connectors::drive::DriveRecord> for DriveConnectorResponse {
    fn from(r: &crate::connectors::drive::DriveRecord) -> Self {
        use crate::connectors::drive::DriveStatus;
        Self {
            id: r.id.clone(),
            agent_id: r.agent_id.clone(),
            kind: "gdrive".into(),
            status: match r.status {
                DriveStatus::Pairing => "pairing",
                DriveStatus::Active => "active",
                DriveStatus::ReauthRequired => "reauthRequired",
                DriveStatus::Disconnected => "disconnected",
            }
            .into(),
            account_label: r.account_label.clone(),
            selected_file_ids: r
                .selected_file_ids
                .iter()
                .map(|id| id.as_str().into())
                .collect(),
            requested_scope: crate::connectors::drive::DRIVE_SCOPE.into(),
            enforcement: "animaSelectedFiles".into(),
            cleanup_pending: !r.cleanup_generations.is_empty(),
            created_at_ms: r.created_at_ms,
            updated_at_ms: r.updated_at_ms,
        }
    }
}
#[derive(Serialize, ToSchema)]
pub(crate) struct DriveStatusEnvelope {
    pub(crate) configured: bool,
    pub(crate) connector: Option<DriveConnectorResponse>,
}
#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DriveConnectEnvelope {
    pub(crate) connector: DriveConnectorResponse,
    pub(crate) consent_url: String,
}
