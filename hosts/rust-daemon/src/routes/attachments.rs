//! Authenticated bounded upload and byte delivery. Mutations outlive a dropped
//! HTTP future and serialize with deletion using the control-plane transaction.
use super::{
    contracts::{AttachmentEnvelope, AttachmentResponse},
    jobs::{authorize, no_store},
    json_response,
    multipart::{parse_form_with, FormLimits, FORM_TOO_LARGE},
    sessions::rejected,
    ApiError, AppState,
};
use crate::attachments::{
    files::{read_attachment_bytes, save_upload},
    metadata::valid_attachment_id,
    validation::validate_upload,
    AttachmentError, AttachmentKind, AttachmentRecord, MAX_DOCUMENT_BYTES, MAX_UPLOAD_FORM_BYTES,
    MAX_UPLOAD_HEADER_BYTES, MAX_UPLOAD_PARTS,
};
use crate::sessions::{is_valid_session_id, views};
use axum::body::to_bytes;
use axum::extract::{Path, Request, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};

pub(super) const ATTACHMENT_NEEDS_WORKSPACE: &str =
    "Configure a workspace before uploading attachments";
pub(super) const ATTACHMENT_READ_ONLY: &str = "This session cannot receive attachments";
pub(super) const ATTACHMENT_INVALID_FORM: &str = "Expected exactly one file part with a filename";
pub(super) const ATTACHMENT_INVALID_ID: &str = "Attachment route identifier is invalid";
pub(super) const ATTACHMENT_TOO_LARGE: &str = "Attachment exceeds its size limit";
pub(super) const ATTACHMENT_INVALID: &str = "Attachment filename or content is not supported";
pub(super) const ATTACHMENT_UPLOAD_FAILED: &str = "Attachment could not be saved";
pub(super) const ATTACHMENT_CHANGED: &str =
    "Attachment file is unavailable or no longer matches its upload";
pub(super) const ATTACHMENT_STORE_FAILED: &str = "Attachment history is unavailable";
const UPLOAD_LIMITS: FormLimits = FormLimits {
    max_total_bytes: MAX_UPLOAD_FORM_BYTES,
    max_parts: MAX_UPLOAD_PARTS,
    max_header_bytes: MAX_UPLOAD_HEADER_BYTES,
    max_part_bytes: MAX_DOCUMENT_BYTES,
};

fn invalid(error: AttachmentError) -> ApiError {
    match error {
        AttachmentError::TooLarge => ApiError {
            kind: crate::routes::ApiErrorKind::General,
            status: StatusCode::PAYLOAD_TOO_LARGE,
            message: ATTACHMENT_TOO_LARGE.into(),
        },
        AttachmentError::AudioInvalid => ApiError {
            kind: crate::routes::ApiErrorKind::General,
            status: StatusCode::UNPROCESSABLE_ENTITY,
            message: "WAV audio is malformed or exceeds its duration limit".into(),
        },
        AttachmentError::AudioUnsupported => ApiError {
            kind: crate::routes::ApiErrorKind::General,
            status: StatusCode::UNSUPPORTED_MEDIA_TYPE,
            message: "WAV audio format is not supported".into(),
        },
        _ => ApiError::bad_request_static(ATTACHMENT_INVALID),
    }
}

#[utoipa::path(post,path="/api/agents/{agent_id}/sessions/{session_id}/attachments",tag="attachments",
    params(("agent_id"=String,Path),("session_id"=String,Path)),
    request_body(content=String,content_type="multipart/form-data",description="Exactly one file part; image10MiB, UTF-8text1MiB, document25MiB, PCM16 WAV audio4MiB/120s. Client MIME is advisory."),
    responses((status=201,description="Durably saved attachment metadata",body=AttachmentEnvelope),(status=400,description="Invalid form, filename, or non-audio content"),(status=403,description="Owner authorization required"),(status=404,description="Agent or session not found"),(status=409,description="Workspace missing or session read-only"),(status=413,description="Body or classified file exceeds its bound"),(status=415,description="Unsupported WAV format"),(status=422,description="Malformed WAV or excessive audio duration"),(status=503,description="Storage unavailable")))]
pub(super) async fn upload(
    State(state): State<AppState>,
    Path((agent_id, session_id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    if !is_valid_session_id(&agent_id) || !is_valid_session_id(&session_id) {
        return rejected(ApiError::bad_request_static(ATTACHMENT_INVALID_ID));
    }
    let content_type = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let body = match to_bytes(request.into_body(), MAX_UPLOAD_FORM_BYTES).await {
        Ok(body) => body,
        Err(_) => return rejected(invalid(AttachmentError::TooLarge)),
    };
    let parts = match parse_form_with(&content_type, &body, &UPLOAD_LIMITS) {
        Ok(parts) => parts,
        Err(FORM_TOO_LARGE) => return rejected(invalid(AttachmentError::TooLarge)),
        Err(_) => return rejected(ApiError::bad_request_static(ATTACHMENT_INVALID_FORM)),
    };
    let mut parts = parts.into_iter();
    let Some(part) = parts
        .next()
        .filter(|part| part.name == "file" && parts.len() == 0)
    else {
        return rejected(ApiError::bad_request_static(ATTACHMENT_INVALID_FORM));
    };
    let Some(filename) = part.filename.filter(|name| !name.trim().is_empty()) else {
        return rejected(ApiError::bad_request_static(ATTACHMENT_INVALID_FORM));
    };
    let validated = match validate_upload(&filename, &part.bytes) {
        Ok(value) => value,
        Err(error) => return rejected(invalid(error)),
    };
    // The owned task retains the transaction through disk and direct history
    // persistence. No state guard or std mutex crosses either operation.
    let mutation = tokio::spawn(async move {
        let _transaction = state.agent_runs.control_plane_transaction().await;
        let (root, store) = {
            let guard = state.daemon.read().await;
            let session = guard
                .sessions
                .get(&agent_id, &session_id)
                .filter(|_| guard.agents.contains_key(&agent_id))
                .ok_or_else(ApiError::not_found)?;
            if !session
                .capabilities(views::automation_exists(&guard, session))
                .send
            {
                return Err(ApiError::conflict(ATTACHMENT_READ_ONLY));
            }
            let root = guard
                .workspace
                .as_ref()
                .ok_or_else(|| ApiError::conflict(ATTACHMENT_NEEDS_WORKSPACE))?
                .root_path
                .clone();
            (root, guard.history.store())
        };
        let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let name = validated.name;
        let size = part.bytes.len() as u64;
        let saved =
            tokio::task::spawn_blocking(move || save_upload(&root, &date, &name, &part.bytes))
                .await
                .map_err(|_| ApiError::service_unavailable(ATTACHMENT_UPLOAD_FAILED))?
                .map_err(|_| ApiError::service_unavailable(ATTACHMENT_UPLOAD_FAILED))?;
        let name = saved
            .path
            .rsplit('/')
            .next()
            .ok_or_else(|| ApiError::service_unavailable(ATTACHMENT_UPLOAD_FAILED))?
            .to_string();
        let record = AttachmentRecord {
            id: format!("att-{}", uuid::Uuid::new_v4()),
            agent_id,
            session_id,
            name,
            mime: validated.mime,
            size,
            kind: validated.kind,
            path: saved.path,
            created_at_ms: anima_core::primitives::now_millis(),
            workspace_root: saved.workspace_root,
            sha256: saved.sha256,
        };
        store
            .upsert_attachments(std::slice::from_ref(&record))
            .await
            .map_err(|_| ApiError::service_unavailable(ATTACHMENT_STORE_FAILED))?;
        Ok::<_, ApiError>(AttachmentEnvelope {
            attachment: AttachmentResponse::from(&record),
        })
    });
    match mutation.await {
        Ok(Ok(result)) => no_store(json_response(StatusCode::CREATED, &result)),
        Ok(Err(error)) => rejected(error),
        Err(_) => rejected(ApiError::service_unavailable(ATTACHMENT_UPLOAD_FAILED)),
    }
}

fn disposition(name: &str, image: bool) -> String {
    let mut fallback: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ' ') {
                c
            } else {
                '_'
            }
        })
        .collect();
    fallback = fallback.trim_matches([' ', '.']).to_string();
    if fallback.is_empty() {
        fallback = "attachment".into();
    }
    let encoded: String = name
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    format!(
        "{}; filename=\"{fallback}\"; filename*=UTF-8''{encoded}",
        if image { "inline" } else { "attachment" }
    )
}

#[utoipa::path(get,path="/api/agents/{agent_id}/attachments/{attachment_id}",tag="attachments",
    params(("agent_id"=String,Path),("attachment_id"=String,Path)),
    responses((status=200,description="Validated bytes: PNG, JPEG, WebP, GIF inline; text/source/documents download-only. Canonical MIME, nosniff, no-store.",body=Vec<u8>,content_type="application/octet-stream"),(status=403,description="Owner authorization required"),(status=404,description="Attachment, agent or session not found"),(status=409,description="Workspace or file changed"),(status=503,description="Attachment history unavailable")))]
pub(super) async fn serve(
    State(state): State<AppState>,
    Path((agent_id, attachment_id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, true) {
        return response;
    }
    if !is_valid_session_id(&agent_id) || !valid_attachment_id(&attachment_id) {
        return rejected(ApiError::bad_request_static(ATTACHMENT_INVALID_ID));
    }
    let store = {
        let guard = state.daemon.read().await;
        if !guard.agents.contains_key(&agent_id) {
            return rejected(ApiError::not_found());
        }
        guard.history.store()
    };
    let record = match store.get_attachment(&agent_id, &attachment_id).await {
        Ok(Some(row)) => row,
        Ok(None) => return rejected(ApiError::not_found()),
        Err(_) => return rejected(ApiError::service_unavailable(ATTACHMENT_STORE_FAILED)),
    };
    let root = {
        let guard = state.daemon.read().await;
        if !guard.agents.contains_key(&agent_id)
            || guard.sessions.get(&agent_id, &record.session_id).is_none()
            || guard.pending_history_deletions.iter().any(|d| {
                d.agent_id == agent_id
                    && d.session_id
                        .as_ref()
                        .is_none_or(|session| session == &record.session_id)
            })
        {
            return rejected(ApiError::not_found());
        }
        match guard.workspace.as_ref() {
            Some(workspace) => workspace.root_path.clone(),
            None => return rejected(ApiError::conflict(ATTACHMENT_NEEDS_WORKSPACE)),
        }
    };
    let saved = record.clone();
    let bytes =
        match tokio::task::spawn_blocking(move || read_attachment_bytes(&root, &saved)).await {
            Ok(Ok(bytes)) => bytes,
            Ok(Err(_)) => return rejected(ApiError::conflict(ATTACHMENT_CHANGED)),
            Err(_) => return rejected(ApiError::service_unavailable(ATTACHMENT_CHANGED)),
        };
    let content_type = match HeaderValue::from_str(&record.mime) {
        Ok(value) => value,
        Err(_) => return rejected(ApiError::conflict(ATTACHMENT_CHANGED)),
    };
    let content_disposition = match HeaderValue::from_str(&disposition(
        &record.name,
        record.kind == AttachmentKind::Image,
    )) {
        Ok(value) => value,
        Err(_) => return rejected(ApiError::conflict(ATTACHMENT_CHANGED)),
    };
    let mut response = (StatusCode::OK, bytes).into_response();
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, content_type);
    headers.insert(header::CONTENT_DISPOSITION, content_disposition);
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    no_store(response)
}
