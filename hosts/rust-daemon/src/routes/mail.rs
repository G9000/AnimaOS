use crate::connectors::mail::{MailConnector, MailDraft, MailMessage};
use utoipa::ToSchema;

#[derive(serde::Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct MailStatusResponse {
    configured: bool,
    connector: Option<MailConnector>,
}
#[derive(serde::Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct MailConnectResponse {
    connector: MailConnector,
    #[serde(skip_serializing_if = "Option::is_none")]
    consent_url: Option<String>,
}
#[derive(serde::Serialize, ToSchema)]
#[allow(dead_code)]
struct MailMessagesResponse {
    messages: Vec<MailMessage>,
}
#[derive(serde::Serialize, ToSchema)]
#[allow(dead_code)]
struct MailDraftsResponse {
    drafts: Vec<MailDraft>,
}
#[derive(serde::Serialize, ToSchema)]
#[allow(dead_code)]
struct MailDraftResponse {
    draft: MailDraft,
}
use super::{
    http::{json_response, request_query},
    AppState,
};
use crate::connectors::mail::{ConnectStage, DraftInput, GenericEmailSetup, MailError, Provider};
use axum::{
    extract::{Path, Request, State},
    http::{header, HeaderValue, StatusCode},
    response::{Html, IntoResponse, Response},
};
use serde_json::json;
fn response(status: StatusCode, value: serde_json::Value) -> Response {
    let mut r = json_response(status, &value);
    r.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    r
}
fn error(e: MailError) -> Response {
    response(
        match e {
            MailError::Invalid => StatusCode::BAD_REQUEST,
            MailError::NotFound => StatusCode::NOT_FOUND,
            MailError::Conflict | MailError::Unauthorized => StatusCode::CONFLICT,
            _ => StatusCode::SERVICE_UNAVAILABLE,
        },
        json!({"error":e.to_string()}),
    )
}
fn guard(s: &AppState, r: &Request) -> Result<(), Response> {
    let result = if r.method() == axum::http::Method::GET {
        s.local_owner.authorize_read(r.headers())
    } else {
        s.local_owner.authorize(r.headers())
    };
    result.map_err(|_| {
        response(
            StatusCode::FORBIDDEN,
            json!({"error":"Local owner authorization is required"}),
        )
    })
}
#[utoipa::path(
 get, path = "/api/agents/{agent_id}/connectors/mail/{provider}", tag = "connectors", 
 params(("agent_id" = String, Path, description = "Agent identifier"),("provider" = String, Path, description = "gmail, outlook or email")),
 responses((status = 200, description = "Mail operation result", body = MailStatusResponse),
 (status = 400, description = "Invalid mail request", body = super::contracts::ErrorBody),
 (status = 403, description = "Local owner authorization required", body = super::contracts::ErrorBody),
 (status = 404, description = "Agent, connector or draft not found", body = super::contracts::ErrorBody),
 (status = 409, description = "Operation unavailable or reauthorization required", body = super::contracts::ErrorBody),
 (status = 503, description = "Provider, credentials or persistence unavailable", body = super::contracts::ErrorBody))
)]
pub(super) async fn status(
    State(s): State<AppState>,
    Path((agent, provider)): Path<(String, String)>,
    r: Request,
) -> Response {
    if let Err(e) = guard(&s, &r) {
        return e;
    }
    let p = match Provider::parse(&provider) {
        Ok(p) => p,
        Err(e) => return error(e),
    };
    match s.mail.connector_for_agent(&agent, p).await {
        Ok(c) => response(
            StatusCode::OK,
            json!({"configured":s.mail.configured(p).await.unwrap_or(false),"connector":c}),
        ),
        Err(e) => error(e),
    }
}
#[utoipa::path(
 post, path = "/api/agents/{agent_id}/connectors/mail/{provider}", tag = "connectors", request_body = Option<GenericEmailSetup>,
 params(("agent_id" = String, Path, description = "Agent identifier"),("provider" = String, Path, description = "gmail, outlook or email")),
 responses((status = 201, description = "Mail operation result", body = MailConnectResponse),
 (status = 400, description = "Invalid mail request", body = super::contracts::ErrorBody),
 (status = 403, description = "Local owner authorization required", body = super::contracts::ErrorBody),
 (status = 404, description = "Agent, connector or draft not found", body = super::contracts::ErrorBody),
 (status = 409, description = "Operation unavailable or reauthorization required", body = super::contracts::ErrorBody),
 (status = 503, description = "Provider, credentials or persistence unavailable", body = super::contracts::ErrorBody))
)]
pub(super) async fn connect(
    State(s): State<AppState>,
    Path((agent, provider)): Path<(String, String)>,
    r: Request,
) -> Response {
    if let Err(e) = guard(&s, &r) {
        return e;
    }
    let p = match Provider::parse(&provider) {
        Ok(p) => p,
        Err(e) => return error(e),
    };
    if p == Provider::Email {
        let bytes = match axum::body::to_bytes(r.into_body(), 32_768).await {
            Ok(bytes) => zeroize::Zeroizing::new(bytes.to_vec()),
            Err(_) => return error(MailError::Invalid),
        };
        let input = match serde_json::from_slice::<GenericEmailSetup>(&bytes) {
            Ok(input) => input,
            Err(_) => return error(MailError::Invalid),
        };
        return match s.mail.connect_email(&agent, input).await {
            Ok(connector) => response(StatusCode::CREATED, json!({"connector":connector})),
            Err(e) => error(e),
        };
    }
    match s.mail.begin_connect(&agent, p).await {
        Ok((c, url)) => response(StatusCode::CREATED, json!({"connector":c,"consentUrl":url})),
        Err(e) => error(e),
    }
}
#[utoipa::path(
 delete, path = "/api/agents/{agent_id}/connectors/mail/{provider}/{connector_id}", tag = "connectors", 
 params(("agent_id" = String, Path, description = "Agent identifier"),("provider" = String, Path, description = "gmail, outlook or email"),("connector_id" = String, Path, description = "Mail connection identifier")),
 responses((status = 200, description = "Mail operation result", body = super::contracts::DeleteResponse),
 (status = 400, description = "Invalid mail request", body = super::contracts::ErrorBody),
 (status = 403, description = "Local owner authorization required", body = super::contracts::ErrorBody),
 (status = 404, description = "Agent, connector or draft not found", body = super::contracts::ErrorBody),
 (status = 409, description = "Operation unavailable or reauthorization required", body = super::contracts::ErrorBody),
 (status = 503, description = "Provider, credentials or persistence unavailable", body = super::contracts::ErrorBody))
)]
pub(super) async fn disconnect(
    State(s): State<AppState>,
    Path((agent, provider, id)): Path<(String, String, String)>,
    r: Request,
) -> Response {
    if let Err(e) = guard(&s, &r) {
        return e;
    }
    let p = match Provider::parse(&provider) {
        Ok(p) => p,
        Err(e) => return error(e),
    };
    match s.mail.disconnect(&agent, &id, p).await {
        Ok(()) => response(StatusCode::OK, json!({"deleted":true})),
        Err(e) => error(e),
    }
}
#[utoipa::path(
 get, path = "/api/agents/{agent_id}/connectors/mail/{provider}/{connector_id}/messages", tag = "connectors", 
 params(("agent_id" = String, Path, description = "Agent identifier"),("provider" = String, Path, description = "gmail, outlook or email"),("connector_id" = String, Path, description = "Mail connection identifier"),("refresh" = Option<bool>, Query, description = "Refresh provider inbox before returning")),
 responses((status = 200, description = "Mail operation result", body = MailMessagesResponse),
 (status = 400, description = "Invalid mail request", body = super::contracts::ErrorBody),
 (status = 403, description = "Local owner authorization required", body = super::contracts::ErrorBody),
 (status = 404, description = "Agent, connector or draft not found", body = super::contracts::ErrorBody),
 (status = 409, description = "Operation unavailable or reauthorization required", body = super::contracts::ErrorBody),
 (status = 503, description = "Provider, credentials or persistence unavailable", body = super::contracts::ErrorBody))
)]
pub(super) async fn messages(
    State(s): State<AppState>,
    Path((agent, provider, id)): Path<(String, String, String)>,
    r: Request,
) -> Response {
    if let Err(e) = guard(&s, &r) {
        return e;
    }
    let p = match Provider::parse(&provider) {
        Ok(p) => p,
        Err(e) => return error(e),
    };
    let refresh = match request_query(r.uri()) {
        Ok(q) => q.get("refresh").is_some_and(|s| s == "true"),
        Err(_) => return error(MailError::Invalid),
    };
    match s.mail.messages(&agent, &id, p, refresh).await {
        Ok(m) => response(StatusCode::OK, json!({"messages":m})),
        Err(e) => error(e),
    }
}
#[utoipa::path(
 get, path = "/api/agents/{agent_id}/connectors/mail/{provider}/{connector_id}/drafts", tag = "connectors", 
 params(("agent_id" = String, Path, description = "Agent identifier"),("provider" = String, Path, description = "gmail, outlook or email"),("connector_id" = String, Path, description = "Mail connection identifier")),
 responses((status = 200, description = "Mail operation result", body = MailDraftsResponse),
 (status = 400, description = "Invalid mail request", body = super::contracts::ErrorBody),
 (status = 403, description = "Local owner authorization required", body = super::contracts::ErrorBody),
 (status = 404, description = "Agent, connector or draft not found", body = super::contracts::ErrorBody),
 (status = 409, description = "Operation unavailable or reauthorization required", body = super::contracts::ErrorBody),
 (status = 503, description = "Provider, credentials or persistence unavailable", body = super::contracts::ErrorBody))
)]
pub(super) async fn drafts(
    State(s): State<AppState>,
    Path((agent, provider, id)): Path<(String, String, String)>,
    r: Request,
) -> Response {
    if let Err(e) = guard(&s, &r) {
        return e;
    }
    let p = match Provider::parse(&provider) {
        Ok(p) => p,
        Err(e) => return error(e),
    };
    match s.mail.drafts(&agent, &id, p).await {
        Ok(d) => response(StatusCode::OK, json!({"drafts":d})),
        Err(e) => error(e),
    }
}
#[utoipa::path(
 post, path = "/api/agents/{agent_id}/connectors/mail/{provider}/{connector_id}/drafts", tag = "connectors", request_body = DraftInput,
 params(("agent_id" = String, Path, description = "Agent identifier"),("provider" = String, Path, description = "gmail, outlook or email"),("connector_id" = String, Path, description = "Mail connection identifier")),
 responses((status = 201, description = "Mail operation result", body = MailDraftResponse),
 (status = 400, description = "Invalid mail request", body = super::contracts::ErrorBody),
 (status = 403, description = "Local owner authorization required", body = super::contracts::ErrorBody),
 (status = 404, description = "Agent, connector or draft not found", body = super::contracts::ErrorBody),
 (status = 409, description = "Operation unavailable or reauthorization required", body = super::contracts::ErrorBody),
 (status = 503, description = "Provider, credentials or persistence unavailable", body = super::contracts::ErrorBody))
)]
pub(super) async fn create_draft(
    State(s): State<AppState>,
    Path((agent, provider, id)): Path<(String, String, String)>,
    r: Request,
) -> Response {
    if let Err(e) = guard(&s, &r) {
        return e;
    }
    let p = match Provider::parse(&provider) {
        Ok(p) => p,
        Err(e) => return error(e),
    };
    let bytes = match axum::body::to_bytes(r.into_body(), 110_000).await {
        Ok(b) => b,
        Err(_) => return error(MailError::Invalid),
    };
    let input = match serde_json::from_slice::<DraftInput>(&bytes) {
        Ok(v) => v,
        Err(_) => return error(MailError::Invalid),
    };
    match s.mail.create_draft(&agent, &id, p, input).await {
        Ok(d) => response(StatusCode::CREATED, json!({"draft":d})),
        Err(e) => error(e),
    }
}
#[utoipa::path(
 post, path = "/api/agents/{agent_id}/connectors/mail/{provider}/{connector_id}/drafts/{draft_id}/approve", tag = "connectors", 
 params(("agent_id" = String, Path, description = "Agent identifier"),("provider" = String, Path, description = "gmail, outlook or email"),("connector_id" = String, Path, description = "Mail connection identifier"),("draft_id" = String, Path, description = "Immutable local draft identifier")),
 responses((status = 200, description = "Mail operation result", body = MailDraftResponse),
 (status = 400, description = "Invalid mail request", body = super::contracts::ErrorBody),
 (status = 403, description = "Local owner authorization required", body = super::contracts::ErrorBody),
 (status = 404, description = "Agent, connector or draft not found", body = super::contracts::ErrorBody),
 (status = 409, description = "Operation unavailable or reauthorization required", body = super::contracts::ErrorBody),
 (status = 503, description = "Provider, credentials or persistence unavailable", body = super::contracts::ErrorBody))
)]
pub(super) async fn approve(
    State(s): State<AppState>,
    Path((agent, provider, id, draft)): Path<(String, String, String, String)>,
    r: Request,
) -> Response {
    resolve(s, agent, provider, id, draft, r, true).await
}
#[utoipa::path(
 post, path = "/api/agents/{agent_id}/connectors/mail/{provider}/{connector_id}/drafts/{draft_id}/reject", tag = "connectors", 
 params(("agent_id" = String, Path, description = "Agent identifier"),("provider" = String, Path, description = "gmail, outlook or email"),("connector_id" = String, Path, description = "Mail connection identifier"),("draft_id" = String, Path, description = "Immutable local draft identifier")),
 responses((status = 200, description = "Mail operation result", body = MailDraftResponse),
 (status = 400, description = "Invalid mail request", body = super::contracts::ErrorBody),
 (status = 403, description = "Local owner authorization required", body = super::contracts::ErrorBody),
 (status = 404, description = "Agent, connector or draft not found", body = super::contracts::ErrorBody),
 (status = 409, description = "Operation unavailable or reauthorization required", body = super::contracts::ErrorBody),
 (status = 503, description = "Provider, credentials or persistence unavailable", body = super::contracts::ErrorBody))
)]
pub(super) async fn reject(
    State(s): State<AppState>,
    Path((agent, provider, id, draft)): Path<(String, String, String, String)>,
    r: Request,
) -> Response {
    resolve(s, agent, provider, id, draft, r, false).await
}
async fn resolve(
    s: AppState,
    agent: String,
    provider: String,
    id: String,
    draft: String,
    r: Request,
    approve: bool,
) -> Response {
    if let Err(e) = guard(&s, &r) {
        return e;
    }
    let p = match Provider::parse(&provider) {
        Ok(p) => p,
        Err(e) => return error(e),
    };
    match s.mail.resolve(&agent, &id, p, &draft, approve).await {
        Ok(d) => response(StatusCode::OK, json!({"draft":d})),
        Err(e) => error(e),
    }
}
/// What to check, in fixed words, for the step of finishing a sign-in that failed.
fn failure_hint(provider: &str, stage: ConnectStage, error: MailError) -> String {
    let (console, mailbox_fix) = if provider == "Google" {
        (
            "Google Cloud",
            "Make sure the Gmail API is turned on for your Google Cloud project",
        )
    } else {
        (
            "Microsoft Entra",
            "Check that the app has permission to read your mail",
        )
    };
    match (stage, error) {
        (ConnectStage::Setup, _) => format!(
            "Anima could not read the {provider} app credentials. Check OAuth app setup in Connectors, then reconnect."
        ),
        (ConnectStage::Attempt, _) => "This sign-in attempt is no longer valid. It may have expired, been used already, been replaced by a newer attempt, or Anima may have restarted. Start again with Reconnect in Connectors and finish in that same flow.".to_string(),
        (ConnectStage::Exchange | ConnectStage::Profile, MailError::Upstream) => format!(
            "{provider} could not be reached, or had a problem. Wait a moment, then reconnect from Connectors."
        ),
        (ConnectStage::Exchange, _) => format!(
            "{provider} did not accept the sign-in code. Check that the client ID and client secret in OAuth app setup match the app in {console}, and that its redirect URI matches exactly. Then reconnect."
        ),
        (ConnectStage::Profile, _) => format!(
            "{provider} signed you in, but Anima could not read your mailbox. {mailbox_fix}, then reconnect."
        ),
        (ConnectStage::Save, _) => "Anima could not save the connection. Check that the credential vault is available, then reconnect.".to_string(),
    }
}
#[utoipa::path(get, path = "/api/connectors/mail/{provider}/callback", tag = "connectors",
 params(("provider" = String, Path, description = "gmail, outlook or email")),
 responses((status = 200, description = "OAuth connection completed"), (status = 400, description = "Invalid, expired or unsuccessful OAuth callback")))]
pub(super) async fn callback(
    State(s): State<AppState>,
    Path(provider): Path<String>,
    uri: axum::http::Uri,
) -> Response {
    let p = match Provider::parse(&provider) {
        Ok(p) => p,
        Err(e) => return error(e),
    };
    let query = match request_query(&uri) {
        Ok(q) => q,
        Err(_) => return error(MailError::Invalid),
    };
    let code = query.get("code").map(String::as_str).unwrap_or("");
    let nonce = query.get("state").map(String::as_str).unwrap_or("");
    let result = s.mail.complete_connect_staged(p, nonce, code).await;
    let provider_name = match p {
        Provider::Gmail => Some("Google"),
        Provider::Outlook => Some("Microsoft"),
        Provider::Email => None,
    };
    let (status, message) = match result {
        Ok(_) => (
            StatusCode::OK,
            "Mail connected. Close this tab and return to Connectors.".to_string(),
        ),
        Err((stage, error)) => (
            StatusCode::BAD_REQUEST,
            // The provider's own refusal comes first; then the step that failed.
            provider_name
                .and_then(|name| super::oauth_callback::denial_hint(name, &uri))
                .or_else(|| provider_name.map(|name| failure_hint(name, stage, error)))
                .unwrap_or_else(|| {
                    "Authorization could not be completed. Reconnect from the Connectors tab."
                        .to_string()
                }),
        ),
    };
    let mut r = (
        status,
        Html(format!(
            "<!doctype html><html><body><p>{message}</p></body></html>"
        )),
    )
        .into_response();
    r.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    r.headers_mut().insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    r
}

#[cfg(test)]
mod generic_email_tests {
    use crate::app::SharedDaemonState;
    use crate::connectors::mail::{
        generic_tests::{fixture, setup_json},
        MailManager,
    };
    use std::sync::{atomic::Ordering, Arc};
    fn router(state: SharedDaemonState, mail: MailManager) -> axum::Router {
        let runs = crate::agent_runs::AgentRunCoordinator::new(
            state.clone(),
            Arc::new(tokio::sync::Semaphore::new(4)),
        );
        let limiter = Arc::new(tokio::sync::Semaphore::new(4));
        let oauth = mail.oauth_apps.clone();
        let connectors = crate::connectors::runtime::ConnectorManager::new(
            state.clone(),
            runs.clone(),
            Arc::new(crate::connectors::credentials::InMemoryCredentialStore::default()),
            Arc::new(crate::connectors::telegram::TelegramClient::new().unwrap()),
        );
        let scheduler = crate::schedules::SchedulerService::new(
            state.clone(),
            runs.clone(),
            connectors.clone(),
        );
        let calendar = crate::connectors::gcalendar::CalendarManager::new(
            &state,
            runs.clone(),
            Arc::new(crate::connectors::gcalendar::store::InMemoryGoogleCredentialStore::default()),
            Arc::new(crate::connectors::gcalendar::client::UnconfiguredGoogleTransport),
            oauth.clone(),
        );
        let jobs = crate::jobs::JobService::new(state.clone(), runs.clone());
        crate::routes::router_with_services_with_policies(
            state,
            crate::app::DaemonConfig::default(),
            limiter,
            runs,
            connectors,
            calendar,
            mail,
            oauth,
            scheduler,
            jobs,
            crate::history::HistoryWorkerOwner::new(),
            crate::logs::global(),
            crate::routes::http::LocalOwnerPolicy::for_test(
                true,
                Some(crate::routes::owner_test_support::ADMIN_TOKEN),
            ),
            crate::routes::http::ApiKeyPolicy::for_test(None),
            crate::system::SupervisorRelay::unsupervised(),
        )
    }

    /// The page a browser lands on after the provider's sign-in, for a callback
    /// with no code: it explains a refusal in fixed words and never echoes the URL.
    #[tokio::test]
    async fn callback_explains_a_provider_refusal_without_echoing_the_url() {
        use axum::{
            body::Body,
            http::{Request, StatusCode},
        };
        use tower::ServiceExt;
        let (state, manager, _fake, _credentials, _agent) = fixture(false).await;
        let app = router(state, manager.clone());
        let page = |uri: &'static str| {
            let app = app.clone();
            async move {
                let response = app
                    .oneshot(
                        Request::builder()
                            .method("GET")
                            .uri(uri)
                            .header("host", "127.0.0.1:8080")
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                let status = response.status();
                let bytes = axum::body::to_bytes(response.into_body(), 32_768)
                    .await
                    .unwrap();
                (status, String::from_utf8_lossy(&bytes).into_owned())
            }
        };

        let (status, body) =
            page("/api/connectors/mail/gmail/callback?error=access_denied&state=nonce-sentinel")
                .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.contains("Google did not give access."), "{body}");
        assert!(body.contains("test user"), "{body}");
        assert!(!body.contains("nonce-sentinel"), "{body}");

        let (_, body) =
            page("/api/connectors/mail/outlook/callback?error=access_denied&state=abc").await;
        assert!(body.contains("Microsoft did not give access."), "{body}");

        // An unknown value falls back to the step that failed (here, no app
        // credentials) and shows nothing from the URL.
        let (status, body) = page(
            "/api/connectors/mail/gmail/callback?error=%3Cscript%3Ealert(1)%3C%2Fscript%3E&state=abc",
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            body.contains("could not read the Google app credentials"),
            "{body}"
        );
        assert!(!body.contains("script"), "{body}");
        manager.shutdown().await;
    }

    /// A callback that reaches Anima with a code says which step failed.
    #[tokio::test]
    async fn callback_names_the_step_that_failed_when_finishing_sign_in() {
        use crate::connectors::oauth_apps::{OAuthAppCredentials, OAuthProvider};
        use axum::{
            body::Body,
            http::{Request, StatusCode},
        };
        use tower::ServiceExt;
        let (state, manager, _fake, _credentials, _agent) = fixture(false).await;
        let app = router(state, manager.clone());
        let page = |uri: &'static str| {
            let app = app.clone();
            async move {
                let response = app
                    .oneshot(
                        Request::builder()
                            .method("GET")
                            .uri(uri)
                            .header("host", "127.0.0.1:8080")
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                let status = response.status();
                let bytes = axum::body::to_bytes(response.into_body(), 32_768)
                    .await
                    .unwrap();
                (status, String::from_utf8_lossy(&bytes).into_owned())
            }
        };

        // No app credentials saved: the first step fails.
        let (status, body) =
            page("/api/connectors/mail/gmail/callback?code=abc&state=unknown").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            body.contains("could not read the Google app credentials"),
            "{body}"
        );

        // With credentials, an attempt this daemon never started is named as such.
        manager
            .oauth_apps
            .put(
                OAuthProvider::Google,
                OAuthAppCredentials::new(OAuthProvider::Google, "client", "secret", None).unwrap(),
            )
            .await
            .unwrap();
        let (status, body) =
            page("/api/connectors/mail/gmail/callback?code=abc&state=unknown").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.contains("no longer valid"), "{body}");
        assert!(!body.contains("unknown"), "{body}");
        manager.shutdown().await;
    }

    #[tokio::test]
    async fn generic_setup_http_requires_owner_and_same_origin_before_verification() {
        use axum::{
            body::Body,
            http::{Request, StatusCode},
        };
        use tower::ServiceExt;
        let (state, manager, fake, _credentials, agent) = fixture(false).await;
        let app = router(state, manager.clone());
        let uri = format!("/api/agents/{agent}/connectors/mail/email");
        for (owner, origin) in [
            (None, None),
            (Some("Bearer wrong-token"), None),
            (None, Some("https://untrusted.example")),
            (
                Some("Bearer local-admin"),
                Some("https://untrusted.example"),
            ),
        ] {
            let mut request = Request::builder()
                .method("POST")
                .uri(&uri)
                .header("host", "127.0.0.1:8080")
                .header("content-type", "application/json");
            if let Some(owner) = owner {
                request = request.header("authorization", owner)
            }
            if let Some(origin) = origin {
                request = request.header("origin", origin)
            }
            let response = app
                .clone()
                .oneshot(
                    request
                        .body(Body::from(setup_json(false).to_string()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
        }
        assert_eq!(fake.verifies.load(Ordering::SeqCst), 0);
        assert_eq!(fake.sends.load(Ordering::SeqCst), 0);
        let response = app
            .clone()
            .oneshot(
                crate::routes::owner_test_support::authenticate(
                    Request::builder()
                        .method("POST")
                        .uri("/api/agents/other-agent/connectors/mail/email")
                        .header("host", "127.0.0.1:8080")
                        .header("origin", "http://localhost:4200"),
                )
                .body(Body::from(setup_json(false).to_string()))
                .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(fake.verifies.load(Ordering::SeqCst), 0);
        let response = app
            .oneshot(
                crate::routes::owner_test_support::authenticate(
                    Request::builder()
                        .method("POST")
                        .uri(&uri)
                        .header("host", "127.0.0.1:8080")
                        .header("origin", "http://localhost:4200")
                        .header("authorization", "Bearer local-admin"),
                )
                .body(Body::from(setup_json(false).to_string()))
                .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let bytes = axum::body::to_bytes(response.into_body(), 32_768)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["connector"]["type"], "email");
        assert!(!String::from_utf8_lossy(&bytes).contains("secret-sentinel"));
        assert_eq!(fake.verifies.load(Ordering::SeqCst), 1);
        assert_eq!(fake.sends.load(Ordering::SeqCst), 0);
        manager.shutdown().await;
    }
}
