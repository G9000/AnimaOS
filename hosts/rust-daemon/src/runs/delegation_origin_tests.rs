//! Origin/pending persistence contracts. Existing job attempts and automation
//! claims carry the references; these fixtures create no worker or sender.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::jobs::AgentJobRecord;

fn digest(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

fn pending() -> Value {
    json!({
        "launchRunId": "run_launch",
        "childRunIds": ["run_child"],
        "completionRunIds": ["run_callback"]
    })
}

fn job(status: &str) -> Value {
    json!({
        "id": "job_origin", "agentId": "companion", "title": "Invoice work",
        "prompt": "Inspect the exact assigned evidence", "requestKey": "origin-job",
        "status": status, "revision": 2, "attempt": 1,
        "createdAtMs": 1, "updatedAtMs": 4, "startedAtMs": 2,
        "finishedAtMs": null, "result": null, "error": null,
        "maxAttempts": 3, "requiresApproval": false, "approvedAtMs": null,
        "attempts": [], "goalId": null
    })
}

fn valid_job(value: Value) -> bool {
    serde_json::from_value::<AgentJobRecord>(value).is_ok_and(|record| record.validate().is_ok())
}

fn pending_job() -> Value {
    let mut value = job("pending_delegations");
    value["pendingDelegations"] = pending();
    value
}

fn final_job() -> Value {
    let mut value = job("completed");
    let result = "All required outputs, in original launch order";
    let evidence = json!({
        "generation": 1,
        "sha256": digest(result),
        "outputs": [{
            "runId": "run_callback", "replyMessageId": "reply_callback",
            "outputSha256": digest("full callback output"),
            "preview": "full callback output", "previewTruncated": false
        }]
    });
    value["finishedAtMs"] = json!(4);
    value["result"] = json!(result);
    value["finalOutput"] = evidence.clone();
    value["attempts"] = json!([{
        "attempt": 1, "status": "completed", "startedAtMs": 2,
        "finishedAtMs": 4, "result": result, "error": null,
        "resultTruncated": false, "finalOutput": evidence,
        "review": {
            "decision": "accepted", "note": "Reviewed exact aggregate",
            "reviewedAtMs": 4, "finalOutputGeneration": 1,
            "finalOutputSha256": digest(result)
        }
    }]);
    let provenance = json!({"launchRunId":"run_launch","childRunIds":["run_child"],"completionRunIds":["run_callback"],
        "disposition":"completed","reason":"Exact final source fixture"});
    value["delegationEvidence"] = provenance.clone();
    value["attempts"][0]["delegationEvidence"] = provenance;
    value
}

#[test]
fn fp05_origin_contract_pending_job_round_trip_retains_exact_attempt_references() {
    let value = pending_job();
    let record: AgentJobRecord = serde_json::from_value(value.clone()).unwrap();
    record.validate().unwrap();
    let written = serde_json::to_value(record).unwrap();
    assert_eq!(written["status"], "pending_delegations");
    assert_eq!(written["attempt"], 1);
    assert_eq!(written["pendingDelegations"], value["pendingDelegations"]);
    assert!(written["finishedAtMs"].is_null());
    assert!(written["result"].is_null());
    assert!(written["attempts"].as_array().unwrap().is_empty());
}

#[test]
fn fp05_origin_contract_pending_references_cannot_hide_on_terminal_job() {
    let mut value = final_job();
    value["pendingDelegations"] = pending();
    assert!(
        !valid_job(value),
        "Completed cannot stand for pending useful work"
    );
}

#[test]
fn fp05_origin_contract_pending_job_rejects_final_or_review_evidence() {
    for (field, invalid) in [
        ("finishedAtMs", json!(4)),
        ("result", json!("Accepted helpers is not final output")),
        ("finalOutput", final_job()["finalOutput"].clone()),
        ("attempts", final_job()["attempts"].clone()),
    ] {
        let mut value = pending_job();
        value[field] = invalid;
        assert!(!valid_job(value), "pending attempt must reject {field}");
    }
}

#[test]
fn fp05_origin_contract_pending_job_rejects_missing_duplicate_self_and_oversize_links() {
    for links in [
        Value::Null,
        json!({"launchRunId":"run_launch", "childRunIds":[], "completionRunIds":[]}),
        json!({"launchRunId":"run_launch", "childRunIds":["run_child","run_child"], "completionRunIds":["run_callback"]}),
        json!({"launchRunId":"run_launch", "childRunIds":["run_launch"], "completionRunIds":["run_callback"]}),
        json!({"launchRunId":"run_launch", "childRunIds":["run_child"], "completionRunIds":["run_child"]}),
        json!({"launchRunId":"run_launch", "childRunIds":["a","b","c","d","e"], "completionRunIds":["run_callback"]}),
        json!({"launchRunId":"run_launch", "childRunIds":["run_child"], "completionRunIds":["run_callback"], "recipient":"forged"}),
    ] {
        let mut value = pending_job();
        value["pendingDelegations"] = links;
        assert!(
            !valid_job(value),
            "pending linkage must be bounded and closed"
        );
    }
}

#[test]
fn fp05_origin_contract_final_output_and_exact_review_binding_round_trip() {
    let value = final_job();
    let record: AgentJobRecord = serde_json::from_value(value.clone()).unwrap();
    record.validate().unwrap();
    let written = serde_json::to_value(record).unwrap();
    assert_eq!(written["finalOutput"], value["finalOutput"]);
    assert_eq!(written["attempts"][0]["finalOutput"], value["finalOutput"]);
    assert_eq!(
        written["attempts"][0]["review"],
        value["attempts"][0]["review"]
    );
}

#[test]
fn fp05_origin_contract_async_review_rejects_stale_missing_and_changed_output_bindings() {
    for (field, invalid) in [
        ("finalOutputGeneration", Value::Null),
        ("finalOutputGeneration", json!(0)),
        ("finalOutputGeneration", json!(2)),
        ("finalOutputSha256", Value::Null),
        ("finalOutputSha256", json!("a".repeat(64))),
    ] {
        let mut value = final_job();
        value["attempts"][0]["review"][field] = invalid;
        assert!(
            !valid_job(value),
            "review must bind exact final generation/digest: {field}"
        );
    }
    let mut value = final_job();
    value["result"] = json!("changed after acceptance");
    value["attempts"][0]["result"] = value["result"].clone();
    assert!(
        !valid_job(value),
        "aggregate bytes cannot change under an existing digest/review"
    );
}

#[test]
fn fp05_origin_contract_final_output_rejects_missing_duplicate_and_invalid_evidence() {
    for invalid in [
        json!({"generation":0,"sha256":"a".repeat(64),"outputs":[]}),
        json!({"generation":1,"sha256":"A".repeat(64),"outputs":[]}),
        json!({"generation":1,"sha256":"a".repeat(63),"outputs":[]}),
        json!({"generation":1,"sha256":"a".repeat(64),"outputs":[]}),
    ] {
        let mut value = final_job();
        value["finalOutput"] = invalid.clone();
        value["attempts"][0]["finalOutput"] = invalid;
        assert!(
            !valid_job(value),
            "final evidence has strict nonempty bounded syntax"
        );
    }
    let mut value = final_job();
    let entry = value["finalOutput"]["outputs"][0].clone();
    value["finalOutput"]["outputs"] = json!([entry.clone(), entry]);
    value["attempts"][0]["finalOutput"] = value["finalOutput"].clone();
    assert!(
        !valid_job(value),
        "one callback cannot occupy two aggregate positions"
    );
}

#[test]
fn fp05_origin_contract_legacy_records_keep_absent_async_fields() {
    let record: AgentJobRecord = serde_json::from_value(job("running")).unwrap();
    record.validate().unwrap();
    let value = serde_json::to_value(record).unwrap();
    assert!(value.get("pendingDelegations").is_none());
    assert!(value.get("finalOutput").is_none());
    let schedule = crate::schedules::test_automation("companion", "legacy_schedule");
    crate::schedules::validate_stored_automation(&schedule).unwrap();
    assert!(serde_json::to_value(schedule)
        .unwrap()
        .get("pendingDelegations")
        .is_none());
}

#[test]
fn fp05_origin_runtime_v21_reserves_distinct_origin_backup_without_retargeting_older_backups() {
    use crate::control_plane_store::{pre_upgrade_backup_path, CONTROL_PLANE_STORE_VERSION};
    assert_eq!(CONTROL_PLANE_STORE_VERSION, 21);
    let path = std::path::Path::new("control-plane.json");
    assert_eq!(
        pre_upgrade_backup_path(path, 20),
        std::path::PathBuf::from("control-plane.json.pre-async-origin-batches.bak")
    );
    assert_eq!(
        pre_upgrade_backup_path(path, 19),
        std::path::PathBuf::from("control-plane.json.pre-async-helpers.bak")
    );
    assert_eq!(
        pre_upgrade_backup_path(path, 18),
        std::path::PathBuf::from("control-plane.json.pre-community-bundles.bak")
    );
}

fn backup_files() -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
    let path = std::env::temp_dir().join(format!(
        "anima-origin-upgrade-{}.json",
        uuid::Uuid::new_v4()
    ));
    let name = path.file_name().unwrap().to_string_lossy();
    let old = path.with_file_name(format!("{name}.pre-async-helpers.bak"));
    let next = path.with_file_name(format!("{name}.pre-async-origin-batches.bak"));
    (path, old, next)
}

#[tokio::test]
async fn fp05_origin_runtime_v20_backup_keeps_original_bytes_and_older_backup_bytes() {
    use crate::control_plane_store::{write_pre_upgrade_backup, ControlPlaneStoreConfig};
    let (path, old, next) = backup_files();
    let original = b"{\n  \"version\":20, \"agents\":[], \"swarms\":[]\n}\n";
    let older = b"older-helper-upgrade-evidence";
    std::fs::write(&path, original).unwrap();
    std::fs::write(&old, older).unwrap();
    let result = write_pre_upgrade_backup(&ControlPlaneStoreConfig::Json(path.clone()), 20).await;
    let kept = std::fs::read(&path).unwrap();
    let old_bytes = std::fs::read(&old).unwrap();
    let new_bytes = std::fs::read(&next).ok();
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(&old);
    let _ = std::fs::remove_file(&next);
    assert_eq!(result.unwrap(), next.display().to_string());
    assert_eq!(kept, original);
    assert_eq!(
        old_bytes, older,
        "new version backup must not overwrite the previous helper backup"
    );
    assert_eq!(new_bytes.as_deref(), Some(original.as_slice()));
}

#[tokio::test]
async fn fp05_origin_runtime_failed_v20_backup_preserves_original_and_older_evidence() {
    use crate::control_plane_store::{write_pre_upgrade_backup, ControlPlaneStoreConfig};
    let (path, old, next) = backup_files();
    let original = b"{\"version\":20,\"agents\":[]}";
    let older = b"preserve-prior-helper-upgrade";
    std::fs::write(&path, original).unwrap();
    std::fs::write(&old, older).unwrap();
    std::fs::create_dir(&next).unwrap();
    let result = write_pre_upgrade_backup(&ControlPlaneStoreConfig::Json(path.clone()), 20).await;
    let kept = std::fs::read(&path).unwrap();
    let old_bytes = std::fs::read(&old).unwrap();
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(&old);
    std::fs::remove_dir(&next).unwrap();
    assert!(result.is_err(), "the required new backup path is blocked");
    assert_eq!(kept, original);
    assert_eq!(old_bytes, older);
}

#[test]
fn fp05_origin_runtime_in_memory_restore_never_inherits_publication_witness() {
    use super::{RunLedger, RunRecord, RunSource, RunStart, RunStatus};
    use std::collections::HashSet;
    let mut run = RunRecord::running(
        RunStart {
            agent_id: "owner".into(),
            session_id: "chat:owner".into(),
            source: RunSource::Web,
            source_ref: None,
            idempotency_key: None,
            text: "settled".into(),
            model: "recording".into(),
            provider: None,
            parent_run_id: None,
        },
        1,
    );
    run.finish(RunStatus::Completed, None, 2);
    run.terminal_published = true;
    let id = run.id.clone();
    assert!(serde_json::to_value(&run)
        .unwrap()
        .get("terminalPublished")
        .is_none());
    let agents = HashSet::from(["owner".to_string()]);
    let first = RunLedger::restored(vec![run], &agents, 3);
    assert!(
        !first.get(&id).unwrap().terminal_published,
        "in-memory snapshot clones are also restart boundaries"
    );
    let second = RunLedger::restored(vec![first.get(&id).unwrap().clone()], &agents, 4);
    assert!(!second.get(&id).unwrap().terminal_published);
    assert_eq!(second.get(&id).unwrap().status, RunStatus::Completed);
}
