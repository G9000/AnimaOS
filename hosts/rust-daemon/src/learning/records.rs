use crate::routes::ApiError;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use utoipa::ToSchema;

pub(super) const POLICY_VERSION: u32 = 1;
/// Reviews admitted per rolling window, shared by manual and automatic
/// triggers. A rolling window has no calendar day, so no host time zone can
/// move the reset away from what the owner sees.
pub(super) const DAILY_LIMIT: usize = 2;
pub(super) const WINDOW_MS: u64 = 86_400_000;
/// Automatic review only follows tasks with at least this many tool calls;
/// short tasks would spend the shared budget on little evidence.
pub(super) const AUTO_MIN_ACTIONS: usize = 5;
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
/// How lessons from automatic reviews are kept.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LearningMode {
    /// Long tasks are reviewed by themselves and their lessons are saved to
    /// memory at once; the owner can undo each one.
    #[default]
    Free,
    /// Every lesson waits in the Inbox until the owner accepts it.
    Strict,
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
    #[serde(default)]
    pub(crate) skill_draft_id: Option<String>,
    /// The memory a Free-mode lesson was saved as; Undo deletes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) memory_id: Option<String>,
}

/// Immutable lineage for an explicitly reviewed procedural handoff. Advice
/// remains unverified; this is not permission to install or run a skill.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LearningSkillSource {
    pub(crate) agent_id: String,
    pub(crate) review_id: String,
    pub(crate) proposal_id: String,
    pub(crate) source_run_id: String,
    pub(crate) session_id: String,
    pub(crate) evidence_digest: String,
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
    pub(crate) mode: LearningMode,
    pub(crate) auto_enabled: bool,
    pub(crate) daily_limit: usize,
    pub(crate) pending_limit: usize,
    pub(crate) timeout_ms: u64,
    pub(crate) physical_reviewer_active: bool,
    /// The rolling window `daily_limit` applies to.
    pub(crate) window_ms: u64,
    /// Workspace-wide reviews still admissible in the current window.
    pub(crate) remaining: usize,
    /// When the next review becomes admissible; `None` while one is.
    pub(crate) next_available_at_ms: Option<u64>,
    pub(crate) auto_min_actions: usize,
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
        let mut handoffs = HashSet::new();
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
                    || proposal.skill_draft_id.as_ref().is_some_and(|id| {
                        !id.starts_with(crate::skills::DRAFT_ID_PREFIX)
                            || id.len() > 100
                            || !handoffs.insert(id)
                    })
                    || proposal
                        .memory_id
                        .as_ref()
                        .is_some_and(|id| id.is_empty() || id.len() > 128)
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
    pub(super) fn policy(
        &self,
        agent: &str,
        now: u64,
        mode: LearningMode,
    ) -> LearningPolicyResponse {
        let (remaining, next_available_at_ms) = self.availability(now);
        LearningPolicyResponse {
            mode,
            auto_enabled: self.enabled(agent),
            daily_limit: DAILY_LIMIT,
            pending_limit: PENDING_LIMIT,
            timeout_ms: TIMEOUT_MS,
            physical_reviewer_active: false,
            window_ms: WINDOW_MS,
            remaining,
            next_available_at_ms,
            auto_min_actions: AUTO_MIN_ACTIONS,
        }
    }
    /// Reservations inside the window ending at `now`, oldest first.
    fn in_window(&self, now: u64) -> Vec<u64> {
        let start = now.saturating_sub(WINDOW_MS);
        let mut times: Vec<u64> = self
            .reviews
            .values()
            .map(|record| record.reserved_at_ms)
            .filter(|at| *at > start)
            .collect();
        times.sort_unstable();
        times
    }
    /// Remaining admissions and, when none remain, when the oldest counted
    /// reservation leaves the window.
    pub(super) fn availability(&self, now: u64) -> (usize, Option<u64>) {
        let times = self.in_window(now);
        let remaining = DAILY_LIMIT.saturating_sub(times.len());
        let next =
            (remaining == 0).then(|| times[times.len() - DAILY_LIMIT].saturating_add(WINDOW_MS));
        (remaining, next)
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
        if self.in_window(review.reserved_at_ms).len() >= DAILY_LIMIT {
            return Err(ApiError::conflict(format!(
                "Task reviews are used up for now ({DAILY_LIMIT} per 24 hours)"
            )));
        }
        if self
            .reviews
            .values()
            .filter(|record| record.state.pending())
            .count()
            >= PENDING_LIMIT
        {
            return Err(ApiError::conflict(
                "Too many task reviews are waiting to run; try again shortly",
            ));
        }
        self.reviews.insert(review.id.clone(), review.clone());
        Ok(review)
    }
}
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LearningSkillDraftRequest {
    pub(crate) expected_version: u64,
    pub(crate) evidence_digest: String,
    pub(crate) skill: LearningSkillContent,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LearningSkillContent {
    pub(crate) slug: String,
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) body: String,
    #[serde(deserialize_with = "required_nullable_hash")]
    pub(crate) expected_hash: Option<String>,
}

fn required_nullable_hash<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    Option::<String>::deserialize(deserializer)
}
