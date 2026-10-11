use super::*;
use axum::{
    body::{to_bytes, Body},
    extract::State,
    http::{Request, StatusCode},
    response::Response,
    routing::any,
    Router,
};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

type Call = (String, String, String);
struct Fixture {
    responses: Mutex<VecDeque<(u16, Vec<u8>, Vec<(String, String)>)>>,
    calls: Mutex<Vec<Call>>,
}
async fn handler(State(fixture): State<Arc<Fixture>>, request: Request<Body>) -> Response {
    let path = request.uri().to_string();
    let auth = request
        .headers()
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("")
        .to_owned();
    let bytes = to_bytes(request.into_body(), 128 * 1024).await.unwrap();
    fixture
        .calls
        .lock()
        .unwrap()
        .push((path, auth, String::from_utf8(bytes.to_vec()).unwrap()));
    let (status, body, headers) = fixture
        .responses
        .lock()
        .unwrap()
        .pop_front()
        .expect("unexpected provider call");
    let mut response = Response::builder().status(StatusCode::from_u16(status).unwrap());
    for (name, value) in headers {
        response = response.header(name, value);
    }
    // Unknown streamed length exercises the real cap, rather than only Content-Length.
    let chunks = body
        .chunks(8192)
        .map(|chunk| Ok::<_, std::io::Error>(chunk.to_vec()))
        .collect::<Vec<_>>();
    response
        .body(Body::from_stream(futures::stream::iter(chunks)))
        .unwrap()
}
async fn fixture(
    responses: Vec<(u16, Vec<u8>, Vec<(String, String)>)>,
) -> (DriveClient, Arc<Fixture>, tokio::task::JoinHandle<()>) {
    let fixture = Arc::new(Fixture {
        responses: Mutex::new(responses.into()),
        calls: Mutex::new(Vec::new()),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = Router::new()
        .fallback(any(handler))
        .with_state(fixture.clone());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (
        DriveClient::for_test_base(
            &format!("http://{addr}/drive/v3/"),
            &format!("http://{addr}/token"),
        )
        .unwrap(),
        fixture,
        task,
    )
}
fn json(value: serde_json::Value) -> (u16, Vec<u8>, Vec<(String, String)>) {
    (200, value.to_string().into_bytes(), Vec::new())
}
fn metadata(id: &str, mime: &str) -> serde_json::Value {
    serde_json::json!({"id":id,"name":"selected","mimeType":mime,"version":"9007199254740993","modifiedTime":"2026-10-12T01:00:00Z","size":"8","trashed":false,"capabilities":{"canDownload":true},"webContentLink":"http://127.0.0.1:1/never-fetch"})
}
fn config() -> DriveOAuthConfig {
    DriveOAuthConfig::new(
        "client".into(),
        Zeroizing::new("secret-sentinel".into()),
        "http://127.0.0.1:8080/api/connectors/gdrive/callback".into(),
        8,
    )
    .unwrap()
}
#[tokio::test]
async fn drive_exact_metadata_and_docs_media_branches_use_host_owned_paths() {
    for (mime, expected, representation) in [
        (
            "application/vnd.google-apps.document",
            "/files/doc_1/export?mimeType=text%2Fplain",
            "google_docs_plain_text",
        ),
        ("text/plain", "/files/doc_1?alt=media", "stored_plain_text"),
    ] {
        let (client, capture, task) = fixture(vec![
            json(metadata("doc_1", mime)),
            (200, "🦀 selected".as_bytes().to_vec(), Vec::new()),
        ])
        .await;
        let meta = client
            .metadata("access-sentinel", &DriveFileId::parse("doc_1").unwrap())
            .await
            .unwrap();
        assert_eq!(
            meta.source_url,
            "https://drive.google.com/file/d/doc_1/view"
        );
        assert_eq!(meta.version, "9007199254740993");
        let text = client.read_text("access-sentinel", &meta).await.unwrap();
        assert_eq!(text.bytes, "🦀 selected".as_bytes());
        assert_eq!(text.representation, representation);
        let calls = capture.calls.lock().unwrap();
        assert!(calls[0].0.starts_with("/drive/v3/files/doc_1?fields="));
        assert!(!calls[0].0.contains("owners"));
        assert!(calls[1].0.ends_with(expected));
        assert!(calls.iter().all(|c| c.1 == "Bearer access-sentinel"));
        assert!(!calls.iter().any(|c| c.0.contains("sentinel")));
        task.abort();
    }
}
#[tokio::test]
async fn drive_rejects_mismatched_metadata_and_unsupported_denied_content_before_fetch() {
    let (client, capture, task) = fixture(vec![
        json(metadata("wrong", "text/plain")),
        json(metadata("selected", "application/pdf")),
    ])
    .await;
    let id = DriveFileId::parse("selected").unwrap();
    assert_eq!(
        client.metadata("access", &id).await.unwrap_err(),
        DriveError::InvalidResponse
    );
    let mut meta = client.metadata("access", &id).await.unwrap();
    assert_eq!(
        client.read_text("access", &meta).await.unwrap_err(),
        DriveError::InvalidInput
    );
    meta.mime_type = "text/plain".into();
    meta.can_download = false;
    assert_eq!(
        client.read_text("access", &meta).await.unwrap_err(),
        DriveError::PermissionDenied
    );
    meta.can_download = true;
    meta.trashed = true;
    assert_eq!(
        client.read_text("access", &meta).await.unwrap_err(),
        DriveError::NotFound
    );
    assert_eq!(capture.calls.lock().unwrap().len(), 2);
    task.abort();
}
#[tokio::test]
async fn drive_oauth_requires_initial_scope_rejects_reduced_refresh_and_preserves_omissions() {
    for scope in [None, Some("openid")] {
        let mut value = serde_json::json!({"access_token":"access","refresh_token":"refresh","expires_in":3600});
        if let Some(scope) = scope {
            value["scope"] = serde_json::json!(scope);
        }
        let (client, _, task) = fixture(vec![json(value)]).await;
        assert_eq!(
            client
                .exchange_code(&config(), "code", &"a".repeat(64))
                .await
                .unwrap_err(),
            DriveError::ReauthRequired
        );
        task.abort();
    }
    let (client,capture,task)=fixture(vec![json(serde_json::json!({"access_token":"access","refresh_token":"refresh","expires_in":3600,"scope":DRIVE_SCOPE})),json(serde_json::json!({"access_token":"replacement","expires_in":3600})),json(serde_json::json!({"access_token":"reduced","expires_in":3600,"scope":"openid"}))]).await;
    let grant = client
        .exchange_code(&config(), "code", &"a".repeat(64))
        .await
        .unwrap();
    assert_eq!(grant.granted_scopes, vec![DRIVE_SCOPE]);
    let refreshed = client
        .refresh_tokens(&config(), grant.tokens.refresh_token())
        .await
        .unwrap();
    assert_eq!(refreshed.tokens.refresh_token(), "refresh");
    assert!(refreshed.granted_scopes.is_empty());
    assert_eq!(
        client
            .refresh_tokens(&config(), "refresh")
            .await
            .unwrap_err(),
        DriveError::ReauthRequired
    );
    let calls = capture.calls.lock().unwrap();
    assert!(calls[0].2.contains("code_verifier="));
    assert!(calls[0].2.contains("redirect_uri="));
    assert!(!calls[0].0.contains("secret"));
    assert!(!format!("{grant:?}").contains("refresh"));
    task.abort();
}
#[tokio::test]
async fn drive_identify_requires_authentic_me_and_opaque_bounded_identity() {
    for value in [
        serde_json::json!({"user":{"permissionId":"opaque:account/42","me":true,"displayName":"Owner"}}),
        serde_json::json!({"user":{"permissionId":"account","me":false}}),
        serde_json::json!({"user":{"me":true}}),
    ] {
        let okay = value["user"]["permissionId"] == "opaque:account/42";
        let (client, _, task) = fixture(vec![json(value)]).await;
        let result = client.identify("access").await;
        if okay {
            assert_eq!(result.unwrap().permission_id, "opaque:account/42");
        } else {
            assert_eq!(result.unwrap_err(), DriveError::InvalidResponse);
        }
        task.abort();
    }
}
#[tokio::test]
async fn drive_response_caps_redirects_utf8_and_provider_errors_are_static() {
    for (status, body, headers, error) in [
        (
            302,
            Vec::new(),
            vec![("location".into(), "http://127.0.0.1:1/secret".into())],
            DriveError::InvalidResponse,
        ),
        (
            200,
            vec![b'x'; METADATA_LIMIT + 1],
            Vec::new(),
            DriveError::TooLarge,
        ),
        (
            401,
            b"secret upstream body".to_vec(),
            Vec::new(),
            DriveError::ReauthRequired,
        ),
        (
            403,
            b"secret upstream body".to_vec(),
            Vec::new(),
            DriveError::PermissionDenied,
        ),
    ] {
        let (client, capture, task) = fixture(vec![(status, body, headers)]).await;
        let observed = client.identify("access").await.unwrap_err();
        assert_eq!(observed, error);
        assert!(!format!("{observed:?} {observed}").contains("secret"));
        assert_eq!(capture.calls.lock().unwrap().len(), 1);
        task.abort();
    }
    for (body, error) in [
        (vec![0xff], DriveError::InvalidUtf8),
        (vec![b'x'; CONTENT_LIMIT + 1], DriveError::TooLarge),
    ] {
        let (client, _, task) = fixture(vec![
            json(metadata("selected", "text/plain")),
            (200, body, Vec::new()),
        ])
        .await;
        let meta = client
            .metadata("access", &DriveFileId::parse("selected").unwrap())
            .await
            .unwrap();
        assert_eq!(client.read_text("access", &meta).await.unwrap_err(), error);
        task.abort();
    }
}
