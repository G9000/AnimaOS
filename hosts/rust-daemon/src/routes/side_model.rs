//! The workspace side-task model: the model titles, compaction, the smart
//! check and learning reviews try before an agent's own. Every route
//! requires the local owner and answers `Cache-Control: no-store`.
use anima_core::ModelChoice;
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::response::Response;
use serde::Deserialize;

use super::contracts::{ErrorBody, ModelChoiceResponse};
use super::http::json_response;
use super::jobs::{authorize, body, no_store};
use super::sessions::rejected;
use super::{ApiError, AppState};

/// The longest model name a side model may have.
const MAX_MODEL_CHARS: usize = 200;

/// A provider and model; the request body is this or `null` (clear).
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct SideModelRequest {
    /// A provider the daemon can route to, for example `openai`.
    provider: String,
    /// Up to 200 characters.
    model: String,
}

fn validated(input: SideModelRequest) -> Result<ModelChoice, ApiError> {
    let provider = input.provider.trim().to_ascii_lowercase();
    let model = input.model.trim().to_string();
    if provider.is_empty() || model.is_empty() {
        return Err(ApiError::bad_request_static(
            "provider and model must not be empty",
        ));
    }
    if crate::runtime_model::is_mock_provider(&provider) {
        return Err(ApiError::bad_request_static(
            "the test providers (deterministic, test) cannot be the side model",
        ));
    }
    if !crate::runtime_model::is_routable_provider(&provider) {
        return Err(ApiError::bad_request(format!(
            "provider '{provider}' is not a known provider"
        )));
    }
    if model.chars().count() > MAX_MODEL_CHARS {
        return Err(ApiError::bad_request_static(
            "model must be at most 200 characters",
        ));
    }
    Ok(ModelChoice { provider, model })
}

fn respond(choice: &Option<ModelChoice>) -> Response {
    no_store(json_response(
        StatusCode::OK,
        &choice.as_ref().map(ModelChoiceResponse::from),
    ))
}

#[utoipa::path(get, path = "/api/settings/side-model", tag = "settings",
    responses(
        (status = 200, description = "The side-task model as { provider, model }, or null for each agent's own", body = Option<ModelChoiceResponse>),
        (status = 403, description = "Local owner required", body = ErrorBody)
    ))]
pub(super) async fn get_side_model(State(state): State<AppState>, request: Request) -> Response {
    if let Err(response) = authorize(&state, &request, true) {
        return response;
    }
    respond(&state.daemon.read().await.side_model)
}

#[utoipa::path(put, path = "/api/settings/side-model", tag = "settings",
    request_body = Option<SideModelRequest>,
    responses(
        (status = 200, description = "Saved; null cleared it", body = Option<ModelChoiceResponse>),
        (status = 400, description = "An unknown or test provider, an empty or overlong model", body = ErrorBody),
        (status = 403, description = "Local owner required", body = ErrorBody),
        (status = 503, description = "Could not be saved; nothing changed", body = ErrorBody)
    ))]
pub(super) async fn put_side_model(State(state): State<AppState>, request: Request) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let input: Option<SideModelRequest> = match body(&state, request).await {
        Ok(input) => input,
        Err(response) => return response,
    };
    let choice = match input.map(validated).transpose() {
        Ok(choice) => choice,
        Err(error) => return rejected(error),
    };
    // Keep the transaction through the save and any revert even if the HTTP
    // caller disconnects.
    let operation = tokio::spawn(async move {
        let transaction = state.agent_runs.control_plane_transaction().await;
        let (previous, persist) = {
            let mut guard = state.daemon.write().await;
            let previous = guard.set_side_model(choice.clone());
            (previous, guard.control_plane_persist_request())
        };
        if let Err(error) = persist.save().await {
            state.daemon.write().await.set_side_model(previous);
            return rejected(ApiError::service_unavailable(error.to_string()));
        }
        drop(transaction);
        respond(&choice)
    });
    operation.await.unwrap_or_else(|_| {
        rejected(ApiError::service_unavailable(
            "The side-task model could not be saved".to_string(),
        ))
    })
}
