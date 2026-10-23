use super::{
    http::json_response,
    jobs::{authorize, no_store},
    AppState,
};
use axum::extract::{Path, Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

#[utoipa::path(get, path = "/api/agents/{agent_id}/sessions/{session_id}/capabilities", tag = "sessions",
    params(("agent_id" = String, Path), ("session_id" = String, Path)),
    responses((status = 200, body = crate::agent_runs::tool_scope::ChatCapabilities),
        (status = 403, description = "Owner required"), (status = 404, description = "Unknown conversation")))]
pub(super) async fn get(
    State(state): State<AppState>,
    Path((agent, session)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, true) {
        return response;
    }
    match state.agent_runs.chat_capabilities(&agent, &session).await {
        Ok(view) => no_store(json_response(StatusCode::OK, &view)),
        Err(error) => no_store(error.into_response()),
    }
}
