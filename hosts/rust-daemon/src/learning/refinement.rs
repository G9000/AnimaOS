//! Bounded reviewer suggestions handed to the existing skill approval services.

use anima_core::skill_learning::{apply_patches, TextPatch};
use serde::Deserialize;

use super::records::{
    LearningDecision, LearningReviewState, LearningSkillContent, LearningSkillDraftRequest,
    LearningTrigger,
};
use super::LearningService;
use crate::skills::{parse_skill_file, DraftStatus, SkillError};

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct SkillPatch {
    old_text: String,
    new_text: String,
    #[serde(default)]
    file_path: Option<String>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct SkillCandidate {
    slug: String,
    expected_hash: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    patches: Vec<SkillPatch>,
    #[serde(default)]
    files: Option<Vec<crate::skills::SkillSupportingFile>>,
}

pub(super) fn valid_candidate(value: &serde_json::Value) -> bool {
    if value.to_string().len() > 4096 || value.get("expected_hash").is_none() {
        return false;
    }
    let Ok(candidate) = serde_json::from_value::<SkillCandidate>(value.clone()) else {
        return false;
    };
    if !crate::skills::is_valid_slug(&candidate.slug) {
        return false;
    }
    match &candidate.expected_hash {
        Some(hash) => {
            hash.len() == 64
                && hash
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
                && candidate.body.is_none()
                && candidate.files.is_none()
                && !candidate.patches.is_empty()
                && candidate.patches.len() <= 16
                && candidate.patches.iter().all(|p| {
                    !p.old_text.is_empty()
                        && p.old_text != p.new_text
                        && p.file_path.as_ref().is_none_or(|path| {
                            anima_core::skill_packages::validate_supporting_path(path).is_ok()
                        })
                })
        }
        None => {
            candidate.patches.is_empty()
                && candidate
                    .name
                    .as_ref()
                    .is_some_and(|n| crate::skills::validate_name(n).is_ok())
                && candidate
                    .description
                    .as_ref()
                    .is_some_and(|d| crate::skills::validate_description(d).is_ok())
                && candidate
                    .body
                    .as_ref()
                    .is_some_and(|b| crate::skills::validate_body(b).is_ok())
        }
    }
}

impl LearningService {
    pub(crate) fn recover_skill_work(&self) {
        if tokio::runtime::Handle::try_current().is_err() {
            return;
        }
        let service = self.clone();
        tokio::spawn(async move {
            loop {
                let pending: Vec<_> = {
                    let guard = service.state.read().await;
                    guard
                        .learning
                        .reviews
                        .values()
                        .filter(|r| {
                            r.state == LearningReviewState::Completed
                                && r.trigger != LearningTrigger::Conversation
                                && (r.trigger == LearningTrigger::Manual
                                    || guard.learning_auto_allowed(&r.agent_id))
                                && serde_json::from_str::<serde_json::Value>(&r.evidence_brief)
                                    .is_ok_and(|b| b["skillLearningAllowed"] == true)
                                && r.proposals.iter().any(|p| {
                                    p.skill_candidate.is_some()
                                        && p.skill_error.is_none()
                                        && p.decision == LearningDecision::Pending
                                        && match &p.skill_draft_id {
                                            None => true,
                                            Some(id) => {
                                                guard.learning_mode(&r.agent_id)
                                                    == super::LearningMode::Free
                                                    && guard.skills.draft(id).is_some_and(|d| {
                                                        d.status == DraftStatus::Pending
                                                    })
                                            }
                                        }
                                })
                        })
                        .take(10)
                        .map(|r| r.id.clone())
                        .collect()
                };
                if pending.is_empty() {
                    break;
                }
                for id in pending {
                    service.refine_skills(&id).await;
                }
            }
        });
    }

    pub(super) async fn refine_skills(&self, review_id: &str) {
        let _refining = self.refinements.lock().await;
        let review = {
            let guard = self.state.read().await;
            let Some(review) = guard.learning.reviews.get(review_id).filter(|r| {
                r.state == LearningReviewState::Completed
                    && r.trigger != LearningTrigger::Conversation
            }) else {
                return;
            };
            let brief: serde_json::Value =
                serde_json::from_str(&review.evidence_brief).unwrap_or_default();
            if brief["skillLearningAllowed"] != true {
                return;
            }
            if review.trigger != LearningTrigger::Manual
                && !guard.learning_auto_allowed(&review.agent_id)
            {
                return;
            }
            review.clone()
        };
        for proposal in &review.proposals {
            let Some(value) = &proposal.skill_candidate else {
                continue;
            };
            if proposal.skill_error.is_some() || proposal.decision != LearningDecision::Pending {
                continue;
            }
            let result = async {
                let fresh = super::evidence::capture_source(
                    &self.state,
                    &review.agent_id,
                    &review.source_run_id,
                )
                .await
                .map_err(|_| SkillError::conflict("Learning source is unavailable"))?;
                if fresh.digest != review.evidence_digest
                    || !serde_json::from_str::<serde_json::Value>(&fresh.brief)
                        .is_ok_and(|b| b["skillLearningAllowed"] == true)
                {
                    return Err(SkillError::conflict(
                        "Current source is no longer eligible for automatic skill learning",
                    ));
                }
                let draft = if let Some(id) = &proposal.skill_draft_id {
                    self.state
                        .read()
                        .await
                        .skills
                        .draft(id)
                        .cloned()
                        .ok_or_else(|| {
                            SkillError::conflict("Linked draft is no longer available")
                        })?
                } else {
                    let candidate: SkillCandidate = serde_json::from_value(value.clone())
                        .map_err(|_| invalid("Invalid skill candidate"))?;
                    let input = self
                        .prepare_refinement(&review, proposal, candidate)
                        .await?;
                    self.adopt_skill(&review.agent_id, &review.id, &proposal.id, input)
                        .await
                        .map_err(|e| SkillError::Conflict(e.message().into()))?
                        .0
                };
                if draft.status == DraftStatus::Pending {
                    let service = crate::skills::SkillService::new(
                        self.state.clone(),
                        self.transactions.clone(),
                    );
                    service.activate_learning_draft(draft).await?;
                }
                Ok::<(), SkillError>(())
            }
            .await;
            if let Err(error) = result {
                let _transaction = self.transactions.lock().await;
                let mut guard = self.state.write().await;
                if let Some(p) = guard
                    .learning
                    .reviews
                    .get_mut(review_id)
                    .and_then(|r| r.proposals.iter_mut().find(|p| p.id == proposal.id))
                {
                    p.skill_error = Some(super::evidence::bounded_text(&error.message(), 512));
                }
                let persist = guard.control_plane_persist_request();
                drop(guard);
                let _ = persist.save().await;
            }
        }
    }

    async fn prepare_refinement(
        &self,
        review: &super::LearningReviewRecord,
        proposal: &super::LearningProposal,
        candidate: SkillCandidate,
    ) -> Result<LearningSkillDraftRequest, SkillError> {
        let (root, existing, duplicate_bodies) = {
            let guard = self.state.read().await;
            let root = guard
                .workspace
                .as_ref()
                .ok_or(SkillError::NoWorkspace)?
                .root_path
                .clone();
            let record = guard.skills.get(&candidate.slug).cloned();
            let bodies: Vec<String> = guard
                .skills
                .records()
                .into_iter()
                .filter(|r| r.status == crate::skills::SkillStatus::Active)
                .filter_map(|r| {
                    guard
                        .skills
                        .scanned(&r.slug)
                        .and_then(|f| f.parsed.as_ref().ok())
                        .map(|f| normalize(&f.body))
                })
                .collect();
            (root, record, bodies)
        };
        let brief: serde_json::Value = serde_json::from_str(&review.evidence_brief)
            .map_err(|_| invalid("Invalid learning evidence"))?;
        if let Some(hash) = &candidate.expected_hash {
            let supplied = brief["skills"]
                .as_array()
                .and_then(|skills| {
                    skills
                        .iter()
                        .find(|s| s["slug"] == candidate.slug && s["hash"] == *hash)
                })
                .ok_or_else(|| {
                    SkillError::conflict("The target was not read in this review's skill evidence")
                })?;
            for patch in &candidate.patches {
                let shown = match &patch.file_path {
                    None => supplied["body"].as_str(),
                    Some(path) => supplied["files"]
                        .as_array()
                        .and_then(|files| files.iter().find(|f| f["path"] == *path))
                        .and_then(|f| f["text"].as_str()),
                };
                if !shown.is_some_and(|text| text.contains(&patch.old_text)) {
                    return Err(SkillError::conflict(
                        "Patch text was not supplied in this review's read evidence",
                    ));
                }
            }
        }
        let slug = candidate.slug.clone();
        let package = crate::skills::service::blocking(move || {
            crate::skills::package::read_package(&root, &slug)
        })
        .await?
        .map_err(SkillError::Unavailable)?;
        if package.as_ref().map(|p| p.hash()) != candidate.expected_hash
            || existing.as_ref().map(|r| r.approved_hash.clone()) != candidate.expected_hash
        {
            return Err(SkillError::conflict("The skill base changed after review"));
        }
        let (name, description, body, files) = if let Some(package) = package {
            let file = parse_skill_file(&package.bytes).map_err(invalid)?;
            let mut body = file.body;
            let mut files = package.files;
            let mut grouped = std::collections::BTreeMap::<Option<String>, Vec<TextPatch>>::new();
            for patch in candidate.patches {
                grouped.entry(patch.file_path).or_default().push(TextPatch {
                    old_text: patch.old_text,
                    new_text: patch.new_text,
                });
            }
            for (path, patches) in grouped {
                if let Some(path) = path {
                    let file = files
                        .iter_mut()
                        .find(|f| f.path == path)
                        .ok_or_else(|| invalid("Supporting patch target was not reviewed"))?;
                    file.content =
                        apply_patches(&file.content, &patches).map_err(SkillError::Invalid)?;
                } else {
                    body = apply_patches(&body, &patches).map_err(SkillError::Invalid)?;
                }
            }
            (file.name, file.description, body, files)
        } else {
            let body = candidate
                .body
                .ok_or_else(|| invalid("New skill needs its procedure"))?;
            if duplicate_bodies.contains(&normalize(&body)) {
                return Err(SkillError::conflict(
                    "This procedure already exists; improve the existing skill",
                ));
            }
            (
                candidate.name.unwrap_or_default(),
                candidate.description.unwrap_or_default(),
                body,
                candidate.files.unwrap_or_default(),
            )
        };
        crate::threat_scan::refuse_flagged(&body).map_err(SkillError::Invalid)?;
        for file in &files {
            crate::threat_scan::refuse_flagged(&file.content).map_err(SkillError::Invalid)?;
        }
        anima_core::skill_packages::validate_skill_package(
            crate::skills::compose_skill_file(&name, &description, &body).as_bytes(),
            &files,
        )
        .map_err(invalid)?;
        Ok(LearningSkillDraftRequest {
            expected_version: proposal.decision_version,
            evidence_digest: review.evidence_digest.clone(),
            skill: LearningSkillContent {
                slug: candidate.slug,
                name,
                description,
                body,
                expected_hash: candidate.expected_hash,
                files: Some(files),
            },
        })
    }
}

fn normalize(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn invalid(message: &str) -> SkillError {
    SkillError::Invalid(message.to_owned())
}
