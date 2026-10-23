//! A legacy secondary-call API bound to one explicit execution operation.
//! Physical attempts remain counted inside the native transport, including keys,
//! fallback models and retries. The logical ID survives candidate timeouts.
use anima_core::execution_limits::{ExecutionContext, ModelCallContext};
use anima_core::{
    AgentConfig, ModelAdapter, ModelGenerateRequest, ModelGenerateResponse, ModelStreamSink,
};
use async_trait::async_trait;
use std::sync::Arc;

pub(crate) fn bind(
    inner: Arc<dyn ModelAdapter>,
    execution: ExecutionContext,
    logical_id: String,
) -> Arc<dyn ModelAdapter> {
    Arc::new(ControlledModel {
        inner,
        execution,
        logical_id,
    })
}

/// Keep metering inside the authority boundary: a known Final is retained even
/// when the outer call refuses success after accounting/cancellation failure.
pub(crate) fn metered(
    inner: Arc<dyn ModelAdapter>,
    execution: ExecutionContext,
    logical_id: String,
) -> (Arc<crate::usage::metered::Metered>, Arc<dyn ModelAdapter>) {
    let meter = Arc::new(crate::usage::metered::Metered::new(inner));
    let model = bind(meter.clone(), execution, logical_id);
    (meter, model)
}

struct ControlledModel {
    inner: Arc<dyn ModelAdapter>,
    execution: ExecutionContext,
    logical_id: String,
}
#[async_trait]
impl ModelAdapter for ControlledModel {
    fn provider(&self) -> &str {
        self.inner.provider()
    }
    async fn generate(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        let context = ModelCallContext::new(self.execution.clone(), self.logical_id.clone());
        context.check_live()?;
        let result = self
            .inner
            .generate_controlled(config, request, &context)
            .await;
        context.check_live()?;
        result
    }
    async fn stream(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
        sink: &dyn ModelStreamSink,
    ) -> Result<(), String> {
        let context = ModelCallContext::new(self.execution.clone(), self.logical_id.clone());
        context.check_live()?;
        let result = self
            .inner
            .stream_controlled(config, request, sink, &context)
            .await;
        context.check_live()?;
        result
    }
}
