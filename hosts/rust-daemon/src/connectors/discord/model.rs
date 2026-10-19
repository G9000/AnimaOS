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
}
impl DiscordChannelKind {
    /// Whether the channel is a direct message with one approved user. A guild
    /// channel is shared, so its messages may come from anyone approved.
    pub(crate) fn is_direct_message(&self) -> bool {
        match self {
            Self::GuildText => false,
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
