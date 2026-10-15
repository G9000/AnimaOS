use super::models::{
    decode_models, ChatGptCatalogDispatch, ChatGptModelsAdapter, ChatGptModelsReason,
    ChatGptModelsStage, MODELS_ENDPOINT,
};
use serde_json::json;
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

#[test]
fn m49_chatgpt_models_visible_catalog_ranks_dedupes_and_preserves_exact_ids() {
    let body = br#"{"models":[
        {"slug":"synthetic-model-default","priority":null,"supported_in_api":false},
        {"slug":"synthetic-model-b","priority":2,"visibility":"list","supported_in_api":false},
        {"slug":"synthetic-model-a","priority":2},
        {"slug":"Owner/Model_A:v1.0","priority":-1},
        {"slug":"owner/model_a:v1.0","priority":-1},
        {"slug":"synthetic-model-b","priority":-2},
        {"slug":"synthetic-model-hidden","priority":-99,"visibility":"hidden"},
        {"slug":"synthetic-model-hide","priority":-99,"visibility":"hide"},
        {"slug":"synthetic-model-a","priority":-20,"visibility":"hidden"},
        {"slug":"synthetic-model-fallback","visibility":null,"description":{"ignored":true}}
    ],"unrelated":{"ignored":true}}"#;
    let models = decode_models(body).expect("a valid subscription catalog must decode");
    assert_eq!(
        models,
        [
            "synthetic-model-b",
            "Owner/Model_A:v1.0",
            "owner/model_a:v1.0",
            "synthetic-model-a",
            "synthetic-model-default",
            "synthetic-model-fallback"
        ]
    );
}

#[test]
fn m49_chatgpt_models_empty_and_fully_hidden_catalogs_are_successful_empty_results() {
    for body in [br#"{"models":[]}"#.as_slice(),
        br#"{"models":[{"slug":"synthetic-model-a","visibility":"hide"},{"slug":"synthetic-model-b","visibility":"hidden"}]}"#] {
        assert_eq!(decode_models(body).unwrap(), Vec::<String>::new());
    }
}

#[test]
fn m49_chatgpt_models_invalid_consumed_fields_reject_the_entire_catalog() {
    for row in [
        json!(null),
        json!("synthetic-model"),
        json!({}),
        json!({"slug":null}),
        json!({"slug":7}),
        json!({"slug":""}),
        json!({"slug":" synthetic-model"}),
        json!({"slug":"synthetic-model "}),
        json!({"slug":"synthetic\nmodel"}),
        json!({"slug":"synthetic-🟣"}),
        json!({"slug":"/synthetic-model"}),
        json!({"slug":"synthetic@model"}),
        json!({"slug":"A".repeat(129)}),
        json!({"slug":"synthetic-model","visibility":"LIST"}),
        json!({"slug":"synthetic-model","visibility":"unknown"}),
        json!({"slug":"synthetic-model","visibility":true}),
        json!({"slug":"synthetic-model","priority":"1"}),
        json!({"slug":"synthetic-model","priority":false}),
        json!({"slug":"synthetic-model","priority":1.25}),
        json!({"slug":"synthetic-model","priority":9007199254740992i64}),
        json!({"slug":"synthetic-model","priority":-9007199254740992i64}),
        json!({"slug":"synthetic-🟣","visibility":"hidden"}),
        json!({"slug":"synthetic-model","visibility":"hidden","priority":{}}),
    ] {
        let body = json!({"models":[{"slug":"synthetic-valid"},row]});
        assert_eq!(
            decode_models(&serde_json::to_vec(&body).unwrap()).unwrap_err(),
            "Model metadata response is invalid."
        );
    }
    for body in [
        b"".as_slice(),
        b"{",
        b"\xff",
        b"null",
        b"[]",
        b"{}",
        br#"{"models":null}"#,
        br#"{"models":{}}"#,
        br#"{"models":"synthetic-model"}"#,
    ] {
        assert!(decode_models(body).is_err());
    }
}

#[test]
fn m49_chatgpt_models_byte_row_id_and_priority_boundaries_are_exact() {
    let body = json!({"models":[
        {"slug":"synthetic-model-last","priority":9007199254740991i64},
        {"slug":"A".repeat(128),"priority":-9007199254740991i64}
    ]});
    assert_eq!(
        decode_models(&serde_json::to_vec(&body).unwrap()).unwrap(),
        vec!["A".repeat(128), "synthetic-model-last".to_owned()]
    );
    for count in [512, 513] {
        let rows = vec![json!({"slug":"synthetic-hidden","visibility":"hidden"}); count];
        let result = decode_models(&serde_json::to_vec(&json!({"models":rows})).unwrap());
        if count == 512 {
            assert_eq!(result.unwrap(), Vec::<String>::new());
        } else {
            assert!(
                result.is_err(),
                "row count precedes visibility and deduplication"
            );
        }
    }
    let mut body = br#"{"models":[]}"#.to_vec();
    body.resize(512 * 1024, b' ');
    assert_eq!(decode_models(&body).unwrap(), Vec::<String>::new());
    body.push(b' ');
    assert!(decode_models(&body).is_err());
}

struct CatalogPeer {
    origin: String,
    shutdown: tokio::sync::oneshot::Sender<()>,
    task: tokio::task::JoinHandle<()>,
}

impl CatalogPeer {
    async fn start(app: axum::Router) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let (shutdown, stopped) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
                .unwrap();
        });
        Self {
            origin,
            shutdown,
            task,
        }
    }
    fn adapter(&self) -> ChatGptModelsAdapter {
        ChatGptModelsAdapter::new("m49-private-token".into(), "m49-private-account".into())
            .unwrap()
            .at_test_origin(self.origin.clone(), Duration::from_secs(2))
            .unwrap()
    }
    async fn join(self) {
        let _ = self.shutdown.send(());
        let mut task = self.task;
        let joined = matches!(
            tokio::time::timeout(Duration::from_secs(2), &mut task).await,
            Ok(Ok(()))
        );
        if !joined {
            task.abort();
            let _ = task.await;
        }
        assert!(
            joined,
            "synthetic catalog peer must join before test assertions"
        );
    }
}

#[tokio::test]
async fn m50_catalog_diagnostics_http_403_retains_closed_response_evidence() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let peer = CatalogPeer::start(axum::Router::new().route(
        "/backend-api/codex/models",
        axum::routing::get({
            let attempts = attempts.clone();
            move || {
                let attempts = attempts.clone();
                async move {
                    attempts.fetch_add(1, Ordering::SeqCst);
                    (
                        axum::http::StatusCode::FORBIDDEN,
                        [("x-m50-private", "m50-private-header-marker")],
                        "m50-private-body-marker",
                    )
                }
            }
        }),
    ))
    .await;
    let adapter = ChatGptModelsAdapter::new_detailed(
        "m50-private-token-marker".into(),
        "m50-private-account-marker".into(),
    )
    .unwrap()
    .at_test_origin(peer.origin.clone(), Duration::from_secs(2))
    .unwrap();
    let result = adapter.fetch_detailed().await;
    peer.join().await;
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        1,
        "one controlled response, without retry"
    );
    let failure = result.unwrap_err();
    assert_eq!(failure.stage(), ChatGptModelsStage::HttpStatus);
    assert_eq!(
        failure.catalog_dispatch(),
        ChatGptCatalogDispatch::ResponseReceived
    );
    assert_eq!(failure.reason(), ChatGptModelsReason::HttpStatus);
    assert_eq!(failure.http_status(), Some(403));
    assert_eq!(failure.public_message(), "Model metadata is unavailable.");
    let safe = format!("{failure:?}|{failure}");
    assert!(
        !safe.contains("m50-private"),
        "closed diagnostics retain no synthetic sensitive marker"
    );
    assert!(
        !safe.contains("http://"),
        "closed diagnostics retain no endpoint"
    );
}

#[tokio::test]
async fn m49_chatgpt_models_fetch_uses_fixed_get_query_and_subscription_headers() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let captured = Arc::new(Mutex::new(None));
    let peer = CatalogPeer::start(axum::Router::new().route(
        "/backend-api/codex/models",
        axum::routing::any({
            let attempts = attempts.clone();
            let captured = captured.clone();
            move |method: axum::http::Method,
                  uri: axum::http::Uri,
                  headers: axum::http::HeaderMap| {
                let attempts = attempts.clone();
                let captured = captured.clone();
                async move {
                    attempts.fetch_add(1, Ordering::SeqCst);
                    *captured.lock().unwrap() = Some((method, uri, headers));
                    axum::Json(
                        json!({"models":[{"slug":"synthetic-model-a","supported_in_api":false}]}),
                    )
                }
            }
        }),
    ))
    .await;
    let result = peer.adapter().fetch().await;
    peer.join().await;
    assert_eq!(result.unwrap(), vec!["synthetic-model-a"]);
    assert_eq!(
        MODELS_ENDPOINT,
        "https://chatgpt.com/backend-api/codex/models?client_version=99.0.0"
    );
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    let guard = captured.lock().unwrap();
    let (method, uri, headers) = guard.as_ref().unwrap();
    assert_eq!(method, &axum::http::Method::GET);
    assert_eq!(uri.path(), "/backend-api/codex/models");
    assert_eq!(uri.query(), Some("client_version=99.0.0"));
    assert!(
        headers["authorization"] == "Bearer m49-private-token",
        "fixed synthetic bearer must be present"
    );
    assert!(
        headers["chatgpt-account-id"] == "m49-private-account",
        "current synthetic account header must be present"
    );
    assert_eq!(headers["originator"], "anima");
    assert_eq!(headers["user-agent"], "anima/0.1.0");
}

#[tokio::test]
async fn m49_chatgpt_models_status_invalid_and_known_length_overflow_errors_are_closed() {
    for (status, body, expected) in [
        (
            401,
            b"m49-private auth".to_vec(),
            "Model metadata is unavailable.",
        ),
        (
            403,
            b"m49-private account".to_vec(),
            "Model metadata is unavailable.",
        ),
        (
            429,
            b"m49-private limit".to_vec(),
            "Model metadata is unavailable.",
        ),
        (
            500,
            b"m49-private server".to_vec(),
            "Model metadata is unavailable.",
        ),
        (
            503,
            b"m49-private server".to_vec(),
            "Model metadata is unavailable.",
        ),
        (
            200,
            b"{\"models\":".to_vec(),
            "Model metadata response is invalid.",
        ),
        (200, vec![0xff, 0xfe], "Model metadata response is invalid."),
        (
            200,
            b"{\"models\":null,\"detail\":\"m49-private\"}".to_vec(),
            "Model metadata response is invalid.",
        ),
        (
            200,
            vec![b'x'; 512 * 1024 + 1],
            "Model metadata is unavailable.",
        ),
    ] {
        let attempts = Arc::new(AtomicUsize::new(0));
        let peer = CatalogPeer::start(axum::Router::new().route(
            "/backend-api/codex/models",
            axum::routing::get({
                let attempts = attempts.clone();
                move || {
                    let attempts = attempts.clone();
                    let body = body.clone();
                    async move {
                        attempts.fetch_add(1, Ordering::SeqCst);
                        (axum::http::StatusCode::from_u16(status).unwrap(), body)
                    }
                }
            }),
        ))
        .await;
        let result = peer.adapter().fetch().await;
        peer.join().await;
        assert_eq!(result.unwrap_err(), expected);
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }
    assert!(ChatGptModelsAdapter::new(String::new(), "synthetic-account".into()).is_err());
    assert!(ChatGptModelsAdapter::new("synthetic-token".into(), String::new()).is_err());
}

#[tokio::test]
async fn m49_chatgpt_models_streamed_overflow_is_unavailable_without_retry() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let peer = CatalogPeer::start(axum::Router::new().route(
        "/backend-api/codex/models",
        axum::routing::get({
            let attempts = attempts.clone();
            move || {
                let attempts = attempts.clone();
                async move {
                    attempts.fetch_add(1, Ordering::SeqCst);
                    let chunks = vec![
                        Ok::<_, std::io::Error>(vec![b' '; 256 * 1024]),
                        Ok(vec![b' '; 256 * 1024 + 1]),
                    ];
                    axum::body::Body::from_stream(futures::stream::iter(chunks))
                }
            }
        }),
    ))
    .await;
    let result = peer.adapter().fetch().await;
    peer.join().await;
    assert_eq!(result.unwrap_err(), "Model metadata is unavailable.");
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn m49_chatgpt_models_refuses_redirects_without_sending_credentials_to_the_sink() {
    let sink_attempts = Arc::new(AtomicUsize::new(0));
    let sink = CatalogPeer::start(axum::Router::new().fallback({
        let sink_attempts = sink_attempts.clone();
        move || {
            let sink_attempts = sink_attempts.clone();
            async move {
                sink_attempts.fetch_add(1, Ordering::SeqCst);
                axum::Json(json!({"models":[]}))
            }
        }
    }))
    .await;
    let attempts = Arc::new(AtomicUsize::new(0));
    let location = format!("{}/sink", sink.origin);
    let peer = CatalogPeer::start(axum::Router::new().route(
        "/backend-api/codex/models",
        axum::routing::get({
            let attempts = attempts.clone();
            move || {
                let attempts = attempts.clone();
                let location = location.clone();
                async move {
                    attempts.fetch_add(1, Ordering::SeqCst);
                    (axum::http::StatusCode::FOUND, [("location", location)])
                }
            }
        }),
    ))
    .await;
    let result = peer.adapter().fetch().await;
    peer.join().await;
    sink.join().await;
    assert_eq!(result.unwrap_err(), "Model metadata is unavailable.");
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    assert_eq!(sink_attempts.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn m49_chatgpt_models_incomplete_body_read_is_unavailable() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        tokio::time::timeout(Duration::from_secs(2), async move {
            let (mut socket, _) = listener.accept().await?;
            let mut headers = Vec::new();
            while !headers.ends_with(b"\r\n\r\n") {
                if headers.len() >= 16 * 1024 { return Err(std::io::Error::other("synthetic header bound")); }
                let mut byte = [0u8; 1]; socket.read_exact(&mut byte).await?; headers.push(byte[0]);
            }
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 128\r\nConnection: close\r\n\r\n{\"models\":[").await?;
            socket.shutdown().await
        }).await
    });
    let adapter =
        ChatGptModelsAdapter::new("m49-private-token".into(), "m49-private-account".into())
            .unwrap()
            .at_test_origin(origin, Duration::from_secs(2))
            .unwrap();
    let result = adapter.fetch().await;
    let joined = task.await;
    assert!(
        matches!(joined, Ok(Ok(Ok(())))),
        "finite partial-response peer must finish"
    );
    assert_eq!(result.unwrap_err(), "Model metadata is unavailable.");
}

#[tokio::test]
async fn m49_chatgpt_models_timeout_is_closed_and_does_not_retry() {
    let entered = Arc::new(tokio::sync::Semaphore::new(0));
    let release = Arc::new(tokio::sync::Semaphore::new(0));
    let attempts = Arc::new(AtomicUsize::new(0));
    let peer = CatalogPeer::start(axum::Router::new().route(
        "/backend-api/codex/models",
        axum::routing::get({
            let entered = entered.clone();
            let release = release.clone();
            let attempts = attempts.clone();
            move || {
                let entered = entered.clone();
                let release = release.clone();
                let attempts = attempts.clone();
                async move {
                    attempts.fetch_add(1, Ordering::SeqCst);
                    entered.add_permits(1);
                    let _permit = release.acquire().await.unwrap();
                    axum::Json(json!({"models":[]}))
                }
            }
        }),
    ))
    .await;
    let adapter = peer
        .adapter()
        .at_test_origin(peer.origin.clone(), Duration::from_millis(150))
        .unwrap();
    let mut fetch = tokio::spawn(async move { adapter.fetch().await });
    let observed = tokio::time::timeout(Duration::from_secs(1), entered.acquire())
        .await
        .is_ok();
    let result = tokio::time::timeout(Duration::from_secs(2), &mut fetch).await;
    if result.is_err() {
        fetch.abort();
        let _ = fetch.await;
    }
    release.add_permits(1);
    peer.join().await;
    assert!(
        observed,
        "the controlled request must reach the held response"
    );
    assert_eq!(
        result.unwrap().unwrap().unwrap_err(),
        "Model metadata request timed out."
    );
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
}

fn assert_m50_closed(failure: &super::models::ChatGptModelsFailure) {
    let safe = format!("{failure:?}|{failure}");
    assert!(!safe.contains("m50-private"));
    assert!(!safe.contains("m49-private"));
    assert!(!safe.contains("http://"));
    assert!(std::error::Error::source(failure).is_none());
}

#[tokio::test]
async fn m50_catalog_diagnostics_local_build_and_connect_have_honest_dispatch() {
    use ChatGptCatalogDispatch::*;
    use ChatGptModelsReason::{ClientBuild, Connect, Credential, RequestBuild};
    use ChatGptModelsStage::*;
    let failure =
        match ChatGptModelsAdapter::new_detailed(String::new(), "m50-private-account".into()) {
            Err(error) => error,
            Ok(_) => panic!("empty credential must fail"),
        };
    assert_eq!(
        (
            failure.stage(),
            failure.catalog_dispatch(),
            failure.reason(),
            failure.http_status()
        ),
        (Client, NotStarted, Credential, None)
    );
    assert_m50_closed(&failure);
    let failure = super::models::build_client(reqwest::Client::builder().user_agent("\n"))
        .err()
        .expect("invalid local client header");
    assert_eq!(
        (
            failure.stage(),
            failure.catalog_dispatch(),
            failure.reason(),
            failure.http_status()
        ),
        (Client, NotStarted, ClientBuild, None)
    );
    assert_m50_closed(&failure);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let adapter = ChatGptModelsAdapter::new_detailed(
        "m50-private-token".into(),
        "m50-private-account\n".into(),
    )
    .unwrap()
    .at_test_origin(origin.clone(), Duration::from_secs(1))
    .unwrap();
    let failure = adapter.fetch_detailed().await.unwrap_err();
    assert_eq!(
        (
            failure.stage(),
            failure.catalog_dispatch(),
            failure.reason(),
            failure.http_status()
        ),
        (Request, NotStarted, RequestBuild, None)
    );
    assert_m50_closed(&failure);
    drop(listener);
    struct FailingResolver(Arc<AtomicUsize>);
    impl reqwest::dns::Resolve for FailingResolver {
        fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
            assert_eq!(name.as_str(), "m50-catalog.invalid");
            self.0.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Err(std::io::Error::other("m50-private-resolver-marker").into()) })
        }
    }
    let resolutions = Arc::new(AtomicUsize::new(0));
    let controlled_client = super::models::build_client(
        reqwest::Client::builder()
            .no_proxy()
            .dns_resolver(Arc::new(FailingResolver(resolutions.clone())))
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(1)),
    )
    .unwrap();
    let failure = ChatGptModelsAdapter::new_detailed(
        "m50-private-token".into(),
        "m50-private-account".into(),
    )
    .unwrap()
    .at_test_origin("http://m50-catalog.invalid".into(), Duration::from_secs(1))
    .unwrap()
    .with_test_client(controlled_client)
    .fetch_detailed()
    .await
    .unwrap_err();
    assert_eq!(
        (
            failure.stage(),
            failure.catalog_dispatch(),
            failure.reason(),
            failure.http_status()
        ),
        (Request, Attempted, Connect, None)
    );
    assert_eq!(
        resolutions.load(Ordering::SeqCst),
        1,
        "one controlled resolver failure without DNS or socket I/O"
    );
    assert_m50_closed(&failure);
}

#[tokio::test]
async fn m50_catalog_diagnostics_status_body_limit_and_decode_are_distinct() {
    use ChatGptModelsReason as R;
    use ChatGptModelsStage as S;
    let mut cases = vec![];
    for status in [302, 401, 403, 429, 500, 503] {
        cases.push((
            status,
            b"m50-private-body-marker".to_vec(),
            S::HttpStatus,
            R::HttpStatus,
        ));
    }
    cases.push((200, vec![b'x'; 512 * 1024 + 1], S::Body, R::DeclaredLimit));
    for body in [
        b"{".to_vec(),
        vec![0xff],
        br#"{"models":[{"slug":"m50-private invalid"}]}"#.to_vec(),
    ] {
        cases.push((200, body, S::Decode, R::InvalidCatalog));
    }
    for (status, body, stage, reason) in cases {
        let peer = CatalogPeer::start(axum::Router::new().route(
            "/backend-api/codex/models",
            axum::routing::get(move || {
                let body = body.clone();
                async move { (axum::http::StatusCode::from_u16(status).unwrap(), body) }
            }),
        ))
        .await;
        let result = peer.adapter().fetch_detailed().await;
        peer.join().await;
        let failure = result.unwrap_err();
        assert_eq!(
            (
                failure.stage(),
                failure.catalog_dispatch(),
                failure.reason(),
                failure.http_status()
            ),
            (
                stage,
                ChatGptCatalogDispatch::ResponseReceived,
                reason,
                Some(status)
            )
        );
        assert_eq!(
            failure.public_message(),
            if reason == R::InvalidCatalog {
                "Model metadata response is invalid."
            } else {
                "Model metadata is unavailable."
            }
        );
        assert_m50_closed(&failure);
    }
    for body in [
        json!({"models":[]}),
        json!({"models":[{"slug":"synthetic-model-a"}]}),
    ] {
        let expected = if body["models"].as_array().unwrap().is_empty() {
            vec![]
        } else {
            vec!["synthetic-model-a".to_owned()]
        };
        let peer = CatalogPeer::start(axum::Router::new().route(
            "/backend-api/codex/models",
            axum::routing::get(move || {
                let body = body.clone();
                async move { axum::Json(body) }
            }),
        ))
        .await;
        let result = peer.adapter().fetch_detailed().await;
        peer.join().await;
        assert_eq!(result.unwrap(), expected);
    }
    let peer = CatalogPeer::start(axum::Router::new().route(
        "/backend-api/codex/models",
        axum::routing::get(|| async {
            axum::body::Body::from_stream(futures::stream::iter(vec![
                Ok::<_, std::io::Error>(vec![b' '; 256 * 1024]),
                Ok(vec![b' '; 256 * 1024 + 1]),
            ]))
        }),
    ))
    .await;
    let result = peer.adapter().fetch_detailed().await;
    peer.join().await;
    let failure = result.unwrap_err();
    assert_eq!(
        (failure.stage(), failure.reason(), failure.http_status()),
        (S::Body, R::StreamedLimit, Some(200))
    );
    assert_m50_closed(&failure);
}

#[tokio::test]
async fn m50_catalog_diagnostics_request_transport_and_body_read_do_not_retain_errors() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    for body_read in [false, true] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            tokio::time::timeout(Duration::from_secs(2),async move {
            let(mut socket,_)=listener.accept().await?;let mut headers=Vec::new();
            while !headers.ends_with(b"\r\n\r\n") {if headers.len()>=16384{return Err(std::io::Error::other("synthetic header bound"));}let mut byte=[0];socket.read_exact(&mut byte).await?;headers.push(byte[0]);}
            let bytes=if body_read{b"HTTP/1.1 200 OK\r\nContent-Length: 128\r\nConnection: close\r\n\r\nm50-private-incomplete".as_slice()}else{b"m50-private-invalid-http\r\n".as_slice()};
            socket.write_all(bytes).await?;socket.shutdown().await
        }).await
        });
        let result = ChatGptModelsAdapter::new_detailed(
            "m50-private-token".into(),
            "m50-private-account".into(),
        )
        .unwrap()
        .at_test_origin(origin, Duration::from_secs(1))
        .unwrap()
        .fetch_detailed()
        .await;
        let joined = task.await;
        assert!(
            matches!(joined, Ok(Ok(Ok(())))),
            "finite synthetic response peer must join"
        );
        let failure = result.unwrap_err();
        assert_eq!(
            (
                failure.stage(),
                failure.catalog_dispatch(),
                failure.reason(),
                failure.http_status()
            ),
            if body_read {
                (
                    ChatGptModelsStage::Body,
                    ChatGptCatalogDispatch::ResponseReceived,
                    ChatGptModelsReason::BodyRead,
                    Some(200),
                )
            } else {
                (
                    ChatGptModelsStage::Request,
                    ChatGptCatalogDispatch::Attempted,
                    ChatGptModelsReason::Transport,
                    None,
                )
            }
        );
        assert_m50_closed(&failure);
    }
}

#[tokio::test]
async fn m50_catalog_diagnostics_request_and_body_timeouts_preserve_timeout_message() {
    use futures::StreamExt;
    for body_timeout in [false, true] {
        let entered = Arc::new(tokio::sync::Semaphore::new(0));
        let release = Arc::new(tokio::sync::Semaphore::new(0));
        let peer = CatalogPeer::start(axum::Router::new().route(
            "/backend-api/codex/models",
            axum::routing::get({
                let entered = entered.clone();
                let release = release.clone();
                move || {
                    let entered = entered.clone();
                    let release = release.clone();
                    async move {
                        entered.add_permits(1);
                        if body_timeout {
                            axum::body::Body::from_stream(
                                futures::stream::once(async {
                                    Ok::<_, std::io::Error>(b"{\"models\":[".to_vec())
                                })
                                .chain(futures::stream::once(
                                    async move {
                                        release.acquire().await.unwrap().forget();
                                        Ok::<_, std::io::Error>(b"]}".to_vec())
                                    },
                                )),
                            )
                        } else {
                            release.acquire().await.unwrap().forget();
                            axum::body::Body::from("{\"models\":[]}")
                        }
                    }
                }
            }),
        ))
        .await;
        let adapter = peer
            .adapter()
            .at_test_origin(peer.origin.clone(), Duration::from_millis(150))
            .unwrap();
        let mut fetch = Box::pin(adapter.fetch_detailed());
        tokio::select! {_ = entered.acquire()=>{},_ = &mut fetch=>panic!("controlled response must wait")};
        let result = tokio::time::timeout(Duration::from_secs(2), &mut fetch).await;
        drop(fetch);
        release.add_permits(1);
        peer.join().await;
        let failure = result.unwrap().unwrap_err();
        assert_eq!(
            (
                failure.stage(),
                failure.catalog_dispatch(),
                failure.reason(),
                failure.http_status()
            ),
            if body_timeout {
                (
                    ChatGptModelsStage::Body,
                    ChatGptCatalogDispatch::ResponseReceived,
                    ChatGptModelsReason::Timeout,
                    Some(200),
                )
            } else {
                (
                    ChatGptModelsStage::Request,
                    ChatGptCatalogDispatch::Attempted,
                    ChatGptModelsReason::Timeout,
                    None,
                )
            }
        );
        assert_eq!(
            failure.public_message(),
            "Model metadata request timed out."
        );
        assert_m50_closed(&failure);
    }
}
