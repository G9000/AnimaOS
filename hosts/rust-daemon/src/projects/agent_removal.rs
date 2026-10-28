//! Project portion of the existing native agent-deletion transaction.

use super::{ProjectError, ProjectInvalidation, ProjectSourceChange};
use crate::state::{ControlPlanePersistRequest, DaemonState};

/// Contains an unpublished registry and existing native stop undo plans.
/// No binding overlay targets the deleting agent's soon-to-be-removed sessions.
pub(crate) struct ProjectAgentRemovalCandidate {
    change: ProjectSourceChange,
}

impl ProjectAgentRemovalCandidate {
    /// Call under the shared control-plane transaction before removing native
    /// agent/session/run records, after unrelated fallible preflight checks.
    pub(crate) fn prepare(
        state: &mut DaemonState,
        agent_id: &str,
        now_ms: u64,
    ) -> Result<Self, ProjectError> {
        let mut registry = state.projects.clone();
        let affected: Vec<_> = registry
            .records()
            .filter(|project| project.member(agent_id).is_some())
            .cloned()
            .collect();
        let mut changes = Vec::with_capacity(affected.len());
        // Finish every fallible registry edit before planning any native stop.
        for project in affected {
            let members = project
                .members
                .iter()
                .filter(|member| member.agent_id != agent_id)
                .map(|member| member.agent_id.clone())
                .collect();
            let next = registry.replace_members(&project.id, project.revision, members, now_ms)?;
            changes.push(ProjectInvalidation {
                project_id: next.id,
                revision: next.revision,
                access_epoch: next.access_epoch,
                deleted: false,
                membership_changed: true,
                destinations: project
                    .members
                    .iter()
                    .map(|member| member.agent_id.clone())
                    .collect(),
            });
        }
        Ok(Self {
            change: ProjectSourceChange::from_candidate(state, registry, changes, now_ms),
        })
    }

    /// Capture after native removals. Its empty binding overlay cannot recreate
    /// a removed session, and the project creation receipts remain unchanged.
    pub(crate) fn persistence_request(
        &self,
        state: &mut DaemonState,
    ) -> Result<ControlPlanePersistRequest, ProjectError> {
        state
            .control_plane_persist_request()
            .with_project_candidate(
                self.change.registry.snapshot(),
                self.change.registry.creation_receipts_snapshot(),
                Vec::new(),
            )
            .ok_or(ProjectError::ScopeChanged)
    }

    /// Use only after save_project_candidate acknowledged. Publish before the
    /// lifecycle closes the deleted agent's stream so old/new members see notice.
    pub(crate) fn publish(self, state: &mut DaemonState) -> Result<(), ProjectError> {
        self.change.publish(state)
    }

    /// Known failure only, after native run/session/agent rollback is restored.
    pub(crate) fn rollback(self, state: &mut DaemonState) {
        self.change.rollback(state);
    }

    /// Unknown durability never restores old sharing authority. Keep stop plans
    /// and close leases even if the lifecycle restores native records locally.
    pub(crate) fn fence_uncertain(&self, state: &DaemonState) {
        self.change.fence_uncertain(state);
    }
}

#[cfg(test)]
#[path = "agent_removal_tests.rs"]
mod tests;
