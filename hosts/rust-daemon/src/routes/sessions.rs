//! Session routes (spec §3.3). Every answer is owner-authorized and `no-store`.

use std::collections::HashMap;

use anima_core::primitives::now_millis;
use axum::extract::{Path, Request, State};
use axum::http::{header, HeaderValue, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use tracing::warn;

use super::contracts::{
    DeleteResponse, ErrorBody, MainChatRequest, SessionCreateRequest, SessionEnvelope,
    SessionMessageResponse, SessionMessageSearchEnvelope, SessionMessageSearchHitResponse,
    SessionMessagesEnvelope, SessionResponse, SessionUpdateRequest, SessionsEnvelope,
};
use super::http::{json_response, read_limited_body, request_query};
use super::jobs::{authorize, body, no_store};
use super::{parse_json_body, ApiError, AppState};
use crate::agent_runs::HELPER_MUST_RUN_THROUGH_COMPANION;
use crate::history::HistoryDeletion;
use crate::live::{LiveEvent, LiveEventBody};
use crate::sessions::compaction::manual_compaction_input;
use crate::sessions::context::{conversation_context_usage, uncovered_pruned_through};
use crate::sessions::history_read::{self, HistoryReadError, MessageSearchRequest, WindowRequest};
use crate::sessions::views::{
    self, MessagePageError, MessagePageRequest, SessionCursor, SessionListQuery,
    DEFAULT_MESSAGE_PAGE, DEFAULT_SESSION_PAGE, MAX_MESSAGE_PAGE, MAX_SEARCH_QUERY_CHARS,
    MAX_SESSION_PAGE,
};
use crate::sessions::{
    clean_owner_title, is_valid_session_id, new_chat_session_id, SessionKind, SessionOrigin,
    SessionRecord, TitleSource, DEFAULT_CHAT_TITLE,
};

const TOO_MANY_NEW_CHATS: &str = "Too many new chats; try again in a minute";
const SESSION_RUN_IN_PROGRESS: &str = "A run in this session is still in progress";
const MAIN_CHAT_UNAVAILABLE: &str = "The main chat is unavailable; restore its history or explicitly reset the companion to recover";

type QueryParams = HashMap<String, String>;

pub(super) fn rejected(error: ApiError) -> Response {
    no_store(error.into_response())
}

fn params(uri: &Uri) -> Result<QueryParams, ApiError> {
    request_query(uri).map_err(|()| ApiError::bad_request_static("malformed query"))
}

fn flag(params: &QueryParams, name: &str, default: bool) -> Result<bool, ApiError> {
    match params.get(name).map(String::as_str) {
        None | Some("") => Ok(default),
        Some("true") => Ok(true),
        Some("false") => Ok(false),
        Some(_) => Err(ApiError::bad_request(format!(
            "{name} must be true or false"
        ))),
    }
}

fn limit(params: &QueryParams, default: usize, max: usize) -> Result<usize, ApiError> {
    match params.get("limit").map(String::as_str) {
        None | Some("") => Ok(default),
        Some(value) => value
            .parse::<usize>()
            .ok()
            .filter(|limit| (1..=max).contains(limit))
            .ok_or_else(|| ApiError::bad_request(format!("limit must be between 1 and {max}"))),
    }
}

fn list_query(uri: &Uri) -> Result<SessionListQuery, ApiError> {
    let params = params(uri)?;
    let kind = match params.get("kind").map(String::as_str) {
        None | Some("") => None,
        Some(value) => Some(SessionKind::parse(value).ok_or_else(|| {
            ApiError::bad_request_static(
                "kind must be one of chat, telegram, discord, email, checkin, job, helper",
            )
        })?),
    };
    let q = params
        .get("q")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    if q.as_ref()
        .is_some_and(|q| q.chars().count() > MAX_SEARCH_QUERY_CHARS)
    {
        return Err(ApiError::bad_request(format!(
            "q must be at most {MAX_SEARCH_QUERY_CHARS} characters"
        )));
    }
    let cursor = match params.get("cursor").map(String::as_str) {
        None | Some("") => None,
        Some(value) => Some(
            SessionCursor::decode(value)
                .ok_or_else(|| ApiError::bad_request_static("cursor is invalid"))?,
        ),
    };
    Ok(SessionListQuery {
        kind,
        archived: flag(&params, "archived", false)?,
        q,
        cursor,
        limit: limit(&params, DEFAULT_SESSION_PAGE, MAX_SESSION_PAGE)?,
        include_helpers: flag(&params, "includeHelpers", true)?,
    })
}

enum SessionMessageQuery {
    Page(MessagePageRequest),
    Anchor(WindowRequest),
}

fn message_query(uri: &Uri, session_id: &str) -> Result<SessionMessageQuery, ApiError> {
    let params = params(uri)?;
    if let Some(anchor) = params.get("anchor") {
        if params.contains_key("before") || flag(&params, "includeHidden", false)? {
            return Err(ApiError::bad_request_static(
                "anchor cannot be combined with before or includeHidden=true",
            ));
        }
        if anchor.trim().is_empty()
            || anchor.len() > 256
            || anchor.chars().any(char::is_control)
            || crate::memory_text::has_hidden_text(anchor)
        {
            return Err(ApiError::bad_request_static("anchor is invalid"));
        }
        let count = limit(&params, 20, 20)?;
        let before_count = (count - 1) / 2;
        return Ok(SessionMessageQuery::Anchor(WindowRequest {
            session_id: session_id.into(),
            anchor_message_id: anchor.clone(),
            before_count,
            after_count: count - before_count - 1,
        }));
    }
    Ok(SessionMessageQuery::Page(MessagePageRequest {
        before: params
            .get("before")
            .filter(|value| !value.is_empty())
            .cloned(),
        limit: limit(&params, DEFAULT_MESSAGE_PAGE, MAX_MESSAGE_PAGE)?,
        include_hidden: flag(&params, "includeHidden", false)?,
    }))
}

fn history_error(error: HistoryReadError) -> Response {
    rejected(match error {
        HistoryReadError::InvalidRequest => {
            ApiError::bad_request_static("history lookup arguments are invalid")
        }
        HistoryReadError::Unavailable => {
            ApiError::service_unavailable("history store is unavailable")
        }
        HistoryReadError::ScopeLimit => ApiError::service_unavailable(
            "history scope exceeds 500 sessions or 5000 hot messages; narrow the requested scope",
        ),
    })
}

#[utoipa::path(get, path = "/api/agents/{agent_id}/message-search", tag = "sessions",
    params(("agent_id" = String, Path), ("q" = String, Query, description = "Message prefix search, 1-200 characters"), ("limit" = Option<usize>, Query, description = "1-20, default 20")),
    responses(
        (status = 200, description = "Newest individual matching messages from allowed conversations", body = SessionMessageSearchEnvelope),
        (status = 400, description = "Invalid query", body = ErrorBody),
        (status = 403, description = "Local owner required", body = ErrorBody),
        (status = 404, description = "Agent not found", body = ErrorBody),
        (status = 503, description = "History store or bounded scope unavailable", body = ErrorBody)
    ))]
pub(super) async fn search_session_messages(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, true) {
        return response;
    }
    let query = match params(request.uri()).and_then(|params| {
        let query = params
            .get("q")
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| ApiError::bad_request_static("q must contain 1-200 characters"))?
            .to_string();
        Ok(MessageSearchRequest {
            query,
            limit: limit(&params, 20, 20)?,
        })
    }) {
        Ok(query) => query,
        Err(error) => return rejected(error),
    };
    match history_read::search_messages(&state.daemon, &agent_id, query).await {
        Ok(Some(page)) => no_store(json_response(
            StatusCode::OK,
            &SessionMessageSearchEnvelope {
                results: page
                    .results
                    .into_iter()
                    .map(|hit| SessionMessageSearchHitResponse {
                        session_id: hit.session_id,
                        title: hit.title,
                        message_id: hit.message.id,
                        role: crate::history::role_name(hit.message.role).into(),
                        created_at_ms: hit.message.created_at_ms,
                        snippet: hit.snippet,
                    })
                    .collect(),
                has_more: page.has_more,
            },
        )),
        Ok(None) => rejected(ApiError::not_found()),
        Err(error) => history_error(error),
    }
}

#[utoipa::path(get, path = "/api/agents/{agent_id}/sessions", tag = "sessions",
    params(
        ("agent_id" = String, Path),
        ("kind" = Option<String>, Query, description = "chat, telegram, discord, email, checkin, job, or helper"),
        ("archived" = Option<bool>, Query, description = "true lists only archived sessions (default false)"),
        ("q" = Option<String>, Query, description = "Search titles and message text (up to 200 characters)"),
        ("cursor" = Option<String>, Query, description = "nextCursor of the previous page"),
        ("limit" = Option<usize>, Query, description = "1-200, default 50"),
        ("includeHelpers" = Option<bool>, Query, description = "Include helper and delegated sessions whose parentAgentId is this agent (default true)")
    ),
    responses(
        (status = 200, description = "Sessions, newest activity first", body = SessionsEnvelope),
        (status = 400, description = "Invalid query", body = ErrorBody),
        (status = 403, description = "Local owner required", body = ErrorBody),
        (status = 404, description = "Agent not found", body = ErrorBody)
    ))]
pub(super) async fn list_sessions(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, true) {
        return response;
    }
    let query = match list_query(request.uri()) {
        Ok(query) => query,
        Err(error) => return rejected(error),
    };
    match views::list_sessions(&state.daemon, &agent_id, &query).await {
        Some(page) => no_store(json_response(
            StatusCode::OK,
            &SessionsEnvelope::from(&page),
        )),
        None => rejected(ApiError::not_found()),
    }
}

#[utoipa::path(get, path = "/api/agents/{agent_id}/sessions/{session_id}", tag = "sessions",
    params(("agent_id" = String, Path), ("session_id" = String, Path, description = "Percent-encoded session id")),
    responses(
        (status = 200, description = "One session", body = SessionEnvelope),
        (status = 403, description = "Local owner required", body = ErrorBody),
        (status = 404, description = "Agent or session not found", body = ErrorBody)
    ))]
pub(super) async fn get_session(
    State(state): State<AppState>,
    Path((agent_id, session_id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, true) {
        return response;
    }
    if !is_valid_session_id(&session_id) {
        return rejected(ApiError::not_found());
    }
    match views::session_view(&state.daemon, &agent_id, &session_id).await {
        Some(view) => {
            let mut session = SessionResponse::from(&view);
            session.usage =
                Some(super::usage::session_totals(&state, &agent_id, &session_id).await);
            {
                let guard = state.daemon.read().await;
                session.context_usage = guard
                    .agents
                    .get(&agent_id)
                    .zip(guard.sessions.get(&agent_id, &session_id))
                    .map(|(runtime, record)| {
                        conversation_context_usage(
                            runtime.config(),
                            record,
                            &guard.model_visible_history(&agent_id, record.room_id()),
                        )
                        .into()
                    });
            }
            no_store(json_response(StatusCode::OK, &SessionEnvelope { session }))
        }
        None => rejected(ApiError::not_found()),
    }
}

#[utoipa::path(get, path = "/api/agents/{agent_id}/sessions/{session_id}/messages", tag = "sessions",
    params(
        ("agent_id" = String, Path),
        ("session_id" = String, Path, description = "Percent-encoded session id"),
        ("before" = Option<String>, Query, description = "Only messages older than this message id"),
        ("anchor" = Option<String>, Query, description = "Exact visible owned message for a bounded surrounding window; excludes before/includeHidden=true"),
        ("limit" = Option<usize>, Query, description = "1-200, default 50; anchor mode 1-20, default 20"),
        ("includeHidden" = Option<bool>, Query, description = "Include silent check-in turns (default false)")
    ),
    responses(
        (status = 200, description = "Messages oldest to newest within the page", body = SessionMessagesEnvelope),
        (status = 400, description = "Invalid query or unknown before message", body = ErrorBody),
        (status = 403, description = "Local owner required", body = ErrorBody),
        (status = 404, description = "Agent or session not found", body = ErrorBody),
        (status = 503, description = "The page needs the history store and it cannot be read", body = ErrorBody)
    ))]
pub(super) async fn list_session_messages(
    State(state): State<AppState>,
    Path((agent_id, session_id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, true) {
        return response;
    }
    if !is_valid_session_id(&session_id) {
        return rejected(ApiError::not_found());
    }
    let query = match message_query(request.uri(), &session_id) {
        Ok(query) => query,
        Err(error) => return rejected(error),
    };
    let query = match query {
        SessionMessageQuery::Page(query) => query,
        SessionMessageQuery::Anchor(query) => {
            return match history_read::select_conversation_window(&state.daemon, &agent_id, query)
                .await
            {
                Ok(Some(window)) => {
                    let next_before = window
                        .has_earlier
                        .then(|| window.messages.first().map(|message| message.id.clone()))
                        .flatten();
                    let page = SessionMessagesEnvelope {
                        messages: window
                            .messages
                            .into_iter()
                            .map(|message| {
                                SessionMessageResponse::from(&views::PageMessage {
                                    message,
                                    hidden: false,
                                })
                            })
                            .collect(),
                        next_before,
                        anchor_message_id: Some(window.anchor_message_id),
                        has_newer: Some(window.has_later),
                    };
                    no_store(json_response(StatusCode::OK, &page))
                }
                Ok(None) => rejected(ApiError::not_found()),
                Err(error) => history_error(error),
            }
        }
    };
    match views::session_messages(&state.daemon, &agent_id, &session_id, &query).await {
        Ok(page) => no_store(json_response(
            StatusCode::OK,
            &SessionMessagesEnvelope::from(&page),
        )),
        Err(MessagePageError::NotFound) => rejected(ApiError::not_found()),
        Err(MessagePageError::BeforeNotFound) => {
            rejected(ApiError::bad_request_static("before message was not found"))
        }
        Err(MessagePageError::Unavailable) => rejected(ApiError::service_unavailable(
            "history store is unavailable",
        )),
    }
}

async fn session_response(
    state: &AppState,
    agent_id: &str,
    session_id: &str,
    status: StatusCode,
) -> Response {
    match views::session_view(&state.daemon, agent_id, session_id).await {
        Some(view) => no_store(json_response(
            status,
            &SessionEnvelope {
                session: SessionResponse::from(&view),
            },
        )),
        None => rejected(ApiError::not_found()),
    }
}

#[utoipa::path(post, path = "/api/agents/{agent_id}/sessions", tag = "sessions",
    params(("agent_id" = String, Path)), request_body = SessionCreateRequest,
    responses(
        (status = 201, description = "A new chat session", body = SessionEnvelope),
        (status = 400, description = "Invalid title", body = ErrorBody),
        (status = 403, description = "Local owner required", body = ErrorBody),
        (status = 404, description = "Agent not found", body = ErrorBody),
        (status = 409, description = "The agent is a helper; helpers run only through their companion", body = ErrorBody),
        (status = 429, description = "More than 60 new sessions this minute", body = ErrorBody),
        (status = 503, description = "The control plane could not be saved", body = ErrorBody)
    ))]
pub(super) async fn create_session(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let bytes = match read_limited_body(request, state.config.max_request_bytes).await {
        Ok(bytes) => bytes,
        Err(response) => return no_store(response),
    };
    let input = if bytes.iter().all(u8::is_ascii_whitespace) {
        SessionCreateRequest::default()
    } else {
        match parse_json_body::<SessionCreateRequest>(bytes) {
            Ok(input) => input,
            Err(error) => return rejected(error),
        }
    };
    let owner_title = match input.title.as_deref().map(clean_owner_title).transpose() {
        Ok(title) => title,
        Err(message) => return rejected(ApiError::bad_request_static(message)),
    };
    let transaction = state.agent_runs.control_plane_transaction().await;
    let (session_id, persist) = {
        let mut guard = state.daemon.write().await;
        match guard.agents.get(&agent_id) {
            None => return rejected(ApiError::not_found()),
            // Controller ruling 1 (M2 pre-flight audit): helpers run only
            // through the companion that delegates to them.
            Some(runtime) if crate::agent_runs::is_helper_config(runtime.config()) => {
                return rejected(ApiError::conflict(HELPER_MUST_RUN_THROUGH_COMPANION));
            }
            Some(_) => {}
        }
        let now_ms = now_millis();
        if !guard.session_limiter.try_acquire(&agent_id, now_ms) {
            return rejected(ApiError {
                kind: crate::routes::ApiErrorKind::General,
                status: StatusCode::TOO_MANY_REQUESTS,
                message: TOO_MANY_NEW_CHATS.into(),
            });
        }
        let (title, title_source) = match owner_title {
            Some(title) => (title, TitleSource::Owner),
            None => (DEFAULT_CHAT_TITLE.to_string(), TitleSource::FirstMessage),
        };
        let record = SessionRecord::new(
            &agent_id,
            &new_chat_session_id(),
            SessionKind::Chat,
            SessionOrigin::Web,
            title,
            title_source,
            now_ms,
        );
        let session_id = record.id.clone();
        guard.sessions.insert(record);
        (session_id, guard.control_plane_persist_request())
    };
    if let Err(error) = persist.save().await {
        state
            .daemon
            .write()
            .await
            .sessions
            .remove(&agent_id, &session_id);
        return rejected(ApiError::service_unavailable(error.to_string()));
    }
    state.daemon.read().await.publish_session_event(
        &agent_id,
        &session_id,
        LiveEventBody::SessionCreated,
    );
    drop(transaction);
    session_response(&state, &agent_id, &session_id, StatusCode::CREATED).await
}

fn history_deletion_pending(state: &crate::state::DaemonState, record: &SessionRecord) -> bool {
    state.pending_history_deletions.iter().any(|deletion| {
        deletion.agent_id == record.agent_id
            && deletion
                .session_id
                .as_deref()
                .is_none_or(|id| id == record.id)
    })
}

#[utoipa::path(post, path = "/api/agents/{agent_id}/main-chat", tag = "sessions",
    params(("agent_id" = String, Path)), request_body = MainChatRequest,
    responses(
        (status = 200, description = "The existing or adopted main chat", body = SessionEnvelope),
        (status = 201, description = "A newly created main chat", body = SessionEnvelope),
        (status = 400, description = "Only an empty object or body is accepted", body = ErrorBody),
        (status = 403, description = "Local owner required", body = ErrorBody),
        (status = 404, description = "Agent not found", body = ErrorBody),
        (status = 409, description = "The agent is a helper or the designated main chat needs recovery", body = ErrorBody),
        (status = 429, description = "More than 60 new sessions this minute", body = ErrorBody),
        (status = 503, description = "The control plane could not be saved", body = ErrorBody)
    ))]
pub(super) async fn main_chat(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let bytes = match read_limited_body(request, state.config.max_request_bytes).await {
        Ok(bytes) => bytes,
        Err(response) => return no_store(response),
    };
    if !bytes.iter().all(u8::is_ascii_whitespace) {
        if bytes
            .iter()
            .find(|byte| !byte.is_ascii_whitespace())
            .copied()
            != Some(b'{')
        {
            return rejected(ApiError::bad_request_static(
                "main chat request must be an empty object",
            ));
        }
        if let Err(error) = parse_json_body::<MainChatRequest>(bytes) {
            return rejected(error);
        }
    }
    let transaction = state.agent_runs.control_plane_transaction().await;
    let (session_id, previous, created, persist) = {
        let mut guard = state.daemon.write().await;
        match guard.agents.get(&agent_id) {
            None => return rejected(ApiError::not_found()),
            Some(runtime) if crate::agent_runs::is_helper_config(runtime.config()) => {
                return rejected(ApiError::conflict(HELPER_MUST_RUN_THROUGH_COMPANION));
            }
            Some(_) => {}
        }
        let existing = {
            let mut designated = guard
                .sessions
                .records()
                .filter(|record| record.agent_id == agent_id && record.main_chat);
            let existing = designated.next().cloned();
            if designated.next().is_some() {
                return rejected(ApiError::conflict(MAIN_CHAT_UNAVAILABLE));
            }
            existing
        };
        if let Some(record) = existing {
            if !record.has_main_chat_identity()
                || record.archived
                || history_deletion_pending(&guard, &record)
            {
                return rejected(ApiError::conflict(MAIN_CHAT_UNAVAILABLE));
            }
            (record.id, None, false, None)
        } else if let Some(previous) = guard
            .sessions
            .main_chat_candidate(&agent_id, |record| history_deletion_pending(&guard, record))
            .cloned()
        {
            let mut record = previous.clone();
            record.main_chat = true;
            let id = record.id.clone();
            guard.sessions.insert(record);
            (
                id,
                Some(previous),
                false,
                Some(guard.control_plane_persist_request()),
            )
        } else {
            let now_ms = now_millis();
            if !guard.session_limiter.try_acquire(&agent_id, now_ms) {
                return rejected(ApiError {
                    kind: crate::routes::ApiErrorKind::General,
                    status: StatusCode::TOO_MANY_REQUESTS,
                    message: TOO_MANY_NEW_CHATS.into(),
                });
            }
            let mut record = SessionRecord::new(
                &agent_id,
                &new_chat_session_id(),
                SessionKind::Chat,
                SessionOrigin::Web,
                DEFAULT_CHAT_TITLE.to_string(),
                TitleSource::FirstMessage,
                now_ms,
            );
            record.main_chat = true;
            let id = record.id.clone();
            guard.sessions.insert(record);
            (id, None, true, Some(guard.control_plane_persist_request()))
        }
    };
    if let Some(persist) = persist {
        if let Err(error) = persist.save().await {
            let mut guard = state.daemon.write().await;
            if let Some(previous) = previous {
                guard.sessions.insert(previous);
            } else {
                guard.sessions.remove(&agent_id, &session_id);
            }
            return rejected(ApiError::service_unavailable(error.to_string()));
        }
        state.daemon.read().await.publish_session_event(
            &agent_id,
            &session_id,
            if created {
                LiveEventBody::SessionCreated
            } else {
                LiveEventBody::SessionUpdated
            },
        );
    }
    drop(transaction);
    session_response(
        &state,
        &agent_id,
        &session_id,
        if created {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        },
    )
    .await
}

#[utoipa::path(patch, path = "/api/agents/{agent_id}/sessions/{session_id}", tag = "sessions",
    params(("agent_id" = String, Path), ("session_id" = String, Path, description = "Percent-encoded session id")),
    request_body = SessionUpdateRequest,
    responses(
        (status = 200, description = "The updated session", body = SessionEnvelope),
        (status = 400, description = "Invalid or empty update", body = ErrorBody),
        (status = 403, description = "Local owner required", body = ErrorBody),
        (status = 404, description = "Agent or session not found", body = ErrorBody),
        (status = 409, description = "This session cannot be renamed", body = ErrorBody),
        (status = 503, description = "The control plane could not be saved", body = ErrorBody)
    ))]
pub(super) async fn update_session(
    State(state): State<AppState>,
    Path((agent_id, session_id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    if !is_valid_session_id(&session_id) {
        return rejected(ApiError::not_found());
    }
    let input: SessionUpdateRequest = match body(&state, request).await {
        Ok(input) => input,
        Err(response) => return response,
    };
    if input.title.is_none() && input.archived.is_none() && input.last_read_at_ms.is_none() {
        return rejected(ApiError::bad_request_static(
            "at least one of title, archived, or lastReadAtMs is required",
        ));
    }
    let title = match input.title.as_deref().map(clean_owner_title).transpose() {
        Ok(title) => title,
        Err(message) => return rejected(ApiError::bad_request_static(message)),
    };
    if input
        .last_read_at_ms
        .is_some_and(|read_at| read_at > now_millis())
    {
        return rejected(ApiError::bad_request_static(
            "lastReadAtMs must not be in the future",
        ));
    }
    let transaction = state.agent_runs.control_plane_transaction().await;
    let (previous, persist) = {
        let mut guard = state.daemon.write().await;
        if !guard.agents.contains_key(&agent_id) {
            return rejected(ApiError::not_found());
        }
        let schedule_exists = guard
            .sessions
            .get(&agent_id, &session_id)
            .is_some_and(|record| views::automation_exists(&guard, record));
        let Some(record) = guard.sessions.get_mut(&agent_id, &session_id) else {
            return rejected(ApiError::not_found());
        };
        if title.is_some() && !record.capabilities(schedule_exists).rename {
            return rejected(ApiError::conflict("This session cannot be renamed"));
        }
        if input.archived.is_some() && !record.capabilities(schedule_exists).archive {
            return rejected(ApiError::conflict("This session cannot be archived"));
        }
        let previous = record.clone();
        if let Some(title) = title {
            record.title = title;
            record.title_source = TitleSource::Owner;
        }
        if let Some(archived) = input.archived {
            record.archived = archived;
        }
        if let Some(read_at) = input.last_read_at_ms {
            record.last_read_at_ms = Some(read_at);
        }
        (previous, guard.control_plane_persist_request())
    };
    if let Err(error) = persist.save().await {
        state.daemon.write().await.sessions.insert(previous);
        return rejected(ApiError::service_unavailable(error.to_string()));
    }
    state.daemon.read().await.publish_session_event(
        &agent_id,
        &session_id,
        LiveEventBody::SessionUpdated,
    );
    drop(transaction);
    session_response(&state, &agent_id, &session_id, StatusCode::OK).await
}

#[utoipa::path(delete, path = "/api/agents/{agent_id}/sessions/{session_id}", tag = "sessions",
    params(("agent_id" = String, Path), ("session_id" = String, Path, description = "Percent-encoded session id")),
    responses(
        (status = 200, description = "Deleted: the record, hot and mirrored messages, and attachment records; memories are kept", body = DeleteResponse),
        (status = 403, description = "Local owner required", body = ErrorBody),
        (status = 404, description = "Agent or session not found", body = ErrorBody),
        (status = 409, description = "The kind does not allow deletion, or a run in the session is active", body = ErrorBody),
        (status = 503, description = "The control plane could not be saved", body = ErrorBody)
    ))]
pub(super) async fn delete_session(
    State(state): State<AppState>,
    Path((agent_id, session_id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    if !is_valid_session_id(&session_id) {
        return rejected(ApiError::not_found());
    }
    let room_id = {
        let guard = state.daemon.read().await;
        let record = match guard.sessions.get(&agent_id, &session_id) {
            Some(record) if guard.agents.contains_key(&agent_id) => record,
            _ => return rejected(ApiError::not_found()),
        };
        if !record
            .capabilities(views::automation_exists(&guard, record))
            .delete
        {
            return rejected(ApiError::conflict("This session cannot be deleted"));
        }
        record.room_id().to_string()
    };
    // Runs for this room wait behind the reservation, so none starts meanwhile.
    let Some(reservation) = state.agent_runs.try_reserve_room(&agent_id, &room_id) else {
        return rejected(ApiError::conflict(SESSION_RUN_IN_PROGRESS));
    };
    let transaction = state.agent_runs.control_plane_transaction().await;
    // Recorded in the same save as the removal below (Controller ruling 2, M2
    // pre-flight audit; Task 6's durable-deletion protocol): a crash right
    // after a successful save still replays this deletion on restart.
    let deletion = HistoryDeletion::session(&agent_id, &session_id);
    let (previous_agent, record, removed_runs, removed_ids, persist) = {
        let mut guard = state.daemon.write().await;
        // Re-checked under the transaction, not just in the fast-path check
        // above (fix round 1, M2 review): a concurrent change -- for example
        // a check-in's schedule being deleted and then restored -- must not
        // let a delete proceed, or be refused, on stale capabilities.
        let Some(record) = guard.sessions.get(&agent_id, &session_id) else {
            return rejected(ApiError::not_found());
        };
        let schedule_exists = views::automation_exists(&guard, record);
        if !record.capabilities(schedule_exists).delete {
            return rejected(ApiError::conflict("This session cannot be deleted"));
        }
        if guard.runs.active_count_for_session(&agent_id, &session_id) > 0 {
            return rejected(ApiError::conflict(SESSION_RUN_IN_PROGRESS));
        }
        let Some(runtime) = guard.agents.get_mut(&agent_id) else {
            return rejected(ApiError::not_found());
        };
        let previous_agent = runtime.snapshot();
        let removed_ids = runtime
            .retain_messages(|message| message.room_id != room_id)
            .into_iter()
            .map(|message| message.id)
            .collect::<Vec<_>>();
        let Some(record) = guard.sessions.remove(&agent_id, &session_id) else {
            if let Err(error) = guard.restore_removed_agent(previous_agent) {
                warn!(agent_id = %agent_id, error = %error, "could not restore an agent after a session vanished");
            }
            return rejected(ApiError::not_found());
        };
        let removed_runs = guard
            .runs
            .remove_terminal_for_session(&agent_id, &session_id);
        guard.record_history_deletion(deletion.clone());
        (
            previous_agent,
            record,
            removed_runs,
            removed_ids,
            guard.control_plane_persist_request(),
        )
    };
    if let Err(error) = persist.save().await {
        let mut guard = state.daemon.write().await;
        if let Err(restore_error) = guard.restore_removed_agent(previous_agent) {
            warn!(agent_id = %agent_id, error = %restore_error, "could not restore an agent after a failed session delete");
        }
        guard.sessions.insert(record);
        for run in removed_runs {
            guard.runs.insert(run);
        }
        guard.clear_history_deletion(&deletion);
        return rejected(ApiError::service_unavailable(error.to_string()));
    }
    // Durable now: the history rows may go (spec §3.3).
    let history = {
        let guard = state.daemon.read().await;
        // The record is gone, so its parent comes from the removed copy.
        let parent = guard
            .live_parent_agent(&agent_id, &session_id)
            .or(record.parent_agent_id);
        guard.live.publish(
            LiveEvent::new(&agent_id, LiveEventBody::SessionDeleted).session(&session_id),
            parent.as_deref(),
        );
        guard.history.clone()
    };
    history.enqueue_session_deletion(&agent_id, &session_id);
    history.forget_mirrored(removed_ids.iter().map(String::as_str));
    drop(transaction);
    drop(reservation);
    no_store(json_response(
        StatusCode::OK,
        &DeleteResponse { deleted: true },
    ))
}

#[utoipa::path(post, path = "/api/agents/{agent_id}/sessions/{session_id}/clear", tag = "sessions",
    params(("agent_id" = String, Path), ("session_id" = String, Path)),
    responses((status = 200, description = "Main conversation cleared; companion and saved memories kept", body = SessionEnvelope),
        (status = 403, description = "Owner required", body = ErrorBody),
        (status = 409, description = "Not the main chat or a reply is active", body = ErrorBody),
        (status = 503, description = "Persistence or history cleanup unavailable", body = ErrorBody)))]
pub(super) async fn clear_session(
    State(state): State<AppState>,
    Path((agent_id, session_id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    if let Err(error) = state
        .agent_runs
        .clear_main_chat(&agent_id, &session_id)
        .await
    {
        return rejected(error);
    }
    session_response(&state, &agent_id, &session_id, StatusCode::OK).await
}

#[utoipa::path(post, path = "/api/agents/{agent_id}/sessions/{session_id}/compact", tag = "sessions",
    params(("agent_id" = String, Path), ("session_id" = String, Path, description = "Percent-encoded session id")),
    responses(
        (status = 202, description = "Compaction started: every uncovered turn but the newest, and any pruned turns the summary does not cover, is folded into the summary, which arrives with session.updated", body = SessionEnvelope),
        (status = 403, description = "Local owner required", body = ErrorBody),
        (status = 404, description = "Agent or session not found", body = ErrorBody),
        (status = 409, description = "The kind cannot be compacted, a run is active, or there is nothing to compact yet", body = ErrorBody)
    ))]
pub(super) async fn compact_session(
    State(state): State<AppState>,
    Path((agent_id, session_id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    if !is_valid_session_id(&session_id) {
        return rejected(ApiError::not_found());
    }
    let room_id = {
        let guard = state.daemon.read().await;
        let record = match guard.sessions.get(&agent_id, &session_id) {
            Some(record) if guard.agents.contains_key(&agent_id) => record,
            _ => return rejected(ApiError::not_found()),
        };
        if !record
            .capabilities(views::automation_exists(&guard, record))
            .compact
        {
            return rejected(ApiError::conflict("This session cannot be compacted"));
        }
        record.room_id().to_string()
    };
    // No run starts in the room until the summary is saved. The input is
    // read only once the room is held (Task 12 fix round 1), so a summary a
    // run saved just before is never folded in again.
    let Some(reservation) = state.agent_runs.try_reserve_room(&agent_id, &room_id) else {
        return rejected(ApiError::conflict(SESSION_RUN_IN_PROGRESS));
    };
    let input = {
        let guard = state.daemon.read().await;
        let Some(record) = guard.sessions.get(&agent_id, &session_id) else {
            return rejected(ApiError::not_found());
        };
        let history = guard.model_visible_history(&agent_id, &room_id);
        let input = manual_compaction_input(&history, record.summary.as_ref());
        // Pruned turns the summary does not cover are compacted too
        // (controller ruling 2, Task 12).
        if input.is_empty() && uncovered_pruned_through(record, &history).is_none() {
            return rejected(ApiError::conflict("Nothing to compact yet"));
        }
        input
    };
    let runs = state.agent_runs.clone();
    let (agent, session) = (agent_id.clone(), session_id.clone());
    tokio::spawn(async move {
        let _reservation = reservation;
        if let Err(error) = runs.compact_session(&agent, &session, &input).await {
            warn!(agent_id = %agent, session_id = %session, error = %error, "session compaction failed");
        }
    });
    session_response(&state, &agent_id, &session_id, StatusCode::ACCEPTED).await
}

#[utoipa::path(get, path = "/api/agents/{agent_id}/sessions/{session_id}/export", tag = "sessions",
    params(("agent_id" = String, Path), ("session_id" = String, Path, description = "Percent-encoded session id")),
    responses(
        (status = 200, description = "The full transcript, including messages kept only in the history store, with silent check-in turns marked rather than hidden", body = String, content_type = "text/markdown"),
        (status = 403, description = "Local owner required", body = ErrorBody),
        (status = 404, description = "Agent or session not found", body = ErrorBody),
        (status = 503, description = "The history store cannot be read", body = ErrorBody)
    ))]
pub(super) async fn export_session(
    State(state): State<AppState>,
    Path((agent_id, session_id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, true) {
        return response;
    }
    if !is_valid_session_id(&session_id) {
        return rejected(ApiError::not_found());
    }
    match views::full_transcript(&state.daemon, &agent_id, &session_id).await {
        Ok((record, agent_name, messages)) => {
            let markdown = views::transcript_markdown(&record, &agent_name, &messages);
            let mut response = (StatusCode::OK, markdown).into_response();
            let headers = response.headers_mut();
            headers.insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/markdown; charset=utf-8"),
            );
            if let Ok(disposition) = HeaderValue::from_str(&format!(
                "attachment; filename=\"{}.md\"",
                views::export_file_stem(&record.title)
            )) {
                headers.insert(header::CONTENT_DISPOSITION, disposition);
            }
            no_store(response)
        }
        Err(MessagePageError::Unavailable) => rejected(ApiError::service_unavailable(
            "history store is unavailable",
        )),
        Err(MessagePageError::NotFound | MessagePageError::BeforeNotFound) => {
            rejected(ApiError::not_found())
        }
    }
}
