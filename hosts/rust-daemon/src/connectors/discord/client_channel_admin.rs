//! Explicit REST route mapping for owner-scoped channel administration.
use super::{validate_id, DiscordClient, DiscordTransportError};
use crate::connectors::discord::{
    channel_admin_model::{
        DiscordChannelAdminMethod as M, DiscordChannelAdminRequest, DiscordChannelAdminRoute as R,
    },
    credentials::DiscordBotToken,
};
use reqwest::Method;

const MAX_ADMIN_BODY_BYTES: usize = 64 * 1024;

fn id(value: &str) -> Result<&str, DiscordTransportError> {
    validate_id(value)?;
    Ok(value)
}

fn member_id(value: &str) -> Result<&str, DiscordTransportError> {
    if value == "@me" {
        Ok(value)
    } else {
        id(value)
    }
}

fn query(path: String, pairs: &[(&str, String)]) -> String {
    let mut encoded = reqwest::Url::parse("https://discord.invalid/").expect("constant URL");
    encoded
        .query_pairs_mut()
        .extend_pairs(pairs.iter().map(|(key, value)| (*key, value.as_str())));
    format!("{path}?{}", encoded.query().unwrap_or_default())
}

fn route(
    request: &DiscordChannelAdminRequest,
) -> Result<(Method, String, bool, bool), DiscordTransportError> {
    let method = match request.method {
        M::Get => Method::GET,
        M::Post => Method::POST,
        M::Patch => Method::PATCH,
        M::Put => Method::PUT,
        M::Delete => Method::DELETE,
    };
    let mut empty_ack = false;
    let mut needs_body = false;
    let path = match (&request.method, &request.route) {
        (M::Get | M::Patch | M::Delete, R::Channel { channel_id }) => {
            needs_body = request.method == M::Patch;
            format!("channels/{}", id(channel_id)?)
        }
        (M::Post, R::GuildChannels { guild_id }) => {
            needs_body = true;
            format!("guilds/{}/channels", id(guild_id)?)
        }
        (M::Patch, R::GuildPositions { guild_id }) => {
            needs_body = true;
            empty_ack = true;
            format!("guilds/{}/channels", id(guild_id)?)
        }
        (
            M::Put | M::Delete,
            R::Permission {
                channel_id,
                overwrite_id,
            },
        ) => {
            needs_body = request.method == M::Put;
            empty_ack = true;
            format!(
                "channels/{}/permissions/{}",
                id(channel_id)?,
                id(overwrite_id)?
            )
        }
        (
            M::Get,
            R::MemberPermissions {
                channel_id,
                user_id,
            },
        ) => {
            format!(
                "channels/{}/permissions/member/{}",
                id(channel_id)?,
                id(user_id)?
            )
        }
        (
            M::Get,
            R::RolePermissions {
                channel_id,
                role_id,
            },
        ) => {
            format!(
                "channels/{}/permissions/role/{}",
                id(channel_id)?,
                id(role_id)?
            )
        }
        (
            M::Post,
            R::ThreadStart {
                channel_id,
                message_id,
            },
        ) => {
            needs_body = true;
            let channel_id = id(channel_id)?;
            if let Some(message_id) = message_id {
                format!("channels/{channel_id}/messages/{}/threads", id(message_id)?)
            } else {
                format!("channels/{channel_id}/threads")
            }
        }
        (M::Get, R::GuildThreads { guild_id }) => {
            format!("guilds/{}/threads/active", id(guild_id)?)
        }
        (
            M::Get,
            R::ArchivedThreads {
                channel_id,
                kind,
                before,
                limit,
            },
        ) => {
            if !(1..=100).contains(limit) {
                return Err(DiscordTransportError::InvalidInput);
            }
            let channel_id = id(channel_id)?;
            let path = match kind.as_str() {
                "public" | "private" => format!("channels/{channel_id}/threads/archived/{kind}"),
                "joined_private" => {
                    format!("channels/{channel_id}/users/@me/threads/archived/private")
                }
                _ => return Err(DiscordTransportError::InvalidInput),
            };
            let mut pairs = vec![("limit", limit.to_string())];
            if let Some(before) = before {
                if kind == "joined_private" {
                    id(before)?;
                } else if before.len() > 64 || chrono::DateTime::parse_from_rfc3339(before).is_err()
                {
                    return Err(DiscordTransportError::InvalidInput);
                }
                pairs.push(("before", before.clone()));
            }
            query(path, &pairs)
        }
        (
            M::Get | M::Put | M::Delete,
            R::ThreadMembers {
                channel_id,
                user_id,
                after,
                limit,
            },
        ) => {
            let path = format!("channels/{}/thread-members", id(channel_id)?);
            if let Some(user_id) = user_id {
                if after.is_some() || limit.is_some() {
                    return Err(DiscordTransportError::InvalidInput);
                }
                let path = format!("{path}/{}", member_id(user_id)?);
                if request.method == M::Get {
                    query(path, &[("with_member", "true".into())])
                } else {
                    empty_ack = true;
                    path
                }
            } else {
                if request.method != M::Get {
                    return Err(DiscordTransportError::InvalidInput);
                }
                let limit = limit.unwrap_or(100);
                if !(1..=100).contains(&limit) {
                    return Err(DiscordTransportError::InvalidInput);
                }
                let mut pairs = vec![("with_member", "true".into()), ("limit", limit.to_string())];
                if let Some(after) = after {
                    id(after)?;
                    pairs.push(("after", after.clone()));
                }
                query(path, &pairs)
            }
        }
        (M::Put, R::VoiceStatus { channel_id }) => {
            needs_body = true;
            empty_ack = true;
            format!("channels/{}/voice-status", id(channel_id)?)
        }
        (M::Post, R::Follow { channel_id }) => {
            needs_body = true;
            format!("channels/{}/followers", id(channel_id)?)
        }
        _ => return Err(DiscordTransportError::InvalidInput),
    };
    Ok((method, path, empty_ack, needs_body))
}

impl DiscordClient {
    pub(super) async fn admin_request(
        &self,
        token: &DiscordBotToken,
        request: &DiscordChannelAdminRequest,
    ) -> Result<serde_json::Value, DiscordTransportError> {
        let (method, path, empty_ack, needs_body) = route(request)?;
        if needs_body != request.body.is_some() {
            return Err(DiscordTransportError::InvalidInput);
        }
        let mut body = request.body.clone();
        if let Some(body) = &mut body {
            if matches!(request.route, R::GuildPositions { .. }) {
                let positions = body.as_array().ok_or(DiscordTransportError::InvalidInput)?;
                if positions.is_empty()
                    || positions.len() > 500
                    || positions.iter().any(|v| !v.is_object())
                {
                    return Err(DiscordTransportError::InvalidInput);
                }
            } else if !body.is_object() {
                return Err(DiscordTransportError::InvalidInput);
            }
            if let R::ThreadStart { message_id, .. } = &request.route {
                if let Some(starter) = body.get_mut("message") {
                    if message_id.is_some() {
                        return Err(DiscordTransportError::InvalidInput);
                    }
                    let starter = starter
                        .as_object_mut()
                        .ok_or(DiscordTransportError::InvalidInput)?;
                    starter.insert(
                        "allowed_mentions".into(),
                        serde_json::json!({"parse":[],"replied_user":false}),
                    );
                }
            }
            if serde_json::to_vec(body)
                .map_err(|_| DiscordTransportError::InvalidInput)?
                .len()
                > MAX_ADMIN_BODY_BYTES
            {
                return Err(DiscordTransportError::InvalidInput);
            }
        }
        self.request_body_expected(token, method, &path, body, None, empty_ack)
            .await
    }
}
