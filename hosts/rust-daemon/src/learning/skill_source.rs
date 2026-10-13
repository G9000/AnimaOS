//! Revalidation used by the existing skill approval transaction. No model call,
//! copied history, additional approval engine or canonical memory mutation.
use super::evidence::capture_source;
use super::{LearningDecision, LearningReviewState, LearningSkillSource};
use crate::app::SharedDaemonState;
use crate::skills::SkillError;

/// Caller holds the shared control-plane transaction until installation settles.
pub(crate) async fn validate_skill_source(
    state: &SharedDaemonState,
    source: &LearningSkillSource,
    draft_id: &str,
) -> Result<(), SkillError> {
    let captured = capture_source(state, &source.agent_id, &source.source_run_id)
        .await
        .map_err(|error| match error.status().as_u16() {
            503 => SkillError::Unavailable(error.message().into()),
            _ => {
                SkillError::Conflict("Lesson source changed or is unavailable; review again".into())
            }
        })?;
    let guard = state.read().await;
    let review = guard
        .learning
        .reviews
        .get(&source.review_id)
        .filter(|review| {
            review.agent_id == source.agent_id
                && review.source_run_id == source.source_run_id
                && review.session_id == source.session_id
                && review.evidence_digest == source.evidence_digest
                && review.state == LearningReviewState::Completed
        })
        .ok_or_else(|| SkillError::Conflict("Lesson source changed; review again".into()))?;
    let proposal = review
        .proposals
        .iter()
        .find(|proposal| proposal.id == source.proposal_id)
        .ok_or_else(|| SkillError::Conflict("Lesson source changed; review again".into()))?;
    if captured.digest != source.evidence_digest
        || captured.session_id != source.session_id
        || !guard.agents.contains_key(&source.agent_id)
        || guard
            .sessions
            .get(&source.agent_id, &source.session_id)
            .is_none()
        || proposal.decision == LearningDecision::Dismissed
        || proposal.skill_draft_id.as_deref() != Some(draft_id)
        || source.decision_version > proposal.decision_version
    {
        return Err(SkillError::Conflict(
            "Lesson source changed or was dismissed; review again".into(),
        ));
    }
    Ok(())
}
