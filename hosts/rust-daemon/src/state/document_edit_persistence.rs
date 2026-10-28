//! Private document transitions join the same ordered atomic control-plane save.
use super::{ControlPlaneAcknowledged, ControlPlanePersistRequest, SupersededControlPlaneSave};
use crate::document_edits::DocumentEditRegistry;

/// Observed uncertainty is settlement data, never permission for a file effect.
pub(crate) enum DocumentEditSaveOutcome {
    Acknowledged,
    ObservedUncertain(DocumentEditRegistry),
}

impl super::DaemonState {
    pub(crate) fn document_edit_invalidation(&self, agent: &str) -> Option<DocumentEditRegistry> {
        let mut next = self.document_edits.clone();
        next.invalidate_agent(agent);
        (next != self.document_edits).then_some(next)
    }
}

impl ControlPlanePersistRequest {
    pub(crate) fn with_document_edit_candidate(
        mut self,
        registry: DocumentEditRegistry,
    ) -> Option<Self> {
        if self.config.is_none() || registry.validate().is_err() {
            return None;
        }
        self.document_edit_candidate = Some(self.snapshot.document_edits.clone());
        self.snapshot.document_edits = registry;
        Some(self)
    }

    pub(crate) async fn save_document_edit_candidate(self) -> std::io::Result<()> {
        self.save_ordered(true).await
    }

    pub(crate) async fn save_document_edit_transition(
        self,
    ) -> std::io::Result<DocumentEditSaveOutcome> {
        if self.document_edit_candidate.is_none() {
            return Err(std::io::Error::other(
                "Document transition has no private candidate",
            ));
        }
        let mut observed = None;
        match self
            .save_ordered_with_document_observation(true, Some(&mut observed))
            .await
        {
            Ok(()) => Ok(DocumentEditSaveOutcome::Acknowledged),
            Err(error) => match observed {
                Some(registry) => Ok(DocumentEditSaveOutcome::ObservedUncertain(registry)),
                None => Err(error),
            },
        }
    }

    pub(super) fn merge_acknowledged_document_edits(
        &mut self,
        acknowledged: &ControlPlaneAcknowledged,
    ) -> std::io::Result<()> {
        if let Some(saved) = &acknowledged.document_edits {
            if let Some(base) = &self.document_edit_candidate {
                if base != saved {
                    return Err(std::io::Error::other(SupersededControlPlaneSave));
                }
            } else {
                self.snapshot.document_edits = saved.clone();
            }
        }
        self.snapshot
            .document_edits
            .validate()
            .map_err(std::io::Error::other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control_plane_store::{load_control_plane_snapshot, ControlPlaneStoreConfig};
    use crate::state::DaemonState;

    fn publication() -> DocumentEditRegistry {
        use sha2::{Digest, Sha256};
        let key = hex::encode(Sha256::digest(
            serde_json::to_vec(&("profile", "workspace", "report.docx")).unwrap(),
        ));
        serde_json::from_value(serde_json::json!({
            "drafts": {}, "reservations": {}, "publications": {key: {
                "proposal_id":"proposal", "profile_key":"profile", "workspace_key":"workspace",
                "agent_id":"agent", "session_id":"chat:one", "clear_generation":0,
                "file_path":"report.docx", "sha256":"a".repeat(64), "size_bytes":100,
                "format":"docx", "source":{"kind":"workspace","file_path":"source.docx","sha256":"b".repeat(64)},
                "expected_destination":{"state":"absent"}, "review_sha256":"c".repeat(64),
                "run_id":"run", "approval_id":"approval", "tool_call_id":"call", "checkpoint_id":null,
                "published_at_ms":10
            }}
        })).unwrap()
    }

    #[tokio::test]
    async fn document_acknowledgement_survives_delayed_ordinary_save_without_live_publication() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("control-plane.json");
        let mut state = DaemonState::new();
        state.set_control_plane_store(Some(ControlPlaneStoreConfig::Json(path.clone())));
        let registry = publication();
        registry.validate().unwrap();
        let candidate = state
            .control_plane_persist_request()
            .with_document_edit_candidate(registry.clone())
            .unwrap();
        let delayed = state.control_plane_persist_request();
        candidate.save_document_edit_candidate().await.unwrap();
        assert_eq!(state.document_edits, DocumentEditRegistry::default());
        delayed.save().await.unwrap();
        let saved = load_control_plane_snapshot(&ControlPlaneStoreConfig::Json(path))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(saved.document_edits, registry);
    }

    #[tokio::test]
    async fn project_and_document_candidates_share_one_atomic_snapshot() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("control-plane.json");
        let mut state = DaemonState::new();
        state.set_control_plane_store(Some(ControlPlaneStoreConfig::Json(path.clone())));
        let config =
            serde_json::from_value(serde_json::json!({"name":"owner","model":"deterministic"}))
                .unwrap();
        let agent = state.create_agent(config).unwrap().state.id;
        let mut projects = crate::projects::ProjectRegistry::default();
        projects
            .create(
                crate::projects::ProjectCreateRequest {
                    request_key: "create".into(),
                    title: "Project".into(),
                    member_agent_ids: vec![agent],
                    instructions: String::new(),
                },
                1,
            )
            .unwrap();
        let registry = publication();
        state
            .control_plane_persist_request()
            .with_project_candidate(
                projects.snapshot(),
                projects.creation_receipts_snapshot(),
                Vec::new(),
            )
            .unwrap()
            .with_document_edit_candidate(registry.clone())
            .unwrap()
            .save_document_edit_candidate()
            .await
            .unwrap();
        let saved = load_control_plane_snapshot(&ControlPlaneStoreConfig::Json(path))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(saved.projects, projects.snapshot());
        assert_eq!(
            saved.project_creation_receipts,
            projects.creation_receipts_snapshot()
        );
        assert_eq!(saved.document_edits, registry);
    }

    #[test]
    fn document_candidates_require_durable_metadata_and_valid_seals() {
        let mut state = DaemonState::new();
        assert!(state
            .control_plane_persist_request()
            .with_document_edit_candidate(publication())
            .is_none());
        let directory = tempfile::tempdir().unwrap();
        state.set_control_plane_store(Some(ControlPlaneStoreConfig::Json(
            directory.path().join("control-plane.json"),
        )));
        let mut invalid = serde_json::to_value(publication()).unwrap();
        invalid["reservations"] = serde_json::json!({"unsealed":"proposal"});
        let invalid = serde_json::from_value(invalid).unwrap();
        assert!(state
            .control_plane_persist_request()
            .with_document_edit_candidate(invalid)
            .is_none());
    }

    #[tokio::test]
    async fn observed_uncertain_document_section_can_publish_and_accept_later_saves() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("control-plane.json");
        let mut state = DaemonState::new();
        state.set_control_plane_store(Some(ControlPlaneStoreConfig::Json(path.clone())));
        state.control_plane_persist_request().save().await.unwrap();
        let registry = publication();
        let gate = state.install_test_control_plane_uncertain_save_gate();
        let candidate = state
            .control_plane_persist_request()
            .with_document_edit_candidate(registry.clone())
            .unwrap();
        let delayed = state.control_plane_persist_request();
        let save = tokio::spawn(candidate.save_document_edit_transition());
        gate.entered.acquire().await.unwrap().forget();
        assert_eq!(state.document_edits, DocumentEditRegistry::default());
        gate.release.add_permits(1);
        let observed = match save.await.unwrap().unwrap() {
            DocumentEditSaveOutcome::ObservedUncertain(observed) => observed,
            DocumentEditSaveOutcome::Acknowledged => {
                panic!("fault must report observed uncertainty")
            }
        };
        assert_eq!(observed, registry);
        // The service publishes exactly the observed section under its existing
        // transaction. Ordinary carry-forward and new transitions then agree.
        state.document_edits = observed;
        delayed.save().await.unwrap();
        assert!(matches!(
            state
                .control_plane_persist_request()
                .with_document_edit_candidate(state.document_edits.clone())
                .unwrap()
                .save_document_edit_transition()
                .await
                .unwrap(),
            DocumentEditSaveOutcome::Acknowledged
        ));
        let saved = load_control_plane_snapshot(&ControlPlaneStoreConfig::Json(path))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(saved.document_edits, registry);
    }

    #[tokio::test]
    async fn known_failure_and_supersession_are_never_observed_uncertain_outcomes() {
        let directory = tempfile::tempdir().unwrap();
        let mut state = DaemonState::new();
        state.set_control_plane_store(Some(ControlPlaneStoreConfig::Json(
            directory.path().to_owned(),
        )));
        assert!(state
            .control_plane_persist_request()
            .with_document_edit_candidate(publication())
            .unwrap()
            .save_document_edit_transition()
            .await
            .is_err());
        state.set_control_plane_store(Some(ControlPlaneStoreConfig::Json(
            directory.path().join("control-plane.json"),
        )));
        let stale = state
            .control_plane_persist_request()
            .with_document_edit_candidate(publication())
            .unwrap();
        state.control_plane_persist_request().save().await.unwrap();
        let error = match stale.save_document_edit_transition().await {
            Err(error) => error,
            Ok(_) => panic!("supersession cannot report an observed section"),
        };
        assert!(super::super::is_superseded_control_plane_save(&error));
        assert_eq!(state.document_edits, DocumentEditRegistry::default());
    }
}
