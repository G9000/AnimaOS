//! The owner's console look over HTTP: what the Appearance page saves and
//! every console loads, the same record `change_look` changes from chat.
//! Every route requires the local owner and answers `Cache-Control: no-store`.
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::response::Response;

use super::contracts::ErrorBody;
use super::http::json_response;
use super::jobs::{authorize, body, no_store};
use super::sessions::rejected;
use super::{ApiError, AppState};
use crate::owner_look::OwnerLook;

fn respond(look: &Option<OwnerLook>) -> Response {
    no_store(json_response(StatusCode::OK, look))
}

#[utoipa::path(get, path = "/api/settings/look", tag = "settings",
    responses(
        (status = 200, description = "The owner's console look, or null until a console or chat first saves one", body = Option<OwnerLook>),
        (status = 403, description = "Local owner required", body = ErrorBody)
    ))]
pub(super) async fn get_look(State(state): State<AppState>, request: Request) -> Response {
    if let Err(response) = authorize(&state, &request, true) {
        return response;
    }
    respond(&state.daemon.read().await.owner_look.as_deref().cloned())
}

#[utoipa::path(put, path = "/api/settings/look", tag = "settings",
    request_body = OwnerLook,
    responses(
        (status = 200, description = "Saved; answered with the look now in effect", body = OwnerLook),
        (status = 400, description = "A choice the console does not offer", body = ErrorBody),
        (status = 403, description = "Local owner required", body = ErrorBody),
        (status = 503, description = "Could not be saved; nothing changed", body = ErrorBody)
    ))]
pub(super) async fn put_look(State(state): State<AppState>, request: Request) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let look: OwnerLook = match body(&state, request).await {
        Ok(look) => look,
        Err(response) => return response,
    };
    if let Err(error) = look.validate() {
        return rejected(ApiError::bad_request(error));
    }
    // Keep the transaction through the save and any revert even if the HTTP
    // caller disconnects.
    let operation = tokio::spawn(async move {
        let transaction = state.agent_runs.control_plane_transaction().await;
        let (previous, persist) = {
            let mut guard = state.daemon.write().await;
            let previous = guard.set_owner_look(Some(look.clone()));
            (previous, guard.control_plane_persist_request())
        };
        if let Err(error) = persist.save().await {
            state.daemon.write().await.set_owner_look(previous);
            return rejected(ApiError::service_unavailable(error.to_string()));
        }
        drop(transaction);
        respond(&Some(look))
    });
    operation.await.unwrap_or_else(|_| {
        rejected(ApiError::service_unavailable(
            "The look could not be saved".to_string(),
        ))
    })
}
