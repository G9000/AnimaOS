//! Shared admission accounting. A saved reservation is not a tool approval.
mod context;
#[cfg(test)]
#[path = "execution_limits/native_artifact_tests.rs"]
mod native_artifact_tests;
pub use context::{
    ExecutionAllowance, ExecutionBudgetAuthority, ExecutionBudgetPort, ExecutionContext,
    ExecutionPermit, ExecutionStreamSink, InMemoryExecutionBudget, ModelCallContext,
    ModelDispatchState, SystemBudgetClock,
};
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{Budget, BudgetDecision, TokenUsage, Usage};

pub const MAX_EXECUTION_RESERVATIONS: usize = 8192;

/// A snapshot hint detects missing or mismatched authority after restore. It
/// grants nothing: the host must reattach the matching live/durable context.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutionBindingHint {
    pub scope_id: String,
    pub run_id: String,
    pub deadline_at_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutionLimits {
    pub budget: Budget,
    pub max_model_attempts: u64,
    pub max_tool_calls: u64,
    pub max_retries: u64,
    pub max_inflight_operations: u64,
}

impl Default for ExecutionLimits {
    fn default() -> Self {
        Self {
            budget: Budget {
                max_wall_time_ms: Some(30 * 60 * 1000),
                max_input_tokens: Some(4_000_000),
                max_output_tokens: Some(262_144),
                max_total_tokens: Some(4_262_144),
                ..Budget::default()
            },
            max_model_attempts: 96,
            max_tool_calls: 192,
            max_retries: 24,
            max_inflight_operations: 16,
        }
    }
}

impl ExecutionLimits {
    pub fn validate(&self) -> Result<(), String> {
        self.budget.validate().map_err(|e| e.to_string())?;
        if self.max_model_attempts == 0
            || self
                .max_model_attempts
                .checked_add(self.max_tool_calls)
                .is_none_or(|n| n > MAX_EXECUTION_RESERVATIONS as u64)
            || self.max_retries > self.max_model_attempts
            || self.max_inflight_operations == 0
            || self.max_inflight_operations > 256
            || [
                self.budget.max_wall_time_ms,
                self.budget.max_input_tokens,
                self.budget.max_output_tokens,
                self.budget.max_total_tokens,
            ]
            .iter()
            .any(Option::is_none)
        {
            return Err("Execution limits must be finite, positive and bounded".into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ExecutionOperation {
    ModelAttempt,
    Tool,
    /// One bounded native capability dispatch on this same admitted scope.
    NativeArtifact {
        max_artifact_bytes: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReservationRequest {
    pub id: String,
    pub run_id: String,
    pub logical_call_id: String,
    pub operation: ExecutionOperation,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

impl ReservationRequest {
    fn validate(&self) -> Result<(), String> {
        for id in [&self.id, &self.run_id, &self.logical_call_id] {
            validate_id(id)?;
        }
        if (!matches!(self.operation, ExecutionOperation::ModelAttempt)
            && (self.input_tokens != 0 || self.output_tokens != 0))
            || (self.operation == ExecutionOperation::ModelAttempt
                && (self.input_tokens == 0 || self.output_tokens == 0))
        {
            return Err("Invalid execution token reservation".into());
        }
        if matches!(
            self.operation,
            ExecutionOperation::NativeArtifact {
                max_artifact_bytes: 0
            }
        ) {
            return Err("Native artifact reservation requires a positive finite byte bound".into());
        }
        self.input_tokens
            .checked_add(self.output_tokens)
            .ok_or("Execution token reservation overflow")?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum SettlementOutcome {
    Known(TokenUsage),
    /// Actual native artifact bytes, including known overruns. Unretired
    /// observations retain at least the reservation ceiling.
    Artifact {
        artifact_bytes: u64,
    },
    Unknown {
        observed: Option<TokenUsage>,
    },
    ProvenNeverSent,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReservationSettlement {
    pub reservation_id: String,
    pub outcome: SettlementOutcome,
    /// Only a retained executor may assert that the physical operation ended.
    #[serde(default)]
    pub physical_retired: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_usage: Option<TokenUsage>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutionReservation {
    pub request: ReservationRequest,
    pub settlement: Option<ReservationSettlement>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_usage: Option<TokenUsage>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReservationAdmission {
    NewlyGranted,
    /// Historical evidence only. This result never authorizes another dispatch.
    AlreadyRecorded,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ExecutionConsumption {
    pub known: Usage,
    pub charged_input_tokens: u64,
    pub charged_output_tokens: u64,
    pub charged_total_tokens: u64,
    pub charged_artifact_bytes: u64,
    pub model_attempts: u64,
    pub logical_model_calls: u64,
    pub tool_calls: u64,
    pub retries: u64,
    pub inflight_operations: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutionScope {
    pub id: String,
    pub root_run_id: String,
    pub limits: ExecutionLimits,
    pub created_at_ms: u64,
    pub deadline_at_ms: u64,
    pub reservations: BTreeMap<String, ExecutionReservation>,
    pub version: u64,
}

impl ExecutionScope {
    pub fn new(
        id: String,
        root_run_id: String,
        limits: ExecutionLimits,
        now_ms: u64,
    ) -> Result<Self, String> {
        limits.validate()?;
        let deadline_at_ms = now_ms
            .checked_add(
                limits
                    .budget
                    .max_wall_time_ms
                    .ok_or("Missing execution deadline")?,
            )
            .ok_or("Execution deadline overflow")?;
        let value = Self {
            id,
            root_run_id,
            deadline_at_ms,
            limits,
            created_at_ms: now_ms,
            reservations: BTreeMap::new(),
            version: 0,
        };
        value.validate()?;
        Ok(value)
    }

    /// Structural validation deliberately accepts exhausted and overrun evidence.
    pub fn validate(&self) -> Result<(), String> {
        validate_id(&self.id)?;
        validate_id(&self.root_run_id)?;
        self.limits.validate()?;
        let latest = self
            .created_at_ms
            .checked_add(self.limits.budget.max_wall_time_ms.unwrap())
            .ok_or("Execution deadline overflow")?;
        if self.deadline_at_ms < self.created_at_ms
            || self.deadline_at_ms > latest
            || self.reservations.len() > MAX_EXECUTION_RESERVATIONS
        {
            return Err("Invalid execution scope".into());
        }
        for (id, reservation) in &self.reservations {
            reservation.request.validate()?;
            if id != &reservation.request.id {
                return Err("Execution reservation identity mismatch".into());
            }
            if let Some(settlement) = &reservation.settlement {
                if &settlement.reservation_id != id {
                    return Err("Execution settlement identity mismatch".into());
                }
                if reservation.request.operation == ExecutionOperation::Tool
                    && matches!(&settlement.outcome, SettlementOutcome::Known(usage) if usage != &TokenUsage::default())
                {
                    return Err("A tool reservation cannot settle model usage".into());
                }
                if !matches!(
                    reservation.request.operation,
                    ExecutionOperation::ModelAttempt
                ) && settlement
                    .observed_usage
                    .as_ref()
                    .is_some_and(|usage| usage != &TokenUsage::default())
                {
                    return Err("A native or tool settlement cannot observe model usage".into());
                }
                match (&reservation.request.operation, &settlement.outcome) {
                    (ExecutionOperation::NativeArtifact { .. }, SettlementOutcome::Known(_)) => {
                        return Err("Native artifact reservations cannot settle model usage".into())
                    }
                    (
                        ExecutionOperation::NativeArtifact { .. },
                        SettlementOutcome::Unknown { observed },
                    ) if observed
                        .as_ref()
                        .is_some_and(|usage| usage != &TokenUsage::default()) =>
                    {
                        return Err("Native artifact reservations cannot observe model usage".into())
                    }
                    (
                        ExecutionOperation::ModelAttempt | ExecutionOperation::Tool,
                        SettlementOutcome::Artifact { .. },
                    ) => return Err("Artifact usage requires a native artifact reservation".into()),
                    _ => {}
                }
            }
            if !matches!(
                reservation.request.operation,
                ExecutionOperation::ModelAttempt
            ) && reservation
                .observed_usage
                .as_ref()
                .is_some_and(|usage| usage != &TokenUsage::default())
            {
                return Err("A native or tool reservation cannot observe model usage".into());
            }
        }
        Ok(())
    }

    pub fn reserve(
        &mut self,
        request: ReservationRequest,
        now_ms: u64,
    ) -> Result<ReservationAdmission, String> {
        self.validate()?;
        request.validate()?;
        if let Some(existing) = self.reservations.get(&request.id) {
            return if existing.request == request {
                Ok(ReservationAdmission::AlreadyRecorded)
            } else {
                Err("Execution reservation identity was already used".into())
            };
        }
        if now_ms < self.created_at_ms || now_ms >= self.deadline_at_ms {
            return Err("Shared execution deadline reached".into());
        }
        let mut next = self.clone();
        next.version = next
            .version
            .checked_add(1)
            .ok_or("Execution scope version exhausted")?;
        next.reservations.insert(
            request.id.clone(),
            ExecutionReservation {
                request,
                settlement: None,
                observed_usage: None,
            },
        );
        next.check_allowance(now_ms)?;
        *self = next;
        Ok(ReservationAdmission::NewlyGranted)
    }

    /// Accounting remains admissible after cancellation, expiry and overrun.
    pub fn settle(&mut self, settlement: ReservationSettlement) -> Result<(), String> {
        self.validate()?;
        let existing = self
            .reservations
            .get(&settlement.reservation_id)
            .ok_or("Execution reservation is unavailable")?;
        let mut settlement = settlement;
        let observed = match &settlement.outcome {
            SettlementOutcome::Known(usage) => Some(usage),
            SettlementOutcome::Unknown { observed } => observed.as_ref(),
            SettlementOutcome::ProvenNeverSent | SettlementOutcome::Artifact { .. } => None,
        };
        let observed = merge_observed(settlement.observed_usage.as_ref(), observed);
        let observed_usage = merge_observed(existing.observed_usage.as_ref(), observed.as_ref());
        settlement.observed_usage = observed_usage.clone();
        if let Some(previous) = &existing.settlement {
            settlement.physical_retired |= previous.physical_retired;
            match (&previous.outcome, &mut settlement.outcome) {
                (
                    SettlementOutcome::Unknown { observed: old },
                    SettlementOutcome::Unknown { observed: new },
                ) => {
                    *new = merge_observed(old.as_ref(), new.as_ref());
                }
                (
                    SettlementOutcome::Unknown { .. },
                    SettlementOutcome::Known(_) | SettlementOutcome::Artifact { .. },
                ) => {}
                (SettlementOutcome::Known(old), SettlementOutcome::Known(new)) if old == new => {}
                (
                    SettlementOutcome::Artifact {
                        artifact_bytes: old,
                    },
                    SettlementOutcome::Artifact {
                        artifact_bytes: new,
                    },
                ) if old == new => {}
                (
                    SettlementOutcome::Known(_) | SettlementOutcome::Artifact { .. },
                    SettlementOutcome::Unknown { .. },
                ) => {
                    settlement.outcome = previous.outcome.clone();
                }
                (SettlementOutcome::ProvenNeverSent, SettlementOutcome::ProvenNeverSent) => {}
                (
                    SettlementOutcome::ProvenNeverSent,
                    SettlementOutcome::Unknown { observed: None },
                ) => {
                    settlement.outcome = previous.outcome.clone();
                }
                _ => return Err("Conflicting execution settlement".into()),
            }
            if previous == &settlement && existing.observed_usage == observed_usage {
                return Ok(());
            }
        }
        let mut next = self.clone();
        next.version = next
            .version
            .checked_add(1)
            .ok_or("Execution scope version exhausted")?;
        let reservation = next
            .reservations
            .get_mut(&settlement.reservation_id)
            .unwrap();
        reservation.settlement = Some(settlement);
        reservation.observed_usage = observed_usage;
        next.validate()?;
        *self = next;
        Ok(())
    }

    pub fn consumption(&self) -> Result<ExecutionConsumption, String> {
        self.validate()?;
        let mut usage = ExecutionConsumption::default();
        let mut logical = std::collections::BTreeSet::new();
        for reservation in self.reservations.values() {
            if reservation
                .settlement
                .as_ref()
                .is_none_or(|s| !s.physical_retired)
            {
                usage.inflight_operations += 1;
            }
            let request = &reservation.request;
            match request.operation {
                ExecutionOperation::ModelAttempt => {
                    usage.model_attempts += 1;
                    logical.insert(&request.logical_call_id);
                }
                ExecutionOperation::Tool | ExecutionOperation::NativeArtifact { .. } => {
                    usage.tool_calls += 1;
                }
            }
            if let ExecutionOperation::NativeArtifact { max_artifact_bytes } = request.operation {
                let bytes = match reservation.settlement.as_ref() {
                    Some(ReservationSettlement {
                        outcome: SettlementOutcome::Artifact { artifact_bytes },
                        physical_retired,
                        ..
                    }) => {
                        if *physical_retired {
                            *artifact_bytes
                        } else {
                            (*artifact_bytes).max(max_artifact_bytes)
                        }
                    }
                    Some(ReservationSettlement {
                        outcome: SettlementOutcome::ProvenNeverSent,
                        physical_retired: true,
                        ..
                    }) => 0,
                    _ => max_artifact_bytes,
                };
                add(&mut usage.charged_artifact_bytes, bytes)?;
            }
            let reserved_total = request
                .input_tokens
                .checked_add(request.output_tokens)
                .ok_or("Execution usage overflow")?;
            let (mut input, mut output, mut total) =
                match reservation.settlement.as_ref().map(|s| &s.outcome) {
                    Some(
                        SettlementOutcome::ProvenNeverSent | SettlementOutcome::Artifact { .. },
                    ) => (0, 0, 0),
                    Some(SettlementOutcome::Known(known)) => {
                        let (input, output, total) = normalized_tokens(known)?;
                        add(&mut usage.known.input_tokens, input)?;
                        add(&mut usage.known.output_tokens, output)?;
                        add(&mut usage.known.total_tokens, total)?;
                        (input, output, total)
                    }
                    Some(SettlementOutcome::Unknown {
                        observed: Some(observed),
                    }) => {
                        let (input, output, total) = normalized_tokens(observed)?;
                        let input = input.max(request.input_tokens);
                        let output = output.max(request.output_tokens);
                        (
                            input,
                            output,
                            total.max(reserved_total).max(
                                input
                                    .checked_add(output)
                                    .ok_or("Execution usage overflow")?,
                            ),
                        )
                    }
                    _ => (request.input_tokens, request.output_tokens, reserved_total),
                };
            if let Some(observed) = &reservation.observed_usage {
                let (seen_input, seen_output, seen_total) = normalized_tokens(observed)?;
                input = input.max(seen_input);
                output = output.max(seen_output);
                total = total.max(seen_total).max(
                    input
                        .checked_add(output)
                        .ok_or("Execution usage overflow")?,
                );
            }
            add(&mut usage.charged_input_tokens, input)?;
            add(&mut usage.charged_output_tokens, output)?;
            add(&mut usage.charged_total_tokens, total)?;
        }
        usage.logical_model_calls = logical.len() as u64;
        usage.retries = usage.model_attempts - usage.logical_model_calls;
        Ok(usage)
    }

    pub fn allowance(&self, now_ms: u64) -> Result<ExecutionAllowance, String> {
        if now_ms < self.created_at_ms || now_ms >= self.deadline_at_ms {
            return Err("Shared execution deadline reached".into());
        }
        self.check_allowance(now_ms)?;
        let usage = self.consumption()?;
        Ok(ExecutionAllowance {
            deadline_at_ms: self.deadline_at_ms,
            remaining_input_tokens: self
                .limits
                .budget
                .max_input_tokens
                .unwrap()
                .saturating_sub(usage.charged_input_tokens),
            remaining_output_tokens: self
                .limits
                .budget
                .max_output_tokens
                .unwrap()
                .saturating_sub(usage.charged_output_tokens),
            remaining_total_tokens: self
                .limits
                .budget
                .max_total_tokens
                .unwrap()
                .saturating_sub(usage.charged_total_tokens),
        })
    }

    fn check_allowance(&self, now_ms: u64) -> Result<(), String> {
        let usage = self.consumption()?;
        if usage.model_attempts > self.limits.max_model_attempts
            || usage.tool_calls > self.limits.max_tool_calls
            || usage.retries > self.limits.max_retries
            || usage.inflight_operations > self.limits.max_inflight_operations
        {
            return Err("Shared execution operation allowance exhausted".into());
        }
        let charged = Usage {
            wall_time_ms: now_ms.saturating_sub(self.created_at_ms),
            turns: usage.logical_model_calls,
            capability_steps: usage.tool_calls,
            input_tokens: usage.charged_input_tokens,
            output_tokens: usage.charged_output_tokens,
            total_tokens: usage.charged_total_tokens,
            concurrent_runs: usage.inflight_operations,
            artifact_bytes: usage.charged_artifact_bytes,
            ..Usage::default()
        };
        if self
            .limits
            .budget
            .evaluate(&charged)
            .map_err(|e| e.to_string())?
            != BudgetDecision::Continue
        {
            return Err("Shared execution token or policy allowance exhausted".into());
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExecutionScopeWire {
    id: String,
    root_run_id: String,
    limits: ExecutionLimits,
    created_at_ms: u64,
    deadline_at_ms: u64,
    reservations: BTreeMap<String, ExecutionReservation>,
    version: u64,
}

impl<'de> Deserialize<'de> for ExecutionScope {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = ExecutionScopeWire::deserialize(deserializer)?;
        let scope = Self {
            id: wire.id,
            root_run_id: wire.root_run_id,
            limits: wire.limits,
            created_at_ms: wire.created_at_ms,
            deadline_at_ms: wire.deadline_at_ms,
            reservations: wire.reservations,
            version: wire.version,
        };
        scope.validate().map_err(serde::de::Error::custom)?;
        Ok(scope)
    }
}

fn validate_id(id: &str) -> Result<(), String> {
    if id.trim().is_empty() || id.len() > 256 || id.chars().any(char::is_control) {
        Err("Invalid execution identity".into())
    } else {
        Ok(())
    }
}

fn add(total: &mut u64, delta: u64) -> Result<(), String> {
    *total = total.checked_add(delta).ok_or("Execution usage overflow")?;
    Ok(())
}

fn normalized_tokens(usage: &TokenUsage) -> Result<(u64, u64, u64), String> {
    let input = usage.prompt_tokens.max(usage.cached_prompt_tokens);
    let output = usage.completion_tokens.max(usage.reasoning_tokens);
    Ok((
        input,
        output,
        usage.total_tokens.max(
            input
                .checked_add(output)
                .ok_or("Execution usage overflow")?,
        ),
    ))
}

fn merge_observed(a: Option<&TokenUsage>, b: Option<&TokenUsage>) -> Option<TokenUsage> {
    match (a, b) {
        (None, None) => None,
        (Some(value), None) | (None, Some(value)) => Some(value.clone()),
        (Some(a), Some(b)) => Some(TokenUsage {
            prompt_tokens: a.prompt_tokens.max(b.prompt_tokens),
            completion_tokens: a.completion_tokens.max(b.completion_tokens),
            total_tokens: a.total_tokens.max(b.total_tokens),
            cached_prompt_tokens: a.cached_prompt_tokens.max(b.cached_prompt_tokens),
            reasoning_tokens: a.reasoning_tokens.max(b.reasoning_tokens),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execution_limits_known_usage_preserves_observed_lower_bound() {
        let mut value = scope(3, 2);
        value.reserve(request("one", "call"), 101).unwrap();
        value
            .settle(ReservationSettlement {
                observed_usage: None,
                physical_retired: true,
                reservation_id: "one".into(),
                outcome: SettlementOutcome::Unknown {
                    observed: Some(TokenUsage {
                        prompt_tokens: 500,
                        ..TokenUsage::default()
                    }),
                },
            })
            .unwrap();
        value
            .settle(ReservationSettlement {
                observed_usage: None,
                physical_retired: true,
                reservation_id: "one".into(),
                outcome: SettlementOutcome::Known(TokenUsage {
                    prompt_tokens: 100,
                    ..TokenUsage::default()
                }),
            })
            .unwrap();
        assert_eq!(
            value.consumption().unwrap().charged_input_tokens,
            500,
            "late final erased an accepted lower bound"
        );
    }

    #[test]
    fn execution_limits_physical_retirement_and_usage_never_regress() {
        let mut value = scope(3, 2);
        value.reserve(request("one", "call"), 101).unwrap();
        let known = SettlementOutcome::Known(TokenUsage {
            prompt_tokens: 100,
            ..TokenUsage::default()
        });
        value
            .settle(ReservationSettlement {
                observed_usage: None,
                reservation_id: "one".into(),
                outcome: known.clone(),
                physical_retired: false,
            })
            .unwrap();
        assert_eq!(value.consumption().unwrap().inflight_operations, 1);
        value
            .settle(ReservationSettlement {
                observed_usage: None,
                reservation_id: "one".into(),
                outcome: known.clone(),
                physical_retired: true,
            })
            .unwrap();
        value
            .settle(ReservationSettlement {
                observed_usage: None,
                reservation_id: "one".into(),
                outcome: SettlementOutcome::Unknown {
                    observed: Some(TokenUsage {
                        prompt_tokens: 500,
                        ..TokenUsage::default()
                    }),
                },
                physical_retired: false,
            })
            .unwrap();
        assert_eq!(value.consumption().unwrap().inflight_operations, 0);
        assert_eq!(value.consumption().unwrap().charged_input_tokens, 500);
        assert_eq!(
            value.reservations["one"]
                .settlement
                .as_ref()
                .unwrap()
                .outcome,
            known
        );
    }

    fn scope(attempts: u64, retries: u64) -> ExecutionScope {
        let limits = ExecutionLimits {
            max_model_attempts: attempts,
            max_retries: retries,
            ..ExecutionLimits::default()
        };
        ExecutionScope::new("scope".into(), "root".into(), limits, 100).unwrap()
    }

    fn request(id: &str, logical: &str) -> ReservationRequest {
        ReservationRequest {
            id: id.into(),
            run_id: "root".into(),
            logical_call_id: logical.into(),
            operation: ExecutionOperation::ModelAttempt,
            input_tokens: 100,
            output_tokens: 50,
        }
    }

    #[test]
    fn execution_limits_last_slot_is_shared_between_concurrent_callers() {
        let shared = std::sync::Arc::new(std::sync::Mutex::new(scope(1, 0)));
        let outcomes: Vec<_> = std::thread::scope(|threads| {
            let workers: Vec<_> = (0..8)
                .map(|i| {
                    let shared = shared.clone();
                    threads.spawn(move || {
                        shared
                            .lock()
                            .unwrap()
                            .reserve(request(&format!("attempt-{i}"), &format!("call-{i}")), 101)
                    })
                })
                .collect();
            workers.into_iter().map(|t| t.join().unwrap()).collect()
        });
        assert_eq!(
            outcomes
                .iter()
                .filter(|r| matches!(r, Ok(ReservationAdmission::NewlyGranted)))
                .count(),
            1
        );
    }

    #[test]
    fn execution_limits_duplicate_reservation_is_evidence_not_dispatch_authority() {
        let mut value = scope(2, 1);
        assert_eq!(
            value.reserve(request("one", "logical"), 101).unwrap(),
            ReservationAdmission::NewlyGranted
        );
        assert_eq!(
            value.reserve(request("one", "logical"), 101).unwrap(),
            ReservationAdmission::AlreadyRecorded
        );
        assert_eq!(value.consumption().unwrap().model_attempts, 1);
        let mut changed = request("one", "other");
        changed.input_tokens = 1;
        assert!(value.reserve(changed, 101).is_err());
    }

    #[test]
    fn execution_limits_retries_share_logical_identity_across_run_ids() {
        let mut value = scope(10, 1);
        value.reserve(request("first", "logical"), 101).unwrap();
        let mut second = request("fallback", "logical");
        second.run_id = "helper".into();
        value.reserve(second, 101).unwrap();
        assert!(value.reserve(request("rotation", "logical"), 101).is_err());
        assert_eq!(value.consumption().unwrap().retries, 1);
    }

    #[test]
    fn execution_limits_unknown_usage_is_retired_but_stays_charged_after_restore() {
        let mut value = scope(1, 0);
        value.reserve(request("first", "logical"), 101).unwrap();
        value
            .settle(ReservationSettlement {
                observed_usage: None,
                physical_retired: true,
                reservation_id: "first".into(),
                outcome: SettlementOutcome::Unknown { observed: None },
            })
            .unwrap();
        let restored: ExecutionScope =
            serde_json::from_str(&serde_json::to_string(&value).unwrap()).unwrap();
        let usage = restored.consumption().unwrap();
        assert_eq!(
            (
                usage.charged_input_tokens,
                usage.charged_output_tokens,
                usage.inflight_operations
            ),
            (100, 50, 0)
        );
        assert!(restored
            .clone()
            .reserve(request("recovery", "new-logical"), 102)
            .is_err());
        assert!(restored
            .clone()
            .reserve(request("expired", "logical"), restored.deadline_at_ms)
            .is_err());
    }

    #[test]
    fn execution_limits_settlement_is_idempotent_and_records_overrun_before_blocking() {
        let mut value = scope(5, 4);
        value.limits.budget.max_input_tokens = Some(100);
        value.reserve(request("one", "logical"), 101).unwrap();
        let settlement = ReservationSettlement {
            observed_usage: None,
            physical_retired: true,
            reservation_id: "one".into(),
            outcome: SettlementOutcome::Known(TokenUsage {
                prompt_tokens: 101,
                completion_tokens: 2,
                total_tokens: 103,
                ..TokenUsage::default()
            }),
        };
        value.settle(settlement.clone()).unwrap();
        let version = value.version;
        value.settle(settlement).unwrap();
        assert_eq!(value.version, version);
        assert_eq!(value.consumption().unwrap().known.input_tokens, 101);
        assert!(value.reserve(request("two", "next"), 102).is_err());
        assert!(value
            .settle(ReservationSettlement {
                observed_usage: None,
                physical_retired: true,
                reservation_id: "one".into(),
                outcome: SettlementOutcome::Known(TokenUsage::default())
            })
            .is_err());
    }

    #[test]
    fn execution_limits_invalid_or_unbounded_limits_are_refused() {
        let mut limits = ExecutionLimits::default();
        limits.budget.max_wall_time_ms = None;
        assert!(ExecutionScope::new("x".into(), "root".into(), limits, 100).is_err());
        let limits = ExecutionLimits {
            max_model_attempts: 0,
            ..ExecutionLimits::default()
        };
        assert!(ExecutionScope::new("x".into(), "root".into(), limits, 100).is_err());
        assert!(
            ExecutionScope::new("".into(), "root".into(), ExecutionLimits::default(), 100).is_err()
        );
        assert!(ExecutionScope::new(
            "x".into(),
            "root".into(),
            ExecutionLimits::default(),
            u64::MAX
        )
        .is_err());
    }
}
