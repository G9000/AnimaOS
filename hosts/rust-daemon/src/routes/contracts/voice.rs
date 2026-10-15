use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TranscriptionRequest {
    pub(crate) operation_id: String,
    pub(crate) attachment_id: String,
    pub(crate) provider: VoiceProvider,
    pub(crate) consent: bool,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum VoiceProvider {
    Openai,
}
#[derive(Clone, Copy, Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum VoiceReason {
    Available,
    Disabled,
    MissingCredentials,
    UnsupportedConfiguration,
    Unavailable,
}
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct VoiceCapabilities {
    pub(crate) version: u8,
    pub(crate) provider: VoiceProvider,
    pub(crate) model: String,
    pub(crate) available: bool,
    pub(crate) reason: VoiceReason,
}
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TranscriptionResponse {
    pub(crate) version: u8,
    pub(crate) operation_id: String,
    pub(crate) attachment_id: String,
    pub(crate) provider: VoiceProvider,
    pub(crate) model: String,
    pub(crate) text: String,
}
