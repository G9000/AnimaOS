use super::*;
use crate::mcp::transport::{McpDiscovery, McpTransportError, PhysicalClose};
use anima_core::external_tools::{prepare_external_tool, ExternalToolResult};
use axum::{
    extract::State,
    http::HeaderMap,
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

fn result(text: &str) -> ExternalToolResult {
    ExternalToolResult {
        text_blocks: vec![text.into()],
        structured_data: None,
        is_error: false,
        omitted_blocks: 0,
        shortened: false,
    }
}

pub(crate) fn labelled() -> String {
    format!("Title: Fixture research\nURL: https://example.com/research?q=one&lang=en\nHighlights: observed fixture evidence\n{}\n---\nTitle: Second fixture\nURL: https://example.org/report#section\nHighlights: supporting material\n{}", "a".repeat(1150), "b".repeat(1150))
}

fn descriptor() -> external_tools::ExternalToolDescriptor {
    prepare_external_tool(REMOTE_TOOL, "fixture", json!({"type":"object","properties":{"query":{"type":"string"},"numResults":{"type":"integer"}},"required":["query"],"additionalProperties":false}), None).unwrap()
}

#[test]
fn m83_parser_preserves_rank_inline_multiline_unicode_and_exact_urls() {
    let text = "Title: 東京 <b>source</b>\nPublished: 2026\nURL: https://example.com/a?x=1&y=2#part\nHighlights: inline 一\nmultiline 二\nURL: https://example.net/excerpt-only\n---\nTitle: duplicate\nURL: https://example.com/a?x=1&y=2#part\nHighlights: repeated\n---\nTitle: Last\nURL: https://example.org/last\nHighlights: final";
    let normalized = normalize(result(text), 5).unwrap();
    let value: Value = serde_json::from_str(&normalized).unwrap();
    assert_eq!(value["results"].as_array().unwrap().len(), 2);
    assert_eq!(value["results"][0]["title"], "東京 <b>source</b>");
    assert_eq!(
        value["results"][0]["excerpt"],
        "inline 一\nmultiline 二\nURL: https://example.net/excerpt-only"
    );
    let sources = sources_from_envelope(&normalized).unwrap();
    assert_eq!(
        sources
            .iter()
            .map(|source| source.url.as_str())
            .collect::<Vec<_>>(),
        [
            "https://example.com/a?x=1&y=2#part",
            "https://example.org/last"
        ]
    );
    let empty: Value =
        serde_json::from_str(&normalize(result("No results found."), 3).unwrap()).unwrap();
    assert_eq!(empty["results"], json!([]));
    assert_eq!(empty["truncated"], false);
}

#[test]
fn m83_normalization_rejects_ambiguous_unsafe_and_incomplete_sources_and_bounds_escaped_json() {
    for text in [
        "unrecognized payload https://example.com/not-a-source",
        "Title: ambiguous\nURL: https://example.com/a\nURL: https://example.com/b\nHighlights: body",
        "Title: missing URL\nHighlights: URL: https://example.com/not-metadata",
    ] { assert!(normalize(result(text), 3).is_err(), "{text}"); }
    for url in [
        "javascript:alert(1)",
        "https://user:pass@example.com/",
        "http://localhost/",
        "http://127.1/",
        "http://2130706433/",
        "http://10.0.0.1/",
        "http://[::ffff:127.0.0.1]/",
        "http://[fe80::1]/",
    ] {
        assert!(!public_url(url), "{url}");
        assert!(normalize(
            result(&format!("Title: unsafe\nURL: {url}\nHighlights: data")),
            3
        )
        .is_err());
    }
    assert!(!public_url("https://example.com/\nattack"));
    let mut cut = result("Title: complete\nURL: https://example.com/whole\nHighlights: accepted\n---\nTitle: cut off\nURL: https://example.org/valid-looking-prefix");
    cut.shortened = true;
    let normalized = normalize(cut, 3).unwrap();
    assert_eq!(sources_from_envelope(&normalized).unwrap().len(), 1);
    assert!(
        serde_json::from_str::<Value>(&normalized).unwrap()["truncated"]
            .as_bool()
            .unwrap()
    );
    let large = format!(
        "Title: {}\nURL: https://example.com/escaped\nHighlights: {}",
        "東".repeat(200),
        "\"\\\t".repeat(2000)
    );
    let normalized = normalize(result(&large), 5).unwrap();
    assert!(normalized.len() <= MAX_ENVELOPE);
    let sources = sources_from_envelope(&normalized).unwrap();
    assert!(sources[0].title.len() <= 256);
    assert!(serde_json::to_vec(&sources).unwrap().len() <= MAX_SOURCES);
    let value: Value = serde_json::from_str(&normalized).unwrap();
    assert!(value["results"][0]["excerpt"].as_str().unwrap().len() <= 1200);
    assert_eq!(value["truncated"], true);
}

#[test]
fn m83_public_sources_reject_malformed_arrays_oversized_rows_and_metadata_spoofs() {
    let good = normalize(result(&labelled()), 3).unwrap();
    let mut value: Value = serde_json::from_str(&good).unwrap();
    for replacement in [
        json!([{"title":"","url":"https://example.com/","excerpt":""}]),
        json!([{"title":"x".repeat(257),"url":"https://example.com/","excerpt":""}]),
        json!([{"title":"x","url":"https://example.com/","excerpt":"x".repeat(1201)}]),
        json!([{"title":"x","url":"https://example.com/"}]),
    ] {
        value["results"] = replacement;
        assert!(sources_from_envelope(&value.to_string()).is_none());
    }
    let mut error = TaskResult::success(
        Content {
            text: good,
            metadata: Some(BTreeMap::from([(
                "webSources".into(),
                DataValue::String("spoof".into()),
            )])),
            ..Default::default()
        },
        0,
    );
    error.status = TaskStatus::Error;
    let projected = project_result("web_search", error);
    assert!(!projected
        .data
        .unwrap()
        .metadata
        .unwrap()
        .contains_key("webSources"));
    assert!(normalize(
        ExternalToolResult {
            is_error: true,
            ..result("quota exceeded")
        },
        3
    )
    .unwrap_err()
    .contains("rate-limited"));
}

#[test]
fn m83_native_arguments_are_bounded_before_connect_and_never_select_endpoint_or_tool() {
    let call = |query: &str, count: Option<DataValue>| ToolCall {
        id: "args".into(),
        name: "web_search".into(),
        args: std::iter::once(("query".into(), DataValue::String(query.into())))
            .chain(count.map(|count| ("num_results".into(), count)))
            .collect(),
    };
    assert_eq!(
        arguments(&call("  Unicode 東京  ", None)).unwrap(),
        json!({"query":"Unicode 東京","numResults":3})
    );
    for query in [String::new(), " ".into(), "x".repeat(513), "東".repeat(171)] {
        assert!(arguments(&call(&query, None)).is_err());
    }
    for count in [
        DataValue::Number(0.0),
        DataValue::Number(6.0),
        DataValue::Number(1.5),
        DataValue::Number(f64::NAN),
        DataValue::String("3".into()),
    ] {
        assert!(arguments(&call("query", Some(count))).is_err());
    }
    let mut extra = call("query", None);
    extra.args.insert(
        "endpoint".into(),
        DataValue::String("https://other.example/".into()),
    );
    assert!(arguments(&extra).is_err());
}

fn structured(rows: &[(&str, &str, &str)]) -> ExternalToolResult {
    ExternalToolResult {
        structured_data: Some(DataValue::Object(BTreeMap::from([(
            "results".into(),
            DataValue::Array(
                rows.iter()
                    .map(|(title, url, text)| {
                        DataValue::Object(BTreeMap::from([
                            ("title".into(), DataValue::String((*title).into())),
                            ("url".into(), DataValue::String((*url).into())),
                            ("text".into(), DataValue::String((*text).into())),
                        ]))
                    })
                    .collect(),
            ),
        )]))),
        text_blocks: vec![],
        ..result("")
    }
}

#[test]
fn m83_mixed_invalid_raw_urls_and_literals_keep_valid_siblings_in_both_formats() {
    // Same rejected corpus as the console source validator. Neither layer
    // rewrites invalid spellings into a different clickable source.
    for invalid in [
        "https://example.com/a b",
        "https:example.com",
        "http://192.0.0.1/",
        "http://192.0.2.1/",
        "http://198.51.100.1/",
        "http://203.0.113.1/",
        "http://[2001:db8::1]/",
        "http://[fec0::1]/",
        "http://[::ffff:8.8.8.8]/",
    ] {
        assert!(!public_url(invalid), "{invalid}");
        for response in [
            result(&format!("Title: Actual source\nURL: https://example.com/whole?q=1&lang=en\nHighlights: observed\n---\nTitle: Invalid neighbor\nURL: {invalid}\nHighlights: omitted")),
            structured(&[("Actual source","https://example.com/whole?q=1&lang=en","observed"),("Invalid neighbor",invalid,"omitted")]),
        ] {
            let normalized=normalize(response,5).unwrap();
            let value:Value=serde_json::from_str(&normalized).unwrap();
            assert_eq!(value["truncated"],true);
            let sources=sources_from_envelope(&normalized).unwrap();
            assert_eq!(sources,vec![WebSource{title:"Actual source".into(),url:"https://example.com/whole?q=1&lang=en".into()}]);
            assert_eq!(value["results"][0]["url"],sources[0].url);
        }
    }
}

#[test]
fn m83_canonical_url_deduplication_preserves_first_complete_returned_spelling() {
    let first = "HTTPS://EXAMPLE.COM:443/path?q=1#part";
    let duplicate = "https://example.com/path?q=1#part";
    for response in [
        result(&format!("Title: First source\nURL: {first}\nHighlights: first\n---\nTitle: Equivalent duplicate\nURL: {duplicate}\nHighlights: duplicate\n---\nTitle: Distinct\nURL: https://example.com/path?q=2#part\nHighlights: distinct")),
        structured(&[("First source",first,"first"),("Equivalent duplicate",duplicate,"duplicate"),("Distinct","https://example.com/path?q=2#part","distinct")]),
    ] {
        let normalized=normalize(response,5).unwrap();
        let sources=sources_from_envelope(&normalized).unwrap();
        assert_eq!(sources.len(),2);
        assert_eq!(sources[0].title,"First source");
        assert_eq!(sources[0].url,first);
        let content=project_result("web_search",TaskResult::success(Content{text:normalized.clone(),..Default::default()},0)).data.unwrap();
        assert_eq!(content.metadata.unwrap()["webSources"],source_value(&sources));
        assert_eq!(serde_json::from_str::<Value>(&normalized).unwrap()["results"][0]["url"],first);
    }
}

#[derive(Clone)]
struct HttpState {
    sse: bool,
    result: Value,
    tools: Value,
    expected_arguments: Value,
    records: Arc<Mutex<Vec<(HeaderMap, Value)>>>,
}

async fn http_handler(
    State(state): State<HttpState>,
    headers: HeaderMap,
    Json(request): Json<Value>,
) -> Response {
    assert!(headers.get("authorization").is_none());
    assert!(headers.get("cookie").is_none());
    state
        .records
        .lock()
        .unwrap()
        .push((headers, request.clone()));
    let response = match request["method"].as_str().unwrap_or("") {
        "initialize" => {
            assert_eq!(request["params"]["protocolVersion"], "2025-11-25");
            json!({"protocolVersion":"2025-11-25","capabilities":{"tools":{}},"serverInfo":{"name":"m83-controlled","version":"1"}})
        }
        "notifications/initialized" | "notifications/cancelled" => {
            return axum::http::StatusCode::ACCEPTED.into_response()
        }
        "tools/list" => json!({"tools":state.tools}),
        "tools/call" => {
            assert_eq!(request["params"]["name"], REMOTE_TOOL);
            assert_eq!(request["params"]["arguments"], state.expected_arguments);
            state.result.clone()
        }
        _ => return axum::http::StatusCode::BAD_REQUEST.into_response(),
    };
    let value = json!({"jsonrpc":"2.0","id":request["id"],"result":response});
    if state.sse && request["method"] == "tools/call" {
        (
            [("content-type", "text/event-stream")],
            format!("data: {value}\n\n"),
        )
            .into_response()
    } else {
        Json(value).into_response()
    }
}

struct MappedFactory {
    local: String,
}
impl McpTransportFactory for MappedFactory {
    fn connect(
        &self,
        launch: ValidatedMcpLaunch,
        control: TransportControl,
    ) -> BoxFuture<'static, Result<Arc<dyn McpTransportSession>, McpTransportError>> {
        let ValidatedMcpLaunch::Http(endpoint) = launch else {
            panic!("anonymous HTTP only");
        };
        assert_eq!(endpoint.0, ENDPOINT);
        DefaultMcpTransportFactory::default().connect(
            ValidatedMcpLaunch::Http(ValidatedMcpEndpoint(self.local.clone())),
            control,
        )
    }
}

pub(crate) struct HttpFixture {
    pub(crate) executor: Arc<SearchExecutor>,
    pub(crate) records: Arc<Mutex<Vec<(HeaderMap, Value)>>>,
    stop: CancellationToken,
    server: JoinHandle<()>,
}
impl HttpFixture {
    pub(crate) async fn new(sse: bool, result: Value, tools: Option<Value>) -> Self {
        Self::with_arguments(
            sse,
            result,
            tools,
            json!({"query":"fixture query","numResults":3}),
        )
        .await
    }
    async fn with_arguments(
        sse: bool,
        result: Value,
        tools: Option<Value>,
        expected_arguments: Value,
    ) -> Self {
        let records = Arc::new(Mutex::new(Vec::new()));
        let tools = tools.unwrap_or_else(|| json!([
            {"name":REMOTE_TOOL,"description":"fixture search","inputSchema":{"type":"object","properties":{"query":{"type":"string"},"numResults":{"type":"integer"}},"required":["query"],"additionalProperties":false}},
            {"name":"agent_run","description":"must never be called","inputSchema":{"type":"object","properties":{}}}
        ]));
        let state = HttpState {
            sse,
            result,
            tools,
            expected_arguments,
            records: records.clone(),
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let local = format!("http://{}/mcp", listener.local_addr().unwrap());
        let stop = CancellationToken::new();
        let shutdown = stop.clone();
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new()
                    .route("/mcp", post(http_handler))
                    .with_state(state),
            )
            .with_graceful_shutdown(shutdown.cancelled_owned())
            .await
            .unwrap();
        });
        Self {
            executor: SearchExecutor::for_test(
                Arc::new(MappedFactory { local }),
                4,
                Duration::from_secs(5),
            ),
            records,
            stop,
            server,
        }
    }
    pub(crate) async fn close(self) {
        self.executor.join().await;
        self.stop.cancel();
        tokio::time::timeout(Duration::from_secs(5), self.server)
            .await
            .unwrap()
            .unwrap();
    }
}

// The actual anonymous catalogue captured through the unchanged SDK path.
fn public_search_tool() -> Value {
    json!({"name":REMOTE_TOOL,"description":"Search the web for any topic and get clean, ready-to-use content.","inputSchema":{
        "$schema":"http://json-schema.org/draft-07/schema#","type":"object","additionalProperties":false,
        "properties":{
            "query":{"type":"string","minLength":1,"description":"Natural language search query. Should be a semantically rich description of the ideal page, not just keywords. Optionally include category:<type> (company, people) to focus results — e.g. 'category:people John Doe software engineer'."},
            "numResults":{"type":"number","description":"Number of search results to return (default: 10)."},
            "objective":{"type":"string","minLength":1,"maxLength":4096,"description":"Goal for this search turn; say which documents should rank first, which should be excluded, and what specific facts or figures to pull from them."}
        },"required":["query","objective"]
    }})
}

#[tokio::test]
async fn m83_actual_public_draft7_schema_json_and_sse_derive_exact_objective_without_other_tools() {
    for sse in [false, true] {
        let fixture = HttpFixture::with_arguments(
            sse,
            json!({"content":[{"type":"text","text":labelled()}],"isError":false}),
            Some(json!([public_search_tool(),{"name":"web_fetch_exa","description":"not selected","inputSchema":{"$schema":"http://json-schema.org/draft-07/schema#","type":"object","properties":{"url":{"type":"string"}}}}])),
            json!({"query":"fixture query","numResults":3,"objective":"fixture query"}),
        ).await;
        let output = fixture
            .executor
            .search(
                json!({"query":"fixture query","numResults":3}),
                CancelSignal::new(),
            )
            .await
            .unwrap();
        assert_eq!(sources_from_envelope(&output).unwrap().len(), 2);
        let calls = fixture
            .records
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, request)| request["method"] == "tools/call")
            .map(|(_, request)| request.clone())
            .collect::<Vec<_>>();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0]["params"]["name"], REMOTE_TOOL);
        assert_eq!(
            calls[0]["params"]["arguments"],
            json!({"query":"fixture query","numResults":3,"objective":"fixture query"})
        );
        fixture.close().await;
    }
}

#[tokio::test]
async fn m83_native_compatibility_rejects_unknown_required_refs_dialects_constraints_and_catalogue_bounds_before_call(
) {
    let base = public_search_tool();
    let mut unknown_required = base.clone();
    unknown_required["inputSchema"]["required"] = json!(["query", "objective", "unprovided"]);
    let mut wrong_objective = base.clone();
    wrong_objective["inputSchema"]["properties"]["objective"] = json!({"type":"number"});
    let mut reference = base.clone();
    reference["inputSchema"]["properties"]["objective"] = json!({"$ref":"#/$defs/objective"});
    reference["inputSchema"]["$defs"] = json!({"objective":{"type":"string"}});
    let mut dialect = base.clone();
    dialect["inputSchema"]["$schema"] = json!("http://json-schema.org/draft-04/schema#");
    let mut unsupported = base.clone();
    unsupported["inputSchema"]["properties"]["query"]["pattern"] = json!("fixture");
    let mut constrained = base.clone();
    constrained["inputSchema"]["properties"]["query"]["minLength"] = json!(100);
    let mut output = base.clone();
    output["outputSchema"] = json!({"type":"object","properties":{"requiredEvidence":{"const":true}},"required":["requiredEvidence"]});
    let duplicate_ignored =
        json!({"name":"unselected","inputSchema":{"type":"object","properties":{}}});
    let mut too_many=(0..64).map(|i|json!({"name":format!("unselected_{i}"),"inputSchema":{"type":"object","properties":{}}})).collect::<Vec<_>>();
    too_many.push(base.clone());
    for tools in [
        json!([unknown_required]),
        json!([wrong_objective]),
        json!([reference]),
        json!([dialect]),
        json!([unsupported]),
        json!([constrained]),
        json!([base.clone(), duplicate_ignored.clone(), duplicate_ignored]),
        json!(too_many),
        json!([base.clone(),{"name":" ","inputSchema":{"type":"object"}}]),
        json!([base.clone(),{"name":"x".repeat(129),"inputSchema":{"type":"object"}}]),
        json!([base.clone(),{"name":"bad\nname","inputSchema":{"type":"object"}}]),
    ] {
        let fixture = HttpFixture::new(false, json!({"content":[]}), Some(tools)).await;
        assert!(fixture
            .executor
            .search(
                json!({"query":"fixture query","numResults":3}),
                CancelSignal::new()
            )
            .await
            .is_err());
        assert!(!fixture
            .records
            .lock()
            .unwrap()
            .iter()
            .any(|(_, request)| request["method"] == "tools/call"));
        fixture.close().await;
    }
    let fixture=HttpFixture::with_arguments(false,json!({"content":[{"type":"text","text":labelled()}],"structuredContent":{},"isError":false}),Some(json!([output])),json!({"query":"fixture query","numResults":3,"objective":"fixture query"})).await;
    assert!(fixture
        .executor
        .search(
            json!({"query":"fixture query","numResults":3}),
            CancelSignal::new()
        )
        .await
        .is_err());
    assert_eq!(
        fixture
            .records
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, request)| request["method"] == "tools/call")
            .count(),
        1,
        "Ordinary advertised output constraints remain enforced"
    );
    fixture.close().await;
}

#[tokio::test]
async fn m83_generic_mcp_discovery_still_rejects_observed_draft7_schema_without_call() {
    let fixture = HttpFixture::new(
        false,
        json!({"content":[]}),
        Some(json!([public_search_tool()])),
    )
    .await;
    let control = TransportControl::new(
        Instant::now() + Duration::from_secs(5),
        CancellationToken::new(),
    );
    let session = fixture
        .executor
        .factory
        .connect(
            ValidatedMcpLaunch::Http(ValidatedMcpEndpoint(ENDPOINT.into())),
            control.clone(),
        )
        .await
        .unwrap();
    assert_eq!(
        session.discover(control).await.unwrap_err(),
        McpTransportError::SchemaRejected
    );
    session.close().await.unwrap();
    assert!(!fixture
        .records
        .lock()
        .unwrap()
        .iter()
        .any(|(_, request)| request["method"] == "tools/call"));
    fixture.close().await;
}

#[tokio::test]
async fn m83_actual_http_json_and_sse_initialize_discover_call_once_without_credentials() {
    for sse in [false, true] {
        let fixture = HttpFixture::new(
            sse,
            json!({"content":[{"type":"text","text":labelled()}],"isError":false}),
            None,
        )
        .await;
        let output = fixture
            .executor
            .search(
                json!({"query":"fixture query","numResults":3}),
                CancelSignal::new(),
            )
            .await
            .unwrap();
        assert_eq!(sources_from_envelope(&output).unwrap().len(), 2);
        let requests = fixture
            .records
            .lock()
            .unwrap()
            .iter()
            .map(|(_, request)| request.clone())
            .collect::<Vec<_>>();
        for method in ["initialize", "tools/list", "tools/call"] {
            assert_eq!(
                requests
                    .iter()
                    .filter(|request| request["method"] == method)
                    .count(),
                1
            );
        }
        assert!(requests
            .iter()
            .all(|request| request["params"]["name"] != "agent_run"));
        fixture.close().await;
    }
}

#[tokio::test]
async fn m83_discovery_rejects_missing_duplicate_and_incompatible_search_without_call() {
    let valid = json!({"name":REMOTE_TOOL,"inputSchema":{"type":"object","properties":{"query":{"type":"string"},"numResults":{"type":"integer"}}}});
    for tools in [
        json!([]),
        json!([valid.clone(), valid]),
        json!([{"name":REMOTE_TOOL,"inputSchema":{"type":"object","properties":{"query":{"type":"number"},"numResults":{"type":"integer"}}}}]),
    ] {
        let fixture = HttpFixture::new(false, json!({"content":[]}), Some(tools)).await;
        assert!(fixture
            .executor
            .search(
                json!({"query":"fixture query","numResults":3}),
                CancelSignal::new()
            )
            .await
            .is_err());
        assert!(!fixture
            .records
            .lock()
            .unwrap()
            .iter()
            .any(|(_, request)| request["method"] == "tools/call"));
        fixture.close().await;
    }
}

#[tokio::test]
async fn m83_actual_http_remote_errors_malformed_results_and_wire_limits_return_no_sources() {
    for body in [
        json!({"content":[{"type":"text","text":"quota exceeded"}],"isError":true}),
        json!({"content":[{"type":"text","text":"not a supported search result"}],"isError":false}),
        json!({"content":[{"type":"text","text":"x".repeat(270*1024)}],"isError":false}),
    ] {
        let fixture = HttpFixture::new(false, body, None).await;
        assert!(fixture
            .executor
            .search(
                json!({"query":"fixture query","numResults":3}),
                CancelSignal::new()
            )
            .await
            .is_err());
        assert_eq!(
            fixture
                .records
                .lock()
                .unwrap()
                .iter()
                .filter(|(_, request)| request["method"] == "tools/call")
                .count(),
            1
        );
        fixture.close().await;
    }
}

struct SlowFactory {
    phase: &'static str,
    entered: Arc<Semaphore>,
    release_connect: Arc<Semaphore>,
    close_entered: Arc<Semaphore>,
    release_close: Arc<Semaphore>,
    connects: Arc<AtomicUsize>,
    calls: Arc<AtomicUsize>,
    first_close: Arc<AtomicBool>,
}
impl McpTransportFactory for SlowFactory {
    fn connect(
        &self,
        _: ValidatedMcpLaunch,
        control: TransportControl,
    ) -> BoxFuture<'static, Result<Arc<dyn McpTransportSession>, McpTransportError>> {
        let phase = self.phase;
        let entered = self.entered.clone();
        let release = self.release_connect.clone();
        let first = self.connects.fetch_add(1, Ordering::SeqCst) == 0;
        let session = SlowSession {
            phase,
            stall: first,
            entered: entered.clone(),
            close_entered: self.close_entered.clone(),
            release_close: self.release_close.clone(),
            calls: self.calls.clone(),
            first_close: self.first_close.clone(),
        };
        Box::pin(async move {
            if first && phase == "connect" {
                entered.add_permits(1);
                release.acquire().await.unwrap().forget();
            }
            let _ = control;
            Ok(Arc::new(session) as Arc<dyn McpTransportSession>)
        })
    }
}
struct SlowSession {
    phase: &'static str,
    stall: bool,
    entered: Arc<Semaphore>,
    close_entered: Arc<Semaphore>,
    release_close: Arc<Semaphore>,
    calls: Arc<AtomicUsize>,
    first_close: Arc<AtomicBool>,
}
impl McpTransportSession for SlowSession {
    fn discover(
        &self,
        control: TransportControl,
    ) -> BoxFuture<'static, Result<McpDiscovery, McpTransportError>> {
        let phase = self.phase;
        let entered = self.entered.clone();
        let stall = self.stall;
        Box::pin(async move {
            if stall && phase == "discover" {
                entered.add_permits(1);
                control.cancel.cancelled().await;
                return Err(McpTransportError::Cancelled);
            }
            Ok(McpDiscovery {
                protocol_version: "2025-11-25".into(),
                tools: vec![descriptor()],
            })
        })
    }
    fn call_once(
        &self,
        _: McpCall,
        control: TransportControl,
    ) -> BoxFuture<'static, Result<ExternalToolResult, McpTransportError>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let phase = self.phase;
        let entered = self.entered.clone();
        let stall = self.stall;
        Box::pin(async move {
            if stall && phase == "call" {
                entered.add_permits(1);
                control.cancel.cancelled().await;
                return Err(McpTransportError::Cancelled);
            }
            Ok(result(
                "Title: source\nURL: https://example.com/result\nHighlights: actual",
            ))
        })
    }
    fn close(&self) -> BoxFuture<'static, Result<PhysicalClose, McpTransportError>> {
        let first = self.first_close.swap(false, Ordering::SeqCst);
        let entered = self.close_entered.clone();
        let release = self.release_close.clone();
        Box::pin(async move {
            if first {
                entered.add_permits(1);
                release.acquire().await.unwrap().forget();
            }
            Ok(PhysicalClose::for_test())
        })
    }
}

#[tokio::test]
async fn m83_cancel_drop_and_deadline_keep_admission_until_connect_and_close_join() {
    for (phase, action) in [
        ("connect", "cancel"),
        ("discover", "cancel"),
        ("call", "cancel"),
        ("close", "cancel"),
        ("call", "drop"),
        ("close", "drop"),
        ("close", "deadline"),
    ] {
        let entered = Arc::new(Semaphore::new(0));
        let close_entered = Arc::new(Semaphore::new(0));
        let release_connect = Arc::new(Semaphore::new(0));
        let release_close = Arc::new(Semaphore::new(0));
        let connects = Arc::new(AtomicUsize::new(0));
        let calls = Arc::new(AtomicUsize::new(0));
        let executor = SearchExecutor::for_test(
            Arc::new(SlowFactory {
                phase,
                entered: entered.clone(),
                release_connect: release_connect.clone(),
                close_entered: close_entered.clone(),
                release_close: release_close.clone(),
                connects: connects.clone(),
                calls: calls.clone(),
                first_close: Arc::new(AtomicBool::new(true)),
            }),
            1,
            Duration::from_millis(if action == "deadline" { 30 } else { 1000 }),
        );
        let cancel = CancelSignal::new();
        let request_cancel = cancel.clone();
        let owner = executor.clone();
        let task = tokio::spawn(async move {
            owner
                .search(
                    json!({"query":"fixture query","numResults":3}),
                    request_cancel,
                )
                .await
        });
        let barrier = if phase == "close" {
            &close_entered
        } else {
            &entered
        };
        tokio::time::timeout(Duration::from_secs(2), barrier.acquire())
            .await
            .unwrap()
            .unwrap()
            .forget();
        if action == "drop" {
            task.abort();
            assert!(task.await.unwrap_err().is_cancelled());
        } else {
            if action == "cancel" {
                cancel.cancel();
            }
            assert!(task.await.unwrap().is_err());
        }
        assert_eq!(executor.permits.available_permits(), 0, "{phase}/{action}");
        assert!(executor
            .search(
                json!({"query":"fixture query","numResults":3}),
                CancelSignal::new()
            )
            .await
            .unwrap_err()
            .contains("busy"));
        assert_eq!(connects.load(Ordering::SeqCst), 1);
        if phase == "connect" {
            release_connect.add_permits(1);
        }
        if phase != "close" {
            tokio::time::timeout(Duration::from_secs(2), close_entered.acquire())
                .await
                .unwrap()
                .unwrap()
                .forget();
        }
        release_close.add_permits(1);
        executor.join().await;
        assert_eq!(executor.permits.available_permits(), 1);
        if phase == "connect" || phase == "discover" {
            assert_eq!(calls.load(Ordering::SeqCst), 0);
        }
        let cancelled = CancelSignal::new();
        cancelled.cancel();
        assert!(executor
            .search(json!({"query":"fixture query","numResults":3}), cancelled)
            .await
            .is_err());
        assert_eq!(connects.load(Ordering::SeqCst), 1);
        assert!(executor
            .search(
                json!({"query":"fixture query","numResults":3}),
                CancelSignal::new()
            )
            .await
            .is_ok());
        executor.join().await;
        assert_eq!(connects.load(Ordering::SeqCst), 2);
        assert_eq!(executor.permits.available_permits(), 1);
    }
}
