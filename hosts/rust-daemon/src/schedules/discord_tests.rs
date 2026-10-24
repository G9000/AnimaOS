use super::tests::service_with_daemon;
use super::*;
use crate::agent_runs::test_support::{Gate, ScriptedModel, Step};
use crate::connectors::discord::DiscordConnectorRecord;
use crate::state::DaemonState;

fn connection(agent_id: &str, home: Option<&str>) -> DiscordConnectorRecord {
    serde_json::from_value(serde_json::json!({
        "id": "discord-scheduled", "agentId": agent_id,
        "roomId": "discord:discord-scheduled",
        "bot": {"id": "11", "username": "fixture", "displayName": null},
        "channel": {"id": "42", "guildId": "7", "name": "home", "kind": "guildText"},
        "approvedUserIds": ["9"], "credentialGeneration": 1, "configurationRevision": 1,
        "cursor": "0", "activatedAtMs": 1, "enabled": true,
        "deletedAtMs": null, "createdAtMs": 1, "updatedAtMs": 1,
        "settings": {"allowedChannelIds": [], "freeResponseChannelIds": [],
            "approvedRoleIds": [], "homeChannelId": home, "requireMention": true,
            "autoThread": true, "threadRequireMention": false},
        "botWide": false, "applicationId": "11", "conversations": []
    }))
    .unwrap()
}

async fn schedule(service: &SchedulerService, agent: &str) -> ScheduledPromptRecord {
    service
        .create(
            agent.into(),
            "Check status".into(),
            ScheduleTrigger::Interval {
                interval_ms: 60_000,
            },
            ScheduleTarget::Connector {
                connector_id: "discord-scheduled".into(),
            },
            true,
            None,
            Some(2),
            Some(1),
        )
        .await
        .expect("an enabled owned Discord connection with an explicit home is a schedule target")
        .0
}

#[tokio::test]
async fn discord_home_schedule_queues_output_in_a_separate_schedule_session_and_restores() {
    home_schedule_queues_and_restores(false).await;
}

#[tokio::test]
async fn discord_bot_wide_home_schedule_delivers_without_a_selected_channel() {
    home_schedule_queues_and_restores(true).await;
}

async fn home_schedule_queues_and_restores(bot_wide: bool) {
    let daemon =
        DaemonState::with_model_adapter(ScriptedModel::new(vec![Step::Text(vec!["Heads up"])]));
    let (service, state, agent, _) = service_with_daemon(daemon);
    let mut connector = connection(&agent, Some("42"));
    if bot_wide {
        connector.bot_wide = true;
        connector
            .conversations
            .push(crate::connectors::discord::DiscordConversation {
                channel: connector.channel.clone(),
                room_id: "discord:discord-scheduled:42".into(),
                cursor: "0".into(),
                participating: false,
                auto_title: None,
            });
        connector.channel.id.clear();
        connector.channel.guild_id.clear();
        connector.channel.name = None;
    }
    let destination_room = connector.home_destination().unwrap().room_id;
    state
        .write()
        .await
        .discord_connectors
        .insert(connector.id.clone(), connector.clone());
    let record = schedule(&service, &agent).await;
    assert_eq!(service.tick_at(2).await.unwrap(), 1);
    let snapshot = {
        let state = state.read().await;
        assert_eq!(
            state.schedules[&record.id]
                .last_safe_outcome
                .as_ref()
                .unwrap()
                .status,
            ScheduleOutcomeStatus::Spoke
        );
        let outbound = state
            .discord_outbound
            .values()
            .next()
            .expect("durable native Discord delivery");
        assert_eq!(outbound.connector_id, connector.id);
        assert_eq!(outbound.room_id, destination_room);
        assert_eq!(outbound.text, "Heads up");
        assert_eq!(
            outbound.delivery_state,
            crate::connectors::discord::DiscordDeliveryState::Pending
        );
        assert!(state.outbound.is_empty());
        let assistant = state
            .get_agent(&agent)
            .unwrap()
            .messages
            .into_iter()
            .find(|m| m.id == outbound.assistant_message_id)
            .unwrap();
        assert_eq!(
            assistant.room_id,
            crate::sessions::schedule_room_id(&record.id)
        );
        state.control_plane_snapshot()
    };
    let mut restored = DaemonState::new();
    restored
        .restore_control_plane_snapshot(snapshot)
        .expect("Discord-targeted schedules and their native outbox survive restart");
    assert_eq!(restored.discord_outbound.len(), 1);
    assert!(restored.schedules[&record.id].enabled);
}

#[tokio::test]
async fn discord_home_schedule_keeps_silent_checkins_out_of_the_outbox() {
    let daemon = DaemonState::with_model_adapter(ScriptedModel::new(vec![Step::Text(vec![
        CHECKIN_SENTINEL,
    ])]));
    let (service, state, agent, _) = service_with_daemon(daemon);
    let connector = connection(&agent, Some("42"));
    state
        .write()
        .await
        .discord_connectors
        .insert(connector.id.clone(), connector);
    let record = schedule(&service, &agent).await;
    assert_eq!(service.tick_at(2).await.unwrap(), 1);
    let state = state.read().await;
    assert_eq!(
        state.schedules[&record.id]
            .last_safe_outcome
            .as_ref()
            .unwrap()
            .status,
        ScheduleOutcomeStatus::Silent
    );
    assert!(state.discord_outbound.is_empty());
}

#[tokio::test]
async fn discord_home_schedule_rejects_changed_generation_before_output_commit() {
    changed_destination_does_not_deliver(0).await;
}

#[tokio::test]
async fn discord_home_schedule_rejects_changed_home_before_output_commit() {
    changed_destination_does_not_deliver(1).await;
}

#[tokio::test]
async fn discord_home_schedule_rejects_configuration_changed_away_and_back() {
    changed_destination_does_not_deliver(2).await;
}

async fn changed_destination_does_not_deliver(change: u8) {
    let gate = Gate::new();
    let daemon = DaemonState::with_model_adapter(ScriptedModel::gated(
        vec![Step::Text(vec!["Heads up"])],
        gate.clone(),
    ));
    let (service, state, agent, _) = service_with_daemon(daemon);
    let connector = connection(&agent, Some("42"));
    state
        .write()
        .await
        .discord_connectors
        .insert(connector.id.clone(), connector);
    let record = schedule(&service, &agent).await;
    let tick = tokio::spawn({
        let service = service.clone();
        async move { service.tick_at(2).await }
    });
    gate.entered().await;
    {
        let mut state = state.write().await;
        let connector = state
            .discord_connectors
            .get_mut("discord-scheduled")
            .unwrap();
        if change == 1 {
            let mut changed = serde_json::to_value(&*connector).unwrap();
            changed["settings"]["homeChannelId"] = serde_json::Value::Null;
            *connector = serde_json::from_value(changed).unwrap();
        } else if change == 0 {
            connector.credential_generation += 1;
        } else {
            let mut changed = serde_json::to_value(&*connector).unwrap();
            changed["configurationRevision"] = serde_json::json!(2);
            *connector = serde_json::from_value(changed).unwrap();
        }
    }
    gate.release();
    tokio::time::timeout(Duration::from_secs(5), tick)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let state = state.read().await;
    assert!(state.discord_outbound.is_empty());
    assert_eq!(
        state.schedules[&record.id]
            .last_safe_outcome
            .as_ref()
            .unwrap()
            .status,
        ScheduleOutcomeStatus::Failed
    );
}

#[tokio::test]
async fn discord_schedule_requires_an_explicit_home() {
    let (service, state, agent, _) = service_with_daemon(DaemonState::new());
    let connector = connection(&agent, None);
    state
        .write()
        .await
        .discord_connectors
        .insert(connector.id.clone(), connector);
    assert!(service
        .create(
            agent,
            "Check".into(),
            ScheduleTrigger::Interval {
                interval_ms: 60_000
            },
            ScheduleTarget::Connector {
                connector_id: "discord-scheduled".into()
            },
            true,
            None,
            Some(2),
            Some(1)
        )
        .await
        .is_err());
}

#[tokio::test]
async fn discord_automation_target_selects_only_the_owners_active_explicit_home() {
    let (service, state, agent, _) = service_with_daemon(DaemonState::new());
    let connector = connection(&agent, None);
    state
        .write()
        .await
        .discord_connectors
        .insert(connector.id.clone(), connector);
    assert_eq!(service.automations().discord_target(&agent).await, None);
    let connector = connection(&agent, Some("42"));
    state
        .write()
        .await
        .discord_connectors
        .insert(connector.id.clone(), connector);
    assert_eq!(
        service.automations().discord_target(&agent).await,
        Some(ScheduleTarget::Connector {
            connector_id: "discord-scheduled".into()
        })
    );
    assert_eq!(
        service.automations().discord_target("someone-else").await,
        None
    );
    state
        .write()
        .await
        .discord_connectors
        .get_mut("discord-scheduled")
        .unwrap()
        .enabled = false;
    assert_eq!(service.automations().discord_target(&agent).await, None);
}

#[tokio::test]
async fn discord_schedule_snapshot_validates_home_and_agent_ownership() {
    let (service, state, agent, _) = service_with_daemon(DaemonState::new());
    let connector = connection(&agent, Some("42"));
    state
        .write()
        .await
        .discord_connectors
        .insert(connector.id.clone(), connector);
    let record = schedule(&service, &agent).await;
    let snapshot = state.read().await.control_plane_snapshot();
    let mut missing_home = snapshot.clone();
    missing_home.discord_connectors[0]
        .settings
        .as_mut()
        .unwrap()
        .home_channel_id = None;
    assert!(DaemonState::new()
        .restore_control_plane_snapshot(missing_home.clone())
        .is_err());
    missing_home
        .schedules
        .iter_mut()
        .find(|schedule| schedule.id == record.id)
        .unwrap()
        .enabled = false;
    DaemonState::new()
        .restore_control_plane_snapshot(missing_home)
        .expect("a paused automation can keep its unavailable Discord target");
    let mut wrong_owner = snapshot;
    wrong_owner
        .schedules
        .iter_mut()
        .find(|schedule| schedule.id == record.id)
        .unwrap()
        .agent_id = "someone-else".into();
    assert!(DaemonState::new()
        .restore_control_plane_snapshot(wrong_owner)
        .is_err());
}

#[tokio::test]
async fn discord_tombstoned_schedule_history_does_not_keep_a_deleted_agent_alive() {
    let (service, state, agent, _) = service_with_daemon(DaemonState::new());
    let connector = connection(&agent, Some("42"));
    state
        .write()
        .await
        .discord_connectors
        .insert(connector.id.clone(), connector);
    let record = schedule(&service, &agent).await;
    let mut snapshot = state.read().await.control_plane_snapshot();
    let connector = &mut snapshot.discord_connectors[0];
    connector.enabled = false;
    connector.deleted_at_ms = Some(20);
    connector.updated_at_ms = 20;
    snapshot
        .schedules
        .iter_mut()
        .find(|schedule| schedule.id == record.id)
        .unwrap()
        .enabled = false;
    snapshot.agents.clear();
    let mut restored = DaemonState::new();
    restored
        .restore_control_plane_snapshot(snapshot)
        .expect("archived Discord schedule history must not retain a deleted agent");
    assert!(!restored.schedules[&record.id].enabled);
    assert!(restored.discord_connectors["discord-scheduled"]
        .deleted_at_ms
        .is_some());
}

#[tokio::test]
async fn discord_pending_token_replacement_restores_enabled_schedule_without_running_it() {
    use crate::connectors::discord::{DiscordCredentialIntent, DiscordCredentialOperation};
    let (service, state, agent, _) = service_with_daemon(DaemonState::new());
    let connector = connection(&agent, Some("42"));
    state
        .write()
        .await
        .discord_connectors
        .insert(connector.id.clone(), connector);
    let record = schedule(&service, &agent).await;
    let mut snapshot = state.read().await.control_plane_snapshot();
    snapshot.discord_connectors[0].enabled = false;
    snapshot.discord_connectors[0].credential_generation = 2;
    snapshot
        .discord_credential_intents
        .push(DiscordCredentialIntent {
            connector_id: "discord-scheduled".into(),
            generation: 2,
            operation: DiscordCredentialOperation::Replace,
            created_at_ms: 2,
        });
    for invalid in 0..3 {
        let mut unrelated = snapshot.clone();
        match invalid {
            0 => unrelated.discord_credential_intents.clear(),
            1 => unrelated.discord_credential_intents[0].generation = 1,
            _ => {
                unrelated.discord_credential_intents[0].operation =
                    DiscordCredentialOperation::Delete
            }
        }
        assert!(
            DaemonState::new()
                .restore_control_plane_snapshot(unrelated)
                .is_err(),
            "only the exact pending Replace intent may preserve an enabled schedule"
        );
    }
    let model = ScriptedModel::new(vec![Step::Fail(
        "inactive replacement must not call the model",
    )]);
    let mut restored = DaemonState::with_model_adapter(model.clone());
    restored
        .restore_control_plane_snapshot(snapshot)
        .expect("pending token replacement is a valid interrupted snapshot");
    assert!(restored.schedules[&record.id].enabled);
    let (restored_service, restored_state, _, _) = service_with_daemon(restored);
    assert_eq!(restored_service.tick_at(2).await.unwrap(), 1);
    assert!(model.requests().is_empty());
    let state = restored_state.read().await;
    assert!(state.discord_outbound.is_empty());
    assert_eq!(
        state.schedules[&record.id]
            .last_safe_outcome
            .as_ref()
            .unwrap()
            .error_code
            .as_deref(),
        Some("schedule_target_unavailable")
    );
}

#[tokio::test]
async fn discord_schedule_failed_commit_rolls_back_only_its_new_outbound() {
    let gate = Gate::new();
    let daemon = DaemonState::with_model_adapter(ScriptedModel::gated(
        vec![Step::Text(vec!["Heads up"])],
        gate.clone(),
    ));
    let (service, state, agent, _) = service_with_daemon(daemon);
    let connector = connection(&agent, Some("42"));
    let existing = outbound_for(&connector, "earlier-reply", "Keep this delivery", false);
    {
        let mut state = state.write().await;
        state
            .discord_connectors
            .insert(connector.id.clone(), connector);
        state
            .discord_outbound
            .insert(existing.id.clone(), existing.clone());
    }
    let record = schedule(&service, &agent).await;
    let tick = tokio::spawn({
        let service = service.clone();
        async move { service.tick_at(2).await }
    });
    gate.entered().await;
    let blocked_path = std::env::temp_dir().join(format!(
        "anima-discord-schedule-commit-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir(&blocked_path).unwrap();
    struct BlockedPath(std::path::PathBuf);
    impl Drop for BlockedPath {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir(&self.0);
        }
    }
    let _blocked = BlockedPath(blocked_path.clone());
    state.write().await.set_control_plane_store(Some(
        crate::control_plane_store::ControlPlaneStoreConfig::Json(blocked_path),
    ));
    gate.release();
    tokio::time::timeout(Duration::from_secs(5), tick)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let state = state.read().await;
    assert_eq!(state.discord_outbound.len(), 1);
    assert_eq!(state.discord_outbound.get(&existing.id), Some(&existing));
    assert_ne!(
        state.schedules[&record.id]
            .last_safe_outcome
            .as_ref()
            .map(|outcome| &outcome.status),
        Some(&ScheduleOutcomeStatus::Spoke)
    );
}

#[tokio::test]
async fn discord_schedule_respects_native_outbox_capacity() {
    let daemon =
        DaemonState::with_model_adapter(ScriptedModel::new(vec![Step::Text(vec!["Heads up"])]));
    let (service, state, agent, _) = service_with_daemon(daemon);
    let connector = connection(&agent, Some("42"));
    {
        let mut state = state.write().await;
        state
            .discord_connectors
            .insert(connector.id.clone(), connector.clone());
        for index in 0..crate::connectors::discord::MAX_OUTBOUND {
            let item = outbound_for(
                &connector,
                &format!("fixture-{index}"),
                "Already queued",
                false,
            );
            state.discord_outbound.insert(item.id.clone(), item);
        }
    }
    let record = schedule(&service, &agent).await;
    service.tick_at(2).await.unwrap();
    let state = state.read().await;
    assert_eq!(
        state.discord_outbound.len(),
        crate::connectors::discord::MAX_OUTBOUND
    );
    assert!(state
        .discord_outbound
        .values()
        .all(|item| item.text == "Already queued"));
    assert_eq!(
        state.schedules[&record.id]
            .last_safe_outcome
            .as_ref()
            .unwrap()
            .status,
        ScheduleOutcomeStatus::Failed
    );
}
