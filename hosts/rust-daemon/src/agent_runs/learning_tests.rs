//! The actual completed-task hook must stay separate from the primary loop.
use super::test_support::{
    accept_web, add_chat, coordinator_state, coordinator_with, wait_for, ScriptedModel, Step,
};
use crate::control_plane_store::ControlPlaneStoreConfig;
use crate::learning::{LearningDecision, LearningMode, LearningReviewState};
use crate::runs::RunStatus;
use anima_core::{DataValue, ToolCall};
use std::collections::BTreeMap;

/// The owner's autonomy decides the learning mode; Free maps to Free.
fn set_mode(state: &mut crate::state::DaemonState, agent: &str, mode: LearningMode) {
    use crate::approvals::autonomy::{AgentAutonomy, AutonomyMode};
    state.autonomy.set(AgentAutonomy {
        agent_id: agent.into(),
        mode: match mode {
            LearningMode::Free => AutonomyMode::Free,
            LearningMode::Strict => AutonomyMode::Strict,
        },
        ..AgentAutonomy::default()
    });
}

#[tokio::test]
async fn m38_actual_completed_run_hook_defaults_off_and_opt_in_never_forks_primary_runtime() {
    // Free reviews long tasks by itself; Strict only after the opt-in. A
    // short task never spends the budget.
    for (mode, auto_enabled, calls, expect_review) in [
        (LearningMode::Strict, false, 8, false),
        (LearningMode::Strict, true, 8, true),
        (LearningMode::Strict, true, 1, false),
        (LearningMode::Free, false, 1, false),
        (LearningMode::Free, false, 5, true),
    ] {
        let model = ScriptedModel::new(vec![
            Step::Tools(
                (0..calls)
                    .map(|index| ToolCall {
                        id: format!("learning-calculate-{index}"),
                        name: "calculate".into(),
                        args: BTreeMap::from([(
                            "expression".into(),
                            DataValue::String("2 + 2".into()),
                        )]),
                    })
                    .collect(),
            ),
            Step::Text(vec![
                "The arithmetic was checked; no broader goal is proved.",
            ]),
            Step::Text(vec!["{\"proposals\":[]}"]),
        ]);
        let (coordinator, agent) = coordinator_with(model.clone()).await;
        add_chat(&coordinator, &agent, "chat:learning-hook").await;
        let path =
            std::env::temp_dir().join(format!("anima-learning-hook-{}.json", uuid::Uuid::new_v4()));
        let state = coordinator_state(&coordinator);
        state
            .write()
            .await
            .set_control_plane_store(Some(ControlPlaneStoreConfig::Json(path.clone())));
        set_mode(&mut *state.write().await, &agent, mode);
        if auto_enabled {
            coordinator
                .learning()
                .set_policy(&agent, true)
                .await
                .unwrap();
        }
        let source_id = accept_web(
            &coordinator,
            &agent,
            "chat:learning-hook",
            "check the arithmetic",
        )
        .await;
        wait_for(&coordinator, &source_id, RunStatus::Completed).await;
        for _ in 0..500 {
            let reviews = coordinator.learning().list(&agent).await.unwrap();
            if expect_review
                && reviews.first().is_some_and(|r| {
                    !matches!(
                        r.state,
                        crate::learning::LearningReviewState::Queued
                            | crate::learning::LearningReviewState::Running
                    )
                })
            {
                break;
            }
            if !expect_review
                && state
                    .read()
                    .await
                    .agents
                    .get(&agent)
                    .unwrap()
                    .snapshot()
                    .last_task
                    .is_some()
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        // Let the detached default-off hook settle; it never enters the model.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let reviews = coordinator.learning().list(&agent).await.unwrap();
        assert_eq!(reviews.len(), usize::from(expect_review));
        assert_eq!(model.requests().len(), if expect_review { 3 } else { 2 });
        if expect_review {
            assert_eq!(
                reviews[0].state,
                crate::learning::LearningReviewState::Completed
            );
            assert_eq!(reviews[0].source_run_id, source_id);
            assert_eq!(
                reviews[0].trigger,
                crate::learning::LearningTrigger::Automatic
            );
        }
        let guard = state.read().await;
        assert_eq!(
            guard
                .runs
                .snapshot_records(&guard.agents.keys().cloned().collect())
                .len(),
            1,
            "shadow review creates no child task"
        );
        let runtime = guard.agents.get(&agent).unwrap();
        assert!(
            !runtime
                .messages()
                .iter()
                .any(|m| m.content.text == "{\"proposals\":[]}"),
            "review output never enters primary chat"
        );
        assert!(!runtime
            .config()
            .settings
            .as_ref()
            .unwrap()
            .additional
            .contains_key("autoLearningReview"));
        assert!(runtime.config().allows_tool("calculate"));
        drop(guard);
        let _ = std::fs::remove_file(path);
    }
}

/// Runs one long task whose automatic review returns one lesson.
async fn reviewed_long_task(
    mode: LearningMode,
) -> (
    super::AgentRunCoordinator,
    String,
    crate::app::SharedDaemonState,
    crate::learning::LearningReviewRecord,
    std::path::PathBuf,
) {
    let model = ScriptedModel::new(vec![
        Step::Tools(
            (0..5)
                .map(|index| ToolCall {
                    id: format!("lesson-calculate-{index}"),
                    name: "calculate".into(),
                    args: BTreeMap::from([(
                        "expression".into(),
                        DataValue::String("2 + 2".into()),
                    )]),
                })
                .collect(),
        ),
        Step::Text(vec!["Checked the arithmetic five times."]),
        Step::Text(vec![
            "{\"proposals\":[{\"observation\":\"The same sum was checked five times.\",\"suggestion\":\"Check a sum once, then move on.\",\"anchors\":[\"input\"]}]}",
        ]),
    ]);
    let (coordinator, agent) = coordinator_with(model).await;
    add_chat(&coordinator, &agent, "chat:lesson").await;
    let path = std::env::temp_dir().join(format!("anima-lesson-{}.json", uuid::Uuid::new_v4()));
    let state = coordinator_state(&coordinator);
    {
        let mut guard = state.write().await;
        guard.set_control_plane_store(Some(ControlPlaneStoreConfig::Json(path.clone())));
        set_mode(&mut guard, &agent, mode);
    }
    if mode == LearningMode::Strict {
        coordinator
            .learning()
            .set_policy(&agent, true)
            .await
            .unwrap();
    }
    let source = accept_web(&coordinator, &agent, "chat:lesson", "check the sum").await;
    wait_for(&coordinator, &source, RunStatus::Completed).await;
    for _ in 0..500 {
        let reviews = coordinator.learning().list(&agent).await.unwrap();
        if let Some(review) = reviews.first().filter(|r| {
            r.state == LearningReviewState::Completed
                && (mode == LearningMode::Strict || r.proposals[0].memory_id.is_some())
        }) {
            let review = review.clone();
            return (coordinator, agent, state, review, path);
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("the automatic review never settled");
}

#[tokio::test]
async fn free_mode_keeps_a_lesson_in_memory_and_undo_forgets_it() {
    let (coordinator, agent, state, review, path) = reviewed_long_task(LearningMode::Free).await;
    let lesson = &review.proposals[0];
    assert_eq!(lesson.decision, LearningDecision::Acknowledged);
    let memory_id = lesson.memory_id.clone().unwrap();
    let memory = state.read().await.memory_handle();
    let saved = memory.read().await.get(&memory_id).unwrap();
    assert_eq!(saved.content, "Check a sum once, then move on.");
    assert_eq!(saved.agent_id, agent);

    let stale = coordinator
        .learning()
        .undo_lesson(&agent, &review.id, &lesson.id, lesson.decision_version + 1)
        .await
        .unwrap_err();
    assert_eq!(stale.status(), axum::http::StatusCode::CONFLICT);
    let undone = coordinator
        .learning()
        .undo_lesson(&agent, &review.id, &lesson.id, lesson.decision_version)
        .await
        .unwrap();
    assert_eq!(undone.proposals[0].decision, LearningDecision::Dismissed);
    assert!(memory.read().await.get(&memory_id).is_none());
    // A second undo finds nothing left to forget.
    assert!(coordinator
        .learning()
        .undo_lesson(
            &agent,
            &review.id,
            &lesson.id,
            undone.proposals[0].decision_version
        )
        .await
        .is_err());
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn strict_mode_leaves_the_lesson_waiting_for_the_owner() {
    let (_coordinator, _agent, state, review, path) =
        reviewed_long_task(LearningMode::Strict).await;
    // Give a mistaken keep step time to run before asserting it did not.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let lesson = &review.proposals[0];
    assert_eq!(lesson.decision, LearningDecision::Pending);
    assert!(lesson.memory_id.is_none());
    let guard = state.read().await;
    let current = &guard.learning.snapshot().reviews[0];
    assert_eq!(current.proposals[0].decision, LearningDecision::Pending);
    assert!(current.proposals[0].memory_id.is_none());
    drop(guard);
    let _ = std::fs::remove_file(path);
}
