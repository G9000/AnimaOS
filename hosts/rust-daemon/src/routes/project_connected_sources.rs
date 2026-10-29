//! Owner-selected account content shared as revocable derived project knowledge.

use axum::extract::{Path, Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

use super::{
    jobs::{authorize, no_store},
    json_response, ApiError, AppState,
};
use crate::projects::{
    connected::{self, AddConnectedSourceRequest},
    ProjectError, ProjectMutation, ProjectService,
};

fn response<T: Serialize>(result: Result<T, ProjectError>) -> Response {
    match result {
        Ok(value) => match serde_json::to_value(value) {
            Ok(value) => no_store(json_response(StatusCode::OK, &value)),
            Err(_) => no_store(
                ApiError::service_unavailable("Selected source is unavailable").into_response(),
            ),
        },
        Err(error) => no_store(
            match error {
                ProjectError::NotFound | ProjectError::NotMember => ApiError::not_found(),
                ProjectError::StaleRevision
                | ProjectError::ScopeChanged
                | ProjectError::AccessRevoked
                | ProjectError::Busy
                | ProjectError::Archived => ApiError::conflict(error.to_string()),
                ProjectError::AccessUnavailable | ProjectError::Persistence(_) => {
                    ApiError::service_unavailable(error.to_string())
                }
                _ => ApiError::bad_request(error.to_string()),
            }
            .into_response(),
        ),
    }
}

#[utoipa::path(get,path="/api/projects/{project_id}/knowledge/connected-sources",tag="projects",params(("project_id"=String,Path)),responses((status=200,description="Bounded content-free selected-account choices",body=serde_json::Value)))]
pub(super) async fn choices(
    State(state): State<AppState>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    if let Err(error) = authorize(&state, &request, true) {
        return error;
    }
    let params = match super::http::request_query(request.uri()) {
        Ok(value) => value,
        Err(_) => {
            return no_store(
                ApiError::bad_request_static("Selected source query is invalid").into_response(),
            )
        }
    };
    let Some(agent) = params
        .get("asAgentId")
        .filter(|agent| crate::sessions::is_valid_session_id(agent))
    else {
        return response::<serde_json::Value>(Err(ProjectError::InvalidRecord(
            "Choose a project member",
        )));
    };
    response(
        connected::choices(
            &state.daemon,
            &id,
            agent,
            params.get("query").map_or("", String::as_str),
        )
        .await,
    )
}

#[utoipa::path(post,path="/api/projects/{project_id}/knowledge/connected-sources",tag="projects",params(("project_id"=String,Path)),request_body=serde_json::Value,responses((status=200,description="Selected source grant durably saved after explicit sharing consent",body=serde_json::Value)))]
pub(super) async fn add(
    State(state): State<AppState>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    if let Err(error) = authorize(&state, &request, false) {
        return error;
    }
    let bytes = match axum::body::to_bytes(
        request.into_body(),
        state.config.max_request_bytes.min(64 * 1024),
    )
    .await
    {
        Ok(value) => value,
        Err(_) => {
            return no_store(
                ApiError::bad_request_static("Selected source request is malformed or too large")
                    .into_response(),
            )
        }
    };
    let input: AddConnectedSourceRequest = match super::parse_json_body(bytes.to_vec()) {
        Ok(value) => value,
        Err(error) => return no_store(error.into_response()),
    };
    let member = input.as_agent_id.clone();
    let result = ProjectService::new(
        state.daemon.clone(),
        state.agent_runs.control_plane_transactions(),
    )
    .mutate(ProjectMutation::AddConnectedSource {
        id: id.clone(),
        input,
    })
    .await;
    if result.is_ok() {
        let service = state.daemon.read().await.project_knowledge.clone();
        let _ = service.refresh(&state.daemon, &id, &member, None).await;
    }
    response(result)
}
