use super::{http::json_response, AppState};
use axum::{
    extract::{Request, State},
    http::{header, HeaderValue, StatusCode},
    response::Response,
};

fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}
enum Operation {
    Status,
    Login,
    Cancel,
    Disconnect,
}
async fn handle(state: AppState, request: Request, operation: Operation) -> Response {
    let authorized = match operation {
        Operation::Status => state.local_owner.authorize_read(request.headers()),
        _ => state.local_owner.authorize(request.headers()),
    };
    if authorized.is_err() {
        return no_store(json_response(
            StatusCode::FORBIDDEN,
            &serde_json::json!({"error":"Local owner authorization required"}),
        ));
    }
    let service = state.daemon.read().await.chatgpt_auth.clone();
    let result = match operation {
        Operation::Login => service.login().await,
        Operation::Cancel => service.cancel(false).await,
        Operation::Disconnect => service.cancel(true).await,
        Operation::Status => service.status().await,
    };
    no_store(match result {
        Ok(status) => json_response(StatusCode::OK, &status),
        Err(error) => json_response(
            StatusCode::SERVICE_UNAVAILABLE,
            &serde_json::json!({"error":error}),
        ),
    })
}
#[utoipa::path(get, path = "/api/providers/chatgpt/status", tag = "providers", responses((status = 200, description = "Redacted subscription status", body = crate::chatgpt_auth::Status), (status = 403, description = "Local owner required"), (status = 503, description = "Authorization or vault unavailable")))]
pub(super) async fn status(State(state): State<AppState>, request: Request) -> Response {
    handle(state, request, Operation::Status).await
}
#[utoipa::path(post, path = "/api/providers/chatgpt/login", tag = "providers", responses((status = 200, description = "Redacted subscription status", body = crate::chatgpt_auth::Status), (status = 403, description = "Local owner required"), (status = 503, description = "Authorization or vault unavailable")))]
pub(super) async fn login(State(state): State<AppState>, request: Request) -> Response {
    handle(state, request, Operation::Login).await
}
#[utoipa::path(delete, path = "/api/providers/chatgpt/login", tag = "providers", responses((status = 200, description = "Redacted subscription status", body = crate::chatgpt_auth::Status), (status = 403, description = "Local owner required"), (status = 503, description = "Authorization or vault unavailable")))]
pub(super) async fn cancel(State(state): State<AppState>, request: Request) -> Response {
    handle(state, request, Operation::Cancel).await
}
#[utoipa::path(delete, path = "/api/providers/chatgpt", tag = "providers", responses((status = 200, description = "Redacted subscription status", body = crate::chatgpt_auth::Status), (status = 403, description = "Local owner required"), (status = 503, description = "Authorization or vault unavailable")))]
pub(super) async fn disconnect(State(state): State<AppState>, request: Request) -> Response {
    handle(state, request, Operation::Disconnect).await
}

#[derive(utoipa::ToSchema)]
pub(super) struct ModelsRefreshRequest {}

fn models_response(
    result: Result<crate::chatgpt_auth::ChatGptModelsSnapshot, crate::chatgpt_auth::ModelsError>,
) -> Response {
    use crate::chatgpt_auth::ModelsError;
    no_store(match result {
        Ok(snapshot) => json_response(StatusCode::OK, &snapshot),
        Err(ModelsError::Busy) => json_response(
            StatusCode::CONFLICT,
            &serde_json::json!({"error":"Model metadata refresh is already in progress."}),
        ),
        Err(ModelsError::Vault) => json_response(
            StatusCode::SERVICE_UNAVAILABLE,
            &serde_json::json!({"error":"ChatGPT credential vault is unavailable"}),
        ),
    })
}

#[utoipa::path(get,path="/api/providers/chatgpt/models",tag="providers",responses(
    (status=200,description="Cached current-connection metadata; no upstream request",body=crate::chatgpt_auth::ChatGptModelsSnapshot),
    (status=403,description="Local owner required"),(status=503,description="Credential vault unavailable")))]
pub(super) async fn models(State(state): State<AppState>, request: Request) -> Response {
    if state.local_owner.authorize_read(request.headers()).is_err() {
        return no_store(json_response(
            StatusCode::FORBIDDEN,
            &serde_json::json!({"error":"Local owner authorization required"}),
        ));
    }
    let service = state.daemon.read().await.chatgpt_auth.clone();
    models_response(service.models().await)
}

#[utoipa::path(post,path="/api/providers/chatgpt/models/refresh",tag="providers",request_body=ModelsRefreshRequest,responses(
    (status=200,description="One bounded explicit metadata refresh; no generation",body=crate::chatgpt_auth::ChatGptModelsSnapshot),
    (status=400,description="Expected empty object"),(status=403,description="Local owner required"),
    (status=409,description="Refresh in progress"),(status=503,description="Credential vault unavailable")))]
pub(super) async fn refresh_models(State(state): State<AppState>, request: Request) -> Response {
    if state.local_owner.authorize(request.headers()).is_err() {
        return no_store(json_response(
            StatusCode::FORBIDDEN,
            &serde_json::json!({"error":"Local owner authorization required"}),
        ));
    }
    let valid =
        match super::http::read_limited_body(request, state.config.max_request_bytes.min(4096))
            .await
        {
            Ok(bytes) => serde_json::from_slice::<serde_json::Value>(&bytes)
                .ok()
                .is_some_and(|value| value.as_object().is_some_and(|object| object.is_empty())),
            Err(_) => false,
        };
    if !valid {
        return no_store(json_response(
            StatusCode::BAD_REQUEST,
            &serde_json::json!({"error":"Model metadata refresh requires an empty object."}),
        ));
    }
    let service = state.daemon.read().await.chatgpt_auth.clone();
    models_response(service.refresh_models().await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{to_bytes, Body},
        http::Request,
    };
    use tower::ServiceExt;
    #[tokio::test]
    async fn m49_chatgpt_models_cached_get_returns_canonical_disconnected_no_store() {
        // The ordinary deterministic app owns isolated in-memory auth. Reading
        // cached metadata must not require a vault, remote catalog or generation.
        let app = crate::app();
        for _ in 0..2 {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("GET")
                        .uri("/api/providers/chatgpt/models")
                        .header("host", "127.0.0.1:8080")
                        .header("origin", "http://localhost:4200")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()["cache-control"], "no-store");
            let body = to_bytes(response.into_body(), 8192).await.unwrap();
            let snapshot: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(
                snapshot,
                serde_json::json!({
                    "state":"disconnected",
                    "connectionRevision":null,
                    "catalogRevision":null,
                    "fetchedAtMs":null,
                    "expiresAtMs":null,
                    "models":[],
                    "error":null,
                })
            );
        }
    }

    #[tokio::test]
    async fn m49_chatgpt_models_routes_reject_foreign_origin_before_work() {
        let app = crate::app();
        for (method, path) in [
            ("GET", "/api/providers/chatgpt/models"),
            ("POST", "/api/providers/chatgpt/models/refresh"),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(path)
                        .header("host", "127.0.0.1:8080")
                        .header("origin", "https://attacker.example")
                        .header("content-type", "application/json")
                        .body(Body::from("{}"))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
            assert_eq!(response.headers()["cache-control"], "no-store");
            let body = to_bytes(response.into_body(), 8192).await.unwrap();
            let error: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(
                error,
                serde_json::json!({"error":"Local owner authorization required"})
            );
        }
    }

    #[tokio::test]
    async fn m49_chatgpt_models_refresh_is_strict_no_store_and_preserves_agent_and_calls() {
        use crate::agent_runs::test_support::{companion_config, ScriptedModel};
        use crate::state::DaemonState;
        use std::sync::Arc;
        use tokio::sync::RwLock;
        let model = ScriptedModel::new(vec![]);
        let mut daemon = DaemonState::with_model_adapter(model.clone());
        let agent = daemon
            .create_agent(companion_config("M49 protected agent"))
            .unwrap()
            .state
            .id;
        let original = daemon.get_agent(&agent).unwrap().state.config;
        let state = Arc::new(RwLock::new(daemon));
        let app = crate::app::app_with_state(state.clone(), crate::app::DaemonConfig::default());
        for body in [
            "",
            "null",
            "[]",
            "\"\"",
            "{",
            "{\"model\":\"synthetic\"}",
            "{\"x\":0,\"x\":1}",
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/providers/chatgpt/models/refresh")
                        .header("host", "127.0.0.1:8080")
                        .header("origin", "http://localhost:4200")
                        .header("content-type", "application/json")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            assert_eq!(response.headers()["cache-control"], "no-store");
        }
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/providers/chatgpt/models/refresh")
                    .header("host", "127.0.0.1:8080")
                    .header("origin", "http://localhost:4200")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["cache-control"], "no-store");
        let body = to_bytes(response.into_body(), 8192).await.unwrap();
        let snapshot: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(snapshot["state"], "disconnected");
        assert_eq!(snapshot["models"], serde_json::json!([]));
        let guard = state.read().await;
        assert_eq!(guard.get_agent(&agent).unwrap().state.config, original);
        assert!(guard.runs.for_agent(&agent).is_empty());
        assert_eq!(guard.list_agents().len(), 1);
        assert!(guard.control_plane_snapshot().approvals.is_empty());
        assert!(model.requests().is_empty());
        assert!(model.secondary_requests().is_empty());
    }

    #[tokio::test]
    async fn account_routes_require_owner_and_do_not_cache_rejections() {
        for (method, path) in [
            ("GET", "/api/providers/chatgpt/status"),
            ("POST", "/api/providers/chatgpt/login"),
            ("DELETE", "/api/providers/chatgpt/login"),
            ("DELETE", "/api/providers/chatgpt"),
        ] {
            let response = crate::app()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(path)
                        .header("host", "127.0.0.1:8080")
                        .header("origin", "https://attacker.example")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
            assert_eq!(response.headers()["cache-control"], "no-store");
        }
    }
    #[tokio::test]
    async fn deterministic_account_status_and_login_are_isolated() {
        let app = crate::app();
        for (method, path, expected) in [
            ("GET", "/api/providers/chatgpt/status", StatusCode::OK),
            (
                "POST",
                "/api/providers/chatgpt/login",
                StatusCode::SERVICE_UNAVAILABLE,
            ),
            ("DELETE", "/api/providers/chatgpt", StatusCode::OK),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(path)
                        .header("host", "127.0.0.1:8080")
                        .header("origin", "http://localhost:4200")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), expected);
            assert_eq!(response.headers()["cache-control"], "no-store");
            let body = to_bytes(response.into_body(), 8192).await.unwrap();
            let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
            if expected == StatusCode::OK {
                assert_eq!(json["connected"], false);
                assert_eq!(json["accountId"], serde_json::Value::Null);
            }
        }
    }
    #[tokio::test]
    async fn browser_same_origin_read_uses_referrer_without_origin() {
        let response = crate::app()
            .oneshot(
                Request::builder()
                    .uri("/api/providers/chatgpt/status")
                    .header("host", "127.0.0.1:8080")
                    .header("sec-fetch-site", "same-origin")
                    .header("referer", "http://localhost:4200/settings")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["cache-control"], "no-store");
    }
}
