use super::*;

#[tokio::test]
async fn chat_mcp_setup_stop_during_vault_read_prevents_transport_start() {
    let fixture = Fixture::new().await;
    let saved = saved_bearer_connection(&fixture).await;
    let enabled = enabled_connection(&fixture, &saved).await;
    let reviewed = fixture.reviewed().await;
    let stamp = fixture.grant_and_stamp(&reviewed).await;
    let run = RunLink {
        agent_id: stamp.agent_id,
        session_id: stamp.session_id,
        run_id: stamp.run_id,
    };
    fixture.state.read().await.live.runs().register(&run.run_id);
    let entered = Arc::new(Semaphore::new(0));
    let release = Arc::new(Semaphore::new(0));
    *fixture.credentials.read_gate.lock().unwrap() = Some((entered.clone(), release.clone()));
    let connects = fixture.factory.connects.load(Ordering::SeqCst);
    let worker = {
        let manager = fixture.manager.for_owner_chat(run.clone());
        let agent = fixture.agent.clone();
        let id = enabled.record.id;
        tokio::spawn(async move { manager.connect(&agent, &id, enabled.record.version).await })
    };
    timeout(Duration::from_secs(2), entered.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    fixture
        .state
        .read()
        .await
        .live
        .runs()
        .control(&run.run_id)
        .unwrap()
        .cancel
        .cancel();
    release.add_permits(1);
    assert_eq!(
        worker.await.unwrap().unwrap_err(),
        McpServiceError::Cancelled
    );
    assert_eq!(fixture.factory.connects.load(Ordering::SeqCst), connects);
    fixture.manager.shutdown().await;
}
