//! Owner-only indexed account content and explicit background-read grants.
use super::{
    http::{json_response, read_limited_body},
    AppState,
};
use crate::connected_search::*;
use axum::{
    extract::{Path, Request, State},
    http::{header, HeaderValue, StatusCode},
    response::Response,
};
use serde::Serialize;
#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct SourceResponse {
    source: SourceView,
}
#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct ItemResponse {
    item: SearchItem,
}
fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}
fn error(e: SearchError) -> Response {
    no_store(json_response(
        match e {
            SearchError::Invalid | SearchError::UnsupportedSetting => StatusCode::BAD_REQUEST,
            SearchError::NotFound => StatusCode::NOT_FOUND,
            SearchError::Conflict => StatusCode::CONFLICT,
            _ => StatusCode::SERVICE_UNAVAILABLE,
        },
        &serde_json::json!({"code":match e{SearchError::Invalid=>"invalid_request",SearchError::UnsupportedSetting=>"unsupported_setting",SearchError::NotFound=>"not_found",SearchError::Conflict=>"source_changed",SearchError::Capacity=>"capacity",SearchError::Persistence=>"persistence_unavailable",SearchError::Unavailable=>"source_unavailable"},"error":e.to_string()}),
    ))
}
fn guard(state: &AppState, request: &Request, mutation: bool) -> Result<(), Response> {
    let r = if mutation {
        state.local_owner.authorize(request.headers())
    } else {
        state.local_owner.authorize_read(request.headers())
    };
    r.map_err(|_|no_store(json_response(StatusCode::FORBIDDEN,&serde_json::json!({"code":"owner_required","error":"Local owner authorization required"}))))
}
async fn body<T: serde::de::DeserializeOwned>(
    state: &AppState,
    request: Request,
) -> Result<T, Response> {
    let bytes = read_limited_body(request, state.config.max_request_bytes.min(16384))
        .await
        .map_err(no_store)?;
    serde_json::from_slice(&bytes).map_err(|_| error(SearchError::Invalid))
}

#[utoipa::path(get,path="/api/agents/{agent_id}/connected-search/sources",tag="connectors",params(("agent_id"=String,Path)),responses((status=200,body=SourcesResponse),(status=403),(status=404)))]
pub(super) async fn sources(
    State(s): State<AppState>,
    Path(agent): Path<String>,
    request: Request,
) -> Response {
    if let Err(r) = guard(&s, &request, false) {
        return r;
    }
    match s.connected_search.sources(&agent).await {
        Ok(r) => no_store(json_response(StatusCode::OK, &r)),
        Err(e) => error(e),
    }
}
#[utoipa::path(put,path="/api/agents/{agent_id}/connected-search/sources/{connector_id}",tag="connectors",params(("agent_id"=String,Path),("connector_id"=String,Path)),request_body=SourceInput,responses((status=200,body=SourceResponse),(status=400),(status=403),(status=409)))]
pub(super) async fn configure(
    State(s): State<AppState>,
    Path((agent, id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(r) = guard(&s, &request, true) {
        return r;
    }
    let input = match body(&s, request).await {
        Ok(i) => i,
        Err(r) => return r,
    };
    match s.connected_search.configure(&agent, &id, input).await {
        Ok(source) => no_store(json_response(StatusCode::OK, &SourceResponse { source })),
        Err(e) => error(e),
    }
}
#[utoipa::path(post,path="/api/agents/{agent_id}/connected-search/refresh",tag="connectors",params(("agent_id"=String,Path)),request_body=RefreshInput,responses((status=200,body=SourcesResponse),(status=400),(status=403),(status=404)))]
pub(super) async fn refresh(
    State(s): State<AppState>,
    Path(agent): Path<String>,
    request: Request,
) -> Response {
    if let Err(r) = guard(&s, &request, true) {
        return r;
    }
    let input: RefreshInput = match body(&s, request).await {
        Ok(i) => i,
        Err(r) => return r,
    };
    match s
        .connected_search
        .request_refresh(&agent, input.connector_id.as_deref(), input.work)
        .await
    {
        Ok(r) => no_store(json_response(StatusCode::OK, &r)),
        Err(e) => error(e),
    }
}
#[utoipa::path(post,path="/api/agents/{agent_id}/connected-search/query",tag="connectors",params(("agent_id"=String,Path)),request_body=SearchQuery,responses((status=200,body=SearchResponse),(status=400),(status=403),(status=404)))]
pub(super) async fn search(
    State(s): State<AppState>,
    Path(agent): Path<String>,
    request: Request,
) -> Response {
    if let Err(r) = guard(&s, &request, false) {
        return r;
    }
    let input = match body(&s, request).await {
        Ok(i) => i,
        Err(r) => return r,
    };
    match s.connected_search.query(&agent, &input).await {
        Ok(r) => no_store(json_response(StatusCode::OK, &r)),
        Err(e) => error(e),
    }
}
#[utoipa::path(get,path="/api/agents/{agent_id}/connected-search/items/{citation_id}",tag="connectors",params(("agent_id"=String,Path),("citation_id"=String,Path)),responses((status=200,body=ItemResponse),(status=403),(status=404)))]
pub(super) async fn item(
    State(s): State<AppState>,
    Path((agent, id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(r) = guard(&s, &request, false) {
        return r;
    }
    match s.connected_search.item(&agent, &id).await {
        Ok(item) => no_store(json_response(StatusCode::OK, &ItemResponse { item })),
        Err(e) => error(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower::ServiceExt;
    #[tokio::test]
    async fn owner_guard_precedes_private_state_and_body() {
        let shared =
            std::sync::Arc::new(tokio::sync::RwLock::new(crate::state::DaemonState::new()));
        let app =
            crate::app::app_with_state_for_owner_test(shared, crate::app::DaemonConfig::default());
        for (method, path) in [
            ("GET", "/api/agents/missing/connected-search/sources"),
            (
                "PUT",
                "/api/agents/missing/connected-search/sources/private",
            ),
            ("POST", "/api/agents/missing/connected-search/query"),
            ("POST", "/api/agents/missing/connected-search/refresh"),
            ("GET", "/api/agents/missing/connected-search/items/private"),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(path)
                        .header("host", "127.0.0.1:8080")
                        .header("origin", "https://unapproved.example")
                        .body(axum::body::Body::from("private-body"))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
            assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        }
    }
}
