//! Subscription inference only. OAuth storage, refresh, and account selection belong to the host.
use crate::attachments::{model_supports_images, prepare_user_content};
use crate::common::{
    assistant_tool_calls_json, tool_call_args, tool_call_id, tool_parameters_schema_json,
};
use anima_core::{
    model_transport_delay, AgentConfig, Content, MessageRole, ModelAdapter, ModelGenerateRequest,
    ModelGenerateResponse, ModelStopReason, ModelStreamFrame, ModelStreamSink, TokenUsage,
    ToolCall, MAX_CAPABILITY_ARGUMENT_BYTES, MAX_CAPABILITY_ID_BYTES,
};
use async_trait::async_trait;
use futures::StreamExt;
use serde_json::{json, Value};
use std::{collections::BTreeMap, sync::Mutex, time::Duration};
use zeroize::Zeroizing;

const ENDPOINT: &str = "https://chatgpt.com/backend-api/codex/responses";
const MAX_EVENT: usize = 4 * 1024 * 1024;
const MAX_STREAM: usize = 16 * 1024 * 1024;
const MAX_TEXT: usize = 1024 * 1024;
const MAX_ERROR_BODY: usize = 64 * 1024;

/// A host-supplied ChatGPT subscription credential; never falls back to API billing.
pub struct ChatGptResponsesAdapter {
    client: reqwest::Client,
    token: Zeroizing<String>,
    account_id: String,
    endpoint: String,
}

impl ChatGptResponsesAdapter {
    pub fn new(access_token: String, account_id: String) -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(300))
            .build()
            .map_err(|_| "Unable to initialize ChatGPT transport".to_owned())?;
        Ok(Self {
            client,
            token: Zeroizing::new(access_token),
            account_id,
            endpoint: ENDPOINT.into(),
        })
    }

    async fn request(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
    ) -> Result<reqwest::Response, String> {
        if self.token.is_empty() || self.account_id.is_empty() {
            return Err("Connect your ChatGPT subscription in settings".into());
        }
        let body = request_body(config, request)?;
        let retry_count = config
            .settings
            .as_ref()
            .and_then(|settings| settings.max_retries)
            .map(|value| value.min(1))
            .unwrap_or(1);
        let mut attempt = 0;
        loop {
            let response = self
                .client
                .post(&self.endpoint)
                .bearer_auth(self.token.as_str())
                .header("chatgpt-account-id", &self.account_id)
                .header("originator", "anima")
                .header("User-Agent", "anima/0.1.0")
                .header("OpenAI-Beta", "responses=experimental")
                .header("accept", "text/event-stream")
                .json(&body)
                .send()
                .await
                .map_err(|_| "ChatGPT request failed or timed out".to_owned())?;
            if response.status().is_success() {
                return Ok(response);
            }
            let failure = http_failure(response).await;
            if attempt >= retry_count || !failure.retryable {
                return Err(failure.diagnostic);
            }
            attempt += 1;
            model_transport_delay(Duration::from_millis(250)).await;
        }
    }
}

#[async_trait]
impl ModelAdapter for ChatGptResponsesAdapter {
    fn provider(&self) -> &str {
        "chatgpt"
    }

    async fn generate(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        let sink = FinalSink(Mutex::new(None));
        self.stream(config, request, &sink).await?;
        sink.0
            .into_inner()
            .map_err(|_| "ChatGPT response lock failed".to_owned())?
            .ok_or_else(|| "ChatGPT stream ended without a completed response".into())
    }

    async fn stream(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
        sink: &dyn ModelStreamSink,
    ) -> Result<(), String> {
        consume(self.request(config, request).await?, sink).await
    }
}

struct FinalSink(Mutex<Option<ModelGenerateResponse>>);
#[async_trait]
impl ModelStreamSink for FinalSink {
    async fn emit(&self, frame: ModelStreamFrame) -> Result<(), String> {
        if let ModelStreamFrame::Final(response) = frame {
            *self
                .0
                .lock()
                .map_err(|_| "ChatGPT response lock failed".to_owned())? = Some(response);
        }
        Ok(())
    }
}

fn request_body(config: &AgentConfig, request: &ModelGenerateRequest) -> Result<Value, String> {
    let supports_images = model_supports_images("chatgpt", &config.model);
    let mut input = Vec::new();
    for message in &request.messages {
        match message.role {
            MessageRole::Tool => input.push(json!({"type":"function_call_output", "call_id":tool_call_id(message), "output":message.content.text})),
            MessageRole::Assistant => {
                if !message.content.text.is_empty() {
                    input.push(json!({"role":"assistant", "content":[{"type":"output_text","text":message.content.text}]}));
                }
                for call in assistant_tool_calls_json(message)?.unwrap_or_default() {
                    input.push(json!({"type":"function_call", "call_id":call["id"],
                        "name":call["function"]["name"], "arguments":call["function"]["arguments"]}));
                }
            }
            MessageRole::User => {
                let prepared = prepare_user_content(&message.content, supports_images)?;
                let mut parts = vec![json!({"type":"input_text", "text":prepared.text})];
                parts.extend(prepared.images.iter().map(|image| json!({"type":"input_image", "image_url":image.data_uri})));
                input.push(json!({"role":"user", "content":parts}));
            }
            MessageRole::System => input.push(json!({"role":"developer",
                "content":[{"type":"input_text", "text":message.content.text}]})),
        }
    }
    let tools: Vec<Value> = config
        .tools
        .iter()
        .flatten()
        .map(|tool| {
            json!({
                "type":"function", "name":tool.name, "description":tool.description,
                "parameters":tool_parameters_schema_json(tool), "strict":false,
            })
        })
        .collect();
    // ChatGPT subscription transport requires streaming and store=false. API-only
    // temperature / max_tokens controls are deliberately not sent to reasoning models.
    let mut body = json!({"model":config.model, "instructions":request.system, "input":input,
        "stream":true, "store":false});
    if !tools.is_empty() {
        body["tools"] = json!(tools);
        body["tool_choice"] = json!("auto");
        body["parallel_tool_calls"] = json!(true);
    }
    Ok(body)
}

fn malformed() -> String {
    "Malformed ChatGPT subscription response".into()
}

const UNKNOWN_FAILURE: &str =
    "ChatGPT subscription generation failed; retry or check account access";

// These are protocol constants, never arbitrary provider diagnostic text. Official
// recovery guidance: https://developers.openai.com/siwc/token-sharing-open-source/errors-and-recovery
const KNOWN_FAILURES: &[(&str, &str)] = &[
    ("invalid_function_parameters", "ChatGPT subscription tool parameter schema is invalid; review the tool schema configuration"),
    ("invalid_prompt", "ChatGPT subscription request was rejected; review the input and tool configuration"),
    ("model_not_found", "ChatGPT subscription selected model is unavailable; review the model and account access"),
    ("unsupported_model", "ChatGPT subscription selected model is unsupported; review the model selection"),
    ("invalid_request_error", "ChatGPT subscription request is invalid; review the request configuration"),
    ("invalid_api_key", "ChatGPT subscription authorization was rejected; check the selected account and sign-in"),
    ("authentication_error", "ChatGPT subscription authorization was rejected; check the selected account and sign-in"),
    ("invalid_authentication", "ChatGPT subscription authorization was rejected; check the selected account and sign-in"),
    ("token_expired", "ChatGPT subscription authorization expired; reconnect in settings"),
    ("permission_denied", "ChatGPT subscription permission was denied; check account and integration access"),
    ("permission_error", "ChatGPT subscription permission was denied; check account and integration access"),
    ("rate_limit_exceeded", "ChatGPT subscription rate limit reached; pause requests before retrying"),
    ("insufficient_quota", "ChatGPT subscription usage limit reached; check ChatGPT settings Usage before retrying"),
    ("usage_limit_exceeded", "ChatGPT subscription usage limit reached; check ChatGPT settings Usage before retrying"),
    ("server_error", "ChatGPT subscription is temporarily unavailable; preserve the connection and try later"),
    ("internal_server_error", "ChatGPT subscription is temporarily unavailable; preserve the connection and try later"),
    ("service_unavailable", "ChatGPT subscription is temporarily unavailable; preserve the connection and try later"),
    ("context_length_exceeded", "ChatGPT subscription context limit exceeded; reduce the conversation or input"),
    ("subscription_sharing_user_not_eligible", "ChatGPT plan usage is not eligible for this account or policy; review account and integration access"),
    ("subscription_sharing_usage_limit_exceeded", "ChatGPT plan usage limit reached; check ChatGPT settings Usage before retrying"),
    ("subscription_sharing_usage_unavailable", "ChatGPT plan usage availability is temporarily unavailable; preserve the connection and try later"),
    ("subscription_sharing_unsupported_capability", "ChatGPT plan request has an unsupported capability; review the indicated request setting"),
    ("subscription_sharing_route_not_supported", "ChatGPT plan inference route is unsupported; review the integration endpoint"),
    ("subscription_sharing_invalid_user", "ChatGPT subscriber context could not be validated; check account authorization"),
    ("subscription_sharing_user_unavailable", "ChatGPT account availability could not be checked; preserve the connection and try later"),
    ("chatpass_v2_scope_not_authorized", "ChatGPT plan permission context does not authorize this operation; review the client and granted scopes"),
    ("chatpass_v2_invalid_authorization_context", "ChatGPT plan permission context is invalid; review the client and granted scopes"),
];

fn known_parameter(param: &str) -> Option<&'static str> {
    if indexed_tool_parameter(param) {
        return Some("tools");
    }
    [
        "model",
        "input",
        "instructions",
        "tools",
        "tool_choice",
        "parallel_tool_calls",
        "service_tier",
        "reasoning",
        "reasoning.effort",
        "max_output_tokens",
        "temperature",
        "top_p",
        "store",
        "stream",
        "include",
        "background",
        "text",
        "text.format",
    ]
    .into_iter()
    .find(|known| *known == param)
}

fn indexed_tool_parameter(param: &str) -> bool {
    if param.len() > 256 {
        return false;
    }
    let Some(rest) = param.strip_prefix("tools") else {
        return false;
    };
    let (index, suffix) = if let Some(rest) = rest.strip_prefix('[') {
        let Some((index, suffix)) = rest.split_once(']') else {
            return false;
        };
        let Some(suffix) = suffix.strip_prefix('.') else {
            return false;
        };
        (index, suffix)
    } else if let Some(rest) = rest.strip_prefix('.') {
        let Some((index, suffix)) = rest.split_once('.') else {
            return false;
        };
        (index, suffix)
    } else {
        return false;
    };
    if index.is_empty() || index.len() > 4 || !index.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    let suffix = suffix.strip_prefix("function.").unwrap_or(suffix);
    if suffix != "parameters" && !suffix.starts_with("parameters.") {
        return false;
    }
    suffix.split('.').all(|part| {
        !part.is_empty() && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
    })
}

fn structured_failure(payload: &Value) -> Option<String> {
    // Responses error events carry top-level fields; response.failed carries
    // response.error. Upstream agent/codex_runtime.py::_raise_stream_error also
    // prefers top-level error fields. Here only whitelisted constants escape.
    let error = failure_payload(payload);
    let (source, code) = match error.get("code") {
        None | Some(Value::Null) => ("type", known_failure_type(error.get("type")?)?),
        Some(code) => ("code", code.as_str()?),
    };
    let &(safe_code, message) = KNOWN_FAILURES.iter().find(|(known, _)| *known == code)?;
    let mut diagnostic = format!("{message} ({source}: {safe_code}");
    if let Some(param) = error
        .get("param")
        .and_then(Value::as_str)
        .and_then(known_parameter)
    {
        diagnostic.push_str("; parameter: ");
        diagnostic.push_str(param);
    }
    diagnostic.push(')');
    Some(diagnostic)
}

fn known_failure_type(value: &Value) -> Option<&'static str> {
    match value.as_str()? {
        "invalid_request_error" => Some("invalid_request_error"),
        "authentication_error" => Some("authentication_error"),
        "permission_error" => Some("permission_error"),
        "server_error" => Some("server_error"),
        _ => None,
    }
}

fn http_model_access_failure(status: u16, payload: &Value) -> Option<&'static str> {
    if status != 400 {
        return None;
    }
    let error = failure_payload(payload);
    if !error.is_object() {
        return None;
    }
    if error.get("code").is_some_and(|value| !value.is_null()) {
        return None;
    }
    if let Some(kind) = error.get("type").filter(|value| !value.is_null()) {
        known_failure_type(kind)?;
    }
    let detail = payload.get("detail")?;
    let text = detail
        .as_str()
        .or_else(|| detail.get("message")?.as_str())?;
    // Pinned Hermes model-entitlement marker, restricted to this HTTP shape.
    // ASCII comparison keeps provider/model text out of diagnostic allocation.
    const MARKER: &[u8] = b"model is not supported when using codex with a chatgpt account";
    text.as_bytes().windows(MARKER.len()).any(|part| part.eq_ignore_ascii_case(MARKER))
        .then_some("ChatGPT subscription selected model is not supported for this account; review the model and account access")
}

fn http_status_failure(status: u16) -> String {
    match status {
        401 => "ChatGPT subscription authorization expired; reconnect in settings".into(),
        403 => "ChatGPT subscription does not allow this request or model".into(),
        429 => {
            "ChatGPT subscription usage limit reached; check ChatGPT settings Usage before retrying"
                .into()
        }
        status => format!("ChatGPT subscription request failed (HTTP {status})"),
    }
}

struct HttpFailure {
    diagnostic: String,
    retryable: bool,
}

async fn http_failure(response: reqwest::Response) -> HttpFailure {
    let status = response.status();
    let fallback = http_status_failure(status.as_u16());
    let retryable_status = matches!(status.as_u16(), 502 | 503 | 504);
    let media_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| {
            value
                .split(';')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase()
        });
    let event_stream = media_type.as_deref() == Some("text/event-stream");
    if response
        .content_length()
        .is_some_and(|size| size > MAX_ERROR_BODY as u64)
    {
        return HttpFailure {
            diagnostic: fallback,
            retryable: false,
        };
    }
    let mut bytes = Zeroizing::new(Vec::new());
    let mut chunks = response.bytes_stream();
    while let Some(chunk) = chunks.next().await {
        let Ok(chunk) = chunk else {
            return HttpFailure {
                diagnostic: fallback,
                retryable: false,
            };
        };
        if bytes.len().saturating_add(chunk.len()) > MAX_ERROR_BODY {
            return HttpFailure {
                diagnostic: fallback,
                retryable: false,
            };
        }
        bytes.extend_from_slice(&chunk);
    }
    let parsed = serde_json::from_slice::<Value>(&bytes).ok();
    let structured = parsed.as_ref().and_then(structured_failure);
    let diagnostic = parsed
        .as_ref()
        .and_then(|payload| http_model_access_failure(status.as_u16(), payload))
        .map(str::to_owned)
        .or(structured)
        .unwrap_or(fallback);
    let structured_retryable = parsed.as_ref().and_then(failure_code).map(|code| {
        matches!(
            code,
            "server_error"
                | "internal_server_error"
                | "service_unavailable"
                | "subscription_sharing_usage_unavailable"
                | "subscription_sharing_user_unavailable"
        )
    });
    let plain_gateway_body = matches!(media_type.as_deref(), Some("text/plain" | "text/html"))
        && std::str::from_utf8(&bytes).is_ok();
    let retryable = retryable_status
        && !event_stream
        && (structured_retryable == Some(true) || (parsed.is_none() && plain_gateway_body));
    HttpFailure {
        diagnostic,
        retryable,
    }
}

fn failure_payload(payload: &Value) -> &Value {
    if payload["type"].as_str() == Some("error") && payload.get("code").is_some() {
        payload
    } else {
        payload
            .pointer("/response/error")
            .or_else(|| payload.get("error"))
            .unwrap_or(payload)
    }
}

fn failure_code(payload: &Value) -> Option<&str> {
    failure_payload(payload).get("code")?.as_str()
}

fn completed_response(response: &Value) -> Result<ModelGenerateResponse, String> {
    let status = response["status"].as_str().ok_or_else(malformed)?;
    if status != "completed" && status != "incomplete" {
        return Err(malformed());
    }
    let output = response["output"].as_array().ok_or_else(malformed)?;
    let mut text = String::new();
    let mut calls = Vec::new();
    for item in output {
        match item["type"].as_str() {
            Some("message") => {
                for part in item["content"].as_array().ok_or_else(malformed)? {
                    let next = match part["type"].as_str() {
                        Some("output_text") => part["text"].as_str().ok_or_else(malformed)?,
                        Some("refusal") => part["refusal"].as_str().ok_or_else(malformed)?,
                        _ => return Err(malformed()),
                    };
                    if text.len().saturating_add(next.len()) > MAX_TEXT {
                        return Err(malformed());
                    }
                    text.push_str(next);
                }
            }
            Some("function_call") => {
                let id = item["call_id"]
                    .as_str()
                    .filter(|s| !s.is_empty() && s.len() <= MAX_CAPABILITY_ID_BYTES)
                    .ok_or_else(malformed)?;
                let name = item["name"]
                    .as_str()
                    .filter(|s| !s.is_empty() && s.len() <= MAX_CAPABILITY_ID_BYTES)
                    .ok_or_else(malformed)?;
                let args = item["arguments"]
                    .as_str()
                    .filter(|s| s.len() <= MAX_CAPABILITY_ARGUMENT_BYTES)
                    .ok_or_else(malformed)?;
                if calls.len() >= 32 || calls.iter().any(|call: &ToolCall| call.id == id) {
                    return Err(malformed());
                }
                calls.push(ToolCall {
                    id: id.into(),
                    name: name.into(),
                    args: tool_call_args(&json!(args)).map_err(|_| malformed())?,
                });
            }
            Some("reasoning") => {}
            _ => return Err(malformed()),
        }
    }
    // Never execute potentially truncated tool calls.
    if status == "incomplete" && !calls.is_empty() {
        return Err("ChatGPT response was incomplete; no tools were executed".into());
    }
    if text.trim().is_empty() && calls.is_empty() {
        return Err("ChatGPT returned no reply or tool calls; please retry".into());
    }
    let stop_reason = if status == "incomplete" {
        ModelStopReason::MaxTokens
    } else if calls.is_empty() {
        ModelStopReason::End
    } else {
        ModelStopReason::ToolCall
    };
    let usage = &response["usage"];
    let prompt_tokens = usage["input_tokens"].as_u64().unwrap_or(0);
    let completion_tokens = usage["output_tokens"].as_u64().unwrap_or(0);
    Ok(ModelGenerateResponse {
        content: Content {
            text,
            attachments: None,
            metadata: None,
        },
        tool_calls: (!calls.is_empty()).then_some(calls),
        stop_reason,
        usage: TokenUsage {
            prompt_tokens,
            completion_tokens,
            total_tokens: usage["total_tokens"]
                .as_u64()
                .unwrap_or(prompt_tokens.saturating_add(completion_tokens)),
            cached_prompt_tokens: usage["input_tokens_details"]["cached_tokens"]
                .as_u64()
                .unwrap_or(0),
            reasoning_tokens: usage["output_tokens_details"]["reasoning_tokens"]
                .as_u64()
                .unwrap_or(0),
        },
    })
}

async fn consume(response: reqwest::Response, sink: &dyn ModelStreamSink) -> Result<(), String> {
    let mut chunks = response.bytes_stream();
    let mut pending = Vec::new();
    let mut total = 0usize;
    let mut text_bytes = 0usize;
    let mut completed_items = BTreeMap::new();
    while let Some(chunk) = chunks.next().await {
        let chunk = chunk.map_err(|_| "ChatGPT stream interrupted".to_owned())?;
        total = total.saturating_add(chunk.len());
        if total > MAX_STREAM {
            return Err(malformed());
        }
        pending.extend_from_slice(&chunk);
        loop {
            let boundary = pending
                .windows(2)
                .position(|w| w == b"\n\n")
                .map(|p| (p, 2))
                .into_iter()
                .chain(
                    pending
                        .windows(4)
                        .position(|w| w == b"\r\n\r\n")
                        .map(|p| (p, 4)),
                )
                .min_by_key(|&(p, _)| p);
            let Some((pos, len)) = boundary else {
                break;
            };
            if pos > MAX_EVENT {
                return Err(malformed());
            }
            let event = std::str::from_utf8(&pending[..pos]).map_err(|_| malformed())?;
            let data = event
                .lines()
                .filter_map(|line| line.strip_prefix("data:").map(str::trim_start))
                .collect::<Vec<_>>()
                .join("\n");
            if !data.is_empty() && data != "[DONE]" {
                let payload: Value = serde_json::from_str(&data).map_err(|_| malformed())?;
                match payload["type"].as_str() {
                    Some("response.output_text.delta") => {
                        let delta = payload["delta"].as_str().ok_or_else(malformed)?;
                        text_bytes = text_bytes.saturating_add(delta.len());
                        if text_bytes > MAX_TEXT {
                            return Err(malformed());
                        }
                        sink.emit(ModelStreamFrame::TextDelta(delta.into())).await?;
                    }
                    Some("response.output_item.done") => {
                        let index = payload["output_index"].as_u64().ok_or_else(malformed)?;
                        let item = payload
                            .get("item")
                            .filter(|v| v.is_object())
                            .ok_or_else(malformed)?;
                        if index >= 1024 || completed_items.insert(index, item.clone()).is_some() {
                            return Err(malformed());
                        }
                    }
                    Some("response.completed" | "response.incomplete") => {
                        let mut response = payload["response"].clone();
                        // Subscription streams may omit output from the terminal event.
                        // Retain completed item snapshots in output order, including tool
                        // calls. Never reconstruct executable arguments from partial deltas.
                        if response["output"].is_null()
                            || response["output"].as_array().is_some_and(Vec::is_empty)
                        {
                            response.as_object_mut().ok_or_else(malformed)?.insert(
                                "output".into(),
                                Value::Array(completed_items.into_values().collect()),
                            );
                        }
                        return sink
                            .emit(ModelStreamFrame::Final(completed_response(&response)?))
                            .await;
                    }
                    Some("error" | "response.failed") => {
                        return Err(if data.len() <= MAX_ERROR_BODY {
                            structured_failure(&payload).unwrap_or_else(|| UNKNOWN_FAILURE.into())
                        } else {
                            UNKNOWN_FAILURE.into()
                        });
                    }
                    Some(_) => {}
                    None => return Err(malformed()),
                }
            }
            pending.drain(..pos + len);
        }
        if pending.len() > MAX_EVENT {
            return Err(malformed());
        }
    }
    Err("ChatGPT stream ended without a completed response".into())
}

mod models;
#[cfg(test)]
mod models_tests;
#[cfg(test)]
mod request_fixture_tests;
#[cfg(test)]
mod tests;
pub use models::ChatGptModelsAdapter;
