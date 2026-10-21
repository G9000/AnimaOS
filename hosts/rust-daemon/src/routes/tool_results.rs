use super::{
    jobs::{authorize, no_store},
    json_response, ApiError, AppState,
};
use axum::{
    extract::{Path, Query, Request, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Deserialize;

#[cfg(test)]
#[path = "tool_results/tests.rs"]
mod tests;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PageQuery {
    #[serde(default)]
    offset: usize,
    #[serde(default = "page_limit")]
    limit: usize,
}
fn page_limit() -> usize {
    crate::tool_results::MAX_PAGE_BYTES
}

async fn scope(
    state: &AppState,
    agent: &str,
) -> Result<(crate::tool_results::ToolResultStore, std::path::PathBuf), Response> {
    let guard = state.daemon.read().await;
    if !guard.agents.contains_key(agent) {
        return Err(no_store(ApiError::not_found().into_response()));
    }
    let store = guard.tool_result_store().ok_or_else(|| {
        no_store(
            ApiError::service_unavailable("Durable tool result storage is unavailable")
                .into_response(),
        )
    })?;
    let workspace = guard
        .workspace
        .as_ref()
        .map(|w| w.root_path.clone())
        .ok_or_else(|| {
            no_store(
                ApiError::conflict("Configure a workspace before reading saved tool results")
                    .into_response(),
            )
        })?;
    Ok((store, workspace))
}

#[utoipa::path(get,path="/api/agents/{agent_id}/tool-results/{artifact_id}",tag="tool-results",
    params(("agent_id"=String,Path),("artifact_id"=String,Path),("offset"=Option<usize>,Query,description="UTF-8 byte offset"),("limit"=Option<usize>,Query,description="1 to 8192 bytes")),
    responses((status=200,description="Integrity-checked saved JSON payload page",body=serde_json::Value),(status=403,description="Local owner required"),(status=404,description="No artifact in this agent and workspace"),(status=503,description="Durable storage unavailable")))]
pub(super) async fn get_tool_result(
    State(state): State<AppState>,
    Path((agent, id)): Path<(String, String)>,
    Query(query): Query<PageQuery>,
    request: Request,
) -> Response {
    if let Err(error) = authorize(&state, &request, true) {
        return error;
    }
    let (store, root) = match scope(&state, &agent).await {
        Ok(scope) => scope,
        Err(error) => return error,
    };
    if query.limit == 0 || query.limit > crate::tool_results::MAX_PAGE_BYTES {
        return no_store(
            ApiError::bad_request("Page limit must be 1 to 8192 bytes").into_response(),
        );
    }
    match tokio::task::spawn_blocking(move || {
        store.page(&agent, &root, &id, query.offset, query.limit)
    })
    .await
    {
        Ok(Ok(page)) => no_store(json_response(StatusCode::OK, &page)),
        Ok(Err(_)) => no_store(ApiError::not_found().into_response()),
        Err(_) => {
            no_store(ApiError::service_unavailable("Saved result read failed").into_response())
        }
    }
}

#[utoipa::path(get,path="/api/agents/{agent_id}/tool-results/{artifact_id}/download",tag="tool-results",
    params(("agent_id"=String,Path),("artifact_id"=String,Path)),
    responses((status=200,description="Exact saved JSON payload as private download",body=Vec<u8>,content_type="application/octet-stream"),(status=403,description="Local owner required"),(status=404,description="No artifact in this agent and workspace")))]
pub(super) async fn download_tool_result(
    State(state): State<AppState>,
    Path((agent, id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(error) = authorize(&state, &request, true) {
        return error;
    }
    let (store, root) = match scope(&state, &agent).await {
        Ok(scope) => scope,
        Err(error) => return error,
    };
    match tokio::task::spawn_blocking(move || store.download(&agent, &root, &id)).await {
        Ok(Ok((artifact, bytes))) => {
            let mut response = (StatusCode::OK, bytes).into_response();
            response.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/octet-stream"),
            );
            response.headers_mut().insert(
                header::X_CONTENT_TYPE_OPTIONS,
                HeaderValue::from_static("nosniff"),
            );
            response.headers_mut().insert(
                header::CONTENT_DISPOSITION,
                HeaderValue::from_str(&format!("attachment; filename=\"{}.json\"", artifact.id))
                    .expect("validated opaque id"),
            );
            no_store(response)
        }
        Ok(Err(_)) => no_store(ApiError::not_found().into_response()),
        Err(_) => {
            no_store(ApiError::service_unavailable("Saved result download failed").into_response())
        }
    }
}
