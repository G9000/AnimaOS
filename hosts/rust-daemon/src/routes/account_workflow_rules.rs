//! Local-owner controls for connection events that start saved workflows.
use axum::{
    extract::{Path, Request, State},
    http::StatusCode,
    response::Response,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::{
    jobs::{authorize, body, error_response, no_store},
    json_response, AppState,
};
use crate::{
    account_workflows::{
        AccountWorkflowEvent, AccountWorkflowRule, AccountWorkflowRuleInput,
        AccountWorkflowRulePatch,
    },
    jobs::JobError,
};

#[derive(Serialize, ToSchema)]
pub(super) struct RulesResponse {
    rules: Vec<AccountWorkflowRule>,
}
#[derive(Serialize, ToSchema)]
pub(super) struct EventsResponse {
    events: Vec<AccountWorkflowEvent>,
}
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct DeleteRuleRequest {
    revision: u64,
}

fn response(result: Result<AccountWorkflowRule, JobError>) -> Response {
    match result {
        Ok(rule) => no_store(json_response(StatusCode::OK, &rule)),
        Err(error) => error_response(error),
    }
}

#[utoipa::path(get,path="/api/agents/{agent_id}/account-workflow-rules",tag="workflows",
    params(("agent_id"=String,Path)),responses((status=200,description="Owner account workflow rules",body=RulesResponse),(status=403,description="Local owner required")))]
pub(super) async fn list(
    State(state): State<AppState>,
    Path(agent): Path<String>,
    request: Request,
) -> Response {
    if let Err(error) = authorize(&state, &request, true) {
        return error;
    }
    match state.jobs.list_account_workflow_rules(&agent).await {
        Ok(rules) => no_store(json_response(StatusCode::OK, &RulesResponse { rules })),
        Err(error) => error_response(error),
    }
}

#[utoipa::path(post,path="/api/agents/{agent_id}/account-workflow-rules",tag="workflows",
    params(("agent_id"=String,Path)),request_body=AccountWorkflowRuleInput,
    responses((status=200,description="Rule saved; first fresh snapshot establishes a baseline",body=AccountWorkflowRule),(status=400,description="Invalid bounded filters or bindings"),(status=409,description="Connection, template or capacity conflict"),(status=403,description="Local owner required")))]
pub(super) async fn create(
    State(state): State<AppState>,
    Path(agent): Path<String>,
    request: Request,
) -> Response {
    if let Err(error) = authorize(&state, &request, false) {
        return error;
    }
    let input: AccountWorkflowRuleInput = match body(&state, request).await {
        Ok(input) => input,
        Err(error) => return error,
    };
    response(state.jobs.create_account_workflow_rule(&agent, input).await)
}

#[utoipa::path(patch,path="/api/agents/{agent_id}/account-workflow-rules/{rule_id}",tag="workflows",
    params(("agent_id"=String,Path),("rule_id"=String,Path)),request_body=AccountWorkflowRulePatch,
    responses((status=200,description="Rule revision saved with a fresh pending baseline",body=AccountWorkflowRule),(status=409,description="Stale revision or changed authority"),(status=403,description="Local owner required")))]
pub(super) async fn update(
    State(state): State<AppState>,
    Path((agent, id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(error) = authorize(&state, &request, false) {
        return error;
    }
    let patch: AccountWorkflowRulePatch = match body(&state, request).await {
        Ok(input) => input,
        Err(error) => return error,
    };
    response(
        state
            .jobs
            .update_account_workflow_rule(&agent, &id, patch)
            .await,
    )
}

#[utoipa::path(delete,path="/api/agents/{agent_id}/account-workflow-rules/{rule_id}",tag="workflows",
    params(("agent_id"=String,Path),("rule_id"=String,Path)),request_body=DeleteRuleRequest,
    responses((status=200,description="Rule tombstoned; workflow history retained",body=AccountWorkflowRule),(status=409,description="Stale revision"),(status=403,description="Local owner required")))]
pub(super) async fn delete(
    State(state): State<AppState>,
    Path((agent, id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(error) = authorize(&state, &request, false) {
        return error;
    }
    let input: DeleteRuleRequest = match body(&state, request).await {
        Ok(input) => input,
        Err(error) => return error,
    };
    response(
        state
            .jobs
            .delete_account_workflow_rule(&agent, &id, input.revision)
            .await,
    )
}

#[utoipa::path(get,path="/api/agents/{agent_id}/account-workflow-rules/{rule_id}/events",tag="workflows",
    params(("agent_id"=String,Path),("rule_id"=String,Path)),responses((status=200,description="Durable event admissions linked to saved workflow runs",body=EventsResponse),(status=404,description="No owned rule"),(status=403,description="Local owner required")))]
pub(super) async fn events(
    State(state): State<AppState>,
    Path((agent, id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(error) = authorize(&state, &request, true) {
        return error;
    }
    match state.jobs.account_workflow_events(&agent, &id).await {
        Ok(events) => no_store(json_response(StatusCode::OK, &EventsResponse { events })),
        Err(error) => error_response(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{app::DaemonConfig, state::DaemonState};
    use axum::{body::Body, http::Request as HttpRequest};
    use std::sync::Arc;
    use tokio::sync::RwLock;
    use tower::ServiceExt;

    #[tokio::test]
    async fn account_rule_routes_require_owner_before_reading_or_parsing() {
        let app = super::super::router(
            Arc::new(RwLock::new(DaemonState::new())),
            DaemonConfig::default(),
        );
        for (method, path) in [
            ("GET", "/api/agents/other/account-workflow-rules"),
            ("POST", "/api/agents/other/account-workflow-rules"),
            ("PATCH", "/api/agents/other/account-workflow-rules/rule"),
            ("DELETE", "/api/agents/other/account-workflow-rules/rule"),
            (
                "GET",
                "/api/agents/other/account-workflow-rules/rule/events",
            ),
        ] {
            let result = app
                .clone()
                .oneshot(
                    HttpRequest::builder()
                        .method(method)
                        .uri(path)
                        .header("content-type", "application/json")
                        .body(Body::from("malformed"))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(result.status(), StatusCode::FORBIDDEN, "{method} {path}");
            assert_eq!(result.headers()["cache-control"], "no-store");
        }
    }
}
