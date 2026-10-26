use anima_core::{execution_limits::*, *};
use anima_execution_store_sqlite::SqliteExecutionStore;
use anima_harness::durable::*;
use anima_memory::{IdentityRegistry, MemoryAuthority};
use anima_memory_store_sqlite::SqliteMemoryStore;
use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use uuid::Uuid;
mod common;
use common::*;

struct Ready(bool);
#[async_trait]
impl MemoryProjectionReadiness for Ready {
    async fn ensure_projected(&self, _: &IdentityBinding) -> Result<(), MemoryPortError> {
        if self.0 {
            Ok(())
        } else {
            Err(MemoryPortError::unsupported(
                "accepted memory effects still pending",
            ))
        }
    }
}

#[tokio::test]
async fn required_preference_is_scoped_and_recalled_in_a_new_session_after_reopen() {
    let temp = tempfile::tempdir().unwrap();
    let clock = Arc::new(ManualExecutionClock::default());
    let execution_store = Arc::new(
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
    let path = temp.path().join("memory.sqlite");
    let memory = SqliteMemoryStore::open(&path, clock.clone()).await.unwrap();
    let owner = Uuid::new_v4();
    let identity = IdentityBinding::new(owner.to_string(), "assistant").unwrap();
    memory.register_identity(identity.clone()).await.unwrap();
    let scope = MemoryScope::agent("assistant").unwrap();
    let authority =
        MemoryAuthority::trusted(identity.clone(), vec![scope.clone()], vec![scope.clone()])
            .unwrap();
    let bound = memory.bind(authority.clone()).await.unwrap();
    bound
        .write(
            MemoryWrite::new(
                "preference",
                MemoryKind::Preference,
                scope,
                "likes jasmine tea",
                1.0,
                MemoryProvenance::new("user-statement", "accepted-event-1", clock.now_ms())
                    .unwrap(),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    memory.shutdown().await.unwrap();
    let memory = SqliteMemoryStore::open(&path, clock.clone()).await.unwrap();
    let bound = Arc::new(memory.bind(authority.clone()).await.unwrap());
    let persistent =
        PersistentAgentMemory::new(authority.clone(), bound.clone(), Arc::new(Ready(true)));
    let mut def = definition();
    def.memory.enabled = true;
    let calls = Arc::new(Mutex::new(vec![]));
    let harness = DurableHarness::new(
        execution_store.clone(),
        Arc::new(Model(calls.clone())),
        Arc::new(Definitions(def.clone())),
        Arc::new(Policy),
        Arc::new(CapabilityRegistry::new(ManifestCatalog::default())),
        clock.clone(),
        Arc::new(Live),
    );
    let run = Uuid::new_v4();
    let binding = DurableRunBinding::new(
        owner,
        identity.clone(),
        Uuid::new_v4(),
        run,
        ApplicationScope::new(owner, Uuid::new_v4()).unwrap(),
    )
    .unwrap();
    let budget = ExecutionScope::new(
        "persistent-run".into(),
        run.to_string(),
        ExecutionLimits::default(),
        clock.now_ms(),
    )
    .unwrap();
    let port = execution_store
        .register_budget(owner, budget.clone())
        .await
        .unwrap();
    let control = DurableControl::new();
    let execution = control
        .execution_context(&budget, run, Arc::new(port), clock)
        .unwrap();
    assert!(harness
        .admit(&binding, &def, "what tea do I like?", &execution)
        .await
        .is_err());
    let blocked = PersistentAgentMemory::new(authority, bound, Arc::new(Ready(false)));
    assert!(harness
        .admit_persistent(&binding, &def, "what tea do I like?", &execution, &blocked)
        .await
        .is_err());
    harness
        .admit_persistent(
            &binding,
            &def,
            "what tea do I like?",
            &execution,
            &persistent,
        )
        .await
        .unwrap();
    assert!(harness.run(&binding, &control, &execution).await.is_err());
    harness
        .run_persistent(&binding, &control, &execution, &persistent)
        .await
        .unwrap();
    assert!(calls.lock().unwrap()[0]
        .system
        .contains("likes jasmine tea"));
    assert!(calls.lock().unwrap()[0].system.contains("accepted-event-1"));
    memory.shutdown().await.unwrap();
    // Exact completed replay performs no new memory read or provider dispatch.
    assert!(matches!(
        harness
            .run_persistent(&binding, &control, &execution, &persistent)
            .await
            .unwrap(),
        EngineRunOutcome::Completed { .. }
    ));
    assert_eq!(calls.lock().unwrap().len(), 1);
    execution_store.shutdown().await.unwrap();
}
