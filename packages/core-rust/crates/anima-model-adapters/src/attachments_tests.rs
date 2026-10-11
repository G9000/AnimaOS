use anima_core::{
    AgentConfig, Attachment, AttachmentType, Content, DataValue, Message, MessageRole,
    ModelGenerateRequest,
};
use serde_json::json;
use std::collections::BTreeMap;

fn config(provider: &str, model: &str) -> AgentConfig {
    serde_json::from_value(json!({"name":"image fixture", "provider":provider, "model":model}))
        .unwrap()
}

fn image(mime: &str, payload: &str) -> Attachment {
    Attachment {
        attachment_type: AttachmentType::Image,
        name: "photo.png".into(),
        data: format!("data:{mime};base64,{payload}"),
        mime_type: Some(mime.into()),
    }
}

fn request() -> ModelGenerateRequest {
    ModelGenerateRequest {
        system: "System".into(),
        temperature: None,
        max_tokens: None,
        messages: vec![Message {
            id: "user".into(),
            agent_id: "agent".into(),
            room_id: "session".into(),
            role: MessageRole::User,
            created_at_ms: 0,
            content: Content {
                text: "Compare these".into(),
                attachments: Some(vec![
                    image("image/png", "aGVsbG8="),
                    image("image/jpeg", "d29ybGQ="),
                ]),
                metadata: None,
            },
        }],
    }
}

#[test]
fn attachment_anthropic_request_retains_order_and_coalesces_system_content() {
    let mut request = request();
    let mut system = request.messages[0].clone();
    system.role = MessageRole::System;
    system.content = Content {
        text: "Additional instruction".into(),
        ..Content::default()
    };
    request.messages.push(system);
    let body =
        crate::anthropic::build_anthropic_body(&config("anthropic", "claude-sonnet-4-5"), &request)
            .unwrap();
    assert_eq!(body["messages"].as_array().unwrap().len(), 1);
    assert_eq!(
        body["messages"][0]["content"],
        json!([
            {"type":"text", "text":"Compare these"},
            {"type":"image", "source":{"type":"base64", "media_type":"image/png", "data":"aGVsbG8="}},
            {"type":"image", "source":{"type":"base64", "media_type":"image/jpeg", "data":"d29ybGQ="}},
            {"type":"text", "text":"Additional instruction"}
        ])
    );
}

#[test]
fn attachment_openai_request_keeps_text_then_image_urls() {
    let body = crate::openai_compatible::build_openai_compatible_body(
        &config("openai", "gpt-4o-mini"),
        &request(),
    )
    .unwrap();
    assert_eq!(
        body["messages"][1]["content"],
        json!([
            {"type":"text", "text":"Compare these"},
            {"type":"image_url", "image_url":{"url":"data:image/png;base64,aGVsbG8="}},
            {"type":"image_url", "image_url":{"url":"data:image/jpeg;base64,d29ybGQ="}}
        ])
    );
}

#[test]
fn attachment_google_request_keeps_inline_data_and_mime_order() {
    let body = crate::google::build_google_body(&config("gemini", "gemini-3.8-flash"), &request())
        .unwrap();
    assert_eq!(
        body["contents"][0]["parts"],
        json!([
            {"text":"Compare these"},
            {"inline_data":{"mime_type":"image/png", "data":"aGVsbG8="}},
            {"inline_data":{"mime_type":"image/jpeg", "data":"d29ybGQ="}}
        ])
    );
}

#[test]
fn attachment_ollama_native_request_shape_and_unknown_model_fallback() {
    // The current table has no Ollama rows. Exercise its pure serializer with a
    // known vision config; real Ollama configs remain gated to safe references.
    let body =
        crate::ollama::build_ollama_body(&config("openai", "gpt-4o-mini"), &request()).unwrap();
    assert_eq!(body["messages"][1]["content"], "Compare these");
    assert_eq!(
        body["messages"][1]["images"],
        json!(["aGVsbG8=", "d29ybGQ="])
    );
    for body in [
        crate::ollama::build_ollama_body(&config("ollama", "llava"), &request()).unwrap(),
        crate::openai_compatible::build_openai_compatible_body(
            &config("ollama", "llava"),
            &request(),
        )
        .unwrap(),
    ] {
        assert!(body["messages"][1].get("images").is_none());
        let text = body["messages"][1]["content"].as_str().unwrap();
        assert!(text.contains("cannot view"));
        assert!(!text.contains("aGVsbG8="));
    }
}

#[test]
fn attachment_unknown_and_nonvision_models_keep_safe_path_references() {
    let mut request = request();
    request.messages[0].content.attachments.as_mut().unwrap()[0].data = "uploads/photo.png".into();
    for model in ["unknown-model", "o3-mini", "gpt-4o-mini"] {
        let body = crate::openai_compatible::build_openai_compatible_body(
            &config("openai", model),
            &request,
        )
        .unwrap();
        let text = body["messages"][1]["content"].to_string();
        assert!(text.contains("photo.png"));
        assert!(text.contains("uploads/photo.png"));
        if model != "gpt-4o-mini" {
            assert!(text.contains("cannot view"));
            assert!(!text.contains("data:image"));
            assert!(!text.contains("d29ybGQ="));
        }
    }
    let bodies = [
        crate::anthropic::build_anthropic_body(&config("anthropic", "unknown"), &request).unwrap(),
        crate::google::build_google_body(&config("google", "unknown"), &request).unwrap(),
    ];
    for body in bodies {
        let serialized = body.to_string();
        assert!(serialized.contains("uploads/photo.png"));
        assert!(serialized.contains("cannot view"));
        assert!(!serialized.contains("data:image"));
        assert!(!serialized.contains("d29ybGQ="));
        assert!(!serialized.contains("inline_data"));
    }
}

#[test]
fn attachment_malformed_prepared_payload_errors_never_echo_payload() {
    for data in [
        "data:image/jpeg;base64,SECRET",
        "data:image/png;base64,SECRET!",
        "data:image/png;base64,",
        "data:image/png;base64,A===",
    ] {
        let mut request = request();
        request.messages[0].content.attachments.as_mut().unwrap()[0].data = data.into();
        let error = crate::openai_compatible::build_openai_compatible_body(
            &config("openai", "gpt-4o-mini"),
            &request,
        )
        .unwrap_err();
        assert!(error.len() < 160);
        assert!(!error.contains("SECRET"));
        assert!(!error.contains(data));
    }
}

#[test]
fn attachment_preparation_bounds_size_and_requires_supported_matching_mime() {
    use crate::attachments::prepared_image;
    let mut attachment = image("image/png", "aGVsbG8=");
    attachment.mime_type = None;
    assert!(prepared_image(&attachment).is_err());
    attachment.mime_type = Some("image/svg+xml".into());
    assert!(prepared_image(&attachment).is_err());
    attachment = image("image/png", "aGVsbG8=");
    attachment.data = format!(
        "data:image/png;base64,{}",
        "A".repeat(4 * (10 * 1024 * 1024usize).div_ceil(3) + 4)
    );
    let error = prepared_image(&attachment).err().unwrap();
    assert!(error.len() < 160);
    assert!(!error.contains("data:"));
    for mime in ["image/png", "image/jpeg", "image/webp", "image/gif"] {
        let attachment = image(mime, "aGVsbG8=");
        assert_eq!(prepared_image(&attachment).unwrap().unwrap().mime, mime);
    }
}

#[test]
fn attachment_vision_gate_uses_existing_provider_and_subscription_aliases() {
    assert!(crate::model_supports_images("Gemini", "gemini-3.8-flash"));
    assert!(crate::model_supports_images("CHATGPT", "gpt-4o-mini"));
    assert!(crate::model_supports_images("grok", "grok-4.5-latest"));
    assert!(!crate::model_supports_images("openai", "o3-mini"));
    assert!(!crate::model_supports_images("openai", "unknown"));
    assert!(!crate::model_supports_images("unknown", "gpt-4o-mini"));
}

#[tokio::test]
async fn attachment_deterministic_fallback_preserves_path_and_fixed_response() {
    use anima_core::{ModelAdapter, ModelStreamFrame, ModelStreamSink};
    let mut request = request();
    request.messages[0].content.attachments.as_mut().unwrap()[0].data = "uploads/photo.png".into();
    let fallback = crate::attachments::text_fallback_request(&request).unwrap();
    assert!(fallback.messages[0]
        .content
        .text
        .contains("uploads/photo.png"));
    assert!(fallback.messages[0].content.text.contains("cannot view"));
    assert!(fallback.messages[0].content.attachments.is_none());
    assert!(!fallback.messages[0].content.text.contains("d29ybGQ="));
    assert_eq!(
        request.messages[0]
            .content
            .attachments
            .as_ref()
            .unwrap()
            .len(),
        2
    );
    let adapter = crate::DeterministicModelAdapter::new("Fixed reply");
    let config = config("deterministic", "fixed");
    assert_eq!(
        adapter
            .generate(&config, &request)
            .await
            .unwrap()
            .content
            .text,
        "Fixed reply"
    );
    struct Sink(std::sync::Mutex<Vec<ModelStreamFrame>>);
    #[async_trait::async_trait]
    impl ModelStreamSink for Sink {
        async fn emit(&self, frame: ModelStreamFrame) -> Result<(), String> {
            self.0.lock().unwrap().push(frame);
            Ok(())
        }
    }
    let sink = Sink(std::sync::Mutex::new(Vec::new()));
    adapter.stream(&config, &request, &sink).await.unwrap();
    let frames = sink.0.lock().unwrap();
    assert!(
        matches!(frames.last(), Some(ModelStreamFrame::Final(response)) if response.content.text == "Fixed reply")
    );
}

#[test]
fn attachment_tool_calls_and_results_survive_adjacent_image_turns() {
    let mut request = request();
    let mut assistant = request.messages[0].clone();
    assistant.role = MessageRole::Assistant;
    assistant.content = Content {
        text: String::new(),
        attachments: None,
        metadata: Some(BTreeMap::from([(
            "toolCalls".into(),
            DataValue::Array(vec![DataValue::Object(BTreeMap::from([
                ("id".into(), DataValue::String("call-1".into())),
                ("name".into(), DataValue::String("lookup".into())),
                ("args".into(), DataValue::Object(BTreeMap::new())),
            ]))]),
        )])),
    };
    let mut tool = assistant.clone();
    tool.role = MessageRole::Tool;
    tool.content = Content {
        text: "result".into(),
        metadata: Some(BTreeMap::from([(
            "toolCallId".into(),
            DataValue::String("call-1".into()),
        )])),
        ..Content::default()
    };
    request
        .messages
        .extend([assistant, tool, request.messages[0].clone()]);
    let body = crate::openai_compatible::build_openai_compatible_body(
        &config("openai", "gpt-4o-mini"),
        &request,
    )
    .unwrap();
    assert_eq!(body["messages"][2]["tool_calls"][0]["id"], "call-1");
    assert_eq!(
        body["messages"][3],
        json!({"role":"tool", "tool_call_id":"call-1", "content":"result"})
    );
    assert_eq!(body["messages"][4]["content"][1]["type"], "image_url");
    let body =
        crate::anthropic::build_anthropic_body(&config("anthropic", "claude-sonnet-4-5"), &request)
            .unwrap();
    assert_eq!(body["messages"][1]["content"][0]["id"], "call-1");
    assert_eq!(body["messages"][2]["content"][0]["tool_use_id"], "call-1");
    assert_eq!(body["messages"][3]["content"][1]["type"], "image");
    let body =
        crate::google::build_google_body(&config("google", "gemini-3.8-flash"), &request).unwrap();
    assert_eq!(
        body["contents"][1]["parts"][0]["functionCall"]["name"],
        "lookup"
    );
    assert_eq!(
        body["contents"][2]["parts"][0]["functionResponse"]["name"],
        "lookup"
    );
    assert!(body["contents"][3]["parts"][1].get("inline_data").is_some());
}

#[test]
fn attachment_text_only_request_shape_is_unchanged() {
    let mut request = request();
    request.messages[0].content.attachments = None;
    let body = crate::openai_compatible::build_openai_compatible_body(
        &config("openai", "gpt-4o-mini"),
        &request,
    )
    .unwrap();
    assert_eq!(
        body["messages"][1],
        json!({"role":"user", "content":"Compare these"})
    );
    let body =
        crate::anthropic::build_anthropic_body(&config("anthropic", "claude-sonnet-4-5"), &request)
            .unwrap();
    assert_eq!(
        body["messages"][0],
        json!({"role":"user", "content":"Compare these"})
    );
    let body = crate::ollama::build_ollama_body(&config("ollama", "llava"), &request).unwrap();
    assert_eq!(
        body["messages"][1],
        json!({"role":"user", "content":"Compare these"})
    );
}

#[test]
fn attachment_upstream_error_does_not_expose_image_data() {
    for body in [
        "invalid image data:image/png;base64,aGVsbG8= token-key",
        r#"{"source":{"type":"base64","media_type":"image/png","data":"aGVsbG8="}}"#,
        r#"{"inline_data":{"mime_type":"image/png","data":"aGVsbG8="}}"#,
        r#"{"inlineData":{"mimeType":"image/png","data":"aGVsbG8="}}"#,
        r#"{"images":["aGVsbG8="]}"#,
    ] {
        let error = crate::adapter::sanitize_upstream_body(body, Some("token-key"));
        assert!(!error.contains("aGVsbG8="));
        assert!(!error.contains("token-key"));
    }
    assert_eq!(
        crate::adapter::sanitize_upstream_body("ordinary error token-key", Some("token-key")),
        "ordinary error [REDACTED]"
    );
}

#[test]
fn attachment_native_stream_errors_redact_before_detail_truncation() {
    let detail = format!("aGVsbG8={} data:image/png;base64,aGVsbG8=", "x".repeat(300));
    let error = crate::google::GoogleStreamAccumulator::default()
        .push(&json!({"error":{"message":detail}}))
        .unwrap_err();
    let ollama_error = crate::ollama::OllamaStreamAccumulator::default()
        .push(&json!({"error":detail}))
        .unwrap_err();
    assert_eq!(
        [
            error.contains("aGVsbG8="),
            ollama_error.contains("aGVsbG8=")
        ],
        [false, false]
    );
}
