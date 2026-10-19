//! The drain and shutdown routes a supervisor (admin token) and the console
//! (allowed Origin) call.
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
    super::router_with_services_with_policies(
        state,
        DaemonConfig::default(),
        limiter,
        runs,
        connectors,
        calendar,
        mail,
        oauth_apps,
        scheduler,
        jobs,
        crate::history::HistoryWorkerOwner::new(),
        crate::logs::global(),
        LocalOwnerPolicy::for_test(true, Some(ADMIN_TOKEN)),
        ApiKeyPolicy::for_test(None),
    )
}

async fn send(
    app: &axum::Router,
    method: &str,
    path: &str,
    auth: &[(&str, &str)],
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
        .oneshot(builder.body(Body::empty()).unwrap())
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

fn idle() -> Value {
    serde_json::json!({
        "chatRuns": 0, "queuedRuns": 0, "swarmRuns": 0, "helperRuns": 0,
        "jobs": 0, "backgroundProcesses": 0, "appSessions": 0, "acpSessions": 0,
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
    let (_, body) = send(&app, "GET", "/api/system/drain", &origin).await;
    assert_eq!(body["draining"], false, "a refused begin changes nothing");

    let (status, body) = send(&app, "POST", "/api/system/drain", &origin).await;
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

    let (status, _) = send(&app, "DELETE", "/api/system/drain", &origin).await;
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
    let (status, _) = send(
        &app,
        "POST",
        "/api/system/drain",
        &[("origin", OWNER_ORIGIN)],
    )
    .await;
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
async fn shutdown_route_stops_server() {
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

    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    let request = format!(
        "POST /api/system/shutdown HTTP/1.1\r\nHost: {address}\r\nOrigin: {OWNER_ORIGIN}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut received = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut received))
        .await
        .expect("the shutdown response arrives")
        .unwrap();
    let received = String::from_utf8_lossy(&received);
    assert!(received.starts_with("HTTP/1.1 202"), "{received}");

    tokio::time::timeout(crate::app::lifecycle::SHUTDOWN_BUDGET, server)
        .await
        .expect("the server stops within the shutdown budget")
        .unwrap()
        .unwrap();
}
