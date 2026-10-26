use anima_core::{execution_limits::*, *};
use std::{
    io::{BufRead, BufReader},
    path::Path,
    process::{Command, Stdio},
    sync::{mpsc, Arc},
    time::Duration,
};
mod common;
#[path = "fixtures/durable_recovery_peer.rs"]
mod peer;

#[tokio::test]
#[ignore = "parent-controlled killed process fixture"]
async fn recovery_process() {
    let path = std::env::var("ANIMA_DURABLE_RECOVERY_DIRECTORY").unwrap();
    let retry = std::env::var("ANIMA_DURABLE_RECOVERY_RETRY")
        .ok()
        .as_deref()
        == Some("1");
    peer::execute(Path::new(&path), retry, false, false)
        .await
        .unwrap();
}

fn kill_at(path: &Path, point: &str, retry: bool) {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--ignored",
            "--exact",
            "recovery_process",
            "--nocapture",
            "--quiet",
        ])
        .env("ANIMA_DURABLE_RECOVERY_DIRECTORY", path)
        .env("ANIMA_DURABLE_RECOVERY_POINT", point)
        .env(
            "ANIMA_DURABLE_RECOVERY_RETRY",
            if retry { "1" } else { "0" },
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command.spawn().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (send, recv) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let line = line.unwrap();
            if line.starts_with("READY:") {
                let _ = send.send(line);
                return;
            }
        }
    });
    let ready = recv.recv_timeout(Duration::from_secs(30));
    let _ = child.kill();
    child.wait().unwrap();
    assert_eq!(ready.unwrap(), format!("READY:{point}"));
}
async fn snapshot(path: &Path) -> ExecutionScope {
    let store = peer::open(path, Arc::new(ManualExecutionClock::new(100_000))).await;
    let scope = store
        .bind_budget(peer::OWNER, peer::ALLOWANCE)
        .await
        .unwrap()
        .snapshot()
        .await
        .unwrap();
    assert_eq!(scope.created_at_ms, 1_000);
    assert_eq!(scope.root_run_id, peer::RUN.to_string());
    store.shutdown().await.unwrap();
    scope
}

#[tokio::test]
async fn killed_retry_with_fresh_default_registry_persists_manual_recovery_without_dispatch() {
    for point in ["retry-before-entry", "retry-after-effect"] {
        let temp = tempfile::tempdir().unwrap();
        kill_at(temp.path(), point, true);
        let before = snapshot(temp.path()).await;
        let effects = peer::entries(&temp.path().join("effects.log"));
        let providers = peer::entries(&temp.path().join("providers.log"));
        assert_eq!(effects.len(), usize::from(point == "retry-after-effect"));
        assert_eq!(providers.len(), 1);
        let store = peer::open(temp.path(), Arc::new(ManualExecutionClock::new(100_000))).await;
        let (_, checkpoint) = store
            .load_checkpoint(peer::OWNER, peer::RUN)
            .await
            .unwrap()
            .unwrap();
        assert!(checkpoint
            .attempts()
            .iter()
            .any(|a| a.attempt_number() == 2 && a.state() == AttemptRecordState::Dispatching));
        store.shutdown().await.unwrap();
        assert_eq!(
            peer::execute(temp.path(), true, true, false).await.unwrap(),
            EngineRunOutcome::RecoveryRequired,
            "{point}"
        );
        assert_eq!(peer::entries(&temp.path().join("effects.log")), effects);
        assert_eq!(peer::entries(&temp.path().join("providers.log")), providers);
        assert_eq!(
            snapshot(temp.path()).await,
            before,
            "review cannot retire or refill unproven work"
        );
        let store = peer::open(temp.path(), Arc::new(ManualExecutionClock::new(200_000))).await;
        assert_eq!(
            store
                .load_run(peer::OWNER, peer::RUN)
                .await
                .unwrap()
                .unwrap()
                .run()
                .state(),
            RunState::RecoveryRequired
        );
        let (_, checkpoint) = store
            .load_checkpoint(peer::OWNER, peer::RUN)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(checkpoint.state(), RunState::RecoveryRequired);
        assert!(checkpoint
            .uncertain_invocations()
            .iter()
            .any(|r| r.attempt_number() == 2));
        store.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn killed_provider_retains_original_unknown_allowance_and_explicit_host_pause() {
    for point in ["provider-entered", "provider-final-entered"] {
        let temp = tempfile::tempdir().unwrap();
        kill_at(temp.path(), point, false);
        let before = snapshot(temp.path()).await;
        let usage = before.consumption().unwrap();
        assert_eq!(usage.inflight_operations, 1);
        assert_eq!(
            usage.model_attempts,
            if point == "provider-entered" { 1 } else { 2 }
        );
        assert_eq!(
            usage.known.total_tokens,
            if point == "provider-entered" { 0 } else { 5 }
        );
        assert!(usage.charged_total_tokens > usage.known.total_tokens);
        assert!(before
            .reservations
            .values()
            .any(|r| r.request.operation == ExecutionOperation::ModelAttempt
                && r.settlement.is_none()));
        let providers = peer::entries(&temp.path().join("providers.log"));
        let effects = peer::entries(&temp.path().join("effects.log"));
        // Killing the client is not proof the remote provider retired. The host
        // explicitly pauses instead of releasing/replaying its reservation.
        assert_eq!(
            peer::execute(temp.path(), false, true, true).await.unwrap(),
            EngineRunOutcome::PausedByRequest
        );
        assert_eq!(snapshot(temp.path()).await, before);
        assert_eq!(peer::entries(&temp.path().join("providers.log")), providers);
        assert_eq!(peer::entries(&temp.path().join("effects.log")), effects);
        let store = peer::open(temp.path(), Arc::new(ManualExecutionClock::new(200_000))).await;
        assert_eq!(
            store
                .load_run(peer::OWNER, peer::RUN)
                .await
                .unwrap()
                .unwrap()
                .run()
                .state(),
            RunState::Paused
        );
        store.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn killed_harness_settles_recovered_effect_and_replays_committed_boundaries_exactly() {
    for point in [
        "model-checkpoint",
        "effect-before-receipt",
        "result-committed",
        "terminal-committed",
    ] {
        let temp = tempfile::tempdir().unwrap();
        kill_at(temp.path(), point, false);
        let before = snapshot(temp.path()).await;
        let old = before.consumption().unwrap();
        assert_eq!(
            old.known.total_tokens,
            if point == "terminal-committed" { 10 } else { 5 }
        );
        if point == "effect-before-receipt" {
            let (id, tool) = before
                .reservations
                .iter()
                .find(|(_, r)| r.request.operation == ExecutionOperation::Tool)
                .unwrap();
            assert!(id.starts_with("durable-tool:"));
            assert!(tool.settlement.is_none());
            assert_eq!(old.inflight_operations, 1);
        }
        assert_eq!(
            peer::execute(temp.path(), false, true, false)
                .await
                .unwrap(),
            EngineRunOutcome::Completed {
                content: Content {
                    text: "recovered dialogue".into(),
                    ..Content::default()
                }
            }
        );
        assert_eq!(
            peer::entries(&temp.path().join("effects.log")).len(),
            1,
            "{point}: physical effect replayed"
        );
        assert_eq!(
            peer::entries(&temp.path().join("providers.log")).len(),
            2,
            "{point}: model checkpoint lost"
        );
        let after = snapshot(temp.path()).await;
        assert_eq!(after.deadline_at_ms, before.deadline_at_ms);
        assert_eq!(after.limits, before.limits);
        let usage = after.consumption().unwrap();
        assert_eq!(usage.model_attempts, 2);
        assert_eq!(usage.known.total_tokens, 10);
        assert_eq!(usage.tool_calls, 1);
        assert_eq!(usage.inflight_operations, 0);
        for (id, record) in &before.reservations {
            assert_eq!(after.reservations[id].request, record.request);
        }
        assert!(matches!(
            peer::execute(temp.path(), false, true, false)
                .await
                .unwrap(),
            EngineRunOutcome::Completed { .. }
        ));
        assert_eq!(
            snapshot(temp.path()).await,
            after,
            "terminal replay changed accounting"
        );
        assert_eq!(peer::entries(&temp.path().join("effects.log")).len(), 1);
        assert_eq!(peer::entries(&temp.path().join("providers.log")).len(), 2);
    }
}

#[tokio::test]
async fn killed_successful_retry_checkpoint_never_revives_the_older_uncertain_attempt() {
    for point in ["result-committed", "terminal-committed"] {
        let temp = tempfile::tempdir().unwrap();
        kill_at(temp.path(), point, true);
        let before = snapshot(temp.path()).await;
        assert_eq!(peer::entries(&temp.path().join("effects.log")).len(), 1);
        assert!(matches!(
            peer::execute(temp.path(), true, true, false).await.unwrap(),
            EngineRunOutcome::Completed { .. }
        ));
        assert_eq!(peer::entries(&temp.path().join("effects.log")).len(), 1);
        assert_eq!(peer::entries(&temp.path().join("providers.log")).len(), 2);
        let after = snapshot(temp.path()).await;
        assert_eq!(after.deadline_at_ms, before.deadline_at_ms);
        let usage = after.consumption().unwrap();
        assert_eq!(usage.known.total_tokens, 10);
        assert_eq!(usage.model_attempts, 2);
        assert_eq!(usage.tool_calls, 2);
        assert_eq!(usage.inflight_operations, 0);
    }
}
