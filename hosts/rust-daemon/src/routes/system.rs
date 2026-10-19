//! `/api/system/*`: drain and shutdown for a supervisor (`crate::system`).
//! It calls with `Authorization: Bearer <ANIMA_LOCAL_ADMIN_TOKEN>` and no
//! Origin; the console calls with an allowed Origin. The console's status
//! read and lifecycle actions are relayed to the supervisor's control API
//! with its token, which the browser never sees.
use axum::extract::{Path, Request, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

use super::jobs::no_store;
use super::{json_response, parse_json_body, read_limited_body, AppState};
use crate::system::{InFlight, Unreachable, Upstream};

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

/// The lifecycle actions the supervisor takes; anything else is `404`.
const ACTIONS: [&str; 7] = [
    "start",
    "stop",
    "restart",
    "quit",
    "update",
    "check-update",
    "cancel",
];

/// Optional body of `POST /api/system/actions/{action}`.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct ActionRequest {
    /// Skip waiting for running work to finish.
    #[serde(default)]
    force: bool,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct UnsupervisedStatus {
    supervised: bool,
    version: &'static str,
}

fn error(status: StatusCode, message: &str) -> Response {
    no_store(json_response(
        status,
        &serde_json::json!({ "error": message }),
    ))
}

/// The supervisor's answer, status and body as it sent them.
fn relayed(upstream: Result<Upstream, Unreachable>) -> Response {
    match upstream {
        Ok(Upstream { status, body }) => no_store(
            (
                status,
                [(
                    header::CONTENT_TYPE,
                    HeaderValue::from_static("application/json"),
                )],
                body,
            )
                .into_response(),
        ),
        Err(Unreachable) => error(
            StatusCode::BAD_GATEWAY,
            "The Anima supervisor is not responding",
        ),
    }
}

#[utoipa::path(get, path = "/api/system", tag = "system",
    responses((status = 200, description = "Under a supervisor: its status, as it sent it. Otherwise `{ supervised: false, version }`", body = UnsupervisedStatus), (status = 403, description = "Local owner required"), (status = 502, description = "The supervisor is not responding")))]
pub(super) async fn status(State(state): State<AppState>, request: Request) -> Response {
    if state.local_owner.authorize_read(request.headers()).is_err() {
        return forbidden();
    }
    if !state.supervisor.is_supervised() {
        return no_store(json_response(
            StatusCode::OK,
            &UnsupervisedStatus {
                supervised: false,
                version: env!("CARGO_PKG_VERSION"),
            },
        ));
    }
    relayed(state.supervisor.status().await)
}

#[utoipa::path(post, path = "/api/system/actions/{action}", tag = "system",
    params(("action" = String, Path, description = "start, stop, restart, quit, update, check-update or cancel")),
    request_body(content = Option<ActionRequest>, description = "Optional; `force` skips waiting for running work"),
    responses((status = 202, description = "The supervisor took the action; its status and body pass through, a refusal such as 409 included"), (status = 403, description = "Local owner required"), (status = 404, description = "Unknown action"), (status = 409, description = "Not running under the supervisor"), (status = 502, description = "The supervisor is not responding")))]
pub(super) async fn action(
    State(state): State<AppState>,
    Path(action): Path<String>,
    request: Request,
) -> Response {
    if state.local_owner.authorize(request.headers()).is_err() {
        return forbidden();
    }
    if !ACTIONS.contains(&action.as_str()) {
        return error(StatusCode::NOT_FOUND, "Unknown system action");
    }
    if !state.supervisor.is_supervised() {
        return error(
            StatusCode::CONFLICT,
            "Anima is not running under the supervisor",
        );
    }
    let bytes = match read_limited_body(request, state.config.max_request_bytes).await {
        Ok(bytes) => bytes,
        Err(response) => return no_store(response),
    };
    let force = if bytes.iter().all(u8::is_ascii_whitespace) {
        false
    } else {
        match parse_json_body::<ActionRequest>(bytes) {
            Ok(body) => body.force,
            Err(rejection) => return no_store(rejection.into_response()),
        }
    };
    relayed(state.supervisor.action(&action, force).await)
}
