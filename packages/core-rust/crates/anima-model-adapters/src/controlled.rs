//! Physical-attempt custody shared by native transports. No retry is free.
use anima_core::execution_limits::{ExecutionPermit, ExecutionStreamSink, ModelCallContext};
use anima_core::{
    AgentConfig, ModelGenerateRequest, ModelStreamFrame, ModelStreamSink, TokenUsage,
};
use async_trait::async_trait;

pub(crate) struct PreparedAttempt {
    pub request: ModelGenerateRequest,
    permit: Option<ExecutionPermit>,
    context: Option<ModelCallContext>,
}

impl PreparedAttempt {
    pub async fn new(
        context: Option<&ModelCallContext>,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
    ) -> Result<Self, String> {
        let (request, permit) = match context {
            Some(context) => {
                let (request, permit) = context.reserve_prepared(config, request).await?;
                permit.revalidate().await?;
                (request, Some(permit))
            }
            None => (request.clone(), None),
        };
        Ok(Self {
            request,
            permit,
            context: context.cloned(),
        })
    }
    pub async fn bounded<T>(
        &self,
        work: impl std::future::Future<Output = Result<T, String>>,
    ) -> Result<T, String> {
        match &self.context {
            Some(context) => context.bounded(work).await,
            None => work.await,
        }
    }
    pub fn sink<'a>(&self, downstream: &'a dyn ModelStreamSink) -> AttemptSink<'a> {
        match &self.permit {
            Some(permit) => AttemptSink::Observed(permit.stream_sink(downstream)),
            None => AttemptSink::Direct(downstream),
        }
    }
    pub async fn finish<T>(&mut self, result: &Result<T, String>) {
        if let Some(permit) = &mut self.permit {
            let retired = result.is_ok()
                || self
                    .context
                    .as_ref()
                    .is_none_or(|context| context.check_live().is_ok());
            let _ = permit
                .finish_with_retirement(permit.observed_outcome(), retired)
                .await;
        }
    }
}

pub(crate) enum AttemptSink<'a> {
    Direct(&'a dyn ModelStreamSink),
    Observed(ExecutionStreamSink<'a>),
}
#[async_trait]
impl ModelStreamSink for AttemptSink<'_> {
    async fn emit(&self, frame: ModelStreamFrame) -> Result<(), String> {
        match self {
            Self::Direct(sink) => sink.emit(frame).await,
            Self::Observed(sink) => sink.emit(frame).await,
        }
    }
    async fn complete_usage(&self, usage: TokenUsage) -> Result<(), String> {
        match self {
            Self::Direct(sink) => sink.complete_usage(usage).await,
            Self::Observed(sink) => sink.complete_usage(usage).await,
        }
    }
}

/// Presence, rather than a nonzero normalized count, attests completeness.
pub(crate) fn json_usage_complete(payload: &serde_json::Value, kind: &str) -> bool {
    let (usage, fields): (&serde_json::Value, &[&str]) = match kind {
        "anthropic" => (&payload["usage"], &["input_tokens", "output_tokens"]),
        "google" => (
            &payload["usageMetadata"],
            &[
                "promptTokenCount",
                "candidatesTokenCount",
                "totalTokenCount",
            ],
        ),
        "ollama" => (payload, &["prompt_eval_count", "eval_count"]),
        "chatgpt" => (
            &payload["usage"],
            &["input_tokens", "output_tokens", "total_tokens"],
        ),
        _ => (
            &payload["usage"],
            &["prompt_tokens", "completion_tokens", "total_tokens"],
        ),
    };
    fields.iter().all(|field| usage[*field].as_u64().is_some())
}
