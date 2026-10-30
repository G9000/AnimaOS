//! Live workspace-document grants outlive model schema selection, not revocation.

use std::{collections::BTreeMap, sync::Mutex};

use anima_core::CancelSignal;

#[derive(Debug, Default)]
pub(crate) struct WorkspaceDocumentAuthority {
    state: Mutex<AuthorityState>,
}

#[derive(Debug, Default)]
struct AuthorityState {
    leases: BTreeMap<String, CancelSignal>,
    scope: CancelSignal,
}

impl WorkspaceDocumentAuthority {
    pub(crate) fn scope_lease(&self) -> CancelSignal {
        self.state
            .lock()
            .expect("workspace document authority lock poisoned")
            .scope
            .clone()
    }

    pub(crate) fn lease(&self, agent: &str, allowed: bool) -> CancelSignal {
        if !allowed {
            self.revoke(agent);
            let disabled = CancelSignal::new();
            disabled.cancel();
            return disabled;
        }
        self.state
            .lock()
            .expect("workspace document authority lock poisoned")
            .leases
            .entry(agent.to_owned())
            .or_default()
            .clone()
    }

    pub(crate) fn update(&self, agent: &str, allowed: bool) {
        if !allowed {
            self.revoke(agent);
        }
    }

    pub(crate) fn revoke(&self, agent: &str) {
        let previous = self
            .state
            .lock()
            .expect("workspace document authority lock poisoned")
            .leases
            .remove(agent);
        if let Some(previous) = previous {
            previous.cancel();
        }
    }

    pub(crate) fn revoke_all(&self) {
        let previous = {
            let mut state = self
                .state
                .lock()
                .expect("workspace document authority lock poisoned");
            std::mem::take(&mut *state)
        };
        previous.scope.cancel();
        for signal in previous.leases.into_values() {
            signal.cancel();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::WorkspaceDocumentAuthority;

    #[test]
    fn an_absent_grant_returns_a_permanently_disabled_lease() {
        let authority = WorkspaceDocumentAuthority::default();
        let denied = authority.lease("agent", false);
        assert!(denied.is_cancelled());
        let granted = authority.lease("agent", true);
        assert!(!granted.is_cancelled());
        assert!(denied.is_cancelled());
    }

    #[test]
    fn revocation_cancels_every_lease_of_the_current_generation() {
        let authority = WorkspaceDocumentAuthority::default();
        let first = authority.lease("agent", true);
        let second = authority.lease("agent", true);
        authority.update("agent", true);
        assert!(!first.is_cancelled());
        assert!(!second.is_cancelled());
        authority.update("agent", false);
        assert!(first.is_cancelled());
        assert!(second.is_cancelled());
    }

    #[test]
    fn regrant_creates_a_new_generation_without_reviving_old_leases() {
        let authority = WorkspaceDocumentAuthority::default();
        let old = authority.lease("agent", true);
        authority.revoke("agent");
        authority.update("agent", true);
        assert!(old.is_cancelled());
        let current = authority.lease("agent", true);
        assert!(old.is_cancelled());
        assert!(!current.is_cancelled());
        authority.revoke("agent");
        assert!(old.is_cancelled());
        assert!(current.is_cancelled());
    }

    #[test]
    fn revoking_one_agent_preserves_other_agents_and_workspace_reset_revokes_all() {
        let authority = WorkspaceDocumentAuthority::default();
        let first = authority.lease("first", true);
        let second = authority.lease("second", true);
        authority.revoke("first");
        assert!(first.is_cancelled());
        assert!(!second.is_cancelled());
        let replacement = authority.lease("first", true);
        authority.revoke_all();
        assert!(replacement.is_cancelled());
        assert!(second.is_cancelled());
        assert!(!authority.lease("second", true).is_cancelled());
    }

    #[test]
    fn denied_lookup_revokes_an_existing_grant() {
        let authority = WorkspaceDocumentAuthority::default();
        let active = authority.lease("agent", true);
        let denied = authority.lease("agent", false);
        assert!(active.is_cancelled());
        assert!(denied.is_cancelled());
    }

    #[test]
    fn workspace_generation_changes_independently_of_agent_read_permission() {
        let authority = WorkspaceDocumentAuthority::default();
        let old_scope = authority.scope_lease();
        let same_scope = authority.scope_lease();
        let read = authority.lease("agent", true);
        authority.revoke("agent");
        assert!(read.is_cancelled());
        assert!(!old_scope.is_cancelled());
        let replacement_read = authority.lease("agent", true);
        authority.revoke_all();
        assert!(old_scope.is_cancelled());
        assert!(same_scope.is_cancelled());
        assert!(replacement_read.is_cancelled());
        let new_scope = authority.scope_lease();
        assert!(!new_scope.is_cancelled());
        assert!(old_scope.is_cancelled());
        authority.revoke_all();
        assert!(new_scope.is_cancelled());
        assert!(!authority.scope_lease().is_cancelled());
    }
}
