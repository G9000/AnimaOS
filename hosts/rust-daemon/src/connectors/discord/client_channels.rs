//! Bounded channel REST operations; caller capabilities enforce live guild authority.
use super::{provider_id, validate_id, DiscordClient, DiscordTransportError};
use crate::connectors::discord::{
    channel_model::topic_matches, credentials::DiscordBotToken, DiscordChannelCreate,
    DiscordChannelEdit, DiscordGuildChannel,
};
use reqwest::Method;
use std::collections::HashSet;

/// Discord's maximum number of guild channels, excluding threads.
const MAX_GUILD_CHANNELS: usize = 500;

fn valid_name(name: &str) -> bool {
    !name.trim().is_empty()
        && name.encode_utf16().count() <= 100
        && !name.chars().any(char::is_control)
}

fn valid_topic(topic: &str, limit: usize) -> bool {
    topic.encode_utf16().count() <= limit && !topic.contains('\0')
}

fn validate_channel(
    channel: &DiscordGuildChannel,
    guild: &str,
) -> Result<(), DiscordTransportError> {
    provider_id(&channel.id)?;
    provider_id(&channel.guild_id)?;
    if channel.guild_id != guild
        || !matches!(channel.kind, 0 | 2 | 4 | 5 | 10..=16)
        || !valid_name(&channel.name)
        || channel.topic.as_deref().is_some_and(|topic| {
            !valid_topic(
                topic,
                if matches!(channel.kind, 15 | 16) {
                    4096
                } else {
                    1024
                },
            )
        })
    {
        return Err(DiscordTransportError::InvalidResponse);
    }
    if let Some(parent) = &channel.parent_id {
        provider_id(parent)?;
        if parent == &channel.id || channel.kind == 4 {
            return Err(DiscordTransportError::InvalidResponse);
        }
    }
    Ok(())
}

impl DiscordClient {
    pub(super) async fn guild_channels(
        &self,
        token: &DiscordBotToken,
        guild: &str,
    ) -> Result<Vec<DiscordGuildChannel>, DiscordTransportError> {
        validate_id(guild)?;
        let channels: Vec<DiscordGuildChannel> = self
            .request(
                token,
                Method::GET,
                &format!("guilds/{guild}/channels"),
                None,
            )
            .await?;
        if channels.len() > MAX_GUILD_CHANNELS {
            return Err(DiscordTransportError::InvalidResponse);
        }
        let mut ids = HashSet::with_capacity(channels.len());
        for channel in &channels {
            validate_channel(channel, guild)?;
            if !ids.insert(&channel.id) {
                return Err(DiscordTransportError::InvalidResponse);
            }
        }
        Ok(channels)
    }

    pub(super) async fn create_channel(
        &self,
        token: &DiscordBotToken,
        guild: &str,
        create: &DiscordChannelCreate,
    ) -> Result<DiscordGuildChannel, DiscordTransportError> {
        validate_id(guild)?;
        if !matches!(create.kind, 0 | 2 | 4)
            || !valid_name(&create.name)
            || create
                .topic
                .as_deref()
                .is_some_and(|topic| create.kind != 0 || !valid_topic(topic, 1024))
            || (create.kind == 4 && create.parent_id.is_some())
        {
            return Err(DiscordTransportError::InvalidInput);
        }
        if let Some(parent) = &create.parent_id {
            validate_id(parent)?;
        }
        let channel: DiscordGuildChannel = self
            .request(
                token,
                Method::POST,
                &format!("guilds/{guild}/channels"),
                Some(
                    serde_json::to_value(create)
                        .map_err(|_| DiscordTransportError::InvalidInput)?,
                ),
            )
            .await?;
        validate_channel(&channel, guild).map_err(|_| DiscordTransportError::Uncertain)?;
        if channel.name != create.name
            || channel.kind != create.kind
            || channel.parent_id != create.parent_id
            || create
                .topic
                .as_ref()
                .is_some_and(|topic| !topic_matches(topic, channel.topic.as_deref()))
        {
            return Err(DiscordTransportError::Uncertain);
        }
        Ok(channel)
    }

    pub(super) async fn edit_channel(
        &self,
        token: &DiscordBotToken,
        guild: &str,
        channel_id: &str,
        edit: &DiscordChannelEdit,
    ) -> Result<DiscordGuildChannel, DiscordTransportError> {
        validate_id(guild)?;
        validate_id(channel_id)?;
        if (edit.name.is_none() && edit.topic.is_none() && edit.parent_id.is_none())
            || edit.name.as_deref().is_some_and(|name| !valid_name(name))
            || edit
                .topic
                .as_deref()
                .is_some_and(|topic| !valid_topic(topic, 1024))
        {
            return Err(DiscordTransportError::InvalidInput);
        }
        if let Some(Some(parent)) = &edit.parent_id {
            validate_id(parent)?;
            if parent == channel_id {
                return Err(DiscordTransportError::InvalidInput);
            }
        }
        let channel: DiscordGuildChannel = self
            .request(
                token,
                Method::PATCH,
                &format!("channels/{channel_id}"),
                Some(serde_json::to_value(edit).map_err(|_| DiscordTransportError::InvalidInput)?),
            )
            .await?;
        validate_channel(&channel, guild).map_err(|_| DiscordTransportError::Uncertain)?;
        if channel.id != channel_id
            || !matches!(channel.kind, 0 | 2 | 4)
            || edit.name.as_ref().is_some_and(|name| &channel.name != name)
            || edit
                .topic
                .as_ref()
                .is_some_and(|topic| !topic_matches(topic, channel.topic.as_deref()))
            || edit
                .parent_id
                .as_ref()
                .is_some_and(|parent| &channel.parent_id != parent)
            || (channel.kind != 0 && edit.topic.is_some())
            || (channel.kind == 4 && edit.parent_id.is_some())
        {
            return Err(DiscordTransportError::Uncertain);
        }
        Ok(channel)
    }
}
