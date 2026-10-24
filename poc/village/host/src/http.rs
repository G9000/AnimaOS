use crate::{Customization, Error, Interaction, Result, Service};
use anima_village::{GameView, Language, NpcId};
use axum::{
    extract::Request,
    extract::{rejection::JsonRejection, DefaultBodyLimit, Path, State},
    http::{header, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;

pub fn router(service: Service) -> Router {
    Router::new()
        .route("/api/village/health", get(health))
        .route("/api/village/saves", post(create))
        .route("/api/village/saves/{id}", get(restore))
        .route("/api/village/saves/{id}/traveler", post(customize))
        .route("/api/village/saves/{id}/visit", post(visit))
        .route("/api/village/saves/{id}/turns", post(turn))
        .fallback(|| async { Error::new(404, "Village route not found") })
        .layer(DefaultBodyLimit::max(16_384))
        .layer(middleware::from_fn(no_store))
        .with_state(service)
}
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        (
            StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            Json(json!({"error":self.message})),
        )
            .into_response()
    }
}
async fn no_store(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    response
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateSave {
    language: Language,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Visit {
    request_id: String,
    npc_id: Option<NpcId>,
}
fn body<T>(value: std::result::Result<Json<T>, JsonRejection>) -> Result<T> {
    value
        .map(|Json(v)| v)
        .map_err(|_| Error::new(400, "Invalid JSON request body"))
}
async fn create(
    State(service): State<Service>,
    input: std::result::Result<Json<CreateSave>, JsonRejection>,
) -> Result<(StatusCode, Json<GameView>)> {
    Ok((
        StatusCode::CREATED,
        Json(service.create(body(input)?.language)?),
    ))
}
async fn restore(State(service): State<Service>, Path(id): Path<String>) -> Result<Json<GameView>> {
    Ok(Json(service.get(&id)?))
}
async fn customize(
    State(service): State<Service>,
    Path(id): Path<String>,
    input: std::result::Result<Json<Customization>, JsonRejection>,
) -> Result<Json<GameView>> {
    Ok(Json(service.customize(&id, body(input)?)?))
}
async fn visit(
    State(service): State<Service>,
    Path(id): Path<String>,
    input: std::result::Result<Json<Visit>, JsonRejection>,
) -> Result<Json<GameView>> {
    let input = body(input)?;
    Ok(Json(
        service
            .interact(
                &id,
                Interaction {
                    request_id: input.request_id,
                    message: String::new(),
                    choice_id: None,
                    recovery_of: None,
                    npc_id: input.npc_id,
                },
                true,
            )
            .await?,
    ))
}
async fn turn(
    State(service): State<Service>,
    Path(id): Path<String>,
    input: std::result::Result<Json<Interaction>, JsonRejection>,
) -> Result<Json<GameView>> {
    Ok(Json(service.interact(&id, body(input)?, false).await?))
}
async fn health(State(service): State<Service>) -> Json<serde_json::Value> {
    let runtime = service.runtime();
    Json(
        json!({"available":service.engine.is_some(),"mode":runtime.mode,"engine":runtime.engine,"provider":runtime.provider,"model":runtime.model,"reason":service.unavailable_reason}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use http_body_util::BodyExt;
    use std::time::Duration;
    use tower::ServiceExt;
    #[tokio::test]
    async fn api_uses_frozen_camel_case_contract_and_json_errors() {
        let dir = tempfile::tempdir().unwrap();
        let service = Service::new(
            Store::open(&dir.path().join("api.sqlite")).unwrap(),
            None,
            Duration::from_secs(1),
        );
        let app = router(service);
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/village/saves")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"language":"ja"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let body: serde_json::Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert!(body["saveId"].is_string());
        assert_eq!(body["runtime"]["engine"], "anima-harness");
        assert_eq!(body["questStatus"], "pending");
        let id = body["saveId"].as_str().unwrap();
        let response=app.clone().oneshot(Request::builder().method("POST").uri(format!("/api/village/saves/{id}/turns")).header("content-type","application/json").body(Body::from(r#"{"requestId":"order","message":"ミラのパンを二つ、お願いします。","choiceId":"collect_order"}"#)).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(body["coins"], 20);
        assert_eq!(body["inventory"][0]["quantity"], 2);
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/village/saves")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"language":"fr"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body: serde_json::Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert!(body["error"].is_string());
    }

    struct ProviderFixture {
        calls: std::sync::atomic::AtomicUsize,
    }
    #[async_trait::async_trait]
    impl anima_harness::anima_core::ModelAdapter for ProviderFixture {
        fn provider(&self) -> &str {
            "fixture"
        }
        fn single_attempt_contract(&self) -> bool {
            true
        }
        fn generated_usage_is_complete(
            &self,
            _response: &anima_harness::anima_core::ModelGenerateResponse,
        ) -> bool {
            true
        }
        async fn generate(
            &self,
            config: &anima_harness::anima_core::AgentConfig,
            request: &anima_harness::anima_core::ModelGenerateRequest,
        ) -> std::result::Result<anima_harness::anima_core::ModelGenerateResponse, String> {
            use anima_harness::anima_core::{
                Content, ModelGenerateResponse, ModelStopReason, TokenUsage,
            };
            assert!(config.tools.as_ref().is_none_or(Vec::is_empty));
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let text = if request.system.contains("You interpret") {
                r#"{"order":{"recipient":"Mira","quantity":2}}"#
            } else {
                r#"{"text":"ミラのパンを二つ、どうぞ！","translation":"Here are Mira's two loaves and twenty coins.","reading":"Mira no pan o futatsu, dōzo!","expression":"happy"}"#
            };
            Ok(ModelGenerateResponse {
                content: Content {
                    text: text.into(),
                    ..Default::default()
                },
                tool_calls: None,
                usage: TokenUsage::default(),
                stop_reason: ModelStopReason::End,
            })
        }
    }
    #[tokio::test]
    async fn http_free_text_runs_core_harness_and_atomic_retries_do_not_generate_again() {
        let dir = tempfile::tempdir().unwrap();
        let fixture = std::sync::Arc::new(ProviderFixture {
            calls: std::sync::atomic::AtomicUsize::new(0),
        });
        let engine = std::sync::Arc::new(anima_village::HarnessNpc {
            adapter: fixture.clone(),
            provider: "fixture".into(),
            model: "controlled".into(),
        });
        let service = Service::new(
            Store::open(&dir.path().join("fixture.sqlite")).unwrap(),
            Some(engine),
            Duration::from_secs(1),
        );
        let save = service.create(anima_village::Language::Ja).unwrap();
        let app = router(service);
        let uri = format!("/api/village/saves/{}/turns", save.save_id);
        let request = || {
            Request::builder().method("POST").uri(&uri).header("content-type","application/json").body(Body::from(r#"{"requestId":"natural-order","message":"Could I collect the pair of loaves for Mira?"}"#)).unwrap()
        };
        let first = app.clone().oneshot(request()).await.unwrap();
        assert_eq!(first.status(), StatusCode::OK);
        let first: serde_json::Value =
            serde_json::from_slice(&first.into_body().collect().await.unwrap().to_bytes()).unwrap();
        assert_eq!(first["dialogue"]["source"], "model");
        assert_eq!(first["runtime"]["mode"], "live");
        assert_eq!(first["inventory"][0]["quantity"], 2);
        assert_eq!(first["coins"], 20);
        let second = app.oneshot(request()).await.unwrap();
        let second: serde_json::Value =
            serde_json::from_slice(&second.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(first, second);
        assert_eq!(fixture.calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    }
}
