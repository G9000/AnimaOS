//! The owner's private Telegram chat joins their main chat (like OpenClaw's
//! shared main session); groups and agents without a main chat keep the
//! connector's own room. Replies still go back to Telegram.
use super::*;
use crate::sessions::{SessionKind, SessionOrigin, SessionRecord, TitleSource};
use anima_core::MessageRole;

const MAIN_ROOM: &str = "chat:main";

async fn paired(
    designate_main: bool,
) -> (
    SharedDaemonState,
    ConnectorManager,
    Arc<FakeTransport>,
    String,
    String,
) {
    let state = state_with_agent();
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    if designate_main {
        let mut record = SessionRecord::new(
            &agent_id,
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
    let transport = Arc::new(FakeTransport::default());
    let manager = manager(
        Arc::clone(&state),
        Arc::new(InMemoryCredentialStore::default()),
        transport.clone(),
    );
    let connector = manager
        .create(
            agent_id.clone(),
            TelegramBotToken::parse("42:main").unwrap(),
        )
        .await
        .unwrap();
    manager
        .accept_batch(
            connector.id.clone(),
            TelegramUpdateBatch {
                updates: vec![text_update(1, "101", "pair")],
                next_update_id: 2,
            },
        )
        .await
        .unwrap();
    manager.approve_pending(connector.id.clone()).await.unwrap();
    (state, manager, transport, agent_id, connector.id)
}

async fn send(manager: &ConnectorManager, connector_id: &str, update: TelegramTextUpdate) {
    let next = update.update_id + 1;
    manager
        .accept_batch(
            connector_id.to_owned(),
            TelegramUpdateBatch {
                updates: vec![update],
                next_update_id: next,
            },
        )
        .await
        .unwrap();
}

fn rooms_of(state: &DaemonState, agent_id: &str, text: &str) -> Vec<(String, MessageRole)> {
    let snapshot = state.get_agent(agent_id).unwrap();
    let user = snapshot
        .messages
        .iter()
        .position(|message| message.content.text == text)
        .expect("the Telegram message was saved");
    snapshot.messages[user..]
        .iter()
        .take(2)
        .map(|message| (message.room_id.clone(), message.role.clone()))
        .collect()
}

#[tokio::test]
async fn a_private_telegram_message_runs_in_the_main_chat_and_replies_on_telegram() {
    let (state, manager, transport, agent_id, connector_id) = paired(true).await;
    send(
        &manager,
        &connector_id,
        text_update(2, "101", "remind me at noon"),
    )
    .await;

    assert_eq!(
        manager.process_pending_once(connector_id.clone()).await,
        Ok(true)
    );

    let rooms = rooms_of(&*state.read().await, &agent_id, "remind me at noon");
    assert_eq!(
        rooms,
        vec![
            (MAIN_ROOM.to_owned(), MessageRole::User),
            (MAIN_ROOM.to_owned(), MessageRole::Assistant),
        ]
    );
    let source = state
        .read()
        .await
        .get_agent(&agent_id)
        .unwrap()
        .messages
        .iter()
        .find(|message| message.content.text == "remind me at noon")
        .and_then(|message| message.content.metadata.clone())
        .and_then(|metadata| metadata.get("source").cloned());
    assert_eq!(source, Some(DataValue::String("telegram".into())));

    assert_eq!(manager.deliver_pending_once(connector_id).await, Ok(true));
    let sent = transport.sent.lock().unwrap().clone();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].0, "101");
}

#[tokio::test]
async fn without_a_main_chat_telegram_keeps_its_own_conversation() {
    let (state, manager, _transport, agent_id, connector_id) = paired(false).await;
    let connector_room = state.read().await.connectors[&connector_id].room_id.clone();
    send(
        &manager,
        &connector_id,
        text_update(2, "101", "hello there"),
    )
    .await;

    assert_eq!(manager.process_pending_once(connector_id).await, Ok(true));

    let rooms = rooms_of(&*state.read().await, &agent_id, "hello there");
    assert!(rooms.iter().all(|(room, _)| room == &connector_room));
}

#[tokio::test]
async fn a_group_chat_keeps_its_own_conversation_even_with_a_main_chat() {
    let state = state_with_agent();
    let agent_id = state.read().await.list_agents()[0].state.id.clone();
    let mut record = SessionRecord::new(
        &agent_id,
        MAIN_ROOM,
        SessionKind::Chat,
        SessionOrigin::Web,
        "Main".into(),
        TitleSource::FirstMessage,
        1,
    );
    record.main_chat = true;
    state.write().await.sessions.insert(record);
    let manager = manager(
        Arc::clone(&state),
        Arc::new(InMemoryCredentialStore::default()),
        Arc::new(FakeTransport::default()),
    );
    let connector = manager
        .create(
            agent_id.clone(),
            TelegramBotToken::parse("42:group").unwrap(),
        )
        .await
        .unwrap();
    let mut pair = text_update(1, "-500", "pair");
    pair.chat.kind = TelegramChatKind::Group;
    send(&manager, &connector.id, pair).await;
    manager.approve_pending(connector.id.clone()).await.unwrap();
    let mut update = text_update(2, "-500", "team question");
    update.chat.kind = TelegramChatKind::Group;
    send(&manager, &connector.id, update).await;

    assert_eq!(
        manager.process_pending_once(connector.id.clone()).await,
        Ok(true)
    );

    let connector_room = state.read().await.connectors[&connector.id].room_id.clone();
    let rooms = rooms_of(&*state.read().await, &agent_id, "team question");
    assert!(rooms.iter().all(|(room, _)| room == &connector_room));
}

#[tokio::test]
async fn a_redelivered_update_keeps_the_room_it_was_first_given() {
    let (state, manager, _transport, agent_id, connector_id) = paired(false).await;
    let connector_room = state.read().await.connectors[&connector_id].room_id.clone();
    send(
        &manager,
        &connector_id,
        text_update(2, "101", "before main"),
    )
    .await;

    // The console resolves the main chat while the update is still queued.
    let mut record = SessionRecord::new(
        &agent_id,
        MAIN_ROOM,
        SessionKind::Chat,
        SessionOrigin::Web,
        "Main".into(),
        TitleSource::FirstMessage,
        1,
    );
    record.main_chat = true;
    state.write().await.sessions.insert(record);
    send(
        &manager,
        &connector_id,
        text_update(2, "101", "before main"),
    )
    .await;

    let key = (connector_id.clone(), 2);
    assert_eq!(state.read().await.inbound[&key].room_id, connector_room);
}
