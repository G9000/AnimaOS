//! Foreign session routes: browse the Claude Code and Codex conversations on
//! this computer and import one into an agent as a chat. Every answer is
//! owner-authorized and `no-store`; logs are named only by handles.

use anima_core::primitives::now_millis;
use axum::extract::{Path, Request, State};
use axum::http::StatusCode;
use axum::response::Response;
use serde::Serialize;
use utoipa::ToSchema;

use super::contracts::ErrorBody;
use super::http::{json_response, request_query};
use super::jobs::{authorize, no_store};
use super::sessions::rejected;
use super::{ApiError, ApiErrorKind, AppState};
use crate::agent_runs::HELPER_MUST_RUN_THROUGH_COMPANION;
use crate::foreign_sessions::{
    self as foreign, Candidate, ForeignError, ForeignSource, ParsedSession, Roots,
};
use crate::history::HistoryMessage;
use crate::live::LiveEventBody;
use crate::sessions::{
    new_chat_session_id, SessionKind, SessionOrigin, SessionRecord, TitleSource,
};

const TOO_MANY_IMPORTS: &str = "Too many new chats; try again in a minute";
const MAX_CURSOR: usize = 1_000_000;
/// Messages per history store write.
const COLD_BATCH: usize = 500;

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ForeignSessionResponse {
    /// Hex SHA-256 of the log's resolved path; the only way to name a log.
    pub(crate) handle: String,
    /// `claude-code` or `codex`.
    pub(crate) source: String,
    pub(crate) title: String,
    /// The project folder the conversation ran in.
    pub(crate) cwd: Option<String>,
    /// The log's modification time, in Unix milliseconds.
    pub(crate) modified_at: u64,
    pub(crate) size_bytes: u64,
    /// The tool's own session id, when the log names one.
    pub(crate) foreign_session_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ForeignSessionsEnvelope {
    pub(crate) sessions: Vec<ForeignSessionResponse>,
    /// Pass as `cursor` for the next page; `null` on the last page.
    pub(crate) next_cursor: Option<String>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ForeignPreviewMessageResponse {
    /// `user` or `assistant`.
    pub(crate) role: String,
    /// Redacted, at most 2000 characters.
    pub(crate) text: String,
    /// When the log recorded the turn, in Unix milliseconds.
    pub(crate) at: Option<u64>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ForeignSessionPreviewResponse {
    pub(crate) title: String,
    pub(crate) cwd: Option<String>,
    pub(crate) source: String,
    /// Messages an import would create.
    pub(crate) message_count: usize,
    /// The newest 40 of them, oldest first.
    pub(crate) messages: Vec<ForeignPreviewMessageResponse>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ForeignSessionImportResponse {
    pub(crate) session_id: String,
    pub(crate) title: String,
    pub(crate) message_count: usize,
    /// The conversation was imported into this agent before; nothing new
    /// was created.
    pub(crate) already_imported: bool,
}

fn error(status: StatusCode, message: &str) -> Response {
    rejected(ApiError {
        kind: ApiErrorKind::General,
        status,
        message: message.into(),
    })
}

fn foreign_error(failure: ForeignError) -> Response {
    match failure {
        ForeignError::NotFound => rejected(ApiError::not_found()),
        ForeignError::TooLarge => error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "This conversation's log exceeds the 32 MiB import limit",
        ),
        ForeignError::Empty => error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "This conversation has no readable messages",
        ),
    }
}

/// Resolves a handle and reads its whole log off the async runtime.
async fn load(handle: String) -> Result<(Candidate, ParsedSession), Response> {
    tokio::task::spawn_blocking(move || {
        let candidate = foreign::resolve(&Roots::from_env(), &handle)?;
        let parsed = foreign::load(&candidate)?;
        Ok((candidate, parsed))
    })
    .await
    .map_err(|_| {
        rejected(ApiError::service_unavailable(
            "Could not read this conversation",
        ))
    })?
    .map_err(foreign_error)
}

#[utoipa::path(get, path = "/api/foreign-sessions", tag = "foreign-sessions",
    params(
        ("source" = Option<String>, Query, description = "claude-code or codex (default both)"),
        ("cursor" = Option<String>, Query, description = "nextCursor of the previous page")
    ),
    responses(
        (status = 200, description = "Claude Code and Codex conversations on this computer, newest first, 50 per page", body = ForeignSessionsEnvelope),
        (status = 400, description = "Invalid source or cursor", body = ErrorBody),
        (status = 403, description = "Local owner required", body = ErrorBody)
    ))]
pub(super) async fn list_foreign_sessions(
    State(state): State<AppState>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, true) {
        return response;
    }
    let Ok(params) = request_query(request.uri()) else {
        return rejected(ApiError::malformed_request());
    };
    let source = match params.get("source").map(String::as_str) {
        None | Some("") => None,
        Some(value) => match ForeignSource::parse(value) {
            Some(source) => Some(source),
            None => {
                return rejected(ApiError::bad_request_static(
                    "source must be claude-code or codex",
                ))
            }
        },
    };
    let offset = match params.get("cursor").map(String::as_str) {
        None | Some("") => 0,
        Some(value) => match value.parse::<usize>() {
            Ok(offset) if offset <= MAX_CURSOR => offset,
            _ => return rejected(ApiError::bad_request_static("invalid cursor")),
        },
    };
    let page = match tokio::task::spawn_blocking(move || {
        foreign::list_page(&Roots::from_env(), source, offset)
    })
    .await
    {
        Ok(page) => page,
        Err(_) => {
            return rejected(ApiError::service_unavailable(
                "Could not read the session folders",
            ))
        }
    };
    no_store(json_response(
        StatusCode::OK,
        &ForeignSessionsEnvelope {
            sessions: page
                .sessions
                .into_iter()
                .map(|session| ForeignSessionResponse {
                    handle: session.handle,
                    source: session.source.as_str().into(),
                    title: session.title,
                    cwd: session.cwd,
                    modified_at: session.modified_ms,
                    size_bytes: session.size_bytes,
                    foreign_session_id: session.foreign_session_id,
                })
                .collect(),
            next_cursor: page.next_offset.map(|offset| offset.to_string()),
        },
    ))
}

#[utoipa::path(get, path = "/api/foreign-sessions/{handle}/preview", tag = "foreign-sessions",
    params(("handle" = String, Path, description = "handle from the session list")),
    responses(
        (status = 200, description = "The newest 40 messages an import would create, redacted", body = ForeignSessionPreviewResponse),
        (status = 403, description = "Local owner required", body = ErrorBody),
        (status = 404, description = "Unknown handle", body = ErrorBody),
        (status = 413, description = "The log exceeds 32 MiB", body = ErrorBody),
        (status = 422, description = "The log holds no readable messages", body = ErrorBody)
    ))]
pub(super) async fn preview_foreign_session(
    State(state): State<AppState>,
    Path(handle): Path<String>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, true) {
        return response;
    }
    let (candidate, parsed) = match load(handle).await {
        Ok(loaded) => loaded,
        Err(response) => return response,
    };
    let prepared = foreign::prepare_messages(&parsed, now_millis());
    let newest = prepared.len().saturating_sub(foreign::PREVIEW_MESSAGES);
    no_store(json_response(
        StatusCode::OK,
        &ForeignSessionPreviewResponse {
            title: foreign::display_title(&parsed, candidate.source),
            cwd: foreign::provenance(&candidate, &parsed).cwd,
            source: candidate.source.as_str().into(),
            message_count: prepared.len(),
            messages: prepared[newest..]
                .iter()
                .map(|message| ForeignPreviewMessageResponse {
                    role: message.role.as_str().into(),
                    text: crate::sessions::truncate_chars(
                        &message.text,
                        foreign::PREVIEW_TEXT_CHARS,
                    ),
                    at: message.timed.then_some(message.at_ms),
                })
                .collect(),
        },
    ))
}

#[utoipa::path(post, path = "/api/agents/{agent_id}/foreign-sessions/{handle}/import", tag = "foreign-sessions",
    params(("agent_id" = String, Path), ("handle" = String, Path, description = "handle from the session list")),
    responses(
        (status = 200, description = "Imported into this agent before; the existing session", body = ForeignSessionImportResponse),
        (status = 201, description = "A new chat session holding the conversation", body = ForeignSessionImportResponse),
        (status = 403, description = "Local owner required", body = ErrorBody),
        (status = 404, description = "Unknown agent or handle", body = ErrorBody),
        (status = 409, description = "The agent is a helper; helpers run only through their companion", body = ErrorBody),
        (status = 413, description = "The log exceeds 32 MiB", body = ErrorBody),
        (status = 422, description = "The log holds no readable messages", body = ErrorBody),
        (status = 429, description = "More than 60 new sessions this minute", body = ErrorBody),
        (status = 503, description = "The history store or the control plane could not be saved", body = ErrorBody)
    ))]
pub(super) async fn import_foreign_session(
    State(state): State<AppState>,
    Path((agent_id, handle)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    if let Some(response) = agent_refusal(&*state.daemon.read().await, &agent_id) {
        return response;
    }
    let (candidate, parsed) = match load(handle).await {
        Ok(loaded) => loaded,
        Err(response) => return response,
    };
    let provenance = foreign::provenance(&candidate, &parsed);
    let title = foreign::import_title(&parsed);
    let now_ms = now_millis();
    let prepared = foreign::prepare_messages(&parsed, now_ms);

    // One import at a time decides "already imported" and saves.
    let transaction = state.agent_runs.control_plane_transaction().await;
    let (record, store, ephemeral) = {
        let mut guard = state.daemon.write().await;
        if let Some(response) = agent_refusal(&guard, &agent_id) {
            return response;
        }
        if let Some(existing) = guard.find_imported_session(&agent_id, &provenance) {
            let response = ForeignSessionImportResponse {
                session_id: existing.id.clone(),
                title: existing.title.clone(),
                message_count: prepared.len(),
                already_imported: true,
            };
            return no_store(json_response(StatusCode::OK, &response));
        }
        if !guard.session_limiter.try_acquire(&agent_id, now_ms) {
            return error(StatusCode::TOO_MANY_REQUESTS, TOO_MANY_IMPORTS);
        }
        let mut record = SessionRecord::new(
            &agent_id,
            &new_chat_session_id(),
            SessionKind::Chat,
            SessionOrigin::Import,
            title.clone(),
            TitleSource::System,
            now_ms,
        );
        record.last_read_at_ms = Some(now_ms);
        record.imported_from = Some(provenance);
        (record, guard.history.store(), guard.history.is_ephemeral())
    };
    let session_id = record.id.clone();
    let mut messages = foreign::transcript_messages(&agent_id, record.room_id(), &prepared, now_ms);
    let hot = messages.split_off(foreign::hot_start(&messages, ephemeral));
    let cold = messages;

    // Older messages go straight to the history store, as if already
    // pruned; the record saved below points at the newest of them.
    let cold_rows = cold
        .iter()
        .map(|message| HistoryMessage {
            agent_id: agent_id.clone(),
            session_id: session_id.clone(),
            hidden: false,
            message: message.clone(),
        })
        .collect::<Vec<_>>();
    for batch in cold_rows.chunks(COLD_BATCH) {
        if let Err(failure) = store.upsert_messages(batch).await {
            discard_cold_rows(&store, &agent_id, &session_id, !cold_rows.is_empty()).await;
            return rejected(ApiError::service_unavailable(failure.to_string()));
        }
    }

    let (undo, persist, history) = {
        let mut guard = state.daemon.write().await;
        let Some(undo) = guard.commit_imported_session(record, hot.clone(), cold.last()) else {
            drop(guard);
            discard_cold_rows(&store, &agent_id, &session_id, !cold_rows.is_empty()).await;
            return rejected(ApiError::not_found());
        };
        (
            undo,
            guard.control_plane_persist_request(),
            guard.history.clone(),
        )
    };
    if let Err(failure) = persist.save().await {
        state.daemon.write().await.revert_imported_session(undo);
        discard_cold_rows(&store, &agent_id, &session_id, !cold_rows.is_empty()).await;
        return rejected(ApiError::service_unavailable(failure.to_string()));
    }
    // Queued inside the transaction, as a run commit is, so a deletion of
    // the session always queues after it.
    history.enqueue_committed(&agent_id, &session_id, &hot);
    state.daemon.read().await.publish_session_event(
        &agent_id,
        &session_id,
        LiveEventBody::SessionCreated,
    );
    drop(transaction);
    no_store(json_response(
        StatusCode::CREATED,
        &ForeignSessionImportResponse {
            session_id,
            title,
            message_count: prepared.len(),
            already_imported: false,
        },
    ))
}

fn agent_refusal(state: &crate::state::DaemonState, agent_id: &str) -> Option<Response> {
    match state.agents.get(agent_id) {
        None => Some(rejected(ApiError::not_found())),
        Some(runtime) if crate::agent_runs::is_helper_config(runtime.config()) => Some(rejected(
            ApiError::conflict(HELPER_MUST_RUN_THROUGH_COMPANION),
        )),
        Some(_) => None,
    }
}

/// Removes the rows an import that did not stand wrote to the store.
async fn discard_cold_rows(
    store: &std::sync::Arc<dyn crate::history::HistoryStore>,
    agent_id: &str,
    session_id: &str,
    wrote: bool,
) {
    if wrote {
        if let Err(failure) = store.delete_session(agent_id, session_id).await {
            tracing::warn!(agent_id = %agent_id, session_id = %session_id, error = %failure, "could not remove the rows of an import that was not saved");
        }
    }
}
