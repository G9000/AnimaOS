use crate::routes::ApiError;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use utoipa::ToSchema;

pub(super) const POLICY_VERSION: u32 = 1;
pub(super) const DAILY_LIMIT: usize = 2;
pub(super) const PENDING_LIMIT: usize = 10;
pub(super) const RETAINED_LIMIT: usize = 1000;
pub(super) const BRIEF_LIMIT: usize = 12 * 1024;
pub(super) const OUTPUT_LIMIT: usize = 6 * 1024;
pub(super) const TIMEOUT_MS: u64 = 20_000;

pub(super) fn valid_advice_text(text: &str) -> bool {
    !text.trim().is_empty()
        && !crate::memory_text::has_hidden_text(text)
        && !text
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LearningReviewState {
    Queued,
    Running,
    Completed,
    Cancelled,
    Interrupted,
    Failed,
}
impl LearningReviewState {
    pub(super) fn pending(self) -> bool {
        matches!(self, Self::Queued | Self::Running)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LearningTrigger {
    Manual,
    Automatic,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LearningDecision {
    Pending,
    Dismissed,
    Acknowledged,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LearningUsage {
    pub(crate) prompt_tokens: u64,
    pub(crate) completion_tokens: u64,
    pub(crate) total_tokens: u64,
    pub(crate) cached_prompt_tokens: u64,
    pub(crate) reasoning_tokens: u64,
}
impl From<anima_core::TokenUsage> for LearningUsage {
    fn from(value: anima_core::TokenUsage) -> Self {
        Self {
            prompt_tokens: value.prompt_tokens,
            completion_tokens: value.completion_tokens,
            total_tokens: value.total_tokens,
            cached_prompt_tokens: value.cached_prompt_tokens,
            reasoning_tokens: value.reasoning_tokens,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub(crate) struct LearningFailure {
    pub(crate) code: String,
    pub(crate) message: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LearningProposal {
    pub(crate) id: String,
    pub(crate) observation: String,
    pub(crate) suggestion: String,
    pub(crate) anchors: Vec<String>,
    pub(crate) decision: LearningDecision,
    pub(crate) decision_version: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LearningReviewRecord {
    pub(crate) id: String,
    pub(crate) agent_id: String,
    pub(crate) source_run_id: String,
    pub(crate) session_id: String,
    pub(crate) trigger: LearningTrigger,
    pub(crate) policy_version: u32,
    pub(crate) evidence_digest: String,
    pub(crate) evidence_brief: String,
    pub(crate) evidence_anchors: Vec<String>,
    pub(crate) state: LearningReviewState,
    pub(crate) reserved_at_ms: u64,
    pub(crate) started_at_ms: Option<u64>,
    pub(crate) finished_at_ms: Option<u64>,
    pub(crate) model: String,
    pub(crate) provider: Option<String>,
    pub(crate) usage: Option<LearningUsage>,
    pub(crate) usage_complete: bool,
    pub(crate) error: Option<LearningFailure>,
    pub(crate) proposals: Vec<LearningProposal>,
}
impl LearningReviewRecord {
    pub(super) fn finish(
        &mut self,
        state: LearningReviewState,
        code: &str,
        message: &str,
        now: u64,
    ) {
        self.state = state;
        self.finished_at_ms = Some(now.max(self.reserved_at_ms));
        self.error = (!code.is_empty()).then(|| LearningFailure {
            code: code.into(),
            message: message.into(),
        });
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LearningPolicy {
    pub(crate) agent_id: String,
    pub(crate) auto_enabled: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LearningPolicyResponse {
    pub(crate) auto_enabled: bool,
    pub(crate) daily_limit: usize,
    pub(crate) pending_limit: usize,
    pub(crate) timeout_ms: u64,
    pub(crate) physical_reviewer_active: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct LearningSnapshot {
    pub(crate) reviews: Vec<LearningReviewRecord>,
    pub(crate) policies: Vec<LearningPolicy>,
}
#[derive(Clone, Debug, Default)]
pub(crate) struct LearningRegistry {
    pub(super) reviews: BTreeMap<String, LearningReviewRecord>,
    pub(super) policies: BTreeMap<String, bool>,
}
impl LearningRegistry {
    pub(crate) fn snapshot(&self) -> LearningSnapshot {
        LearningSnapshot {
            reviews: self.reviews.values().cloned().collect(),
            policies: self
                .policies
                .iter()
                .map(|(agent_id, auto_enabled)| LearningPolicy {
                    agent_id: agent_id.clone(),
                    auto_enabled: *auto_enabled,
                })
                .collect(),
        }
    }
    pub(crate) fn restored(snapshot: LearningSnapshot, live: &HashSet<String>, now: u64) -> Self {
        let mut registry = Self::default();
        for mut review in snapshot.reviews {
            // Retain every reservation, including deleted-agent attempts: deletion is not a quota refund.
            if review.state.pending() {
                review.finish(
                    LearningReviewState::Interrupted,
                    "review_interrupted",
                    "Review interrupted; no automatic replay was scheduled",
                    now,
                );
            }
            registry.reviews.insert(review.id.clone(), review);
        }
        registry.policies = snapshot
            .policies
            .into_iter()
            .filter(|p| live.contains(&p.agent_id))
            .map(|p| (p.agent_id, p.auto_enabled))
            .collect();
        registry
    }
    pub(crate) fn validate(snapshot: &LearningSnapshot) -> Result<(), String> {
        if snapshot.reviews.len() > RETAINED_LIMIT {
            return Err("learning review storage limit exceeded".into());
        }
        let mut ids = HashSet::new();
        let mut keys = HashSet::new();
        let mut policies = HashSet::new();
        for policy in &snapshot.policies {
            if policy.agent_id.is_empty() || !policies.insert(&policy.agent_id) {
                return Err("invalid learning policy".into());
            }
        }
        for record in &snapshot.reviews {
            if record.id.is_empty()
                || record.agent_id.is_empty()
                || record.source_run_id.is_empty()
                || record.session_id.is_empty()
                || record.model.is_empty()
                || record.reserved_at_ms == 0
                || record.evidence_brief.len() > BRIEF_LIMIT
                || record.evidence_digest.len() != 64
                || !record
                    .evidence_digest
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit())
                || record.evidence_anchors.is_empty()
                || record.evidence_anchors.len() > 10
                || record
                    .evidence_anchors
                    .iter()
                    .any(|a| a.is_empty() || a.len() > 300)
                || !ids.insert(&record.id)
                || !keys.insert((
                    &record.agent_id,
                    &record.source_run_id,
                    record.policy_version,
                ))
                || record.proposals.len() > 3
                || record
                    .started_at_ms
                    .is_some_and(|time| time < record.reserved_at_ms)
                || record
                    .finished_at_ms
                    .is_some_and(|time| time < record.reserved_at_ms)
            {
                return Err("invalid learning review record".into());
            }
            let mut proposal_ids = HashSet::new();
            for proposal in &record.proposals {
                if proposal.id.is_empty()
                    || !proposal_ids.insert(&proposal.id)
                    || proposal.observation.trim().is_empty()
                    || proposal.suggestion.trim().is_empty()
                    || proposal
                        .observation
                        .len()
                        .saturating_add(proposal.suggestion.len())
                        > 2048
                    || proposal.anchors.is_empty()
                    || proposal.anchors.len() > 10
                    || proposal
                        .anchors
                        .iter()
                        .any(|anchor| !record.evidence_anchors.contains(anchor))
                    || !valid_advice_text(&proposal.observation)
                    || !valid_advice_text(&proposal.suggestion)
                {
                    return Err("invalid learning proposal".into());
                }
            }
        }
        Ok(())
    }
    pub(super) fn enabled(&self, agent: &str) -> bool {
        self.policies.get(agent).copied().unwrap_or(false)
    }
    pub(super) fn policy(&self, agent: &str) -> LearningPolicyResponse {
        LearningPolicyResponse {
            auto_enabled: self.enabled(agent),
            daily_limit: DAILY_LIMIT,
            pending_limit: PENDING_LIMIT,
            timeout_ms: TIMEOUT_MS,
            physical_reviewer_active: false,
        }
    }
    pub(super) fn reserve(
        &mut self,
        review: LearningReviewRecord,
    ) -> Result<LearningReviewRecord, ApiError> {
        if let Some(existing) = self.reviews.values().find(|record| {
            record.agent_id == review.agent_id
                && record.source_run_id == review.source_run_id
                && record.policy_version == review.policy_version
        }) {
            return if existing.evidence_digest == review.evidence_digest {
                Ok(existing.clone())
            } else {
                Err(ApiError::conflict("Learning source evidence changed"))
            };
        }
        if self.reviews.len() >= RETAINED_LIMIT {
            return Err(ApiError::service_unavailable(
                "Learning review storage limit reached",
            ));
        }
        if self
            .reviews
            .values()
            .filter(|record| {
                record.reserved_at_ms / 86_400_000 == review.reserved_at_ms / 86_400_000
            })
            .count()
            >= DAILY_LIMIT
        {
            return Err(ApiError::conflict("Learning review daily budget exhausted"));
        }
        if self
            .reviews
            .values()
            .filter(|record| record.state.pending())
            .count()
            >= PENDING_LIMIT
        {
            return Err(ApiError::conflict("Learning review pending limit reached"));
        }
        self.reviews.insert(review.id.clone(), review.clone());
        Ok(review)
    }
}
