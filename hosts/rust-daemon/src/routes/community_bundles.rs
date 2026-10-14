//! Local owner review and metadata-only community bundle lifecycle.
use super::contracts::ErrorBody;
use super::http::{json_response, read_limited_body};
use super::jobs::{authorize, no_store};
use super::{ApiError, AppState};
use crate::community_bundles::*;
use axum::extract::{Path, Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::de::DeserializeOwned;

// Six-byte JSON escaping of the decoded manifest cap plus bounded bindings.
const MAX_BODY: usize = 128 * 1024 * 6 + 16 * 1024;
async fn body<T: DeserializeOwned>(request: Request) -> Result<T, Response> {
    let bytes = read_limited_body(request, MAX_BODY).await.map_err(|_| {
        no_store(ApiError::bad_request_static("Community bundle input is invalid").into_response())
    })?;
    serde_json::from_slice(&bytes).map_err(|_| {
        no_store(ApiError::bad_request_static("Community bundle input is invalid").into_response())
    })
}
fn service(state: &AppState) -> BundleService {
    BundleService::new(state.daemon.clone(), state.mcp.clone())
}
fn error(error: BundleError) -> Response {
    let response = match error {
        BundleError::Invalid => ApiError::bad_request_static(error.message()),
        BundleError::NotFound => ApiError::not_found(),
        BundleError::Conflict => ApiError::conflict(error.message()),
        BundleError::Unavailable => ApiError::service_unavailable(error.message()),
    };
    no_store(response.into_response())
}
fn one(status: StatusCode, result: Result<BundleView, BundleError>) -> Response {
    match result {
        Ok(bundle) => no_store(json_response(status, &BundleEnvelope { bundle })),
        Err(problem) => error(problem),
    }
}

#[utoipa::path(post,path="/api/agents/{agent_id}/community-bundles/reviews",tag="community-bundles",params(("agent_id"=String,Path)),request_body=ReviewRequest,
 responses((status=200,description="Read-only exact manifest and component review",body=ReviewEnvelope),(status=400,description="Invalid input",body=ErrorBody),(status=403,description="Local owner required",body=ErrorBody),(status=409,description="Workspace, capability or component state conflict",body=ErrorBody))) ]
pub(super) async fn review(
    State(state): State<AppState>,
    Path(agent): Path<String>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let input = match body::<ReviewRequest>(request).await {
        Ok(input) => input,
        Err(response) => return response,
    };
    match service(&state).review(&agent, input).await {
        Ok(review) => no_store(json_response(StatusCode::OK, &ReviewEnvelope { review })),
        Err(problem) => error(problem),
    }
}
#[utoipa::path(post,path="/api/agents/{agent_id}/community-bundles",tag="community-bundles",params(("agent_id"=String,Path)),request_body=InstallRequest,
 responses((status=201,description="Disabled MCP metadata and pending skill drafts saved together",body=BundleEnvelope),(status=403,description="Local owner required",body=ErrorBody),(status=409,description="Exact review changed",body=ErrorBody),(status=503,description="Settlement unavailable",body=ErrorBody))) ]
pub(super) async fn install(
    State(state): State<AppState>,
    Path(agent): Path<String>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let input = match body::<InstallRequest>(request).await {
        Ok(input) => input,
        Err(response) => return response,
    };
    one(
        StatusCode::CREATED,
        service(&state).install(&agent, input).await,
    )
}
#[utoipa::path(get,path="/api/agents/{agent_id}/community-bundles",tag="community-bundles",params(("agent_id"=String,Path)),
 responses((status=200,description="Own bounded bundle summaries",body=BundlesEnvelope),(status=403,description="Local owner required",body=ErrorBody),(status=409,description="Workspace required",body=ErrorBody))) ]
pub(super) async fn list(
    State(state): State<AppState>,
    Path(agent): Path<String>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, true) {
        return response;
    }
    match service(&state).list(&agent).await {
        Ok(bundles) => no_store(json_response(StatusCode::OK, &BundlesEnvelope { bundles })),
        Err(problem) => error(problem),
    }
}
#[utoipa::path(get,path="/api/agents/{agent_id}/community-bundles/{bundle_id}",tag="community-bundles",params(("agent_id"=String,Path),("bundle_id"=String,Path)),
 responses((status=200,description="Current exact owned component witnesses",body=BundleEnvelope),(status=403,description="Local owner required",body=ErrorBody),(status=404,description="Own bundle not found",body=ErrorBody))) ]
pub(super) async fn get(
    State(state): State<AppState>,
    Path((agent, id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, true) {
        return response;
    }
    one(StatusCode::OK, service(&state).get(&agent, &id).await)
}
#[utoipa::path(post,path="/api/agents/{agent_id}/community-bundles/{bundle_id}/update",tag="community-bundles",params(("agent_id"=String,Path),("bundle_id"=String,Path)),request_body=UpdateRequest,
 responses((status=200,description="Reviewed metadata replacement saved together",body=BundleEnvelope),(status=409,description="Exact target or component state changed",body=ErrorBody),(status=503,description="Settlement unavailable",body=ErrorBody))) ]
pub(super) async fn update(
    State(state): State<AppState>,
    Path((agent, id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let input = match body::<UpdateRequest>(request).await {
        Ok(input) => input,
        Err(response) => return response,
    };
    one(
        StatusCode::OK,
        service(&state).update(&agent, &id, input).await,
    )
}
#[utoipa::path(patch,path="/api/agents/{agent_id}/community-bundles/{bundle_id}",tag="community-bundles",params(("agent_id"=String,Path),("bundle_id"=String,Path)),request_body=ChangeRequest,
 responses((status=200,description="Exact bundle availability settled without file writes or launches",body=BundleEnvelope),(status=409,description="Exact component state changed",body=ErrorBody),(status=503,description="Physical or durable settlement unavailable",body=ErrorBody))) ]
pub(super) async fn set_enabled(
    State(state): State<AppState>,
    Path((agent, id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let input = match body::<ChangeRequest>(request).await {
        Ok(input) => input,
        Err(response) => return response,
    };
    one(
        StatusCode::OK,
        service(&state).set_enabled(&agent, &id, input).await,
    )
}
#[utoipa::path(delete,path="/api/agents/{agent_id}/community-bundles/{bundle_id}",tag="community-bundles",params(("agent_id"=String,Path),("bundle_id"=String,Path)),request_body=RemoveRequest,
 responses((status=200,description="Owned retirement, trash and durable removal settled",body=RemoveOutcome),(status=409,description="Exact component state changed",body=ErrorBody),(status=503,description="Retained cleanup intent requires reconciliation",body=ErrorBody))) ]
pub(super) async fn remove(
    State(state): State<AppState>,
    Path((agent, id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let input = match body::<RemoveRequest>(request).await {
        Ok(input) => input,
        Err(response) => return response,
    };
    match service(&state).remove(&agent, &id, input).await {
        Ok(outcome) => no_store(json_response(StatusCode::OK, &outcome)),
        Err(problem) => error(problem),
    }
}
