use super::*;
use anima_core::{Attachment, AttachmentType};

const FRAGMENT: &str = "aGVsbG8=";

fn image_request() -> ModelGenerateRequest {
    let mut request = request();
    request.messages[0].content.attachments = Some(vec![Attachment {
        attachment_type: AttachmentType::Image,
        name: "photo.png".into(),
        data: format!("data:image/png;base64,{FRAGMENT}"),
        mime_type: Some("image/png".into()),
    }]);
    request
}

fn provider_config(provider: &str) -> AgentConfig {
    let mut config = agent_config(provider, false);
    config.model = match provider {
        "anthropic" => "claude-sonnet-4-5",
        "google" => "gemini-3.8-flash",
        "ollama" => "llava",
        _ => "gpt-4o-mini",
    }
    .into();
    config
}

#[tokio::test]
async fn attachment_error_context_suppresses_http_fragments_and_keeps_plain_diagnostics() {
    for body in [
        FRAGMENT.to_owned(),
        format!("{FRAGMENT} {} inline_data", "x".repeat(1100)),
        format!("inline_data {} {FRAGMENT}", "x".repeat(1100)),
    ] {
        let base = spawn_server(Router::new().fallback(move || {
            let body = body.clone();
            async move { (axum::http::StatusCode::BAD_REQUEST, body) }
        }))
        .await;
        for provider in ["openai", "anthropic", "google", "ollama"] {
            let adapter = adapter_with(&[(provider, Some("key"), &base)]);
            let config = provider_config(provider);
            let request = image_request();
            for error in [
                adapter.generate(&config, &request).await.unwrap_err(),
                adapter
                    .stream(&config, &request, &FrameSink(Mutex::new(Vec::new())))
                    .await
                    .unwrap_err(),
            ] {
                assert!(!error.contains(FRAGMENT), "{provider}: {error}");
                assert!(error.len() < 200);
            }
        }
    }
    let base = spawn_server(Router::new().fallback(|| async {
        (
            axum::http::StatusCode::BAD_REQUEST,
            "model is unavailable key",
        )
    }))
    .await;
    for provider in ["openai", "anthropic", "google", "ollama"] {
        let adapter = adapter_with(&[(provider, Some("key"), &base)]);
        let config = provider_config(provider);
        for error in [
            adapter.generate(&config, &request()).await.unwrap_err(),
            adapter
                .stream(&config, &request(), &FrameSink(Mutex::new(Vec::new())))
                .await
                .unwrap_err(),
        ] {
            assert!(error.contains("model is unavailable"));
            assert!(!error.contains(" key"));
        }
    }
}

#[tokio::test]
async fn attachment_error_context_suppresses_native_frame_fragments() {
    for detail in [
        FRAGMENT.to_owned(),
        format!("{FRAGMENT} {} inline_data", "x".repeat(300)),
        format!("inline_data {} {FRAGMENT}", "x".repeat(300)),
    ] {
        for provider in ["openai", "anthropic", "google", "ollama"] {
            let payload = match provider {
                "anthropic" => json!({"type":"error", "error":{"message":detail}}),
                "google" => json!({"error":{"message":detail}}),
                "ollama" => json!({"error":detail}),
                _ => json!({"error":{"message":detail}}),
            };
            let body = if provider == "ollama" {
                format!("{payload}\n")
            } else {
                format!("data: {payload}\n\n")
            };
            let base = spawn_server(Router::new().fallback(move || {
                let body = body.clone();
                async move {
                    (
                        [(
                            "content-type",
                            if provider == "ollama" {
                                "application/x-ndjson"
                            } else {
                                "text/event-stream"
                            },
                        )],
                        body,
                    )
                }
            }))
            .await;
            let adapter = adapter_with(&[(provider, Some("key"), &base)]);
            let error = adapter
                .stream(
                    &provider_config(provider),
                    &image_request(),
                    &FrameSink(Mutex::new(Vec::new())),
                )
                .await
                .unwrap_err();
            assert!(!error.contains(FRAGMENT), "{provider}: {error}");
            if detail == FRAGMENT && matches!(provider, "google" | "ollama") {
                let plain = adapter
                    .stream(
                        &provider_config(provider),
                        &request(),
                        &FrameSink(Mutex::new(Vec::new())),
                    )
                    .await
                    .unwrap_err();
                assert!(
                    plain.contains(FRAGMENT),
                    "ordinary text-request diagnostics retain their existing detail"
                );
            }
        }
    }
}
