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

#[tokio::test]
async fn tracker_owner_edits_survive_durable_reload_and_stale_replacements_fail() {
    let (coordinator, agent, path) = fixture().await;
    let mut create = input();
    create.payload = serde_json::from_value(json!({"kind":"dashboard","widgets":[
        {"id":"tasks","title":"Tasks","payload":{"kind":"tracker","columns":[{"key":"task","label":"Task","kind":"text"},{"key":"done","label":"Done","kind":"checkbox"}],"rows":[{"id":"first","cells":{"task":"Draft","done":false}}]}}
    ]})).unwrap();
    let first = coordinator
        .save_chat_result_owner(&agent, "chat:results", 0, create)
        .await
        .unwrap();
    let mut edited_payload = serde_json::to_value(&first.payload).unwrap();
    edited_payload["widgets"][0]["payload"]["rows"][0]["cells"]["done"] = json!(true);
    let mut edit = input();
    edit.id = Some(first.id.clone());
    edit.expected_revision = first.revision;
    edit.request_key = "owner_edit".into();
    edit.payload = serde_json::from_value(edited_payload).unwrap();
    let saved = coordinator
        .save_chat_result_owner(&agent, "chat:results", 0, edit)
        .await
        .unwrap();
    let mut stale = input();
    stale.id = Some(first.id);
    stale.expected_revision = 1;
    stale.request_key = "stale_replace".into();
    assert!(coordinator
        .save_chat_result_owner(&agent, "chat:results", 0, stale)
        .await
        .is_err());
    let snapshot = crate::control_plane_store::load_control_plane_snapshot(
        &crate::control_plane_store::ControlPlaneStoreConfig::Json(path.clone()),
    )
    .await
    .unwrap()
    .unwrap();
    let session = snapshot
        .sessions
        .iter()
        .find(|session| session.agent_id == agent && session.id == "chat:results")
        .unwrap();
    assert_eq!(session.chat_results, vec![saved]);
    assert!(crate::chat_results::validate_session(session).is_ok());
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn model_result_publication_rechecks_current_owner_eligibility() {
    let (coordinator, agent, path) = fixture().await;
    let link =
        crate::agent_runs::test_support::ledger_run(&coordinator, &agent, "chat:results").await;
    coordinator
        .state
        .read()
        .await
        .live
        .runs()
        .register(&link.run_id);
    coordinator
        .state
        .write()
        .await
        .sessions
        .get_mut(&agent, "chat:results")
        .unwrap()
        .origin = crate::sessions::SessionOrigin::Peer;
    // Owner controls are host-derived. Revocation of the owner conversation's
    // eligibility, rather than the raw saved grant list, must stop publication.
    assert!(coordinator
        .save_chat_result_from_run(&link, "call", input())
        .await
        .is_err());
    assert!(coordinator
        .state
        .read()
        .await
        .sessions
        .get(&agent, "chat:results")
        .unwrap()
        .chat_results
        .is_empty());
    coordinator
        .state
        .write()
        .await
        .sessions
        .get_mut(&agent, "chat:results")
        .unwrap()
        .origin = crate::sessions::SessionOrigin::Web;
    coordinator
        .state
        .write()
        .await
        .runs
        .get_mut(&link.run_id)
        .unwrap()
        .system_followup = Some(true);
    assert!(coordinator
        .save_chat_result_from_run(&link, "followup_call", input())
        .await
        .is_err());
    coordinator
        .state
        .write()
        .await
        .runs
        .get_mut(&link.run_id)
        .unwrap()
        .system_followup = None;
    coordinator
        .state
        .read()
        .await
        .live
        .runs()
        .control(&link.run_id)
        .unwrap()
        .cancel
        .cancel();
    assert!(coordinator
        .save_chat_result_from_run(&link, "cancelled_call", input())
        .await
        .is_err());
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn ordinary_owner_chat_publishes_a_dashboard_through_host_derived_result_controls() {
    use crate::agent_runs::test_support::{chat_request, Step};
    use anima_core::{DataValue, ToolCall};
    let payload = json!({"kind":"dashboard","widgets":[{"id":"tasks","title":"Tasks","payload":{"kind":"tracker","columns":[{"key":"task","label":"Task","kind":"text"},{"key":"done","label":"Done","kind":"checkbox"}],"rows":[{"id":"first","cells":{"task":"Draft","done":false}}]}}]});
    let DataValue::Object(args) = crate::tools::generated_documents::schema_data(
        json!({"expectedRevision":0,"requestKey":"derived_result","title":"Project dashboard","payload":payload}),
    ) else {
        unreachable!("fixture arguments are an object")
    };
    let model = ScriptedModel::new(vec![
        Step::Tools(vec![ToolCall {
            id: "derived-result-call".into(),
            name: "show_result".into(),
            args,
        }]),
        Step::Text(vec!["The project dashboard is saved."]),
    ]);
    let (coordinator, agent, path) = fixture().await;
    {
        let mut state = coordinator.state.write().await;
        state.model_adapter = model.clone();
        let raw_config = state.get_agent(&agent).unwrap().state.config;
        assert_eq!(
            raw_config
                .tools
                .as_ref()
                .unwrap()
                .iter()
                .map(|tool| tool.name.as_str())
                .collect::<Vec<_>>(),
            vec!["calculate"]
        );
    }
    let completed = coordinator
        .run(chat_request(
            &agent,
            "chat:results",
            "Create a project dashboard with a task tracker.",
        ))
        .await
        .unwrap();
    assert_eq!(completed.result.status, "success");
    assert!(
        model.configs()[0].allows_tool("show_result"),
        "the ordinary owner provider request must receive the host control"
    );
    {
        let state = coordinator.state.read().await;
        assert!(
            !state
                .get_agent(&agent)
                .unwrap()
                .state
                .config
                .allows_tool("show_result"),
            "host-derived controls must not expand saved grants"
        );
        let saved = &state
            .sessions
            .get(&agent, "chat:results")
            .unwrap()
            .chat_results;
        assert_eq!(
            saved.len(),
            1,
            "the model's normal native result call must save its dashboard"
        );
        assert_eq!(serde_json::to_value(&saved[0].payload).unwrap(), payload);
        assert_eq!(saved[0].source_tool_call_id, "derived-result-call");
        let record = state
            .runs
            .for_agent(&agent)
            .into_iter()
            .find(|record| record.session_id == "chat:results")
            .unwrap();
        let receipt = record
            .action_receipts
            .iter()
            .find(|receipt| receipt.name == "show_result")
            .unwrap();
        assert_eq!(receipt.status, crate::runs::ActionStatus::Succeeded);
        assert_eq!(
            receipt.durability,
            crate::runs::ActionDurability::Persistent
        );
    }
    let snapshot = crate::control_plane_store::load_control_plane_snapshot(
        &crate::control_plane_store::ControlPlaneStoreConfig::Json(path.clone()),
    )
    .await
    .unwrap()
    .unwrap();
    let reopened = snapshot
        .sessions
        .iter()
        .find(|session| session.agent_id == agent && session.id == "chat:results")
        .unwrap();
    assert_eq!(
        serde_json::to_value(&reopened.chat_results[0].payload).unwrap(),
        payload
    );
    let _ = std::fs::remove_file(path);
}
