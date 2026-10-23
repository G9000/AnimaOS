//! Per-agent fallback chain for a run's model calls.
//!
//! A call that fails with a fallback-worthy error before streaming any text is
//! retried on the agent's next listed model. Once a fallback answers, the rest
//! of the run starts there; each run gets a fresh adapter, so the next run
//! starts at the primary again.

#[cfg(test)]
#[path = "fallback_model/tests.rs"]
mod tests;

use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use anima_core::{
    classify_model_error, AgentConfig, LockRecover, ModelAdapter, ModelFailureKind,
    ModelGenerateRequest, ModelGenerateResponse, ModelStreamFrame, ModelStreamSink,
};
use async_trait::async_trait;

/// Tries the agent's fallbacks when its model fails before any text streams.
pub(crate) struct FallbackModelAdapter {
    inner: Arc<dyn ModelAdapter>,
    sticky: Arc<Mutex<Option<Sticky>>>,
}

/// The fallback that last answered in this run, and why the primary did not.
#[derive(Clone, Copy)]
struct Sticky {
    index: usize,
    reason: &'static str,
}

/// What the owner is told about the primary when a fallback answered.
fn reason_for(kind: ModelFailureKind) -> &'static str {
    match kind {
        ModelFailureKind::RateLimited => "was rate limited",
        ModelFailureKind::Overloaded => "was overloaded",
        ModelFailureKind::Billing => "was out of credit",
        ModelFailureKind::AuthFailed => "did not accept its key",
        ModelFailureKind::ModelNotFound => "did not have that model",
        ModelFailureKind::Timeout
        | ModelFailureKind::Transport
        | ModelFailureKind::ContextTooLong
        | ModelFailureKind::BadRequest
        | ModelFailureKind::Other => "was unavailable",
    }
}

/// The config each choice runs with: index 0 is the agent's own model.
fn choice_configs(config: &AgentConfig) -> Vec<AgentConfig> {
    std::iter::once(config.clone())
        .chain(config.fallbacks.iter().map(|choice| AgentConfig {
            provider: Some(choice.provider.clone()),
            model: choice.model.clone(),
            ..config.clone()
        }))
        .collect()
}

fn label(config: &AgentConfig) -> String {
    format!(
        "{}/{}",
        config.provider.as_deref().unwrap_or_default(),
        config.model
    )
}

impl FallbackModelAdapter {
    pub(crate) fn new(inner: Arc<dyn ModelAdapter>) -> Self {
        Self {
            inner,
            sticky: Arc::new(Mutex::new(None)),
        }
    }

    /// Runs `attempt` down the chain from where this run last answered.
    /// `attempt` gets the choice's config and, for a fallback, the `Served`
    /// frame to send before `Final`; it reports whether the call already gave
    /// the consumer output.
    async fn through_chain<T, F, Fut>(&self, config: &AgentConfig, attempt: F) -> Result<T, String>
    where
        F: Fn(AgentConfig, Option<ModelStreamFrame>) -> Fut,
        Fut: Future<Output = (Result<T, String>, bool)>,
    {
        let choices = choice_configs(config);
        let sticky = *self.sticky.lock_recover();
        let (start, reason) = match sticky {
            Some(sticky) if sticky.index < choices.len() => (sticky.index, Some(sticky.reason)),
            _ => (0, None),
        };
        let (value, answered) = down_the_chain(&choices, start, reason, attempt).await?;
        if let Some(answered) = answered {
            *self.sticky.lock_recover() = Some(answered);
        }
        Ok(value)
    }
}

/// Runs `call` with the agent's own model and then, by a run's rules, each of
/// its fallbacks; for one-off calls outside a run, so nothing is sticky.
pub(crate) async fn call_with_fallbacks<T, F, Fut>(
    config: &AgentConfig,
    call: F,
) -> Result<T, String>
where
    F: Fn(AgentConfig) -> Fut,
    Fut: Future<Output = Result<T, String>>,
{
    let choices = choice_configs(config);
    let attempt = |choice, _served| {
        let call = call(choice);
        async move { (call.await, false) }
    };
    down_the_chain(&choices, 0, None, attempt)
        .await
        .map(|(value, _)| value)
}

/// Tries `choices` from `start` until one answers. Also says which fallback
/// answered, and why the first choice did not, when it was not `choices[0]`.
async fn down_the_chain<T, F, Fut>(
    choices: &[AgentConfig],
    start: usize,
    mut reason: Option<&'static str>,
    attempt: F,
) -> Result<(T, Option<Sticky>), String>
where
    F: Fn(AgentConfig, Option<ModelStreamFrame>) -> Fut,
    Fut: Future<Output = (Result<T, String>, bool)>,
{
    let mut first_error = None;
    for (index, choice) in choices.iter().enumerate().skip(start) {
        let served = (index > 0).then(|| ModelStreamFrame::Served {
            provider: choice.provider.clone().unwrap_or_default(),
            model: choice.model.clone(),
            reason: reason.unwrap_or("was unavailable").to_owned(),
        });
        let (result, committed) = attempt(choice.clone(), served).await;
        let error = match result {
            Ok(value) => {
                let answered = (index > 0).then(|| Sticky {
                    index,
                    reason: reason.unwrap_or("was unavailable"),
                });
                return Ok((value, answered));
            }
            Err(error) => error,
        };
        let kind = classify_model_error(&error).kind;
        // Only the first model tried decides whether to fall back at all;
        // after that every choice is tried, whatever its own error.
        if committed || (index == start && !kind.falls_back()) {
            return Err(error);
        }
        if index + 1 < choices.len() {
            tracing::warn!(
                from = %label(choice),
                to = %label(&choices[index + 1]),
                ?kind,
                "model call failed before any text; trying the agent's next model"
            );
        }
        reason.get_or_insert(reason_for(kind));
        first_error.get_or_insert(error);
    }
    let first_error = first_error.expect("the chain has at least the agent's own model");
    if choices.len() - start == 1 {
        return Err(first_error);
    }
    let also_tried = choices[start + 1..]
        .iter()
        .map(label)
        .collect::<Vec<_>>()
        .join(", ");
    Err(format!("{first_error} (also tried: {also_tried})"))
}

/// Forwards one attempt's frames, noting whether any output reached the
/// consumer and sending `Served` just before `Final`.
struct AttemptSink<'a> {
    sink: &'a dyn ModelStreamSink,
    served: Mutex<Option<ModelStreamFrame>>,
    committed: AtomicBool,
}

#[async_trait]
impl ModelStreamSink for AttemptSink<'_> {
    async fn emit(&self, frame: ModelStreamFrame) -> Result<(), String> {
        match &frame {
            ModelStreamFrame::TextDelta(text) if !text.is_empty() => {
                self.committed.store(true, Ordering::SeqCst);
            }
            ModelStreamFrame::Final(_) => {
                self.committed.store(true, Ordering::SeqCst);
                let served = self.served.lock_recover().take();
                if let Some(served) = served {
                    self.sink.emit(served).await?;
                }
            }
            _ => {}
        }
        self.sink.emit(frame).await
    }
    async fn complete_usage(&self, usage: anima_core::TokenUsage) -> Result<(), String> {
        self.sink.complete_usage(usage).await
    }
}

#[async_trait]
impl ModelAdapter for FallbackModelAdapter {
    fn provider(&self) -> &str {
        self.inner.provider()
    }

    async fn generate_controlled(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
        context: &anima_core::execution_limits::ModelCallContext,
    ) -> Result<ModelGenerateResponse, String> {
        self.through_chain(config, |choice, _served| async move {
            let result = match context.check_live() {
                Ok(()) => {
                    self.inner
                        .generate_controlled(&choice, request, context)
                        .await
                }
                Err(error) => Err(error),
            };
            (result, context.check_live().is_err())
        })
        .await
    }

    async fn stream_controlled(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
        sink: &dyn ModelStreamSink,
        context: &anima_core::execution_limits::ModelCallContext,
    ) -> Result<(), String> {
        self.through_chain(config, |choice, served| async move {
            let attempt = AttemptSink {
                sink,
                served: Mutex::new(served),
                committed: AtomicBool::new(false),
            };
            let result = match context.check_live() {
                Ok(()) => {
                    self.inner
                        .stream_controlled(&choice, request, &attempt, context)
                        .await
                }
                Err(error) => Err(error),
            };
            (
                result,
                attempt.committed.load(Ordering::SeqCst) || context.check_live().is_err(),
            )
        })
        .await
    }

    async fn generate(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        self.through_chain(config, |choice, _served| async move {
            (self.inner.generate(&choice, request).await, false)
        })
        .await
    }

    async fn stream(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
        sink: &dyn ModelStreamSink,
    ) -> Result<(), String> {
        self.through_chain(config, |choice, served| async move {
            let attempt = AttemptSink {
                sink,
                served: Mutex::new(served),
                committed: AtomicBool::new(false),
            };
            let result = self.inner.stream(&choice, request, &attempt).await;
            (result, attempt.committed.load(Ordering::SeqCst))
        })
        .await
    }
}
