//! Default provider transport bounds; injected clients retain caller ownership.
use reqwest::Client;
use std::time::Duration;
pub(crate) const PROVIDER_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
pub(crate) const PROVIDER_REQUEST_TIMEOUT: Duration = Duration::from_secs(300);
pub(crate) fn bounded_provider_client() -> Result<Client, String> {
    Client::builder()
        .connect_timeout(PROVIDER_CONNECT_TIMEOUT)
        .timeout(PROVIDER_REQUEST_TIMEOUT)
        .build()
        .map_err(|_| "bounded provider transport initialization failed".to_owned())
}
pub(crate) fn transport_error(provider: &str, operation: &str, error: reqwest::Error) -> String {
    format!("{provider} {operation} failed: {}", error.without_url())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_transport_has_named_bounds() {
        assert_eq!(PROVIDER_CONNECT_TIMEOUT, Duration::from_secs(15));
        assert_eq!(PROVIDER_REQUEST_TIMEOUT, Duration::from_secs(300));
        assert!(bounded_provider_client().is_ok());
    }

    #[test]
    fn transport_error_omits_secret_request_url_and_header() {
        let error = Client::new()
            .get("https://private.test/?token=fake-secret-key")
            .header(reqwest::header::AUTHORIZATION, "fake-secret-key\n")
            .build()
            .unwrap_err();
        let error = transport_error("OpenAI", "request", error);
        assert!(error.starts_with("OpenAI request failed:"));
        assert!(!error.contains("fake-secret-key"));
        assert!(!error.contains("private.test"));
    }

    #[tokio::test]
    async fn scripted_transport_timeout_keeps_partial_frames_and_usage() {
        use anima_core::{ModelStreamFrame, ModelStreamSink};
        use std::sync::Mutex;
        struct Sink(Mutex<Vec<ModelStreamFrame>>);
        #[async_trait::async_trait]
        impl ModelStreamSink for Sink {
            async fn emit(&self, frame: ModelStreamFrame) -> Result<(), String> {
                self.0.lock().unwrap().push(frame);
                Ok(())
            }
        }
        let data = b"data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Partial\"}}],\"usage\":{\"prompt_tokens\":12,\"completion_tokens\":3,\"total_tokens\":15}}\n\n".to_vec();
        let chunks = futures::stream::iter([
            Ok(data),
            Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "request timed out",
            )),
        ]);
        let response = reqwest::Response::from(axum::http::Response::new(
            reqwest::Body::wrap_stream(chunks),
        ));
        let sink = Sink(Mutex::new(Vec::new()));
        let provider = crate::catalog::resolve_provider("openai").unwrap();
        let error = crate::stream::consume_openai_sse(response, &sink, provider.definition, false)
            .await
            .unwrap_err();
        assert!(error.starts_with("provider stream read failed:"));
        let frames = sink.0.lock().unwrap();
        assert!(frames
            .iter()
            .any(|frame| matches!(frame, ModelStreamFrame::TextDelta(text) if text == "Partial")));
        assert!(frames.iter().any(
            |frame| matches!(frame, ModelStreamFrame::Usage(usage) if usage.total_tokens == 15)
        ));
        assert!(!frames
            .iter()
            .any(|frame| matches!(frame, ModelStreamFrame::Final(_))));
    }
}
