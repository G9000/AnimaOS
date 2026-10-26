use anima_core::{execution_limits::*, *};
use anima_execution_store_sqlite::SqliteExecutionStore;
use anima_harness::durable::*;
use std::sync::{Arc, Mutex};
use uuid::Uuid;
mod common;
use common::*;

#[tokio::test]
async fn dialogue_preserves_persona_parameters_and_replays_exact_output_without_workspace() {
    let temp = tempfile::tempdir().unwrap();
    let clock = Arc::new(ManualExecutionClock::default());
    let store = Arc::new(
        SqliteExecutionStore::open(
            temp.path().join("execution.sqlite"),
            PersistenceProtection::allow_model_payloads(
                PersistenceSnapshotSealKey::new([4; 32]).unwrap(),
                vec![],
            )
            .unwrap(),
            clock.clone(),
        )
        .await
        .unwrap(),
    );
    let owner = Uuid::new_v4();
    let run = Uuid::new_v4();
    let session = Uuid::new_v4();
    let identity = IdentityBinding::new(owner.to_string(), "stable-assistant").unwrap();
    let binding = DurableRunBinding::new(
        owner,
        identity,
        session,
        run,
        ApplicationScope::new(owner, Uuid::new_v4()).unwrap(),
    )
    .unwrap();
    let scope = ExecutionScope::new(
        "shared-allowance".into(),
        run.to_string(),
        ExecutionLimits::default(),
        clock.now_ms(),
    )
    .unwrap();
    let port = store.register_budget(owner, scope.clone()).await.unwrap();
    let control = DurableControl::new();
    let execution = control
        .execution_context(&scope, run, Arc::new(port), clock.clone())
        .unwrap();
    let calls = Arc::new(Mutex::new(vec![]));
    let catalog = ManifestCatalog::default();
    let capabilities = Arc::new(CapabilityRegistry::new(catalog));
    let harness = DurableHarness::new(
        store.clone(),
        Arc::new(Model(calls.clone())),
        Arc::new(Definitions(definition())),
        Arc::new(Policy),
        capabilities,
        clock,
        Arc::new(Live),
    );
    harness
        .admit(&binding, &definition(), "hello", &execution)
        .await
        .unwrap();
    assert!(harness
        .admit(&binding, &definition(), "different", &execution)
        .await
        .is_err());
    let mut changed_identity = binding.clone();
    changed_identity.individual =
        IdentityBinding::new(owner.to_string(), "other-individual").unwrap();
    assert!(harness
        .admit(&changed_identity, &definition(), "hello", &execution)
        .await
        .is_err());
    let mut changed_definition = definition();
    changed_definition.persona = "changed without a new version".into();
    assert!(harness
        .admit(&binding, &changed_definition, "hello", &execution)
        .await
        .is_err());
    assert_eq!(
        harness.run(&binding, &control, &execution).await.unwrap(),
        EngineRunOutcome::Completed {
            content: Content {
                text: "hello".into(),
                ..Content::default()
            }
        }
    );
    assert!(matches!(
        harness.run(&binding, &control, &execution).await.unwrap(),
        EngineRunOutcome::Completed { .. }
    ));
    let replacement = SqliteExecutionStore::open(
        temp.path().join("independent-allowance.sqlite"),
        PersistenceProtection::allow_model_payloads(
            PersistenceSnapshotSealKey::new([4; 32]).unwrap(),
            vec![],
        )
        .unwrap(),
        execution.clock.clone(),
    )
    .await
    .unwrap();
    let replacement_port = replacement
        .register_budget(owner, scope.clone())
        .await
        .unwrap();
    let replacement_context = control
        .execution_context(
            &scope,
            run,
            Arc::new(replacement_port),
            execution.clock.clone(),
        )
        .unwrap();
    assert!(harness
        .run(&binding, &control, &replacement_context)
        .await
        .is_err());
    assert!(harness
        .admit(&binding, &definition(), "hello", &replacement_context)
        .await
        .is_err());
    replacement.shutdown().await.unwrap();
    let mut next = binding.clone();
    next.run = Uuid::new_v4();
    let next_scope = ExecutionScope::new(
        "next-allowance".into(),
        next.run.to_string(),
        ExecutionLimits::default(),
        execution.clock.now_ms(),
    )
    .unwrap();
    let next_port = store
        .register_budget(owner, next_scope.clone())
        .await
        .unwrap();
    let next_execution = control
        .execution_context(
            &next_scope,
            next.run,
            Arc::new(next_port),
            execution.clock.clone(),
        )
        .unwrap();
    harness
        .admit(
            &next,
            &definition(),
            "a second turn in the same session",
            &next_execution,
        )
        .await
        .unwrap();
    let calls = calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].system, "Be gentle.\n\nKeep it concise.");
    assert_eq!(calls[0].temperature, Some(0.2));
    assert!(calls[0].max_tokens.is_some());
    drop(calls);
    store.shutdown().await.unwrap();
}

#[tokio::test]
async fn stop_before_model_dispatch_uses_no_physical_allowance() {
    // The facade controls share the exact cancellation token used by provider
    // reservation; stopping cannot create a fresh per-call scope.
    let clock = Arc::new(ManualExecutionClock::default());
    let run = Uuid::new_v4();
    let scope = ExecutionScope::new(
        "scope".into(),
        run.to_string(),
        ExecutionLimits::default(),
        clock.now_ms(),
    )
    .unwrap();
    let budget = Arc::new(InMemoryExecutionBudget::new(scope.clone(), clock.clone()).unwrap());
    let control = DurableControl::new();
    let execution = control
        .execution_context(&scope, run, budget.clone(), clock)
        .unwrap();
    control.stop();
    assert!(execution.check_live().is_err());
    assert_eq!(
        budget
            .snapshot("scope")
            .unwrap()
            .consumption()
            .unwrap()
            .model_attempts,
        0
    );
}
