//! Owner metadata routes; none opens a browser session or grants a tool.
use super::{
    contracts::ErrorBody,
    http::{json_response, read_limited_body},
    jobs::{authorize, no_store},
    parse_json_body, ApiError, AppState,
};
use crate::browser::*;
use axum::extract::{Path, Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::de::DeserializeOwned;

async fn body<T: DeserializeOwned>(state: &AppState, request: Request) -> Result<T, Response> {
    let bytes = read_limited_body(request, state.config.max_request_bytes.min(8 * 1024))
        .await
        .map_err(no_store)?;
    parse_json_body(bytes).map_err(|error: ApiError| no_store(error.into_response()))
}
fn error(error: BrowserServiceError) -> Response {
    no_store(
        match error {
            BrowserServiceError::Invalid => {
                ApiError::bad_request_static("invalid browser scope request")
            }
            BrowserServiceError::NotFound => ApiError::not_found(),
            BrowserServiceError::Conflict => ApiError::conflict(
                "browser scope changed or run unavailable; inspect current state",
            ),
            BrowserServiceError::Busy => {
                ApiError::too_many_requests("browser scope capacity reached")
            }
            BrowserServiceError::Unavailable => {
                ApiError::service_unavailable("browser service unavailable")
            }
        }
        .into_response(),
    )
}
fn one(result: Result<Option<BrowserScopeView>, BrowserServiceError>) -> Response {
    match result {
        Ok(scope) => no_store(json_response(
            StatusCode::OK,
            &BrowserScopeEnvelope { scope },
        )),
        Err(e) => error(e),
    }
}
fn saved(result: Result<BrowserScopeView, BrowserServiceError>) -> Response {
    match result {
        Ok(scope) => no_store(json_response(
            StatusCode::OK,
            &BrowserSavedScopeEnvelope { scope },
        )),
        Err(e) => error(e),
    }
}
#[utoipa::path(get, path="/api/agents/{agent_id}/runs/{run_id}/browser-scope",tag="browser",
    params(("agent_id"=String,Path),("run_id"=String,Path)),
    responses((status=200,description="Current ephemeral scope; no worker opens",body=BrowserScopeEnvelope),(status=403,description="Owner required",body=ErrorBody),(status=404,description="Run not found",body=ErrorBody)))]
pub(super) async fn get(
    State(state): State<AppState>,
    Path((agent, run)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(r) = authorize(&state, &request, true) {
        return r;
    }
    one(state.browser.get(&agent, &run).await)
}
#[utoipa::path(put, path="/api/agents/{agent_id}/runs/{run_id}/browser-scope",tag="browser",
    params(("agent_id"=String,Path),("run_id"=String,Path)),request_body=SaveBrowserScope,
    responses((status=200,description="Versioned scope metadata; no grant or worker opens",body=BrowserSavedScopeEnvelope),(status=400,description="Invalid input",body=ErrorBody),(status=403,description="Owner required",body=ErrorBody),(status=404,description="Run not found",body=ErrorBody),(status=409,description="Stale or unavailable run",body=ErrorBody),(status=503,description="Policy/service unavailable",body=ErrorBody)))]
pub(super) async fn save(
    State(state): State<AppState>,
    Path((agent, run)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(r) = authorize(&state, &request, false) {
        return r;
    }
    let input = match body::<SaveBrowserScope>(&state, request).await {
        Ok(input) => input,
        Err(e) => return e,
    };
    saved(state.browser.save(&agent, &run, input).await)
}
#[utoipa::path(post, path="/api/agents/{agent_id}/runs/{run_id}/browser-scope/stop",tag="browser",
    params(("agent_id"=String,Path),("run_id"=String,Path)),request_body=StopBrowserScope,
    responses((status=200,description="Owner retirement without tool approval",body=BrowserSavedScopeEnvelope),(status=400,description="Invalid version",body=ErrorBody),(status=403,description="Owner required",body=ErrorBody),(status=404,description="Scope not found",body=ErrorBody),(status=409,description="Stale version",body=ErrorBody)))]
pub(super) async fn stop(
    State(state): State<AppState>,
    Path((agent, run)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(r) = authorize(&state, &request, false) {
        return r;
    }
    let input = match body::<StopBrowserScope>(&state, request).await {
        Ok(input) => input,
        Err(e) => return e,
    };
    saved(
        state
            .browser
            .stop(&agent, &run, input.expected_version)
            .await,
    )
}
#[utoipa::path(get,path="/api/agents/{agent_id}/browser-scopes",tag="browser",params(("agent_id"=String,Path)),
    responses((status=200,description="Own bounded ephemeral scopes",body=BrowserScopesEnvelope),(status=403,description="Owner required",body=ErrorBody),(status=404,description="Owner not found",body=ErrorBody)))]
pub(super) async fn list(
    State(state): State<AppState>,
    Path(agent): Path<String>,
    request: Request,
) -> Response {
    if let Err(r) = authorize(&state, &request, true) {
        return r;
    }
    match state.browser.list(&agent).await {
        Ok(scopes) => no_store(json_response(
            StatusCode::OK,
            &BrowserScopesEnvelope { scopes },
        )),
        Err(e) => error(e),
    }
}
#[utoipa::path(get,path="/api/agents/{agent_id}/browser-policies",tag="browser",params(("agent_id"=String,Path)),
    responses((status=200,description="Immutable installed operator policy metadata",body=BrowserPoliciesEnvelope),(status=403,description="Owner required",body=ErrorBody),(status=404,description="Owner not found",body=ErrorBody)))]
pub(super) async fn policies(
    State(state): State<AppState>,
    Path(agent): Path<String>,
    request: Request,
) -> Response {
    if let Err(r) = authorize(&state, &request, true) {
        return r;
    }
    match state.browser.policies(&agent).await {
        Ok(policies) => no_store(json_response(
            StatusCode::OK,
            &BrowserPoliciesEnvelope { policies },
        )),
        Err(e) => error(e),
    }
}

fn profile_result(result: Result<BrowserProfileView, BrowserServiceError>) -> Response {
    match result {
        Ok(profile) => no_store(json_response(
            StatusCode::OK,
            &BrowserProfileEnvelope { profile },
        )),
        Err(e) => error(e),
    }
}
#[utoipa::path(get,path="/api/agents/{agent_id}/browser-profiles",tag="browser",params(("agent_id"=String,Path)),responses((status=200,description="Owner dedicated browser profiles; credentials are never returned",body=BrowserProfilesEnvelope)))]
pub(super) async fn profiles(
    State(state): State<AppState>,
    Path(agent): Path<String>,
    request: Request,
) -> Response {
    if let Err(r) = authorize(&state, &request, true) {
        return r;
    }
    match state.browser.list_profiles(&agent).await {
        Ok(profiles) => no_store(json_response(
            StatusCode::OK,
            &BrowserProfilesEnvelope { profiles },
        )),
        Err(e) => error(e),
    }
}
#[utoipa::path(post,path="/api/agents/{agent_id}/browser-profiles",tag="browser",params(("agent_id"=String,Path)),request_body=CreateBrowserProfile,responses((status=200,description="Dedicated exact-origin profile created without opening a browser",body=BrowserProfileEnvelope)))]
pub(super) async fn create_profile(
    State(state): State<AppState>,
    Path(agent): Path<String>,
    request: Request,
) -> Response {
    if let Err(r) = authorize(&state, &request, false) {
        return r;
    }
    let input = match body::<CreateBrowserProfile>(&state, request).await {
        Ok(v) => v,
        Err(e) => return e,
    };
    profile_result(state.browser.create_profile(&agent, input).await)
}
#[utoipa::path(post,path="/api/agents/{agent_id}/browser-profiles/{profile_id}/sign-in/start",tag="browser",params(("agent_id"=String,Path),("profile_id"=String,Path)),request_body=StartBrowserLogin,responses((status=200,description="Dedicated visible owner sign-in browser opened",body=BrowserProfileEnvelope)))]
pub(super) async fn start_profile_login(
    State(state): State<AppState>,
    Path((agent, id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(r) = authorize(&state, &request, false) {
        return r;
    }
    let input = match body::<StartBrowserLogin>(&state, request).await {
        Ok(v) => v,
        Err(e) => return e,
    };
    profile_result(state.browser.start_profile_login(&agent, &id, input).await)
}
#[utoipa::path(post,path="/api/agents/{agent_id}/browser-profiles/{profile_id}/sign-in/finish",tag="browser",params(("agent_id"=String,Path),("profile_id"=String,Path)),request_body=BrowserProfileMutation,responses((status=200,description="Owner confirmed sign-in sealed and browser physically closed",body=BrowserProfileEnvelope)))]
pub(super) async fn finish_profile_login(
    State(state): State<AppState>,
    Path((agent, id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(r) = authorize(&state, &request, false) {
        return r;
    }
    let input = match body::<BrowserProfileMutation>(&state, request).await {
        Ok(v) => v,
        Err(e) => return e,
    };
    profile_result(state.browser.finish_profile_login(&agent, &id, input).await)
}
#[utoipa::path(post,path="/api/agents/{agent_id}/browser-profiles/{profile_id}/revoke",tag="browser",params(("agent_id"=String,Path),("profile_id"=String,Path)),request_body=BrowserProfileMutation,responses((status=200,description="Profile revoked and all owned browser workers closed",body=BrowserProfileEnvelope)))]
pub(super) async fn revoke_profile(
    State(state): State<AppState>,
    Path((agent, id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(r) = authorize(&state, &request, false) {
        return r;
    }
    let input = match body::<BrowserProfileMutation>(&state, request).await {
        Ok(v) => v,
        Err(e) => return e,
    };
    profile_result(state.browser.revoke_profile(&agent, &id, input).await)
}

#[utoipa::path(get,path="/api/agents/{agent_id}/browser-setup",tag="browser",params(("agent_id"=String,Path)),responses((status=200,description="Maintained browser installation status",body=BrowserSetupView)))]
pub(super) async fn setup(
    State(state): State<AppState>,
    Path(agent): Path<String>,
    request: Request,
) -> Response {
    if let Err(r) = authorize(&state, &request, true) {
        return r;
    }
    match state.browser.setup_status(&agent).await {
        Ok(view) => no_store(json_response(StatusCode::OK, &view)),
        Err(e) => error(e),
    }
}

#[utoipa::path(post,path="/api/agents/{agent_id}/browser-destinations",tag="browser",params(("agent_id"=String,Path)),request_body=CreateBrowserDestination,responses((status=200,description="Owner reviewed destination pinned without opening a browser",body=BrowserDestinationEnvelope)))]
pub(super) async fn create_destination(
    State(state): State<AppState>,
    Path(agent): Path<String>,
    request: Request,
) -> Response {
    if let Err(r) = authorize(&state, &request, false) {
        return r;
    }
    let input = match body::<CreateBrowserDestination>(&state, request).await {
        Ok(input) => input,
        Err(e) => return e,
    };
    match state.browser.create_destination(&agent, input).await {
        Ok(policy) => no_store(json_response(
            StatusCode::OK,
            &BrowserDestinationEnvelope { policy },
        )),
        Err(e) => error(e),
    }
}

#[utoipa::path(post,path="/api/agents/{agent_id}/browser-destinations/{destination_id}/revoke",tag="browser",params(("agent_id"=String,Path),("destination_id"=String,Path)),request_body=StopBrowserScope,responses((status=200,description="Destination revoked and owned scopes retired",body=BrowserDestinationRevoked)))]
pub(super) async fn revoke_destination(
    State(state): State<AppState>,
    Path((agent, destination)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(r) = authorize(&state, &request, false) {
        return r;
    }
    let input = match body::<StopBrowserScope>(&state, request).await {
        Ok(input) => input,
        Err(e) => return e,
    };
    match state
        .browser
        .revoke_destination(&agent, &destination, input.expected_version)
        .await
    {
        Ok(()) => no_store(json_response(
            StatusCode::OK,
            &BrowserDestinationRevoked { revoked: true },
        )),
        Err(e) => error(e),
    }
}

#[utoipa::path(post,path="/api/agents/{agent_id}/runs/{run_id}/browser-scope/takeover",tag="browser",params(("agent_id"=String,Path),("run_id"=String,Path)),request_body=StopBrowserScope,responses((status=200,description="Owner takes the same dedicated page at an action boundary",body=BrowserSavedScopeEnvelope)))]
pub(super) async fn takeover(
    State(state): State<AppState>,
    Path((agent, run)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(r) = authorize(&state, &request, false) {
        return r;
    }
    let input = match body::<StopBrowserScope>(&state, request).await {
        Ok(input) => input,
        Err(e) => return e,
    };
    saved(
        state
            .browser
            .owner_control(&agent, &run, input.expected_version, true)
            .await,
    )
}

#[utoipa::path(post,path="/api/agents/{agent_id}/runs/{run_id}/browser-scope/resume",tag="browser",params(("agent_id"=String,Path),("run_id"=String,Path)),request_body=StopBrowserScope,responses((status=200,description="Owner returns page with fresh observation required",body=BrowserSavedScopeEnvelope)))]
pub(super) async fn resume(
    State(state): State<AppState>,
    Path((agent, run)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(r) = authorize(&state, &request, false) {
        return r;
    }
    let input = match body::<StopBrowserScope>(&state, request).await {
        Ok(input) => input,
        Err(e) => return e,
    };
    saved(
        state
            .browser
            .owner_control(&agent, &run, input.expected_version, false)
            .await,
    )
}
