//! Microsoft Graph file operations with explicit redirect and network boundaries.
use super::{model::*, store::MicrosoftFileGrant};
use crate::connectors::oauth_apps::ResolvedOAuthApp;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use std::{
    net::{IpAddr, Ipv4Addr},
    time::Duration,
};
use zeroize::Zeroizing;

#[derive(Clone)]
pub(crate) struct MicrosoftFileOAuthConfig {
    pub(crate) client_id: String,
    pub(crate) client_secret: Zeroizing<String>,
    pub(crate) tenant: String,
    pub(crate) redirect_uri: String,
    pub(crate) revision: u64,
}
impl MicrosoftFileOAuthConfig {
    pub(crate) fn from_resolved(value: &ResolvedOAuthApp) -> Self {
        Self {
            client_id: value.credentials().client_id().into(),
            client_secret: value.credentials().client_secret().to_string().into(),
            tenant: value.credentials().tenant().unwrap_or("common").into(),
            redirect_uri: value.callback_uri("/api/connectors/microsoft-files/callback"),
            revision: value.revision(),
        }
    }
    pub(crate) fn scopes(mode: MicrosoftFileMode) -> &'static str {
        match mode {
            MicrosoftFileMode::OneDrive => "offline_access https://graph.microsoft.com/User.Read https://graph.microsoft.com/Files.Read",
            MicrosoftFileMode::SharePoint => "offline_access https://graph.microsoft.com/User.Read https://graph.microsoft.com/Sites.Selected",
        }
    }
    pub(crate) fn consent_url(
        &self,
        mode: MicrosoftFileMode,
        state: &str,
        challenge: &str,
    ) -> Result<String, MicrosoftFileError> {
        let mut url = reqwest::Url::parse(&format!(
            "https://login.microsoftonline.com/{}/oauth2/v2.0/authorize",
            self.tenant
        ))
        .map_err(|_| MicrosoftFileError::Unconfigured)?;
        url.query_pairs_mut().extend_pairs([
            ("client_id", self.client_id.as_str()),
            ("redirect_uri", self.redirect_uri.as_str()),
            ("response_type", "code"),
            ("response_mode", "query"),
            ("scope", Self::scopes(mode)),
            ("state", state),
            ("code_challenge", challenge),
            ("code_challenge_method", "S256"),
            ("prompt", "consent"),
        ]);
        Ok(url.to_string())
    }
}

#[derive(Clone, Debug)]
pub(crate) struct MicrosoftIdentity {
    pub(crate) id: String,
    pub(crate) label: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct MicrosoftFilePage {
    pub(crate) files: Vec<MicrosoftFileMetadata>,
    pub(crate) next_url: Option<String>,
}

#[async_trait]
pub(crate) trait MicrosoftFileTransport: Send + Sync {
    async fn exchange(
        &self,
        config: &MicrosoftFileOAuthConfig,
        mode: MicrosoftFileMode,
        code: &str,
        verifier: &str,
    ) -> Result<MicrosoftFileGrant, MicrosoftFileError>;
    async fn refresh(
        &self,
        config: &MicrosoftFileOAuthConfig,
        old: &MicrosoftFileGrant,
    ) -> Result<MicrosoftFileGrant, MicrosoftFileError>;
    async fn identity(&self, access: &str) -> Result<MicrosoftIdentity, MicrosoftFileError>;
    async fn site_id(&self, access: &str, url: &str) -> Result<String, MicrosoftFileError>;
    async fn list(
        &self,
        access: &str,
        mode: MicrosoftFileMode,
        site_id: Option<&str>,
        folder: Option<(&str, &str)>,
        next: Option<&str>,
    ) -> Result<MicrosoftFilePage, MicrosoftFileError>;
    async fn metadata(
        &self,
        access: &str,
        drive: &str,
        item: &str,
    ) -> Result<MicrosoftFileMetadata, MicrosoftFileError>;
    async fn content(
        &self,
        access: &str,
        drive: &str,
        item: &str,
        approved_host: Option<&str>,
    ) -> Result<Vec<u8>, MicrosoftFileError>;
}

pub(crate) struct MicrosoftGraphClient {
    http: reqwest::Client,
}
pub(crate) struct UnconfiguredMicrosoftFileTransport;
#[async_trait]
impl MicrosoftFileTransport for UnconfiguredMicrosoftFileTransport {
    async fn exchange(
        &self,
        _: &MicrosoftFileOAuthConfig,
        _: MicrosoftFileMode,
        _: &str,
        _: &str,
    ) -> Result<MicrosoftFileGrant, MicrosoftFileError> {
        Err(MicrosoftFileError::Unconfigured)
    }
    async fn refresh(
        &self,
        _: &MicrosoftFileOAuthConfig,
        _: &MicrosoftFileGrant,
    ) -> Result<MicrosoftFileGrant, MicrosoftFileError> {
        Err(MicrosoftFileError::Unconfigured)
    }
    async fn identity(&self, _: &str) -> Result<MicrosoftIdentity, MicrosoftFileError> {
        Err(MicrosoftFileError::Unconfigured)
    }
    async fn site_id(&self, _: &str, _: &str) -> Result<String, MicrosoftFileError> {
        Err(MicrosoftFileError::Unconfigured)
    }
    async fn list(
        &self,
        _: &str,
        _: MicrosoftFileMode,
        _: Option<&str>,
        _: Option<(&str, &str)>,
        _: Option<&str>,
    ) -> Result<MicrosoftFilePage, MicrosoftFileError> {
        Err(MicrosoftFileError::Unconfigured)
    }
    async fn metadata(
        &self,
        _: &str,
        _: &str,
        _: &str,
    ) -> Result<MicrosoftFileMetadata, MicrosoftFileError> {
        Err(MicrosoftFileError::Unconfigured)
    }
    async fn content(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: Option<&str>,
    ) -> Result<Vec<u8>, MicrosoftFileError> {
        Err(MicrosoftFileError::Unconfigured)
    }
}
impl MicrosoftGraphClient {
    pub(crate) fn new() -> Result<Self, MicrosoftFileError> {
        Ok(Self {
            http: reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(20))
                .build()
                .map_err(|_| MicrosoftFileError::Unconfigured)?,
        })
    }
    async fn bytes(
        &self,
        request: reqwest::RequestBuilder,
        limit: usize,
    ) -> Result<Zeroizing<Vec<u8>>, MicrosoftFileError> {
        let response = request
            .send()
            .await
            .map_err(|_| MicrosoftFileError::Unavailable)?;
        match response.status().as_u16() {
            200..=299 => {}
            401 => return Err(MicrosoftFileError::Reauthorization),
            403 => return Err(MicrosoftFileError::PermissionDenied),
            404 => return Err(MicrosoftFileError::NotFound),
            _ => return Err(MicrosoftFileError::Unavailable),
        }
        bounded_response(response, limit).await
    }
    async fn json(&self, request: reqwest::RequestBuilder) -> Result<Value, MicrosoftFileError> {
        serde_json::from_slice(&self.bytes(request, 1024 * 1024).await?)
            .map_err(|_| MicrosoftFileError::Unavailable)
    }
    async fn token(
        &self,
        config: &MicrosoftFileOAuthConfig,
        mode: MicrosoftFileMode,
        form: &[(&str, &str)],
        old: Option<&MicrosoftFileGrant>,
    ) -> Result<MicrosoftFileGrant, MicrosoftFileError> {
        let url = format!(
            "https://login.microsoftonline.com/{}/oauth2/v2.0/token",
            config.tenant
        );
        let mut fields = vec![
            ("client_id", config.client_id.as_str()),
            ("client_secret", config.client_secret.as_str()),
            ("redirect_uri", config.redirect_uri.as_str()),
        ];
        fields.extend_from_slice(form);
        let response = self
            .http
            .post(url)
            .form(&fields)
            .send()
            .await
            .map_err(|_| MicrosoftFileError::Unavailable)?;
        let status = response.status().as_u16();
        if !(200..=299).contains(&status) {
            if status == 400 {
                let body = bounded_response(response, 16 * 1024).await?;
                return Err(token_failure(status, &body));
            }
            return Err(token_failure(status, &[]));
        }
        let raw = bounded_response(response, 128 * 1024).await?;
        let wire: TokenResponse =
            serde_json::from_slice(&raw).map_err(|_| MicrosoftFileError::Reauthorization)?;
        if wire
            .token_type
            .as_deref()
            .is_some_and(|kind| !kind.eq_ignore_ascii_case("bearer"))
        {
            return Err(MicrosoftFileError::Reauthorization);
        }
        let refresh = wire
            .refresh_token
            .or_else(|| old.map(|grant| grant.refresh_token.clone()))
            .ok_or(MicrosoftFileError::Reauthorization)?;
        let scopes = wire
            .scope
            .map(|value| value.split_whitespace().map(str::to_owned).collect())
            .or_else(|| old.map(|grant| grant.scopes.clone()))
            .ok_or(MicrosoftFileError::Reauthorization)?;
        let seconds = wire
            .expires_in
            .filter(|seconds| *seconds > 0 && *seconds <= 86_400)
            .ok_or(MicrosoftFileError::Reauthorization)?;
        let grant = MicrosoftFileGrant {
            mode,
            access_token: wire.access_token,
            refresh_token: refresh,
            expires_at_ms: crate::connectors::gcalendar::now_ms()
                .saturating_add(seconds.saturating_mul(1000)),
            scopes,
        };
        grant
            .validate()
            .map_err(|_| MicrosoftFileError::Reauthorization)?;
        Ok(grant)
    }
}

async fn bounded_response(
    mut response: reqwest::Response,
    limit: usize,
) -> Result<Zeroizing<Vec<u8>>, MicrosoftFileError> {
    if response
        .content_length()
        .is_some_and(|size| size > limit as u64)
    {
        return Err(MicrosoftFileError::Capacity);
    }
    let mut bytes = Zeroizing::new(Vec::new());
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| MicrosoftFileError::Unavailable)?
    {
        if bytes.len().saturating_add(chunk.len()) > limit {
            return Err(MicrosoftFileError::Capacity);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn token_failure(status: u16, body: &[u8]) -> MicrosoftFileError {
    #[derive(Deserialize)]
    struct OAuthError {
        error: String,
    }
    if status == 401
        || (status == 400
            && serde_json::from_slice::<OAuthError>(body)
                .ok()
                .is_some_and(|value| {
                    matches!(
                        value.error.as_str(),
                        "invalid_grant"
                            | "interaction_required"
                            | "consent_required"
                            | "login_required"
                    )
                }))
    {
        MicrosoftFileError::Reauthorization
    } else {
        MicrosoftFileError::Unavailable
    }
}

fn deserialize_secret<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Zeroizing<String>, D::Error> {
    String::deserialize(deserializer).map(Zeroizing::new)
}
fn deserialize_optional_secret<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Zeroizing<String>>, D::Error> {
    Option::<String>::deserialize(deserializer).map(|value| value.map(Zeroizing::new))
}
#[derive(Deserialize)]
struct TokenResponse {
    #[serde(deserialize_with = "deserialize_secret")]
    access_token: Zeroizing<String>,
    #[serde(default, deserialize_with = "deserialize_optional_secret")]
    refresh_token: Option<Zeroizing<String>>,
    expires_in: Option<u64>,
    scope: Option<String>,
    token_type: Option<String>,
}

fn opaque(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}
fn graph_url(parts: &[&str]) -> Result<reqwest::Url, MicrosoftFileError> {
    let mut url = reqwest::Url::parse("https://graph.microsoft.com/v1.0/")
        .map_err(|_| MicrosoftFileError::Unconfigured)?;
    {
        let mut segments = url
            .path_segments_mut()
            .map_err(|_| MicrosoftFileError::Invalid)?;
        segments.pop_if_empty();
        for part in parts {
            if !opaque(part, 1024) {
                return Err(MicrosoftFileError::Invalid);
            }
            segments.push(part);
        }
    }
    Ok(url)
}
fn parse_metadata(value: &Value) -> Result<MicrosoftFileMetadata, MicrosoftFileError> {
    let item_id = value["id"]
        .as_str()
        .filter(|value| opaque(value, 1024))
        .ok_or(MicrosoftFileError::Unavailable)?
        .to_owned();
    let drive_id = value["parentReference"]["driveId"]
        .as_str()
        .filter(|value| opaque(value, 1024))
        .ok_or(MicrosoftFileError::Unavailable)?
        .to_owned();
    let name = value["name"]
        .as_str()
        .filter(|value| opaque(value, 2048))
        .ok_or(MicrosoftFileError::Unavailable)?
        .to_owned();
    Ok(MicrosoftFileMetadata {
        item_id,
        drive_id,
        name,
        mime: value["file"]["mimeType"].as_str().map(str::to_owned),
        size: value["size"].as_u64(),
        modified_at: value["lastModifiedDateTime"].as_str().map(str::to_owned),
        revision: value["eTag"].as_str().map(str::to_owned),
        source_url: value["webUrl"].as_str().map(str::to_owned),
        is_folder: value["folder"].is_object(),
    })
}
fn valid_next(base: &reqwest::Url, raw: &str) -> Result<reqwest::Url, MicrosoftFileError> {
    if raw.len() > 8192 {
        return Err(MicrosoftFileError::Unavailable);
    }
    let next = reqwest::Url::parse(raw).map_err(|_| MicrosoftFileError::Unavailable)?;
    if next.origin() != base.origin()
        || next.path() != base.path()
        || !next.username().is_empty()
        || next.password().is_some()
        || next.fragment().is_some()
    {
        return Err(MicrosoftFileError::Unavailable);
    }
    Ok(next)
}
fn public_ipv4(address: Ipv4Addr) -> bool {
    let [a, b, c, _] = address.octets();
    !(a == 0
        || a == 10
        || a == 127
        || a >= 224
        || (a == 169 && b == 254)
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && b == 168)
        || (a == 100 && (64..=127).contains(&b))
        || (a == 198 && (b == 18 || b == 19))
        || (a == 192 && b == 0 && (c == 0 || c == 2))
        || (a == 198 && b == 51 && c == 100)
        || (a == 203 && b == 0 && c == 113))
}
fn download_url(
    raw: &str,
    approved_host: Option<&str>,
) -> Result<reqwest::Url, MicrosoftFileError> {
    if raw.len() > 8192 {
        return Err(MicrosoftFileError::Unavailable);
    }
    let url = reqwest::Url::parse(raw).map_err(|_| MicrosoftFileError::Unavailable)?;
    let host = url.host_str().ok_or(MicrosoftFileError::Unavailable)?;
    let allowed = host.ends_with(".1drv.com")
        || host.ends_with(".sharepoint.com") && approved_host == Some(host);
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.fragment().is_some()
        || !allowed
    {
        return Err(MicrosoftFileError::Unavailable);
    }
    Ok(url)
}

#[async_trait]
impl MicrosoftFileTransport for MicrosoftGraphClient {
    async fn exchange(
        &self,
        config: &MicrosoftFileOAuthConfig,
        mode: MicrosoftFileMode,
        code: &str,
        verifier: &str,
    ) -> Result<MicrosoftFileGrant, MicrosoftFileError> {
        if !opaque(code, 8192)
            || !(43..=128).contains(&verifier.len())
            || !verifier.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~')
            })
        {
            return Err(MicrosoftFileError::Invalid);
        }
        self.token(
            config,
            mode,
            &[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("code_verifier", verifier),
            ],
            None,
        )
        .await
    }
    async fn refresh(
        &self,
        config: &MicrosoftFileOAuthConfig,
        old: &MicrosoftFileGrant,
    ) -> Result<MicrosoftFileGrant, MicrosoftFileError> {
        self.token(
            config,
            old.mode,
            &[
                ("grant_type", "refresh_token"),
                ("refresh_token", old.refresh_token.as_str()),
                ("scope", MicrosoftFileOAuthConfig::scopes(old.mode)),
            ],
            Some(old),
        )
        .await
    }
    async fn identity(&self, access: &str) -> Result<MicrosoftIdentity, MicrosoftFileError> {
        let value = self
            .json(
                self.http
                    .get(graph_url(&["me"])?)
                    .query(&[("$select", "id,displayName,mail,userPrincipalName")])
                    .bearer_auth(access),
            )
            .await?;
        let id = value["id"]
            .as_str()
            .filter(|value| opaque(value, 1024))
            .ok_or(MicrosoftFileError::Unavailable)?
            .to_owned();
        let label = ["mail", "userPrincipalName", "displayName"]
            .iter()
            .find_map(|key| value[*key].as_str().filter(|value| opaque(value, 320)))
            .map(str::to_owned);
        Ok(MicrosoftIdentity { id, label })
    }
    async fn site_id(&self, access: &str, raw: &str) -> Result<String, MicrosoftFileError> {
        if !valid_site_url(raw) {
            return Err(MicrosoftFileError::Invalid);
        }
        let site = reqwest::Url::parse(raw).map_err(|_| MicrosoftFileError::Invalid)?;
        let url = reqwest::Url::parse(&format!(
            "https://graph.microsoft.com/v1.0/sites/{}:{}",
            site.host_str().ok_or(MicrosoftFileError::Invalid)?,
            site.path()
        ))
        .map_err(|_| MicrosoftFileError::Invalid)?;
        let value = self
            .json(
                self.http
                    .get(url)
                    .query(&[("$select", "id,webUrl")])
                    .bearer_auth(access),
            )
            .await?;
        value["id"]
            .as_str()
            .filter(|value| opaque(value, 1024))
            .map(str::to_owned)
            .ok_or(MicrosoftFileError::Unavailable)
    }
    async fn list(
        &self,
        access: &str,
        mode: MicrosoftFileMode,
        site_id: Option<&str>,
        folder: Option<(&str, &str)>,
        next: Option<&str>,
    ) -> Result<MicrosoftFilePage, MicrosoftFileError> {
        let base = if let Some((drive, item)) = folder {
            graph_url(&["drives", drive, "items", item, "children"])?
        } else {
            match mode {
                MicrosoftFileMode::OneDrive => graph_url(&["me", "drive", "root", "children"])?,
                MicrosoftFileMode::SharePoint => graph_url(&[
                    "sites",
                    site_id.ok_or(MicrosoftFileError::Invalid)?,
                    "drive",
                    "root",
                    "children",
                ])?,
            }
        };
        let url = match next {
            Some(next) => valid_next(&base, next)?,
            None => {
                let mut url = base.clone();
                url.query_pairs_mut().append_pair("$top", "50").append_pair(
                    "$select",
                    "id,name,size,file,folder,webUrl,lastModifiedDateTime,eTag,parentReference",
                );
                url
            }
        };
        let page = self.json(self.http.get(url).bearer_auth(access)).await?;
        let values = page["value"]
            .as_array()
            .ok_or(MicrosoftFileError::Unavailable)?;
        if values.len() > 50 {
            return Err(MicrosoftFileError::Unavailable);
        }
        let files = values
            .iter()
            .map(parse_metadata)
            .collect::<Result<Vec<_>, _>>()?;
        let next_url = match &page["@odata.nextLink"] {
            Value::Null => None,
            Value::String(value) => {
                valid_next(&base, value)?;
                Some(value.clone())
            }
            _ => return Err(MicrosoftFileError::Unavailable),
        };
        if next_url.as_deref().is_some_and(|value| Some(value) == next) {
            return Err(MicrosoftFileError::Unavailable);
        }
        Ok(MicrosoftFilePage { files, next_url })
    }
    async fn metadata(
        &self,
        access: &str,
        drive: &str,
        item: &str,
    ) -> Result<MicrosoftFileMetadata, MicrosoftFileError> {
        let value = self
            .json(
                self.http
                    .get(graph_url(&["drives", drive, "items", item])?)
                    .query(&[(
                        "$select",
                        "id,name,size,file,folder,webUrl,lastModifiedDateTime,eTag,parentReference",
                    )])
                    .bearer_auth(access),
            )
            .await?;
        let metadata = parse_metadata(&value)?;
        if metadata.drive_id != drive || metadata.item_id != item {
            return Err(MicrosoftFileError::Unavailable);
        }
        Ok(metadata)
    }
    async fn content(
        &self,
        access: &str,
        drive: &str,
        item: &str,
        approved_host: Option<&str>,
    ) -> Result<Vec<u8>, MicrosoftFileError> {
        let url = graph_url(&["drives", drive, "items", item, "content"])?;
        let response = self
            .http
            .get(url)
            .bearer_auth(access)
            .send()
            .await
            .map_err(|_| MicrosoftFileError::Unavailable)?;
        if response.status().as_u16() == 401 {
            return Err(MicrosoftFileError::Reauthorization);
        }
        if response.status().as_u16() == 403 {
            return Err(MicrosoftFileError::PermissionDenied);
        }
        if response.status().as_u16() == 404 {
            return Err(MicrosoftFileError::NotFound);
        }
        if !response.status().is_redirection() {
            return Err(MicrosoftFileError::Unavailable);
        }
        let mut url = download_url(
            response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or(MicrosoftFileError::Unavailable)?,
            approved_host,
        )?;
        for _ in 0..=2 {
            let host = url
                .host_str()
                .ok_or(MicrosoftFileError::Unavailable)?
                .to_owned();
            let addresses = tokio::net::lookup_host((host.as_str(), 443))
                .await
                .map_err(|_| MicrosoftFileError::Unavailable)?
                .collect::<Vec<_>>();
            if addresses.is_empty()
                || addresses.len() > 16
                || addresses
                    .iter()
                    .any(|address| !matches!(address.ip(), IpAddr::V4(ip) if public_ipv4(ip)))
            {
                return Err(MicrosoftFileError::Unavailable);
            }
            let client = reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .resolve(&host, addresses[0])
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(15))
                .build()
                .map_err(|_| MicrosoftFileError::Unavailable)?;
            let mut response = client
                .get(url.clone())
                .send()
                .await
                .map_err(|_| MicrosoftFileError::Unavailable)?;
            if response.status().is_redirection() {
                url = download_url(
                    response
                        .headers()
                        .get(reqwest::header::LOCATION)
                        .and_then(|value| value.to_str().ok())
                        .ok_or(MicrosoftFileError::Unavailable)?,
                    approved_host,
                )?;
                continue;
            }
            if !response.status().is_success()
                || response
                    .content_length()
                    .is_some_and(|size| size > 2 * 1024 * 1024)
            {
                return Err(MicrosoftFileError::Unavailable);
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| MicrosoftFileError::Unavailable)?
            {
                if bytes.len().saturating_add(chunk.len()) > 2 * 1024 * 1024 {
                    return Err(MicrosoftFileError::Capacity);
                }
                bytes.extend_from_slice(&chunk);
            }
            return Ok(bytes);
        }
        Err(MicrosoftFileError::Unavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn revoked_grants_require_reauthorization_but_transient_errors_do_not() {
        assert_eq!(
            token_failure(400, br#"{"error":"invalid_grant"}"#),
            MicrosoftFileError::Reauthorization
        );
        assert_eq!(
            token_failure(400, br#"{"error":"interaction_required"}"#),
            MicrosoftFileError::Reauthorization
        );
        assert_eq!(
            token_failure(429, br#"{"error":"invalid_grant"}"#),
            MicrosoftFileError::Unavailable
        );
        assert_eq!(
            token_failure(400, br#"{"error":"temporarily_unavailable"}"#),
            MicrosoftFileError::Unavailable
        );
        assert_eq!(
            token_failure(400, b"not json"),
            MicrosoftFileError::Unavailable
        );
    }
    #[test]
    fn redirects_require_provider_host_and_public_addresses() {
        for host in [
            "https://127.0.0.1/private",
            "https://evil.example/download",
            "http://a.1drv.com/file",
            "https://a.1drv.com:444/file",
        ] {
            assert!(download_url(host, None).is_err());
        }
        assert!(download_url("https://a.1drv.com/file?token=example", None).is_ok());
        assert!(!public_ipv4(Ipv4Addr::new(10, 0, 0, 1)));
        assert!(!public_ipv4(Ipv4Addr::new(100, 64, 0, 1)));
        assert!(public_ipv4(Ipv4Addr::new(8, 8, 8, 8)));
    }
}
