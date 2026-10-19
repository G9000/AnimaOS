use super::*;
use crate::{KeySource, KeysResting};
use anima_core::{classify_model_error, ModelFailure, ModelFailureKind};
use axum::http::StatusCode;

/// A key source holding `(id, key)` pairs in order. A key reported with a
/// rotating failure rests for good, like a resting key in the daemon's pool.
struct ScriptedKeys {
    keys: Vec<(String, String)>,
    resting: Mutex<Vec<String>>,
    reports: Mutex<Vec<(String, Option<ModelFailure>)>>,
    picks: AtomicUsize,
    /// What `resting` answers.
    why: Mutex<Option<KeysResting>>,
}

impl ScriptedKeys {
    fn new(keys: &[(&str, &str)]) -> Arc<Self> {
        Arc::new(Self {
            keys: keys
                .iter()
                .map(|(id, key)| ((*id).into(), (*key).into()))
                .collect(),
            resting: Mutex::new(Vec::new()),
            reports: Mutex::new(Vec::new()),
            picks: AtomicUsize::new(0),
            why: Mutex::new(None),
        })
    }

    fn reports(&self) -> Vec<(String, Option<ModelFailureKind>)> {
        self.reports
            .lock()
            .unwrap()
            .iter()
            .map(|(id, failure)| (id.clone(), failure.map(|failure| failure.kind)))
            .collect()
    }
}

impl KeySource for ScriptedKeys {
    fn pick(&self, provider: &str) -> Option<(String, String)> {
        self.picks.fetch_add(1, Ordering::SeqCst);
        if provider != "openai" && provider != "anthropic" {
            return None;
        }
        let resting = self.resting.lock().unwrap();
        self.keys
            .iter()
            .find(|(id, _)| !resting.contains(id))
            .cloned()
    }

    fn holds_keys(&self, provider: &str) -> bool {
        (provider == "openai" || provider == "anthropic") && !self.keys.is_empty()
    }

    fn resting(&self, _provider: &str) -> Option<KeysResting> {
        self.why.lock().unwrap().clone()
    }

    fn report(&self, _provider: &str, key_id: &str, failure: Option<&ModelFailure>) {
        if failure.is_some_and(|failure| failure.kind.rotates_key()) {
            self.resting.lock().unwrap().push(key_id.to_owned());
        }
        self.reports
            .lock()
            .unwrap()
            .push((key_id.to_owned(), failure.copied()));
    }
}

fn adapter_with_keys(base_url: &str, source: &Arc<ScriptedKeys>) -> ProviderModelAdapter {
    ProviderModelAdapter::new(ProviderAdapterConfig {
        providers: BTreeMap::from([(
            "openai".into(),
            ProviderCredential {
                api_key: Some("env-key".into()),
                base_url: format!("{base_url}/v1"),
            },
        )]),
        key_source: Some(source.clone()),
    })
}

/// Answers by bearer token: `(token, status, body)` rows; a JSON success for
/// the token marked 200. Records the tokens it saw.
async fn bearer_server(
    rows: &'static [(&'static str, u16, &'static str)],
) -> (String, Arc<Mutex<Vec<String>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new().route(
        "/v1/chat/completions",
        post({
            let seen = Arc::clone(&seen);
            move |headers: HeaderMap| {
                let seen = Arc::clone(&seen);
                async move {
                    let auth = headers
                        .get("authorization")
                        .and_then(|value| value.to_str().ok())
                        .unwrap_or_default()
                        .to_owned();
                    seen.lock().unwrap().push(auth.clone());
                    let row = rows
                        .iter()
                        .find(|(token, _, _)| auth == format!("Bearer {token}"));
                    match row {
                        Some((_, 200, text)) => openai_response(text).into_response(),
                        Some((_, status, body)) => {
                            (StatusCode::from_u16(*status).unwrap(), *body).into_response()
                        }
                        None => (StatusCode::IM_A_TEAPOT, "unknown token").into_response(),
                    }
                }
            }
        }),
    );
    (spawn_server(app).await, seen)
}

use axum::response::IntoResponse;

#[tokio::test]
async fn a_rate_limited_key_rotates_to_the_next_within_the_call() {
    let (base_url, seen) =
        bearer_server(&[("key-a", 429, "slow down"), ("key-b", 200, "from key b")]).await;
    let source = ScriptedKeys::new(&[("a", "key-a"), ("b", "key-b")]);

    let response = adapter_with_keys(&base_url, &source)
        .generate(&agent_config("openai", false), &request())
        .await
        .expect("the second key should answer");

    assert_eq!(response.content.text, "from key b");
    assert_eq!(
        source.reports(),
        vec![
            ("a".to_owned(), Some(ModelFailureKind::RateLimited)),
            ("b".to_owned(), None)
        ]
    );
    assert_eq!(
        seen.lock().unwrap().as_slice(),
        ["Bearer key-a", "Bearer key-b"]
    );
}

#[tokio::test]
async fn a_rate_limited_key_rotates_to_the_next_within_a_stream() {
    let (base_url, seen) = bearer_server(&[
        ("key-a", 429, "slow down"),
        ("key-b", 200, "streamed from b"),
    ])
    .await;
    let source = ScriptedKeys::new(&[("a", "key-a"), ("b", "key-b")]);
    let sink = FrameSink(Mutex::new(Vec::new()));

    adapter_with_keys(&base_url, &source)
        .stream(&agent_config("openai", false), &request(), &sink)
        .await
        .expect("the second key should answer");

    let frames = sink.0.lock().unwrap();
    assert!(matches!(
        frames.last(),
        Some(ModelStreamFrame::Final(response)) if response.content.text == "streamed from b"
    ));
    assert_eq!(
        source.reports(),
        vec![
            ("a".to_owned(), Some(ModelFailureKind::RateLimited)),
            ("b".to_owned(), None)
        ]
    );
    // The existing single same-key retry of a 429 stream runs before rotating.
    assert_eq!(
        seen.lock().unwrap().as_slice(),
        ["Bearer key-a", "Bearer key-a", "Bearer key-b"]
    );
}

#[tokio::test]
async fn a_billing_failure_rotates_and_a_bad_request_does_not() {
    let (base_url, _) = bearer_server(&[
        ("key-a", 402, "insufficient_quota"),
        ("key-b", 400, "bad request body"),
        ("key-c", 200, "never reached"),
    ])
    .await;
    let source = ScriptedKeys::new(&[("a", "key-a"), ("b", "key-b"), ("c", "key-c")]);

    let error = adapter_with_keys(&base_url, &source)
        .generate(&agent_config("openai", false), &request())
        .await
        .unwrap_err();

    assert_eq!(error, "OpenAI API error (400): bad request body");
    assert_eq!(
        source.reports(),
        vec![
            ("a".to_owned(), Some(ModelFailureKind::Billing)),
            ("b".to_owned(), Some(ModelFailureKind::BadRequest))
        ]
    );
}

#[tokio::test]
async fn all_keys_failing_returns_the_last_error() {
    let (base_url, seen) = bearer_server(&[
        ("key-a", 429, "a is limited"),
        ("key-b", 401, "b is invalid"),
    ])
    .await;
    let source = ScriptedKeys::new(&[("a", "key-a"), ("b", "key-b")]);

    let error = adapter_with_keys(&base_url, &source)
        .generate(&agent_config("openai", false), &request())
        .await
        .unwrap_err();

    assert_eq!(error, "OpenAI API error (401): b is invalid");
    assert_eq!(
        classify_model_error(&error).kind,
        ModelFailureKind::AuthFailed
    );
    assert_eq!(
        source.reports(),
        vec![
            ("a".to_owned(), Some(ModelFailureKind::RateLimited)),
            ("b".to_owned(), Some(ModelFailureKind::AuthFailed))
        ]
    );
    assert_eq!(seen.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn a_source_that_keeps_returning_the_same_key_is_asked_once_per_key() {
    struct Stuck(AtomicUsize);
    impl KeySource for Stuck {
        fn pick(&self, _provider: &str) -> Option<(String, String)> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Some(("only".into(), "key-a".into()))
        }
        fn report(&self, _provider: &str, _key_id: &str, _failure: Option<&ModelFailure>) {}
    }
    let (base_url, seen) = bearer_server(&[("key-a", 429, "slow down")]).await;
    let stuck = Arc::new(Stuck(AtomicUsize::new(0)));
    let adapter = ProviderModelAdapter::new(ProviderAdapterConfig {
        providers: BTreeMap::from([(
            "openai".into(),
            ProviderCredential {
                api_key: None,
                base_url: format!("{base_url}/v1"),
            },
        )]),
        key_source: Some(stuck),
    });

    let error = adapter
        .generate(&agent_config("openai", false), &request())
        .await
        .unwrap_err();

    assert_eq!(error, "OpenAI API error (429): slow down");
    assert_eq!(seen.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn no_key_source_behaves_as_before() {
    let (base_url, seen) = bearer_server(&[("env-key", 200, "env answer")]).await;
    let adapter = adapter_with(&[("openai", Some("env-key"), &format!("{base_url}/v1"))]);

    let response = adapter
        .generate(&agent_config("openai", false), &request())
        .await
        .expect("the configured key should answer");

    assert_eq!(response.content.text, "env answer");
    assert_eq!(seen.lock().unwrap().as_slice(), ["Bearer env-key"]);
}

#[tokio::test]
async fn a_provider_the_source_has_no_key_for_uses_the_configured_credential() {
    let (base_url, seen) = bearer_server(&[("env-key", 200, "env answer")]).await;
    let source = ScriptedKeys::new(&[]);

    let response = adapter_with_keys(&base_url, &source)
        .generate(&agent_config("openai", false), &request())
        .await
        .expect("the configured key should answer");

    assert_eq!(response.content.text, "env answer");
    assert_eq!(seen.lock().unwrap().as_slice(), ["Bearer env-key"]);
    assert!(source.reports().is_empty());
}

#[tokio::test]
async fn a_provider_with_no_key_anywhere_fails_as_it_does_today() {
    let source = ScriptedKeys::new(&[]);
    let adapter = ProviderModelAdapter::new(ProviderAdapterConfig {
        providers: BTreeMap::new(),
        key_source: Some(source),
    });

    let error = adapter
        .generate(&agent_config("openai", false), &request())
        .await
        .unwrap_err();

    assert_eq!(
        error,
        "OPENAI_API_KEY is not configured for host-supplied OpenAI models"
    );
}

fn rate_limit() -> ModelFailure {
    ModelFailure {
        kind: ModelFailureKind::RateLimited,
        status: Some(429),
        retry_after_s: None,
    }
}

#[tokio::test]
async fn a_pool_with_every_key_resting_sends_nothing_and_reports_a_rate_limit() {
    let (base_url, seen) = bearer_server(&[("env-key", 200, "must not be asked")]).await;
    // One key (the env key, as the daemon's pool lists it), already resting.
    let source = ScriptedKeys::new(&[("env", "env-key")]);
    source.report("openai", "env", Some(&rate_limit()));
    let adapter = adapter_with_keys(&base_url, &source);

    let error = adapter
        .generate(&agent_config("openai", false), &request())
        .await
        .unwrap_err();
    assert_eq!(error, "OpenAI API error (429): every key is resting");
    assert_eq!(
        classify_model_error(&error).kind,
        ModelFailureKind::RateLimited
    );

    let sink = FrameSink(Mutex::new(Vec::new()));
    let error = adapter
        .stream(&agent_config("openai", false), &request(), &sink)
        .await
        .unwrap_err();
    assert_eq!(error, "OpenAI API error (429): every key is resting");
    assert!(sink.0.lock().unwrap().is_empty());
    assert!(seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_file_only_pool_with_every_key_resting_is_not_reported_as_unconfigured() {
    let (base_url, seen) = bearer_server(&[("key-a", 200, "must not be asked")]).await;
    let source = ScriptedKeys::new(&[("a", "key-a"), ("b", "key-b")]);
    source.report("openai", "a", Some(&rate_limit()));
    source.report("openai", "b", Some(&rate_limit()));
    // No configured (env) credential at all, as with a file-only pool.
    let adapter = ProviderModelAdapter::new(ProviderAdapterConfig {
        providers: BTreeMap::from([(
            "openai".into(),
            ProviderCredential {
                api_key: None,
                base_url: format!("{base_url}/v1"),
            },
        )]),
        key_source: Some(source),
    });

    let error = adapter
        .generate(&agent_config("openai", false), &request())
        .await
        .unwrap_err();

    assert_eq!(error, "OpenAI API error (429): every key is resting");
    assert!(!error.contains("not configured"));
    assert_eq!(
        classify_model_error(&error).kind,
        ModelFailureKind::RateLimited
    );
    assert!(seen.lock().unwrap().is_empty());
}

fn billing() -> ModelFailure {
    ModelFailure {
        kind: ModelFailureKind::Billing,
        status: Some(402),
        retry_after_s: None,
    }
}

#[tokio::test]
async fn every_key_resting_for_billing_says_402_and_until_when() {
    let (base_url, seen) = bearer_server(&[("env-key", 200, "must not be asked")]).await;
    let source = ScriptedKeys::new(&[("env", "env-key")]);
    source.report("openai", "env", Some(&billing()));
    *source.why.lock().unwrap() = Some(KeysResting {
        kind: ModelFailureKind::Billing,
        ready_in_s: 3540,
        ready_at: Some("14:05".into()),
    });
    let adapter = adapter_with_keys(&base_url, &source);

    let error = adapter
        .generate(&agent_config("openai", false), &request())
        .await
        .unwrap_err();

    assert_eq!(
        error,
        "OpenAI API error (402): every key is resting \u{2014} out of credit until 14:05 (retry after 3540s)"
    );
    let failure = classify_model_error(&error);
    assert_eq!(failure.kind, ModelFailureKind::Billing);
    assert_eq!(failure.retry_after_s, Some(3540));
    assert!(seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn every_key_resting_for_a_rejected_key_says_401() {
    let (base_url, seen) = bearer_server(&[("env-key", 200, "must not be asked")]).await;
    let source = ScriptedKeys::new(&[("env", "env-key")]);
    source.report(
        "openai",
        "env",
        Some(&ModelFailure {
            kind: ModelFailureKind::AuthFailed,
            status: Some(401),
            retry_after_s: None,
        }),
    );
    *source.why.lock().unwrap() = Some(KeysResting {
        kind: ModelFailureKind::AuthFailed,
        ready_in_s: 300,
        ready_at: None,
    });
    let adapter = adapter_with_keys(&base_url, &source);

    let sink = FrameSink(Mutex::new(Vec::new()));
    let error = adapter
        .stream(&agent_config("openai", false), &request(), &sink)
        .await
        .unwrap_err();

    assert_eq!(
        error,
        "OpenAI API error (401): every key is resting \u{2014} key rejected (retry after 300s)"
    );
    let failure = classify_model_error(&error);
    assert_eq!(failure.kind, ModelFailureKind::AuthFailed);
    assert_eq!(failure.retry_after_s, Some(300));
    assert!(sink.0.lock().unwrap().is_empty());
    assert!(seen.lock().unwrap().is_empty());
}

/// Fill-first keys whose first key's short rest has run out by the time the
/// second key fails, as with the daemon's pool.
struct WakingKeys {
    resting: Mutex<Vec<String>>,
}

impl KeySource for WakingKeys {
    fn pick(&self, provider: &str) -> Option<(String, String)> {
        self.pick_untried(provider, &[])
    }

    fn pick_untried(&self, _provider: &str, tried: &[String]) -> Option<(String, String)> {
        let resting = self.resting.lock().unwrap();
        ["a", "b", "c"]
            .into_iter()
            .find(|id| !resting.iter().any(|r| r == id) && !tried.iter().any(|t| t == id))
            .map(|id| (id.to_owned(), format!("key-{id}")))
    }

    fn holds_keys(&self, _provider: &str) -> bool {
        true
    }

    fn report(&self, _provider: &str, key_id: &str, failure: Option<&ModelFailure>) {
        if !failure.is_some_and(|failure| failure.kind.rotates_key()) {
            return;
        }
        let mut resting = self.resting.lock().unwrap();
        resting.push(key_id.to_owned());
        if key_id == "b" {
            // a's one-second rest is over.
            resting.retain(|id| id != "a");
        }
    }
}

#[tokio::test]
async fn rotation_still_tries_an_untried_key_when_a_tried_one_wakes_up() {
    let (base_url, seen) = bearer_server(&[
        ("key-a", 429, "a is limited"),
        ("key-b", 429, "b is limited"),
        ("key-c", 200, "from key c"),
    ])
    .await;
    let adapter = ProviderModelAdapter::new(ProviderAdapterConfig {
        providers: BTreeMap::from([(
            "openai".into(),
            ProviderCredential {
                api_key: None,
                base_url: format!("{base_url}/v1"),
            },
        )]),
        key_source: Some(Arc::new(WakingKeys {
            resting: Mutex::new(Vec::new()),
        })),
    });

    let response = adapter
        .generate(&agent_config("openai", false), &request())
        .await
        .expect("key c should answer");

    assert_eq!(response.content.text, "from key c");
    assert_eq!(
        seen.lock().unwrap().as_slice(),
        ["Bearer key-a", "Bearer key-b", "Bearer key-c"]
    );
}

#[tokio::test]
async fn a_rotating_failure_after_streamed_text_neither_rotates_nor_resends() {
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let app = Router::new().route(
        "/v1/messages",
        post({
            let seen = Arc::clone(&seen);
            move |headers: HeaderMap| {
                let seen = Arc::clone(&seen);
                async move {
                    let key = headers
                        .get("x-api-key")
                        .and_then(|value| value.to_str().ok())
                        .unwrap_or_default()
                        .to_owned();
                    seen.lock().unwrap().push(key);
                    (
                        [("content-type", "text/event-stream")],
                        concat!(
                            "data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":9}}}\n\n",
                            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
                            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Half\"}}\n\n",
                            "data: {\"type\":\"error\",\"error\":{\"type\":\"billing_error\",\"message\":\"out of credit\"}}\n\n"
                        ),
                    )
                }
            }
        }),
    );
    let base_url = spawn_server(app).await;
    let source = ScriptedKeys::new(&[("a", "key-a"), ("b", "key-b")]);
    let adapter = ProviderModelAdapter::new(ProviderAdapterConfig {
        providers: BTreeMap::from([(
            "anthropic".into(),
            ProviderCredential {
                api_key: None,
                base_url,
            },
        )]),
        key_source: Some(source.clone()),
    });
    let sink = FrameSink(Mutex::new(Vec::new()));

    let error = adapter
        .stream(&agent_config("anthropic", false), &request(), &sink)
        .await
        .unwrap_err();

    assert_eq!(error, "Anthropic stream error: billing_error");
    assert_eq!(classify_model_error(&error).kind, ModelFailureKind::Billing);
    assert!(sink
        .0
        .lock()
        .unwrap()
        .contains(&ModelStreamFrame::TextDelta("Half".into())));
    // One request, on key a; key b was never tried.
    assert_eq!(seen.lock().unwrap().as_slice(), ["key-a"]);
    assert_eq!(
        source.reports(),
        vec![("a".to_owned(), Some(ModelFailureKind::Billing))]
    );
}
