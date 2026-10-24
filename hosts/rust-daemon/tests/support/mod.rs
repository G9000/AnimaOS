use anima_daemon::app as daemon_app;
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use axum::Router;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tower::util::ServiceExt;

fn workspace_root_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

pub(crate) fn test_app() -> Router {
    daemon_app()
}

pub(crate) async fn configured_loopback_test_app(
    config: anima_daemon::DaemonConfig,
) -> std::io::Result<Router> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    anima_daemon::app_with_configured_persistence_for_listener(config, &listener).await
}

/// Read the deterministic owner credential supplied by the Rust test runner.
pub(crate) fn test_owner_token() -> String {
    let token = std::env::var("ANIMAOS_TEST_OWNER_TOKEN")
        .unwrap_or_else(|_| panic!("test runner must set ANIMAOS_TEST_OWNER_TOKEN"));
    assert!(
        !token.is_empty(),
        "test runner must set a non-empty ANIMAOS_TEST_OWNER_TOKEN"
    );
    token
}

/// Attach owner credentials explicitly so raw requests still test rejection paths.
pub(crate) fn owner_request(builder: axum::http::request::Builder) -> axum::http::request::Builder {
    let token = test_owner_token();
    builder
        .header("host", "127.0.0.1:8080")
        .header("authorization", format!("Bearer {token}"))
}

pub(crate) struct WorkspaceRootGuard {
    previous: Option<OsString>,
    path: PathBuf,
    _lock: MutexGuard<'static, ()>,
}

impl WorkspaceRootGuard {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for WorkspaceRootGuard {
    fn drop(&mut self) {
        if let Some(previous) = &self.previous {
            std::env::set_var("ANIMAOS_WORKSPACE_ROOT", previous);
        } else {
            std::env::remove_var("ANIMAOS_WORKSPACE_ROOT");
        }
        let _ = fs::remove_dir_all(&self.path);
    }
}

pub(crate) fn use_temp_workspace_root(prefix: &str) -> WorkspaceRootGuard {
    let lock = workspace_root_lock()
        .lock()
        .expect("workspace root lock should not be poisoned");
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should move forward")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("anima-daemon-{prefix}-{unique}"));
    fs::create_dir_all(&path).expect("workspace dir should be created");

    let previous = std::env::var_os("ANIMAOS_WORKSPACE_ROOT");
    std::env::set_var("ANIMAOS_WORKSPACE_ROOT", &path);

    WorkspaceRootGuard {
        previous,
        path,
        _lock: lock,
    }
}

pub(crate) async fn send_request(app: &Router, request: Request<Body>) -> (StatusCode, String) {
    let response = app.clone().oneshot(request).await.expect("app responds");
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body reads");
    (
        status,
        std::str::from_utf8(&body)
            .expect("body is utf-8")
            .to_string(),
    )
}

pub(crate) async fn send_json_request(
    app: &Router,
    method: &str,
    uri: &str,
    body: &str,
) -> (StatusCode, String) {
    send_request(
        app,
        owner_request(Request::builder())
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_owned()))
            .expect("request builds"),
    )
    .await
}

pub(crate) async fn send_empty_request(
    app: &Router,
    method: &str,
    uri: &str,
) -> (StatusCode, String) {
    send_request(
        app,
        owner_request(Request::builder())
            .method(method)
            .uri(uri)
            .body(Body::empty())
            .expect("request builds"),
    )
    .await
}

/// Build a GET URI with one query parameter, percent-encoding the value
/// (Windows paths contain `:` and `\`, which must be encoded).
pub(crate) fn query_uri(path: &str, param: &str, value: &str) -> String {
    let encoded: String = value
        .chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            other => format!("%{:02X}", other as u32),
        })
        .collect();
    format!("{path}?{param}={encoded}")
}

pub(crate) fn extract_json_string_field(response: &str, field: &str) -> String {
    let needle = format!("\"{field}\":\"");
    let start = response
        .find(&needle)
        .map(|index| index + needle.len())
        .expect("field should exist");
    let rest = &response[start..];
    let end = rest.find('"').expect("field should terminate");
    rest[..end].to_string()
}

#[allow(dead_code)]
pub(crate) fn extract_json_u64_field(response: &str, field: &str) -> u64 {
    let needle = format!("\"{field}\":");
    let start = response
        .find(&needle)
        .map(|index| index + needle.len())
        .expect("field should exist");
    let rest = &response[start..];
    let end = rest
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(rest.len());
    rest[..end]
        .parse::<u64>()
        .expect("field should be an unsigned integer")
}

#[allow(dead_code)]
pub(crate) fn extract_sse_event_data<'a>(stream: &'a str, event_name: &str) -> Option<&'a str> {
    let marker = format!("event: {event_name}\n");
    let start = stream.find(&marker)? + marker.len();
    let rest = &stream[start..];
    let data_marker = "data: ";
    let data_start = rest.find(data_marker)? + data_marker.len();
    let data = &rest[data_start..];
    let end = data.find("\n\n").unwrap_or(data.len());
    Some(&data[..end])
}

/// End all first-boot workers and blocking persistence before simulating a restart.
#[allow(dead_code)]
pub(crate) async fn run_in_joined_runtime<F, Fut, T>(make_work: F) -> T
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = T>,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test boot runtime should start");
        let result = runtime.block_on(make_work());
        drop(runtime);
        result
    })
    .await
    .expect("test boot runtime should fully shut down")
}
