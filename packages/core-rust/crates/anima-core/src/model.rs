use std::collections::BTreeMap;
use std::time::Duration;

use async_trait::async_trait;

use crate::agent::{AgentConfig, TokenUsage};
use crate::primitives::{Content, DataValue, Message};

/// Wait without requiring a host executor or blocking the calling thread.
/// Dropping the enclosing future cancels this wait; adapters choose the delay
/// and any retry policy.
pub async fn model_transport_delay(duration: Duration) {
    futures_timer::Delay::new(duration).await;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelStopReason {
    End,
    ToolCall,
    MaxTokens,
}

impl ModelStopReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::End => "end",
            Self::ToolCall => "tool_call",
            Self::MaxTokens => "max_tokens",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModelGenerateRequest {
    pub system: String,
    pub messages: Vec<Message>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModelGenerateResponse {
    pub content: Content,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub usage: TokenUsage,
    pub stop_reason: ModelStopReason,
}

/// Normalized frames delivered by an adapter while generation is in flight.
#[derive(Clone, Debug, PartialEq)]
pub enum ModelStreamFrame {
    TextDelta(String),
    /// Provider-reported usage so far; each frame replaces the last. `Final` still carries the definitive usage.
    Usage(TokenUsage),
    /// A fallback model answered instead of the agent's primary. Sent before
    /// `Final`; `reason` says what happened to the primary (e.g. "was rate limited").
    Served {
        provider: String,
        model: String,
        reason: String,
    },
    Final(ModelGenerateResponse),
}

#[async_trait]
pub trait ModelStreamSink: Send + Sync {
    async fn emit(&self, frame: ModelStreamFrame) -> Result<(), String>;
    /// An adapter may attest complete provider counters separately from a Final
    /// whose normalized usage may contain defaults for missing statistics.
    async fn complete_usage(&self, _usage: TokenUsage) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub args: BTreeMap<String, DataValue>,
}

#[async_trait]
pub trait ModelAdapter: Send + Sync {
    fn provider(&self) -> &str;

    /// Opt-in guarantee: one physical attempt, no hidden retry or detached work.
    /// Native retrying adapters override controlled calls at their transport seam.
    fn single_attempt_contract(&self) -> bool {
        false
    }
    /// Explicit attestation for successful generated responses, including zero.
    /// Adapters that cannot distinguish missing counters leave this false.
    fn generated_usage_is_complete(&self, _response: &ModelGenerateResponse) -> bool {
        false
    }

    async fn generate_controlled(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
        context: &crate::execution_limits::ModelCallContext,
    ) -> Result<ModelGenerateResponse, String> {
        if !self.single_attempt_contract() {
            return Err("Adapter must explicitly support controlled physical attempts".into());
        }
        let (request, mut permit) = context.reserve_prepared(config, request).await?;
        permit.revalidate().await?;
        let result = context.bounded(self.generate(config, &request)).await;
        let outcome = match &result {
            Ok(response) if self.generated_usage_is_complete(response) => {
                crate::execution_limits::SettlementOutcome::Known(response.usage.clone())
            }
            Ok(response) => crate::execution_limits::SettlementOutcome::Unknown {
                observed: Some(response.usage.clone()),
            },
            Err(_) => permit.observed_outcome(),
        };
        let physical_retired = result.is_ok() || context.check_live().is_ok();
        let _ = permit
            .finish_with_retirement(outcome, physical_retired)
            .await;
        result
    }

    async fn stream_controlled(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
        sink: &dyn ModelStreamSink,
        context: &crate::execution_limits::ModelCallContext,
    ) -> Result<(), String> {
        if !self.single_attempt_contract() {
            return Err("Adapter must explicitly support controlled physical attempts".into());
        }
        let (request, mut permit) = context.reserve_prepared(config, request).await?;
        permit.revalidate().await?;
        let observed = permit.stream_sink(sink);
        let result = context
            .bounded(self.stream(config, &request, &observed))
            .await;
        drop(observed);
        let physical_retired = result.is_ok() || context.check_live().is_ok();
        let _ = permit
            .finish_with_retirement(permit.observed_outcome(), physical_retired)
            .await;
        result
    }

    async fn generate(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String>;

    /// Default streaming preserves existing adapters by emitting their final normalized result.
    async fn stream(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
        sink: &dyn ModelStreamSink,
    ) -> Result<(), String> {
        let response = self.generate(config, request).await?;
        if self.generated_usage_is_complete(&response) {
            sink.complete_usage(response.usage.clone()).await?;
        }
        let _ = sink.emit(ModelStreamFrame::Final(response)).await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ModelAdapter, ModelGenerateRequest, ModelGenerateResponse, ModelStopReason,
        ModelStreamFrame, ModelStreamSink,
    };
    use crate::{AgentConfig, Content, TokenUsage};
    use async_trait::async_trait;

    struct RecordingSink(std::sync::Mutex<Vec<ModelStreamFrame>>);

    #[async_trait]
    impl ModelStreamSink for RecordingSink {
        async fn emit(&self, frame: ModelStreamFrame) -> Result<(), String> {
            self.0.lock().unwrap().push(frame);
            Ok(())
        }
    }

    struct FinalOnlyAdapter;

    fn config() -> AgentConfig {
        AgentConfig {
            name: "test".into(),
            model: "test".into(),
            bio: None,
            lore: None,
            knowledge: None,
            topics: None,
            adjectives: None,
            style: None,
            provider: None,
            system: None,
            tools: None,
            plugins: None,
            settings: None,
            fallbacks: Vec::new(),
        }
    }

    #[async_trait]
    impl ModelAdapter for FinalOnlyAdapter {
        fn provider(&self) -> &str {
            "test"
        }

        async fn generate(
            &self,
            _config: &AgentConfig,
            _request: &ModelGenerateRequest,
        ) -> Result<ModelGenerateResponse, String> {
            Ok(ModelGenerateResponse {
                content: Content {
                    text: "done".into(),
                    attachments: None,
                    metadata: None,
                },
                tool_calls: None,
                usage: TokenUsage::default(),
                stop_reason: ModelStopReason::End,
            })
        }
    }

    #[test]
    fn model_stop_reason_matches_ts_contract() {
        assert_eq!(ModelStopReason::End.as_str(), "end");
        assert_eq!(ModelStopReason::ToolCall.as_str(), "tool_call");
        assert_eq!(ModelStopReason::MaxTokens.as_str(), "max_tokens");
    }

    #[tokio::test]
    async fn default_stream_emits_one_final_frame_from_generate() {
        let sink = RecordingSink(std::sync::Mutex::new(Vec::new()));
        FinalOnlyAdapter
            .stream(
                &config(),
                &ModelGenerateRequest {
                    system: String::new(),
                    messages: vec![],
                    temperature: None,
                    max_tokens: None,
                },
                &sink,
            )
            .await
            .unwrap();

        assert_eq!(
            sink.0.lock().unwrap().as_slice(),
            &[ModelStreamFrame::Final(ModelGenerateResponse {
                content: Content {
                    text: "done".into(),
                    attachments: None,
                    metadata: None
                },
                tool_calls: None,
                usage: TokenUsage::default(),
                stop_reason: ModelStopReason::End,
            })]
        );
    }
}
