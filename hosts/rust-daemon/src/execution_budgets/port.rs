use std::sync::{Arc, Mutex as StdMutex, Weak};

use anima_core::execution_limits::{
    ExecutionAllowance, ExecutionBudgetPort, ReservationAdmission, ReservationRequest,
    ReservationSettlement,
};
use anima_core::CancelSignal;
use async_trait::async_trait;
use futures::future::BoxFuture;
use futures::FutureExt;
use tokio::sync::{Mutex, RwLock};

use super::{latch_failure, refresh_owner_limits, save_admission, BudgetCandidate};
use crate::app::SharedDaemonState;
use crate::state::DaemonState;

#[derive(Clone)]
pub(super) struct HostBudgetPort {
    state: Weak<RwLock<DaemonState>>,
    transactions: Arc<Mutex<()>>,
    scope_id: String,
    run_id: String,
    cancel: CancelSignal,
    failures: Arc<StdMutex<Option<String>>>,
    runtime: tokio::runtime::Handle,
}

enum Mutation {
    Reserve(ReservationRequest),
    Settle(ReservationSettlement),
    Refresh,
}

impl HostBudgetPort {
    pub(super) fn new(
        state: &SharedDaemonState,
        transactions: Arc<Mutex<()>>,
        scope_id: String,
        run_id: String,
        cancel: CancelSignal,
        failures: Arc<StdMutex<Option<String>>>,
    ) -> Self {
        Self {
            state: Arc::downgrade(state),
            transactions,
            scope_id,
            run_id,
            cancel,
            failures,
            runtime: tokio::runtime::Handle::current(),
        }
    }

    fn check_scope(&self, scope_id: &str) -> Result<(), String> {
        if scope_id != self.scope_id {
            return Err("Execution port cannot access another scope".into());
        }
        Ok(())
    }

    fn check_admission(&self) -> Result<(), String> {
        if self.cancel.is_cancelled() {
            return Err(self.cancel.run_error().into());
        }
        if let Some(error) = self
            .failures
            .lock()
            .map_err(|_| "Shared execution failure state is unavailable")?
            .clone()
        {
            return Err(error);
        }
        Ok(())
    }

    /// Spawn before producing the receipt future: dropping an unpolled receipt
    /// cannot drop the accounting write or its transaction/ownership custody.
    fn submit(
        &self,
        mutation: Mutation,
    ) -> BoxFuture<'static, Result<ReservationAdmission, String>> {
        let Some(state) = self.state.upgrade() else {
            return Box::pin(futures::future::ready(Err(
                "Execution host is unavailable".into()
            )));
        };
        let worker = self.clone();
        let task = self.runtime.spawn(async move {
            match std::panic::AssertUnwindSafe(worker.apply(state, mutation))
                .catch_unwind()
                .await
            {
                Ok(result) => result,
                Err(_) => {
                    let reason = "Execution accounting worker panicked; further dispatch is closed"
                        .to_string();
                    let mut failure = worker
                        .failures
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    if failure.is_none() {
                        *failure = Some(reason.clone());
                    }
                    Err(reason)
                }
            }
        });
        Box::pin(async move {
            task.await
                .map_err(|error| format!("Execution accounting worker failed: {error}"))?
        })
    }

    async fn apply(
        &self,
        state: SharedDaemonState,
        mutation: Mutation,
    ) -> Result<ReservationAdmission, String> {
        let _transaction = self.transactions.clone().lock_owned().await;
        let owner = {
            let guard = state.read().await;
            guard
                .ensure_profile_owned()
                .map_err(|error| error.to_string())?;
            guard.profile_owner()
        };
        if let Some(owner) = owner {
            if let Err(error) = owner.verify_owned().await {
                latch_failure(
                    &mut *state.write().await,
                    &self.scope_id,
                    &error.to_string(),
                );
                return Err(error.to_string());
            }
        }
        let (candidate, request, result, terminal) = {
            let mut guard = state.write().await;
            if guard.execution_budgets.run_scopes.get(&self.run_id) != Some(&self.scope_id) {
                return Err("Execution run binding changed or is unavailable".into());
            }
            let registry = guard.execution_budgets.clone();
            let original = registry
                .scopes
                .get(&self.scope_id)
                .ok_or("Execution scope is unavailable")?;
            let before = original.version;
            let binding = anima_core::execution_limits::ExecutionBindingHint {
                scope_id: self.scope_id.clone(),
                run_id: self.run_id.clone(),
                deadline_at_ms: original.deadline_at_ms,
            };
            let expected_generation = guard.execution_budgets.generation;
            let mut candidate = BudgetCandidate {
                expected_generation,
                registry,
                binding,
            };
            if !matches!(&mutation, Mutation::Settle(_)) {
                self.check_admission()?;
                refresh_owner_limits(&guard, &mut candidate, None)?;
            }
            let scope = candidate
                .registry
                .scopes
                .get_mut(&self.scope_id)
                .ok_or("Execution scope is unavailable")?;
            let now_ms = anima_core::primitives::now_millis();
            let result = match mutation {
                Mutation::Reserve(request) => {
                    self.check_admission()?;
                    if request.run_id != self.run_id {
                        return Err("Execution reservation belongs to another run".into());
                    }
                    match scope.reserve(request, now_ms) {
                        Ok(result) => result,
                        Err(error) => {
                            latch_failure(&mut guard, &self.scope_id, &error);
                            return Err(error);
                        }
                    }
                }
                Mutation::Settle(settlement) => {
                    // Late accounting ignores Stop/deadline/failure. Identity
                    // and retained writer ownership still fence the write.
                    let reservation = scope
                        .reservations
                        .get(&settlement.reservation_id)
                        .ok_or("Execution reservation is unavailable")?;
                    if reservation.request.run_id != self.run_id {
                        return Err("Execution settlement belongs to another run".into());
                    }
                    scope.settle(settlement)?;
                    ReservationAdmission::AlreadyRecorded
                }
                Mutation::Refresh => ReservationAdmission::AlreadyRecorded,
            };
            let terminal = scope.allowance(now_ms).err();
            candidate.binding.deadline_at_ms = scope.deadline_at_ms;
            if before == scope.version {
                if let Some(reason) = terminal {
                    latch_failure(&mut guard, &self.scope_id, &reason);
                }
                return Ok(result);
            }
            // An observed overrun closes already-granted sibling permits
            // before the storage wait, while the owned worker retains and
            // saves the exact late accounting evidence.
            if let Some(reason) = &terminal {
                latch_failure(&mut guard, &self.scope_id, reason);
            }
            candidate.registry.generation = guard.execution_budgets.next_generation()?;
            (
                candidate,
                guard.control_plane_persist_request(),
                result,
                terminal,
            )
        };
        save_admission(&state, candidate, request)
            .await
            .map_err(|error| error.to_string())?;
        if let Some(reason) = terminal {
            latch_failure(&mut *state.write().await, &self.scope_id, &reason);
        }
        Ok(result)
    }
}

#[async_trait]
impl ExecutionBudgetPort for HostBudgetPort {
    async fn reserve(
        &self,
        scope_id: &str,
        request: ReservationRequest,
    ) -> Result<ReservationAdmission, String> {
        self.check_scope(scope_id)?;
        if request.run_id != self.run_id {
            return Err("Execution reservation belongs to another run".into());
        }
        self.check_admission()?;
        self.submit(Mutation::Reserve(request)).await
    }

    fn submit_settlement(
        &self,
        scope_id: &str,
        settlement: ReservationSettlement,
    ) -> BoxFuture<'static, Result<(), String>> {
        if let Err(error) = self.check_scope(scope_id) {
            return Box::pin(futures::future::ready(Err(error)));
        }
        let receipt = self.submit(Mutation::Settle(settlement));
        Box::pin(async move { receipt.await.map(|_| ()) })
    }

    async fn allowance(&self, scope_id: &str) -> Result<ExecutionAllowance, String> {
        self.check_scope(scope_id)?;
        self.check_admission()?;
        self.submit(Mutation::Refresh).await?;
        let state = self
            .state
            .upgrade()
            .ok_or("Execution host is unavailable")?;
        let (owner, result) = {
            let guard = state.read().await;
            guard
                .ensure_profile_owned()
                .map_err(|error| error.to_string())?;
            if guard.execution_budgets.run_scopes.get(&self.run_id) != Some(&self.scope_id) {
                return Err("Execution run binding changed or is unavailable".into());
            }
            (
                guard.profile_owner(),
                guard
                    .execution_budgets
                    .scopes
                    .get(&self.scope_id)
                    .ok_or("Execution scope is unavailable")?
                    .allowance(anima_core::primitives::now_millis()),
            )
        };
        if let Some(owner) = owner {
            owner
                .verify_owned()
                .await
                .map_err(|error| error.to_string())?;
        }
        if let Err(reason) = &result {
            latch_failure(&mut *state.write().await, &self.scope_id, reason);
        }
        self.check_admission()?;
        result
    }
}
