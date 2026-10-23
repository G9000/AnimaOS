//! Bounded production-path sequences. The oracle follows owner commands, not ledger transitions.
use super::turn_crash_tests::{marker, profile, OwnedDaemon};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

const SEEDS: [u64; 8] = [1, 7, 19, 41, 97, 313, 20261023, 0x5eedcafe];
const MAX_ACTIONS: usize = 32;

#[derive(Clone, Copy, Debug)]
enum Decision {
    Allow,
    Deny,
    Stop,
}

struct Oracle {
    seed: u64,
    actions: Vec<String>,
    effects: BTreeSet<String>,
    dispatches: BTreeMap<String, usize>,
    terminal: BTreeMap<String, String>,
    restarts: usize,
}
impl Oracle {
    fn new(seed: u64) -> Self {
        Self {
            seed,
            actions: vec![],
            effects: BTreeSet::new(),
            dispatches: BTreeMap::new(),
            terminal: BTreeMap::new(),
            restarts: 0,
        }
    }
    fn action(&mut self, description: String) {
        self.actions.push(description);
        assert!(
            self.actions.len() <= MAX_ACTIONS,
            "seed={} actions={:?}",
            self.seed,
            self.actions
        );
        eprintln!(
            "HARNESS_RELIABILITY seed={} action={} {}",
            self.seed,
            self.actions.len(),
            self.actions.last().unwrap()
        );
    }
    fn evidence(&self) -> Value {
        json!({"seed":self.seed,"actions":self.actions,"restarts":self.restarts,"expectedEffects":self.effects,"expectedDispatches":self.dispatches,"expectedTerminalRuns":self.terminal})
    }
    fn observe(&self, root: &Path) -> Result<(), String> {
        let actual = fs::read_dir(root.join("workspace"))
            .map_err(|e| e.to_string())?
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let name = entry.file_name().into_string().ok()?;
                name.strip_prefix("effect-")
                    .and_then(|s| s.strip_suffix(".txt"))
                    .map(str::to_owned)
            })
            .collect::<BTreeSet<_>>();
        if actual != self.effects {
            return Err(format!(
                "effect IDs differ: actual={actual:?} expected={:?}",
                self.effects
            ));
        }
        for id in &actual {
            if fs::read_to_string(root.join("workspace").join(format!("effect-{id}.txt")))
                .map_err(|e| e.to_string())?
                != format!("effect:{id}")
            {
                return Err(format!("effect payload differs for {id}"));
            }
        }
        let mut calls = BTreeMap::<String, usize>::new();
        for line in fs::read_to_string(root.join("calls.jsonl"))
            .unwrap_or_default()
            .lines()
        {
            let record: Value = serde_json::from_str(line).map_err(|e| e.to_string())?;
            if let Some(id) = record["effectId"].as_str() {
                *calls.entry(id.into()).or_default() += 1;
            }
        }
        if calls != self.dispatches {
            return Err(format!(
                "physical fixture dispatches differ: actual={calls:?} expected={:?}",
                self.dispatches
            ));
        }
        Ok(())
    }
    async fn check(&self, root: &Path, daemon: &OwnedDaemon, agent: &str) {
        self.observe(root)
            .unwrap_or_else(|error| panic!("{error}; {}", self.evidence()));
        let mut succeeded = BTreeSet::new();
        for (run_id, status) in &self.terminal {
            let run = daemon.run(agent, run_id).await;
            assert_eq!(
                run["status"],
                status.as_str(),
                "terminal run resurrected: {}",
                self.evidence()
            );
            for receipt in run["actionReceipts"]
                .as_array()
                .expect("public native action receipts")
            {
                if receipt["name"] == "write_file" && receipt["status"] == "succeeded" {
                    let id = receipt["toolCallId"]
                        .as_str()
                        .unwrap()
                        .strip_prefix("effect-")
                        .expect("fixture effect identity");
                    assert!(
                        succeeded.insert(id.to_owned()),
                        "duplicate successful receipt: {}",
                        self.evidence()
                    );
                }
            }
        }
        assert_eq!(
            succeeded,
            self.effects,
            "public native receipts differ from allowed effects: {}",
            self.evidence()
        );
    }
}
impl Drop for Oracle {
    fn drop(&mut self) {
        let evidence = self.evidence();
        if std::thread::panicking() {
            eprintln!("HARNESS_RELIABILITY_FAILED {evidence}");
        }
        if let Some(path) = std::env::var_os("ANIMA_HARNESS_RELIABILITY_OUTPUT_DIR") {
            let path = std::path::PathBuf::from(path);
            let _ = fs::create_dir_all(&path);
            let _ = fs::write(
                path.join(format!("seed-{}.json", self.seed)),
                serde_json::to_vec_pretty(&evidence).unwrap(),
            );
        }
    }
}

async fn pending(daemon: &OwnedDaemon, agent: &str, run: &str) -> Value {
    daemon.wait_run(agent, run, "awaiting_approval").await;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let response = daemon
            .api(
                "GET",
                &format!("/api/approvals?agentId={agent}"),
                Value::Null,
                None,
            )
            .await;
        if let Some(approval) = response["approvals"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["runId"] == run && a["status"] == "pending")
        {
            return approval.clone();
        }
        assert!(
            Instant::now() < deadline,
            "public pending approval not visible for {run}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn seeded_sequence(seed: u64) {
    let profile = profile();
    let root = profile.path();
    let mut oracle = Oracle::new(seed);
    let mut daemon = OwnedDaemon::start(root, "run").await;
    let (agent, session) = daemon.setup(root).await;
    let mut decisions = [Decision::Allow, Decision::Deny, Decision::Stop];
    let mut random = seed;
    for index in (1..decisions.len()).rev() {
        random = random
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        decisions.swap(index, (random as usize) % (index + 1));
    }
    for (index, decision) in decisions.into_iter().enumerate() {
        let effect = format!("s{seed}-t{index}");
        let input = format!("CRASH_WRITE EFFECT_ID={effect}");
        let key = format!("seed-{seed}-turn-{index}");
        oracle.action(format!("submit {effect}"));
        let run = daemon.submit(&agent, &session, &input, &key).await;
        let run_id = run["id"].as_str().unwrap().to_owned();
        let approval = pending(&daemon, &agent, &run_id).await;
        oracle.dispatches.insert(effect.clone(), 1);
        oracle.check(root, &daemon, &agent).await;
        oracle.action(format!("replay {effect}"));
        assert_eq!(
            daemon.submit(&agent, &session, &input, &key).await["id"],
            run_id
        );
        oracle.action(format!("changed duplicate {effect}"));
        let (status, _) = daemon
            .request(
                "POST",
                &format!("/api/agents/{agent}/sessions/{session}/runs"),
                json!({"text":format!("{input} changed"),"mode":"queue"}),
                Some(&key),
            )
            .await;
        assert_eq!(
            status,
            409,
            "changed duplicate accepted: {}",
            oracle.evidence()
        );
        oracle.check(root, &daemon, &agent).await;
        oracle.action(format!("owner {decision:?} {effect}"));
        let route = format!(
            "/api/approvals/{}/decision",
            approval["id"].as_str().unwrap()
        );
        let status = match decision {
            Decision::Stop => {
                daemon
                    .api(
                        "POST",
                        &format!("/api/agents/{agent}/runs/{run_id}/stop"),
                        json!({}),
                        None,
                    )
                    .await;
                "cancelled"
            }
            Decision::Allow | Decision::Deny => {
                daemon.api("POST", &route, json!({"decision":if matches!(decision, Decision::Allow) { "allow_once" } else { "deny" },"revision":approval["revision"]}), None).await;
                *oracle.dispatches.get_mut(&effect).unwrap() += 1;
                if matches!(decision, Decision::Allow) {
                    oracle.effects.insert(effect.clone());
                }
                "completed"
            }
        };
        daemon.wait_run(&agent, &run_id, status).await;
        oracle.terminal.insert(run_id.clone(), status.into());
        oracle.check(root, &daemon, &agent).await;
        if !matches!(decision, Decision::Allow) {
            oracle.action(format!("stale allow {effect}"));
            let (status, _) = daemon
                .request(
                    "POST",
                    &route,
                    json!({"decision":"allow_once","revision":approval["revision"]}),
                    None,
                )
                .await;
            assert_eq!(
                status,
                409,
                "stale decision granted authority: {}",
                oracle.evidence()
            );
        }
        oracle.action(format!("replay terminal {effect}"));
        assert_eq!(
            daemon.submit(&agent, &session, &input, &key).await["id"],
            run_id
        );
        if index < 2 {
            oracle.action("kill and restart owned process".into());
            daemon.kill();
            daemon = OwnedDaemon::start(root, "run").await;
            oracle.restarts += 1;
            assert!(oracle.restarts <= 2);
        }
        oracle.check(root, &daemon, &agent).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn harness_reliability_seeded_public_lifecycle_sequences() {
    for seed in SEEDS {
        tokio::time::timeout(Duration::from_secs(150), seeded_sequence(seed))
            .await
            .unwrap_or_else(|_| panic!("seed={seed} exceeded its total sequence deadline"));
    }
}

#[test]
fn harness_reliability_independent_oracle_rejects_unapproved_effect_and_dispatch() {
    let fixture = profile();
    let mut oracle = Oracle::new(0);
    assert!(oracle.observe(fixture.path()).is_ok());
    fs::write(
        fixture.path().join("workspace/effect-forged.txt"),
        "effect:forged",
    )
    .unwrap();
    assert!(oracle
        .observe(fixture.path())
        .unwrap_err()
        .contains("effect IDs differ"));
    oracle.effects.insert("forged".into());
    fs::write(
        fixture.path().join("calls.jsonl"),
        "{\"effectId\":\"unadmitted\"}\n",
    )
    .unwrap();
    assert!(oracle
        .observe(fixture.path())
        .unwrap_err()
        .contains("dispatches differ"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn harness_reliability_actual_save_barrier_prevents_dispatch_on_failed_admission() {
    let profile = profile();
    let root = profile.path();
    let daemon = OwnedDaemon::start(root, "reliability-controls").await;
    let (agent, session) = daemon.setup(root).await;
    fs::write(root.join("arm-save-failure"), "owned fixture command").unwrap();
    marker(root, "save-armed").await;
    let route = format!("/api/agents/{agent}/sessions/{session}/runs");
    let request = daemon.request(
        "POST",
        &route,
        json!({"text":"SAFE_INPUT EFFECT_ID=save-barrier","mode":"queue"}),
        Some("save-barrier"),
    );
    let observe = async {
        marker(root, "save-entered").await;
        assert!(
            !root.join("calls.jsonl").exists(),
            "model dispatched before admission save acknowledgement"
        );
        fs::write(root.join("release-save"), "fail actual save").unwrap();
    };
    let ((status, _), ()) = tokio::join!(request, observe);
    assert_eq!(status, 503);
    assert!(
        !root.join("calls.jsonl").exists(),
        "failed admission still dispatched a model"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn harness_reliability_actual_owner_loss_closes_waiting_approval_and_new_admission() {
    let profile = profile();
    let root = profile.path();
    let daemon = OwnedDaemon::start(root, "reliability-controls").await;
    let (agent, session) = daemon.setup(root).await;
    let run = daemon
        .submit(
            &agent,
            &session,
            "CRASH_WRITE EFFECT_ID=owner-lost",
            "owner-lost",
        )
        .await;
    let approval = pending(&daemon, &agent, run["id"].as_str().unwrap()).await;
    fs::write(root.join("lose-owner"), "revoke owned fixture writer").unwrap();
    marker(root, "owner-lost").await;
    let (status, _) = daemon
        .request(
            "POST",
            &format!(
                "/api/approvals/{}/decision",
                approval["id"].as_str().unwrap()
            ),
            json!({"decision":"allow_once","revision":approval["revision"]}),
            None,
        )
        .await;
    assert!(
        matches!(status, 409 | 503),
        "lost writer granted approval: {status}"
    );
    let (status, _) = daemon
        .request(
            "POST",
            &format!("/api/agents/{agent}/sessions/{session}/runs"),
            json!({"text":"SAFE_INPUT EFFECT_ID=after-owner-loss","mode":"queue"}),
            Some("after-owner-loss"),
        )
        .await;
    assert!(
        matches!(status, 409 | 503),
        "lost writer admitted input: {status}"
    );
    let mut oracle = Oracle::new(99);
    oracle.dispatches.insert("owner-lost".into(), 1);
    oracle.observe(root).unwrap();
}

#[path = "harness_reliability_tests/live_trace.rs"]
mod live_trace;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn harness_reliability_actual_crash_preserves_unknown_model_reservation() {
    let profile = profile();
    let root = profile.path();
    let mut daemon = OwnedDaemon::start(root, "run").await;
    let (agent, session) = daemon.setup(root).await;
    daemon
        .api(
            "PATCH",
            &format!("/api/agents/{agent}"),
            json!({"settings":{"executionLimits":{"maxModelAttempts":1,"maxRetries":0}}}),
            None,
        )
        .await;
    let run = daemon
        .submit(
            &agent,
            &session,
            "BLOCK_INPUT EFFECT_ID=budget-crash",
            "budget-crash",
        )
        .await;
    let run_id = run["id"].as_str().unwrap();
    marker(root, "blocked").await;
    let saved: Value =
        serde_json::from_slice(&fs::read(root.join("control-plane.json")).unwrap()).unwrap();
    let scope_id = saved["executionBudgets"]["runScopes"][run_id]
        .as_str()
        .expect("actual dispatch has a durable budget binding")
        .to_owned();
    let scope = saved["executionBudgets"]["scopes"][&scope_id].clone();
    let reservations = scope["reservations"]
        .as_object()
        .expect("durable reservations");
    assert!(
        !reservations.is_empty(),
        "physical model started without persisted admission"
    );
    assert!(
        reservations
            .values()
            .any(|r| r["request"]["operation"] == "modelAttempt" && r["settlement"].is_null()),
        "pending physical model must retain an unsettled reservation"
    );
    daemon.kill();
    let restored = OwnedDaemon::start(root, "run").await;
    assert_eq!(restored.run(&agent, run_id).await["status"], "interrupted");
    let after: Value =
        serde_json::from_slice(&fs::read(root.join("control-plane.json")).unwrap()).unwrap();
    let retained = &after["executionBudgets"]["scopes"][&scope_id];
    assert_eq!(after["executionBudgets"]["runScopes"][run_id], scope_id);
    assert_eq!(
        retained["deadlineAtMs"], scope["deadlineAtMs"],
        "crash extended the original deadline"
    );
    assert_eq!(retained["limits"], scope["limits"]);
    for (id, reservation) in reservations {
        assert_eq!(
            retained["reservations"][id]["request"], reservation["request"],
            "restart changed admitted physical usage"
        );
        let settlement = &retained["reservations"][id]["settlement"];
        assert!(
            settlement.is_null() || settlement["outcome"].get("unknown").is_some(),
            "restart invented a known outcome/refund for an unretired call"
        );
    }
    let route = format!("/api/agents/{agent}/runs/{run_id}/recovery");
    let summary = restored.api("GET", &route, Value::Null, None).await;
    assert_eq!(summary["reviewRequired"], true);
    let (status, _) = restored
        .request(
            "POST",
            &route,
            json!({"fingerprint":"0".repeat(64),"reviewed":true}),
            None,
        )
        .await;
    assert_eq!(
        status, 409,
        "a stale continuation fingerprint granted authority"
    );
    let (status, _) = restored
        .request(
            "POST",
            &route,
            json!({"fingerprint":summary["fingerprint"],"reviewed":false}),
            None,
        )
        .await;
    assert_eq!(
        status, 409,
        "uncertain physical dispatch resumed without owner review"
    );
    let (status, continuation) = restored
        .request(
            "POST",
            &route,
            json!({"fingerprint":summary["fingerprint"],"reviewed":true}),
            None,
        )
        .await;
    match status {
        200 | 202 => {
            let id = continuation["run"]["id"]
                .as_str()
                .expect("accepted continuation identity");
            restored.wait_run(&agent, id, "failed").await;
            let saved: Value =
                serde_json::from_slice(&fs::read(root.join("control-plane.json")).unwrap())
                    .unwrap();
            assert_eq!(
                saved["executionBudgets"]["runScopes"][id], scope_id,
                "reviewed continuation minted a new allowance"
            );
        }
        409 | 503 => {}
        other => panic!("unexpected reviewed recovery result: {other} {continuation}"),
    }
    let mut oracle = Oracle::new(100);
    oracle.dispatches.insert("budget-crash".into(), 1);
    oracle.observe(root).unwrap();
}
