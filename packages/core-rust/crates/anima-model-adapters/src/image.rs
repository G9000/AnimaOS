//! Canonical API-key-only OpenAI image requests, with no automatic retries.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use anima_core::CancelSignal;
use anima_service_adapters::{
    GeneratedImage, ImageError, ImagePort, ImageRequest, ImageUsage, IMAGE_BACKGROUND, IMAGE_COUNT,
    IMAGE_MODEL, IMAGE_OUTPUT_FORMAT, IMAGE_QUALITY, IMAGE_SIZE, MAX_IMAGE_OUTPUT_BYTES,
};
use async_trait::async_trait;
use base64::Engine;
use bytes::Bytes;
use futures::StreamExt;
use reqwest::header::{
    HeaderValue, ACCEPT, ACCEPT_ENCODING, AUTHORIZATION, CONTENT_ENCODING, CONTENT_TYPE,
};
use serde_json::Value;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use zeroize::Zeroizing;

const ENDPOINT: &str = "https://api.openai.com/v1/images";
const OUTGOING_JSON_BODIES: usize = 2;
const MAX_IMAGE_RESPONSE_BYTES: usize = 14 * 1024 * 1024;
const MAX_IMAGE_REQUEST_BYTES: usize = 14 * 1024 * 1024;
const MAX_IMAGE_BASE64_BYTES: usize = MAX_IMAGE_OUTPUT_BYTES.div_ceil(3) * 4;

fn body_leases() -> Arc<Semaphore> {
    static LEASES: OnceLock<Arc<Semaphore>> = OnceLock::new();
    Arc::clone(LEASES.get_or_init(|| Arc::new(Semaphore::new(OUTGOING_JSON_BODIES))))
}

// The permit follows the actual allocation through transport-owned Bytes
// clones, even when the caller's HTTP future is cancelled or dropped.
struct LeasedJson {
    bytes: Vec<u8>,
    _permit: OwnedSemaphorePermit,
}

impl AsRef<[u8]> for LeasedJson {
    fn as_ref(&self) -> &[u8] {
        &self.bytes
    }
}

fn leased_json(bytes: Vec<u8>, permit: OwnedSemaphorePermit) -> Bytes {
    Bytes::from_owner(LeasedJson {
        bytes,
        _permit: permit,
    })
}

/// One API-key-only OpenAI Platform image operation. Construction performs no
/// provider IO. The host selects one key; this adapter cannot rotate it.
pub struct OpenAiImageAdapter {
    client: reqwest::Client,
    api_key: Zeroizing<String>,
    endpoint: String,
    leases: Arc<Semaphore>,
}

impl std::fmt::Debug for OpenAiImageAdapter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OpenAiImageAdapter")
            .finish_non_exhaustive()
    }
}

impl OpenAiImageAdapter {
    /// # Errors
    /// Returns `NotSent` if credentials or local client construction are invalid.
    pub fn new(api_key: String) -> Result<Self, ImageError> {
        Self::construct(api_key, ENDPOINT.to_owned(), body_leases())
    }

    fn construct(
        api_key: String,
        endpoint: String,
        leases: Arc<Semaphore>,
    ) -> Result<Self, ImageError> {
        let api_key = Zeroizing::new(api_key);
        if api_key.trim().is_empty() || HeaderValue::from_str(api_key.as_str()).is_err() {
            return Err(ImageError::NotSent);
        }
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .retry(reqwest::retry::never())
            .build()
            .map_err(|_| ImageError::NotSent)?;
        Ok(Self {
            client,
            api_key,
            endpoint,
            leases,
        })
    }

    #[cfg(test)]
    fn for_test(endpoint: String, leases: Arc<Semaphore>) -> Result<Self, ImageError> {
        let url = reqwest::Url::parse(&endpoint).map_err(|_| ImageError::NotSent)?;
        if url.scheme() != "http"
            || url.host_str() != Some("127.0.0.1")
            || url.port().is_none()
            || url.path() != "/v1/images"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(ImageError::NotSent);
        }
        Self::construct("synthetic-test-key".to_owned(), endpoint, leases)
    }
}

fn build_body(
    request: ImageRequest,
    permit: OwnedSemaphorePermit,
) -> Result<(&'static str, Bytes), ImageError> {
    let (prompt, image) = match request {
        ImageRequest::Generate { prompt } => (prompt, None),
        ImageRequest::Edit { prompt, image } => (prompt, Some(image)),
    };
    let mut payload = serde_json::json!({
        "model": IMAGE_MODEL,
        "prompt": prompt.as_str(),
        "quality": IMAGE_QUALITY,
        "size": IMAGE_SIZE,
        "output_format": IMAGE_OUTPUT_FORMAT,
        "n": IMAGE_COUNT,
        "background": IMAGE_BACKGROUND,
    });
    let path = if let Some(image) = image {
        let mut data_url = format!("data:{};base64,", image.media_type());
        base64::engine::general_purpose::STANDARD.encode_string(image.bytes(), &mut data_url);
        let mut image_value = serde_json::Map::new();
        image_value.insert("image_url".into(), Value::String(data_url));
        payload["images"] = Value::Array(vec![Value::Object(image_value)]);
        "edits"
    } else {
        "generations"
    };
    let bytes = serde_json::to_vec(&payload).map_err(|_| ImageError::NotSent)?;
    if bytes.len() > MAX_IMAGE_REQUEST_BYTES {
        return Err(ImageError::NotSent);
    }
    Ok((path, leased_json(bytes, permit)))
}

fn check_control(cancel: &CancelSignal, deadline: Instant) -> Result<(), ImageError> {
    if cancel.is_cancelled() || Instant::now() >= deadline {
        Err(ImageError::NotSent)
    } else {
        Ok(())
    }
}

fn controlled_error(dispatched: &AtomicBool) -> ImageError {
    if dispatched.load(Ordering::SeqCst) {
        ImageError::OutcomeUnknown
    } else {
        ImageError::NotSent
    }
}

fn json_content_type(value: &HeaderValue) -> bool {
    let Ok(value) = value.to_str() else {
        return false;
    };
    let mut parts = value.split(';').map(str::trim);
    matches!(parts.next(), Some(mime) if mime.eq_ignore_ascii_case("application/json"))
        && parts
            .next()
            .is_none_or(|parameter| parameter.eq_ignore_ascii_case("charset=utf-8"))
        && parts.next().is_none()
}

fn usage_counter(value: &Value, key: &str) -> Result<Option<u64>, ImageError> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value.as_u64().map(Some).ok_or(ImageError::OutcomeUnknown),
    }
}

fn usage_detail(value: &Value, key: &str, counter: &str) -> Result<Option<u64>, ImageError> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(detail) if detail.is_object() => usage_counter(detail, counter),
        Some(_) => Err(ImageError::OutcomeUnknown),
    }
}

fn image_usage(payload: &Value) -> Result<Option<ImageUsage>, ImageError> {
    let usage = match payload.get("usage") {
        None | Some(Value::Null) => return Ok(None),
        Some(usage) if usage.is_object() => usage,
        Some(_) => return Err(ImageError::OutcomeUnknown),
    };
    Ok(Some(ImageUsage {
        input_tokens: usage_counter(usage, "input_tokens")?,
        output_tokens: usage_counter(usage, "output_tokens")?,
        total_tokens: usage_counter(usage, "total_tokens")?,
        input_text_tokens: usage_detail(usage, "input_tokens_details", "text_tokens")?,
        input_image_tokens: usage_detail(usage, "input_tokens_details", "image_tokens")?,
        output_text_tokens: usage_detail(usage, "output_tokens_details", "text_tokens")?,
        output_image_tokens: usage_detail(usage, "output_tokens_details", "image_tokens")?,
    }))
}

async fn decode_response(response: reqwest::Response) -> Result<GeneratedImage, ImageError> {
    // A received 4xx status is a definitive refusal. Never retain or log its
    // untrusted body, which may echo keys, source images or prompt text.
    if response.status().is_client_error() {
        return Err(ImageError::Rejected);
    }
    if response.status() != reqwest::StatusCode::OK {
        return Err(ImageError::OutcomeUnknown);
    }
    let content_types = response.headers().get_all(CONTENT_TYPE);
    if content_types.iter().count() != 1 || !content_types.iter().all(json_content_type) {
        return Err(ImageError::OutcomeUnknown);
    }
    let encodings = response.headers().get_all(CONTENT_ENCODING);
    if encodings.iter().count() > 1
        || !encodings.iter().all(|value| {
            value
                .to_str()
                .is_ok_and(|value| value.trim().eq_ignore_ascii_case("identity"))
        })
    {
        return Err(ImageError::OutcomeUnknown);
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_IMAGE_RESPONSE_BYTES as u64)
    {
        return Err(ImageError::OutcomeUnknown);
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| ImageError::OutcomeUnknown)?;
        if chunk.len() > MAX_IMAGE_RESPONSE_BYTES.saturating_sub(bytes.len()) {
            return Err(ImageError::OutcomeUnknown);
        }
        bytes.extend_from_slice(&chunk);
    }
    let payload: Value = serde_json::from_slice(&bytes).map_err(|_| ImageError::OutcomeUnknown)?;
    let outputs = payload
        .get("data")
        .and_then(Value::as_array)
        .ok_or(ImageError::OutcomeUnknown)?;
    if outputs.len() != 1 || outputs[0].get("url").is_some_and(|url| !url.is_null()) {
        return Err(ImageError::OutcomeUnknown);
    }
    let encoded = outputs[0]
        .get("b64_json")
        .and_then(Value::as_str)
        .ok_or(ImageError::OutcomeUnknown)?;
    if encoded.is_empty() || encoded.len() > MAX_IMAGE_BASE64_BYTES {
        return Err(ImageError::OutcomeUnknown);
    }
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| ImageError::OutcomeUnknown)?;
    GeneratedImage::new(decoded, image_usage(&payload)?)
}

#[async_trait]
impl ImagePort for OpenAiImageAdapter {
    async fn generate(
        &self,
        request: ImageRequest,
        cancel: CancelSignal,
        deadline: Instant,
    ) -> Result<GeneratedImage, ImageError> {
        check_control(&cancel, deadline)?;
        // Acquire before allocating JSON or base64, so even preparation is
        // bounded. Keep this Bytes clone through response validation too.
        let permit = Arc::clone(&self.leases)
            .try_acquire_owned()
            .map_err(|_| ImageError::NotSent)?;
        let (path, body) = build_body(request, permit)?;
        let authorization = Zeroizing::new(format!("Bearer {}", self.api_key.as_str()));
        let mut header =
            HeaderValue::from_str(authorization.as_str()).map_err(|_| ImageError::NotSent)?;
        header.set_sensitive(true);
        let request = self
            .client
            .post(format!("{}/{path}", self.endpoint))
            .header(AUTHORIZATION, header)
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json")
            .header(ACCEPT_ENCODING, "identity")
            .body(reqwest::Body::from(body.clone()))
            .build()
            .map_err(|_| ImageError::NotSent)?;
        check_control(&cancel, deadline)?;
        let dispatched = AtomicBool::new(false);
        let result = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(controlled_error(&dispatched)),
            _ = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)) => return Err(controlled_error(&dispatched)),
            result = async {
                dispatched.store(true, Ordering::SeqCst);
                let response = self.client.execute(request).await.map_err(|_| ImageError::OutcomeUnknown)?;
                decode_response(response).await
            } => result,
        };
        drop(body);
        if cancel.is_cancelled() || Instant::now() >= deadline {
            return Err(ImageError::OutcomeUnknown);
        }
        result
    }
}

#[cfg(test)]
#[path = "image_tests.rs"]
mod tests;
