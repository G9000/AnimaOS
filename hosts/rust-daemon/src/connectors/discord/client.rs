//! Bounded native Discord REST transport. Mention/recovery source map: HERMES-NOTICE.
use super::{
    credentials::DiscordBotToken,
    model::*,
    text::{stable_nonce, valid_chunk},
};
use async_trait::async_trait;
use futures::StreamExt;
use reqwest::{
    header::{HeaderMap, HeaderValue, AUTHORIZATION},
    redirect::Policy,
    Client, Method, Url,
};
use serde::{de::DeserializeOwned, Deserialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap},
    fmt,
    sync::{Arc, OnceLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;
use zeroize::Zeroizing;
pub(crate) const REST_PAGE_LIMIT: usize = 100;
pub(crate) const MAX_SCAN_MESSAGES: usize = 1000;
const RESPONSE_LIMIT: usize = 1024 * 1024;
/// Application input bound; bot reply limits do not constrain user messages.
pub(crate) const MAX_INBOUND_TEXT_CHARS: usize = 65_536;
#[path = "client_attachments.rs"]
mod attachments;
pub(super) use attachments::valid_attachment_url;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DiscordTransportError {
    Configuration,
    InvalidInput,
    InvalidResponse,
    Authentication,
    PermissionDenied,
    RateLimited { retry_after_ms: u64, global: bool },
    Transport,
    Uncertain,
    BacklogLimit,
    HttpStatus { status: u16 },
    ResponseTooLarge,
}
impl DiscordTransportError {
    pub(crate) fn retry_after_ms(&self) -> Option<u64> {
        if let Self::RateLimited { retry_after_ms, .. } = self {
            Some(*retry_after_ms)
        } else {
            None
        }
    }
    pub(crate) fn is_uncertain(&self) -> bool {
        matches!(self, Self::Uncertain)
    }
    pub(crate) fn category(&self) -> &'static str {
        match self {
            Self::Configuration => "configuration",
            Self::InvalidInput => "invalidInput",
            Self::InvalidResponse => "invalidResponse",
            Self::Authentication => "authentication",
            Self::PermissionDenied => "permissionDenied",
            Self::RateLimited { .. } => "rateLimited",
            Self::Transport => "transport",
            Self::Uncertain => "uncertain",
            Self::BacklogLimit => "backlogLimit",
            Self::HttpStatus { .. } => "httpStatus",
            Self::ResponseTooLarge => "responseTooLarge",
        }
    }
}
impl fmt::Display for DiscordTransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.category())
    }
}
impl std::error::Error for DiscordTransportError {}
#[async_trait]
pub(crate) trait DiscordTransport: Send + Sync {
    async fn send_file(
        &self,
        _token: &DiscordBotToken,
        _channel: &str,
        _file: &DiscordFileUpload,
        _caption: &str,
        _attempt_id: &str,
    ) -> Result<DiscordFileReceipt, DiscordTransportError> {
        Err(DiscordTransportError::Configuration)
    }
    async fn download_attachment(
        &self,
        _channel: &str,
        _attachment: &DiscordAttachment,
    ) -> Result<Vec<u8>, DiscordTransportError> {
        Err(DiscordTransportError::Configuration)
    }
    async fn check_application(
        &self,
        _token: &DiscordBotToken,
    ) -> Result<DiscordBotCheck, DiscordTransportError> {
        Err(DiscordTransportError::Configuration)
    }
    async fn member_roles(
        &self,
        _token: &DiscordBotToken,
        _guild: &str,
        _user: &str,
    ) -> Result<Vec<String>, DiscordTransportError> {
        Err(DiscordTransportError::PermissionDenied)
    }
    async fn create_thread(
        &self,
        _token: &DiscordBotToken,
        _channel: &str,
        _message: &str,
        _name: &str,
    ) -> Result<DiscordChannelMetadata, DiscordTransportError> {
        Err(DiscordTransportError::Configuration)
    }
    async fn send_typing(
        &self,
        _token: &DiscordBotToken,
        _channel_id: &str,
    ) -> Result<(), DiscordTransportError> {
        Ok(())
    }
    async fn rename_thread(
        &self,
        _token: &DiscordBotToken,
        _channel: &str,
        _name: &str,
    ) -> Result<DiscordChannelMetadata, DiscordTransportError> {
        Err(DiscordTransportError::Configuration)
    }
    async fn get_me(
        &self,
        token: &DiscordBotToken,
    ) -> Result<DiscordBotIdentity, DiscordTransportError>;
    async fn get_channel(
        &self,
        token: &DiscordBotToken,
        channel_id: &str,
    ) -> Result<DiscordChannelMetadata, DiscordTransportError>;
    async fn latest_cursor(
        &self,
        token: &DiscordBotToken,
        channel_id: &str,
    ) -> Result<String, DiscordTransportError>;
    async fn read_thread_history(
        &self,
        _token: &DiscordBotToken,
        _channel_id: &str,
        _before: Option<&str>,
        _limit: usize,
    ) -> Result<Vec<DiscordMessage>, DiscordTransportError> {
        Err(DiscordTransportError::Configuration)
    }
    async fn receive(
        &self,
        token: &DiscordBotToken,
        channel_id: &str,
        cursor: &str,
    ) -> Result<DiscordMessageBatch, DiscordTransportError>;
    async fn send_chunk(
        &self,
        token: &DiscordBotToken,
        channel_id: &str,
        text: &str,
        attempt_id: &str,
    ) -> Result<DiscordSendReceipt, DiscordTransportError>;
}
// Conservatively aggregate bucket deadlines at bot scope. This may delay unrelated routes,
// but never sends earlier than any returned bucket/global deadline and shares pacing across
// connector/client instances without persisting or displaying token-derived keys.
type RateState = Arc<Mutex<HashMap<[u8; 32], u128>>>;
fn rate_state() -> RateState {
    static STATE: OnceLock<RateState> = OnceLock::new();
    STATE
        .get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
        .clone()
}
fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
pub(crate) struct DiscordClient {
    client: Client,
    base: Url,
    deadlines: RateState,
}
impl DiscordClient {
    pub(crate) fn new() -> Result<Self, DiscordTransportError> {
        Self::build("https://discord.com/api/v10/")
    }
    fn build(base: &str) -> Result<Self, DiscordTransportError> {
        Ok(Self {
            client: Client::builder()
                .user_agent(concat!(
                    "DiscordBot (https://github.com/G9000/animaos, ",
                    env!("CARGO_PKG_VERSION"),
                    ")"
                ))
                .redirect(Policy::none())
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(15))
                .build()
                .map_err(|_| DiscordTransportError::Configuration)?,
            base: Url::parse(base).map_err(|_| DiscordTransportError::Configuration)?,
            deadlines: rate_state(),
        })
    }
    #[cfg(test)]
    fn for_test_base(base: &str) -> Result<Self, DiscordTransportError> {
        let parsed = Url::parse(base).map_err(|_| DiscordTransportError::Configuration)?;
        if parsed.scheme() != "http"
            || !matches!(parsed.host_str(), Some("127.0.0.1" | "localhost"))
        {
            return Err(DiscordTransportError::Configuration);
        }
        Self::build(base)
    }
    async fn pace(&self, key: [u8; 32]) -> Result<(), DiscordTransportError> {
        let deadline = self.deadlines.lock().await.get(&key).copied().unwrap_or(0);
        let remaining = deadline.saturating_sub(now_ms());
        if remaining == 0 {
            return Ok(());
        }
        // A provider deadline must not keep an owned delivery claim waiting.
        // The host persists the remaining delay; no request is sent early.
        Err(DiscordTransportError::RateLimited {
            retry_after_ms: remaining.min(u128::from(u64::MAX)) as u64,
            // This is conservative local bot-wide pacing, not a new provider assertion.
            global: false,
        })
    }
    async fn defer(&self, key: [u8; 32], delay: u64) {
        let deadline = now_ms().saturating_add(u128::from(delay));
        let mut values = self.deadlines.lock().await;
        let old = values.entry(key).or_default();
        *old = (*old).max(deadline);
    }
    async fn request<T: DeserializeOwned>(
        &self,
        token: &DiscordBotToken,
        method: Method,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> Result<T, DiscordTransportError> {
        self.request_body(token, method, path, body, None).await
    }
    async fn request_body<T: DeserializeOwned>(
        &self,
        token: &DiscordBotToken,
        method: Method,
        path: &str,
        body: Option<serde_json::Value>,
        multipart: Option<(Vec<u8>, String)>,
    ) -> Result<T, DiscordTransportError> {
        let sending = method == Method::POST;
        let key: [u8; 32] = Sha256::digest(token.expose().as_bytes()).into();
        self.pace(key).await?;
        let url = self
            .base
            .join(path)
            .map_err(|_| DiscordTransportError::InvalidInput)?;
        let auth = Zeroizing::new(format!("Bot {}", token.expose()));
        let mut header =
            HeaderValue::from_str(&auth).map_err(|_| DiscordTransportError::InvalidInput)?;
        header.set_sensitive(true);
        let mut request = self
            .client
            .request(method, url)
            .header(AUTHORIZATION, header);
        if let Some(body) = body {
            request = request.json(&body);
        }
        if let Some((bytes, content_type)) = multipart {
            request = request
                .header(reqwest::header::CONTENT_TYPE, content_type)
                .body(bytes);
        }
        let response = request.send().await.map_err(|error| {
            tracing::warn!(
                timeout = error.is_timeout(),
                connection = error.is_connect(),
                "Discord HTTP request failed"
            );
            if sending {
                DiscordTransportError::Uncertain
            } else {
                DiscordTransportError::Transport
            }
        })?;
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let header_delay = header_delay(&headers);
        if headers
            .get("x-ratelimit-remaining")
            .and_then(|v| v.to_str().ok())
            == Some("0")
        {
            if let Some(delay) = header_delay {
                self.defer(key, delay).await;
            }
        }
        let mut bytes = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(part) = stream.next().await {
            let part = part.map_err(|_| {
                if sending {
                    DiscordTransportError::Uncertain
                } else {
                    DiscordTransportError::Transport
                }
            })?;
            if bytes.len().saturating_add(part.len()) > RESPONSE_LIMIT {
                return Err(if sending {
                    DiscordTransportError::Uncertain
                } else {
                    DiscordTransportError::ResponseTooLarge
                });
            }
            bytes.extend_from_slice(&part);
        }
        if status == 429 {
            #[derive(Deserialize)]
            struct Rate {
                retry_after: f64,
                #[serde(default)]
                global: bool,
            }
            let rate = serde_json::from_slice::<Rate>(&bytes).ok();
            let body_delay = rate.as_ref().and_then(|r| seconds_ms(r.retry_after));
            let delay = match (body_delay, header_delay) {
                (Some(a), Some(b)) => a.max(b),
                (Some(a), None) | (None, Some(a)) => a,
                (None, None) => u64::MAX,
            };
            self.defer(key, delay).await;
            return Err(DiscordTransportError::RateLimited {
                retry_after_ms: delay,
                global: rate.is_some_and(|r| r.global),
            });
        }
        if !(200..300).contains(&status) {
            return Err(match status {
                401 => DiscordTransportError::Authentication,
                403 => DiscordTransportError::PermissionDenied,
                400 | 404 => DiscordTransportError::HttpStatus { status },
                _ if sending => DiscordTransportError::Uncertain,
                _ => DiscordTransportError::HttpStatus { status },
            });
        }
        // Discord's ephemeral typing endpoint acknowledges success with an empty 204.
        let bytes = if status == 204 && bytes.is_empty() {
            b"null".as_slice()
        } else {
            bytes.as_slice()
        };
        serde_json::from_slice(bytes).map_err(|_| {
            if sending {
                DiscordTransportError::Uncertain
            } else {
                DiscordTransportError::InvalidResponse
            }
        })
    }
    async fn page(
        &self,
        token: &DiscordBotToken,
        channel: &str,
        before: Option<&str>,
        limit: usize,
    ) -> Result<Vec<DiscordMessage>, DiscordTransportError> {
        validate_id(channel)?;
        let mut path = format!("channels/{channel}/messages?limit={limit}");
        if let Some(before) = before {
            validate_id(before)?;
            path.push_str(&format!("&before={before}"));
        }
        let wire: Vec<WireMessage> = self.request(token, Method::GET, &path, None).await?;
        if wire.len() > limit {
            return Err(DiscordTransportError::InvalidResponse);
        }
        wire.into_iter().map(|w| w.normalize(channel)).collect()
    }
}
fn seconds_ms(value: f64) -> Option<u64> {
    if !value.is_finite() || value <= 0.0 {
        return None;
    }
    let ms = (value * 1000.0).ceil();
    Some(if !ms.is_finite() || ms >= u64::MAX as f64 {
        u64::MAX
    } else {
        ms as u64
    })
}
fn header_delay(headers: &HeaderMap) -> Option<u64> {
    ["retry-after", "x-ratelimit-reset-after"]
        .iter()
        .filter_map(|name| {
            headers
                .get(*name)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<f64>().ok())
                .and_then(seconds_ms)
        })
        .max()
}
fn validate_id(value: &str) -> Result<u64, DiscordTransportError> {
    DiscordSnowflake::parse(value)
        .map(DiscordSnowflake::value)
        .ok_or(DiscordTransportError::InvalidInput)
}
fn provider_id(value: &str) -> Result<u64, DiscordTransportError> {
    validate_id(value).map_err(|_| DiscordTransportError::InvalidResponse)
}
fn metadata(value: Option<String>, limit: usize) -> Result<Option<String>, DiscordTransportError> {
    if value.as_ref().is_some_and(|v| v.chars().count() > limit) {
        Err(DiscordTransportError::InvalidResponse)
    } else {
        Ok(value)
    }
}
#[derive(Deserialize)]
struct WireUser {
    id: String,
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    global_name: Option<String>,
    #[serde(default)]
    bot: bool,
}
#[derive(Deserialize)]
struct WireChannel {
    id: String,
    guild_id: String,
    name: Option<String>,
    #[serde(rename = "type")]
    kind: u64,
    #[serde(default)]
    parent_id: Option<String>,
}
impl WireChannel {
    fn normalize(self) -> Result<DiscordChannelMetadata, DiscordTransportError> {
        provider_id(&self.id)?;
        provider_id(&self.guild_id)?;
        let kind = match self.kind {
            0 if self.parent_id.is_none() => DiscordChannelKind::GuildText,
            // Text channels may have a category parent. It is not a conversation parent.
            0 => DiscordChannelKind::GuildText,
            10..=12
                if self
                    .parent_id
                    .as_ref()
                    .is_some_and(|id| provider_id(id).is_ok() && id != &self.id) =>
            {
                DiscordChannelKind::GuildThread
            }
            _ => return Err(DiscordTransportError::InvalidResponse),
        };
        let parent_channel_id = if kind == DiscordChannelKind::GuildThread {
            self.parent_id
        } else {
            None
        };
        Ok(DiscordChannelMetadata {
            id: self.id,
            guild_id: self.guild_id,
            name: metadata(self.name, 128)?,
            kind,
            parent_channel_id,
        })
    }
}

pub(crate) fn invite_url(application: &str) -> String {
    // Conversation history, public threads, and native file uploads.
    let permissions =
        (1u64 << 10) | (1u64 << 11) | (1u64 << 15) | (1u64 << 16) | (1u64 << 35) | (1u64 << 38);
    format!("https://discord.com/oauth2/authorize?client_id={application}&scope=bot&permissions={permissions}&integration_type=0")
}
#[derive(Deserialize)]
struct WireMessage {
    id: String,
    channel_id: String,
    #[serde(default)]
    guild_id: Option<String>,
    author: WireUser,
    content: String,
    #[serde(default)]
    attachments: Vec<DiscordAttachment>,
    #[serde(default)]
    webhook_id: Option<String>,
    #[serde(rename = "type")]
    message_type: u64,
}
impl WireMessage {
    fn normalize(self, channel: &str) -> Result<DiscordMessage, DiscordTransportError> {
        provider_id(&self.id)?;
        provider_id(&self.channel_id)?;
        provider_id(&self.author.id)?;
        if self.channel_id != channel
            || self
                .content
                .chars()
                .take(MAX_INBOUND_TEXT_CHARS + 1)
                .count()
                > MAX_INBOUND_TEXT_CHARS
        {
            return Err(DiscordTransportError::InvalidResponse);
        }
        if let Some(id) = self.guild_id.as_ref() {
            provider_id(id)?;
        }
        if let Some(id) = self.webhook_id.as_ref() {
            provider_id(id)?;
        }
        Ok(DiscordMessage {
            id: self.id,
            channel_id: self.channel_id,
            guild_id: self.guild_id,
            sender: DiscordSenderMetadata {
                id: self.author.id,
                username: metadata(self.author.username, 128)?,
                display_name: metadata(self.author.global_name, 128)?,
            },
            text: self.content,
            attachments: self.attachments,
            is_bot: self.author.bot,
            webhook_id: self.webhook_id,
            message_type: self.message_type,
        })
    }
}
#[async_trait]
impl DiscordTransport for DiscordClient {
    async fn read_thread_history(
        &self,
        token: &DiscordBotToken,
        channel: &str,
        before: Option<&str>,
        limit: usize,
    ) -> Result<Vec<DiscordMessage>, DiscordTransportError> {
        if !(1..=25).contains(&limit) {
            return Err(DiscordTransportError::InvalidInput);
        }
        self.page(token, channel, before, limit).await
    }

    async fn download_attachment(
        &self,
        channel: &str,
        attachment: &DiscordAttachment,
    ) -> Result<Vec<u8>, DiscordTransportError> {
        self.download_file(channel, attachment).await
    }
    async fn send_file(
        &self,
        token: &DiscordBotToken,
        channel: &str,
        file: &DiscordFileUpload,
        caption: &str,
        attempt_id: &str,
    ) -> Result<DiscordFileReceipt, DiscordTransportError> {
        self.upload_file(token, channel, file, caption, attempt_id)
            .await
    }
    async fn check_application(
        &self,
        token: &DiscordBotToken,
    ) -> Result<DiscordBotCheck, DiscordTransportError> {
        let value: serde_json::Value = self
            .request(token, Method::GET, "applications/@me", None)
            .await?;
        let application_id = value
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or(DiscordTransportError::InvalidResponse)?;
        provider_id(application_id)?;
        let bot = self.get_me(token).await?;
        if value
            .get("bot")
            .and_then(|v| v.get("id"))
            .and_then(|v| v.as_str())
            .is_some_and(|id| id != bot.id)
        {
            return Err(DiscordTransportError::InvalidResponse);
        }
        let mut owners = Vec::new();
        if let Some(team) = value.get("team").filter(|v| !v.is_null()) {
            let members = team
                .get("members")
                .and_then(|v| v.as_array())
                .ok_or(DiscordTransportError::InvalidResponse)?;
            if members.len() > 100 {
                return Err(DiscordTransportError::InvalidResponse);
            }
            for member in members
                .iter()
                .filter(|m| m.get("membership_state").and_then(|v| v.as_u64()) == Some(2))
            {
                let id = member
                    .get("user")
                    .and_then(|v| v.get("id"))
                    .and_then(|v| v.as_str())
                    .ok_or(DiscordTransportError::InvalidResponse)?;
                provider_id(id)?;
                if !owners.iter().any(|v| v == id) {
                    owners.push(id.to_owned());
                }
            }
        } else if let Some(id) = value
            .get("owner")
            .and_then(|v| v.get("id"))
            .and_then(|v| v.as_str())
        {
            provider_id(id)?;
            owners.push(id.to_owned());
        }
        let flags = value.get("flags").and_then(|v| v.as_u64()).unwrap_or(0);
        Ok(DiscordBotCheck {
            application_id: application_id.into(),
            bot,
            owner_user_ids: owners,
            message_content_enabled: flags & ((1 << 18) | (1 << 19)) != 0,
            invite_url: invite_url(application_id),
        })
    }
    async fn member_roles(
        &self,
        token: &DiscordBotToken,
        guild: &str,
        user: &str,
    ) -> Result<Vec<String>, DiscordTransportError> {
        validate_id(guild)?;
        validate_id(user)?;
        let member: serde_json::Value = self
            .request(
                token,
                Method::GET,
                &format!("guilds/{guild}/members/{user}"),
                None,
            )
            .await?;
        if member
            .get("user")
            .and_then(|v| v.get("id"))
            .and_then(|v| v.as_str())
            != Some(user)
        {
            return Err(DiscordTransportError::InvalidResponse);
        }
        let roles = member
            .get("roles")
            .and_then(|v| v.as_array())
            .ok_or(DiscordTransportError::InvalidResponse)?;
        if roles.len() > 250 {
            return Err(DiscordTransportError::InvalidResponse);
        }
        roles
            .iter()
            .map(|v| {
                let id = v.as_str().ok_or(DiscordTransportError::InvalidResponse)?;
                provider_id(id)?;
                Ok(id.to_owned())
            })
            .collect()
    }
    async fn create_thread(
        &self,
        token: &DiscordBotToken,
        channel: &str,
        message: &str,
        name: &str,
    ) -> Result<DiscordChannelMetadata, DiscordTransportError> {
        validate_id(channel)?;
        validate_id(message)?;
        let name = super::text::thread_title(name);
        let wire: WireChannel = self
            .request(
                token,
                Method::POST,
                &format!("channels/{channel}/messages/{message}/threads"),
                Some(serde_json::json!({"name":name,"auto_archive_duration":1440})),
            )
            .await?;
        let result = wire
            .normalize()
            .map_err(|_| DiscordTransportError::Uncertain)?;
        if result.kind != DiscordChannelKind::GuildThread
            || result.parent_channel_id.as_deref() != Some(channel)
        {
            return Err(DiscordTransportError::Uncertain);
        }
        Ok(result)
    }
    async fn rename_thread(
        &self,
        token: &DiscordBotToken,
        channel: &str,
        name: &str,
    ) -> Result<DiscordChannelMetadata, DiscordTransportError> {
        validate_id(channel)?;
        if name.is_empty() || name.encode_utf16().count() > 80 {
            return Err(DiscordTransportError::InvalidInput);
        }
        let wire: WireChannel = self
            .request(
                token,
                Method::PATCH,
                &format!("channels/{channel}"),
                Some(serde_json::json!({"name":name})),
            )
            .await?;
        let result = wire
            .normalize()
            .map_err(|_| DiscordTransportError::Uncertain)?;
        if result.id != channel || result.kind != DiscordChannelKind::GuildThread {
            return Err(DiscordTransportError::Uncertain);
        }
        Ok(result)
    }
    async fn send_typing(
        &self,
        token: &DiscordBotToken,
        channel: &str,
    ) -> Result<(), DiscordTransportError> {
        validate_id(channel)?;
        self.request(
            token,
            Method::POST,
            &format!("channels/{channel}/typing"),
            None,
        )
        .await
    }
    async fn get_me(
        &self,
        token: &DiscordBotToken,
    ) -> Result<DiscordBotIdentity, DiscordTransportError> {
        let user: WireUser = self.request(token, Method::GET, "users/@me", None).await?;
        provider_id(&user.id)?;
        if !user.bot {
            return Err(DiscordTransportError::InvalidResponse);
        }
        Ok(DiscordBotIdentity {
            id: user.id,
            username: metadata(user.username, 128)?,
            display_name: metadata(user.global_name, 128)?,
        })
    }
    async fn get_channel(
        &self,
        token: &DiscordBotToken,
        channel: &str,
    ) -> Result<DiscordChannelMetadata, DiscordTransportError> {
        validate_id(channel)?;
        let wire: WireChannel = self
            .request(token, Method::GET, &format!("channels/{channel}"), None)
            .await?;
        if wire.id != channel {
            return Err(DiscordTransportError::InvalidResponse);
        }
        wire.normalize()
    }
    async fn latest_cursor(
        &self,
        token: &DiscordBotToken,
        channel: &str,
    ) -> Result<String, DiscordTransportError> {
        Ok(self
            .page(token, channel, None, 1)
            .await?
            .into_iter()
            .next()
            .map(|v| v.id)
            .unwrap_or_else(|| "0".into()))
    }
    async fn receive(
        &self,
        token: &DiscordBotToken,
        channel: &str,
        cursor: &str,
    ) -> Result<DiscordMessageBatch, DiscordTransportError> {
        let lower = DiscordSnowflake::cursor(cursor).ok_or(DiscordTransportError::InvalidInput)?;
        let anchor = self.page(token, channel, None, 1).await?;
        let Some(anchor) = anchor.into_iter().next() else {
            return Ok(DiscordMessageBatch {
                messages: Vec::new(),
                next_cursor: cursor.into(),
                boundary_proven: true,
            });
        };
        let high = provider_id(&anchor.id)?;
        if high <= lower {
            return Ok(DiscordMessageBatch {
                messages: Vec::new(),
                next_cursor: cursor.into(),
                boundary_proven: true,
            });
        }
        let mut messages = BTreeMap::new();
        let mut before = anchor.id.clone();
        messages.insert(high, anchor);
        let mut scanned = 1;
        loop {
            // A one-message boundary probe permits exactly 1000 newer messages without
            // accepting a 1001st newer message or trusting a short page as channel start.
            let limit = REST_PAGE_LIMIT.min(MAX_SCAN_MESSAGES.saturating_sub(scanned).max(1));
            let page = self.page(token, channel, Some(&before), limit).await?;
            if page.is_empty() {
                break;
            }
            let upper = provider_id(&before)?;
            let mut smallest = upper;
            let mut boundary = false;
            for message in page {
                let id = provider_id(&message.id)?;
                if id >= upper {
                    return Err(DiscordTransportError::InvalidResponse);
                }
                smallest = smallest.min(id);
                if id <= lower {
                    boundary = true;
                    continue;
                }
                if let Some(existing) = messages.get(&id) {
                    if existing != &message {
                        return Err(DiscordTransportError::InvalidResponse);
                    }
                } else {
                    scanned += 1;
                    if scanned > MAX_SCAN_MESSAGES {
                        return Err(DiscordTransportError::BacklogLimit);
                    }
                    messages.insert(id, message);
                }
            }
            if boundary {
                break;
            }
            if smallest >= upper {
                return Err(DiscordTransportError::InvalidResponse);
            }
            before = smallest.to_string();
        }
        Ok(DiscordMessageBatch {
            messages: messages.into_values().collect(),
            next_cursor: high.to_string(),
            boundary_proven: true,
        })
    }
    async fn send_chunk(
        &self,
        token: &DiscordBotToken,
        channel: &str,
        text: &str,
        attempt_id: &str,
    ) -> Result<DiscordSendReceipt, DiscordTransportError> {
        validate_id(channel)?;
        if !valid_chunk(text) || attempt_id.is_empty() || attempt_id.len() > 128 {
            return Err(DiscordTransportError::InvalidInput);
        }
        // Every ping class is disabled intentionally, including user and replied-user pings.
        let value:serde_json::Value=self.request(token,Method::POST,&format!("channels/{channel}/messages"),Some(serde_json::json!({"content":text,"allowed_mentions":{"parse":[],"replied_user":false},"nonce":stable_nonce(attempt_id),"enforce_nonce":true}))).await?;
        let id = value
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or(DiscordTransportError::Uncertain)?;
        let response_channel = value
            .get("channel_id")
            .and_then(|v| v.as_str())
            .ok_or(DiscordTransportError::Uncertain)?;
        if provider_id(id).is_err() || response_channel != channel {
            return Err(DiscordTransportError::Uncertain);
        }
        Ok(DiscordSendReceipt {
            message_id: id.into(),
            channel_id: channel.into(),
        })
    }
}

#[cfg(test)]
#[path = "native_tests.rs"]
mod tests;
