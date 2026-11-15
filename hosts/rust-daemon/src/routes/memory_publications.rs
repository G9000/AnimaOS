//! Owner-visible durable sharing. Local admission is distinct from remote saving.
use super::{
    jobs::{authorize, body, no_store},
    json_response, ApiError, AppState,
};
use crate::memory_provider::publication::{
    summary, PublicationError, PublicationService, Selection,
};
use axum::{
    extract::{Path, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

fn failure(error: PublicationError) -> Response {
    let error = match error {
        PublicationError::Invalid(message) => ApiError::bad_request(message),
        PublicationError::Conflict(message) => ApiError::conflict(message),
        PublicationError::Unavailable(message) => ApiError::service_unavailable(message),
        PublicationError::NotFound => ApiError::not_found(),
    };
    no_store(error.into_response())
}

async fn service(
    state: &AppState,
    agent: &str,
) -> Result<Option<Arc<PublicationService>>, PublicationError> {
    let guard = state.daemon.read().await;
    guard
        .ensure_profile_owned()
        .map_err(|_| PublicationError::Unavailable("Profile ownership unavailable".into()))?;
    let runtime = guard.agents.get(agent).ok_or(PublicationError::NotFound)?;
    if crate::agent_runs::is_helper_config(runtime.config()) {
        return Err(PublicationError::Invalid(
            "Helpers cannot share garden memories".into(),
        ));
    }
    Ok(guard.memory_publications.clone())
}

fn storage_unavailable() -> PublicationError {
    PublicationError::Unavailable("Persistent local storage is required for durable sharing".into())
}

#[utoipa::path(get, path="/api/agents/{agent_id}/memory-publications",tag="memories",
    params(("agent_id"=String,Path)),responses((status=200,description="Owner-only selected sharing status; no raw memory content"),(status=403,description="Local owner required")))]
pub(super) async fn list(
    State(state): State<AppState>,
    Path(agent): Path<String>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    match service(&state, &agent).await {
        Ok(Some(service)) => match service.list(&agent).await {
            Ok(value) => no_store(json_response(StatusCode::OK, &value)),
            Err(error) => failure(error),
        },
        Ok(None) => {
            let reason = state
                .daemon
                .read()
                .await
                .memory_publications_unavailable
                .clone()
                .unwrap_or_else(|| "persistent_storage_required".into());
            no_store(json_response(
                StatusCode::OK,
                &json!({"available":false,"reason":reason,"items":[],"counts":{},"omitted":0}),
            ))
        }
        Err(error) => failure(error),
    }
}

#[utoipa::path(get,path="/api/agents/{agent_id}/memory-publications/{publication_id}",tag="memories",
    params(("agent_id"=String,Path),("publication_id"=String,Path)),responses((status=200,description="One current local publication outcome"),(status=404,description="Publication not found")))]
pub(super) async fn get(
    State(state): State<AppState>,
    Path((agent, id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let service = match service(&state, &agent).await {
        Ok(Some(value)) => value,
        Ok(None) => return failure(storage_unavailable()),
        Err(error) => return failure(error),
    };
    match service.get(&agent, &id).await {
        Ok(Some(value)) => no_store(json_response(StatusCode::OK, &summary(&value))),
        Ok(None) => failure(PublicationError::NotFound),
        Err(error) => failure(error),
    }
}

#[utoipa::path(post,path="/api/agents/{agent_id}/memory-publications",tag="memories",
    params(("agent_id"=String,Path)),responses((status=200,description="Durable local admission, not confirmation of a remote save"),(status=409,description="Changed source or idempotency conflict"),(status=503,description="Durable admission unavailable")))]
pub(super) async fn submit(
    State(state): State<AppState>,
    Path(agent): Path<String>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let selection: Selection = match body(&state, request).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    let service = match service(&state, &agent).await {
        Ok(Some(value)) => value,
        Ok(None) => return failure(storage_unavailable()),
        Err(error) => return failure(error),
    };
    // A lost HTTP response must not interrupt the atomic durable admission.
    match tokio::spawn(async move { service.admit(&agent, selection).await }).await {
        Ok(Ok(value)) => no_store(json_response(StatusCode::OK, &summary(&value))),
        Ok(Err(error)) => failure(error),
        Err(_) => failure(PublicationError::Unavailable(
            "Admission unconfirmed; check this publication ID before retrying".into(),
        )),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Control {
    expected_revision: u64,
    action: Action,
}
#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Action {
    Resume,
    Cancel,
}

#[utoipa::path(post,path="/api/agents/{agent_id}/memory-publications/{publication_id}/control",tag="memories",
    params(("agent_id"=String,Path),("publication_id"=String,Path)),responses((status=200,description="Revision-checked resume or never-sent cancellation"),(status=409,description="Stale revision or invalid transition")))]
pub(super) async fn control(
    State(state): State<AppState>,
    Path((agent, id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let input: Control = match body(&state, request).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    let service = match service(&state, &agent).await {
        Ok(Some(value)) => value,
        Ok(None) => return failure(storage_unavailable()),
        Err(error) => return failure(error),
    };
    let action = match input.action {
        Action::Resume => "resume",
        Action::Cancel => "cancel",
    };
    match tokio::spawn(async move {
        service
            .control(&agent, &id, input.expected_revision, action)
            .await
    })
    .await
    {
        Ok(Ok(value)) => no_store(json_response(StatusCode::OK, &summary(&value))),
        Ok(Err(error)) => failure(error),
        Err(_) => failure(PublicationError::Unavailable(
            "Change unconfirmed; reread the publication before retrying".into(),
        )),
    }
}
