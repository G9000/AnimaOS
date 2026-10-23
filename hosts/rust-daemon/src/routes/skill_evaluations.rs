//! Local-owner-only evaluation configuration and response-only preview controls.
use super::{
    jobs::authorize,
    skills::{answer, skill_body, skill_error},
    AppState,
};
use crate::skills::{DraftApproval, SkillSupportingFile};
use anima_core::skill_evaluation::EvaluationSuite;
use axum::{
    extract::{Path, Request, State},
    http::StatusCode,
    response::Response,
};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct EvaluationRequest {
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    files: Option<Vec<SkillSupportingFile>>,
    #[serde(default)]
    hash: Option<String>,
}
#[utoipa::path(get, path = "/api/skills/{slug}/evaluation-suite", tag = "skills", params(("slug" = String, Path)), responses((status = 200, description = "Owner configured response-only suite, or null", body = serde_json::Value)))]
pub(super) async fn get_suite(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, true) {
        return response;
    }
    match state.agent_runs.skills().evaluation_suite(&slug).await {
        Ok(suite) => answer(StatusCode::OK, &serde_json::json!({"suite": suite})),
        Err(error) => skill_error(error),
    }
}
#[utoipa::path(put, path = "/api/skills/{slug}/evaluation-suite", tag = "skills", params(("slug" = String, Path)), request_body = serde_json::Value, responses((status = 200, description = "Durably saved owner suite; enabling authorizes bounded model evaluation cost", body = serde_json::Value)))]
pub(super) async fn put_suite(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let suite: EvaluationSuite = match skill_body(&state, request).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    match state
        .agent_runs
        .skills()
        .set_evaluation_suite(&slug, suite)
        .await
    {
        Ok(suite) => answer(StatusCode::OK, &serde_json::json!({"suite": suite})),
        Err(error) => skill_error(error),
    }
}
#[utoipa::path(post, path = "/api/skill-drafts/{draft_id}/evaluation", tag = "skills", params(("draft_id" = String, Path)), request_body = serde_json::Value, responses((status = 202, description = "Bounded response-only preview started or joined; grants no approval or tool permission", body = serde_json::Value)))]
pub(super) async fn start_evaluation(
    State(state): State<AppState>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let input: EvaluationRequest = match skill_body(&state, request).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    match state
        .agent_runs
        .skills()
        .start_evaluation(
            &id,
            DraftApproval {
                body: input.body,
                files: input.files,
                hash: input.hash,
            },
        )
        .await
    {
        Ok(evaluation) => answer(
            StatusCode::ACCEPTED,
            &serde_json::json!({"evaluation": evaluation}),
        ),
        Err(error) => skill_error(error),
    }
}
#[utoipa::path(get, path = "/api/skill-drafts/{draft_id}/evaluation", tag = "skills", params(("draft_id" = String, Path)), responses((status = 200, description = "Latest durable response-only report for this draft and workspace, or null", body = serde_json::Value)))]
pub(super) async fn get_evaluation(
    State(state): State<AppState>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, true) {
        return response;
    }
    match state.agent_runs.skills().draft_evaluation_report(&id).await {
        Ok(evaluation) => answer(
            StatusCode::OK,
            &serde_json::json!({"evaluation": evaluation}),
        ),
        Err(error) => skill_error(error),
    }
}
#[utoipa::path(post, path = "/api/skill-drafts/{draft_id}/evaluation/cancel", tag = "skills", params(("draft_id" = String, Path)), responses((status = 200, description = "Logical cancellation; actual model transport retains custody until retirement", body = serde_json::Value)))]
pub(super) async fn cancel_evaluation(
    State(state): State<AppState>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let skills = state.agent_runs.skills();
    let row = match skills.draft_evaluation_report(&id).await {
        Ok(Some(row)) => row,
        Ok(None) => return skill_error(crate::skills::SkillError::NotFound),
        Err(error) => return skill_error(error),
    };
    match skills.cancel_evaluation(&row.id).await {
        Ok(evaluation) => answer(
            StatusCode::OK,
            &serde_json::json!({"evaluation": evaluation}),
        ),
        Err(error) => skill_error(error),
    }
}
