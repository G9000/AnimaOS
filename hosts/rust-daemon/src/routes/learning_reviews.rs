//! Owner-only, no-store advice controls. Model configuration/evidence are server-owned.
use axum::extract::{Path, Request, State};
use axum::http::StatusCode;
use axum::response::Response;
use serde::de::DeserializeOwned;

use super::contracts::learning_reviews::*;
use super::contracts::ErrorBody;
use super::http::{json_response, read_limited_body};
use super::jobs::{authorize, no_store};
use super::sessions::rejected;
use super::{parse_json_body, ApiError, AppState};
use crate::learning::{LearningPolicyResponse, LearningReviewRecord};

async fn body<T: DeserializeOwned>(state: &AppState, request: Request) -> Result<T, Response> {
    let bytes = read_limited_body(request, state.config.max_request_bytes.min(4096))
        .await
        .map_err(no_store)?;
    parse_json_body(bytes).map_err(rejected)
}
fn review_response(status: StatusCode, result: Result<LearningReviewRecord, ApiError>) -> Response {
    match result {
        Ok(review) => no_store(json_response(status, &LearningReviewEnvelope { review })),
        Err(error) => rejected(error),
    }
}
fn policy_response(result: Result<LearningPolicyResponse, ApiError>) -> Response {
    match result {
        Ok(policy) => no_store(json_response(StatusCode::OK, &policy)),
        Err(error) => rejected(error),
    }
}

#[utoipa::path(post, path = "/api/agents/{agent_id}/runs/{run_id}/learning-review", tag = "learning",
    params(("agent_id" = String, Path), ("run_id" = String, Path)), request_body = LearningReviewRequest,
    responses((status=202, description="Durable review attempt admitted or deduplicated", body=LearningReviewEnvelope),
        (status=400, description="Invalid fields", body=ErrorBody), (status=403, description="Local owner required", body=ErrorBody),
        (status=404, description="Own source unavailable", body=ErrorBody), (status=409, description="Source or quota conflict", body=ErrorBody),
        (status=503, description="Durable review service unavailable", body=ErrorBody)))]
pub(super) async fn request_review(
    State(state): State<AppState>,
    Path((agent_id, run_id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    if let Err(response) = body::<LearningReviewRequest>(&state, request).await {
        return response;
    }
    review_response(
        StatusCode::ACCEPTED,
        state
            .agent_runs
            .learning()
            .request_review(&agent_id, &run_id)
            .await,
    )
}

#[utoipa::path(get, path = "/api/agents/{agent_id}/learning-reviews", tag="learning",
    params(("agent_id"=String, Path)), responses((status=200, description="Own review history", body=LearningReviewsEnvelope),
    (status=403, description="Local owner required", body=ErrorBody), (status=404, description="Agent not found", body=ErrorBody)))]
pub(super) async fn list_reviews(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, true) {
        return response;
    }
    match state.agent_runs.learning().list(&agent_id).await {
        Ok(reviews) => no_store(json_response(
            StatusCode::OK,
            &LearningReviewsEnvelope { reviews },
        )),
        Err(error) => rejected(error),
    }
}

#[utoipa::path(get, path = "/api/agents/{agent_id}/learning-reviews/{review_id}", tag="learning",
    params(("agent_id"=String, Path),("review_id"=String, Path)), responses((status=200, description="Own review detail", body=LearningReviewEnvelope),
    (status=403, description="Local owner required", body=ErrorBody), (status=404, description="Review not found", body=ErrorBody)))]
pub(super) async fn get_review(
    State(state): State<AppState>,
    Path((agent_id, review_id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, true) {
        return response;
    }
    review_response(
        StatusCode::OK,
        state.agent_runs.learning().get(&agent_id, &review_id).await,
    )
}

#[utoipa::path(post, path = "/api/agents/{agent_id}/learning-reviews/{review_id}/cancel", tag="learning",
    params(("agent_id"=String, Path),("review_id"=String, Path)), request_body=LearningReviewRequest,
    responses((status=200, description="Cancellation persisted; physical request may still exit", body=LearningReviewEnvelope),
    (status=400, description="Invalid fields", body=ErrorBody), (status=403, description="Local owner required", body=ErrorBody),
    (status=404, description="Review not found", body=ErrorBody), (status=503, description="Save failed", body=ErrorBody)))]
pub(super) async fn cancel_review(
    State(state): State<AppState>,
    Path((agent_id, review_id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    if let Err(response) = body::<LearningReviewRequest>(&state, request).await {
        return response;
    }
    review_response(
        StatusCode::OK,
        state
            .agent_runs
            .learning()
            .cancel(&agent_id, &review_id)
            .await,
    )
}

#[utoipa::path(patch, path = "/api/agents/{agent_id}/learning-reviews/{review_id}/proposals/{proposal_id}", tag="learning",
    params(("agent_id"=String, Path),("review_id"=String, Path),("proposal_id"=String, Path)), request_body=LearningDecisionRequest,
    responses((status=200, description="Versioned owner decision saved; no fact/skill change", body=LearningReviewEnvelope),
    (status=400, description="Invalid decision", body=ErrorBody), (status=403, description="Local owner required", body=ErrorBody),
    (status=404, description="Proposal not found", body=ErrorBody), (status=409, description="Stale decision version", body=ErrorBody),
    (status=503, description="Save failed", body=ErrorBody)))]
pub(super) async fn decide_proposal(
    State(state): State<AppState>,
    Path((agent_id, review_id, proposal_id)): Path<(String, String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let input: LearningDecisionRequest = match body(&state, request).await {
        Ok(input) => input,
        Err(response) => return response,
    };
    review_response(
        StatusCode::OK,
        state
            .agent_runs
            .learning()
            .decide(
                &agent_id,
                &review_id,
                &proposal_id,
                input.decision,
                input.expected_version,
            )
            .await,
    )
}

#[utoipa::path(post, path = "/api/agents/{agent_id}/learning-reviews/{review_id}/proposals/{proposal_id}/skill-draft", tag="learning",
    params(("agent_id"=String, Path),("review_id"=String, Path),("proposal_id"=String, Path)), request_body=LearningSkillDraftRequest,
    responses((status=201, description="Pending reviewed skill draft; approval remains separate", body=super::contracts::SkillDraftEnvelope),
    (status=200, description="Exact existing handoff; no new write", body=super::contracts::SkillDraftEnvelope),
    (status=400, description="Invalid reviewed content", body=ErrorBody), (status=403, description="Local owner required", body=ErrorBody),
    (status=404, description="Own source unavailable", body=ErrorBody), (status=409, description="Stale source, decision, skill base or different handoff", body=ErrorBody),
    (status=503, description="Save unavailable or uncertain; inspect before retrying", body=ErrorBody)))]
pub(super) async fn adopt_skill(
    State(state): State<AppState>,
    Path((agent_id, review_id, proposal_id)): Path<(String, String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let bytes =
        match read_limited_body(request, state.config.max_request_bytes.min(40 * 1024)).await {
            Ok(bytes) => bytes,
            Err(response) => return no_store(response),
        };
    let input: LearningSkillDraftRequest = match parse_json_body(bytes) {
        Ok(input) => input,
        Err(error) => return rejected(error),
    };
    match state
        .agent_runs
        .learning()
        .adopt_skill(&agent_id, &review_id, &proposal_id, input)
        .await
    {
        Ok((draft, created)) => {
            let current_hash = state
                .daemon
                .read()
                .await
                .skills
                .get(&draft.slug)
                .map(|record| record.approved_hash.clone());
            no_store(json_response(
                if created {
                    StatusCode::CREATED
                } else {
                    StatusCode::OK
                },
                &super::contracts::SkillDraftEnvelope {
                    draft: super::contracts::SkillDraftResponse::of(&draft, current_hash),
                },
            ))
        }
        Err(error) => rejected(error),
    }
}

#[utoipa::path(get, path = "/api/agents/{agent_id}/learning-policy", tag="learning", params(("agent_id"=String, Path)),
    responses((status=200, description="Default-off review policy and server bounds", body=LearningPolicyResponse),
    (status=403, description="Local owner required", body=ErrorBody), (status=404, description="Agent not found", body=ErrorBody)))]
pub(super) async fn get_policy(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, true) {
        return response;
    }
    policy_response(state.agent_runs.learning().policy(&agent_id).await)
}

#[utoipa::path(patch, path = "/api/agents/{agent_id}/learning-policy", tag="learning", params(("agent_id"=String, Path)), request_body=LearningPolicyRequest,
    responses((status=200, description="Separate review opt-in saved; grants unchanged", body=LearningPolicyResponse),
    (status=400, description="Invalid fields", body=ErrorBody), (status=403, description="Local owner required", body=ErrorBody),
    (status=404, description="Agent not found", body=ErrorBody), (status=503, description="Save failed", body=ErrorBody)))]
pub(super) async fn set_policy(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let input: LearningPolicyRequest = match body(&state, request).await {
        Ok(input) => input,
        Err(response) => return response,
    };
    policy_response(
        state
            .agent_runs
            .learning()
            .set_policy(&agent_id, input.auto_enabled)
            .await,
    )
}
