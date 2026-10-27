use anima_mcp_oauth::{OAuthCredentials, OAuthError, OAuthSession};
use axum::{
    body::Bytes,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use url::Url;

#[derive(Clone)]
struct FixtureState {
    origin: String,
    pkce: bool,
    expires: u64,
    calls: Arc<Mutex<Vec<(String, HashMap<String, String>)>>>,
    config: Arc<Mutex<HashMap<String, Value>>>,
}
struct Fixture {
    origin: String,
    calls: Arc<Mutex<Vec<(String, HashMap<String, String>)>>>,
    config: Arc<Mutex<HashMap<String, Value>>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Fixture {
    async fn start(pkce: bool, expires: u64) -> Self {
        Self::start_host(pkce, expires, "127.0.0.1").await
    }
    async fn start_host(pkce: bool, expires: u64, host: &str) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{host}:{}", listener.local_addr().unwrap().port());
        let calls = Arc::new(Mutex::new(Vec::new()));
        let config = Arc::new(Mutex::new(HashMap::new()));
        let state = FixtureState {
            origin: origin.clone(),
            pkce,
            expires,
            calls: calls.clone(),
            config: config.clone(),
        };
        let app = Router::new()
            .route(
                "/mcp",
                get(|| async {
                    (
                        StatusCode::UNAUTHORIZED,
                        [("www-authenticate", "Bearer resource_metadata=\"/resource\"")],
                    )
                }),
            )
            .route("/resource", get(resource))
            .route("/.well-known/oauth-protected-resource/mcp", get(resource))
            .route("/.well-known/oauth-authorization-server", get(metadata))
            .route("/register", post(register))
            .route("/token", post(token))
            .route("/revoke", post(revoke))
            .with_state(state);
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self {
            origin,
            calls,
            config,
            task,
        }
    }
    fn resource(&self) -> String {
        format!("{}/mcp", self.origin)
    }
    fn configure(&self, key: &str, value: Value) {
        self.config.lock().unwrap().insert(key.into(), value);
    }
}

#[tokio::test]
async fn localhost_native_resource_uses_fixed_loopback_addresses() {
    let fixture = Fixture::start_host(true, 3600, "localhost").await;
    let session = OAuthSession::begin(
        &fixture.resource(),
        "http://127.0.0.1:32789/callback",
        Some("native-client"),
    )
    .await
    .unwrap();
    let state = params(&session)["state"].clone();
    let mut credentials = session
        .complete("synthetic-code", &state, Some(&fixture.origin))
        .await
        .unwrap();
    assert_eq!(
        credentials.access_token().await.unwrap().as_str(),
        "first-access"
    );
}
async fn resource(State(state): State<FixtureState>) -> Json<Value> {
    let config = state.config.lock().unwrap();
    let audience = config
        .get("resource")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("{}/mcp", state.origin));
    Json(
        json!({"resource":audience,"authorization_servers":[state.origin],"scopes_supported":["read"]}),
    )
}
async fn metadata(State(state): State<FixtureState>) -> Json<Value> {
    let mut value = json!({"issuer":state.origin,"authorization_endpoint":format!("{}/authorize",state.origin),"token_endpoint":format!("{}/token",state.origin),"registration_endpoint":format!("{}/register",state.origin),"revocation_endpoint":format!("{}/revoke",state.origin),"response_types_supported":["code"],"token_endpoint_auth_methods_supported":["none"],"authorization_response_iss_parameter_supported":true});
    if state.pkce {
        value["code_challenge_methods_supported"] = json!(["S256"]);
    }
    for (key, override_value) in state.config.lock().unwrap().iter().filter(|(key, _)| {
        !matches!(
            key.as_str(),
            "client_secret"
                | "resource"
                | "refresh_error"
                | "omit_refresh"
                | "token_type"
                | "token_redirect"
                | "vendor_fields"
        )
    }) {
        if override_value.is_null() {
            value.as_object_mut().unwrap().remove(key);
        } else {
            value[key] = override_value.clone();
        }
    }
    Json(value)
}
async fn register(State(state): State<FixtureState>, Json(body): Json<Value>) -> Json<Value> {
    assert_eq!(body["token_endpoint_auth_method"], "none");
    assert_eq!(body["application_type"], "native");
    state
        .calls
        .lock()
        .unwrap()
        .push(("register".into(), HashMap::new()));
    let mut value = json!({"client_id":"native-client","redirect_uris":body["redirect_uris"]});
    if let Some(secret) = state.config.lock().unwrap().get("client_secret") {
        value["client_secret"] = secret.clone();
    }
    Json(value)
}
async fn token(State(state): State<FixtureState>, bytes: Bytes) -> Response {
    let form: HashMap<String, String> = serde_urlencoded::from_bytes(&bytes).unwrap();
    let config = state.config.lock().unwrap();
    let audience = config
        .get("resource")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("{}/mcp", state.origin));
    assert_eq!(form.get("resource"), Some(&audience));
    assert_eq!(
        form.get("client_id").map(String::as_str),
        Some("native-client")
    );
    let refresh = form.get("grant_type").map(String::as_str) == Some("refresh_token");
    state.calls.lock().unwrap().push(("token".into(), form));
    if config.get("token_redirect") == Some(&Value::Bool(true)) {
        return (
            StatusCode::TEMPORARY_REDIRECT,
            [("location", format!("{}/revoke", state.origin))],
        )
            .into_response();
    }
    if refresh && config.get("refresh_error") == Some(&Value::Bool(true)) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"invalid_grant","error_description":"sensitive-upstream-text"})),
        )
            .into_response();
    }
    let mut value = if refresh {
        json!({"access_token":"rotated-access","token_type":"Bearer","refresh_token":"rotated-refresh","expires_in":3600})
    } else {
        json!({"access_token":"first-access","token_type":"Bearer","refresh_token":"first-refresh","expires_in":state.expires})
    };
    if refresh && config.get("omit_refresh") == Some(&Value::Bool(true)) {
        value.as_object_mut().unwrap().remove("refresh_token");
    }
    if let Some(token_type) = config.get("token_type") {
        value["token_type"] = token_type.clone();
    }
    if config.get("vendor_fields") == Some(&Value::Bool(true)) {
        value["id_token"] = json!("unneeded-id-token");
        value["unneeded_vendor"] = json!("vendor-field-secret");
    }
    Json(value).into_response()
}
async fn revoke(State(state): State<FixtureState>, bytes: Bytes) -> StatusCode {
    let form = serde_urlencoded::from_bytes(&bytes).unwrap();
    state.calls.lock().unwrap().push(("revoke".into(), form));
    StatusCode::OK
}
fn params(session: &OAuthSession) -> HashMap<String, String> {
    Url::parse(session.authorization_url())
        .unwrap()
        .query_pairs()
        .into_owned()
        .collect()
}

#[tokio::test]
async fn public_native_sign_in_uses_pkce_audience_and_vault_roundtrip() {
    let fixture = Fixture::start(true, 3600).await;
    let session = OAuthSession::begin(&fixture.resource(), "http://127.0.0.1:32789/callback", None)
        .await
        .unwrap();
    let params = params(&session);
    assert_eq!(params["code_challenge_method"], "S256");
    assert_eq!(params["resource"], fixture.resource());
    // oauth2's random state contains 128 bits, encoded as 22 base64url bytes.
    assert!(params["state"].len() >= 22);
    let credentials = session
        .complete("synthetic-code", &params["state"], Some(&fixture.origin))
        .await
        .unwrap();
    assert_eq!(credentials.resource(), fixture.resource());
    assert_eq!(credentials.issuer(), fixture.origin);
    assert_eq!(credentials.scopes(), &["read"]);
    assert!(!format!("{credentials:?}").contains("first-access"));
    let json = credentials.to_json().unwrap();
    let mut restored = OAuthCredentials::from_json(&json).unwrap();
    assert_eq!(
        restored.access_token().await.unwrap().as_str(),
        "first-access"
    );
    let calls = fixture.calls.lock().unwrap();
    let exchange = &calls.iter().find(|(name, _)| name == "token").unwrap().1;
    let verifier = &exchange["code_verifier"];
    assert_eq!(
        URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())),
        params["code_challenge"]
    );
    assert_eq!(calls.iter().filter(|(name, _)| name == "token").count(), 1);
}

#[tokio::test]
async fn rejects_missing_pkce_before_registering_or_exchanging() {
    let fixture = Fixture::start(false, 3600).await;
    assert_eq!(
        OAuthSession::begin(&fixture.resource(), "http://127.0.0.1:32789/callback", None)
            .await
            .unwrap_err(),
        OAuthError::InvalidConfiguration
    );
    assert!(fixture.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn wrong_state_or_issuer_never_exchanges_a_code() {
    let fixture = Fixture::start(true, 3600).await;
    for wrong_state in [true, false] {
        let session = OAuthSession::begin(
            &fixture.resource(),
            "http://127.0.0.1:32789/callback",
            Some("native-client"),
        )
        .await
        .unwrap();
        let params = params(&session);
        assert!(session
            .complete(
                "synthetic-code",
                if wrong_state {
                    "forged-state"
                } else {
                    &params["state"]
                },
                Some(if wrong_state {
                    &fixture.origin
                } else {
                    "https://wrong.invalid"
                })
            )
            .await
            .is_err());
    }
    assert!(!fixture
        .calls
        .lock()
        .unwrap()
        .iter()
        .any(|(name, _)| name == "token"));
}

#[tokio::test]
async fn legacy_provider_without_issuer_identification_is_rejected_before_consent() {
    for advertisement in [Value::Null, json!(false), json!("true")] {
        let fixture = Fixture::start(true, 3600).await;
        fixture.configure(
            "authorization_response_iss_parameter_supported",
            advertisement,
        );
        assert_eq!(
            OAuthSession::begin(&fixture.resource(), "http://127.0.0.1:32789/callback", None)
                .await
                .unwrap_err(),
            OAuthError::InvalidConfiguration
        );
        assert!(
            fixture.calls.lock().unwrap().is_empty(),
            "unsupported provider was registered before rejection"
        );
    }
}

#[tokio::test]
async fn callback_missing_issuer_never_sends_code_or_pkce_verifier_to_token_endpoint() {
    let fixture = Fixture::start(true, 3600).await;
    let session = OAuthSession::begin(
        &fixture.resource(),
        "http://127.0.0.1:32789/callback",
        Some("native-client"),
    )
    .await
    .unwrap();
    let state = params(&session)["state"].clone();
    assert_eq!(
        session.validate_callback_issuer(None),
        Err(OAuthError::Rejected)
    );
    assert_eq!(
        session.validate_callback_issuer(Some("https://wrong.invalid")),
        Err(OAuthError::Rejected)
    );
    assert_eq!(
        session.validate_callback_issuer(Some(&fixture.origin)),
        Ok(())
    );
    assert_eq!(
        session
            .complete("synthetic-code", &state, None)
            .await
            .unwrap_err(),
        OAuthError::Rejected
    );
    assert!(!fixture
        .calls
        .lock()
        .unwrap()
        .iter()
        .any(|(name, _)| name == "token"));
}

#[tokio::test]
async fn refresh_rotates_credentials_and_revoke_prefers_refresh_token() {
    let fixture = Fixture::start(true, 0).await;
    let session = OAuthSession::begin(
        &fixture.resource(),
        "http://127.0.0.1:32789/callback",
        Some("native-client"),
    )
    .await
    .unwrap();
    let state = params(&session)["state"].clone();
    let mut credentials = session
        .complete("synthetic-code", &state, Some(&fixture.origin))
        .await
        .unwrap();
    assert_eq!(
        credentials.access_token().await.unwrap().as_str(),
        "rotated-access"
    );
    assert!(credentials.to_json().unwrap().contains("rotated-refresh"));
    credentials.revoke().await.unwrap();
    let calls = fixture.calls.lock().unwrap();
    let refresh = &calls
        .iter()
        .filter(|(name, _)| name == "token")
        .nth(1)
        .unwrap()
        .1;
    assert_eq!(refresh["refresh_token"], "first-refresh");
    let revoke = &calls.iter().find(|(name, _)| name == "revoke").unwrap().1;
    assert_eq!(revoke["token"], "rotated-refresh");
}

#[tokio::test]
async fn unsafe_resource_and_redirect_are_rejected_without_network() {
    for resource in [
        "http://example.com/mcp",
        "https://user:password@example.com/mcp",
        "https://example.com/mcp#secret",
        "http://192.168.1.1/mcp",
    ] {
        assert_eq!(
            OAuthSession::begin(resource, "http://127.0.0.1:32789/callback", None)
                .await
                .unwrap_err(),
            OAuthError::InvalidConfiguration
        );
    }
    assert_eq!(
        OAuthSession::begin(
            "http://127.0.0.1:9/mcp",
            "https://external.invalid/callback",
            None
        )
        .await
        .unwrap_err(),
        OAuthError::InvalidConfiguration
    );
}

#[tokio::test]
async fn rejects_wrong_issuer_resource_and_confidential_registration() {
    for (key, value) in [
        ("issuer", json!("https://wrong.invalid")),
        ("resource", json!("https://wrong.invalid/mcp")),
        ("client_secret", json!("issued-confidential-secret")),
    ] {
        let fixture = Fixture::start(true, 3600).await;
        fixture.configure(key, value);
        assert!(
            OAuthSession::begin(&fixture.resource(), "http://127.0.0.1:32789/callback", None)
                .await
                .is_err()
        );
        assert!(!fixture
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|(name, _)| name == "token"));
    }
}

#[tokio::test]
async fn parent_audience_is_preserved_on_restored_refresh_and_vendor_fields_are_dropped() {
    let fixture = Fixture::start(true, 0).await;
    fixture.configure("resource", json!(fixture.origin));
    fixture.configure("vendor_fields", json!(true));
    let session = OAuthSession::begin(
        &fixture.resource(),
        "http://127.0.0.1:32789/callback",
        Some("native-client"),
    )
    .await
    .unwrap();
    let state = params(&session)["state"].clone();
    let credentials = session
        .complete("synthetic-code", &state, Some(&fixture.origin))
        .await
        .unwrap();
    let payload = credentials.to_json().unwrap();
    assert!(!payload.contains("unneeded-id-token"));
    assert!(!payload.contains("vendor-field-secret"));
    let mut restored = OAuthCredentials::from_json(&payload).unwrap();
    assert_eq!(
        restored.access_token().await.unwrap().as_str(),
        "rotated-access"
    );
    assert_eq!(restored.resource(), fixture.resource());
}

#[tokio::test]
async fn rejected_refresh_requires_sign_in_without_leaking_upstream_text() {
    let fixture = Fixture::start(true, 0).await;
    fixture.configure("refresh_error", json!(true));
    let session = OAuthSession::begin(
        &fixture.resource(),
        "http://127.0.0.1:32789/callback",
        Some("native-client"),
    )
    .await
    .unwrap();
    let state = params(&session)["state"].clone();
    let mut credentials = session
        .complete("synthetic-code", &state, Some(&fixture.origin))
        .await
        .unwrap();
    let error = credentials.access_token().await.unwrap_err();
    assert_eq!(error, OAuthError::AuthorizationRequired);
    assert!(!format!("{error:?} {error}").contains("sensitive-upstream-text"));
    assert_eq!(
        fixture
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(name, _)| name == "token")
            .count(),
        2
    );
}

#[tokio::test]
async fn refresh_without_replacement_keeps_the_original_refresh_token() {
    let fixture = Fixture::start(true, 0).await;
    fixture.configure("omit_refresh", json!(true));
    let session = OAuthSession::begin(
        &fixture.resource(),
        "http://127.0.0.1:32789/callback",
        Some("native-client"),
    )
    .await
    .unwrap();
    let state = params(&session)["state"].clone();
    let mut credentials = session
        .complete("synthetic-code", &state, Some(&fixture.origin))
        .await
        .unwrap();
    assert_eq!(
        credentials.access_token().await.unwrap().as_str(),
        "rotated-access"
    );
    assert!(credentials.to_json().unwrap().contains("first-refresh"));
}

#[tokio::test]
async fn token_redirect_is_not_followed_and_non_bearer_tokens_are_rejected() {
    for (key, value) in [
        ("token_redirect", json!(true)),
        ("token_type", json!("MAC")),
    ] {
        let fixture = Fixture::start(true, 3600).await;
        fixture.configure(key, value);
        let session = OAuthSession::begin(
            &fixture.resource(),
            "http://127.0.0.1:32789/callback",
            Some("native-client"),
        )
        .await
        .unwrap();
        let state = params(&session)["state"].clone();
        assert!(session
            .complete("synthetic-code", &state, Some(&fixture.origin))
            .await
            .is_err());
        assert_eq!(
            fixture
                .calls
                .lock()
                .unwrap()
                .iter()
                .filter(|(name, _)| name == "token")
                .count(),
            1
        );
        assert!(!fixture
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|(name, _)| name == "revoke"));
    }
}

#[tokio::test]
async fn restored_credentials_require_a_plausible_token_receipt_timestamp() {
    let fixture = Fixture::start(true, 3600).await;
    let session = OAuthSession::begin(
        &fixture.resource(),
        "http://127.0.0.1:32789/callback",
        Some("native-client"),
    )
    .await
    .unwrap();
    let state = params(&session)["state"].clone();
    let credentials = session
        .complete("synthetic-code", &state, Some(&fixture.origin))
        .await
        .unwrap();
    let payload: Value = serde_json::from_str(&credentials.to_json().unwrap()).unwrap();
    for invalid in [Value::Null, json!(0), json!(u64::MAX)] {
        let mut invalid_payload = payload.clone();
        invalid_payload["stored"]["token_received_at"] = invalid;
        assert!(OAuthCredentials::from_json(&invalid_payload.to_string()).is_err());
    }
}

#[derive(Clone, Default)]
struct CapturedEvents(Arc<Mutex<Vec<String>>>);
impl tracing::Subscriber for CapturedEvents {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        struct Fields(String);
        impl tracing::field::Visit for Fields {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                self.0.push_str(&format!("{field}={value:?}"));
            }
        }
        let mut fields = Fields(String::new());
        event.record(&mut fields);
        self.0.lock().unwrap().push(fields.0);
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

#[tokio::test]
async fn enabled_debug_tracing_never_receives_oauth_codes_or_tokens() {
    use tracing::instrument::WithSubscriber;
    let captured = CapturedEvents::default();
    async {
        tracing::debug!("capture-is-active");
        let fixture = Fixture::start(true, 0).await;
        let session = OAuthSession::begin(
            &fixture.resource(),
            "http://127.0.0.1:32789/callback",
            Some("native-client"),
        )
        .await
        .unwrap();
        let state = params(&session)["state"].clone();
        let mut credentials = session
            .complete("synthetic-code", &state, Some(&fixture.origin))
            .await
            .unwrap();
        credentials.access_token().await.unwrap();
    }
    .with_subscriber(captured.clone())
    .await;
    let events = captured.0.lock().unwrap().join("\n");
    assert!(events.contains("capture-is-active"));
    for secret in [
        "synthetic-code",
        "first-access",
        "first-refresh",
        "rotated-access",
        "rotated-refresh",
    ] {
        assert!(!events.contains(secret), "OAuth secret reached tracing");
    }
}
