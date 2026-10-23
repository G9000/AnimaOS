//! Explicit owner-only second opinions. Reading never starts provider work.
use super::{
    http::json_response,
    jobs::{authorize, body, no_store},
    sessions::rejected,
    ApiError, AppState,
};
use crate::chat_review::{self, ReviewInput};
use axum::{
    extract::{Path, Request, State},
    http::StatusCode,
    response::Response,
};

#[utoipa::path(get, path = "/api/agents/{agent_id}/sessions/{session_id}/reviews", tag = "sessions", params(("agent_id" = String, Path), ("session_id" = String, Path)), responses((status = 200, description = "Durable reviews; never starts provider work"), (status = 403, description = "Local owner required"), (status = 404, description = "Session not found")))]
pub(super) async fn list_reviews(
    State(state): State<AppState>,
    Path((agent, session)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, true) {
        return response;
    }
    let guard = state.daemon.read().await;
    match guard.sessions.get(&agent, &session) {
        Some(record) => no_store(json_response(
            StatusCode::OK,
            &serde_json::json!({"reviews":record.chat_reviews}),
        )),
        None => rejected(ApiError::not_found()),
    }
}

#[utoipa::path(post, path = "/api/agents/{agent_id}/sessions/{session_id}/reviews", tag = "sessions", params(("agent_id" = String, Path), ("session_id" = String, Path)), request_body = ReviewInput, responses((status = 200, description = "Admitted review or exact idempotent result; unavailable is explicit"), (status = 400, description = "Invalid bounded review request"), (status = 403, description = "Local owner required"), (status = 404, description = "Session not found"), (status = 409, description = "Review already running or source unavailable"), (status = 503, description = "Admission could not be saved; no model started")))]
pub(super) async fn request_review(
    State(state): State<AppState>,
    Path((agent, session)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let input: ReviewInput = match body(&state, request).await {
        Ok(input) => input,
        Err(response) => return response,
    };
    let operation = tokio::spawn(async move {
        chat_review::request(&state.daemon, &state.agent_runs, &agent, &session, input).await
    });
    match operation.await {
        Ok(Ok(review)) => no_store(json_response(
            StatusCode::OK,
            &serde_json::json!({"review":review}),
        )),
        Ok(Err(error)) => rejected(error),
        Err(_) => rejected(ApiError::service_unavailable("Review admission failed")),
    }
}

#[utoipa::path(post, path = "/api/agents/{agent_id}/sessions/{session_id}/reviews/{review_id}/cancel", tag = "sessions", params(("agent_id" = String, Path), ("session_id" = String, Path), ("review_id" = String, Path)), responses((status = 200, description = "Cancelled review; completed review unchanged"), (status = 403, description = "Local owner required"), (status = 404, description = "Review not found"), (status = 503, description = "Cancellation could not be saved")))]
pub(super) async fn cancel_review(
    State(state): State<AppState>,
    Path((agent, session, review)): Path<(String, String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let operation = tokio::spawn(async move {
        chat_review::cancel(&state.daemon, &state.agent_runs, &agent, &session, &review).await
    });
    match operation.await {
        Ok(Ok(review)) => no_store(json_response(
            StatusCode::OK,
            &serde_json::json!({"review":review}),
        )),
        Ok(Err(error)) => rejected(error),
        Err(_) => rejected(ApiError::service_unavailable("Review cancellation failed")),
    }
}
