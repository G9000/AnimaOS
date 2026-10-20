use super::*;

async fn add_channel(
    manager: &DiscordManager,
    c: &DiscordConnectorRecord,
    channel: &str,
) -> DiscordConnectorRecord {
    manager
        .create(
            c.agent_id.clone(),
            DiscordBotToken::parse("test.token").unwrap(),
            c.channel.guild_id.clone(),
            channel.into(),
            c.approved_user_ids.clone(),
        )
        .await
        .expect("same agent and bot may connect to another channel")
}

#[tokio::test]
async fn one_agent_can_create_three_distinct_channel_connections_and_restore_them() {
    let (manager, _, first) = setup().await;
    let second = add_channel(&manager, &first, "55").await;
    let third = add_channel(&manager, &first, "66").await;
    assert_eq!(first.bot.id, second.bot.id);
    assert_eq!(first.bot.id, third.bot.id);
    assert_ne!(first.room_id, second.room_id);
    assert_ne!(second.room_id, third.room_id);
    assert!(!second.enabled);
    assert!(!third.enabled);

    let snapshot = manager.state.read().await.control_plane_snapshot();
    // Credential finalization may update the saved timestamp after create's
    // returned record. Restore must preserve the exact saved records.
    let expected = snapshot.discord_connectors.clone();
    let mut restored = DaemonState::new();
    restored.restore_control_plane_snapshot(snapshot).unwrap();
    assert_eq!(restored.discord_connectors.len(), 3);
    for connector in expected {
        assert_eq!(restored.discord_connectors[&connector.id], connector);
    }
    manager.shutdown().await;
}

#[tokio::test]
async fn duplicate_channel_is_rejected_but_a_deleted_channel_can_be_reconnected() {
    let (manager, _, first) = setup().await;
    let duplicate = manager
        .create(
            first.agent_id.clone(),
            DiscordBotToken::parse("test.token").unwrap(),
            first.channel.guild_id.clone(),
            first.channel.id.clone(),
            first.approved_user_ids.clone(),
        )
        .await;
    assert_eq!(
        duplicate.unwrap_err(),
        DiscordManagerError::DuplicateChannel
    );
    assert_eq!(manager.state.read().await.discord_connectors.len(), 1);

    manager
        .delete(first.agent_id.clone(), first.id.clone())
        .await
        .unwrap();
    let replacement = add_channel(&manager, &first, &first.channel.id).await;
    assert_ne!(replacement.id, first.id);
    manager.shutdown().await;
}

#[tokio::test]
async fn concurrent_duplicate_creation_saves_only_one_connection_for_the_channel() {
    let (manager, _, first) = setup().await;
    let create = || {
        manager.create(
            first.agent_id.clone(),
            DiscordBotToken::parse("test.token").unwrap(),
            first.channel.guild_id.clone(),
            "55".into(),
            first.approved_user_ids.clone(),
        )
    };
    let (left, right) = tokio::join!(create(), create());
    let results = [left, right];
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results.iter().find_map(|result| result.as_ref().err()),
        Some(&DiscordManagerError::DuplicateChannel)
    );
    assert_eq!(manager.state.read().await.discord_connectors.len(), 2);
    manager.shutdown().await;
}

#[tokio::test]
async fn snapshot_accepts_multiple_channels_but_rejects_duplicate_agent_channel() {
    let (manager, _, first) = setup().await;
    let mut snapshot = manager.state.read().await.control_plane_snapshot();
    let mut second = first.clone();
    second.id = "discord-second".into();
    second.room_id = format!("discord:{}", second.id);
    second.channel.id = "55".into();
    snapshot.discord_connectors.push(second.clone());
    validate_snapshot(&snapshot).expect("different channels are valid for the same agent");

    snapshot.discord_connectors.last_mut().unwrap().channel.id = first.channel.id.clone();
    assert!(validate_snapshot(&snapshot).is_err());
    snapshot
        .discord_connectors
        .last_mut()
        .unwrap()
        .deleted_at_ms = Some(2);
    snapshot.discord_connectors.last_mut().unwrap().enabled = false;
    validate_snapshot(&snapshot).expect("deleted connections do not reserve a channel");
    manager.shutdown().await;
}

#[tokio::test]
async fn replies_stay_in_the_originating_channel_with_independent_sessions_and_outboxes() {
    let (manager, transport, first) = setup().await;
    let mut second = add_channel(&manager, &first, "55").await;
    // Exercise the native intake/run/delivery path without starting background polling.
    second.enabled = true;
    second.activated_at_ms = Some(1);
    manager
        .state
        .write()
        .await
        .discord_connectors
        .insert(second.id.clone(), second.clone());

    for c in [&first, &second] {
        let mut message = incoming(c, "9007199254740993");
        message.text = format!("message from channel {}", c.channel.id);
        manager
            .accept_batch(
                c.id.clone(),
                DiscordMessageBatch {
                    messages: vec![message],
                    next_cursor: "9007199254740993".into(),
                    boundary_proven: true,
                },
            )
            .await
            .unwrap();
        assert!(manager.process_one(c.id.clone()).await.unwrap());
    }
    within(async {
        loop {
            let state = manager.state.read().await;
            if state.discord_inbound.len() == 2
                && state
                    .discord_inbound
                    .values()
                    .all(|r| r.processing_state == DiscordProcessingState::Processed)
            {
                break;
            }
            drop(state);
            tokio::task::yield_now().await;
        }
    })
    .await;

    // Delivering the other connection cannot consume or broadcast this one's reply.
    assert!(manager.deliver_one(second.id.clone()).await.unwrap());
    assert!(!manager.deliver_one(second.id.clone()).await.unwrap());
    {
        let sent = transport.sent_chunks.lock().unwrap();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].0, second.channel.id);
    }
    assert!(manager.deliver_one(first.id.clone()).await.unwrap());
    assert!(!manager.deliver_one(first.id.clone()).await.unwrap());

    {
        let state = manager.state.read().await;
        let sent = transport.sent_chunks.lock().unwrap();
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[1].0, first.channel.id);
        for c in [&first, &second] {
            let receipt = &state.discord_inbound[&(c.id.clone(), "9007199254740993".into())];
            let run = state.runs.get(receipt.run_id.as_deref().unwrap()).unwrap();
            assert_eq!(run.session_id, c.room_id);
            let output = state
                .discord_outbound
                .values()
                .find(|r| r.connector_id == c.id)
                .unwrap();
            assert_eq!(output.room_id, c.room_id);
            assert_eq!(output.delivery_state, DiscordDeliveryState::Delivered);
            assert!(sent.contains(&(c.channel.id.clone(), output.text.clone())));
            let session = state.sessions.get(&c.agent_id, &c.room_id).unwrap();
            assert_eq!(session.kind, crate::sessions::SessionKind::Discord);
        }
        assert_eq!(state.discord_outbound.len(), 2);
    }
    manager.runs.shutdown().await;
    manager.shutdown().await;
}

#[tokio::test]
async fn wrong_channel_messages_are_rejected_and_disabling_one_connection_leaves_the_other_active()
{
    let (manager, _, first) = setup().await;
    let second = add_channel(&manager, &first, "55").await;
    assert_eq!(
        manager
            .accept_batch(
                first.id.clone(),
                DiscordMessageBatch {
                    messages: vec![incoming(&second, "9007199254740993")],
                    next_cursor: "9007199254740993".into(),
                    boundary_proven: true,
                },
            )
            .await
            .unwrap_err(),
        DiscordManagerError::Conflict
    );
    manager
        .activate(first.agent_id.clone(), first.id.clone())
        .await
        .unwrap();
    manager
        .activate(second.agent_id.clone(), second.id.clone())
        .await
        .unwrap();
    {
        let workers = manager.workers.lock().await;
        assert!(workers.contains_key(&first.id));
        assert!(workers.contains_key(&second.id));
    }
    manager
        .set_enabled(second.agent_id.clone(), second.id.clone(), false)
        .await
        .unwrap();
    assert!(manager.state.read().await.discord_connectors[&first.id].is_active());
    {
        let workers = manager.workers.lock().await;
        assert!(workers.contains_key(&first.id));
        assert!(!workers.contains_key(&second.id));
    }
    manager
        .delete(second.agent_id.clone(), second.id.clone())
        .await
        .unwrap();
    let state = manager.state.read().await;
    assert!(state.discord_connectors[&first.id].is_active());
    assert!(state.discord_inbound.is_empty());
    assert!(state.discord_outbound.is_empty());
    drop(state);
    manager.shutdown().await;
}
