//! A weak handle shares the actual scheduler's admission/claim/job ownership.
use super::*;
use std::sync::Weak;

#[derive(Clone)]
pub(crate) struct SchedulerTrigger(Weak<SchedulerInner>);

impl SchedulerTrigger {
    pub(super) fn new(inner: &Arc<SchedulerInner>) -> Self {
        Self(Arc::downgrade(inner))
    }

    pub(crate) async fn run_now(
        &self,
        agent_id: &str,
        schedule_id: &str,
    ) -> Result<ScheduledPromptRecord, ScheduleError> {
        self.admit(agent_id, schedule_id, None).await
    }

    pub(crate) async fn from_run(
        &self,
        agent_id: &str,
        schedule_id: &str,
        source: automations::AutomationRunAuthority,
    ) -> Result<ScheduledPromptRecord, ScheduleError> {
        self.admit(agent_id, schedule_id, Some(source)).await
    }

    async fn admit(
        &self,
        agent_id: &str,
        schedule_id: &str,
        source: Option<automations::AutomationRunAuthority>,
    ) -> Result<ScheduledPromptRecord, ScheduleError> {
        let inner = self.0.upgrade().ok_or(ScheduleError::Rejected(
            "Automation scheduler is unavailable".into(),
        ))?;
        let (agent_id, schedule_id) = (agent_id.to_owned(), schedule_id.to_owned());
        // Returns after durable claim/admission, not after the child run. The
        // active caller can therefore release its own agent/global permit.
        tokio::spawn(async move {
            run_now_inner(&inner, &agent_id, &schedule_id, now_ms(), source).await
        })
        .await
        .unwrap_or(Err(ScheduleError::Persistence))
    }

    pub(super) fn source_is_active(
        state: &crate::state::DaemonState,
        agent_id: &str,
        source: &crate::runs::RunLink,
    ) -> bool {
        state.runs.get(&source.run_id).is_some_and(|run| {
            source.agent_id == agent_id
                && run.agent_id == agent_id
                && run.session_id == source.session_id
                && run.status == RunStatus::Running
                && run.stop.is_none()
                && run.parent_run_id.is_none()
                && matches!(
                    run.source,
                    RunSource::Web | RunSource::Api | RunSource::Telegram | RunSource::Discord
                )
        }) && state.get_agent(agent_id).is_some_and(|agent| {
            agent
                .state
                .config
                .tools
                .iter()
                .flatten()
                .any(|tool| tool.name == "trigger_automation")
        })
    }
}
