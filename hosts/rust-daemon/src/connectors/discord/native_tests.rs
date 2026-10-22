use super::*;
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
}
async fn handler(State(fixture): State<Arc<Fixture>>, request: Request<Body>) -> Response {
    fixture.user_agents.lock().unwrap().push(
        request
            .headers()
            .get("user-agent")
            .map(|value| value.to_str().unwrap().to_owned()),
    );
    let uri = request.uri().to_string();
    let auth = request
        .headers()
        .get(AUTHORIZATION)
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let body = to_bytes(request.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
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
    assert_eq!(check.invite_url,"https://discord.com/oauth2/authorize?client_id=11&scope=bot&permissions=309237713920&integration_type=0");
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
