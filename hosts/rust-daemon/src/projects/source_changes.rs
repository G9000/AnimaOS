//! Canonical source deletion and its project access fence share one durable save.

use std::collections::BTreeSet;

use super::{ProjectError, ProjectInvalidation, ProjectRegistry};
use crate::state::{run_stop::RunStopPlan, DaemonState};

pub(crate) struct ProjectSourceChange {
    pub(crate) registry: ProjectRegistry,
    changes: Vec<ProjectInvalidation>,
    stops: Vec<RunStopPlan>,
}

impl ProjectSourceChange {
    pub(crate) fn prepare(
        state: &mut DaemonState,
        agent: &str,
        session: &str,
        now: u64,
    ) -> Result<Self, ProjectError> {
        let mut registry = state.projects.clone();
        let changes: Vec<_> = registry
            .invalidate_sources_for_session(agent, session, now)?
            .into_iter()
            .map(|project| ProjectInvalidation {
                project_id: project.id,
                revision: project.revision,
                access_epoch: project.access_epoch,
                deleted: false,
                membership_changed: false,
                destinations: project
                    .members
                    .iter()
                    .map(|row| row.agent_id.clone())
                    .collect(),
            })
            .collect();
        Ok(Self::from_candidate(state, registry, changes, now))
    }

    pub(super) fn from_candidate(
        state: &mut DaemonState,
        registry: ProjectRegistry,
        changes: Vec<ProjectInvalidation>,
        now: u64,
    ) -> Self {
        let affected: BTreeSet<_> = changes.iter().map(|row| row.project_id.as_str()).collect();
        let roots: Vec<_> = state
            .runs
            .active_records()
            .into_iter()
            .filter(|run| {
                run.project_witness
                    .as_ref()
                    .and_then(|witness| witness.project.as_ref())
                    .is_some_and(|binding| affected.contains(binding.project_id.as_str()))
            })
            .map(|run| (run.agent_id.clone(), run.id.clone()))
            .collect();
        let stops = roots
            .into_iter()
            .filter_map(|(agent, run)| state.request_run_stop(&agent, &run, now))
            .collect();
        Self {
            registry,
            changes,
            stops,
        }
    }

    pub(crate) fn changed(&self) -> bool {
        !self.changes.is_empty()
    }

    pub(crate) fn rollback(self, state: &mut DaemonState) {
        for plan in self.stops.into_iter().rev() {
            state.revert_run_stop(plan.undo);
        }
    }

    pub(crate) fn fence_uncertain(&self, state: &DaemonState) {
        for notice in &self.changes {
            state.project_access.fence_uncertain(&notice.project_id);
        }
        self.signal(state);
    }

    pub(crate) fn publish(self, state: &mut DaemonState) -> Result<(), ProjectError> {
        state.projects = self.registry.clone();
        if let Err(error) = state.project_access.publish_records(&state.projects) {
            self.fence_uncertain(state);
            return Err(error);
        }
        self.signal(state);
        for plan in &self.stops {
            for run in &plan.cancelled {
                state.live.publish(crate::live::run_status_event(run), None);
            }
        }
        for notice in &self.changes {
            super::publish_project_invalidation(state, notice);
        }
        Ok(())
    }

    fn signal(&self, state: &DaemonState) {
        for plan in &self.stops {
            for run in &plan.signal {
                if let Some(control) = state.live.runs().control(run) {
                    control.cancel.cancel();
                }
            }
        }
    }
}
