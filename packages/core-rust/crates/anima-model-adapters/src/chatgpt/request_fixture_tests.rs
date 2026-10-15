//! Opt-in synthetic native-to-core handoff. This is not a production importer.
use super::*;
use anima_core::Message;
use std::{collections::BTreeSet, io::Read, path::Path};

const MAX_FIXTURE_BYTES: usize = 512 * 1024;
const INPUT_MARKER: &str = "M48_SYNTHETIC_MANAGER_FIRST_REQUEST";

fn exact_keys(value: &Value, names: &[&str]) -> bool {
    value.as_object().is_some_and(|object| {
        object.keys().map(String::as_str).collect::<BTreeSet<_>>()
            == names.iter().copied().collect()
    })
}

fn decode_fixture(bytes: &[u8]) -> Result<(AgentConfig, ModelGenerateRequest), &'static str> {
    if bytes.len() > MAX_FIXTURE_BYTES {
        return Err("synthetic fixture exceeds its bound");
    }
    let value: Value =
        serde_json::from_slice(bytes).map_err(|_| "invalid synthetic fixture JSON")?;
    if !exact_keys(&value, &["fixtureVersion", "config", "request"])
        || value["fixtureVersion"].as_u64() != Some(1)
        || !exact_keys(
            &value["request"],
            &["system", "messages", "temperature", "maxTokens"],
        )
    {
        return Err("unsupported synthetic fixture envelope");
    }
    let config: AgentConfig = serde_json::from_value(value["config"].clone())
        .map_err(|_| "invalid synthetic fixture config")?;
    let request = &value["request"];
    let system = request["system"]
        .as_str()
        .filter(|text| !text.is_empty())
        .ok_or("missing synthetic fixture system")?
        .to_owned();
    let messages: Vec<Message> = serde_json::from_value(request["messages"].clone())
        .map_err(|_| "invalid synthetic fixture messages")?;
    if messages.len() != 1
        || messages[0].role != MessageRole::User
        || messages[0].content.text != INPUT_MARKER
        || messages[0]
            .content
            .attachments
            .as_ref()
            .is_some_and(|items| !items.is_empty())
    {
        return Err("foreign synthetic fixture conversation");
    }
    let temperature = if request["temperature"].is_null() {
        None
    } else {
        Some(
            request["temperature"]
                .as_f64()
                .filter(|value| value.is_finite())
                .ok_or("invalid synthetic fixture temperature")?,
        )
    };
    let max_tokens = if request["maxTokens"].is_null() {
        None
    } else {
        Some(
            request["maxTokens"]
                .as_u64()
                .and_then(|value| u32::try_from(value).ok())
                .ok_or("invalid synthetic fixture token limit")?,
        )
    };
    Ok((
        config,
        ModelGenerateRequest {
            system,
            messages,
            temperature,
            max_tokens,
        },
    ))
}

fn read_fixture(path: &Path) -> Result<(AgentConfig, ModelGenerateRequest), &'static str> {
    if !path.is_absolute() {
        return Err("synthetic fixture path must be absolute");
    }
    let file = std::fs::File::open(path).map_err(|_| "synthetic fixture file is unavailable")?;
    let metadata = file
        .metadata()
        .map_err(|_| "synthetic fixture metadata is unavailable")?;
    if !metadata.is_file() || metadata.len() > MAX_FIXTURE_BYTES as u64 {
        return Err("synthetic fixture must be a bounded regular file");
    }
    let mut bytes = Vec::new();
    file.take((MAX_FIXTURE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "synthetic fixture read failed")?;
    decode_fixture(&bytes)
}

#[test]
fn m48_provider_request_fixture_rejects_foreign_or_malformed_envelopes() {
    let message = Message {
        id: "synthetic-message".into(),
        agent_id: "synthetic-agent".into(),
        room_id: "synthetic-room".into(),
        role: MessageRole::User,
        created_at_ms: 1,
        content: Content {
            text: INPUT_MARKER.into(),
            ..Content::default()
        },
    };
    let valid = json!({"fixtureVersion":1,"config":{"name":"M48 synthetic manager","model":"gpt-5.5"},
        "request":{"system":"Synthetic capture only","messages":[message],"temperature":null,"maxTokens":2048}});
    let (_, request) = decode_fixture(&serde_json::to_vec(&valid).unwrap()).unwrap();
    assert_eq!(request.messages[0].content.text, INPUT_MARKER);
    assert_eq!(request.max_tokens, Some(2048));
    let mut foreign_version = valid.clone();
    foreign_version["fixtureVersion"] = json!(2);
    let mut extra = valid.clone();
    extra["endpoint"] = json!("synthetic-private");
    let mut request_extra = valid.clone();
    request_extra["request"]["endpoint"] = json!("synthetic-private");
    let mut missing = valid.clone();
    missing["request"]
        .as_object_mut()
        .unwrap()
        .remove("temperature");
    let mut foreign_marker = valid.clone();
    foreign_marker["request"]["messages"][0]["content"]["text"] = json!("foreign");
    let mut bad_limit = valid.clone();
    bad_limit["request"]["maxTokens"] = json!(2048.5);
    for value in [
        foreign_version,
        extra,
        request_extra,
        missing,
        foreign_marker,
        bad_limit,
    ] {
        assert!(decode_fixture(&serde_json::to_vec(&value).unwrap()).is_err());
    }
    assert!(decode_fixture(b"{\"fixtureVersion\":1").is_err());
    assert!(decode_fixture(&vec![b' '; MAX_FIXTURE_BYTES + 1]).is_err());
    assert!(read_fixture(Path::new("relative-fixture.json")).is_err());
}

fn assert_integer_schema_bounds(value: &Value) {
    match value {
        Value::Object(fields) => {
            for (key, value) in fields {
                if [
                    "minLength",
                    "maxLength",
                    "minItems",
                    "maxItems",
                    "minProperties",
                    "maxProperties",
                    "minContains",
                    "maxContains",
                ]
                .contains(&key.as_str())
                {
                    assert!(
                        value.as_u64().is_some(),
                        "structural integer schema bound must be an integer"
                    );
                } else if !["enum", "const", "default", "example", "examples"]
                    .contains(&key.as_str())
                {
                    assert_integer_schema_bounds(value);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                assert_integer_schema_bounds(value);
            }
        }
        _ => {}
    }
}

#[tokio::test]
#[ignore = "root-only explicit synthetic native fixture; joined loopback, never provider"]
async fn m48_provider_request_import_uses_subscription_wire() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    let path = std::env::var_os("ANIMAOS_M48_REQUEST_FIXTURE")
        .map(std::path::PathBuf::from)
        .expect("root supplies an explicit synthetic fixture");
    let (config, request) = read_fixture(&path).expect("native fixture must match version one");
    assert_eq!(config.name, "M48 synthetic manager");
    assert_eq!(config.provider.as_deref(), Some("chatgpt"));
    assert_eq!(config.model, "gpt-5.5");
    let settings = config.settings.as_ref().unwrap();
    assert_eq!(settings.max_tool_iterations, Some(1));
    assert_eq!(settings.max_retries, Some(0));
    assert_eq!(settings.timeout_ms, Some(20_000));
    assert_eq!(settings.max_tokens, Some(2048));
    assert_eq!(request.temperature, None);
    assert_eq!(request.max_tokens, Some(2048));
    assert!(request
        .system
        .contains("M48_SYNTHETIC_REQUEST_CONTEXT: Reply M48_CAPTURE_OK without calling tools."));
    let wire = request_body(&config, &request).unwrap();
    assert_eq!(wire["model"], config.model);
    assert!(
        wire["instructions"] == request.system,
        "instructions must preserve the actual captured system"
    );
    assert_eq!(
        wire["input"],
        json!([{"role":"user","content":[{"type":"input_text","text":INPUT_MARKER}]}])
    );
    assert_eq!(wire["stream"], true);
    assert_eq!(wire["store"], false);
    assert_eq!(wire["tool_choice"], "auto");
    assert_eq!(wire["parallel_tool_calls"], true);
    for key in [
        "temperature",
        "max_tokens",
        "max_output_tokens",
        "messages",
        "api_key",
        "endpoint",
    ] {
        assert!(
            wire.get(key).is_none(),
            "API-only controls must not appear on subscription wire"
        );
    }
    let tools = wire["tools"].as_array().unwrap();
    let names = tools
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect::<BTreeSet<_>>();
    let expected = [
        "read_file",
        "list_dir",
        "glob",
        "grep",
        "write_file",
        "edit_file",
        "todo_read",
        "todo_write",
        "spawn_helper",
        "bash",
        "list_workspace_agents",
        "send_message",
        "broadcast_message",
        "delegate_to_agent",
    ]
    .into_iter()
    .collect::<BTreeSet<_>>();
    assert_eq!(tools.len(), 14);
    assert_eq!(names, expected);
    for descriptor in config.tools.as_ref().unwrap() {
        let tool = tools
            .iter()
            .find(|tool| tool["name"].as_str() == Some(descriptor.name.as_str()))
            .unwrap();
        assert_eq!(tool["type"], "function");
        assert_eq!(tool["strict"], false);
        assert_eq!(tool["description"], descriptor.description);
        assert_eq!(tool["parameters"], tool_parameters_schema_json(descriptor));
        assert_integer_schema_bounds(&tool["parameters"]);
    }
    let todo = &tools
        .iter()
        .find(|tool| tool["name"] == "todo_write")
        .unwrap()["parameters"];
    assert_eq!(todo["type"], "object");
    assert_eq!(todo["required"], json!(["todos"]));
    assert_eq!(todo["properties"]["todos"]["type"], "array");
    let item = &todo["properties"]["todos"]["items"];
    assert_eq!(item["type"], "object");
    assert_eq!(
        item["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect::<BTreeSet<_>>(),
        ["content", "status", "activeForm"].into_iter().collect()
    );
    assert_eq!(
        item["properties"]["status"]["enum"],
        json!(["pending", "in_progress", "completed"])
    );
    assert_eq!(item["properties"]["content"]["minLength"].as_u64(), Some(1));
    assert_eq!(
        item["properties"]["activeForm"]["minLength"].as_u64(),
        Some(1)
    );

    let attempts = Arc::new(AtomicUsize::new(0));
    let captured = Arc::new(Mutex::new(None));
    let app = axum::Router::new().route("/responses", axum::routing::post({
        let attempts = attempts.clone(); let captured = captured.clone();
        move |axum::Json(body): axum::Json<Value>| { let attempts = attempts.clone(); let captured = captured.clone(); async move {
            attempts.fetch_add(1, Ordering::SeqCst); *captured.lock().unwrap() = Some(body);
            let done = json!({"type":"response.completed","response":{"status":"completed",
                "output":[{"type":"message","content":[{"type":"output_text","text":"M48_WIRE_OK"}]}],
                "usage":{"input_tokens":2,"output_tokens":1,"total_tokens":3}}});
            ([ ("content-type", "text/event-stream") ], format!("data: {done}\n\n"))
        }}
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (shutdown, stopped) = tokio::sync::oneshot::channel::<()>();
    let mut server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
            .unwrap();
    });
    let mut adapter =
        ChatGptResponsesAdapter::new("m48-fake-subscription".into(), "m48-fake-account".into())
            .unwrap();
    adapter.endpoint = format!("http://{address}/responses");
    let result =
        tokio::time::timeout(Duration::from_secs(5), adapter.generate(&config, &request)).await;
    let _ = shutdown.send(());
    let joined = tokio::time::timeout(Duration::from_secs(3), &mut server).await;
    let clean_join = matches!(joined, Ok(Ok(())));
    if !clean_join {
        server.abort();
        let _ = server.await;
    }
    assert!(
        clean_join,
        "the local HTTP fixture must be joined before assertions"
    );
    let response = result.unwrap().unwrap();
    assert_eq!(response.content.text, "M48_WIRE_OK");
    assert_eq!(response.usage.total_tokens, 3);
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    assert!(
        captured.lock().unwrap().as_ref() == Some(&wire),
        "loopback JSON must match the actual serializer"
    );
    println!(
        "M48_PRIVATE_WIRE {}",
        json!({"fixtureVersion":1,"tools":tools.len(),"userInputs":1,
        "attempts":1,"stream":true,"store":false,"strict":false,"joined":true})
    );
}
