//! Included beneath `async_helper_tests` to reuse its authenticated native fixtures.
use super::{drained, foreign_actor, input, launch, success_json, Harness};
use crate::agent_runs::test_support::{wait_for, Gate, ScriptedModel, Step};
use crate::runs::RunStatus;
use crate::tools::ToolRegistry;
use anima_core::{DataValue, TaskStatus, ToolCall};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::atomic::Ordering;
use std::time::Duration;

async fn grant_collection(harness: &mut Harness) {
    let registry = ToolRegistry::new();
    harness.parent.config.tools.as_mut().unwrap().extend(
        ["spawn_helpers", "wait_helper_runs"]
            .map(|name| registry.descriptor(name).expect("native collection tool")),
    );
    harness
        .coordinator
        .state
        .write()
        .await
        .restore_agent_config(&harness.parent.id, harness.parent.config.clone());
}

fn batch(id: &str, tasks: &[&str]) -> ToolCall {
    ToolCall {
        id: id.into(),
        name: "spawn_helpers".into(),
        args: BTreeMap::from([(
            "tasks".into(),
            DataValue::Array(
                tasks
                    .iter()
                    .map(|task| {
                        let mut args = launch("unused", task).args;
                        args.remove("completion");
                        DataValue::Object(args)
                    })
                    .collect(),
            ),
        )]),
    }
}

fn collect(ids: &[&str], mode: &str, timeout_ms: u64) -> ToolCall {
    ToolCall {
        id: "collect-native".into(),
        name: "wait_helper_runs".into(),
        args: BTreeMap::from([
            (
                "run_ids".into(),
                DataValue::Array(
                    ids.iter()
                        .map(|id| DataValue::String((*id).into()))
                        .collect(),
                ),
            ),
            ("mode".into(), DataValue::String(mode.into())),
            ("timeout_ms".into(), DataValue::Number(timeout_ms as f64)),
        ]),
    }
}

#[tokio::test]
async fn helper_batch_prevalidates_every_item_before_native_admission() {
    let mut harness = Harness::new(0).await;
    grant_collection(&mut harness).await;
    let mut call = batch("invalid-batch", &["First", "Second"]);
    let DataValue::Array(tasks) = call.args.get_mut("tasks").unwrap() else {
        unreachable!()
    };
    let DataValue::Object(second) = &mut tasks[1] else {
        unreachable!()
    };
    second.remove("name");
    let result = harness.call(call).await;
    assert_eq!(result.status, TaskStatus::Error);
    assert_eq!(harness.context.helper_starts.load(Ordering::Acquire), 0);
    assert!(harness
        .coordinator
        .state
        .read()
        .await
        .runs
        .async_helpers_for_owner(&harness.parent.id)
        .is_empty());
    harness.coordinator.request_shutdown();
}

#[tokio::test]
async fn helper_batch_returns_partial_receipts_and_item_retries_are_stable() {
    let mut harness = Harness::new(0).await;
    grant_collection(&mut harness).await;
    let call = batch("partial-batch", &["One", "Two", "Three", "Four", "Five"]);
    let first = success_json(harness.call(call.clone()).await);
    assert_eq!(first["atomic"], false);
    assert_eq!(first["acceptedCount"], 4);
    assert_eq!(first["refusedCount"], 1);
    assert_eq!(
        first["entries"][0]["handle"]["completion"]["state"],
        "manual"
    );
    assert!(first["entries"][4]["error"]
        .as_str()
        .unwrap()
        .contains("configured helper start limit"));
    let replay = success_json(harness.call(call).await);
    for index in 0..4 {
        assert_eq!(
            replay["entries"][index]["handle"]["runId"],
            first["entries"][index]["handle"]["runId"]
        );
    }
    let changed = success_json(
        harness
            .call(batch(
                "partial-batch",
                &["Changed", "Two", "Three", "Four", "Five"],
            ))
            .await,
    );
    assert_eq!(changed["entries"][0]["outcome"], "refused");
    assert!(changed["entries"][0]["error"]
        .as_str()
        .unwrap()
        .contains("different"));
    assert_eq!(
        changed["entries"][1]["handle"]["runId"],
        first["entries"][1]["handle"]["runId"]
    );
    assert_eq!(harness.context.helper_starts.load(Ordering::Acquire), 4);
    harness.coordinator.request_shutdown();
}

#[tokio::test]
async fn helper_collection_any_and_all_distinguish_saved_results_from_pending_handles() {
    let mut harness = Harness::new(1).await;
    grant_collection(&mut harness).await;
    let first = harness.launch("collect-complete", "First").await;
    let first_id = first["runId"].as_str().unwrap();
    wait_for(&harness.coordinator, first_id, RunStatus::Completed).await;
    drained(&harness, &first).await;
    let held = harness.permits.clone().acquire_owned().await.unwrap();
    let second = harness.launch("collect-pending", "Second").await;
    let second_id = second["runId"].as_str().unwrap();
    let any = success_json(
        harness
            .call(collect(&[first_id, second_id], "any", 0))
            .await,
    );
    assert_eq!(any["conditionMet"], true);
    assert_eq!(any["timedOut"], false);
    assert_eq!(any["readyRunIds"], serde_json::json!([first_id]));
    assert_eq!(any["pendingRunIds"], serde_json::json!([second_id]));
    assert_eq!(any["runs"][0]["resultText"], "recorded child result");
    assert!(any["runs"][1]["resultText"].is_null());
    let all = success_json(
        harness
            .call(collect(&[first_id, second_id], "all", 0))
            .await,
    );
    assert_eq!(all["conditionMet"], false);
    assert_eq!(all["timedOut"], true);
    harness.coordinator.request_shutdown();
    drop(held);
}

#[tokio::test]
async fn helper_collection_wait_wakes_for_committed_native_completion() {
    let gate = Gate::new();
    let model = ScriptedModel::gated(vec![Step::Text(vec!["Committed evidence"])], gate.clone());
    let mut harness = Harness::with_model(1, model).await;
    grant_collection(&mut harness).await;
    let child = harness
        .launch("event-completion", "Wait for evidence")
        .await;
    let id = child["runId"].as_str().unwrap().to_string();
    gate.entered().await;
    let coordinator = harness.coordinator.clone();
    let owner = harness.parent.id.clone();
    let parent = harness.context.run_link.clone().unwrap();
    let waiting_id = id.clone();
    let waiting = tokio::spawn(async move {
        coordinator
            .collect_async_helpers(owner, parent, collect(&[&waiting_id], "all", 2_000), None)
            .await
    });
    tokio::task::yield_now().await;
    gate.release();
    let value: Value = serde_json::from_str(
        &tokio::time::timeout(Duration::from_secs(3), waiting)
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(value["conditionMet"], true);
    assert_eq!(value["runs"][0]["resultText"], "Committed evidence");
    assert_eq!(value["readyRunIds"], serde_json::json!([id]));
}

#[tokio::test]
async fn helper_collection_terminal_failure_is_ready_without_inventing_output() {
    let model = ScriptedModel::new(vec![Step::Fail("deliberate failure"); 4]);
    let mut harness = Harness::with_model(1, model).await;
    grant_collection(&mut harness).await;
    let child = harness
        .launch("failed-collection", "Fail bounded task")
        .await;
    let id = child["runId"].as_str().unwrap();
    wait_for(&harness.coordinator, id, RunStatus::Failed).await;
    drained(&harness, &child).await;
    let value = success_json(harness.call(collect(&[id], "all", 0)).await);
    assert_eq!(value["conditionMet"], true);
    assert_eq!(value["runs"][0]["status"], "failed");
    assert!(value["runs"][0]["resultText"].is_null());
    assert!(value["runs"][0]["error"]["message"]
        .as_str()
        .unwrap()
        .contains("deliberate failure"));
}

#[tokio::test]
async fn helper_collection_foreign_and_unknown_handles_are_indistinguishable() {
    let mut harness = Harness::new(0).await;
    grant_collection(&mut harness).await;
    let child = harness
        .launch("private-collection", "Private bounded task")
        .await;
    let (foreign, context) = foreign_actor(&harness).await;
    let mut errors = Vec::new();
    for id in [child["runId"].as_str().unwrap(), "run_unknown"] {
        let result = context
            .clone()
            .execute_tool(
                foreign.clone(),
                input(&foreign.id),
                collect(&[id], "all", 0),
            )
            .await;
        assert_eq!(result.status, TaskStatus::Error);
        errors.push(result.error.unwrap());
    }
    assert_eq!(errors[0], errors[1]);
    harness.coordinator.request_shutdown();
}

#[tokio::test]
async fn helper_collection_cancels_with_the_original_parent_without_holding_transactions() {
    let mut harness = Harness::new(0).await;
    grant_collection(&mut harness).await;
    let child = harness
        .launch("cancel-collection", "Queued bounded task")
        .await;
    let coordinator = harness.coordinator.clone();
    let owner = harness.parent.id.clone();
    let parent = harness.context.run_link.clone().unwrap();
    let cancel = harness
        .coordinator
        .state
        .read()
        .await
        .live
        .runs()
        .control(&parent.run_id)
        .unwrap()
        .cancel;
    let id = child["runId"].as_str().unwrap().to_string();
    let waiting = tokio::spawn(async move {
        coordinator
            .collect_async_helpers(owner, parent, collect(&[&id], "all", 30_000), None)
            .await
    });
    tokio::task::yield_now().await;
    let transaction = tokio::time::timeout(
        Duration::from_millis(250),
        harness.coordinator.control_plane_transaction(),
    )
    .await
    .expect("collector must release the control plane while waiting");
    drop(transaction);
    cancel.cancel();
    let error = tokio::time::timeout(Duration::from_millis(250), waiting)
        .await
        .expect("parent cancellation wakes collection")
        .unwrap()
        .unwrap_err();
    assert!(error.to_lowercase().contains("stop"));
    harness.coordinator.request_shutdown();
}

#[tokio::test]
async fn helper_collection_bounds_escaped_results_without_losing_handles() {
    let text: &'static str = Box::leak("\u{0001}".repeat(10_000).into_boxed_str());
    let mut harness =
        Harness::with_model(4, ScriptedModel::new(vec![Step::Text(vec![text]); 4])).await;
    grant_collection(&mut harness).await;
    let mut ids = Vec::new();
    for index in 0..4 {
        let child = harness
            .launch(&format!("bounded-collection-{index}"), "Bounded output")
            .await;
        let id = child["runId"].as_str().unwrap().to_string();
        wait_for(&harness.coordinator, &id, RunStatus::Completed).await;
        drained(&harness, &child).await;
        ids.push(id);
    }
    let refs: Vec<_> = ids.iter().map(String::as_str).collect();
    let result = harness.call(collect(&refs, "all", 0)).await;
    assert_eq!(result.status, TaskStatus::Success);
    let text = result.data.unwrap().text;
    assert!(text.len() <= 32 * 1024);
    let value: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["readyRunIds"], serde_json::json!(ids));
    assert_eq!(value["truncated"], true);
    assert_eq!(value["runs"].as_array().unwrap().len(), 4);
    assert!(value["runs"]
        .as_array()
        .unwrap()
        .iter()
        .all(|run| run["resultTruncated"] == true));
}

#[tokio::test]
async fn helper_collection_cancellation_interrupts_a_contended_authority_lock() {
    let mut harness = Harness::new(0).await;
    grant_collection(&mut harness).await;
    let child = harness.launch("cancel-under-writer", "Pending task").await;
    let coordinator = harness.coordinator.clone();
    let owner = harness.parent.id.clone();
    let parent = harness.context.run_link.clone().unwrap();
    let cancel = coordinator
        .state
        .read()
        .await
        .live
        .runs()
        .control(&parent.run_id)
        .unwrap()
        .cancel;
    let id = child["runId"].as_str().unwrap();
    let waiting = coordinator.collect_async_helpers(
        owner.clone(),
        parent.clone(),
        collect(&[id], "all", 30_000),
        Some(cancel.clone()),
    );
    tokio::pin!(waiting);
    assert!(
        futures::poll!(waiting.as_mut()).is_pending(),
        "collector must enter its event wait"
    );
    let writer = coordinator.state.write().await;
    let event = crate::live::run_status_event(writer.runs.get(&parent.run_id).unwrap());
    writer.live.publish(event, Some(&owner));
    assert!(
        futures::poll!(waiting.as_mut()).is_pending(),
        "collector must attempt to recheck authority behind the writer"
    );
    cancel.cancel();
    let error = tokio::time::timeout(Duration::from_millis(250), waiting.as_mut())
        .await
        .expect("parent cancellation must interrupt a contended state lock")
        .unwrap_err();
    assert!(error.to_lowercase().contains("stop"));
    drop(writer);
    harness.coordinator.request_shutdown();
}

#[tokio::test]
async fn helper_collection_deadline_bounds_a_contended_authority_lock() {
    let mut harness = Harness::new(0).await;
    grant_collection(&mut harness).await;
    let child = harness.launch("timeout-under-writer", "Pending task").await;
    let coordinator = harness.coordinator.clone();
    let owner = harness.parent.id.clone();
    let parent = harness.context.run_link.clone().unwrap();
    let id = child["runId"].as_str().unwrap();
    let waiting = coordinator.collect_async_helpers(
        owner.clone(),
        parent.clone(),
        collect(&[id], "all", 50),
        None,
    );
    tokio::pin!(waiting);
    assert!(futures::poll!(waiting.as_mut()).is_pending());
    let writer = coordinator.state.write().await;
    let event = crate::live::run_status_event(writer.runs.get(&parent.run_id).unwrap());
    writer.live.publish(event, Some(&owner));
    assert!(futures::poll!(waiting.as_mut()).is_pending());
    let error = tokio::time::timeout(Duration::from_millis(500), waiting.as_mut())
        .await
        .expect("collection deadline must bound a contended state lock")
        .unwrap_err();
    assert!(error.contains("state is unavailable"));
    drop(writer);
    harness.coordinator.request_shutdown();
}

#[tokio::test]
async fn helper_collection_caller_cancel_interrupts_the_initial_state_lock() {
    let mut harness = Harness::new(0).await;
    grant_collection(&mut harness).await;
    let child = harness.launch("cancel-before-state", "Pending task").await;
    let coordinator = harness.coordinator.clone();
    let owner = harness.parent.id.clone();
    let parent = harness.context.run_link.clone().unwrap();
    let cancel = coordinator
        .state
        .read()
        .await
        .live
        .runs()
        .control(&parent.run_id)
        .unwrap()
        .cancel;
    let writer = coordinator.state.write().await;
    let waiting = coordinator.collect_async_helpers(
        owner,
        parent,
        collect(&[child["runId"].as_str().unwrap()], "all", 30_000),
        Some(cancel.clone()),
    );
    tokio::pin!(waiting);
    assert!(futures::poll!(waiting.as_mut()).is_pending());
    cancel.cancel();
    let error = tokio::time::timeout(Duration::from_millis(250), waiting.as_mut())
        .await
        .expect("known caller cancellation must interrupt the initial state lock")
        .unwrap_err();
    assert!(error.to_lowercase().contains("stop"));
    drop(writer);
    harness.coordinator.request_shutdown();
}

#[path = "helper_batch_authority_tests.rs"]
mod batch_authority;
