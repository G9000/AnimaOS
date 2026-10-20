use super::{
    contracts::{ErrorBody, RunEnvelope, RunResponse},
    http::json_response,
    jobs::{authorize, body, no_store},
    sessions::rejected,
    AppState,
};
use axum::{
    extract::{Path, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Deserialize;

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct RecoveryInput {
    fingerprint: String,
    #[serde(default)]
    reviewed: bool,
}

#[utoipa::path(get,path="/api/agents/{agent_id}/runs/{run_id}/recovery",tag="runs",params(("agent_id"=String,Path),("run_id"=String,Path)),responses((status=200,body=crate::runs::recovery::RecoverySummary),(status=403,body=ErrorBody),(status=404,body=ErrorBody)))]
pub(super) async fn get(
    State(state): State<AppState>,
    Path((agent, run)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, true) {
        return response;
    }
    match state.agent_runs.recovery_summary(&agent, &run).await {
        Ok(summary) => no_store(json_response(StatusCode::OK, &summary)),
        Err(error) => rejected(error),
    }
}

#[utoipa::path(post,path="/api/agents/{agent_id}/runs/{run_id}/recovery",tag="runs",params(("agent_id"=String,Path),("run_id"=String,Path)),request_body=RecoveryInput,responses((status=202,body=RunEnvelope),(status=200,body=RunEnvelope),(status=400,body=ErrorBody),(status=403,body=ErrorBody),(status=404,body=ErrorBody),(status=409,body=ErrorBody),(status=503,body=ErrorBody)))]
pub(super) async fn continue_run(
    State(state): State<AppState>,
    Path((agent, run)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let input: RecoveryInput = match body(&state, request).await {
        Ok(input) => input,
        Err(response) => return response,
    };
    if input.fingerprint.len() != 64 || !input.fingerprint.bytes().all(|b| b.is_ascii_hexdigit()) {
        return rejected(super::ApiError::bad_request_static(
            "fingerprint must be the current recovery evidence hash",
        ));
    }
    match state
        .agent_runs
        .continue_recovery(agent, run, input.fingerprint, input.reviewed)
        .await
    {
        Ok(accepted) => {
            let (status, run) = match accepted {
                crate::agent_runs::AcceptedRun::Created(run) => (StatusCode::ACCEPTED, run),
                crate::agent_runs::AcceptedRun::Replayed(run) => (StatusCode::OK, run),
                _ => return StatusCode::CONFLICT.into_response(),
            };
            no_store(json_response(
                status,
                &RunEnvelope {
                    run: RunResponse::from(&run),
                    steer: None,
                },
            ))
        }
        Err(error) => rejected(error),
    }
}
