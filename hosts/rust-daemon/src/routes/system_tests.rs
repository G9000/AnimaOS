//! The drain and shutdown routes a supervisor (admin token) and the console
//! (allowed Origin plus a console session) call.
use std::sync::Arc;
use std::time::Duration;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::{RwLock, Semaphore};
use tower::util::ServiceExt;

use super::http::{ApiKeyPolicy, LocalOwnerPolicy};
use crate::agent_runs::AgentRunCoordinator;
use crate::app::DaemonConfig;
use crate::connectors::credentials::InMemoryCredentialStore;
use crate::connectors::runtime::ConnectorManager;
use crate::state::DaemonState;

const OWNER_ORIGIN: &str = "http://localhost:4200";
const ADMIN_TOKEN: &str = "supervisor-admin";

/// A router whose local-owner policy accepts `ADMIN_TOKEN`, as the packaged
/// supervisor's child daemon does.
fn app() -> axum::Router {
    app_with_scheduler().0
}

/// `app()` with its scheduler, for a test that saves an automation first.
fn app_with_scheduler() -> (
    axum::Router,
    crate::schedules::SchedulerService,
    crate::app::SharedDaemonState,
) {
    app_with_supervisor(crate::system::SupervisorRelay::unsupervised())
}

fn app_with_supervisor(
    supervisor: crate::system::SupervisorRelay,
) -> (
    axum::Router,
    crate::schedules::SchedulerService,
    crate::app::SharedDaemonState,
) {
    let state = Arc::new(RwLock::new(DaemonState::new()));
    let limiter = Arc::new(Semaphore::new(8));
    let runs = AgentRunCoordinator::new(Arc::clone(&state), Arc::clone(&limiter));
    let connectors = ConnectorManager::new(
        Arc::clone(&state),
        runs.clone(),
        Arc::new(InMemoryCredentialStore::default()),
        Arc::new(crate::app::DeterministicTelegramTransport),
    );
    let scheduler = crate::schedules::SchedulerService::new(
        Arc::clone(&state),
        runs.clone(),
        connectors.clone(),
    );
    let oauth_apps = crate::connectors::oauth_apps::OAuthAppService::in_memory();
    let calendar = crate::connectors::gcalendar::CalendarManager::new(
        &state,
        runs.clone(),
        Arc::new(crate::connectors::gcalendar::store::InMemoryGoogleCredentialStore::default()),
        Arc::new(crate::connectors::gcalendar::client::UnconfiguredGoogleTransport),
        oauth_apps.clone(),
    );
    let mail = crate::connectors::mail::MailManager::new(
        &state,
        runs.clone(),
        Arc::new(crate::connectors::gcalendar::store::InMemoryGoogleCredentialStore::default()),
        Arc::new(crate::app::DeterministicMailTransport),
        oauth_apps.clone(),
    );
    let jobs = crate::jobs::JobService::new(Arc::clone(&state), runs.clone());
    let router = super::router_with_services_with_policies(
        Arc::clone(&state),
        DaemonConfig::default(),
        limiter,
        runs,
        connectors,
        calendar,
        mail,
        oauth_apps,
        scheduler.clone(),
        jobs,
        crate::history::HistoryWorkerOwner::new(),
        crate::logs::global(),
        LocalOwnerPolicy::for_test(true, Some(ADMIN_TOKEN)),
        ApiKeyPolicy::for_test(None),
        supervisor,
    );
    (router, scheduler, state)
}

async fn send(
    app: &axum::Router,
    method: &str,
    path: &str,
    auth: &[(&str, &str)],
) -> (StatusCode, Value) {
    send_body(app, method, path, auth, Body::empty()).await
}

async fn send_body(
    app: &axum::Router,
    method: &str,
    path: &str,
    auth: &[(&str, &str)],
    body: Body,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("host", "127.0.0.1:8787");
    for (name, value) in auth {
        builder = builder.header(*name, *value);
    }
    let response = app
        .clone()
        .oneshot(builder.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    assert_eq!(
        response
            .headers()
            .get("cache-control")
            .and_then(|value| value.to_str().ok()),
        Some("no-store"),
        "{method} {path} {status}"
    );
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// A console session's two halves, as the browser holds them.
struct Console {
    cookie: String,
    token: String,
}

impl Console {
    /// What the console's lifecycle requests carry.
    fn headers(&self) -> [(&str, &str); 3] {
        [
            ("origin", OWNER_ORIGIN),
            ("cookie", self.cookie.as_str()),
            ("x-anima-console-session", self.token.as_str()),
        ]
    }
}

async fn trade_code(
    app: &axum::Router,
    code: &str,
    auth: &[(&str, &str)],
) -> axum::response::Response {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/api/system/session")
        .header("host", "127.0.0.1:8787")
        .header("content-type", "application/json");
    for (name, value) in auth {
        builder = builder.header(*name, *value);
    }
    let body = serde_json::json!({ "code": code }).to_string();
    app.clone()
        .oneshot(builder.body(Body::from(body)).unwrap())
        .await
        .unwrap()
}

/// Opens a console session the way the console does: a launch code from
/// the admin token, traded from the console's origin.
async fn open_console(app: &axum::Router) -> Console {
    let code = crate::console_session::launch_code(ADMIN_TOKEN);
    let response = trade_code(app, &code, &[("origin", OWNER_ORIGIN)]).await;
    assert_eq!(response.status(), StatusCode::OK);
    let set_cookie = response.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .to_string();
    for attribute in ["HttpOnly", "SameSite=Strict", "Path=/api/system"] {
        assert!(set_cookie.contains(attribute), "{set_cookie}");
    }
    // A browser-session cookie: none of it is meant to reach the disk.
    assert!(!set_cookie.contains("Max-Age"), "{set_cookie}");
    assert!(!set_cookie.contains("Expires"), "{set_cookie}");
    assert!(
        set_cookie.starts_with("anima_console_8787="),
        "{set_cookie}"
    );
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    Console {
        cookie: set_cookie.split(';').next().unwrap().to_string(),
        token: body["token"].as_str().unwrap().to_string(),
    }
}

fn idle() -> Value {
    serde_json::json!({
        "chatRuns": 0, "queuedRuns": 0, "swarmRuns": 0, "helperRuns": 0,
        "jobs": 0, "backgroundProcesses": 0, "appSessions": 0, "acpSessions": 0,
        "memoryReviews": 0, "doctorRepairs": 0, "standingIntentChanges": 0, "executionJobs": 0, "mcpOperations": 0,
        "browserProfileLogins": 0,
    })
}

#[tokio::test]
async fn drain_routes_require_local_owner() {
    let app = app();
    let origin = [("origin", OWNER_ORIGIN)];
    let foreign = [("origin", "https://foreign.example.invalid")];
    let wrong_token = [("authorization", "Bearer wrong")];
    for (method, path) in [
        ("POST", "/api/system/drain"),
        ("GET", "/api/system/drain"),
        ("DELETE", "/api/system/drain"),
        ("POST", "/api/system/shutdown"),
    ] {
        for auth in [&[][..], &foreign[..], &wrong_token[..]] {
            let (status, _) = send(&app, method, path, auth).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{method} {path} {auth:?}");
        }
    }
    // An approved Origin alone, which any local process can send, may read
    // but not act.
    for (method, path) in [
        ("POST", "/api/system/drain"),
        ("DELETE", "/api/system/drain"),
        ("POST", "/api/system/shutdown"),
    ] {
        let (status, body) = send(&app, method, path, &origin).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {path}");
        assert_eq!(body["code"], "console_session_required", "{method} {path}");
    }
    let (_, body) = send(&app, "GET", "/api/system/drain", &origin).await;
    assert_eq!(body["draining"], false, "a refused begin changes nothing");

    let console = open_console(&app).await;
    let session = console.headers();
    let (status, body) = send(&app, "POST", "/api/system/drain", &session).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(
        body,
        serde_json::json!({"draining": true, "inFlight": idle()})
    );

    let (status, body) = send(&app, "GET", "/api/system/drain", &origin).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        serde_json::json!({"draining": true, "inFlight": idle()})
    );

    let (status, _) = send(&app, "DELETE", "/api/system/drain", &session).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, body) = send(&app, "GET", "/api/system/drain", &origin).await;
    assert_eq!(body["draining"], false);
}

#[tokio::test]
async fn drain_routes_accept_the_supervisor_admin_token() {
    let app = app();
    let token = format!("Bearer {ADMIN_TOKEN}");
    let admin = [("authorization", token.as_str())];

    let (status, body) = send(&app, "POST", "/api/system/drain", &admin).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(body["draining"], true);
    let (status, body) = send(&app, "GET", "/api/system/drain", &admin).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["inFlight"], idle());
    let (status, _) = send(&app, "DELETE", "/api/system/drain", &admin).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn a_drain_refuses_new_work_with_503() {
    let app = app();
    let console = open_console(&app).await;
    let (status, _) = send(&app, "POST", "/api/system/drain", &console.headers()).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    for (path, body) in [
        ("/api/agents/missing/run", "{}"),
        ("/api/agents/missing/messages", "{}"),
        ("/api/swarms/missing/run", "{}"),
        (
            "/api/agents/missing/jobs",
            r#"{"title":"t","prompt":"p","requestKey":"k"}"#,
        ),
        (
            "/api/goals",
            r#"{"title":"t","objective":"o","requestKey":"k","maxAttempts":1}"#,
        ),
    ] {
        let request = Request::builder()
            .method("POST")
            .uri(path)
            .header("host", "127.0.0.1:8787")
            .header("origin", OWNER_ORIGIN)
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{path} {body}");
        assert_eq!(
            body["error"], "Anima is restarting; send the message again in a moment",
            "{path}"
        );
    }
}

#[tokio::test]
async fn a_drain_refuses_run_now_with_503() {
    let (app, scheduler, state) = app_with_scheduler();
    let agent_id = state
        .write()
        .await
        .create_agent(crate::agent_runs::test_support::companion_config("drain"))
        .unwrap()
        .state
        .id;
    let (schedule, _) = scheduler
        .create(
            agent_id.clone(),
            "Check status".into(),
            crate::schedules::ScheduleTrigger::Interval {
                interval_ms: 60_000,
            },
            crate::schedules::ScheduleTarget::Workspace,
            true,
            None,
            None,
            None,
        )
        .await
        .unwrap();
    let origin = [("origin", OWNER_ORIGIN)];
    let console = open_console(&app).await;
    let (status, _) = send(&app, "POST", "/api/system/drain", &console.headers()).await;
    assert_eq!(status, StatusCode::ACCEPTED);

    let path = format!("/api/agents/{agent_id}/schedules/{}/run", schedule.id);
    let (status, body) = send(&app, "POST", &path, &origin).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(body["code"], "schedule_draining");
    assert_eq!(
        body["error"],
        "Anima is restarting; send the message again in a moment"
    );
    assert_eq!(state.read().await.schedules[&schedule.id], schedule);
}

#[tokio::test]
async fn shutdown_route_stops_server() {
    // This server's policy comes from the environment. A token no other test
    // presents leaves their policies' answers as they were.
    let token = format!("shutdown-test-{}", uuid::Uuid::new_v4());
    std::env::set_var("ANIMA_LOCAL_ADMIN_TOKEN", &token);
    let state = Arc::new(RwLock::new(DaemonState::new()));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    // No signal ever arrives: only the route can stop this server.
    let server = tokio::spawn(crate::app::serve_with_state(
        listener,
        state,
        DaemonConfig::default(),
        std::future::pending(),
    ));

    let post = |auth: String| async move {
        let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
        let request = format!(
            "POST /api/system/shutdown HTTP/1.1\r\nHost: {address}\r\n{auth}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut received = Vec::new();
        tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut received))
            .await
            .expect("the shutdown response arrives")
            .unwrap();
        String::from_utf8_lossy(&received).into_owned()
    };
    // A forged Origin, as an agent's `curl` could send, does nothing.
    let received = post(format!("Origin: {OWNER_ORIGIN}")).await;
    assert!(received.starts_with("HTTP/1.1 403"), "{received}");
    let received = post(format!("Authorization: Bearer {token}")).await;
    std::env::remove_var("ANIMA_LOCAL_ADMIN_TOKEN");
    assert!(received.starts_with("HTTP/1.1 202"), "{received}");

    tokio::time::timeout(crate::app::lifecycle::SHUTDOWN_BUDGET, server)
        .await
        .expect("the server stops within the shutdown budget")
        .unwrap()
        .unwrap();
}

const SUPERVISOR_TOKEN: &str = "supervisor-control";

/// What the fake supervisor saw: method, path, `Authorization`, body.
type Seen = Arc<std::sync::Mutex<Vec<(String, String, Option<String>, String)>>>;

/// A stand-in for the supervisor's control API on `127.0.0.1:0`. `update`
/// answers `409`, everything else `202`; status answers its own JSON.
async fn fake_supervisor() -> (String, Seen) {
    use axum::extract::Path;
    use axum::http::HeaderMap;
    use axum::routing::{get, post};

    fn record(seen: &Seen, method: &str, path: String, headers: &HeaderMap, body: String) {
        let auth = headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        seen.lock()
            .unwrap()
            .push((method.to_string(), path, auth, body));
    }

    let seen = Seen::default();
    let status_seen = Arc::clone(&seen);
    let action_seen = Arc::clone(&seen);
    let router = axum::Router::new()
        .route(
            "/v1/status",
            get(move |headers: HeaderMap| async move {
                record(
                    &status_seen,
                    "GET",
                    "/v1/status".into(),
                    &headers,
                    String::new(),
                );
                axum::Json(serde_json::json!({"state": "running", "version": "9.9.9"}))
            }),
        )
        .route(
            "/v1/actions/{action}",
            post(
                move |Path(action): Path<String>, headers: HeaderMap, body: String| async move {
                    record(
                        &action_seen,
                        "POST",
                        format!("/v1/actions/{action}"),
                        &headers,
                        body,
                    );
                    if action == "update" {
                        (
                            StatusCode::CONFLICT,
                            axum::Json(serde_json::json!({"error": "already updating"})),
                        )
                    } else {
                        (
                            StatusCode::ACCEPTED,
                            axum::Json(serde_json::json!({"accepted": action})),
                        )
                    }
                },
            ),
        );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, router).await });
    (url, seen)
}

fn supervised(url: &str) -> axum::Router {
    app_with_supervisor(crate::system::SupervisorRelay::new(Some((
        url.to_string(),
        SUPERVISOR_TOKEN.to_string(),
    ))))
    .0
}

fn json_body(value: &str) -> Body {
    Body::from(value.to_string())
}

#[tokio::test]
async fn status_proxies_with_bearer() {
    let (url, seen) = fake_supervisor().await;
    // A trailing slash on the configured URL is harmless.
    let app = supervised(&format!("{url}/"));
    let (status, body) = send(&app, "GET", "/api/system", &[("origin", OWNER_ORIGIN)]).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        serde_json::json!({"state": "running", "version": "9.9.9"})
    );
    assert_eq!(
        *seen.lock().unwrap(),
        vec![(
            "GET".to_string(),
            "/v1/status".to_string(),
            Some(format!("Bearer {SUPERVISOR_TOKEN}")),
            String::new(),
        )]
    );
    let (status, _) = send(&app, "GET", "/api/system", &[]).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "reads need the local owner");
    assert_eq!(
        seen.lock().unwrap().len(),
        1,
        "a refused read is not relayed"
    );
}

#[tokio::test]
async fn status_unsupervised_reports_version() {
    let app = app();
    let (status, body) = send(&app, "GET", "/api/system", &[("origin", OWNER_ORIGIN)]).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        serde_json::json!({"supervised": false, "version": env!("CARGO_PKG_VERSION")})
    );
    // The exact path does not shadow the drain routes.
    let (status, body) = send(
        &app,
        "GET",
        "/api/system/drain",
        &[("origin", OWNER_ORIGIN)],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["draining"], false);
}

#[tokio::test]
async fn action_proxied_with_force() {
    let (url, seen) = fake_supervisor().await;
    let app = supervised(&url);
    let console = open_console(&app).await;
    let origin = console.headers();

    let (status, body) = send_body(
        &app,
        "POST",
        "/api/system/actions/restart",
        &origin,
        json_body(r#"{"force":true}"#),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(body, serde_json::json!({"accepted": "restart"}));

    // No body: force is false. The supervisor's own refusal passes through.
    let (status, body) = send(&app, "POST", "/api/system/actions/update", &origin).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body, serde_json::json!({"error": "already updating"}));

    let token = format!("Bearer {ADMIN_TOKEN}");
    let (status, _) = send(
        &app,
        "POST",
        "/api/system/actions/check-update",
        &[("authorization", token.as_str())],
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "the admin token may also act");

    let seen = seen.lock().unwrap();
    let bearer = Some(format!("Bearer {SUPERVISOR_TOKEN}"));
    let relayed: Vec<_> = seen
        .iter()
        .map(|(method, path, auth, body)| {
            let force = serde_json::from_str::<Value>(body).unwrap()["force"].clone();
            (method.as_str(), path.as_str(), auth.clone(), force)
        })
        .collect();
    assert_eq!(
        relayed,
        vec![
            (
                "POST",
                "/v1/actions/restart",
                bearer.clone(),
                Value::Bool(true)
            ),
            (
                "POST",
                "/v1/actions/update",
                bearer.clone(),
                Value::Bool(false)
            ),
            (
                "POST",
                "/v1/actions/check-update",
                bearer,
                Value::Bool(false)
            ),
        ]
    );
}

#[tokio::test]
async fn unknown_action_404() {
    let (url, seen) = fake_supervisor().await;
    let app = supervised(&url);
    let console = open_console(&app).await;
    for action in ["reboot", "drain", "START", "shutdown"] {
        let (status, body) = send(
            &app,
            "POST",
            &format!("/api/system/actions/{action}"),
            &console.headers(),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{action}");
        assert!(body["error"].is_string());
    }
    assert!(
        seen.lock().unwrap().is_empty(),
        "nothing unknown is relayed"
    );
}

#[tokio::test]
async fn action_requires_owner() {
    let (url, seen) = fake_supervisor().await;
    let app = supervised(&url);
    let foreign = [("origin", "https://foreign.example.invalid")];
    let wrong_token = [("authorization", "Bearer wrong")];
    // The supervisor's own token is not the daemon's admin token.
    let supervisor_token = format!("Bearer {SUPERVISOR_TOKEN}");
    let supervisor_token = [("authorization", supervisor_token.as_str())];
    for auth in [
        &[][..],
        &foreign[..],
        &wrong_token[..],
        &supervisor_token[..],
    ] {
        let (status, _) = send(&app, "POST", "/api/system/actions/stop", auth).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{auth:?}");
    }
    assert!(
        seen.lock().unwrap().is_empty(),
        "a refused action is not relayed"
    );
}

#[tokio::test]
async fn action_rejects_a_malformed_body() {
    let (url, seen) = fake_supervisor().await;
    let app = supervised(&url);
    let console = open_console(&app).await;
    for body in [
        r#"{"force":"yes"}"#,
        "not json",
        r#"{"force":true,"other":1}"#,
    ] {
        let (status, _) = send_body(
            &app,
            "POST",
            "/api/system/actions/stop",
            &console.headers(),
            json_body(body),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    }
    assert!(seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn action_unsupervised_409() {
    let app = app();
    let console = open_console(&app).await;
    let (status, body) = send(&app, "POST", "/api/system/actions/stop", &console.headers()).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        body,
        serde_json::json!({"error": "Anima is not running under the supervisor"})
    );
}

#[tokio::test]
async fn supervisor_down_502() {
    // A port that was open a moment ago and is not any more.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let app = supervised(&url);
    let origin = [("origin", OWNER_ORIGIN)];
    let expected = serde_json::json!({"error": "The Anima supervisor is not responding"});

    let (status, body) = send(&app, "GET", "/api/system", &origin).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(body, expected);

    let console = open_console(&app).await;
    let (status, body) = send(
        &app,
        "POST",
        "/api/system/actions/restart",
        &console.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(body, expected);
}

#[tokio::test]
async fn only_a_launch_code_opens_a_console_session() {
    let app = app();
    let origin = [("origin", OWNER_ORIGIN)];
    let status = |response: axum::response::Response| response.status();

    // Not from a browser, from a foreign origin, or forwarded: refused, and
    // the code stays unspent.
    let code = crate::console_session::launch_code(ADMIN_TOKEN);
    for auth in [
        &[][..],
        &[("origin", "https://foreign.example.invalid")][..],
        &[("origin", OWNER_ORIGIN), ("x-forwarded-for", "10.0.0.1")][..],
        &[("authorization", "Bearer supervisor-admin")][..],
    ] {
        let response = trade_code(&app, &code, auth).await;
        assert_eq!(status(response), StatusCode::FORBIDDEN, "{auth:?}");
    }
    assert_eq!(
        status(trade_code(&app, &code, &origin).await),
        StatusCode::OK
    );
    // A second trade says so, so the owner learns someone else may hold
    // the session.
    let (status_code, body) = {
        let response = trade_code(&app, &code, &origin).await;
        let status_code = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (
            status_code,
            serde_json::from_slice::<Value>(&bytes).unwrap(),
        )
    };
    assert_eq!(
        status_code,
        StatusCode::CONFLICT,
        "a code opens one session"
    );
    assert_eq!(body["code"], "launch_code_spent");
    assert!(body["error"].as_str().unwrap().contains("already used"));
    // Codes nobody signed with this admin token.
    for code in [
        crate::console_session::launch_code("another-admin-token"),
        "1.2.3".to_string(),
        String::new(),
    ] {
        let response = trade_code(&app, &code, &origin).await;
        assert_eq!(status(response), StatusCode::FORBIDDEN, "{code}");
    }
}

/// What an agent's shell can send without the owner: any headers, any path
/// spelling, and whatever it could scrape or lure out of the browser. None
/// of it stops, updates or drains Anima, and nothing reaches the supervisor.
#[tokio::test]
async fn forged_lifecycle_requests_are_refused() {
    let (url, seen) = fake_supervisor().await;
    let app = supervised(&url);
    let console = open_console(&app).await;
    let other_port = console
        .cookie
        .replacen("anima_console_8787=", "anima_console_9999=", 1);
    let forged: [&[(&str, &str)]; 7] = [
        // `curl -H 'Origin: http://127.0.0.1:8787' …`
        &[("origin", OWNER_ORIGIN)],
        // …dressed as a same-origin browser fetch.
        &[
            ("origin", OWNER_ORIGIN),
            ("sec-fetch-site", "same-origin"),
            ("referer", "http://localhost:4200/"),
        ],
        // The token half scraped from the browser profile, without the cookie.
        &[
            ("origin", OWNER_ORIGIN),
            ("x-anima-console-session", console.token.as_str()),
        ],
        // The cookie half lured to a page on another port, without the token.
        &[
            ("origin", OWNER_ORIGIN),
            ("cookie", console.cookie.as_str()),
        ],
        // One half passed off as the other.
        &[
            ("origin", OWNER_ORIGIN),
            ("cookie", console.cookie.as_str()),
            ("x-anima-console-session", console.cookie.as_str()),
        ],
        // A cookie this daemon's port didn't set.
        &[
            ("origin", OWNER_ORIGIN),
            ("cookie", other_port.as_str()),
            ("x-anima-console-session", console.token.as_str()),
        ],
        // The session, from a page that isn't the console.
        &[
            ("origin", "http://127.0.0.1:9999"),
            ("cookie", console.cookie.as_str()),
            ("x-anima-console-session", console.token.as_str()),
        ],
    ];
    for auth in forged {
        for (method, path) in [
            ("POST", "/api/system/actions/quit"),
            ("POST", "/api/system/actions/update"),
            ("POST", "/api/system/drain"),
            ("DELETE", "/api/system/drain"),
            ("POST", "/api/system/shutdown"),
        ] {
            let (status, _) = send(&app, method, path, auth).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{method} {path} {auth:?}");
        }
    }
    // Path spellings `curl --path-as-is` sends unchanged don't reach a route.
    for path in [
        "/api/./system/actions/quit",
        "/api//system/actions/quit",
        "/api/x/../system/actions/quit",
        "/api/%73ystem/actions/quit",
    ] {
        let request = Request::builder()
            .method("POST")
            .uri(path)
            .header("host", "127.0.0.1:8787")
            .header("origin", OWNER_ORIGIN)
            .body(Body::empty())
            .unwrap();
        let status = app.clone().oneshot(request).await.unwrap().status();
        assert!(!status.is_success(), "{path} {status}");
    }
    assert!(seen.lock().unwrap().is_empty(), "nothing was relayed");
    let (_, body) = send(
        &app,
        "GET",
        "/api/system/drain",
        &[("origin", OWNER_ORIGIN)],
    )
    .await;
    assert_eq!(body["draining"], false, "nothing drained");

    // The console itself still acts.
    let (status, _) = send(&app, "POST", "/api/system/actions/quit", &console.headers()).await;
    assert_eq!(status, StatusCode::ACCEPTED);
}

#[test]
fn a_relay_needs_an_http_url() {
    let off = |url: &str| {
        !crate::system::SupervisorRelay::new(Some((url.to_string(), "token".to_string())))
            .is_supervised()
    };
    assert!(off("not a url"));
    assert!(off("file:///tmp/supervisor.sock"));
    assert!(!off("http://127.0.0.1:8788"));
    assert!(!crate::system::SupervisorRelay::new(None).is_supervised());
}
