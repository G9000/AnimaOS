//! Structured administration payloads. No caller can select a guild or REST route.
use crate::connectors::discord::{
    channel_admin_model::{
        DiscordChannelAdminMethod as M, DiscordChannelAdminRequest, DiscordChannelAdminRoute as R,
    },
    DiscordGuildChannel, DiscordSnowflake,
};
use serde_json::{json, Map, Value};
#[path = "channel_admin_settings.rs"]
mod settings;

pub(super) struct Prepared {
    pub request: DiscordChannelAdminRequest,
    pub ack: Ack,
    pub result: &'static str,
}
pub(super) enum Ack {
    Empty,
    Channel { expected: Value, deleted: bool },
    Threads,
    Members { single: bool },
    Permissions,
    Follow { source: String, target: String },
}
impl Prepared {
    pub fn mutation(&self) -> bool {
        self.request.method != M::Get
    }
}

/// Model numbers arrive as f64; Discord channel parameters require integers.
pub(super) fn normalize(value: Value) -> Result<Value, String> {
    match value {
        Value::Number(number) if number.is_f64() => {
            let value = number.as_f64().ok_or("Invalid channel number")?;
            if !value.is_finite() || value.fract() != 0.0 || value.abs() > 9_007_199_254_740_991.0 {
                return Err("Channel numbers must be exact integers; Discord IDs and permission bits must be strings".into());
            }
            Ok(if value < 0.0 {
                Value::from(value as i64)
            } else {
                Value::from(value as u64)
            })
        }
        Value::Array(values) => values
            .into_iter()
            .map(normalize)
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        Value::Object(values) => values
            .into_iter()
            .map(|(key, value)| Ok((key, normalize(value)?)))
            .collect::<Result<Map<_, _>, String>>()
            .map(Value::Object),
        value => Ok(value),
    }
}

pub(super) fn keys(object: &Map<String, Value>, allowed: &[&str]) -> Result<(), String> {
    if let Some(key) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(format!(
            "Unexpected argument {key}; the host selects the server and operation"
        ));
    }
    Ok(())
}
pub(super) fn id(object: &Map<String, Value>, key: &str) -> Result<String, String> {
    let value = object
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{key} must be a decimal Discord ID"))?;
    if DiscordSnowflake::parse(value).is_none() {
        return Err(format!("Invalid {key}"));
    }
    Ok(value.into())
}
pub(super) fn plain(value: &Value, max: usize, key: &str) -> Result<(), String> {
    let text = value
        .as_str()
        .ok_or_else(|| format!("{key} must be text"))?;
    if text.trim().is_empty()
        || text.trim() != text
        || text.encode_utf16().count() > max
        || text.chars().any(char::is_control)
    {
        return Err(format!("{key} must contain 1–{max} plain characters"));
    }
    Ok(())
}
pub(super) fn number(value: &Value, min: u64, max: u64, key: &str) -> Result<u64, String> {
    value
        .as_u64()
        .filter(|v| (min..=max).contains(v))
        .ok_or_else(|| format!("{key} must be an integer from {min} to {max}"))
}
pub(super) fn parent(
    value: &Value,
    kind: u8,
    channels: &[DiscordGuildChannel],
) -> Result<(), String> {
    if kind == 4 {
        return Err("Categories cannot have parent categories".into());
    }
    if value.is_null() {
        return Ok(());
    }
    let parent = value
        .as_str()
        .ok_or("parent_id must be a category ID or null")?;
    if !channels.iter().any(|v| v.id == parent && v.kind == 4) {
        return Err("Parent must be a category in the current server".into());
    }
    Ok(())
}
fn channel_kind(target: Option<&Value>) -> Result<u8, String> {
    target
        .and_then(|v| v["type"].as_u64())
        .and_then(|v| v.try_into().ok())
        .ok_or_else(|| "Current channel metadata is missing".into())
}
fn thread(target: Option<&Value>) -> Result<u8, String> {
    let kind = channel_kind(target)?;
    if !matches!(kind, 10..=12) {
        return Err("This operation requires a thread in the current server".into());
    }
    Ok(kind)
}
fn guild_channel(kind: u8) -> Result<(), String> {
    if !matches!(kind, 0 | 2 | 4 | 5 | 13 | 15 | 16) {
        return Err("Unsupported guild channel type".into());
    }
    Ok(())
}
fn request(method: M, route: R, body: Option<Value>, ack: Ack, result: &'static str) -> Prepared {
    Prepared {
        request: DiscordChannelAdminRequest {
            method,
            route,
            body,
        },
        ack,
        result,
    }
}
fn required<'a>(args: &'a Map<String, Value>, key: &str) -> Result<&'a Value, String> {
    args.get(key).ok_or_else(|| format!("{key} is required"))
}
fn selected<'a>(
    args: &'a Map<String, Value>,
    key: &str,
    default: &'a str,
) -> Result<&'a str, String> {
    args.get(key)
        .map(|v| v.as_str().ok_or_else(|| format!("{key} must be text")))
        .unwrap_or(Ok(default))
}
fn tags(value: &Value) -> Result<(), String> {
    let values = value
        .as_array()
        .filter(|v| v.len() <= 5)
        .ok_or("At most five applied tags are allowed")?;
    let mut seen = std::collections::HashSet::new();
    for value in values {
        let value = value.as_str().ok_or("Applied tags must be Discord IDs")?;
        if DiscordSnowflake::parse(value).is_none() || !seen.insert(value) {
            return Err("Invalid or duplicate applied tag ID".into());
        }
    }
    Ok(())
}

pub(super) fn prepare(
    name: &str,
    args: &Value,
    guild: &str,
    channels: &[DiscordGuildChannel],
    target: Option<&Value>,
) -> Result<Prepared, String> {
    let args = args.as_object().ok_or("Tool arguments must be an object")?;
    let cid = || id(args, "channel_id");
    match name {
        "create_discord_channel" => {
            keys(args, &["name", "type", "topic", "parent_id", "settings"])?;
            let kind = settings::kind(selected(args, "type", "text")?)?;
            let body = settings::channel_body(args, kind, true, channels)?;
            let expected = Value::Object(body.clone());
            Ok(request(
                M::Post,
                R::GuildChannels {
                    guild_id: guild.into(),
                },
                Some(body.into()),
                Ack::Channel {
                    expected,
                    deleted: false,
                },
                "created",
            ))
        }
        "update_discord_channel" => {
            keys(
                args,
                &["channel_id", "name", "topic", "parent_id", "settings"],
            )?;
            let kind = channel_kind(target)?;
            guild_channel(kind)?;
            let mut body = settings::channel_body(args, kind, false, channels)?;
            if body.get("parent_id").and_then(Value::as_str) == Some(cid()?.as_str()) {
                return Err("A channel cannot be its own category".into());
            }
            let mut expected = body.clone();
            expected.insert("id".into(), cid()?.into());
            if !expected.contains_key("type") {
                expected.insert("type".into(), kind.into());
            }
            Ok(request(
                M::Patch,
                R::Channel { channel_id: cid()? },
                Some(std::mem::take(&mut body).into()),
                Ack::Channel {
                    expected: expected.into(),
                    deleted: false,
                },
                "updated",
            ))
        }
        "get_discord_channel" => {
            keys(args, &["channel_id"])?;
            Ok(request(
                M::Get,
                R::Channel { channel_id: cid()? },
                None,
                Ack::Channel {
                    expected: json!({"id":cid()?}),
                    deleted: false,
                },
                "inspected",
            ))
        }
        "delete_discord_channel" => {
            keys(args, &["channel_id", "expected_name"])?;
            let kind = channel_kind(target)?;
            if !matches!(kind, 10..=12) {
                guild_channel(kind)?;
            }
            let expected_name = required(args, "expected_name")?;
            plain(expected_name, 100, "expected_name")?;
            if target.and_then(|v| v.get("name")) != Some(expected_name) {
                return Err(
                    "Channel name no longer matches expected_name; inspect it before deleting"
                        .into(),
                );
            }
            Ok(request(
                M::Delete,
                R::Channel { channel_id: cid()? },
                None,
                Ack::Channel {
                    expected: json!({"id":cid()?,"name":expected_name,"type":kind}),
                    deleted: true,
                },
                "deleted",
            ))
        }
        "reorder_discord_channels" => {
            keys(args, &["positions"])?;
            let entries = required(args, "positions")?
                .as_array()
                .filter(|v| !v.is_empty() && v.len() <= 500)
                .ok_or("positions must contain 1–500 channel moves")?;
            if entries
                .iter()
                .filter(|v| v.get("parent_id").is_some())
                .count()
                > 1
            {
                return Err("Discord permits only one category move per ordering request; submit separate explicit moves".into());
            }
            let mut seen = std::collections::HashSet::new();
            for entry in entries {
                let entry = entry.as_object().ok_or("Each position must be an object")?;
                keys(entry, &["id", "position", "parent_id", "lock_permissions"])?;
                let channel_id = id(entry, "id")?;
                if !seen.insert(channel_id.clone()) {
                    return Err("Duplicate channel move".into());
                }
                let channel = channels
                    .iter()
                    .find(|v| v.id == channel_id)
                    .ok_or("Move target is not in this server")?;
                guild_channel(channel.kind)?;
                if !entry.contains_key("position") && !entry.contains_key("parent_id") {
                    return Err("Each move requires position or parent_id".into());
                }
                if let Some(v) = entry.get("position") {
                    number(v, 0, 499, "position")?;
                }
                if let Some(v) = entry.get("parent_id") {
                    parent(v, channel.kind, channels)?;
                }
                if let Some(v) = entry.get("lock_permissions") {
                    v.as_bool().ok_or("lock_permissions must be boolean")?;
                    if v == &Value::Bool(true)
                        && !entry.get("parent_id").is_some_and(Value::is_string)
                    {
                        return Err(
                            "Permission synchronization requires an explicit parent category"
                                .into(),
                        );
                    }
                }
            }
            Ok(request(
                M::Patch,
                R::GuildPositions {
                    guild_id: guild.into(),
                },
                Some(entries.clone().into()),
                Ack::Empty,
                "reordered",
            ))
        }
        "set_discord_channel_permissions"
        | "delete_discord_channel_permission"
        | "get_discord_channel_permissions" => {
            let writing = name == "set_discord_channel_permissions";
            let deleting = name == "delete_discord_channel_permission";
            keys(
                args,
                if writing {
                    &["channel_id", "target_id", "target_type", "allow", "deny"]
                } else if deleting {
                    &["channel_id", "target_id"]
                } else {
                    &["channel_id", "target_id", "target_type"]
                },
            )?;
            let kind = channel_kind(target)?;
            if writing || deleting {
                guild_channel(kind)?;
            }
            let target_id = id(args, "target_id")?;
            let route = if deleting || writing {
                R::Permission {
                    channel_id: cid()?,
                    overwrite_id: target_id.clone(),
                }
            } else {
                match selected(args, "target_type", "")? {
                    "role" => R::RolePermissions {
                        channel_id: cid()?,
                        role_id: target_id,
                    },
                    "member" => R::MemberPermissions {
                        channel_id: cid()?,
                        user_id: target_id,
                    },
                    _ => return Err("target_type must be role or member".into()),
                }
            };
            let body = if writing {
                Some(settings::overwrite(
                    &json!({"type":required(args,"target_type")?,"allow":required(args,"allow")?,"deny":required(args,"deny")?}),
                    false,
                )?)
            } else {
                None
            };
            Ok(request(
                if writing {
                    M::Put
                } else if deleting {
                    M::Delete
                } else {
                    M::Get
                },
                route,
                body,
                if writing || deleting {
                    Ack::Empty
                } else {
                    Ack::Permissions
                },
                if deleting {
                    "overwriteDeleted"
                } else if writing {
                    "permissionsUpdated"
                } else {
                    "inspected"
                },
            ))
        }
        "list_discord_threads" => {
            keys(args, &["scope", "channel_id", "before", "limit"])?;
            let scope = selected(args, "scope", "active")?;
            let route = if scope == "active" {
                if args.contains_key("before")
                    || args.contains_key("limit")
                    || args.contains_key("channel_id")
                {
                    return Err(
                        "Active guild threads do not take channel or pagination arguments".into(),
                    );
                }
                R::GuildThreads {
                    guild_id: guild.into(),
                }
            } else {
                if !matches!(channel_kind(target)?, 0 | 5 | 15 | 16) {
                    return Err(
                        "Archived threads require a text, announcement, forum or media parent"
                            .into(),
                    );
                }
                let kind = match scope {
                    "public_archived" => "public",
                    "private_archived" => "private",
                    "joined_private_archived" => "joined_private",
                    _ => return Err("Unknown thread listing scope".into()),
                };
                if kind != "public" && channel_kind(target)? != 0 {
                    return Err("Private threads require a text channel".into());
                }
                let before = args
                    .get("before")
                    .map(|v| v.as_str().map(str::to_owned).ok_or("before must be text"))
                    .transpose()?;
                let limit = args
                    .get("limit")
                    .map(|v| number(v, 1, 100, "limit"))
                    .transpose()?
                    .unwrap_or(100) as u16;
                R::ArchivedThreads {
                    channel_id: cid()?,
                    kind: kind.into(),
                    before,
                    limit,
                }
            };
            Ok(request(M::Get, route, None, Ack::Threads, "listed"))
        }
        "create_discord_thread" => {
            keys(
                args,
                &[
                    "channel_id",
                    "name",
                    "type",
                    "message_id",
                    "auto_archive_duration",
                    "invitable",
                    "rate_limit_per_user",
                    "applied_tags",
                    "starter_message",
                ],
            )?;
            let kind = channel_kind(target)?;
            if !matches!(kind, 0 | 5 | 15 | 16) {
                return Err("Threads require a text, announcement, forum or media parent".into());
            }
            plain(required(args, "name")?, 100, "name")?;
            let mut body = Map::new();
            body.insert("name".into(), required(args, "name")?.clone());
            let message = args
                .contains_key("message_id")
                .then(|| id(args, "message_id"))
                .transpose()?;
            let private = match selected(args, "type", "public")? {
                "public" => false,
                "private" => true,
                _ => return Err("Thread type must be public or private".into()),
            };
            if private && (kind != 0 || message.is_some()) {
                return Err("Private threads require a text parent and no source message".into());
            }
            if kind == 5 && message.is_none() {
                return Err("Announcement threads require a source message".into());
            }
            let forum = matches!(kind, 15 | 16);
            if forum && (private || message.is_some()) {
                return Err("Forum/media posts are public threads with a starter message".into());
            }
            let thread_kind = if kind == 5 {
                10
            } else if private {
                12
            } else {
                11
            };
            if !forum && message.is_none() {
                body.insert("type".into(), thread_kind.into());
            }
            for key in [
                "auto_archive_duration",
                "invitable",
                "rate_limit_per_user",
                "applied_tags",
            ] {
                if let Some(value) = args.get(key) {
                    match key {
                        "auto_archive_duration" => settings::archive(value, key)?,
                        "invitable" => {
                            value.as_bool().ok_or("invitable must be boolean")?;
                            if !private {
                                return Err("invitable only applies to private threads".into());
                            }
                        }
                        "rate_limit_per_user" => {
                            number(value, 0, 21600, key)?;
                        }
                        "applied_tags" => {
                            if !forum {
                                return Err("Applied tags require a forum or media post".into());
                            }
                            tags(value)?;
                        }
                        _ => unreachable!(),
                    }
                    body.insert(key.into(), value.clone());
                }
            }
            if forum {
                let starter = required(args, "starter_message")?
                    .as_str()
                    .ok_or("starter_message must be text")?;
                if starter.trim().is_empty()
                    || starter.encode_utf16().count() > 2000
                    || starter.contains('\0')
                {
                    return Err("Forum starter must contain 1–2000 characters".into());
                }
                body.insert(
                    "message".into(),
                    json!({"content":starter,"allowed_mentions":{"parse":[],"replied_user":false}}),
                );
            } else if args.contains_key("starter_message") {
                return Err("Text/announcement threads use message_id for an existing starter; a text starter is supported for forum/media posts".into());
            }
            let mut expected = Map::new();
            expected.insert("name".into(), required(args, "name")?.clone());
            expected.insert("parent_id".into(), cid()?.into());
            expected.insert("type".into(), thread_kind.into());
            if let Some(message) = &message {
                expected.insert("id".into(), message.clone().into());
            }
            for key in ["rate_limit_per_user", "applied_tags"] {
                if let Some(value) = args.get(key) {
                    expected.insert(key.into(), value.clone());
                }
            }
            let metadata: Map<String, Value> = args
                .iter()
                .filter(|(k, _)| matches!(k.as_str(), "auto_archive_duration" | "invitable"))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            if !metadata.is_empty() {
                expected.insert("thread_metadata".into(), metadata.into());
            }
            Ok(request(
                M::Post,
                R::ThreadStart {
                    channel_id: cid()?,
                    message_id: message,
                },
                Some(body.into()),
                Ack::Channel {
                    expected: expected.into(),
                    deleted: false,
                },
                "created",
            ))
        }
        "update_discord_thread" => {
            keys(
                args,
                &[
                    "channel_id",
                    "name",
                    "archived",
                    "locked",
                    "auto_archive_duration",
                    "invitable",
                    "rate_limit_per_user",
                    "applied_tags",
                    "flags",
                ],
            )?;
            let kind = thread(target)?;
            let forum_parent = target
                .and_then(|v| v["parent_id"].as_str())
                .is_some_and(|id| {
                    channels
                        .iter()
                        .any(|v| v.id == id && matches!(v.kind, 15 | 16))
                });
            let mut body = args.clone();
            body.remove("channel_id");
            if body.is_empty() {
                return Err("At least one thread change is required".into());
            }
            let mut expected = Map::new();
            expected.insert("id".into(), cid()?.into());
            expected.insert("type".into(), kind.into());
            let mut metadata = Map::new();
            for (key, value) in &body {
                match key.as_str() {
                    "name" => plain(value, 100, key)?,
                    "archived" | "locked" => {
                        value.as_bool().ok_or("Thread state must be boolean")?;
                    }
                    "invitable" => {
                        value.as_bool().ok_or("invitable must be boolean")?;
                        if kind != 12 {
                            return Err("Only private threads have invitable settings".into());
                        }
                    }
                    "auto_archive_duration" => settings::archive(value, key)?,
                    "rate_limit_per_user" => {
                        number(value, 0, 21600, key)?;
                    }
                    "applied_tags" => {
                        if !forum_parent {
                            return Err("Applied tags require a forum or media thread".into());
                        }
                        tags(value)?;
                    }
                    "flags" => {
                        if !forum_parent {
                            return Err("Thread pinning requires a forum or media parent".into());
                        }
                        let flags = number(value, 0, 2, key)?;
                        if flags != 0 && flags != 2 {
                            return Err("Only the thread pinned flag may be set".into());
                        }
                    }
                    _ => return Err("Unsupported thread field".into()),
                }
                if matches!(
                    key.as_str(),
                    "archived" | "locked" | "invitable" | "auto_archive_duration"
                ) {
                    metadata.insert(key.clone(), value.clone());
                } else {
                    expected.insert(key.clone(), value.clone());
                }
            }
            if !metadata.is_empty() {
                expected.insert("thread_metadata".into(), metadata.into());
            }
            Ok(request(
                M::Patch,
                R::Channel { channel_id: cid()? },
                Some(body.into()),
                Ack::Channel {
                    expected: expected.into(),
                    deleted: false,
                },
                "updated",
            ))
        }
        "manage_discord_thread_members" => {
            keys(args, &["channel_id", "action", "user_id", "after", "limit"])?;
            thread(target)?;
            let action = selected(args, "action", "")?;
            let (method, user) = match action {
                "list" => (M::Get, None),
                "get" => (M::Get, Some(id(args, "user_id")?)),
                "join" => (M::Put, Some("@me".into())),
                "leave" => (M::Delete, Some("@me".into())),
                "add" => (M::Put, Some(id(args, "user_id")?)),
                "remove" => (M::Delete, Some(id(args, "user_id")?)),
                _ => return Err("Unknown thread membership action".into()),
            };
            if matches!(action, "join" | "leave" | "list") && args.contains_key("user_id") {
                return Err("This action does not take user_id".into());
            }
            if action != "list" && (args.contains_key("after") || args.contains_key("limit")) {
                return Err("Only member listing takes pagination".into());
            }
            let after = args
                .contains_key("after")
                .then(|| id(args, "after"))
                .transpose()?;
            let limit = args
                .get("limit")
                .map(|v| number(v, 1, 100, "limit").map(|v| v as u16))
                .transpose()?;
            Ok(request(
                method,
                R::ThreadMembers {
                    channel_id: cid()?,
                    user_id: user,
                    after,
                    limit,
                },
                None,
                if method == M::Get {
                    Ack::Members {
                        single: action == "get",
                    }
                } else {
                    Ack::Empty
                },
                if method == M::Get {
                    "listed"
                } else {
                    "membershipUpdated"
                },
            ))
        }
        "set_discord_voice_status" => {
            keys(args, &["channel_id", "status"])?;
            if channel_kind(target)? != 2 {
                return Err("Voice status requires a voice channel".into());
            }
            let status = required(args, "status")?;
            if !status.is_null()
                && status
                    .as_str()
                    .is_none_or(|v| v.encode_utf16().count() > 500 || v.contains('\0'))
            {
                return Err("status must be null or at most 500 characters".into());
            }
            Ok(request(
                M::Put,
                R::VoiceStatus { channel_id: cid()? },
                Some(json!({"status":status})),
                Ack::Empty,
                "statusUpdated",
            ))
        }
        "follow_discord_announcement" => {
            keys(args, &["channel_id", "target_channel_id"])?;
            if channel_kind(target)? != 5 {
                return Err("Follow source must be an announcement channel".into());
            }
            let destination = id(args, "target_channel_id")?;
            if !channels.iter().any(|v| v.id == destination && v.kind == 0) {
                return Err(
                    "Follow destination must be a text channel in the current server".into(),
                );
            }
            Ok(request(
                M::Post,
                R::Follow { channel_id: cid()? },
                Some(json!({"webhook_channel_id":destination})),
                Ack::Follow {
                    source: cid()?,
                    target: destination,
                },
                "followed",
            ))
        }
        _ => Err("Unknown native channel administration operation".into()),
    }
}

pub(super) fn validate_target(
    value: &Value,
    id: &str,
    guild: &str,
    channels: &[DiscordGuildChannel],
) -> Result<(), String> {
    if value["id"].as_str() != Some(id) || value["guild_id"].as_str() != Some(guild) {
        return Err("Channel was not verified in the current server".into());
    }
    let kind = value["type"]
        .as_u64()
        .ok_or("Channel type was not returned")?;
    if !matches!(kind, 0 | 2 | 4 | 5 | 10..=13 | 15 | 16) {
        return Err("Unsupported channel type".into());
    }
    if matches!(kind, 10..=12) {
        let parent = value["parent_id"]
            .as_str()
            .ok_or("Thread parent was not returned")?;
        if !channels
            .iter()
            .any(|v| v.id == parent && matches!(v.kind, 0 | 5 | 15 | 16))
        {
            return Err("Thread parent was not verified in the current server".into());
        }
    }
    Ok(())
}

fn subset(expected: &Value, actual: &Value) -> bool {
    match expected {
        Value::Object(object) => object.iter().all(|(k, v)| {
            actual.get(k).is_some_and(|a| {
                if k == "permission_overwrites" {
                    match (v.as_array(), a.as_array()) {
                        (Some(expected), Some(actual)) => {
                            expected.len() == actual.len()
                                && expected.iter().all(|entry| {
                                    actual.iter().any(|candidate| {
                                        candidate["id"] == entry["id"]
                                            && candidate["type"] == entry["type"]
                                            && subset(entry, candidate)
                                    })
                                })
                        }
                        _ => false,
                    }
                } else if k == "applied_tags" {
                    match (v.as_array(), a.as_array()) {
                        (Some(expected), Some(actual)) => {
                            expected.len() == actual.len()
                                && expected.iter().all(|v| actual.contains(v))
                        }
                        _ => false,
                    }
                } else {
                    subset(v, a)
                }
            }) || (v.is_null() && actual.get(k).is_none())
        }),
        Value::Array(values) => actual.as_array().is_some_and(|a| {
            a.len() == values.len() && values.iter().zip(a).all(|(v, a)| subset(v, a))
        }),
        Value::String(text) if text.is_empty() => actual.is_null() || actual == expected,
        _ => expected == actual,
    }
}
pub(super) fn validate_ack(
    plan: &Prepared,
    value: &Value,
    guild: &str,
    channels: &[DiscordGuildChannel],
) -> Result<(), String> {
    let valid = match &plan.ack {
        Ack::Empty => value.is_null(),
        Ack::Channel { expected, .. } => {
            value["id"].as_str().is_some_and(|id| {
                crate::connectors::discord::DiscordSnowflake::parse(id).is_some()
                    && validate_target(value, id, guild, channels).is_ok()
            }) && subset(expected, value)
                && {
                    if let Some(starter) = plan.request.body.as_ref().and_then(|v| v.get("message"))
                    {
                        value.get("message").is_some_and(|message| {
                            message["id"]
                                .as_str()
                                .is_some_and(|id| DiscordSnowflake::parse(id).is_some())
                                && message["channel_id"] == value["id"]
                                && message["content"] == starter["content"]
                        })
                    } else {
                        true
                    }
                }
        }
        Ack::Threads => value
            .get("threads")
            .and_then(Value::as_array)
            .is_some_and(|v| {
                v.len() <= 1000
                    && v.iter().all(|v| {
                        v["id"].as_str().is_some_and(|id| {
                            matches!(v["type"].as_u64(), Some(10..=12))
                                && validate_target(v, id, guild, channels).is_ok()
                                && match &plan.request.route {
                                    R::ArchivedThreads { channel_id, .. } => {
                                        v["parent_id"].as_str() == Some(channel_id)
                                    }
                                    _ => true,
                                }
                        })
                    })
            }),
        Ack::Members { single } => {
            let member_matches = |member: &Value| match &plan.request.route {
                R::ThreadMembers {
                    channel_id,
                    user_id,
                    ..
                } => {
                    member.is_object()
                        && member["id"].as_str() == Some(channel_id)
                        && member["user_id"].as_str().is_some_and(|id| {
                            DiscordSnowflake::parse(id).is_some()
                                && user_id.as_ref().is_none_or(|user| user == id)
                        })
                }
                _ => false,
            };
            if *single {
                member_matches(value)
            } else {
                value
                    .as_array()
                    .is_some_and(|v| v.len() <= 100 && v.iter().all(member_matches))
            }
        }
        Ack::Permissions => value
            .get("permissions")
            .is_some_and(|v| settings::bits(v).is_ok()),
        Ack::Follow { source, target } => {
            value["channel_id"].as_str() == Some(source)
                && value["webhook_id"].as_str().is_some_and(|v| {
                    crate::connectors::discord::DiscordSnowflake::parse(v).is_some()
                })
                && matches!(&plan.request.body,Some(v)if v["webhook_channel_id"].as_str()==Some(target))
        }
    };
    if valid {
        Ok(())
    } else {
        Err("Discord returned an invalid or mismatched acknowledgement".into())
    }
}

#[cfg(test)]
#[path = "channel_admin_validation_tests.rs"]
mod tests;
