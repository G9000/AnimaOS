//! Native Discord identities and journals. Recovery behavior attribution: HERMES-NOTICE.
use serde::{Deserialize, Serialize};

/// Discord IDs stay decimal strings across every persistence/API boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct DiscordSnowflake(u64);
impl DiscordSnowflake {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        if value.is_empty()
            || value.len() > 20
            || value.starts_with('0')
            || !value.bytes().all(|b| b.is_ascii_digit())
        {
            return None;
        }
        value.parse::<u64>().ok().filter(|id| *id > 0).map(Self)
    }
    pub(crate) fn value(self) -> u64 {
        self.0
    }
    pub(crate) fn cursor(value: &str) -> Option<u64> {
        if value == "0" {
            Some(0)
        } else {
            Self::parse(value).map(Self::value)
        }
    }
}
impl Serialize for DiscordSnowflake {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string())
    }
}
impl<'de> Deserialize<'de> for DiscordSnowflake {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).ok_or_else(|| serde::de::Error::custom("invalid Discord snowflake"))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DiscordBotIdentity {
    pub(crate) id: String,
    pub(crate) username: Option<String>,
    pub(crate) display_name: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum DiscordChannelKind {
    GuildText,
    GuildThread,
}
impl DiscordChannelKind {
    /// Whether the channel is a direct message with one approved user. A guild
    /// channel is shared, so its messages may come from anyone approved.
    pub(crate) fn is_direct_message(&self) -> bool {
        match self {
            Self::GuildText | Self::GuildThread => false,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DiscordChannelMetadata {
    pub(crate) id: String,
    pub(crate) guild_id: String,
    pub(crate) name: Option<String>,
    pub(crate) kind: DiscordChannelKind,
    #[serde(default)]
    pub(crate) parent_channel_id: Option<String>,
}
impl DiscordChannelMetadata {
    pub(crate) fn routing_eq(&self, other: &Self) -> bool {
        self.id == other.id
            && self.guild_id == other.guild_id
            && self.kind == other.kind
            && self.parent_channel_id == other.parent_channel_id
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct DiscordSettings {
    pub(crate) allowed_channel_ids: Vec<String>,
    pub(crate) free_response_channel_ids: Vec<String>,
    pub(crate) approved_role_ids: Vec<String>,
    pub(crate) home_channel_id: Option<String>,
    pub(crate) require_mention: bool,
    pub(crate) auto_thread: bool,
    pub(crate) thread_require_mention: bool,
}
impl Default for DiscordSettings {
    fn default() -> Self {
        Self {
            allowed_channel_ids: vec![],
            free_response_channel_ids: vec![],
            approved_role_ids: vec![],
            home_channel_id: None,
            require_mention: true,
            auto_thread: true,
            thread_require_mention: false,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DiscordBotCheck {
    pub(crate) application_id: String,
    pub(crate) bot: DiscordBotIdentity,
    pub(crate) owner_user_ids: Vec<String>,
    pub(crate) message_content_enabled: bool,
    pub(crate) invite_url: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DiscordConversation {
    pub(crate) channel: DiscordChannelMetadata,
    pub(crate) room_id: String,
    pub(crate) cursor: String,
    #[serde(default)]
    pub(crate) participating: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DiscordThreadStart {
    pub(crate) channel_id: String,
    pub(crate) thread_channel_id: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DiscordSenderMetadata {
    pub(crate) id: String,
    pub(crate) username: Option<String>,
    pub(crate) display_name: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DiscordMessage {
    pub(crate) id: String,
    pub(crate) channel_id: String,
    pub(crate) guild_id: Option<String>,
    pub(crate) sender: DiscordSenderMetadata,
    pub(crate) text: String,
    pub(crate) is_bot: bool,
    pub(crate) webhook_id: Option<String>,
    pub(crate) message_type: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DiscordMessageBatch {
    pub(crate) messages: Vec<DiscordMessage>,
    pub(crate) next_cursor: String,
    pub(crate) boundary_proven: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DiscordSendReceipt {
    pub(crate) message_id: String,
    pub(crate) channel_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DiscordConnectorRecord {
    pub(crate) id: String,
    pub(crate) agent_id: String,
    pub(crate) room_id: String,
    pub(crate) bot: DiscordBotIdentity,
    pub(crate) channel: DiscordChannelMetadata,
    #[serde(default)]
    pub(crate) bot_wide: bool,
    #[serde(default)]
    pub(crate) guild_scope: Option<String>,
    #[serde(default)]
    pub(crate) application_id: Option<String>,
    /// None preserves the inline admission policy of pre-settings records.
    #[serde(default)]
    pub(crate) settings: Option<DiscordSettings>,
    #[serde(default)]
    pub(crate) configuration_revision: u64,
    #[serde(default)]
    pub(crate) conversations: Vec<DiscordConversation>,
    /// A persisted None is an attempted thread creation with an unknown/failed result.
    /// It is never automatically retried, including after a crash.
    #[serde(default)]
    pub(crate) thread_starts: std::collections::BTreeMap<String, DiscordThreadStart>,
    pub(crate) approved_user_ids: Vec<String>,
    pub(crate) credential_generation: u64,
    pub(crate) cursor: String,
    #[serde(default)]
    pub(crate) activated_at_ms: Option<u64>,
    pub(crate) enabled: bool,
    #[serde(default)]
    pub(crate) deleted_at_ms: Option<u64>,
    pub(crate) created_at_ms: u64,
    pub(crate) updated_at_ms: u64,
}
impl DiscordConnectorRecord {
    pub(crate) fn is_active(&self) -> bool {
        self.enabled && self.deleted_at_ms.is_none()
    }
    pub(crate) fn destination_for_room(&self, room: &str) -> Option<Self> {
        if !self.bot_wide && self.room_id == room {
            return Some(self.clone());
        }
        let conversation = self.conversations.iter().find(|v| v.room_id == room)?;
        let mut result = self.clone();
        result.room_id = conversation.room_id.clone();
        result.channel = conversation.channel.clone();
        result.cursor = conversation.cursor.clone();
        Some(result)
    }
    pub(crate) fn destination_for_channel(&self, channel: &str) -> Option<Self> {
        if !self.bot_wide && self.channel.id == channel {
            return Some(self.clone());
        }
        let room = &self
            .conversations
            .iter()
            .find(|v| v.channel.id == channel)?
            .room_id;
        self.destination_for_room(room)
    }
    pub(crate) fn owns_room(&self, room: &str) -> bool {
        (!self.bot_wide && self.room_id == room)
            || self.conversations.iter().any(|v| v.room_id == room)
    }
    pub(crate) fn home_destination(&self) -> Option<Self> {
        let id = self.settings.as_ref()?.home_channel_id.as_deref()?;
        let destination = self.destination_for_channel(id)?;
        self.can_send_to_channel(&destination.channel)
            .then_some(destination)
    }
    pub(crate) fn can_send_to_channel(&self, channel: &DiscordChannelMetadata) -> bool {
        self.channel_allowed(channel)
            || (self
                .guild_scope
                .as_ref()
                .is_none_or(|guild| guild == &channel.guild_id)
                && self
                    .settings
                    .as_ref()
                    .and_then(|s| s.home_channel_id.as_deref())
                    == Some(channel.id.as_str()))
    }
    pub(crate) fn channel_allowed(&self, channel: &DiscordChannelMetadata) -> bool {
        if self
            .guild_scope
            .as_ref()
            .is_some_and(|g| g != &channel.guild_id)
        {
            return false;
        }
        let Some(settings) = &self.settings else {
            return channel.routing_eq(&self.channel);
        };
        let channel_key =
            |id: &str| id == channel.id || channel.parent_channel_id.as_deref() == Some(id);
        if settings.allowed_channel_ids.is_empty() {
            self.bot_wide || channel_key(&self.channel.id)
        } else {
            settings
                .allowed_channel_ids
                .iter()
                .any(|id| channel_key(id))
        }
    }
    pub(super) fn matches_destination(&self, captured: &Self) -> bool {
        self.is_active()
            && self.agent_id == captured.agent_id
            && self.bot == captured.bot
            && self.credential_generation == captured.credential_generation
            && self.configuration_revision == captured.configuration_revision
            && self.settings == captured.settings
            && self.can_send_to_channel(&captured.channel)
            && self
                .destination_for_room(&captured.room_id)
                .is_some_and(|v| v.channel.routing_eq(&captured.channel))
    }
    pub(super) fn destinations(&self) -> Vec<Self> {
        let mut result = Vec::new();
        if !self.bot_wide && self.channel_allowed(&self.channel) {
            result.push(self.clone());
        }
        for conversation in &self.conversations {
            if result
                .iter()
                .any(|v| v.channel.id == conversation.channel.id)
                || !self.channel_allowed(&conversation.channel)
            {
                continue;
            }
            if let Some(destination) = self.destination_for_room(&conversation.room_id) {
                result.push(destination);
            }
        }
        result
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum DiscordCredentialOperation {
    Create,
    Replace,
    Delete,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DiscordCredentialIntent {
    pub(crate) connector_id: String,
    pub(crate) generation: u64,
    pub(crate) operation: DiscordCredentialOperation,
    pub(crate) created_at_ms: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum DiscordProcessingState {
    Received,
    Processing,
    Processed,
    Rejected,
    Stopped,
}
impl DiscordProcessingState {
    pub(crate) fn is_terminal(&self) -> bool {
        matches!(self, Self::Processed | Self::Rejected | Self::Stopped)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DiscordInboundRecord {
    pub(crate) connector_id: String,
    pub(crate) message_id: String,
    pub(crate) agent_id: String,
    pub(crate) room_id: String,
    pub(crate) sender: DiscordSenderMetadata,
    pub(crate) channel_id: String,
    pub(crate) text: String,
    pub(crate) received_at_ms: u64,
    pub(crate) processing_state: DiscordProcessingState,
    pub(crate) run_idempotency_key: String,
    #[serde(default)]
    pub(crate) run_id: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum DiscordDeliveryState {
    Pending,
    Sending,
    Delivered,
    Failed,
    Unknown,
    Suppressed,
}
impl DiscordDeliveryState {
    pub(crate) fn is_settled(&self) -> bool {
        matches!(self, Self::Delivered | Self::Suppressed)
    }
    pub(crate) fn awaits_delivery(&self) -> bool {
        matches!(self, Self::Pending | Self::Failed)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DiscordOutboundChunk {
    pub(crate) index: u8,
    pub(crate) text: String,
    pub(crate) state: DiscordDeliveryState,
    pub(crate) attempt_id: String,
    pub(crate) provider_message_id: Option<String>,
    pub(crate) attempts: u32,
    #[serde(default)]
    pub(crate) retry_at_ms: Option<u64>,
    #[serde(default)]
    pub(crate) error_category: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DiscordOutboundRecord {
    pub(crate) id: String,
    pub(crate) connector_id: String,
    pub(crate) agent_id: String,
    pub(crate) room_id: String,
    pub(crate) assistant_message_id: String,
    pub(crate) text: String,
    pub(crate) created_at_ms: u64,
    pub(crate) delivery_state: DiscordDeliveryState,
    pub(crate) chunks: Vec<DiscordOutboundChunk>,
    #[serde(default)]
    pub(crate) message_pruned: bool,
    #[serde(default)]
    pub(crate) stop_requested: bool,
}
