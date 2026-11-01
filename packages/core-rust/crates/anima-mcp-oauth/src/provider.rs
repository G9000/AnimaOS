//! Reviewed compatibility for a single known authorization-server tuple.
//!
//! RFC 9700 §4.4.2.2 permits issuer-specific redirects but warns that dynamic
//! registration can bypass that defense. Thus an unadvertised issuer response
//! is accepted only for this exact independently reviewed provider tuple. This
//! is not an exception for arbitrary discovered authorization servers.
use super::OAuthError;
use rmcp::transport::auth::AuthorizationMetadata;
use serde_json::Value;
use sha2::{Digest, Sha256};
use url::Url;

pub(super) const RESOURCE: &str = "https://ai.metricool.com/mcp";
const ISSUER: &str = "https://ai.metricool.com";
const RESOURCE_METADATA: &str = "https://ai.metricool.com/.well-known/oauth-protected-resource";
const AUTHORIZATION_METADATA: &str =
    "https://ai.metricool.com/.well-known/oauth-authorization-server";
const AUTHORIZE: &str = "https://app.metricool.com/oauth/authorize";
const TOKEN: &str = "https://app.metricool.com/oauth/token";
const REGISTER: &str = "https://app.metricool.com/oauth/register";
const CALLBACK: &str = "/oauth/mcp/callback";

pub(super) fn issuer_advertised(metadata: &AuthorizationMetadata) -> Result<bool, OAuthError> {
    match metadata
        .additional_fields
        .get("authorization_response_iss_parameter_supported")
    {
        None | Some(Value::Bool(false)) => Ok(false),
        Some(Value::Bool(true)) => Ok(true),
        _ => Err(OAuthError::InvalidConfiguration),
    }
}

pub(super) fn known_metadata(resource: &str, metadata: &AuthorizationMetadata) -> bool {
    resource == RESOURCE
        && metadata.issuer.as_deref() == Some(ISSUER)
        && metadata.authorization_endpoint == AUTHORIZE
        && metadata.token_endpoint == TOKEN
        && metadata.registration_endpoint.as_deref() == Some(REGISTER)
        // Never send a credential to a newly discovered, unreviewed endpoint.
        && !metadata.additional_fields.contains_key("revocation_endpoint")
}

fn callback_path() -> String {
    let digest = Sha256::digest(ISSUER.as_bytes());
    let suffix = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("{CALLBACK}/issuer-{suffix}")
}

pub(super) fn prepare_redirect(
    resource: &str,
    metadata: &AuthorizationMetadata,
    base: &str,
) -> Result<String, OAuthError> {
    if issuer_advertised(metadata)? {
        return Ok(base.to_owned());
    }
    if !known_metadata(resource, metadata) {
        return Err(OAuthError::InvalidConfiguration);
    }
    let mut redirect = Url::parse(base).map_err(|_| OAuthError::InvalidConfiguration)?;
    if redirect.path() != CALLBACK {
        return Err(OAuthError::InvalidConfiguration);
    }
    redirect.set_path(&callback_path());
    Ok(redirect.to_string())
}

pub(super) fn validate_binding(
    resource: &str,
    audience: &str,
    metadata: &AuthorizationMetadata,
    redirect: &str,
) -> Result<(), OAuthError> {
    if resource == RESOURCE && (audience != RESOURCE || !known_metadata(resource, metadata)) {
        return Err(OAuthError::InvalidConfiguration);
    }
    if issuer_advertised(metadata)? {
        return Ok(());
    }
    if !known_metadata(resource, metadata)
        || Url::parse(redirect)
            .map_err(|_| OAuthError::InvalidConfiguration)?
            .path()
            != callback_path()
    {
        return Err(OAuthError::InvalidConfiguration);
    }
    Ok(())
}

pub(super) fn validate_callback(
    resource: &str,
    metadata: &AuthorizationMetadata,
    redirect: &str,
    issuer: Option<&str>,
) -> Result<(), OAuthError> {
    if let Some(issuer) = issuer {
        if !super::bounded_text(issuer, 4096) || metadata.issuer.as_deref() != Some(issuer) {
            return Err(OAuthError::Rejected);
        }
        return Ok(());
    }
    if issuer_advertised(metadata).map_err(|_| OAuthError::Rejected)? {
        return Err(OAuthError::Rejected);
    }
    validate_binding(resource, resource, metadata, redirect).map_err(|_| OAuthError::Rejected)
}

pub(super) fn validate_http_request(method: &::http::Method, url: &Url) -> Result<(), OAuthError> {
    let allowed = match *method {
        ::http::Method::GET => {
            [RESOURCE, RESOURCE_METADATA, AUTHORIZATION_METADATA].contains(&url.as_str())
        }
        ::http::Method::POST => [REGISTER, TOKEN].contains(&url.as_str()),
        _ => false,
    };
    if allowed {
        Ok(())
    } else {
        Err(OAuthError::InvalidConfiguration)
    }
}

pub(super) fn validate_resource_document(url: &Url, bytes: &[u8]) -> Result<(), OAuthError> {
    if url.as_str() != RESOURCE_METADATA {
        return Ok(());
    }
    let value: Value =
        serde_json::from_slice(bytes).map_err(|_| OAuthError::InvalidConfiguration)?;
    if value.get("resource").and_then(Value::as_str) != Some(RESOURCE)
        || value.get("authorization_servers") != Some(&serde_json::json!([ISSUER]))
        || value.get("authorization_server").is_some()
    {
        return Err(OAuthError::InvalidConfiguration);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn metadata() -> AuthorizationMetadata {
        serde_json::from_value(serde_json::json!({"issuer":ISSUER,"authorization_endpoint":AUTHORIZE,"token_endpoint":TOKEN,"registration_endpoint":REGISTER,"scopes_supported":["mcp:read","mcp:write"],"response_types_supported":["code"],"response_modes_supported":["query","fragment"],"grant_types_supported":["authorization_code","refresh_token"],"code_challenge_methods_supported":["S256","plain"],"token_endpoint_auth_methods_supported":["client_secret_basic","client_secret_post","none"]})).unwrap()
    }
    #[test]
    fn compatibility_is_exact_and_never_applies_to_metadata_steering() {
        let valid = metadata();
        assert!(known_metadata(RESOURCE, &valid));
        for resource in [
            "https://AI.metricool.com/mcp",
            "https://ai.metricool.com:443/mcp",
            "https://ai.metricool.com/mcp/",
            "https://attacker.example/mcp",
        ] {
            assert!(!known_metadata(resource, &valid));
        }
        for key in [
            "issuer",
            "authorization_endpoint",
            "token_endpoint",
            "registration_endpoint",
        ] {
            let mut altered = serde_json::to_value(&valid).unwrap();
            altered[key] = Value::String("https://attacker.example/steal".into());
            assert!(
                !known_metadata(RESOURCE, &serde_json::from_value(altered).unwrap()),
                "{key}"
            );
        }
        let mut altered = valid;
        altered.additional_fields.insert(
            "revocation_endpoint".into(),
            Value::String("https://attacker.example/steal".into()),
        );
        assert!(!known_metadata(RESOURCE, &altered));
    }
    #[test]
    fn profile_uses_an_issuer_bound_callback_and_preserves_response_issuer_checks() {
        let mut metadata = metadata();
        let redirect = prepare_redirect(
            RESOURCE,
            &metadata,
            "http://127.0.0.1:32789/oauth/mcp/callback",
        )
        .unwrap();
        assert!(redirect.contains("/oauth/mcp/callback/issuer-"));
        assert_eq!(
            validate_callback(RESOURCE, &metadata, &redirect, None),
            Ok(())
        );
        assert_eq!(
            validate_callback(RESOURCE, &metadata, &redirect, Some(ISSUER)),
            Ok(())
        );
        for issuer in [
            "https://AI.metricool.com",
            "https://ai.metricool.com/",
            "https://ai.metricool.com:443",
            "https://attacker.example",
        ] {
            assert!(validate_callback(RESOURCE, &metadata, &redirect, Some(issuer)).is_err());
        }
        assert!(validate_callback(
            RESOURCE,
            &metadata,
            "http://127.0.0.1:32789/oauth/mcp/callback",
            None
        )
        .is_err());
        assert!(validate_binding(RESOURCE, ISSUER, &metadata, &redirect).is_err());
        metadata.additional_fields.insert(
            "authorization_response_iss_parameter_supported".into(),
            Value::Bool(true),
        );
        assert!(validate_callback(RESOURCE, &metadata, &redirect, None).is_err());
        metadata.additional_fields.insert(
            "authorization_response_iss_parameter_supported".into(),
            Value::Bool(false),
        );
        assert!(validate_callback(RESOURCE, &metadata, &redirect, None).is_ok());
        assert!(validate_callback(
            RESOURCE,
            &metadata,
            &redirect,
            Some("https://attacker.example")
        )
        .is_err());
    }
    #[test]
    fn protected_resource_identity_and_authorization_server_list_are_unambiguous() {
        let url = Url::parse(RESOURCE_METADATA).unwrap();
        let valid = serde_json::json!({"resource":RESOURCE,"authorization_servers":[ISSUER]});
        assert!(validate_resource_document(&url, &serde_json::to_vec(&valid).unwrap()).is_ok());
        for servers in [
            serde_json::json!([]),
            serde_json::json!([ISSUER, ISSUER]),
            serde_json::json!([ISSUER, "https://attacker.example"]),
            serde_json::json!(["https://AI.metricool.com"]),
        ] {
            let mut altered = valid.clone();
            altered["authorization_servers"] = servers;
            assert!(
                validate_resource_document(&url, &serde_json::to_vec(&altered).unwrap()).is_err()
            );
        }
        assert!(validate_http_request(
            &::http::Method::POST,
            &Url::parse("https://app.metricool.com/oauth/token?redirect=elsewhere").unwrap()
        )
        .is_err());
        for (method, endpoint) in [
            (
                ::http::Method::GET,
                "https://attacker.example/.well-known/oauth-protected-resource",
            ),
            (::http::Method::POST, AUTHORIZE),
            (::http::Method::GET, TOKEN),
            (
                ::http::Method::POST,
                "https://app.metricool.com/oauth/revoke",
            ),
        ] {
            assert!(validate_http_request(&method, &Url::parse(endpoint).unwrap()).is_err());
        }
    }
    #[test]
    fn restored_credentials_recompute_pins_callback_and_exact_audience() {
        let metadata = metadata();
        let redirect = prepare_redirect(
            RESOURCE,
            &metadata,
            "http://127.0.0.1:32789/oauth/mcp/callback",
        )
        .unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let payload = serde_json::json!({
            "version":1,"resource":RESOURCE,"audience":RESOURCE,"redirect_uri":redirect,"metadata":metadata,
            "stored":{"client_id":"synthetic-client","issuer":ISSUER,"granted_scopes":["mcp:read"],"token_received_at":now,
              "token_response":{"access_token":"synthetic-pinned-access","refresh_token":"synthetic-pinned-refresh","token_type":"Bearer","expires_in":3600}}
        });
        let encoded = serde_json::to_string(&payload).unwrap();
        let restored = crate::OAuthCredentials::from_json(&encoded).unwrap();
        assert!(restored.to_json().is_ok());
        for (field, value) in [
            ("resource", "https://AI.metricool.com/mcp"),
            ("resource", "https://attacker.example/mcp"),
            ("audience", ISSUER),
            ("redirect_uri", "http://127.0.0.1:32789/oauth/mcp/callback"),
        ] {
            let mut altered = payload.clone();
            altered[field] = Value::String(value.into());
            assert!(
                crate::OAuthCredentials::from_json(&serde_json::to_string(&altered).unwrap())
                    .is_err(),
                "{field}"
            );
        }
        let mut unknown_bypass = payload.clone();
        unknown_bypass["allowMissingIssuer"] = Value::Bool(true);
        assert!(crate::OAuthCredentials::from_json(
            &serde_json::to_string(&unknown_bypass).unwrap()
        )
        .is_err());
        for field in [
            "issuer",
            "authorization_endpoint",
            "token_endpoint",
            "registration_endpoint",
            "revocation_endpoint",
        ] {
            let mut altered = payload.clone();
            altered["metadata"][field] = Value::String("https://attacker.example/steal".into());
            assert!(
                crate::OAuthCredentials::from_json(&serde_json::to_string(&altered).unwrap())
                    .is_err(),
                "{field}"
            );
        }
    }
}
