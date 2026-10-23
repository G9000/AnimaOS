use super::AgentRunCoordinator;
use crate::agent_runs::test_support::{add_chat, coordinator_with, ScriptedModel};
use crate::chat_results::SaveResultInput;
use serde_json::json;

fn input() -> SaveResultInput {
    serde_json::from_value(json!({"expectedRevision":0,"requestKey":"report","title":"Saved report","payload":{"kind":"comparison","columns":["Name","Value"],"rows":[["One","10"]]}})).unwrap()
}
async fn fixture() -> (AgentRunCoordinator, String, std::path::PathBuf) {
    let (coordinator, agent) = coordinator_with(ScriptedModel::new(vec![])).await;
    add_chat(&coordinator, &agent, "chat:results").await;
    let path = std::env::temp_dir().join(format!("anima-results-{}.json", uuid::Uuid::new_v4()));
    coordinator
        .state
        .write()
        .await
        .set_control_plane_store(Some(
            crate::control_plane_store::ControlPlaneStoreConfig::Json(path.clone()),
        ));
    (coordinator, agent, path)
}

#[tokio::test]
async fn owner_results_are_durable_and_reject_wrong_conversation_and_generation() {
    let (coordinator, agent, path) = fixture().await;
    let saved = coordinator
        .save_chat_result_owner(&agent, "chat:results", 0, input())
        .await
        .unwrap();
    assert_eq!(saved.revision, 1);
    let pinned = coordinator
        .pin_chat_result_owner(&agent, "chat:results", 0, &saved.id, 1, true)
        .await
        .unwrap();
    assert!(pinned.pinned);
    let disk: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let encoded = disk.to_string();
    assert!(encoded.contains(&saved.id));
    assert!(encoded.contains("\"pinned\":true"));
    for (wrong_agent, wrong_session, generation) in [
        ("other", "chat:results", 0),
        (agent.as_str(), "chat:other", 0),
        (agent.as_str(), "chat:results", 1),
    ] {
        assert!(coordinator
            .pin_chat_result_owner(wrong_agent, wrong_session, generation, &saved.id, 2, false)
            .await
            .is_err());
    }
    assert!(coordinator
        .pin_chat_result_owner(&agent, "chat:results", 0, &saved.id, 1, false)
        .await
        .is_err());
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn dropped_caller_cannot_leave_unacknowledged_results_after_failed_persistence() {
    let (coordinator, agent, path) = fixture().await;
    let gate = coordinator
        .state
        .write()
        .await
        .install_test_control_plane_save_gate(true);
    let worker = {
        let coordinator = coordinator.clone();
        let agent = agent.clone();
        tokio::spawn(async move {
            coordinator
                .save_chat_result_owner(&agent, "chat:results", 0, input())
                .await
        })
    };
    gate.entered.acquire().await.unwrap().forget();
    worker.abort();
    gate.release.add_permits(1);
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        // The transaction lease is released only after the detached worker's rollback.
        let _transaction = coordinator.control_plane_transaction().await;
        assert!(coordinator
            .state
            .read()
            .await
            .sessions
            .get(&agent, "chat:results")
            .unwrap()
            .chat_results
            .is_empty());
    })
    .await
    .unwrap();
    let _ = std::fs::remove_file(path);
}
