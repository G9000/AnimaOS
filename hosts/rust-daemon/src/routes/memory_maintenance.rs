//! Owner-only controls; model reviews cannot opt in, approve sources or decide.
use super::http::{json_response, read_limited_body};
use super::jobs::{authorize, no_store};
use super::sessions::rejected;
use super::{parse_json_body, ApiError, AppState};
use crate::memory_maintenance::MaintenanceOperation;
use crate::memory_maintenance::{MaintenanceDecision, MaintenancePolicy};
use axum::{
    extract::{Path, Request, State},
    http::StatusCode,
    response::Response,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct PolicyRequest {
    enabled: bool,
    expected_revision: u64,
}
#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct SourceRequest {
    memory_ids: Vec<String>,
}
#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct DecisionRequest {
    expected_revision: u64,
    decision: MaintenanceDecision,
}
#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct UndoRequest {
    expected_revision: u64,
}
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct ReviewRequest {
    /// Optional legacy candidate IDs. Reviewing them does not grant automatic trust.
    #[serde(default, rename = "sourceIds")]
    source_ids: Vec<String>,
}
#[derive(Serialize, ToSchema)]
pub(super) struct Operations {
    operations: Vec<MaintenanceOperation>,
}
async fn body<T: DeserializeOwned>(state: &AppState, request: Request) -> Result<T, Response> {
    let bytes = read_limited_body(request, state.config.max_request_bytes.min(4096))
        .await
        .map_err(no_store)?;
    parse_json_body(bytes).map_err(rejected)
}
fn result<T: Serialize>(status: StatusCode, result: Result<T, ApiError>) -> Response {
    match result {
        Ok(value) => no_store(json_response(status, &value)),
        Err(error) => rejected(error),
    }
}

#[utoipa::path(get, path="/api/agents/{agent_id}/memory-maintenance/policy", tag="learning", params(("agent_id"=String,Path)), responses((status=200,description="Separate default-off maintenance policy",body=MaintenancePolicy),(status=403,description="Owner required")))]
pub(super) async fn policy(
    State(state): State<AppState>,
    Path(agent): Path<String>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, true) {
        return response;
    }
    result(
        StatusCode::OK,
        state.agent_runs.memory_maintenance().policy(&agent).await,
    )
}
#[utoipa::path(patch, path="/api/agents/{agent_id}/memory-maintenance/policy", tag="learning", params(("agent_id"=String,Path)), request_body=PolicyRequest, responses((status=200,description="Policy saved",body=MaintenancePolicy),(status=409,description="Policy changed")))]
pub(super) async fn set_policy(
    State(state): State<AppState>,
    Path(agent): Path<String>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let input: PolicyRequest = match body(&state, request).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    result(
        StatusCode::OK,
        state
            .agent_runs
            .memory_maintenance()
            .set_policy(&agent, input.enabled, input.expected_revision)
            .await,
    )
}
#[utoipa::path(post, path="/api/agents/{agent_id}/memory-maintenance/sources", tag="learning", params(("agent_id"=String,Path)), request_body=SourceRequest, responses((status=200,description="Exact source images approved for maintenance"),(status=409,description="Source unavailable or protected")))]
pub(super) async fn sources(
    State(state): State<AppState>,
    Path(agent): Path<String>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let input: SourceRequest = match body(&state, request).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    result(
        StatusCode::OK,
        state
            .agent_runs
            .memory_maintenance()
            .approve_sources(&agent, &input.memory_ids)
            .await
            .map(|_| serde_json::json!({"approved":true})),
    )
}
#[utoipa::path(get, path="/api/agents/{agent_id}/memory-maintenance/operations", tag="learning", params(("agent_id"=String,Path)), responses((status=200,description="Bounded source review and exact undo history",body=Operations)))]
pub(super) async fn list(
    State(state): State<AppState>,
    Path(agent): Path<String>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, true) {
        return response;
    }
    result(
        StatusCode::OK,
        state
            .agent_runs
            .memory_maintenance()
            .list(&agent)
            .await
            .map(|operations| Operations { operations }),
    )
}
#[utoipa::path(post, path="/api/agents/{agent_id}/memory-maintenance/review", tag="learning", params(("agent_id"=String,Path)), request_body=ReviewRequest, responses((status=202,description="Bounded attempt durably reserved",body=MaintenanceOperation),(status=409,description="Opt-in, cohort or budget unavailable")))]
pub(super) async fn review(
    State(state): State<AppState>,
    Path(agent): Path<String>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let input: ReviewRequest = match body(&state, request).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    result(
        StatusCode::ACCEPTED,
        state
            .agent_runs
            .memory_maintenance()
            .request_sources_review(&agent, false, &input.source_ids)
            .await,
    )
}
#[utoipa::path(patch, path="/api/agents/{agent_id}/memory-maintenance/operations/{operation_id}", tag="learning", params(("agent_id"=String,Path),("operation_id"=String,Path)), request_body=DecisionRequest, responses((status=200,description="Exact owner decision persisted",body=MaintenanceOperation),(status=409,description="Revision or source changed")))]
pub(super) async fn decide(
    State(state): State<AppState>,
    Path((agent, id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let input: DecisionRequest = match body(&state, request).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    result(
        StatusCode::OK,
        state
            .agent_runs
            .memory_maintenance()
            .decide(&agent, &id, input.expected_revision, input.decision)
            .await,
    )
}
#[utoipa::path(post, path="/api/agents/{agent_id}/memory-maintenance/operations/{operation_id}/undo", tag="learning", params(("agent_id"=String,Path),("operation_id"=String,Path)), request_body=UndoRequest, responses((status=200,description="Only this consolidation reversed",body=MaintenanceOperation),(status=409,description="Intervening edits or graph citations prevent undo")))]
pub(super) async fn undo(
    State(state): State<AppState>,
    Path((agent, id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let input: UndoRequest = match body(&state, request).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    result(
        StatusCode::OK,
        state
            .agent_runs
            .memory_maintenance()
            .undo(&agent, &id, input.expected_revision)
            .await,
    )
}
