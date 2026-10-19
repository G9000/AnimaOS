//! The API-key pool for each provider: list, add, remove, wake a resting key,
//! and set how the keys take turns. Every route requires the local owner and answers
//! `Cache-Control: no-store`, errors included. Keys are never returned or
//! logged: answers carry masked views only, request bodies are not traced
//! (the request span records the method and path), and body parse errors do
//! not echo the body.
use axum::extract::{Path, Request, State};
use axum::http::StatusCode;
use axum::response::Response;
use serde::Deserialize;

use super::contracts::{
    DeleteResponse, ErrorBody, ProviderKeyResponse, ProviderKeysEnvelope, ProviderKeysResponse,
};
use super::http::json_response;
use super::jobs::{authorize, body, no_store};
use super::sessions::rejected;
use super::{ApiError, AppState};
use crate::provider_keys::{KeyError, KeyPool, Strategy};

/// No `Debug`: the body holds a key.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct AddKeyRequest {
    /// A name for the key; trimmed, at most 80 characters.
    label: Option<String>,
    /// The API key; trimmed, at most 2,000 characters.
    key: String,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct StrategyRequest {
    /// `fill_first`, `round_robin`, `random` or `least_used`.
    strategy: String,
}

fn provider_id(raw: &str) -> String {
    raw.trim().to_ascii_lowercase()
}

fn now_ms() -> u64 {
    anima_core::primitives::now_millis()
}

async fn pool(state: &AppState) -> KeyPool {
    state.daemon.read().await.provider_keys.clone()
}

/// What a pool refusal means for the caller. `missing` is the status for an
/// unknown provider or key: a bad provider in an add or strategy call is a bad
/// request, while a key that is not there is `404`.
fn key_error(error: KeyError, missing: ApiError) -> ApiError {
    match error {
        KeyError::Unknown => missing,
        KeyError::Empty => ApiError::bad_request_static("key must not be empty"),
        KeyError::TooLong => ApiError::bad_request_static("key must be at most 2,000 characters"),
        KeyError::Duplicate => ApiError::conflict("that key is already in the pool"),
        KeyError::EnvKey => ApiError::conflict(
            "the environment key can only be changed in the daemon's environment",
        ),
        KeyError::NotPersisted | KeyError::Io(_) => {
            if let KeyError::Io(error) = &error {
                tracing::warn!(%error, "could not save the provider key file");
            }
            ApiError::service_unavailable("The keys could not be saved; nothing changed")
        }
    }
}

fn unknown_provider(provider: &str) -> ApiError {
    ApiError::bad_request(format!("provider '{provider}' does not take API keys"))
}

#[utoipa::path(get, path = "/api/provider-keys", tag = "providers",
    responses(
        (status = 200, description = "Every provider that takes a key, with its key pool; keys are masked", body = ProviderKeysEnvelope),
        (status = 403, description = "Local owner required", body = ErrorBody)
    ))]
pub(super) async fn list_keys(State(state): State<AppState>, request: Request) -> Response {
    if let Err(response) = authorize(&state, &request, true) {
        return response;
    }
    let providers = pool(&state)
        .await
        .list(now_ms())
        .into_iter()
        .map(ProviderKeysResponse::from)
        .collect();
    no_store(json_response(
        StatusCode::OK,
        &ProviderKeysEnvelope { providers },
    ))
}

#[utoipa::path(post, path = "/api/provider-keys/{provider}", tag = "providers",
    params(("provider" = String, Path, description = "Provider id, for example openai")),
    request_body = AddKeyRequest,
    responses(
        (status = 201, description = "Added; the key as listed, masked", body = ProviderKeyResponse),
        (status = 400, description = "An unknown provider, or an empty key or one over 2,000 characters", body = ErrorBody),
        (status = 403, description = "Local owner required", body = ErrorBody),
        (status = 409, description = "The key is already in the pool", body = ErrorBody),
        (status = 503, description = "The key file could not be written; nothing changed", body = ErrorBody)
    ))]
pub(super) async fn add_key(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let input: AddKeyRequest = match body(&state, request).await {
        Ok(input) => input,
        Err(response) => return response,
    };
    let provider = provider_id(&provider);
    match pool(&state)
        .await
        .add(&provider, input.label, input.key, now_ms())
    {
        Ok(view) => no_store(json_response(
            StatusCode::CREATED,
            &ProviderKeyResponse::from(view),
        )),
        Err(error) => rejected(key_error(error, unknown_provider(&provider))),
    }
}

#[utoipa::path(delete, path = "/api/provider-keys/{provider}/{key_id}", tag = "providers",
    params(
        ("provider" = String, Path, description = "Provider id"),
        ("key_id" = String, Path, description = "Key id from the list")
    ),
    responses(
        (status = 200, description = "Removed", body = DeleteResponse),
        (status = 403, description = "Local owner required", body = ErrorBody),
        (status = 404, description = "No such key", body = ErrorBody),
        (status = 409, description = "The environment key cannot be removed here", body = ErrorBody),
        (status = 503, description = "The key file could not be written; nothing changed", body = ErrorBody)
    ))]
pub(super) async fn remove_key(
    State(state): State<AppState>,
    Path((provider, key_id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    match pool(&state).await.remove(&provider_id(&provider), &key_id) {
        Ok(()) => no_store(json_response(
            StatusCode::OK,
            &DeleteResponse { deleted: true },
        )),
        Err(error) => rejected(key_error(error, ApiError::not_found())),
    }
}

#[utoipa::path(put, path = "/api/provider-keys/{provider}/strategy", tag = "providers",
    params(("provider" = String, Path, description = "Provider id")),
    request_body = StrategyRequest,
    responses(
        (status = 200, description = "Saved; the provider's key pool", body = ProviderKeysResponse),
        (status = 400, description = "An unknown provider or strategy", body = ErrorBody),
        (status = 403, description = "Local owner required", body = ErrorBody)
    ))]
pub(super) async fn set_strategy(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let input: StrategyRequest = match body(&state, request).await {
        Ok(input) => input,
        Err(response) => return response,
    };
    let Some(strategy) = Strategy::parse(input.strategy.trim()) else {
        return rejected(ApiError::bad_request_static(
            "strategy must be fill_first, round_robin, random or least_used",
        ));
    };
    let provider = provider_id(&provider);
    let pool = pool(&state).await;
    if let Err(error) = pool.set_strategy(&provider, strategy) {
        return rejected(key_error(error, unknown_provider(&provider)));
    }
    match pool
        .list(now_ms())
        .into_iter()
        .find(|view| view.id == provider)
    {
        Some(view) => no_store(json_response(
            StatusCode::OK,
            &ProviderKeysResponse::from(view),
        )),
        None => rejected(unknown_provider(&provider)),
    }
}

#[utoipa::path(post, path = "/api/provider-keys/{provider}/{key_id}/wake", tag = "providers",
    params(
        ("provider" = String, Path, description = "Provider id"),
        ("key_id" = String, Path, description = "Key id from the list; `env` for the environment key")
    ),
    responses(
        (status = 200, description = "The key's rest ended (\"Try now\"); the key as listed, masked", body = ProviderKeyResponse),
        (status = 403, description = "Local owner required", body = ErrorBody),
        (status = 404, description = "No such key", body = ErrorBody)
    ))]
pub(super) async fn wake_key(
    State(state): State<AppState>,
    Path((provider, key_id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    match pool(&state)
        .await
        .wake(&provider_id(&provider), &key_id, now_ms())
    {
        Ok(view) => no_store(json_response(
            StatusCode::OK,
            &ProviderKeyResponse::from(view),
        )),
        Err(error) => rejected(key_error(error, ApiError::not_found())),
    }
}
