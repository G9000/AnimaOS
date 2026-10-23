use super::*;

fn with_playbook(mut call: ToolCall, playbook: DataValue) -> ToolCall {
    call.args.insert("playbook".into(), playbook);
    call
}

#[tokio::test]
async fn self_clone_async_playbook_is_durable_and_changed_replay_is_refused() {
    let harness = Harness::new(0).await;
    let call = with_playbook(
        launch("durable-playbook", "Prepare a report"),
        DataValue::String("Inspect sources; return evidence and uncertainties".into()),
    );
    let first = success_json(harness.call(call.clone()).await);
    let retry = success_json(harness.call(call.clone()).await);
    assert_eq!(first, retry);
    let changed = with_playbook(call, DataValue::String("Different playbook".into()));
    let refused = harness.call(changed).await;
    assert_eq!(refused.status, anima_core::TaskStatus::Error);
    assert!(refused
        .error
        .unwrap()
        .contains("different helper launch arguments"));
    let snapshot = harness
        .coordinator
        .state
        .read()
        .await
        .control_plane_snapshot();
    let encoded = serde_json::to_vec(&snapshot).unwrap();
    let mut restored = DaemonState::new();
    restored
        .restore_control_plane_snapshot(serde_json::from_slice(&encoded).unwrap())
        .unwrap();
    let run = restored.runs.get(first["runId"].as_str().unwrap()).unwrap();
    assert!(run.input.text.contains("Prepare a report"));
    assert!(run
        .input
        .text
        .contains("Inspect sources; return evidence and uncertainties"));
    let child = restored.get_agent(&run.agent_id).unwrap().state;
    assert!(child
        .config
        .legacy_personality()
        .contains("bounded task helper"));
    assert_ne!(child.config.system, harness.parent.config.system);
    assert_eq!(harness.context.helper_starts.load(Ordering::Acquire), 1);
}

#[tokio::test]
async fn self_clone_invalid_playbooks_create_no_helper_and_consume_no_start() {
    let harness = Harness::new(0).await;
    for value in [
        DataValue::Bool(true),
        DataValue::String(" ".into()),
        DataValue::String("x".repeat(32768)),
    ] {
        let result = harness
            .call(with_playbook(launch("bad-playbook", "Task"), value))
            .await;
        assert_eq!(result.status, anima_core::TaskStatus::Error, "{result:?}");
        assert_eq!(harness.context.helper_starts.load(Ordering::Acquire), 0);
        assert_eq!(
            harness.coordinator.state.read().await.agent_states().len(),
            1
        );
    }
}

#[tokio::test]
async fn self_clone_async_model_receives_its_playbook_without_parent_personality() {
    let mut harness = Harness::new(1).await;
    harness.parent.config.bio = Some("Main private persona sentinel".into());
    harness.parent.config.system = Some("Main relationship instructions sentinel".into());
    harness
        .coordinator
        .state
        .write()
        .await
        .restore_agent_config(&harness.parent.id, harness.parent.config.clone());
    let handle = success_json(
        harness
            .call(with_playbook(
                launch("focused-worker", "Inspect project evidence"),
                DataValue::String("Worker playbook sentinel: list facts and uncertainties".into()),
            ))
            .await,
    );
    wait_for(
        &harness.coordinator,
        handle["runId"].as_str().unwrap(),
        RunStatus::Completed,
    )
    .await;
    let requests = harness.model.requests();
    assert_eq!(requests.len(), 1);
    assert!(!requests[0].system.contains("Main private persona sentinel"));
    assert!(!requests[0]
        .system
        .contains("Main relationship instructions sentinel"));
    assert!(requests[0].system.contains("bounded task helper"));
    assert!(requests[0]
        .messages
        .iter()
        .any(|message| message.role == MessageRole::User
            && message.content.text.contains("Worker playbook sentinel")));
}
