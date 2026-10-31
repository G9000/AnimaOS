//! Explicit local-owner task comparisons. No handler is exposed as an actor tool.
use super::jobs::{authorize, no_store};
use super::skills::answer;
use super::{ApiError, ApiErrorKind, AppState};
use crate::task_regressions::{CompareRequest, RegressionError};
use anima_core::task_regressions::RegressionSuite;
use axum::{
    extract::{Path, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::de::DeserializeOwned;

fn error(error: RegressionError) -> Response {
    no_store(
        match error {
            RegressionError::NotFound => ApiError::not_found(),
            RegressionError::Invalid(message) => ApiError {
                kind: ApiErrorKind::General,
                status: StatusCode::UNPROCESSABLE_ENTITY,
                message,
            },
            RegressionError::Conflict(message) => ApiError::conflict(message),
            RegressionError::Unavailable(message) => ApiError::service_unavailable(message),
        }
        .into_response(),
    )
}

async fn body<T: DeserializeOwned>(state: &AppState, request: Request) -> Result<T, Response> {
    let bytes =
        super::read_limited_body(request, state.config.max_request_bytes.max(8 * 1024 * 1024))
            .await
            .map_err(no_store)?;
    super::parse_json_body(bytes).map_err(|error: ApiError| no_store(error.into_response()))
}

#[utoipa::path(get, path = "/api/task-regression-suites", tag = "task regressions", responses((status = 200, description = "Owner saved synthetic task suites; no private expected values", body = serde_json::Value)))]
pub(super) async fn suites(State(state): State<AppState>, request: Request) -> Response {
    if let Err(response) = authorize(&state, &request, true) {
        return response;
    }
    match state.task_regressions.suites().await {
        Ok(suites) => answer(StatusCode::OK, &serde_json::json!({"suites": suites})),
        Err(e) => error(e),
    }
}

#[utoipa::path(post, path = "/api/task-regression-suites", tag = "task regressions", request_body = serde_json::Value, responses((status = 201, description = "Save immutable bounded synthetic fixtures and hidden oracles; dispatches no model calls", body = serde_json::Value)))]
pub(super) async fn define(State(state): State<AppState>, request: Request) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let suite: RegressionSuite = match body(&state, request).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    match state.task_regressions.define(suite).await {
        Ok(suite) => answer(StatusCode::CREATED, &serde_json::json!({"suite": suite})),
        Err(e) => error(e),
    }
}

#[utoipa::path(get, path = "/api/task-regression-configs/{agent_id}", tag = "task regressions", params(("agent_id" = String, Path)), responses((status = 200, description = "Capture current owner configuration identity without dispatch", body = serde_json::Value)))]
pub(super) async fn configuration(
    State(state): State<AppState>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, true) {
        return response;
    }
    match state.task_regressions.configuration(&id).await {
        Ok(configuration) => answer(
            StatusCode::OK,
            &serde_json::json!({"configuration": configuration}),
        ),
        Err(e) => error(e),
    }
}

#[utoipa::path(post, path = "/api/task-regressions", tag = "task regressions", request_body = serde_json::Value, responses((status = 202, description = "Explicit owner admission of bounded selected-provider task calls; reports grant no permissions or promotion", body = serde_json::Value)))]
pub(super) async fn start(State(state): State<AppState>, request: Request) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let input: CompareRequest = match body(&state, request).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    match state.task_regressions.joined(&input).await {
        Ok(Some(report)) => {
            return answer(StatusCode::ACCEPTED, &serde_json::json!({"report": report}))
        }
        Err(e) => return error(e),
        Ok(None) => {}
    }
    if let Err(error) = state.agent_runs.enforce_monthly_spend_limit().await {
        return no_store(error.into_response());
    }
    match state.task_regressions.start(input).await {
        Ok(report) => answer(StatusCode::ACCEPTED, &serde_json::json!({"report": report})),
        Err(e) => error(e),
    }
}

#[utoipa::path(get, path = "/api/task-regressions/{id}", tag = "task regressions", params(("id" = String, Path)), responses((status = 200, description = "Pinned task facts and independently regraded matrix; reading never dispatches", body = serde_json::Value)))]
pub(super) async fn report(
    State(state): State<AppState>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, true) {
        return response;
    }
    match state.task_regressions.report(&id).await {
        Ok(report) => answer(StatusCode::OK, &serde_json::json!({"report": report})),
        Err(e) => error(e),
    }
}

#[utoipa::path(post, path = "/api/task-regressions/{id}/cancel", tag = "task regressions", params(("id" = String, Path)), responses((status = 200, description = "Stop logical eligibility while retaining in-flight transport and uncertain usage custody", body = serde_json::Value)))]
pub(super) async fn cancel(
    State(state): State<AppState>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    if let Err(response) = authorize(&state, &request, false) {
        return response;
    }
    let input: serde_json::Map<String, serde_json::Value> = match body(&state, request).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    if !input.is_empty() {
        return error(RegressionError::Invalid(
            "Cancellation accepts an empty object".into(),
        ));
    }
    match state.task_regressions.cancel(&id).await {
        Ok(report) => answer(StatusCode::OK, &serde_json::json!({"report": report})),
        Err(e) => error(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anima_core::{AgentConfig, ModelAdapter, ModelGenerateRequest, ModelGenerateResponse};
    use async_trait::async_trait;
    use axum::{
        body::{to_bytes, Body},
        http::Request,
    };
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use tower::ServiceExt;

    #[derive(Default)]
    struct RefuseModel(AtomicUsize);
    #[async_trait]
    impl ModelAdapter for RefuseModel {
        fn provider(&self) -> &str {
            "test"
        }
        async fn generate(
            &self,
            _: &AgentConfig,
            _: &ModelGenerateRequest,
        ) -> Result<ModelGenerateResponse, String> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Err("No test setup/read operation may dispatch a model".into())
        }
    }

    #[tokio::test]
    async fn task_regressions_routes_require_owner_and_suite_setup_never_dispatches() {
        let temporary = tempfile::tempdir().unwrap();
        let model = Arc::new(RefuseModel::default());
        let mut daemon = crate::state::DaemonState::with_model_adapter(model.clone());
        daemon.set_control_plane_store(Some(
            crate::control_plane_store::ControlPlaneStoreConfig::Json(
                temporary.path().join("control.json"),
            ),
        ));
        let state = Arc::new(tokio::sync::RwLock::new(daemon));
        let app = crate::routes::router(state.clone(), crate::app::DaemonConfig::default());
        for (method, path) in [
            ("GET", "/api/task-regression-suites"),
            ("POST", "/api/task-regressions"),
            ("GET", "/api/task-regressions/missing"),
            ("POST", "/api/task-regressions/missing/cancel"),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(path)
                        .body(Body::from("{}"))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{method} {path}");
        }
        let suite = serde_json::json!({"name":"Owner synthetic task","fixtures":[{"id":"sum","actor":{"prompt":"Compute the sum in input.txt","initialFiles":{"input.txt":"2 3"},"readableFiles":["input.txt"],"writableFiles":["answer.txt"],"mockEffects":false},"oracle":{"expectedFiles":{"input.txt":"2 3","answer.txt":"5"},"expectedOutput":"PRIVATE_ROUTE_CANARY","expectedMockEffects":[]}}]});
        let request = crate::routes::owner_test_support::authenticate(
            Request::builder()
                .method("POST")
                .uri("/api/task-regression-suites")
                .header("content-type", "application/json"),
        );
        let response = app
            .clone()
            .oneshot(request.body(Body::from(suite.to_string())).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(response.headers().get("cache-control").unwrap(), "no-store");
        let saved = String::from_utf8(
            to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(!saved.contains("PRIVATE_ROUTE_CANARY"));
        assert!(!saved.contains("expectedFiles"));
        let request = crate::routes::owner_test_support::authenticate(
            Request::builder().uri("/api/task-regression-suites"),
        );
        let response = app
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("cache-control").unwrap(), "no-store");
        let mut malformed = suite;
        malformed["fixtures"][0]["actor"]["privateOracle"] =
            serde_json::json!("not actor authority");
        let request = crate::routes::owner_test_support::authenticate(
            Request::builder()
                .method("POST")
                .uri("/api/task-regression-suites")
                .header("content-type", "application/json"),
        );
        let response = app
            .oneshot(request.body(Body::from(malformed.to_string())).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(model.0.load(Ordering::SeqCst), 0);
        assert_eq!(state.read().await.task_regressions.run_count(), 0);
    }
}
