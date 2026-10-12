use super::*;
use anima_core::{Content, DataValue, Message, MessageRole, ModelStopReason, ToolDescriptor};
use serde_json::json;
use std::collections::BTreeMap;

fn wire_tool_schema(schema: Value) -> Value {
    let mut config = config();
    config.tools.as_mut().unwrap()[0].parameters_schema =
        crate::common::json_value_to_data_map(&schema).unwrap();
    request_body(&config, &request()).unwrap()["tools"][0]["parameters"].clone()
}

#[test]
fn tool_schema_integer_bounds_are_canonical_through_structural_schema_fields() {
    let bounds = json!({"minLength":1.0,"maxLength":2.0,"minItems":3.0,"maxItems":4.0,"minProperties":5.0,"maxProperties":6.0,"minContains":7.0,"maxContains":8.0});
    let mut schema = json!({"type":"object","properties":{"query":{"type":"string","minLength":1.0},"list":{"type":"array","minItems":1.0,"items":{"minLength":1.0}}}});
    for field in [
        "properties",
        "patternProperties",
        "$defs",
        "definitions",
        "dependentSchemas",
        "dependencies",
    ] {
        schema[field]["nested"] = bounds.clone();
    }
    for field in [
        "contains",
        "additionalProperties",
        "additionalItems",
        "propertyNames",
        "not",
        "if",
        "then",
        "else",
        "unevaluatedProperties",
        "unevaluatedItems",
        "contentSchema",
    ] {
        schema[field] = bounds.clone();
    }
    for field in ["allOf", "anyOf", "oneOf", "prefixItems"] {
        schema[field] = json!([bounds.clone(), true, false]);
    }
    let wire = wire_tool_schema(schema);
    for field in [
        "properties",
        "patternProperties",
        "$defs",
        "definitions",
        "dependentSchemas",
        "dependencies",
    ] {
        for (keyword, number) in bounds.as_object().unwrap() {
            assert_eq!(
                wire[field]["nested"][keyword].as_u64(),
                Some(number.as_f64().unwrap() as u64),
                "{field}.{keyword}"
            );
        }
    }
    for field in [
        "contains",
        "additionalProperties",
        "additionalItems",
        "propertyNames",
        "not",
        "if",
        "then",
        "else",
        "unevaluatedProperties",
        "unevaluatedItems",
        "contentSchema",
    ] {
        assert_eq!(wire[field]["minItems"].as_u64(), Some(3), "{field}");
    }
    for field in ["allOf", "anyOf", "oneOf", "prefixItems"] {
        assert_eq!(wire[field][0]["maxLength"].as_u64(), Some(2));
        assert_eq!(wire[field][1], true);
        assert_eq!(wire[field][2], false);
    }
    assert_eq!(wire["properties"]["query"]["minLength"].as_u64(), Some(1));
    assert_eq!(
        wire["properties"]["list"]["items"]["minLength"].as_u64(),
        Some(1)
    );
    let tuple = wire_tool_schema(json!({"type":"array","items":[{"minLength":1.0},false]}));
    assert_eq!(tuple["items"][0]["minLength"].as_u64(), Some(1));
    assert_eq!(tuple["items"][1], false);
    let legacy =
        wire_tool_schema(json!({"query":{"type":"string","minLength":1.0},"opaque":false}));
    assert_eq!(legacy["type"], "object");
    assert_eq!(legacy["properties"]["query"]["minLength"].as_u64(), Some(1));
    assert_eq!(legacy["properties"]["opaque"], false);
}

#[test]
fn tool_schema_canonicalization_preserves_instance_numbers_and_noninteger_numeric_constraints() {
    let instance = json!({"minLength":1.0,"properties":{"nested":{"maxItems":2.0}}});
    let wire = wire_tool_schema(
        json!({"type":"object","minLength":1.5,"maxLength":-1.0,"minItems":9007199254740992.0,"maxItems":9007199254740991.0,"minimum":0.25,"maximum":3.75,"exclusiveMinimum":1.0,"multipleOf":0.5,"enum":[instance.clone()],"const":instance.clone(),"default":instance.clone(),"example":instance.clone(),"examples":[instance.clone()],"x-metadata":instance.clone()}),
    );
    assert_eq!(wire["minLength"].as_f64(), Some(1.5));
    assert!(wire["minLength"].is_f64());
    assert!(wire["maxLength"].is_f64());
    assert!(wire["minItems"].is_f64());
    assert_eq!(wire["maxItems"].as_u64(), Some(9007199254740991));
    for (keyword, number) in [
        ("minimum", 0.25),
        ("maximum", 3.75),
        ("exclusiveMinimum", 1.0),
        ("multipleOf", 0.5),
    ] {
        assert!(wire[keyword].is_f64());
        assert_eq!(wire[keyword].as_f64(), Some(number));
    }
    for keyword in ["const", "default", "example", "x-metadata"] {
        assert_eq!(wire[keyword], instance);
        assert!(wire[keyword]["minLength"].is_f64());
    }
    for keyword in ["enum", "examples"] {
        assert_eq!(wire[keyword][0], instance);
        assert!(wire[keyword][0]["minLength"].is_f64());
    }
    // Tool-call arguments and generic DataValue JSON retain their established representation.
    let data = DataValue::Object(BTreeMap::from([(
        "minLength".into(),
        DataValue::Number(1.0),
    )]));
    assert!(crate::common::data_value_to_json(&data)["minLength"].is_f64());
}

#[test]
fn subscription_schema_error_only_maps_bounded_indexed_tool_parameters_to_static_tools_label() {
    for parameter in [
        "tools",
        "tools[0].parameters",
        "tools[31].function.parameters.properties.query.minLength",
        "tools.2.parameters.properties.ids.items",
    ] {
        let error=structured_failure(&json!({"error":{"code":"invalid_function_parameters","param":parameter,"message":"secret-sentinel"}})).unwrap();
        assert!(error.contains("tool parameter schema"));
        assert!(error.contains("parameter: tools"));
        assert!(!error.contains("secret-sentinel"));
    }
    for parameter in [
        "tools[0].parameters.secret-sentinel",
        "tools[0].parameters\nsecret",
        "tools[0].parameters'https://secret'",
        "tools[0].parameters[secret]",
        &format!("tools[0].parameters.{}", "a".repeat(257)),
    ] {
        let error=structured_failure(&json!({"error":{"code":"invalid_function_parameters","param":parameter,"message":"secret-sentinel"}})).unwrap();
        assert!(!error.contains("parameter:"));
        assert!(!error.contains("secret-sentinel"));
    }
}

#[test]
fn subscription_failure_classifier_only_exposes_whitelisted_codes_and_parameters() {
    for (code, expected) in [
        ("invalid_prompt", "request was rejected"),
        ("model_not_found", "selected model"),
        ("unsupported_model", "selected model"),
        ("invalid_request_error", "request is invalid"),
        ("invalid_api_key", "authorization"),
        ("permission_denied", "permission"),
        ("rate_limit_exceeded", "rate limit"),
        ("insufficient_quota", "usage limit"),
        ("server_error", "temporarily unavailable"),
        ("context_length_exceeded", "context limit"),
        ("subscription_sharing_user_not_eligible", "not eligible"),
        ("subscription_sharing_usage_limit_exceeded", "usage limit"),
        (
            "subscription_sharing_usage_unavailable",
            "usage availability",
        ),
        (
            "subscription_sharing_unsupported_capability",
            "unsupported capability",
        ),
        (
            "subscription_sharing_route_not_supported",
            "route is unsupported",
        ),
        ("subscription_sharing_invalid_user", "subscriber context"),
        (
            "subscription_sharing_user_unavailable",
            "account availability",
        ),
        ("chatpass_v2_scope_not_authorized", "permission context"),
        (
            "chatpass_v2_invalid_authorization_context",
            "permission context",
        ),
    ] {
        let payload = json!({"response":{"error":{"code":code,"param":"model","message":"secret-sentinel account details"}}});
        let error = structured_failure(&payload).expect("known code");
        assert!(error.contains(expected), "{code}: {error}");
        assert!(error.contains(code));
        assert!(error.contains("parameter: model"));
        assert!(!error.contains("secret-sentinel"));
        assert!(!error.contains("resets"));
        assert!(!error.contains("at 20"));
    }
    for payload in [
        json!({"error":{"code":"secret-sentinel","param":"model","message":"model_not_found"}}),
        json!({"error":{"code":42,"param":"model"}}),
        json!({"error":"model_not_found"}),
        json!({"detail":"secret-sentinel model_not_found"}),
        json!({"code":"secret-sentinel","param":"model"}),
        json!({"type":"error","code":"secret-sentinel","error":{"code":"model_not_found"}}),
    ] {
        assert!(structured_failure(&payload).is_none());
    }
    let error=structured_failure(&json!({"error":{"code":"invalid_request_error","param":"secret-sentinel","message":"secret-sentinel"}})).unwrap();
    assert!(!error.contains("secret-sentinel"));
    assert!(!error.contains("parameter:"));
}

#[tokio::test]
async fn subscription_failed_stream_uses_response_error_and_top_level_code_without_final_tools() {
    for payload in [
        json!({"type":"response.failed","response":{"status":"failed","error":{"code":"invalid_prompt","message":"secret-sentinel"}}}),
        json!({"type":"response.failed","response":{"status":"failed","error":{"code":"model_not_found","param":"model","message":"secret-sentinel"}}}),
        json!({"type":"error","code":"subscription_sharing_unsupported_capability","param":"tools","message":"secret-sentinel"}),
        json!({"type":"error","code":"subscription_sharing_unsupported_capability","param":"tools","error":{"code":"secret-sentinel","message":"secret-sentinel"}}),
    ] {
        let body = format!("data: {payload}\n\n");
        let expected_code = payload["response"]["error"]["code"]
            .as_str()
            .or_else(|| payload["code"].as_str())
            .unwrap()
            .to_owned();
        let (endpoint, task) = server(axum::Router::new().route(
            "/responses",
            axum::routing::post(move || {
                let body = body.clone();
                async move { body }
            }),
        ))
        .await;
        let mut adapter = ChatGptResponsesAdapter::new("fake".into(), "account".into()).unwrap();
        adapter.endpoint = endpoint;
        let sink = FinalSink(Mutex::new(None));
        let error = adapter
            .stream(&config(), &request(), &sink)
            .await
            .unwrap_err();
        assert!(error.contains(&expected_code), "{error}");
        assert!(!error.contains("secret-sentinel"));
        assert!(sink.0.lock().unwrap().is_none());
        task.abort();
    }
}

#[tokio::test]
async fn subscription_http_errors_classify_bounded_chunked_json_and_preserve_status_fallbacks() {
    use axum::{http::StatusCode, routing::post};
    for (status,body,expected) in [
        (StatusCode::BAD_REQUEST,json!({"error":{"code":"subscription_sharing_unsupported_capability","param":"model","message":"secret-sentinel"}}).to_string(),"unsupported capability"),
        (StatusCode::SERVICE_UNAVAILABLE,json!({"error":{"code":"subscription_sharing_usage_unavailable","message":"secret-sentinel"}}).to_string(),"usage availability"),
        (StatusCode::SERVICE_UNAVAILABLE,json!({"detail":"secret-sentinel"}).to_string(),"HTTP 503"),
        (StatusCode::UNAUTHORIZED,"secret-sentinel".repeat(MAX_ERROR_BODY),"authorization expired"),
        (StatusCode::TOO_MANY_REQUESTS,"secret-sentinel malformed".into(),"usage limit reached"),
    ] {
        let (endpoint,task)=server(axum::Router::new().route("/responses",post(move||{let body=body.clone();async move {
            let chunks=body.as_bytes().chunks(8192).map(|bytes|Ok::<_,std::io::Error>(bytes.to_vec())).collect::<Vec<_>>();
            (status,axum::body::Body::from_stream(futures::stream::iter(chunks)))
        }}))).await;
        let mut adapter=ChatGptResponsesAdapter::new("fake".into(),"account".into()).unwrap();adapter.endpoint=endpoint;
        let error=adapter.generate(&config(),&request()).await.unwrap_err();assert!(error.contains(expected),"{error}");assert!(!error.contains("secret-sentinel"));assert!(!error.contains("resets"));task.abort();
    }
}

#[tokio::test]
async fn attachment_error_subscription_http_and_frames_keep_static_diagnostics() {
    for (status, body) in [
        (
            axum::http::StatusCode::BAD_REQUEST,
            "aGVsbG8= bare upstream fragment".to_owned(),
        ),
        (
            axum::http::StatusCode::OK,
            "data: {\"type\":\"error\",\"message\":\"aGVsbG8=\"}\n\n".to_owned(),
        ),
    ] {
        let (endpoint, task) = server(axum::Router::new().route(
            "/responses",
            axum::routing::post(move || {
                let body = body.clone();
                async move { (status, body) }
            }),
        ))
        .await;
        let mut adapter = ChatGptResponsesAdapter::new("fake".into(), "account".into()).unwrap();
        adapter.endpoint = endpoint;
        let mut image = request();
        image.messages[0].content.attachments = Some(vec![anima_core::Attachment {
            attachment_type: anima_core::AttachmentType::Image,
            name: "photo.png".into(),
            data: "data:image/png;base64,aGVsbG8=".into(),
            mime_type: Some("image/png".into()),
        }]);
        for request in [&image, &request()] {
            let error = adapter.generate(&config(), request).await.unwrap_err();
            assert!(!error.contains("aGVsbG8="));
            let error = adapter
                .stream(&config(), request, &FinalSink(Mutex::new(None)))
                .await
                .unwrap_err();
            assert!(!error.contains("aGVsbG8="));
        }
        task.abort();
    }
}

#[test]
fn attachment_subscription_request_keeps_images_and_tool_history() {
    let mut request = ModelGenerateRequest {
        system: "System".into(),
        temperature: None,
        max_tokens: None,
        messages: vec![message(MessageRole::User, "Describe", None)],
    };
    request.messages[0].content.attachments = Some(vec![
        anima_core::Attachment {
            attachment_type: anima_core::AttachmentType::Image,
            name: "photo.png".into(),
            data: "data:image/png;base64,aGVsbG8=".into(),
            mime_type: Some("image/png".into()),
        },
        anima_core::Attachment {
            attachment_type: anima_core::AttachmentType::Image,
            name: "photo.jpg".into(),
            data: "data:image/jpeg;base64,d29ybGQ=".into(),
            mime_type: Some("image/jpeg".into()),
        },
    ]);
    request.messages.extend([
        message(
            MessageRole::Assistant,
            "",
            Some(BTreeMap::from([(
                "toolCalls".into(),
                DataValue::Array(vec![DataValue::Object(BTreeMap::from([
                    ("id".into(), DataValue::String("call_image".into())),
                    ("name".into(), DataValue::String("lookup".into())),
                    ("args".into(), DataValue::Object(BTreeMap::new())),
                ]))]),
            )])),
        ),
        message(
            MessageRole::Tool,
            "record",
            Some(BTreeMap::from([(
                "toolCallId".into(),
                DataValue::String("call_image".into()),
            )])),
        ),
    ]);
    let body = request_body(&config(), &request).unwrap();
    assert_eq!(
        body["input"][0]["content"],
        json!([
            {"type":"input_text", "text":"Describe"},
            {"type":"input_image", "image_url":"data:image/png;base64,aGVsbG8="},
            {"type":"input_image", "image_url":"data:image/jpeg;base64,d29ybGQ="}
        ])
    );
    assert_eq!(body["input"][1]["type"], "function_call");
    assert_eq!(body["input"][1]["call_id"], "call_image");
    assert_eq!(body["input"][2]["type"], "function_call_output");
    assert_eq!(body["input"][2]["call_id"], "call_image");
    let mut unknown = config();
    unknown.model = "unknown".into();
    let body = request_body(&unknown, &request).unwrap();
    assert_eq!(body["input"][0]["content"].as_array().unwrap().len(), 1);
    assert!(body["input"][0]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("cannot view"));
    assert!(!body.to_string().contains("aGVsbG8="));
}

fn config() -> AgentConfig {
    AgentConfig {
        name: "subscription test".into(),
        model: "gpt-5.5".into(),
        provider: Some("chatgpt".into()),
        bio: None,
        lore: None,
        knowledge: None,
        topics: None,
        adjectives: None,
        style: None,
        system: None,
        plugins: None,
        settings: None,
        tools: Some(vec![ToolDescriptor {
            name: "lookup".into(),
            description: "Look up a record".into(),
            parameters_schema: BTreeMap::from([(
                "type".into(),
                DataValue::String("object".into()),
            )]),
            examples: None,
        }]),
    }
}

fn message(
    role: MessageRole,
    text: &str,
    metadata: Option<BTreeMap<String, DataValue>>,
) -> Message {
    Message {
        id: "message-1".into(),
        agent_id: "agent-1".into(),
        room_id: "room-1".into(),
        role,
        content: Content {
            text: text.into(),
            attachments: None,
            metadata,
        },
        created_at_ms: 0,
    }
}

#[test]
fn subscription_request_keeps_anima_tool_history_and_disables_storage() {
    let call = DataValue::Object(BTreeMap::from([
        ("id".into(), DataValue::String("call_1".into())),
        ("name".into(), DataValue::String("lookup".into())),
        ("args".into(), DataValue::Object(BTreeMap::new())),
    ]));
    let request = ModelGenerateRequest {
        system: "Use Anima permissions".into(),
        temperature: Some(0.7),
        max_tokens: Some(1000),
        messages: vec![
            message(MessageRole::User, "hello", None),
            message(
                MessageRole::Assistant,
                "",
                Some(BTreeMap::from([(
                    "toolCalls".into(),
                    DataValue::Array(vec![call]),
                )])),
            ),
            message(
                MessageRole::Tool,
                "record",
                Some(BTreeMap::from([(
                    "toolCallId".into(),
                    DataValue::String("call_1".into()),
                )])),
            ),
        ],
    };
    let body = request_body(&config(), &request).unwrap();
    assert_eq!(body["store"], false);
    assert_eq!(body["stream"], true);
    assert_eq!(body["instructions"], request.system);
    assert_eq!(body["input"][1]["type"], "function_call");
    assert_eq!(body["input"][1]["call_id"], "call_1");
    assert_eq!(body["input"][2]["type"], "function_call_output");
    assert_eq!(body["input"][2]["call_id"], "call_1");
    assert_eq!(body["tools"][0]["name"], "lookup");
    assert!(body.get("temperature").is_none());
    assert!(body.get("max_tokens").is_none());
}

#[test]
fn subscription_response_normalizes_tool_calls_and_usage() {
    let response = completed_response(&json!({"status":"completed", "output":[
        {"type":"message","content":[{"type":"output_text","text":"Checking"}]},
        {"type":"function_call","call_id":"call_1","name":"lookup","arguments":"{\"id\":7}"}
    ],"usage":{"input_tokens":10,"output_tokens":5,"total_tokens":15}}))
    .unwrap();
    assert_eq!(response.content.text, "Checking");
    assert_eq!(response.stop_reason, ModelStopReason::ToolCall);
    assert_eq!(response.usage.prompt_tokens, 10);
    assert_eq!(response.usage.completion_tokens, 5);
    assert_eq!(response.tool_calls.unwrap()[0].name, "lookup");
}

#[test]
fn subscription_rejects_failed_or_malformed_output() {
    for payload in [
        json!({"status":"completed","output":[]}),
        json!({"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"  "}]}]}),
        json!({"status":"incomplete","output":[{"type":"function_call","call_id":"call_1","name":"lookup","arguments":"{}"}]}),
        json!({"status":"failed","output":[]}),
        json!({"status":"completed","output":[{"type":"function_call","name":"lookup","arguments":"{}"}]}),
        json!({"status":"completed"}),
    ] {
        assert!(completed_response(&payload).is_err());
    }
}

#[tokio::test]
async fn subscription_retains_completed_items_when_terminal_output_is_empty() {
    use axum::routing::post;
    for terminal_output in [
        json!([]),
        Value::Null,
        json!([
            {"type":"message","content":[{"type":"output_text","text":"Checking"}]},
            {"type":"function_call","call_id":"call_1","name":"lookup","arguments":"{\"id\":7}"}
        ]),
    ] {
        let events = [
            json!({"type":"response.output_text.delta","delta":"Checking"}),
            json!({"type":"response.output_item.done","output_index":1,"item":{
                "type":"function_call","call_id":"call_1","name":"lookup","arguments":"{\"id\":7}"}}),
            json!({"type":"response.output_item.done","output_index":0,"item":{
                "type":"message","content":[{"type":"output_text","text":"Checking"}]}}),
            json!({"type":"response.completed","response":{"status":"completed",
                "output":terminal_output,"usage":{"input_tokens":2,"output_tokens":3}}}),
        ];
        let body = events
            .iter()
            .map(|event| format!("data: {event}\n\n"))
            .collect::<String>();
        let (endpoint, task) = server(axum::Router::new().route(
            "/responses",
            post(move || {
                let body = body.clone();
                async move { body }
            }),
        ))
        .await;
        let mut adapter = ChatGptResponsesAdapter::new("fake".into(), "account".into()).unwrap();
        adapter.endpoint = endpoint;
        let response = adapter.generate(&config(), &request()).await.unwrap();
        assert_eq!(response.content.text, "Checking");
        assert_eq!(response.tool_calls.unwrap()[0].id, "call_1");
        assert_eq!(response.usage.total_tokens, 5);
        task.abort();
    }
}

async fn server(app: axum::Router) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}/responses"), task)
}

fn request() -> ModelGenerateRequest {
    ModelGenerateRequest {
        system: "System".into(),
        messages: vec![message(MessageRole::User, "Hi", None)],
        temperature: None,
        max_tokens: None,
    }
}

#[tokio::test]
async fn subscription_http_uses_only_subscription_auth_and_normalizes_stream() {
    use axum::{http::HeaderMap, routing::post, Json};
    let (endpoint, task) = server(axum::Router::new().route("/responses", post(|headers: HeaderMap, Json(body): Json<Value>| async move {
        assert_eq!(headers["authorization"], "Bearer fake-subscription");
        assert_eq!(headers["chatgpt-account-id"], "test-account");
        assert_eq!(body["model"], "gpt-5.5");
        assert_eq!(body["store"], false);
        let delta = json!({"type":"response.output_text.delta", "delta":"Hello 世界"});
        let done = json!({"type":"response.completed", "response":{
            "status":"completed", "output":[{"type":"message","content":[{"type":"output_text","text":"Hello 世界"}]}],
            "usage":{"input_tokens":2,"output_tokens":3,"total_tokens":5}}});
        // Split within UTF-8 sequences and SSE delimiters, as real network chunks may do.
        let bytes = format!("data: {delta}\r\n\r\ndata: {done}\n\n").into_bytes();
        let chunks: Vec<Result<Vec<u8>, std::io::Error>> = bytes.chunks(3).map(|s| Ok(s.to_vec())).collect();
        axum::body::Body::from_stream(futures::stream::iter(chunks))
    }))).await;
    let mut adapter =
        ChatGptResponsesAdapter::new("fake-subscription".into(), "test-account".into()).unwrap();
    adapter.endpoint = endpoint;
    let response = adapter.generate(&config(), &request()).await.unwrap();
    assert_eq!(response.content.text, "Hello 世界");
    assert_eq!(response.usage.total_tokens, 5);
    task.abort();
}

#[tokio::test]
async fn subscription_rejects_truncated_stream_and_redacts_upstream_errors() {
    use axum::{http::StatusCode, routing::post};
    for (status, body, expected) in [
        (
            StatusCode::OK,
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n\n",
            "without a completed response",
        ),
        (
            StatusCode::OK,
            "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"call_id\":\"call_1\",\"name\":\"lookup\",\"arguments\":\"{}\"}}\n\n",
            "without a completed response",
        ),
        (
            StatusCode::UNAUTHORIZED,
            "fake-subscription secret account details",
            "authorization expired",
        ),
        (
            StatusCode::TOO_MANY_REQUESTS,
            "fake-subscription",
            "usage limit reached",
        ),
        (
            StatusCode::OK,
            "data: {\"type\":\"response.failed\",\"error\":\"fake-subscription\"}\n\n",
            "generation failed",
        ),
    ] {
        let (endpoint, task) = server(
            axum::Router::new().route("/responses", post(move || async move { (status, body) })),
        )
        .await;
        let mut adapter =
            ChatGptResponsesAdapter::new("fake-subscription".into(), "test-account".into())
                .unwrap();
        adapter.endpoint = endpoint;
        let error = adapter.generate(&config(), &request()).await.unwrap_err();
        assert!(error.contains(expected), "{error}");
        assert!(!error.contains("fake-subscription"));
        task.abort();
    }
}

#[tokio::test]
async fn subscription_does_not_follow_redirects() {
    use axum::{http::StatusCode, routing::post};
    let (endpoint, task) = server(axum::Router::new().route(
        "/responses",
        post(|| async {
            (
                StatusCode::TEMPORARY_REDIRECT,
                [("location", "https://example.com/collect")],
            )
        }),
    ))
    .await;
    let mut adapter =
        ChatGptResponsesAdapter::new("fake-subscription".into(), "test-account".into()).unwrap();
    adapter.endpoint = endpoint;
    assert!(adapter
        .generate(&config(), &request())
        .await
        .unwrap_err()
        .contains("HTTP 307"));
    task.abort();
}

#[test]
fn completed_response_parses_cached_and_reasoning_usage() {
    let response = json!({
        "status": "completed",
        "output": [{"type":"message","role":"assistant","content":[{"type":"output_text","text":"done"}]}],
        "usage": {"input_tokens": 12, "output_tokens": 8, "total_tokens": 20,
                  "input_tokens_details": {"cached_tokens": 5},
                  "output_tokens_details": {"reasoning_tokens": 3}}
    });

    let parsed = completed_response(&response).expect("completed response parses");

    assert_eq!(parsed.usage.prompt_tokens, 12);
    assert_eq!(parsed.usage.cached_prompt_tokens, 5);
    assert_eq!(parsed.usage.reasoning_tokens, 3);
    assert_eq!(parsed.usage.total_tokens, 20);
}
