//! Operator-owned execution profiles. Model requests carry data, never transport configuration.
mod config;
mod docker;
mod process;
mod service;
mod ssh;
pub(crate) mod worker;
pub(crate) use config::{Backend, ExecutionProfile};
#[cfg(test)]
pub(crate) use config::{DockerProfile, SshProfile};
use serde::{Deserialize, Serialize};
pub(crate) use service::ExecutionService;
#[derive(Clone)]
pub(crate) struct ExecutionScope {
    pub(crate) agent_id: String,
    pub(crate) authority: std::sync::Arc<
        dyn Fn() -> futures::future::BoxFuture<'static, Result<(), String>> + Send + Sync,
    >,
}
impl ExecutionScope {
    pub(super) async fn revalidate(&self) -> Result<(), String> {
        (self.authority)().await
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ExecutionRequest {
    pub(crate) target_id: String,
    pub(crate) command: String,
    #[serde(default = "default_cwd")]
    pub(crate) cwd: String,
    #[serde(default = "default_timeout")]
    pub(crate) timeout_ms: u64,
}
fn default_cwd() -> String {
    ".".into()
}
fn default_timeout() -> u64 {
    30_000
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ExecutionStatus {
    Succeeded,
    Failed,
    Cancelled,
    TimedOut,
    OutcomeUnknown,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ExecutionReceipt {
    pub(crate) execution_id: String,
    pub(crate) target_id: String,
    pub(crate) backend: String,
    pub(crate) status: ExecutionStatus,
    pub(crate) exit_code: Option<i32>,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) cleanup_confirmed: bool,
    pub(crate) message: String,
}
impl ExecutionReceipt {
    fn unknown(id: &str, target: &str, backend: &str, message: impl Into<String>) -> Self {
        Self {
            execution_id: id.into(),
            target_id: target.into(),
            backend: backend.into(),
            status: ExecutionStatus::OutcomeUnknown,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            cleanup_confirmed: false,
            message: message.into(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TargetSummary {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) backend: String,
    pub(crate) max_timeout_ms: u64,
}

#[cfg(test)]
mod tests;
