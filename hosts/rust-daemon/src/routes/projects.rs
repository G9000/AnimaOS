//! Owner-controlled shared project routes; member selectors never authenticate.

#[cfg(test)]
#[path = "project_tests.rs"]
mod tests;

use axum::extract::{Path, Request, State};
use axum::http::{StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::{json, Value};

use super::{
    jobs::{authorize, no_store},
    json_response, ApiError, AppState,
};
use crate::projects::{
    AddResourceRequest, MoveChatRequest, ProjectCreateRequest, ProjectError, ProjectMutation,
    ProjectPatchRequest, ProjectResourceRole, ProjectService,
};

fn service(state: &AppState) -> ProjectService {
    ProjectService::new(
        state.daemon.clone(),
        state.agent_runs.control_plane_transactions(),
    )
}

fn rejected(error: ProjectError) -> Response {
    let error = match error {
        ProjectError::NotFound | ProjectError::NotMember => ApiError::not_found(),
        ProjectError::StaleRevision
        | ProjectError::ScopeChanged
        | ProjectError::AccessRevoked
        | ProjectError::UnresolvedBinding
        | ProjectError::Archived
        | ProjectError::DeletedCreation
        | ProjectError::ReceiptCapacity
        | ProjectError::Busy => ApiError::conflict(error.to_string()),
        ProjectError::AccessUnavailable | ProjectError::Persistence(_) => {
            ApiError::service_unavailable(error.to_string())
        }
        _ => ApiError::bad_request(error.to_string()),
    };
    no_store(error.into_response())
}
fn response(result: Result<Value, ProjectError>) -> Response {
    match result {
        Ok(value) => no_store(json_response(StatusCode::OK, &value)),
        Err(error) => rejected(error),
    }
}
async fn body<T: serde::de::DeserializeOwned>(
    request: Request,
    state: &AppState,
) -> Result<T, Response> {
    let bytes = axum::body::to_bytes(
        request.into_body(),
        state.config.max_request_bytes.min(64 * 1024),
    )
    .await
    .map_err(|_| {
        no_store(
            ApiError::bad_request_static("Project request is malformed or too large")
                .into_response(),
        )
    })?;
    super::parse_json_body(bytes.to_vec()).map_err(|error| no_store(error.into_response()))
}
fn query(uri: &Uri) -> Result<std::collections::HashMap<String, String>, Response> {
    super::http::request_query(uri).map_err(|_| {
        no_store(ApiError::bad_request_static("Project query is invalid").into_response())
    })
}
fn acting_agent(uri: &Uri) -> Result<String, Response> {
    query(uri)?
        .get("asAgentId")
        .filter(|agent| crate::sessions::is_valid_session_id(agent))
        .cloned()
        .ok_or_else(|| {
            no_store(ApiError::bad_request_static("Choose a project member").into_response())
        })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Revision {
    expected_revision: u64,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Members {
    expected_revision: u64,
    agent_ids: Vec<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ResourcePatch {
    expected_revision: u64,
    as_agent_id: String,
    label: Option<String>,
    role: Option<ProjectResourceRole>,
    use_in_context: Option<bool>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ResourceRemove {
    expected_revision: u64,
    as_agent_id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ResourceOrder {
    expected_revision: u64,
    as_agent_id: String,
    resource_ids: Vec<String>,
}

#[utoipa::path(get,path="/api/projects",tag="projects",responses((status=200,description="Current member's project metadata",body=Value),(status=403,description="Owner required")))]
pub(super) async fn list(State(state): State<AppState>, request: Request) -> Response {
    if let Err(error) = authorize(&state, &request, true) {
        return error;
    }
    let params = match query(request.uri()) {
        Ok(value) => value,
        Err(error) => return error,
    };
    let Some(member) = params
        .get("memberAgentId")
        .filter(|agent| crate::sessions::is_valid_session_id(agent))
    else {
        return no_store(
            ApiError::bad_request_static("Choose an agent to list its projects").into_response(),
        );
    };
    let archived = match params.get("status").map(String::as_str) {
        None | Some("active") => false,
        Some("archived") => true,
        _ => {
            return no_store(
                ApiError::bad_request_static("Project status is invalid").into_response(),
            )
        }
    };
    response(service(&state).list(Some(member), archived).await)
}

#[utoipa::path(get,path="/api/projects/manage",tag="projects",responses((status=200,description="Owner metadata management including projects with no members",body=Value)))]
pub(super) async fn manage(State(state): State<AppState>, request: Request) -> Response {
    if let Err(error) = authorize(&state, &request, true) {
        return error;
    }
    response(service(&state).list(None, false).await)
}

#[utoipa::path(post,path="/api/projects",tag="projects",request_body=Value,responses((status=201,description="Shared project created with durable replay receipt",body=Value),(status=409,description="Deleted or mismatched replay, or creation history full")))]
pub(super) async fn create(State(state): State<AppState>, request: Request) -> Response {
    if let Err(error) = authorize(&state, &request, false) {
        return error;
    }
    let input: ProjectCreateRequest = match body(request, &state).await {
        Ok(value) => value,
        Err(error) => return error,
    };
    match service(&state).mutate(ProjectMutation::Create(input)).await {
        Ok(value) => no_store(json_response(
            if value["created"] == true {
                StatusCode::CREATED
            } else {
                StatusCode::OK
            },
            &value,
        )),
        Err(error) => rejected(error),
    }
}

#[utoipa::path(get,path="/api/projects/{project_id}",tag="projects",params(("project_id"=String,Path)),responses((status=200,description="Current member project projection",body=Value),(status=404,description="Unavailable for this member")))]
pub(super) async fn get(
    State(state): State<AppState>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    if let Err(error) = authorize(&state, &request, true) {
        return error;
    }
    let agent = match acting_agent(request.uri()) {
        Ok(value) => value,
        Err(error) => return error,
    };
    response(service(&state).get(&id, &agent).await)
}

#[utoipa::path(patch,path="/api/projects/{project_id}",tag="projects",params(("project_id"=String,Path)),request_body=Value,responses((status=200,description="Revision-checked metadata or instructions saved",body=Value),(status=409,description="Project changed or is working")))]
pub(super) async fn patch(
    State(state): State<AppState>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    if let Err(error) = authorize(&state, &request, false) {
        return error;
    }
    let input: ProjectPatchRequest = match body(request, &state).await {
        Ok(value) => value,
        Err(error) => return error,
    };
    response(
        service(&state)
            .mutate(ProjectMutation::Patch { id, input })
            .await,
    )
}

#[utoipa::path(put,path="/api/projects/{project_id}/members",tag="projects",params(("project_id"=String,Path)),request_body=Value,responses((status=200,description="Membership persisted, old access fenced and removed member chats detached",body=Value),(status=409,description="Stale revision")))]
pub(super) async fn members(
    State(state): State<AppState>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    if let Err(error) = authorize(&state, &request, false) {
        return error;
    }
    let input: Members = match body(request, &state).await {
        Ok(value) => value,
        Err(error) => return error,
    };
    response(
        service(&state)
            .mutate(ProjectMutation::Members {
                id,
                expected_revision: input.expected_revision,
                agents: input.agent_ids,
            })
            .await,
    )
}

#[utoipa::path(delete,path="/api/projects/{project_id}",tag="projects",params(("project_id"=String,Path)),request_body=Value,responses((status=200,description="Project metadata deleted, chats and native content retained",body=Value),(status=409,description="Stale or busy project")))]
pub(super) async fn remove(
    State(state): State<AppState>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    if let Err(error) = authorize(&state, &request, false) {
        return error;
    }
    let input: Revision = match body(request, &state).await {
        Ok(value) => value,
        Err(error) => return error,
    };
    response(
        service(&state)
            .mutate(ProjectMutation::Delete {
                id,
                expected_revision: input.expected_revision,
            })
            .await,
    )
}

#[utoipa::path(post,path="/api/projects/{project_id}/resources",tag="projects",params(("project_id"=String,Path)),request_body=Value,responses((status=200,description="Exact selected source linked after owner/member checks",body=Value)))]
pub(super) async fn add_resource(
    State(state): State<AppState>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    if let Err(error) = authorize(&state, &request, false) {
        return error;
    }
    let input: AddResourceRequest = match body(request, &state).await {
        Ok(value) => value,
        Err(error) => return error,
    };
    response(
        service(&state)
            .mutate(ProjectMutation::AddResource { id, input })
            .await,
    )
}

#[utoipa::path(patch,path="/api/projects/{project_id}/resources/{resource_id}",tag="projects",params(("project_id"=String,Path),("resource_id"=String,Path)),request_body=Value,responses((status=200,description="Selected source preferences saved",body=Value)))]
pub(super) async fn patch_resource(
    State(state): State<AppState>,
    Path((id, resource_id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(error) = authorize(&state, &request, false) {
        return error;
    }
    let input: ResourcePatch = match body(request, &state).await {
        Ok(value) => value,
        Err(error) => return error,
    };
    response(
        service(&state)
            .mutate(ProjectMutation::PatchResource {
                id,
                resource_id,
                as_agent: input.as_agent_id,
                expected_revision: input.expected_revision,
                label: input.label,
                role: input.role,
                use_in_context: input.use_in_context,
            })
            .await,
    )
}

#[utoipa::path(delete,path="/api/projects/{project_id}/resources/{resource_id}",tag="projects",params(("project_id"=String,Path),("resource_id"=String,Path)),request_body=Value,responses((status=200,description="Source unshared and prior access fenced without deleting native content",body=Value)))]
pub(super) async fn remove_resource(
    State(state): State<AppState>,
    Path((id, resource_id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(error) = authorize(&state, &request, false) {
        return error;
    }
    let input: ResourceRemove = match body(request, &state).await {
        Ok(value) => value,
        Err(error) => return error,
    };
    response(
        service(&state)
            .mutate(ProjectMutation::RemoveResource {
                id,
                resource_id,
                as_agent: input.as_agent_id,
                expected_revision: input.expected_revision,
            })
            .await,
    )
}

#[utoipa::path(put,path="/api/projects/{project_id}/resource-order",tag="projects",params(("project_id"=String,Path)),request_body=Value,responses((status=200,description="Complete context source order saved",body=Value)))]
pub(super) async fn resource_order(
    State(state): State<AppState>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    if let Err(error) = authorize(&state, &request, false) {
        return error;
    }
    let input: ResourceOrder = match body(request, &state).await {
        Ok(value) => value,
        Err(error) => return error,
    };
    response(
        service(&state)
            .mutate(ProjectMutation::OrderResources {
                id,
                as_agent: input.as_agent_id,
                expected_revision: input.expected_revision,
                resource_ids: input.resource_ids,
            })
            .await,
    )
}

#[utoipa::path(get,path="/api/projects/{project_id}/resources/{resource_id}/preview",tag="projects",params(("project_id"=String,Path),("resource_id"=String,Path)),responses((status=200,description="Bounded pinned shared projection only",body=Value)))]
pub(super) async fn preview_resource(
    State(state): State<AppState>,
    Path((id, resource_id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(error) = authorize(&state, &request, true) {
        return error;
    }
    let agent = match acting_agent(request.uri()) {
        Ok(value) => value,
        Err(error) => return error,
    };
    response(crate::projects::preview_source(&state.daemon, &id, &agent, &resource_id).await)
}

#[utoipa::path(get,path="/api/projects/{project_id}/context",tag="projects",params(("project_id"=String,Path)),responses((status=200,description="Current selected context coverage, not billed usage",body=Value)))]
pub(super) async fn context(
    State(state): State<AppState>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    if let Err(error) = authorize(&state, &request, true) {
        return error;
    }
    let agent = match acting_agent(request.uri()) {
        Ok(value) => value,
        Err(error) => return error,
    };
    response(
        crate::projects::preview_context(&state.daemon, &id, &agent)
            .await
            .and_then(|value| {
                serde_json::to_value(value).map_err(|_| ProjectError::AccessUnavailable)
            }),
    )
}

#[utoipa::path(get,path="/api/projects/{project_id}/chats",tag="projects",params(("project_id"=String,Path)),responses((status=200,description="Member chat grouping metadata, without transcript previews",body=Value)))]
pub(super) async fn chats(
    State(state): State<AppState>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    if let Err(error) = authorize(&state, &request, true) {
        return error;
    }
    let agent = match acting_agent(request.uri()) {
        Ok(value) => value,
        Err(error) => return error,
    };
    let params = match query(request.uri()) {
        Ok(value) => value,
        Err(error) => return error,
    };
    let limit = match params.get("limit") {
        None => 50,
        Some(value) => match value.parse::<usize>() {
            Ok(value) if (1..=100).contains(&value) => value,
            _ => {
                return no_store(
                    ApiError::bad_request_static("Choose a chat page size from 1 to 100")
                        .into_response(),
                )
            }
        },
    };
    response(
        service(&state)
            .chats(&id, &agent, params.get("cursor").map(String::as_str), limit)
            .await,
    )
}

#[utoipa::path(put,path="/api/agents/{agent_id}/sessions/{session_id}/project",tag="projects",params(("agent_id"=String,Path),("session_id"=String,Path)),request_body=Value,responses((status=200,description="Chat moved without copying transcript or grants",body=Value)))]
pub(super) async fn set_chat_project(
    State(state): State<AppState>,
    Path((agent, session)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(error) = authorize(&state, &request, false) {
        return error;
    }
    let input: MoveChatRequest = match body(request, &state).await {
        Ok(value) => value,
        Err(error) => return error,
    };
    if let Err(error) = service(&state)
        .mutate(ProjectMutation::MoveChat {
            agent: agent.clone(),
            session: session.clone(),
            input,
        })
        .await
    {
        return rejected(error);
    }
    match crate::sessions::views::session_view(&state.daemon, &agent, &session).await {
        Some(view) => no_store(json_response(
            StatusCode::OK,
            &json!({"session":super::contracts::SessionResponse::from(&view)}),
        )),
        None => rejected(ProjectError::NotFound),
    }
}
