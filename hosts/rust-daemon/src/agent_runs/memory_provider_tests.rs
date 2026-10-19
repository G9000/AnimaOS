//! A completed owner turn reaches the external memory provider, on the same
//! terms as a conversation review count: check-ins never do.
use super::test_support::{add_chat, chat_request, coordinator_with, ScriptedModel, Step};
use crate::memory_provider::test_support::{inject, FakeProvider};
use anima_core::DataValue;
use std::collections::BTreeMap;
use std::sync::Arc;

const ROOM: &str = "chat:provider";

#[tokio::test(flavor = "current_thread")]
async fn a_completed_owner_turn_sends_one_sync_turn_and_a_check_in_sends_none() {
    let fake = Arc::new(FakeProvider::new("fake"));
    let _active = inject(fake.clone());
    let model = ScriptedModel::new(vec![
        Step::Text(vec!["Noted, kilometres it is."]),
        Step::Text(vec!["All quiet here."]),
        Step::Text(vec!["Second answer."]),
    ]);
    let (coordinator, agent) = coordinator_with(model).await;
    add_chat(&coordinator, &agent, ROOM).await;

    coordinator
        .run(chat_request(&agent, ROOM, "I prefer kilometres"))
        .await
        .unwrap();
    fake.wait_until(|fake| fake.turns.lock().unwrap().len() == 1)
        .await;
    {
        let turns = fake.turns.lock().unwrap();
        assert_eq!(turns[0].agent_id, agent);
        assert_eq!(turns[0].session_id, "chat:provider");
        assert_eq!(turns[0].user_text, "I prefer kilometres");
        assert_eq!(turns[0].reply_text, "Noted, kilometres it is.");
        assert!(turns[0].at_ms > 0);
    }

    let mut checkin = chat_request(&agent, ROOM, "checkin: anything new?");
    checkin.content.metadata = Some(BTreeMap::from([(
        "kind".to_string(),
        DataValue::String("checkin".into()),
    )]));
    coordinator.run(checkin).await.unwrap();
    // A later owner turn proves the check-in's slot has passed.
    coordinator
        .run(chat_request(&agent, ROOM, "and another"))
        .await
        .unwrap();
    fake.wait_until(|fake| fake.turns.lock().unwrap().len() == 2)
        .await;
    let turns = fake.turns.lock().unwrap();
    let users: Vec<_> = turns.iter().map(|turn| turn.user_text.as_str()).collect();
    assert_eq!(users, ["I prefer kilometres", "and another"]);
}
