use anima_core::workflows::{
    WorkflowInput, WorkflowResumeAction, WorkflowResumeRequest, WorkflowRun, WorkflowStepKind,
    WorkflowStepSpec,
};
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
use crate::jobs::JobError;

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct WorkflowStepRequest {
    id: String,
    title: String,
    /// task, approval, or input.
    #[schema(value_type=String)]
    kind: WorkflowStepKind,
    prompt: String,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CreateWorkflowRequest {
    title: String,
    request_key: String,
    steps: Vec<WorkflowStepRequest>,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ResumeWorkflowRequest {
    revision: u64,
    /// approve, answer, retry, or resolve. Only the owner can decide checkpoints.
    #[schema(value_type=String)]
    action: WorkflowResumeAction,
    value: Option<String>,
    #[serde(default)]
    acknowledge_uncertain: bool,
}

#[derive(Serialize)]
struct WorkflowsResponse {
    workflows: Vec<WorkflowRun>,
}

fn response(result: Result<WorkflowRun, JobError>) -> Response {
    match result {
        Ok(workflow) => no_store(json_response(StatusCode::OK, &workflow)),
        Err(error) => error_response(error),
    }
}

#[utoipa::path(get, path="/api/agents/{agent_id}/workflows", tag="workflows",
    params(("agent_id"=String,Path)),
    responses((status=200,description="Saved workflow history",body=serde_json::Value),(status=403,description="Local owner required")))]
pub(super) async fn list(
    State(state): State<AppState>,
    Path(agent): Path<String>,
    request: Request,
) -> Response {
    if let Err(error) = authorize(&state, &request, true) {
        return error;
    }
    match state.jobs.list_workflows(&agent).await {
        Ok(workflows) => no_store(json_response(
            StatusCode::OK,
            &WorkflowsResponse { workflows },
        )),
        Err(error) => error_response(error),
    }
}

#[utoipa::path(post, path="/api/agents/{agent_id}/workflows", tag="workflows",
    params(("agent_id"=String,Path)),request_body=CreateWorkflowRequest,
    responses((status=200,description="Saved workflow or identical request replay",body=serde_json::Value),(status=400,description="Invalid definition"),(status=409,description="Request key or capacity conflict"),(status=503,description="Durable storage unavailable")))]
pub(super) async fn create(
    State(state): State<AppState>,
    Path(agent): Path<String>,
    request: Request,
) -> Response {
    if let Err(error) = authorize(&state, &request, false) {
        return error;
    }
    let input: CreateWorkflowRequest = match body(&state, request).await {
        Ok(v) => v,
        Err(error) => return error,
    };
    response(
        state
            .jobs
            .create_workflow(
                &agent,
                WorkflowInput {
                    title: input.title,
                    request_key: input.request_key,
                    steps: input
                        .steps
                        .into_iter()
                        .map(|s| WorkflowStepSpec {
                            id: s.id,
                            title: s.title,
                            kind: s.kind,
                            prompt: s.prompt,
                        })
                        .collect(),
                },
            )
            .await,
    )
}

#[utoipa::path(get, path="/api/agents/{agent_id}/workflows/{workflow_id}", tag="workflows",
    params(("agent_id"=String,Path),("workflow_id"=String,Path)),
    responses((status=200,description="Saved workflow with step results",body=serde_json::Value),(status=404,description="No owned workflow")))]
pub(super) async fn get(
    State(state): State<AppState>,
    Path((agent, id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(error) = authorize(&state, &request, true) {
        return error;
    }
    response(state.jobs.get_workflow(&agent, &id).await)
}

#[utoipa::path(post, path="/api/agents/{agent_id}/workflows/{workflow_id}/resume", tag="workflows",
    params(("agent_id"=String,Path),("workflow_id"=String,Path)),request_body=ResumeWorkflowRequest,
    responses((status=200,description="Owner decision saved; exact decision replay is a no-op",body=serde_json::Value),(status=400,description="Invalid decision payload"),(status=409,description="Stale revision or invalid transition")))]
pub(super) async fn resume(
    State(state): State<AppState>,
    Path((agent, id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(error) = authorize(&state, &request, false) {
        return error;
    }
    let input: ResumeWorkflowRequest = match body(&state, request).await {
        Ok(v) => v,
        Err(error) => return error,
    };
    response(
        state
            .jobs
            .resume_workflow(
                &agent,
                &id,
                WorkflowResumeRequest {
                    revision: input.revision,
                    action: input.action,
                    value: input.value,
                    acknowledge_uncertain: input.acknowledge_uncertain,
                },
            )
            .await,
    )
}

#[utoipa::path(post, path="/api/agents/{agent_id}/workflows/{workflow_id}/cancel", tag="workflows",
    params(("agent_id"=String,Path),("workflow_id"=String,Path)),request_body=super::jobs::CancelJobRequest,
    responses((status=200,description="Workflow cancelled; an active task is signalled to stop",body=serde_json::Value),(status=409,description="Stale revision or completed workflow")))]
pub(super) async fn cancel(
    State(state): State<AppState>,
    Path((agent, id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(error) = authorize(&state, &request, false) {
        return error;
    }
    let input: CancelWorkflowRequest = match body(&state, request).await {
        Ok(v) => v,
        Err(error) => return error,
    };
    response(
        state
            .jobs
            .cancel_workflow(&agent, &id, input.revision)
            .await,
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CancelWorkflowRequest {
    revision: u64,
}
