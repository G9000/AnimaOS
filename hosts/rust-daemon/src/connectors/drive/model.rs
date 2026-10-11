//! Anima selected-file Drive model. Source behavior adaptation: HERMES-NOTICE.
use crate::connectors::{gcalendar::store::GoogleOAuthTokens, oauth_apps::ResolvedOAuthApp};
use serde::{Deserialize, Serialize};
use std::fmt;
use zeroize::Zeroizing;

pub(crate) const DRIVE_SCOPE: &str = "https://www.googleapis.com/auth/drive.readonly";
pub(crate) const DRIVE_TOOL_NAMES: [&str; 2] = ["drive_search", "drive_read"];
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct DriveFileId(String);
impl DriveFileId {
    pub(crate) fn parse(value: impl Into<String>) -> Result<Self, DriveError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 256
            || !value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
        {
            return Err(DriveError::InvalidInput);
        }
        Ok(Self(value))
    }
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}
impl Serialize for DriveFileId {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}
impl<'de> Deserialize<'de> for DriveFileId {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let value = String::deserialize(d)?;
        Self::parse(value)
            .map_err(|_| serde::de::Error::custom("invalid selected Drive file identifier"))
    }
}
impl fmt::Display for DriveFileId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum DriveStatus {
    Pairing,
    Active,
    ReauthRequired,
    Disconnected,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DriveRecord {
    pub(crate) id: String,
    pub(crate) agent_id: String,
    pub(crate) status: DriveStatus,
    pub(crate) generation: String,
    pub(crate) selected_file_ids: Vec<DriveFileId>,
    pub(crate) account_permission_id: Option<String>,
    pub(crate) account_label: Option<String>,
    pub(crate) oauth_config_revision: u64,
    pub(crate) created_at_ms: u64,
    pub(crate) updated_at_ms: u64,
    #[serde(default)]
    pub(crate) cleanup_generations: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DriveAccount {
    pub(crate) permission_id: String,
    pub(crate) label: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DriveFileMetadata {
    pub(crate) id: DriveFileId,
    pub(crate) name: String,
    pub(crate) mime_type: String,
    pub(crate) version: String,
    pub(crate) modified_time: Option<String>,
    pub(crate) size_bytes: Option<u64>,
    pub(crate) trashed: bool,
    pub(crate) can_download: bool,
    pub(crate) source_url: String,
}
impl DriveFileMetadata {
    pub(crate) fn supported_text(&self) -> bool {
        matches!(
            self.mime_type.as_str(),
            "application/vnd.google-apps.document" | "text/plain"
        )
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum DriveFileAvailability {
    Available,
    Unsupported,
    Unavailable,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DriveSearchItem {
    pub(crate) file_id: DriveFileId,
    pub(crate) metadata: Option<DriveFileMetadata>,
    pub(crate) availability: DriveFileAvailability,
    pub(crate) reason: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DriveSearchResult {
    pub(crate) files: Vec<DriveSearchItem>,
    pub(crate) searched_selected_count: usize,
    pub(crate) search_scope: String,
    pub(crate) fetched_at_ms: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DriveTextResult {
    pub(crate) metadata: DriveFileMetadata,
    pub(crate) text: String,
    pub(crate) representation: String,
    pub(crate) source_bytes: usize,
    pub(crate) returned_bytes: usize,
    pub(crate) returned_scalars: usize,
    pub(crate) truncated: bool,
    pub(crate) source_bytes_sha256: String,
    pub(crate) returned_text_sha256: String,
    pub(crate) fetched_at_ms: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum DriveReadResult {
    Text(DriveTextResult),
    Unsupported {
        metadata: DriveFileMetadata,
        reason: String,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DriveTextBytes {
    pub(crate) bytes: Vec<u8>,
    pub(crate) representation: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum DriveError {
    InvalidInput,
    NotFound,
    NotConnected,
    ReauthRequired,
    Unconfigured,
    NotSelected,
    Conflict,
    PermissionDenied,
    RateLimited,
    Transport,
    InvalidResponse,
    TooLarge,
    InvalidUtf8,
    Deadline,
    Credential,
    Persistence,
}
impl fmt::Display for DriveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidInput => "invalid selected Drive request",
            Self::NotFound => "selected Drive resource unavailable",
            Self::NotConnected => "Drive is not connected",
            Self::ReauthRequired => "Drive requires renewed consent",
            Self::Unconfigured => "Google OAuth application is not configured",
            Self::NotSelected => "Drive file is not selected",
            Self::Conflict => "Drive connection or file changed",
            Self::PermissionDenied => "Drive read permission denied",
            Self::RateLimited => "Drive rate limit reached",
            Self::Transport => "Drive request unavailable",
            Self::InvalidResponse => "Drive returned an invalid response",
            Self::TooLarge => "Drive response exceeds the read limit",
            Self::InvalidUtf8 => "Drive text is not valid UTF-8",
            Self::Deadline => "Drive request deadline reached",
            Self::Credential => "Drive credential vault outcome could not be confirmed",
            Self::Persistence => "Drive state could not be saved",
        })
    }
}
impl std::error::Error for DriveError {}

pub(crate) struct DriveOAuthConfig {
    pub(crate) client_id: String,
    client_secret: Zeroizing<String>,
    pub(crate) redirect_uri: String,
    pub(crate) revision: u64,
}
impl DriveOAuthConfig {
    pub(crate) fn new(
        client_id: String,
        client_secret: Zeroizing<String>,
        redirect_uri: String,
        revision: u64,
    ) -> Result<Self, DriveError> {
        let redirect = reqwest::Url::parse(&redirect_uri).map_err(|_| DriveError::InvalidInput)?;
        if client_id.is_empty()
            || client_id.len() > 2048
            || client_id.chars().any(char::is_control)
            || client_secret.is_empty()
            || client_secret.len() > 4096
            || client_secret.chars().any(char::is_control)
            || !matches!(redirect.scheme(), "http" | "https")
            || redirect.host_str().is_none()
            || !redirect.username().is_empty()
            || redirect.password().is_some()
            || redirect.query().is_some()
            || redirect.fragment().is_some()
        {
            return Err(DriveError::InvalidInput);
        }
        Ok(Self {
            client_id,
            client_secret,
            redirect_uri,
            revision,
        })
    }
    pub(crate) fn from_resolved(resolved: &ResolvedOAuthApp) -> Self {
        Self {
            client_id: resolved.credentials().client_id().into(),
            client_secret: Zeroizing::new(resolved.credentials().client_secret().into()),
            redirect_uri: resolved.callback_uri("/api/connectors/gdrive/callback"),
            revision: resolved.revision(),
        }
    }
    pub(crate) fn client_secret(&self) -> &str {
        self.client_secret.as_str()
    }
    pub(crate) fn consent_url(&self, state: &str, challenge: &str) -> String {
        let mut url = reqwest::Url::parse("https://accounts.google.com/o/oauth2/v2/auth")
            .expect("constant Google OAuth origin");
        url.query_pairs_mut().extend_pairs([
            ("client_id", self.client_id.as_str()),
            ("redirect_uri", self.redirect_uri.as_str()),
            ("response_type", "code"),
            ("scope", DRIVE_SCOPE),
            ("access_type", "offline"),
            ("prompt", "consent"),
            ("state", state),
            ("code_challenge_method", "S256"),
            ("code_challenge", challenge),
        ]);
        url.into()
    }
}
impl fmt::Debug for DriveOAuthConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DriveOAuthConfig([REDACTED])")
    }
}
#[derive(Clone)]
pub(crate) struct DriveTokenGrant {
    pub(crate) tokens: GoogleOAuthTokens,
    pub(crate) granted_scopes: Vec<String>,
}
impl DriveTokenGrant {
    /// Refresh may temporarily have empty scopes only when Google omitted that field.
    /// The manager inherits its previous validated grant before any vault write.
    pub(crate) fn validate(&self) -> Result<(), DriveError> {
        if !self.granted_scopes.iter().any(|s| s == DRIVE_SCOPE) {
            return Err(DriveError::ReauthRequired);
        }
        if self.granted_scopes.len() > 32
            || self.granted_scopes.iter().any(|s| {
                s.is_empty()
                    || s.len() > 2048
                    || !s.is_ascii()
                    || s.chars().any(char::is_whitespace)
            })
            || !valid_token(self.tokens.access_token())
            || !valid_token(self.tokens.refresh_token())
            || self.tokens.expires_at_ms() == 0
        {
            return Err(DriveError::Credential);
        }
        Ok(())
    }
}
impl fmt::Debug for DriveTokenGrant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DriveTokenGrant([REDACTED])")
    }
}
pub(crate) fn valid_token(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= 16384
        && token.is_ascii()
        && !token
            .bytes()
            .any(|b| b.is_ascii_control() || b.is_ascii_whitespace())
}
