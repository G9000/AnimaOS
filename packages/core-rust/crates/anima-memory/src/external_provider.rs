//! The plug-in point for an external memory service.
//!
//! The local memory store is always on. At most one external provider runs
//! beside it: it adds recall results to a run's context, receives finished
//! owner turns, and mirrors local memory writes. Provider `none` (the default,
//! [`NoExternalMemory`]) does nothing, so behaviour is identical to having no
//! provider. A host registers an id for each provider it can build.

use std::time::Duration;

use async_trait::async_trait;

/// A provider failure. The message may reach logs: never put memory content,
/// turn text or credentials in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalMemoryError(pub String);

impl ExternalMemoryError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl std::fmt::Display for ExternalMemoryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ExternalMemoryError {}

/// What a host asks of [`ExternalMemoryProvider::recall`] when it builds a
/// run's context.
#[derive(Clone, Debug, PartialEq)]
pub struct RecallRequest {
    pub agent_id: String,
    /// The owner's message text.
    pub query: String,
    /// The most items the host will use.
    pub limit: usize,
    /// How long the host will wait. It abandons the call after this, so a
    /// provider should answer sooner or stop work.
    pub deadline: Duration,
}

/// One item a provider recalled. Untrusted text.
#[derive(Clone, Debug, PartialEq)]
pub struct RecalledItem {
    pub text: String,
    pub score: f64,
    pub source_id: Option<String>,
}

/// A finished owner turn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TurnRecord {
    pub agent_id: String,
    pub session_id: String,
    pub user_text: String,
    pub reply_text: String,
    pub at_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryWriteAction {
    Add,
    Update,
    Remove,
}

/// A local memory change that has been saved. For `Remove`, `content` is the
/// content the memory had.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryWriteEvent {
    pub agent_id: String,
    pub action: MemoryWriteAction,
    /// The local type name, e.g. `fact`.
    pub memory_type: String,
    pub memory_id: String,
    pub content: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProviderStatus {
    Ready,
    /// The service cannot be used right now. The reason is shown to the owner:
    /// no paths, URLs or credentials.
    Unavailable(String),
}

/// An external memory service that runs beside the local store.
///
/// # Contract
///
/// - **Instances:** one per daemon, built at boot. No method may block; each
///   call gets a deadline from the host and is abandoned after it.
/// - **Untrusted input:** everything a provider returns is untrusted data. The
///   host scans it, bounds it and labels it, and never runs it as
///   instructions.
/// - **Local store only:** a provider never sees or changes the local store
///   directly. It only receives events ([`Self::sync_turn`],
///   [`Self::on_memory_write`]).
///
/// - **Ordering and loss:** the host sends `sync_turn` and `on_memory_write`
///   calls one at a time, in the order the changes were saved, so an `Add`
///   always arrives before a later `Remove` of the same memory. Each call has
///   its own timeout. The queue behind them is bounded: when it is full the
///   host drops new events (logging the provider id and a count) instead of
///   waiting, so a provider that falls behind loses events and must not
///   assume it sees every change. `status` and `recall` are separate calls
///   with their own, shorter deadlines.
///
/// Errors are logged by id and message, never retried, and never fail a run.
#[async_trait]
pub trait ExternalMemoryProvider: Send + Sync {
    /// A stable name, e.g. `none` or `mem0`.
    fn id(&self) -> &str;

    /// Items relevant to the request, when a run's context is built.
    async fn recall(
        &self,
        request: RecallRequest,
    ) -> Result<Vec<RecalledItem>, ExternalMemoryError>;

    /// After a completed owner turn, so the service can learn from it.
    async fn sync_turn(&self, _turn: TurnRecord) -> Result<(), ExternalMemoryError> {
        Ok(())
    }

    /// After a local memory change is saved, to mirror adds, updates and
    /// removes.
    async fn on_memory_write(&self, _event: MemoryWriteEvent) -> Result<(), ExternalMemoryError> {
        Ok(())
    }

    /// For the readiness route.
    async fn status(&self) -> ProviderStatus;
}

/// The `none` provider: no recall, no events, always ready.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoExternalMemory;

pub const NO_EXTERNAL_MEMORY_ID: &str = "none";

#[async_trait]
impl ExternalMemoryProvider for NoExternalMemory {
    fn id(&self) -> &str {
        NO_EXTERNAL_MEMORY_ID
    }

    async fn recall(
        &self,
        _request: RecallRequest,
    ) -> Result<Vec<RecalledItem>, ExternalMemoryError> {
        Ok(Vec::new())
    }

    async fn status(&self) -> ProviderStatus {
        ProviderStatus::Ready
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_none_provider_returns_nothing_and_reports_ready() {
        let provider = NoExternalMemory;
        assert_eq!(provider.id(), "none");
        let items = provider
            .recall(RecallRequest {
                agent_id: "a".into(),
                query: "q".into(),
                limit: 6,
                deadline: Duration::from_secs(2),
            })
            .await
            .unwrap();
        assert!(items.is_empty());
        assert!(provider
            .sync_turn(TurnRecord {
                agent_id: "a".into(),
                session_id: "s".into(),
                user_text: "u".into(),
                reply_text: "r".into(),
                at_ms: 1,
            })
            .await
            .is_ok());
        assert!(provider
            .on_memory_write(MemoryWriteEvent {
                agent_id: "a".into(),
                action: MemoryWriteAction::Add,
                memory_type: "fact".into(),
                memory_id: "m".into(),
                content: "c".into(),
            })
            .await
            .is_ok());
        assert_eq!(provider.status().await, ProviderStatus::Ready);
    }
}
