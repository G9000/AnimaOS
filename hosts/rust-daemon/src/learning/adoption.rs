//! Explicit owner handoff to the existing pending skill-draft pipeline.
//! No inference, SKILL.md installation, grants or advice decision occur here.

use std::time::{SystemTime, UNIX_EPOCH};

use super::evidence::capture_source;
use super::records::{
    LearningDecision, LearningReviewRecord, LearningReviewState, LearningSkillSource,
};
use super::LearningService;
use super::LearningSkillDraftRequest;
use crate::routes::ApiError;
use crate::skills::{
    compose_skill_file, is_valid_slug, skill_hash, validate_body, validate_description,
    validate_name, DraftSource, ProposedBy, SkillDraft, SkillFile, MAX_PENDING_DRAFTS_PER_AGENT,
    SKILL_SLUG_INVALID, TOO_MANY_PENDING_DRAFTS,
};

impl LearningService {
    pub(crate) async fn adopt_skill(
        &self,
        agent_id: &str,
        review_id: &str,
        proposal_id: &str,
        input: LearningSkillDraftRequest,
    ) -> Result<(SkillDraft, bool), ApiError> {
        let file = checked_content(&input)?;
        let service = self.clone();
        let agent_id = agent_id.to_owned();
        let review_id = review_id.to_owned();
        let proposal_id = proposal_id.to_owned();
        // Own only data and the shared transaction. Dropping the HTTP request
        // cannot leave a draft installed without its durable learning pointer.
        tokio::spawn(async move {
            service
                .settle_skill_handoff(&agent_id, &review_id, &proposal_id, input, file)
                .await
        })
        .await
        .map_err(|_| unavailable())?
    }

    async fn settle_skill_handoff(
        &self,
        agent_id: &str,
        review_id: &str,
        proposal_id: &str,
        input: LearningSkillDraftRequest,
        file: SkillFile,
    ) -> Result<(SkillDraft, bool), ApiError> {
        let _transaction = self.transactions.lock().await;
        let (review, linked, root) = {
            let guard = self.state.read().await;
            if !guard.agents.contains_key(agent_id) {
                return Err(ApiError::not_found());
            }
            if guard.control_plane_store.is_none() || guard.learning_slots.is_closed() {
                return Err(unavailable());
            }
            let review = guard
                .learning
                .reviews
                .get(review_id)
                .filter(|row| row.agent_id == agent_id)
                .ok_or_else(ApiError::not_found)?;
            let proposal = checked_proposal(review, proposal_id, &input)?;
            if guard.sessions.get(agent_id, &review.session_id).is_none() {
                return Err(ApiError::not_found());
            }
            let linked = match &proposal.skill_draft_id {
                Some(id) => Some(guard.skills.draft(id).cloned().ok_or_else(|| {
                    ApiError::conflict(
                        "The linked skill draft is no longer available; no new draft was created",
                    )
                })?),
                None => None,
            };
            let root = guard
                .workspace
                .as_ref()
                .map(|workspace| workspace.root_path.clone())
                .ok_or_else(|| ApiError::conflict("Skills need a configured workspace"))?;
            (review.clone(), linked, root)
        };

        if let Some(draft) = linked {
            // A decided/pruned target is never reopened. Idempotent retry is
            // about the original immutable handoff, not its later file state.
            if !same_handoff(&draft, &review, proposal_id, &input, &file) {
                return Err(ApiError::conflict(
                    "This lesson already has a different skill handoff",
                ));
            }
            let fresh = capture_source(&self.state, agent_id, &review.source_run_id).await?;
            if fresh.digest != review.evidence_digest || fresh.session_id != review.session_id {
                return Err(ApiError::conflict(
                    "Learning source evidence changed; review it again",
                ));
            }
            let guard = self.state.read().await;
            recheck_owner(&guard, agent_id, &review, &root)?;
            let current = guard
                .learning
                .reviews
                .get(review_id)
                .ok_or_else(ApiError::not_found)?;
            let proposal = checked_proposal(current, proposal_id, &input)?;
            if proposal.skill_draft_id.as_deref() != Some(draft.id.as_str())
                || guard.skills.draft(&draft.id) != Some(&draft)
            {
                return Err(ApiError::conflict(
                    "The linked skill draft changed; refresh before retrying",
                ));
            }
            return Ok((draft, false));
        }

        // File work and exact history reads never hold a state read/write
        // guard. This is a bounded read using the existing hardened skill I/O.
        let disk_root = root.clone();
        let slug = input.skill.slug.clone();
        let bytes = crate::skills::service::blocking(move || {
            crate::skills::disk::read_skill_bytes(&disk_root, &slug)
        })
        .await
        .map_err(|_| unavailable())?
        .map_err(|_| unavailable())?;
        let actual_hash = bytes.as_ref().map(|bytes| skill_hash(bytes));
        let fresh = capture_source(&self.state, agent_id, &review.source_run_id).await?;
        if fresh.digest != review.evidence_digest || fresh.session_id != review.session_id {
            return Err(ApiError::conflict(
                "Learning source evidence changed; review it again",
            ));
        }

        let mut guard = self.state.write().await;
        recheck_owner(&guard, agent_id, &review, &root)?;
        let current = guard
            .learning
            .reviews
            .get(review_id)
            .ok_or_else(ApiError::not_found)?;
        let proposal = checked_proposal(current, proposal_id, &input)?;
        if proposal.skill_draft_id.is_some() {
            return Err(ApiError::conflict(
                "This lesson already has a skill handoff; refresh before retrying",
            ));
        }
        let expected_hash = input.skill.expected_hash.as_ref();
        let registered_hash = guard
            .skills
            .get(&input.skill.slug)
            .map(|record| &record.approved_hash);
        if registered_hash != expected_hash || actual_hash.as_ref() != expected_hash {
            // null means no registered target AND no file, including a rejected
            // file draft: handoff cannot silently replace unreviewed bytes.
            return Err(ApiError::conflict(
                "The skill changed; review its current hash before proposing",
            ));
        }
        if guard.skills.pending_from(agent_id) >= MAX_PENDING_DRAFTS_PER_AGENT {
            return Err(ApiError::conflict(TOO_MANY_PENDING_DRAFTS));
        }

        let old_skills = guard.skills.clone();
        let old_learning = guard.learning.clone();
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(u64::MAX as u128) as u64;
        let mut draft = SkillDraft::new(
            &input.skill.slug,
            file,
            DraftSource::Agent,
            Some(ProposedBy {
                agent_id: agent_id.into(),
                session_id: review.session_id.clone(),
                run_id: review.source_run_id.clone(),
            }),
            input.skill.expected_hash.clone(),
            timestamp,
        );
        draft.learning_source = Some(LearningSkillSource {
            agent_id: agent_id.into(),
            review_id: review.id.clone(),
            proposal_id: proposal_id.into(),
            source_run_id: review.source_run_id.clone(),
            session_id: review.session_id.clone(),
            evidence_digest: review.evidence_digest.clone(),
            decision_version: input.expected_version,
        });
        guard.skills.put_draft(draft.clone());
        // All identity checks above occur under this same state write guard.
        let proposal = guard
            .learning
            .reviews
            .get_mut(review_id)
            .and_then(|row| {
                row.proposals
                    .iter_mut()
                    .find(|proposal| proposal.id == proposal_id)
            })
            .expect("checked learning proposal remains present under write guard");
        proposal.skill_draft_id = Some(draft.id.clone());
        let persist = guard.control_plane_persist_request();
        drop(guard);
        if persist.save().await.is_err() {
            // Match existing SkillService registry rollback semantics. A store
            // may report an uncertain acknowledgement; do not claim durable
            // success, exactly-once storage, or write an unapproved file.
            let mut guard = self.state.write().await;
            guard.skills = old_skills;
            guard.learning = old_learning;
            return Err(unavailable());
        }
        self.state
            .read()
            .await
            .publish_skill_updated(Some(&draft.slug), Some(&draft.id));
        Ok((draft, true))
    }
}

fn checked_content(input: &LearningSkillDraftRequest) -> Result<SkillFile, ApiError> {
    if !is_valid_slug(&input.skill.slug) {
        return Err(ApiError::bad_request(SKILL_SLUG_INVALID));
    }
    if !hex_digest(&input.evidence_digest)
        || input
            .skill
            .expected_hash
            .as_ref()
            .is_some_and(|hash| !hex_digest(hash))
    {
        return Err(ApiError::bad_request(
            "Evidence and skill hashes must be 64 lowercase hexadecimal characters",
        ));
    }
    let name = validate_name(&input.skill.name).map_err(ApiError::bad_request)?;
    let description =
        validate_description(&input.skill.description).map_err(ApiError::bad_request)?;
    validate_body(&input.skill.body).map_err(ApiError::bad_request)?;
    Ok(SkillFile {
        name,
        description,
        body: input.skill.body.clone(),
    })
}

fn hex_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn checked_proposal<'a>(
    review: &'a LearningReviewRecord,
    proposal_id: &str,
    input: &LearningSkillDraftRequest,
) -> Result<&'a super::records::LearningProposal, ApiError> {
    if review.state != LearningReviewState::Completed {
        return Err(ApiError::conflict(
            "Only completed learning reviews can be handed off",
        ));
    }
    if review.evidence_digest != input.evidence_digest {
        return Err(ApiError::conflict(
            "Learning source evidence changed; review it again",
        ));
    }
    let proposal = review
        .proposals
        .iter()
        .find(|proposal| proposal.id == proposal_id)
        .ok_or_else(ApiError::not_found)?;
    if proposal.decision == LearningDecision::Dismissed
        || proposal.decision_version != input.expected_version
    {
        return Err(ApiError::conflict(
            "This lesson changed or was dismissed; refresh before proposing",
        ));
    }
    Ok(proposal)
}

fn same_handoff(
    draft: &SkillDraft,
    review: &LearningReviewRecord,
    proposal_id: &str,
    input: &LearningSkillDraftRequest,
    file: &SkillFile,
) -> bool {
    draft.slug == input.skill.slug
        && draft.base_hash == input.skill.expected_hash
        && compose_skill_file(&draft.name, &draft.description, &draft.body)
            == compose_skill_file(&file.name, &file.description, &file.body)
        && draft.learning_source.as_ref().is_some_and(|source| {
            source.agent_id == review.agent_id
                && source.review_id == review.id
                && source.proposal_id == proposal_id
                && source.source_run_id == review.source_run_id
                && source.session_id == review.session_id
                && source.evidence_digest == review.evidence_digest
                && source.decision_version <= input.expected_version
        })
}

fn recheck_owner(
    guard: &crate::state::DaemonState,
    agent_id: &str,
    review: &LearningReviewRecord,
    root: &std::path::Path,
) -> Result<(), ApiError> {
    if !guard.agents.contains_key(agent_id)
        || guard.sessions.get(agent_id, &review.session_id).is_none()
    {
        return Err(ApiError::not_found());
    }
    if guard.control_plane_store.is_none() || guard.learning_slots.is_closed() {
        return Err(unavailable());
    }
    if guard
        .workspace
        .as_ref()
        .is_none_or(|workspace| workspace.root_path != root)
    {
        return Err(ApiError::conflict(
            "The skill workspace changed; review again",
        ));
    }
    if guard.runs.get(&review.source_run_id).is_some_and(|run| {
        run.agent_id != agent_id
            || run.session_id != review.session_id
            || run.status != crate::runs::RunStatus::Completed
            || run.error.is_some()
            || run.stop.is_some()
    }) {
        return Err(ApiError::conflict("Learning source is no longer eligible"));
    }
    Ok(())
}

fn unavailable() -> ApiError {
    ApiError::service_unavailable(
        "Learning skill handoff could not be acknowledged; inspect the saved draft before retrying",
    )
}
