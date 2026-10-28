//! The gate in `ToolExecutionContext::execute_tool` (spec §7.3): judges each
//! call, and for one that needs the owner saves a request and waits for its
//! decision, the run's stop, or the timeout, whichever settles it first. The
//! waiting call holds no lock; every settlement takes the control-plane
//! transaction, so exactly one of them wins.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex as StdMutex, MutexGuard};
use std::time::Duration;

use anima_core::{
    tool_not_configured_error, AgentState, CancelSignal, Content, TaskResult, ToolCall,
};
use tokio::sync::oneshot;

use super::{
    risk_class, ApprovalRequest, ApprovalStatus, RiskClass, Verdict, APPROVAL_LOST,
    APPROVAL_TIMEOUT_MS, DENIED_BY_OWNER, DENIED_BY_POLICY, HELPER_NEEDS_APPROVAL, REFUSAL_BREAKER,
    TELEGRAM_APPROVAL_TIMEOUT_MS,
};
use crate::agent_runs::{is_helper_config, AgentRunCoordinator};
use crate::runs::{RunLink, RunSource};
use crate::state::{ApprovalAsk, Settlement};

/// How long a call waits for the owner (spec §7.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ApprovalTimeouts {
    pub(crate) default: Duration,
    /// For Telegram-started runs: the connector handles one message at a time.
    pub(crate) telegram: Duration,
}

impl Default for ApprovalTimeouts {
    fn default() -> Self {
        Self {
            default: Duration::from_millis(APPROVAL_TIMEOUT_MS),
            telegram: Duration::from_millis(TELEGRAM_APPROVAL_TIMEOUT_MS),
        }
    }
}

impl ApprovalTimeouts {
    pub(crate) fn for_source(self, source: RunSource) -> Duration {
        if matches!(source, RunSource::Telegram | RunSource::Discord) {
            self.telegram
        } else {
            self.default
        }
    }
}

type Senders = HashMap<String, oneshot::Sender<ApprovalRequest>>;

/// The calls waiting for the owner, by approval id. Whoever settles an
/// approval sends the settled record to its one waiter. A leaf lock, never
/// held across `.await` or while taking the state lock.
#[derive(Clone, Default)]
pub(crate) struct ApprovalWaiters {
    senders: Arc<StdMutex<Senders>>,
}

impl ApprovalWaiters {
    fn senders(&self) -> MutexGuard<'_, Senders> {
        self.senders
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The wake-up of approval `id`; registered before the request is saved.
    pub(crate) fn register(&self, id: &str) -> oneshot::Receiver<ApprovalRequest> {
        let (sender, receiver) = oneshot::channel();
        self.senders().insert(id.to_string(), sender);
        receiver
    }

    pub(crate) fn forget(&self, id: &str) {
        self.senders().remove(id);
    }

    /// Sends the settled record to its waiter, once; a later call finds none.
    pub(crate) fn wake(&self, approval: &ApprovalRequest) {
        let sender = self.senders().remove(&approval.id);
        if let Some(sender) = sender {
            let _ = sender.send(approval.clone());
        }
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.senders().len()
    }
}

/// A request, from the moment it exists in the control plane, and the
/// wake-up its settlement sends. Dropped before it has an outcome (its call
/// went away: an aborted run task), it settles the request as `stopped`.
pub(crate) struct PendingApproval {
    pub(crate) id: String,
    pub(crate) woken: oneshot::Receiver<ApprovalRequest>,
    abandoned: AbandonGuard,
}

impl PendingApproval {
    /// Armed as soon as the request is recorded and its waiter registered,
    /// before the request's save: a call dropped while that save is in
    /// flight cannot know whether it landed, so the request ends `stopped`
    /// either way, and that settlement saves it.
    pub(crate) fn armed(
        coordinator: AgentRunCoordinator,
        id: String,
        woken: oneshot::Receiver<ApprovalRequest>,
    ) -> Self {
        Self {
            abandoned: AbandonGuard {
                coordinator,
                id: Some(id.clone()),
            },
            id,
            woken,
        }
    }

    /// The request was taken back (its save failed): nothing is left to settle.
    pub(crate) fn withdraw(mut self) {
        self.abandoned.disarm();
    }
}

/// Settles an approval as `stopped` when its waiting call is dropped before
/// it had an outcome, so no request waits forever, and forgets its waiter at
/// once (nobody listens any more). Drop cannot await: the settlement runs in
/// a spawned task, under the control-plane transaction like every other.
struct AbandonGuard {
    coordinator: AgentRunCoordinator,
    id: Option<String>,
}

impl AbandonGuard {
    fn disarm(&mut self) {
        self.id = None;
    }
}

impl Drop for AbandonGuard {
    fn drop(&mut self) {
        let Some(id) = self.id.take() else {
            return;
        };
        // The waiters mutex is a leaf: taking it here holds nothing else.
        self.coordinator.forget_approval_waiter(&id);
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let coordinator = self.coordinator.clone();
        handle.spawn(async move {
            coordinator.settle_approval(&id, Settlement::Stopped).await;
        });
    }
}

/// A denial's tool result: `Denied by owner: <note>`, or `Denied by owner`.
pub(crate) fn denial_text(note: Option<&str>) -> String {
    match note {
        Some(note) => format!("{DENIED_BY_OWNER}: {note}"),
        None => DENIED_BY_OWNER.to_string(),
    }
}

/// Whether an approved call lost its permission while it waited: the agent
/// is gone, or its own configuration listed the tool when it asked and no
/// longer does. A tool its configuration never listed was added for the run
/// (the peer and team tools); the run's live checks judge that one.
fn lost_during_wait(configured_then: Option<bool>, configured_now: Option<bool>) -> bool {
    match configured_now {
        None => true,
        Some(now) => configured_then == Some(true) && !now,
    }
}

/// What `execute_tool` does with a call once the gate has judged it.
pub(crate) enum GateOutcome {
    /// Allowed without asking: dispatch.
    Proceed,
    /// The owner allowed it after a wait: check again, then dispatch.
    Approved,
    /// Answer the model with this instead of running the tool.
    Refuse(TaskResult<Content>),
}

/// One coordinator run's gate (spec §7.3).
#[derive(Clone)]
pub(crate) struct ApprovalGate {
    coordinator: AgentRunCoordinator,
    run: RunLink,
    source: RunSource,
    cancel: CancelSignal,
    /// Refusals in a row in this run; an action that runs resets it.
    refusals: RefusalStreak,
}

/// Refused actions in a row, shared by every clone of one run's gate.
#[derive(Clone, Default)]
struct RefusalStreak(Arc<AtomicU32>);

impl RefusalStreak {
    /// Moves the count for one decision and, from `REFUSAL_BREAKER` refusals
    /// in a row on, adds the breaker note to the refusal.
    fn apply(&self, outcome: GateOutcome, streak: Streak) -> GateOutcome {
        match streak {
            Streak::Resets => {
                self.0.store(0, Ordering::SeqCst);
                outcome
            }
            Streak::Neutral => outcome,
            Streak::Counts => {
                let count = self.0.fetch_add(1, Ordering::SeqCst) + 1;
                match outcome {
                    GateOutcome::Refuse(refused) if count >= REFUSAL_BREAKER => {
                        GateOutcome::Refuse(with_breaker_note(refused, count))
                    }
                    other => other,
                }
            }
        }
    }
}

/// How one gate decision moves the run's refusal streak.
enum Streak {
    /// Blocked, denied by policy, by the smart check, or by the owner.
    Counts,
    /// The action runs.
    Resets,
    /// Helper limits, cancellation, lost or expired approvals.
    Neutral,
}

impl ApprovalGate {
    pub(crate) fn bound_run(&self) -> &RunLink {
        &self.run
    }

    /// Recheck only a newly explicit Exec deny. An outer AllowOnce has already
    /// been consumed; Ask must not prompt again for each nested native call.
    pub(crate) async fn rpc_outer_denied(&self, agent: &AgentState) -> bool {
        let exec_probe = ToolCall {
            id: String::new(),
            name: "bash".into(),
            args: Default::default(),
        };
        self.coordinator
            .approval_verdict(agent, &self.run.session_id, &exec_probe)
            .await
            == Verdict::Deny
    }

    pub(crate) fn new(
        coordinator: AgentRunCoordinator,
        run: RunLink,
        source: RunSource,
        cancel: CancelSignal,
    ) -> Self {
        Self {
            coordinator,
            run,
            source,
            cancel,
            refusals: RefusalStreak::default(),
        }
    }

    /// Judges `call`, then appends the breaker note once the run has had
    /// `REFUSAL_BREAKER` refusals in a row.
    pub(crate) async fn check(&self, agent: &AgentState, call: &ToolCall) -> GateOutcome {
        let (outcome, streak) = self.check_inner(agent, call).await;
        self.refusals.apply(outcome, streak)
    }

    async fn check_inner(&self, agent: &AgentState, call: &ToolCall) -> (GateOutcome, Streak) {
        // Read-class tools never ask (spec §7.2), so they never wait for the
        // state lock either: a read tool runs while the lock is held (M3).
        if risk_class(&call.name) == RiskClass::Read {
            return (GateOutcome::Proceed, Streak::Resets);
        }
        use crate::approvals::autonomy::{command_of, fits_smart_check, Floor, SmartVerdict};
        let floor = self.coordinator.approval_floor(agent, call).await;
        if let Floor::Blocked(reason) = &floor {
            return (
                GateOutcome::Refuse(TaskResult::error(
                    format!(
                        "Blocked: this command {reason}, so it never runs, in any mode. Don't retry it or work around it; tell the owner what you wanted to do."
                    ),
                    0,
                )),
                Streak::Counts,
            );
        }
        match self
            .coordinator
            .approval_verdict(agent, &self.run.session_id, call)
            .await
        {
            Verdict::Allow => (GateOutcome::Proceed, Streak::Resets),
            Verdict::Deny => (
                GateOutcome::Refuse(TaskResult::error(DENIED_BY_POLICY, 0)),
                Streak::Counts,
            ),
            // Helpers never wait (spec §7.3).
            Verdict::Ask if is_helper_config(&agent.config) => (
                GateOutcome::Refuse(TaskResult::error(HELPER_NEEDS_APPROVAL, 0)),
                Streak::Neutral,
            ),
            Verdict::Ask => {
                let floor_only = matches!(floor, Floor::Dangerous(_))
                    && self
                        .coordinator
                        .approval_policy_verdict(agent, &self.run.session_id, call)
                        .await
                        == Verdict::Allow;
                if let (true, Floor::Dangerous(why), Some(command)) = (
                    floor_only,
                    &floor,
                    command_of(call).filter(|command| fits_smart_check(command)),
                ) {
                    match self
                        .coordinator
                        .smart_check(agent, &self.run.session_id, command, why)
                        .await
                    {
                        SmartVerdict::Approve => return (GateOutcome::Proceed, Streak::Resets),
                        SmartVerdict::Deny => {
                            return (
                                GateOutcome::Refuse(TaskResult::error(
                                    format!(
                                        "Denied by the owner's smart check: this command {why} and did not look like a safe step. Ask the owner if it is really needed."
                                    ),
                                    0,
                                )),
                                Streak::Counts,
                            )
                        }
                        SmartVerdict::Escalate => {}
                    }
                }
                self.ask_counted(agent, call).await
            }
        }
    }

    /// The additional receipt save is an authorization wait, not another
    /// approval prompt or a reusable grant for an exact approved call.
    pub(crate) async fn configured_baseline(
        &self,
        agent: &AgentState,
        call: &ToolCall,
    ) -> Option<bool> {
        self.coordinator
            .configured_tool(&agent.id, &call.name)
            .await
    }

    pub(crate) async fn revalidate_after_receipt(
        &self,
        agent: &AgentState,
        call: &ToolCall,
        configured: Option<bool>,
        approved: bool,
    ) -> Result<(), TaskResult<Content>> {
        if self.cancel.is_cancelled() {
            return Err(TaskResult::error(self.cancel.tool_result(), 0));
        }
        if self.coordinator.is_being_deleted(&agent.id)
            || lost_during_wait(
                configured,
                self.coordinator
                    .configured_tool(&agent.id, &call.name)
                    .await,
            )
        {
            return Err(TaskResult::error(tool_not_configured_error(&call.name), 0));
        }
        if risk_class(&call.name) != RiskClass::Read {
            match self
                .coordinator
                .approval_verdict(agent, &self.run.session_id, call)
                .await
            {
                Verdict::Allow => {}
                Verdict::Ask if approved => {}
                _ => return Err(TaskResult::error(DENIED_BY_POLICY, 0)),
            }
        }
        Ok(())
    }

    /// Protected instructions ignore reusable allows, while preserving policy deny.
    pub(crate) async fn check_exact(
        &self,
        agent: &AgentState,
        original: &ToolCall,
        redacted: &ToolCall,
    ) -> GateOutcome {
        self.check_exact_with_record(agent, original, redacted)
            .await
            .0
    }

    /// The settled native record is required for one exact document publication.
    pub(crate) async fn check_exact_with_record(
        &self,
        agent: &AgentState,
        original: &ToolCall,
        redacted: &ToolCall,
    ) -> (GateOutcome, Option<ApprovalRequest>) {
        let (outcome, streak, record) = self.check_exact_inner(agent, original, redacted).await;
        (self.refusals.apply(outcome, streak), record)
    }

    async fn check_exact_inner(
        &self,
        agent: &AgentState,
        original: &ToolCall,
        redacted: &ToolCall,
    ) -> (GateOutcome, Streak, Option<ApprovalRequest>) {
        if self
            .coordinator
            .approval_verdict(agent, &self.run.session_id, original)
            .await
            == Verdict::Deny
        {
            return (
                GateOutcome::Refuse(TaskResult::error(DENIED_BY_POLICY, 0)),
                Streak::Counts,
                None,
            );
        }
        if is_helper_config(&agent.config) {
            return (
                GateOutcome::Refuse(TaskResult::error(HELPER_NEEDS_APPROVAL, 0)),
                Streak::Neutral,
                None,
            );
        }
        let (outcome, streak, record) = self.ask_counted_record(agent, redacted).await;
        if matches!(outcome, GateOutcome::Approved)
            && self
                .coordinator
                .approval_verdict(agent, &self.run.session_id, original)
                .await
                == Verdict::Deny
        {
            return (
                GateOutcome::Refuse(TaskResult::error(DENIED_BY_POLICY, 0)),
                Streak::Counts,
                None,
            );
        }
        (outcome, streak, record)
    }

    /// Asks the owner; an owner Deny counts toward the refusal streak.
    async fn ask_counted(&self, agent: &AgentState, call: &ToolCall) -> (GateOutcome, Streak) {
        let (outcome, streak, _) = self.ask_counted_record(agent, call).await;
        (outcome, streak)
    }

    async fn ask_counted_record(
        &self,
        agent: &AgentState,
        call: &ToolCall,
    ) -> (GateOutcome, Streak, Option<ApprovalRequest>) {
        let timeout = self.coordinator.approval_timeout(self.source);
        // Taken before the request is made, so the wait ends when the
        // record's `expiresAtMs` says it does.
        let deadline = tokio::time::Instant::now() + timeout;
        let ask = ApprovalAsk {
            run: self.run.clone(),
            call: call.clone(),
            timeout_ms: u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX),
        };
        // What the agent's own configuration said when it asked, for the
        // check after an approval.
        let configured = self
            .coordinator
            .configured_tool(&agent.id, &call.name)
            .await;
        let pending = match self.coordinator.open_approval(ask).await {
            Ok(pending) => pending,
            Err(refused) => return (GateOutcome::Refuse(refused), Streak::Neutral, None),
        };
        let (outcome, streak, record) = match self.wait(pending, deadline).await {
            Some(approval) => (
                self.outcome(&approval),
                match approval.status {
                    ApprovalStatus::Denied => Streak::Counts,
                    ApprovalStatus::Allowed if !self.cancel.is_cancelled() => Streak::Resets,
                    _ => Streak::Neutral,
                },
                Some(approval),
            ),
            None => (
                GateOutcome::Refuse(TaskResult::error(APPROVAL_LOST, 0)),
                Streak::Neutral,
                None,
            ),
        };
        // The owner may have changed the agent during the wait: what runs is
        // what the agent allows now, not what it allowed when it asked.
        if matches!(outcome, GateOutcome::Approved)
            && lost_during_wait(
                configured,
                self.coordinator
                    .configured_tool(&agent.id, &call.name)
                    .await,
            )
        {
            return (
                GateOutcome::Refuse(TaskResult::error(tool_not_configured_error(&call.name), 0)),
                Streak::Neutral,
                None,
            );
        }
        let record = matches!(outcome, GateOutcome::Approved)
            .then_some(record)
            .flatten();
        (outcome, streak, record)
    }

    /// The approval as it was settled, by whoever settled it first. Holds no
    /// lock while it waits.
    async fn wait(
        &self,
        pending: PendingApproval,
        deadline: tokio::time::Instant,
    ) -> Option<ApprovalRequest> {
        let PendingApproval {
            id,
            mut woken,
            mut abandoned,
        } = pending;
        let profile_lost = self.coordinator.profile_loss_signal().await;
        let mut wake_open = true;
        let settled = loop {
            tokio::select! {
                biased;
                () = profile_lost.cancelled() => {
                    // The former writer cannot settle durable approval state.
                    // Retain that evidence for recovery and release only its waiter.
                    self.coordinator.forget_approval_waiter(&id);
                    abandoned.disarm();
                    return None;
                },
                settled = &mut woken, if wake_open => match settled {
                    Ok(approval) => break Some(approval),
                    // Dropped unsent: no wake-up is coming, but that is not a
                    // timeout. The stop or the deadline still settles it.
                    Err(_) => wake_open = false,
                },
                () = self.cancel.cancelled() => break None,
                () = tokio::time::sleep_until(deadline) => break None,
            }
        };
        if settled.is_some() {
            abandoned.disarm();
            return settled;
        }
        let settlement = if self.cancel.is_cancelled() {
            Settlement::Stopped
        } else {
            Settlement::TimedOut
        };
        let settled = match self.coordinator.settle_approval(&id, settlement).await {
            Some(approval) => Some(approval),
            // Settled first by someone whose record already left the control
            // plane; that settlement sent it here before it let go of the
            // transaction this settle waited for.
            None => woken.try_recv().ok(),
        };
        // Whoever won has woken (and so removed) this waiter, or nobody will.
        self.coordinator.forget_approval_waiter(&id);
        abandoned.disarm();
        settled
    }

    fn outcome(&self, approval: &ApprovalRequest) -> GateOutcome {
        match approval.status {
            ApprovalStatus::Allowed if !self.cancel.is_cancelled() => GateOutcome::Approved,
            ApprovalStatus::Denied => GateOutcome::Refuse(TaskResult::error(
                denial_text(
                    approval
                        .resolution
                        .as_ref()
                        .and_then(|resolution| resolution.note.as_deref()),
                ),
                0,
            )),
            // Allowed, but the run is being stopped: nothing new starts.
            ApprovalStatus::Allowed | ApprovalStatus::Stopped => {
                GateOutcome::Refuse(TaskResult::error(self.cancel.tool_result(), 0))
            }
            ApprovalStatus::Pending | ApprovalStatus::Expired => {
                GateOutcome::Refuse(TaskResult::error(APPROVAL_LOST, 0))
            }
        }
    }
}

/// The refusal's text with the breaker note: after `count` refusals in a row
/// the agent should stop and ask the owner.
fn with_breaker_note(refused: TaskResult<Content>, count: u32) -> TaskResult<Content> {
    let text = refused.error.clone().unwrap_or_default();
    TaskResult::error(
    format!(
        "{text}\n\nThat is {count} refused actions in a row. Stop trying other ways to do this; tell the owner what you were trying to do and ask how to proceed."
    ),
    refused.duration_ms,
)
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_refusal_streak_counts_resets_and_ignores() {
        let streak = RefusalStreak::default();
        let refuse = || GateOutcome::Refuse(TaskResult::error("no", 0));
        let note = |outcome: &GateOutcome| match outcome {
            GateOutcome::Refuse(result) => result
                .error
                .as_deref()
                .unwrap_or_default()
                .contains("refused actions in a row"),
            _ => false,
        };
        assert!(!note(&streak.apply(refuse(), Streak::Counts)));
        assert!(!note(&streak.apply(refuse(), Streak::Neutral)));
        assert!(!note(&streak.apply(refuse(), Streak::Counts)));
        assert!(note(&streak.apply(refuse(), Streak::Counts)));
        assert!(matches!(
            streak.apply(GateOutcome::Proceed, Streak::Resets),
            GateOutcome::Proceed
        ));
        assert!(!note(&streak.apply(refuse(), Streak::Counts)));
    }

    use std::collections::BTreeMap;

    use anima_core::ToolCall;

    use super::*;
    use crate::approvals::PendingApprovalStart;

    fn approval(id: &str) -> ApprovalRequest {
        let call = ToolCall {
            id: "call-1".into(),
            name: "memory_add".into(),
            args: BTreeMap::new(),
        };
        let mut approval = ApprovalRequest::pending(
            PendingApprovalStart {
                agent_id: "agent",
                session_id: "chat:a",
                run_id: "run-1",
                call: &call,
                timeout_ms: 1_000,
                reason: None,
            },
            1,
        );
        approval.id = id.into();
        approval
    }

    #[test]
    fn telegram_runs_wait_fifteen_minutes_and_every_other_source_thirty() {
        let timeouts = ApprovalTimeouts::default();
        assert_eq!(
            timeouts.for_source(RunSource::Telegram),
            Duration::from_secs(15 * 60)
        );
        assert_eq!(
            timeouts.for_source(RunSource::Web),
            Duration::from_secs(30 * 60)
        );
        assert_eq!(
            timeouts.for_source(RunSource::Schedule),
            Duration::from_secs(30 * 60)
        );
    }

    #[test]
    fn a_wake_reaches_its_waiter_once_and_an_unknown_id_is_a_no_op() {
        let waiters = ApprovalWaiters::default();
        let mut woken = waiters.register("apr_1");
        assert_eq!(waiters.len(), 1);

        waiters.wake(&approval("apr_other"));
        assert!(woken.try_recv().is_err(), "another id wakes nothing");
        assert_eq!(waiters.len(), 1);

        waiters.wake(&approval("apr_1"));
        assert_eq!(woken.try_recv().unwrap().id, "apr_1");
        assert_eq!(waiters.len(), 0);

        waiters.wake(&approval("apr_1"));
        assert_eq!(waiters.len(), 0, "a second wake finds no waiter");

        let mut forgotten = waiters.register("apr_2");
        waiters.forget("apr_2");
        waiters.wake(&approval("apr_2"));
        assert!(forgotten.try_recv().is_err());
    }

    #[test]
    fn an_approval_is_lost_with_the_agent_or_with_a_configured_tool_only() {
        assert!(lost_during_wait(Some(true), None), "the agent is gone");
        assert!(lost_during_wait(Some(false), None), "the agent is gone");
        assert!(
            lost_during_wait(Some(true), Some(false)),
            "the tool was removed"
        );
        assert!(!lost_during_wait(Some(true), Some(true)));
        assert!(
            !lost_during_wait(Some(false), Some(false)),
            "a tool added for the run"
        );
        assert!(!lost_during_wait(Some(false), Some(true)));
    }

    #[test]
    fn a_denial_reads_denied_by_owner_with_the_note_when_there_is_one() {
        assert_eq!(denial_text(None), "Denied by owner");
        assert_eq!(denial_text(Some("not now")), "Denied by owner: not now");
    }
}
