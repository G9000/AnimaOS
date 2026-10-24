use super::*;

async fn title_fixture(
    steps: Vec<Step>,
    secondary: Vec<Step>,
) -> (
    DiscordManager,
    Arc<FakeTransport>,
    DiscordConnectorRecord,
    Arc<ScriptedModel>,
) {
    let (manager, transport, c) = bot_wide().await;
    let model = ScriptedModel::with_secondary(steps, secondary);
    {
        let mut state = manager.state.write().await;
        state.model_adapter = model.clone();
        state.set_generated_titles(true);
    }
    let parent = discover(&manager, &c, "22", "101").await;
    admit(&manager, &parent, "101", "<@11> review the landing page").await;
    (manager, transport, c, model)
}

async fn title_call_finished(manager: &DiscordManager, model: &ScriptedModel) {
    within(async {
        while model.secondary_requests().is_empty() {
            tokio::task::yield_now().await;
        }
        while manager.owned_count.load(Ordering::SeqCst) != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await;
}

#[tokio::test]
async fn auto_thread_generation_failure_keeps_placeholder_and_does_not_retry() {
    for secondary in [
        Step::Fail("title provider unavailable"),
        Step::Text(vec!["Unusable"]),
    ] {
        let (manager, transport, c, model) = title_fixture(
            vec![Step::Text(vec!["Reviewed."]), Step::Text(vec!["Done."])],
            vec![secondary],
        )
        .await;
        processed(&manager, &c, "101").await;
        title_call_finished(&manager, &model).await;
        assert_eq!(
            transport.channels.lock().unwrap()["101"].name.as_deref(),
            Some("review the landing page")
        );
        assert_eq!(transport.rename_calls.load(Ordering::SeqCst), 0);
        let thread = manager
            .get(&c.agent_id, &c.id)
            .await
            .unwrap()
            .destination_for_channel("101")
            .unwrap();
        admit(&manager, &thread, "201", "Thanks").await;
        processed(&manager, &c, "201").await;
        assert_eq!(model.secondary_requests().len(), 1);
        manager.shutdown().await;
    }
}

#[tokio::test]
async fn auto_thread_disabled_and_preexisting_conversations_never_generate_titles() {
    for preexisting in [false, true] {
        let (manager, transport, c, model) =
            title_fixture(vec![Step::Text(vec!["Reviewed."])], vec![]).await;
        {
            let mut state = manager.state.write().await;
            if preexisting {
                let conversation = state
                    .discord_connectors
                    .get_mut(&c.id)
                    .unwrap()
                    .conversations
                    .iter_mut()
                    .find(|v| v.channel.id == "101")
                    .unwrap();
                // Older persisted conversations have no autoTitle marker.
                let mut json = serde_json::to_value(&*conversation).unwrap();
                json.as_object_mut().unwrap().remove("autoTitle");
                *conversation = serde_json::from_value(json).unwrap();
                assert!(conversation.auto_title.is_none());
            } else {
                let mut config = state.agents[&c.agent_id].config().clone();
                config
                    .settings
                    .as_mut()
                    .unwrap()
                    .additional
                    .insert("autoTitle".into(), anima_core::DataValue::Bool(false));
                state.restore_agent_config(&c.agent_id, config);
            }
        }
        processed(&manager, &c, "101").await;
        assert!(manager.deliver_one(c.id.clone()).await.unwrap());
        manager.shutdown().await;
        assert!(model.secondary_requests().is_empty());
        assert_eq!(transport.rename_calls.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn auto_thread_failed_first_run_leaves_title_for_the_first_successful_reply() {
    let (manager, transport, c, model) = title_fixture(
        vec![
            Step::Fail("reply provider unavailable"),
            Step::Text(vec!["Reviewed."]),
        ],
        vec![Step::Text(vec!["Landing Page Review"])],
    )
    .await;
    processed(&manager, &c, "101").await;
    assert!(model.secondary_requests().is_empty());
    let thread = manager
        .get(&c.agent_id, &c.id)
        .await
        .unwrap()
        .destination_for_channel("101")
        .unwrap();
    admit(&manager, &thread, "201", "Retry the review").await;
    processed(&manager, &c, "201").await;
    title_call_finished(&manager, &model).await;
    assert_eq!(transport.rename_calls.load(Ordering::SeqCst), 1);
    assert!(model.secondary_requests()[0].messages[0]
        .content
        .text
        .contains("First message:\nRetry the review"));
    manager.shutdown().await;
}

#[tokio::test]
async fn auto_thread_uncertain_or_denied_rename_is_not_replayed_after_restart() {
    for error in [
        DiscordTransportError::Uncertain,
        DiscordTransportError::PermissionDenied,
    ] {
        let (manager, transport, c, model) = title_fixture(
            vec![Step::Text(vec!["Reviewed."])],
            vec![Step::Text(vec!["Landing Page Review"])],
        )
        .await;
        *transport.rename_error.lock().unwrap() = Some(error);
        processed(&manager, &c, "101").await;
        title_call_finished(&manager, &model).await;
        assert_eq!(transport.rename_calls.load(Ordering::SeqCst), 1);
        let snapshot = manager.state.read().await.control_plane_snapshot();
        validate_snapshot(&snapshot).unwrap();
        assert!(snapshot.discord_connectors[0].thread_starts.is_empty());
        let saved_thread = snapshot.discord_connectors[0]
            .conversations
            .iter()
            .find(|v| v.channel.id == "101")
            .unwrap();
        assert!(saved_thread.auto_title.as_ref().unwrap().attempted);
        let mut restored = DaemonState::new();
        restored.restore_control_plane_snapshot(snapshot).unwrap();
        restored.model_adapter = model.clone();
        restored.set_generated_titles(true);
        manager.shutdown().await;
        manager.runs.shutdown().await;
        let state = Arc::new(RwLock::new(restored));
        let runs = AgentRunCoordinator::new(state.clone(), Arc::new(Semaphore::new(4)));
        let restarted = DiscordManager::new(
            state,
            runs,
            manager.credentials.clone(),
            manager.registry.clone(),
        );
        let thread = restarted
            .get(&c.agent_id, &c.id)
            .await
            .unwrap()
            .destination_for_channel("101")
            .unwrap();
        admit(&restarted, &thread, "201", "Thanks").await;
        processed(&restarted, &c, "201").await;
        assert_eq!(model.secondary_requests().len(), 1);
        assert_eq!(transport.rename_calls.load(Ordering::SeqCst), 1);
        restarted.shutdown().await;
    }
}

#[tokio::test]
async fn auto_thread_revoked_authority_cannot_rename_after_a_late_read() {
    let (manager, transport, c, _) = title_fixture(
        vec![Step::Text(vec!["Reviewed."])],
        vec![Step::Text(vec!["Landing Page Review"])],
    )
    .await;
    transport.title_lookup_gate.store(true, Ordering::SeqCst);
    processed(&manager, &c, "101").await;
    within(transport.title_lookup_entered.0.acquire())
        .await
        .unwrap()
        .forget();
    manager.revoke(&c.id);
    transport.title_lookup_gate.store(false, Ordering::SeqCst);
    transport.title_lookup_release.0.add_permits(1);
    within(async {
        while manager.owned_count.load(Ordering::SeqCst) != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await;
    assert_eq!(transport.rename_calls.load(Ordering::SeqCst), 0);
    manager.shutdown().await;
}

#[tokio::test]
async fn auto_thread_shutdown_cancels_a_hung_title_call() {
    let (manager, transport, c, model) = title_fixture(
        vec![Step::Text(vec!["Reviewed."])],
        vec![Step::Hold(vec![])],
    )
    .await;
    processed(&manager, &c, "101").await;
    within(async {
        while model.secondary_requests().is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await;
    within(manager.shutdown()).await;
    assert_eq!(transport.rename_calls.load(Ordering::SeqCst), 0);
}
