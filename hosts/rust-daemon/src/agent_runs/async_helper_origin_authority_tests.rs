//! Native-admitted assignment IDs/config/private session, then a typed saved
//! job source fixture. No origin producer is enabled by these source fixtures.
use super::*;
use crate::control_plane_store::{ControlPlaneSnapshot, ControlPlaneStoreConfig};
use crate::runs::{DelegationOrigin, HelperCompletionState};
use crate::sessions::{SessionKind, SessionOrigin, SessionRecord, TitleSource};
use serde_json::json;

async fn pending_source() -> ControlPlaneSnapshot {
    let harness = Harness::new(0).await;
    let accepted = harness
        .launch("source-fixture", "Bounded source evidence")
        .await;
    let parent_id = harness.context.run_link.as_ref().unwrap().run_id.clone();
    let child_id = accepted["runId"].as_str().unwrap().to_string();
    let callback_id = format!("run_{}", uuid::Uuid::new_v4());
    let room = "job:job_origin";
    let snapshot = {
        let mut state = harness.coordinator.state.write().await;
        state.sessions.insert(SessionRecord::new(
            &harness.parent.id,
            room,
            SessionKind::Job,
            SessionOrigin::Job,
            "Origin job".into(),
            TitleSource::System,
            1,
        ));
        let parent = state.runs.get_mut(&parent_id).unwrap();
        parent.session_id = room.into();
        parent.source = RunSource::Job;
        parent.source_ref = Some("job_origin:1".into());
        parent.idempotency_key = Some("job:job_origin:attempt:1".into());
        parent.finish(RunStatus::Completed, None, 4);
        parent.reply_message_id = Some("reply_parent".into());
        parent.mirrored = true;
        let child = state.runs.get_mut(&child_id).unwrap();
        let helper = child.async_helper.as_mut().unwrap();
        helper.owner_session_id = room.into();
        helper.origin = Some(DelegationOrigin::JobAttempt {
            job_id: "job_origin".into(),
            attempt: 1,
            claim_revision: 2,
        });
        helper.completion = HelperCompletionState::AwaitingResult {
            reserved_run_id: callback_id.clone(),
        };
        let session = state
            .sessions
            .get_mut(
                accepted["agentId"].as_str().unwrap(),
                accepted["sessionId"].as_str().unwrap(),
            )
            .unwrap();
        session.parent_session_id = Some(room.into());
        let job:crate::jobs::AgentJobRecord=serde_json::from_value(json!({
            "id":"job_origin","agentId":harness.parent.id,"title":"Origin source","prompt":"Inspect exact evidence",
            "requestKey":"source-fixture","status":"pending_delegations","revision":3,"attempt":1,
            "createdAtMs":1,"updatedAtMs":4,"startedAtMs":2,"finishedAtMs":null,"result":null,"error":null,
            "maxAttempts":3,"requiresApproval":false,"approvedAtMs":null,"attempts":[],"goalId":null,
            "pendingDelegations":{"launchRunId":parent_id,"childRunIds":[child_id],"completionRunIds":[callback_id]}
        })).unwrap();
        job.validate().unwrap();
        state.jobs.insert(job.id.clone(), job);
        state.control_plane_snapshot()
    };
    harness.coordinator.request_shutdown();
    harness.permits.close();
    assert!(harness.model.requests().is_empty());
    snapshot
}

fn child(snapshot: &ControlPlaneSnapshot) -> &RunRecord {
    snapshot
        .runs
        .iter()
        .find(|run| run.async_helper.is_some())
        .unwrap()
}

fn close_for_review(snapshot: &mut ControlPlaneSnapshot) {
    let pending = snapshot.jobs[0].pending_delegations.take().unwrap();
    let job = &mut snapshot.jobs[0];
    job.status = crate::jobs::AgentJobStatus::NeedsReview;
    job.revision = 4;
    job.updated_at_ms = 5;
    job.finished_at_ms = Some(5);
    job.error = Some("Interrupted source chain; owner review required".into());
    job.delegation_evidence = Some(crate::runs::DelegationEvidence::review(
        pending,
        "Interrupted source chain; owner review required",
    ));
    job.preserve_legacy_attempt();
    job.validate().unwrap();
    let child = snapshot
        .runs
        .iter_mut()
        .find(|run| run.async_helper.is_some())
        .unwrap();
    child.finish(RunStatus::Interrupted, None, 5.max(child.created_at_ms));
    let helper = child.async_helper.as_mut().unwrap();
    let run_id = helper.completion.run_id().map(str::to_owned);
    helper.completion = HelperCompletionState::ReviewRequired {
        reason: "Interrupted; no replay".into(),
        run_id,
    };
    for run in &mut snapshot.runs {
        run.mirrored = true;
    }
}

#[tokio::test]
async fn fp05_origin_authority_typed_witness_is_closed_and_preserves_claim_revision() {
    let snapshot = pending_source().await;
    let original = serde_json::to_value(child(&snapshot)).unwrap();
    assert_eq!(
        original["asyncHelper"]["origin"],
        json!({"kind":"job_attempt","jobId":"job_origin","attempt":1,"claimRevision":2})
    );
    assert_eq!(
        snapshot.jobs[0].revision, 3,
        "admission witness does not follow mutable source revision"
    );
    assert_eq!(
        serde_json::to_value(serde_json::from_value::<RunRecord>(original.clone()).unwrap())
            .unwrap(),
        original
    );
    let mut forged = original.clone();
    forged["asyncHelper"]["origin"]["recipient"] = json!("model-selected-destination");
    assert!(serde_json::from_value::<RunRecord>(forged).is_err());
    let mut schedule = original;
    schedule["asyncHelper"]["origin"] = json!({"kind":"schedule_occurrence","scheduleId":"schedule-one",
        "runIdempotencyKey":"schedule:schedule-one:original","firedAtMs":7,"manual":false});
    assert!(
        serde_json::from_value::<RunRecord>(schedule).is_err(),
        "unimplemented schedule-origin metadata is not an accepted source witness"
    );
}

#[tokio::test]
async fn fp05_origin_authority_present_wrong_missing_and_replaced_source_claims_are_rejected() {
    let baseline = pending_source().await;
    crate::runs::validate_origin_snapshot(&baseline).unwrap();
    let mut failures = Vec::new();
    for variant in [
        "missing_job",
        "wrong_agent",
        "replacement_attempt",
        "wrong_parent_source",
        "wrong_source_ref",
        "missing_parent",
        "missing_child",
        "wrong_callback",
        "claim_zero",
    ] {
        let mut snapshot = baseline.clone();
        match variant {
            "missing_job" => snapshot.jobs.clear(),
            "wrong_agent" => snapshot.jobs[0].agent_id = "foreign_owner".into(),
            "replacement_attempt" => snapshot.jobs[0].attempt = 2,
            "wrong_parent_source" => {
                snapshot
                    .runs
                    .iter_mut()
                    .find(|run| run.async_helper.is_none())
                    .unwrap()
                    .source = RunSource::Web
            }
            "wrong_source_ref" => {
                snapshot
                    .runs
                    .iter_mut()
                    .find(|run| run.async_helper.is_none())
                    .unwrap()
                    .source_ref = Some("job_other:1".into())
            }
            "missing_parent" => snapshot.runs.retain(|run| run.async_helper.is_some()),
            "missing_child" => snapshot.runs.retain(|run| run.async_helper.is_none()),
            "wrong_callback" => {
                snapshot.jobs[0]
                    .pending_delegations
                    .as_mut()
                    .unwrap()
                    .completion_run_ids[0] = "run_foreign".into()
            }
            "claim_zero" => {
                snapshot
                    .runs
                    .iter_mut()
                    .find(|run| run.async_helper.is_some())
                    .unwrap()
                    .async_helper
                    .as_mut()
                    .unwrap()
                    .origin = Some(DelegationOrigin::JobAttempt {
                    job_id: "job_origin".into(),
                    attempt: 1,
                    claim_revision: 0,
                })
            }
            _ => unreachable!(),
        }
        if crate::runs::validate_origin_snapshot(&snapshot).is_ok() {
            failures.push(variant);
        }
    }
    assert!(
        failures.is_empty(),
        "strict authentic source claim checks missing: {failures:?}"
    );
}

#[tokio::test]
async fn fp05_origin_authority_recovery_uses_two_fresh_states_without_models_or_new_attempts() {
    let before = pending_source().await;
    let ids: HashSet<_> = before.runs.iter().map(|run| run.id.clone()).collect();
    let refs = before.jobs[0].pending_delegations.clone().unwrap();
    let path = std::env::temp_dir().join(format!(
        "anima-origin-auth-restart-{}.json",
        uuid::Uuid::new_v4()
    ));
    let store = ControlPlaneStoreConfig::Json(path.clone());
    crate::control_plane_store::save_control_plane_snapshot(Some(&store), &before)
        .await
        .unwrap();
    let mut last = None;
    for _ in 0..2 {
        let loaded = crate::control_plane_store::load_control_plane_snapshot(&store)
            .await
            .unwrap()
            .unwrap();
        let model = ScriptedModel::new(vec![]);
        let mut daemon = DaemonState::with_model_adapter(model.clone());
        daemon
            .restore_control_plane_snapshot(loaded)
            .expect("valid saved origin source recovers conservatively");
        daemon.set_control_plane_store(Some(store.clone()));
        let state = Arc::new(RwLock::new(daemon));
        let runs = AgentRunCoordinator::new(state.clone(), Arc::new(Semaphore::new(1)));
        let service = crate::jobs::JobService::new(state.clone(), runs.clone());
        service.start().await.unwrap();
        service.shutdown().await;
        runs.shutdown().await;
        let saved = state.read().await.control_plane_snapshot();
        assert_eq!(
            saved
                .runs
                .iter()
                .map(|run| run.id.clone())
                .collect::<HashSet<_>>(),
            ids
        );
        let job = &saved.jobs[0];
        assert_eq!(job.status, crate::jobs::AgentJobStatus::NeedsReview);
        assert_eq!(job.attempt, 1);
        assert_eq!(job.attempts.len(), 1);
        assert!(job.pending_delegations.is_none());
        let evidence = job.delegation_evidence.as_ref().unwrap();
        assert_eq!(evidence.launch_run_id, refs.launch_run_id);
        assert_eq!(evidence.child_run_ids, refs.child_run_ids);
        assert_eq!(evidence.completion_run_ids, refs.completion_run_ids);
        assert!(model.requests().is_empty());
        if let Some(previous) = last {
            assert_eq!(serde_json::to_value(&saved.jobs).unwrap(), previous);
        }
        last = Some(serde_json::to_value(&saved.jobs).unwrap());
        state
            .write()
            .await
            .control_plane_persist_request()
            .save()
            .await
            .unwrap();
    }
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn fp05_origin_authority_terminal_evidence_pins_age_count_and_retires_with_owner_history() {
    let mut snapshot = pending_source().await;
    close_for_review(&mut snapshot);
    let expected: HashSet<_> = snapshot.runs.iter().map(|run| run.id.clone()).collect();
    let live: HashSet<_> = snapshot
        .agents
        .iter()
        .map(|agent| agent.state.id.clone())
        .collect();
    // Independent literal ownership oracle, so the prune contract reaches its
    // own assertion even while host pin derivation is still the RED seam.
    let mut pins = expected.clone();
    pins.extend(
        snapshot.jobs[0]
            .delegation_evidence
            .as_ref()
            .unwrap()
            .completion_run_ids
            .clone(),
    );
    for age in [false, true] {
        let now = if age { u64::MAX / 2 } else { 100 };
        let mut ledger = crate::runs::RunLedger::default();
        for run in &snapshot.runs {
            ledger.insert(run.clone());
        }
        if !age {
            for agent in &live {
                for n in 0..crate::runs::MAX_TERMINAL_RUNS_PER_AGENT + 1 {
                    let mut newer = snapshot.runs[0].clone();
                    newer.id = format!("newer_{agent}_{n}");
                    newer.agent_id = agent.clone();
                    newer.async_helper = None;
                    newer.helper_completion = None;
                    newer.parent_run_id = None;
                    newer.created_at_ms = 1000 + n as u64;
                    newer.finished_at_ms = Some(1001 + n as u64);
                    newer.mirrored = true;
                    ledger.insert(newer);
                }
            }
        }
        ledger.prune_with_pins(now, &pins);
        assert!(
            expected.iter().all(|id| ledger.get(id).is_some()),
            "age={age}"
        );
        let mut retired = snapshot.clone();
        retired.jobs.clear();
        let retired_pins = crate::runs::origin_pins(&retired);
        assert!(expected.is_disjoint(&retired_pins));
        ledger.prune_with_pins(u64::MAX / 2, &retired_pins);
        assert!(
            expected.iter().all(|id| ledger.get(id).is_none()),
            "retired owner evidence releases bounded retention"
        );
    }
    let restored = crate::runs::RunLedger::restored_with_pins(
        snapshot.runs.clone(),
        &live,
        u64::MAX / 2,
        &pins,
    );
    assert!(
        expected.iter().all(|id| restored.get(id).is_some()),
        "pins must be supplied before first restore pruning"
    );
}

#[tokio::test]
async fn fp05_origin_authority_pin_derivation_keeps_archived_attempt_until_owner_retirement() {
    let mut snapshot = pending_source().await;
    close_for_review(&mut snapshot);
    let evidence = snapshot.jobs[0]
        .delegation_evidence
        .as_ref()
        .unwrap()
        .clone();
    let expected: HashSet<_> = std::iter::once(evidence.launch_run_id.clone())
        .chain(evidence.child_run_ids.clone())
        .chain(evidence.completion_run_ids.clone())
        .collect();
    assert!(expected.is_subset(&crate::runs::origin_pins(&snapshot)));
    let job = &mut snapshot.jobs[0];
    job.status = crate::jobs::AgentJobStatus::Queued;
    job.revision += 1;
    job.started_at_ms = None;
    job.finished_at_ms = None;
    job.error = None;
    job.delegation_evidence = None;
    job.validate().unwrap();
    assert!(
        expected.is_subset(&crate::runs::origin_pins(&snapshot)),
        "historical uncertain attempt retains visible source evidence on retry"
    );
    snapshot.jobs.clear();
    assert!(
        expected.is_disjoint(&crate::runs::origin_pins(&snapshot)),
        "actual owner-history retirement releases the source pins"
    );
}

#[tokio::test]
async fn fp05_origin_authority_snapshot_active_budget_counts_nine_pending_jobs_before_mutation() {
    let mut snapshot = pending_source().await;
    let template = snapshot.jobs[0].clone();
    snapshot.jobs.clear();
    snapshot.runs.clear();
    for n in 0..9 {
        let mut job = template.clone();
        job.id = format!("job_pending_{n}");
        job.request_key = job.id.clone();
        job.pending_delegations = Some(crate::runs::PendingDelegations {
            launch_run_id: format!("launch_{n}"),
            child_run_ids: vec![format!("child_{n}")],
            completion_run_ids: vec![format!("callback_{n}")],
        });
        snapshot.jobs.push(job);
    }
    let mut target = DaemonState::new();
    let before = serde_json::to_value(target.control_plane_snapshot()).unwrap();
    let result = target.restore_control_plane_snapshot(snapshot);
    assert!(
        result.is_err(),
        "nine pending attempts exceed the existing eight active job slots"
    );
    assert_eq!(
        serde_json::to_value(target.control_plane_snapshot()).unwrap(),
        before,
        "strict rejection precedes restore mutation"
    );
}

#[test]
fn fp05_origin_authority_required_source_evidence_has_finite_4096_run_capacity() {
    for (count, additional, allowed) in [
        (0, 9, true),
        (4087, 9, true),
        (4088, 9, false),
        (4096, 1, false),
        (4096, 0, true),
        (4097, 0, false),
        (1, usize::MAX, false),
    ] {
        let pins: HashSet<_> = (0..count).map(|n| format!("retained_run_{n}")).collect();
        assert_eq!(
            crate::runs::ensure_origin_evidence_capacity(&pins, additional).is_ok(),
            allowed,
            "{count} retained sources plus {additional} new references"
        );
        assert_eq!(pins.len(), count);
    }
}

#[tokio::test]
async fn fp05_origin_authority_committed_reply_digest_is_host_saved_and_reset_on_failed_finish() {
    use sha2::{Digest, Sha256};
    let harness = Harness::new(1).await;
    let handle = harness
        .launch("reply-digest", "Return exact committed bytes")
        .await;
    let id = handle["runId"].as_str().unwrap();
    wait_for(&harness.coordinator, id, RunStatus::Completed).await;
    let mut record = harness
        .coordinator
        .state
        .read()
        .await
        .runs
        .get(id)
        .unwrap()
        .clone();
    harness.coordinator.request_shutdown();
    let value = serde_json::to_value(&record).unwrap();
    assert!(record.reply_message_id.is_some());
    assert_eq!(
        value["replySha256"],
        json!(format!("{:x}", Sha256::digest(b"recorded child result"))),
        "a committed reply digest survives transcript hot-tail eviction"
    );
    record.finish(RunStatus::Failed, None, record.created_at_ms + 100);
    assert!(
        serde_json::to_value(record)
            .unwrap()
            .get("replySha256")
            .is_none(),
        "failed/rolled-back finish cannot retain successful evidence"
    );
}
