//! Owner-authenticated ingress for local adapters and webhooks.
use axum::{
    extract::{Path, Request, State},
    http::StatusCode,
    response::Response,
};
use serde::Deserialize;
use serde_json::{json, Value};

use super::http::{json_response, read_limited_body};
use super::{parse_json_body, AppState};

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct EventRequest {
    event_id: String,
    topic: String,
    #[serde(default = "empty_object")]
    payload: Value,
}
fn empty_object() -> Value {
    json!({})
}

#[utoipa::path(post, path = "/api/agents/{agent_id}/automation-events", tag = "schedules", params(("agent_id" = String, Path)), request_body = EventRequest, responses((status = 202, body = crate::schedules::event::EventAdmission), (status = 400), (status = 403), (status = 404), (status = 409), (status = 429), (status = 503)))]
pub(super) async fn emit_event(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
    request: Request,
) -> Response {
    if let Err(rejection) = state.local_owner.authorize(request.headers()) {
        return super::schedules::local_owner_error(rejection);
    }
    let body = match read_limited_body(request, state.config.max_request_bytes.min(16 * 1024)).await
    {
        Ok(body) => body,
        Err(_) => {
            return super::schedules::schedule_error(crate::schedules::ScheduleError::Invalid(
                "Event request exceeds its limit or is malformed",
            ))
        }
    };
    let request = match parse_json_body::<EventRequest>(body) {
        Ok(request) => request,
        Err(_) => {
            return super::schedules::schedule_error(crate::schedules::ScheduleError::Invalid(
                "Event request is invalid",
            ))
        }
    };
    match state
        .scheduler
        .automations()
        .emit_owner_event(
            agent_id,
            request.event_id,
            request.topic,
            request.payload,
            anima_core::primitives::now_millis(),
        )
        .await
    {
        Ok(admission) => {
            let mut response = json_response(StatusCode::ACCEPTED, &admission);
            response.headers_mut().insert(
                axum::http::header::CACHE_CONTROL,
                axum::http::HeaderValue::from_static("no-store"),
            );
            response
        }
        Err(error) => super::schedules::schedule_error(error),
    }
}
