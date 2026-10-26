use anima_core::{execution_limits::*, *};
use async_trait::async_trait;
use std::sync::Arc;

pub(super) struct ControlledCapabilities<R> {
    pub inner: Arc<R>,
    pub execution: ExecutionContext,
}
fn reservation_id(context: &CapabilityExecutionContext) -> String {
    format!(
        "durable-tool:{}:{}",
        context.invocation().id(),
        context.attempt().number()
    )
}
impl<R: EngineCapabilityRuntime> ControlledCapabilities<R> {
    async fn execute_attempt(
        &self,
        context: CapabilityExecutionContext,
        authorization: Option<CapabilityRetryAuthorization>,
    ) -> Result<EngineCapabilityResult, CapabilityError> {
        let mut permit = self
            .execution
            .reserve_tool_with_id(
                &context.invocation().id().to_string(),
                &reservation_id(&context),
            )
            .await
            .map_err(|_| CapabilityError::unavailable())?;
        if permit.revalidate().await.is_err() {
            permit
                .finish_owned(SettlementOutcome::ProvenNeverSent, true)
                .await
                .map_err(|_| CapabilityError::unavailable())?;
            return Err(CapabilityError::cancelled());
        }
        let operation = async {
            match authorization {
                Some(authorization) => self.inner.execute_retry(context, authorization).await,
                None => self.inner.execute(context).await,
            }
        };
        let result = match self.execution.until_retired(operation, None).await {
            Ok(result) => result,
            Err(_) => {
                permit
                    .finish_owned(SettlementOutcome::ProvenNeverSent, true)
                    .await
                    .map_err(|_| CapabilityError::unavailable())?;
                return Err(CapabilityError::cancelled());
            }
        };
        let outcome = if result.is_ok() {
            SettlementOutcome::Known(TokenUsage::default())
        } else {
            SettlementOutcome::Unknown { observed: None }
        };
        // A failed transport/executor call does not prove that a remote physical
        // operation retired. Reconciliation owns that proof after restart too.
        permit
            .finish_owned(outcome, result.is_ok())
            .await
            .map_err(|_| CapabilityError::unavailable())?;
        result
    }
}
#[async_trait]
impl<R: EngineCapabilityRuntime> EngineCapabilityRuntime for ControlledCapabilities<R> {
    fn manifest(&self, id: &str, version: u32) -> Option<CapabilityManifest> {
        self.inner.manifest(id, version)
    }
    async fn execute(
        &self,
        context: CapabilityExecutionContext,
    ) -> Result<EngineCapabilityResult, CapabilityError> {
        self.execute_attempt(context, None).await
    }
    async fn recover_dispatched(
        &self,
        context: CapabilityExecutionContext,
    ) -> Result<RecoveryAction, CapabilityError> {
        let id = reservation_id(&context);
        let invocation_id = context.invocation().id().to_string();
        let recovered = self.inner.recover_dispatched(context).await?;
        if matches!(
            recovered,
            RecoveryAction::Completed(_)
                | RecoveryAction::AuthoritativeAbsence
                | RecoveryAction::RetrySameKey { .. }
        ) {
            match self
                .execution
                .port
                .reservation(&self.execution.scope_id, &id)
                .await
                .map_err(|_| CapabilityError::reconciliation())?
            {
                Some(record) => {
                    if record.request.run_id != self.execution.run_id
                        || record.request.logical_call_id != invocation_id
                        || record.request.operation != ExecutionOperation::Tool
                    {
                        return Err(CapabilityError::reconciliation());
                    }
                    let never_sent = record.settlement.as_ref().is_some_and(|settlement| {
                        matches!(settlement.outcome, SettlementOutcome::ProvenNeverSent)
                    });
                    if never_sent && matches!(recovered, RecoveryAction::Completed(_)) {
                        // Contradictory evidence is not authority to rewrite a
                        // historical proof that the executor was never entered.
                        return Err(CapabilityError::reconciliation());
                    }
                    self.execution
                        .port
                        .submit_settlement(
                            &self.execution.scope_id,
                            ReservationSettlement {
                                reservation_id: id,
                                outcome: if never_sent {
                                    SettlementOutcome::ProvenNeverSent
                                } else {
                                    SettlementOutcome::Known(TokenUsage::default())
                                },
                                physical_retired: true,
                                observed_usage: None,
                            },
                        )
                        .await
                        .map_err(|_| CapabilityError::reconciliation())?;
                }
                None if matches!(recovered, RecoveryAction::Completed(_)) => {
                    return Err(CapabilityError::reconciliation())
                }
                // A dispatch checkpoint can precede budget reservation/executor
                // entry. Proven absence needs no invented historical charge.
                None => {}
            }
        }
        Ok(recovered)
    }
    async fn completed_output(
        &self,
        context: &CapabilityExecutionContext,
        durable: &DurableCapabilityResult,
    ) -> Result<CapabilityResult, CapabilityError> {
        self.inner.completed_output(context, durable).await
    }
    async fn execute_retry(
        &self,
        context: CapabilityExecutionContext,
        authorization: CapabilityRetryAuthorization,
    ) -> Result<EngineCapabilityResult, CapabilityError> {
        self.execute_attempt(context, Some(authorization)).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        future::Future,
        pin::Pin,
        sync::atomic::{AtomicBool, Ordering},
        task::Poll,
    };
    use uuid::Uuid;
    struct TestBudget {
        inner: Arc<InMemoryExecutionBudget>,
        fail_allowance_once: AtomicBool,
        fail_settlement_once: AtomicBool,
        delay: Option<(Arc<tokio::sync::Notify>, crate::durable::DurableControl)>,
    }
    #[async_trait]
    impl ExecutionBudgetPort for TestBudget {
        async fn reserve(
            &self,
            id: &str,
            request: ReservationRequest,
        ) -> Result<ReservationAdmission, String> {
            self.inner.reserve(id, request).await
        }
        async fn reservation(
            &self,
            id: &str,
            key: &str,
        ) -> Result<Option<ExecutionReservation>, String> {
            self.inner.reservation(id, key).await
        }
        async fn allowance(&self, id: &str) -> Result<ExecutionAllowance, String> {
            if self.fail_allowance_once.swap(false, Ordering::SeqCst) {
                return Err("fixture revalidation failure".into());
            }
            self.inner.allowance(id).await
        }
        fn submit_settlement(
            &self,
            id: &str,
            settlement: ReservationSettlement,
        ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'static>> {
            if self.fail_settlement_once.swap(false, Ordering::SeqCst) {
                return Box::pin(async { Err("fixture durable settlement failed".into()) });
            }
            if let Some((release, control)) = &self.delay {
                control.stop();
                let inner = self.inner.clone();
                let id = id.to_owned();
                let release = release.clone();
                let (done, finished) = tokio::sync::oneshot::channel();
                // The write owns its custody before the acknowledgement is returned.
                tokio::spawn(async move {
                    release.notified().await;
                    let result = inner.submit_settlement(&id, settlement).await;
                    let _ = done.send(result);
                });
                return Box::pin(async move {
                    finished.await.map_err(|_| "fixture receipt unavailable")?
                });
            }
            self.inner.submit_settlement(id, settlement)
        }
    }
    enum Reconciler {
        Completed,
        Absent,
    }
    #[async_trait]
    impl EngineCapabilityRuntime for Reconciler {
        fn manifest(&self, _: &str, _: u32) -> Option<CapabilityManifest> {
            None
        }
        async fn execute(
            &self,
            _: CapabilityExecutionContext,
        ) -> Result<EngineCapabilityResult, CapabilityError> {
            assert!(
                matches!(self, Self::Completed),
                "revalidation must prevent executor entry"
            );
            Ok(EngineCapabilityResult {
                output: CapabilityResult::new(serde_json::json!({"ok":true})),
                durable: durable(),
            })
        }
        async fn execute_retry(
            &self,
            _: CapabilityExecutionContext,
            _: CapabilityRetryAuthorization,
        ) -> Result<EngineCapabilityResult, CapabilityError> {
            unreachable!()
        }
        async fn recover_dispatched(
            &self,
            _: CapabilityExecutionContext,
        ) -> Result<RecoveryAction, CapabilityError> {
            Ok(match self {
                Self::Completed => RecoveryAction::Completed(durable()),
                Self::Absent => RecoveryAction::AuthoritativeAbsence,
            })
        }
    }
    fn context(run: Uuid) -> CapabilityExecutionContext {
        let invocation =
            LogicalInvocation::new(run, "tool-step", "bounded", 1, serde_json::json!({})).unwrap();
        let attempt = CapabilityAttempt::new(&invocation, 1).unwrap();
        CapabilityExecutionContext::for_attempt(invocation, attempt).unwrap()
    }
    fn fixture(
        fail_allowance: bool,
        fail_settlement: bool,
        delay: Option<(Arc<tokio::sync::Notify>, crate::durable::DurableControl)>,
    ) -> (
        ExecutionContext,
        Arc<TestBudget>,
        CapabilityExecutionContext,
    ) {
        let clock = Arc::new(ManualExecutionClock::default());
        let run = Uuid::new_v4();
        let scope = ExecutionScope::new(
            "scope".into(),
            run.to_string(),
            ExecutionLimits::default(),
            clock.now_ms(),
        )
        .unwrap();
        let inner = Arc::new(InMemoryExecutionBudget::new(scope.clone(), clock.clone()).unwrap());
        let port = Arc::new(TestBudget {
            inner,
            fail_allowance_once: AtomicBool::new(fail_allowance),
            fail_settlement_once: AtomicBool::new(fail_settlement),
            delay,
        });
        let control = port
            .delay
            .as_ref()
            .map(|(_, control)| control.clone())
            .unwrap_or_default();
        let execution = control
            .execution_context(&scope, run, port.clone(), clock)
            .unwrap();
        (execution, port, context(run))
    }
    #[tokio::test]
    async fn proven_never_sent_reservation_survives_authoritative_absence_recovery() {
        let (execution, port, context) = fixture(true, false, None);
        let capabilities = ControlledCapabilities {
            inner: Arc::new(Reconciler::Absent),
            execution,
        };
        assert!(capabilities.execute(context.clone()).await.is_err());
        assert!(matches!(
            capabilities
                .recover_dispatched(context.clone())
                .await
                .unwrap(),
            RecoveryAction::AuthoritativeAbsence
        ));
        let record = port
            .reservation("scope", &reservation_id(&context))
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            record.settlement.unwrap().outcome,
            SettlementOutcome::ProvenNeverSent
        ));
        assert_eq!(
            port.inner
                .snapshot("scope")
                .unwrap()
                .consumption()
                .unwrap()
                .inflight_operations,
            0
        );
    }
    #[tokio::test]
    async fn failed_settlement_cannot_publish_success_and_recovery_retires_existing_charge() {
        let (execution, port, context) = fixture(false, true, None);
        let capabilities = ControlledCapabilities {
            inner: Arc::new(Reconciler::Completed),
            execution,
        };
        assert!(
            capabilities.execute(context.clone()).await.is_err(),
            "success must wait for durable accounting"
        );
        assert_eq!(
            port.inner
                .snapshot("scope")
                .unwrap()
                .consumption()
                .unwrap()
                .inflight_operations,
            1
        );
        assert!(matches!(
            capabilities.recover_dispatched(context).await.unwrap(),
            RecoveryAction::Completed(_)
        ));
        assert_eq!(
            port.inner
                .snapshot("scope")
                .unwrap()
                .consumption()
                .unwrap()
                .inflight_operations,
            0
        );
    }
    #[tokio::test]
    async fn stop_during_owned_settlement_waits_for_its_actual_receipt() {
        let release = Arc::new(tokio::sync::Notify::new());
        let control = crate::durable::DurableControl::new();
        let (execution, port, context) = fixture(false, false, Some((release.clone(), control)));
        let capabilities = ControlledCapabilities {
            inner: Arc::new(Reconciler::Completed),
            execution,
        };
        let mut work = Box::pin(capabilities.execute(context));
        let pending =
            std::future::poll_fn(|cx| Poll::Ready(work.as_mut().poll(cx).is_pending())).await;
        release.notify_one();
        assert!(
            pending,
            "Stop must not acknowledge successful completion before accounting settles"
        );
        assert!(work.await.is_ok());
        assert_eq!(
            port.inner
                .snapshot("scope")
                .unwrap()
                .consumption()
                .unwrap()
                .inflight_operations,
            0
        );
    }
    struct UnknownThenCompleted;
    fn durable() -> DurableCapabilityResult {
        DurableCapabilityResult::new(
            CapabilityReferenceId::new(Uuid::from_u128(91)),
            format!("jcs-v1:{}", "a".repeat(64)),
            format!("sha256:{}", "b".repeat(64)),
            1,
            DurableCapabilityStatus::Completed,
        )
        .unwrap()
    }
    #[async_trait]
    impl EngineCapabilityRuntime for UnknownThenCompleted {
        fn manifest(&self, _: &str, _: u32) -> Option<CapabilityManifest> {
            None
        }
        async fn execute(
            &self,
            _: CapabilityExecutionContext,
        ) -> Result<EngineCapabilityResult, CapabilityError> {
            Err(CapabilityError::execution())
        }
        async fn execute_retry(
            &self,
            _: CapabilityExecutionContext,
            _: CapabilityRetryAuthorization,
        ) -> Result<EngineCapabilityResult, CapabilityError> {
            unreachable!()
        }
        async fn recover_dispatched(
            &self,
            _: CapabilityExecutionContext,
        ) -> Result<RecoveryAction, CapabilityError> {
            Ok(RecoveryAction::Completed(durable()))
        }
    }
    #[tokio::test]
    async fn uncertain_tool_keeps_its_inflight_reservation_until_reconciliation_proves_completion()
    {
        let clock = Arc::new(ManualExecutionClock::default());
        let run = Uuid::new_v4();
        let scope = ExecutionScope::new(
            "scope".into(),
            run.to_string(),
            ExecutionLimits::default(),
            clock.now_ms(),
        )
        .unwrap();
        let port = Arc::new(InMemoryExecutionBudget::new(scope.clone(), clock.clone()).unwrap());
        let control = crate::durable::DurableControl::new();
        let execution = control
            .execution_context(&scope, run, port.clone(), clock)
            .unwrap();
        let invocation =
            LogicalInvocation::new(run, "tool-step", "bounded", 1, serde_json::json!({})).unwrap();
        let attempt = CapabilityAttempt::new(&invocation, 1).unwrap();
        let context = CapabilityExecutionContext::for_attempt(invocation, attempt).unwrap();
        let capabilities = ControlledCapabilities {
            inner: Arc::new(UnknownThenCompleted),
            execution,
        };
        assert!(capabilities.execute(context.clone()).await.is_err());
        assert_eq!(
            port.snapshot("scope")
                .unwrap()
                .consumption()
                .unwrap()
                .inflight_operations,
            1
        );
        assert!(matches!(
            capabilities.recover_dispatched(context).await.unwrap(),
            RecoveryAction::Completed(_)
        ));
        assert_eq!(
            port.snapshot("scope")
                .unwrap()
                .consumption()
                .unwrap()
                .inflight_operations,
            0
        );
    }
}
