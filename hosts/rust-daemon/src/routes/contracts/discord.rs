use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DiscordCreateRequest {
    pub(crate) bot_token: String,
    pub(crate) guild_id: String,
    pub(crate) channel_id: String,
    pub(crate) approved_user_ids: Vec<String>,
    pub(crate) private_channel_confirmed: bool,
}
#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DiscordCredentialRequest {
    pub(crate) bot_token: String,
}
#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DiscordEnabledRequest {
    pub(crate) enabled: bool,
}
#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DiscordSuppressRequest {
    pub(crate) acknowledge_uncertainty: bool,
}
#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DiscordBotResponse {
    pub(crate) id: String,
    pub(crate) username: Option<String>,
    pub(crate) display_name: Option<String>,
}
#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DiscordChannelResponse {
    pub(crate) id: String,
    pub(crate) guild_id: String,
    pub(crate) name: Option<String>,
    pub(crate) kind: String,
}
#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DiscordConnectorResponse {
    pub(crate) id: String,
    pub(crate) agent_id: String,
    #[serde(rename = "type")]
    pub(crate) kind: String,
    pub(crate) room_id: String,
    pub(crate) bot: DiscordBotResponse,
    pub(crate) channel: DiscordChannelResponse,
    pub(crate) approved_user_ids: Vec<String>,
    pub(crate) enabled: bool,
    pub(crate) status: String,
    pub(crate) created_at_ms: u64,
    pub(crate) updated_at_ms: u64,
    pub(crate) pending_inbound: usize,
    pub(crate) pending_outbound: usize,
    pub(crate) unknown_outbound: usize,
    pub(crate) delivered_message_ids: Vec<String>,
    pub(crate) warning: Option<String>,
}
#[derive(Serialize, ToSchema)]
pub(crate) struct DiscordConnectorEnvelope {
    pub(crate) connector: DiscordConnectorResponse,
}
#[derive(Serialize, ToSchema)]
pub(crate) struct DiscordConnectorsEnvelope {
    pub(crate) connectors: Vec<DiscordConnectorResponse>,
}
