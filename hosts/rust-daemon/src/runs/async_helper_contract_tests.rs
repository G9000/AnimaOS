//! FP-05 durable contract fixtures. These exercise the persisted wire shape,
//! not a second helper registry or a provider/network-backed runtime.
use std::collections::HashSet;

use serde_json::{json, Value};

use super::{RunLedger, RunRecord, RunSource, RunStart, RunStatus, TERMINAL_RUN_RETENTION_MS};

fn parent() -> RunRecord {
    let mut run = RunRecord::running(
        RunStart {
            agent_id: "companion".into(),
            session_id: "chat:owner".into(),
            source: RunSource::Web,
            source_ref: None,
            idempotency_key: Some("owner-message".into()),
            text: "Research the task".into(),
            model: "recording".into(),
            provider: None,
            parent_run_id: None,
        },
        1,
    );
    run.id = "run_parent".into();
    run.finish(RunStatus::Completed, None, 2);
    run.reply_message_id = Some("reply_parent".into());
    run
}

fn child_json() -> Value {
    let mut value = serde_json::to_value(parent()).unwrap();
    value["id"] = json!("run_child");
    value["agentId"] = json!("helper");
    value["sessionId"] = json!("helper:assignment");
    value["source"] = json!("delegation");
    value["parentRunId"] = json!("run_parent");
    value["idempotencyKey"] = Value::Null;
    value["replyMessageId"] = json!("reply_child");
    value["asyncHelper"] = json!({
        "ownerAgentId": "companion",
        "ownerSessionId": "chat:owner",
        "parentRunId": "run_parent",
        "launchKey": "helper-launch:parent:call",
        "launchFingerprint": "a".repeat(64),
        "effectiveBounds": {
            "maxToolTurns": 8, "maxDurationMs": 120000,
            "maxOutputTokensPerResponse": 4096, "maxAcceptedSteers": 8
        },
        "completion": {"state": "awaiting_parent", "reservedRunId": "run_completion"},
        "unconsumedSteers": []
    });
    value
}

fn decode(value: Value) -> RunRecord {
    serde_json::from_value(value).expect("contract fixture has a valid run record shape")
}

fn invalid(value: Value) -> bool {
    match serde_json::from_value::<RunRecord>(value) {
        Ok(child) => RunLedger::validate(&[parent(), child]).is_err(),
        Err(_) => true,
    }
}

fn steer(index: usize) -> Value {
    json!({
        "idempotencyKey": format!("helper-steer:{index}"),
        "text": format!("Additional evidence {index}"),
        "acceptedAtMs": 10 + index,
        "attachmentIds": []
    })
}

#[test]
fn async_helper_contract_round_trip_preserves_both_typed_origins() {
    let child = child_json();
    assert!(RunLedger::validate(&[parent(), decode(child.clone())]).is_ok());
    let written = serde_json::to_value(decode(child.clone())).unwrap();
    assert_eq!(written["asyncHelper"], child["asyncHelper"]);

    let mut completion = serde_json::to_value(parent()).unwrap();
    completion["id"] = json!("run_completion");
    completion["source"] = json!("delegation");
    completion["parentRunId"] = json!("run_parent");
    completion["helperCompletion"] = json!({
        "helperRunId": "run_child", "launchParentRunId": "run_parent"
    });
    let written = serde_json::to_value(decode(completion.clone())).unwrap();
    assert_eq!(written["helperCompletion"], completion["helperCompletion"]);
}

#[test]
fn async_helper_contract_old_records_do_not_acquire_helper_behavior() {
    let old = serde_json::to_value(parent()).unwrap();
    let written = serde_json::to_value(decode(old.clone())).unwrap();
    assert!(written.get("asyncHelper").is_none());
    assert!(written.get("helperCompletion").is_none());
    assert_eq!(written, old);
}

#[test]
fn async_helper_contract_rejects_duplicate_launch_keys() {
    let first = child_json();
    let mut second = first.clone();
    second["id"] = json!("run_second");
    second["sessionId"] = json!("helper:second");
    second["asyncHelper"]["completion"]["reservedRunId"] = json!("run_second_completion");
    assert!(RunLedger::validate(&[parent(), decode(first), decode(second)]).is_err());
}

#[test]
fn async_helper_contract_rejects_duplicate_completion_reservations() {
    let first = child_json();
    let mut second = first.clone();
    second["id"] = json!("run_second");
    second["sessionId"] = json!("helper:second");
    second["asyncHelper"]["launchKey"] = json!("helper-launch:parent:second-call");
    assert!(RunLedger::validate(&[parent(), decode(first), decode(second)]).is_err());
}

#[test]
fn async_helper_contract_rejects_parent_cycles() {
    let mut launching_parent = parent();
    launching_parent.parent_run_id = Some("run_child".into());
    assert!(RunLedger::validate(&[launching_parent, decode(child_json())]).is_err());
}

#[test]
fn async_helper_contract_rejects_mismatched_owner_parent_and_source() {
    for (path, value) in [
        (vec!["asyncHelper", "ownerAgentId"], json!("foreign")),
        (vec!["asyncHelper", "ownerSessionId"], json!("chat:foreign")),
        (vec!["asyncHelper", "parentRunId"], json!("run_other")),
        (vec!["parentRunId"], json!("run_other")),
        (vec!["source"], json!("web")),
    ] {
        let mut child = child_json();
        let mut cursor = &mut child;
        for key in &path[..path.len() - 1] {
            cursor = &mut cursor[*key];
        }
        cursor[*path.last().unwrap()] = value;
        assert!(invalid(child), "must reject invalid path {path:?}");
    }
}

#[test]
fn async_helper_contract_rejects_invalid_status_state_pairs() {
    for completion in [
        json!({"state": "awaiting_result", "reservedRunId": "run_completion"}),
        json!({"state": "awaiting_parent", "reservedRunId": "run_child"}),
        json!({"state": "queued", "runId": "run_missing"}),
    ] {
        let mut child = child_json();
        child["asyncHelper"]["completion"] = completion.clone();
        assert!(invalid(child), "must reject {completion}");
    }
    let mut child = child_json();
    child["status"] = json!("failed");
    assert!(invalid(child));
}

#[test]
fn async_helper_contract_rejects_oversize_fields_and_bad_fingerprint() {
    for (field, value) in [
        ("ownerAgentId", " ".to_string()),
        ("ownerSessionId", "s".repeat(201)),
        ("launchKey", "k".repeat(4097)),
        ("launchFingerprint", "A".repeat(64)),
        ("launchFingerprint", "a".repeat(63)),
    ] {
        let mut child = child_json();
        child["asyncHelper"][field] = json!(value);
        assert!(invalid(child), "must reject invalid {field}");
    }
    let mut child = child_json();
    child["asyncHelper"]["completion"] =
        json!({"state": "review_required", "reason": "r".repeat(1025)});
    assert!(invalid(child));
}

#[test]
fn async_helper_contract_rejects_excess_unconsumed_steers() {
    let mut child = child_json();
    child["asyncHelper"]["unconsumedSteers"] = json!((0..9).map(steer).collect::<Vec<_>>());
    assert!(invalid(child));
}

#[test]
fn async_helper_contract_rejects_completion_with_foreign_provenance() {
    let child = decode(child_json());
    let mut completion = serde_json::to_value(parent()).unwrap();
    completion["id"] = json!("run_completion");
    completion["source"] = json!("delegation");
    completion["parentRunId"] = json!("run_parent");
    completion["helperCompletion"] =
        json!({"helperRunId": "run_child", "launchParentRunId": "run_other"});
    assert!(RunLedger::validate(&[parent(), child, decode(completion)]).is_err());
}

#[test]
fn async_helper_contract_restart_archives_steers_without_new_web_runs() {
    let mut child = child_json();
    child["status"] = json!("running");
    child["finishedAtMs"] = Value::Null;
    child["replyMessageId"] = Value::Null;
    child["asyncHelper"]["completion"]["state"] = json!("awaiting_result");
    child["pendingSteers"] = json!([steer(0)]);
    let live = HashSet::from(["companion".into(), "helper".into()]);
    let first = RunLedger::restored(vec![parent(), decode(child)], &live, 100);
    let records = first.snapshot_records(&live);
    assert_eq!(
        records.len(),
        2,
        "helper steering must never create an owner-like run"
    );
    let child = records.iter().find(|run| run.id == "run_child").unwrap();
    assert_eq!(child.status, RunStatus::Interrupted);
    assert!(child.pending_steers.is_empty());
    let value = serde_json::to_value(child).unwrap();
    assert_eq!(value["asyncHelper"]["unconsumedSteers"], json!([steer(0)]));
    assert_eq!(
        value["asyncHelper"]["completion"]["state"],
        "review_required"
    );
    let second = RunLedger::restored(records.clone(), &live, 200).snapshot_records(&live);
    assert_eq!(
        second, records,
        "a second fresh restore must preserve exact IDs and disposition"
    );
}

#[test]
fn async_helper_contract_retention_pins_unresolved_relationships() {
    let mut parent = parent();
    parent.mirrored = true;
    let mut child = decode(child_json());
    child.mirrored = true;
    let mut ledger = RunLedger::default();
    ledger.insert(parent);
    ledger.insert(child);
    ledger.prune(TERMINAL_RUN_RETENTION_MS + 100);
    assert!(
        ledger.get("run_parent").is_some(),
        "the launch parent must remain while completion is unresolved"
    );
    assert!(
        ledger.get("run_child").is_some(),
        "the child result must remain while completion is unresolved"
    );
}

#[test]
fn async_helper_contract_restart_review_is_mirrored_before_retention() {
    let mut child = decode(child_json());
    child.mirrored = true;
    let live = HashSet::from(["companion".into(), "helper".into()]);
    let restored = RunLedger::restored(
        vec![parent(), child],
        &live,
        TERMINAL_RUN_RETENTION_MS + 100,
    );
    let child = restored
        .get("run_child")
        .expect("new review disposition must remain until history has the changed record");
    assert!(!child.mirrored);
    assert_eq!(
        serde_json::to_value(child).unwrap()["asyncHelper"]["completion"]["state"],
        "review_required"
    );
}

#[test]
fn async_helper_contract_present_ordinary_source_cannot_impersonate_pruned_helper() {
    let source = parent();
    let mut completion = source.clone();
    completion.id = "run_completion".into();
    completion.source = RunSource::Delegation;
    completion.parent_run_id = Some("run_launch_parent".into());
    completion.mirrored = true;
    completion.helper_completion = Some(super::HelperCompletion {
        helper_run_id: source.id.clone(),
        launch_parent_run_id: "run_launch_parent".into(),
    });
    assert!(RunLedger::validate(&[source, completion]).is_err());
}

#[test]
fn async_helper_contract_manual_result_is_pinned_until_launching_parent_settles() {
    let mut parent = parent();
    parent.status = RunStatus::Running;
    parent.finished_at_ms = None;
    parent.reply_message_id = None;
    let mut value = child_json();
    value["asyncHelper"]["completion"] = json!({"state": "manual"});
    let mut child = decode(value);
    child.mirrored = true;
    let mut ledger = RunLedger::default();
    ledger.insert(parent);
    ledger.insert(child);
    ledger.prune(TERMINAL_RUN_RETENTION_MS + 100);
    assert!(
        ledger.get("run_child").is_some(),
        "a live parent's same-call retry must retain the immutable handle"
    );
}

#[test]
fn async_helper_contract_effective_bounds_reject_widening_and_missing_limits() {
    for (field, value) in [
        ("maxToolTurns", json!(9)),
        ("maxDurationMs", json!(120001)),
        ("maxOutputTokensPerResponse", json!(4097)),
        ("maxAcceptedSteers", json!(9)),
    ] {
        let mut child = child_json();
        child["asyncHelper"]["effectiveBounds"][field] = value;
        assert!(invalid(child), "must reject widened {field}");
    }
    let mut child = child_json();
    child["asyncHelper"]
        .as_object_mut()
        .unwrap()
        .remove("effectiveBounds");
    assert!(invalid(child));
}

#[test]
fn async_helper_contract_steer_keys_are_disjoint_and_lifetime_bounded() {
    let mut child = child_json();
    child["asyncHelper"]["unconsumedSteers"] = json!([steer(0)]);
    child["pendingSteers"] = json!([steer(0)]);
    assert!(invalid(child));
    let mut child = child_json();
    child["asyncHelper"]["unconsumedSteers"] = json!((0..8).map(steer).collect::<Vec<_>>());
    child["steeredKeys"] = json!([{
        "idempotencyKey": "consumed-extra", "acceptedAtMs": 1,
        "input": {"text": "Already consumed", "attachmentIds": []}
    }]);
    assert!(invalid(child));
}

fn closed_source_with_live_completion(status: RunStatus) -> Vec<RunRecord> {
    let mut launch_parent = parent();
    launch_parent.mirrored = true;
    let mut value = child_json();
    value["asyncHelper"]["completion"] = json!({
        "state": "suppressed", "reason": "Stopped through helper handle", "runId": "run_completion"
    });
    let mut child = decode(value);
    child.mirrored = true;
    let mut completion = parent();
    completion.id = "run_completion".into();
    completion.source = RunSource::Delegation;
    completion.parent_run_id = Some(launch_parent.id.clone());
    completion.helper_completion = Some(super::HelperCompletion {
        helper_run_id: child.id.clone(),
        launch_parent_run_id: launch_parent.id.clone(),
    });
    completion.status = status;
    completion.mirrored = false;
    if !status.is_terminal() {
        completion.finished_at_ms = None;
        completion.reply_message_id = None;
    }
    vec![launch_parent, child, completion]
}

#[test]
fn async_helper_contract_retention_age_pins_source_until_linked_completion_is_mirrored_terminal() {
    for status in [RunStatus::Running, RunStatus::Completed] {
        let records = closed_source_with_live_completion(status);
        RunLedger::validate(&records).unwrap();
        let mut ledger = RunLedger::default();
        for record in records {
            ledger.insert(record);
        }
        ledger.prune(TERMINAL_RUN_RETENTION_MS + 100);
        assert!(
            ledger.get("run_child").is_some(),
            "{status:?} callback retains exact source even after child suppression"
        );
        assert!(ledger.get("run_parent").is_some());
        RunLedger::validate(
            &ledger.snapshot_records(&HashSet::from(["companion".into(), "helper".into()])),
        )
        .unwrap();
        let completion = ledger.get_mut("run_completion").unwrap();
        completion.finish(RunStatus::Completed, None, 3);
        completion.mirrored = true;
        ledger.prune(TERMINAL_RUN_RETENTION_MS + 100);
        assert!(
            ledger.get("run_child").is_none(),
            "settled mirrored evidence follows ordinary retention"
        );
    }
}

#[test]
fn async_helper_contract_retention_count_pins_source_until_linked_completion_is_mirrored_terminal()
{
    for status in [RunStatus::Running, RunStatus::Completed] {
        let mut ledger = RunLedger::default();
        for record in closed_source_with_live_completion(status) {
            ledger.insert(record);
        }
        for index in 0..super::MAX_TERMINAL_RUNS_PER_AGENT + 1 {
            let mut newer = parent();
            newer.id = format!("run_newer_{index}");
            newer.agent_id = "helper".into();
            newer.created_at_ms = 100 + index as u64;
            newer.finished_at_ms = Some(101 + index as u64);
            newer.mirrored = true;
            ledger.insert(newer);
        }
        ledger.prune(1000);
        assert!(
            ledger.get("run_child").is_some(),
            "{status:?} callback must not lose the source to the per-agent count cap"
        );
        let completion = ledger.get_mut("run_completion").unwrap();
        completion.finish(RunStatus::Completed, None, 3);
        completion.mirrored = true;
        ledger.prune(1000);
        assert!(ledger.get("run_child").is_none());
    }
}
