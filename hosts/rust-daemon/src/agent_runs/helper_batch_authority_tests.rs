use super::*;

#[tokio::test]
async fn helper_batch_requires_the_launching_runs_captured_async_grant() {
    let mut h = Harness::new(0).await;
    grant_collection(&mut h).await;
    // Canonical owner config has both grants, but this earlier run never had
    // permission for individual background launches.
    h.parent
        .config
        .tools
        .as_mut()
        .unwrap()
        .retain(|tool| tool.name != "spawn_helper_async");
    let result = success_json(h.call(batch("captured-grant", &["Inspect evidence"])).await);
    assert_eq!(result["acceptedCount"], 0);
    assert_eq!(result["entries"][0]["outcome"], "refused");
    assert!(result["entries"][0]["error"]
        .as_str()
        .unwrap()
        .contains("captured"));
    assert_eq!(h.context.helper_starts.load(Ordering::Acquire), 0);
    assert!(h
        .coordinator
        .state
        .read()
        .await
        .runs
        .async_helpers_for_owner(&h.parent.id)
        .is_empty());
    h.coordinator.request_shutdown();
}

#[tokio::test]
async fn helper_batch_requires_the_current_batch_grant_before_admission() {
    let mut h = Harness::new(0).await;
    grant_collection(&mut h).await;
    let mut current = h.parent.config.clone();
    current
        .tools
        .as_mut()
        .unwrap()
        .retain(|tool| tool.name != "spawn_helpers");
    h.coordinator
        .state
        .write()
        .await
        .restore_agent_config(&h.parent.id, current);
    let result = h
        .call(batch("revoked-batch-grant", &["Inspect evidence"]))
        .await;
    if let Some(data) = result.data {
        let result: Value = serde_json::from_str(&data.text).unwrap();
        assert_eq!(result["acceptedCount"], 0);
    } else {
        assert_eq!(result.status, TaskStatus::Error);
    }
    assert_eq!(h.context.helper_starts.load(Ordering::Acquire), 0);
    assert!(h
        .coordinator
        .state
        .read()
        .await
        .runs
        .async_helpers_for_owner(&h.parent.id)
        .is_empty());
    h.coordinator.request_shutdown();
}
