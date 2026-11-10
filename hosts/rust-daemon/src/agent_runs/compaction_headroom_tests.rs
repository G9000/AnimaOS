//! Regressions for compaction runway, retry cooldown, and phase transitions.

use super::*;

#[tokio::test]
async fn expanded_compaction_hashes_original_images_before_prompt_image_trimming() {
    use sha2::{Digest, Sha256};
    let model = ScriptedModel::new(vec![]);
    let (coordinator, agent_id) = session(model.clone(), 24_000.0, true).await;
    let canonical = {
        let mut state = coordinator.state.write().await;
        let mut snapshot = state.agents[&agent_id].snapshot();
        for n in 0..16 {
            let mut user = message(
                &agent_id,
                &format!("image-u{n}"),
                MessageRole::User,
                &"u".repeat(4_000),
            );
            user.content.attachments = Some(vec![anima_core::Attachment {
                attachment_type: anima_core::AttachmentType::Image,
                name: format!("image-{n}.png"),
                data: format!("uploads/image-{n}.png"),
                mime_type: Some("image/png".into()),
            }]);
            snapshot.messages.extend([
                user,
                message(
                    &agent_id,
                    &format!("image-a{n}"),
                    MessageRole::Assistant,
                    &"a".repeat(4_000),
                ),
            ]);
        }
        let canonical = snapshot.messages.clone();
        state.agents.insert(
            agent_id.clone(),
            anima_core::AgentRuntime::from_snapshot(snapshot, model),
        );
        canonical
    };
    let (build, _, _) = run_parts(&coordinator, &agent_id, &book_one()).await;
    let demoted = build
        .runtime
        .messages()
        .iter()
        .find(|m| {
            m.id.starts_with("image-u")
                && m.content.attachments.is_none()
                && build
                    .context
                    .compaction_input
                    .iter()
                    .any(|source| source.id == m.id)
        })
        .expect("the fixture must expand a selected turn whose image was demoted for the prompt");
    let original = canonical.iter().find(|m| m.id == demoted.id).unwrap();
    let captured = build
        .context
        .compaction_input
        .iter()
        .find(|m| m.id == demoted.id)
        .unwrap();
    assert_eq!(
        captured, original,
        "summary input must retain canonical attachments and text"
    );
    let inputs = crate::sessions::checkpoint::CheckpointInputs::capture(
        &agent_id,
        "chat:long",
        "chat:long",
        None,
        &build.context.compaction_input,
    );
    let expected = hex::encode(Sha256::digest(serde_json::to_vec(original).unwrap()));
    assert!(
        inputs.request_data(None).contains(&expected),
        "checkpoint source hashes bind to stored messages"
    );
}

#[tokio::test]
async fn compaction_boundary_respects_cold_history_newer_than_a_pinned_hot_turn() {
    let model = ScriptedModel::with_secondary(
        vec![Step::Text(vec!["Booked"])],
        vec![Step::Text(vec!["Trip and budget discussion."])],
    );
    let (coordinator, agent_id) = long_session(model.clone(), true).await;
    {
        let mut state = coordinator.state.write().await;
        let mut snapshot = state.agents[&agent_id].snapshot();
        snapshot.messages[0].created_at_ms = 1;
        snapshot.messages[1].created_at_ms = 2;
        state.agents.insert(
            agent_id.clone(),
            anima_core::AgentRuntime::from_snapshot(snapshot, model.clone()),
        );
    }
    seed_pruned(&coordinator, &agent_id).await;
    coordinator
        .run(chat_request(&agent_id, "chat:long", "book one"))
        .await
        .unwrap();
    let state = coordinator.state.read().await;
    let record = state.sessions.get(&agent_id, "chat:long").unwrap();
    assert_eq!(
        record.summary.as_ref().unwrap().through_message_id,
        "p6",
        "cold history is newer than a1"
    );
    assert!(
        model.requests()[0].messages.iter().any(|m| m.id == "u2"),
        "the latest hot exchange remains available"
    );
}

#[tokio::test]
async fn automatic_compaction_leaves_room_for_followup_messages() {
    let model = ScriptedModel::with_secondary(
        (0..4).map(|_| Step::Text(vec!["Continuing"])).collect(),
        vec![Step::Text(vec![
            "Earlier work is saved in conversation history.",
        ])],
    );
    let (coordinator, agent_id) = session(model.clone(), 2_000.0, true).await;
    {
        let mut state = coordinator.state.write().await;
        let messages = (0..12)
            .flat_map(|n| {
                [
                    message(
                        &agent_id,
                        &format!("work-u{n}"),
                        MessageRole::User,
                        &format!("Task {n}: {}", "x".repeat(416)),
                    ),
                    message(
                        &agent_id,
                        &format!("work-a{n}"),
                        MessageRole::Assistant,
                        &format!("Result {n}: {}", "y".repeat(416)),
                    ),
                ]
            })
            .collect();
        state
            .agents
            .get_mut(&agent_id)
            .unwrap()
            .apply_run_delta(&RuntimeRunDelta {
                messages,
                events: vec![],
                event_total: 0,
                token_usage: TokenUsage::default(),
                step_count: 0,
                last_task: None,
                status: AgentStatus::Idle,
            });
    }
    coordinator
        .run(chat_request(&agent_id, "chat:long", "continue"))
        .await
        .unwrap();
    let first = &model.requests()[0];
    assert!(
        first.messages.iter().any(|m| m.id == "work-u11"),
        "latest complete exchange survives"
    );
    assert!(
        first
            .messages
            .iter()
            .filter(|m| m.id.starts_with("work-"))
            .count()
            < 12,
        "compaction must reclaim a larger block, not only overflow"
    );
    for _ in 0..3 {
        coordinator
            .run(chat_request(&agent_id, "chat:long", "continue"))
            .await
            .unwrap();
    }
    assert_eq!(
        model.secondary_requests().len(),
        1,
        "short followups must not repeatedly pay for a summary"
    );
    let state = coordinator.state.read().await;
    let record = state.sessions.get(&agent_id, "chat:long").unwrap();
    assert!(record.compaction_error.is_none());
    assert!(record.context_trimmed.is_none());
}

#[tokio::test]
async fn failed_automatic_compaction_waits_before_retrying_but_manual_retry_works() {
    let model = ScriptedModel::with_secondary(
        vec![Step::Text(vec!["Booked"]), Step::Text(vec!["Continuing"])],
        vec![
            Step::Fail("rate limited"),
            Step::Text(vec!["They planned a trip."]),
        ],
    );
    let (coordinator, agent_id) = long_session(model.clone(), true).await;
    coordinator
        .run(chat_request(&agent_id, "chat:long", "book one"))
        .await
        .unwrap();
    coordinator
        .state
        .write()
        .await
        .sessions
        .get_mut(&agent_id, "chat:long")
        .unwrap()
        .context_calibration_permille = Some(1_000);
    coordinator
        .run(chat_request(&agent_id, "chat:long", "continue"))
        .await
        .unwrap();
    assert_eq!(
        model.secondary_requests().len(),
        1,
        "a new message must not retry a failing summarizer immediately"
    );
    let dropped = {
        let state = coordinator.state.read().await;
        let record = state.sessions.get(&agent_id, "chat:long").unwrap();
        let restored: SessionRecord =
            serde_json::from_value(serde_json::to_value(record).unwrap()).unwrap();
        assert_eq!(
            restored.compaction_error, record.compaction_error,
            "cooldown evidence survives restart"
        );
        crate::sessions::compaction::manual_compaction_input(
            &state.model_visible_history(&agent_id, "chat:long"),
            record.summary.as_ref(),
        )
    };
    coordinator
        .compact_session(&agent_id, "chat:long", &dropped)
        .await
        .unwrap();
    assert_eq!(
        model.secondary_requests().len(),
        2,
        "manual retry bypasses automatic cooldown"
    );
    assert!(coordinator
        .state
        .read()
        .await
        .sessions
        .get(&agent_id, "chat:long")
        .unwrap()
        .compaction_error
        .is_none());
}

#[tokio::test]
async fn automatic_compaction_retries_after_failure_cooldown_expires() {
    let model = ScriptedModel::with_secondary(
        vec![Step::Text(vec!["Booked"]), Step::Text(vec!["Continuing"])],
        vec![
            Step::Fail("rate limited"),
            Step::Text(vec!["Trip discussion."]),
        ],
    );
    let (coordinator, agent_id) = long_session(model.clone(), true).await;
    coordinator
        .run(chat_request(&agent_id, "chat:long", "book one"))
        .await
        .unwrap();
    {
        let mut state = coordinator.state.write().await;
        let session = state.sessions.get_mut(&agent_id, "chat:long").unwrap();
        session.compaction_error.as_mut().unwrap().at_ms = 0;
        session.context_calibration_permille = Some(1_000);
    }
    coordinator
        .run(chat_request(&agent_id, "chat:long", "continue"))
        .await
        .unwrap();
    assert_eq!(model.secondary_requests().len(), 2);
    assert!(coordinator
        .state
        .read()
        .await
        .sessions
        .get(&agent_id, "chat:long")
        .unwrap()
        .compaction_error
        .is_none());
}

#[tokio::test]
async fn compaction_phase_ends_before_main_model_output_on_success_or_failure() {
    for secondary in [
        Step::Text(vec!["Trip discussion."]),
        Step::Fail("rate limited"),
    ] {
        let model =
            ScriptedModel::with_secondary(vec![Step::Text(vec!["Booked"])], vec![secondary]);
        let (coordinator, agent_id) = long_session(model, true).await;
        let mut events = coordinator
            .state
            .read()
            .await
            .live
            .subscribe(&agent_id)
            .unwrap();
        coordinator
            .run(chat_request(&agent_id, "chat:long", "book one"))
            .await
            .unwrap();
        let events = events_until(&mut events, "run.completed").await;
        let responding = events
            .iter()
            .position(|e| e["type"] == "run.progress" && e["phase"] == "responding")
            .expect("compaction must explicitly finish before main model work");
        let text = events
            .iter()
            .position(|e| e["type"] == "step.delta")
            .unwrap();
        assert!(responding < text);
    }
}
