//! Automations from the coordinator (spec §9.3): the service the
//! companion's tools change them through, under this coordinator's
//! control-plane transaction.

use std::sync::Arc;

use super::AgentRunCoordinator;
use crate::schedules::{AutomationService, ScheduleError, ScheduledPromptRecord, SchedulerTrigger};

impl AgentRunCoordinator {
    pub(crate) fn bind_scheduler_trigger(&self, trigger: SchedulerTrigger) {
        *self
            .scheduler_trigger
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(trigger);
    }

    pub(crate) async fn trigger_automation_from_run(
        &self,
        agent_id: &str,
        authority: crate::schedules::automations::AutomationRunAuthority,
        schedule_id: &str,
    ) -> Result<ScheduledPromptRecord, ScheduleError> {
        let trigger = self
            .scheduler_trigger
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
            .ok_or(ScheduleError::Rejected(
                "Automation scheduler is unavailable".into(),
            ))?;
        trigger.from_run(agent_id, schedule_id, authority).await
    }

    pub(crate) fn automations(&self) -> AutomationService {
        AutomationService::new(Arc::clone(&self.state), self.control_plane_transactions())
    }
}
