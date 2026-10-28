//! Ordered project candidates retain durable grouping without publishing access.

use std::collections::HashMap;

use super::{ControlPlaneAcknowledged, ControlPlanePersistRequest, SupersededControlPlaneSave};
use crate::control_plane_store::ControlPlaneSnapshot;
use crate::projects::{ProjectCreationReceipt, ProjectRecord, ProjectSessionBindingPatch};

pub(super) struct ProjectCandidateBase {
    records: Vec<ProjectRecord>,
    receipts: Vec<ProjectCreationReceipt>,
    bindings: HashMap<(String, String, u64), (Option<String>, u64)>,
}

pub(super) struct AcknowledgedProjects {
    records: Vec<ProjectRecord>,
    receipts: Vec<ProjectCreationReceipt>,
    bindings: HashMap<(String, String, u64), (Option<String>, u64)>,
}

pub(super) fn acknowledged_projects(snapshot: &ControlPlaneSnapshot) -> AcknowledgedProjects {
    AcknowledgedProjects {
        records: snapshot.projects.clone(),
        receipts: snapshot.project_creation_receipts.clone(),
        bindings: snapshot
            .sessions
            .iter()
            .map(|session| {
                (
                    (
                        session.agent_id.clone(),
                        session.id.clone(),
                        session.created_at_ms,
                    ),
                    (session.project_id.clone(), session.project_binding_revision),
                )
            })
            .collect(),
    }
}

impl ControlPlanePersistRequest {
    pub(crate) fn with_project_candidate(
        mut self,
        records: Vec<ProjectRecord>,
        receipts: Vec<ProjectCreationReceipt>,
        bindings: Vec<ProjectSessionBindingPatch>,
    ) -> Option<Self> {
        let mut base = ProjectCandidateBase {
            records: self.snapshot.projects.clone(),
            receipts: self.snapshot.project_creation_receipts.clone(),
            bindings: HashMap::new(),
        };
        let mut seen = std::collections::HashSet::new();
        for patch in bindings {
            if !seen.insert((patch.agent_id.clone(), patch.session_id.clone())) {
                return None;
            }
            let session = self.snapshot.sessions.iter_mut().find(|session| {
                session.agent_id == patch.agent_id && session.id == patch.session_id
            })?;
            if session.created_at_ms != patch.created_at_ms
                || session.project_binding_revision != patch.expected_binding_revision
                || patch.expected_binding_revision.checked_add(1)
                    != Some(patch.project_binding_revision)
            {
                return None;
            }
            base.bindings.insert(
                (
                    session.agent_id.clone(),
                    session.id.clone(),
                    session.created_at_ms,
                ),
                (session.project_id.clone(), session.project_binding_revision),
            );
            session.project_id = patch.project_id;
            session.project_binding_revision = patch.project_binding_revision;
        }
        self.snapshot.projects = records;
        self.snapshot.project_creation_receipts = receipts;
        self.project_candidate = Some(base);
        Some(self)
    }

    pub(crate) async fn save_project_candidate(self) -> std::io::Result<()> {
        self.save_ordered(true).await
    }

    pub(super) fn merge_acknowledged_projects(
        &mut self,
        acknowledged: &ControlPlaneAcknowledged,
    ) -> std::io::Result<()> {
        if let Some(saved) = &acknowledged.projects {
            if let Some(base) = &self.project_candidate {
                if base.records != saved.records || base.receipts != saved.receipts {
                    return Err(std::io::Error::other(SupersededControlPlaneSave));
                }
                if base
                    .bindings
                    .iter()
                    .any(|(key, expected)| saved.bindings.get(key) != Some(expected))
                {
                    return Err(std::io::Error::other(SupersededControlPlaneSave));
                }
            } else {
                self.snapshot.projects = saved.records.clone();
                self.snapshot.project_creation_receipts = saved.receipts.clone();
            }
            // Overlay only the grouping of an existing exact incarnation.
            // Missing/deleted sessions are never inserted; a newer move wins.
            for session in &mut self.snapshot.sessions {
                let key = (
                    session.agent_id.clone(),
                    session.id.clone(),
                    session.created_at_ms,
                );
                let Some((project, revision)) = saved.bindings.get(&key) else {
                    continue;
                };
                if session.project_binding_revision < *revision {
                    session.project_id = project.clone();
                    session.project_binding_revision = *revision;
                } else if session.project_binding_revision == *revision
                    && &session.project_id != project
                {
                    return Err(std::io::Error::other(SupersededControlPlaneSave));
                }
            }
        }
        crate::projects::ProjectRegistry::from_snapshot(
            self.snapshot.projects.clone(),
            self.snapshot.project_creation_receipts.clone(),
        )
        .map_err(std::io::Error::other)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control_plane_store::{load_control_plane_snapshot, ControlPlaneStoreConfig};
    use crate::projects::{ProjectCreateRequest, ProjectRegistry};
    use crate::sessions::{SessionKind, SessionOrigin, SessionRecord, TitleSource};
    use crate::state::DaemonState;

    fn fixture(path: &std::path::Path) -> (DaemonState, String, ProjectRegistry, String) {
        let mut state = DaemonState::new();
        let config = serde_json::from_value(serde_json::json!({
            "name": "project-persistence-fixture", "model": "deterministic"
        }))
        .unwrap();
        let agent = state.create_agent(config).unwrap().state.id;
        state.sessions.insert(SessionRecord::new(
            &agent,
            "chat:project",
            SessionKind::Chat,
            SessionOrigin::Web,
            "Fixture".into(),
            TitleSource::Owner,
            10,
        ));
        state.set_control_plane_store(Some(ControlPlaneStoreConfig::Json(path.to_owned())));
        let mut projects = ProjectRegistry::default();
        let (project, _) = projects
            .create(
                ProjectCreateRequest {
                    request_key: "fixture-create".into(),
                    title: "Shared fixture".into(),
                    member_agent_ids: vec![agent.clone()],
                    instructions: "Shared guidance".into(),
                },
                20,
            )
            .unwrap();
        (state, agent, projects, project.id)
    }

    fn binding(agent: &str, project: Option<String>, expected: u64) -> ProjectSessionBindingPatch {
        ProjectSessionBindingPatch {
            agent_id: agent.into(),
            session_id: "chat:project".into(),
            created_at_ms: 10,
            expected_binding_revision: expected,
            project_id: project,
            project_binding_revision: expected + 1,
        }
    }

    #[tokio::test]
    async fn delayed_ordinary_save_retains_all_acknowledged_project_candidate_parts() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("control-plane.json");
        let (mut state, agent, projects, project) = fixture(&path);
        let candidate = state
            .control_plane_persist_request()
            .with_project_candidate(
                projects.snapshot(),
                projects.creation_receipts_snapshot(),
                vec![binding(&agent, Some(project.clone()), 0)],
            )
            .unwrap();
        let delayed = state.control_plane_persist_request();
        candidate.save_project_candidate().await.unwrap();
        assert!(
            state.projects.records().next().is_none(),
            "save cannot publish access"
        );
        assert!(state
            .sessions
            .get(&agent, "chat:project")
            .unwrap()
            .project_id
            .is_none());
        delayed.save().await.unwrap();
        let saved = load_control_plane_snapshot(&ControlPlaneStoreConfig::Json(path))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(saved.projects, projects.snapshot());
        assert_eq!(
            saved.project_creation_receipts,
            projects.creation_receipts_snapshot()
        );
        let session = saved
            .sessions
            .iter()
            .find(|row| row.id == "chat:project")
            .unwrap();
        assert_eq!(session.project_id.as_deref(), Some(project.as_str()));
        assert_eq!(session.project_binding_revision, 1);
    }

    #[tokio::test]
    async fn delayed_ordinary_save_cannot_undo_project_deletion_or_its_chat_detach() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("control-plane.json");
        let (mut state, agent, mut projects, project) = fixture(&path);
        state.projects = projects.clone();
        let session = state.sessions.get_mut(&agent, "chat:project").unwrap();
        session.project_id = Some(project.clone());
        session.project_binding_revision = 1;
        state.control_plane_persist_request().save().await.unwrap();
        projects.delete(&project, 1, 30).unwrap();
        let candidate = state
            .control_plane_persist_request()
            .with_project_candidate(
                projects.snapshot(),
                projects.creation_receipts_snapshot(),
                vec![binding(&agent, None, 1)],
            )
            .unwrap();
        let delayed = state.control_plane_persist_request();
        candidate.save_project_candidate().await.unwrap();
        delayed.save().await.unwrap();
        let saved = load_control_plane_snapshot(&ControlPlaneStoreConfig::Json(path))
            .await
            .unwrap()
            .unwrap();
        assert!(saved.projects.is_empty());
        assert_eq!(saved.project_creation_receipts[0].deleted_at_ms, Some(30));
        let session = saved
            .sessions
            .iter()
            .find(|row| row.id == "chat:project")
            .unwrap();
        assert!(session.project_id.is_none());
        assert_eq!(session.project_binding_revision, 2);
    }

    #[test]
    fn binding_overlay_rejects_missing_recreated_or_changed_session() {
        let directory = tempfile::tempdir().unwrap();
        let (mut state, agent, projects, project) =
            fixture(&directory.path().join("control-plane.json"));
        for mutate in 0..3 {
            let mut patch = binding(&agent, Some(project.clone()), 0);
            match mutate {
                0 => patch.session_id = "chat:missing".into(),
                1 => patch.created_at_ms = 11,
                _ => patch.expected_binding_revision = 1,
            }
            assert!(state
                .control_plane_persist_request()
                .with_project_candidate(
                    projects.snapshot(),
                    projects.creation_receipts_snapshot(),
                    vec![patch],
                )
                .is_none());
        }
    }

    #[tokio::test]
    async fn candidate_cannot_acknowledge_an_overlay_against_a_newer_saved_binding() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("control-plane.json");
        let (mut state, agent, projects, project) = fixture(&path);
        state.projects = projects.clone();
        state
            .sessions
            .get_mut(&agent, "chat:project")
            .unwrap()
            .project_binding_revision = 2;
        state.control_plane_persist_request().save().await.unwrap();
        // A private preparation can still see stale live grouping after another
        // acknowledged candidate. Saving it must request a fresh preparation.
        state
            .sessions
            .get_mut(&agent, "chat:project")
            .unwrap()
            .project_binding_revision = 0;
        let candidate = state
            .control_plane_persist_request()
            .with_project_candidate(
                projects.snapshot(),
                projects.creation_receipts_snapshot(),
                vec![binding(&agent, Some(project), 0)],
            )
            .unwrap();
        let error = candidate.save_project_candidate().await.unwrap_err();
        assert!(super::super::is_superseded_control_plane_save(&error));
        let saved = load_control_plane_snapshot(&ControlPlaneStoreConfig::Json(path))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(saved.sessions[0].project_binding_revision, 2);
        assert!(saved.sessions[0].project_id.is_none());
    }

    #[tokio::test]
    async fn acknowledged_bindings_never_insert_deleted_sessions_or_replace_newer_grouping() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("control-plane.json");
        let (mut state, agent, projects, project) = fixture(&path);
        state
            .control_plane_persist_request()
            .with_project_candidate(
                projects.snapshot(),
                projects.creation_receipts_snapshot(),
                vec![binding(&agent, Some(project), 0)],
            )
            .unwrap()
            .save_project_candidate()
            .await
            .unwrap();
        let session = state.sessions.get_mut(&agent, "chat:project").unwrap();
        session.project_binding_revision = 2;
        session.project_id = None;
        state.control_plane_persist_request().save().await.unwrap();
        let config = ControlPlaneStoreConfig::Json(path.clone());
        let saved = load_control_plane_snapshot(&config).await.unwrap().unwrap();
        let session = saved
            .sessions
            .iter()
            .find(|row| row.id == "chat:project")
            .unwrap();
        assert!(session.project_id.is_none());
        assert_eq!(session.project_binding_revision, 2);
        state.sessions.remove(&agent, "chat:project");
        state.control_plane_persist_request().save().await.unwrap();
        let saved = load_control_plane_snapshot(&config).await.unwrap().unwrap();
        assert!(!saved.sessions.iter().any(|row| row.id == "chat:project"));
    }

    #[test]
    fn restoring_equal_durable_epochs_retires_preexisting_live_leases() {
        let directory = tempfile::tempdir().unwrap();
        let (mut state, agent, projects, project) =
            fixture(&directory.path().join("control-plane.json"));
        state.projects = projects;
        state
            .project_access
            .publish_records(&state.projects)
            .unwrap();
        let old = crate::projects::member_read_lease(&state, &project, &agent).unwrap();
        old.check().unwrap();
        state
            .restore_control_plane_snapshot(state.control_plane_snapshot())
            .unwrap();
        assert!(old.check().is_err());
        crate::projects::member_read_lease(&state, &project, &agent)
            .unwrap()
            .check()
            .unwrap();
    }

    #[test]
    fn invalid_restored_chat_binding_is_rejected_before_replacing_state_or_leases() {
        let directory = tempfile::tempdir().unwrap();
        let (mut state, agent, projects, project) =
            fixture(&directory.path().join("control-plane.json"));
        state.projects = projects;
        state
            .project_access
            .publish_records(&state.projects)
            .unwrap();
        let lease = crate::projects::member_read_lease(&state, &project, &agent).unwrap();
        let original = state.control_plane_snapshot();
        let mut invalid = state.control_plane_snapshot();
        invalid.sessions[0].project_id = Some("invalid-project-id".into());
        invalid.sessions[0].project_binding_revision = 1;
        invalid.agents[0].state.config.name = "must not be installed".into();
        assert!(state.restore_control_plane_snapshot(invalid).is_err());
        assert_eq!(
            state.control_plane_snapshot().agents[0].state.config.name,
            original.agents[0].state.config.name
        );
        assert_eq!(
            state
                .sessions
                .get(&agent, "chat:project")
                .unwrap()
                .project_id,
            None
        );
        lease.check().unwrap();
    }
}
