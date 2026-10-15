use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SpeechRequest {
    pub(crate) operation_id: String,
    pub(crate) message_id: String,
    pub(crate) text: String,
    pub(crate) consent: bool,
}
#[derive(Clone, Copy, Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SpeechReason {
    Available,
    Disabled,
    MissingCredentials,
    UnsupportedConfiguration,
    Unavailable,
}
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SpeechCapabilities {
    pub(crate) version: u8,
    pub(crate) provider: super::VoiceProvider,
    pub(crate) model: String,
    pub(crate) voice: String,
    pub(crate) format: String,
    pub(crate) available: bool,
    pub(crate) reason: SpeechReason,
    pub(crate) max_input_chars: usize,
    pub(crate) max_input_bytes: usize,
    pub(crate) max_audio_bytes: usize,
    pub(crate) max_duration_ms: u32,
}
