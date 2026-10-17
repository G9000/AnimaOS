use super::*;
use crate::runs::{RunRecord, RunStart, RunStatus};

async fn queued_run(manager: &DiscordManager, c: &DiscordConnectorRecord) -> String {
    let run = RunRecord::queued(
        RunStart {
            agent_id: c.agent_id.clone(),
            session_id: c.room_id.clone(),
            source: crate::runs::RunSource::Discord,
            source_ref: Some(c.id.clone()),
            idempotency_key: Some("typing-fixture".into()),
            text: "hello".into(),
            model: "deterministic".into(),
            provider: None,
            parent_run_id: None,
        },
        1,
    );
    let id = run.id.clone();
    manager.state.write().await.runs.insert(run);
    id
}

#[tokio::test]
async fn queued_and_replayed_runs_do_not_type_and_completion_joins_the_indicator() {
    let (manager, transport, c) = setup().await;
    let run = queued_run(&manager, &c).await;
    let epoch = manager.epoch(&c.id);
    let guard = manager.start_typing(&c, &run, epoch).await;
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert_eq!(transport.typing_calls.load(Ordering::SeqCst), 0);
    manager
        .state
        .write()
        .await
        .runs
        .get_mut(&run)
        .unwrap()
        .start("deterministic".into(), None, 2);
    within(transport.typing_entered.0.acquire())
        .await
        .unwrap()
        .forget();
    manager
        .state
        .write()
        .await
        .runs
        .get_mut(&run)
        .unwrap()
        .finish(RunStatus::Completed, None, 3);
    drop(guard);
    within(manager.stop_typing_run(&c.id, &run)).await;
    assert_eq!(transport.typing_inflight.load(Ordering::SeqCst), 0);
    let before = transport.typing_calls.load(Ordering::SeqCst);
    let replay = manager.start_typing(&c, &run, epoch).await;
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    drop(replay);
    manager.stop_typing_run(&c.id, &run).await;
    assert_eq!(transport.typing_calls.load(Ordering::SeqCst), before);
}

#[tokio::test]
async fn stop_intent_cancels_a_held_typing_post_while_the_native_run_is_still_draining() {
    let (manager, transport, c) = setup().await;
    transport.typing_gate.store(true, Ordering::SeqCst);
    let run = queued_run(&manager, &c).await;
    manager
        .state
        .write()
        .await
        .runs
        .get_mut(&run)
        .unwrap()
        .start("deterministic".into(), None, 2);
    let guard = manager.start_typing(&c, &run, manager.epoch(&c.id)).await;
    within(transport.typing_entered.0.acquire())
        .await
        .unwrap()
        .forget();
    manager.runs.stop_run(&c.agent_id, &run).await.unwrap();
    within(async {
        loop {
            if transport.typing_inflight.load(Ordering::SeqCst) == 0 {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    assert_eq!(transport.typing_calls.load(Ordering::SeqCst), 1);
    drop(guard);
    manager.stop_typing_run(&c.id, &run).await;
}

#[tokio::test]
async fn credential_replacement_and_shutdown_drain_the_old_typing_request() {
    for replace in [true, false] {
        let (manager, transport, c) = setup().await;
        transport.typing_gate.store(true, Ordering::SeqCst);
        let run = queued_run(&manager, &c).await;
        manager
            .state
            .write()
            .await
            .runs
            .get_mut(&run)
            .unwrap()
            .start("deterministic".into(), None, 2);
        let guard = manager.start_typing(&c, &run, manager.epoch(&c.id)).await;
        within(transport.typing_entered.0.acquire())
            .await
            .unwrap()
            .forget();
        if replace {
            within(manager.replace_token(
                c.agent_id.clone(),
                c.id.clone(),
                DiscordBotToken::parse("replacement").unwrap(),
            ))
            .await
            .unwrap();
        } else {
            within(manager.shutdown()).await;
        }
        assert_eq!(transport.typing_inflight.load(Ordering::SeqCst), 0);
        assert_eq!(transport.typing_calls.load(Ordering::SeqCst), 1);
        drop(guard);
        manager.shutdown().await;
    }
}

#[tokio::test]
async fn typing_respects_rate_limit_and_refreshes_before_indicator_expiry() {
    let (manager, transport, c) = setup().await;
    transport
        .typing_results
        .lock()
        .unwrap()
        .push_back(Err(DiscordTransportError::RateLimited {
            retry_after_ms: 1000,
            global: false,
        }));
    let run = queued_run(&manager, &c).await;
    manager
        .state
        .write()
        .await
        .runs
        .get_mut(&run)
        .unwrap()
        .start("deterministic".into(), None, 2);
    let guard = manager.start_typing(&c, &run, manager.epoch(&c.id)).await;
    within(transport.typing_entered.0.acquire())
        .await
        .unwrap()
        .forget();
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    assert_eq!(transport.typing_calls.load(Ordering::SeqCst), 1);
    within(transport.typing_entered.0.acquire())
        .await
        .unwrap()
        .forget();
    let second = tokio::time::Instant::now();
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        transport.typing_entered.0.acquire(),
    )
    .await
    .unwrap()
    .unwrap()
    .forget();
    assert!(second.elapsed() >= std::time::Duration::from_millis(7800));
    assert_eq!(transport.typing_calls.load(Ordering::SeqCst), 3);
    drop(guard);
    within(manager.stop_typing_run(&c.id, &run)).await;
    assert_eq!(transport.sends.load(Ordering::SeqCst), 0);
    assert!(manager.state.read().await.discord_outbound.is_empty());
    manager.shutdown().await;
}
