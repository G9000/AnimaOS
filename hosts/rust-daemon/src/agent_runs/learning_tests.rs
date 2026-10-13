//! The actual completed-task hook must stay separate from the primary loop.
use super::test_support::{
    accept_web, add_chat, coordinator_state, coordinator_with, wait_for, ScriptedModel, Step,
};
use crate::control_plane_store::ControlPlaneStoreConfig;
use crate::runs::RunStatus;
use anima_core::{DataValue, ToolCall};
use std::collections::BTreeMap;

#[tokio::test]
async fn m38_actual_completed_run_hook_defaults_off_and_opt_in_never_forks_primary_runtime() {
    for auto_enabled in [false, true] {
        let model = ScriptedModel::new(vec![
            Step::Tools(vec![ToolCall {
                id: "learning-calculate".into(),
                name: "calculate".into(),
                args: BTreeMap::from([("expression".into(), DataValue::String("2 + 2".into()))]),
            }]),
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
            if auto_enabled
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
            if !auto_enabled
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
        assert_eq!(reviews.len(), usize::from(auto_enabled));
        assert_eq!(model.requests().len(), if auto_enabled { 3 } else { 2 });
        if auto_enabled {
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
