//! Process-level continuity checks use a private fixture profile and file effect.
use super::turn_crash_tests::{marker, profile, OwnedDaemon};
use serde_json::{json, Value};
use std::{
    fs,
    time::{Duration, Instant},
};

async fn wait_workflow(daemon: &OwnedDaemon, path: &str, status: &str) -> Value {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let workflow = daemon.api("GET", path, Value::Null, None).await;
        if workflow["status"] == status {
            return workflow;
        }
        assert!(
            Instant::now() < deadline,
            "workflow did not reach {status}: {workflow}"
        );
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
}

#[tokio::test]
async fn continuity_process_kill_preserves_hold_and_never_replays_a_verified_effect() {
    let profile = profile();
    let root = profile.path();
    let mut first = OwnedDaemon::start(root, "after-effect").await;
    let (agent, _) = first.setup(root).await;
    let collection = format!("/api/agents/{agent}/workflows");
    let created = first.api("POST", &collection, json!({
        "title": "Recover a bounded task", "requestKey": "crash-continuity",
        "steps": [
            {"id":"checkpoint", "title":"Review scope", "kind":"approval", "prompt":"Proceed?"},
            {"id":"write", "title":"Write once", "kind":"task", "prompt":"CRASH_WRITE"}
        ]
    }), None).await;
    let path = format!("{collection}/{}", created["id"].as_str().unwrap());
    let resume = format!("{path}/resume");
    let held = first
        .api(
            "POST",
            &resume,
            json!({"revision":created["revision"],"action":"pause"}),
            None,
        )
        .await;
    first.kill();

    let mut second = OwnedDaemon::start(root, "after-effect").await;
    let restored = second.api("GET", &path, Value::Null, None).await;
    assert_eq!(restored["paused"], true);
    assert_eq!(restored["status"], "awaiting_approval");
    assert_eq!(restored["revision"], held["revision"]);
    let continued = second
        .api(
            "POST",
            &resume,
            json!({"revision":restored["revision"],"action":"continue"}),
            None,
        )
        .await;
    assert_eq!(continued["status"], "awaiting_approval");
    second
        .api(
            "POST",
            &resume,
            json!({"revision":continued["revision"],"action":"approve"}),
            None,
        )
        .await;

    let deadline = Instant::now() + Duration::from_secs(30);
    let approval = loop {
        let view = second
            .api(
                "GET",
                &format!("/api/approvals?agentId={agent}"),
                Value::Null,
                None,
            )
            .await;
        if let Some(approval) = view["approvals"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["status"] == "pending")
            .cloned()
        {
            break approval;
        }
        assert!(Instant::now() < deadline, "tool approval did not appear");
        tokio::time::sleep(Duration::from_millis(40)).await;
    };
    second
        .api(
            "POST",
            &format!(
                "/api/approvals/{}/decision",
                approval["id"].as_str().unwrap()
            ),
            json!({"decision":"allow_once","revision":approval["revision"]}),
            None,
        )
        .await;
    marker(root, "effect-settled").await;
    assert_eq!(
        fs::read(root.join("workspace/effect.txt")).unwrap(),
        b"exact-approved-effect"
    );
    let running = second.api("GET", &path, Value::Null, None).await;
    let held = second
        .api(
            "POST",
            &resume,
            json!({"revision":running["revision"],"action":"pause"}),
            None,
        )
        .await;
    second.kill();
    let calls = fs::read_to_string(root.join("calls.jsonl"))
        .unwrap()
        .lines()
        .count();

    let third = OwnedDaemon::start(root, "run").await;
    let reviewed = wait_workflow(&third, &path, "needs_review").await;
    assert_eq!(reviewed["paused"], true);
    assert_eq!(reviewed["steps"][0]["status"], "completed");
    assert!(reviewed["revision"].as_u64().unwrap() >= held["revision"].as_u64().unwrap());
    assert_eq!(
        fs::read_to_string(root.join("calls.jsonl"))
            .unwrap()
            .lines()
            .count(),
        calls
    );
    let continued = third
        .api(
            "POST",
            &resume,
            json!({"revision":reviewed["revision"],"action":"continue"}),
            None,
        )
        .await;
    let finished = third.api("POST", &resume, json!({"revision":continued["revision"],"action":"resolve", "value":"Verified file effect", "acknowledgeUncertain":true}), None).await;
    assert_eq!(finished["status"], "completed");
    assert_eq!(
        fs::read_to_string(root.join("calls.jsonl"))
            .unwrap()
            .lines()
            .count(),
        calls
    );
    assert_eq!(
        fs::read(root.join("workspace/effect.txt")).unwrap(),
        b"exact-approved-effect"
    );
}
