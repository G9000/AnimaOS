use super::{
    http::json_response,
    jobs::{authorize, body, no_store},
    AppState,
};
use axum::{
    extract::{Path, Request, State},
    http::StatusCode,
    response::Response,
};
use serde::Deserialize;

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct AcceptanceRequest {
    revision: u64,
    report_sha256: String,
}
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct CloseRequest {
    revision: u64,
}

#[utoipa::path(post,path="/api/agents/{agent_id}/runs/{run_id}/outcome/close",tag="runs",params(("agent_id"=String,Path),("run_id"=String,Path)),request_body=CloseRequest,responses((status=200,body=serde_json::Value),(status=403),(status=409)))]
pub(super) async fn close(
    State(state): State<AppState>,
    Path((agent, run)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let input: CloseRequest = match body(&state, request).await {
        Ok(input) => input,
        Err(response) => return response,
    };
    respond(
        crate::outcomes::close_report(
            &state.daemon,
            state.agent_runs.control_plane_transactions(),
            &agent,
            &run,
            input.revision,
        )
        .await,
    )
}

#[utoipa::path(get, path="/api/agents/{agent_id}/runs/{run_id}/outcome", tag="runs", params(("agent_id"=String,Path),("run_id"=String,Path)), responses((status=200,body=serde_json::Value),(status=403),(status=409)))]
pub(super) async fn get(
    State(state): State<AppState>,
    Path((agent, run)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, true) {
        return response;
    }
    respond(
        crate::outcomes::owner_report(
            &state.daemon,
            state.agent_runs.control_plane_transactions(),
            &agent,
            &run,
            None,
        )
        .await,
    )
}

#[utoipa::path(post, path="/api/agents/{agent_id}/runs/{run_id}/outcome/accept", tag="runs", params(("agent_id"=String,Path),("run_id"=String,Path)), request_body=AcceptanceRequest, responses((status=200,body=serde_json::Value),(status=403),(status=409)))]
pub(super) async fn accept(
    State(state): State<AppState>,
    Path((agent, run)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let input: AcceptanceRequest = match body(&state, request).await {
        Ok(input) => input,
        Err(response) => return response,
    };
    respond(
        crate::outcomes::owner_report(
            &state.daemon,
            state.agent_runs.control_plane_transactions(),
            &agent,
            &run,
            Some((input.revision, input.report_sha256)),
        )
        .await,
    )
}

fn respond(result: Result<anima_core::outcomes::RunOutcome, String>) -> Response {
    match result {
        Ok(outcome) => no_store(json_response(
            StatusCode::OK,
            &serde_json::json!({"outcome":outcome}),
        )),
        Err(error) => no_store(json_response(
            StatusCode::CONFLICT,
            &serde_json::json!({"error":{"message":error}}),
        )),
    }
}
