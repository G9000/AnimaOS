//! The provider key pool over HTTP: owner-only, masked, validated, and the
//! providers list reports `configured` from the pool.
use super::*;

use std::collections::BTreeMap;

use serde_json::{json, Value};

use crate::provider_keys::KeyPool;

const OWNER_ORIGIN: &str = "http://localhost:4200";
const KEY: &str = "sk-test-abcdefghijklmnop1234";

async fn send_from(
    app: &axum::Router,
    origin: &str,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Option<String>, String) {
    let builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("host", "127.0.0.1:8080")
        .header("origin", origin);
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
    (status, cache, String::from_utf8(bytes.to_vec()).unwrap())
}

/// An owner request; every answer, errors included, must be `no-store`, and
/// no answer may carry the full key.
async fn send(
    app: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value, String) {
    let (status, cache, text) = send_from(app, OWNER_ORIGIN, method, uri, body).await;
    assert_eq!(
        cache.as_deref(),
        Some("no-store"),
        "{method} {uri} {status}"
    );
    assert!(!text.contains(KEY), "{method} {uri} echoed the key: {text}");
    let value = serde_json::from_str(&text).unwrap_or(Value::Null);
    (status, value, text)
}

fn app() -> (axum::Router, Arc<RwLock<DaemonState>>) {
    let state = Arc::new(RwLock::new(DaemonState::new()));
    (router(state.clone(), DaemonConfig::default()), state)
}

fn provider<'a>(list: &'a Value, id: &str) -> &'a Value {
    list["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|provider| provider["id"] == id)
        .unwrap_or_else(|| panic!("provider {id} in {list}"))
}

#[tokio::test]
async fn keys_are_added_listed_masked_and_removed() {
    let (app, _) = app();

    let (status, list, _) = send(&app, "GET", "/api/provider-keys", None).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    let openai = provider(&list, "openai");
    assert_eq!(openai["keys"], json!([]));
    assert_eq!(openai["strategy"], "fill_first");
    assert_eq!(openai["persisted"], false, "no key file in this daemon");

    let (status, added, text) = send(
        &app,
        "POST",
        "/api/provider-keys/openai",
        Some(json!({"label": " Work ", "key": format!(" {KEY} ")})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{text}");
    assert_eq!(added["label"], "Work");
    assert_eq!(added["masked"], "sk-tes…1234");
    assert_eq!(added["source"], "file");
    assert_eq!(added["status"], "ready");
    assert_eq!(added["uses"], 0);
    let key_id = added["id"].as_str().unwrap().to_string();
    assert!(key_id.starts_with("key_"));
    assert!(added.get("key").is_none(), "{added}");

    let (_, list, text) = send(&app, "GET", "/api/provider-keys", None).await;
    let openai = provider(&list, "openai");
    assert_eq!(openai["keys"].as_array().unwrap().len(), 1, "{text}");
    assert_eq!(openai["keys"][0]["id"], key_id.as_str());
    assert_eq!(openai["keys"][0]["masked"], "sk-tes…1234");
    assert_eq!(openai["persisted"], false);

    let (status, value, text) = send(
        &app,
        "DELETE",
        &format!("/api/provider-keys/openai/{key_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{text}");
    assert_eq!(value, json!({"deleted": true}));
    let (_, list, _) = send(&app, "GET", "/api/provider-keys", None).await;
    assert_eq!(provider(&list, "openai")["keys"], json!([]));

    let (status, _, _) = send(
        &app,
        "DELETE",
        &format!("/api/provider-keys/openai/{key_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "already removed");
}

#[tokio::test]
async fn strategy_is_set_per_provider() {
    let (app, _) = app();
    let (status, value, text) = send(
        &app,
        "PUT",
        "/api/provider-keys/anthropic/strategy",
        Some(json!({"strategy": "round_robin"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{text}");
    assert_eq!(value["id"], "anthropic");
    assert_eq!(value["strategy"], "round_robin");
    let (_, list, _) = send(&app, "GET", "/api/provider-keys", None).await;
    assert_eq!(provider(&list, "anthropic")["strategy"], "round_robin");
    assert_eq!(provider(&list, "openai")["strategy"], "fill_first");
}

#[tokio::test]
async fn key_routes_require_the_owner() {
    let (app, state) = app();
    let key_id = state
        .read()
        .await
        .provider_keys
        .add("openai", None, KEY.to_string(), 1)
        .unwrap()
        .id;
    let cases = [
        ("GET", "/api/provider-keys".to_string(), None),
        (
            "POST",
            "/api/provider-keys/openai".to_string(),
            Some(json!({"key": "sk-other-abcdefghijkl"})),
        ),
        (
            "DELETE",
            format!("/api/provider-keys/openai/{key_id}"),
            None,
        ),
        (
            "PUT",
            "/api/provider-keys/openai/strategy".to_string(),
            Some(json!({"strategy": "random"})),
        ),
        (
            "POST",
            format!("/api/provider-keys/openai/{key_id}/wake"),
            None,
        ),
    ];
    for (method, uri, body) in cases {
        let (status, cache, text) =
            send_from(&app, "https://evil.example", method, &uri, body).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {uri}");
        assert_eq!(cache.as_deref(), Some("no-store"), "{method} {uri}");
        assert!(!text.contains(KEY));
    }
    // Nothing the foreign origin sent changed the pool.
    let pool = state.read().await.provider_keys.clone();
    let openai = pool
        .list(1)
        .into_iter()
        .find(|view| view.id == "openai")
        .unwrap();
    assert_eq!(openai.keys.len(), 1);
    assert_eq!(openai.keys[0].id, key_id);
}

#[tokio::test]
async fn bad_keys_and_strategies_are_rejected() {
    let state = Arc::new(RwLock::new(DaemonState::new()));
    state.write().await.provider_keys = KeyPool::open(
        None,
        &BTreeMap::from([("openai".to_string(), "sk-env-0123456789abcdef".to_string())]),
    )
    .unwrap();
    let app = router(state, DaemonConfig::default());

    for (case, uri, body) in [
        (
            "unknown provider",
            "/api/provider-keys/nope",
            json!({"key": KEY}),
        ),
        ("empty key", "/api/provider-keys/openai", json!({"key": ""})),
        (
            "blank key",
            "/api/provider-keys/openai",
            json!({"key": "   "}),
        ),
        (
            "key past 2,000 characters",
            "/api/provider-keys/openai",
            json!({"key": "k".repeat(2_001)}),
        ),
        (
            "extra field",
            "/api/provider-keys/openai",
            json!({"key": KEY, "extra": 1}),
        ),
        ("missing key", "/api/provider-keys/openai", json!({})),
    ] {
        let (status, _, text) = send(&app, "POST", uri, Some(body)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{case}: {text}");
    }
    let (status, _, _) = send(&app, "POST", "/api/provider-keys/openai", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "no body");

    let (status, _, text) = send(
        &app,
        "POST",
        "/api/provider-keys/openai",
        Some(json!({"key": KEY})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{text}");
    let (status, _, text) = send(
        &app,
        "POST",
        "/api/provider-keys/openai",
        Some(json!({"key": KEY})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "duplicate: {text}");
    let (status, _, text) = send(
        &app,
        "POST",
        "/api/provider-keys/openai",
        Some(json!({"key": "sk-env-0123456789abcdef"})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "the env key again: {text}");

    let (status, _, text) = send(&app, "DELETE", "/api/provider-keys/openai/env", None).await;
    assert_eq!(status, StatusCode::CONFLICT, "env key: {text}");
    let (status, _, _) = send(&app, "DELETE", "/api/provider-keys/openai/key_nope", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _, _) = send(&app, "DELETE", "/api/provider-keys/nope/key_nope", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    for (case, uri, body) in [
        (
            "bad strategy",
            "/api/provider-keys/openai/strategy",
            json!({"strategy": "sideways"}),
        ),
        (
            "camel case is not a strategy",
            "/api/provider-keys/openai/strategy",
            json!({"strategy": "fillFirst"}),
        ),
        (
            "unknown provider",
            "/api/provider-keys/nope/strategy",
            json!({"strategy": "random"}),
        ),
        (
            "extra field",
            "/api/provider-keys/openai/strategy",
            json!({"strategy": "random", "extra": 1}),
        ),
        (
            "missing strategy",
            "/api/provider-keys/openai/strategy",
            json!({}),
        ),
    ] {
        let (status, _, text) = send(&app, "PUT", uri, Some(body)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{case}: {text}");
    }

    // The rejected calls changed nothing: the env key and the one added key.
    let (_, list, _) = send(&app, "GET", "/api/provider-keys", None).await;
    let openai = provider(&list, "openai");
    assert_eq!(openai["keys"].as_array().unwrap().len(), 2, "{list}");
    assert_eq!(openai["keys"][0]["source"], "env");
    assert_eq!(openai["strategy"], "fill_first");
}

#[tokio::test]
async fn providers_report_configured_from_the_pool() {
    let (app, _) = app();
    let configured = |list: &Value, id: &str| {
        list["providers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|provider| provider["id"] == id)
            .unwrap()["configured"]
            .as_bool()
            .unwrap()
    };
    // A provider this test process has no environment key for.
    let id = crate::runtime_model::provider_summaries()
        .into_iter()
        .find(|summary| summary.requires_key && !summary.configured)
        .map(|summary| summary.id)
        .expect("a key provider without an environment key");

    let (_, _, text) = send_from(&app, OWNER_ORIGIN, "GET", "/api/providers", None).await;
    let list: Value = serde_json::from_str(&text).unwrap();
    assert!(!configured(&list, id), "{id} has no key yet");

    let (status, added, text) = send(
        &app,
        "POST",
        &format!("/api/provider-keys/{id}"),
        Some(json!({"key": KEY})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{text}");
    let (_, _, text) = send_from(&app, OWNER_ORIGIN, "GET", "/api/providers", None).await;
    let list: Value = serde_json::from_str(&text).unwrap();
    assert!(configured(&list, id), "a pooled key configures {id}");
    assert!(!text.contains(KEY));

    let key_id = added["id"].as_str().unwrap();
    send(
        &app,
        "DELETE",
        &format!("/api/provider-keys/{id}/{key_id}"),
        None,
    )
    .await;
    let (_, _, text) = send_from(&app, OWNER_ORIGIN, "GET", "/api/providers", None).await;
    let list: Value = serde_json::from_str(&text).unwrap();
    assert!(!configured(&list, id), "removing the key unconfigures {id}");
}

#[tokio::test]
async fn try_now_wakes_a_resting_key_including_the_env_key() {
    let state = Arc::new(RwLock::new(DaemonState::new()));
    let pool = KeyPool::open(
        None,
        &BTreeMap::from([("openai".to_string(), "sk-env-0123456789abcdef".to_string())]),
    )
    .unwrap();
    let now = anima_core::primitives::now_millis();
    pool.report(
        "openai",
        "env",
        crate::provider_keys::KeyOutcome::Failed(anima_core::ModelFailure {
            kind: anima_core::ModelFailureKind::Billing,
            status: Some(402),
            retry_after_s: None,
        }),
        now,
    );
    state.write().await.provider_keys = pool.clone();
    let app = router(state, DaemonConfig::default());

    let (status, key, _) = send(&app, "POST", "/api/provider-keys/OpenAI/env/wake", None).await;
    assert_eq!(status, StatusCode::OK, "{key}");
    assert_eq!(key["id"], "env");
    assert_eq!(key["status"], "ready");
    assert_eq!(key["restingUntilMs"], Value::Null);
    assert_eq!(key["lastError"], Value::Null);
    assert_eq!(pool.pick("openai", now + 1).unwrap().id, "env");

    let (status, _, _) = send(
        &app,
        "POST",
        "/api/provider-keys/openai/key_missing/wake",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
