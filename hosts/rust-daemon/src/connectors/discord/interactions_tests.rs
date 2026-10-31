use super::*;
use crate::agent_runs::test_support::{ScriptedModel, Step};
use anima_core::MessageRole;
#[path = "attachment_tests.rs"]
mod attachment_tests;
#[path = "rename_tests.rs"]
mod rename_tests;
#[path = "thread_history_tests.rs"]
mod thread_history_tests;
#[path = "title_tests.rs"]
mod title_tests;

async fn bot_wide() -> (DiscordManager, Arc<FakeTransport>, DiscordConnectorRecord) {
    let (manager, transport, legacy) = setup().await;
    manager
        .state
        .write()
        .await
        .discord_connectors
        .remove(&legacy.id);
    let mut c = manager
        .create_with_settings(
            legacy.agent_id,
            DiscordBotToken::parse("test.token").unwrap(),
            None,
            None,
            vec![],
            DiscordSettings::default(),
        )
        .await
        .unwrap();
    assert!(c.bot_wide);
    assert!(c.channel.id.is_empty());
    assert_eq!(c.approved_user_ids, vec!["44"]);
    assert!(manager
        .state
        .read()
        .await
        .sessions
        .get(&c.agent_id, &c.room_id)
        .is_none());
    c.enabled = true;
    c.activated_at_ms = Some(1);
    manager
        .state
        .write()
        .await
        .discord_connectors
        .insert(c.id.clone(), c.clone());
    (manager, transport, c)
}
async fn discover(
    manager: &DiscordManager,
    c: &DiscordConnectorRecord,
    channel: &str,
    message: &str,
) -> DiscordConnectorRecord {
    let epoch = manager.epoch(&c.id);
    manager.discovered.lock().await.insert(
        c.id.clone(),
        vec![(epoch, "33".into(), channel.into(), Some(message.into()))],
    );
    manager
        .discover_channels(c, epoch, &DiscordBotToken::parse("test.token").unwrap())
        .await
        .unwrap();
    manager
        .get(&c.agent_id, &c.id)
        .await
        .unwrap()
        .destination_for_channel(channel)
        .unwrap()
}
async fn admit(manager: &DiscordManager, c: &DiscordConnectorRecord, id: &str, text: &str) {
    let mut message = incoming(c, id);
    message.text = text.into();
    manager
        .accept_batch(
            c.id.clone(),
            DiscordMessageBatch {
                messages: vec![message],
                next_cursor: id.into(),
                boundary_proven: true,
            },
        )
        .await
        .unwrap();
}
async fn processed(manager: &DiscordManager, c: &DiscordConnectorRecord, id: &str) {
    assert!(manager.process_one(c.id.clone()).await.unwrap());
    within(async {
        loop {
            let state = manager.state.read().await;
            if state.discord_inbound[&(c.id.clone(), id.into())]
                .processing_state
                .is_terminal()
            {
                break;
            }
            drop(state);
            tokio::task::yield_now().await;
        }
    })
    .await;
}

#[tokio::test]
async fn auto_thread_placeholder_strips_all_discord_mentions() {
    let (manager, transport, c) = bot_wide().await;
    let parent = discover(&manager, &c, "22", "101").await;
    admit(
        &manager,
        &parent,
        "101",
        "<@11>  review <@!44> <@&55> in <#22>\n landing page",
    )
    .await;
    assert_eq!(
        transport.channels.lock().unwrap()["101"].name.as_deref(),
        Some("review in landing page")
    );
}

#[tokio::test]
async fn auto_thread_gets_one_ai_title_after_its_first_completed_reply() {
    let (manager, transport, c) = bot_wide().await;
    let model = ScriptedModel::with_secondary(
        vec![
            Step::Text(vec!["I reviewed the page."]),
            Step::Text(vec!["Done."]),
        ],
        vec![Step::Text(vec!["Landing Page Review"])],
    );
    {
        let mut state = manager.state.write().await;
        state.model_adapter = model.clone();
        state.set_generated_titles(true);
    }
    let parent = discover(&manager, &c, "22", "101").await;
    admit(&manager, &parent, "101", "<@11> review the landing page").await;
    assert!(model.secondary_requests().is_empty());
    processed(&manager, &c, "101").await;
    within(async {
        loop {
            if transport.channels.lock().unwrap()["101"].name.as_deref()
                == Some("Landing Page Review")
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    assert!(manager.deliver_one(c.id.clone()).await.unwrap());
    let current = manager.get(&c.agent_id, &c.id).await.unwrap();
    let thread = current.destination_for_channel("101").unwrap();
    admit(&manager, &thread, "201", "Thanks").await;
    processed(&manager, &c, "201").await;
    assert_eq!(model.secondary_requests().len(), 1);
    let rows = manager.state.read().await.history.pending_usage();
    assert_eq!(
        rows.iter()
            .filter(|r| r.source == crate::usage::UsageSource::Title)
            .count(),
        1
    );
}

#[tokio::test]
async fn auto_thread_preserves_a_manual_name_changed_while_the_title_is_generated() {
    let (manager, transport, c) = bot_wide().await;
    let model = ScriptedModel::with_secondary(
        vec![Step::Text(vec!["I reviewed the page."])],
        vec![Step::Text(vec!["Landing Page Review"])],
    );
    {
        let mut state = manager.state.write().await;
        state.model_adapter = model.clone();
        state.set_generated_titles(true);
    }
    let parent = discover(&manager, &c, "22", "101").await;
    admit(&manager, &parent, "101", "<@11> review the landing page").await;
    transport.title_lookup_gate.store(true, Ordering::SeqCst);
    processed(&manager, &c, "101").await;
    within(transport.title_lookup_entered.0.acquire())
        .await
        .unwrap()
        .forget();
    // Title work cannot delay the already committed reply's delivery.
    assert!(manager.deliver_one(c.id.clone()).await.unwrap());
    transport
        .channels
        .lock()
        .unwrap()
        .get_mut("101")
        .unwrap()
        .name = Some("My chosen title".into());
    transport.title_lookup_gate.store(false, Ordering::SeqCst);
    transport.title_lookup_release.0.add_permits(1);
    within(async {
        while manager.owned_count.load(Ordering::SeqCst) != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await;
    assert_eq!(
        transport.channels.lock().unwrap()["101"].name.as_deref(),
        Some("My chosen title")
    );
    manager.shutdown().await;
}

#[tokio::test]
async fn bot_wide_discovers_trigger_and_keeps_two_native_threads_and_outboxes_isolated() {
    let (manager, transport, c) = bot_wide().await;
    let model = ScriptedModel::new(vec![
        Step::Text(vec!["reply one"]),
        Step::Text(vec!["reply two"]),
        Step::Text(vec!["thread followup"]),
        Step::Text(vec!["owner reply in selected thread"]),
    ]);
    manager.state.write().await.model_adapter = model.clone();
    let parent = discover(&manager, &c, "22", "101").await;
    assert_eq!(
        parent.cursor, "100",
        "first discovery includes its triggering message and avoids scanning old channel chatter"
    );
    admit(&manager, &parent, "101", "<@11> topic one").await;
    processed(&manager, &c, "101").await;
    admit(&manager, &parent, "102", "<@11> topic two").await;
    processed(&manager, &c, "102").await;
    assert!(manager.deliver_one(c.id.clone()).await.unwrap());
    assert!(manager.deliver_one(c.id.clone()).await.unwrap());
    let current = manager.get(&c.agent_id, &c.id).await.unwrap();
    let first = current.destination_for_channel("101").unwrap();
    let second = current.destination_for_channel("102").unwrap();
    assert_ne!(first.room_id, second.room_id);
    admit(
        &manager,
        &first,
        "201",
        "continue topic one without a mention",
    )
    .await;
    processed(&manager, &c, "201").await;
    let requests = model.requests();
    let transcript = requests
        .last()
        .unwrap()
        .messages
        .iter()
        .filter(|m| m.role == MessageRole::User)
        .map(|m| m.content.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(transcript.contains("topic one"));
    assert!(!transcript.contains("topic two"));
    assert!(manager.deliver_one(c.id.clone()).await.unwrap());
    let owner_manager = manager.clone();
    let owner_agent = c.agent_id.clone();
    let owner_connector = c.id.clone();
    let owner_room = first.room_id.clone();
    let start = crate::agent_runs::QueuedRunStart::new(move |run_id| {
        Box::pin(async move {
            owner_manager
                .send_from_owner_in_room_accepted(
                    owner_agent,
                    owner_connector,
                    owner_room,
                    "owner followup".into(),
                    "owner-thread".into(),
                    run_id,
                )
                .await
                .map_err(|error| crate::agent_runs::QueuedStartError::Failed(error.to_string()))
        })
    });
    let accepted = manager
        .runs
        .accept_run(
            crate::agent_runs::AcceptRun {
                outcome: None,
                document_edit_context: None,
                agent_id: c.agent_id.clone(),
                session_id: first.room_id.clone(),
                source: crate::runs::RunSource::Discord,
                source_ref: Some(c.id.clone()),
                idempotency_key: "owner-thread".into(),
                text: "owner followup".into(),
                attachment_ids: vec![],
                mode: crate::agent_runs::SessionRunMode::Queue,
                skill: None,
            },
            start,
        )
        .await
        .unwrap();
    let crate::agent_runs::AcceptedRun::Created(owner) = accepted else {
        panic!("new owner request must be accepted once")
    };
    within(async {
        loop {
            let state = manager.state.read().await;
            let run = state.runs.get(&owner.id).unwrap();
            if run.status.is_terminal() {
                assert_eq!(run.status, crate::runs::RunStatus::Completed, "{run:?}");
                break;
            }
            drop(state);
            tokio::task::yield_now().await;
        }
    })
    .await;
    assert!(manager.deliver_one(c.id.clone()).await.unwrap());
    assert_eq!(transport.thread_calls.load(Ordering::SeqCst), 2);
    let sent = transport.sent_chunks.lock().unwrap();
    assert_eq!(sent.len(), 4);
    assert!(sent.iter().any(|(id, _)| id == "101"));
    assert!(sent.iter().any(|(id, _)| id == "102"));
    assert!(!sent.iter().any(|(id, _)| id == "22"));
    assert_eq!(sent.last().unwrap().0, "101");
    drop(sent);
    let snapshot = manager.state.read().await.control_plane_snapshot();
    validate_snapshot(&snapshot).unwrap();
    let mut restored = DaemonState::new();
    restored.restore_control_plane_snapshot(snapshot).unwrap();
    assert_eq!(restored.discord_connectors[&c.id].conversations.len(), 3);
    manager.runs.shutdown().await;
    manager.shutdown().await;
}

#[tokio::test]
async fn role_authorization_and_parent_allowlist_are_separate_from_mentions() {
    let (manager, transport, mut c) = setup().await;
    c.settings = Some(DiscordSettings {
        allowed_channel_ids: vec!["22".into()],
        approved_role_ids: vec!["77".into()],
        auto_thread: false,
        ..Default::default()
    });
    manager
        .state
        .write()
        .await
        .discord_connectors
        .insert(c.id.clone(), c.clone());
    transport
        .member_roles
        .lock()
        .unwrap()
        .insert("55".into(), vec!["77".into()]);
    let mut role = incoming(&c, "101");
    role.sender.id = "55".into();
    role.text = "<@11> role authorized".into();
    let mut outsider = role.clone();
    outsider.id = "102".into();
    outsider.sender.id = "66".into();
    manager
        .accept_batch(
            c.id.clone(),
            DiscordMessageBatch {
                messages: vec![role, outsider],
                next_cursor: "102".into(),
                boundary_proven: true,
            },
        )
        .await
        .unwrap();
    assert_eq!(manager.state.read().await.discord_inbound.len(), 1);
    let mut snapshot = manager.state.read().await.control_plane_snapshot();
    snapshot.discord_connectors[0].approved_user_ids = vec!["88".into()];
    validate_snapshot(&snapshot).expect("admitted role history survives later user changes");
    let mut thread = c.channel.clone();
    thread.id = "99".into();
    thread.kind = DiscordChannelKind::GuildThread;
    thread.parent_channel_id = Some("23".into());
    assert!(!c.channel_allowed(&thread));
    thread.parent_channel_id = Some("22".into());
    assert!(c.channel_allowed(&thread));
    manager.shutdown().await;
}

#[tokio::test]
async fn uncertain_thread_creation_never_redirects_or_retries_after_restart() {
    let (manager, transport, mut c) = setup().await;
    c.settings = Some(DiscordSettings::default());
    manager
        .state
        .write()
        .await
        .discord_connectors
        .insert(c.id.clone(), c.clone());
    *transport.thread_error.lock().unwrap() = Some(DiscordTransportError::Uncertain);
    admit(&manager, &c, "101", "<@11> start a thread").await;
    assert!(manager.state.read().await.discord_inbound.is_empty());
    assert_eq!(transport.thread_calls.load(Ordering::SeqCst), 1);
    admit(&manager, &c, "101", "<@11> start a thread").await;
    assert_eq!(transport.thread_calls.load(Ordering::SeqCst), 1);
    assert!(!manager.process_one(c.id.clone()).await.unwrap());
    let snapshot = manager.state.read().await.control_plane_snapshot();
    validate_snapshot(&snapshot).unwrap();
    assert!(
        snapshot.discord_connectors[0].thread_starts.is_empty(),
        "claims retire atomically behind the durable cursor"
    );
    manager.shutdown().await;
}

#[tokio::test]
async fn first_thread_mention_and_later_plain_messages_are_admitted_in_numeric_order() {
    let (manager, transport, c) = bot_wide().await;
    transport.channels.lock().unwrap().insert(
        "88".into(),
        DiscordChannelMetadata {
            id: "88".into(),
            guild_id: "33".into(),
            name: None,
            kind: DiscordChannelKind::GuildThread,
            parent_channel_id: Some("22".into()),
        },
    );
    let thread = discover(&manager, &c, "88", "9").await;
    let mut first = incoming(&thread, "9");
    first.text = "<@11> join this thread".into();
    let mut next = incoming(&thread, "10");
    next.text = "plain followup".into();
    manager
        .accept_batch(
            c.id.clone(),
            DiscordMessageBatch {
                messages: vec![next, first],
                next_cursor: "10".into(),
                boundary_proven: true,
            },
        )
        .await
        .unwrap();
    assert_eq!(manager.state.read().await.discord_inbound.len(), 2);
    manager.shutdown().await;
}

#[tokio::test]
async fn configuring_a_new_channel_starts_at_its_current_boundary() {
    let (manager, transport, c) = bot_wide().await;
    transport.latest.store(5000, Ordering::SeqCst);
    let next = manager
        .configure(
            c.agent_id.clone(),
            c.id.clone(),
            DiscordSettings {
                allowed_channel_ids: vec!["22".into()],
                home_channel_id: Some("22".into()),
                ..Default::default()
            },
            vec!["44".into()],
        )
        .await
        .unwrap();
    assert_eq!(next.home_destination().unwrap().cursor, "5000");
    assert!(manager.state.read().await.discord_inbound.is_empty());
    manager.shutdown().await;
}

#[tokio::test]
async fn explicit_home_is_independent_of_incoming_channel_restrictions() {
    let (manager, transport, c) = bot_wide().await;
    let next = manager
        .configure(
            c.agent_id.clone(),
            c.id.clone(),
            DiscordSettings {
                allowed_channel_ids: vec!["22".into()],
                home_channel_id: Some("23".into()),
                auto_thread: false,
                ..Default::default()
            },
            vec!["44".into()],
        )
        .await
        .unwrap();
    let home = next.home_destination().unwrap();
    assert_eq!(home.channel.id, "23");
    assert!(next.can_send_to_channel(&home.channel));
    assert!(!next.channel_allowed(&home.channel));
    assert!(
        !next.destinations().iter().any(|v| v.channel.id == "23"),
        "home is not scanned as an input channel unless separately allowed"
    );
    manager.stop_worker(&c.id).await;
    let output = outbound_for(&home, "home-reply", "proactive hello", false);
    manager
        .state
        .write()
        .await
        .discord_outbound
        .insert(output.id.clone(), output);
    assert!(manager.deliver_one(c.id.clone()).await.unwrap());
    assert_eq!(
        transport.sent_chunks.lock().unwrap().as_slice(),
        &[("23".into(), "proactive hello".into())]
    );
    validate_snapshot(&manager.state.read().await.control_plane_snapshot()).unwrap();
    manager.shutdown().await;
}

#[tokio::test]
async fn failed_dynamic_admission_save_removes_only_its_new_session_and_retains_cursor() {
    let (manager, _, c) = bot_wide().await;
    let parent = discover(&manager, &c, "22", "101").await;
    {
        let mut state = manager.state.write().await;
        state
            .discord_connectors
            .get_mut(&c.id)
            .unwrap()
            .settings
            .as_mut()
            .unwrap()
            .auto_thread = false;
    }
    let preserved = manager
        .state
        .read()
        .await
        .sessions
        .get(&c.agent_id, "discord:discord-fixture")
        .cloned()
        .unwrap();
    let gate = manager
        .state
        .write()
        .await
        .install_test_control_plane_save_gate(true);
    let job = tokio::spawn({
        let manager = manager.clone();
        let parent = parent.clone();
        async move {
            let mut message = incoming(&parent, "101");
            message.text = "<@11> a new room".into();
            manager
                .accept_batch(
                    parent.id.clone(),
                    DiscordMessageBatch {
                        messages: vec![message],
                        next_cursor: "101".into(),
                        boundary_proven: true,
                    },
                )
                .await
        }
    });
    within(gate.entered.acquire()).await.unwrap().forget();
    assert!(manager
        .state
        .read()
        .await
        .sessions
        .get(&c.agent_id, &parent.room_id)
        .is_some());
    gate.release.add_permits(1);
    assert_eq!(
        within(job).await.unwrap().unwrap_err(),
        DiscordManagerError::Persistence
    );
    {
        let state = manager.state.read().await;
        assert!(state.sessions.get(&c.agent_id, &parent.room_id).is_none());
        assert_eq!(
            state.sessions.get(&c.agent_id, &preserved.id),
            Some(&preserved)
        );
        assert_eq!(
            state.discord_connectors[&c.id]
                .destination_for_channel("22")
                .unwrap()
                .cursor,
            "100"
        );
        assert!(state.discord_inbound.is_empty());
    }
    manager.shutdown().await;
}

#[tokio::test]
async fn transient_discovery_failure_keeps_trigger_and_other_queued_channels() {
    let (manager, transport, c) = bot_wide().await;
    let epoch = manager.epoch(&c.id);
    manager.discovered.lock().await.insert(
        c.id.clone(),
        vec![
            (epoch, "33".into(), "22".into(), Some("101".into())),
            (epoch, "33".into(), "23".into(), Some("102".into())),
        ],
    );
    transport
        .channel_errors
        .lock()
        .unwrap()
        .push_back(DiscordTransportError::Transport);
    let token = DiscordBotToken::parse("test.token").unwrap();
    assert!(manager.discover_channels(&c, epoch, &token).await.is_err());
    assert_eq!(manager.discovered.lock().await[&c.id].len(), 2);
    manager.discover_channels(&c, epoch, &token).await.unwrap();
    let current = manager.get(&c.agent_id, &c.id).await.unwrap();
    assert_eq!(current.destination_for_channel("22").unwrap().cursor, "100");
    assert_eq!(current.destination_for_channel("23").unwrap().cursor, "101");
    manager.shutdown().await;
}

#[tokio::test]
async fn renaming_a_participating_selected_thread_keeps_its_room_and_restore_valid() {
    let (manager, transport, mut c) = setup().await;
    c.channel.kind = DiscordChannelKind::GuildThread;
    c.channel.parent_channel_id = Some("21".into());
    c.settings = Some(DiscordSettings {
        home_channel_id: Some("22".into()),
        ..Default::default()
    });
    c.conversations.push(DiscordConversation {
        channel: c.channel.clone(),
        room_id: c.room_id.clone(),
        cursor: c.cursor.clone(),
        participating: true,
        auto_title: None,
    });
    manager
        .state
        .write()
        .await
        .discord_connectors
        .insert(c.id.clone(), c.clone());
    let mut renamed = c.channel.clone();
    renamed.name = Some("renamed thread".into());
    transport
        .channels
        .lock()
        .unwrap()
        .insert("22".into(), renamed);
    let next = manager
        .configure(
            c.agent_id.clone(),
            c.id.clone(),
            c.settings.clone().unwrap(),
            c.approved_user_ids.clone(),
        )
        .await
        .unwrap();
    assert_eq!(next.room_id, c.room_id);
    assert_eq!(next.channel.name.as_deref(), Some("renamed thread"));
    validate_snapshot(&manager.state.read().await.control_plane_snapshot()).unwrap();
    manager.shutdown().await;
}

#[tokio::test]
async fn free_response_channel_replies_inline_even_when_bot_is_mentioned() {
    let (manager, transport, mut c) = setup().await;
    c.settings = Some(DiscordSettings {
        free_response_channel_ids: vec!["22".into()],
        ..Default::default()
    });
    manager
        .state
        .write()
        .await
        .discord_connectors
        .insert(c.id.clone(), c.clone());
    admit(&manager, &c, "101", "<@11> answer inline").await;
    assert_eq!(
        manager.state.read().await.discord_inbound[&(c.id.clone(), "101".into())].room_id,
        c.room_id
    );
    assert_eq!(transport.thread_calls.load(Ordering::SeqCst), 0);
    manager.shutdown().await;
}
