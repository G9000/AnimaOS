use super::*;

fn order(recipient: Option<&str>, quantity: Option<u32>) -> Intent {
    Intent {
        order: Some(OrderClaim {
            recipient: recipient.map(str::to_owned),
            quantity,
        }),
        ..Default::default()
    }
}

#[test]
fn miras_two_loaves_grant_once_with_equal_reward() {
    for preference in [None, Some(Preference::Honey), Some(Preference::Sesame)] {
        for help in [false, true] {
            let mut state = GameState::new(Language::Ja);
            state.apply(Intent {
                preference,
                help,
                ..Default::default()
            });
            assert_eq!(
                state.apply(order(Some("Mira"), Some(2))),
                Outcome::Collected
            );
            assert_eq!((state.loaves, state.coins), (2, 20));
            assert_eq!(
                state.apply(order(Some("Mira"), Some(2))),
                Outcome::AlreadyCollected
            );
            assert_eq!((state.loaves, state.coins), (2, 20));
        }
    }
}

#[test]
fn ambiguous_and_wrong_orders_require_both_correct_fields() {
    for intent in [
        order(None, None),
        order(Some("Nora"), Some(2)),
        order(Some("Mira"), Some(1)),
        order(None, Some(2)),
    ] {
        let mut state = GameState::new(Language::Zh);
        assert_eq!(state.apply(intent), Outcome::ClarifyOrder);
        assert_eq!((state.loaves, state.coins), (0, 0));
    }
}

#[test]
fn recipient_and_quantity_can_be_clarified_across_turns() {
    let mut state = GameState::new(Language::Ja);
    assert_eq!(
        state.apply(order(Some("ミラ"), None)),
        Outcome::ClarifyOrder
    );
    assert_eq!(state.apply(order(None, Some(2))), Outcome::Collected);
    let mut wrong = GameState::new(Language::Zh);
    wrong.apply(order(Some("Nora"), Some(1)));
    assert_eq!(
        wrong.apply(order(Some("米拉"), None)),
        Outcome::ClarifyOrder
    );
    assert_eq!(wrong.apply(order(None, Some(2))), Outcome::Collected);
}

#[test]
fn state_roundtrip_recalls_preference_and_help_on_next_visit() {
    let mut state = GameState::new(Language::Ja);
    state.visit();
    state.apply(Intent {
        preference: Some(Preference::Sesame),
        help: true,
        ..Default::default()
    });
    let serialized = serde_json::to_string(&state).unwrap();
    let mut reopened: GameState = serde_json::from_str(&serialized).unwrap();
    reopened.visit();
    assert_eq!(reopened.visits, 2);
    assert_eq!(reopened.bread_preference, Some(Preference::Sesame));
    assert!(reopened.memories().iter().any(|m| m.contains("sesame")));
    assert!(reopened.memories().iter().any(|m| m.contains("recipe")));
    assert!(!reopened.journal.is_empty());
    assert!(GameState::new(Language::Ja).memories().is_empty());
}

#[test]
fn collection_introduces_recipe_concern_and_revisit_recalls_completed_order() {
    for language in [Language::Ja, Language::Zh] {
        for preference in [None, Some(Preference::Honey), Some(Preference::Sesame)] {
            let mut state = GameState::new(language);
            state.visit();
            if preference.is_some() {
                state.apply(Intent {
                    preference,
                    ..Default::default()
                });
            }
            let outcome = state.apply(order(Some("Mira"), Some(2)));
            let collected = reviewed_dialogue(&state, outcome);
            assert!(collected.translation.contains("new recipe"));
            assert!(collected.translation.contains("nervous"));
            assert!(state.recipe_discussed);
            state.visit();
            let returned = reviewed_dialogue(&state, Outcome::Welcome);
            assert!(returned
                .translation
                .contains("already collected Mira's two loaves"));
            assert!(!returned.translation.contains("Are you here for Mira"));
        }
    }
}

#[test]
fn ordinary_conversation_introduces_the_nervous_recipe_without_a_special_choice() {
    let mut state = GameState::new(Language::Ja);
    let outcome = state.apply(Intent::default());
    assert_eq!(outcome, Outcome::Recipe);
    assert!(state.recipe_discussed);
}

fn speaker_input(npc: &str, choice: Option<&str>) -> GameInput {
    let input = match choice {
        Some(id) => serde_json::json!({"kind":"turn","message":"","choice_id":id}),
        None => serde_json::json!({"kind":"visit"}),
    };
    serde_json::from_value(serde_json::json!({"kind":"withNpc","npcId":npc,"input":input})).unwrap()
}
fn played(state: GameState, npc: &str, choice: Option<&str>) -> PlayedTurn {
    reviewed_turn(
        state,
        &speaker_input(npc, choice),
        &RuntimeInfo::unavailable(),
    )
    .unwrap()
}
#[test]
fn delivery_requires_mira_and_bread_then_evening_return_completes_without_recollection() {
    let no_bread = played(GameState::new(Language::Ja), "mira", Some("deliver_order"));
    assert_eq!(no_bread.state.loaves, 0);
    assert_eq!(
        serde_json::to_value(&no_bread.state).unwrap()["phase"],
        "day"
    );
    let collected = played(no_bread.state, "tavi", Some("collect_order"));
    let wrong_speaker = played(collected.state, "tavi", Some("deliver_order"));
    assert_eq!(wrong_speaker.state.loaves, 2);
    let delivered = played(wrong_speaker.state, "mira", Some("deliver_order"));
    assert_eq!((delivered.state.loaves, delivered.state.coins), (0, 20));
    assert_eq!(
        serde_json::to_value(&delivered.state).unwrap()["phase"],
        "evening"
    );
    let repeat = played(delivered.state, "mira", Some("deliver_order"));
    assert_eq!((repeat.state.loaves, repeat.state.coins), (0, 20));
    let completed = played(repeat.state, "tavi", None);
    assert_eq!(
        serde_json::to_value(&completed.state).unwrap()["phase"],
        "complete"
    );
    assert!(completed.dialogue.translation.contains("supper"));
    assert!(completed
        .state
        .conversation
        .iter()
        .any(|line| line.contains("delivered")));
    let recollect = played(completed.state, "tavi", Some("collect_order"));
    assert_eq!((recollect.state.loaves, recollect.state.coins), (0, 20));
    assert!(!recollect
        .dialogue
        .suggestions
        .iter()
        .any(|s| s.id == "collect_order"));
}
#[test]
fn mira_cannot_collect_or_see_private_tavi_memories_and_history() {
    let mut state = GameState::new(Language::Zh);
    state.apply(choice_intent("honey").unwrap());
    state.apply(choice_intent("help").unwrap());
    state.conversation.push("Player: PRIVATE_TAVI_WORDS".into());
    let mira = played(state, "mira", Some("collect_order"));
    assert_eq!((mira.state.loaves, mira.state.coins), (0, 0));
    let view = GameView::new(
        "id".into(),
        &mira.state,
        mira.dialogue,
        RuntimeInfo::unavailable(),
    );
    assert!(!view
        .memories
        .iter()
        .any(|m| m.contains("honey") || m.contains("encourag")));
    assert!(view
        .dialogue
        .suggestions
        .iter()
        .all(|s| s.id == "deliver_order" || s.id == "ask_supper"));
}
#[test]
fn legacy_collected_save_keeps_progress_after_delivery() {
    let mut legacy = serde_json::to_value(GameState::new(Language::Ja)).unwrap();
    legacy["loaves"] = serde_json::json!(2);
    legacy["coins"] = serde_json::json!(20);
    for field in [
        "collected",
        "phase",
        "npcId",
        "traveler",
        "miraConversation",
        "miraVisits",
    ] {
        legacy.as_object_mut().unwrap().remove(field);
    }
    let delivered = played(
        serde_json::from_value(legacy).unwrap(),
        "mira",
        Some("deliver_order"),
    );
    let recollect = played(delivered.state, "tavi", Some("collect_order"));
    assert_eq!((recollect.state.loaves, recollect.state.coins), (0, 20));
}
