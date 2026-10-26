use anima_core::{execution_limits::*, *};
use anima_execution_store_sqlite::SqliteExecutionStore;
use std::sync::Arc;
use uuid::Uuid;
mod common;

#[test]
fn execution_budget_authority_wire_requires_valid_stable_identifiers() {
    let authority = ExecutionBudgetAuthority::new("owner", "store", "scope").unwrap();
    let wire = serde_json::to_value(&authority).unwrap();
    assert_eq!(
        wire,
        serde_json::json!({"ownerId":"owner","storeId":"store","scopeId":"scope"})
    );
    assert_eq!(
        serde_json::from_value::<ExecutionBudgetAuthority>(wire.clone()).unwrap(),
        authority
    );
    for invalid in ["", " padded", "line\nfeed"] {
        let mut changed = wire.clone();
        changed["storeId"] = invalid.into();
        assert!(serde_json::from_value::<ExecutionBudgetAuthority>(changed).is_err());
    }
    assert!(ExecutionBudgetAuthority::new("owner", "x".repeat(257), "scope").is_err());
}

#[tokio::test]
async fn durable_allowance_authority_distinguishes_database_owner_and_scope_across_reopen() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("original.sqlite");
    let clock = Arc::new(ManualExecutionClock::default());
    let owner = Uuid::new_v4();
    let scope = ExecutionScope::new(
        "shared".into(),
        "root".into(),
        ExecutionLimits::default(),
        clock.now_ms(),
    )
    .unwrap();
    let original = SqliteExecutionStore::open(&path, common::protection(), clock.clone())
        .await
        .unwrap();
    let port = original
        .register_budget(owner, scope.clone())
        .await
        .unwrap();
    let authority = port.authority_binding().unwrap();
    assert_eq!(authority.owner_id, owner.to_string());
    assert_eq!(authority.scope_id, "shared");
    assert!(!Uuid::parse_str(&authority.store_id).unwrap().is_nil());
    let other_owner = Uuid::new_v4();
    let foreign = original
        .register_budget(other_owner, scope.clone())
        .await
        .unwrap()
        .authority_binding()
        .unwrap();
    assert_ne!(authority, foreign);
    assert_eq!(authority.store_id, foreign.store_id);
    let mut other_scope = scope.clone();
    other_scope.id = "second-scope".into();
    assert_ne!(
        authority,
        original
            .register_budget(owner, other_scope)
            .await
            .unwrap()
            .authority_binding()
            .unwrap()
    );
    let independent = SqliteExecutionStore::open(
        temp.path().join("independent.sqlite"),
        common::protection(),
        clock.clone(),
    )
    .await
    .unwrap();
    let different = independent
        .register_budget(owner, scope.clone())
        .await
        .unwrap()
        .authority_binding()
        .unwrap();
    assert_ne!(authority, different);
    assert_eq!(authority.owner_id, different.owner_id);
    assert_eq!(authority.scope_id, different.scope_id);
    assert_ne!(authority.store_id, different.store_id);
    original.shutdown().await.unwrap();
    let reopened = SqliteExecutionStore::open(&path, common::protection(), clock.clone())
        .await
        .unwrap();
    assert_eq!(
        authority,
        reopened
            .bind_budget(owner, "shared")
            .await
            .unwrap()
            .authority_binding()
            .unwrap()
    );
    let concurrent = SqliteExecutionStore::open(&path, common::protection(), clock)
        .await
        .unwrap();
    assert_eq!(
        authority,
        concurrent
            .bind_budget(owner, "shared")
            .await
            .unwrap()
            .authority_binding()
            .unwrap()
    );
    reopened.shutdown().await.unwrap();
    concurrent.shutdown().await.unwrap();
    independent.shutdown().await.unwrap();
}

#[tokio::test]
async fn simultaneous_initial_opens_share_one_committed_store_identity() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("simultaneous.sqlite");
    let clock = Arc::new(ManualExecutionClock::default());
    let owner = Uuid::new_v4();
    let (first, second) = tokio::join!(
        SqliteExecutionStore::open(&path, common::protection(), clock.clone()),
        SqliteExecutionStore::open(&path, common::protection(), clock.clone())
    );
    let first = first.unwrap();
    let second = second.unwrap();
    let scope = ExecutionScope::new(
        "scope".into(),
        "root".into(),
        ExecutionLimits::default(),
        clock.now_ms(),
    )
    .unwrap();
    let (a, b) = tokio::join!(
        first.register_budget(owner, scope.clone()),
        second.register_budget(owner, scope)
    );
    assert_eq!(
        a.unwrap().authority_binding(),
        b.unwrap().authority_binding()
    );
    first.shutdown().await.unwrap();
    second.shutdown().await.unwrap();
}

#[tokio::test]
async fn missing_or_invalid_store_identity_is_never_replaced_on_reopen() {
    for damage in [
        "DELETE FROM execution_store_metadata",
        "UPDATE execution_store_metadata SET store_id='not-a-uuid'",
        "UPDATE execution_store_metadata SET store_id='00000000-0000-0000-0000-000000000000'",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("metadata.sqlite");
        let clock = Arc::new(ManualExecutionClock::default());
        let store = SqliteExecutionStore::open(&path, common::protection(), clock.clone())
            .await
            .unwrap();
        let scope = ExecutionScope::new(
            "shared".into(),
            "root".into(),
            ExecutionLimits::default(),
            clock.now_ms(),
        )
        .unwrap();
        store.register_budget(Uuid::new_v4(), scope).await.unwrap();
        store.shutdown().await.unwrap();
        let raw = rusqlite::Connection::open(&path).unwrap();
        let snapshot: Vec<u8> = raw
            .query_row(
                "SELECT snapshot FROM execution_authority WHERE singleton=1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        raw.execute_batch(damage).unwrap();
        assert!(
            SqliteExecutionStore::open(&path, common::protection(), clock)
                .await
                .is_err()
        );
        assert_eq!(
            snapshot,
            raw.query_row::<Vec<u8>, _, _>(
                "SELECT snapshot FROM execution_authority WHERE singleton=1",
                [],
                |row| row.get(0)
            )
            .unwrap()
        );
        assert_eq!(
            raw.query_row::<usize, _, _>("SELECT COUNT(*) FROM execution_allowances", [], |row| {
                row.get(0)
            })
            .unwrap(),
            1
        );
    }
}

#[tokio::test]
async fn shared_physical_attempt_allowance_survives_reopen_and_cannot_be_refilled() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("budget.sqlite");
    let clock = Arc::new(ManualExecutionClock::default());
    let store = SqliteExecutionStore::open(&path, common::protection(), clock.clone())
        .await
        .unwrap();
    let owner = Uuid::new_v4();
    let root = Uuid::new_v4().to_string();
    let mut limits = ExecutionLimits::default();
    limits.max_model_attempts = 1;
    limits.max_retries = 0;
    let scope = ExecutionScope::new("shared".into(), root.clone(), limits, clock.now_ms()).unwrap();
    let port = store.register_budget(owner, scope.clone()).await.unwrap();
    let request = ReservationRequest {
        id: "physical-1".into(),
        run_id: root,
        logical_call_id: "model-1".into(),
        operation: ExecutionOperation::ModelAttempt,
        input_tokens: 100,
        output_tokens: 100,
    };
    assert_eq!(
        port.reserve("shared", request.clone()).await.unwrap(),
        ReservationAdmission::NewlyGranted
    );
    assert!(port.reserve("foreign", request.clone()).await.is_err());
    store.shutdown().await.unwrap();
    let store = SqliteExecutionStore::open(&path, common::protection(), clock)
        .await
        .unwrap();
    let port = store.register_budget(owner, scope.clone()).await.unwrap();
    assert_eq!(
        port.reserve("shared", request.clone()).await.unwrap(),
        ReservationAdmission::AlreadyRecorded
    );
    let mut retry = request;
    retry.id = "physical-2".into();
    retry.run_id = "child-run".into();
    assert!(port.reserve("shared", retry).await.is_err());
    let mut refill = scope;
    refill.deadline_at_ms += 1;
    assert!(store.register_budget(owner, refill).await.is_err());
    assert!(store.bind_budget(Uuid::new_v4(), "shared").await.is_err());
    store.shutdown().await.unwrap();
}

#[tokio::test]
async fn dropping_settlement_receipt_keeps_storage_custody() {
    let temp = tempfile::tempdir().unwrap();
    let clock = Arc::new(ManualExecutionClock::default());
    let store = SqliteExecutionStore::open(
        temp.path().join("budget.sqlite"),
        common::protection(),
        clock.clone(),
    )
    .await
    .unwrap();
    let owner = Uuid::new_v4();
    let scope = ExecutionScope::new(
        "scope".into(),
        "root".into(),
        ExecutionLimits::default(),
        clock.now_ms(),
    )
    .unwrap();
    let port = store.register_budget(owner, scope).await.unwrap();
    port.reserve(
        "scope",
        ReservationRequest {
            id: "physical".into(),
            run_id: "root".into(),
            logical_call_id: "call".into(),
            operation: ExecutionOperation::ModelAttempt,
            input_tokens: 100,
            output_tokens: 100,
        },
    )
    .await
    .unwrap();
    drop(port.submit_settlement(
        "scope",
        ReservationSettlement {
            reservation_id: "physical".into(),
            outcome: SettlementOutcome::Known(TokenUsage {
                prompt_tokens: 3,
                completion_tokens: 2,
                total_tokens: 5,
                ..TokenUsage::default()
            }),
            physical_retired: true,
            observed_usage: None,
        },
    ));
    let scope = port.snapshot().await.unwrap();
    assert_eq!(scope.consumption().unwrap().known.total_tokens, 5);
    assert_eq!(scope.consumption().unwrap().inflight_operations, 0);
    store.shutdown().await.unwrap();
}
