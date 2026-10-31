use super::*;

fn scope(bytes: u64) -> ExecutionScope {
    let mut limits = ExecutionLimits::default();
    limits.budget.max_artifact_bytes = Some(bytes);
    ExecutionScope::new("native-scope".into(), "root".into(), limits, 100).unwrap()
}
fn request(id: &str, bytes: u64) -> ReservationRequest {
    ReservationRequest {
        id: id.into(),
        run_id: "root".into(),
        logical_call_id: id.into(),
        operation: ExecutionOperation::NativeArtifact {
            max_artifact_bytes: bytes,
        },
        input_tokens: 0,
        output_tokens: 0,
    }
}
fn settlement(id: &str, outcome: SettlementOutcome, retired: bool) -> ReservationSettlement {
    ReservationSettlement {
        reservation_id: id.into(),
        outcome,
        physical_retired: retired,
        observed_usage: None,
    }
}

#[test]
fn native_artifact_pending_unknown_and_unretired_observations_keep_the_upper_bound() {
    let mut scope = scope(1000);
    scope.reserve(request("first", 600), 101).unwrap();
    assert!(scope.reserve(request("too-many", 500), 101).is_err());
    scope
        .settle(settlement(
            "first",
            SettlementOutcome::Unknown { observed: None },
            false,
        ))
        .unwrap();
    assert_eq!(scope.consumption().unwrap().charged_artifact_bytes, 600);
    scope
        .settle(settlement(
            "first",
            SettlementOutcome::Artifact {
                artifact_bytes: 200,
            },
            false,
        ))
        .unwrap();
    assert_eq!(scope.consumption().unwrap().charged_artifact_bytes, 600);
    assert!(scope.reserve(request("still-too-many", 500), 101).is_err());
    scope
        .settle(settlement(
            "first",
            SettlementOutcome::Artifact {
                artifact_bytes: 200,
            },
            true,
        ))
        .unwrap();
    assert_eq!(scope.consumption().unwrap().charged_artifact_bytes, 200);
    scope.reserve(request("second", 500), 101).unwrap();
    assert_eq!(scope.consumption().unwrap().charged_artifact_bytes, 700);
    assert_eq!(scope.consumption().unwrap().tool_calls, 2);
}

#[test]
fn native_artifact_known_overrun_is_chargeable_and_survives_restore() {
    let mut scope = scope(1000);
    scope.reserve(request("first", 600), 101).unwrap();
    scope
        .settle(settlement(
            "first",
            SettlementOutcome::Artifact {
                artifact_bytes: 1200,
            },
            true,
        ))
        .unwrap();
    assert_eq!(scope.consumption().unwrap().charged_artifact_bytes, 1200);
    assert!(scope.reserve(request("after-overrun", 1), 101).is_err());
    let restored: ExecutionScope =
        serde_json::from_slice(&serde_json::to_vec(&scope).unwrap()).unwrap();
    assert_eq!(restored.consumption().unwrap().charged_artifact_bytes, 1200);
    assert!(restored.allowance(101).is_err());
}

#[test]
fn native_artifact_zero_output_requires_retirement_and_never_refunds_a_capability_attempt() {
    let mut scope = scope(600);
    scope.limits.budget.max_capability_steps = Some(1);
    scope.reserve(request("first", 600), 101).unwrap();
    scope
        .settle(settlement(
            "first",
            SettlementOutcome::ProvenNeverSent,
            false,
        ))
        .unwrap();
    assert_eq!(scope.consumption().unwrap().charged_artifact_bytes, 600);
    scope
        .settle(settlement(
            "first",
            SettlementOutcome::ProvenNeverSent,
            true,
        ))
        .unwrap();
    assert_eq!(scope.consumption().unwrap().charged_artifact_bytes, 0);
    assert!(scope.reserve(request("second", 1), 101).is_err());
}

#[test]
fn native_artifact_restore_rejects_cross_typed_usage_and_preserves_old_tool_wire() {
    let mut scope = scope(1000);
    scope.reserve(request("first", 600), 101).unwrap();
    assert!(scope
        .settle(settlement(
            "first",
            SettlementOutcome::Known(TokenUsage::default()),
            true
        ))
        .is_err());
    let mut old_tool = request("tool", 600);
    old_tool.operation = ExecutionOperation::Tool;
    let wire = serde_json::to_value(&old_tool).unwrap();
    assert_eq!(wire["operation"], "tool");
    let old_tool: ReservationRequest = serde_json::from_value(wire).unwrap();
    scope.reserve(old_tool, 101).unwrap();
    assert!(scope
        .settle(settlement(
            "tool",
            SettlementOutcome::Artifact { artifact_bytes: 0 },
            true
        ))
        .is_err());
    let mut forged = serde_json::to_value(&scope).unwrap();
    forged["reservations"]["first"]["request"]["operation"] = serde_json::json!("modelAttempt");
    assert!(serde_json::from_value::<ExecutionScope>(forged).is_err());
    assert!(request("zero", 0).validate().is_err());
    assert_eq!(
        serde_json::to_value(request("native", 600)).unwrap()["operation"],
        serde_json::json!({"nativeArtifact":{"maxArtifactBytes":600}})
    );
    assert!(serde_json::from_value::<ExecutionOperation>(
        serde_json::json!({"nativeArtifact":{"maxArtifactBytes":600,"reset":true}})
    )
    .is_err());
}

#[test]
fn native_artifact_restore_rejects_nested_model_observations() {
    for outcome in [
        SettlementOutcome::Artifact {
            artifact_bytes: 200,
        },
        SettlementOutcome::Unknown { observed: None },
    ] {
        let mut scope = scope(1000);
        scope.reserve(request("first", 600), 101).unwrap();
        scope.settle(settlement("first", outcome, false)).unwrap();
        let mut forged = serde_json::to_value(&scope).unwrap();
        forged["reservations"]["first"]["settlement"]["observedUsage"] =
            serde_json::to_value(TokenUsage {
                prompt_tokens: 7,
                total_tokens: 7,
                ..TokenUsage::default()
            })
            .unwrap();
        assert!(forged["reservations"]["first"]
            .get("observedUsage")
            .is_none());
        assert!(
            serde_json::from_value::<ExecutionScope>(forged).is_err(),
            "a nested model observation cannot be silently ignored on native restore"
        );
    }
}

struct Clock(std::sync::atomic::AtomicU64);
#[async_trait::async_trait]
impl crate::ExecutionClock for Clock {
    fn now_ms(&self) -> u64 {
        self.0.load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[tokio::test]
async fn native_artifact_context_uses_existing_deadline_and_drop_cannot_refund_unknown_work() {
    use std::sync::{Arc, Mutex};
    let scope = scope(1000);
    let deadline = scope.deadline_at_ms;
    let clock = Arc::new(Clock(std::sync::atomic::AtomicU64::new(100)));
    let port = Arc::new(InMemoryExecutionBudget::new(scope, clock.clone()).unwrap());
    let execution = ExecutionContext {
        scope_id: "native-scope".into(),
        run_id: "root".into(),
        port: port.clone(),
        clock: clock.clone(),
        cancel: crate::CancelSignal::new(),
        deadline_at_ms: deadline,
        failures: Arc::new(Mutex::new(None)),
        secondary_cancel: None,
    };
    assert_eq!(execution.remaining_wall_time_ms().unwrap(), deadline - 100);
    let permit = execution
        .reserve_native_artifact("first", 600)
        .await
        .unwrap();
    clock
        .0
        .store(deadline - 1, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(execution.remaining_wall_time_ms().unwrap(), 1);
    clock.0.store(deadline, std::sync::atomic::Ordering::SeqCst);
    assert!(execution
        .remaining_wall_time_ms()
        .unwrap_err()
        .contains("deadline"));
    drop(permit);
    let snapshot = port.snapshot("native-scope").unwrap();
    assert_eq!(snapshot.consumption().unwrap().charged_artifact_bytes, 600);
    assert_eq!(snapshot.consumption().unwrap().inflight_operations, 1);
    let saved = snapshot
        .reservations
        .values()
        .next()
        .unwrap()
        .settlement
        .as_ref()
        .unwrap();
    assert!(!saved.physical_retired);
    assert!(matches!(saved.outcome, SettlementOutcome::Unknown { .. }));
}
