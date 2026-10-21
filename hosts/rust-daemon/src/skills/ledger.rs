//! Skills an agent saved itself (autonomy spec 2026-10-19): in Free, a
//! companion's `propose_skill` is approved at once through the owner's
//! approval path, marked `autonomy:free`, and recorded in this ledger so the
//! owner can undo it (`POST /api/skills/{slug}/revert-last`). Strict and
//! Custom keep the draft for the owner.
//!
//! The ledger is saved with the control plane, newest last, at most
//! `MAX_SKILL_LEDGER_ENTRIES` entries. An undo restores the content the
//! automatic change replaced, or removes a skill it created, through the
//! owner's own save and delete, and only while the skill still holds what
//! the automatic change wrote.

use serde::{Deserialize, Serialize};
use tracing::warn;
use utoipa::ToSchema;

use super::drafts::{ApprovedDraft, DraftApproval, Proposal};
use super::service::{blocking, SkillContent, SkillError, SkillService};
use super::{is_valid_slug, parse_skill_file, SkillDraft, SkillRecord};

/// `decidedBy` of a draft approved because its agent runs in Free.
pub(crate) const AUTONOMY_FREE_DECIDER: &str = "autonomy:free";
/// Ledger entries kept, newest first.
pub(crate) const MAX_SKILL_LEDGER_ENTRIES: usize = 200;
/// Entries one `GET /api/skill-ledger` returns at most.
pub(crate) const MAX_SKILL_LEDGER_PAGE: usize = 100;
/// The previous `SKILL.md` an automatic update may retain. Larger changes
/// remain pending so automatic activation always has a complete undo.
pub(crate) const MAX_LEDGER_PREVIOUS_CONTENT_BYTES: usize = 36 * 1024;
/// A ledger entry's id is this plus a v4 UUID.
pub(crate) const LEDGER_ID_PREFIX: &str = "skl_";

pub(crate) const SKILL_CHANGED_SINCE_AUTOMATIC: &str =
    "This skill changed after the agent saved it; edit it on the Skills page instead";
pub(crate) const PREVIOUS_SKILL_NOT_KEPT: &str =
    "The skill's previous content was not kept; edit it on the Skills page instead";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SkillLedgerAction {
    /// The agent's skill was new.
    Created,
    /// The agent's skill replaced approved content.
    Updated,
}

/// One automatic create or update of a skill.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SkillLedgerEntry {
    #[serde(default)]
    #[schema(value_type = Vec<serde_json::Value>)]
    pub(crate) previous_files: Vec<super::SkillSupportingFile>,
    /// `skl_<uuid>`.
    pub(crate) id: String,
    pub(crate) agent_id: String,
    pub(crate) skill_slug: String,
    pub(crate) action: SkillLedgerAction,
    /// The approved `SKILL.md` the change replaced (at most 36 KiB); `null`
    /// for a new skill, or when it was not kept.
    #[serde(default)]
    pub(crate) previous_content: Option<String>,
    /// The replaced content's approved hash; `null` for a new skill.
    #[serde(default)]
    pub(crate) previous_content_hash: Option<String>,
    /// The approved hash the change pinned.
    pub(crate) content_hash: String,
    /// When it was saved, in milliseconds.
    pub(crate) at: u64,
    #[serde(default)]
    pub(crate) run_id: Option<String>,
    /// When the owner undid it, in milliseconds.
    #[serde(default)]
    pub(crate) reverted_at: Option<u64>,
}

/// What a companion's `propose_skill` did.
#[derive(Clone, Debug)]
pub(crate) enum ProposeOutcome {
    /// A draft waits for the owner (Strict or Custom, or an automatic
    /// approval that could not happen).
    Drafted(SkillDraft),
    /// Free: the skill is saved and active now.
    Activated {
        approved: ApprovedDraft,
        action: SkillLedgerAction,
    },
}

/// What undoing an automatic change did.
#[derive(Clone, Debug)]
pub(crate) enum RevertOutcome {
    /// The previous approved content is back.
    Restored(SkillRecord),
    /// The skill the agent created is gone (its folder is in the trash).
    Removed,
}

/// Appends `entry`, keeping the newest `MAX_SKILL_LEDGER_ENTRIES`.
pub(crate) fn push_entry(ledger: &mut Vec<SkillLedgerEntry>, entry: SkillLedgerEntry) {
    ledger.push(entry);
    trim(ledger);
}

/// A saved ledger, cut to the newest `MAX_SKILL_LEDGER_ENTRIES`.
pub(crate) fn restored(mut ledger: Vec<SkillLedgerEntry>) -> Vec<SkillLedgerEntry> {
    ledger.sort_by_key(|entry| entry.at);
    trim(&mut ledger);
    ledger
}

fn trim(ledger: &mut Vec<SkillLedgerEntry>) {
    if ledger.len() > MAX_SKILL_LEDGER_ENTRIES {
        let excess = ledger.len() - MAX_SKILL_LEDGER_ENTRIES;
        ledger.drain(..excess);
    }
}

/// `propose_skill`'s answer once the skill is active.
pub(crate) fn activated_reply(approved: &ApprovedDraft, action: SkillLedgerAction) -> String {
    let verb = match action {
        SkillLedgerAction::Created => "Saved",
        SkillLedgerAction::Updated => "Updated",
    };
    format!(
        "{verb} the skill \"{}\" (/{}). It is active now; use load_skill to read it. The owner can undo this on the Skills page.",
        approved.skill.name, approved.skill.slug
    )
}

impl SkillService {
    /// A companion's `propose_skill` (spec §8.3): a draft for the owner, or
    /// in Free a skill approved at once through `approve_draft_by`, marked
    /// `autonomy:free`, and recorded in the ledger. An automatic approval
    /// that cannot happen (an unreviewed `SKILL.md` in the folder, a failed
    /// save) leaves the draft for the owner.
    pub(crate) async fn propose_from_agent(
        &self,
        proposal: Proposal,
    ) -> Result<ProposeOutcome, SkillError> {
        self.propose_from_agent_with_base(proposal, None).await
    }

    pub(crate) async fn propose_from_agent_with_base(
        &self,
        proposal: Proposal,
        expected_base: Option<Option<String>>,
    ) -> Result<ProposeOutcome, SkillError> {
        let draft = self.propose_with_base(proposal, expected_base).await?;
        match self.activate_learning_draft(draft.clone()).await {
            Ok(outcome) => Ok(outcome),
            Err(error) => {
                warn!(
                    skill = %draft.slug,
                    error = %error.message(),
                    "an agent's skill could not be approved automatically; it waits for the owner"
                );
                Ok(ProposeOutcome::Drafted(draft))
            }
        }
    }

    /// Activate this exact reviewed draft through the same automatic approval
    /// path as an ordinary agent proposal. Strict and Custom retain it for
    /// owner review. Approval rechecks Free while holding the transaction.
    pub(crate) async fn activate_learning_draft(
        &self,
        draft: SkillDraft,
    ) -> Result<ProposeOutcome, SkillError> {
        {
            let guard = self.state.read().await;
            if guard.skills.draft(&draft.id) != Some(&draft) || !draft.is_pending() {
                return Err(SkillError::conflict(
                    "The reviewed skill draft changed before automatic activation",
                ));
            }
            let by = draft.proposed_by.as_ref().ok_or_else(|| {
                SkillError::conflict("Only agent-proposed drafts can activate automatically")
            })?;
            if !guard.agents.contains_key(&by.agent_id) {
                return Err(SkillError::NotFound);
            }
            if !guard.agent_autonomy(&by.agent_id).is_free() {
                return Ok(ProposeOutcome::Drafted(draft));
            }
        }
        let action = if draft.base_hash.is_some() {
            SkillLedgerAction::Updated
        } else {
            SkillLedgerAction::Created
        };
        let approved = self
            .approve_draft_by_exact(
                &draft.id,
                DraftApproval::default(),
                Some(AUTONOMY_FREE_DECIDER.to_string()),
                Some(draft.clone()),
            )
            .await;
        Ok(ProposeOutcome::Activated {
            approved: approved?,
            action,
        })
    }

    /// Serializes bookkeeping with activation and rolls it back on a definite
    /// save failure. An uncertain acknowledgement retains the recoverable
    /// candidate rather than discarding a ledger that may already be on disk.
    async fn change_ledger(
        &self,
        change: impl FnOnce(&mut Vec<SkillLedgerEntry>) + Send + 'static,
    ) {
        let saved = self
            .locked(move |service, _root| async move {
                let (previous, persist) = {
                    let mut guard = service.state.write().await;
                    let previous = guard.skill_ledger.clone();
                    change(&mut guard.skill_ledger);
                    (previous, guard.control_plane_persist_request())
                };
                if let Err(error) = persist.save().await {
                    if !crate::control_plane_store::is_durability_uncertain(&error) {
                        service.state.write().await.skill_ledger = previous;
                    }
                    return Err(SkillError::Unavailable(error.to_string()));
                }
                Ok(())
            })
            .await;
        if let Err(error) = saved {
            warn!(error = %error.message(), "the skill ledger could not be saved");
        }
    }

    /// The ledger, newest first, at most `limit` (capped at 100) entries.
    pub(crate) async fn ledger(&self, limit: usize) -> Vec<SkillLedgerEntry> {
        self.state
            .read()
            .await
            .skill_ledger
            .iter()
            .rev()
            .take(limit.min(MAX_SKILL_LEDGER_PAGE))
            .cloned()
            .collect()
    }

    /// Undoes the newest automatic change to `slug` not undone yet: the
    /// previous approved content comes back, or a skill the agent created is
    /// deleted. 404 without one; 409 when the skill changed since.
    pub(crate) async fn revert_last(&self, slug: &str) -> Result<RevertOutcome, SkillError> {
        if !is_valid_slug(slug) {
            return Err(SkillError::NotFound);
        }
        self.workspace().await?;
        let (entry, current) = {
            let guard = self.state.read().await;
            let entry = guard
                .skill_ledger
                .iter()
                .rev()
                .find(|entry| entry.skill_slug == slug && entry.reverted_at.is_none())
                .cloned()
                .ok_or(SkillError::NotFound)?;
            let current = guard
                .skills
                .get(slug)
                .map(|record| record.approved_hash.clone());
            (entry, current)
        };
        if current.as_deref() != Some(entry.content_hash.as_str()) {
            return Err(SkillError::conflict(SKILL_CHANGED_SINCE_AUTOMATIC));
        }
        let workspace = self.workspace().await?;
        let target = slug.to_string();
        let actual = blocking(move || super::package::read_package(&workspace, &target))
            .await?
            .map_err(SkillError::Unavailable)?;
        if actual.as_ref().map(|p| p.hash()).as_deref() != Some(entry.content_hash.as_str()) {
            return Err(SkillError::conflict(SKILL_CHANGED_SINCE_AUTOMATIC));
        }
        let outcome = match entry.action {
            SkillLedgerAction::Created => {
                self.delete(slug).await?;
                RevertOutcome::Removed
            }
            SkillLedgerAction::Updated => {
                let text = entry
                    .previous_content
                    .as_deref()
                    .ok_or_else(|| SkillError::conflict(PREVIOUS_SKILL_NOT_KEPT))?;
                let file = parse_skill_file(text.as_bytes()).map_err(SkillError::conflict)?;
                let content = SkillContent {
                    files: Some(entry.previous_files.clone()),
                    name: file.name,
                    description: file.description,
                    body: file.body,
                    enabled: None,
                };
                let (record, _) = self.save(slug, content).await?;
                RevertOutcome::Restored(record)
            }
        };
        let (id, at) = (entry.id, anima_core::primitives::now_millis());
        self.change_ledger(move |ledger| {
            if let Some(entry) = ledger.iter_mut().find(|entry| entry.id == id) {
                entry.reverted_at = Some(at);
            }
        })
        .await;
        Ok(outcome)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tokio::sync::RwLock;

    use super::*;
    use crate::agent_runs::test_support::companion_config;
    use crate::app::SharedDaemonState;
    use crate::approvals::autonomy::{AgentAutonomy, AutonomyMode};
    use crate::skills::test_support::{
        content, service, skill_text, temp_workspace, with_workspace, write_skill,
    };
    use crate::skills::{
        compose_skill_file, skill_hash, DraftStatus, ProposedBy, SKILL_FILE_UNREVIEWED,
    };
    use crate::state::DaemonState;

    /// A daemon with a workspace and one companion in `mode`.
    async fn daemon(
        label: &str,
        mode: AutonomyMode,
    ) -> (SharedDaemonState, String, std::path::PathBuf) {
        let root = temp_workspace(label);
        let mut state = with_workspace(DaemonState::new(), &root);
        let agent = state
            .create_agent(companion_config("companion"))
            .unwrap()
            .state
            .id;
        match mode {
            AutonomyMode::Free => state.start_free(&agent),
            mode => {
                state.autonomy.set(AgentAutonomy {
                    agent_id: agent.clone(),
                    mode,
                    ..Default::default()
                });
            }
        }
        (Arc::new(RwLock::new(state)), agent, root)
    }

    fn proposal(agent: &str, name: &str, body: &str) -> Proposal {
        Proposal {
            files: Vec::new(),
            by: ProposedBy {
                agent_id: agent.into(),
                session_id: "chat:1".into(),
                run_id: "run_7".into(),
            },
            name: name.into(),
            description: format!("About {name}"),
            body: body.into(),
            slug: None,
        }
    }

    fn activated(outcome: ProposeOutcome) -> (ApprovedDraft, SkillLedgerAction) {
        match outcome {
            ProposeOutcome::Activated { approved, action } => (approved, action),
            ProposeOutcome::Drafted(draft) => panic!("expected an active skill, got {draft:?}"),
        }
    }

    #[tokio::test]
    async fn automatic_approval_persists_its_full_undo_package_in_the_activation_snapshot() {
        let (state, agent, root) = daemon("ledger-atomic", AutonomyMode::Free).await;
        let path = root.join("control-plane.json");
        state.write().await.set_control_plane_store(Some(
            crate::control_plane_store::ControlPlaneStoreConfig::Json(path.clone()),
        ));
        let skills = service(&state);
        let files = vec![crate::skills::SkillSupportingFile {
            path: "references/check.md".into(),
            content: "Check the output independently.".into(),
        }];
        let mut initial = content("Notes");
        initial.files = Some(files.clone());
        let (before, _) = skills.save("notes", initial).await.unwrap();
        let mut update = proposal(&agent, "Notes", "Use the corrected workflow.");
        update.files = files.clone();
        let draft = skills.propose(update).await.unwrap();
        let gate = state
            .write()
            .await
            .install_test_control_plane_save_gate(false);
        let approving = skills.clone();
        let id = draft.id.clone();
        let task = tokio::spawn(async move {
            approving
                .approve_draft_by(
                    &id,
                    DraftApproval::default(),
                    Some(AUTONOMY_FREE_DECIDER.into()),
                )
                .await
        });
        tokio::time::timeout(std::time::Duration::from_secs(10), gate.entered.acquire())
            .await
            .unwrap()
            .unwrap()
            .forget();
        let snapshot = state.read().await.control_plane_snapshot();
        gate.release.add_permits(1);
        let approved = task.await.unwrap().unwrap();
        assert_eq!(
            snapshot.skill_ledger.len(),
            1,
            "activation snapshot must already include Undo"
        );
        let entry = &snapshot.skill_ledger[0];
        assert_eq!(
            entry.previous_content_hash.as_deref(),
            Some(before.approved_hash.as_str())
        );
        assert_eq!(
            entry.previous_content.as_deref(),
            Some(skill_text("Notes").as_str())
        );
        assert_eq!(entry.previous_files, files);
        assert_eq!(entry.content_hash, approved.skill.approved_hash);
        let persisted: crate::control_plane_store::ControlPlaneSnapshot =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(persisted.skill_ledger, snapshot.skill_ledger);
        assert_eq!(
            persisted
                .skills
                .iter()
                .find(|skill| skill.slug == "notes")
                .unwrap()
                .approved_hash,
            entry.content_hash
        );
    }

    #[tokio::test]
    async fn automatic_approval_rechecks_free_policy_before_changing_files() {
        let (state, agent, root) = daemon("ledger-policy", AutonomyMode::Free).await;
        let skills = service(&state);
        let draft = skills
            .propose(proposal(&agent, "Notes", "Do Notes."))
            .await
            .unwrap();
        let held = skills.transactions.clone().lock_owned().await;
        let approving = skills.clone();
        let id = draft.id.clone();
        let held_count = Arc::strong_count(&skills.transactions);
        let task = tokio::spawn(async move {
            approving
                .approve_draft_by(
                    &id,
                    DraftApproval::default(),
                    Some(AUTONOMY_FREE_DECIDER.into()),
                )
                .await
        });
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            while Arc::strong_count(&skills.transactions) <= held_count {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        state.write().await.autonomy.set(AgentAutonomy {
            agent_id: agent,
            mode: AutonomyMode::Strict,
            ..Default::default()
        });
        drop(held);
        let refused = task.await.unwrap();
        assert!(
            matches!(refused, Err(SkillError::Conflict(_))),
            "{refused:?}"
        );
        assert!(!root.join("skills/notes/SKILL.md").exists());
        assert!(state
            .read()
            .await
            .skills
            .draft(&draft.id)
            .unwrap()
            .is_pending());
        assert!(skills.ledger(10).await.is_empty());
    }

    #[tokio::test]
    async fn automatic_approval_refuses_a_stale_base_on_disk() {
        let (state, agent, root) = daemon("ledger-base", AutonomyMode::Free).await;
        let skills = service(&state);
        let (before, _) = skills.save("notes", content("Notes")).await.unwrap();
        let draft = skills
            .propose(proposal(&agent, "Notes", "Do it better."))
            .await
            .unwrap();
        write_skill(&root, "notes", &skill_text("Owner changed it"));
        let refused = skills
            .approve_draft_by(
                &draft.id,
                DraftApproval::default(),
                Some(AUTONOMY_FREE_DECIDER.into()),
            )
            .await;
        assert!(
            matches!(refused, Err(SkillError::Conflict(_))),
            "{refused:?}"
        );
        assert_eq!(
            state
                .read()
                .await
                .skills
                .get("notes")
                .unwrap()
                .approved_hash,
            before.approved_hash
        );
        assert_eq!(
            std::fs::read_to_string(root.join("skills/notes/SKILL.md")).unwrap(),
            skill_text("Owner changed it")
        );
        assert!(skills.ledger(10).await.is_empty());
    }

    #[tokio::test]
    async fn automatic_approval_save_failure_restores_the_registry_and_undo_ledger() {
        let (state, agent, root) = daemon("ledger-failed-save", AutonomyMode::Free).await;
        let path = root.join("control-plane.json");
        state.write().await.set_control_plane_store(Some(
            crate::control_plane_store::ControlPlaneStoreConfig::Json(path.clone()),
        ));
        let skills = service(&state);
        let (before, _) = activated(
            skills
                .propose_from_agent(proposal(&agent, "Notes", "Do Notes."))
                .await
                .unwrap(),
        );
        let old_ledger = skills.ledger(10).await;
        let draft = skills
            .propose(proposal(&agent, "Notes", "Corrected workflow."))
            .await
            .unwrap();
        let gate = state
            .write()
            .await
            .install_test_control_plane_save_gate(true);
        gate.release.add_permits(1);
        let refused = skills.activate_learning_draft(draft.clone()).await;
        assert!(
            matches!(refused, Err(SkillError::Unavailable(_))),
            "{refused:?}"
        );
        assert_eq!(skills.ledger(10).await, old_ledger);
        assert_eq!(
            state
                .read()
                .await
                .skills
                .get("notes")
                .unwrap()
                .approved_hash,
            before.skill.approved_hash
        );
        assert!(state
            .read()
            .await
            .skills
            .draft(&draft.id)
            .unwrap()
            .is_pending());
        assert_eq!(
            skills.load("notes").await,
            Err(crate::skills::SKILL_CHANGED.into())
        );
        let persisted: crate::control_plane_store::ControlPlaneSnapshot =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(persisted.skill_ledger, old_ledger);
    }

    #[tokio::test]
    async fn automatic_approval_with_uncertain_ack_recovers_the_full_undo_after_restart() {
        let (state, agent, root) = daemon("ledger-uncertain", AutonomyMode::Free).await;
        let path = root.join("control-plane.json");
        state.write().await.set_control_plane_store(Some(
            crate::control_plane_store::ControlPlaneStoreConfig::Json(path.clone()),
        ));
        let skills = service(&state);
        let files = vec![crate::skills::SkillSupportingFile {
            path: "references/check.md".into(),
            content: "Keep the reviewed supporting file.".into(),
        }];
        let mut initial = content("Notes");
        initial.files = Some(files.clone());
        let (before, _) = skills.save("notes", initial).await.unwrap();
        let mut update = proposal(&agent, "Notes", "A corrected workflow.");
        update.files = files.clone();
        let draft = skills.propose(update).await.unwrap();
        let gate = state
            .write()
            .await
            .install_test_control_plane_uncertain_save_gate();
        gate.release.add_permits(1);
        let refused = skills.activate_learning_draft(draft).await;
        assert!(
            matches!(refused, Err(SkillError::Unavailable(_))),
            "{refused:?}"
        );
        let persisted: crate::control_plane_store::ControlPlaneSnapshot =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let activated_hash = persisted.skill_ledger[0].content_hash.clone();
        let mut restarted = DaemonState::new();
        restarted.restore_control_plane_snapshot(persisted).unwrap();
        let restored = service(&Arc::new(RwLock::new(restarted)));
        let loaded = restored.load("notes").await.unwrap();
        assert!(loaded.body.starts_with("A corrected workflow."));
        assert_eq!(loaded.approved_hash, activated_hash);
        let RevertOutcome::Restored(record) = restored.revert_last("notes").await.unwrap() else {
            panic!("an acknowledged disk snapshot must retain the previous package");
        };
        assert_eq!(record.approved_hash, before.approved_hash);
        assert_eq!(
            std::fs::read_to_string(root.join("skills/notes/SKILL.md")).unwrap(),
            skill_text("Notes")
        );
        assert_eq!(
            std::fs::read_to_string(root.join("skills/notes/references/check.md")).unwrap(),
            files[0].content
        );
    }

    #[tokio::test]
    async fn an_automatic_handoff_refuses_a_changed_reviewed_draft() {
        let (state, agent, root) = daemon("ledger-draft-changed", AutonomyMode::Free).await;
        let skills = service(&state);
        let draft = skills
            .propose(proposal(&agent, "Notes", "Reviewed steps."))
            .await
            .unwrap();
        let held = skills.transactions.clone().lock_owned().await;
        let approving = skills.clone();
        let mut changed = draft.clone();
        changed.body = "Different steps.".into();
        let held_count = Arc::strong_count(&skills.transactions);
        let task = tokio::spawn(async move { approving.activate_learning_draft(draft).await });
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            while Arc::strong_count(&skills.transactions) <= held_count {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        state.write().await.skills.put_draft(changed);
        drop(held);
        let refused = task.await.unwrap();
        assert!(
            matches!(refused, Err(SkillError::Conflict(_))),
            "{refused:?}"
        );
        assert!(!root.join("skills/notes/SKILL.md").exists());
        assert!(skills.ledger(10).await.is_empty());
    }

    #[tokio::test]
    async fn ledger_bookkeeping_waits_for_the_shared_transaction() {
        let (state, agent, _) = daemon("ledger-bookkeeping", AutonomyMode::Free).await;
        let skills = service(&state);
        skills
            .propose_from_agent(proposal(&agent, "Notes", "Do Notes."))
            .await
            .unwrap();
        let held = skills.transactions.clone().lock_owned().await;
        let changing = skills.clone();
        let held_count = Arc::strong_count(&skills.transactions);
        let task = tokio::spawn(async move {
            changing
                .change_ledger(|ledger| ledger[0].reverted_at = Some(42))
                .await;
        });
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            while Arc::strong_count(&skills.transactions) <= held_count {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let during = skills.ledger(1).await[0].reverted_at;
        drop(held);
        task.await.unwrap();
        assert_eq!(
            during, None,
            "bookkeeping must not race an activation rollback"
        );
        assert_eq!(skills.ledger(1).await[0].reverted_at, Some(42));
    }

    #[tokio::test]
    async fn ledger_bookkeeping_restores_previous_entries_on_a_definite_save_failure() {
        let (state, agent, _) = daemon("ledger-bookkeeping-failed", AutonomyMode::Free).await;
        let skills = service(&state);
        skills
            .propose_from_agent(proposal(&agent, "Notes", "Do Notes."))
            .await
            .unwrap();
        let before = skills.ledger(1).await;
        state
            .write()
            .await
            .set_control_plane_store(Some(crate::skills::test_support::broken_store()));
        skills
            .change_ledger(|ledger| ledger[0].reverted_at = Some(42))
            .await;
        assert_eq!(skills.ledger(1).await, before);
    }

    #[tokio::test]
    async fn in_free_a_proposed_skill_is_active_at_once_and_logged() {
        let (state, agent, root) = daemon("ledger-free", AutonomyMode::Free).await;
        let skills = service(&state);

        let (approved, action) = activated(
            skills
                .propose_from_agent(proposal(&agent, "Notes", "Do Notes."))
                .await
                .unwrap(),
        );

        assert_eq!(action, SkillLedgerAction::Created);
        assert_eq!(approved.draft.status, DraftStatus::Approved);
        assert_eq!(
            approved.draft.decided_by.as_deref(),
            Some(AUTONOMY_FREE_DECIDER)
        );
        assert_eq!(skills.load("notes").await.unwrap().body, "Do Notes.");
        let written = std::fs::read(root.join("skills/notes/SKILL.md")).unwrap();
        assert_eq!(approved.skill.approved_hash, skill_hash(&written));
        let stored = state
            .read()
            .await
            .skills
            .draft(&approved.draft.id)
            .cloned()
            .unwrap();
        assert_eq!(stored.decided_by.as_deref(), Some(AUTONOMY_FREE_DECIDER));

        let ledger = skills.ledger(10).await;
        assert_eq!(ledger.len(), 1);
        let entry = &ledger[0];
        assert!(entry.id.starts_with(LEDGER_ID_PREFIX));
        assert_eq!(entry.agent_id, agent);
        assert_eq!(entry.skill_slug, "notes");
        assert_eq!(entry.action, SkillLedgerAction::Created);
        assert_eq!(entry.previous_content, None);
        assert_eq!(entry.previous_content_hash, None);
        assert_eq!(entry.content_hash, approved.skill.approved_hash);
        assert_eq!(entry.run_id.as_deref(), Some("run_7"));
        let value = serde_json::to_value(entry).unwrap();
        assert_eq!(value["skillSlug"], "notes");
        assert_eq!(value["action"], "created");
        assert_eq!(
            activated_reply(&approved, action),
            "Saved the skill \"Notes\" (/notes). It is active now; use load_skill to read it. The owner can undo this on the Skills page."
        );
    }

    #[tokio::test]
    async fn undoing_an_update_restores_the_previous_content() {
        let (state, agent, root) = daemon("ledger-update", AutonomyMode::Free).await;
        let skills = service(&state);
        let (before, _) = skills.save("notes", content("Notes")).await.unwrap();

        let (approved, action) = activated(
            skills
                .propose_from_agent(proposal(&agent, "Notes", "Do it better."))
                .await
                .unwrap(),
        );
        assert_eq!(action, SkillLedgerAction::Updated);
        assert_eq!(skills.load("notes").await.unwrap().body, "Do it better.");
        let entry = skills.ledger(1).await.remove(0);
        assert_eq!(
            entry.previous_content.as_deref(),
            Some(skill_text("Notes").as_str())
        );
        assert_eq!(
            entry.previous_content_hash,
            Some(before.approved_hash.clone())
        );
        assert_eq!(entry.content_hash, approved.skill.approved_hash);

        let RevertOutcome::Restored(record) = skills.revert_last("notes").await.unwrap() else {
            panic!("expected the previous content back");
        };
        assert_eq!(record.approved_hash, before.approved_hash);
        assert_eq!(
            std::fs::read_to_string(root.join("skills/notes/SKILL.md")).unwrap(),
            skill_text("Notes")
        );
        assert_eq!(skills.load("notes").await.unwrap().body, "Do Notes.");
        assert!(skills.ledger(1).await[0].reverted_at.is_some());
        assert_eq!(
            skills.revert_last("notes").await.err(),
            Some(SkillError::NotFound),
            "nothing left to undo"
        );
    }

    #[tokio::test]
    async fn undoing_a_creation_removes_the_skill() {
        let (state, agent, root) = daemon("ledger-create", AutonomyMode::Free).await;
        let skills = service(&state);
        activated(
            skills
                .propose_from_agent(proposal(&agent, "Notes", "Do Notes."))
                .await
                .unwrap(),
        );

        assert!(matches!(
            skills.revert_last("notes").await.unwrap(),
            RevertOutcome::Removed
        ));
        assert!(state.read().await.skills.get("notes").is_none());
        assert!(!root.join("skills/notes/SKILL.md").exists());
        assert!(skills.load("notes").await.is_err());
        assert_eq!(
            skills.revert_last("ghost").await.err(),
            Some(SkillError::NotFound)
        );
    }

    #[tokio::test]
    async fn an_owner_edit_since_is_never_undone() {
        let (state, agent, _) = daemon("ledger-edited", AutonomyMode::Free).await;
        let skills = service(&state);
        activated(
            skills
                .propose_from_agent(proposal(&agent, "Notes", "Do Notes."))
                .await
                .unwrap(),
        );
        let mut edited = content("Notes");
        edited.body = "The owner's words.".into();
        skills.save("notes", edited).await.unwrap();

        assert_eq!(
            skills.revert_last("notes").await.err(),
            Some(SkillError::conflict(SKILL_CHANGED_SINCE_AUTOMATIC))
        );
        assert_eq!(
            skills.load("notes").await.unwrap().body,
            "The owner's words."
        );
    }

    #[tokio::test]
    async fn strict_and_custom_agents_still_leave_a_draft() {
        for mode in [AutonomyMode::Strict, AutonomyMode::Custom] {
            let (state, agent, root) = daemon("ledger-strict", mode).await;
            let skills = service(&state);
            let outcome = skills
                .propose_from_agent(proposal(&agent, "Notes", "Do Notes."))
                .await
                .unwrap();
            let ProposeOutcome::Drafted(draft) = outcome else {
                panic!("{mode:?} must leave a draft");
            };
            assert!(draft.is_pending());
            assert!(!root.join("skills/notes/SKILL.md").exists());
            assert!(skills.ledger(10).await.is_empty());
        }
    }

    #[tokio::test]
    async fn an_unreviewed_file_keeps_a_free_proposal_waiting_for_the_owner() {
        let (state, agent, root) = daemon("ledger-unreviewed", AutonomyMode::Free).await;
        let skills = service(&state);
        let path = write_skill(&root, "notes", &skill_text("Written by hand"));

        let outcome = skills
            .propose_from_agent(proposal(&agent, "Notes", "Do Notes."))
            .await
            .unwrap();

        let ProposeOutcome::Drafted(draft) = outcome else {
            panic!("an unreviewed file is never overwritten ({SKILL_FILE_UNREVIEWED})");
        };
        assert!(state
            .read()
            .await
            .skills
            .draft(&draft.id)
            .unwrap()
            .is_pending());
        assert_eq!(
            std::fs::read_to_string(path).unwrap(),
            skill_text("Written by hand")
        );
        assert!(skills.ledger(10).await.is_empty());
    }

    #[test]
    fn the_ledger_keeps_the_newest_two_hundred() {
        let entry = |at: u64| SkillLedgerEntry {
            previous_files: Vec::new(),
            id: format!("{LEDGER_ID_PREFIX}{at}"),
            agent_id: "agent-1".into(),
            skill_slug: "notes".into(),
            action: SkillLedgerAction::Updated,
            previous_content: Some(compose_skill_file("Notes", "About", "Body")),
            previous_content_hash: None,
            content_hash: "ab".repeat(32),
            at,
            run_id: None,
            reverted_at: None,
        };
        let mut ledger = Vec::new();
        for at in 0..(MAX_SKILL_LEDGER_ENTRIES as u64 + 5) {
            push_entry(&mut ledger, entry(at));
        }
        assert_eq!(ledger.len(), MAX_SKILL_LEDGER_ENTRIES);
        assert_eq!(ledger[0].at, 5);
        let saved: Vec<_> = (0..(MAX_SKILL_LEDGER_ENTRIES as u64 + 3))
            .rev()
            .map(entry)
            .collect();
        let back = restored(saved);
        assert_eq!(back.len(), MAX_SKILL_LEDGER_ENTRIES);
        assert_eq!(back[0].at, 3);
        assert_eq!(back.last().unwrap().at, MAX_SKILL_LEDGER_ENTRIES as u64 + 2);
    }
}
