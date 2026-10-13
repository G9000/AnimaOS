//! Owner-scoped MCP connection configuration and review routes.
use axum::extract::{Path, Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::de::DeserializeOwned;

use super::contracts::mcp::*;
use super::contracts::ErrorBody;
use super::http::{json_response, read_limited_body};
use super::jobs::{authorize, no_store};
use super::{parse_json_body, ApiError, AppState};
use crate::mcp::{
    CreateMcpConnection, McpConnectionView, McpServiceError, ReviewMcpTool, UpdateMcpConnection,
};
use anima_core::external_tools::ExternalToolDescriptor;

const MAX_MCP_OWNER_BODY_BYTES: usize = 16 * 1024;

async fn body<T: DeserializeOwned>(state: &AppState, request: Request) -> Result<T, Response> {
    let bytes = read_limited_body(
        request,
        state.config.max_request_bytes.min(MAX_MCP_OWNER_BODY_BYTES),
    )
    .await
    .map_err(no_store)?;
    parse_json_body(bytes).map_err(|error: ApiError| no_store(error.into_response()))
}

fn service_error(error: McpServiceError) -> Response {
    let error = match error {
        McpServiceError::InvalidRequest => ApiError::bad_request_static("invalid MCP request"),
        McpServiceError::NotFound => ApiError::not_found(),
        McpServiceError::Conflict => {
            ApiError::conflict("MCP connection state changed; refresh and review again")
        }
        McpServiceError::Busy => ApiError::too_many_requests("MCP connection is busy"),
        McpServiceError::Unavailable => {
            ApiError::service_unavailable("MCP connection service unavailable")
        }
        McpServiceError::Revoked
        | McpServiceError::Cancelled
        | McpServiceError::TimedOut
        | McpServiceError::OutcomeUnknown => ApiError::service_unavailable(
            "MCP operation did not complete; inspect connection state",
        ),
    };
    no_store(error.into_response())
}

fn descriptor_response(descriptor: &ExternalToolDescriptor) -> McpToolDescriptorResponse {
    McpToolDescriptorResponse {
        remote_name: descriptor.remote_name.clone(),
        description: descriptor.description.clone(),
        input_schema: descriptor.input_schema.clone(),
        output_schema: descriptor.output_schema.clone(),
        descriptor_digest: descriptor.descriptor_digest.clone(),
    }
}

fn connection_response(view: McpConnectionView) -> McpConnectionResponse {
    let state = view.state.as_str().to_owned();
    McpConnectionResponse {
        id: view.record.id,
        agent_id: view.record.agent_id,
        label: view.record.label,
        endpoint: view.record.endpoint,
        enabled: view.record.enabled,
        version: view.record.version,
        reviewed_tools: view
            .record
            .reviewed_tools
            .into_iter()
            .map(|reviewed| ReviewedMcpToolResponse {
                alias: reviewed.alias,
                remote_name: reviewed.descriptor.remote_name,
                description: reviewed.descriptor.description,
                input_schema: reviewed.descriptor.input_schema,
                output_schema: reviewed.descriptor.output_schema,
                descriptor_digest: reviewed.descriptor.descriptor_digest,
            })
            .collect(),
        state,
        wire_generation: view.wire_generation,
        discovery_id: view.discovery_id,
        candidates: view.candidates.iter().map(descriptor_response).collect(),
        physical_calls_active: view.physical_calls_active,
        error: view.error.map(|error| McpStaticErrorResponse {
            code: error.code.to_owned(),
            message: error.message.to_owned(),
        }),
    }
}

fn one(status: StatusCode, result: Result<McpConnectionView, McpServiceError>) -> Response {
    match result {
        Ok(view) => no_store(json_response(
            status,
            &McpConnectionEnvelope {
                connection: connection_response(view),
            },
        )),
        Err(error) => service_error(error),
    }
}

#[utoipa::path(get, path = "/api/agents/{agent_id}/mcp-connections", tag = "mcp",
    params(("agent_id" = String, Path)),
    responses((status = 200, description = "Own connection configuration and reviewed metadata", body = McpConnectionsEnvelope),
        (status = 403, description = "Local owner required", body = ErrorBody),
        (status = 404, description = "Agent not found", body = ErrorBody),
        (status = 503, description = "Connection metadata is temporarily unavailable", body = ErrorBody)))]
pub(super) async fn list(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, true) {
        return response;
    }
    match state.mcp.list(&agent_id).await {
        Ok(views) => no_store(json_response(
            StatusCode::OK,
            &McpConnectionsEnvelope {
                connections: views.into_iter().map(connection_response).collect(),
            },
        )),
        Err(error) => service_error(error),
    }
}

#[utoipa::path(post, path = "/api/agents/{agent_id}/mcp-connections", tag = "mcp",
    params(("agent_id" = String, Path)), request_body = CreateMcpConnectionRequest,
    responses((status = 201, description = "Saved disabled connection; no transport is opened", body = McpConnectionEnvelope),
        (status = 400, description = "Invalid endpoint or request", body = ErrorBody),
        (status = 403, description = "Local owner required", body = ErrorBody),
        (status = 404, description = "Agent not found", body = ErrorBody),
        (status = 429, description = "Connection limit reached", body = ErrorBody),
        (status = 503, description = "Configuration could not be saved", body = ErrorBody)))]
pub(super) async fn create(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let input: CreateMcpConnectionRequest = match body(&state, request).await {
        Ok(input) => input,
        Err(response) => return response,
    };
    one(
        StatusCode::CREATED,
        state
            .mcp
            .create(
                &agent_id,
                CreateMcpConnection {
                    label: input.label,
                    endpoint: input.endpoint,
                },
            )
            .await,
    )
}

#[utoipa::path(get, path = "/api/agents/{agent_id}/mcp-connections/{connection_id}", tag = "mcp",
    params(("agent_id" = String, Path), ("connection_id" = String, Path)),
    responses((status = 200, description = "Own connection metadata", body = McpConnectionEnvelope),
        (status = 403, description = "Local owner required", body = ErrorBody),
        (status = 404, description = "Connection not found", body = ErrorBody),
        (status = 503, description = "Connection metadata is temporarily unavailable", body = ErrorBody)))]
pub(super) async fn get(
    State(state): State<AppState>,
    Path((agent_id, connection_id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, true) {
        return response;
    }
    one(
        StatusCode::OK,
        state.mcp.get(&agent_id, &connection_id).await,
    )
}

#[utoipa::path(patch, path = "/api/agents/{agent_id}/mcp-connections/{connection_id}", tag = "mcp",
    params(("agent_id" = String, Path), ("connection_id" = String, Path)), request_body = UpdateMcpConnectionRequest,
    responses((status = 200, description = "Saved versioned configuration", body = McpConnectionEnvelope),
        (status = 400, description = "Invalid or empty update", body = ErrorBody),
        (status = 403, description = "Local owner required", body = ErrorBody),
        (status = 404, description = "Connection not found", body = ErrorBody),
        (status = 409, description = "Version conflict or connection limit", body = ErrorBody),
        (status = 503, description = "Configuration could not be saved", body = ErrorBody)))]
pub(super) async fn update(
    State(state): State<AppState>,
    Path((agent_id, connection_id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let input: UpdateMcpConnectionRequest = match body(&state, request).await {
        Ok(input) => input,
        Err(response) => return response,
    };
    one(
        StatusCode::OK,
        state
            .mcp
            .update(
                &agent_id,
                &connection_id,
                UpdateMcpConnection {
                    expected_version: input.expected_version,
                    label: input.label,
                    endpoint: input.endpoint,
                    enabled: input.enabled,
                },
            )
            .await,
    )
}

#[utoipa::path(post, path = "/api/agents/{agent_id}/mcp-connections/{connection_id}/connect", tag = "mcp",
    params(("agent_id" = String, Path), ("connection_id" = String, Path)), request_body = McpVersionRequest,
    responses((status = 200, description = "Connected and completely discovered; no tools called", body = McpConnectionEnvelope),
        (status = 400, description = "Invalid request", body = ErrorBody), (status = 403, description = "Local owner required", body = ErrorBody),
        (status = 404, description = "Connection not found", body = ErrorBody), (status = 409, description = "Disabled, stale, or conflicting connection", body = ErrorBody),
        (status = 429, description = "Connection is busy", body = ErrorBody), (status = 503, description = "Connect or discovery unavailable", body = ErrorBody)))]
pub(super) async fn connect(
    State(state): State<AppState>,
    Path((agent_id, connection_id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let input: McpVersionRequest = match body(&state, request).await {
        Ok(input) => input,
        Err(response) => return response,
    };
    one(
        StatusCode::OK,
        state
            .mcp
            .connect(&agent_id, &connection_id, input.expected_version)
            .await,
    )
}

#[utoipa::path(post, path = "/api/agents/{agent_id}/mcp-connections/{connection_id}/discover", tag = "mcp",
    params(("agent_id" = String, Path), ("connection_id" = String, Path)), request_body = McpVersionRequest,
    responses((status = 200, description = "Complete bounded discovery for the current connection", body = McpConnectionEnvelope),
        (status = 400, description = "Invalid request", body = ErrorBody), (status = 403, description = "Local owner required", body = ErrorBody),
        (status = 404, description = "Connection not found", body = ErrorBody), (status = 409, description = "Disconnected or stale connection", body = ErrorBody),
        (status = 429, description = "Connection is busy", body = ErrorBody), (status = 503, description = "Discovery unavailable", body = ErrorBody)))]
pub(super) async fn discover(
    State(state): State<AppState>,
    Path((agent_id, connection_id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let input: McpVersionRequest = match body(&state, request).await {
        Ok(input) => input,
        Err(response) => return response,
    };
    one(
        StatusCode::OK,
        state
            .mcp
            .discover(&agent_id, &connection_id, input.expected_version)
            .await,
    )
}

#[utoipa::path(post, path = "/api/agents/{agent_id}/mcp-connections/{connection_id}/review", tag = "mcp",
    params(("agent_id" = String, Path), ("connection_id" = String, Path)), request_body = ReviewMcpToolRequest,
    responses((status = 200, description = "Reviewed one exact discovered descriptor; grants unchanged", body = McpConnectionEnvelope),
        (status = 400, description = "Invalid request", body = ErrorBody), (status = 403, description = "Local owner required", body = ErrorBody),
        (status = 404, description = "Connection not found", body = ErrorBody), (status = 409, description = "Stale discovery or descriptor", body = ErrorBody),
        (status = 503, description = "Review could not be saved", body = ErrorBody)))]
pub(super) async fn review(
    State(state): State<AppState>,
    Path((agent_id, connection_id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let input: ReviewMcpToolRequest = match body(&state, request).await {
        Ok(input) => input,
        Err(response) => return response,
    };
    one(
        StatusCode::OK,
        state
            .mcp
            .review(
                &agent_id,
                &connection_id,
                ReviewMcpTool {
                    expected_version: input.expected_version,
                    discovery_id: input.discovery_id,
                    descriptor_digest: input.descriptor_digest,
                },
            )
            .await,
    )
}

#[utoipa::path(post, path = "/api/agents/{agent_id}/mcp-connections/{connection_id}/disconnect", tag = "mcp",
    params(("agent_id" = String, Path), ("connection_id" = String, Path)), request_body = McpVersionRequest,
    responses((status = 200, description = "Connection generation retired; remote effects are not undone", body = McpConnectionEnvelope),
        (status = 400, description = "Invalid request", body = ErrorBody), (status = 403, description = "Local owner required", body = ErrorBody),
        (status = 404, description = "Connection not found", body = ErrorBody), (status = 409, description = "Version conflict", body = ErrorBody),
        (status = 429, description = "Connection is busy", body = ErrorBody), (status = 503, description = "Disconnect could not settle", body = ErrorBody)))]
pub(super) async fn disconnect(
    State(state): State<AppState>,
    Path((agent_id, connection_id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let input: McpVersionRequest = match body(&state, request).await {
        Ok(input) => input,
        Err(response) => return response,
    };
    one(
        StatusCode::OK,
        state
            .mcp
            .disconnect(&agent_id, &connection_id, input.expected_version)
            .await,
    )
}

#[utoipa::path(delete, path = "/api/agents/{agent_id}/mcp-connections/{connection_id}", tag = "mcp",
    params(("agent_id" = String, Path), ("connection_id" = String, Path)), request_body = McpVersionRequest,
    responses((status = 204, description = "Connection removed and only its saved aliases revoked"),
        (status = 400, description = "Invalid request", body = ErrorBody), (status = 403, description = "Local owner required", body = ErrorBody),
        (status = 404, description = "Connection not found", body = ErrorBody), (status = 409, description = "Version conflict", body = ErrorBody),
        (status = 503, description = "Removal could not be saved", body = ErrorBody)))]
pub(super) async fn delete(
    State(state): State<AppState>,
    Path((agent_id, connection_id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let input: McpVersionRequest = match body(&state, request).await {
        Ok(input) => input,
        Err(response) => return response,
    };
    match state
        .mcp
        .delete(&agent_id, &connection_id, input.expected_version)
        .await
    {
        Ok(()) => no_store(StatusCode::NO_CONTENT.into_response()),
        Err(error) => service_error(error),
    }
}
