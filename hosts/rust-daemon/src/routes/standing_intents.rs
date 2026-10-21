//! Authenticated owner inspection and explicit management of conversational intentions.
use super::http::{json_response, read_limited_body};
use super::jobs::{authorize, no_store};
use super::sessions::rejected;
use super::{parse_json_body, ApiError, AppState};
use axum::{
    extract::{Path, Request, State},
    http::StatusCode,
    response::Response,
};
use serde::Deserialize;
use utoipa::ToSchema;
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateIntentRequest {
    pub(super) spec: serde_json::Value,
}
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdateIntentRequest {
    pub(super) revision: u64,
    pub(super) spec: serde_json::Value,
}
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct CancelIntentRequest {
    pub(super) revision: u64,
}
#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct UndoIntentRequest {
    pub(super) change_id: String,
}
fn result<T: serde::Serialize>(r: Result<T, ApiError>) -> Response {
    match r {
        Ok(v) => no_store(json_response(StatusCode::OK, &v)),
        Err(e) => rejected(e),
    }
}
fn spec(v: serde_json::Value) -> Result<anima_core::standing_intents::IntentSpec, ApiError> {
    serde_json::from_value(v)
        .map_err(|_| ApiError::bad_request_static("Invalid standing intention specification"))
}
#[utoipa::path(get,path="/api/agents/{agent_id}/standing-intents",tag="agents",params(("agent_id"=String,Path)),responses((status=200,description="Bounded saved intentions, revisions, fire history and Undo handles")))]
pub(super) async fn list(State(s): State<AppState>, Path(a): Path<String>, r: Request) -> Response {
    if let Err(r) = authorize(&s, &r, true) {
        return r;
    }
    result(s.agent_runs.standing_intents().list(&a).await)
}
#[utoipa::path(post,path="/api/agents/{agent_id}/standing-intents",tag="agents",params(("agent_id"=String,Path)),request_body=CreateIntentRequest,responses((status=200,description="Owner instruction durably saved")))]
pub(super) async fn create(
    State(s): State<AppState>,
    Path(a): Path<String>,
    r: Request,
) -> Response {
    if let Err(r) = authorize(&s, &r, false) {
        return r;
    }
    let bytes = match read_limited_body(r, 16384).await {
        Ok(b) => b,
        Err(r) => return no_store(r),
    };
    let out = async {
        let q: CreateIntentRequest = parse_json_body(bytes)?;
        s.agent_runs
            .standing_intents()
            .create(a, spec(q.spec)?, None)
            .await
    }
    .await;
    result(out)
}
#[utoipa::path(put,path="/api/agents/{agent_id}/standing-intents/{intent_id}",tag="agents",params(("agent_id"=String,Path),("intent_id"=String,Path)),request_body=UpdateIntentRequest,responses((status=200,description="Exact-revision owner edit saved"),(status=409,description="Intention changed")))]
pub(super) async fn update(
    State(s): State<AppState>,
    Path((a, id)): Path<(String, String)>,
    r: Request,
) -> Response {
    if let Err(r) = authorize(&s, &r, false) {
        return r;
    }
    let bytes = match read_limited_body(r, 16384).await {
        Ok(b) => b,
        Err(r) => return no_store(r),
    };
    let out = async {
        let q: UpdateIntentRequest = parse_json_body(bytes)?;
        s.agent_runs
            .standing_intents()
            .update(a, id, q.revision, spec(q.spec)?, None)
            .await
    }
    .await;
    result(out)
}
#[utoipa::path(post,path="/api/agents/{agent_id}/standing-intents/{intent_id}/cancel",tag="agents",params(("agent_id"=String,Path),("intent_id"=String,Path)),request_body=CancelIntentRequest,responses((status=200,description="Intention cancelled; any committed run retains normal Stop"),(status=409,description="Intention changed")))]
pub(super) async fn cancel(
    State(s): State<AppState>,
    Path((a, id)): Path<(String, String)>,
    r: Request,
) -> Response {
    if let Err(r) = authorize(&s, &r, false) {
        return r;
    }
    let bytes = match read_limited_body(r, 1024).await {
        Ok(b) => b,
        Err(r) => return no_store(r),
    };
    let out = async {
        let q: CancelIntentRequest = parse_json_body(bytes)?;
        s.agent_runs
            .standing_intents()
            .cancel(a, id, q.revision, None)
            .await
    }
    .await;
    result(out)
}
#[utoipa::path(post,path="/api/agents/{agent_id}/standing-intents/undo",tag="agents",params(("agent_id"=String,Path)),request_body=UndoIntentRequest,responses((status=200,description="Exact saved change reversed"),(status=409,description="Intervening edit or fire; Undo refused")))]
pub(super) async fn undo(State(s): State<AppState>, Path(a): Path<String>, r: Request) -> Response {
    if let Err(r) = authorize(&s, &r, false) {
        return r;
    }
    let bytes = match read_limited_body(r, 1024).await {
        Ok(b) => b,
        Err(r) => return no_store(r),
    };
    let out = async {
        let q: UndoIntentRequest = parse_json_body(bytes)?;
        s.agent_runs
            .standing_intents()
            .undo(a, q.change_id, None)
            .await
    }
    .await;
    result(out)
}
