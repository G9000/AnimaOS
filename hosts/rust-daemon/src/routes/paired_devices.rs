//! Local-owner pairing only; normal MCP review and grant routes authorize tools.
use super::{
    http::json_response,
    jobs::{authorize, no_store},
};
use super::{ApiError, AppState};
use crate::{
    mcp::McpServiceError,
    paired_devices::{PairDeviceInput, PairedDevicesService},
};
use axum::{
    extract::{Path, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
fn service(state: &AppState) -> PairedDevicesService {
    PairedDevicesService::new(std::sync::Arc::downgrade(&state.daemon), state.mcp.clone())
}
fn error(error: McpServiceError) -> Response {
    no_store(
        match error {
            McpServiceError::InvalidRequest => {
                ApiError::bad_request_static("invalid paired device request")
            }
            McpServiceError::NotFound => ApiError::not_found(),
            McpServiceError::Conflict => ApiError::conflict(
                "node identity or capability manifest changed; verify and review again",
            ),
            McpServiceError::Busy => ApiError::too_many_requests("paired device limit reached"),
            _ => ApiError::service_unavailable(
                "pairing operation did not settle; inspect paired devices and MCP connections",
            ),
        }
        .into_response(),
    )
}
#[utoipa::path(get, path="/api/agents/{agent_id}/paired-devices", tag="paired-devices", params(("agent_id"=String,Path)), responses((status=200,description="Owner paired nodes; pairing grants no tools",body=serde_json::Value),(status=403,description="Local owner required"),(status=503,description="Pair metadata unavailable")))]
pub(super) async fn list(
    State(state): State<AppState>,
    Path(agent): Path<String>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, true) {
        return response;
    }
    match service(&state).list(&agent).await {
        Ok(records) => no_store(json_response(
            StatusCode::OK,
            &serde_json::json!({"devices":records}),
        )),
        Err(e) => error(e),
    }
}
#[utoipa::path(post, path="/api/agents/{agent_id}/paired-devices", tag="paired-devices", params(("agent_id"=String,Path)), request_body=serde_json::Value, responses((status=201,description="Identity and digest verified; use normal MCP review and grant APIs",body=serde_json::Value),(status=403,description="Local owner required"),(status=409,description="Identity or manifest mismatch"),(status=503,description="Pairing did not settle")))]
pub(super) async fn pair(
    State(state): State<AppState>,
    Path(agent): Path<String>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let read = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        axum::body::to_bytes(request.into_body(), 16 * 1024),
    )
    .await;
    let bytes = match read {
        Ok(Ok(bytes)) => zeroize::Zeroizing::new(bytes.to_vec()),
        _ => return error(McpServiceError::InvalidRequest),
    };
    let input = match serde_json::from_slice::<PairDeviceInput>(&bytes) {
        Ok(input) => input,
        Err(_) => return error(McpServiceError::InvalidRequest),
    };
    match service(&state).pair(&agent, input).await {
        Ok(outcome) => no_store(json_response(StatusCode::CREATED, &outcome)),
        Err(e) => error(e),
    }
}
#[utoipa::path(post, path="/api/agents/{agent_id}/paired-devices/{device_id}/revoke", tag="paired-devices", params(("agent_id"=String,Path),("device_id"=String,Path)), responses((status=200,description="Local aliases and credentials removed; remote confirmation reported separately",body=serde_json::Value),(status=403,description="Local owner required"),(status=404,description="Paired device not found"),(status=503,description="Revocation did not settle")))]
pub(super) async fn revoke(
    State(state): State<AppState>,
    Path((agent, id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    match service(&state).revoke(&agent, &id).await {
        Ok(record) => no_store(json_response(
            StatusCode::OK,
            &serde_json::json!({"device":record}),
        )),
        Err(e) => error(e),
    }
}
