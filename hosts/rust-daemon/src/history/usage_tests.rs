//! Regression coverage for durable secondary usage custody.

use super::*;
use crate::control_plane_store::{load_control_plane_snapshot, ControlPlaneStoreConfig};
use crate::history::conformance::FlakyHistoryStore;
use anima_core::primitives::now_millis;
use tokio::sync::RwLock;

fn row(id: &str) -> UsageRecord {
    crate::usage::usage_record(
        &crate::usage::UsageCall {
            id: id.into(),
            agent_id: "deleted-agent".into(),
            session_id: Some("deleted-session".into()),
            run_id: None,
            source: crate::usage::UsageSource::Title,
            provider: "test".into(),
            model: "fixture".into(),
            duration_ms: 1,
            created_at_ms: 1,
        },
        &anima_core::TokenUsage {
            prompt_tokens: 2,
            completion_tokens: 1,
            total_tokens: 3,
            ..Default::default()
        },
        &[],
    )
}

fn fixture() -> (
    SharedDaemonState,
    SharedHistory,
    Arc<Mutex<()>>,
    ControlPlaneStoreConfig,
) {
    let path =
        std::env::temp_dir().join(format!("anima-usage-custody-{}.json", uuid::Uuid::new_v4()));
    let config = ControlPlaneStoreConfig::Json(path);
    let history = HistoryService::new(Arc::new(FlakyHistoryStore::new()));
    let mut daemon = DaemonState::new();
    daemon.set_control_plane_store(Some(config.clone()));
    daemon.set_history(history.clone());
    (
        Arc::new(RwLock::new(daemon)),
        history,
        Arc::new(Mutex::new(())),
        config,
    )
}

#[tokio::test]
async fn durable_enqueue_is_acknowledged_only_after_snapshot_save() {
    let (state, history, transactions, config) = fixture();
    let gate = state
        .write()
        .await
        .install_test_control_plane_save_gate(false);
    let writer = {
        let (state, history, transactions) = (state.clone(), history.clone(), transactions.clone());
        tokio::spawn(async move {
            history
                .enqueue_usage_durable(&state, transactions, vec![row("pending")])
                .await
        })
    };
    let permit = tokio::time::timeout(Duration::from_secs(5), gate.entered.acquire())
        .await
        .unwrap()
        .unwrap();
    permit.forget();
    assert!(
        !writer.is_finished(),
        "unacknowledged storage cannot be reported as durable"
    );
    gate.release.add_permits(1);
    assert_eq!(writer.await.unwrap().unwrap(), UsageAcceptance::Durable);
    let saved = load_control_plane_snapshot(&config).await.unwrap().unwrap();
    assert_eq!(saved.pending_usage, [row("pending")]);
    if let Some(path) = config.file_path() {
        let _ = std::fs::remove_file(path);
    }
}

#[tokio::test]
async fn failed_enqueue_rolls_back_only_new_rows_and_reports_failure() {
    let (state, history, transactions, config) = fixture();
    history
        .enqueue_usage_durable(&state, transactions.clone(), vec![row("accepted")])
        .await
        .unwrap();
    let gate = state
        .write()
        .await
        .install_test_control_plane_save_gate(true);
    gate.release.add_permits(1);
    let error = history
        .enqueue_usage_durable(&state, transactions, vec![row("accepted"), row("rejected")])
        .await
        .unwrap_err();
    assert!(error.to_string().contains("usage"));
    assert_eq!(history.pending_usage(), [row("accepted")]);
    assert!(history.stats().last_error.is_some());
    assert_eq!(
        load_control_plane_snapshot(&config)
            .await
            .unwrap()
            .unwrap()
            .pending_usage,
        [row("accepted")]
    );
    if let Some(path) = config.file_path() {
        let _ = std::fs::remove_file(path);
    }
}

#[tokio::test]
async fn uncertain_enqueue_keeps_possibly_saved_usage_for_idempotent_retry() {
    let (state, history, transactions, config) = fixture();
    let gate = state
        .write()
        .await
        .install_test_control_plane_uncertain_save_gate();
    gate.release.add_permits(1);
    assert!(history
        .enqueue_usage_durable(&state, transactions.clone(), vec![row("uncertain")])
        .await
        .is_err());
    assert_eq!(history.pending_usage(), [row("uncertain")]);
    assert_eq!(
        load_control_plane_snapshot(&config)
            .await
            .unwrap()
            .unwrap()
            .pending_usage,
        [row("uncertain")]
    );
    history
        .flush_once(&state, &transactions, now_millis())
        .await
        .unwrap();
    assert!(history.pending_usage().is_empty());
    if let Some(path) = config.file_path() {
        let _ = std::fs::remove_file(path);
    }
}

#[tokio::test]
async fn failed_usage_ack_keeps_pending_rows_until_a_saved_idempotent_retry() {
    let (state, history, transactions, config) = fixture();
    history
        .enqueue_usage_durable(&state, transactions.clone(), vec![row("ack_retry")])
        .await
        .unwrap();
    let gate = state
        .write()
        .await
        .install_test_control_plane_save_gate(true);
    gate.release.add_permits(1);
    assert!(history
        .flush_once(&state, &transactions, now_millis())
        .await
        .is_err());
    assert_eq!(history.pending_usage(), [row("ack_retry")]);
    assert_eq!(
        load_control_plane_snapshot(&config)
            .await
            .unwrap()
            .unwrap()
            .pending_usage,
        [row("ack_retry")]
    );
    history
        .flush_once(&state, &transactions, now_millis())
        .await
        .unwrap();
    assert!(history.pending_usage().is_empty());
    assert!(load_control_plane_snapshot(&config)
        .await
        .unwrap()
        .unwrap()
        .pending_usage
        .is_empty());
    let rows = history
        .store()
        .page_usage(&crate::history::UsagePageQuery {
            agent_id: Some("deleted-agent".into()),
            session_id: None,
            from_ms: 0,
            to_ms: 100,
            before: None,
            limit: 20,
        })
        .await
        .unwrap();
    assert_eq!(rows, [row("ack_retry")]);
    if let Some(path) = config.file_path() {
        let _ = std::fs::remove_file(path);
    }
}

#[tokio::test]
async fn ephemeral_enqueue_reports_ephemeral_custody() {
    let state = Arc::new(RwLock::new(DaemonState::new()));
    let history = state.read().await.history.clone();
    assert_eq!(
        history
            .enqueue_usage_durable(&state, Arc::new(Mutex::new(())), vec![row("ephemeral")])
            .await
            .unwrap(),
        UsageAcceptance::Ephemeral
    );
    assert_eq!(history.pending_usage(), [row("ephemeral")]);
}

#[tokio::test]
async fn late_enqueue_during_history_upsert_keeps_its_own_saved_custody() {
    let (state, _, transactions, config) = fixture();
    let store = Arc::new(FlakyHistoryStore::new());
    let history = HistoryService::new(store.clone());
    state.write().await.set_history(history.clone());
    history
        .enqueue_usage_durable(&state, transactions.clone(), vec![row("first")])
        .await
        .unwrap();
    let gate = store.hold_next_usage_write();
    let flushing = {
        let (state, history, transactions) = (state.clone(), history.clone(), transactions.clone());
        tokio::spawn(async move {
            history
                .flush_once(&state, &transactions, now_millis())
                .await
        })
    };
    let entered = tokio::time::timeout(Duration::from_secs(5), gate.entered.acquire())
        .await
        .unwrap()
        .unwrap();
    entered.forget();
    tokio::time::timeout(
        Duration::from_secs(5),
        history.enqueue_usage_durable(&state, transactions, vec![row("late")]),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(
        load_control_plane_snapshot(&config)
            .await
            .unwrap()
            .unwrap()
            .pending_usage,
        [row("first"), row("late")]
    );
    gate.release.add_permits(1);
    assert_eq!(flushing.await.unwrap().unwrap().usage, 2);
    assert!(history.pending_usage().is_empty());
    assert!(load_control_plane_snapshot(&config)
        .await
        .unwrap()
        .unwrap()
        .pending_usage
        .is_empty());
    assert_eq!(store.usage_batches(), [1, 1]);
    if let Some(path) = config.file_path() {
        let _ = std::fs::remove_file(path);
    }
}

#[tokio::test]
async fn cancelled_usage_waiter_does_not_cancel_the_owned_save() {
    let (state, history, transactions, config) = fixture();
    let gate = state
        .write()
        .await
        .install_test_control_plane_save_gate(false);
    let waiter = {
        let (state, history, transactions) = (state.clone(), history.clone(), transactions.clone());
        tokio::spawn(async move {
            history
                .enqueue_usage_durable(&state, transactions, vec![row("cancelled_waiter")])
                .await
        })
    };
    let entered = tokio::time::timeout(Duration::from_secs(5), gate.entered.acquire())
        .await
        .unwrap()
        .unwrap();
    entered.forget();
    waiter.abort();
    assert!(waiter.await.unwrap_err().is_cancelled());
    gate.release.add_permits(1);
    let _finished = tokio::time::timeout(Duration::from_secs(5), transactions.lock())
        .await
        .unwrap();
    assert_eq!(
        load_control_plane_snapshot(&config)
            .await
            .unwrap()
            .unwrap()
            .pending_usage,
        [row("cancelled_waiter")]
    );
    if let Some(path) = config.file_path() {
        let _ = std::fs::remove_file(path);
    }
}

#[test]
fn restored_usage_is_bounded_without_requiring_a_live_owner() {
    let history = HistoryService::ephemeral();
    history.restore_usage(vec![row("owner_deleted")]).unwrap();
    assert_eq!(history.pending_usage(), [row("owner_deleted")]);
    assert!(history
        .restore_usage(vec![row("duplicate"), row("duplicate")])
        .is_err());
    let mut invalid = row("oversized");
    invalid.model = "x".repeat(1025);
    assert!(history.restore_usage(vec![invalid]).is_err());
    assert_eq!(history.pending_usage(), [row("owner_deleted")]);
}
