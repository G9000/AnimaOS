use crate::routes::ApiError;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use utoipa::ToSchema;

pub(super) const POLICY_VERSION: u32 = 1;
/// Task reviews admitted per rolling window, shared by manual and automatic
/// triggers. A rolling window has no calendar day, so no host time zone can
/// move the reset away from what the owner sees.
pub(super) const DAILY_LIMIT: usize = 2;
/// Owner messages between two conversation reviews.
pub(super) const CONVERSATION_EVERY: u32 = 10;
/// Conversation reviews admitted per agent per rolling window. They have their
/// own window, so they never use the task-review budget, nor it theirs.
pub(super) const CONVERSATION_DAILY_LIMIT: usize = 8;
/// Lesson proposals one review may return.
pub(super) const LESSON_LIMIT: usize = 3;
/// Core-memory proposals one conversation review keeps; extra ones are dropped.
pub(super) const CORE_EDIT_LIMIT: usize = 5;
pub(super) const WINDOW_MS: u64 = 86_400_000;
/// Automatic review only follows tasks with at least this many tool calls;
/// short tasks would spend the shared budget on little evidence.
pub(super) const AUTO_MIN_ACTIONS: usize = 5;
pub(super) const PENDING_LIMIT: usize = 10;
/// Task and manual reviews kept; they are never pruned.
pub(super) const RETAINED_LIMIT: usize = 1000;
/// Conversation reviews kept per agent. A new one prunes the agent's oldest
/// settled conversation review; task reviews never count toward it.
pub(super) const CONVERSATION_RETAINED_LIMIT: usize = 200;
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
    /// Every `CONVERSATION_EVERY` owner messages: the recent conversation is
    /// reviewed for core-memory edits as well as lessons.
    Conversation,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LearningDecision {
    Pending,
    Dismissed,
    Acknowledged,
    /// A core-memory edit a rule refused (budget, no match, scanner); the
    /// reason is on the edit. Never an owner decision.
    Failed,
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
    /// The memory a Free-mode lesson was saved as; Undo deletes it. For an
    /// applied core edit, the entry it added, replaced or removed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) memory_id: Option<String>,
    /// Set when the proposal is a core-memory edit rather than a lesson.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) core_edit: Option<CoreProposal>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CoreProposalAction {
    Add,
    Replace,
    Remove,
}

/// `user` is About you, `notes` is Agent notes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CoreProposalBlock {
    User,
    Notes,
}

impl CoreProposalBlock {
    pub(crate) fn block(self) -> crate::core_memory::CoreBlock {
        match self {
            Self::User => crate::core_memory::CoreBlock::User,
            Self::Notes => crate::core_memory::CoreBlock::Notes,
        }
    }
}

/// A core-memory edit proposed by a conversation review. Free mode applies it
/// at once; Strict mode applies it when the owner accepts. The fields after
/// `old_text` are filled in when it is applied or refused.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CoreProposal {
    pub(crate) action: CoreProposalAction,
    pub(crate) block: CoreProposalBlock,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) old_text: Option<String>,
    /// Why a core-memory rule refused the edit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<String>,
    /// A replaced entry's text before the edit; Undo puts it back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) previous_text: Option<String>,
    /// A removed entry; Undo restores it with its id and position.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) removed: Option<RemovedCoreEntry>,
}

impl CoreProposal {
    /// The line the owner reads: "Add to About you: …".
    pub(crate) fn describe(&self) -> String {
        let label = self.block.block().label();
        let text = self.text.as_deref().unwrap_or_default();
        let old_text = self.old_text.as_deref().unwrap_or_default();
        match self.action {
            CoreProposalAction::Add => format!("Add to {label}: {text}"),
            CoreProposalAction::Replace => {
                format!("Replace in {label}: {old_text} \u{2192} {text}")
            }
            CoreProposalAction::Remove => format!("Remove from {label}: {old_text}"),
        }
    }

    /// Text and `old_text` within bounds, and present exactly where the
    /// action needs them.
    pub(super) fn well_formed(&self) -> bool {
        let field = |value: &Option<String>| {
            value.as_deref().is_some_and(|text| {
                valid_advice_text(text) && text.chars().count() <= CORE_TEXT_LIMIT
            })
        };
        match self.action {
            CoreProposalAction::Add => field(&self.text) && self.old_text.is_none(),
            CoreProposalAction::Replace => field(&self.text) && field(&self.old_text),
            CoreProposalAction::Remove => self.text.is_none() && field(&self.old_text),
        }
    }
}

/// Generated text and `old_text` beyond this many characters are dropped
/// before they reach a rule; within it, the core-memory rules decide.
pub(super) const CORE_TEXT_LIMIT: usize = 2 * crate::core_memory::ENTRY_LIMIT;

/// A core proposal's reason and display line: both texts may be quoted.
pub(super) const CORE_PROPOSAL_BYTES: usize = 16 * 1024;

/// What Undo needs to put a removed core entry back.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RemovedCoreEntry {
    pub(crate) id: String,
    pub(crate) content: String,
    pub(crate) created_at: u64,
    #[serde(default)]
    pub(crate) tags: Vec<String>,
    #[schema(value_type = f64)]
    pub(crate) importance: serde_json::Number,
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
    /// Owner messages counted toward each agent's next conversation review.
    #[serde(default)]
    pub(crate) conversation_counts: BTreeMap<String, u32>,
    /// Each agent's last `CONVERSATION_EVERY` counted turns (run ids, oldest
    /// first): the next conversation review's evidence.
    #[serde(default)]
    pub(crate) conversation_turns: BTreeMap<String, Vec<String>>,
}
#[derive(Clone, Debug, Default)]
pub(crate) struct LearningRegistry {
    pub(super) reviews: BTreeMap<String, LearningReviewRecord>,
    pub(super) policies: BTreeMap<String, bool>,
    pub(super) conversation_counts: BTreeMap<String, u32>,
    pub(super) conversation_turns: BTreeMap<String, Vec<String>>,
}
/// An agent's conversation tally before one counted turn, so a run whose
/// commit was not saved can take its turn back.
#[derive(Debug)]
pub(crate) struct CountedTurn {
    agent_id: String,
    run_id: String,
    /// False when the count was already at `CONVERSATION_EVERY`.
    counted: bool,
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
            conversation_counts: self.conversation_counts.clone(),
            conversation_turns: self.conversation_turns.clone(),
        }
    }
    /// Counts one owner turn (`run_id`) for `agent_id`. The count stops at
    /// `CONVERSATION_EVERY`: there it stays until a conversation review is
    /// admitted, so a turn that cannot be reviewed yet retries with the next.
    pub(crate) fn count_turn(&mut self, agent_id: &str, run_id: &str) -> CountedTurn {
        let count = self.conversation_counts.entry(agent_id.into()).or_insert(0);
        let undo = CountedTurn {
            agent_id: agent_id.into(),
            run_id: run_id.into(),
            counted: *count < CONVERSATION_EVERY,
        };
        *count = count.saturating_add(1).min(CONVERSATION_EVERY);
        let turns = self.conversation_turns.entry(agent_id.into()).or_default();
        turns.retain(|turn| turn != run_id);
        turns.push(run_id.into());
        let excess = turns.len().saturating_sub(CONVERSATION_EVERY as usize);
        turns.drain(..excess);
        undo
    }
    /// Takes back a `count_turn` whose run was rolled back: only that run's
    /// turn, so a turn another run counted meanwhile is kept.
    pub(crate) fn uncount_turn(&mut self, counted: CountedTurn) {
        if counted.counted {
            if let Some(count) = self.conversation_counts.get_mut(&counted.agent_id) {
                *count = count.saturating_sub(1);
            }
        }
        if let Some(turns) = self.conversation_turns.get_mut(&counted.agent_id) {
            turns.retain(|turn| turn != &counted.run_id);
        }
    }
    /// Whether `agent_id` has counted enough turns for a conversation review.
    pub(crate) fn conversation_due(&self, agent_id: &str) -> bool {
        self.conversation_counts
            .get(agent_id)
            .is_some_and(|count| *count >= CONVERSATION_EVERY)
    }
    pub(super) fn conversation_turns(&self, agent_id: &str) -> Vec<String> {
        self.conversation_turns
            .get(agent_id)
            .cloned()
            .unwrap_or_default()
    }
    pub(super) fn reset_conversation(&mut self, agent_id: &str) {
        self.conversation_counts.insert(agent_id.into(), 0);
    }
    #[cfg(test)]
    pub(crate) fn insert_for_tests(&mut self, review: LearningReviewRecord) {
        self.reviews.insert(review.id.clone(), review);
    }
    /// Moves every reservation a full window into the past.
    #[cfg(test)]
    pub(crate) fn age_for_tests(&mut self) {
        for review in self.reviews.values_mut() {
            review.reserved_at_ms = review.reserved_at_ms.saturating_sub(WINDOW_MS).max(1);
            review.started_at_ms = review.started_at_ms.map(|_| review.reserved_at_ms);
            review.finished_at_ms = review.finished_at_ms.map(|_| review.reserved_at_ms);
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
        registry.conversation_counts = snapshot
            .conversation_counts
            .into_iter()
            .filter(|(agent_id, _)| live.contains(agent_id))
            .map(|(agent_id, count)| (agent_id, count.min(CONVERSATION_EVERY)))
            .collect();
        registry.conversation_turns = snapshot
            .conversation_turns
            .into_iter()
            .filter(|(agent_id, _)| live.contains(agent_id))
            .map(|(agent_id, mut turns)| {
                let excess = turns.len().saturating_sub(CONVERSATION_EVERY as usize);
                turns.drain(..excess);
                (agent_id, turns)
            })
            .collect();
        registry
    }
    pub(crate) fn validate(snapshot: &LearningSnapshot) -> Result<(), String> {
        // Conversation reviews are capped when reserved, never at load.
        if snapshot
            .reviews
            .iter()
            .filter(|review| review.trigger != LearningTrigger::Conversation)
            .count()
            > RETAINED_LIMIT
        {
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
                || record
                    .proposals
                    .iter()
                    .filter(|p| p.core_edit.is_none())
                    .count()
                    > LESSON_LIMIT
                || record
                    .proposals
                    .iter()
                    .filter(|p| p.core_edit.is_some())
                    .count()
                    > CORE_EDIT_LIMIT
                || (record.trigger != LearningTrigger::Conversation
                    && record.proposals.iter().any(|p| p.core_edit.is_some()))
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
                        > if proposal.core_edit.is_some() {
                            CORE_PROPOSAL_BYTES
                        } else {
                            2048
                        }
                    || proposal
                        .core_edit
                        .as_ref()
                        .is_some_and(|edit| !edit.well_formed())
                    || (proposal.decision == LearningDecision::Failed
                        && proposal.core_edit.is_none())
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
    /// Task-review reservations inside the window ending at `now`, oldest
    /// first. Conversation reviews have their own window.
    fn in_window(&self, now: u64) -> Vec<u64> {
        self.window(now, |record| {
            record.trigger != LearningTrigger::Conversation
        })
    }
    /// `agent_id`'s conversation-review reservations inside the window.
    pub(super) fn conversations_in_window(&self, agent_id: &str, now: u64) -> usize {
        self.window(now, |record| {
            record.trigger == LearningTrigger::Conversation && record.agent_id == agent_id
        })
        .len()
    }
    fn window(&self, now: u64, counted: impl Fn(&LearningReviewRecord) -> bool) -> Vec<u64> {
        let start = now.saturating_sub(WINDOW_MS);
        let mut times: Vec<u64> = self
            .reviews
            .values()
            .filter(|record| counted(record))
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
            let conversation =
                |record: &LearningReviewRecord| record.trigger == LearningTrigger::Conversation;
            return if conversation(existing) != conversation(&review) {
                Err(ApiError::conflict("This run was already reviewed"))
            } else if existing.evidence_digest == review.evidence_digest {
                Ok(existing.clone())
            } else {
                Err(ApiError::conflict("Learning source evidence changed"))
            };
        }
        let pruned = if review.trigger == LearningTrigger::Conversation {
            self.conversation_room(&review.agent_id)?
        } else {
            if self
                .reviews
                .values()
                .filter(|record| record.trigger != LearningTrigger::Conversation)
                .count()
                >= RETAINED_LIMIT
            {
                tracing::warn!(
                    agent_id = %review.agent_id,
                    cap = "task",
                    limit = RETAINED_LIMIT,
                    "learning review refused: storage cap reached"
                );
                return Err(ApiError::service_unavailable(
                    "Learning review storage limit reached",
                ));
            }
            Vec::new()
        };
        if review.trigger == LearningTrigger::Conversation {
            if self.conversations_in_window(&review.agent_id, review.reserved_at_ms)
                >= CONVERSATION_DAILY_LIMIT
            {
                return Err(ApiError::conflict(format!(
                    "Conversation reviews are used up for now ({CONVERSATION_DAILY_LIMIT} per 24 hours)"
                )));
            }
        } else if self.in_window(review.reserved_at_ms).len() >= DAILY_LIMIT {
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
        for id in pruned {
            self.reviews.remove(&id);
        }
        self.reviews.insert(review.id.clone(), review.clone());
        Ok(review)
    }
    /// The conversation reviews to prune so `agent_id` has room for one more:
    /// its oldest settled ones (finished, nothing waiting for the owner).
    /// Refuses when too few are settled.
    fn conversation_room(&self, agent_id: &str) -> Result<Vec<String>, ApiError> {
        let held: Vec<&LearningReviewRecord> = self
            .reviews
            .values()
            .filter(|r| r.trigger == LearningTrigger::Conversation && r.agent_id == agent_id)
            .collect();
        let excess = (held.len() + 1).saturating_sub(CONVERSATION_RETAINED_LIMIT);
        if excess == 0 {
            return Ok(Vec::new());
        }
        let mut settled: Vec<&LearningReviewRecord> = held
            .into_iter()
            .filter(|r| {
                !r.state.pending()
                    && !r
                        .proposals
                        .iter()
                        .any(|p| p.decision == LearningDecision::Pending)
            })
            .collect();
        if settled.len() < excess {
            tracing::warn!(
                agent_id,
                cap = "conversation",
                limit = CONVERSATION_RETAINED_LIMIT,
                "learning review refused: storage cap reached"
            );
            return Err(ApiError::service_unavailable(
                "Conversation review storage limit reached; settle older suggestions first",
            ));
        }
        settled.sort_by(|a, b| {
            a.reserved_at_ms
                .cmp(&b.reserved_at_ms)
                .then(a.id.cmp(&b.id))
        });
        Ok(settled
            .into_iter()
            .take(excess)
            .map(|r| r.id.clone())
            .collect())
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
