//! Derived owner views and guarded edits through the existing canonical store.
use axum::extract::{Path, Request, State};
use axum::http::StatusCode;
use axum::response::Response;
use serde::Serialize;
use utoipa::ToSchema;

use super::contracts::{ErrorBody, MemoryPatchRequest};
use super::http::{json_response, read_limited_body, request_query};
use super::jobs::{authorize, no_store};
use super::sessions::rejected;
use super::{parse_json_body, ApiError, AppState};
use crate::memory_views::{
    eligible_source, source_hash, MemoryViewCorrectionRequest, MemoryViewPatchRequest,
    MemoryViewResponse, MemoryViewService, MemoryViewSource,
};

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct MemoryViewCandidates {
    candidates: Vec<MemoryViewSource>,
    has_more: bool,
}
fn answer(result: Result<MemoryViewResponse, ApiError>) -> Response {
    match result {
        Ok(view) => no_store(json_response(StatusCode::OK, &view)),
        Err(error) => rejected(error),
    }
}
fn service(state: &AppState) -> MemoryViewService {
    MemoryViewService::new(
        state.daemon.clone(),
        state.agent_runs.control_plane_transactions(),
    )
}

#[utoipa::path(get, path="/api/agents/{agent_id}/memory-views", tag="memories", params(("agent_id"=String,Path)),
    responses((status=200,description="Current selected canonical sources; no profile content copy",body=MemoryViewResponse),
    (status=403,description="Owner required",body=ErrorBody),(status=404,description="Agent unavailable",body=ErrorBody)))]
pub(super) async fn get_views(
    State(state): State<AppState>,
    Path(agent): Path<String>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, true) {
        return response;
    }
    answer(service(&state).get(&agent).await)
}

#[utoipa::path(get, path="/api/agents/{agent_id}/memory-views/candidates", tag="memories",
    params(("agent_id"=String,Path),("query"=Option<String>,Query,description="At most512 characters; exact substring search")),
    responses((status=200,description="At most50 own eligible saved Facts, plus hasMore",body=MemoryViewCandidates),
    (status=400,description="Invalid query",body=ErrorBody),(status=403,description="Owner required",body=ErrorBody),(status=404,description="Agent unavailable",body=ErrorBody)))]
pub(super) async fn candidates(
    State(state): State<AppState>,
    Path(agent): Path<String>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, true) {
        return response;
    }
    let query = match request_query(request.uri()) {
        Ok(query) => query,
        Err(_) => return rejected(ApiError::bad_request_static("Invalid query")),
    };
    if query.keys().any(|key| key != "query") {
        return rejected(ApiError::bad_request_static("Only query is supported"));
    }
    match service(&state)
        .candidates(&agent, query.get("query").map(String::as_str).unwrap_or(""))
        .await
    {
        Ok((candidates, has_more)) => no_store(json_response(
            StatusCode::OK,
            &MemoryViewCandidates {
                candidates,
                has_more,
            },
        )),
        Err(error) => rejected(error),
    }
}

#[utoipa::path(patch, path="/api/agents/{agent_id}/memory-views", tag="memories", params(("agent_id"=String,Path)),request_body=MemoryViewPatchRequest,
    responses((status=200,description="Reference selection saved or exact duplicate",body=MemoryViewResponse),
    (status=400,description="Invalid picks",body=ErrorBody),(status=403,description="Owner required",body=ErrorBody),(status=404,description="Own source unavailable",body=ErrorBody),
    (status=409,description="Selection version/source changed",body=ErrorBody),(status=503,description="Durable selection unavailable or uncertain",body=ErrorBody)))]
pub(super) async fn select_views(
    State(state): State<AppState>,
    Path(agent): Path<String>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let bytes = match read_limited_body(request, state.config.max_request_bytes.min(8192)).await {
        Ok(bytes) => bytes,
        Err(response) => return no_store(response),
    };
    let input: MemoryViewPatchRequest = match parse_json_body(bytes) {
        Ok(input) => input,
        Err(error) => return rejected(error),
    };
    answer(service(&state).select(&agent, input).await)
}

#[utoipa::path(patch, path="/api/agents/{agent_id}/memory-views/sources/{memory_id}", tag="memories", params(("agent_id"=String,Path),("memory_id"=String,Path)),request_body=MemoryViewCorrectionRequest,
    responses((status=200,description="Selected Fact corrected through canonical mutation; current derived views",body=MemoryViewResponse),
    (status=400,description="Invalid correction",body=ErrorBody),(status=403,description="Owner required",body=ErrorBody),(status=404,description="Own selected source unavailable",body=ErrorBody),
    (status=409,description="Version/hash/full text changed",body=ErrorBody),(status=503,description="Mutation unavailable or uncertain; inspect before retrying",body=ErrorBody)))]
pub(super) async fn correct_source(
    State(state): State<AppState>,
    Path((agent, id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let bytes =
        match read_limited_body(request, state.config.max_request_bytes.min(80 * 1024)).await {
            Ok(bytes) => bytes,
            Err(response) => return no_store(response),
        };
    let input: MemoryViewCorrectionRequest = match parse_json_body(bytes) {
        Ok(input) => input,
        Err(error) => return rejected(error),
    };
    if input.expected_source_hash.len() != 64
        || !input
            .expected_source_hash
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
    {
        return rejected(ApiError::bad_request_static(
            "Source hash must be64 lowercase hex characters",
        ));
    }
    let patch = match super::memory_edits::memory_patch(MemoryPatchRequest {
        content: Some(input.content),
        importance: None,
        tags: None,
    }) {
        Ok(patch) => patch,
        Err(error) => return rejected(error),
    };
    let daemon = state.daemon.clone();
    let transactions = state.agent_runs.control_plane_transactions();
    let changed = tokio::spawn(async move {
        let transaction = transactions.lock().await;
        {
            let guard = daemon.read().await;
            if !guard.agents.contains_key(&agent) {
                return Err(ApiError::not_found());
            }
            if guard.control_plane_store.is_none() || guard.memory_store.is_none() {
                return Err(ApiError::service_unavailable(
                    "Durable memory views are unavailable",
                ));
            }
            let selection = guard.memory_views.selection(&agent);
            if selection.version != input.expected_version {
                return Err(ApiError::conflict("Selection changed; review again"));
            }
            if !selection.user_profile_ids.contains(&id) && !selection.agent_note_ids.contains(&id)
            {
                return Err(ApiError::not_found());
            }
        }
        let owner = agent.clone();
        super::memory_edits::apply_patch_checked(daemon.clone(), id, patch, move |current| {
            if !eligible_source(current, &owner) {
                return Err(ApiError::not_found());
            }
            if current.content != input.expected_content
                || source_hash(current) != input.expected_source_hash
            {
                return Err(ApiError::conflict(
                    "Saved fact changed; read its full source again",
                ));
            }
            Ok(())
        })
        .await?;
        drop(transaction);
        MemoryViewService::new(daemon, transactions)
            .get(&agent)
            .await
    })
    .await
    .unwrap_or_else(|_| {
        Err(ApiError::service_unavailable(
            "Memory correction did not finish; inspect Memory before retrying",
        ))
    });
    answer(changed)
}
