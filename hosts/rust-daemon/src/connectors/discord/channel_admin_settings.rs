//! Validated channel configuration, separate from routing and owner authority.
use super::{id, keys, number, parent, plain, DiscordGuildChannel};
use serde_json::{Map, Value};

pub(super) fn kind(value: &str) -> Result<u8, String> {
    match value {
        "text" => Ok(0),
        "voice" => Ok(2),
        "category" => Ok(4),
        "announcement" => Ok(5),
        "stage" => Ok(13),
        "forum" => Ok(15),
        "media" => Ok(16),
        _ => Err("Unsupported guild channel type".into()),
    }
}

pub(super) fn bits(value: &Value) -> Result<u64, String> {
    let text = value
        .as_str()
        .ok_or("Permission bitfields must be decimal strings")?;
    if text.is_empty() || text.len() > 20 || !text.bytes().all(|v| v.is_ascii_digit()) {
        return Err("Permission bitfields must be bounded unsigned decimal strings".into());
    }
    text.parse()
        .map_err(|_| "Permission bitfield exceeds 64 bits".into())
}

pub(super) fn overwrite(value: &Value, with_id: bool) -> Result<Value, String> {
    let object = value
        .as_object()
        .ok_or("Permission overwrite must be an object")?;
    keys(
        object,
        if with_id {
            &["id", "type", "allow", "deny"]
        } else {
            &["type", "allow", "deny"]
        },
    )?;
    let mut result = Map::new();
    if with_id {
        result.insert("id".into(), Value::String(id(object, "id")?));
    }
    let target_type = match object.get("type").and_then(Value::as_str) {
        Some("role") => 0,
        Some("member") => 1,
        _ => return Err("Overwrite type must be role or member".into()),
    };
    let allow = bits(object.get("allow").ok_or("allow is required")?)?;
    let deny = bits(object.get("deny").ok_or("deny is required")?)?;
    if allow & deny != 0 {
        return Err("An overwrite cannot both allow and deny the same permission".into());
    }
    result.insert("type".into(), target_type.into());
    result.insert("allow".into(), allow.to_string().into());
    result.insert("deny".into(), deny.to_string().into());
    Ok(result.into())
}

fn emoji(value: &Value) -> Result<(), String> {
    if value.is_null() {
        return Ok(());
    }
    let object = value
        .as_object()
        .ok_or("Reaction emoji must be an object or null")?;
    keys(object, &["emoji_id", "emoji_name"])?;
    let custom = object.get("emoji_id").is_some_and(|v| !v.is_null());
    let unicode = object.get("emoji_name").is_some_and(|v| !v.is_null());
    if custom == unicode {
        return Err("Specify exactly one emoji_id or emoji_name".into());
    }
    if custom {
        id(object, "emoji_id")?;
    }
    if unicode {
        plain(object.get("emoji_name").unwrap(), 100, "emoji_name")?;
    }
    Ok(())
}

pub(super) fn settings(
    value: &Value,
    channel_kind: u8,
    creating: bool,
) -> Result<Map<String, Value>, String> {
    let object = value.as_object().ok_or("settings must be an object")?;
    keys(
        object,
        &[
            "nsfw",
            "rate_limit_per_user",
            "bitrate",
            "user_limit",
            "rtc_region",
            "video_quality_mode",
            "default_auto_archive_duration",
            "default_thread_rate_limit_per_user",
            "flags",
            "available_tags",
            "default_reaction_emoji",
            "default_sort_order",
            "default_forum_layout",
            "position",
            "type",
            "permission_overwrites",
        ],
    )?;
    let mut result = object.clone();
    for (key, value) in object {
        let voice = matches!(channel_kind, 2 | 13);
        let forum = matches!(channel_kind, 15 | 16);
        let text = matches!(channel_kind, 0 | 5 | 15 | 16);
        match key.as_str() {
            "nsfw" if channel_kind != 4 => {
                value.as_bool().ok_or("nsfw must be boolean")?;
            }
            "rate_limit_per_user" if matches!(channel_kind, 0 | 2 | 13 | 15 | 16) => {
                number(value, 0, 21600, key)?;
            }
            "bitrate" if voice => {
                number(
                    value,
                    8000,
                    if channel_kind == 13 { 64000 } else { 384000 },
                    key,
                )?;
            }
            "user_limit" if voice => {
                number(value, 0, if channel_kind == 13 { 10000 } else { 99 }, key)?;
            }
            "rtc_region" if voice => {
                if !value.is_null() {
                    plain(value, 64, key)?;
                }
            }
            "video_quality_mode" if voice => {
                number(value, 1, 2, key)?;
            }
            "default_auto_archive_duration" if text => {
                archive(value, key)?;
            }
            "default_thread_rate_limit_per_user" if matches!(channel_kind, 0 | 15 | 16) => {
                number(value, 0, 21600, key)?;
            }
            "position" => {
                number(value, 0, 499, key)?;
            }
            "type" if !creating && matches!(channel_kind, 0 | 5) => {
                let target = kind(
                    value
                        .as_str()
                        .ok_or("settings.type must be text or announcement")?,
                )?;
                if !matches!(target, 0 | 5) {
                    return Err("Only text/announcement conversion is supported".into());
                }
                result.insert(key.clone(), target.into());
            }
            "flags" if matches!(channel_kind, 0 | 2 | 5 | 15 | 16) => {
                let flags = number(value, 0, u32::MAX.into(), key)?;
                let mask = if forum {
                    16 | if channel_kind == 16 { 32768 } else { 0 }
                } else {
                    0
                } | if matches!(channel_kind, 0 | 2 | 5 | 15 | 16) {
                    1 << 21
                } else {
                    0
                };
                if flags & !mask != 0 {
                    return Err("Unsupported channel flags for this type".into());
                }
                if flags & (1 << 21) != 0 && object.get("nsfw") == Some(&Value::Bool(true)) {
                    return Err("Spoiler channel flags and nsfw cannot both be enabled".into());
                }
            }
            "available_tags" if forum => {
                let tags = value
                    .as_array()
                    .filter(|v| v.len() <= 20)
                    .ok_or("At most 20 forum tags are allowed")?;
                let mut ids = std::collections::HashSet::new();
                for tag in tags {
                    let tag = tag.as_object().ok_or("Forum tag must be an object")?;
                    keys(tag, &["id", "name", "moderated", "emoji_id", "emoji_name"])?;
                    plain(
                        tag.get("name").ok_or("Tag name is required")?,
                        20,
                        "tag name",
                    )?;
                    if tag.contains_key("id") && !ids.insert(id(tag, "id")?) {
                        return Err("Duplicate forum tag ID".into());
                    }
                    if let Some(moderated) = tag.get("moderated") {
                        moderated.as_bool().ok_or("moderated must be boolean")?;
                    }
                    let reaction: Map<String, Value> = tag
                        .iter()
                        .filter(|(k, _)| k.starts_with("emoji_"))
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect();
                    if reaction.values().any(|v| !v.is_null()) {
                        emoji(&reaction.into())?;
                    }
                }
            }
            "default_reaction_emoji" if forum => {
                emoji(value)?;
            }
            "default_sort_order" if forum => {
                if !value.is_null() {
                    number(value, 0, 1, key)?;
                }
            }
            "default_forum_layout" if channel_kind == 15 => {
                number(value, 0, 2, key)?;
            }
            "permission_overwrites" => {
                let entries = value
                    .as_array()
                    .filter(|v| v.len() <= 1000)
                    .ok_or("Permission overwrites must be a bounded array")?;
                let entries = entries
                    .iter()
                    .map(|v| overwrite(v, true))
                    .collect::<Result<Vec<_>, _>>()?;
                let mut ids = std::collections::HashSet::new();
                if entries.iter().any(|v| !ids.insert(v["id"].clone())) {
                    return Err("Duplicate overwrite target".into());
                }
                result.insert(key.clone(), entries.into());
            }
            _ => return Err(format!("{key} is unsupported for this channel type")),
        }
    }
    Ok(result)
}

pub(super) fn archive(value: &Value, key: &str) -> Result<(), String> {
    if !matches!(value.as_u64(), Some(60 | 1440 | 4320 | 10080)) {
        return Err(format!("{key} must be 60, 1440, 4320 or 10080"));
    }
    Ok(())
}

pub(super) fn channel_body(
    args: &Map<String, Value>,
    channel_kind: u8,
    creating: bool,
    channels: &[DiscordGuildChannel],
) -> Result<Map<String, Value>, String> {
    let mut body = match args.get("settings") {
        Some(value) => settings(value, channel_kind, creating)?,
        None => Map::new(),
    };
    if creating || args.contains_key("name") {
        let name = args.get("name").ok_or("name is required")?;
        plain(name, 100, "name")?;
        body.insert("name".into(), name.clone());
    }
    if creating {
        body.insert("type".into(), channel_kind.into());
    }
    if let Some(topic) = args.get("topic") {
        if !matches!(channel_kind, 0 | 5 | 15 | 16) {
            return Err("Only text, announcement, forum or media channels have topics".into());
        }
        let text = topic.as_str().ok_or("topic must be a string")?;
        if text.encode_utf16().count()
            > if !creating && matches!(channel_kind, 15 | 16) {
                4096
            } else {
                1024
            }
            || text.contains('\0')
        {
            return Err("Channel topic is too long or contains NUL".into());
        }
        body.insert("topic".into(), topic.clone());
    }
    if let Some(value) = args.get("parent_id") {
        parent(value, channel_kind, channels)?;
        body.insert("parent_id".into(), value.clone());
    }
    if body.is_empty() {
        return Err("At least one channel change is required".into());
    }
    Ok(body)
}
