//! Selected Drive metadata/download branch adapted from google_api.py; see HERMES-NOTICE.
use super::model::*;
use crate::connectors::gcalendar::store::GoogleOAuthTokens;
use async_trait::async_trait;
use futures::StreamExt;
use reqwest::{
    header::{HeaderValue, AUTHORIZATION},
    redirect::Policy,
    Client, Method, Url,
};
use serde::{de::DeserializeOwned, Deserialize};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use zeroize::Zeroizing;
const METADATA_LIMIT: usize = 64 * 1024;
const CONTENT_LIMIT: usize = 1024 * 1024;
const FIELDS: &str = "id,name,mimeType,version,modifiedTime,size,trashed,capabilities(canDownload)";

#[async_trait]
pub(crate) trait DriveTransport: Send + Sync {
    async fn exchange_code(
        &self,
        config: &DriveOAuthConfig,
        code: &str,
        verifier: &str,
    ) -> Result<DriveTokenGrant, DriveError>;
    async fn refresh_tokens(
        &self,
        config: &DriveOAuthConfig,
        refresh_token: &str,
    ) -> Result<DriveTokenGrant, DriveError>;
    async fn identify(&self, access_token: &str) -> Result<DriveAccount, DriveError>;
    async fn metadata(
        &self,
        access_token: &str,
        id: &DriveFileId,
    ) -> Result<DriveFileMetadata, DriveError>;
    async fn read_text(
        &self,
        access_token: &str,
        metadata: &DriveFileMetadata,
    ) -> Result<DriveTextBytes, DriveError>;
    async fn read_binary(
        &self,
        _access_token: &str,
        _metadata: &DriveFileMetadata,
    ) -> Result<Vec<u8>, DriveError> {
        Err(DriveError::InvalidInput)
    }
}
pub(crate) struct DriveClient {
    client: Client,
    api_base: Url,
    token_url: Url,
}
impl DriveClient {
    pub(crate) fn new() -> Result<Self, DriveError> {
        Self::build(
            "https://www.googleapis.com/drive/v3/",
            "https://oauth2.googleapis.com/token",
        )
    }
    fn build(api: &str, token: &str) -> Result<Self, DriveError> {
        Ok(Self {
            client: Client::builder()
                .redirect(Policy::none())
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(10))
                .build()
                .map_err(|_| DriveError::Unconfigured)?,
            api_base: Url::parse(api).map_err(|_| DriveError::Unconfigured)?,
            token_url: Url::parse(token).map_err(|_| DriveError::Unconfigured)?,
        })
    }
    #[cfg(test)]
    fn for_test_base(api: &str, token: &str) -> Result<Self, DriveError> {
        for input in [api, token] {
            let url = Url::parse(input).map_err(|_| DriveError::InvalidInput)?;
            if url.scheme() != "http"
                || !matches!(url.host_str(), Some("127.0.0.1" | "localhost"))
                || !url.username().is_empty()
                || url.password().is_some()
            {
                return Err(DriveError::InvalidInput);
            }
        }
        Self::build(api, token)
    }
    async fn bounded(
        &self,
        request: reqwest::RequestBuilder,
        cap: usize,
        oauth: bool,
    ) -> Result<Zeroizing<Vec<u8>>, DriveError> {
        let response = request.send().await.map_err(network_error)?;
        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(match status {
                400 if oauth => DriveError::ReauthRequired,
                400 => DriveError::InvalidInput,
                401 => DriveError::ReauthRequired,
                403 => DriveError::PermissionDenied,
                404 => DriveError::NotFound,
                429 => DriveError::RateLimited,
                300..=399 => DriveError::InvalidResponse,
                _ => DriveError::Transport,
            });
        }
        if response
            .content_length()
            .is_some_and(|len| len > cap as u64)
        {
            return Err(DriveError::TooLarge);
        }
        let mut bytes = Zeroizing::new(Vec::new());
        let mut stream = response.bytes_stream();
        while let Some(part) = stream.next().await {
            let part = part.map_err(network_error)?;
            if bytes.len().saturating_add(part.len()) > cap {
                return Err(DriveError::TooLarge);
            }
            bytes.extend_from_slice(&part);
        }
        Ok(bytes)
    }
    async fn token(
        &self,
        pairs: &[(&str, &str)],
        old_refresh: Option<&str>,
    ) -> Result<DriveTokenGrant, DriveError> {
        let bytes = self
            .bounded(
                self.client.post(self.token_url.clone()).form(pairs),
                METADATA_LIMIT,
                true,
            )
            .await?;
        let wire: TokenResponse = decode(&bytes)?;
        if wire
            .token_type
            .as_ref()
            .is_some_and(|t| !t.eq_ignore_ascii_case("bearer"))
            || !valid_token(&wire.access_token.0)
        {
            return Err(DriveError::InvalidResponse);
        }
        let refresh = match wire.refresh_token {
            Some(value) if valid_token(&value.0) => value.0,
            None => Zeroizing::new(old_refresh.ok_or(DriveError::ReauthRequired)?.to_owned()),
            Some(_) => return Err(DriveError::InvalidResponse),
        };
        let seconds = wire
            .expires_in
            .filter(|v| *v > 0)
            .ok_or(DriveError::InvalidResponse)?;
        let expires_at_ms = now_ms()
            .checked_add(
                seconds
                    .checked_mul(1000)
                    .ok_or(DriveError::InvalidResponse)?,
            )
            .ok_or(DriveError::InvalidResponse)?;
        let scopes = match wire.scope {
            Some(scope) => {
                let scopes: Vec<String> = scope.split_whitespace().map(str::to_owned).collect();
                if !scopes.iter().any(|s| s == DRIVE_SCOPE) {
                    return Err(DriveError::ReauthRequired);
                }
                if scopes.len() > 32 || scopes.iter().any(|s| s.len() > 2048 || !s.is_ascii()) {
                    return Err(DriveError::InvalidResponse);
                }
                scopes
            }
            None if old_refresh.is_some() => Vec::new(),
            None => return Err(DriveError::ReauthRequired),
        };
        Ok(DriveTokenGrant {
            tokens: GoogleOAuthTokens::new(wire.access_token.0, refresh, expires_at_ms),
            granted_scopes: scopes,
        })
    }
    fn get(&self, access: &str, url: Url) -> Result<reqwest::RequestBuilder, DriveError> {
        if !valid_token(access) {
            return Err(DriveError::InvalidInput);
        }
        let auth = Zeroizing::new(format!("Bearer {access}"));
        let mut value = HeaderValue::from_str(&auth).map_err(|_| DriveError::InvalidInput)?;
        value.set_sensitive(true);
        Ok(self
            .client
            .request(Method::GET, url)
            .header(AUTHORIZATION, value))
    }
    async fn get_json<T: DeserializeOwned>(&self, access: &str, url: Url) -> Result<T, DriveError> {
        decode(
            &self
                .bounded(self.get(access, url)?, METADATA_LIMIT, false)
                .await?,
        )
    }
    fn file_url(&self, id: &DriveFileId, suffix: &str) -> Result<Url, DriveError> {
        self.api_base
            .join(&format!("files/{}{suffix}", id.as_str()))
            .map_err(|_| DriveError::InvalidInput)
    }
}
fn network_error(error: reqwest::Error) -> DriveError {
    if error.is_timeout() {
        DriveError::Deadline
    } else {
        DriveError::Transport
    }
}
fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, DriveError> {
    serde_json::from_slice(bytes).map_err(|_| DriveError::InvalidResponse)
}
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}
#[derive(Deserialize)]
struct TokenResponse {
    access_token: OAuthSecret,
    refresh_token: Option<OAuthSecret>,
    expires_in: Option<u64>,
    scope: Option<String>,
    token_type: Option<String>,
}
struct OAuthSecret(Zeroizing<String>);
impl<'de> Deserialize<'de> for OAuthSecret {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d).map(|s| Self(Zeroizing::new(s)))
    }
}
#[derive(Deserialize)]
struct About {
    user: AboutUser,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AboutUser {
    permission_id: String,
    me: bool,
    email_address: Option<String>,
    display_name: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileMetadata {
    id: String,
    name: String,
    mime_type: String,
    version: String,
    modified_time: Option<String>,
    size: Option<String>,
    trashed: bool,
    capabilities: Capabilities,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Capabilities {
    can_download: bool,
}
#[async_trait]
impl DriveTransport for DriveClient {
    async fn read_binary(
        &self,
        access: &str,
        metadata: &DriveFileMetadata,
    ) -> Result<Vec<u8>, DriveError> {
        if metadata.trashed
            || !metadata.can_download
            || !matches!(
                metadata.mime_type.as_str(),
                "application/pdf"
                    | "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
                    | "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
                    | "text/csv"
            )
        {
            return Err(DriveError::InvalidInput);
        }
        let mut url = self.file_url(&metadata.id, "")?;
        url.query_pairs_mut().append_pair("alt", "media");
        Ok(self
            .bounded(self.get(access, url)?, 2 * 1024 * 1024, false)
            .await?
            .to_vec())
    }
    async fn exchange_code(
        &self,
        config: &DriveOAuthConfig,
        code: &str,
        verifier: &str,
    ) -> Result<DriveTokenGrant, DriveError> {
        if code.is_empty()
            || code.len() > 8192
            || code.chars().any(char::is_control)
            || !(43..=128).contains(&verifier.len())
            || !verifier
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~'))
        {
            return Err(DriveError::InvalidInput);
        }
        self.token(
            &[
                ("grant_type", "authorization_code"),
                ("client_id", &config.client_id),
                ("client_secret", config.client_secret()),
                ("code", code),
                ("code_verifier", verifier),
                ("redirect_uri", &config.redirect_uri),
            ],
            None,
        )
        .await
    }
    async fn refresh_tokens(
        &self,
        config: &DriveOAuthConfig,
        refresh: &str,
    ) -> Result<DriveTokenGrant, DriveError> {
        if !valid_token(refresh) {
            return Err(DriveError::InvalidInput);
        }
        self.token(
            &[
                ("grant_type", "refresh_token"),
                ("client_id", &config.client_id),
                ("client_secret", config.client_secret()),
                ("refresh_token", refresh),
            ],
            Some(refresh),
        )
        .await
    }
    async fn identify(&self, access: &str) -> Result<DriveAccount, DriveError> {
        let mut url = self
            .api_base
            .join("about")
            .map_err(|_| DriveError::Unconfigured)?;
        url.query_pairs_mut()
            .append_pair("fields", "user(permissionId,emailAddress,displayName,me)");
        let wire: About = self.get_json(access, url).await?;
        let user = wire.user;
        if !user.me
            || user.permission_id.trim().is_empty()
            || user.permission_id.chars().count() > 256
            || user.permission_id.chars().any(char::is_control)
        {
            return Err(DriveError::InvalidResponse);
        }
        for label in [&user.email_address, &user.display_name]
            .into_iter()
            .flatten()
        {
            if label.chars().count() > 320 || label.chars().any(char::is_control) {
                return Err(DriveError::InvalidResponse);
            }
        }
        Ok(DriveAccount {
            permission_id: user.permission_id,
            label: user
                .email_address
                .filter(|v| !v.trim().is_empty())
                .or_else(|| user.display_name.filter(|v| !v.trim().is_empty())),
        })
    }
    async fn metadata(
        &self,
        access: &str,
        id: &DriveFileId,
    ) -> Result<DriveFileMetadata, DriveError> {
        let mut url = self.file_url(id, "")?;
        url.query_pairs_mut().append_pair("fields", FIELDS);
        let wire: FileMetadata = self.get_json(access, url).await?;
        if wire.id != id.as_str()
            || wire.name.chars().count() > 1024
            || wire.mime_type.is_empty()
            || wire.mime_type.len() > 256
            || !wire.mime_type.is_ascii()
            || wire.mime_type.chars().any(char::is_control)
            || wire.version.is_empty()
            || wire.version.len() > 20
            || !wire.version.bytes().all(|b| b.is_ascii_digit())
            || wire
                .version
                .parse::<u64>()
                .ok()
                .filter(|v| *v > 0)
                .is_none()
        {
            return Err(DriveError::InvalidResponse);
        }
        if let Some(time) = &wire.modified_time {
            if time.len() > 64 || chrono::DateTime::parse_from_rfc3339(time).is_err() {
                return Err(DriveError::InvalidResponse);
            }
        }
        let size = wire
            .size
            .map(|s| {
                if s.is_empty() || s.len() > 20 || !s.bytes().all(|b| b.is_ascii_digit()) {
                    Err(DriveError::InvalidResponse)
                } else {
                    s.parse::<u64>().map_err(|_| DriveError::InvalidResponse)
                }
            })
            .transpose()?;
        Ok(DriveFileMetadata {
            id: id.clone(),
            name: wire.name,
            mime_type: wire.mime_type,
            version: wire.version,
            modified_time: wire.modified_time,
            size_bytes: size,
            trashed: wire.trashed,
            can_download: wire.capabilities.can_download,
            source_url: format!("https://drive.google.com/file/d/{id}/view"),
        })
    }
    async fn read_text(
        &self,
        access: &str,
        metadata: &DriveFileMetadata,
    ) -> Result<DriveTextBytes, DriveError> {
        if metadata.trashed {
            return Err(DriveError::NotFound);
        }
        if !metadata.can_download {
            return Err(DriveError::PermissionDenied);
        }
        let (mut url, representation) = match metadata.mime_type.as_str() {
            "application/vnd.google-apps.document" => {
                let mut url = self.file_url(&metadata.id, "/export")?;
                url.query_pairs_mut().append_pair("mimeType", "text/plain");
                (url, "google_docs_plain_text")
            }
            "text/plain" => {
                let mut url = self.file_url(&metadata.id, "")?;
                url.query_pairs_mut().append_pair("alt", "media");
                (url, "stored_plain_text")
            }
            _ => return Err(DriveError::InvalidInput),
        };
        // URL derives solely from validated file ID and fixed API base; provider links are ignored.
        url.set_fragment(None);
        let bytes = self
            .bounded(self.get(access, url)?, CONTENT_LIMIT, false)
            .await?;
        std::str::from_utf8(&bytes).map_err(|_| DriveError::InvalidUtf8)?;
        Ok(DriveTextBytes {
            bytes: bytes.to_vec(),
            representation: representation.into(),
        })
    }
}
#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
