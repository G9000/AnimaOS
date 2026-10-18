//! Coding apps: stopping an agent's app and coding-agent sessions from the
//! chat, the keys the owner hands to coding apps, and which coding apps this
//! computer has. Every route requires the local owner.
use axum::extract::{Path, Request, State};
use axum::http::StatusCode;
use axum::response::Response;
use serde::{Deserialize, Serialize};

use super::contracts::ErrorBody;
use super::http::json_response;
use super::jobs::{authorize, body, no_store};
use super::sessions::rejected;
use super::{ApiError, AppState};
use crate::agent_runs::is_helper_config;
use crate::coding_keys::CodingKeyView;
use crate::tools::process::live::live_sessions;

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct StopOutcome {
    stopped: bool,
    /// Every process the session started is proven gone.
    tree_gone: bool,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct CancelOutcome {
    cancelled: bool,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct CodingKeysEnvelope {
    keys: Vec<CodingKeyView>,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct CodingKeysRequest {
    enabled: Vec<String>,
}

async fn known_agent(state: &AppState, agent_id: &str) -> Result<(), Response> {
    if state.daemon.read().await.agents.contains_key(agent_id) {
        Ok(())
    } else {
        Err(rejected(ApiError::not_found()))
    }
}

#[utoipa::path(post, path = "/api/agents/{agent_id}/app-sessions/{session_id}/stop", tag = "coding",
    params(("agent_id" = String, Path), ("session_id" = String, Path)),
    responses(
        (status = 200, description = "Stopped", body = StopOutcome),
        (status = 403, description = "Local owner required", body = ErrorBody),
        (status = 404, description = "No such session for this agent", body = ErrorBody)
    ))]
pub(super) async fn stop_app_session(
    State(state): State<AppState>,
    Path((agent_id, session_id)): Path<(String, String)>,
    request: Request,
) -> Response {
    stop_session(state, agent_id, session_id, "app-", request).await
}

#[utoipa::path(post, path = "/api/agents/{agent_id}/acp-sessions/{session_id}/stop", tag = "coding",
    params(("agent_id" = String, Path), ("session_id" = String, Path)),
    responses(
        (status = 200, description = "Stopped", body = StopOutcome),
        (status = 403, description = "Local owner required", body = ErrorBody),
        (status = 404, description = "No such session for this agent", body = ErrorBody)
    ))]
pub(super) async fn stop_acp_session(
    State(state): State<AppState>,
    Path((agent_id, session_id)): Path<(String, String)>,
    request: Request,
) -> Response {
    stop_session(state, agent_id, session_id, "acp-", request).await
}

async fn stop_session(
    state: AppState,
    agent_id: String,
    session_id: String,
    prefix: &str,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    if !session_id.starts_with(prefix) {
        return rejected(ApiError::not_found());
    }
    match live_sessions().stop(&agent_id, &session_id).await {
        Ok(tree_gone) => no_store(json_response(
            StatusCode::OK,
            &StopOutcome {
                stopped: true,
                tree_gone,
            },
        )),
        Err(_) => rejected(ApiError::not_found()),
    }
}

#[utoipa::path(post, path = "/api/agents/{agent_id}/acp-sessions/{session_id}/cancel", tag = "coding",
    params(("agent_id" = String, Path), ("session_id" = String, Path)),
    responses(
        (status = 200, description = "The coding agent's turn was cancelled", body = CancelOutcome),
        (status = 403, description = "Local owner required", body = ErrorBody),
        (status = 404, description = "No such coding agent for this agent", body = ErrorBody)
    ))]
pub(super) async fn cancel_acp_session(
    State(state): State<AppState>,
    Path((agent_id, session_id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    if !session_id.starts_with("acp-") {
        return rejected(ApiError::not_found());
    }
    match live_sessions().cancel_acp(&agent_id, &session_id).await {
        Ok(()) => no_store(json_response(
            StatusCode::OK,
            &CancelOutcome { cancelled: true },
        )),
        Err(_) => rejected(ApiError::not_found()),
    }
}

#[utoipa::path(get, path = "/api/agents/{agent_id}/coding-keys", tag = "coding",
    params(("agent_id" = String, Path)),
    responses(
        (status = 200, description = "Keys coding apps may be given; values are never returned", body = CodingKeysEnvelope),
        (status = 403, description = "Local owner required", body = ErrorBody),
        (status = 404, description = "Agent not found", body = ErrorBody)
    ))]
pub(super) async fn get_coding_keys(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, true) {
        return response;
    }
    if let Err(response) = known_agent(&state, &agent_id).await {
        return response;
    }
    no_store(json_response(
        StatusCode::OK,
        &CodingKeysEnvelope {
            keys: crate::coding_keys::view(&agent_id),
        },
    ))
}

#[utoipa::path(put, path = "/api/agents/{agent_id}/coding-keys", tag = "coding",
    params(("agent_id" = String, Path)),
    request_body = CodingKeysRequest,
    responses(
        (status = 200, description = "Saved", body = CodingKeysEnvelope),
        (status = 400, description = "An unknown or unsaved key", body = ErrorBody),
        (status = 403, description = "Local owner required", body = ErrorBody),
        (status = 404, description = "Agent not found", body = ErrorBody),
        (status = 409, description = "A helper, which starts no coding apps", body = ErrorBody),
        (status = 503, description = "Could not be saved; the old choice stays", body = ErrorBody)
    ))]
pub(super) async fn put_coding_keys(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let input: CodingKeysRequest = match body(&state, request).await {
        Ok(input) => input,
        Err(response) => return response,
    };
    let enabled = match crate::coding_keys::validate(&input.enabled) {
        Ok(enabled) => enabled,
        Err(message) => return rejected(ApiError::bad_request(message)),
    };
    let operation = tokio::spawn(async move {
        let transaction = state.agent_runs.control_plane_transaction().await;
        let (previous, persist) = {
            let mut guard = state.daemon.write().await;
            match guard.agents.get(&agent_id) {
                None => return rejected(ApiError::not_found()),
                Some(runtime) if is_helper_config(runtime.config()) => {
                    return rejected(ApiError::conflict(
                        "Helpers don't start coding apps".to_string(),
                    ))
                }
                Some(_) => {}
            }
            let previous = crate::coding_keys::set(&agent_id, enabled);
            (previous, guard.control_plane_persist_request())
        };
        if let Err(error) = persist.save().await {
            crate::coding_keys::set(&agent_id, previous);
            return rejected(ApiError::service_unavailable(error.to_string()));
        }
        drop(transaction);
        no_store(json_response(
            StatusCode::OK,
            &CodingKeysEnvelope {
                keys: crate::coding_keys::view(&agent_id),
            },
        ))
    });
    operation.await.unwrap_or_else(|_| {
        rejected(ApiError::service_unavailable(
            "The keys could not be saved".to_string(),
        ))
    })
}

#[utoipa::path(get, path = "/api/coding-apps", tag = "coding",
    responses(
        (status = 200, description = "Coding apps on this computer and their know-how bundles", body = crate::coding_apps::CodingAppsEnvelope),
        (status = 403, description = "Local owner required", body = ErrorBody)
    ))]
pub(super) async fn list_coding_apps(State(state): State<AppState>, request: Request) -> Response {
    if let Err(response) = authorize(&state, &request, true) {
        return response;
    }
    match tokio::task::spawn_blocking(crate::coding_apps::detect).await {
        Ok(apps) => no_store(json_response(StatusCode::OK, &apps)),
        Err(_) => rejected(ApiError::service_unavailable(
            "Could not look for coding apps".to_string(),
        )),
    }
}
