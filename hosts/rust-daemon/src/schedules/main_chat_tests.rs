//! An automation that speaks leaves its card in the main chat; one-time
//! reminders can be marked done or snoozed, and leave the list a day after
//! they finish (reminders design, section 10).

use std::sync::Arc;

use anima_core::{AgentConfig, ModelGenerateRequest, ModelGenerateResponse};
use async_trait::async_trait;

use super::tests::service_with_daemon;
use super::*;
use crate::sessions::{SessionKind, SessionOrigin, SessionRecord, TitleSource};
use crate::state::DaemonState;

const MAIN_ROOM: &str = "chat:main";

/// Answers every model call with `text`.
struct Says(&'static str);

#[async_trait]
impl anima_core::ModelAdapter for Says {
    fn single_attempt_contract(&self) -> bool {
        true
    }
    fn generated_usage_is_complete(&self, _: &anima_core::ModelGenerateResponse) -> bool {
        true
    }
    fn provider(&self) -> &str {
        "test"
    }
    async fn generate(
        &self,
        _: &AgentConfig,
        _: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        Ok(ModelGenerateResponse {
            content: Content {
                text: self.0.into(),
                ..Default::default()
            },
            tool_calls: None,
            usage: Default::default(),
            stop_reason: anima_core::ModelStopReason::End,
        })
    }
}

async fn designate_main_chat(state: &SharedDaemonState, agent_id: &str) {
    let mut record = SessionRecord::new(
        agent_id,
        MAIN_ROOM,
        SessionKind::Chat,
        SessionOrigin::Web,
        "Main".into(),
        TitleSource::FirstMessage,
        1,
    );
    record.main_chat = true;
    state.write().await.sessions.insert(record);
}

async fn once_due_now(service: &SchedulerService, agent_id: &str) -> ScheduledPromptRecord {
    service
        .create(
            agent_id.into(),
            "Remind me to call the bank".into(),
            ScheduleTrigger::Once { at_ms: now_ms() },
            ScheduleTarget::Workspace,
            true,
            None,
            Some(now_ms()),
            None,
        )
        .await
        .unwrap()
        .0
}

async fn messages_in(state: &SharedDaemonState, agent_id: &str, room: &str) -> Vec<Message> {
    state.read().await.agents[agent_id]
        .messages()
        .iter()
        .filter(|message| message.room_id == room)
        .cloned()
        .collect()
}

fn meta<'m>(message: &'m Message, key: &str) -> Option<&'m DataValue> {
    message.content.metadata.as_ref()?.get(key)
}

use anima_core::Message;

#[tokio::test]
async fn a_spoken_reminder_posts_its_card_in_the_main_chat() {
    let (service, state, agent_id, _) = service_with_daemon(DaemonState::with_model_adapter(
        Arc::new(Says("Call the bank now.")),
    ));
    designate_main_chat(&state, &agent_id).await;
    let record = once_due_now(&service, &agent_id).await;

    assert_eq!(service.tick_at(now_ms()).await.unwrap(), 1);

    let main = messages_in(&state, &agent_id, MAIN_ROOM).await;
    assert_eq!(main.len(), 1, "exactly one card: {main:?}");
    let card = &main[0];
    assert_eq!(card.role, anima_core::MessageRole::Assistant);
    assert_eq!(
        meta(card, "kind"),
        Some(&DataValue::String("automation".into()))
    );
    assert_eq!(
        meta(card, "automationId"),
        Some(&DataValue::String(record.id.clone()))
    );
    assert_eq!(meta(card, "reminder"), Some(&DataValue::Bool(true)));
    assert_eq!(
        meta(card, "delivery"),
        Some(&DataValue::String("main".into()))
    );
    assert_eq!(
        meta(card, "body"),
        Some(&DataValue::String("Call the bank now.".into()))
    );
    // The model reading the main chat learns the id it can mark done.
    assert!(card.content.text.contains(&record.id));
    assert!(card.content.text.contains("Call the bank now."));
    // The prompt and the run stay in the automation's own room.
    let own = messages_in(
        &state,
        &agent_id,
        &crate::sessions::schedule_room_id(&record.id),
    )
    .await;
    assert!(own
        .iter()
        .any(|message| message.content.text.contains("Remind me to call the bank")));
    assert!(main
        .iter()
        .all(|message| !message.content.text.contains("CHECKIN")));
    // The main chat's activity moved with the card.
    let guard = state.read().await;
    let session = guard.sessions.get(&agent_id, MAIN_ROOM).unwrap();
    assert!(session.last_activity_at_ms >= card.created_at_ms);
    assert!(
        !guard.schedules[&record.id].enabled,
        "a once turns itself off"
    );
    assert!(guard.side_posts.is_empty(), "the commit drained its posts");
}

#[tokio::test]
async fn without_a_main_chat_the_output_stays_in_the_automation_room() {
    let (service, state, agent_id, _) =
        service_with_daemon(DaemonState::with_model_adapter(Arc::new(Says("Hello."))));
    let record = once_due_now(&service, &agent_id).await;
    assert_eq!(service.tick_at(now_ms()).await.unwrap(), 1);
    assert!(messages_in(&state, &agent_id, MAIN_ROOM).await.is_empty());
    let own = messages_in(
        &state,
        &agent_id,
        &crate::sessions::schedule_room_id(&record.id),
    )
    .await;
    assert!(own.iter().any(|message| message.content.text == "Hello."));
}

#[tokio::test]
async fn a_silent_run_leaves_nothing_in_the_main_chat() {
    let (service, state, agent_id, _) = service_with_daemon(DaemonState::with_model_adapter(
        Arc::new(Says(CHECKIN_SENTINEL)),
    ));
    designate_main_chat(&state, &agent_id).await;
    once_due_now(&service, &agent_id).await;
    assert_eq!(service.tick_at(now_ms()).await.unwrap(), 1);
    assert!(messages_in(&state, &agent_id, MAIN_ROOM).await.is_empty());
}

#[tokio::test]
async fn a_reminder_can_be_marked_done_and_snoozed() {
    let (service, state, agent_id, _) =
        service_with_daemon(DaemonState::with_model_adapter(Arc::new(Says("Now."))));
    let record = once_due_now(&service, &agent_id).await;
    service.tick_at(now_ms()).await.unwrap();
    let automations = service.automations();

    let now = now_ms();
    let done = automations
        .complete(&agent_id, &record.id, now, None)
        .await
        .unwrap();
    assert_eq!(done.done_at_ms, Some(now));
    assert!(!done.enabled);
    let again = automations
        .complete(&agent_id, &record.id, now + 5, None)
        .await
        .unwrap();
    assert_eq!(again.done_at_ms, Some(now), "the first done time stays");

    let snoozed = automations
        .snooze(&agent_id, &record.id, 15, now, None)
        .await
        .unwrap();
    assert!(snoozed.enabled);
    assert_eq!(snoozed.done_at_ms, None);
    assert_eq!(snoozed.next_due_at_ms, now + 15 * 60_000);
    assert_eq!(
        snoozed.trigger,
        ScheduleTrigger::Once {
            at_ms: now + 15 * 60_000
        }
    );
    assert_eq!(state.read().await.schedules[&record.id], snoozed, "saved");

    assert_eq!(
        automations
            .snooze(&agent_id, &record.id, 0, now, None)
            .await,
        Err(ScheduleError::Invalid(
            super::automations::SNOOZE_MINUTES_INVALID
        ))
    );
    assert_eq!(
        automations
            .snooze(&agent_id, &record.id, 1441, now, None)
            .await,
        Err(ScheduleError::Invalid(
            super::automations::SNOOZE_MINUTES_INVALID
        ))
    );
    assert_eq!(
        automations
            .complete("someone-else", &record.id, now, None)
            .await,
        Err(ScheduleError::NotFound)
    );
}

#[tokio::test]
async fn only_one_time_automations_can_be_done_or_snoozed() {
    let (service, _state, agent_id, _) =
        service_with_daemon(DaemonState::with_model_adapter(Arc::new(Says("Now."))));
    let repeating = super::tests::due_schedule(&service, &agent_id).await;
    let automations = service.automations();
    let refused = Err(ScheduleError::Conflict(
        super::automations::ONLY_ONE_TIME_REMINDERS,
    ));
    assert_eq!(
        automations
            .complete(&agent_id, &repeating.id, now_ms(), None)
            .await,
        refused
    );
    assert_eq!(
        automations
            .snooze(&agent_id, &repeating.id, 5, now_ms(), None)
            .await,
        refused
    );
}

#[tokio::test]
async fn finished_one_time_automations_leave_the_list_a_day_later() {
    let (service, state, agent_id, _) =
        service_with_daemon(DaemonState::with_model_adapter(Arc::new(Says("Now."))));
    let fired = once_due_now(&service, &agent_id).await;
    let done_early = service
        .create(
            agent_id.clone(),
            "Later".into(),
            ScheduleTrigger::Once {
                at_ms: now_ms() + 3_600_000,
            },
            ScheduleTarget::Workspace,
            true,
            None,
            Some(now_ms() + 3_600_000),
            None,
        )
        .await
        .unwrap()
        .0;
    let upcoming = service
        .create(
            agent_id.clone(),
            "Tomorrow".into(),
            ScheduleTrigger::Once {
                at_ms: now_ms() + 2 * FINISHED_ONCE_RETENTION_MS,
            },
            ScheduleTarget::Workspace,
            true,
            None,
            Some(now_ms() + 2 * FINISHED_ONCE_RETENTION_MS),
            None,
        )
        .await
        .unwrap()
        .0;
    let repeating = super::tests::due_schedule(&service, &agent_id).await;
    let start = now_ms();
    service.tick_at(start).await.unwrap();
    let finished = once_finished_at(&state.read().await.schedules[&fired.id]).unwrap();
    service
        .automations()
        .complete(&agent_id, &done_early.id, start, None)
        .await
        .unwrap();

    // Before a day passes, everything stays.
    let early = finished.max(start) + FINISHED_ONCE_RETENTION_MS - 1_000;
    service.tick_at(early).await.unwrap();
    assert!(state.read().await.schedules.contains_key(&fired.id));
    assert!(state.read().await.schedules.contains_key(&done_early.id));

    service
        .tick_at(finished.max(start) + FINISHED_ONCE_RETENTION_MS)
        .await
        .unwrap();
    let guard = state.read().await;
    assert!(
        !guard.schedules.contains_key(&fired.id),
        "fired and a day old"
    );
    assert!(
        !guard.schedules.contains_key(&done_early.id),
        "done and a day old"
    );
    assert!(guard.schedules.contains_key(&upcoming.id), "not finished");
    assert!(guard.schedules.contains_key(&repeating.id), "repeats");
}

#[tokio::test]
async fn a_discord_post_leaves_a_note_in_the_main_chat_and_a_copy_in_its_thread() {
    let (service, state, agent_id, _) = service_with_daemon(DaemonState::with_model_adapter(
        Arc::new(Says("Weekly: all good.")),
    ));
    designate_main_chat(&state, &agent_id).await;
    let connector: crate::connectors::discord::DiscordConnectorRecord =
        serde_json::from_value(serde_json::json!({
            "id": "discord-scheduled", "agentId": agent_id,
            "roomId": "discord:discord-scheduled",
            "bot": {"id": "11", "username": "fixture", "displayName": null},
            "channel": {"id": "42", "guildId": "7", "name": "home", "kind": "guildText"},
            "approvedUserIds": ["9"], "credentialGeneration": 1, "configurationRevision": 1,
            "cursor": "0", "activatedAtMs": 1, "enabled": true,
            "deletedAtMs": null, "createdAtMs": 1, "updatedAtMs": 1,
            "settings": {"allowedChannelIds": [], "freeResponseChannelIds": [],
                "approvedRoleIds": [], "homeChannelId": "42", "requireMention": true,
                "autoThread": true, "threadRequireMention": false},
            "botWide": false, "applicationId": "11", "conversations": []
        }))
        .unwrap();
    let home_room = connector.home_destination().unwrap().room_id;
    {
        let mut guard = state.write().await;
        guard
            .discord_connectors
            .insert(connector.id.clone(), connector.clone());
        guard.sessions.insert(SessionRecord::new(
            &agent_id,
            &home_room,
            SessionKind::Discord,
            SessionOrigin::Discord,
            "#home".into(),
            TitleSource::System,
            1,
        ));
    }
    let (record, _) = service
        .create(
            agent_id.clone(),
            "Weekly review".into(),
            ScheduleTrigger::Interval {
                interval_ms: 60_000,
            },
            ScheduleTarget::Connector {
                connector_id: connector.id.clone(),
            },
            true,
            None,
            Some(now_ms()),
            None,
        )
        .await
        .unwrap();
    assert_eq!(service.tick_at(now_ms()).await.unwrap(), 1);

    let copies = messages_in(&state, &agent_id, &home_room).await;
    assert_eq!(copies.len(), 1, "the post is saved in #home's thread");
    let copy = &copies[0];
    assert_eq!(copy.content.text, "Weekly: all good.");
    assert_eq!(
        meta(copy, "kind"),
        Some(&DataValue::String("automationPost".into()))
    );

    let main = messages_in(&state, &agent_id, MAIN_ROOM).await;
    assert_eq!(main.len(), 1);
    let note = &main[0];
    assert_eq!(
        meta(note, "kind"),
        Some(&DataValue::String("automationNote".into()))
    );
    assert_eq!(
        meta(note, "delivery"),
        Some(&DataValue::String("discord".into()))
    );
    assert_eq!(
        meta(note, "destinationLabel"),
        Some(&DataValue::String("#home".into()))
    );
    assert_eq!(
        meta(note, "destinationSessionId"),
        Some(&DataValue::String(home_room.clone()))
    );
    assert_eq!(
        meta(note, "destinationMessageId"),
        Some(&DataValue::String(copy.id.clone()))
    );
    assert_eq!(meta(note, "reminder"), Some(&DataValue::Bool(false)));
    assert!(note.content.text.contains("posted to Discord #home"));
    assert!(note.content.text.contains(&record.id));
    // Discord still gets its outbound, as before.
    let guard = state.read().await;
    let outbound = guard
        .discord_outbound
        .values()
        .next()
        .expect("queued for Discord");
    assert_eq!(outbound.text, "Weekly: all good.");
}

#[tokio::test]
async fn a_telegram_reminder_runs_aside_posts_its_card_in_the_main_chat_and_sends_that_copy() {
    use crate::connectors::{TelegramChatKind, TelegramChatMetadata};
    let (service, state, agent_id, manager) = service_with_daemon(DaemonState::with_model_adapter(
        Arc::new(Says("Call the bank now.")),
    ));
    designate_main_chat(&state, &agent_id).await;
    let connector = manager
        .create(
            agent_id.clone(),
            crate::connectors::credentials::TelegramBotToken::parse("42:test-token").unwrap(),
        )
        .await
        .unwrap();
    state
        .write()
        .await
        .connectors
        .get_mut(&connector.id)
        .unwrap()
        .approved_chat = Some(TelegramChatMetadata {
        id: "2".into(),
        kind: TelegramChatKind::Private,
        title: None,
        username: None,
    });
    manager.restart(connector.id.clone()).await.unwrap();
    let (record, _) = service
        .create(
            agent_id.clone(),
            "Remind me to call the bank".into(),
            ScheduleTrigger::Once { at_ms: now_ms() },
            ScheduleTarget::Connector {
                connector_id: connector.id.clone(),
            },
            true,
            None,
            Some(now_ms()),
            None,
        )
        .await
        .unwrap();
    assert_eq!(service.tick_at(now_ms()).await.unwrap(), 1);

    let main = messages_in(&state, &agent_id, MAIN_ROOM).await;
    assert_eq!(main.len(), 1);
    let card = &main[0];
    assert_eq!(
        meta(card, "kind"),
        Some(&DataValue::String("automation".into()))
    );
    assert_eq!(
        meta(card, "delivery"),
        Some(&DataValue::String("telegram".into()))
    );
    // The prompt never reaches the connector's room or the main chat.
    assert!(messages_in(&state, &agent_id, &connector.room_id)
        .await
        .is_empty());
    let own = messages_in(
        &state,
        &agent_id,
        &crate::sessions::schedule_room_id(&record.id),
    )
    .await;
    assert!(own
        .iter()
        .any(|message| message.content.text.contains("Remind me")));
    let guard = state.read().await;
    let outbound = guard
        .outbound
        .values()
        .find(|item| item.connector_id == connector.id)
        .expect("Telegram gets the copy");
    assert_eq!(outbound.room_id, MAIN_ROOM);
    assert_eq!(outbound.assistant_message_id, card.id);
    assert_eq!(outbound.text, "Call the bank now.");
    drop(guard);
    manager.shutdown().await;
}
