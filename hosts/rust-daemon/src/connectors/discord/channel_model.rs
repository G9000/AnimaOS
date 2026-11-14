//! Guild channel inventory and deliberately limited channel mutation payloads.
use serde::{Deserialize, Serialize};

/// Discord may acknowledge an empty topic with either null or an empty string.
pub(crate) fn topic_matches(requested: &str, actual: Option<&str>) -> bool {
    actual == Some(requested) || (requested.is_empty() && actual.is_none())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DiscordGuildChannel {
    pub(crate) id: String,
    pub(crate) guild_id: String,
    pub(crate) name: String,
    #[serde(rename = "type")]
    pub(crate) kind: u8,
    #[serde(default)]
    pub(crate) topic: Option<String>,
    #[serde(default)]
    pub(crate) parent_id: Option<String>,
    #[serde(default)]
    pub(crate) position: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct DiscordChannelCreate {
    pub(crate) name: String,
    #[serde(rename = "type")]
    pub(crate) kind: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) topic: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) parent_id: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub(crate) struct DiscordChannelEdit {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) topic: Option<String>,
    /// None leaves the category unchanged; Some(None) removes its category.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) parent_id: Option<Option<String>>,
}
