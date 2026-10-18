use super::{
    contracts::{
        TranscriptionRequest, TranscriptionResponse, VoiceCapabilities, VoiceProvider, VoiceReason,
    },
    jobs::{authorize, body, no_store},
    json_response, ApiError, ApiErrorKind, AppState,
};
use crate::{
    attachments::metadata::valid_attachment_id,
    sessions::is_valid_session_id,
    voice::{check_scope, VoiceError},
};
use anima_service_adapters::audio_content::TranscriptionAvailability;
use axum::{
    extract::{Path, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};

fn error(error: VoiceError) -> Response {
    let (status, message) = match error {
        VoiceError::NotFound => (StatusCode::NOT_FOUND, "Voice scope or source was not found"),
        VoiceError::Duplicate => (
            StatusCode::CONFLICT,
            "Voice operation identifier was already used",
        ),
        VoiceError::Busy => (
            StatusCode::TOO_MANY_REQUESTS,
            "Voice operation capacity is busy",
        ),
        VoiceError::Unavailable => (
            StatusCode::SERVICE_UNAVAILABLE,
            "Voice transcription is unavailable",
        ),
        VoiceError::Cancelled => (
            StatusCode::CONFLICT,
            "Voice operation was cancelled locally",
        ),
        VoiceError::TimedOut => (StatusCode::GATEWAY_TIMEOUT, "Voice operation timed out"),
        VoiceError::InvalidSource => (
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "Voice source must be a supported WAV attachment",
        ),
        VoiceError::InvalidAudio => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "WAV audio is malformed or exceeds its duration limit",
        ),
        VoiceError::AudioTooLarge => (
            StatusCode::PAYLOAD_TOO_LARGE,
            "WAV audio exceeds its size limit",
        ),
        VoiceError::Failed => (StatusCode::BAD_GATEWAY, "Voice transcription failed"),
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
    no_store(ApiError::bad_request_static("Voice request is invalid").into_response())
}
fn valid_scope(agent: &str, session: &str) -> bool {
    is_valid_session_id(agent) && is_valid_session_id(session)
}
fn operation_id(id: &str) -> Option<uuid::Uuid> {
    let id = uuid::Uuid::parse_str(id).ok()?;
    (id.get_version_num() == 4 && id.get_variant() == uuid::Variant::RFC4122).then_some(id)
}

#[utoipa::path(get, path="/api/agents/{agent_id}/sessions/{session_id}/voice", tag="voice", params(("agent_id"=String,Path),("session_id"=String,Path)), responses((status=200, description="Safe configured voice capability; no provider request",body=VoiceCapabilities),(status=403,description="Owner required"),(status=404,description="Own Web Chat scope not found")))]
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
    let availability = state.voice.availability();
    let reason = match availability {
        TranscriptionAvailability::Available => VoiceReason::Available,
        TranscriptionAvailability::Disabled => VoiceReason::Disabled,
        TranscriptionAvailability::MissingCredentials => VoiceReason::MissingCredentials,
        TranscriptionAvailability::UnsupportedConfiguration => {
            VoiceReason::UnsupportedConfiguration
        }
        TranscriptionAvailability::Unavailable => VoiceReason::Unavailable,
    };
    no_store(json_response(
        StatusCode::OK,
        &VoiceCapabilities {
            version: 1,
            provider: VoiceProvider::Openai,
            model: "whisper-1".into(),
            available: availability == TranscriptionAvailability::Available,
            reason,
        },
    ))
}

#[utoipa::path(post, path="/api/agents/{agent_id}/sessions/{session_id}/voice/transcriptions", tag="voice", params(("agent_id"=String,Path),("session_id"=String,Path)), request_body=TranscriptionRequest, responses((status=200,description="Ephemeral transcript draft; never sent or persisted",body=TranscriptionResponse),(status=400,description="Invalid request"),(status=403,description="Owner required"),(status=404,description="Scope/source changed or absent"),(status=408,description="Outer request deadline expired"),(status=409,description="Duplicate or locally cancelled operation"),(status=413,description="Audio size limit exceeded"),(status=415,description="Unsupported WAV source"),(status=422,description="Invalid WAV source"),(status=429,description="Busy"),(status=502,description="Transcription failed"),(status=503,description="Unavailable"),(status=504,description="Absolute deadline expired")))]
pub(super) async fn transcribe(
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
    let input: TranscriptionRequest = match body(&state, request).await {
        Ok(input) => input,
        Err(response) => return response,
    };
    let Some(operation) = operation_id(&input.operation_id) else {
        return invalid();
    };
    if !input.consent || !valid_attachment_id(&input.attachment_id) {
        return invalid();
    }
    match state
        .voice
        .transcribe(
            state.daemon.clone(),
            agent,
            session,
            operation,
            input.attachment_id.clone(),
        )
        .await
    {
        Ok(text) => no_store(json_response(
            StatusCode::OK,
            &TranscriptionResponse {
                version: 1,
                operation_id: input.operation_id,
                attachment_id: input.attachment_id,
                provider: input.provider,
                model: "whisper-1".into(),
                text,
            },
        )),
        Err(e) => error(e),
    }
}

#[utoipa::path(delete, path="/api/agents/{agent_id}/sessions/{session_id}/voice/transcriptions/{operation_id}", tag="voice", params(("agent_id"=String,Path),("session_id"=String,Path),("operation_id"=String,Path)), responses((status=204,description="Idempotent local cancellation requested; remote completion/billing uncertain"),(status=400,description="Invalid identifier"),(status=403,description="Owner required"),(status=404,description="Own Web Chat scope not found"),(status=429,description="Operation map busy")))]
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
    match state.voice.cancel(&agent, &session, operation) {
        Ok(()) => no_store(StatusCode::NO_CONTENT.into_response()),
        Err(e) => error(e),
    }
}
