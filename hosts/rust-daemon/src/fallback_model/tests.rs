use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};

use anima_core::{
    AgentConfig, Content, ModelAdapter, ModelChoice, ModelGenerateRequest, ModelGenerateResponse,
    ModelStopReason, ModelStreamFrame, ModelStreamSink, TokenUsage,
};
use async_trait::async_trait;

use super::FallbackModelAdapter;

/// One scripted call on one provider/model.
#[derive(Clone)]
enum Outcome {
    /// Finishes with this text, streaming no deltas.
    Answer(&'static str),
    /// Streams this text, then fails with the error.
    TextThenFail(&'static str, &'static str),
    /// Fails with the error before streaming anything.
    Fail(&'static str),
}

/// Answers each call from the script for the call's `provider/model`
/// (`Answer("ok")` once that script runs out) and records which ones it got.
#[derive(Default)]
struct Scripted {
    scripts: Mutex<BTreeMap<String, VecDeque<Outcome>>>,
    calls: Mutex<Vec<String>>,
}

impl Scripted {
    fn new(scripts: &[(&str, Vec<Outcome>)]) -> Arc<Self> {
        Arc::new(Self {
            scripts: Mutex::new(
                scripts
                    .iter()
                    .map(|(key, outcomes)| (key.to_string(), outcomes.clone().into()))
                    .collect(),
            ),
            calls: Mutex::default(),
        })
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    fn next(&self, config: &AgentConfig) -> Outcome {
        let key = format!(
            "{}/{}",
            config.provider.as_deref().unwrap_or_default(),
            config.model
        );
        self.calls.lock().unwrap().push(key.clone());
        self.scripts
            .lock()
            .unwrap()
            .get_mut(&key)
            .and_then(VecDeque::pop_front)
            .unwrap_or(Outcome::Answer("ok"))
    }
}

fn response(text: &str) -> ModelGenerateResponse {
    ModelGenerateResponse {
        content: Content {
            text: text.into(),
            ..Content::default()
        },
        tool_calls: None,
        usage: TokenUsage::default(),
        stop_reason: ModelStopReason::End,
    }
}

#[async_trait]
impl ModelAdapter for Scripted {
    fn provider(&self) -> &str {
        "scripted"
    }

    async fn generate(
        &self,
        config: &AgentConfig,
        _request: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        match self.next(config) {
            Outcome::Answer(text) => Ok(response(text)),
            Outcome::TextThenFail(_, error) | Outcome::Fail(error) => Err(error.into()),
        }
    }

    async fn stream(
        &self,
        config: &AgentConfig,
        _request: &ModelGenerateRequest,
        sink: &dyn ModelStreamSink,
    ) -> Result<(), String> {
        match self.next(config) {
            Outcome::Answer(text) => sink.emit(ModelStreamFrame::Final(response(text))).await,
            Outcome::TextThenFail(text, error) => {
                sink.emit(ModelStreamFrame::TextDelta(text.into())).await?;
                Err(error.into())
            }
            Outcome::Fail(error) => Err(error.into()),
        }
    }
}

#[derive(Default)]
struct Frames(Mutex<Vec<ModelStreamFrame>>);

impl Frames {
    fn take(&self) -> Vec<ModelStreamFrame> {
        std::mem::take(&mut self.0.lock().unwrap())
    }
}

#[async_trait]
impl ModelStreamSink for Frames {
    async fn emit(&self, frame: ModelStreamFrame) -> Result<(), String> {
        self.0.lock().unwrap().push(frame);
        Ok(())
    }
}

fn config(fallbacks: &[(&str, &str)]) -> AgentConfig {
    AgentConfig {
        name: "agent".into(),
        model: "gpt-a".into(),
        bio: None,
        lore: None,
        knowledge: None,
        topics: None,
        adjectives: None,
        style: None,
        provider: Some("openai".into()),
        system: None,
        tools: None,
        plugins: None,
        settings: None,
        fallbacks: fallbacks
            .iter()
            .map(|(provider, model)| ModelChoice {
                provider: (*provider).into(),
                model: (*model).into(),
            })
            .collect(),
    }
}

fn request() -> ModelGenerateRequest {
    ModelGenerateRequest {
        system: String::new(),
        messages: Vec::new(),
        temperature: None,
        max_tokens: None,
    }
}

fn served(provider: &str, model: &str, reason: &str) -> ModelStreamFrame {
    ModelStreamFrame::Served {
        provider: provider.into(),
        model: model.into(),
        reason: reason.into(),
    }
}

const RATE_LIMITED: &str = "OpenAI API error (429): x";

#[tokio::test]
async fn falls_back_before_any_text_and_says_why() {
    let inner = Scripted::new(&[("openai/gpt-a", vec![Outcome::Fail(RATE_LIMITED)])]);
    let adapter = FallbackModelAdapter::new(inner.clone());
    let frames = Frames::default();

    adapter
        .stream(&config(&[("anthropic", "claude-b")]), &request(), &frames)
        .await
        .unwrap();

    assert_eq!(
        frames.take(),
        vec![
            served("anthropic", "claude-b", "was rate limited"),
            ModelStreamFrame::Final(response("ok")),
        ]
    );
    assert_eq!(inner.calls(), vec!["openai/gpt-a", "anthropic/claude-b"]);
}

#[tokio::test]
async fn a_mid_stream_overloaded_error_before_any_text_falls_back() {
    let inner = Scripted::new(&[(
        "openai/gpt-a",
        vec![Outcome::Fail("Anthropic stream error: overloaded_error")],
    )]);
    let adapter = FallbackModelAdapter::new(inner.clone());
    let frames = Frames::default();

    adapter
        .stream(&config(&[("anthropic", "claude-b")]), &request(), &frames)
        .await
        .unwrap();

    assert_eq!(
        frames.take(),
        vec![
            served("anthropic", "claude-b", "was overloaded"),
            ModelStreamFrame::Final(response("ok")),
        ]
    );
    assert_eq!(inner.calls(), vec!["openai/gpt-a", "anthropic/claude-b"]);
}

#[tokio::test]
async fn served_reason_names_what_happened_to_the_primary() {
    let cases = [
        ("OpenAI API error (429): x", "was rate limited"),
        ("OpenAI API error (529): busy", "was overloaded"),
        ("Model call timed out", "was unavailable"),
        ("OpenAI request failed: connection reset", "was unavailable"),
        (
            "OpenAI API error (402): insufficient_quota",
            "was out of credit",
        ),
        ("OpenAI API error (401): bad key", "did not accept its key"),
        (
            "OpenAI API error (404): model_not_found",
            "did not have that model",
        ),
    ];
    for (error, reason) in cases {
        let inner = Scripted::new(&[("openai/gpt-a", vec![Outcome::Fail(error)])]);
        let adapter = FallbackModelAdapter::new(inner);
        let frames = Frames::default();
        adapter
            .stream(&config(&[("anthropic", "claude-b")]), &request(), &frames)
            .await
            .unwrap();
        assert_eq!(
            frames.take().first(),
            Some(&served("anthropic", "claude-b", reason)),
            "{error}"
        );
    }
}

#[tokio::test]
async fn does_not_fall_back_after_text() {
    let inner = Scripted::new(&[(
        "openai/gpt-a",
        vec![Outcome::TextThenFail("par", "OpenAI API error (503): down")],
    )]);
    let adapter = FallbackModelAdapter::new(inner.clone());
    let frames = Frames::default();

    let error = adapter
        .stream(&config(&[("anthropic", "claude-b")]), &request(), &frames)
        .await
        .unwrap_err();

    assert_eq!(error, "OpenAI API error (503): down");
    assert_eq!(inner.calls(), vec!["openai/gpt-a"]);
    assert_eq!(
        frames.take(),
        vec![ModelStreamFrame::TextDelta("par".into())]
    );
}

#[tokio::test]
async fn does_not_fall_back_on_bad_request_or_context_too_long() {
    for error in [
        "OpenAI API error (400): bad tool schema",
        "OpenAI API error (400): context_length_exceeded",
    ] {
        let inner = Scripted::new(&[("openai/gpt-a", vec![Outcome::Fail(error)])]);
        let adapter = FallbackModelAdapter::new(inner.clone());

        let streamed = adapter
            .stream(
                &config(&[("anthropic", "claude-b")]),
                &request(),
                &Frames::default(),
            )
            .await
            .unwrap_err();

        assert_eq!(streamed, error);
        assert_eq!(inner.calls(), vec!["openai/gpt-a"], "{error}");
    }
}

#[tokio::test]
async fn sticks_to_the_working_fallback_within_a_run() {
    let inner = Scripted::new(&[("openai/gpt-a", vec![Outcome::Fail(RATE_LIMITED)])]);
    let adapter = FallbackModelAdapter::new(inner.clone());
    let config = config(&[("anthropic", "claude-b")]);
    adapter
        .stream(&config, &request(), &Frames::default())
        .await
        .unwrap();

    let frames = Frames::default();
    adapter.stream(&config, &request(), &frames).await.unwrap();

    assert_eq!(
        inner.calls(),
        vec!["openai/gpt-a", "anthropic/claude-b", "anthropic/claude-b"]
    );
    assert_eq!(
        frames.take(),
        vec![
            served("anthropic", "claude-b", "was rate limited"),
            ModelStreamFrame::Final(response("ok")),
        ]
    );

    // A new run (a new adapter over the same inner model) starts at the primary.
    FallbackModelAdapter::new(inner.clone())
        .stream(&config, &request(), &Frames::default())
        .await
        .unwrap();
    assert_eq!(inner.calls().last().unwrap(), "openai/gpt-a");
}

#[tokio::test]
async fn all_failing_lists_what_was_tried() {
    let inner = Scripted::new(&[
        ("openai/gpt-a", vec![Outcome::Fail(RATE_LIMITED)]),
        (
            "anthropic/claude-b",
            vec![Outcome::Fail("Anthropic API error (529): overloaded")],
        ),
        (
            "google/gemini-c",
            vec![Outcome::Fail("Google API error (503): unavailable")],
        ),
    ]);
    let adapter = FallbackModelAdapter::new(inner.clone());

    let error = adapter
        .stream(
            &config(&[("anthropic", "claude-b"), ("google", "gemini-c")]),
            &request(),
            &Frames::default(),
        )
        .await
        .unwrap_err();

    assert_eq!(
        error,
        "OpenAI API error (429): x (also tried: anthropic/claude-b, google/gemini-c)"
    );
    assert_eq!(inner.calls().len(), 3);
}

#[tokio::test]
async fn later_choices_are_tried_whatever_their_own_error() {
    let inner = Scripted::new(&[
        ("openai/gpt-a", vec![Outcome::Fail(RATE_LIMITED)]),
        (
            "anthropic/claude-b",
            vec![Outcome::Fail(
                "Anthropic API error (400): prompt is too long",
            )],
        ),
        (
            "google/gemini-c",
            vec![Outcome::Fail("Google API error (400): bad request")],
        ),
    ]);
    let adapter = FallbackModelAdapter::new(inner.clone());

    let error = adapter
        .stream(
            &config(&[("anthropic", "claude-b"), ("google", "gemini-c")]),
            &request(),
            &Frames::default(),
        )
        .await
        .unwrap_err();

    assert_eq!(
        error,
        "OpenAI API error (429): x (also tried: anthropic/claude-b, google/gemini-c)"
    );
    assert_eq!(
        inner.calls(),
        vec!["openai/gpt-a", "anthropic/claude-b", "google/gemini-c"]
    );
}

#[tokio::test]
async fn an_unconfigured_only_fallback_still_reports_the_primary_error() {
    let inner = Scripted::new(&[
        ("openai/gpt-a", vec![Outcome::Fail(RATE_LIMITED)]),
        (
            "anthropic/claude-b",
            vec![Outcome::Fail(
                "ANTHROPIC_API_KEY is not configured for provider anthropic",
            )],
        ),
    ]);
    let adapter = FallbackModelAdapter::new(inner);

    let error = adapter
        .stream(
            &config(&[("anthropic", "claude-b")]),
            &request(),
            &Frames::default(),
        )
        .await
        .unwrap_err();

    assert!(error.starts_with(RATE_LIMITED), "{error}");
    assert!(
        error.ends_with("(also tried: anthropic/claude-b)"),
        "{error}"
    );
}

#[tokio::test]
async fn a_bad_request_on_one_fallback_moves_on_to_the_next() {
    let inner = Scripted::new(&[
        ("openai/gpt-a", vec![Outcome::Fail(RATE_LIMITED)]),
        (
            "anthropic/claude-b",
            vec![Outcome::Fail("Anthropic API error (400): bad tool schema")],
        ),
    ]);
    let adapter = FallbackModelAdapter::new(inner.clone());
    let frames = Frames::default();

    adapter
        .stream(
            &config(&[("anthropic", "claude-b"), ("google", "gemini-c")]),
            &request(),
            &frames,
        )
        .await
        .unwrap();

    assert_eq!(
        frames.take(),
        vec![
            served("google", "gemini-c", "was rate limited"),
            ModelStreamFrame::Final(response("ok")),
        ]
    );
    assert_eq!(
        inner.calls(),
        vec!["openai/gpt-a", "anthropic/claude-b", "google/gemini-c"]
    );
}

#[tokio::test]
async fn no_fallbacks_is_unchanged() {
    let inner = Scripted::new(&[("openai/gpt-a", vec![Outcome::Fail(RATE_LIMITED)])]);
    let adapter = FallbackModelAdapter::new(inner.clone());
    let frames = Frames::default();

    let error = adapter
        .stream(&config(&[]), &request(), &frames)
        .await
        .unwrap_err();
    assert_eq!(error, RATE_LIMITED);

    adapter
        .stream(&config(&[]), &request(), &frames)
        .await
        .unwrap();
    assert_eq!(frames.take(), vec![ModelStreamFrame::Final(response("ok"))]);
    assert_eq!(inner.calls(), vec!["openai/gpt-a", "openai/gpt-a"]);
}

#[tokio::test]
async fn generate_falls_back_and_sticks_too() {
    let inner = Scripted::new(&[("openai/gpt-a", vec![Outcome::Fail(RATE_LIMITED)])]);
    let adapter = FallbackModelAdapter::new(inner.clone());
    let config = config(&[("anthropic", "claude-b")]);

    assert_eq!(
        adapter.generate(&config, &request()).await.unwrap(),
        response("ok")
    );
    adapter.generate(&config, &request()).await.unwrap();

    assert_eq!(
        inner.calls(),
        vec!["openai/gpt-a", "anthropic/claude-b", "anthropic/claude-b"]
    );
}
