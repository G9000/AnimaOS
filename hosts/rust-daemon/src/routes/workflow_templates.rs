use anima_core::workflow_templates::{WorkflowTemplateInput, WorkflowTemplateRunInput};
use axum::{
    extract::{Path, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Deserialize;

use super::{
    jobs::{authorize, error_response, no_store},
    json_response, AppState,
};

async fn bounded<T: serde::de::DeserializeOwned>(
    state: &AppState,
    request: Request,
) -> Result<T, Response> {
    use std::error::Error as _;
    let limit = state.config.max_request_bytes.min(64 * 1024);
    let bytes = axum::body::to_bytes(request.into_body(), limit)
        .await
        .map_err(|error| {
            let oversized = error
                .source()
                .is_some_and(|source| source.is::<http_body_util::LengthLimitError>());
            let (status, message) = if oversized {
                (
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "Workflow template request exceeds the body limit",
                )
            } else {
                (
                    StatusCode::BAD_REQUEST,
                    "Malformed workflow template request body",
                )
            };
            no_store(json_response(status, &serde_json::json!({"error":message})))
        })?;
    super::http::parse_json_body(bytes.to_vec()).map_err(|error| no_store(error.into_response()))
}

fn response<T: serde::Serialize>(result: Result<T, crate::jobs::JobError>) -> Response {
    match result {
        Ok(value) => no_store(json_response(StatusCode::OK, &value)),
        Err(error) => error_response(error),
    }
}

#[utoipa::path(get, path="/api/agents/{agent_id}/workflow-templates", tag="workflows", params(("agent_id"=String,Path)),
    responses((status=200,description="Agent-owned templates",body=serde_json::Value),(status=403,description="Local owner required")))]
pub(super) async fn list(
    State(state): State<AppState>,
    Path(agent): Path<String>,
    request: Request,
) -> Response {
    if let Err(error) = authorize(&state, &request, true) {
        return error;
    }
    response(
        state
            .jobs
            .list_workflow_templates(&agent)
            .await
            .map(|templates| serde_json::json!({"templates":templates})),
    )
}

#[utoipa::path(post, path="/api/agents/{agent_id}/workflow-templates", tag="workflows", params(("agent_id"=String,Path)), request_body=serde_json::Value,
    responses((status=200,description="Revision-checked save; exact retries do not add revisions",body=serde_json::Value),(status=400,description="Invalid template"),(status=409,description="Revision or capacity conflict")))]
pub(super) async fn save(
    State(state): State<AppState>,
    Path(agent): Path<String>,
    request: Request,
) -> Response {
    if let Err(error) = authorize(&state, &request, false) {
        return error;
    }
    let input: WorkflowTemplateInput = match bounded(&state, request).await {
        Ok(input) => input,
        Err(error) => return error,
    };
    response(state.jobs.save_workflow_template(&agent, input).await)
}

#[utoipa::path(get, path="/api/agents/{agent_id}/workflow-templates/{slug}", tag="workflows", params(("agent_id"=String,Path),("slug"=String,Path)),
    responses((status=200,description="Saved template",body=serde_json::Value),(status=404,description="Template not found")))]
pub(super) async fn get(
    State(state): State<AppState>,
    Path((agent, slug)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(error) = authorize(&state, &request, true) {
        return error;
    }
    response(state.jobs.get_workflow_template(&agent, &slug).await)
}

#[utoipa::path(delete, path="/api/agents/{agent_id}/workflow-templates/{slug}", tag="workflows", params(("agent_id"=String,Path),("slug"=String,Path)), request_body=serde_json::Value,
    responses((status=200,description="Template removed from picker; saved runs and replay records retained",body=serde_json::Value),(status=409,description="Revision conflict")))]
pub(super) async fn delete(
    State(state): State<AppState>,
    Path((agent, slug)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(error) = authorize(&state, &request, false) {
        return error;
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Delete {
        revision: u64,
    }
    let input: Delete = match bounded(&state, request).await {
        Ok(input) => input,
        Err(error) => return error,
    };
    response(
        state
            .jobs
            .delete_workflow_template(&agent, &slug, input.revision)
            .await,
    )
}

#[utoipa::path(post, path="/api/agents/{agent_id}/workflow-templates/{slug}/run", tag="workflows", params(("agent_id"=String,Path),("slug"=String,Path)), request_body=serde_json::Value,
    responses((status=200,description="Fresh immutable workflow or exact request replay",body=serde_json::Value),(status=400,description="Invalid arguments"),(status=409,description="Revision or key conflict")))]
pub(super) async fn run(
    State(state): State<AppState>,
    Path((agent, slug)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(error) = authorize(&state, &request, false) {
        return error;
    }
    let input: WorkflowTemplateRunInput = match bounded(&state, request).await {
        Ok(input) => input,
        Err(error) => return error,
    };
    response(state.jobs.run_workflow_template(&agent, &slug, input).await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{to_bytes, Body},
        http::Request as HttpRequest,
        Router,
    };
    use std::sync::Arc;
    use tokio::sync::RwLock;
    use tower::ServiceExt;

    async fn request(app: Router, method: &str, uri: &str, input: serde_json::Value) -> Response {
        app.oneshot(
            HttpRequest::builder()
                .method(method)
                .uri(uri)
                .header("host", "127.0.0.1:8080")
                .header("origin", "http://localhost:4200")
                .header("content-type", "application/json")
                .body(Body::from(input.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
    }
    async fn json(response: Response) -> serde_json::Value {
        serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap()).unwrap()
    }

    #[tokio::test]
    async fn workflow_templates_http_owned_revisions_strict_bodies_and_fresh_runs() {
        let path =
            std::env::temp_dir().join(format!("anima-template-http-{}.json", uuid::Uuid::new_v4()));
        let mut state = crate::state::DaemonState::new();
        state.set_control_plane_store(Some(
            crate::control_plane_store::ControlPlaneStoreConfig::Json(path.clone()),
        ));
        let owner = state
            .create_agent(
                serde_json::from_value(serde_json::json!({"name":"owner","model":"deterministic"}))
                    .unwrap(),
            )
            .unwrap()
            .state
            .id;
        let other = state
            .create_agent(
                serde_json::from_value(serde_json::json!({"name":"other","model":"deterministic"}))
                    .unwrap(),
            )
            .unwrap()
            .state
            .id;
        let app = crate::routes::router(
            Arc::new(RwLock::new(state)),
            crate::app::DaemonConfig::default(),
        );
        let uri = format!("/api/agents/{owner}/workflow-templates");
        let definition = serde_json::json!({"slug":"brief","revision":0,"title":"Brief ${topic}","parameters":[{"name":"topic","default":"Daily"}],"steps":[{"id":"review","title":"Review","kind":"approval","prompt":"Review ${topic}"}]});
        let response = request(app.clone(), "POST", &uri, definition.clone()).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["cache-control"], "no-store");
        let saved = json(response).await;
        assert_eq!(saved["revision"], 1);
        let run_uri = format!("{uri}/brief/run");
        let first_input =
            serde_json::json!({"requestKey":"first","revision":1,"inputs":{"topic":"Alpha"}});
        let first = json(request(app.clone(), "POST", &run_uri, first_input.clone()).await).await;
        assert_eq!(first["title"], "Brief Alpha");
        assert_eq!(first["sourceTemplate"]["revision"], 1);
        let second = json(
            request(
                app.clone(),
                "POST",
                &run_uri,
                serde_json::json!({"requestKey":"second","revision":1,"inputs":{"topic":"Beta"}}),
            )
            .await,
        )
        .await;
        assert_ne!(first["id"], second["id"]);
        assert_eq!(second["title"], "Brief Beta");
        let mut edit = definition.clone();
        edit["revision"] = serde_json::json!(1);
        edit["title"] = serde_json::json!("New ${topic}");
        assert_eq!(
            request(app.clone(), "POST", &uri, edit).await.status(),
            StatusCode::OK
        );
        let mut stale = definition.clone();
        stale["title"] = serde_json::json!("Stale");
        assert_eq!(
            request(app.clone(), "POST", &uri, stale).await.status(),
            StatusCode::CONFLICT
        );
        assert_eq!(
            json(request(app.clone(), "POST", &run_uri, first_input).await).await,
            first
        );
        let mut unknown = definition;
        unknown["surprise"] = serde_json::json!(true);
        assert_eq!(
            request(app.clone(), "POST", &uri, unknown).await.status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            request(
                app.clone(),
                "GET",
                &format!("/api/agents/{other}/workflow-templates/brief"),
                serde_json::Value::Null
            )
            .await
            .status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            request(
                app.clone(),
                "POST",
                &format!("/api/agents/{other}/workflow-templates/brief/run"),
                serde_json::json!({"requestKey":"x","revision":1,"inputs":{}})
            )
            .await
            .status(),
            StatusCode::NOT_FOUND
        );
        let malformed = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri(&uri)
                    .header("host", "127.0.0.1:8080")
                    .header("origin", "http://localhost:4200")
                    .header("content-type", "application/json")
                    .body(Body::from("not json"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(malformed.status(), StatusCode::BAD_REQUEST);
        assert_eq!(malformed.headers()["cache-control"], "no-store");
        // Valid JSON over the byte limit must be refused before schema parsing.
        let response = request(
            app,
            "POST",
            &uri,
            serde_json::json!({"slug":"x".repeat(64 * 1024)}),
        )
        .await;
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(response.headers()["cache-control"], "no-store");
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn workflow_templates_http_owner_guard_precedes_parse_and_lookup_for_every_route() {
        let app = crate::routes::router(
            Arc::new(RwLock::new(crate::state::DaemonState::new())),
            crate::app::DaemonConfig::default(),
        );
        for (method, uri) in [
            ("GET", "/api/agents/missing/workflow-templates"),
            ("POST", "/api/agents/missing/workflow-templates"),
            ("GET", "/api/agents/missing/workflow-templates/missing"),
            ("DELETE", "/api/agents/missing/workflow-templates/missing"),
            ("POST", "/api/agents/missing/workflow-templates/missing/run"),
        ] {
            for (host, origin) in [
                ("127.0.0.1:8080", "https://untrusted.example"),
                ("192.0.2.1:8080", "http://localhost:4200"),
            ] {
                let response = app
                    .clone()
                    .oneshot(
                        HttpRequest::builder()
                            .method(method)
                            .uri(uri)
                            .header("host", host)
                            .header("origin", origin)
                            .body(Body::from("not json"))
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(
                    response.status(),
                    StatusCode::FORBIDDEN,
                    "{method} {uri} {host}"
                );
                assert_eq!(response.headers()["cache-control"], "no-store");
            }
        }
    }
}
