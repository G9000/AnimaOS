use super::records::*;
use super::*;
use std::collections::HashSet;

fn record(source: &str, at: u64) -> LearningReviewRecord {
    LearningReviewRecord {
        id: format!("review-{source}"),
        agent_id: "owner".into(),
        source_run_id: source.into(),
        session_id: "chat:case".into(),
        trigger: LearningTrigger::Manual,
        policy_version: POLICY_VERSION,
        evidence_digest: "a".repeat(64),
        evidence_brief: "bounded supplied source".into(),
        evidence_anchors: vec!["reply".into()],
        state: LearningReviewState::Queued,
        reserved_at_ms: at,
        started_at_ms: None,
        finished_at_ms: None,
        model: "fixture".into(),
        provider: Some("test".into()),
        usage: None,
        usage_complete: false,
        error: None,
        proposals: vec![],
    }
}

#[test]
fn m38_shared_daily_reservations_are_not_refunded_by_cancel_failure_or_restart() {
    let mut registry = LearningRegistry::default();
    for (index, state) in [LearningReviewState::Cancelled, LearningReviewState::Failed]
        .into_iter()
        .enumerate()
    {
        let mut review = record(&index.to_string(), 86_400_001);
        review.finish(state, "fixture", "Fixture terminal outcome", 86_400_002);
        registry.reserve(review).unwrap();
    }
    assert!(registry.reserve(record("third", 86_400_003)).is_err());
    let mut restored = LearningRegistry::restored(
        registry.snapshot(),
        &HashSet::from(["owner".into()]),
        86_400_004,
    );
    assert!(restored.reserve(record("third", 86_400_005)).is_err());
    assert!(restored.reserve(record("tomorrow", 172_800_001)).is_ok());
}

#[test]
fn the_review_budget_rolls_over_24_hours_instead_of_a_utc_day() {
    const HOUR: u64 = 3_600_000;
    let mut registry = LearningRegistry::default();
    let late = 10 * WINDOW_MS - HOUR;
    assert_eq!(registry.availability(late), (DAILY_LIMIT, None));
    registry.reserve(record("first", late)).unwrap();
    registry.reserve(record("second", late + 1)).unwrap();
    // Two hours later is a new UTC day, but not a new budget.
    let after_midnight = late + 2 * HOUR;
    assert_eq!(
        registry.availability(after_midnight),
        (0, Some(late + WINDOW_MS))
    );
    let refused = registry
        .reserve(record("third", after_midnight))
        .unwrap_err();
    assert!(refused.message().contains("used up"));
    assert_eq!(
        registry.availability(late + WINDOW_MS),
        (1, None),
        "the oldest review leaves the window"
    );
    registry.reserve(record("third", late + WINDOW_MS)).unwrap();
    assert_eq!(
        registry.availability(late + WINDOW_MS),
        (0, Some(late + 1 + WINDOW_MS))
    );
    assert_eq!(
        registry
            .policy("owner", late + WINDOW_MS, LearningMode::Strict)
            .next_available_at_ms,
        Some(late + 1 + WINDOW_MS)
    );
}

#[test]
fn m38_manual_automatic_dedup_and_evidence_conflict_keep_one_attempt() {
    let mut registry = LearningRegistry::default();
    let original = registry.reserve(record("same", 1)).unwrap();
    let mut automatic = record("same", 2);
    automatic.trigger = LearningTrigger::Automatic;
    assert_eq!(registry.reserve(automatic.clone()).unwrap(), original);
    automatic.evidence_digest = "b".repeat(64);
    assert!(registry.reserve(automatic).is_err());
    assert_eq!(registry.snapshot().reviews.len(), 1);
}

#[test]
fn m38_recovery_interrupts_pending_without_replay_or_quota_refund() {
    let mut snapshot = LearningSnapshot::default();
    snapshot.reviews.push(record("queued", 1));
    let mut running = record("running", 1);
    running.state = LearningReviewState::Running;
    running.started_at_ms = Some(2);
    snapshot.reviews.push(running);
    snapshot.policies.push(LearningPolicy {
        agent_id: "deleted".into(),
        auto_enabled: true,
    });
    let restored = LearningRegistry::restored(snapshot, &HashSet::from(["owner".into()]), 3);
    assert!(!restored.enabled("owner"));
    assert!(!restored.enabled("deleted"));
    assert!(restored
        .snapshot()
        .reviews
        .iter()
        .all(|r| r.state == LearningReviewState::Interrupted && r.proposals.is_empty()));
    assert!(restored
        .snapshot()
        .reviews
        .iter()
        .all(|r| r.reserved_at_ms == 1));
}

#[test]
fn m38_capacity_refuses_new_work_without_dropping_keys_or_old_reservations() {
    let mut registry = LearningRegistry::default();
    for i in 0..RETAINED_LIMIT {
        let review = record(&i.to_string(), (i as u64 + 1) * 86_400_000);
        registry.reviews.insert(review.id.clone(), review);
    }
    assert!(registry.reserve(record("new", 2000 * 86_400_000)).is_err());
    let old = record("0", 86_400_000);
    assert_eq!(registry.reserve(old.clone()).unwrap(), old);
    assert_eq!(registry.snapshot().reviews.len(), RETAINED_LIMIT);
}

#[test]
fn m38_snapshot_validation_rejects_duplicate_attempts_unknown_anchors_and_hidden_advice() {
    let review = record("valid", 1);
    let mut snapshot = LearningSnapshot {
        reviews: vec![review.clone()],
        ..LearningSnapshot::default()
    };
    LearningRegistry::validate(&snapshot).unwrap();
    snapshot.reviews.push(review.clone());
    assert!(LearningRegistry::validate(&snapshot).is_err());
    snapshot.reviews = vec![review];
    snapshot.reviews[0].proposals.push(LearningProposal {
        id: "p1".into(),
        observation: "Observed source".into(),
        suggestion: "Inspect the source".into(),
        anchors: vec!["foreign".into()],
        decision: LearningDecision::Pending,
        decision_version: 0,
        skill_draft_id: None,
        memory_id: None,
        core_edit: None,
        skill_candidate: None,
        skill_error: None,
    });
    assert!(LearningRegistry::validate(&snapshot).is_err());
    snapshot.reviews[0].proposals[0].anchors = vec!["reply".into()];
    snapshot.reviews[0].proposals[0].suggestion = "hidden\u{202e}text".into();
    assert!(LearningRegistry::validate(&snapshot).is_err());
}

#[test]
fn m38_utf8_evidence_excerpt_preserves_byte_bounds() {
    let text = "🦀".repeat(1000);
    let cut = super::evidence::bounded_text(&text, 1536);
    assert!(cut.len() <= 1536);
    assert!(text.starts_with(&cut));
    assert!(cut.is_char_boundary(cut.len()));
    assert!(!valid_advice_text("control\u{0008}text"));
    assert!(valid_advice_text("Ordinary\nformatted\tadvice"));
}

fn conversation(agent: &str, source: &str, at: u64) -> LearningReviewRecord {
    let mut review = record(source, at);
    review.id = format!("conversation-{agent}-{source}");
    review.agent_id = agent.into();
    review.trigger = LearningTrigger::Conversation;
    // Settled, so the shared queue bound stays out of the way.
    review.state = LearningReviewState::Completed;
    review
}

#[test]
fn conversation_turns_count_to_ten_and_stay_there_until_a_review_resets_them() {
    let mut registry = LearningRegistry::default();
    for index in 0..9 {
        registry.count_turn("owner", &format!("run-{index}"));
        assert!(!registry.conversation_due("owner"));
    }
    let tenth = registry.count_turn("owner", "run-9");
    assert!(registry.conversation_due("owner"));
    // A turn that could not be reviewed keeps the count at ten.
    registry.count_turn("owner", "run-10");
    assert_eq!(registry.conversation_counts["owner"], CONVERSATION_EVERY);
    assert_eq!(
        registry.conversation_turns("owner"),
        (1..=10)
            .map(|index| format!("run-{index}"))
            .collect::<Vec<_>>(),
        "only the last ten turns are kept, oldest first"
    );
    assert!(!registry.conversation_due("other"));
    registry.reset_conversation("owner");
    assert!(!registry.conversation_due("owner"));
    assert_eq!(registry.conversation_turns("owner").len(), 10);
    // A rolled-back commit takes its turn back.
    let mut fresh = LearningRegistry::default();
    let first = fresh.count_turn("owner", "run-0");
    fresh.uncount_turn(first);
    assert!(!fresh.conversation_due("owner"));
    assert!(fresh.conversation_turns("owner").is_empty());
    drop(tenth);
}

#[test]
fn conversation_reviews_and_task_reviews_have_separate_daily_windows() {
    let mut registry = LearningRegistry::default();
    registry.reserve(record("task-1", 10)).unwrap();
    registry.reserve(record("task-2", 11)).unwrap();
    // Task reviews are used up; conversation reviews are not.
    for index in 0..CONVERSATION_DAILY_LIMIT {
        registry
            .reserve(conversation(
                "owner",
                &format!("turn-{index}"),
                20 + index as u64,
            ))
            .unwrap();
    }
    let refused = registry
        .reserve(conversation("owner", "turn-extra", 40))
        .unwrap_err();
    assert!(refused
        .message()
        .contains("Conversation reviews are used up"));
    // Each agent has its own conversation window.
    registry
        .reserve(conversation("other", "turn-0", 41))
        .unwrap();
    // Eight conversation reviews never use the task budget.
    let mut tasks = LearningRegistry::default();
    for index in 0..CONVERSATION_DAILY_LIMIT {
        tasks
            .reserve(conversation(
                "owner",
                &format!("turn-{index}"),
                20 + index as u64,
            ))
            .unwrap();
    }
    assert_eq!(tasks.availability(50), (DAILY_LIMIT, None));
    tasks.reserve(record("task-1", 50)).unwrap();
    // The window rolls: a day later the agent may be reviewed again.
    assert!(registry
        .reserve(conversation("owner", "turn-later", 40 + WINDOW_MS))
        .is_ok());
}

#[test]
fn a_run_with_a_task_review_is_not_reviewed_again_as_conversation() {
    let mut registry = LearningRegistry::default();
    registry.reserve(record("same", 1)).unwrap();
    let mut again = conversation("owner", "same", 2);
    again.evidence_digest = "a".repeat(64);
    let refused = registry.reserve(again).unwrap_err();
    assert!(refused.message().contains("already reviewed"));
}

fn response(text: &str) -> anima_core::ModelGenerateResponse {
    anima_core::ModelGenerateResponse {
        content: anima_core::Content {
            text: text.into(),
            ..anima_core::Content::default()
        },
        tool_calls: None,
        usage: anima_core::TokenUsage::default(),
        stop_reason: anima_core::ModelStopReason::End,
    }
}

#[test]
fn a_conversation_review_keeps_at_most_five_well_formed_core_edits() {
    let anchors = vec!["turn:a".to_string()];
    let edit = |n: usize| {
        format!(
            "{{\"action\":\"add\",\"block\":\"user\",\"text\":\"fact {n}\",\"reason\":\"said so\"}}"
        )
    };
    let malformed = [
        // add without text, remove with text, an unknown block and field
        "{\"action\":\"add\",\"block\":\"user\",\"reason\":\"r\"}",
        "{\"action\":\"remove\",\"block\":\"notes\",\"text\":\"x\",\"old_text\":\"y\",\"reason\":\"r\"}",
        "{\"action\":\"add\",\"block\":\"soul\",\"text\":\"x\",\"reason\":\"r\"}",
        "{\"action\":\"add\",\"block\":\"user\",\"text\":\"x\",\"reason\":\"r\",\"extra\":1}",
        "{\"action\":\"add\",\"block\":\"user\",\"text\":\"hidden\u{202e}text\",\"reason\":\"r\"}",
    ];
    let mut edits: Vec<String> = malformed.iter().map(|text| text.to_string()).collect();
    edits.extend((0..7).map(edit));
    let text = format!(
        "{{\"proposals\":[{{\"observation\":\"o\",\"suggestion\":\"s\",\"anchors\":[\"turn:a\"]}}],\"core_edits\":[{}]}}",
        edits.join(",")
    );
    // The first five edits are the malformed ones: each is dropped, and the
    // cap applies before filtering, so no well-formed edit is kept.
    let parsed = super::service::parse_proposals(&response(&text), &anchors, true).unwrap();
    assert_eq!(parsed.len(), 1);

    let text = format!(
        "{{\"proposals\":[],\"core_edits\":[{}]}}",
        (0..7).map(edit).collect::<Vec<_>>().join(",")
    );
    let parsed = super::service::parse_proposals(&response(&text), &anchors, true).unwrap();
    assert_eq!(parsed.len(), CORE_EDIT_LIMIT, "extra edits are dropped");
    let first = &parsed[0];
    let core = first.core_edit.as_ref().unwrap();
    assert_eq!(core.action, CoreProposalAction::Add);
    assert_eq!(core.block, CoreProposalBlock::User);
    assert_eq!(first.suggestion, "Add to About you: fact 0");
    assert_eq!(first.observation, "said so");
    assert_eq!(first.anchors, anchors);
    // A task review never proposes core edits.
    assert!(super::service::parse_proposals(&response(&text), &anchors, false).is_err());
}

#[test]
fn empty_text_or_old_text_counts_as_absent_in_core_edits() {
    // Models echo the schema and send the unused field empty.
    let anchors = vec!["turn:a".to_string()];
    let text = "{\"proposals\":[],\"core_edits\":[\
        {\"action\":\"add\",\"block\":\"user\",\"text\":\"Is vegetarian.\",\"old_text\":\"\",\"reason\":\"stated\"},\
        {\"action\":\"remove\",\"block\":\"notes\",\"text\":\" \",\"old_text\":\"old note\",\"reason\":\"stale\"}]}";
    let parsed = super::service::parse_proposals(&response(text), &anchors, true).unwrap();
    assert_eq!(parsed.len(), 2);
    let add = parsed[0].core_edit.as_ref().unwrap();
    assert_eq!(add.action, CoreProposalAction::Add);
    assert_eq!(add.text.as_deref(), Some("Is vegetarian."));
    assert_eq!(add.old_text, None);
    let remove = parsed[1].core_edit.as_ref().unwrap();
    assert_eq!(remove.action, CoreProposalAction::Remove);
    assert_eq!(remove.text, None);
    assert_eq!(remove.old_text.as_deref(), Some("old note"));
}

#[test]
fn core_proposals_describe_each_edit_for_the_owner() {
    let proposal = |action, block, text: Option<&str>, old_text: Option<&str>| CoreProposal {
        action,
        block,
        text: text.map(Into::into),
        old_text: old_text.map(Into::into),
        error: None,
        previous_text: None,
        removed: None,
    };
    assert_eq!(
        proposal(
            CoreProposalAction::Replace,
            CoreProposalBlock::Notes,
            Some("spaces"),
            Some("tabs")
        )
        .describe(),
        "Replace in Agent notes: tabs \u{2192} spaces"
    );
    assert_eq!(
        proposal(
            CoreProposalAction::Remove,
            CoreProposalBlock::User,
            None,
            Some("Paris")
        )
        .describe(),
        "Remove from About you: Paris"
    );
}

#[test]
fn a_stored_review_without_core_fields_or_counts_loads() {
    // A review saved before conversation reviews: no `coreEdit`, no counts.
    let mut old = serde_json::to_value(record("old", 1)).unwrap();
    old["proposals"] = serde_json::json!([{"id":"p","observation":"o","suggestion":"s",
        "anchors":["reply"],"decision":"pending","decisionVersion":0}]);
    let snapshot: LearningSnapshot =
        serde_json::from_value(serde_json::json!({"reviews":[old],"policies":[]})).unwrap();
    LearningRegistry::validate(&snapshot).unwrap();
    assert!(snapshot.reviews[0].proposals[0].core_edit.is_none());
    let restored = LearningRegistry::restored(snapshot, &HashSet::from(["owner".into()]), 2);
    assert!(!restored.conversation_due("owner"));
    // A task review can never carry a core edit.
    let mut forged = restored.snapshot();
    forged.reviews[0].proposals[0].core_edit = Some(CoreProposal {
        action: CoreProposalAction::Add,
        block: CoreProposalBlock::User,
        text: Some("x".into()),
        old_text: None,
        error: None,
        previous_text: None,
        removed: None,
    });
    assert!(LearningRegistry::validate(&forged).is_err());
}

#[tokio::test]
async fn an_old_control_plane_without_conversation_counts_loads() {
    let path = std::env::temp_dir().join(format!(
        "anima-conversation-counts-{}.json",
        uuid::Uuid::new_v4()
    ));
    std::fs::write(&path, "{\"version\":21,\"agents\":[],\"swarms\":[]}").unwrap();
    let config = crate::control_plane_store::ControlPlaneStoreConfig::Json(path.clone());
    let loaded = crate::control_plane_store::load_control_plane_snapshot(&config)
        .await
        .unwrap()
        .unwrap();
    assert!(loaded.conversation_counts.is_empty());
    assert!(loaded.conversation_turns.is_empty());
    let _ = std::fs::remove_file(path);
}

fn turn_run(source: crate::runs::RunSource) -> crate::runs::RunRecord {
    let mut run = crate::runs::RunRecord::running(
        crate::runs::RunStart {
            agent_id: "owner".into(),
            session_id: "chat:a".into(),
            source,
            source_ref: None,
            idempotency_key: None,
            text: "hi".into(),
            model: "m".into(),
            provider: None,
            parent_run_id: None,
        },
        1,
    );
    run.status = crate::runs::RunStatus::Completed;
    run
}

fn owner_message(checkin: bool) -> anima_core::Message {
    let mut message = anima_core::Message {
        id: "m".into(),
        agent_id: "owner".into(),
        room_id: "chat:a".into(),
        content: anima_core::Content {
            text: "hi".into(),
            ..anima_core::Content::default()
        },
        role: anima_core::MessageRole::User,
        created_at_ms: 1,
    };
    if checkin {
        message.content.metadata = Some(std::collections::BTreeMap::from([(
            "kind".to_string(),
            anima_core::DataValue::String("checkin".into()),
        )]));
    }
    message
}

#[test]
fn only_completed_owner_chat_turns_count() {
    use crate::runs::RunSource;
    let agent: anima_core::AgentConfig =
        serde_json::from_value(serde_json::json!({"name":"owner","model":"m"})).unwrap();
    let owner = owner_message(false);
    let counts = |run: &crate::runs::RunRecord, message: &anima_core::Message, reply: &str| {
        counts_as_owner_turn(run, &agent, Some(message), Some(reply), true)
    };
    for source in [
        RunSource::Web,
        RunSource::Api,
        RunSource::Telegram,
        RunSource::Discord,
    ] {
        assert!(counts(&turn_run(source), &owner, "hello"), "{source:?}");
    }
    // A connector run counts only from a chat proven to hold the owner alone.
    for source in [RunSource::Telegram, RunSource::Discord] {
        assert!(
            !counts_as_owner_turn(
                &turn_run(source),
                &agent,
                Some(&owner),
                Some("hello"),
                false
            ),
            "{source:?} in a shared chat"
        );
    }
    for source in [RunSource::Web, RunSource::Api] {
        assert!(
            counts_as_owner_turn(
                &turn_run(source),
                &agent,
                Some(&owner),
                Some("hello"),
                false
            ),
            "{source:?} is the owner's console"
        );
    }
    for source in [
        RunSource::Schedule,
        RunSource::Job,
        RunSource::Delegation,
        RunSource::Peer,
    ] {
        assert!(!counts(&turn_run(source), &owner, "hello"), "{source:?}");
    }
    let web = turn_run(RunSource::Web);
    assert!(!counts(&web, &owner_message(true), "hello"), "a check-in");
    assert!(!counts(&web, &owner, "CHECKIN_OK"), "a silent reply");
    assert!(!counts(&web, &owner, "  "), "no reply");
    assert!(!counts_as_owner_turn(
        &web,
        &agent,
        None,
        Some("hello"),
        true
    ));
    let mut child = turn_run(RunSource::Web);
    child.parent_run_id = Some("parent".into());
    assert!(!counts(&child, &owner, "hello"), "a helper's run");
    let mut followup = turn_run(RunSource::Api);
    followup.system_followup = Some(true);
    assert!(!counts(&followup, &owner, "hello"), "a daemon follow-up");
    let mut failed = turn_run(RunSource::Web);
    failed.status = crate::runs::RunStatus::Failed;
    assert!(!counts(&failed, &owner, "hello"));
    let mut helper = agent.clone();
    let mut settings = anima_core::AgentSettings::default();
    settings.additional.insert(
        "workspaceRole".into(),
        anima_core::DataValue::String("helper".into()),
    );
    helper.settings = Some(settings);
    assert!(
        !counts_as_owner_turn(&web, &helper, Some(&owner), Some("hello"), true),
        "a helper agent"
    );
}

#[test]
fn an_owner_turn_sent_from_the_console_into_a_shared_thread_counts() {
    use crate::runs::RunSource;
    let agent: anima_core::AgentConfig =
        serde_json::from_value(serde_json::json!({"name":"owner","model":"m"})).unwrap();
    let tagged = |source: &str| {
        let mut message = owner_message(false);
        message.content.metadata = Some(std::collections::BTreeMap::from([(
            "source".to_string(),
            anima_core::DataValue::String(source.into()),
        )]));
        message
    };
    for (run_source, thread, inbound) in [
        (RunSource::Telegram, "telegramThread", "telegram"),
        (RunSource::Discord, "discordThread", "discord"),
    ] {
        let run = turn_run(run_source);
        let counts = |message: &anima_core::Message| {
            counts_as_owner_turn(&run, &agent, Some(message), Some("hello"), false)
        };
        assert!(counts(&tagged(thread)), "{run_source:?} console turn");
        assert!(
            !counts(&tagged(inbound)),
            "{run_source:?} shared-chat message"
        );
        assert!(!counts(&owner_message(false)), "{run_source:?} untagged");
    }
}

#[test]
fn only_a_private_telegram_chat_proves_a_connector_turn_is_the_owner_alone() {
    use crate::connectors::discord::model::{
        DiscordBotIdentity, DiscordChannelKind, DiscordChannelMetadata, DiscordConnectorRecord,
    };
    use crate::connectors::{
        InboundProcessingState, TelegramBotIdentity, TelegramChatKind, TelegramChatMetadata,
        TelegramConnectorRecord, TelegramInboundRecord, TelegramSenderMetadata,
    };
    use crate::runs::RunSource;

    let chat = |id: &str, kind| TelegramChatMetadata {
        id: id.into(),
        kind,
        title: None,
        username: None,
    };
    let telegram = |id: &str, approved: TelegramChatMetadata| TelegramConnectorRecord {
        id: id.into(),
        agent_id: "owner".into(),
        room_id: "room".into(),
        bot: TelegramBotIdentity {
            id: "bot".into(),
            username: None,
            display_name: None,
        },
        approved_chat: Some(approved),
        pending_pairing: None,
        next_update_id: 2,
        enabled: true,
        deleted_at_ms: None,
        created_at_ms: 1,
        updated_at_ms: 1,
    };
    let inbound = |connector: &str, chat: TelegramChatMetadata| TelegramInboundRecord {
        connector_id: connector.into(),
        update_id: 1,
        agent_id: "owner".into(),
        room_id: "room".into(),
        normalized_text: "hi".into(),
        sender: TelegramSenderMetadata {
            id: "someone".into(),
            username: None,
            display_name: None,
        },
        chat,
        received_at_ms: 1,
        processing_state: InboundProcessingState::Processing,
        run_idempotency_key: format!("telegram:{connector}:update:1"),
    };
    let mut state = crate::state::DaemonState::new();
    for (id, kind) in [
        ("tg-private", TelegramChatKind::Private),
        ("tg-group", TelegramChatKind::Group),
        ("tg-supergroup", TelegramChatKind::Supergroup),
    ] {
        let paired = chat(&format!("chat-{id}"), kind);
        state
            .connectors
            .insert(id.into(), telegram(id, paired.clone()));
        state.inbound.insert((id.into(), 1), inbound(id, paired));
    }
    state.discord_connectors.insert(
        "dc".into(),
        DiscordConnectorRecord {
            id: "dc".into(),
            agent_id: "owner".into(),
            room_id: "discord:dc".into(),
            bot: DiscordBotIdentity {
                id: "11".into(),
                username: None,
                display_name: None,
            },
            channel: DiscordChannelMetadata {
                id: "22".into(),
                guild_id: "33".into(),
                name: None,
                kind: DiscordChannelKind::GuildText,
                parent_channel_id: None,
            },
            bot_wide: false,
            guild_scope: None,
            application_id: None,
            settings: None,
            configuration_revision: 0,
            conversations: vec![],
            thread_starts: Default::default(),
            approved_user_ids: vec!["44".into(), "55".into()],
            credential_generation: 1,
            cursor: "0".into(),
            activated_at_ms: Some(1),
            enabled: true,
            deleted_at_ms: None,
            created_at_ms: 1,
            updated_at_ms: 1,
        },
    );
    let run = |source, source_ref: Option<&str>| {
        let mut run = turn_run(source);
        run.source_ref = source_ref.map(Into::into);
        run
    };
    let proven = |state: &crate::state::DaemonState, run: &crate::runs::RunRecord| {
        super::owner_only_connector_chat(state, run)
    };

    assert!(proven(
        &state,
        &run(RunSource::Telegram, Some("tg-private:1"))
    ));
    assert!(
        !proven(&state, &run(RunSource::Telegram, Some("tg-group:1"))),
        "a group"
    );
    assert!(!proven(
        &state,
        &run(RunSource::Telegram, Some("tg-supergroup:1"))
    ));
    assert!(
        !proven(&state, &run(RunSource::Telegram, Some("tg-private:9"))),
        "unknown update"
    );
    assert!(
        !proven(&state, &run(RunSource::Telegram, None)),
        "nothing to prove it"
    );
    // Discord binds a guild text channel shared by its approved users; no
    // Discord connector can hold a direct message yet.
    assert!(
        !proven(&state, &run(RunSource::Discord, Some("dc:99"))),
        "a guild channel"
    );
    assert!(
        !proven(&state, &run(RunSource::Discord, Some("dc"))),
        "a thread run"
    );
    assert!(
        !proven(&state, &run(RunSource::Web, None)),
        "only connector runs are proven here"
    );

    // A private chat that later re-pairs to a group no longer proves it.
    state
        .connectors
        .get_mut("tg-private")
        .unwrap()
        .approved_chat = Some(chat("chat-other", TelegramChatKind::Group));
    assert!(!proven(
        &state,
        &run(RunSource::Telegram, Some("tg-private:1"))
    ));
}

fn settled_conversation(agent: &str, index: usize, at: u64) -> LearningReviewRecord {
    let mut review = conversation(agent, &format!("old-{index:03}"), at);
    review.finished_at_ms = Some(at);
    review
}

fn pending_lesson() -> LearningProposal {
    LearningProposal {
        id: "p".into(),
        observation: "o".into(),
        suggestion: "s".into(),
        anchors: vec!["reply".into()],
        decision: LearningDecision::Pending,
        decision_version: 0,
        skill_draft_id: None,
        memory_id: None,
        core_edit: None,
        skill_candidate: None,
        skill_error: None,
    }
}

#[test]
fn a_full_conversation_store_prunes_the_agents_oldest_settled_review() {
    let mut registry = LearningRegistry::default();
    // The oldest one still waits for the owner, so it is kept.
    let mut waiting = settled_conversation("owner", 0, 1);
    waiting.proposals.push(pending_lesson());
    registry.insert_for_tests(waiting);
    for index in 1..CONVERSATION_RETAINED_LIMIT {
        registry.insert_for_tests(settled_conversation("owner", index, 1 + index as u64));
    }
    // Another agent's reviews are never pruned for this one.
    registry.insert_for_tests(settled_conversation("other", 0, 0));
    let later = 10 * WINDOW_MS;
    registry
        .reserve(conversation("owner", "new", later))
        .unwrap();
    let ids: HashSet<String> = registry.reviews.keys().cloned().collect();
    assert!(
        !ids.contains("conversation-owner-old-001"),
        "the oldest settled one goes"
    );
    assert!(
        ids.contains("conversation-owner-old-000"),
        "a pending one stays"
    );
    assert!(ids.contains("conversation-other-old-000"));
    assert!(ids.contains("conversation-owner-new"));
    assert_eq!(
        registry
            .reviews
            .values()
            .filter(|r| r.agent_id == "owner")
            .count(),
        CONVERSATION_RETAINED_LIMIT
    );
}

#[test]
fn a_full_conversation_store_with_nothing_settled_refuses() {
    let mut registry = LearningRegistry::default();
    for index in 0..CONVERSATION_RETAINED_LIMIT {
        let mut review = settled_conversation("owner", index, 1 + index as u64);
        review.proposals.push(pending_lesson());
        registry.insert_for_tests(review);
    }
    let refused = registry
        .reserve(conversation("owner", "new", 10 * WINDOW_MS))
        .unwrap_err();
    assert!(
        refused.message().contains("storage limit"),
        "{}",
        refused.message()
    );
    assert_eq!(registry.reviews.len(), CONVERSATION_RETAINED_LIMIT);
}

#[test]
fn conversation_volume_never_uses_the_task_review_store() {
    let mut registry = LearningRegistry::default();
    for index in 0..CONVERSATION_RETAINED_LIMIT {
        registry.insert_for_tests(settled_conversation("owner", index, 1));
    }
    for index in 0..RETAINED_LIMIT - 1 {
        let mut review = record(&format!("task-{index}"), 1);
        review.state = LearningReviewState::Completed;
        registry.insert_for_tests(review);
    }
    let later = 10 * WINDOW_MS;
    registry.reserve(record("last-task", later)).unwrap();
    assert!(registry.reserve(record("one-too-many", later + 1)).is_err());
    // Task reviews are never pruned.
    assert_eq!(
        registry
            .reviews
            .values()
            .filter(|r| r.trigger != LearningTrigger::Conversation)
            .count(),
        RETAINED_LIMIT
    );
}

#[test]
fn a_stored_learning_registry_above_the_caps_loads_without_pruning() {
    let mut snapshot = LearningSnapshot::default();
    for index in 0..CONVERSATION_RETAINED_LIMIT + 50 {
        snapshot
            .reviews
            .push(settled_conversation("owner", index, 1 + index as u64));
    }
    for index in 0..RETAINED_LIMIT {
        snapshot.reviews.push(record(&format!("task-{index}"), 1));
    }
    LearningRegistry::validate(&snapshot).unwrap();
    let restored = LearningRegistry::restored(snapshot, &HashSet::from(["owner".into()]), 2);
    assert_eq!(
        restored.reviews.len(),
        CONVERSATION_RETAINED_LIMIT + 50 + RETAINED_LIMIT
    );
}

#[test]
fn a_rolled_back_turn_takes_back_only_itself() {
    let mut registry = LearningRegistry::default();
    let first = registry.count_turn("owner", "run-a");
    // Another run counts while the first one's save is pending.
    registry.count_turn("owner", "run-b");
    registry.uncount_turn(first);
    assert_eq!(registry.conversation_counts["owner"], 1);
    assert_eq!(
        registry.conversation_turns("owner"),
        vec!["run-b".to_string()]
    );
}

#[test]
fn skill_learning_review_accepts_an_anchored_targeted_skill_patch() {
    let response = response(
        r#"{"proposals":[{"observation":"The procedure omitted checking the saved result.","suggestion":"Read back the output before reporting success.","anchors":["input"],"skill":{"slug":"notes","expected_hash":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","patches":[{"old_text":"Write the file.","new_text":"Write the file, then read it back."}]}}]}"#,
    );
    let proposals = super::service::parse_proposals(&response, &["input".into()], false);
    assert!(
        proposals.is_ok(),
        "a bounded skill patch must survive the reviewer parser"
    );
}
