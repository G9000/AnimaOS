//! The workspace side-task model over HTTP: owner-only, validated, and
//! `null` clears it.
use super::*;

use serde_json::{json, Value};

const OWNER_ORIGIN: &str = "http://localhost:4200";
const PATH: &str = "/api/settings/side-model";

async fn send_from(
    app: &axum::Router,
    origin: &str,
    method: &str,
    body: Option<Value>,
) -> (StatusCode, Option<String>, Value) {
    let builder = crate::routes::owner_test_support::authenticate(
        Request::builder()
            .method(method)
            .uri(PATH)
            .header("host", "127.0.0.1:8080")
            .header("origin", origin),
    );
    let request = match body {
        Some(body) => builder
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    };
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let cache = response
        .headers()
        .get("cache-control")
        .map(|value| value.to_str().unwrap().to_string());
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (
        status,
        cache,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn send(app: &axum::Router, method: &str, body: Option<Value>) -> (StatusCode, Value) {
    let (status, cache, value) = send_from(app, OWNER_ORIGIN, method, body).await;
    assert_eq!(cache.as_deref(), Some("no-store"), "{method} {status}");
    (status, value)
}

#[tokio::test]
async fn side_model_round_trips_and_validates() {
    let state = Arc::new(RwLock::new(DaemonState::new()));
    let app = router(state.clone(), DaemonConfig::default());

    let (status, value) = send(&app, "GET", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value, Value::Null, "none until the owner sets one");

    let (status, value) = send(
        &app,
        "PUT",
        Some(json!({"provider": " Anthropic ", "model": " claude-haiku-x "})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{value}");
    assert_eq!(
        value,
        json!({"provider": "anthropic", "model": "claude-haiku-x"})
    );
    assert_eq!(
        state.read().await.side_model,
        Some(anima_core::ModelChoice {
            provider: "anthropic".into(),
            model: "claude-haiku-x".into()
        })
    );
    let (_, value) = send(&app, "GET", None).await;
    assert_eq!(
        value,
        json!({"provider": "anthropic", "model": "claude-haiku-x"})
    );
    let snapshot = serde_json::to_value(state.read().await.control_plane_snapshot()).unwrap();
    assert_eq!(
        snapshot["sideModel"],
        json!({"provider": "anthropic", "model": "claude-haiku-x"})
    );

    for (case, body) in [
        (
            "unknown provider",
            json!({"provider": "nope", "model": "m"}),
        ),
        ("empty provider", json!({"provider": "", "model": "m"})),
        (
            "deterministic mock provider",
            json!({"provider": "deterministic", "model": "m"}),
        ),
        (
            "test mock provider",
            json!({"provider": " Test ", "model": "m"}),
        ),
        ("empty model", json!({"provider": "openai", "model": "  "})),
        (
            "model past 200 characters",
            json!({"provider": "openai", "model": "m".repeat(201)}),
        ),
        (
            "extra field",
            json!({"provider": "openai", "model": "m", "extra": 1}),
        ),
    ] {
        let (status, value) = send(&app, "PUT", Some(body)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{case}: {value}");
    }
    let (_, value) = send(&app, "GET", None).await;
    assert_eq!(
        value,
        json!({"provider": "anthropic", "model": "claude-haiku-x"}),
        "a rejected PUT changes nothing"
    );

    let (status, value) = send(&app, "PUT", Some(Value::Null)).await;
    assert_eq!(status, StatusCode::OK, "{value}");
    assert_eq!(value, Value::Null);
    assert_eq!(state.read().await.side_model, None);
    let (_, value) = send(&app, "GET", None).await;
    assert_eq!(value, Value::Null);
}

#[tokio::test]
async fn side_model_is_owner_only() {
    let state = Arc::new(RwLock::new(DaemonState::new()));
    let app = router(state, DaemonConfig::default());
    for method in ["GET", "PUT"] {
        let body = (method == "PUT").then(|| json!({"provider": "openai", "model": "m"}));
        let (status, _, _) = send_from(&app, "https://evil.example", method, body).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method}");
    }
}

#[tokio::test]
async fn side_model_survives_a_save_failure_unchanged() {
    // A control plane that cannot be written (the path is a directory) keeps
    // the old choice.
    let invalid = std::env::temp_dir().join(format!(
        "anima-side-model-invalid-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&invalid).unwrap();
    let state = Arc::new(RwLock::new(DaemonState::new()));
    state.write().await.set_control_plane_store(Some(
        crate::control_plane_store::ControlPlaneStoreConfig::Json(invalid.clone()),
    ));
    let app = router(state.clone(), DaemonConfig::default());
    let (status, value) = send(
        &app,
        "PUT",
        Some(json!({"provider": "openai", "model": "gpt-x"})),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{value}");
    assert_eq!(state.read().await.side_model, None);
    let _ = std::fs::remove_dir_all(invalid);
}
