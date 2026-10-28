//! Owner-controlled project folder grants and revocable knowledge projections.

use axum::extract::{Path, Request, State};
use axum::http::{StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

use super::{
    jobs::{authorize, no_store},
    json_response, ApiError, AppState,
};
use crate::projects::knowledge::KnowledgeService;
use crate::projects::{AddKnowledgeFolderRequest, ProjectError, ProjectMutation, ProjectService};

fn rejected(error: ProjectError) -> Response {
    let error = match error {
        ProjectError::NotFound | ProjectError::NotMember => ApiError::not_found(),
        ProjectError::StaleRevision
        | ProjectError::ScopeChanged
        | ProjectError::AccessRevoked
        | ProjectError::Archived
        | ProjectError::Busy => ApiError::conflict(error.to_string()),
        ProjectError::AccessUnavailable | ProjectError::Persistence(_) => {
            ApiError::service_unavailable(error.to_string())
        }
        _ => ApiError::bad_request(error.to_string()),
    };
    no_store(error.into_response())
}

fn response<T: Serialize>(result: Result<T, ProjectError>) -> Response {
    match result {
        Ok(value) => match serde_json::to_value(value) {
            Ok(value) => no_store(json_response(StatusCode::OK, &value)),
            Err(_) => rejected(ProjectError::AccessUnavailable),
        },
        Err(error) => rejected(error),
    }
}

async fn service(state: &AppState) -> KnowledgeService {
    state.daemon.read().await.project_knowledge.clone()
}

async fn body<T: serde::de::DeserializeOwned>(
    request: Request,
    state: &AppState,
) -> Result<T, Response> {
    let bytes = axum::body::to_bytes(
        request.into_body(),
        state.config.max_request_bytes.min(64 * 1024),
    )
    .await
    .map_err(|_| {
        no_store(
            ApiError::bad_request_static("Knowledge request is malformed or too large")
                .into_response(),
        )
    })?;
    super::parse_json_body(bytes.to_vec()).map_err(|error| no_store(error.into_response()))
}

fn query(uri: &Uri) -> Result<std::collections::HashMap<String, String>, Response> {
    super::http::request_query(uri).map_err(|_| {
        no_store(ApiError::bad_request_static("Knowledge query is invalid").into_response())
    })
}

fn agent(params: &std::collections::HashMap<String, String>) -> Result<String, Response> {
    params
        .get("asAgentId")
        .filter(|agent| crate::sessions::is_valid_session_id(agent))
        .cloned()
        .ok_or_else(|| {
            no_store(ApiError::bad_request_static("Choose a project member").into_response())
        })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RefreshRequest {
    as_agent_id: String,
    resource_id: Option<String>,
}

#[utoipa::path(post,path="/api/projects/{project_id}/knowledge/sources",tag="projects",params(("project_id"=String,Path)),request_body=serde_json::Value,responses((status=200,description="Explicit read-only folder grant durably saved",body=serde_json::Value)))]
pub(super) async fn add_source(
    State(state): State<AppState>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    if let Err(error) = authorize(&state, &request, false) {
        return error;
    }
    let input: AddKnowledgeFolderRequest = match body(request, &state).await {
        Ok(value) => value,
        Err(error) => return error,
    };
    let agent = input.as_agent_id.clone();
    let result = ProjectService::new(
        state.daemon.clone(),
        state.agent_runs.control_plane_transactions(),
    )
    .mutate(ProjectMutation::AddKnowledgeFolder {
        id: id.clone(),
        input,
    })
    .await;
    if result.is_ok() {
        // A persisted grant remains saved even if its derived cache cannot start.
        let _ = service(&state)
            .await
            .refresh(&state.daemon, &id, &agent, None)
            .await;
    }
    response(result)
}

#[utoipa::path(get,path="/api/projects/{project_id}/knowledge/status",tag="projects",params(("project_id"=String,Path)),responses((status=200,description="Current member index coverage and source generations",body=serde_json::Value)))]
pub(super) async fn status(
    State(state): State<AppState>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    if let Err(error) = authorize(&state, &request, true) {
        return error;
    }
    let params = match query(request.uri()) {
        Ok(value) => value,
        Err(error) => return error,
    };
    let agent = match agent(&params) {
        Ok(value) => value,
        Err(error) => return error,
    };
    response(
        service(&state)
            .await
            .status(&state.daemon, &id, &agent)
            .await,
    )
}

#[utoipa::path(post,path="/api/projects/{project_id}/knowledge/refresh",tag="projects",params(("project_id"=String,Path)),request_body=serde_json::Value,responses((status=200,description="Bounded indexing job started",body=serde_json::Value)))]
pub(super) async fn refresh(
    State(state): State<AppState>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    if let Err(error) = authorize(&state, &request, false) {
        return error;
    }
    let input: RefreshRequest = match body(request, &state).await {
        Ok(value) => value,
        Err(error) => return error,
    };
    if !crate::sessions::is_valid_session_id(&input.as_agent_id) {
        return rejected(ProjectError::InvalidRecord("Choose a project member"));
    }
    response(
        service(&state)
            .await
            .refresh(
                &state.daemon,
                &id,
                &input.as_agent_id,
                input.resource_id.as_deref(),
            )
            .await,
    )
}

#[utoipa::path(get,path="/api/projects/{project_id}/knowledge/search",tag="projects",params(("project_id"=String,Path)),responses((status=200,description="Current verified passages from selected folder grants",body=serde_json::Value)))]
pub(super) async fn search(
    State(state): State<AppState>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    if let Err(error) = authorize(&state, &request, true) {
        return error;
    }
    let params = match query(request.uri()) {
        Ok(value) => value,
        Err(error) => return error,
    };
    let agent = match agent(&params) {
        Ok(value) => value,
        Err(error) => return error,
    };
    let Some(query) = params.get("query") else {
        return rejected(ProjectError::InvalidRecord("Enter a search query"));
    };
    let limit = match params.get("limit") {
        None => 8,
        Some(value) => match value.parse::<usize>() {
            Ok(value) if (1..=20).contains(&value) => value,
            _ => {
                return rejected(ProjectError::InvalidRecord(
                    "Choose a result limit from 1 to 20",
                ))
            }
        },
    };
    response(
        service(&state)
            .await
            .search(
                &state.daemon,
                &id,
                &agent,
                query,
                limit,
                anima_core::CancelSignal::new(),
                None,
            )
            .await,
    )
}

#[utoipa::path(get,path="/api/projects/{project_id}/knowledge/citations/{citation_id}",tag="projects",params(("project_id"=String,Path),("citation_id"=String,Path)),responses((status=200,description="Citation text after current grant and original-byte verification",body=serde_json::Value),(status=404,description="Source or indexed revision unavailable")))]
pub(super) async fn citation(
    State(state): State<AppState>,
    Path((id, citation_id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(error) = authorize(&state, &request, true) {
        return error;
    }
    let params = match query(request.uri()) {
        Ok(value) => value,
        Err(error) => return error,
    };
    let agent = match agent(&params) {
        Ok(value) => value,
        Err(error) => return error,
    };
    response(
        service(&state)
            .await
            .citation(&state.daemon, &id, &agent, &citation_id)
            .await,
    )
}
