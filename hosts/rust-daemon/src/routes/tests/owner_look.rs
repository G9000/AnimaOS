//! The owner's console look over HTTP: owner-only, validated, saved with
//! the control plane, and the same record `change_look` changes.
use super::*;

use serde_json::{json, Value};

const OWNER_ORIGIN: &str = "http://localhost:4200";
const PATH: &str = "/api/settings/look";

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
async fn look_round_trips_and_validates() {
    let state = Arc::new(RwLock::new(DaemonState::new()));
    let app = router(state.clone(), DaemonConfig::default());

    let (status, value) = send(&app, "GET", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value, Value::Null, "none until a console or chat saves one");

    let look = json!({
        "theme": "light",
        "accent": "custom",
        "customAccent": "#14b8a6",
        "textSize": "large",
        "density": "compact",
        "reduceMotion": true,
        "advancedInput": true,
        "showWork": true
    });
    let (status, value) = send(&app, "PUT", Some(look.clone())).await;
    assert_eq!(status, StatusCode::OK, "{value}");
    assert_eq!(value, look);
    let (_, value) = send(&app, "GET", None).await;
    assert_eq!(value, look);
    let snapshot = serde_json::to_value(state.read().await.control_plane_snapshot()).unwrap();
    assert_eq!(snapshot["ownerLook"], look);

    for (case, body) in [
        ("unknown theme", json!({"theme": "neon"})),
        ("unknown accent", json!({"accent": "teal"})),
        (
            "custom accent not a colour",
            json!({"customAccent": "teal"}),
        ),
        ("switch as text", json!({"showWork": "yes"})),
    ] {
        let (status, value) = send(&app, "PUT", Some(body)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{case}: {value}");
    }
    let (_, value) = send(&app, "GET", None).await;
    assert_eq!(value, look, "a rejected PUT changes nothing");

    let (status, value) = send(&app, "PUT", Some(json!({"theme": "system"}))).await;
    assert_eq!(status, StatusCode::OK, "{value}");
    assert_eq!(value["theme"], "system");
    assert_eq!(
        value["density"], "comfortable",
        "a missing choice takes its default"
    );

    let (status, value) = send(
        &app,
        "PUT",
        Some(json!({"theme": "dark", "chatBackground": "full"})),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "an older console's look still saves: {value}"
    );
    assert_eq!(value.get("chatBackground"), None);
}

#[tokio::test]
async fn look_is_owner_only() {
    let state = Arc::new(RwLock::new(DaemonState::new()));
    let app = router(state, DaemonConfig::default());
    for method in ["GET", "PUT"] {
        let body = (method == "PUT").then(|| json!({"theme": "light"}));
        let (status, _, _) = send_from(&app, "https://evil.example", method, body).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method}");
    }
}
