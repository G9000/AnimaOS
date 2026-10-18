//! How much an agent asks: Free, Strict or Custom, the opt-in smart check,
//! and the owner's own blocked commands. Every route requires the local
//! owner and answers `Cache-Control: no-store`.
use axum::extract::{Path, Request, State};
use axum::http::StatusCode;
use axum::response::Response;
use serde::{Deserialize, Serialize};

use super::contracts::{ApprovalPolicyResponse, ErrorBody};
use super::http::json_response;
use super::jobs::{authorize, body, no_store};
use super::sessions::rejected;
use super::{ApiError, AppState};
use crate::agent_runs::is_helper_config;
use crate::approvals::autonomy::{validate_deny_patterns, AgentAutonomy, AutonomyMode};
use crate::approvals::HELPERS_USE_COMPANION_APPROVALS;

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct AutonomyResponse {
    /// `free`, `strict` or `custom`.
    mode: AutonomyMode,
    smart_check: bool,
    deny_patterns: Vec<String>,
    policy: ApprovalPolicyResponse,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct AutonomyRequest {
    /// `free` or `strict`; `custom` comes from editing the class policy.
    mode: Option<AutonomyMode>,
    smart_check: Option<bool>,
    deny_patterns: Option<Vec<String>>,
}

fn response(record: &AgentAutonomy, policy: &crate::approvals::ApprovalPolicy) -> AutonomyResponse {
    AutonomyResponse {
        mode: record.mode,
        smart_check: record.smart_check,
        deny_patterns: record.deny_patterns.clone(),
        policy: ApprovalPolicyResponse::from(policy),
    }
}

#[utoipa::path(get, path = "/api/agents/{agent_id}/autonomy", tag = "approvals",
    params(("agent_id" = String, Path)),
    responses(
        (status = 200, description = "How much the agent asks; a helper's is its companion's", body = AutonomyResponse),
        (status = 403, description = "Local owner required", body = ErrorBody),
        (status = 404, description = "Agent not found", body = ErrorBody)
    ))]
pub(super) async fn get_autonomy(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
    request: Request,
) -> Response {
    if let Err(response_) = authorize(&state, &request, true) {
        return response_;
    }
    let guard = state.daemon.read().await;
    let Some(runtime) = guard.agents.get(&agent_id) else {
        return rejected(ApiError::not_found());
    };
    let owner = crate::agent_runs::config_helper_parent(runtime.config())
        .unwrap_or(&agent_id)
        .to_string();
    no_store(json_response(
        StatusCode::OK,
        &response(&guard.autonomy_of(&owner), &guard.approvals.policy(&owner)),
    ))
}

#[utoipa::path(put, path = "/api/agents/{agent_id}/autonomy", tag = "approvals",
    params(("agent_id" = String, Path)),
    request_body = AutonomyRequest,
    responses(
        (status = 200, description = "Saved", body = AutonomyResponse),
        (status = 400, description = "Custom mode or invalid blocked commands", body = ErrorBody),
        (status = 403, description = "Local owner required", body = ErrorBody),
        (status = 404, description = "Agent not found", body = ErrorBody),
        (status = 409, description = "A helper, which uses its companion's", body = ErrorBody),
        (status = 503, description = "Could not be saved; nothing changed", body = ErrorBody)
    ))]
pub(super) async fn put_autonomy(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
    request: Request,
) -> Response {
    if let Err(response_) = authorize(&state, &request, false) {
        return response_;
    }
    let input: AutonomyRequest = match body(&state, request).await {
        Ok(input) => input,
        Err(response_) => return response_,
    };
    if input.mode == Some(AutonomyMode::Custom) {
        return rejected(ApiError::bad_request(
            "Custom comes from choosing rules on the Approvals page".to_string(),
        ));
    }
    let deny_patterns = match input.deny_patterns.as_deref().map(validate_deny_patterns) {
        Some(Err(message)) => return rejected(ApiError::bad_request(message)),
        Some(Ok(patterns)) => Some(patterns),
        None => None,
    };
    let operation = tokio::spawn(async move {
        let transaction = state.agent_runs.control_plane_transaction().await;
        let (previous_record, previous_policy, persist) = {
            let mut guard = state.daemon.write().await;
            match guard.agents.get(&agent_id) {
                None => return rejected(ApiError::not_found()),
                Some(runtime) if is_helper_config(runtime.config()) => {
                    return rejected(ApiError::conflict(HELPERS_USE_COMPANION_APPROVALS))
                }
                Some(_) => {}
            }
            let previous_record = guard.autonomy_of(&agent_id);
            let mut record = previous_record.clone();
            let mut previous_policy = None;
            if let Some(mode) = input.mode {
                record.mode = mode;
                if let Some(policy) = mode.preset() {
                    previous_policy = Some(guard.approvals.set_policy(&agent_id, policy));
                }
            }
            if let Some(smart_check) = input.smart_check {
                record.smart_check = smart_check;
            }
            if let Some(patterns) = deny_patterns {
                record.deny_patterns = patterns;
            }
            guard.autonomy.set(record);
            (
                previous_record,
                previous_policy,
                guard.control_plane_persist_request(),
            )
        };
        if let Err(error) = persist.save().await {
            let mut guard = state.daemon.write().await;
            guard.autonomy.set(previous_record);
            if let Some(previous) = previous_policy {
                guard.approvals.restore_policy(&agent_id, previous);
            }
            return rejected(ApiError::service_unavailable(error.to_string()));
        }
        drop(transaction);
        let guard = state.daemon.read().await;
        no_store(json_response(
            StatusCode::OK,
            &response(
                &guard.autonomy_of(&agent_id),
                &guard.approvals.policy(&agent_id),
            ),
        ))
    });
    operation.await.unwrap_or_else(|_| {
        rejected(ApiError::service_unavailable(
            "Autonomy could not be saved".to_string(),
        ))
    })
}
