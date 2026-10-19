//! The control API on `127.0.0.1:{control_port}`, used by the CLI, the tray
//! and the server's console relay. Every route needs
//! `Authorization: Bearer <control-token>`.

use std::time::Duration;

use axum::body::Bytes;
use axum::extract::{Path, Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::{mpsc, oneshot};

use super::state::{Action, Conflict, SupervisorStatus};
use super::tokens::ControlToken;
use super::SharedStatus;

/// How long a request waits for the supervisor to take an action.
const ACTION_TIMEOUT: Duration = Duration::from_secs(10);

/// A requested action, answered with the status once it is under way.
pub struct ActionRequest {
    pub action: Action,
    pub force: bool,
    pub reply: oneshot::Sender<Result<SupervisorStatus, Conflict>>,
}

#[derive(Clone)]
struct ApiState {
    token: ControlToken,
    status: SharedStatus,
    requests: mpsc::Sender<ActionRequest>,
}

pub fn router(
    token: ControlToken,
    status: SharedStatus,
    requests: mpsc::Sender<ActionRequest>,
) -> Router {
    let state = ApiState {
        token,
        status,
        requests,
    };
    Router::new()
        .route("/v1/status", get(status_route))
        .route("/v1/actions/{action}", post(action_route))
        .layer(middleware::from_fn_with_state(state.clone(), require_token))
        .with_state(state)
}

fn error(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({ "error": message }))).into_response()
}

async fn require_token(State(state): State<ApiState>, request: Request, next: Next) -> Response {
    let presented = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    match presented {
        Some(presented)
            if constant_time_eq(presented.as_bytes(), state.token.as_str().as_bytes()) =>
        {
            next.run(request).await
        }
        _ => error(StatusCode::UNAUTHORIZED, "Unauthorized"),
    }
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0u8, |diff, (a, b)| diff | (a ^ b))
        == 0
}

async fn status_route(State(state): State<ApiState>) -> Response {
    let status = super::lock(&state.status).clone();
    Json(status).into_response()
}

#[derive(Deserialize)]
struct ActionBody {
    #[serde(default)]
    force: bool,
}

async fn action_route(
    State(state): State<ApiState>,
    Path(action): Path<String>,
    body: Bytes,
) -> Response {
    let Ok(action) = serde_json::from_value::<Action>(serde_json::Value::String(action)) else {
        return error(StatusCode::NOT_FOUND, "Unknown action");
    };
    let force = if body.iter().all(u8::is_ascii_whitespace) {
        false
    } else {
        match serde_json::from_slice::<ActionBody>(&body) {
            Ok(body) => body.force,
            Err(_) => return error(StatusCode::BAD_REQUEST, "Expected {\"force\": bool}"),
        }
    };
    let (reply, answer) = oneshot::channel();
    let request = ActionRequest {
        action,
        force,
        reply,
    };
    if state.requests.send(request).await.is_err() {
        return error(StatusCode::SERVICE_UNAVAILABLE, "Anima is shutting down");
    }
    match tokio::time::timeout(ACTION_TIMEOUT, answer).await {
        Ok(Ok(Ok(status))) => (StatusCode::ACCEPTED, Json(status)).into_response(),
        Ok(Ok(Err(Conflict { pending }))) => (
            StatusCode::CONFLICT,
            Json(json!({ "error": "Another action is in progress", "pending": pending })),
        )
            .into_response(),
        _ => error(
            StatusCode::SERVICE_UNAVAILABLE,
            "The supervisor did not take the action",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_comparison() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
    }
}
