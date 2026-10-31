use std::collections::BTreeMap;
use std::future::Future;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

use async_trait::async_trait;
use futures::FutureExt;
use serde::{Deserialize, Serialize};

use super::{
    ExecutionLimits, ExecutionOperation, ExecutionScope, ReservationAdmission, ReservationRequest,
    ReservationSettlement, SettlementOutcome,
};
use crate::{
    AgentConfig, CancelSignal, ExecutionClock, ModelGenerateRequest, ModelStreamFrame,
    ModelStreamSink, TokenUsage,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecutionAllowance {
    pub deadline_at_ms: u64,
    pub remaining_input_tokens: u64,
    pub remaining_output_tokens: u64,
    pub remaining_total_tokens: u64,
}

/// Stable identity of one durable allowance authority, independent of its live handle.
/// A host must compare this complete binding before reattaching a persisted run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", try_from = "ExecutionBudgetAuthorityWire")]
pub struct ExecutionBudgetAuthority {
    pub owner_id: String,
    pub store_id: String,
    pub scope_id: String,
}

impl ExecutionBudgetAuthority {
    pub fn new(
        owner_id: impl Into<String>,
        store_id: impl Into<String>,
        scope_id: impl Into<String>,
    ) -> Result<Self, String> {
        let authority = Self {
            owner_id: owner_id.into(),
            store_id: store_id.into(),
            scope_id: scope_id.into(),
        };
        authority.validate()?;
        Ok(authority)
    }
    pub fn validate(&self) -> Result<(), String> {
        for id in [&self.owner_id, &self.store_id, &self.scope_id] {
            if id.is_empty()
                || id.len() > 256
                || id.trim() != id
                || id.chars().any(char::is_control)
            {
                return Err("Invalid execution budget authority identity".into());
            }
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExecutionBudgetAuthorityWire {
    owner_id: String,
    store_id: String,
    scope_id: String,
}
impl TryFrom<ExecutionBudgetAuthorityWire> for ExecutionBudgetAuthority {
    type Error = String;
    fn try_from(wire: ExecutionBudgetAuthorityWire) -> Result<Self, Self::Error> {
        Self::new(wire.owner_id, wire.store_id, wire.scope_id)
    }
}

/// Hosts atomically persist admission before returning NewlyGranted. Cancellation
/// of an awaiting caller must not abandon an in-progress persistence operation.
#[async_trait]
pub trait ExecutionBudgetPort: Send + Sync {
    /// Recovery inspection grants no dispatch authority. Durable embedders use
    /// the stable physical-attempt key to settle only an existing reservation.
    async fn reservation(
        &self,
        _scope_id: &str,
        _reservation_id: &str,
    ) -> Result<Option<super::ExecutionReservation>, String> {
        Err("execution port does not support durable reservation inspection".into())
    }
    /// None denotes an ephemeral/helper authority and cannot certify durable reattachment.
    fn authority_binding(&self) -> Option<ExecutionBudgetAuthority> {
        None
    }
    async fn reserve(
        &self,
        scope_id: &str,
        request: ReservationRequest,
    ) -> Result<ReservationAdmission, String>;
    /// Start owned accounting synchronously. Dropping the returned receipt only
    /// stops waiting; it must never cancel the write that was just submitted.
    fn submit_settlement(
        &self,
        scope_id: &str,
        settlement: ReservationSettlement,
    ) -> futures::future::BoxFuture<'static, Result<(), String>>;
    async fn settle(
        &self,
        scope_id: &str,
        settlement: ReservationSettlement,
    ) -> Result<(), String> {
        self.submit_settlement(scope_id, settlement).await
    }
    async fn allowance(&self, scope_id: &str) -> Result<ExecutionAllowance, String>;
    /// Abandonment never proves physical retirement or refunds unknown usage.
    fn abandon(&self, scope_id: &str, mut settlement: ReservationSettlement) {
        settlement.physical_retired = false;
        drop(self.submit_settlement(scope_id, settlement));
    }
}

pub struct SystemBudgetClock;
#[async_trait]
impl ExecutionClock for SystemBudgetClock {
    fn now_ms(&self) -> u64 {
        crate::primitives::now_millis()
    }
}

/// Explicit ephemeral store for embedders. Durable hosts implement the same port.
pub struct InMemoryExecutionBudget {
    scopes: Mutex<BTreeMap<String, ExecutionScope>>,
    clock: Arc<dyn ExecutionClock>,
}

impl InMemoryExecutionBudget {
    pub fn new(scope: ExecutionScope, clock: Arc<dyn ExecutionClock>) -> Result<Self, String> {
        scope.validate()?;
        Ok(Self {
            scopes: Mutex::new(BTreeMap::from([(scope.id.clone(), scope)])),
            clock,
        })
    }
    pub fn snapshot(&self, scope_id: &str) -> Result<ExecutionScope, String> {
        self.scopes
            .lock()
            .map_err(|_| "Execution budget state unavailable")?
            .get(scope_id)
            .cloned()
            .ok_or("Execution scope unavailable".into())
    }
}

#[async_trait]
impl ExecutionBudgetPort for InMemoryExecutionBudget {
    async fn reservation(
        &self,
        scope_id: &str,
        reservation_id: &str,
    ) -> Result<Option<super::ExecutionReservation>, String> {
        Ok(self
            .snapshot(scope_id)?
            .reservations
            .get(reservation_id)
            .cloned())
    }
    async fn reserve(
        &self,
        scope_id: &str,
        request: ReservationRequest,
    ) -> Result<ReservationAdmission, String> {
        self.scopes
            .lock()
            .map_err(|_| "Execution budget state unavailable")?
            .get_mut(scope_id)
            .ok_or("Execution scope unavailable")?
            .reserve(request, self.clock.now_ms())
    }
    fn submit_settlement(
        &self,
        scope_id: &str,
        settlement: ReservationSettlement,
    ) -> futures::future::BoxFuture<'static, Result<(), String>> {
        let result = (|| {
            self.scopes
                .lock()
                .map_err(|_| "Execution budget state unavailable")?
                .get_mut(scope_id)
                .ok_or("Execution scope unavailable")?
                .settle(settlement)
        })();
        futures::future::ready(result).boxed()
    }
    async fn allowance(&self, scope_id: &str) -> Result<ExecutionAllowance, String> {
        self.snapshot(scope_id)?.allowance(self.clock.now_ms())
    }
}

/// Request-local authority, deliberately absent from runtime snapshots.
#[derive(Clone)]
pub struct ExecutionContext {
    pub scope_id: String,
    pub run_id: String,
    pub port: Arc<dyn ExecutionBudgetPort>,
    pub clock: Arc<dyn ExecutionClock>,
    pub cancel: CancelSignal,
    pub deadline_at_ms: u64,
    /// Transient terminal failure; never serialized as execution authority.
    pub failures: Arc<Mutex<Option<String>>>,
    pub secondary_cancel: Option<CancelSignal>,
}

impl ExecutionContext {
    /// Remaining time on the existing admitted clock; never starts a new budget.
    pub fn remaining_wall_time_ms(&self) -> Result<u64, String> {
        self.check_live()?;
        self.deadline_at_ms
            .checked_sub(self.clock.now_ms())
            .filter(|remaining| *remaining > 0)
            .ok_or_else(|| "Shared execution deadline reached".into())
    }
    pub fn binding_hint(&self) -> super::ExecutionBindingHint {
        super::ExecutionBindingHint {
            scope_id: self.scope_id.clone(),
            run_id: self.run_id.clone(),
            deadline_at_ms: self.deadline_at_ms,
        }
    }
    pub fn ephemeral(
        run_id: String,
        limits: ExecutionLimits,
        cancel: CancelSignal,
    ) -> Result<Self, String> {
        let clock: Arc<dyn ExecutionClock> = Arc::new(SystemBudgetClock);
        let scope = ExecutionScope::new(
            uuid::Uuid::new_v4().to_string(),
            run_id.clone(),
            limits,
            clock.now_ms(),
        )?;
        Ok(Self {
            scope_id: scope.id.clone(),
            run_id,
            deadline_at_ms: scope.deadline_at_ms,
            port: Arc::new(InMemoryExecutionBudget::new(scope, clock.clone())?),
            clock,
            cancel,
            failures: Arc::new(Mutex::new(None)),
            secondary_cancel: None,
        })
    }

    pub fn check_live(&self) -> Result<(), String> {
        if let Some(error) = self.failure() {
            return Err(error);
        }
        if self.cancel.is_cancelled() {
            return Err(self.cancel.run_error().into());
        }
        if let Some(cancel) = self
            .secondary_cancel
            .as_ref()
            .filter(|cancel| cancel.is_cancelled())
        {
            return Err(cancel.run_error().into());
        }
        if self.clock.now_ms() >= self.deadline_at_ms {
            return Err("Shared execution deadline reached".into());
        }
        Ok(())
    }

    pub async fn check_authority(&self) -> Result<ExecutionAllowance, String> {
        self.check_authority_for(None).await
    }

    async fn check_authority_for(
        &self,
        request_cancel: Option<&CancelSignal>,
    ) -> Result<ExecutionAllowance, String> {
        self.check_live()?;
        let allowance = self
            .wait(self.port.allowance(&self.scope_id), request_cancel)
            .await?;
        self.check_live()?;
        if allowance.deadline_at_ms < self.deadline_at_ms {
            return Err(
                "Execution deadline binding changed; explicit reattachment required".into(),
            );
        }
        if self.clock.now_ms() >= allowance.deadline_at_ms {
            return Err("Shared execution deadline reached".into());
        }
        Ok(allowance)
    }

    async fn reserve(
        &self,
        request: ReservationRequest,
        request_cancel: CancelSignal,
    ) -> Result<ExecutionPermit, String> {
        self.check_live()?;
        if request_cancel.is_cancelled() {
            return Err("Execution request cancelled".into());
        }
        let id = request.id.clone();
        match self
            .wait(
                self.port.reserve(&self.scope_id, request),
                Some(&request_cancel),
            )
            .await?
        {
            ReservationAdmission::AlreadyRecorded => {
                return Err("Execution reservation already recorded; replay refused".into());
            }
            ReservationAdmission::NewlyGranted => {}
        }
        let mut permit = ExecutionPermit {
            execution: self.clone(),
            request_cancel,
            id,
            observation: Arc::new(Mutex::new(Observation::default())),
            retired: false,
        };
        if let Err(error) = permit.check_live() {
            // An admitted reservation can outlive its caller. Proving that this
            // call has not dispatched permits token retirement, never a replay.
            let _ = permit.finish(SettlementOutcome::ProvenNeverSent).await;
            return Err(error);
        }
        Ok(permit)
    }

    pub async fn reserve_tool(&self, logical_id: &str) -> Result<ExecutionPermit, String> {
        self.reserve_tool_with_id(logical_id, &uuid::Uuid::new_v4().to_string())
            .await
    }

    /// Reserve a finite artifact ceiling and one native capability attempt on
    /// this same scope. Only physical retirement can reduce its byte ceiling.
    pub async fn reserve_native_artifact(
        &self,
        logical_id: &str,
        max_artifact_bytes: u64,
    ) -> Result<ExecutionPermit, String> {
        self.reserve(
            ReservationRequest {
                id: uuid::Uuid::new_v4().to_string(),
                run_id: self.run_id.clone(),
                logical_call_id: logical_id.into(),
                operation: ExecutionOperation::NativeArtifact { max_artifact_bytes },
                input_tokens: 0,
                output_tokens: 0,
            },
            self.cancel.clone(),
        )
        .await
    }

    /// Stable physical-attempt identity for durable effect reconciliation. Exact
    /// reservation replay is evidence, never authority to dispatch twice.
    pub async fn reserve_tool_with_id(
        &self,
        logical_id: &str,
        reservation_id: &str,
    ) -> Result<ExecutionPermit, String> {
        self.reserve(
            ReservationRequest {
                id: reservation_id.into(),
                run_id: self.run_id.clone(),
                logical_call_id: logical_id.into(),
                operation: ExecutionOperation::Tool,
                input_tokens: 0,
                output_tokens: 0,
            },
            self.cancel.clone(),
        )
        .await
    }

    pub async fn bounded<T>(
        &self,
        work: impl Future<Output = Result<T, String>>,
    ) -> Result<T, String> {
        ModelCallContext::new(self.clone(), "tool-deadline".into())
            .bounded(work)
            .await
    }

    pub fn failure(&self) -> Option<String> {
        self.failures
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
    pub fn record_failure(&self, error: String) {
        self.failures
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_or_insert(error);
    }

    /// Bounded acknowledgement of already-observed evidence. This performs no
    /// admission or allowance check: known usage and settled effects must remain
    /// recordable after exhaustion. A started storage operation owns its write
    /// independently of the caller abandoning this acknowledgement wait.
    pub async fn observe<T>(
        &self,
        work: impl Future<Output = Result<T, String>>,
    ) -> Result<T, String> {
        let work = work.fuse();
        let cancelled = self.cancel.cancelled().fuse();
        let secondary = self.secondary_cancel.clone().unwrap_or_default();
        let secondary_cancelled = secondary.cancelled().fuse();
        let timeout = self.clock.wait_until_ms(self.deadline_at_ms).fuse();
        futures::pin_mut!(work, cancelled, secondary_cancelled, timeout);
        futures::select_biased! {
            result = work => result,
            _ = cancelled => Err(self.cancel.run_error().into()),
            _ = secondary_cancelled => { self.cancel.cancel_with(secondary.cause().unwrap_or(crate::CancelCause::OwnerStop)); Err(secondary.run_error().into()) },
            _ = timeout => { self.cancel.cancel_with(crate::CancelCause::ExecutionDeadline); Err("Shared execution deadline reached".into()) },
        }
    }

    async fn wait<T>(
        &self,
        work: impl Future<Output = Result<T, String>>,
        request_cancel: Option<&CancelSignal>,
    ) -> Result<T, String> {
        self.check_live()?;
        let local = request_cancel.cloned().unwrap_or_default();
        let work = work.fuse();
        let cancelled = self.cancel.cancelled().fuse();
        let secondary = self.secondary_cancel.clone().unwrap_or_default();
        let secondary_cancelled = secondary.cancelled().fuse();
        let local_cancelled = local.cancelled().fuse();
        let timeout = self.clock.wait_until_ms(self.deadline_at_ms).fuse();
        futures::pin_mut!(
            work,
            cancelled,
            secondary_cancelled,
            local_cancelled,
            timeout
        );
        futures::select_biased! {
            _ = cancelled => Err(self.cancel.run_error().into()),
            _ = secondary_cancelled => { self.cancel.cancel_with(secondary.cause().unwrap_or(crate::CancelCause::OwnerStop)); Err(secondary.run_error().into()) },
            _ = local_cancelled => Err("Execution request cancelled".into()),
            _ = timeout => { self.cancel.cancel_with(crate::CancelCause::ExecutionDeadline); local.cancel(); Err("Shared execution deadline reached".into()) },
            result = work => result,
        }
    }

    /// Once admitted, keep polling the actual effect until it retires. Stop and
    /// deadline close admission and signal cancellation; they do not invent an
    /// effect result. Host executors must retain custody if the outer run drops.
    pub async fn until_retired<T>(
        &self,
        work: impl Future<Output = T>,
        other: Option<CancelSignal>,
    ) -> Result<T, String> {
        self.check_live()?;
        if other.as_ref().is_some_and(CancelSignal::is_cancelled) {
            return Err(other.as_ref().unwrap().run_error().into());
        }
        let other = other
            .or_else(|| self.secondary_cancel.clone())
            .unwrap_or_default();
        let work = work.fuse();
        let cancelled = self.cancel.cancelled().fuse();
        let external = other.cancelled().fuse();
        let timeout = self.clock.wait_until_ms(self.deadline_at_ms).fuse();
        futures::pin_mut!(work, cancelled, external, timeout);
        futures::select_biased! {
            _ = cancelled => Ok(work.await),
            _ = external => { self.cancel.cancel_with(other.cause().unwrap_or(crate::CancelCause::OwnerStop)); Ok(work.await) },
            _ = timeout => { self.cancel.cancel_with(crate::CancelCause::ExecutionDeadline); Ok(work.await) },
            result = work => Ok(result),
        }
    }
}

/// One marker per physical attempt. It becomes true only immediately before
/// transport work is first polled, never for validation or allowance failure.
#[derive(Default)]
pub struct ModelDispatchState(AtomicBool);
impl ModelDispatchState {
    pub fn started(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}
type DispatchValidator =
    Arc<dyn Fn() -> futures::future::BoxFuture<'static, Result<(), String>> + Send + Sync>;

#[derive(Clone)]
pub struct ModelCallContext {
    pub execution: ExecutionContext,
    pub logical_call_id: String,
    pub request_cancel: CancelSignal,
    dispatch_validators: Vec<DispatchValidator>,
}

impl ModelCallContext {
    pub fn new(execution: ExecutionContext, logical_call_id: String) -> Self {
        Self {
            execution,
            logical_call_id,
            request_cancel: CancelSignal::new(),
            dispatch_validators: Vec::new(),
        }
    }
    /// Appends host validation without replacing existing caller validators.
    /// Clones retain the complete chain across transport retries and fallbacks.
    pub fn with_dispatch_validator<F, Fut>(mut self, validator: F) -> Self
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), String>> + Send + 'static,
    {
        self.dispatch_validators
            .push(Arc::new(move || Box::pin(validator())));
        self
    }
    pub fn check_live(&self) -> Result<(), String> {
        self.execution.check_live()?;
        if self.request_cancel.is_cancelled() {
            return Err("Model request cancelled".into());
        }
        Ok(())
    }

    /// Clamp output before building the provider body. Reservations use a
    /// conservative text byte estimate; this is not a remote billing guarantee.
    pub async fn prepare_request(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
    ) -> Result<ModelGenerateRequest, String> {
        self.check_live()?;
        let allowance = self
            .execution
            .check_authority_for(Some(&self.request_cancel))
            .await?;
        let input = reservation_input(config, request, &allowance)?;
        let output = u64::from(
            request
                .max_tokens
                .or_else(|| config.settings.as_ref().and_then(|s| s.max_tokens))
                .unwrap_or(4096),
        )
        .min(allowance.remaining_output_tokens)
        .min(allowance.remaining_total_tokens.saturating_sub(input));
        if input == 0 || input > allowance.remaining_input_tokens || output == 0 {
            return Err("Shared execution token allowance exhausted".into());
        }
        let mut prepared = request.clone();
        prepared.max_tokens =
            Some(u32::try_from(output).map_err(|_| "Invalid model output allowance")?);
        self.check_live()?;
        Ok(prepared)
    }

    pub async fn reserve_model_attempt(
        &self,
        input: u64,
        output: u64,
    ) -> Result<ExecutionPermit, String> {
        self.check_live()?;
        self.execution
            .reserve(
                ReservationRequest {
                    id: uuid::Uuid::new_v4().to_string(),
                    run_id: self.execution.run_id.clone(),
                    logical_call_id: self.logical_call_id.clone(),
                    operation: ExecutionOperation::ModelAttempt,
                    input_tokens: input,
                    output_tokens: output,
                },
                self.request_cancel.clone(),
            )
            .await
    }

    /// Call after attachment hydration and before constructing each physical send.
    pub async fn reserve_prepared(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
    ) -> Result<(ModelGenerateRequest, ExecutionPermit), String> {
        let prepared = self.prepare_request(config, request).await?;
        let allowance = self
            .execution
            .check_authority_for(Some(&self.request_cancel))
            .await?;
        let input = reservation_input(config, &prepared, &allowance)?;
        let permit = self
            .reserve_model_attempt(
                input,
                u64::from(
                    prepared
                        .max_tokens
                        .ok_or("Missing finite model output allowance")?,
                ),
            )
            .await?;
        Ok((prepared, permit))
    }

    /// Controlled adapters must wrap EVERY physical transport attempt,
    /// including retry/key-rotation/fallback sends, in this boundary. Validation
    /// runs after allowance waits and before transport work is first polled.
    /// The caller must settle a false marker as ProvenNeverSent through its
    /// existing reservation; ordinary bounded waits/backoff grant no dispatch.
    pub async fn dispatch<T>(
        &self,
        state: &ModelDispatchState,
        work: impl Future<Output = Result<T, String>>,
    ) -> Result<T, String> {
        self.check_live()?;
        self.execution
            .check_authority_for(Some(&self.request_cancel))
            .await?;
        for validator in &self.dispatch_validators {
            if let Err(error) = self
                .execution
                .wait(validator(), Some(&self.request_cancel))
                .await
            {
                // A rejected read must not be retried with another key/model.
                self.request_cancel.cancel();
                return Err(error);
            }
        }
        self.check_live()?;
        let mut retirement = RequestRetirement {
            cancel: self.request_cancel.clone(),
            complete: false,
        };
        let transport = async {
            state.0.store(true, Ordering::SeqCst);
            work.await
        };
        let result = self
            .execution
            .wait(transport, Some(&self.request_cancel))
            .await;
        if self.execution.cancel.is_cancelled() {
            self.request_cancel.cancel();
        }
        retirement.complete = true;
        result
    }

    /// Cancellation is marked before an enclosing adapter can start a retry.
    pub async fn bounded<T>(
        &self,
        work: impl Future<Output = Result<T, String>>,
    ) -> Result<T, String> {
        self.check_live()?;
        self.execution
            .check_authority_for(Some(&self.request_cancel))
            .await?;
        let mut retirement = RequestRetirement {
            cancel: self.request_cancel.clone(),
            complete: false,
        };
        let result = self.execution.wait(work, Some(&self.request_cancel)).await;
        if self.execution.cancel.is_cancelled() {
            self.request_cancel.cancel();
        }
        retirement.complete = true;
        result
    }
}

fn reservation_input(
    config: &AgentConfig,
    request: &ModelGenerateRequest,
    allowance: &ExecutionAllowance,
) -> Result<u64, String> {
    if request.messages.iter().any(|m| {
        m.content
            .attachments
            .as_ref()
            .is_some_and(|a| !a.is_empty())
    }) {
        // Opaque media has no portable tokenizer bound. Reserve all remaining
        // input allowance so two concurrent calls cannot assume it is free.
        return Ok(allowance.remaining_input_tokens);
    }
    let bytes = serde_json::to_vec(&(&request.system, &request.messages, &config.tools))
        .map_err(|_| "Model request cannot be reserved")?
        .len();
    u64::try_from(bytes)
        .ok()
        .and_then(|n| n.checked_add(256))
        .ok_or("Model request reservation overflow".into())
}

struct RequestRetirement {
    cancel: CancelSignal,
    complete: bool,
}
impl Drop for RequestRetirement {
    fn drop(&mut self) {
        if !self.complete {
            self.cancel.cancel();
        }
    }
}

#[derive(Default)]
struct Observation {
    partial: Option<TokenUsage>,
    definitive: Option<TokenUsage>,
}

/// A non-clone permit authorizes one dispatch. Dropping it cannot refund usage.
pub struct ExecutionPermit {
    execution: ExecutionContext,
    request_cancel: CancelSignal,
    id: String,
    observation: Arc<Mutex<Observation>>,
    retired: bool,
}

impl ExecutionPermit {
    pub fn reservation_id(&self) -> &str {
        &self.id
    }
    pub fn check_live(&self) -> Result<(), String> {
        self.execution.check_live()?;
        if self.retired || self.request_cancel.is_cancelled() {
            return Err("Execution permit is retired or cancelled".into());
        }
        Ok(())
    }
    pub async fn revalidate(&self) -> Result<(), String> {
        self.check_live()?;
        self.execution
            .check_authority_for(Some(&self.request_cancel))
            .await?;
        self.check_live()
    }
    pub fn observe_usage(&self, usage: &TokenUsage) {
        if let Ok(mut observation) = self.observation.lock() {
            observation.partial = super::merge_observed(observation.partial.as_ref(), Some(usage));
        }
    }
    pub fn stream_sink<'a>(&self, downstream: &'a dyn ModelStreamSink) -> ExecutionStreamSink<'a> {
        ExecutionStreamSink {
            downstream,
            observation: self.observation.clone(),
        }
    }
    pub fn observed_outcome(&self) -> SettlementOutcome {
        let observation = self.observation.lock().unwrap_or_else(|e| e.into_inner());
        match &observation.definitive {
            Some(usage) => SettlementOutcome::Known(usage.clone()),
            None => SettlementOutcome::Unknown {
                observed: observation.partial.clone(),
            },
        }
    }
    pub async fn finish(&mut self, outcome: SettlementOutcome) -> Result<(), String> {
        self.finish_with_retirement(outcome, true).await
    }
    pub async fn finish_with_retirement(
        &mut self,
        outcome: SettlementOutcome,
        physical_retired: bool,
    ) -> Result<(), String> {
        let mut receipt = self.submit_finish(outcome, physical_retired);
        // Accounting is allowed after Stop. Accept an already-ready receipt
        // before deciding whether the caller can keep waiting for a slow one.
        let result = match receipt.as_mut().now_or_never() {
            Some(result) => result,
            None => self.execution.wait(receipt, None).await,
        };
        if let Err(error) = &result {
            let cancelled = self.execution.cancel.is_cancelled()
                || self
                    .execution
                    .secondary_cancel
                    .as_ref()
                    .is_some_and(CancelSignal::is_cancelled)
                || self.execution.clock.now_ms() >= self.execution.deadline_at_ms;
            if !cancelled {
                self.execution.record_failure(format!(
                    "Execution accounting acknowledgement failed: {error}"
                ));
            }
        }
        result
    }

    /// Retain custody of a retired effect's accounting until its owned storage
    /// receipt arrives. Stop/deadline cannot stand in for the actual commit
    /// result. Use only with a host port that bounds and owns admitted writes.
    pub async fn finish_owned(
        &mut self,
        outcome: SettlementOutcome,
        physical_retired: bool,
    ) -> Result<(), String> {
        let result = self.submit_finish(outcome, physical_retired).await;
        if let Err(error) = &result {
            self.execution
                .record_failure(format!("Execution accounting commit failed: {error}"));
        }
        result
    }

    fn submit_finish(
        &mut self,
        mut outcome: SettlementOutcome,
        physical_retired: bool,
    ) -> futures::future::BoxFuture<'static, Result<(), String>> {
        if let SettlementOutcome::Unknown { observed } = &mut outcome {
            let observation = self.observation.lock().unwrap_or_else(|e| e.into_inner());
            *observed = super::merge_observed(observed.as_ref(), observation.partial.as_ref());
        }
        // Submission must happen even when cancellation/deadline already won.
        let observed_usage = self
            .observation
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .partial
            .clone();
        let receipt = self.execution.port.submit_settlement(
            &self.execution.scope_id,
            ReservationSettlement {
                reservation_id: self.id.clone(),
                outcome,
                physical_retired,
                observed_usage,
            },
        );
        self.retired = true;
        receipt
    }
}

impl Drop for ExecutionPermit {
    fn drop(&mut self) {
        if !self.retired {
            self.request_cancel.cancel();
            self.execution.port.abandon(
                &self.execution.scope_id,
                ReservationSettlement {
                    reservation_id: self.id.clone(),
                    outcome: self.observed_outcome(),
                    physical_retired: false,
                    observed_usage: self
                        .observation
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .partial
                        .clone(),
                },
            );
        }
    }
}

pub struct ExecutionStreamSink<'a> {
    downstream: &'a dyn ModelStreamSink,
    observation: Arc<Mutex<Observation>>,
}

#[async_trait]
impl ModelStreamSink for ExecutionStreamSink<'_> {
    async fn emit(&self, frame: ModelStreamFrame) -> Result<(), String> {
        {
            let mut observation = self
                .observation
                .lock()
                .map_err(|_| "Execution usage observation unavailable")?;
            match &frame {
                ModelStreamFrame::Usage(usage) => {
                    observation.partial =
                        super::merge_observed(observation.partial.as_ref(), Some(usage))
                }
                ModelStreamFrame::Final(response) => {
                    observation.partial =
                        super::merge_observed(observation.partial.as_ref(), Some(&response.usage));
                }
                _ => {}
            }
        }
        self.downstream.emit(frame).await
    }
    async fn complete_usage(&self, usage: TokenUsage) -> Result<(), String> {
        {
            let mut observation = self
                .observation
                .lock()
                .map_err(|_| "Execution usage observation unavailable")?;
            observation.partial = super::merge_observed(observation.partial.as_ref(), Some(&usage));
            observation.definitive = Some(usage.clone());
        }
        self.downstream.complete_usage(usage).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Content, ModelAdapter, ModelGenerateResponse, ModelStopReason, TokenUsage};
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct TestAdapter {
        calls: AtomicUsize,
        opt_in: bool,
    }
    #[async_trait]
    impl ModelAdapter for TestAdapter {
        fn provider(&self) -> &str {
            "test"
        }
        fn single_attempt_contract(&self) -> bool {
            self.opt_in
        }
        async fn generate(
            &self,
            _: &AgentConfig,
            _: &ModelGenerateRequest,
        ) -> Result<ModelGenerateResponse, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(ModelGenerateResponse {
                content: Content {
                    text: "ok".into(),
                    attachments: None,
                    metadata: None,
                },
                tool_calls: None,
                usage: TokenUsage::default(),
                stop_reason: ModelStopReason::End,
            })
        }
    }

    fn config() -> AgentConfig {
        serde_json::from_value(serde_json::json!({"name":"budget-test","model":"test"})).unwrap()
    }
    fn request() -> ModelGenerateRequest {
        ModelGenerateRequest {
            system: "test".into(),
            messages: vec![],
            temperature: None,
            max_tokens: None,
        }
    }
    fn context() -> ModelCallContext {
        ModelCallContext::new(
            ExecutionContext::ephemeral(
                "run".into(),
                ExecutionLimits::default(),
                CancelSignal::new(),
            )
            .unwrap(),
            "logical".into(),
        )
    }

    #[tokio::test]
    async fn execution_limits_opaque_adapter_is_refused_before_dispatch() {
        let adapter = TestAdapter {
            calls: AtomicUsize::new(0),
            opt_in: false,
        };
        assert!(adapter
            .generate_controlled(&config(), &request(), &context())
            .await
            .is_err());
        assert_eq!(adapter.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn execution_limits_cancelled_context_never_calls_opted_in_adapter() {
        let adapter = TestAdapter {
            calls: AtomicUsize::new(0),
            opt_in: true,
        };
        let context = context();
        context.request_cancel.cancel();
        assert!(adapter
            .generate_controlled(&config(), &request(), &context)
            .await
            .is_err());
        assert_eq!(adapter.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn execution_limits_prepared_request_has_finite_output_cap() {
        let prepared = context()
            .prepare_request(&config(), &request())
            .await
            .unwrap();
        assert!(prepared.max_tokens.is_some_and(|n| n > 0));
    }

    struct LoopAdapter {
        calls: AtomicUsize,
        tools: usize,
    }
    #[async_trait]
    impl ModelAdapter for LoopAdapter {
        fn provider(&self) -> &str {
            "test"
        }
        fn single_attempt_contract(&self) -> bool {
            true
        }
        async fn generate(
            &self,
            _: &AgentConfig,
            _: &ModelGenerateRequest,
        ) -> Result<ModelGenerateResponse, String> {
            let first = self.calls.fetch_add(1, Ordering::SeqCst) == 0;
            Ok(ModelGenerateResponse {
                content: Content {
                    text: if first { "working" } else { "done" }.into(),
                    attachments: None,
                    metadata: None,
                },
                tool_calls: first.then(|| {
                    (0..self.tools)
                        .map(|i| crate::ToolCall {
                            id: format!("tool-{i}"),
                            name: "echo".into(),
                            args: BTreeMap::new(),
                        })
                        .collect()
                }),
                usage: TokenUsage::default(),
                stop_reason: if first {
                    ModelStopReason::ToolCall
                } else {
                    ModelStopReason::End
                },
            })
        }
    }

    fn runtime(adapter: Arc<LoopAdapter>, limits: ExecutionLimits) -> crate::AgentRuntime {
        let mut config = config();
        config.tools = Some(vec![crate::ToolDescriptor {
            name: "echo".into(),
            description: "test".into(),
            parameters_schema: BTreeMap::new(),
            examples: None,
        }]);
        let mut runtime = crate::AgentRuntime::new(config, adapter);
        runtime.init();
        runtime
            .set_execution_context(
                ExecutionContext::ephemeral("runtime-run".into(), limits, CancelSignal::new())
                    .unwrap(),
            )
            .unwrap();
        runtime
    }

    #[tokio::test]
    async fn execution_limits_runtime_never_mints_an_extra_wrapup_model_attempt() {
        let adapter = Arc::new(LoopAdapter {
            calls: AtomicUsize::new(0),
            tools: 1,
        });
        let limits = ExecutionLimits {
            max_model_attempts: 1,
            max_retries: 0,
            ..ExecutionLimits::default()
        };
        let mut runtime = runtime(adapter.clone(), limits);
        let result = runtime
            .run_with_tools(
                Content {
                    text: "test".into(),
                    attachments: None,
                    metadata: None,
                },
                |_, _, _| async {
                    crate::TaskResult::success(
                        Content {
                            text: "echo".into(),
                            attachments: None,
                            metadata: None,
                        },
                        0,
                    )
                },
            )
            .await;
        assert_eq!(
            adapter.calls.load(Ordering::SeqCst),
            1,
            "runtime dispatched past the shared model allowance"
        );
        assert_eq!(result.status, crate::TaskStatus::Error);
    }

    #[tokio::test]
    async fn execution_limits_runtime_parallel_tools_share_one_remaining_slot() {
        let adapter = Arc::new(LoopAdapter {
            calls: AtomicUsize::new(0),
            tools: 2,
        });
        let limits = ExecutionLimits {
            max_tool_calls: 1,
            ..ExecutionLimits::default()
        };
        let mut runtime = runtime(adapter.clone(), limits);
        let dispatched = Arc::new(AtomicUsize::new(0));
        let observed = dispatched.clone();
        let result = runtime
            .run_with_tools(
                Content {
                    text: "test".into(),
                    attachments: None,
                    metadata: None,
                },
                move |_, _, _| {
                    let observed = observed.clone();
                    async move {
                        observed.fetch_add(1, Ordering::SeqCst);
                        crate::TaskResult::success(
                            Content {
                                text: "echo".into(),
                                attachments: None,
                                metadata: None,
                            },
                            0,
                        )
                    }
                },
            )
            .await;
        assert_eq!(
            dispatched.load(Ordering::SeqCst),
            1,
            "parallel tool requests overspent one slot"
        );
        assert_eq!(
            adapter.calls.load(Ordering::SeqCst),
            1,
            "exhaustion must not dispatch another model call"
        );
        assert_eq!(result.status, crate::TaskStatus::Error);
    }
}

#[cfg(test)]
mod regressions;
