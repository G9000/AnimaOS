//! Agent lifecycle operations use the owner skill service and its transaction.
use super::ledger::{LedgerChange, SkillLedgerAction, SkillLedgerEntry, LEDGER_ID_PREFIX};
use super::service::blocking;
use super::{is_valid_slug, ProposedBy, SkillError, SkillService};

impl SkillService {
    pub(super) async fn check_agent_skill_write(
        &self,
        by: &ProposedBy,
        tool: &str,
        slug: &str,
        approved: bool,
    ) -> Result<(), SkillError> {
        let guard = self.state.read().await;
        let agent = guard
            .agents
            .get(&by.agent_id)
            .ok_or(SkillError::NotFound)?
            .state();
        if crate::agent_runs::is_helper_config(&agent.config) || !agent.config.allows_tool(tool) {
            return Err(SkillError::conflict("This agent cannot manage skills"));
        }
        let call = anima_core::ToolCall {
            id: "skill-policy-recheck".into(),
            name: tool.into(),
            args: std::collections::BTreeMap::from([(
                "slug".into(),
                anima_core::DataValue::String(slug.into()),
            )]),
        };
        match guard.approval_verdict(&agent, &by.session_id, &call) {
            crate::approvals::Verdict::Allow => {}
            crate::approvals::Verdict::Ask if approved => {}
            _ => {
                return Err(SkillError::conflict(
                    "Current write policy requires owner approval or denies this skill change",
                ))
            }
        }
        Ok(())
    }

    pub(crate) async fn remove_from_agent(
        &self,
        slug: &str,
        by: ProposedBy,
        approved: bool,
    ) -> Result<Option<String>, SkillError> {
        if !is_valid_slug(slug) {
            return Err(SkillError::NotFound);
        }
        let slug = slug.to_string();
        self.locked(move |service, root| async move {
            service
                .check_agent_skill_write(&by, "remove_skill", &slug, approved)
                .await?;
            let record = service
                .state
                .read()
                .await
                .skills
                .get(&slug)
                .cloned()
                .ok_or(SkillError::NotFound)?;
            let (workspace, target) = (root.clone(), slug.clone());
            let package = blocking(move || super::package::read_package(&workspace, &target))
                .await?
                .map_err(SkillError::Unavailable)?
                .ok_or_else(|| SkillError::conflict(super::SKILL_MISSING))?;
            if package.hash() != record.approved_hash {
                return Err(SkillError::conflict(super::SKILL_HASH_MISMATCH));
            }
            let entry = SkillLedgerEntry {
                id: format!("{LEDGER_ID_PREFIX}{}", uuid::Uuid::new_v4()),
                agent_id: by.agent_id,
                skill_slug: slug.clone(),
                action: SkillLedgerAction::Removed,
                previous_content: Some(
                    String::from_utf8(package.bytes)
                        .map_err(|_| SkillError::conflict("The skill package is not valid text"))?,
                ),
                previous_files: package.files,
                previous_enabled: Some(record.enabled),
                previous_content_hash: Some(record.approved_hash),
                // Removal has no resulting content; retain the existing string schema.
                content_hash: String::new(),
                at: 0,
                run_id: Some(by.run_id),
                reverted_at: None,
                superseded_at: None,
            };
            service
                .delete_locked(
                    root,
                    slug,
                    Some(LedgerChange::Append {
                        entry,
                        require_free: false,
                    }),
                )
                .await
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::SharedDaemonState;
    use crate::approvals::autonomy::{AgentAutonomy, AutonomyMode};
    use crate::skills::ledger::{ProposeOutcome, RevertOutcome};
    use crate::skills::test_support::{
        broken_store, content, service, temp_workspace, with_workspace,
    };
    use crate::skills::{DraftApproval, Proposal, SkillSupportingFile};
    use crate::state::DaemonState;
    use std::{path::PathBuf, sync::Arc};
    use tokio::sync::RwLock;

    async fn fixture(mode: AutonomyMode) -> (SharedDaemonState, SkillService, ProposedBy, PathBuf) {
        let root = temp_workspace("agent-lifecycle");
        let mut config = crate::agent_runs::test_support::companion_config("companion");
        config.tools = Some(
            crate::tools::ToolRegistry::new()
                .resolve_descriptors([
                    "propose_skill",
                    "remove_skill",
                    "undo_skill",
                    "skill_history",
                ])
                .unwrap(),
        );
        let mut state = with_workspace(DaemonState::new(), &root);
        let agent = state.create_agent(config).unwrap().state.id;
        if mode == AutonomyMode::Free {
            state.start_free(&agent);
        } else {
            state.autonomy.set(AgentAutonomy {
                agent_id: agent.clone(),
                mode,
                ..Default::default()
            });
            if let Some(policy) = mode.preset() {
                state.approvals.set_policy(&agent, policy);
            }
        }
        let by = ProposedBy {
            agent_id: agent,
            session_id: "chat:lifecycle".into(),
            run_id: "run_lifecycle".into(),
        };
        let state = Arc::new(RwLock::new(state));
        let skills = service(&state);
        (state, skills, by, root)
    }

    #[tokio::test]
    async fn strict_skill_mutations_cannot_bypass_owner_approval() {
        let (state, skills, by, root) = fixture(AutonomyMode::Strict).await;
        skills.save("notes", content("Notes")).await.unwrap();
        assert!(skills
            .remove_from_agent("notes", by.clone(), false)
            .await
            .is_err());
        assert!(root.join("skills/notes/SKILL.md").exists());
        assert!(skills.ledger(10).await.is_empty());
        skills
            .remove_from_agent("notes", by.clone(), true)
            .await
            .unwrap();
        assert!(skills
            .revert_from_agent("notes", by.clone(), false)
            .await
            .is_err());
        assert!(state.read().await.skills.get("notes").is_none());
        skills.revert_from_agent("notes", by, true).await.unwrap();
        assert!(root.join("skills/notes/SKILL.md").exists());
    }

    #[tokio::test]
    async fn removal_rolls_back_the_package_registry_and_history_when_save_fails() {
        let (state, skills, by, root) = fixture(AutonomyMode::Free).await;
        let (record, _) = skills.save("notes", content("Notes")).await.unwrap();
        let bytes = std::fs::read(root.join("skills/notes/SKILL.md")).unwrap();
        state
            .write()
            .await
            .set_control_plane_store(Some(broken_store()));
        assert!(skills.remove_from_agent("notes", by, false).await.is_err());
        assert_eq!(
            state
                .read()
                .await
                .skills
                .get("notes")
                .unwrap()
                .approved_hash,
            record.approved_hash
        );
        assert_eq!(
            std::fs::read(root.join("skills/notes/SKILL.md")).unwrap(),
            bytes
        );
        assert!(skills.ledger(10).await.is_empty());
    }

    #[tokio::test]
    async fn undo_of_removal_refuses_a_replacement_and_unreviewed_supporting_edits() {
        let (_, skills, by, root) = fixture(AutonomyMode::Free).await;
        let mut initial = content("Notes");
        initial.files = Some(vec![SkillSupportingFile {
            path: "references/guide.md".into(),
            content: "Reviewed".into(),
        }]);
        skills.save("notes", initial).await.unwrap();
        std::fs::write(
            root.join("skills/notes/references/guide.md"),
            "An owner edit",
        )
        .unwrap();
        assert!(skills
            .remove_from_agent("notes", by.clone(), false)
            .await
            .is_err());
        assert!(skills.ledger(10).await.is_empty());
        std::fs::write(root.join("skills/notes/references/guide.md"), "Reviewed").unwrap();
        skills.remove_from_agent("notes", by, false).await.unwrap();
        skills.save("notes", content("Replacement")).await.unwrap();
        assert!(skills.revert_last("notes").await.is_err());
        assert!(skills
            .load("notes")
            .await
            .unwrap()
            .body
            .contains("Replacement"));
        assert!(skills.ledger(10).await[0].reverted_at.is_none());
    }

    #[tokio::test]
    async fn removal_history_and_complete_undo_survive_restart() {
        let (state, skills, by, root) = fixture(AutonomyMode::Free).await;
        let path = root.join("control-plane.json");
        let store = crate::control_plane_store::ControlPlaneStoreConfig::Json(path.clone());
        state
            .write()
            .await
            .set_control_plane_store(Some(store.clone()));
        let mut initial = content("Notes");
        initial.enabled = Some(false);
        initial.files = Some(vec![SkillSupportingFile {
            path: "templates/note.md".into(),
            content: "Exact template".into(),
        }]);
        let (record, _) = skills.save("notes", initial).await.unwrap();
        let before = std::fs::read(root.join("skills/notes/SKILL.md")).unwrap();
        skills.remove_from_agent("notes", by, false).await.unwrap();
        let persisted: crate::control_plane_store::ControlPlaneSnapshot =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert!(!persisted.skills.iter().any(|r| r.slug == "notes"));
        assert_eq!(persisted.skill_ledger.len(), 1);
        assert_eq!(persisted.skill_ledger[0].action, SkillLedgerAction::Removed);
        let mut restarted = DaemonState::new();
        restarted.restore_control_plane_snapshot(persisted).unwrap();
        restarted.set_control_plane_store(Some(store));
        let restarted = Arc::new(RwLock::new(restarted));
        let restored = service(&restarted);
        let RevertOutcome::Restored(result) = restored.revert_last("notes").await.unwrap() else {
            panic!("expected restored skill");
        };
        assert!(!result.enabled);
        assert_eq!(result.approved_hash, record.approved_hash);
        assert_eq!(
            std::fs::read(root.join("skills/notes/SKILL.md")).unwrap(),
            before
        );
        assert_eq!(
            std::fs::read_to_string(root.join("skills/notes/templates/note.md")).unwrap(),
            "Exact template"
        );
        let persisted: crate::control_plane_store::ControlPlaneSnapshot =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert!(persisted.skills.iter().any(|r| r.slug == "notes"));
        assert!(persisted.skill_ledger[0].reverted_at.is_some());
    }

    #[tokio::test]
    async fn approved_strict_agent_creations_and_updates_are_recorded_and_undoable() {
        let (_, skills, by, root) = fixture(AutonomyMode::Strict).await;
        for body in ["First version", "Second version"] {
            let proposed = skills
                .propose_from_agent(Proposal {
                    files: vec![],
                    by: by.clone(),
                    name: "Notes".into(),
                    description: "About Notes".into(),
                    body: body.into(),
                    slug: Some("notes".into()),
                })
                .await
                .unwrap();
            let ProposeOutcome::Drafted(draft) = proposed else {
                panic!("Strict must wait");
            };
            skills
                .approve_draft(&draft.id, DraftApproval::default())
                .await
                .unwrap();
        }
        let history = skills.ledger(10).await;
        assert_eq!(history[0].action, SkillLedgerAction::Updated);
        assert_eq!(history[1].action, SkillLedgerAction::Created);
        skills.revert_last("notes").await.unwrap();
        assert_eq!(
            skills.load("notes").await.unwrap().body.trim(),
            "First version"
        );
        skills.revert_last("notes").await.unwrap();
        assert!(!root.join("skills/notes").exists());
    }

    #[tokio::test]
    async fn uncertain_removal_acknowledgement_preserves_recoverable_history() {
        let (state, skills, by, root) = fixture(AutonomyMode::Free).await;
        let path = root.join("control-plane.json");
        state.write().await.set_control_plane_store(Some(
            crate::control_plane_store::ControlPlaneStoreConfig::Json(path.clone()),
        ));
        skills.save("notes", content("Notes")).await.unwrap();
        let gate = state
            .write()
            .await
            .install_test_control_plane_uncertain_save_gate();
        gate.release.add_permits(1);
        assert!(skills.remove_from_agent("notes", by, false).await.is_err());
        assert!(state.read().await.skills.get("notes").is_none());
        let persisted: crate::control_plane_store::ControlPlaneSnapshot =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(persisted.skill_ledger[0].action, SkillLedgerAction::Removed);
        assert!(!persisted.skills.iter().any(|r| r.slug == "notes"));
        let mut restarted = DaemonState::new();
        restarted.restore_control_plane_snapshot(persisted).unwrap();
        let restored = service(&Arc::new(RwLock::new(restarted)));
        assert!(matches!(
            restored.revert_last("notes").await.unwrap(),
            RevertOutcome::Restored(_)
        ));
    }
    #[tokio::test]
    async fn owner_replacements_fence_undo_even_when_their_content_is_identical_or_later_removed() {
        let (_, skills, by, root) = fixture(AutonomyMode::Free).await;
        skills
            .propose_from_agent(Proposal {
                files: vec![],
                by: by.clone(),
                name: "Notes".into(),
                description: "About Notes".into(),
                body: "Do Notes.".into(),
                slug: Some("notes".into()),
            })
            .await
            .unwrap();
        skills.save("notes", content("Notes")).await.unwrap();
        assert!(skills.revert_last("notes").await.is_err());
        assert!(root.join("skills/notes").exists());
        assert!(skills.ledger(10).await[0].superseded_at.is_some());
        skills.remove_from_agent("notes", by, false).await.unwrap();
        skills.save("notes", content("Replacement")).await.unwrap();
        skills.delete("notes").await.unwrap();
        assert!(skills.revert_last("notes").await.is_err());
        assert!(!root.join("skills/notes").exists());
    }

    #[tokio::test]
    async fn definite_undo_failure_restores_its_preimage_and_can_be_retried() {
        for removed in [false, true] {
            let (state, skills, by, root) = fixture(AutonomyMode::Free).await;
            skills.save("notes", content("Notes")).await.unwrap();
            skills
                .propose_from_agent(Proposal {
                    files: vec![],
                    by: by.clone(),
                    name: "Notes".into(),
                    description: "About Notes".into(),
                    body: "Corrected steps".into(),
                    slug: Some("notes".into()),
                })
                .await
                .unwrap();
            if removed {
                skills.remove_from_agent("notes", by, false).await.unwrap();
            }
            let before = std::fs::read(root.join("skills/notes/SKILL.md")).ok();
            let history = skills.ledger(10).await;
            state
                .write()
                .await
                .set_control_plane_store(Some(broken_store()));
            assert!(skills.revert_last("notes").await.is_err());
            assert_eq!(skills.ledger(10).await, history);
            assert_eq!(
                std::fs::read(root.join("skills/notes/SKILL.md")).ok(),
                before
            );
            state.write().await.set_control_plane_store(None);
            assert!(skills.revert_last("notes").await.is_ok());
        }
    }

    #[tokio::test]
    async fn skill_write_rechecks_custom_ask_and_deny_after_waiting_for_the_transaction() {
        use crate::approvals::{ApprovalPolicy, PolicyAction, RiskClass};
        for action in [PolicyAction::Ask, PolicyAction::Deny] {
            let (state, skills, by, root) = fixture(AutonomyMode::Free).await;
            skills.save("notes", content("Notes")).await.unwrap();
            let held = skills.transactions.clone().lock_owned().await;
            let changing = skills.clone();
            let count = Arc::strong_count(&skills.transactions);
            let task =
                tokio::spawn(async move { changing.remove_from_agent("notes", by, false).await });
            tokio::time::timeout(std::time::Duration::from_secs(10), async {
                while Arc::strong_count(&skills.transactions) <= count {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            let agent = state.read().await.agents.keys().next().unwrap().clone();
            state.write().await.approvals.set_policy(
                &agent,
                ApprovalPolicy::default().with(RiskClass::Write, action),
            );
            drop(held);
            assert!(task.await.unwrap().is_err());
            assert!(root.join("skills/notes/SKILL.md").exists());
            assert!(skills.ledger(10).await.is_empty());
        }
    }
    #[tokio::test]
    async fn uncertain_activation_is_reported_as_unconfirmed_instead_of_a_pending_draft() {
        let (state, skills, by, root) = fixture(AutonomyMode::Free).await;
        let path = root.join("control-plane.json");
        state.write().await.set_control_plane_store(Some(
            crate::control_plane_store::ControlPlaneStoreConfig::Json(path),
        ));
        let first = state
            .write()
            .await
            .install_test_control_plane_save_gate(false);
        let changing = skills.clone();
        let task = tokio::spawn(async move {
            changing
                .propose_from_agent(Proposal {
                    files: vec![],
                    by,
                    name: "Notes".into(),
                    description: "About Notes".into(),
                    body: "Reviewed workflow".into(),
                    slug: Some("notes".into()),
                })
                .await
        });
        tokio::time::timeout(std::time::Duration::from_secs(10), first.entered.acquire())
            .await
            .unwrap()
            .unwrap()
            .forget();
        let uncertain = state
            .write()
            .await
            .install_test_control_plane_uncertain_save_gate();
        uncertain.release.add_permits(1);
        first.release.add_permits(1);
        assert!(matches!(
            task.await.unwrap(),
            Err(SkillError::Unavailable(_))
        ));
        assert_eq!(
            skills.ledger(10).await[0].action,
            SkillLedgerAction::Created
        );
        assert!(state.read().await.skills.get("notes").is_some());
    }
}
