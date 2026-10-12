use super::*;
use anima_core::{
    AgentSettings, Content, DataValue, Message, MessageRole, ModelStopReason, ToolDescriptor,
};
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
    assert_eq!(body["tool_choice"], "auto");
    assert_eq!(body["parallel_tool_calls"], true);
    assert!(body.get("temperature").is_none());
    assert!(body.get("max_tokens").is_none());
}

#[test]
fn subscription_request_omits_all_tool_controls_when_tools_are_absent_or_empty() {
    let request = request();
    for tools in [None, Some(Vec::new())] {
        let mut config = config();
        config.tools = tools;

        let body = request_body(&config, &request).unwrap();
        assert!(body.get("tools").is_none(), "body: {body}");
        assert!(body.get("tool_choice").is_none(), "body: {body}");
        assert!(body.get("parallel_tool_calls").is_none(), "body: {body}");
    }
}

#[test]
fn subscription_no_tools_request_preserves_historical_function_call_and_result() {
    let request = ModelGenerateRequest {
        system: "Finish using the recorded result".into(),
        temperature: None,
        max_tokens: None,
        messages: vec![
            message(MessageRole::User, "Look up record 7", None),
            message(
                MessageRole::Assistant,
                "",
                Some(BTreeMap::from([(
                    "toolCalls".into(),
                    DataValue::Array(vec![DataValue::Object(BTreeMap::from([
                        ("id".into(), DataValue::String("call_historical".into())),
                        ("name".into(), DataValue::String("lookup".into())),
                        (
                            "args".into(),
                            DataValue::Object(BTreeMap::from([(
                                "id".into(),
                                DataValue::Number(7.0),
                            )])),
                        ),
                    ]))]),
                )])),
            ),
            message(
                MessageRole::Tool,
                "Found record seven",
                Some(BTreeMap::from([(
                    "toolCallId".into(),
                    DataValue::String("call_historical".into()),
                )])),
            ),
        ],
    };
    let mut config = config();
    config.tools = None;

    let body = request_body(&config, &request).unwrap();
    assert_eq!(
        body["input"][1],
        json!({
            "type":"function_call",
            "call_id":"call_historical",
            "name":"lookup",
            "arguments":"{\"id\":7.0}"
        })
    );
    assert_eq!(
        body["input"][2],
        json!({
            "type":"function_call_output",
            "call_id":"call_historical",
            "output":"Found record seven"
        })
    );
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

fn retry_config(max_retries: Option<u32>) -> AgentConfig {
    let mut config = config();
    config.settings = Some(AgentSettings {
        max_retries,
        ..AgentSettings::default()
    });
    config
}

fn completed_sse() -> String {
    concat!(
        "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",",
        "\"output\":[{\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"ok\"}]}]}}\n\n"
    )
    .into()
}

#[tokio::test]
async fn prestream_retry_none_reuses_the_same_request_after_503() {
    use axum::{extract::Json, http::HeaderMap, response::Response, routing::post};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    };

    let calls = Arc::new(AtomicUsize::new(0));
    let received = Arc::new(Mutex::new(Vec::new()));
    let (endpoint, task) = server(axum::Router::new().route(
        "/responses",
        post({
            let calls = calls.clone();
            let received = received.clone();
            move |headers: HeaderMap, Json(body): Json<Value>| {
                let calls = calls.clone();
                let received = received.clone();
                async move {
                    received.lock().unwrap().push((
                        headers["authorization"].to_str().unwrap().to_owned(),
                        headers["chatgpt-account-id"].to_str().unwrap().to_owned(),
                        body,
                    ));
                    let attempt = calls.fetch_add(1, Ordering::SeqCst);
                    let (status, content_type, body) = if attempt == 0 {
                        (502, "text/plain", "gateway unavailable".to_owned())
                    } else {
                        (200, "text/event-stream", completed_sse())
                    };
                    Response::builder()
                        .status(status)
                        .header("content-type", content_type)
                        .body(axum::body::Body::from(body))
                        .unwrap()
                }
            }
        }),
    ))
    .await;
    let mut adapter =
        ChatGptResponsesAdapter::new("fake-subscription".into(), "test-account".into()).unwrap();
    adapter.endpoint = endpoint;

    let response = adapter
        .generate(&retry_config(None), &request())
        .await
        .expect("an eligible pre-stream gateway failure gets one retry");

    assert_eq!(response.content.text, "ok");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let received = received.lock().unwrap();
    assert_eq!(received.len(), 2);
    assert_eq!(received[0], received[1]);
    assert_eq!(received[0].0, "Bearer fake-subscription");
    assert_eq!(received[0].1, "test-account");
    task.abort();
}

#[tokio::test]
async fn prestream_retry_zero_opts_out_and_positive_values_clamp_at_one() {
    use axum::{http::StatusCode, routing::post};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    for (configured, expected_calls) in [(Some(0), 1), (Some(9), 2)] {
        let calls = Arc::new(AtomicUsize::new(0));
        let (endpoint, task) = server(axum::Router::new().route(
            "/responses",
            post({
                let calls = calls.clone();
                move || {
                    let calls = calls.clone();
                    async move {
                        calls.fetch_add(1, Ordering::SeqCst);
                        (StatusCode::SERVICE_UNAVAILABLE, "gateway unavailable")
                    }
                }
            }),
        ))
        .await;
        let mut adapter =
            ChatGptResponsesAdapter::new("fake-subscription".into(), "test-account".into())
                .unwrap();
        adapter.endpoint = endpoint;
        let error = adapter
            .generate(&retry_config(configured), &request())
            .await
            .unwrap_err();
        assert!(error.contains("HTTP 503"), "{error}");
        assert_eq!(calls.load(Ordering::SeqCst), expected_calls);
        task.abort();
    }
}

#[tokio::test]
async fn prestream_retry_never_makes_a_third_request_after_second_gateway_failure() {
    use axum::{http::StatusCode, routing::post};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    let calls = Arc::new(AtomicUsize::new(0));
    let (endpoint, task) = server(axum::Router::new().route(
        "/responses",
        post({
            let calls = calls.clone();
            move || {
                let calls = calls.clone();
                async move {
                    let attempt = calls.fetch_add(1, Ordering::SeqCst);
                    let status = if attempt == 0 {
                        StatusCode::BAD_GATEWAY
                    } else {
                        StatusCode::GATEWAY_TIMEOUT
                    };
                    (status, "gateway unavailable")
                }
            }
        }),
    ))
    .await;
    let mut adapter =
        ChatGptResponsesAdapter::new("fake-subscription".into(), "test-account".into()).unwrap();
    adapter.endpoint = endpoint;

    let error = adapter
        .generate(&retry_config(Some(20)), &request())
        .await
        .unwrap_err();

    assert!(error.contains("HTTP 504"), "{error}");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    task.abort();
}

#[tokio::test]
async fn prestream_retry_covers_502_503_and_504_gateway_responses() {
    use axum::{http::StatusCode, response::Response, routing::post};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    for status in [
        StatusCode::BAD_GATEWAY,
        StatusCode::SERVICE_UNAVAILABLE,
        StatusCode::GATEWAY_TIMEOUT,
    ] {
        let calls = Arc::new(AtomicUsize::new(0));
        let (endpoint, task) = server(axum::Router::new().route(
            "/responses",
            post({
                let calls = calls.clone();
                move || {
                    let calls = calls.clone();
                    async move {
                        let first = calls.fetch_add(1, Ordering::SeqCst) == 0;
                        let (status, content_type, body) = if first {
                            (status, "text/plain", "gateway unavailable".to_owned())
                        } else {
                            (StatusCode::OK, "text/event-stream", completed_sse())
                        };
                        Response::builder()
                            .status(status)
                            .header("content-type", content_type)
                            .body(axum::body::Body::from(body))
                            .unwrap()
                    }
                }
            }),
        ))
        .await;
        let mut adapter =
            ChatGptResponsesAdapter::new("fake-subscription".into(), "test-account".into())
                .unwrap();
        adapter.endpoint = endpoint;

        let response = adapter
            .generate(&retry_config(None), &request())
            .await
            .unwrap_or_else(|error| panic!("{status} should retry once: {error}"));

        assert_eq!(response.content.text, "ok");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        task.abort();
    }
}

#[tokio::test]
async fn prestream_retry_does_not_retry_other_http_statuses() {
    use axum::{http::StatusCode, routing::post};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    for status in [
        StatusCode::BAD_REQUEST,
        StatusCode::UNAUTHORIZED,
        StatusCode::FORBIDDEN,
        StatusCode::TOO_MANY_REQUESTS,
        StatusCode::INTERNAL_SERVER_ERROR,
        StatusCode::NOT_IMPLEMENTED,
        StatusCode::HTTP_VERSION_NOT_SUPPORTED,
    ] {
        let calls = Arc::new(AtomicUsize::new(0));
        let (endpoint, task) = server(axum::Router::new().route(
            "/responses",
            post({
                let calls = calls.clone();
                move || {
                    let calls = calls.clone();
                    async move {
                        calls.fetch_add(1, Ordering::SeqCst);
                        (status, "static test diagnostic")
                    }
                }
            }),
        ))
        .await;
        let mut adapter =
            ChatGptResponsesAdapter::new("fake-subscription".into(), "test-account".into())
                .unwrap();
        adapter.endpoint = endpoint;
        let _ = adapter
            .generate(&retry_config(None), &request())
            .await
            .unwrap_err();
        assert_eq!(calls.load(Ordering::SeqCst), 1, "{status}");
        task.abort();
    }
}

#[tokio::test]
async fn prestream_retry_normalizes_plain_and_html_gateway_media_types() {
    use axum::{http::StatusCode, response::Response, routing::post};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    for content_type in ["text/plain; charset=utf-8", "TEXT/HTML; Charset=UTF-8"] {
        let calls = Arc::new(AtomicUsize::new(0));
        let (endpoint, task) = server(axum::Router::new().route(
            "/responses",
            post({
                let calls = calls.clone();
                move || {
                    let calls = calls.clone();
                    async move {
                        let first = calls.fetch_add(1, Ordering::SeqCst) == 0;
                        if first {
                            Response::builder()
                                .status(StatusCode::BAD_GATEWAY)
                                .header("content-type", content_type)
                                .body(axum::body::Body::from("static gateway response"))
                                .unwrap()
                        } else {
                            Response::builder()
                                .status(StatusCode::OK)
                                .header("content-type", "text/event-stream")
                                .body(axum::body::Body::from(completed_sse()))
                                .unwrap()
                        }
                    }
                }
            }),
        ))
        .await;
        let mut adapter =
            ChatGptResponsesAdapter::new("fake-subscription".into(), "test-account".into())
                .unwrap();
        adapter.endpoint = endpoint;

        adapter
            .generate(&retry_config(None), &request())
            .await
            .unwrap_or_else(|error| panic!("{content_type} should be eligible: {error}"));
        assert_eq!(calls.load(Ordering::SeqCst), 2, "{content_type}");
        task.abort();
    }
}

#[tokio::test]
async fn prestream_retry_plain_gateway_requires_exact_text_mime_and_utf8() {
    use axum::{http::StatusCode, response::Response, routing::post};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    for case in [
        "event-stream-suffix",
        "plain-suffix",
        "missing-content-type",
        "invalid-utf8",
    ] {
        let calls = Arc::new(AtomicUsize::new(0));
        let (endpoint, task) = server(axum::Router::new().route(
            "/responses",
            post({
                let calls = calls.clone();
                move || {
                    let calls = calls.clone();
                    async move {
                        calls.fetch_add(1, Ordering::SeqCst);
                        let builder = Response::builder().status(StatusCode::BAD_GATEWAY);
                        match case {
                            "event-stream-suffix" => builder
                                .header("content-type", "text/event-stream-extra")
                                .body(axum::body::Body::from("gateway unavailable"))
                                .unwrap(),
                            "plain-suffix" => builder
                                .header("content-type", "text/plain-other")
                                .body(axum::body::Body::from("gateway unavailable"))
                                .unwrap(),
                            "missing-content-type" => builder
                                .body(axum::body::Body::from("gateway unavailable"))
                                .unwrap(),
                            "invalid-utf8" => builder
                                .header("content-type", "text/plain")
                                .body(axum::body::Body::from(vec![0xff, 0xfe]))
                                .unwrap(),
                            _ => unreachable!(),
                        }
                    }
                }
            }),
        ))
        .await;
        let mut adapter =
            ChatGptResponsesAdapter::new("fake-subscription".into(), "test-account".into())
                .unwrap();
        adapter.endpoint = endpoint;

        let _ = adapter
            .generate(&retry_config(None), &request())
            .await
            .unwrap_err();
        assert_eq!(calls.load(Ordering::SeqCst), 1, "{case}");
        task.abort();
    }
}

#[tokio::test]
async fn prestream_retry_recognized_transient_json_code_ignores_non_eventstream_mime() {
    use axum::{http::StatusCode, response::Response, routing::post};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    let calls = Arc::new(AtomicUsize::new(0));
    let (endpoint, task) = server(axum::Router::new().route(
        "/responses",
        post({
            let calls = calls.clone();
            move || {
                let calls = calls.clone();
                async move {
                    let first = calls.fetch_add(1, Ordering::SeqCst) == 0;
                    if first {
                        Response::builder()
                            .status(StatusCode::SERVICE_UNAVAILABLE)
                            .header("content-type", "text/plain")
                            .body(axum::body::Body::from(
                                r#"{"error":{"code":"service_unavailable"}}"#,
                            ))
                            .unwrap()
                    } else {
                        Response::builder()
                            .status(StatusCode::OK)
                            .header("content-type", "text/event-stream")
                            .body(axum::body::Body::from(completed_sse()))
                            .unwrap()
                    }
                }
            }
        }),
    ))
    .await;
    let mut adapter =
        ChatGptResponsesAdapter::new("fake-subscription".into(), "test-account".into()).unwrap();
    adapter.endpoint = endpoint;

    adapter
        .generate(&retry_config(None), &request())
        .await
        .expect("a recognized transient JSON code retries despite a non-JSON media type");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    task.abort();
}

#[tokio::test]
async fn prestream_retry_allows_only_the_five_transient_structured_codes() {
    use axum::{http::StatusCode, response::Response, routing::post};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    for code in [
        "server_error",
        "internal_server_error",
        "service_unavailable",
        "subscription_sharing_usage_unavailable",
        "subscription_sharing_user_unavailable",
    ] {
        let calls = Arc::new(AtomicUsize::new(0));
        let (endpoint, task) = server(axum::Router::new().route(
            "/responses",
            post({
                let calls = calls.clone();
                move || {
                    let calls = calls.clone();
                    async move {
                        let first = calls.fetch_add(1, Ordering::SeqCst) == 0;
                        let (status, content_type, body) = if first {
                            (
                                StatusCode::SERVICE_UNAVAILABLE,
                                "application/json",
                                json!({"error":{"code":code,"message":"private upstream detail"}})
                                    .to_string(),
                            )
                        } else {
                            (StatusCode::OK, "text/event-stream", completed_sse())
                        };
                        Response::builder()
                            .status(status)
                            .header("content-type", content_type)
                            .body(axum::body::Body::from(body))
                            .unwrap()
                    }
                }
            }),
        ))
        .await;
        let mut adapter =
            ChatGptResponsesAdapter::new("fake-subscription".into(), "test-account".into())
                .unwrap();
        adapter.endpoint = endpoint;

        let response = adapter
            .generate(&retry_config(None), &request())
            .await
            .unwrap_or_else(|error| panic!("retryable code {code}: {error}"));

        assert_eq!(response.content.text, "ok");
        assert_eq!(calls.load(Ordering::SeqCst), 2, "{code}");
        task.abort();
    }
}

#[tokio::test]
async fn prestream_retry_known_terminal_codes_override_502() {
    use axum::{http::StatusCode, routing::post};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    let terminal_codes = [
        "invalid_function_parameters",
        "invalid_prompt",
        "model_not_found",
        "unsupported_model",
        "invalid_request_error",
        "invalid_api_key",
        "authentication_error",
        "invalid_authentication",
        "token_expired",
        "permission_denied",
        "permission_error",
        "rate_limit_exceeded",
        "insufficient_quota",
        "usage_limit_exceeded",
        "context_length_exceeded",
        "subscription_sharing_user_not_eligible",
        "subscription_sharing_usage_limit_exceeded",
        "subscription_sharing_unsupported_capability",
        "subscription_sharing_route_not_supported",
        "subscription_sharing_invalid_user",
        "chatpass_v2_scope_not_authorized",
        "chatpass_v2_invalid_authorization_context",
    ];
    for code in terminal_codes {
        let calls = Arc::new(AtomicUsize::new(0));
        let (endpoint, task) = server(axum::Router::new().route(
            "/responses",
            post({
                let calls = calls.clone();
                move || {
                    let calls = calls.clone();
                    async move {
                        calls.fetch_add(1, Ordering::SeqCst);
                        (
                            StatusCode::BAD_GATEWAY,
                            axum::Json(json!({"error":{"code":code,"message":"secret-sentinel"}})),
                        )
                    }
                }
            }),
        ))
        .await;
        let mut adapter =
            ChatGptResponsesAdapter::new("fake-subscription".into(), "test-account".into())
                .unwrap();
        adapter.endpoint = endpoint;

        let error = adapter
            .generate(&retry_config(None), &request())
            .await
            .unwrap_err();

        assert_eq!(calls.load(Ordering::SeqCst), 1, "{code}: {error}");
        assert!(error.contains(code), "{code}: {error}");
        assert!(!error.contains("secret-sentinel"));
        task.abort();
    }
}

#[tokio::test]
async fn prestream_retry_fails_closed_for_unknown_json_event_stream_and_bounded_read_failures() {
    use axum::{http::StatusCode, response::Response, routing::post};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    for case in [
        "unknown-json-code",
        "unstructured-json",
        "event-stream-error",
        "oversized-body",
        "chunked-overflow",
        "body-read-failure",
    ] {
        let calls = Arc::new(AtomicUsize::new(0));
        let (endpoint, task) = server(axum::Router::new().route(
            "/responses",
            post({
                let calls = calls.clone();
                move || {
                    let calls = calls.clone();
                    async move {
                        let attempt = calls.fetch_add(1, Ordering::SeqCst);
                        if attempt > 0 {
                            return Response::builder()
                                .status(StatusCode::OK)
                                .header("content-type", "text/event-stream")
                                .body(axum::body::Body::from(completed_sse()))
                                .unwrap();
                        }
                        match case {
                            "unknown-json-code" => Response::builder()
                                .status(StatusCode::BAD_GATEWAY)
                                .header("content-type", "application/json")
                                .body(axum::body::Body::from(
                                    r#"{"error":{"code":"future_gateway_code"}}"#,
                                ))
                                .unwrap(),
                            "unstructured-json" => Response::builder()
                                .status(StatusCode::BAD_GATEWAY)
                                .header("content-type", "application/json")
                                .body(axum::body::Body::from(r#"{"detail":"gateway"}"#))
                                .unwrap(),
                            "event-stream-error" => Response::builder()
                                .status(StatusCode::SERVICE_UNAVAILABLE)
                                .header("content-type", "text/event-stream")
                                .body(axum::body::Body::from(
                                    r#"data: {"type":"error","code":"service_unavailable"}"#,
                                ))
                                .unwrap(),
                            "oversized-body" => Response::builder()
                                .status(StatusCode::SERVICE_UNAVAILABLE)
                                .header("content-type", "text/plain")
                                .body(axum::body::Body::from("x".repeat(MAX_ERROR_BODY + 1)))
                                .unwrap(),
                            "chunked-overflow" => {
                                let chunks: Vec<Result<Vec<u8>, std::io::Error>> =
                                    vec![Ok(vec![b'x'; MAX_ERROR_BODY + 1])];
                                Response::builder()
                                    .status(StatusCode::SERVICE_UNAVAILABLE)
                                    .header("content-type", "text/plain")
                                    .body(axum::body::Body::from_stream(futures::stream::iter(
                                        chunks,
                                    )))
                                    .unwrap()
                            }
                            "body-read-failure" => {
                                let chunks: Vec<Result<Vec<u8>, std::io::Error>> = vec![
                                    Ok(b"{\"error\":".to_vec()),
                                    Err(std::io::Error::other("mock body read failed")),
                                ];
                                Response::builder()
                                    .status(StatusCode::SERVICE_UNAVAILABLE)
                                    .header("content-type", "application/json")
                                    .body(axum::body::Body::from_stream(futures::stream::iter(
                                        chunks,
                                    )))
                                    .unwrap()
                            }
                            _ => unreachable!(),
                        }
                    }
                }
            }),
        ))
        .await;
        let mut adapter =
            ChatGptResponsesAdapter::new("fake-subscription".into(), "test-account".into())
                .unwrap();
        adapter.endpoint = endpoint;

        let error = adapter
            .generate(&retry_config(None), &request())
            .await
            .unwrap_err();

        assert_eq!(calls.load(Ordering::SeqCst), 1, "{case}: {error}");
        task.abort();
    }
}

struct FailingStreamSink;

#[async_trait::async_trait]
impl ModelStreamSink for FailingStreamSink {
    async fn emit(&self, _frame: ModelStreamFrame) -> Result<(), String> {
        Err("mock sink closed".into())
    }
}

#[tokio::test]
async fn prestream_retry_never_replays_partial_sse_or_sink_failures() {
    use axum::{http::StatusCode, response::Response, routing::post};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    let partial_failure = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n\n",
        "data: {\"type\":\"response.failed\",\"response\":{\"error\":{\"code\":\"server_error\"}}}\n\n"
    );
    for (body, reject_sink) in [(partial_failure.to_owned(), false), (completed_sse(), true)] {
        let calls = Arc::new(AtomicUsize::new(0));
        let (endpoint, task) = server(axum::Router::new().route(
            "/responses",
            post({
                let calls = calls.clone();
                let body = body.clone();
                move || {
                    let calls = calls.clone();
                    let body = body.clone();
                    async move {
                        calls.fetch_add(1, Ordering::SeqCst);
                        Response::builder()
                            .status(StatusCode::OK)
                            .header("content-type", "text/event-stream")
                            .body(axum::body::Body::from(body))
                            .unwrap()
                    }
                }
            }),
        ))
        .await;
        let mut adapter =
            ChatGptResponsesAdapter::new("fake-subscription".into(), "test-account".into())
                .unwrap();
        adapter.endpoint = endpoint;
        let result = if reject_sink {
            adapter
                .stream(&retry_config(None), &request(), &FailingStreamSink)
                .await
        } else {
            adapter
                .generate(&retry_config(None), &request())
                .await
                .map(|_| ())
        };

        assert!(result.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        task.abort();
    }
}

#[tokio::test]
async fn prestream_retry_does_not_replay_request_send_failure() {
    let (endpoint, task) = server(axum::Router::new()).await;
    task.abort();
    let mut adapter =
        ChatGptResponsesAdapter::new("fake-subscription".into(), "test-account".into()).unwrap();
    adapter.endpoint = endpoint;

    let error = adapter
        .generate(&retry_config(None), &request())
        .await
        .unwrap_err();

    assert!(error.contains("request failed or timed out"), "{error}");
}

#[tokio::test]
async fn prestream_retry_cancel_signal_drops_backoff_before_the_next_request() {
    use anima_core::{AgentRuntime, CancelSignal, Content, RunControl, RUN_STOPPED_ERROR};
    use axum::{http::StatusCode, routing::post};
    use std::{
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
        time::Duration,
    };

    let calls = Arc::new(AtomicUsize::new(0));
    let (endpoint, server_task) = server(axum::Router::new().route(
        "/responses",
        post({
            let calls = calls.clone();
            move || {
                let calls = calls.clone();
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    (StatusCode::SERVICE_UNAVAILABLE, "gateway unavailable")
                }
            }
        }),
    ))
    .await;
    let mut adapter =
        ChatGptResponsesAdapter::new("fake-subscription".into(), "test-account".into()).unwrap();
    adapter.endpoint = endpoint;
    let mut runtime = AgentRuntime::new(config(), Arc::new(adapter));
    let cancel = CancelSignal::new();
    runtime.set_run_control(RunControl {
        cancel: cancel.clone(),
        ..RunControl::new()
    });
    let run = tokio::spawn(async move {
        runtime
            .run(Content {
                text: "Say hello".into(),
                attachments: None,
                metadata: None,
            })
            .await
    });

    tokio::time::timeout(Duration::from_secs(2), async {
        while calls.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the first HTTP response is in flight");
    tokio::time::sleep(Duration::from_millis(60)).await;
    cancel.cancel();
    let stopped = tokio::time::timeout(Duration::from_secs(2), run)
        .await
        .expect("cancellation stops the runtime")
        .unwrap();
    assert_eq!(stopped.error.as_deref(), Some(RUN_STOPPED_ERROR));
    tokio::time::sleep(Duration::from_millis(320)).await;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    server_task.abort();
}
