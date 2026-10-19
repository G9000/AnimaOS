//! `/api/system/*`: drain and shutdown for a supervisor (`crate::system`).
//! It calls with `Authorization: Bearer <ANIMA_LOCAL_ADMIN_TOKEN>` and no
//! Origin; the console calls with an allowed Origin.
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

use super::jobs::no_store;
use super::{json_response, AppState};
use crate::system::InFlight;

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct DrainStatus {
    draining: bool,
    in_flight: InFlight,
}

fn forbidden() -> Response {
    no_store(json_response(
        StatusCode::FORBIDDEN,
        &serde_json::json!({"error": "Local owner authorization required"}),
    ))
}

#[utoipa::path(post, path = "/api/system/drain", tag = "system",
    responses((status = 202, description = "Draining: new messages, swarm runs, jobs and goals are refused (503) and queued runs still finish", body = DrainStatus), (status = 403, description = "Local owner required")))]
pub(super) async fn begin_drain(State(state): State<AppState>, request: Request) -> Response {
    if state.local_owner.authorize(request.headers()).is_err() {
        return forbidden();
    }
    let in_flight = state.drain.begin().await;
    no_store(json_response(
        StatusCode::ACCEPTED,
        &DrainStatus {
            draining: true,
            in_flight,
        },
    ))
}

#[utoipa::path(get, path = "/api/system/drain", tag = "system",
    responses((status = 200, description = "Whether a drain is under way, and the work still in flight", body = DrainStatus), (status = 403, description = "Local owner required")))]
pub(super) async fn drain_status(State(state): State<AppState>, request: Request) -> Response {
    if state.local_owner.authorize_read(request.headers()).is_err() {
        return forbidden();
    }
    let in_flight = state.drain.in_flight().await;
    no_store(json_response(
        StatusCode::OK,
        &DrainStatus {
            draining: state.drain.is_draining(),
            in_flight,
        },
    ))
}

#[utoipa::path(delete, path = "/api/system/drain", tag = "system",
    responses((status = 204, description = "Drain cancelled; admission is as before"), (status = 403, description = "Local owner required")))]
pub(super) async fn cancel_drain(State(state): State<AppState>, request: Request) -> Response {
    if state.local_owner.authorize(request.headers()).is_err() {
        return forbidden();
    }
    state.drain.cancel();
    no_store(StatusCode::NO_CONTENT.into_response())
}

#[utoipa::path(post, path = "/api/system/shutdown", tag = "system",
    responses((status = 202, description = "Graceful shutdown begins, as on SIGTERM"), (status = 403, description = "Local owner required")))]
pub(super) async fn shutdown(State(state): State<AppState>, request: Request) -> Response {
    if state.local_owner.authorize(request.headers()).is_err() {
        return forbidden();
    }
    state.drain.request_shutdown();
    no_store(StatusCode::ACCEPTED.into_response())
}
