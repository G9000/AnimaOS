//! Complete channel administration stays behind an admitted owner run.
use super::*;
use crate::connectors::discord::channel_admin_model::DiscordChannelAdminMethod;
use serde_json::{json, Value};

const ADMIN_TOOLS: [&str; 12] = [
    "get_discord_channel",
    "delete_discord_channel",
    "reorder_discord_channels",
    "set_discord_channel_permissions",
    "delete_discord_channel_permission",
    "get_discord_channel_permissions",
    "list_discord_threads",
    "create_discord_thread",
    "update_discord_thread",
    "manage_discord_thread_members",
    "set_discord_voice_status",
    "follow_discord_announcement",
];

fn admin_call(name: &str, args: Value) -> ToolCall {
    ToolCall {
        id: format!("{name}-{}", uuid::Uuid::new_v4()),
        name: name.into(),
        args: args
            .as_object()
            .unwrap()
            .iter()
            .map(|(key, value)| (key.clone(), data_value(value)))
            .collect(),
    }
}

fn data_value(value: &Value) -> DataValue {
    match value {
        Value::Null => DataValue::Null,
        Value::Bool(value) => DataValue::Bool(*value),
        Value::Number(value) => DataValue::Number(value.as_f64().unwrap()),
        Value::String(value) => DataValue::String(value.clone()),
        Value::Array(values) => DataValue::Array(values.iter().map(data_value).collect()),
        Value::Object(values) => DataValue::Object(
            values
                .iter()
                .map(|(key, value)| (key.clone(), data_value(value)))
                .collect(),
        ),
    }
}

fn delete_call(id: &str, name: &str) -> ToolCall {
    admin_call(
        "delete_discord_channel",
        json!({"channel_id":id,"expected_name":name}),
    )
}

fn valid_admin_operations() -> Vec<(&'static str, Value)> {
    vec![
        ("get_discord_channel", json!({"channel_id":"22"})),
        (
            "delete_discord_channel",
            json!({"channel_id":"77","expected_name":"channel-77"}),
        ),
        (
            "reorder_discord_channels",
            json!({"positions":[{"id":"22","position":2}]}),
        ),
        (
            "set_discord_channel_permissions",
            json!({"channel_id":"22","target_id":"33","target_type":"role","allow":"1024","deny":"0"}),
        ),
        (
            "delete_discord_channel_permission",
            json!({"channel_id":"22","target_id":"33"}),
        ),
        (
            "get_discord_channel_permissions",
            json!({"channel_id":"22","target_id":"33","target_type":"role"}),
        ),
        ("list_discord_threads", json!({"scope":"active"})),
        (
            "create_discord_thread",
            json!({"channel_id":"22","name":"new-topic","type":"private"}),
        ),
        (
            "update_discord_thread",
            json!({"channel_id":"111","archived":true}),
        ),
        (
            "manage_discord_thread_members",
            json!({"channel_id":"111","action":"join"}),
        ),
        (
            "set_discord_voice_status",
            json!({"channel_id":"77","status":"Ready"}),
        ),
        (
            "follow_discord_announcement",
            json!({"channel_id":"99","target_channel_id":"22"}),
        ),
    ]
}

fn mutations(transport: &FakeTransport) -> usize {
    transport
        .admin_requests
        .lock()
        .unwrap()
        .iter()
        .filter(|request| request.method != DiscordChannelAdminMethod::Get)
        .count()
}

fn install_thread(transport: &FakeTransport, id: &str, parent: &str, guild: &str) {
    transport.admin_channels.lock().unwrap().insert(
        id.into(),
        json!({"id":id,"guild_id":guild,"type":11,"name":"ongoing-topic",
            "parent_id":parent,"rate_limit_per_user":0,
            "thread_metadata":{"archived":false,"locked":false,"auto_archive_duration":1440}}),
    );
}

#[tokio::test]
async fn owner_delete_checks_exact_current_name_before_deleting() {
    let (manager, transport, c, model) = fixture(vec![
        Step::Tools(vec![delete_call("77", "wrong-name")]),
        Step::Tools(vec![delete_call("77", "channel-77")]),
        Step::Text(vec!["Result"]),
    ])
    .await;
    admit_as(&manager, &c, "101", "44").await;
    finish(&manager, &c, "101").await;
    assert_eq!(mutations(&transport), 1, "{}", result(&model));
    assert!(!transport
        .guild_channels
        .lock()
        .unwrap()
        .iter()
        .any(|channel| channel.id == "77"));
    assert!(
        result(&model).contains("\"deleted\":true"),
        "{}",
        result(&model)
    );
    assert_eq!(
        manager
            .get(&c.agent_id, &c.id)
            .await
            .unwrap()
            .approved_user_ids,
        c.approved_user_ids
    );
    manager.shutdown().await;
}

#[tokio::test]
async fn current_thread_rename_requires_the_verified_owner_even_for_approved_participants() {
    for sender in ["44", "66"] {
        let (manager, transport, mut c, model) = fixture(vec![
            Step::Tools(vec![admin_call(
                "rename_discord_thread",
                json!({"title":"Owner chosen title"}),
            )]),
            Step::Text(vec!["Result"]),
        ])
        .await;
        c.channel.kind = DiscordChannelKind::GuildThread;
        c.channel.parent_channel_id = Some("21".into());
        c.channel.name = Some("ongoing-topic".into());
        manager
            .state
            .write()
            .await
            .discord_connectors
            .insert(c.id.clone(), c.clone());
        transport
            .channels
            .lock()
            .unwrap()
            .insert(c.channel.id.clone(), c.channel.clone());
        {
            let mut inventory = transport.guild_channels.lock().unwrap();
            inventory.retain(|channel| channel.id != "22");
            inventory.push(channel("21", 0));
        }
        install_thread(&transport, "22", "21", "33");
        admit_as(&manager, &c, "101", sender).await;
        finish(&manager, &c, "101").await;
        let owner = sender == "44";
        assert_eq!(
            model.configs()[0].allows_tool("rename_discord_thread"),
            owner,
            "{}",
            result(&model)
        );
        assert_eq!(
            transport.rename_calls.load(Ordering::SeqCst),
            usize::from(owner),
            "{}",
            result(&model)
        );
        assert_eq!(
            mutations(&transport),
            usize::from(owner),
            "{}",
            result(&model)
        );
        assert_eq!(
            result(&model).contains("\"renamed\":true"),
            owner,
            "{}",
            result(&model)
        );
        if owner {
            assert_eq!(
                manager
                    .get(&c.agent_id, &c.id)
                    .await
                    .unwrap()
                    .channel
                    .name
                    .as_deref(),
                Some("Owner chosen title")
            );
        }
        manager.shutdown().await;
    }
}

#[tokio::test]
async fn owner_can_change_settings_reorder_and_manage_decimal_permission_overwrites() {
    let (manager, transport, c, model) = fixture(vec![
        Step::Tools(vec![admin_call(UPDATE, json!({"channel_id":"22","settings":{"nsfw":true,"rate_limit_per_user":15}}))]),
        Step::Tools(vec![admin_call("reorder_discord_channels", json!({"positions":[{"id":"22","position":2,"parent_id":"55","lock_permissions":true}]}))]),
        Step::Tools(vec![admin_call("set_discord_channel_permissions", json!({"channel_id":"22","target_id":"33","target_type":"role","allow":"1024","deny":"2048"}))]),
        Step::Tools(vec![admin_call("get_discord_channel_permissions", json!({"channel_id":"22","target_id":"33","target_type":"role"}))]),
        Step::Tools(vec![admin_call("delete_discord_channel_permission", json!({"channel_id":"22","target_id":"33"}))]),
        Step::Text(vec!["Result"]),
    ])
    .await;
    admit_as(&manager, &c, "101", "44").await;
    finish(&manager, &c, "101").await;
    assert_eq!(mutations(&transport), 4, "{}", result(&model));
    {
        let requests = transport.admin_requests.lock().unwrap();
        assert!(requests.iter().any(|request| request
            .body
            .as_ref()
            .is_some_and(|body| body.get("nsfw") == Some(&json!(true)))));
        assert!(requests.iter().any(|request| request
            .body
            .as_ref()
            .is_some_and(|body| body.get("allow") == Some(&json!("1024"))
                && body.get("deny") == Some(&json!("2048")))));
    }
    assert_eq!(
        manager
            .get(&c.agent_id, &c.id)
            .await
            .unwrap()
            .approved_user_ids,
        c.approved_user_ids
    );
    manager.shutdown().await;
}

#[tokio::test]
async fn owner_can_inspect_archive_lock_and_manage_thread_members() {
    let (manager, transport, c, model) = fixture(vec![
        Step::Tools(vec![admin_call(
            "list_discord_threads",
            json!({"scope":"active"}),
        )]),
        Step::Tools(vec![admin_call(
            "get_discord_channel",
            json!({"channel_id":"111"}),
        )]),
        Step::Tools(vec![admin_call(
            "update_discord_thread",
            json!({"channel_id":"111","archived":true,"locked":true,"auto_archive_duration":60}),
        )]),
        Step::Tools(vec![admin_call(
            "update_discord_thread",
            json!({"channel_id":"111","archived":false,"locked":false}),
        )]),
        Step::Tools(vec![admin_call(
            "manage_discord_thread_members",
            json!({"channel_id":"111","action":"add","user_id":"66"}),
        )]),
        Step::Tools(vec![admin_call(
            "manage_discord_thread_members",
            json!({"channel_id":"111","action":"list","limit":10}),
        )]),
        Step::Tools(vec![admin_call(
            "manage_discord_thread_members",
            json!({"channel_id":"111","action":"remove","user_id":"66"}),
        )]),
        Step::Text(vec!["Result"]),
    ])
    .await;
    install_thread(&transport, "111", "22", "33");
    admit_as(&manager, &c, "101", "44").await;
    finish(&manager, &c, "101").await;
    assert_eq!(mutations(&transport), 4, "{}", result(&model));
    let thread = transport.admin_channels.lock().unwrap()["111"].clone();
    assert_eq!(thread["thread_metadata"]["archived"], false);
    assert_eq!(thread["thread_metadata"]["locked"], false);
    manager.shutdown().await;
}

#[tokio::test]
async fn thread_update_accepts_provider_channel_objects_without_position() {
    let (manager, transport, c, model) = fixture(vec![
        Step::Tools(vec![admin_call(
            "update_discord_thread",
            json!({"channel_id":"111","archived":true}),
        )]),
        Step::Text(vec!["Result"]),
    ])
    .await;
    install_thread(&transport, "111", "22", "33");
    assert!(transport.admin_channels.lock().unwrap()["111"]
        .get("position")
        .is_none());
    admit_as(&manager, &c, "101", "44").await;
    finish(&manager, &c, "101").await;
    assert_eq!(mutations(&transport), 1, "{}", result(&model));
    assert!(
        result(&model).contains("\"updated\":true"),
        "{}",
        result(&model)
    );
    manager.shutdown().await;
}

#[tokio::test]
async fn manual_thread_name_persists_auto_title_suppression_even_when_the_effect_is_uncertain() {
    for uncertain in [false, true] {
        let (manager, transport, c, model) = fixture(vec![Step::Hold(vec!["waiting"])]).await;
        install_thread(&transport, "111", "22", "33");
        let (link, scope) = active_scope(&manager, &c, &model).await;
        let path = std::env::temp_dir().join(format!(
            "anima-discord-admin-title-{}.json",
            uuid::Uuid::new_v4()
        ));
        let store = crate::control_plane_store::ControlPlaneStoreConfig::Json(path.clone());
        {
            let mut state = manager.state.write().await;
            state
                .discord_connectors
                .get_mut(&c.id)
                .unwrap()
                .conversations
                .push(DiscordConversation {
                    channel: DiscordChannelMetadata {
                        id: "111".into(),
                        guild_id: "33".into(),
                        name: Some("ongoing-topic".into()),
                        kind: DiscordChannelKind::GuildThread,
                        parent_channel_id: Some("22".into()),
                    },
                    room_id: format!("discord:{}:111", c.id),
                    cursor: "0".into(),
                    participating: true,
                    auto_title: Some(DiscordThreadTitle {
                        initial_name: "ongoing-topic".into(),
                        attempted: false,
                    }),
                });
            state.set_control_plane_store(Some(store.clone()));
        }
        if uncertain {
            transport
                .admin_write_responses
                .lock()
                .unwrap()
                .push_back(Err(DiscordTransportError::Uncertain));
        }
        let response = scope
            .execute_admin(
                &link,
                "update_discord_thread",
                json!({"channel_id":"111","name":"Owner chosen title"}),
                None,
            )
            .await;
        if uncertain {
            assert!(response.unwrap_err().contains("uncertain"));
            let retry = scope
                .execute_admin(
                    &link,
                    "set_discord_voice_status",
                    json!({"channel_id":"77","status":"Retry"}),
                    None,
                )
                .await
                .unwrap_err();
            assert!(retry.contains("new explicit owner message"), "{retry}");
        } else {
            assert_eq!(response.unwrap()["updated"], true);
        }
        assert_eq!(mutations(&transport), 1);
        let expected_name = if uncertain {
            "ongoing-topic"
        } else {
            "Owner chosen title"
        };
        let current = manager.get(&c.agent_id, &c.id).await.unwrap();
        let conversation = current
            .conversations
            .iter()
            .find(|conversation| conversation.channel.id == "111")
            .unwrap();
        assert!(conversation.auto_title.is_none());
        assert_eq!(conversation.channel.name.as_deref(), Some(expected_name));
        let snapshot = crate::control_plane_store::load_control_plane_snapshot(&store)
            .await
            .unwrap()
            .unwrap();
        let saved = snapshot
            .discord_connectors
            .iter()
            .find(|connector| connector.id == c.id)
            .unwrap()
            .conversations
            .iter()
            .find(|conversation| conversation.channel.id == "111")
            .unwrap();
        assert!(
            saved.auto_title.is_none(),
            "manual intent must stay suppressed in the persisted snapshot"
        );
        assert_eq!(saved.channel.name.as_deref(), Some(expected_name));
        stop_active_fixture(&manager, &link).await;
        std::fs::remove_file(path).unwrap();
    }
}

#[tokio::test]
async fn admin_thread_name_waits_for_the_automatic_title_lock_before_sending_a_patch() {
    let (manager, transport, c, model) = fixture(vec![Step::Hold(vec!["waiting"])]).await;
    install_thread(&transport, "111", "22", "33");
    let (link, scope) = active_scope(&manager, &c, &model).await;
    let mut thread_key = c.clone();
    thread_key.channel.id = "111".into();
    let title_lock = manager.title_lock(&thread_key);
    let automatic_title = title_lock.lock().await;
    let request_link = link.clone();
    let rename = tokio::spawn(async move {
        scope
            .execute_admin(
                &request_link,
                "update_discord_thread",
                json!({"channel_id":"111","name":"Owner chosen title"}),
                None,
            )
            .await
    });
    within(async {
        while transport.admin_requests.lock().unwrap().is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await;
    tokio::task::yield_now().await;
    assert!(
        !rename.is_finished(),
        "the admin operation must wait for the actual thread title lock"
    );
    assert_eq!(
        mutations(&transport),
        0,
        "a pending automatic title must exclude the manual patch"
    );
    assert_eq!(
        transport.admin_requests.lock().unwrap().len(),
        1,
        "only the initial inspection may complete while the title lock is held"
    );
    drop(automatic_title);
    let response = within(rename).await.unwrap().unwrap();
    assert_eq!(response["updated"], true);
    assert_eq!(mutations(&transport), 1);
    assert_eq!(
        transport.admin_channels.lock().unwrap()["111"]["name"],
        "Owner chosen title"
    );
    stop_active_fixture(&manager, &link).await;
}

#[tokio::test]
async fn owner_can_use_voice_follow_permission_archive_and_thread_creation_endpoints() {
    let (manager, transport, c, model) = fixture(vec![
        Step::Tools(vec![admin_call("set_discord_voice_status", json!({"channel_id":"77","status":"Ready"}))]),
        Step::Tools(vec![admin_call("follow_discord_announcement", json!({"channel_id":"99","target_channel_id":"22"}))]),
        Step::Tools(vec![admin_call("get_discord_channel_permissions", json!({"channel_id":"22","target_id":"33","target_type":"role"}))]),
        Step::Tools(vec![admin_call("get_discord_channel_permissions", json!({"channel_id":"22","target_id":"66","target_type":"member"}))]),
        Step::Tools(vec![admin_call("list_discord_threads", json!({"scope":"public_archived","channel_id":"22","limit":10}))]),
        Step::Tools(vec![admin_call("list_discord_threads", json!({"scope":"private_archived","channel_id":"22","limit":10}))]),
        Step::Tools(vec![admin_call("list_discord_threads", json!({"scope":"joined_private_archived","channel_id":"22","limit":10}))]),
        Step::Tools(vec![admin_call("manage_discord_thread_members", json!({"channel_id":"111","action":"get","user_id":"66"}))]),
        Step::Tools(vec![admin_call("manage_discord_thread_members", json!({"channel_id":"111","action":"join"}))]),
        Step::Tools(vec![admin_call("manage_discord_thread_members", json!({"channel_id":"111","action":"leave"}))]),
        Step::Tools(vec![admin_call("create_discord_thread", json!({"channel_id":"22","name":"private-topic","type":"private","auto_archive_duration":60,"invitable":true,"rate_limit_per_user":15}))]),
        Step::Tools(vec![admin_call("create_discord_thread", json!({"channel_id":"22","name":"message-topic","message_id":"222"}))]),
        Step::Tools(vec![admin_call("create_discord_thread", json!({"channel_id":"88","name":"tagged-post","starter_message":"Start here","auto_archive_duration":60,"rate_limit_per_user":30,"applied_tags":["123"]}))]),
        Step::Text(vec!["Result"]),
    ]).await;
    {
        let mut state = manager.state.write().await;
        let mut config = state.agents[&c.agent_id].config().clone();
        config
            .settings
            .get_or_insert_with(Default::default)
            .max_tool_iterations = Some(16);
        state.restore_agent_config(&c.agent_id, config);
    }
    transport
        .guild_channels
        .lock()
        .unwrap()
        .push(channel("99", 5));
    let mut forum = serde_json::to_value(channel("88", 15)).unwrap();
    forum["available_tags"] = json!([{"id":"123","name":"Useful","moderated":false}]);
    transport
        .admin_channels
        .lock()
        .unwrap()
        .insert("88".into(), forum);
    install_thread(&transport, "111", "22", "33");
    admit_as(&manager, &c, "101", "44").await;
    finish(&manager, &c, "101").await;
    assert_eq!(mutations(&transport), 7, "{}", result(&model));
    let output = result(&model);
    for receipt in [
        "\"statusUpdated\":true",
        "\"followed\":true",
        "\"listed\":true",
        "\"created\":true",
    ] {
        assert!(output.contains(receipt), "missing {receipt}: {output}");
    }
    assert!(!output.contains("uncertain"), "{output}");
    {
        let requests = transport.admin_requests.lock().unwrap();
        for name in ADMIN_TOOLS {
            assert!(model.configs()[0].allows_tool(name), "{name}");
        }
        assert!(requests.iter().any(|request| request
            .body
            .as_ref()
            .is_some_and(|body| body.get("invitable") == Some(&json!(true)))));
    }
    manager.shutdown().await;
}

#[tokio::test]
async fn archived_thread_inspection_rejects_a_response_for_another_parent_in_the_same_guild() {
    let (manager, transport, c, model) = fixture(vec![Step::Hold(vec!["waiting"])]).await;
    let (link, scope) = active_scope(&manager, &c, &model).await;
    transport.admin_responses.lock().unwrap().extend([
        Ok(serde_json::to_value(channel("22", 0)).unwrap()),
        Ok(json!({"threads":[{"id":"111","guild_id":"33","parent_id":"88","type":11,"name":"wrong-parent", "thread_metadata":{"archived":true,"locked":false,"auto_archive_duration":1440}}],"members":[],"has_more":false})),
    ]);
    let response = scope
        .execute_admin(
            &link,
            "list_discord_threads",
            json!({"scope":"public_archived","channel_id":"22"}),
            None,
        )
        .await;
    assert!(
        response.is_err(),
        "wrong-parent archive response was accepted: {response:?}"
    );
    assert_eq!(mutations(&transport), 0);
    stop_active_fixture(&manager, &link).await;
}

#[tokio::test]
async fn thread_member_inspection_rejects_mismatched_thread_and_user_ids() {
    for member in [
        json!({"id":"112","user_id":"66","join_timestamp":"2026-11-14T00:00:00Z","flags":0}),
        json!({"id":"111","user_id":"99","join_timestamp":"2026-11-14T00:00:00Z","flags":0}),
    ] {
        let (manager, transport, c, model) = fixture(vec![Step::Hold(vec!["waiting"])]).await;
        install_thread(&transport, "111", "22", "33");
        let thread = transport.admin_channels.lock().unwrap()["111"].clone();
        let (link, scope) = active_scope(&manager, &c, &model).await;
        transport.admin_responses.lock().unwrap().extend([
            Ok(thread.clone()),
            Ok(thread),
            Ok(member),
        ]);
        let response = scope
            .execute_admin(
                &link,
                "manage_discord_thread_members",
                json!({"channel_id":"111","action":"get","user_id":"66"}),
                None,
            )
            .await;
        assert!(
            response.is_err(),
            "mismatched member was accepted: {response:?}"
        );
        assert_eq!(mutations(&transport), 0);
        assert_eq!(
            transport.admin_requests.lock().unwrap().len(),
            3,
            "the malformed member must reach acknowledgement validation after both thread reads"
        );
        stop_active_fixture(&manager, &link).await;
    }
}

#[tokio::test]
async fn forum_starter_suppresses_everyone_role_and_user_mentions() {
    let starter = "@everyone <@&66> <@44>";
    let (manager, transport, c, model) = fixture(vec![
        Step::Tools(vec![admin_call(
            "create_discord_thread",
            json!({"channel_id":"88","name":"new-topic","starter_message":starter}),
        )]),
        Step::Text(vec!["Result"]),
    ])
    .await;
    admit_as(&manager, &c, "101", "44").await;
    finish(&manager, &c, "101").await;
    assert_eq!(mutations(&transport), 1, "{}", result(&model));
    {
        let requests = transport.admin_requests.lock().unwrap();
        let message = requests
            .iter()
            .filter_map(|request| request.body.as_ref())
            .find_map(|body| body.get("message"))
            .expect("forum creation carries its starter in the same request");
        assert_eq!(message["content"], starter);
        assert_eq!(
            message["allowed_mentions"],
            json!({"parse":[],"replied_user":false})
        );
    }
    manager.shutdown().await;
}

#[test]
fn saved_admin_descriptors_do_not_grant_console_or_thread_authority() {
    let mut config = crate::agent_runs::test_support::companion_config("Owner channel management");
    config.tools = Some(
        crate::tools::ToolRegistry::new()
            .resolve_descriptors(ADMIN_TOOLS)
            .unwrap(),
    );
    for scope in [
        crate::agent_runs::tool_scope::Scope::default(),
        crate::agent_runs::tool_scope::Scope {
            owner_chat: true,
            discord_thread: true,
            ..Default::default()
        },
    ] {
        let tools = crate::agent_runs::tool_scope::select(&config, &[], scope);
        assert!(tools
            .iter()
            .all(|tool| !ADMIN_TOOLS.contains(&tool.name.as_str())));
    }
}

#[tokio::test]
async fn approved_nonowner_cannot_invoke_any_admin_tool_with_spoofed_owner_text() {
    let calls = ADMIN_TOOLS
        .iter()
        .map(|name| admin_call(name, json!({})))
        .collect();
    let (manager, transport, c, model) =
        fixture(vec![Step::Tools(calls), Step::Text(vec!["Denied"])]).await;
    admit_as(&manager, &c, "101", "66").await;
    finish(&manager, &c, "101").await;
    for config in model.configs() {
        for name in ADMIN_TOOLS {
            assert!(!config.allows_tool(name), "unexpected authority for {name}");
        }
    }
    assert!(
        transport.admin_requests.lock().unwrap().is_empty(),
        "{}",
        result(&model)
    );
    manager.shutdown().await;
}

#[tokio::test]
async fn console_owner_run_cannot_invoke_any_new_admin_operation() {
    let calls = ADMIN_TOOLS
        .iter()
        .map(|name| admin_call(name, json!({})))
        .collect();
    let (manager, transport, c, model) =
        fixture(vec![Step::Tools(calls), Step::Text(vec!["Denied"])]).await;
    let owner_manager = manager.clone();
    let owner_c = c.clone();
    let start = crate::agent_runs::QueuedRunStart::new(move |run_id| {
        Box::pin(async move {
            owner_manager
                .send_from_owner_accepted(
                    owner_c.agent_id,
                    owner_c.id,
                    "Manage channels".into(),
                    "console-admin-test".into(),
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
                session_id: c.room_id.clone(),
                text: "Manage channels".into(),
                attachment_ids: vec![],
                idempotency_key: "console-admin-test".into(),
                mode: crate::agent_runs::SessionRunMode::Queue,
                source: crate::runs::RunSource::Discord,
                source_ref: Some(c.id.clone()),
                skill: None,
            },
            start,
        )
        .await
        .unwrap();
    let run_id = match accepted {
        crate::agent_runs::AcceptedRun::Created(run) => run.id,
        other => panic!("expected new console run: {other:?}"),
    };
    within(async {
        while manager
            .state
            .read()
            .await
            .runs
            .get(&run_id)
            .is_some_and(|run| !run.status.is_terminal())
        {
            tokio::task::yield_now().await;
        }
    })
    .await;
    for config in model.configs() {
        for name in ADMIN_TOOLS {
            assert!(!config.allows_tool(name), "console gained {name}");
        }
    }
    assert!(
        transport.admin_requests.lock().unwrap().is_empty(),
        "{}",
        result(&model)
    );
    manager.shutdown().await;
}

#[tokio::test]
async fn invalid_payloads_and_foreign_thread_parents_never_send_mutations() {
    let calls = vec![
        delete_call("999", "foreign"),
        admin_call(
            "reorder_discord_channels",
            json!({"positions":[{"id":"22","parent_id":"77"}]}),
        ),
        admin_call(
            "set_discord_channel_permissions",
            json!({"channel_id":"22","target_id":"33","target_type":"role","allow":1024,"deny":"0"}),
        ),
        admin_call(
            "set_discord_channel_permissions",
            json!({"channel_id":"22","target_id":"33","target_type":"role","allow":"-1","deny":"0"}),
        ),
        admin_call(
            "set_discord_channel_permissions",
            json!({"channel_id":"22","target_id":"33","target_type":"role","allow":"1","deny":"1"}),
        ),
        admin_call(
            "update_discord_thread",
            json!({"channel_id":"22","archived":true}),
        ),
        admin_call(
            "update_discord_thread",
            json!({"channel_id":"111","locked":true}),
        ),
        admin_call(
            "update_discord_thread",
            json!({"channel_id":"112","locked":true}),
        ),
        admin_call(
            "set_discord_voice_status",
            json!({"channel_id":"22","status":"incorrect type"}),
        ),
        admin_call(
            "follow_discord_announcement",
            json!({"channel_id":"22","target_channel_id":"77"}),
        ),
    ];
    for request in calls {
        let name = request.name.clone();
        let (manager, transport, c, model) =
            fixture(vec![Step::Tools(vec![request]), Step::Text(vec!["Denied"])]).await;
        install_thread(&transport, "111", "999", "33");
        install_thread(&transport, "112", "22", "99");
        admit_as(&manager, &c, "101", "44").await;
        finish(&manager, &c, "101").await;
        assert_eq!(mutations(&transport), 0, "{name}: {}", result(&model));
        manager.shutdown().await;
    }
}

#[tokio::test]
async fn uncertain_admin_write_blocks_another_mutation_but_inspection_remains_available() {
    let (manager, transport, c, model) = fixture(vec![
        Step::Tools(vec![delete_call("77", "channel-77")]),
        Step::Tools(vec![admin_call(
            "set_discord_voice_status",
            json!({"channel_id":"77","status":"another mutation"}),
        )]),
        Step::Tools(vec![admin_call(
            "get_discord_channel",
            json!({"channel_id":"22"}),
        )]),
        Step::Text(vec!["Result"]),
    ])
    .await;
    transport
        .admin_write_responses
        .lock()
        .unwrap()
        .push_back(Err(DiscordTransportError::Transport));
    admit_as(&manager, &c, "101", "44").await;
    finish(&manager, &c, "101").await;
    assert_eq!(mutations(&transport), 1, "{}", result(&model));
    assert!(
        result(&model).contains("new explicit owner message"),
        "{}",
        result(&model)
    );
    assert!(
        result(&model).contains("channel-22"),
        "inspection should remain available: {}",
        result(&model)
    );
    manager.shutdown().await;
}

#[tokio::test]
async fn mismatched_or_empty_delete_acknowledgements_are_uncertain_and_never_replayed() {
    for acknowledgement in [
        Value::Null,
        json!({"id":"999","guild_id":"33","type":2,"name":"channel-77"}),
    ] {
        let (manager, transport, c, model) = fixture(vec![
            Step::Tools(vec![delete_call("77", "channel-77")]),
            Step::Tools(vec![delete_call("77", "channel-77")]),
            Step::Text(vec!["Result"]),
        ])
        .await;
        transport
            .admin_write_responses
            .lock()
            .unwrap()
            .push_back(Ok(acknowledgement));
        admit_as(&manager, &c, "101", "44").await;
        finish(&manager, &c, "101").await;
        assert_eq!(mutations(&transport), 1, "{}", result(&model));
        assert!(result(&model).contains("uncertain"), "{}", result(&model));
        assert!(
            !result(&model).contains("\"deleted\":true"),
            "{}",
            result(&model)
        );
        manager.shutdown().await;
    }
}

async fn stop_active_fixture(manager: &DiscordManager, link: &crate::runs::RunLink) {
    if let Some(control) = manager.state.read().await.live.runs().control(&link.run_id) {
        control.cancel.cancel();
    }
    within(manager.runs.shutdown()).await;
    manager.shutdown().await;
}

#[tokio::test]
async fn revoked_capability_blocks_every_new_admin_operation_before_provider_reads() {
    let (manager, transport, c, model) = fixture(vec![Step::Hold(vec!["waiting"])]).await;
    let (link, scope) = active_scope(&manager, &c, &model).await;
    manager.revoke(&c.id);
    for (name, args) in valid_admin_operations() {
        assert!(
            scope.execute_admin(&link, name, args, None).await.is_err(),
            "{name}"
        );
    }
    assert!(transport.admin_requests.lock().unwrap().is_empty());
    stop_active_fixture(&manager, &link).await;
}

#[tokio::test]
async fn changed_application_owner_blocks_every_new_admin_operation() {
    let (manager, transport, c, model) = fixture(vec![Step::Hold(vec!["waiting"])]).await;
    let (link, scope) = active_scope(&manager, &c, &model).await;
    let mut check = transport
        .check_application(&DiscordBotToken::parse("test.token").unwrap())
        .await
        .unwrap();
    check.management_owner_user_id = Some("66".into());
    *transport.application_check.lock().unwrap() = Some(Ok(check));
    for (name, args) in valid_admin_operations() {
        assert!(
            scope.execute_admin(&link, name, args, None).await.is_err(),
            "{name}"
        );
    }
    assert!(transport.admin_requests.lock().unwrap().is_empty());
    stop_active_fixture(&manager, &link).await;
}

#[tokio::test]
async fn deletion_metadata_save_failure_reports_provider_success_and_fences_mutations() {
    let (manager, transport, c, model) = fixture(vec![Step::Hold(vec!["waiting"])]).await;
    let (link, scope) = active_scope(&manager, &c, &model).await;
    let gate = manager
        .state
        .write()
        .await
        .install_test_control_plane_save_gate(true);
    let running_scope = scope.clone();
    let running_link = link.clone();
    let deletion = tokio::spawn(async move {
        running_scope
            .execute_admin(
                &running_link,
                "delete_discord_channel",
                json!({"channel_id":"77","expected_name":"channel-77"}),
                None,
            )
            .await
    });
    within(gate.entered.acquire()).await.unwrap().forget();
    gate.release.add_permits(1);
    let error = within(deletion).await.unwrap().unwrap_err();
    assert!(
        (error.contains("Discord accepted") || error.contains("Discord deleted"))
            && error.contains("metadata"),
        "{error}"
    );
    let retry = scope
        .execute_admin(
            &link,
            UPDATE,
            json!({"channel_id":"22","settings":{"nsfw":true}}),
            None,
        )
        .await
        .unwrap_err();
    assert!(retry.contains("new explicit owner message"), "{retry}");
    assert_eq!(mutations(&transport), 1);
    assert!(!transport
        .guild_channels
        .lock()
        .unwrap()
        .iter()
        .any(|channel| channel.id == "77"));
    stop_active_fixture(&manager, &link).await;
}

#[tokio::test]
async fn admin_catalog_discovery_withdraws_every_descriptor_after_revocation() {
    use anima_core::ModelToolSelector;
    let (manager, _, c, model) = fixture(vec![Step::Hold(vec!["waiting"])]).await;
    let (link, scope) = active_scope(&manager, &c, &model).await;
    let mut agent = manager
        .state
        .read()
        .await
        .get_agent(&c.agent_id)
        .unwrap()
        .state;
    let mut original = agent.config.clone();
    original
        .settings
        .as_mut()
        .unwrap()
        .additional
        .insert("toolDiscovery".into(), DataValue::Bool(true));
    agent.config.tools = Some(
        crate::tools::ToolRegistry::new()
            .resolve_descriptors(ADMIN_TOOLS)
            .unwrap(),
    );
    let catalog = crate::tools::catalog::RunToolCatalog::new(
        &manager.state,
        link.clone(),
        anima_core::CancelSignal::default(),
        &original,
        &agent,
        None,
    )
    .with_discord_guild(Some(scope));
    for name in ADMIN_TOOLS {
        assert!(catalog.visible_tool_names().contains(name), "{name}");
        assert!(
            catalog.describe(&agent, &link, name).await.is_ok(),
            "{name}"
        );
    }
    manager.revoke(&c.id);
    for name in ADMIN_TOOLS {
        assert!(!catalog.visible_tool_names().contains(name), "{name}");
        assert!(catalog.check(&agent, &link, name).await.is_err(), "{name}");
        assert!(
            catalog.describe(&agent, &link, name).await.is_err(),
            "{name}"
        );
    }
    stop_active_fixture(&manager, &link).await;
}
