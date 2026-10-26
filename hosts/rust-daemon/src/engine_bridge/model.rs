use anima_core::*;
use async_trait::async_trait;
use std::sync::Arc;
pub(super) struct ConfiguredModel {
    pub inner: Arc<dyn ModelAdapter>,
    pub config: AgentConfig,
}
impl ConfiguredModel {
    fn accepted_config(&self, engine: &AgentConfig) -> AgentConfig {
        let mut config = self.config.clone();
        config.tools = engine.tools.clone();
        config
    }
}
#[async_trait]
impl ModelAdapter for ConfiguredModel {
    fn provider(&self) -> &str {
        self.inner.provider()
    }
    fn single_attempt_contract(&self) -> bool {
        self.inner.single_attempt_contract()
    }
    fn generated_usage_is_complete(&self, response: &ModelGenerateResponse) -> bool {
        self.inner.generated_usage_is_complete(response)
    }
    async fn generate(
        &self,
        engine: &AgentConfig,
        request: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        self.inner
            .generate(&self.accepted_config(engine), request)
            .await
    }
    async fn generate_controlled(
        &self,
        engine: &AgentConfig,
        request: &ModelGenerateRequest,
        context: &anima_core::execution_limits::ModelCallContext,
    ) -> Result<ModelGenerateResponse, String> {
        self.inner
            .generate_controlled(&self.accepted_config(engine), request, context)
            .await
    }
    async fn stream_controlled(
        &self,
        engine: &AgentConfig,
        request: &ModelGenerateRequest,
        sink: &dyn ModelStreamSink,
        context: &anima_core::execution_limits::ModelCallContext,
    ) -> Result<(), String> {
        self.inner
            .stream_controlled(&self.accepted_config(engine), request, sink, context)
            .await
    }
}
pub(super) struct Clock(pub Arc<dyn ExecutionClock>);
#[async_trait]
impl ExecutionClock for Clock {
    fn now_ms(&self) -> u64 {
        self.0.now_ms()
    }
    async fn wait_until_ms(&self, deadline: u64) {
        self.0.wait_until_ms(deadline).await
    }
}
