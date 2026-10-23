//! Owner grant administration and a separate, text-only viewer read surface.
use super::{
    jobs::{authorize, body, no_store},
    json_response,
    sessions::rejected,
    ApiError, AppState,
};
use crate::conversation_shares::{
    self, ConversationShareView, SharedMessage, VIEWER_ID_HEADER, VIEWER_SECRET_HEADER,
};
use crate::sessions::{
    views::{self, MessagePageError, MessagePageRequest},
    SessionRecord,
};
use axum::{
    extract::{Path, Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[cfg(test)]
#[path = "conversation_shares/tests.rs"]
mod tests;

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CreateShareRequest {
    viewer_name: String,
    /// Required owner-observed boundary; omissions must not grant a later chat.
    clear_generation: u64,
}

#[derive(Serialize, ToSchema)]
pub(super) struct SharesEnvelope {
    grants: Vec<ConversationShareView>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct CreatedShare {
    grant: ConversationShareView,
    /// Shown once, separately from the non-secret share link.
    secret: String,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct SharedConversation {
    id: String,
    title: String,
    viewer_name: String,
    read_only: bool,
    attachments_omitted: bool,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct SharedMessages {
    title: String,
    messages: Vec<SharedMessage>,
    next_before: Option<String>,
    attachments_omitted: bool,
}

/// Only these exact GET route shapes bypass the optional owner API key gate.
pub(super) fn is_shared_read_request(request: &Request) -> bool {
    if request.method() != axum::http::Method::GET {
        return false;
    }
    let pieces = request.uri().path().split('/').collect::<Vec<_>>();
    matches!(pieces.as_slice(), ["", "api", "shared", "conversations", id] if valid_id(id))
        || matches!(pieces.as_slice(), ["", "api", "shared", "conversations", id, "messages"] if valid_id(id))
}

fn valid_id(id: &str) -> bool {
    id.strip_prefix("share-")
        .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok())
}

/// Apply outside the owner API key middleware: an approved Origin cannot
/// turn a viewer credential into owner authority on any other route.
pub(super) async fn reject_viewer_credentials(request: Request, next: Next) -> Response {
    if (request.headers().contains_key(VIEWER_ID_HEADER)
        || request.headers().contains_key(VIEWER_SECRET_HEADER))
        && !is_shared_read_request(&request)
    {
        return no_store((StatusCode::FORBIDDEN, axum::Json(serde_json::json!({"error":"Viewer credentials grant read-only access to the selected shared conversation"}))).into_response());
    }
    next.run(request).await
}

fn unavailable(session: &SessionRecord, state: &crate::state::DaemonState) -> bool {
    !state.agents.contains_key(&session.agent_id)
        || state.pending_history_deletions.iter().any(|d| {
            d.agent_id == session.agent_id
                && d.session_id.as_ref().is_none_or(|id| id == &session.id)
        })
}

#[utoipa::path(get, path="/api/agents/{agent_id}/sessions/{session_id}/shares", tag="sessions", params(("agent_id"=String,Path),("session_id"=String,Path)), responses((status=200,description="Viewer grants without secrets or digests",body=SharesEnvelope),(status=403,description="Local owner required")))]
pub(super) async fn list(
    State(state): State<AppState>,
    Path((agent, session)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(error) = authorize(&state, &request, true) {
        return error;
    }
    let guard = state.daemon.read().await;
    let Some(record) = guard
        .sessions
        .get(&agent, &session)
        .filter(|s| !unavailable(s, &guard))
    else {
        return rejected(ApiError::not_found());
    };
    no_store(json_response(
        StatusCode::OK,
        &SharesEnvelope {
            grants: record
                .conversation_shares
                .iter()
                .map(|g| g.view(record))
                .collect(),
        },
    ))
}

#[utoipa::path(post, path="/api/agents/{agent_id}/sessions/{session_id}/shares", tag="sessions", params(("agent_id"=String,Path),("session_id"=String,Path)), request_body=CreateShareRequest, responses((status=201,description="Persisted viewer grant; secret returned once",body=CreatedShare),(status=400,description="Invalid viewer name or missing expected clearGeneration"),(status=403,description="Local owner required"),(status=409,description="Conversation cleared, unavailable chat or grant limit"),(status=503,description="Grant not saved")))]
pub(super) async fn create(
    State(state): State<AppState>,
    Path((agent, session)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(error) = authorize(&state, &request, false) {
        return error;
    }
    if !state.sharing_owner_auth_configured {
        return owner_auth_required();
    }
    let input: CreateShareRequest = match body(&state, request).await {
        Ok(v) => v,
        Err(e) => return e,
    };
    let mutation = tokio::spawn(async move {
        let _transaction = state.agent_runs.control_plane_transaction().await;
        let (previous, created, persist) = {
            let mut guard = state.daemon.write().await;
            let Some(record) = guard
                .sessions
                .get(&agent, &session)
                .filter(|s| !unavailable(s, &guard))
            else {
                return Err(ApiError::not_found());
            };
            if input.clear_generation != record.clear_generation {
                return Err(ApiError::conflict(
                    "The conversation was cleared; reopen sharing before granting viewer access",
                ));
            }
            let (grant, secret) = conversation_shares::issue(
                record,
                &input.viewer_name,
                anima_core::primitives::now_millis(),
            )
            .map_err(ApiError::bad_request_static)?;
            let created = CreatedShare {
                grant: grant.view(record),
                secret,
            };
            let previous = record.clone();
            guard
                .sessions
                .get_mut(&agent, &session)
                .unwrap()
                .conversation_shares
                .push(grant);
            (previous, created, guard.control_plane_persist_request())
        };
        if persist.store_config().is_none() {
            state.daemon.write().await.sessions.insert(previous);
            return Err(ApiError::conflict(
                "Configure durable storage before sharing a conversation",
            ));
        }
        if persist.save().await.is_err() {
            state.daemon.write().await.sessions.insert(previous);
            return Err(ApiError::service_unavailable(
                "Viewer grant could not be saved",
            ));
        }
        Ok(created)
    });
    match mutation.await {
        Ok(Ok(created)) => no_store(json_response(StatusCode::CREATED, &created)),
        Ok(Err(e)) => rejected(e),
        Err(_) => rejected(ApiError::service_unavailable(
            "Viewer grant could not be saved",
        )),
    }
}

#[utoipa::path(delete, path="/api/agents/{agent_id}/sessions/{session_id}/shares/{share_id}", tag="sessions", params(("agent_id"=String,Path),("session_id"=String,Path),("share_id"=String,Path)), responses((status=200,description="Viewer access revoked",body=SharesEnvelope),(status=403,description="Local owner required"),(status=503,description="Revocation not saved")))]
pub(super) async fn revoke(
    State(state): State<AppState>,
    Path((agent, session, share)): Path<(String, String, String)>,
    request: Request,
) -> Response {
    if let Err(error) = authorize(&state, &request, false) {
        return error;
    }
    let mutation = tokio::spawn(async move {
        let _transaction = state.agent_runs.control_plane_transaction().await;
        let (result, persist) = {
            let mut guard = state.daemon.write().await;
            let Some(_) = guard
                .sessions
                .get(&agent, &session)
                .filter(|s| !unavailable(s, &guard))
            else {
                return Err(ApiError::not_found());
            };
            let record = guard.sessions.get_mut(&agent, &session).unwrap();
            let grant = record
                .conversation_shares
                .iter_mut()
                .find(|g| g.id == share)
                .ok_or_else(ApiError::not_found)?;
            grant
                .revoked_at_ms
                .get_or_insert_with(anima_core::primitives::now_millis);
            let result = SharesEnvelope {
                grants: record
                    .conversation_shares
                    .iter()
                    .map(|g| g.view(record))
                    .collect(),
            };
            (result, guard.control_plane_persist_request())
        };
        if persist.save().await.is_err() {
            // Fail closed for this daemon even when disk acknowledgement is
            // uncertain. A retry persists the already stopped grant.
            return Err(ApiError::service_unavailable(
                "Viewer access is stopped, but revocation could not be saved; retry before restarting Anima",
            ));
        }
        Ok(result)
    });
    match mutation.await {
        Ok(Ok(result)) => no_store(json_response(StatusCode::OK, &result)),
        Ok(Err(e)) => rejected(e),
        Err(_) => rejected(ApiError::service_unavailable(
            "Viewer revocation could not be saved",
        )),
    }
}

#[derive(Clone)]
struct ViewerCredentials {
    viewer: String,
    secret: String,
}

fn credentials(headers: &HeaderMap) -> Option<ViewerCredentials> {
    let one = |name: &str| {
        let mut values = headers.get_all(name).iter();
        let first = values.next()?.to_str().ok()?;
        (values.next().is_none() && first.len() <= 128).then(|| first.to_string())
    };
    Some(ViewerCredentials {
        viewer: one(VIEWER_ID_HEADER)?,
        secret: one(VIEWER_SECRET_HEADER)?,
    })
}

fn viewer_denied() -> Response {
    no_store((StatusCode::UNAUTHORIZED, axum::Json(serde_json::json!({"error":"Viewer access is unavailable; check the viewer ID and secret or ask the owner for a new grant"}))).into_response())
}

fn owner_auth_required() -> Response {
    rejected(ApiError::service_unavailable(
        "Configure an owner API key before enabling viewer sharing",
    ))
}

async fn selected(
    state: &AppState,
    share: &str,
    credentials: &ViewerCredentials,
) -> Option<SessionRecord> {
    let guard = state.daemon.read().await;
    let record = guard
        .sessions
        .records()
        .find(|record| {
            !unavailable(record, &guard)
                && record.conversation_shares.iter().any(|g| {
                    g.id == share
                        && g.authorizes(
                            record,
                            &credentials.viewer,
                            &credentials.secret,
                            anima_core::primitives::now_millis(),
                        )
                })
        })
        .cloned();
    record
}

#[utoipa::path(get, path="/api/shared/conversations/{share_id}", tag="sessions", params(("share_id"=String,Path)), responses((status=200,description="Selected conversation title for authenticated viewer",body=SharedConversation),(status=401,description="Viewer credential missing, revoked or conversation cleared")))]
pub(super) async fn get(
    State(state): State<AppState>,
    Path(share): Path<String>,
    request: Request,
) -> Response {
    if !state.sharing_owner_auth_configured {
        return owner_auth_required();
    }
    let Some(credentials) = credentials(request.headers()) else {
        return viewer_denied();
    };
    let Some(record) = selected(&state, &share, &credentials).await else {
        return viewer_denied();
    };
    let viewer_name = record
        .conversation_shares
        .iter()
        .find(|g| g.id == share)
        .unwrap()
        .viewer_name
        .clone();
    no_store(json_response(
        StatusCode::OK,
        &SharedConversation {
            id: share,
            title: record.title,
            viewer_name,
            read_only: true,
            attachments_omitted: true,
        },
    ))
}

#[utoipa::path(get, path="/api/shared/conversations/{share_id}/messages", tag="sessions", params(("share_id"=String,Path),("before"=Option<String>,Query)), responses((status=200,description="Text-only user and assistant messages; no files, tools or private metadata",body=SharedMessages),(status=401,description="Viewer access unavailable"),(status=503,description="History unavailable")))]
pub(super) async fn messages(
    State(state): State<AppState>,
    Path(share): Path<String>,
    request: Request,
) -> Response {
    if !state.sharing_owner_auth_configured {
        return owner_auth_required();
    }
    let Some(credentials) = credentials(request.headers()) else {
        return viewer_denied();
    };
    let Some(record) = selected(&state, &share, &credentials).await else {
        return viewer_denied();
    };
    let query = match super::http::request_query(request.uri()) {
        Ok(query) => query,
        Err(_) => return rejected(ApiError::bad_request_static("Malformed query")),
    };
    if query.keys().any(|k| k != "before") {
        return rejected(ApiError::bad_request_static(
            "Only before is supported for shared transcripts",
        ));
    }
    let before = query.get("before").cloned();
    if before
        .as_ref()
        .is_some_and(|id| id.is_empty() || id.len() > 256 || id.chars().any(char::is_control))
    {
        return rejected(ApiError::bad_request_static("Invalid transcript cursor"));
    }
    let page = views::session_messages(
        &state.daemon,
        &record.agent_id,
        &record.id,
        &MessagePageRequest {
            before,
            limit: 50,
            include_hidden: false,
        },
    )
    .await;
    // History reads await storage. A revoke, clear or reset while waiting must
    // discard even a successfully loaded page before any bytes are exposed.
    let Some(current) = selected(&state, &share, &credentials).await else {
        return viewer_denied();
    };
    if current.created_at_ms != record.created_at_ms
        || current.clear_generation != record.clear_generation
    {
        return viewer_denied();
    }
    match page {
        Ok(page) => no_store(json_response(
            StatusCode::OK,
            &SharedMessages {
                title: current.title,
                messages: page
                    .messages
                    .iter()
                    .filter_map(conversation_shares::project_message)
                    .collect(),
                next_before: page.next_before,
                attachments_omitted: true,
            },
        )),
        Err(MessagePageError::NotFound) => viewer_denied(),
        Err(MessagePageError::BeforeNotFound) => rejected(ApiError::bad_request_static(
            "Transcript cursor was not found",
        )),
        Err(MessagePageError::Unavailable) => rejected(ApiError::service_unavailable(
            "Shared transcript history is unavailable",
        )),
    }
}
