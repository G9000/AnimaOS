//! Bounded native public-client OAuth for MCP resources.
//!
//! Authentication does not authorize tools. Hosts retain ownership of callbacks,
//! vault persistence, cancellation and tool permissions. Refreshed credentials
//! must be durably saved before their access token is used.
mod http;
mod store;

use http::{NetworkPolicy, RestrictedHttp};
use oauth2::{basic::BasicTokenType, TokenResponse};
use rmcp::transport::auth::{
    AuthError, AuthorizationManager, AuthorizationMetadata, AuthorizationMetadataSource,
    AuthorizationRequest, AuthorizationSession, OAuthClientConfig, StoredCredentials,
};
use serde::{Deserialize, Serialize};
use std::{
    fmt,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use store::MemoryCredentials;
use tracing::instrument::WithSubscriber;
use url::Url;
use zeroize::Zeroizing;

const OPERATION_TIMEOUT: Duration = Duration::from_secs(20);
const SESSION_LIFETIME: Duration = Duration::from_secs(600);
const MAX_CREDENTIAL_BYTES: usize = 32 * 1024;
const MAX_TOKEN_BYTES: usize = 4096;

/// Static errors deliberately omit upstream responses and secret values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OAuthError {
    /// The resource, metadata or public-client setup is unsupported.
    InvalidConfiguration,
    /// The token expired or its refresh grant was rejected; sign in again.
    AuthorizationRequired,
    /// The callback could not be validated or the provider rejected sign-in.
    Rejected,
    /// A bounded network or credential operation did not complete.
    Unavailable,
}
impl fmt::Display for OAuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidConfiguration => "OAuth configuration is unsupported",
            Self::AuthorizationRequired => "Sign in to this account again",
            Self::Rejected => "OAuth sign-in was rejected",
            Self::Unavailable => "OAuth service is temporarily unavailable",
        })
    }
}
impl std::error::Error for OAuthError {}
fn static_error(error: AuthError) -> OAuthError {
    match error {
        AuthError::AuthorizationRequired
        | AuthError::TokenExpired
        | AuthError::TokenRefreshRejected(_) => OAuthError::AuthorizationRequired,
        AuthError::AuthorizationFailed(_)
        | AuthError::AuthorizationServerMismatch { .. }
        | AuthError::AuthorizationServerMissingIssuer { .. }
        | AuthError::TokenExchangeFailed(_) => OAuthError::Rejected,
        AuthError::TokenRefreshFailed(_)
        | AuthError::HttpError(_)
        | AuthError::CredentialStoreError(_) => OAuthError::Unavailable,
        _ => OAuthError::InvalidConfiguration,
    }
}
// rmcp emits debug events containing codes/token responses. Poll every SDK
// future under an empty subscriber, including its network callbacks.
async fn quiet<T>(future: impl std::future::Future<Output = T>) -> T {
    future
        .with_subscriber(tracing::subscriber::NoSubscriber::default())
        .await
}

/// A one-use, in-memory attempt. Dropping it discards its PKCE state.
pub struct OAuthSession {
    session: AuthorizationSession,
    resource: String,
    audience: String,
    metadata: AuthorizationMetadata,
    store: MemoryCredentials,
    created: Instant,
}
impl fmt::Debug for OAuthSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("OAuthSession([REDACTED])")
    }
}
impl OAuthSession {
    /// Discover a protected MCP resource and prepare an S256 native-client flow.
    /// Without a pre-registered public client ID, use advertised dynamic
    /// registration. No hosted client metadata URL is synthesized.
    pub async fn begin(
        resource: &str,
        redirect_uri: &str,
        client_id: Option<&str>,
    ) -> Result<Self, OAuthError> {
        let policy = NetworkPolicy::for_resource(resource)?;
        validate_redirect(redirect_uri)?;
        if client_id.is_some_and(|id| !bounded_text(id, 2048)) {
            return Err(OAuthError::InvalidConfiguration);
        }
        let resource = Url::parse(resource)
            .map_err(|_| OAuthError::InvalidConfiguration)?
            .to_string();
        let http = Arc::new(RestrictedHttp::new(policy));
        let flow = async {
            let challenge = http.challenge(&resource).await?;
            let mut manager =
                AuthorizationManager::new_with_oauth_http_client(resource.as_str(), http.clone())
                    .await
                    .map_err(static_error)?;
            let resolution = manager
                .resolve_metadata_from_challenge(challenge.as_deref())
                .await
                .map_err(static_error)?;
            if resolution.source != AuthorizationMetadataSource::ProtectedResourceMetadata {
                return Err(OAuthError::InvalidConfiguration);
            }
            validate_metadata(&resolution.metadata, policy)?;
            let metadata = minimal_metadata(resolution.metadata);
            manager.set_metadata(metadata.clone());
            let store = MemoryCredentials::default();
            manager.set_credential_store(store.clone());
            let client_id = if let Some(client_id) = client_id {
                client_id.to_owned()
            } else {
                let scopes = manager.select_initial_scopes(&[]).await;
                let scope_refs: Vec<_> = scopes.iter().map(String::as_str).collect();
                let config = manager
                    .register_client("AnimaOS", redirect_uri, &scope_refs)
                    .await
                    .map_err(static_error)?;
                // Public native clients cannot depend on an issued client secret.
                if config.client_secret.is_some() || !bounded_text(&config.client_id, 2048) {
                    return Err(OAuthError::InvalidConfiguration);
                }
                config.client_id
            };
            let request = AuthorizationRequest::new(redirect_uri)
                .with_preregistered_client(client_id)
                .with_application_type("native")
                .with_initial_scope_selection();
            let session = AuthorizationSession::new(manager, request)
                .await
                .map_err(|(_, error)| static_error(error))?;
            let url = Url::parse(session.get_authorization_url())
                .map_err(|_| OAuthError::InvalidConfiguration)?;
            policy.validate_url(&url)?;
            let audiences: Vec<_> = url
                .query_pairs()
                .filter(|(key, _)| key == "resource")
                .map(|(_, value)| value.into_owned())
                .collect();
            let [audience] = audiences.as_slice() else {
                return Err(OAuthError::InvalidConfiguration);
            };
            validate_audience(&resource, audience)?;
            Ok(Self {
                session,
                resource,
                audience: audience.clone(),
                metadata,
                store,
                created: Instant::now(),
            })
        };
        tokio::time::timeout(OPERATION_TIMEOUT, quiet(flow))
            .await
            .map_err(|_| OAuthError::Unavailable)?
    }
    /// Provider URL contains OAuth state and must never be logged.
    pub fn authorization_url(&self) -> &str {
        self.session.get_authorization_url()
    }
    /// Validate the decoded RFC 9207 issuer on every callback, including denial
    /// responses, before consuming the attempt or trusting callback parameters.
    pub fn validate_callback_issuer(&self, issuer: Option<&str>) -> Result<(), OAuthError> {
        if issuer.is_none_or(|value| !bounded_text(value, 4096))
            || issuer != self.metadata.issuer.as_deref()
        {
            return Err(OAuthError::Rejected);
        }
        Ok(())
    }
    /// Consume this attempt, validate state/issuer and exchange its code once.
    pub async fn complete(
        self,
        code: &str,
        state: &str,
        issuer: Option<&str>,
    ) -> Result<OAuthCredentials, OAuthError> {
        self.validate_callback_issuer(issuer)?;
        if self.created.elapsed() >= SESSION_LIFETIME
            || !bounded_text(code, 4096)
            || !bounded_text(state, 512)
            || issuer.is_some_and(|v| !bounded_text(v, 4096))
        {
            return Err(OAuthError::Rejected);
        }
        let operation = async {
            self.session
                .handle_callback_with_issuer(code, state, issuer)
                .await
                .map_err(static_error)?;
            let stored = self.store.snapshot().await.ok_or(OAuthError::Unavailable)?;
            let credentials = OAuthCredentials {
                version: 1,
                resource: self.resource,
                audience: self.audience,
                redirect_uri: self.session.redirect_uri,
                metadata: self.metadata,
                stored,
            };
            credentials.validate()?;
            Ok(credentials)
        };
        tokio::time::timeout(OPERATION_TIMEOUT, quiet(operation))
            .await
            .map_err(|_| OAuthError::Unavailable)?
    }
}

/// Endpoint-bound credential material. Serialize only into a credential vault.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OAuthCredentials {
    version: u8,
    resource: String,
    audience: String,
    redirect_uri: String,
    metadata: AuthorizationMetadata,
    stored: StoredCredentials,
}
impl fmt::Debug for OAuthCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("OAuthCredentials([REDACTED])")
    }
}
impl OAuthCredentials {
    /// Exact normalized MCP endpoint that owns these credentials.
    pub fn resource(&self) -> &str {
        &self.resource
    }
    /// Issuer discovered and checked during sign-in.
    pub fn issuer(&self) -> &str {
        self.metadata.issuer.as_deref().unwrap_or_default()
    }
    /// Granted account scopes; these are not tool permissions.
    pub fn scopes(&self) -> &[String] {
        &self.stored.granted_scopes
    }
    /// Serialize for vault persistence, clearing the serialized buffer on drop.
    pub fn to_json(&self) -> Result<Zeroizing<String>, OAuthError> {
        self.validate()?;
        let json = Zeroizing::new(
            serde_json::to_string(self).map_err(|_| OAuthError::InvalidConfiguration)?,
        );
        if json.len() > MAX_CREDENTIAL_BYTES {
            return Err(OAuthError::InvalidConfiguration);
        }
        Ok(json)
    }
    /// Restore a vault payload and validate all bindings before using it.
    pub fn from_json(value: &str) -> Result<Self, OAuthError> {
        if value.len() > MAX_CREDENTIAL_BYTES {
            return Err(OAuthError::InvalidConfiguration);
        }
        let credentials: Self =
            serde_json::from_str(value).map_err(|_| OAuthError::InvalidConfiguration)?;
        credentials.validate()?;
        Ok(credentials)
    }
    fn validate(&self) -> Result<(), OAuthError> {
        let policy = NetworkPolicy::for_resource(&self.resource)?;
        validate_redirect(&self.redirect_uri)?;
        validate_audience(&self.resource, &self.audience)?;
        validate_metadata(&self.metadata, policy)?;
        // Every version-1 grant is created with a receipt time. The SDK treats a
        // missing time as an old, non-expiring token, which must not turn a
        // truncated vault payload into timeless credentials.
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| OAuthError::Unavailable)?
            .as_secs();
        if !self
            .stored
            .token_received_at
            .is_some_and(|received| received > 0 && received <= now.saturating_add(60))
        {
            return Err(OAuthError::InvalidConfiguration);
        }
        if self.version != 1
            || self.stored.issuer.as_deref() != self.metadata.issuer.as_deref()
            || !bounded_text(&self.stored.client_id, 2048)
            || self.stored.granted_scopes.len() > 32
            || self
                .stored
                .granted_scopes
                .iter()
                .any(|scope| !bounded_text(scope, 256) || scope.contains(' '))
        {
            return Err(OAuthError::InvalidConfiguration);
        }
        let token = self
            .stored
            .token_response
            .as_ref()
            .ok_or(OAuthError::AuthorizationRequired)?;
        if token.token_type() != &BasicTokenType::Bearer
            || !valid_token(token.access_token().secret())
            || token
                .refresh_token()
                .is_some_and(|token| !valid_token(token.secret()))
        {
            return Err(OAuthError::InvalidConfiguration);
        }
        Ok(())
    }
    /// Get a valid token, refreshing once when required. The host must save
    /// `self` after success before dispatch. Never sends/retries an MCP operation.
    pub async fn access_token(&mut self) -> Result<Zeroizing<String>, OAuthError> {
        self.validate()?;
        let operation = async {
            let http = Arc::new(RestrictedHttp::with_refresh_audience(
                NetworkPolicy::for_resource(&self.resource)?,
                self.audience.clone(),
            ));
            // Preserve the original resource audience, which may be a parent path.
            let mut manager =
                AuthorizationManager::new_with_oauth_http_client(self.audience.as_str(), http)
                    .await
                    .map_err(static_error)?;
            manager.set_metadata(self.metadata.clone());
            let store = MemoryCredentials::with_credentials(self.stored.clone());
            manager.set_credential_store(store.clone());
            manager
                .configure_client(OAuthClientConfig::new(
                    &self.stored.client_id,
                    &self.redirect_uri,
                ))
                .map_err(static_error)?;
            let token = manager.get_access_token().await.map_err(static_error)?;
            self.stored = store.snapshot().await.ok_or(OAuthError::Unavailable)?;
            self.validate()?;
            Ok(Zeroizing::new(token))
        };
        tokio::time::timeout(OPERATION_TIMEOUT, quiet(operation))
            .await
            .map_err(|_| OAuthError::Unavailable)?
    }
    /// Best-effort provider revocation when advertised. Hosts must erase local
    /// credentials even if the provider is unavailable or has no revoke API.
    pub async fn revoke(&self) -> Result<(), OAuthError> {
        self.validate()?;
        let Some(endpoint) = self
            .metadata
            .additional_fields
            .get("revocation_endpoint")
            .and_then(serde_json::Value::as_str)
        else {
            return Ok(());
        };
        let token = self
            .stored
            .token_response
            .as_ref()
            .ok_or(OAuthError::AuthorizationRequired)?;
        let (token, hint) = match token.refresh_token() {
            Some(refresh) => (refresh.secret(), "refresh_token"),
            None => (token.access_token().secret(), "access_token"),
        };
        let body = Zeroizing::new(
            url::form_urlencoded::Serializer::new(String::new())
                .append_pair("token", token)
                .append_pair("token_type_hint", hint)
                .append_pair("client_id", &self.stored.client_id)
                .finish(),
        );
        let http = RestrictedHttp::new(NetworkPolicy::for_resource(&self.resource)?);
        let request = ::http::Request::builder()
            .method("POST")
            .uri(endpoint)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(body.as_bytes().to_vec())
            .map_err(|_| OAuthError::InvalidConfiguration)?;
        let response = tokio::time::timeout(OPERATION_TIMEOUT, http.send(request))
            .await
            .map_err(|_| OAuthError::Unavailable)??;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(OAuthError::Unavailable)
        }
    }
}
fn bounded_text(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}
fn valid_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_TOKEN_BYTES
        && value.bytes().all(|b| b.is_ascii_graphic())
}
fn validate_redirect(value: &str) -> Result<(), OAuthError> {
    let url = Url::parse(value).map_err(|_| OAuthError::InvalidConfiguration)?;
    if url.scheme() != "http"
        || !http::literal_loopback(&url)
        || url.port().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() == "/"
        || value.len() > 2048
    {
        return Err(OAuthError::InvalidConfiguration);
    }
    Ok(())
}
fn validate_audience(resource: &str, audience: &str) -> Result<(), OAuthError> {
    let endpoint = Url::parse(resource).map_err(|_| OAuthError::InvalidConfiguration)?;
    let audience = Url::parse(audience).map_err(|_| OAuthError::InvalidConfiguration)?;
    NetworkPolicy::for_resource(resource)?.validate_url(&audience)?;
    if endpoint.origin() != audience.origin()
        || !(endpoint.path() == audience.path()
            || endpoint
                .path()
                .starts_with(&format!("{}/", audience.path().trim_end_matches('/'))))
    {
        return Err(OAuthError::InvalidConfiguration);
    }
    Ok(())
}
fn validate_metadata(
    metadata: &AuthorizationMetadata,
    policy: NetworkPolicy,
) -> Result<(), OAuthError> {
    // A shared registered loopback callback cannot identify the issuer by its
    // path. Require RFC 9207 before consent, preventing a malicious authorization
    // server from routing the browser to another server and stealing its code
    // plus PKCE verifier through the malicious token endpoint.
    if metadata
        .additional_fields
        .get("authorization_response_iss_parameter_supported")
        .and_then(serde_json::Value::as_bool)
        != Some(true)
    {
        return Err(OAuthError::InvalidConfiguration);
    }
    if !metadata
        .code_challenge_methods_supported
        .as_ref()
        .is_some_and(|methods| methods.iter().any(|method| method == "S256"))
    {
        return Err(OAuthError::InvalidConfiguration);
    }
    for value in [
        Some(metadata.authorization_endpoint.as_str()),
        Some(metadata.token_endpoint.as_str()),
        metadata.issuer.as_deref(),
        metadata.registration_endpoint.as_deref(),
        metadata
            .additional_fields
            .get("revocation_endpoint")
            .and_then(serde_json::Value::as_str),
    ]
    .into_iter()
    .flatten()
    {
        policy.validate_url(&Url::parse(value).map_err(|_| OAuthError::InvalidConfiguration)?)?;
    }
    if metadata.issuer.is_none() {
        return Err(OAuthError::InvalidConfiguration);
    }
    if let Some(methods) = metadata
        .additional_fields
        .get("token_endpoint_auth_methods_supported")
    {
        if !methods
            .as_array()
            .is_some_and(|methods| methods.iter().any(|method| method.as_str() == Some("none")))
        {
            return Err(OAuthError::InvalidConfiguration);
        }
    }
    Ok(())
}
fn minimal_metadata(mut metadata: AuthorizationMetadata) -> AuthorizationMetadata {
    metadata.jwks_uri = None;
    metadata.additional_fields.retain(|key, _| {
        matches!(
            key.as_str(),
            "revocation_endpoint"
                | "authorization_response_iss_parameter_supported"
                | "token_endpoint_auth_methods_supported"
        )
    });
    metadata
}
