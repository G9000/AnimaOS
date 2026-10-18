use super::{
    contracts::{SpeechCapabilities, SpeechReason, SpeechRequest, VoiceProvider},
    jobs::{authorize, no_store},
    json_response, ApiError, ApiErrorKind, AppState,
};
use crate::{
    sessions::is_valid_session_id,
    speech::{check_scope, ServiceError},
};
use anima_service_adapters::{
    SpeechAvailability, SpeechText, MAX_SPEECH_DURATION_MS, MAX_SPEECH_INPUT_BYTES,
    MAX_SPEECH_INPUT_CHARS, MAX_SPEECH_WAV_BYTES,
};
use axum::{
    body::{to_bytes, Body},
    extract::{Path, Request, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};

fn error(error: ServiceError) -> Response {
    let (status, message) = match error {
        ServiceError::NotFound => (
            StatusCode::NOT_FOUND,
            "Speech scope or source was not found",
        ),
        ServiceError::Duplicate => (
            StatusCode::CONFLICT,
            "Speech operation identifier was already used",
        ),
        ServiceError::Busy => (StatusCode::TOO_MANY_REQUESTS, "Speech capacity is busy"),
        ServiceError::Unavailable => (
            StatusCode::SERVICE_UNAVAILABLE,
            "Speech generation is unavailable",
        ),
        ServiceError::Cancelled => (
            StatusCode::CONFLICT,
            "Speech operation was cancelled locally",
        ),
        ServiceError::TimedOut => (StatusCode::GATEWAY_TIMEOUT, "Speech operation timed out"),
        ServiceError::InvalidInput => (StatusCode::BAD_REQUEST, "Speech request is invalid"),
        ServiceError::Failed => (StatusCode::BAD_GATEWAY, "Speech generation failed"),
    };
    no_store(
        ApiError {
            kind: ApiErrorKind::General,
            status,
            message: message.into(),
        }
        .into_response(),
    )
}
fn invalid() -> Response {
    error(ServiceError::InvalidInput)
}
fn valid_scope(agent: &str, session: &str) -> bool {
    is_valid_session_id(agent) && is_valid_session_id(session)
}
fn operation_id(id: &str) -> Option<uuid::Uuid> {
    let id = uuid::Uuid::parse_str(id).ok()?;
    (id.get_version_num() == 4 && id.get_variant() == uuid::Variant::RFC4122).then_some(id)
}

#[utoipa::path(get, path="/api/agents/{agent_id}/sessions/{session_id}/voice/speech", tag="voice", params(("agent_id"=String,Path),("session_id"=String,Path)), responses((status=200,description="Safe speech capability; no provider request",body=SpeechCapabilities),(status=400,description="Invalid scope"),(status=403,description="Owner required"),(status=404,description="Own Web Chat not found"),(status=408,description="Outer deadline expired")))]
pub(super) async fn capabilities(
    State(state): State<AppState>,
    Path((agent, session)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, true) {
        return response;
    }
    if !valid_scope(&agent, &session) {
        return invalid();
    }
    if let Err(e) = check_scope(&*state.daemon.read().await, &agent, &session) {
        return error(e);
    }
    let availability = state.speech.availability();
    let reason = match availability {
        SpeechAvailability::Available => SpeechReason::Available,
        SpeechAvailability::Disabled => SpeechReason::Disabled,
        SpeechAvailability::MissingCredentials => SpeechReason::MissingCredentials,
        SpeechAvailability::UnsupportedConfiguration => SpeechReason::UnsupportedConfiguration,
        SpeechAvailability::Unavailable => SpeechReason::Unavailable,
    };
    no_store(json_response(
        StatusCode::OK,
        &SpeechCapabilities {
            version: 1,
            provider: VoiceProvider::Openai,
            model: "gpt-4o-mini-tts".into(),
            voice: "alloy".into(),
            format: "wav".into(),
            available: availability == SpeechAvailability::Available,
            reason,
            max_input_chars: MAX_SPEECH_INPUT_CHARS,
            max_input_bytes: MAX_SPEECH_INPUT_BYTES,
            max_audio_bytes: MAX_SPEECH_WAV_BYTES,
            max_duration_ms: MAX_SPEECH_DURATION_MS,
        },
    ))
}

#[utoipa::path(post, path="/api/agents/{agent_id}/sessions/{session_id}/voice/speech", tag="voice", params(("agent_id"=String,Path),("session_id"=String,Path)), request_body=SpeechRequest, responses((status=200,description="Ephemeral canonical PCM24k mono16 WAV; manual playback only",body=Vec<u8>,content_type="audio/wav",headers(("Content-Length"=u64,description="Exact WAV byte length"),("X-Anima-Speech-Duration-Ms"=u32,description="Audio duration in milliseconds"))),(status=400,description="Invalid request or input limit"),(status=403,description="Owner required"),(status=404,description="Scope/source changed or absent"),(status=408,description="Outer deadline expired"),(status=409,description="Duplicate or locally cancelled"),(status=429,description="Busy"),(status=502,description="Invalid provider response or failure"),(status=503,description="Unavailable"),(status=504,description="Absolute operation deadline expired")))]
pub(super) async fn generate(
    State(state): State<AppState>,
    Path((agent, session)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    if !valid_scope(&agent, &session) {
        return invalid();
    }
    if let Err(e) = check_scope(&*state.daemon.read().await, &agent, &session) {
        return error(e);
    }
    // This bound applies to actual streamed bytes, independently of global body configuration.
    let bytes = match to_bytes(request.into_body(), 16 * 1024).await {
        Ok(bytes) => bytes,
        Err(_) => return invalid(),
    };
    let input: SpeechRequest = match serde_json::from_slice(&bytes) {
        Ok(input) => input,
        Err(_) => return invalid(),
    };
    let Some(operation) = operation_id(&input.operation_id) else {
        return invalid();
    };
    if !input.consent
        || input.message_id.len() > 128
        || !is_valid_session_id(&input.message_id)
        || SpeechText::new(input.text.clone()).is_err()
    {
        return invalid();
    }
    match state
        .speech
        .generate(
            state.daemon.clone(),
            agent,
            session,
            operation,
            input.message_id,
            input.text,
        )
        .await
    {
        Ok(output) => {
            let duration = output.duration_ms();
            let bytes = output.into_bytes();
            let length = bytes.len();
            let mut response = Body::from(bytes).into_response();
            response
                .headers_mut()
                .insert(header::CONTENT_TYPE, HeaderValue::from_static("audio/wav"));
            response.headers_mut().insert(
                header::CONTENT_LENGTH,
                HeaderValue::from_str(&length.to_string()).expect("bounded numeric length"),
            );
            response.headers_mut().insert(
                "x-anima-speech-duration-ms",
                HeaderValue::from_str(&duration.to_string()).expect("bounded numeric duration"),
            );
            no_store(response)
        }
        Err(e) => error(e),
    }
}

#[utoipa::path(delete, path="/api/agents/{agent_id}/sessions/{session_id}/voice/speech/{operation_id}", tag="voice", params(("agent_id"=String,Path),("session_id"=String,Path),("operation_id"=String,Path)), responses((status=204,description="Local cancel acknowledged; remote inference and billing uncertain"),(status=400,description="Invalid identifier"),(status=403,description="Owner required"),(status=404,description="Own Web Chat not found"),(status=408,description="Outer deadline expired"),(status=429,description="Operation map busy")))]
pub(super) async fn cancel(
    State(state): State<AppState>,
    Path((agent, session, operation)): Path<(String, String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    if !valid_scope(&agent, &session) {
        return invalid();
    }
    let Some(operation) = operation_id(&operation) else {
        return invalid();
    };
    if let Err(e) = check_scope(&*state.daemon.read().await, &agent, &session) {
        return error(e);
    }
    match state.speech.cancel(&agent, &session, operation) {
        Ok(()) => no_store(StatusCode::NO_CONTENT.into_response()),
        Err(e) => error(e),
    }
}
