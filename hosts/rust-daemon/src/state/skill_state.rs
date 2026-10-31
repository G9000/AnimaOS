//! Announcing skill changes (spec §6 `skill.updated`). Skills are
//! workspace-wide, so every companion's stream hears each change; helpers'
//! streams do not (their companion's does). Switching the workspace forgets
//! the last scan of the old one.

use super::DaemonState;
use crate::agent_runs::is_helper_config;
use crate::control_plane_store::WorkspaceConfig;
use crate::live::{LiveEvent, LiveEventBody};

impl DaemonState {
    /// Publishes `skill.updated` once to each non-helper agent's stream.
    /// Call it only after the change was saved.
    pub(crate) fn publish_skill_updated(&self, slug: Option<&str>, draft_id: Option<&str>) {
        for (agent_id, runtime) in &self.agents {
            if is_helper_config(runtime.config()) {
                continue;
            }
            self.live.publish(
                LiveEvent::new(
                    agent_id,
                    LiveEventBody::SkillUpdated {
                        slug: slug.map(str::to_string),
                        draft_id: draft_id.map(str::to_string),
                    },
                ),
                None,
            );
        }
    }

    /// Sets (or clears) the workspace. The last scan described the old
    /// folder, so it is forgotten and a scan in flight is dropped.
    pub(crate) fn set_workspace(&mut self, workspace: Option<WorkspaceConfig>) {
        if self.workspace.as_ref().map(|known| &known.root_path)
            != workspace.as_ref().map(|next| &next.root_path)
        {
            self.workspace_document_authority.revoke_all();
            self.document_inspections.revoke_all();
        }
        self.workspace = workspace;
        self.skills.reset_scan();
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use anima_core::DataValue;

    use crate::agent_runs::test_support::{companion_config, next_event};
    use crate::control_plane_store::WorkspaceConfig;
    use crate::skills::{compose_skill_file, ScannedFile};
    use crate::state::DaemonState;

    #[tokio::test]
    async fn skill_changes_reach_every_companion_but_not_helpers() {
        let mut state = DaemonState::new();
        let companion = state
            .create_agent(companion_config("companion"))
            .unwrap()
            .state
            .id;
        let mut helper = companion_config("helper");
        let additional = &mut helper.settings.as_mut().unwrap().additional;
        additional.insert("workspaceRole".into(), DataValue::String("helper".into()));
        additional.insert("parentAgentId".into(), DataValue::String(companion.clone()));
        let helper = state.create_agent(helper).unwrap().state.id;
        let mut companion_stream = state.live.subscribe(&companion).unwrap();
        let mut helper_stream = state.live.subscribe(&helper).unwrap();

        state.publish_skill_updated(Some("notes"), Some("skd_1"));

        let event = next_event(&mut companion_stream).await.to_json(1);
        assert_eq!(event["type"], "skill.updated");
        assert_eq!(event["agentId"], companion.as_str());
        assert_eq!(event["slug"], "notes");
        assert_eq!(event["draftId"], "skd_1");
        assert!(event.get("sessionId").is_none());
        assert!(
            tokio::time::timeout(Duration::from_millis(100), helper_stream.next())
                .await
                .is_err(),
            "a helper's stream hears nothing"
        );
    }

    #[test]
    fn changing_the_workspace_forgets_the_last_scan() {
        let mut state = DaemonState::new();
        let bytes = compose_skill_file("Notes", "About notes", "Do notes.").into_bytes();
        state
            .skills
            .set_scanned("notes", Some(ScannedFile::read(&bytes, None)));
        let generation = state.skills.generation();
        let workspace = WorkspaceConfig {
            root_path: std::env::temp_dir().join("anima-skills-switched"),
            company_name: "Acme".into(),
            mission: "Ship carefully".into(),
            values: vec![],
        };

        state.set_workspace(Some(workspace.clone()));

        assert!(state.skills.scanned_files().is_empty());
        assert!(state.skills.generation() > generation);
        assert_eq!(
            state.workspace.as_ref().map(|known| &known.root_path),
            Some(&workspace.root_path)
        );
    }
    #[test]
    fn workspace_switch_retires_read_leases_without_reusing_old_authority() {
        let mut state = DaemonState::new();
        let before = state.workspace_document_authority.lease("agent", true);
        let workspace = WorkspaceConfig {
            root_path: std::env::temp_dir().join("anima-document-authority-test"),
            company_name: String::new(),
            mission: String::new(),
            values: vec![],
        };
        state.set_workspace(Some(workspace.clone()));
        assert!(before.is_cancelled());
        let current = state.workspace_document_authority.lease("agent", true);
        state.set_workspace(Some(workspace));
        assert!(!current.is_cancelled());
        state.set_workspace(None);
        assert!(current.is_cancelled());
        assert!(before.is_cancelled());
    }
}
