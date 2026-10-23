//! Owner-only durable chat model preferences.
use super::{
    http::json_response,
    jobs::{authorize, body, no_store},
    sessions::rejected,
    ApiError, AppState,
};
use crate::{
    chat_models::{self, ModelPreference},
    live::LiveEventBody,
    sessions::SessionKind,
};
use axum::{
    extract::{Path, Request, State},
    http::StatusCode,
    response::Response,
};

#[derive(serde::Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PreferencesPatch {
    primary: Option<ModelPreference>,
    reviewer: Option<ModelPreference>,
    clear_generation: Option<u64>,
}

#[utoipa::path(get, path = "/api/agents/{agent_id}/sessions/{session_id}/model-preferences", tag = "sessions", params(("agent_id" = String, Path), ("session_id" = String, Path)), responses((status = 200, description = "Preferences, effective next-turn choices, and supported controls"), (status = 403, description = "Local owner required"), (status = 404, description = "Session not found")))]
pub(super) async fn get_model_preferences(
    State(state): State<AppState>,
    Path((agent, session)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, true) {
        return response;
    }
    respond(&state, &agent, &session).await
}

async fn respond(state: &AppState, agent: &str, session: &str) -> Response {
    match chat_models::view(&state.daemon, agent, session).await {
        Ok(view) => no_store(json_response(StatusCode::OK, &view)),
        Err(error) => rejected(error),
    }
}

#[utoipa::path(patch, path = "/api/agents/{agent_id}/sessions/{session_id}/model-preferences", tag = "sessions", params(("agent_id" = String, Path), ("session_id" = String, Path)), request_body = PreferencesPatch, responses((status = 200, description = "Saved for next turn"), (status = 400, description = "Unsupported or empty preferences"), (status = 403, description = "Local owner required"), (status = 404, description = "Session not found"), (status = 409, description = "Provider unavailable or conversation changed"), (status = 503, description = "Save failed; preference change reverted")))]
pub(super) async fn update_model_preferences(
    State(state): State<AppState>,
    Path((agent, session)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let input: PreferencesPatch = match body(&state, request).await {
        Ok(input) => input,
        Err(response) => return response,
    };
    if input.primary.is_none() && input.reviewer.is_none() {
        return rejected(ApiError::bad_request_static(
            "At least one of primary or reviewer is required",
        ));
    }
    let primary = match input
        .primary
        .as_ref()
        .map(chat_models::validate_preference)
        .transpose()
    {
        Ok(value) => value,
        Err(error) => return rejected(error),
    };
    let reviewer = match input
        .reviewer
        .as_ref()
        .map(chat_models::validate_preference)
        .transpose()
    {
        Ok(value) => value,
        Err(error) => return rejected(error),
    };
    for choice in [&primary, &reviewer].into_iter().flatten() {
        if let ModelPreference::Override { provider, .. } = choice {
            if !chat_models::provider_available(&state.daemon, provider).await {
                return rejected(ApiError::conflict(
                    "The selected provider is unavailable; connect it in Models",
                ));
            }
        }
    }
    // The worker owns rollback even if the HTTP caller disconnects.
    let operation = tokio::spawn(async move {
        let transaction = state.agent_runs.control_plane_transaction().await;
        let (candidate, persist) = {
            let mut guard = state.daemon.write().await;
            let record = match guard.sessions.get_mut(&agent, &session) {
                Some(record) if record.kind == SessionKind::Chat => record,
                Some(_) => {
                    return rejected(ApiError::conflict(
                        "Only chat sessions support model preferences",
                    ))
                }
                None => return rejected(ApiError::not_found()),
            };
            if input
                .clear_generation
                .is_some_and(|generation| generation != record.clear_generation)
            {
                return rejected(ApiError::conflict(
                    "The conversation was cleared; reload its model settings",
                ));
            }
            let previous = record.model_preferences.clone();
            if let Some(primary) = primary {
                record.model_preferences.primary = primary;
            }
            if let Some(reviewer) = reviewer {
                record.model_preferences.reviewer = reviewer;
            }
            let candidate = record.model_preferences.clone();
            let persist = guard.control_plane_persist_request();
            guard
                .sessions
                .get_mut(&agent, &session)
                .expect("model settings session exists")
                .model_preferences = previous;
            (candidate, persist)
        };
        if let Err(error) = persist.save().await {
            return rejected(ApiError::service_unavailable(error.to_string()));
        }
        state
            .daemon
            .write()
            .await
            .sessions
            .get_mut(&agent, &session)
            .expect("model settings transaction retains session")
            .model_preferences = candidate;
        state.daemon.read().await.publish_session_event(
            &agent,
            &session,
            LiveEventBody::SessionUpdated,
        );
        drop(transaction);
        respond(&state, &agent, &session).await
    });
    operation.await.unwrap_or_else(|_| {
        rejected(ApiError::service_unavailable(
            "Model preference update failed",
        ))
    })
}
