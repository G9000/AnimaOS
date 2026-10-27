use super::*;
use axum::{
    extract::State,
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};

#[derive(Clone)]
struct AuthState {
    origin: String,
    refreshes: Arc<AtomicUsize>,
    exchanges: Arc<AtomicUsize>,
    metadata_requests: Arc<AtomicUsize>,
    hold_refresh: Arc<AtomicBool>,
    refresh_entered: Arc<Semaphore>,
    refresh_release: Arc<Semaphore>,
}
struct AuthFixture {
    state: AuthState,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for AuthFixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl AuthFixture {
    async fn new() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let state = AuthState {
            origin: format!("http://{}", listener.local_addr().unwrap()),
            refreshes: Arc::new(AtomicUsize::new(0)),
            exchanges: Arc::new(AtomicUsize::new(0)),
            metadata_requests: Arc::new(AtomicUsize::new(0)),
            hold_refresh: Arc::new(AtomicBool::new(false)),
            refresh_entered: Arc::new(Semaphore::new(0)),
            refresh_release: Arc::new(Semaphore::new(0)),
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
            .route("/token", post(token))
            .route("/revoke", post(|| async { StatusCode::OK }))
            .with_state(state.clone());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self { state, task }
    }
    async fn configure(&self, fixture: &Fixture, enabled: bool) -> McpConnectionView {
        let row = fixture.create().await;
        fixture
            .manager
            .update(
                &fixture.agent,
                &row.record.id,
                UpdateMcpConnection {
                    expected_version: row.record.version,
                    label: None,
                    endpoint: Some(format!("{}/mcp", self.state.origin)),
                    enabled: Some(enabled),
                },
            )
            .await
            .unwrap()
    }
}
async fn resource(State(state): State<AuthState>) -> Json<serde_json::Value> {
    state.metadata_requests.fetch_add(1, Ordering::SeqCst);
    Json(
        json!({"resource":format!("{}/mcp",state.origin),"authorization_servers":[state.origin],"scopes_supported":["read"]}),
    )
}
async fn metadata(State(state): State<AuthState>) -> Json<serde_json::Value> {
    state.metadata_requests.fetch_add(1, Ordering::SeqCst);
    Json(
        json!({"issuer":state.origin,"authorization_endpoint":format!("{}/authorize",state.origin),"token_endpoint":format!("{}/token",state.origin),"revocation_endpoint":format!("{}/revoke",state.origin),"response_types_supported":["code"],"token_endpoint_auth_methods_supported":["none"],"code_challenge_methods_supported":["S256"],"authorization_response_iss_parameter_supported":true}),
    )
}
async fn token(State(state): State<AuthState>, body: String) -> Json<serde_json::Value> {
    let url = reqwest::Url::parse(&format!("http://localhost/?{body}")).unwrap();
    let params: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
    assert_eq!(params["resource"], format!("{}/mcp", state.origin));
    if params["grant_type"] == "refresh_token" {
        state.refreshes.fetch_add(1, Ordering::SeqCst);
        if state.hold_refresh.load(Ordering::SeqCst) {
            state.refresh_entered.add_permits(1);
            state.refresh_release.acquire().await.unwrap().forget();
        }
        Json(
            json!({"access_token":"oauth-rotated-synthetic","token_type":"Bearer","refresh_token":"oauth-refresh-rotated-synthetic","expires_in":3600}),
        )
    } else {
        state.exchanges.fetch_add(1, Ordering::SeqCst);
        assert!(!params["code_verifier"].is_empty());
        Json(
            json!({"access_token":"oauth-initial-synthetic","token_type":"Bearer","refresh_token":"oauth-refresh-initial-synthetic","expires_in":1}),
        )
    }
}
fn authorization(start: &McpOAuthStart) -> (reqwest::Url, String) {
    let data = serde_json::to_value(start).unwrap();
    let url = reqwest::Url::parse(data["authorizationUrl"].as_str().unwrap()).unwrap();
    let params: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
    (
        reqwest::Url::parse(&params["redirect_uri"]).unwrap(),
        params["state"].clone(),
    )
}
async fn callback(start: &McpOAuthStart, wrong: bool) -> reqwest::Response {
    let (mut url, state) = authorization(start);
    let data = serde_json::to_value(start).unwrap();
    let issuer = reqwest::Url::parse(data["authorizationUrl"].as_str().unwrap())
        .unwrap()
        .origin()
        .ascii_serialization();
    url.query_pairs_mut()
        .append_pair("code", "synthetic-code")
        .append_pair("state", if wrong { "wrong-state" } else { &state })
        .append_pair("iss", &issuer);
    reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(4))
        .build()
        .unwrap()
        .get(url)
        .send()
        .await
        .unwrap()
}

#[tokio::test]
async fn oauth_missing_or_wrong_callback_issuer_never_exchanges_authorization_code() {
    let fixture = Fixture::new().await;
    let auth = AuthFixture::new().await;
    let row = auth.configure(&fixture, false).await;
    let start = fixture
        .manager
        .start_oauth(
            &fixture.agent,
            &row.record.id,
            row.record.version,
            Some("native-test-client".into()),
        )
        .await
        .unwrap();
    for (issuer, denied) in [
        (None, false),
        (Some("https://wrong-issuer.invalid"), false),
        (Some("https://wrong-issuer.invalid"), true),
    ] {
        let (mut url, state) = authorization(&start);
        url.query_pairs_mut().append_pair("state", &state);
        if denied {
            url.query_pairs_mut().append_pair("error", "access_denied");
        } else {
            url.query_pairs_mut().append_pair("code", "synthetic-code");
        }
        if let Some(issuer) = issuer {
            url.query_pairs_mut().append_pair("iss", issuer);
        }
        let response = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(4))
            .build()
            .unwrap()
            .get(url)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let status = fixture
            .manager
            .oauth_status(&fixture.agent, &row.record.id)
            .await
            .unwrap();
        assert_eq!(serde_json::to_value(status).unwrap()["state"], "pending");
        assert!(fixture
            .manager
            .get(&fixture.agent, &row.record.id)
            .await
            .unwrap()
            .record
            .oauth_credential_ref
            .is_none());
        assert_eq!(auth.state.exchanges.load(Ordering::SeqCst), 0);
        assert_eq!(fixture.factory.connects.load(Ordering::SeqCst), 0);
    }
    assert_eq!(callback(&start, false).await.status(), StatusCode::OK);
    assert!(settled(&fixture, &row.record.id)
        .await
        .record
        .oauth_credential_ref
        .is_some());
    assert_eq!(auth.state.exchanges.load(Ordering::SeqCst), 1);
    fixture.manager.shutdown().await;
}
async fn settled(fixture: &Fixture, id: &str) -> McpConnectionView {
    timeout(Duration::from_secs(5), async {
        loop {
            let status = fixture
                .manager
                .oauth_status(&fixture.agent, id)
                .await
                .unwrap();
            if serde_json::to_value(status).unwrap()["state"] != "pending" {
                break fixture.manager.get(&fixture.agent, id).await.unwrap();
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn oauth_callback_state_refresh_rotation_and_signout_preserve_tool_review() {
    let fixture = Fixture::new().await;
    let auth = AuthFixture::new().await;
    let row = auth.configure(&fixture, true).await;
    let start = fixture
        .manager
        .start_oauth(
            &fixture.agent,
            &row.record.id,
            row.record.version,
            Some("native-test-client".into()),
        )
        .await
        .unwrap();
    assert_eq!(fixture.factory.connects.load(Ordering::SeqCst), 0);
    assert_eq!(
        callback(&start, true).await.status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(auth.state.exchanges.load(Ordering::SeqCst), 0);
    let response = callback(&start, false).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let row = settled(&fixture, &row.record.id).await;
    assert_eq!(row.state, McpConnectionState::NeedsReview);
    assert!(row.record.reviewed_tools.is_empty());
    assert!(row.record.bearer_credential_ref.is_none());
    let reference = row.record.oauth_credential_ref.clone().unwrap();
    let snapshot = std::fs::read_to_string(fixture.folder.join("control.json")).unwrap();
    assert!(snapshot.contains(&reference));
    assert!(!snapshot.contains("oauth-refresh-initial-synthetic"));
    let provider = fixture.factory.oauth_launches.lock().unwrap()[0].clone();
    // Expiry is persisted; simultaneous requests share one refresh rotation.
    tokio::time::sleep(Duration::from_secs(2)).await;
    let (first, second) = tokio::join!(provider.token(), provider.token());
    assert_eq!(first.unwrap().expose(), "oauth-rotated-synthetic");
    assert_eq!(second.unwrap().expose(), "oauth-rotated-synthetic");
    assert_eq!(auth.state.refreshes.load(Ordering::SeqCst), 1);
    let payload = fixture
        .credentials
        .inner
        .load_oauth(&row.record.id, &row.record.endpoint, &reference)
        .await
        .unwrap()
        .unwrap();
    assert!(payload.contains("oauth-refresh-rotated-synthetic"));
    provider.authentication_rejected();
    assert_eq!(
        serde_json::to_value(
            fixture
                .manager
                .oauth_status(&fixture.agent, &row.record.id)
                .await
                .unwrap()
        )
        .unwrap()["state"],
        "needs_sign_in"
    );
    assert!(provider.token().await.is_err());
    let signed_out = fixture
        .manager
        .sign_out_oauth(&fixture.agent, &row.record.id, row.record.version)
        .await
        .unwrap();
    assert!(signed_out.record.oauth_credential_ref.is_none());
    assert!(fixture
        .credentials
        .inner
        .load_oauth(&row.record.id, &row.record.endpoint, &reference)
        .await
        .unwrap()
        .is_none());
    assert!(provider.token().await.is_err());
    fixture.manager.shutdown().await;
}

#[tokio::test]
async fn oauth_cancel_and_endpoint_edit_fence_callbacks_before_token_exchange() {
    let fixture = Fixture::new().await;
    let auth = AuthFixture::new().await;
    let row = auth.configure(&fixture, false).await;
    let start = fixture
        .manager
        .start_oauth(
            &fixture.agent,
            &row.record.id,
            row.record.version,
            Some("native-test-client".into()),
        )
        .await
        .unwrap();
    let attempt = serde_json::to_value(&start).unwrap()["attemptId"]
        .as_str()
        .unwrap()
        .to_owned();
    fixture
        .manager
        .cancel_oauth_attempt(&fixture.agent, &row.record.id, row.record.version, &attempt)
        .await
        .unwrap();
    let status = serde_json::to_value(
        fixture
            .manager
            .oauth_status(&fixture.agent, &row.record.id)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(status["state"], "signed_out");
    let second = fixture
        .manager
        .start_oauth(
            &fixture.agent,
            &row.record.id,
            row.record.version,
            Some("native-test-client".into()),
        )
        .await
        .unwrap();
    let (first_redirect, first_state) = authorization(&start);
    let (second_redirect, second_state) = authorization(&second);
    assert_eq!(first_redirect.path(), "/oauth/mcp/callback");
    assert_eq!(second_redirect.path(), first_redirect.path());
    assert_ne!(first_state, second_state);
    for attempt in [&start, &second] {
        let encoded = serde_json::to_value(attempt).unwrap();
        let url = reqwest::Url::parse(encoded["authorizationUrl"].as_str().unwrap()).unwrap();
        let params: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(params["client_id"], "native-test-client");
        assert_eq!(params["code_challenge_method"], "S256");
        assert!(!params["code_challenge"].is_empty());
    }
    fixture
        .manager
        .update(
            &fixture.agent,
            &row.record.id,
            UpdateMcpConnection {
                expected_version: row.record.version,
                label: None,
                endpoint: Some(format!("{}/changed-mcp", auth.state.origin)),
                enabled: None,
            },
        )
        .await
        .unwrap();
    let (mut url, state) = authorization(&second);
    url.query_pairs_mut()
        .append_pair("code", "synthetic-code")
        .append_pair("state", &state);
    let response = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_millis(500))
        .build()
        .unwrap()
        .get(url)
        .send()
        .await;
    assert!(response.is_err() || !response.unwrap().status().is_success());
    assert_eq!(auth.state.exchanges.load(Ordering::SeqCst), 0);
    assert!(fixture
        .manager
        .get(&fixture.agent, &row.record.id)
        .await
        .unwrap()
        .record
        .oauth_credential_ref
        .is_none());
    fixture.manager.shutdown().await;
}

#[tokio::test]
async fn oauth_continuation_requires_its_exact_version_and_uncancelled_admission() {
    let fixture = Fixture::new().await;
    let row = fixture.create().await;
    let row = fixture
        .manager
        .update(
            &fixture.agent,
            &row.record.id,
            UpdateMcpConnection {
                expected_version: row.record.version,
                label: None,
                endpoint: None,
                enabled: Some(true),
            },
        )
        .await
        .unwrap();
    let admission = tokio_util::sync::CancellationToken::new();
    admission.cancel();
    assert!(matches!(
        fixture
            .manager
            .discover_owned_with_oauth(
                &fixture.agent,
                &row.record.id,
                row.record.version,
                true,
                Some(admission)
            )
            .await,
        Err(McpServiceError::Revoked)
    ));
    let changed = fixture
        .manager
        .update(
            &fixture.agent,
            &row.record.id,
            UpdateMcpConnection {
                expected_version: row.record.version,
                label: Some("Changed after OAuth commit".into()),
                endpoint: None,
                enabled: None,
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        fixture
            .manager
            .discover_owned_with_oauth(
                &fixture.agent,
                &row.record.id,
                row.record.version,
                true,
                Some(tokio_util::sync::CancellationToken::new())
            )
            .await,
        Err(McpServiceError::Conflict)
    ));
    assert_eq!(fixture.factory.connects.load(Ordering::SeqCst), 0);
    assert!(changed.record.oauth_credential_ref.is_none());
    fixture.manager.shutdown().await;
}

#[tokio::test]
async fn oauth_shutdown_closes_pending_callback_and_failed_vault_put_never_connects() {
    let fixture = Fixture::new().await;
    let auth = AuthFixture::new().await;
    let row = auth.configure(&fixture, true).await;
    let start = fixture
        .manager
        .start_oauth(
            &fixture.agent,
            &row.record.id,
            row.record.version,
            Some("native-test-client".into()),
        )
        .await
        .unwrap();
    fixture
        .credentials
        .uncertain_put
        .store(true, Ordering::SeqCst);
    assert_eq!(
        callback(&start, false).await.status(),
        StatusCode::BAD_REQUEST
    );
    let failed = settled(&fixture, &row.record.id).await;
    assert!(failed.record.oauth_credential_ref.is_none());
    assert_eq!(fixture.factory.connects.load(Ordering::SeqCst), 0);
    fixture
        .credentials
        .uncertain_put
        .store(false, Ordering::SeqCst);
    let pending = fixture
        .manager
        .start_oauth(
            &fixture.agent,
            &row.record.id,
            row.record.version,
            Some("native-test-client".into()),
        )
        .await
        .unwrap();
    let (url, _) = authorization(&pending);
    fixture.manager.shutdown().await;
    assert!(reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_millis(500))
        .build()
        .unwrap()
        .get(url)
        .send()
        .await
        .is_err());
    assert_eq!(auth.state.exchanges.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn cancelled_oauth_refresh_settles_before_signout_and_cannot_resurrect_credentials() {
    let fixture = Fixture::new().await;
    let auth = AuthFixture::new().await;
    let row = auth.configure(&fixture, true).await;
    let start = fixture
        .manager
        .start_oauth(
            &fixture.agent,
            &row.record.id,
            row.record.version,
            Some("native-test-client".into()),
        )
        .await
        .unwrap();
    assert_eq!(callback(&start, false).await.status(), StatusCode::OK);
    let row = settled(&fixture, &row.record.id).await;
    let reference = row.record.oauth_credential_ref.clone().unwrap();
    let provider = fixture.factory.oauth_launches.lock().unwrap()[0].clone();
    tokio::time::sleep(Duration::from_secs(2)).await;
    auth.state.hold_refresh.store(true, Ordering::SeqCst);
    let refresh = tokio::spawn(async move { provider.token().await });
    timeout(Duration::from_secs(3), auth.state.refresh_entered.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    refresh.abort();
    let _ = refresh.await;
    let manager = fixture.manager.clone();
    let agent = fixture.agent.clone();
    let id = row.record.id.clone();
    let version = row.record.version;
    let signout = tokio::spawn(async move { manager.sign_out_oauth(&agent, &id, version).await });
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert!(!signout.is_finished());
    auth.state.refresh_release.add_permits(1);
    let signed_out = timeout(Duration::from_secs(5), signout)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(signed_out.record.oauth_credential_ref.is_none());
    assert!(fixture
        .credentials
        .inner
        .load_oauth(&row.record.id, &row.record.endpoint, &reference)
        .await
        .unwrap()
        .is_none());
    assert_eq!(auth.state.refreshes.load(Ordering::SeqCst), 1);
    fixture.manager.shutdown().await;
}

#[tokio::test]
async fn agent_deletion_retains_oauth_cleanup_lookup_until_vault_cleanup_succeeds() {
    for published in [false, true] {
        let fixture = Fixture::new().await;
        let auth = AuthFixture::new().await;
        let row = auth.configure(&fixture, true).await;
        let start = fixture
            .manager
            .start_oauth(
                &fixture.agent,
                &row.record.id,
                row.record.version,
                Some("native-test-client".into()),
            )
            .await
            .unwrap();
        let row = if published {
            assert_eq!(callback(&start, false).await.status(), StatusCode::OK);
            settled(&fixture, &row.record.id).await
        } else {
            row
        };
        let reference = row.record.oauth_credential_ref.clone();
        let runs = crate::agent_runs::AgentRunCoordinator::new(
            fixture.state.clone(),
            Arc::new(Semaphore::new(4)),
        )
        .with_mcp(fixture.manager.clone());
        let connectors = crate::connectors::runtime::ConnectorManager::new(
            fixture.state.clone(),
            runs,
            Arc::new(crate::connectors::credentials::InMemoryCredentialStore::default()),
            Arc::new(NoTelegram),
        );
        fixture
            .credentials
            .fail_delete
            .store(true, Ordering::SeqCst);
        assert!(connectors
            .delete_agent(fixture.agent.clone())
            .await
            .is_err());
        assert!(fixture
            .state
            .read()
            .await
            .agents
            .contains_key(&fixture.agent));
        assert_eq!(
            fixture.state.read().await.mcp_registry.connections[&row.record.id],
            row.record
        );
        fixture
            .credentials
            .fail_delete
            .store(false, Ordering::SeqCst);
        connectors
            .delete_agent(fixture.agent.clone())
            .await
            .unwrap();
        assert!(!fixture
            .state
            .read()
            .await
            .agents
            .contains_key(&fixture.agent));
        assert!(!fixture
            .state
            .read()
            .await
            .mcp_registry
            .connections
            .contains_key(&row.record.id));
        if let Some(reference) = reference {
            assert!(fixture
                .credentials
                .inner
                .load_oauth(&row.record.id, &row.record.endpoint, &reference)
                .await
                .unwrap()
                .is_none());
        }
        fixture.manager.shutdown().await;
        connectors.shutdown().await;
    }
}

#[tokio::test]
async fn queued_oauth_start_cannot_admit_after_shutdown_cancellation_pass() {
    let fixture = Fixture::new().await;
    let auth = AuthFixture::new().await;
    let row = auth.configure(&fixture, false).await;
    let transactions = fixture.manager.control_plane_transactions();
    let transaction = transactions.lock().await;
    let manager = fixture.manager.clone();
    let agent = fixture.agent.clone();
    let id = row.record.id.clone();
    let version = row.record.version;
    let start = tokio::spawn(async move {
        manager
            .start_oauth(&agent, &id, version, Some("native-test-client".into()))
            .await
    });
    timeout(Duration::from_secs(3), async {
        while fixture.manager.pending_work_count() == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    fixture.manager.request_shutdown();
    drop(transaction);
    assert!(matches!(
        start.await.unwrap(),
        Err(McpServiceError::Unavailable)
    ));
    assert_eq!(auth.state.metadata_requests.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.factory.connects.load(Ordering::SeqCst), 0);
    assert_eq!(
        serde_json::to_value(
            fixture
                .manager
                .oauth_status(&fixture.agent, &row.record.id)
                .await
                .unwrap()
        )
        .unwrap()["state"],
        "signed_out"
    );
    fixture.manager.shutdown().await;
}
