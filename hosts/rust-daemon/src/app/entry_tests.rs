use super::*;
use std::time::Duration;

#[test]
fn max_runs_per_agent_above_64_is_rejected() {
    let error = validate_max_runs_per_agent(65).expect_err("65 exceeds the limit");
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    assert_eq!(
        error.to_string(),
        "ANIMAOS_RS_MAX_RUNS_PER_AGENT must be between 1 and 64"
    );
}

#[test]
fn max_runs_per_agent_at_or_below_64_is_accepted() {
    assert_eq!(validate_max_runs_per_agent(64).unwrap(), 64);
    assert_eq!(validate_max_runs_per_agent(1).unwrap(), 1);
}

#[test]
fn a_session_event_buffer_outside_1_to_65536_is_rejected() {
    for value in [0, MAX_SESSION_EVENT_BUFFER + 1] {
        let error = validate_session_event_buffer(value).expect_err("outside the range");
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(
            error.to_string(),
            "ANIMAOS_RS_SESSION_EVENT_BUFFER must be between 1 and 65536"
        );
    }
}

#[test]
fn a_session_event_buffer_from_1_to_65536_is_accepted() {
    assert_eq!(MAX_SESSION_EVENT_BUFFER, 65_536);
    for value in [
        1,
        DaemonConfig::default().session_event_buffer,
        MAX_SESSION_EVENT_BUFFER,
    ] {
        assert_eq!(validate_session_event_buffer(value).unwrap(), value);
    }
}

async fn http_get(address: std::net::SocketAddr, path: &str) -> (u16, String) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    let request = format!("GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut raw = String::new();
    tokio::time::timeout(Duration::from_secs(5), stream.read_to_string(&mut raw))
        .await
        .expect("the response arrives")
        .unwrap();
    let status = raw
        .strip_prefix("HTTP/1.1 ")
        .and_then(|rest| rest.get(..3))
        .and_then(|code| code.parse().ok())
        .unwrap_or_else(|| panic!("unexpected response: {raw}"));
    let body = raw
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .unwrap_or("");
    (status, body.to_string())
}

#[tokio::test(flavor = "multi_thread")]
async fn ui_fallback_serves_unmatched_paths() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let shutdown = Arc::new(Notify::new());
    let server = tokio::spawn(serve_with(
        listener,
        DaemonConfig::default(),
        ServeOptions {
            ui: Some(Router::new().fallback(|| async { "UI" })),
            shutdown: Some(Arc::clone(&shutdown)),
        },
    ));

    let (status, body) = http_get(address, "/").await;
    assert_eq!((status, body.as_str()), (200, "UI"));

    let (status, body) = http_get(address, "/health").await;
    assert_eq!(status, 200);
    assert!(
        body.contains(r#""status":"ok""#),
        "the daemon's health JSON still wins over the UI: {body}"
    );

    shutdown.notify_one();
    tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .expect("the server stops")
        .unwrap()
        .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn shutdown_notify_stops_server() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let shutdown = Arc::new(Notify::new());
    let server = tokio::spawn(serve_with(
        listener,
        DaemonConfig::default(),
        ServeOptions {
            ui: None,
            shutdown: Some(Arc::clone(&shutdown)),
        },
    ));

    shutdown.notify_one();

    let outcome = tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .expect("serve_with returns once the shutdown handle is notified")
        .expect("the server task does not panic");
    assert!(outcome.is_ok());
}

#[tokio::test]
async fn an_invalid_keychain_namespace_fails_startup_before_binding() {
    // An invalid value is ignored by `service_name`, so no other test is
    // affected by this process-wide variable.
    std::env::set_var("ANIMAOS_RS_KEYCHAIN_NAMESPACE", "Not Valid");
    let result = run_from_env(ServeOptions::default()).await;
    std::env::remove_var("ANIMAOS_RS_KEYCHAIN_NAMESPACE");

    let error = result.expect_err("an invalid namespace must stop startup");
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    assert_eq!(
        error.to_string(),
        "ANIMAOS_RS_KEYCHAIN_NAMESPACE must be 1-32 of a-z, 0-9, -"
    );
}
