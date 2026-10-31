//! Host-owned shared execution accounting. Reservations never grant tool approval.
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex as StdMutex};

use anima_core::execution_limits::{
    ExecutionBindingHint, ExecutionContext, ExecutionLimits, ExecutionPermit, ExecutionScope,
    SystemBudgetClock,
};
use anima_core::CancelSignal;
use futures::FutureExt;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::app::SharedDaemonState;
use crate::runs::RunRecord;
use crate::state::{ControlPlanePersistRequest, DaemonState};

mod port;

pub(crate) const MAX_EXECUTION_SCOPES: usize = 8192;
pub(crate) const MAX_EXECUTION_BINDINGS: usize = 65536;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct HostExecutionBudgets {
    pub(crate) generation: u64,
    pub(crate) scopes: BTreeMap<String, ExecutionScope>,
    pub(crate) run_scopes: BTreeMap<String, String>,
}

#[derive(Clone, Debug)]
pub(crate) struct BudgetCandidate {
    pub(crate) expected_generation: u64,
    pub(crate) registry: HostExecutionBudgets,
    pub(crate) binding: ExecutionBindingHint,
}

impl HostExecutionBudgets {
    /// A failed write may leave the live registry ahead of acknowledgement.
    /// Reconciliation can add evidence or tighten limits, never erase charges.
    pub(crate) fn retains(&self, previous: &Self) -> Result<(), String> {
        self.validate()?;
        for (run, scope) in &previous.run_scopes {
            if self.run_scopes.get(run) != Some(scope) {
                return Err("Execution accounting removed an immutable binding".into());
            }
        }
        for (id, old) in &previous.scopes {
            let new = self
                .scopes
                .get(id)
                .ok_or("Execution accounting removed a retained scope")?;
            if new.root_run_id != old.root_run_id
                || new.created_at_ms != old.created_at_ms
                || new.deadline_at_ms > old.deadline_at_ms
                || new.version < old.version
                || new.limits.max_model_attempts > old.limits.max_model_attempts
                || new.limits.max_tool_calls > old.limits.max_tool_calls
                || new.limits.max_retries > old.limits.max_retries
                || new.limits.max_inflight_operations > old.limits.max_inflight_operations
            {
                return Err("Execution accounting broadened a retained scope".into());
            }
            macro_rules! retains_limit {
                ($field:ident) => {
                    if old
                        .limits
                        .budget
                        .$field
                        .is_some_and(|old| new.limits.budget.$field.is_none_or(|new| new > old))
                    {
                        return Err("Execution accounting broadened a retained budget".into());
                    }
                };
            }
            retains_limit!(max_wall_time_ms);
            retains_limit!(max_input_tokens);
            retains_limit!(max_output_tokens);
            retains_limit!(max_total_tokens);
            retains_limit!(max_turns);
            retains_limit!(max_capability_steps);
            retains_limit!(max_estimated_cost_micros);
            retains_limit!(max_concurrent_runs);
            retains_limit!(max_artifact_bytes);
            retains_limit!(max_download_bytes);
            retains_limit!(require_approval_at_percent);
            for (key, old_reservation) in &old.reservations {
                let current = new
                    .reservations
                    .get(key)
                    .ok_or("Execution accounting removed a reservation")?;
                if current.request != old_reservation.request
                    || !retains_usage(
                        current.observed_usage.as_ref(),
                        old_reservation.observed_usage.as_ref(),
                    )
                {
                    return Err("Execution accounting changed retained reservation evidence".into());
                }
                if let Some(settled) = &old_reservation.settlement {
                    let next = current
                        .settlement
                        .as_ref()
                        .ok_or("Execution accounting removed a settlement")?;
                    use anima_core::execution_limits::SettlementOutcome;
                    let compatible = match (&settled.outcome, &next.outcome) {
                        (SettlementOutcome::Known(old), SettlementOutcome::Known(new)) => {
                            old == new
                        }
                        (
                            SettlementOutcome::Artifact {
                                artifact_bytes: old,
                            },
                            SettlementOutcome::Artifact {
                                artifact_bytes: new,
                            },
                        ) => old == new,
                        (
                            SettlementOutcome::ProvenNeverSent,
                            SettlementOutcome::ProvenNeverSent,
                        ) => true,
                        (
                            SettlementOutcome::Unknown { .. },
                            SettlementOutcome::Unknown { .. }
                            | SettlementOutcome::Known(_)
                            | SettlementOutcome::Artifact { .. },
                        ) => true,
                        _ => false,
                    };
                    if !compatible
                        || (settled.physical_retired && !next.physical_retired)
                        || !retains_usage(
                            next.observed_usage.as_ref(),
                            settled.observed_usage.as_ref(),
                        )
                    {
                        return Err("Execution accounting regressed an accepted settlement".into());
                    }
                }
            }
        }
        Ok(())
    }
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.scopes.len() > MAX_EXECUTION_SCOPES
            || self.run_scopes.len() > MAX_EXECUTION_BINDINGS
        {
            return Err("Shared execution registry capacity exceeded".into());
        }
        for (id, scope) in &self.scopes {
            scope.validate()?;
            if id != &scope.id || self.run_scopes.get(&scope.root_run_id) != Some(id) {
                return Err("Shared execution root binding is missing or mismatched".into());
            }
            for reservation in scope.reservations.values() {
                if self.run_scopes.get(&reservation.request.run_id) != Some(id) {
                    return Err("Shared execution reservation has no matching run binding".into());
                }
            }
        }
        for (run, scope) in &self.run_scopes {
            validate_identity(run)?;
            if !self.scopes.contains_key(scope) {
                return Err("Shared execution run binding has no accounting scope".into());
            }
        }
        Ok(())
    }

    pub(crate) fn prepare_run(
        &self,
        record: &RunRecord,
        now_ms: u64,
    ) -> Result<BudgetCandidate, String> {
        let mut lineage = Vec::new();
        lineage.extend(record.parent_run_id.as_deref());
        lineage.extend(record.recovery_of.as_deref());
        if let Some(helper) = &record.async_helper {
            lineage.push(&helper.parent_run_id);
        }
        if let Some(completion) = &record.helper_completion {
            lineage.push(&completion.helper_run_id);
            lineage.push(&completion.launch_parent_run_id);
        }
        self.prepare_binding(&record.id, &lineage, now_ms)
    }

    pub(crate) fn prepare_operation(
        &self,
        operation_id: &str,
        now_ms: u64,
    ) -> Result<BudgetCandidate, String> {
        self.prepare_binding(operation_id, &[], now_ms)
    }

    fn prepare_binding(
        &self,
        run_id: &str,
        lineage: &[&str],
        now_ms: u64,
    ) -> Result<BudgetCandidate, String> {
        self.validate()?;
        validate_identity(run_id)?;
        let mut inherited: Option<&String> = None;
        for source in lineage {
            validate_identity(source)?;
            if *source == run_id {
                return Err("Shared execution lineage cannot refer to itself".into());
            }
            let scope = self
                .run_scopes
                .get(*source)
                .ok_or("Shared execution lineage has no durable budget binding")?;
            if inherited.is_some_and(|expected| expected != scope) {
                return Err("Shared execution lineage disagrees about its original scope".into());
            }
            inherited = Some(scope);
        }
        let mut registry = self.clone();
        let scope_id = if let Some(existing) = self.run_scopes.get(run_id) {
            if inherited.is_some_and(|expected| expected != existing)
                || (lineage.is_empty() && self.scopes[existing].root_run_id != run_id)
            {
                return Err("An existing execution binding cannot be changed".into());
            }
            existing.clone()
        } else {
            if self.run_scopes.len() >= MAX_EXECUTION_BINDINGS {
                return Err("Shared execution binding capacity reached; retained accounting cannot be evicted".into());
            }
            let id = if let Some(scope) = inherited {
                scope.clone()
            } else {
                if self.scopes.len() >= MAX_EXECUTION_SCOPES {
                    return Err("Shared execution scope capacity reached; retained accounting cannot be evicted".into());
                }
                let id = format!("execution_{}", uuid::Uuid::new_v4());
                registry.scopes.insert(
                    id.clone(),
                    ExecutionScope::new(
                        id.clone(),
                        run_id.to_string(),
                        ExecutionLimits::default(),
                        now_ms,
                    )?,
                );
                id
            };
            registry.run_scopes.insert(run_id.to_string(), id.clone());
            registry.generation = self.next_generation()?;
            id
        };
        let scope = registry
            .scopes
            .get(&scope_id)
            .ok_or("Shared execution scope is unavailable")?;
        scope.allowance(now_ms)?;
        let binding = ExecutionBindingHint {
            scope_id,
            run_id: run_id.to_string(),
            deadline_at_ms: scope.deadline_at_ms,
        };
        registry.validate()?;
        Ok(BudgetCandidate {
            expected_generation: self.generation,
            registry,
            binding,
        })
    }

    pub(super) fn next_generation(&self) -> Result<u64, String> {
        self.generation
            .checked_add(1)
            .ok_or("Shared execution registry generation exhausted".into())
    }
}

pub(crate) fn prepare_run_binding(
    state: &DaemonState,
    record: &RunRecord,
    now_ms: u64,
) -> Result<BudgetCandidate, String> {
    state
        .ensure_profile_owned()
        .map_err(|error| error.to_string())?;
    // Old snapshots may adopt only an original, verified never-started input.
    // An interrupted physical attempt has no evidence of its remaining budget.
    let mut registry = state.execution_budgets.clone();
    if let Some(source_id) = record
        .recovery_of
        .as_ref()
        .filter(|id| !registry.run_scopes.contains_key(*id))
    {
        let source = state
            .runs
            .get(source_id)
            .ok_or("Recovery execution source is unavailable")?;
        if has_started_evidence(source) {
            return Err(
                "Started legacy recovery has no durable execution accounting; admission refused"
                    .into(),
            );
        }
        registry = registry.prepare_run(source, now_ms)?.registry;
    }
    if !registry.run_scopes.contains_key(&record.id)
        && state.runs.get(&record.id).is_some_and(has_started_evidence)
    {
        return Err("Started execution has no durable budget binding".into());
    }
    let mut candidate = registry.prepare_run(record, now_ms)?;
    candidate.expected_generation = state.execution_budgets.generation;
    if candidate.registry != state.execution_budgets {
        candidate.registry.generation = state.execution_budgets.next_generation()?;
    }
    refresh_owner_limits(state, &mut candidate, Some(record))?;
    if let Some(failure) = state
        .execution_budget_failures
        .get(&candidate.binding.scope_id)
    {
        if let Some(reason) = failure
            .lock()
            .map_err(|_| "Shared execution failure state is unavailable")?
            .clone()
        {
            return Err(reason);
        }
    }
    Ok(candidate)
}

/// The caller owns the coordinator transaction until this returns. Only the
/// accounting candidate is unpublished; the existing run admission owns its
/// provisional record and rollback behavior.
pub(crate) async fn save_admission(
    state: &SharedDaemonState,
    candidate: BudgetCandidate,
    request: ControlPlanePersistRequest,
) -> std::io::Result<()> {
    candidate
        .registry
        .validate()
        .map_err(std::io::Error::other)?;
    {
        let guard = state.read().await;
        guard.ensure_profile_owned()?;
        if guard.execution_budgets.generation != candidate.expected_generation {
            return Err(std::io::Error::other(
                "Shared execution admission generation changed",
            ));
        }
    }
    let scope_id = candidate.binding.scope_id.clone();
    let save = request
        .with_execution_budgets(candidate.expected_generation, candidate.registry.clone())
        .save_execution_candidate();
    let saved = match std::panic::AssertUnwindSafe(save).catch_unwind().await {
        Ok(result) => result,
        Err(_) => Err(std::io::Error::other(
            "Execution persistence panicked; conservative accounting was retained",
        )),
    };
    let mut guard = state.write().await;
    if guard.execution_budgets.generation != candidate.expected_generation {
        let error = "Shared execution publication generation changed";
        latch_failure(&mut guard, &scope_id, error);
        return Err(std::io::Error::other(error));
    }
    // An uncertain write may already be canonical. Keeping the conservative
    // candidate prevents a later ordinary snapshot from rolling charges back.
    if guard.execution_budget_candidate_snapshot.as_ref() == Some(&candidate.registry) {
        guard.execution_budget_candidate_snapshot = None;
    }
    guard.execution_budgets = candidate.registry;
    if let Err(error) = &saved {
        latch_failure(
            &mut guard,
            &scope_id,
            &format!("Shared execution accounting could not be durably saved: {error}"),
        );
    }
    saved
}

pub(crate) async fn context_for_run(
    state: &SharedDaemonState,
    transactions: Arc<Mutex<()>>,
    run_id: &str,
    cancel: CancelSignal,
) -> Result<ExecutionContext, String> {
    let owner = {
        let guard = state.read().await;
        guard
            .ensure_profile_owned()
            .map_err(|error| error.to_string())?;
        guard.profile_owner()
    };
    if let Some(owner) = owner {
        owner
            .verify_owned()
            .await
            .map_err(|error| error.to_string())?;
    }
    let mut guard = state.write().await;
    let scope_id = guard
        .execution_budgets
        .run_scopes
        .get(run_id)
        .cloned()
        .ok_or("Execution run has no durable budget binding")?;
    let deadline_at_ms = guard
        .execution_budgets
        .scopes
        .get(&scope_id)
        .ok_or("Execution scope is unavailable")?
        .deadline_at_ms;
    let failures = guard
        .execution_budget_failures
        .entry(scope_id.clone())
        .or_insert_with(|| Arc::new(StdMutex::new(None)))
        .clone();
    let port = Arc::new(port::HostBudgetPort::new(
        state,
        transactions,
        scope_id.clone(),
        run_id.to_string(),
        cancel.clone(),
        failures.clone(),
    ));
    Ok(ExecutionContext {
        scope_id,
        run_id: run_id.to_string(),
        port,
        clock: Arc::new(SystemBudgetClock),
        cancel,
        deadline_at_ms,
        failures,
        secondary_cancel: None,
    })
}

pub(crate) async fn standalone_context(
    state: &SharedDaemonState,
    transactions: Arc<Mutex<()>>,
    operation_id: String,
    cancel: CancelSignal,
) -> Result<ExecutionContext, String> {
    standalone_context_with_limits(state, transactions, operation_id, cancel, None).await
}

pub(crate) async fn standalone_context_with_limits(
    state: &SharedDaemonState,
    transactions: Arc<Mutex<()>>,
    operation_id: String,
    cancel: CancelSignal,
    limits: Option<ExecutionLimits>,
) -> Result<ExecutionContext, String> {
    let state = Arc::clone(state);
    // Admission, like reservation, outlives the caller waiting for its receipt.
    tokio::spawn(async move {
        let transaction = transactions.clone().lock_owned().await;
        if cancel.is_cancelled() {
            return Err(cancel.run_error().into());
        }
        let (candidate, request) = {
            let mut guard = state.write().await;
            guard
                .ensure_profile_owned()
                .map_err(|error| error.to_string())?;
            let mut candidate = guard
                .execution_budgets
                .prepare_operation(&operation_id, anima_core::primitives::now_millis())?;
            if let Some(limits) = &limits {
                tighten_candidate(&mut candidate, limits)?;
            }
            (candidate, guard.control_plane_persist_request())
        };
        save_admission(&state, candidate, request)
            .await
            .map_err(|error| error.to_string())?;
        drop(transaction);
        context_for_run(&state, transactions, &operation_id, cancel).await
    })
    .await
    .map_err(|error| format!("Execution admission worker failed: {error}"))?
}

pub(crate) struct RetainedChildAdmission {
    pub(crate) context: ExecutionContext,
    pub(crate) dispatch: ExecutionPermit,
}

/// Binds one explicit child identity to its parent's retained accounting. The
/// caller must revalidate `dispatch` immediately before dispatch and settle it
/// with the actual outcome and physical retirement. A binding is never replayed.
pub(crate) async fn retained_child_context(
    state: &SharedDaemonState,
    transactions: Arc<Mutex<()>>,
    parent: &ExecutionContext,
    child_id: String,
    cancel: CancelSignal,
) -> Result<RetainedChildAdmission, String> {
    let state = Arc::clone(state);
    let parent = parent.clone();
    // The worker retains admission and its storage acknowledgement even when
    // the waiting caller drops. An undelivered permit keeps its conservative
    // unknown settlement through the existing permit Drop path.
    tokio::spawn(async move {
        parent.check_live()?;
        if cancel.is_cancelled() {
            return Err(cancel.run_error().into());
        }
        // ExecutionContext has only two cancellation slots. Refuse a third
        // ancestor rather than silently discarding an existing stop signal.
        if parent.secondary_cancel.is_some() {
            return Err("Retained child cancellation ancestry exceeds the supported bound".into());
        }
        parent.check_authority().await?;
        let transaction = transactions.clone().lock_owned().await;
        parent.check_live()?;
        if cancel.is_cancelled() {
            return Err(cancel.run_error().into());
        }
        let owner = {
            let guard = state.read().await;
            guard
                .ensure_profile_owned()
                .map_err(|error| error.to_string())?;
            guard.profile_owner()
        };
        if let Some(owner) = owner {
            owner
                .verify_owned()
                .await
                .map_err(|error| error.to_string())?;
        }
        let (candidate, request) = {
            let mut guard = state.write().await;
            guard
                .ensure_profile_owned()
                .map_err(|error| error.to_string())?;
            parent.check_live()?;
            if cancel.is_cancelled() {
                return Err(cancel.run_error().into());
            }
            let registry = &guard.execution_budgets;
            if registry.run_scopes.get(&parent.run_id) != Some(&parent.scope_id) {
                return Err("Retained child parent has no matching durable scope".into());
            }
            let scope = registry
                .scopes
                .get(&parent.scope_id)
                .ok_or("Retained child parent scope is unavailable")?;
            if scope.deadline_at_ms != parent.deadline_at_ms {
                return Err(
                    "Retained child parent deadline changed; explicit reattachment required".into(),
                );
            }
            if registry.run_scopes.contains_key(&child_id) {
                return Err("Retained child identity already admitted; replay refused".into());
            }
            if let Some(failure) = guard.execution_budget_failures.get(&parent.scope_id) {
                if let Some(reason) = failure
                    .lock()
                    .map_err(|_| "Shared execution failure state is unavailable")?
                    .clone()
                {
                    return Err(reason);
                }
            }
            // Budget-only operations need no synthetic run. When the identity
            // names a real run, its owner/session must match the actual parent.
            let parent_run = guard.runs.get(&parent.run_id);
            if let Some(child) = guard.runs.get(&child_id) {
                let parent_run = parent_run.ok_or("Retained child run parent is unavailable")?;
                if child.agent_id != parent_run.agent_id
                    || child.session_id != parent_run.session_id
                {
                    return Err(
                        "Retained child run owner or session disagrees with its parent".into(),
                    );
                }
            }
            let now_ms = anima_core::primitives::now_millis();
            let mut candidate = registry.prepare_binding(&child_id, &[&parent.run_id], now_ms)?;
            refresh_owner_limits(&guard, &mut candidate, parent_run)?;
            candidate.registry.scopes[&parent.scope_id].allowance(now_ms)?;
            (candidate, guard.control_plane_persist_request())
        };
        save_admission(&state, candidate, request)
            .await
            .map_err(|error| error.to_string())?;
        drop(transaction);
        let mut context = context_for_run(&state, transactions, &child_id, cancel).await?;
        context.secondary_cancel = Some(parent.cancel.clone());
        // Reserve through the canonical port only after releasing its shared
        // transaction. This debit covers orchestration; child work also draws
        // on exactly the same remaining model/tool/token allowance.
        let dispatch = context.reserve_tool("retained-child-dispatch").await?;
        Ok(RetainedChildAdmission { context, dispatch })
    })
    .await
    .map_err(|error| format!("Retained child admission worker failed: {error}"))?
}

pub(crate) fn tighten_candidate(
    candidate: &mut BudgetCandidate,
    limits: &ExecutionLimits,
) -> Result<(), String> {
    limits.validate()?;
    let scope = candidate
        .registry
        .scopes
        .get_mut(&candidate.binding.scope_id)
        .ok_or("Execution scope is unavailable")?;
    let previous = scope.clone();
    scope.limits = intersect_limits(&scope.limits, limits)?;
    scope.deadline_at_ms = scope.deadline_at_ms.min(
        scope
            .created_at_ms
            .checked_add(scope.limits.budget.max_wall_time_ms.unwrap())
            .ok_or("Execution deadline overflow")?,
    );
    if *scope != previous {
        scope.version = scope
            .version
            .checked_add(1)
            .ok_or("Execution scope version exhausted")?;
        candidate.registry.generation = candidate
            .expected_generation
            .checked_add(1)
            .ok_or("Execution registry generation exhausted")?;
        candidate.binding.deadline_at_ms = scope.deadline_at_ms;
    }
    candidate.registry.validate()?;
    Ok(())
}

pub(crate) fn intersect_limits(
    current: &ExecutionLimits,
    limits: &ExecutionLimits,
) -> Result<ExecutionLimits, String> {
    current.validate()?;
    limits.validate()?;
    let mut result = current.clone();
    result.max_model_attempts = current.max_model_attempts.min(limits.max_model_attempts);
    result.max_tool_calls = current.max_tool_calls.min(limits.max_tool_calls);
    result.max_retries = current
        .max_retries
        .min(limits.max_retries)
        .min(result.max_model_attempts);
    result.max_inflight_operations = current
        .max_inflight_operations
        .min(limits.max_inflight_operations);
    macro_rules! tighten {
        ($field:ident) => {
            result.budget.$field = Some(
                current
                    .budget
                    .$field
                    .unwrap()
                    .min(limits.budget.$field.unwrap()),
            );
        };
    }
    tighten!(max_wall_time_ms);
    tighten!(max_input_tokens);
    tighten!(max_output_tokens);
    tighten!(max_total_tokens);
    macro_rules! tighten_optional {
        ($field:ident) => {
            result.budget.$field = current
                .budget
                .$field
                .into_iter()
                .chain(limits.budget.$field)
                .min();
        };
    }
    tighten_optional!(max_turns);
    tighten_optional!(max_capability_steps);
    tighten_optional!(max_estimated_cost_micros);
    tighten_optional!(max_concurrent_runs);
    tighten_optional!(max_artifact_bytes);
    tighten_optional!(max_download_bytes);
    tighten_optional!(require_approval_at_percent);
    result.validate()?;
    Ok(result)
}

fn refresh_owner_limits(
    state: &DaemonState,
    candidate: &mut BudgetCandidate,
    admitted: Option<&RunRecord>,
) -> Result<(), String> {
    let scope = candidate
        .registry
        .scopes
        .get(&candidate.binding.scope_id)
        .ok_or("Execution scope is unavailable")?;
    let mut owners = std::collections::BTreeSet::new();
    if let Some(root) = state.runs.get(&scope.root_run_id) {
        owners.insert(root.agent_id.clone());
    }
    if let Some(run) = admitted.or_else(|| state.runs.get(&candidate.binding.run_id)) {
        owners.insert(run.agent_id.clone());
    }
    for owner in owners {
        let config = state
            .agents
            .get(&owner)
            .ok_or("Execution scope owner is unavailable")?
            .config();
        if let Some(limits) = config
            .settings
            .as_ref()
            .and_then(|settings| settings.execution_limits.as_ref())
        {
            tighten_candidate(candidate, limits)?;
        }
    }
    Ok(())
}

fn validate_identity(id: &str) -> Result<(), String> {
    if id.trim().is_empty() || id.len() > 256 || id.chars().any(char::is_control) {
        Err("Invalid execution identity".into())
    } else {
        Ok(())
    }
}

fn has_started_evidence(run: &RunRecord) -> bool {
    run.started_at_ms.is_some() || !run.tools_started.is_empty() || !run.action_receipts.is_empty()
        || !run.steps.is_empty() || run.usage != anima_core::TokenUsage::default()
        || run.reply_message_id.is_some() || run.reply_sha256.is_some()
        // Admission's bind() creates checkpoint=None. Host checkpoint saves
        // require a Running record, so even BeforeModel proves runtime entry.
        || run.turn.as_ref().is_some_and(|turn| turn.checkpoint.is_some())
}

fn retains_usage(
    new: Option<&anima_core::TokenUsage>,
    old: Option<&anima_core::TokenUsage>,
) -> bool {
    old.is_none_or(|old| {
        new.is_some_and(|new| {
            new.prompt_tokens >= old.prompt_tokens
                && new.completion_tokens >= old.completion_tokens
                && new.total_tokens >= old.total_tokens
                && new.cached_prompt_tokens >= old.cached_prompt_tokens
                && new.reasoning_tokens >= old.reasoning_tokens
        })
    })
}

fn latch_failure(state: &mut DaemonState, scope_id: &str, reason: &str) {
    let failures = state
        .execution_budget_failures
        .entry(scope_id.to_string())
        .or_insert_with(|| Arc::new(StdMutex::new(None)));
    let mut failure = failures
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if failure.is_none() {
        *failure = Some(reason.to_string());
    }
}

#[cfg(test)]
mod tests;
