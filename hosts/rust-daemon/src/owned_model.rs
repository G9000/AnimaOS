//! Bind all host model consumers, including swarm and secondary work, to custody.
use crate::profile_owner::ProfileOwner;
use anima_core::execution_limits::ModelCallContext;
use anima_core::{
    AgentConfig, ModelAdapter, ModelGenerateRequest, ModelGenerateResponse, ModelStreamSink,
};
use async_trait::async_trait;
use std::sync::Arc;

pub(crate) fn bind(
    inner: Arc<dyn ModelAdapter>,
    owner: Option<ProfileOwner>,
) -> Arc<dyn ModelAdapter> {
    match owner {
        Some(owner) => Arc::new(OwnedModel { inner, owner }),
        None => inner,
    }
}

struct OwnedModel {
    inner: Arc<dyn ModelAdapter>,
    owner: ProfileOwner,
}

impl OwnedModel {
    async fn guard<T>(
        &self,
        work: impl std::future::Future<Output = Result<T, String>>,
        context: Option<&ModelCallContext>,
    ) -> Result<T, String> {
        self.owner
            .verify_owned()
            .await
            .map_err(|error| error.to_string())?;
        let lost = self.owner.lost_signal();
        let result = tokio::select! {
            biased;
            _ = lost.cancelled() => {
                if let Some(context) = context { context.request_cancel.cancel_with(anima_core::CancelCause::ProfileOwnershipLost); }
                return Err(lost.run_error().into());
            }
            result = work => result,
        };
        if let Err(error) = self.owner.verify_owned().await {
            if let Some(context) = context {
                context
                    .request_cancel
                    .cancel_with(anima_core::CancelCause::ProfileOwnershipLost);
            }
            return Err(error.to_string());
        }
        result
    }
}

#[async_trait]
impl ModelAdapter for OwnedModel {
    fn provider(&self) -> &str {
        self.inner.provider()
    }
    fn single_attempt_contract(&self) -> bool {
        self.inner.single_attempt_contract()
    }
    async fn generate(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        self.guard(async { self.inner.generate(config, request).await }, None)
            .await
    }
    async fn stream(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
        sink: &dyn ModelStreamSink,
    ) -> Result<(), String> {
        self.guard(
            async { self.inner.stream(config, request, sink).await },
            None,
        )
        .await
    }
    async fn generate_controlled(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
        context: &ModelCallContext,
    ) -> Result<ModelGenerateResponse, String> {
        self.guard(
            async {
                self.inner
                    .generate_controlled(config, request, context)
                    .await
            },
            Some(context),
        )
        .await
    }
    async fn stream_controlled(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
        sink: &dyn ModelStreamSink,
        context: &ModelCallContext,
    ) -> Result<(), String> {
        self.guard(
            async {
                self.inner
                    .stream_controlled(config, request, sink, context)
                    .await
            },
            Some(context),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct CountModel(Arc<AtomicUsize>);
    #[async_trait]
    impl ModelAdapter for CountModel {
        fn single_attempt_contract(&self) -> bool {
            true
        }
        fn provider(&self) -> &str {
            "test"
        }
        async fn generate(
            &self,
            _: &AgentConfig,
            _: &ModelGenerateRequest,
        ) -> Result<ModelGenerateResponse, String> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Err("model invoked".into())
        }
    }

    #[tokio::test]
    async fn profile_owner_secondary_or_swarm_adapter_cannot_dispatch_after_revocation() {
        let temp = tempfile::tempdir().unwrap();
        let owner = ProfileOwner::acquire([temp.path().join("profile.json")]).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let adapter = bind(Arc::new(CountModel(calls.clone())), Some(owner.clone()));
        owner.mark_lost();
        let config =
            serde_json::from_value(serde_json::json!({"name":"test","model":"test"})).unwrap();
        let request = ModelGenerateRequest {
            system: "test".into(),
            messages: vec![],
            temperature: None,
            max_tokens: None,
        };
        assert!(adapter.generate(&config, &request).await.is_err());
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "stale model consumer obtained new provider work"
        );
    }
}
