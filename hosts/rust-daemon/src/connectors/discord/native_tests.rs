use super::*;
#[path = "channel_admin_native_tests.rs"]
mod channel_admin_tests;
use axum::{
    body::{to_bytes, Body},
    extract::State,
    http::{Request, StatusCode},
    response::Response,
    routing::any,
    Router,
};
use std::{collections::VecDeque, sync::Mutex as StdMutex};

type Call = (String, String, serde_json::Value);
struct Fixture {
    responses: StdMutex<VecDeque<(u16, String, Vec<(String, String)>)>>,
    calls: StdMutex<Vec<Call>>,
    user_agents: StdMutex<Vec<Option<String>>>,
    raw_bodies: StdMutex<Vec<(String, Vec<u8>)>>,
    methods: StdMutex<Vec<String>>,
}
async fn handler(State(fixture): State<Arc<Fixture>>, request: Request<Body>) -> Response {
    fixture
        .methods
        .lock()
        .unwrap()
        .push(request.method().to_string());
    fixture.user_agents.lock().unwrap().push(
        request
            .headers()
            .get("user-agent")
            .map(|value| value.to_str().unwrap().to_owned()),
    );
    let uri = request.uri().to_string();
    let content_type = request
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let auth = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_owned();
    let body = to_bytes(request.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    fixture
        .raw_bodies
        .lock()
        .unwrap()
        .push((content_type, body.to_vec()));
    fixture.calls.lock().unwrap().push((
        uri,
        auth,
        serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null),
    ));
    let (status, body, headers) = fixture
        .responses
        .lock()
        .unwrap()
        .pop_front()
        .expect("unexpected native effect");
    let mut response = Response::builder().status(StatusCode::from_u16(status).unwrap());
    for (name, value) in headers {
        response = response.header(name, value);
    }
    response.body(Body::from(body)).unwrap()
}
async fn fixture(
    responses: Vec<(u16, String, Vec<(String, String)>)>,
) -> (DiscordClient, Arc<Fixture>, tokio::task::JoinHandle<()>) {
    let fixture = Arc::new(Fixture {
        responses: StdMutex::new(responses.into()),
        calls: StdMutex::new(Vec::new()),
        user_agents: StdMutex::new(Vec::new()),
        raw_bodies: StdMutex::new(Vec::new()),
        methods: StdMutex::new(Vec::new()),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = Router::new()
        .fallback(any(handler))
        .with_state(fixture.clone());
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    (
        DiscordClient::for_test_base(&format!("http://{address}/api/v10/")).unwrap(),
        fixture,
        task,
    )
}
fn json(value: serde_json::Value) -> (u16, String, Vec<(String, String)>) {
    (200, value.to_string(), Vec::new())
}
fn message(id: u64) -> serde_json::Value {
    serde_json::json!({"id":id.to_string(),"channel_id":"42","guild_id":"7","author":{"id":"9","bot":false},"content":"fixture","type":0})
}

#[test]
fn native_discord_preserves_attachment_only_message_metadata() {
    let mut value = message(101);
    value["content"] = serde_json::json!("");
    value["attachments"] = serde_json::json!([{
        "id":"201", "filename":"report.md", "size":9,
        "url":"https://cdn.discordapp.com/attachments/42/201/report.md?ex=ffff",
        "content_type":"text/markdown"
    }]);
    let wire: WireMessage = serde_json::from_value(value).unwrap();
    let normalized = serde_json::to_value(wire.normalize("42").unwrap()).unwrap();
    assert_eq!(normalized["attachments"][0]["filename"], "report.md");
    assert_eq!(normalized["attachments"][0]["size"], 9);
}

#[test]
fn discord_thread_reader_is_registered() {
    let registry = crate::tools::ToolRegistry::new();
    let tool = registry.descriptor("read_discord_thread");
    assert!(
        tool.is_some(),
        "current-thread history must be available to Discord agents"
    );
}

#[tokio::test]
async fn discord_thread_history_uses_bounded_page_and_keeps_bot_identity() {
    let mut other = message(102);
    other["author"]["bot"] = serde_json::json!(true);
    other["author"]["username"] = serde_json::json!("other-agent");
    let (client, fixture, task) = fixture(vec![json(serde_json::json!([other]))]).await;
    let result = client
        .read_thread_history(
            &DiscordBotToken::parse("history-fixture").unwrap(),
            "42",
            Some("103"),
            20,
        )
        .await
        .unwrap();
    assert_eq!(result.len(), 1);
    assert!(result[0].is_bot);
    assert_eq!(result[0].sender.username.as_deref(), Some("other-agent"));
    assert_eq!(
        fixture.calls.lock().unwrap()[0].0,
        "/api/v10/channels/42/messages?limit=20&before=103"
    );
    assert!(client
        .read_thread_history(
            &DiscordBotToken::parse("history-fixture").unwrap(),
            "42",
            None,
            26
        )
        .await
        .is_err());
    task.abort();
}

#[test]
fn discord_attachment_tool_is_registered_for_generated_files() {
    let tool = crate::tools::ToolRegistry::new().descriptor("send_discord_file");
    assert!(
        tool.is_some(),
        "current-thread file sending must be registered"
    );
}

#[test]
fn discord_attachment_cdn_urls_are_bound_to_native_source_ids() {
    let mut a = DiscordAttachment {
        id: "201".into(),
        filename: "report.md".into(),
        size: 9,
        url: "https://cdn.discordapp.com/attachments/42/201/report.md?ex=123&hm=signature".into(),
    };
    assert!(valid_attachment_url("42", &a).is_ok());
    for url in [
        "http://cdn.discordapp.com/attachments/42/201/report.md",
        "https://evil.example/attachments/42/201/report.md",
        "https://cdn.discordapp.com.evil.example/attachments/42/201/report.md",
        "https://cdn.discordapp.com/attachments/43/201/report.md",
        "https://cdn.discordapp.com/attachments/42/202/report.md",
        "https://cdn.discordapp.com:444/attachments/42/201/report.md",
        "https://user@cdn.discordapp.com/attachments/42/201/report.md",
        "https://cdn.discordapp.com/attachments/42/201/report.md/extra",
        "https://127.0.0.1/attachments/42/201/report.md",
    ] {
        a.url = url.into();
        assert_eq!(
            valid_attachment_url("42", &a),
            Err(DiscordTransportError::InvalidInput),
            "{url}"
        );
    }
}

fn upload() -> DiscordFileUpload {
    DiscordFileUpload {
        filename: "report.md".into(),
        mime: "text/markdown".into(),
        bytes: b"# report\n".to_vec(),
    }
}

#[tokio::test]
async fn discord_attachment_upload_is_native_multipart_and_proves_its_receipt() {
    let (client, fixture, task) = fixture(vec![json(serde_json::json!({"id":"901","channel_id":"42","attachments":[{"id":"902","filename":"report.md","size":9}]}))]).await;
    let receipt = client
        .send_file(
            &DiscordBotToken::parse("file-fixture").unwrap(),
            "42",
            &upload(),
            "@everyone report",
            "run:call",
        )
        .await
        .unwrap();
    assert_eq!(receipt.attachment_id, "902");
    let raw = fixture.raw_bodies.lock().unwrap();
    assert!(raw[0].0.starts_with("multipart/form-data; boundary="));
    let body = String::from_utf8(raw[0].1.clone()).unwrap();
    assert!(body.contains("name=\"files[0]\"; filename=\"report.md\""));
    assert!(body.contains("# report\n"));
    assert!(body.contains("\"parse\":[]"));
    assert!(body.contains("\"replied_user\":false"));
    assert!(body.contains("\"enforce_nonce\":true"));
    assert!(body.contains(&stable_nonce("run:call")));
    task.abort();
}

#[tokio::test]
async fn discord_attachment_upload_never_confirms_missing_or_mismatched_files() {
    for value in [
        serde_json::json!({"id":"901","channel_id":"42","attachments":[]}),
        serde_json::json!({"id":"901","channel_id":"43","attachments":[{"id":"902","filename":"report.md","size":9}]}),
        serde_json::json!({"id":"901","channel_id":"42","attachments":[{"id":"902","filename":"other.md","size":9}]}),
        serde_json::json!({"id":"901","channel_id":"42","attachments":[{"id":"902","filename":"report.md","size":8}]}),
    ] {
        let (client, _, task) = fixture(vec![json(value)]).await;
        assert!(matches!(
            client
                .send_file(
                    &DiscordBotToken::parse("file-receipt-fixture").unwrap(),
                    "42",
                    &upload(),
                    "",
                    "call"
                )
                .await,
            Err(DiscordTransportError::Uncertain)
        ));
        task.abort();
    }
}

#[tokio::test]
async fn discord_attachment_download_body_bounds_and_missing_status_are_distinct_from_transport() {
    for (status, body, expected, error) in [
        (200, "short", 9, DiscordTransportError::InvalidResponse),
        (200, "oversize", 2, DiscordTransportError::ResponseTooLarge),
        (
            404,
            "gone",
            9,
            DiscordTransportError::HttpStatus { status: 404 },
        ),
        (503, "retry", 9, DiscordTransportError::Transport),
    ] {
        let (client, fixture, task) = fixture(vec![(status, body.into(), vec![])]).await;
        let response = client
            .client
            .get(client.base.join("fixture").unwrap())
            .send()
            .await
            .unwrap();
        assert_eq!(
            DiscordClient::download_response(response, expected).await,
            Err(error)
        );
        assert!(
            fixture.calls.lock().unwrap()[0].1.is_empty(),
            "CDN reads carry no bot Authorization"
        );
        task.abort();
    }
}

#[tokio::test]
async fn native_discord_accepts_guild_threads_and_retains_parent_authority() {
    let (client, _, task) = fixture(vec![json(serde_json::json!({
        "id":"42","guild_id":"7","type":11,"parent_id":"41","name":"topic"
    }))])
    .await;
    let channel = client
        .get_channel(&DiscordBotToken::parse("thread-fixture").unwrap(), "42")
        .await;
    assert!(
        channel.is_ok(),
        "a Discord public thread is a valid conversation destination"
    );
    assert_eq!(
        serde_json::to_value(channel.unwrap()).unwrap()["parentChannelId"],
        "41"
    );
    task.abort();
}

#[tokio::test]
async fn native_application_check_uses_verified_bot_and_only_accepted_team_owners() {
    let (client, fixture, task) = fixture(vec![
        json(
            serde_json::json!({"id":"11","flags":1<<19,"bot":{"id":"11"},"team":{"members":[
                {"membership_state":2,"user":{"id":"44"}},
                {"membership_state":1,"user":{"id":"55"}},
                {"membership_state":2,"user":{"id":"44"}}
            ]}}),
        ),
        json(serde_json::json!({"id":"11","username":"fixture","bot":true})),
    ])
    .await;
    let check = client
        .check_application(&DiscordBotToken::parse("check-fixture").unwrap())
        .await
        .unwrap();
    assert_eq!(check.owner_user_ids, vec!["44"]);
    assert_eq!(check.bot.id, "11");
    assert!(check.message_content_enabled);
    assert_eq!(check.management_owner_user_id, None);
    let invite = Url::parse(&check.invite_url).unwrap();
    let query: std::collections::HashMap<_, _> = invite.query_pairs().collect();
    assert_eq!(invite.origin().ascii_serialization(), "https://discord.com");
    assert_eq!(invite.path(), "/oauth2/authorize");
    assert_eq!(query["client_id"], "11");
    assert_eq!(query["scope"], "bot");
    assert_eq!(query["integration_type"], "0");
    let permissions: u64 = query["permissions"].parse().unwrap();
    // Channel administration, readable history, uploads and public/private thread use.
    for bit in [4, 10, 11, 15, 16, 28, 29, 34, 35, 36, 38, 48] {
        assert_ne!(
            permissions & (1u64 << bit),
            0,
            "missing permission bit {bit}"
        );
    }
    assert_eq!(
        permissions & (1u64 << 3),
        0,
        "must not request Administrator"
    );
    assert!(!serde_json::to_string(&check)
        .unwrap()
        .contains("check-fixture"));
    assert_eq!(
        fixture.calls.lock().unwrap()[0].0,
        "/api/v10/applications/@me"
    );
    task.abort();
}

#[tokio::test]
async fn native_auto_thread_names_strip_mentions_and_bound_utf16() {
    for (input, expected) in [
        (
            "<@11> review <@!44> <@&55> <#22>\n page".to_string(),
            "review page".to_string(),
        ),
        ("<@11> <@&55>".to_string(), "Anima conversation".to_string()),
        ("🦀".repeat(50), format!("{}...", "🦀".repeat(38))),
        ("界".repeat(81), format!("{}...", "界".repeat(77))),
    ] {
        let (client, fixture, task) = fixture(vec![json(serde_json::json!({
            "id":"101","guild_id":"33","type":11,"parent_id":"22","name":expected
        }))])
        .await;
        client
            .create_thread(
                &DiscordBotToken::parse("title-fixture").unwrap(),
                "22",
                "101",
                &input,
            )
            .await
            .unwrap_or_else(|error| panic!("thread name input {input:?}: {error:?}"));
        assert_eq!(fixture.calls.lock().unwrap()[0].2["name"], expected);
        task.abort();
    }
}

#[tokio::test]
async fn native_thread_rename_patches_only_name_and_validates_response_identity() {
    let (client, fixture, task) = fixture(vec![
        json(serde_json::json!({"id":"101","guild_id":"33","type":11,"parent_id":"22","name":"Landing Page Review"})),
        json(serde_json::json!({"id":"999","guild_id":"33","type":11,"parent_id":"22","name":"Other Title"})),
    ]).await;
    let token = DiscordBotToken::parse("rename-fixture").unwrap();
    assert_eq!(
        client
            .rename_thread(&token, "101", "Landing Page Review")
            .await
            .unwrap()
            .name
            .as_deref(),
        Some("Landing Page Review")
    );
    assert_eq!(
        client
            .rename_thread(&token, "101", "Other Title")
            .await
            .unwrap_err(),
        DiscordTransportError::Uncertain
    );
    assert_eq!(
        client
            .rename_thread(&token, "101", &"🦀".repeat(41))
            .await
            .unwrap_err(),
        DiscordTransportError::InvalidInput
    );
    let calls = fixture.calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].0, "/api/v10/channels/101");
    assert_eq!(
        calls[0].2,
        serde_json::json!({"name":"Landing Page Review"})
    );
    task.abort();
}

#[tokio::test]
async fn native_role_lookup_verifies_member_identity_and_thread_posts_verify_parent() {
    let (client, fixture, task) = fixture(vec![
        json(serde_json::json!({"user":{"id":"44"},"roles":["77"]})),
        json(serde_json::json!({"id":"101","guild_id":"33","type":11,"parent_id":"99"})),
    ])
    .await;
    let token = DiscordBotToken::parse("role-thread-fixture").unwrap();
    assert_eq!(
        client.member_roles(&token, "33", "44").await.unwrap(),
        vec!["77"]
    );
    assert_eq!(
        client
            .create_thread(&token, "22", "101", "topic")
            .await
            .unwrap_err(),
        DiscordTransportError::Uncertain
    );
    let calls = fixture.calls.lock().unwrap();
    assert_eq!(calls[0].0, "/api/v10/guilds/33/members/44");
    assert_eq!(calls[1].0, "/api/v10/channels/22/messages/101/threads");
    assert_eq!(
        calls[1].2,
        serde_json::json!({"name":"topic","auto_archive_duration":1440})
    );
    task.abort();
}

#[tokio::test]
async fn discord_requests_identify_the_client_with_the_required_user_agent() {
    let (client, fixture, task) = fixture(vec![json(serde_json::json!({
        "id": "9", "username": "fixture", "bot": true
    }))])
    .await;
    let token = DiscordBotToken::parse("fixture-token").unwrap();
    client.get_me(&token).await.unwrap();
    assert_eq!(
        fixture.user_agents.lock().unwrap().as_slice(),
        &[Some(format!(
            "DiscordBot (https://github.com/G9000/animaos, {})",
            env!("CARGO_PKG_VERSION")
        ))]
    );
    task.abort();
}

#[tokio::test]
async fn discord_typing_accepts_the_providers_empty_no_content_response() {
    let (client, fixture, task) = fixture(vec![(204, String::new(), vec![])]).await;
    let token = DiscordBotToken::parse("typing-fixture").unwrap();
    let result: Result<(), _> = client
        .request(&token, Method::POST, "channels/42/typing", None)
        .await;
    assert_eq!(result, Ok(()));
    let calls = fixture.calls.lock().unwrap();
    assert_eq!(calls[0].0, "/api/v10/channels/42/typing");
    assert_eq!(calls[0].2, serde_json::Value::Null);
    task.abort();
}

#[tokio::test]
async fn discord_backward_scan_proves_numeric_boundary_and_preserves_native_flags() {
    let mut bot = message(102);
    bot["author"]["bot"] = serde_json::json!(true);
    bot["webhook_id"] = serde_json::json!("55");
    bot["type"] = serde_json::json!(19);
    let (client, fixture, task) = fixture(vec![
        json(serde_json::json!([message(103)])),
        json(serde_json::json!([message(100), bot.clone(), message(101)])),
    ])
    .await;
    let batch = client
        .receive(
            &DiscordBotToken::parse("scan-fixture").unwrap(),
            "42",
            "100",
        )
        .await
        .unwrap();
    assert_eq!(batch.next_cursor, "103");
    assert!(batch.boundary_proven);
    assert_eq!(
        batch
            .messages
            .iter()
            .map(|m| m.id.as_str())
            .collect::<Vec<_>>(),
        vec!["101", "102", "103"]
    );
    assert!(batch.messages[1].is_bot);
    assert_eq!(batch.messages[1].webhook_id.as_deref(), Some("55"));
    assert_eq!(batch.messages[1].message_type, 19);
    assert!(fixture.calls.lock().unwrap()[1].0.contains("before=103"));
    assert!(fixture
        .calls
        .lock()
        .unwrap()
        .iter()
        .all(|c| !c.0.contains("after=")));
    task.abort();
}
#[tokio::test]
async fn discord_scan_never_advances_an_unproven_thousand_message_window() {
    for cursor in ["999", "1000"] {
        let mut responses = vec![json(serde_json::json!([message(2000)]))];
        let mut upper = 2000;
        for count in [100, 100, 100, 100, 100, 100, 100, 100, 100, 99] {
            let lower = upper - count;
            responses.push(json(serde_json::json!((lower..upper)
                .map(message)
                .collect::<Vec<_>>())));
            upper = lower;
        }
        responses.push(json(serde_json::json!([message(1000)])));
        let (client, _, task) = fixture(responses).await;
        let result = client
            .receive(
                &DiscordBotToken::parse("backlog-fixture").unwrap(),
                "42",
                cursor,
            )
            .await;
        if cursor == "999" {
            assert_eq!(result.unwrap_err(), DiscordTransportError::BacklogLimit);
        } else {
            let batch = result.unwrap();
            assert!(batch.boundary_proven);
            assert_eq!(batch.messages.len(), 1000);
            assert_eq!(batch.next_cursor, "2000");
        }
        task.abort();
    }
}
#[tokio::test]
async fn discord_scan_rejects_conflicting_duplicate_and_nonprogressing_pages() {
    for page in [serde_json::json!([message(103)]), {
        let mut changed = message(102);
        changed["content"] = serde_json::json!("changed");
        serde_json::json!([message(102), changed])
    }] {
        let (client, _, task) =
            fixture(vec![json(serde_json::json!([message(103)])), json(page)]).await;
        assert_eq!(
            client
                .receive(
                    &DiscordBotToken::parse("invalid-scan-fixture").unwrap(),
                    "42",
                    "100"
                )
                .await
                .unwrap_err(),
            DiscordTransportError::InvalidResponse
        );
        task.abort();
    }
}
#[tokio::test]
async fn discord_send_suppresses_all_mentions_and_classifies_ambiguous_success() {
    let (client, fixture, task) = fixture(vec![
        json(serde_json::json!({"id":"88","channel_id":"42"})),
        json(serde_json::json!({"id":"89","channel_id":"43"})),
    ])
    .await;
    let token = DiscordBotToken::parse("send-fixture").unwrap();
    assert_eq!(
        client
            .send_chunk(&token, "42", "@everyone <@9>", "outbound:0")
            .await
            .unwrap()
            .message_id,
        "88"
    );
    assert_eq!(
        client
            .send_chunk(&token, "42", "tail", "outbound:1")
            .await
            .unwrap_err(),
        DiscordTransportError::Uncertain
    );
    let calls = fixture.calls.lock().unwrap();
    assert_eq!(calls[0].1, "Bot send-fixture");
    assert!(!calls[0].0.contains("fixture"));
    let body = &calls[0].2;
    assert_eq!(
        body["allowed_mentions"],
        serde_json::json!({"parse":[],"replied_user":false})
    );
    assert_eq!(body["enforce_nonce"], true);
    assert!(body["nonce"].as_str().unwrap().len() <= 25);
    task.abort();
}
#[tokio::test]
async fn discord_native_selection_and_post_errors_are_static_and_fail_closed() {
    let token = DiscordBotToken::parse("selection-fixture").unwrap();
    for channel in [
        serde_json::json!({"id":"43","guild_id":"7","type":0}),
        serde_json::json!({"id":"42","guild_id":"7","type":1}),
        serde_json::json!({"id":"42","guild_id":"0","type":0}),
    ] {
        let (client, _, task) = fixture(vec![json(channel)]).await;
        assert_eq!(
            client.get_channel(&token, "42").await.unwrap_err(),
            DiscordTransportError::InvalidResponse
        );
        task.abort();
    }
    for (status, body, error) in [
        (
            401,
            "secret upstream body",
            DiscordTransportError::Authentication,
        ),
        (
            403,
            "secret upstream body",
            DiscordTransportError::PermissionDenied,
        ),
        (
            400,
            "secret upstream body",
            DiscordTransportError::HttpStatus { status: 400 },
        ),
        (
            500,
            "secret upstream body",
            DiscordTransportError::Uncertain,
        ),
        (
            200,
            "not-json secret upstream body",
            DiscordTransportError::Uncertain,
        ),
    ] {
        let (client, _, task) = fixture(vec![(status, body.into(), Vec::new())]).await;
        let observed = client
            .send_chunk(&token, "42", "fixture", "attempt")
            .await
            .unwrap_err();
        assert_eq!(observed, error);
        assert!(!format!("{observed:?} {observed}").contains("secret"));
        task.abort();
    }
}
#[tokio::test]
async fn discord_http_bounds_redirect_and_shared_rate_deadline_do_not_leak_payloads() {
    let (client, capture, task) = fixture(vec![
        (
            429,
            r#"{"retry_after":0.2,"global":true}"#.into(),
            vec![("retry-after".into(), "3600".into())],
        ),
        json(serde_json::json!({"id":"88","channel_id":"42"})),
    ])
    .await;
    let token = DiscordBotToken::parse("rate-fixture").unwrap();
    assert_eq!(
        client
            .send_chunk(&token, "42", "one", "a")
            .await
            .unwrap_err(),
        DiscordTransportError::RateLimited {
            retry_after_ms: 3_600_000,
            global: true
        }
    );
    let another = DiscordClient::for_test_base(client.base.as_str()).unwrap();
    let deferred = tokio::time::timeout(
        Duration::from_secs(1),
        another.send_chunk(&token, "42", "two", "b"),
    )
    .await
    .expect("a long provider deadline must release the caller without dispatch")
    .unwrap_err();
    assert!(matches!(
        deferred,
        DiscordTransportError::RateLimited {
            retry_after_ms: 1..=3_600_000,
            ..
        }
    ));
    assert_eq!(capture.calls.lock().unwrap().len(), 1);
    task.abort();
    for (status, body, headers, error) in [
        (
            302,
            "".into(),
            vec![("location".into(), "http://127.0.0.1:1/secret".into())],
            DiscordTransportError::HttpStatus { status: 302 },
        ),
        (
            200,
            "x".repeat(RESPONSE_LIMIT + 1),
            Vec::new(),
            DiscordTransportError::ResponseTooLarge,
        ),
        (
            200,
            r#"{"id":42,"bot":true}"#.into(),
            Vec::new(),
            DiscordTransportError::InvalidResponse,
        ),
    ] {
        let (client, fixture, task) = fixture(vec![(status, body, headers)]).await;
        assert_eq!(
            client
                .get_me(&DiscordBotToken::parse("bounds-fixture").unwrap())
                .await
                .unwrap_err(),
            error
        );
        assert_eq!(fixture.calls.lock().unwrap().len(), 1);
        task.abort();
    }
}
#[test]
fn discord_snowflakes_are_canonical_exact_u64_and_rate_delays_never_clamp_down() {
    for invalid in ["0", "01", "-1", "1.0", "18446744073709551616"] {
        assert!(DiscordSnowflake::parse(invalid).is_none());
    }
    assert_eq!(
        DiscordSnowflake::parse("18446744073709551615")
            .unwrap()
            .value(),
        u64::MAX
    );
    assert_eq!(DiscordSnowflake::cursor("0"), Some(0));
    assert_eq!(
        serde_json::to_string(&DiscordSnowflake::parse("9007199254740993").unwrap()).unwrap(),
        r#""9007199254740993""#
    );
    assert!(serde_json::from_str::<DiscordSnowflake>("9007199254740993").is_err());
    assert_eq!(seconds_ms(0.0001), Some(1));
    assert_eq!(seconds_ms(f64::MAX), Some(u64::MAX));
    assert_eq!(seconds_ms(f64::INFINITY), None);
}

fn guild_channel(id: &str, kind: u8) -> serde_json::Value {
    serde_json::json!({"id":id,"guild_id":"7","name":"planning","type":kind,
        "topic":null,"parent_id":null,"position":0})
}

#[tokio::test]
async fn native_guild_inventory_validates_bounds_guild_and_unique_canonical_ids() {
    let token = DiscordBotToken::parse("guild-inventory-fixture").unwrap();
    let inventory: Vec<_> = [0, 2, 4, 5, 10, 11, 12, 13, 14, 15, 16]
        .into_iter()
        .enumerate()
        .map(|(i, kind)| guild_channel(&(100 + i).to_string(), kind))
        .collect();
    let (client, capture, task) = fixture(vec![json(serde_json::json!(inventory))]).await;
    let channels = client.list_guild_channels(&token, "7").await.unwrap();
    assert_eq!(channels.len(), 11);
    assert_eq!(
        capture.calls.lock().unwrap()[0].0,
        "/api/v10/guilds/7/channels"
    );
    assert_eq!(capture.methods.lock().unwrap().as_slice(), ["GET"]);
    task.abort();
    for invalid in [
        serde_json::json!([guild_channel("42", 0), guild_channel("42", 0)]),
        serde_json::json!((100..601)
            .map(|id| guild_channel(&id.to_string(), 0))
            .collect::<Vec<_>>()),
        {
            let mut c = guild_channel("42", 0);
            c["guild_id"] = "8".into();
            serde_json::json!([c])
        },
        serde_json::json!([guild_channel("01", 0)]),
        serde_json::json!([guild_channel("42", 1)]),
        {
            let mut c = guild_channel("42", 0);
            c["parent_id"] = "42".into();
            serde_json::json!([c])
        },
    ] {
        let (client, capture, task) = fixture(vec![json(invalid)]).await;
        assert_eq!(
            client.list_guild_channels(&token, "7").await.unwrap_err(),
            DiscordTransportError::InvalidResponse
        );
        assert_eq!(capture.calls.lock().unwrap().len(), 1);
        task.abort();
    }
}

#[tokio::test]
async fn native_guild_create_and_edit_send_only_requested_fields_and_validate_acknowledgements() {
    let token = DiscordBotToken::parse("guild-mutations-fixture").unwrap();
    let mut created = guild_channel("42", 0);
    created["topic"] = "Roadmap".into();
    created["parent_id"] = "40".into();
    let mut edited = guild_channel("42", 0);
    edited["name"] = "shipping".into();
    let (client, capture, task) = fixture(vec![json(created), json(edited)]).await;
    let create = super::super::DiscordChannelCreate {
        name: "planning".into(),
        kind: 0,
        topic: Some("Roadmap".into()),
        parent_id: Some("40".into()),
    };
    assert_eq!(
        client
            .create_guild_channel(&token, "7", &create)
            .await
            .unwrap()
            .id,
        "42"
    );
    let edit = super::super::DiscordChannelEdit {
        name: Some("shipping".into()),
        topic: None,
        parent_id: Some(None),
    };
    assert_eq!(
        client
            .edit_guild_channel(&token, "7", "42", &edit)
            .await
            .unwrap()
            .name,
        "shipping"
    );
    let calls = capture.calls.lock().unwrap();
    assert_eq!(calls[0].0, "/api/v10/guilds/7/channels");
    assert_eq!(
        calls[0].2,
        serde_json::json!({"name":"planning","type":0,"topic":"Roadmap","parent_id":"40"})
    );
    assert_eq!(calls[1].0, "/api/v10/channels/42");
    assert_eq!(
        calls[1].2,
        serde_json::json!({"name":"shipping","parent_id":null})
    );
    assert_eq!(
        capture.methods.lock().unwrap().as_slice(),
        ["POST", "PATCH"]
    );
    task.abort();
}

#[tokio::test]
async fn native_guild_topic_clearing_accepts_null_or_empty_but_nonempty_topics_match_exactly() {
    let token = DiscordBotToken::parse("guild-topic-clearing-fixture").unwrap();
    for (requested, actual, confirmed) in [
        ("", None, true),
        ("", Some(""), true),
        ("", Some("leftover"), false),
        ("Roadmap", None, false),
        ("Roadmap", Some(""), false),
        ("Roadmap", Some("Other"), false),
        ("Roadmap", Some("Roadmap"), true),
    ] {
        let mut ack = guild_channel("42", 0);
        ack["topic"] = serde_json::json!(actual);
        let (client, capture, task) = fixture(vec![json(ack.clone()), json(ack)]).await;
        let create = super::super::DiscordChannelCreate {
            name: "planning".into(),
            kind: 0,
            topic: Some(requested.into()),
            parent_id: None,
        };
        let created = client.create_guild_channel(&token, "7", &create).await;
        let edit = super::super::DiscordChannelEdit {
            topic: Some(requested.into()),
            ..Default::default()
        };
        let edited = client.edit_guild_channel(&token, "7", "42", &edit).await;
        for result in [created, edited] {
            if confirmed {
                assert_eq!(result.unwrap().topic.as_deref(), actual);
            } else {
                assert_eq!(result.unwrap_err(), DiscordTransportError::Uncertain);
            }
        }
        let calls = capture.calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].2["topic"], requested);
        assert_eq!(calls[1].2, serde_json::json!({"topic":requested}));
        task.abort();
    }
}

#[tokio::test]
async fn native_guild_mutations_do_not_retry_or_claim_mismatched_success() {
    let token = DiscordBotToken::parse("guild-ack-fixture").unwrap();
    let create = super::super::DiscordChannelCreate {
        name: "planning".into(),
        kind: 0,
        topic: None,
        parent_id: None,
    };
    for response in [
        {
            let mut c = guild_channel("42", 0);
            c["guild_id"] = "8".into();
            json(c)
        },
        {
            let mut c = guild_channel("42", 0);
            c["name"] = "other".into();
            json(c)
        },
        json(guild_channel("42", 2)),
        {
            let mut c = guild_channel("42", 0);
            c["parent_id"] = "40".into();
            json(c)
        },
        (500, "upstream secret".into(), vec![]),
        (200, "malformed".into(), vec![]),
    ] {
        let (client, capture, task) = fixture(vec![response]).await;
        assert_eq!(
            client
                .create_guild_channel(&token, "7", &create)
                .await
                .unwrap_err(),
            DiscordTransportError::Uncertain
        );
        assert_eq!(
            capture.calls.lock().unwrap().len(),
            1,
            "writes must never be retried"
        );
        task.abort();
    }
    for ack in [
        guild_channel("43", 0),
        {
            let mut c = guild_channel("42", 0);
            c["guild_id"] = "8".into();
            c
        },
        {
            let mut c = guild_channel("42", 0);
            c["topic"] = "other".into();
            c
        },
        {
            let mut c = guild_channel("42", 0);
            c["parent_id"] = "40".into();
            c
        },
    ] {
        let (client, capture, task) = fixture(vec![json(ack)]).await;
        let edit = super::super::DiscordChannelEdit {
            name: None,
            topic: Some(String::new()),
            parent_id: Some(None),
        };
        assert_eq!(
            client
                .edit_guild_channel(&token, "7", "42", &edit)
                .await
                .unwrap_err(),
            DiscordTransportError::Uncertain
        );
        assert_eq!(capture.calls.lock().unwrap().len(), 1);
        task.abort();
    }
}

#[tokio::test]
async fn native_guild_patch_provider_failures_remain_truthful_and_never_retry() {
    let edit = super::super::DiscordChannelEdit {
        name: Some("shipping".into()),
        ..Default::default()
    };
    for (index, (status, body, expected)) in [
        (
            403,
            "denied".to_string(),
            DiscordTransportError::PermissionDenied,
        ),
        (
            500,
            "secret upstream body".to_string(),
            DiscordTransportError::Uncertain,
        ),
        (
            200,
            "malformed".to_string(),
            DiscordTransportError::Uncertain,
        ),
        (
            200,
            "x".repeat(RESPONSE_LIMIT + 1),
            DiscordTransportError::Uncertain,
        ),
        (
            429,
            r#"{"retry_after":1,"global":false}"#.to_string(),
            DiscordTransportError::RateLimited {
                retry_after_ms: 1000,
                global: false,
            },
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let token = DiscordBotToken::parse(&format!("guild-patch-error-fixture-{index}")).unwrap();
        let (client, capture, task) = fixture(vec![(status, body, vec![])]).await;
        assert_eq!(
            client
                .edit_guild_channel(&token, "7", "42", &edit)
                .await
                .unwrap_err(),
            expected
        );
        assert_eq!(capture.calls.lock().unwrap().len(), 1);
        assert_eq!(capture.methods.lock().unwrap().as_slice(), ["PATCH"]);
        task.abort();
    }
}

#[tokio::test]
async fn native_guild_invalid_requests_never_dispatch() {
    let token = DiscordBotToken::parse("guild-invalid-fixture").unwrap();
    let (client, capture, task) = fixture(vec![]).await;
    for (name, kind, topic, parent) in [
        ("".into(), 0, None, None),
        (" ".into(), 0, None, None),
        ("🦀".repeat(51), 0, None, None),
        ("bad\nname".into(), 0, None, None),
        ("forum".into(), 15, None, None),
        ("voice".into(), 2, Some("topic".into()), None),
        ("category".into(), 4, None, Some("40".into())),
        ("text".into(), 0, Some("x".repeat(1025)), None),
        ("text".into(), 0, None, Some("01".into())),
    ] {
        let create = super::super::DiscordChannelCreate {
            name,
            kind,
            topic,
            parent_id: parent,
        };
        assert_eq!(
            client
                .create_guild_channel(&token, "7", &create)
                .await
                .unwrap_err(),
            DiscordTransportError::InvalidInput
        );
    }
    let edit = super::super::DiscordChannelEdit {
        name: None,
        topic: None,
        parent_id: None,
    };
    assert_eq!(
        client
            .edit_guild_channel(&token, "7", "42", &edit)
            .await
            .unwrap_err(),
        DiscordTransportError::InvalidInput
    );
    let edit = super::super::DiscordChannelEdit {
        parent_id: Some(Some("42".into())),
        ..edit
    };
    assert_eq!(
        client
            .edit_guild_channel(&token, "7", "42", &edit)
            .await
            .unwrap_err(),
        DiscordTransportError::InvalidInput
    );
    assert_eq!(
        client
            .list_guild_channels(&token, "7/evil")
            .await
            .unwrap_err(),
        DiscordTransportError::InvalidInput
    );
    assert!(capture.calls.lock().unwrap().is_empty());
    task.abort();
}

#[tokio::test]
async fn native_management_owner_is_only_the_personal_or_team_owner() {
    let token = DiscordBotToken::parse("management-owner-fixture").unwrap();
    for (ownership, expected) in [
        (serde_json::json!({"owner":{"id":"44"}}), Some("44")),
        (
            serde_json::json!({"owner":{"id":"99"},"team":{"owner_user_id":"55","members":[{"membership_state":2,"user":{"id":"44"}}]}}),
            Some("55"),
        ),
        (
            serde_json::json!({"team":{"members":[{"membership_state":2,"user":{"id":"44"}}]}}),
            None,
        ),
    ] {
        let mut application = ownership;
        application["id"] = "11".into();
        let (client, _, task) = fixture(vec![
            json(application),
            json(serde_json::json!({"id":"11","bot":true})),
        ])
        .await;
        assert_eq!(
            client
                .check_application(&token)
                .await
                .unwrap()
                .management_owner_user_id
                .as_deref(),
            expected
        );
        task.abort();
    }
    for ownership in [
        serde_json::json!({"owner":{"id":"01"}}),
        serde_json::json!({"team":{"owner_user_id":"0","members":[]}}),
    ] {
        let mut application = ownership;
        application["id"] = "11".into();
        let (client, _, task) = fixture(vec![
            json(application),
            json(serde_json::json!({"id":"11","bot":true})),
        ])
        .await;
        assert_eq!(
            client.check_application(&token).await.unwrap_err(),
            DiscordTransportError::InvalidResponse
        );
        task.abort();
    }
}

#[tokio::test]
async fn incoming_user_text_is_not_limited_to_bot_reply_size() {
    let mut incoming = message(1);
    let body = "λ".repeat(4_000);
    incoming["content"] = serde_json::json!(body);
    let (client, _, task) = fixture(vec![
        json(serde_json::json!([incoming])),
        json(serde_json::json!([])),
    ])
    .await;
    let batch = client
        .receive(
            &DiscordBotToken::parse("long-input-fixture").unwrap(),
            "42",
            "0",
        )
        .await
        .unwrap();
    assert!(batch.boundary_proven);
    assert_eq!(batch.messages[0].text, body);
    assert_eq!(batch.next_cursor, "1");
    task.abort();
}
