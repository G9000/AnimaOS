//! Narrow OpenAI speech request adapted from Hermes's MIT-licensed
//! `tools/tts_tool_openai.py` at 1fd18277267152893a8a24792b79213e7ca3f899.
//! See HERMES-NOTICE for provenance and the retained license.

use std::sync::{Arc, OnceLock};
use std::time::Instant;

use anima_core::{
    CancelSignal, SpeechAudio, SpeechAvailability, SpeechError, SpeechPort, SpeechText,
    MAX_SPEECH_PCM_BYTES,
};
use async_trait::async_trait;
use bytes::Bytes;
use futures::StreamExt;
use reqwest::header::{
    HeaderValue, ACCEPT, ACCEPT_ENCODING, AUTHORIZATION, CONTENT_ENCODING, CONTENT_TYPE,
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use zeroize::Zeroizing;

const ENDPOINT: &str = "https://api.openai.com/v1/audio/speech";
const OUTGOING_JSON_BODIES: usize = 2;

fn body_leases() -> Arc<Semaphore> {
    static LEASES: OnceLock<Arc<Semaphore>> = OnceLock::new();
    Arc::clone(LEASES.get_or_init(|| Arc::new(Semaphore::new(OUTGOING_JSON_BODIES))))
}

// A permit follows the actual JSON allocation through transport-owned Bytes
// clones after the caller's HTTP future is cancelled or dropped.
struct LeasedJson {
    bytes: Vec<u8>,
    _permit: OwnedSemaphorePermit,
}

impl AsRef<[u8]> for LeasedJson {
    fn as_ref(&self) -> &[u8] {
        &self.bytes
    }
}

fn leased_json(bytes: Vec<u8>, leases: Arc<Semaphore>) -> Result<Bytes, SpeechError> {
    let permit = leases.try_acquire_owned().map_err(|_| SpeechError::Busy)?;
    Ok(Bytes::from_owner(LeasedJson {
        bytes,
        _permit: permit,
    }))
}

/// API-key-only OpenAI Platform speech. Construction performs no provider IO.
/// The authority, model, voice, speed and response format are fixed.
pub struct OpenAiSpeechAdapter {
    client: reqwest::Client,
    api_key: Zeroizing<String>,
    endpoint: String,
    leases: Arc<Semaphore>,
}

impl OpenAiSpeechAdapter {
    pub fn new(api_key: String) -> Result<Self, SpeechError> {
        Self::construct(api_key, ENDPOINT.to_owned(), body_leases())
    }

    fn construct(
        api_key: String,
        endpoint: String,
        leases: Arc<Semaphore>,
    ) -> Result<Self, SpeechError> {
        let api_key = Zeroizing::new(api_key);
        if api_key.trim().is_empty() || HeaderValue::from_str(api_key.as_str()).is_err() {
            return Err(SpeechError::Unavailable);
        }
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .retry(reqwest::retry::never())
            .build()
            .map_err(|_| SpeechError::Unavailable)?;
        Ok(Self {
            client,
            api_key,
            endpoint,
            leases,
        })
    }

    #[cfg(test)]
    fn for_test(endpoint: String, leases: Arc<Semaphore>) -> Result<Self, SpeechError> {
        let url = reqwest::Url::parse(&endpoint).map_err(|_| SpeechError::Unavailable)?;
        if url.scheme() != "http"
            || url.host_str() != Some("127.0.0.1")
            || url.port().is_none()
            || url.path() != "/v1/audio/speech"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(SpeechError::Unavailable);
        }
        Self::construct("synthetic-test-key".to_owned(), endpoint, leases)
    }

    async fn request(&self, text: SpeechText) -> Result<SpeechAudio, SpeechError> {
        let json = serde_json::to_vec(&serde_json::json!({
            "model": "gpt-4o-mini-tts",
            "voice": "alloy",
            "input": text.as_str(),
            "response_format": "pcm",
            "speed": 1.0,
        }))
        .map_err(|_| SpeechError::Failed)?;
        let body = leased_json(json, Arc::clone(&self.leases))?;
        let authorization = Zeroizing::new(format!("Bearer {}", self.api_key.as_str()));
        let mut header =
            HeaderValue::from_str(authorization.as_str()).map_err(|_| SpeechError::Unavailable)?;
        header.set_sensitive(true);
        let response = self
            .client
            .post(&self.endpoint)
            .header(AUTHORIZATION, header)
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "audio/pcm")
            .header(ACCEPT_ENCODING, "identity")
            .body(reqwest::Body::from(body))
            .send()
            .await
            .map_err(|_| SpeechError::Failed)?;
        if response.status() != reqwest::StatusCode::OK {
            return Err(SpeechError::Failed);
        }
        let mime = response.headers().get_all(CONTENT_TYPE);
        if mime.iter().count() != 1
            || !mime
                .iter()
                .next()
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| {
                    let value = value.trim();
                    value.eq_ignore_ascii_case("audio/pcm")
                        || value.eq_ignore_ascii_case("application/octet-stream")
                })
        {
            return Err(SpeechError::InvalidResponse);
        }
        let encodings = response.headers().get_all(CONTENT_ENCODING);
        if encodings.iter().count() > 1
            || !encodings.iter().all(|value| {
                value
                    .to_str()
                    .is_ok_and(|value| value.trim().eq_ignore_ascii_case("identity"))
            })
        {
            return Err(SpeechError::InvalidResponse);
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_SPEECH_PCM_BYTES as u64)
        {
            return Err(SpeechError::InvalidResponse);
        }
        let mut chunks = response.bytes_stream();
        let mut pcm = Vec::new();
        while let Some(chunk) = chunks.next().await {
            let chunk = chunk.map_err(|_| SpeechError::Failed)?;
            if chunk.len() > MAX_SPEECH_PCM_BYTES.saturating_sub(pcm.len()) {
                return Err(SpeechError::InvalidResponse);
            }
            pcm.extend_from_slice(&chunk);
        }
        SpeechAudio::from_pcm24k(pcm)
    }
}

fn check_control(cancel: &CancelSignal, deadline: Instant) -> Result<(), SpeechError> {
    if cancel.is_cancelled() {
        Err(SpeechError::Cancelled)
    } else if Instant::now() >= deadline {
        Err(SpeechError::TimedOut)
    } else {
        Ok(())
    }
}

#[async_trait]
impl SpeechPort for OpenAiSpeechAdapter {
    fn availability(&self) -> SpeechAvailability {
        SpeechAvailability::Available
    }

    async fn synthesize(
        &self,
        text: SpeechText,
        cancel: CancelSignal,
        deadline: Instant,
    ) -> Result<SpeechAudio, SpeechError> {
        check_control(&cancel, deadline)?;
        let result = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(SpeechError::Cancelled),
            _ = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)) => return Err(SpeechError::TimedOut),
            result = self.request(text) => result,
        };
        check_control(&cancel, deadline)?;
        result
    }
}

#[cfg(test)]
#[path = "speech_tests.rs"]
mod tests;
