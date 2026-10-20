use super::{
    contracts::{
        interactions::{
            ClarificationAnswerInput, ClarificationEnvelope, ClarificationsEnvelope,
            SideQuestionInput,
        },
        ErrorBody,
    },
    http::json_response,
    jobs::{authorize, body, no_store},
    sessions::rejected,
    AppState,
};
use axum::{
    extract::{Path, Request, State},
    http::StatusCode,
    response::Response,
};

#[utoipa::path(post, path = "/api/agents/{agent_id}/sessions/{session_id}/side-question", tag = "sessions", params(("agent_id" = String, Path), ("session_id" = String, Path)), request_body = SideQuestionInput, responses((status = 200, description = "Text-only detached answer from a bounded conversation snapshot", body = crate::agent_runs::SideQuestionAnswer), (status = 400, description = "Invalid question", body = ErrorBody), (status = 403, description = "Owner required", body = ErrorBody), (status = 404, description = "Owned chat not found", body = ErrorBody), (status = 429, description = "Side question capacity full", body = ErrorBody), (status = 503, description = "Model unavailable", body = ErrorBody)))]
pub(super) async fn side_question(
    State(state): State<AppState>,
    Path((agent, session)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let input: SideQuestionInput = match body(&state, request).await {
        Ok(input) => input,
        Err(response) => return response,
    };
    match state
        .agent_runs
        .side_question(&agent, &session, &input.question)
        .await
    {
        Ok(answer) => no_store(json_response(StatusCode::OK, &answer)),
        Err(error) => rejected(error),
    }
}

#[utoipa::path(get, path = "/api/agents/{agent_id}/sessions/{session_id}/clarifications", tag = "sessions", params(("agent_id" = String, Path), ("session_id" = String, Path)), responses((status = 200, description = "Saved questions and answers, including pending interrupted questions", body = ClarificationsEnvelope), (status = 403, description = "Owner required", body = ErrorBody), (status = 404, description = "Owned session not found", body = ErrorBody)))]
pub(super) async fn list(
    State(state): State<AppState>,
    Path((agent, session)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, true) {
        return response;
    }
    match state.agent_runs.list_clarifications(&agent, &session).await {
        Ok(clarifications) => no_store(json_response(
            StatusCode::OK,
            &ClarificationsEnvelope { clarifications },
        )),
        Err(error) => rejected(error),
    }
}

#[utoipa::path(post, path = "/api/agents/{agent_id}/sessions/{session_id}/clarifications/{request_id}/answer", tag = "sessions", params(("agent_id" = String, Path), ("session_id" = String, Path), ("request_id" = String, Path)), request_body = ClarificationAnswerInput, responses((status = 200, description = "Durably saved answer; interrupted work continues only through recovery review", body = ClarificationEnvelope), (status = 400, description = "Invalid answer IDs or values", body = ErrorBody), (status = 403, description = "Owner required", body = ErrorBody), (status = 404, description = "Exact owned question not found", body = ErrorBody), (status = 409, description = "Question changed or run stopped", body = ErrorBody), (status = 503, description = "Answer was not saved", body = ErrorBody)))]
pub(super) async fn answer(
    State(state): State<AppState>,
    Path((agent, session, id)): Path<(String, String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let input: ClarificationAnswerInput = match body(&state, request).await {
        Ok(input) => input,
        Err(response) => return response,
    };
    match state
        .agent_runs
        .answer_clarification(agent, session, id, input.revision, input.answers)
        .await
    {
        Ok(clarification) => no_store(json_response(
            StatusCode::OK,
            &ClarificationEnvelope { clarification },
        )),
        Err(error) => rejected(error),
    }
}
