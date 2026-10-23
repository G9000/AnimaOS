use super::{
    jobs::{authorize, no_store},
    json_response, ApiError, AppState,
};
use axum::{
    extract::{Path, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Deserialize;

#[cfg(test)]
#[path = "chat_results/tests.rs"]
mod tests;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SaveRequest {
    clear_generation: u64,
    result: crate::chat_results::SaveResultInput,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PinRequest {
    clear_generation: u64,
    expected_revision: u64,
    pinned: bool,
}

async fn body<T: serde::de::DeserializeOwned>(request: Request) -> Result<T, Response> {
    let bytes = axum::body::to_bytes(
        request.into_body(),
        crate::chat_results::MAX_RESULT_BYTES + 4096,
    )
    .await
    .map_err(|_| {
        no_store(
            ApiError::bad_request("Result request is malformed or exceeds the body limit")
                .into_response(),
        )
    })?;
    super::http::parse_json_body(bytes.to_vec()).map_err(|error| no_store(error.into_response()))
}
fn response(value: Result<crate::chat_results::ChatResult, ApiError>) -> Response {
    match value {
        Ok(result) => no_store(json_response(
            StatusCode::OK,
            &serde_json::json!({"result":result}),
        )),
        Err(error) => no_store(error.into_response()),
    }
}

#[utoipa::path(get, path="/api/agents/{agent_id}/sessions/{session_id}/results", tag="sessions", params(("agent_id"=String,Path),("session_id"=String,Path)), responses((status=200,description="Conversation-owned inline results and pin state",body=serde_json::Value),(status=403,description="Owner required"),(status=404,description="Conversation not found")))]
pub(super) async fn list(
    State(state): State<AppState>,
    Path((agent, session)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(error) = authorize(&state, &request, true) {
        return error;
    }
    // Readers observe only an acknowledged save or its completed rollback.
    let _transaction = state.agent_runs.control_plane_transaction().await;
    let state = state.daemon.read().await;
    if !state.agents.contains_key(&agent) {
        return no_store(ApiError::not_found().into_response());
    }
    let Some(session) = state.sessions.get(&agent, &session) else {
        return no_store(ApiError::not_found().into_response());
    };
    if let Err(error) = crate::chat_results::validate_session(session) {
        return no_store(ApiError::service_unavailable(error).into_response());
    }
    no_store(json_response(
        StatusCode::OK,
        &serde_json::json!({"clearGeneration":session.clear_generation,"results":session.chat_results}),
    ))
}

#[utoipa::path(post, path="/api/agents/{agent_id}/sessions/{session_id}/results", tag="sessions", params(("agent_id"=String,Path),("session_id"=String,Path)), request_body=serde_json::Value, responses((status=200,description="Result saved after durable acknowledgement",body=serde_json::Value),(status=400,description="Invalid closed data payload"),(status=409,description="Revision or generation conflict")))]
pub(super) async fn save(
    State(state): State<AppState>,
    Path((agent, session)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(error) = authorize(&state, &request, false) {
        return error;
    }
    let input: SaveRequest = match body(request).await {
        Ok(v) => v,
        Err(error) => return error,
    };
    response(
        state
            .agent_runs
            .save_chat_result_owner(&agent, &session, input.clear_generation, input.result)
            .await,
    )
}

#[utoipa::path(patch, path="/api/agents/{agent_id}/sessions/{session_id}/results/{result_id}", tag="sessions", params(("agent_id"=String,Path),("session_id"=String,Path),("result_id"=String,Path)), request_body=serde_json::Value, responses((status=200,description="Revision-checked durable pin update",body=serde_json::Value),(status=409,description="Stale revision or conversation generation")))]
pub(super) async fn pin(
    State(state): State<AppState>,
    Path((agent, session, id)): Path<(String, String, String)>,
    request: Request,
) -> Response {
    if let Err(error) = authorize(&state, &request, false) {
        return error;
    }
    let input: PinRequest = match body(request).await {
        Ok(v) => v,
        Err(error) => return error,
    };
    response(
        state
            .agent_runs
            .pin_chat_result_owner(
                &agent,
                &session,
                input.clear_generation,
                &id,
                input.expected_revision,
                input.pinned,
            )
            .await,
    )
}
