use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::Notify;
use tracing::warn;

use crate::agent_runs::AgentRunCoordinator;
use crate::app::SharedDaemonState;
use crate::jobs::JobService;
use crate::runs::{RunSource, RunStatus};
use crate::schedules::SchedulerService;

/// The work a drain waits for, by kind.
#[derive(Serialize, Deserialize, Clone, Copy, Default, PartialEq, Eq, Debug, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InFlight {
    /// Agent runs going (running or awaiting approval), with the automation
    /// occurrences claimed and not yet finished.
    pub chat_runs: u32,
    /// Accepted messages waiting in their session's queue; they still start
    /// while draining.
    pub queued_runs: u32,
    pub swarm_runs: u32,
    /// Asynchronous helper runs not yet finished, queued ones included.
    pub helper_runs: u32,
    /// Job attempts running or waiting for their delegations.
    pub jobs: u32,
    pub background_processes: u32,
    pub app_sessions: u32,
    pub acp_sessions: u32,
    #[serde(default)]
    pub memory_reviews: u32,
    /// Includes logically stopped evaluations whose model transport is retiring.
    #[serde(default)]
    pub skill_evaluations: u32,
    /// Complete-task comparisons, including logically stopped retiring transports.
    #[serde(default)]
    pub task_regressions: u32,
    #[serde(default)]
    pub doctor_repairs: u32,
    #[serde(default)]
    pub standing_intent_changes: u32,
    #[serde(default)]
    pub execution_jobs: u32,
    #[serde(default)]
    pub mcp_operations: u32,
    /// Pending, active or physically quarantined owner sign-in workflows.
    #[serde(default)]
    pub browser_profile_logins: u32,
}

impl InFlight {
    pub fn total(&self) -> u32 {
        self.chat_runs
            + self.queued_runs
            + self.swarm_runs
            + self.helper_runs
            + self.jobs
            + self.background_processes
            + self.app_sessions
            + self.acp_sessions
            + self.memory_reviews
            + self.skill_evaluations
            + self.task_regressions
            + self.doctor_repairs
            + self.standing_intent_changes
            + self.execution_jobs
            + self.mcp_operations
            + self.browser_profile_logins
    }
}

/// Drains the daemon before a supervisor stops it: new messages, swarm runs,
/// jobs and goals are refused, connector messages and due automations wait,
/// and the work already accepted finishes. Shared by the router's clones.
#[derive(Clone)]
pub(crate) struct DrainController {
    state: SharedDaemonState,
    agent_runs: AgentRunCoordinator,
    jobs: JobService,
    scheduler: SchedulerService,
    shutdown: Arc<Notify>,
}

impl DrainController {
    /// A controller whose `request_shutdown` notifies a handle of its own;
    /// `with_shutdown` gives it the host's.
    pub(crate) fn new(
        state: SharedDaemonState,
        agent_runs: AgentRunCoordinator,
        jobs: JobService,
        scheduler: SchedulerService,
    ) -> Self {
        Self {
            state,
            agent_runs,
            jobs,
            scheduler,
            shutdown: Arc::new(Notify::new()),
        }
    }

    pub(crate) fn with_shutdown(mut self, shutdown: Arc<Notify>) -> Self {
        self.shutdown = shutdown;
        self
    }

    /// What graceful shutdown waits on, besides the OS signals.
    pub(crate) fn shutdown_handle(&self) -> Arc<Notify> {
        Arc::clone(&self.shutdown)
    }

    /// Begins draining (again is harmless) and answers what is in flight.
    pub(crate) async fn begin(&self) -> InFlight {
        self.agent_runs.begin_drain();
        self.in_flight().await
    }

    /// Ends the drain: admission is as before.
    pub(crate) fn cancel(&self) {
        self.agent_runs.cancel_drain();
    }

    pub(crate) fn is_draining(&self) -> bool {
        self.agent_runs.is_draining()
    }

    pub(crate) async fn in_flight(&self) -> InFlight {
        // Each count takes its own lock in turn; none is held across another.
        let occurrences = self.scheduler.in_flight_count().await;
        let jobs = self.jobs.in_flight_count().await;
        let guard = self.state.read().await;
        let mut in_flight = InFlight {
            chat_runs: count(occurrences),
            jobs: count(jobs),
            memory_reviews: u32::from(guard.learning_slots.available_permits() == 0),
            skill_evaluations: u32::from(guard.skill_evaluation_slots.available_permits() == 0),
            task_regressions: u32::from(guard.task_regression_slots.available_permits() == 0),
            doctor_repairs: u32::from(guard.doctor_slots.available_permits() == 0),
            standing_intent_changes: u32::from(
                guard.standing_intent_slots.available_permits() == 0,
            ),
            execution_jobs: count(guard.execution.in_flight_count()),
            mcp_operations: count(
                guard
                    .mcp_manager
                    .as_ref()
                    .map_or(0, |m| m.pending_work_count()),
            ),
            browser_profile_logins: count(
                guard
                    .browser_service
                    .as_ref()
                    .map_or(0, |browser| browser.profile_in_flight_count()),
            ),
            swarm_runs: count(guard.running_swarm_count()),
            background_processes: match guard.background_process_count() {
                Ok(running) => count(running),
                // Unknown is not none: the owner can still force the stop.
                Err(error) => {
                    warn!(%error, "background processes could not be counted for the drain");
                    1
                }
            },
            ..InFlight::default()
        };
        for record in guard.runs.active_records() {
            if record.async_helper.is_some() {
                in_flight.helper_runs += 1;
                continue;
            }
            // A job's or an automation's run is counted above, from its claim on.
            if matches!(record.source, RunSource::Job | RunSource::Schedule) {
                continue;
            }
            if record.status == RunStatus::Queued {
                in_flight.queued_runs += 1;
            } else {
                in_flight.chat_runs += 1;
            }
        }
        // Live sessions are process-wide; count this daemon's agents' and
        // swarm members' (`<swarm id>:agent:<name>`) only.
        let (apps, acp) = crate::tools::process::live::live_sessions().open_counts(|agent_id| {
            guard.agents.contains_key(agent_id)
                || agent_id
                    .split_once(":agent:")
                    .is_some_and(|(swarm_id, _)| guard.swarms.contains_key(swarm_id))
        });
        in_flight.app_sessions = count(apps);
        in_flight.acp_sessions = count(acp);
        in_flight
    }

    /// Starts graceful shutdown, the same path as Ctrl+C or SIGTERM. The
    /// permit `notify_one` stores is kept until the server waits for it.
    pub(crate) fn request_shutdown(&self) {
        self.shutdown.notify_one();
    }
}

fn count(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}
