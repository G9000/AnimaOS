use anima_core::standing_intents::*;
fn spec(condition: IntentCondition) -> IntentSpec {
    IntentSpec {
        scope: IntentScope {
            session_id: None,
            sender: SenderScope::Owner,
        },
        condition,
        instruction: "Ask about the saved project before answering.".into(),
        expires_at_ms: 100_000,
        cooldown_ms: 100,
        max_fires: 2,
    }
}
fn turn<'a>(id: &'a str, session: &'a str, text: &'a str, now: u64) -> OwnerTurn<'a> {
    OwnerTurn {
        agent_id: "agent",
        session_id: session,
        sender: OwnerSender::Local,
        run_id: id,
        retry_key: None,
        text,
        conversation_started: true,
        now_ms: now,
    }
}
#[test]
fn standing_intents_new_session_matches_same_instruction_and_restored_budget() {
    let mut registry = StandingIntentRegistry::default();
    let (row, _) = registry
        .create(
            "intent",
            "agent",
            spec(IntentCondition::Keyword {
                phrase: "Project Alpha".into(),
            }),
            1,
        )
        .unwrap();
    assert_eq!(row.revision, 1);
    let matched = registry
        .admit(&turn("r1", "new-room", "How is PROJECT ALPHA?", 10))
        .unwrap();
    assert_eq!(matched.len(), 1);
    assert_eq!(matched[0].instruction, row.spec.instruction);
    assert!(registry
        .admit(&turn("r2", "other-room", "nothing relevant", 200))
        .unwrap()
        .is_empty());
    let encoded = serde_json::to_vec(&registry).unwrap();
    let mut restored: StandingIntentRegistry = serde_json::from_slice(&encoded).unwrap();
    restored.validate().unwrap();
    assert!(restored
        .admit(&turn("r1", "new-room", "Project Alpha", 300))
        .unwrap()
        .is_empty());
    assert_eq!(
        restored
            .admit(&turn("r3", "fresh-room", "Project Alpha", 300))
            .unwrap()
            .len(),
        1
    );
    assert!(restored
        .admit(&turn("r4", "fresh-room", "Project Alpha", 600))
        .unwrap()
        .is_empty());
    assert_eq!(restored.intents["intent"].fires.len(), 2);
}
#[test]
fn standing_intents_scope_expiry_cooldown_and_event_are_bounded() {
    let mut registry = StandingIntentRegistry::default();
    let mut s = spec(IntentCondition::Event {
        event: ConversationEvent::Started,
    });
    s.scope.session_id = Some("selected".into());
    registry.create("intent", "agent", s, 1).unwrap();
    assert!(registry
        .admit(&turn("r1", "other", "hello", 10))
        .unwrap()
        .is_empty());
    let mut t = turn("r2", "selected", "hello", 10);
    t.sender = OwnerSender::Telegram;
    assert_eq!(registry.admit(&t).unwrap().len(), 1);
    assert!(registry
        .admit(&turn("r3", "selected", "hello", 50))
        .unwrap()
        .is_empty());
    let mut t = turn("r4", "selected", "hello", 200);
    t.conversation_started = false;
    assert!(registry.admit(&t).unwrap().is_empty());
    assert!(registry
        .admit(&turn("r5", "selected", "hello", 100_000))
        .unwrap()
        .is_empty());
}
#[test]
fn standing_intents_cancel_and_undo_are_exact_and_refuse_intervening_fire() {
    let mut registry = StandingIntentRegistry::default();
    let (before, create) = registry
        .create(
            "intent",
            "agent",
            spec(IntentCondition::Event {
                event: ConversationEvent::Message,
            }),
            1,
        )
        .unwrap();
    let (cancelled, change) = registry.cancel("intent", before.revision, 5).unwrap();
    assert!(cancelled.cancelled);
    assert!(registry
        .admit(&turn("r1", "room", "hello", 10))
        .unwrap()
        .is_empty());
    let restored = registry.undo(&change.id, 20).unwrap().unwrap();
    assert_eq!(restored.spec.instruction, before.spec.instruction);
    assert!(!restored.cancelled);
    registry.admit(&turn("r2", "room", "hello", 30)).unwrap();
    assert!(registry.undo(&create.id, 40).is_err());
}
#[test]
fn standing_intents_narrowing_retry_and_stable_three_match_limit() {
    let mut registry = StandingIntentRegistry::default();
    for id in ["d", "c", "b", "a"] {
        registry
            .create(
                id,
                "agent",
                spec(IntentCondition::Event {
                    event: ConversationEvent::Message,
                }),
                1,
            )
            .unwrap();
    }
    let mut t = turn("r1", "room", "hello", 10);
    t.retry_key = Some("request-key");
    let rows = registry.admit(&t).unwrap();
    assert_eq!(
        rows.iter()
            .map(|m| m.intent_id.as_str())
            .collect::<Vec<_>>(),
        vec!["a", "b", "c"]
    );
    t.run_id = "r1-retry";
    t.now_ms = 200;
    assert!(
        registry.admit(&t).unwrap().is_empty(),
        "a retry cannot consume a fourth intention"
    );
    let mut narrowed = registry.intents["d"].spec.clone();
    narrowed.scope.sender = SenderScope::LocalOwner;
    narrowed.scope.session_id = Some("selected".into());
    registry.update("d", 1, narrowed, 20).unwrap();
    let mut external = turn("r2", "selected", "hello", 300);
    external.sender = OwnerSender::Telegram;
    assert!(!registry
        .admit(&external)
        .unwrap()
        .iter()
        .any(|m| m.intent_id == "d"));
}
#[test]
fn standing_intents_reject_unbounded_records_and_invalid_restore() {
    let mut registry = StandingIntentRegistry::default();
    let mut s = spec(IntentCondition::Keyword { phrase: " ".into() });
    assert!(registry.create("id", "agent", s.clone(), 1).is_err());
    s.condition = IntentCondition::Event {
        event: ConversationEvent::Message,
    };
    s.instruction = "x".repeat(MAX_INSTRUCTION_BYTES + 1);
    assert!(registry.create("id", "agent", s.clone(), 1).is_err());
    s.instruction = "bounded".into();
    s.max_fires = MAX_FIRES + 1;
    assert!(registry.create("id", "agent", s, 1).is_err());
}

#[test]
fn standing_intents_full_old_change_history_never_blocks_cancelling_current_intent() {
    let mut registry = StandingIntentRegistry::default();
    registry
        .create(
            "intent",
            "agent",
            spec(IntentCondition::Event {
                event: ConversationEvent::Message,
            }),
            1,
        )
        .unwrap();
    for now in 2..=MAX_CHANGES as u64 {
        let row = registry.intents["intent"].clone();
        registry
            .update("intent", row.revision, row.spec, now)
            .unwrap();
    }
    let row = registry.intents["intent"].clone();
    let (cancelled, change) = registry.cancel("intent", row.revision, 1000).unwrap();
    assert!(cancelled.cancelled);
    assert!(registry.changes.len() <= MAX_CHANGES);
    assert!(registry.undo(&change.id, 1001).unwrap().is_some());
    registry.validate().unwrap();
}
