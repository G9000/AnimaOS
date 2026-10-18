//! Narrow OpenAI speech-to-text transport, adapted from Hermes's MIT-licensed
//! `tools/transcription_cloud.py` at 1fd18277267152893a8a24792b79213e7ca3f899.
//! See the repository third-party notice for provenance and the retained license.
//! Host configuration, attachment authority and consent remain outside this crate.

use std::sync::{Arc, OnceLock};
use std::time::Instant;

use anima_core::CancelSignal;
use anima_service_adapters::audio_content::{
    validate_transcript_text, WavAudio, MAX_TRANSCRIPT_BYTES,
};
use anima_service_adapters::{TranscriptionAvailability, TranscriptionError, TranscriptionPort};
use async_trait::async_trait;
use bytes::Bytes;
use futures::StreamExt;
use reqwest::header::{HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use reqwest::multipart::{Form, Part};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use zeroize::Zeroizing;

const ENDPOINT: &str = "https://api.openai.com/v1/audio/transcriptions";
const OUTGOING_AUDIO_BODIES: usize = 2;

fn body_leases() -> Arc<Semaphore> {
    static LEASES: OnceLock<Arc<Semaphore>> = OnceLock::new();
    Arc::clone(LEASES.get_or_init(|| Arc::new(Semaphore::new(OUTGOING_AUDIO_BODIES))))
}

// The permit follows the physical allocation, including any transport-owned
// Bytes clones after the calling future is cancelled or dropped.
struct LeasedAudio {
    bytes: Vec<u8>,
    _permit: OwnedSemaphorePermit,
}

impl AsRef<[u8]> for LeasedAudio {
    fn as_ref(&self) -> &[u8] {
        &self.bytes
    }
}

fn leased_audio(audio: WavAudio, leases: Arc<Semaphore>) -> Result<Bytes, TranscriptionError> {
    let permit = leases
        .try_acquire_owned()
        .map_err(|_| TranscriptionError::Busy)?;
    Ok(Bytes::from_owner(LeasedAudio {
        bytes: audio.into_bytes(),
        _permit: permit,
    }))
}

/// API-key-only OpenAI Platform transcription. Construction performs no request.
/// The endpoint and model cannot be changed by callers.
pub struct OpenAiTranscriptionAdapter {
    client: reqwest::Client,
    api_key: Zeroizing<String>,
    endpoint: String,
    leases: Arc<Semaphore>,
}

impl OpenAiTranscriptionAdapter {
    pub fn new(api_key: String) -> Result<Self, TranscriptionError> {
        Self::construct(api_key, ENDPOINT.to_owned(), body_leases())
    }

    fn construct(
        api_key: String,
        endpoint: String,
        leases: Arc<Semaphore>,
    ) -> Result<Self, TranscriptionError> {
        let api_key = Zeroizing::new(api_key);
        if api_key.trim().is_empty() || HeaderValue::from_str(api_key.as_str()).is_err() {
            return Err(TranscriptionError::Unavailable);
        }
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .retry(reqwest::retry::never())
            .build()
            .map_err(|_| TranscriptionError::Unavailable)?;
        Ok(Self {
            client,
            api_key,
            endpoint,
            leases,
        })
    }

    #[cfg(test)]
    fn for_test(endpoint: String, leases: Arc<Semaphore>) -> Result<Self, TranscriptionError> {
        let url = reqwest::Url::parse(&endpoint).map_err(|_| TranscriptionError::Unavailable)?;
        if url.scheme() != "http"
            || url.host_str() != Some("127.0.0.1")
            || url.port().is_none()
            || url.path() != "/v1/audio/transcriptions"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(TranscriptionError::Unavailable);
        }
        Self::construct("synthetic-test-key".to_owned(), endpoint, leases)
    }

    async fn request(&self, audio: WavAudio) -> Result<String, TranscriptionError> {
        let bytes = leased_audio(audio, Arc::clone(&self.leases))?;
        let length = u64::try_from(bytes.len()).map_err(|_| TranscriptionError::Failed)?;
        let part = Part::stream_with_length(reqwest::Body::from(bytes), length)
            .file_name("audio.wav")
            .mime_str("audio/wav")
            .map_err(|_| TranscriptionError::Failed)?;
        let form = Form::new()
            .text("model", "whisper-1")
            .part("file", part)
            .text("response_format", "text");
        let authorization = Zeroizing::new(format!("Bearer {}", self.api_key.as_str()));
        let mut header = HeaderValue::from_str(authorization.as_str())
            .map_err(|_| TranscriptionError::Unavailable)?;
        header.set_sensitive(true);
        let response = self
            .client
            .post(&self.endpoint)
            .header(AUTHORIZATION, header)
            .multipart(form)
            .send()
            .await
            .map_err(|_| TranscriptionError::Failed)?;
        if !response.status().is_success() {
            return Err(TranscriptionError::Failed);
        }
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .ok_or(TranscriptionError::InvalidResponse)?;
        let mut content_type_parts = content_type.split(';');
        if !content_type_parts
            .next()
            .is_some_and(|value| value.trim().eq_ignore_ascii_case("text/plain"))
        {
            return Err(TranscriptionError::InvalidResponse);
        }
        // A declared alternative encoding is incompatible with the fixed UTF-8 contract.
        for parameter in content_type_parts {
            let Some((name, value)) = parameter.trim().split_once('=') else {
                return Err(TranscriptionError::InvalidResponse);
            };
            if !name.trim().eq_ignore_ascii_case("charset")
                || !value.trim().trim_matches('"').eq_ignore_ascii_case("utf-8")
            {
                return Err(TranscriptionError::InvalidResponse);
            }
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_TRANSCRIPT_BYTES as u64)
        {
            return Err(TranscriptionError::InvalidResponse);
        }
        let mut chunks = response.bytes_stream();
        let mut body = Vec::with_capacity(MAX_TRANSCRIPT_BYTES + 1);
        while let Some(chunk) = chunks.next().await {
            let chunk = chunk.map_err(|_| TranscriptionError::Failed)?;
            let remaining = (MAX_TRANSCRIPT_BYTES + 1).saturating_sub(body.len());
            body.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
            if body.len() > MAX_TRANSCRIPT_BYTES {
                return Err(TranscriptionError::InvalidResponse);
            }
        }
        let text = std::str::from_utf8(&body).map_err(|_| TranscriptionError::InvalidResponse)?;
        validate_transcript_text(text)
    }
}

fn check_control(cancel: &CancelSignal, deadline: Instant) -> Result<(), TranscriptionError> {
    if cancel.is_cancelled() {
        Err(TranscriptionError::Cancelled)
    } else if Instant::now() >= deadline {
        Err(TranscriptionError::TimedOut)
    } else {
        Ok(())
    }
}

#[async_trait]
impl TranscriptionPort for OpenAiTranscriptionAdapter {
    fn availability(&self) -> TranscriptionAvailability {
        TranscriptionAvailability::Available
    }

    async fn transcribe(
        &self,
        audio: WavAudio,
        cancel: CancelSignal,
        deadline: Instant,
    ) -> Result<String, TranscriptionError> {
        check_control(&cancel, deadline)?;
        let result = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(TranscriptionError::Cancelled),
            _ = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)) => return Err(TranscriptionError::TimedOut),
            result = self.request(audio) => result,
        };
        check_control(&cancel, deadline)?;
        result
    }
}

#[cfg(test)]
#[path = "transcription_tests.rs"]
mod tests;
