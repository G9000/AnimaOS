//! Account-catalog metadata only; presence is not generation acceptance.
use futures::StreamExt;
use serde_json::Value;
use std::{collections::BTreeSet, time::Duration};
use zeroize::Zeroizing;

pub(super) const MODELS_ENDPOINT: &str =
    "https://chatgpt.com/backend-api/codex/models?client_version=99.0.0";
const MAX_BODY: usize = 8 * 1024 * 1024;
const MAX_ROWS: usize = 512;
const JS_SAFE_INTEGER: i64 = 9_007_199_254_740_991;
const UNAVAILABLE: &str = "Model metadata is unavailable.";
const TIMED_OUT: &str = "Model metadata request timed out.";
const INVALID: &str = "Model metadata response is invalid.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChatGptModelsStage {
    Authorization,
    Client,
    Request,
    HttpStatus,
    Body,
    Decode,
    Scope,
    Deadline,
    Complete,
    Control,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChatGptCatalogDispatch {
    NotStarted,
    Attempted,
    ResponseReceived,
    Unknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChatGptModelsReason {
    None,
    Credential,
    ClientBuild,
    RequestBuild,
    Connect,
    Transport,
    HttpStatus,
    DeclaredLimit,
    StreamedLimit,
    BodyRead,
    InvalidCatalog,
    ScopeChanged,
    Timeout,
    Disconnected,
    Busy,
    Vault,
}

/// Closed catalog evidence only: no source error, credential or upstream text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChatGptModelsFailure {
    stage: ChatGptModelsStage,
    catalog_dispatch: ChatGptCatalogDispatch,
    reason: ChatGptModelsReason,
    http_status: Option<u16>,
}
impl ChatGptModelsFailure {
    fn new(
        stage: ChatGptModelsStage,
        catalog_dispatch: ChatGptCatalogDispatch,
        reason: ChatGptModelsReason,
        http_status: Option<u16>,
    ) -> Self {
        Self {
            stage,
            catalog_dispatch,
            reason,
            http_status,
        }
    }
    pub const fn stage(&self) -> ChatGptModelsStage {
        self.stage
    }
    pub const fn catalog_dispatch(&self) -> ChatGptCatalogDispatch {
        self.catalog_dispatch
    }
    pub const fn reason(&self) -> ChatGptModelsReason {
        self.reason
    }
    pub const fn http_status(&self) -> Option<u16> {
        self.http_status
    }
    pub const fn public_message(&self) -> &'static str {
        match self.reason {
            ChatGptModelsReason::Timeout => TIMED_OUT,
            ChatGptModelsReason::InvalidCatalog => INVALID,
            _ => UNAVAILABLE,
        }
    }
}
impl std::fmt::Display for ChatGptModelsFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.public_message())
    }
}
impl std::error::Error for ChatGptModelsFailure {}

impl ChatGptModelsStage {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Authorization => "authorization",
            Self::Client => "client",
            Self::Request => "request",
            Self::HttpStatus => "http_status",
            Self::Body => "body",
            Self::Decode => "decode",
            Self::Scope => "scope",
            Self::Deadline => "deadline",
            Self::Complete => "complete",
            Self::Control => "control",
        }
    }
}
impl ChatGptCatalogDispatch {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotStarted => "not_started",
            Self::Attempted => "attempted",
            Self::ResponseReceived => "response_received",
            Self::Unknown => "unknown",
        }
    }
}
impl ChatGptModelsReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Credential => "credential",
            Self::ClientBuild => "client_build",
            Self::RequestBuild => "request_build",
            Self::Connect => "connect",
            Self::Transport => "transport",
            Self::HttpStatus => "http_status",
            Self::DeclaredLimit => "declared_limit",
            Self::StreamedLimit => "streamed_limit",
            Self::BodyRead => "body_read",
            Self::InvalidCatalog => "invalid_catalog",
            Self::ScopeChanged => "scope_changed",
            Self::Timeout => "timeout",
            Self::Disconnected => "disconnected",
            Self::Busy => "busy",
            Self::Vault => "vault",
        }
    }
}

/// A host-supplied current subscription credential. No fallback or inference.
pub struct ChatGptModelsAdapter {
    client: reqwest::Client,
    token: Zeroizing<String>,
    account_id: String,
    #[cfg(test)]
    test_origin: Option<String>,
}

fn client(timeout: Duration) -> Result<reqwest::Client, ChatGptModelsFailure> {
    build_client(
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(timeout),
    )
}
pub(super) fn build_client(
    builder: reqwest::ClientBuilder,
) -> Result<reqwest::Client, ChatGptModelsFailure> {
    builder.build().map_err(|_| {
        ChatGptModelsFailure::new(
            ChatGptModelsStage::Client,
            ChatGptCatalogDispatch::NotStarted,
            ChatGptModelsReason::ClientBuild,
            None,
        )
    })
}

impl ChatGptModelsAdapter {
    pub fn new_detailed(
        access_token: String,
        account_id: String,
    ) -> Result<Self, ChatGptModelsFailure> {
        let token = Zeroizing::new(access_token);
        if token.is_empty() || account_id.is_empty() {
            return Err(ChatGptModelsFailure::new(
                ChatGptModelsStage::Client,
                ChatGptCatalogDispatch::NotStarted,
                ChatGptModelsReason::Credential,
                None,
            ));
        }
        Ok(Self {
            client: client(Duration::from_secs(10))?,
            token,
            account_id,
            #[cfg(test)]
            test_origin: None,
        })
    }
    pub fn new(access_token: String, account_id: String) -> Result<Self, String> {
        Self::new_detailed(access_token, account_id)
            .map_err(|failure| failure.public_message().to_owned())
    }
    pub async fn fetch(&self) -> Result<Vec<String>, String> {
        self.fetch_detailed()
            .await
            .map_err(|failure| failure.public_message().to_owned())
    }
    pub async fn fetch_detailed(&self) -> Result<Vec<String>, ChatGptModelsFailure> {
        #[cfg(test)]
        let test_endpoint = self.test_origin.as_ref().map(|origin| {
            format!(
                "{origin}{}",
                MODELS_ENDPOINT.strip_prefix("https://chatgpt.com").unwrap()
            )
        });
        let endpoint = MODELS_ENDPOINT;
        #[cfg(test)]
        let endpoint = test_endpoint.as_deref().unwrap_or(endpoint);
        let request = self
            .client
            .get(endpoint)
            .bearer_auth(self.token.as_str())
            .header("chatgpt-account-id", &self.account_id)
            .header("originator", "anima")
            .header("User-Agent", "anima/0.1.0")
            .build()
            .map_err(|_| {
                ChatGptModelsFailure::new(
                    ChatGptModelsStage::Request,
                    ChatGptCatalogDispatch::NotStarted,
                    ChatGptModelsReason::RequestBuild,
                    None,
                )
            })?;
        let response = self.client.execute(request).await.map_err(|error| {
            ChatGptModelsFailure::new(
                ChatGptModelsStage::Request,
                ChatGptCatalogDispatch::Attempted,
                if error.is_timeout() {
                    ChatGptModelsReason::Timeout
                } else if error.is_connect() {
                    ChatGptModelsReason::Connect
                } else {
                    ChatGptModelsReason::Transport
                },
                None,
            )
        })?;
        let status = Some(response.status().as_u16());
        // Error bodies have no catalog authority and are never read or echoed.
        if !response.status().is_success() {
            return Err(ChatGptModelsFailure::new(
                ChatGptModelsStage::HttpStatus,
                ChatGptCatalogDispatch::ResponseReceived,
                ChatGptModelsReason::HttpStatus,
                status,
            ));
        }
        if response
            .content_length()
            .is_some_and(|size| size > MAX_BODY as u64)
        {
            return Err(ChatGptModelsFailure::new(
                ChatGptModelsStage::Body,
                ChatGptCatalogDispatch::ResponseReceived,
                ChatGptModelsReason::DeclaredLimit,
                status,
            ));
        }
        let mut body = Zeroizing::new(Vec::new());
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|error| {
                ChatGptModelsFailure::new(
                    ChatGptModelsStage::Body,
                    ChatGptCatalogDispatch::ResponseReceived,
                    if error.is_timeout() {
                        ChatGptModelsReason::Timeout
                    } else {
                        ChatGptModelsReason::BodyRead
                    },
                    status,
                )
            })?;
            if body.len().saturating_add(chunk.len()) > MAX_BODY {
                return Err(ChatGptModelsFailure::new(
                    ChatGptModelsStage::Body,
                    ChatGptCatalogDispatch::ResponseReceived,
                    ChatGptModelsReason::StreamedLimit,
                    status,
                ));
            }
            body.extend_from_slice(&chunk);
        }
        decode_models(&body).map_err(|_| {
            ChatGptModelsFailure::new(
                ChatGptModelsStage::Decode,
                ChatGptCatalogDispatch::ResponseReceived,
                ChatGptModelsReason::InvalidCatalog,
                status,
            )
        })
    }

    #[cfg(test)]
    pub(super) fn at_test_origin(
        mut self,
        origin: String,
        timeout: Duration,
    ) -> Result<Self, String> {
        self.client = client(timeout).map_err(|failure| failure.public_message().to_owned())?;
        self.test_origin = Some(origin);
        Ok(self)
    }
    #[cfg(test)]
    pub(super) fn with_test_client(mut self, client: reqwest::Client) -> Self {
        self.client = client;
        self
    }
}

pub(super) fn decode_models(body: &[u8]) -> Result<Vec<String>, String> {
    let invalid = || INVALID.to_owned();
    if body.len() > MAX_BODY {
        return Err(invalid());
    }
    let payload: Value = serde_json::from_slice(body).map_err(|_| invalid())?;
    let rows = payload
        .get("models")
        .and_then(Value::as_array)
        .ok_or_else(invalid)?;
    if rows.len() > MAX_ROWS {
        return Err(invalid());
    }
    let mut ranked = Vec::with_capacity(rows.len());
    for row in rows {
        let row = row.as_object().ok_or_else(invalid)?;
        let id = row
            .get("slug")
            .and_then(Value::as_str)
            .ok_or_else(invalid)?;
        if id.is_empty()
            || id.len() > 128
            || !id.as_bytes()[0].is_ascii_alphanumeric()
            || !id.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'/' | b'-')
            })
        {
            return Err(invalid());
        }
        let priority = match row.get("priority") {
            None | Some(Value::Null) => 10_000,
            Some(value) => value
                .as_i64()
                .filter(|value| (-JS_SAFE_INTEGER..=JS_SAFE_INTEGER).contains(value))
                .ok_or_else(invalid)?,
        };
        let visible = match row.get("visibility") {
            None | Some(Value::Null) => true,
            Some(Value::String(value)) if value == "list" => true,
            Some(Value::String(value)) if value == "hide" || value == "hidden" => false,
            _ => return Err(invalid()),
        };
        // Validate consumed fields even on hidden rows. supported_in_api and
        // all other unused metadata are deliberately ignored.
        if visible {
            ranked.push((priority, id.to_owned()));
        }
    }
    ranked.sort_unstable();
    let mut seen = BTreeSet::new();
    Ok(ranked
        .into_iter()
        .filter_map(|(_, id)| seen.insert(id.clone()).then_some(id))
        .collect())
}
