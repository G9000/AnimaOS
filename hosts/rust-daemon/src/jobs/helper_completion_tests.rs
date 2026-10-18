//! Runtime source/budget/review contracts. Recording models, isolated files,
//! exact native service boundaries; no provider, transport or compiler.
use super::*;
use crate::agent_runs::test_support::{lead_config, Gate, ScriptedModel};
use crate::approvals::{ApprovalPolicy, PolicyAction, RiskClass};
use crate::control_plane_store::ControlPlaneStoreConfig;
use crate::runs::DelegationOutputBinding;
use anima_core::{
    AgentConfig, DataValue, ModelAdapter, ModelGenerateRequest, ModelGenerateResponse,
    ModelStopReason, TokenUsage, ToolCall,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::{RwLock, Semaphore};

struct Store(std::path::PathBuf);
impl Store {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("anima-origin-job-{}.json", uuid::Uuid::new_v4())))
    }
}
impl Drop for Store {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn hash(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

fn job_value(agent: &str, id: &str, status: &str) -> Value {
    json!({
        "id":id,"agentId":agent,"title":"Origin work","prompt":"Inspect exact evidence",
        "requestKey":id,"status":status,"revision":2,"attempt":1,"createdAtMs":1,
        "updatedAtMs":4,"startedAtMs":2,"finishedAtMs":null,"result":null,"error":null,
        "maxAttempts":3,"requiresApproval":false,"approvedAtMs":null,"attempts":[],"goalId":null
    })
}

fn pending_job(agent: &str, id: &str) -> AgentJobRecord {
    let mut value = job_value(agent, id, "pending_delegations");
    value["pendingDelegations"] = json!({"launchRunId":format!("run_parent_{id}"),"childRunIds":[format!("run_child_{id}")],"completionRunIds":[format!("run_callback_{id}")]});
    let record: AgentJobRecord = serde_json::from_value(value).unwrap();
    record.validate().unwrap();
    record
}

fn completed_job(agent: &str, reviewed: bool) -> AgentJobRecord {
    let mut value = job_value(agent, "job_final", "completed");
    let result = "Visible aggregate output";
    let evidence = json!({"generation":1,"sha256":hash(result),"outputs":[{"runId":"run_callback","replyMessageId":"reply_callback","outputSha256":hash(result),"preview":result,"previewTruncated":false}]});
    value["finishedAtMs"] = json!(4);
    value["result"] = json!(result);
    value["finalOutput"] = evidence.clone();
    value["attempts"] = json!([{"attempt":1,"status":"completed","startedAtMs":2,"finishedAtMs":4,"result":result,"error":null,"resultTruncated":false,"review":null,"finalOutput":evidence}]);
    let provenance = json!({"launchRunId":"run_parent","childRunIds":["run_child"],"completionRunIds":["run_callback"],
        "disposition":"completed","reason":"Exact native review fixture"});
    value["delegationEvidence"] = provenance.clone();
    value["attempts"][0]["delegationEvidence"] = provenance;
    if reviewed {
        value["attempts"][0]["review"] = json!({"decision":"changes_requested","note":"Revise after inspecting current evidence","reviewedAtMs":4,"finalOutputGeneration":1,"finalOutputSha256":hash(result)});
    }
    let record: AgentJobRecord = serde_json::from_value(value).unwrap();
    record.validate().unwrap();
    record
}

async fn fixture(model: Arc<dyn ModelAdapter>) -> (JobService, String, Store) {
    let file = Store::new();
    let mut state = DaemonState::with_model_adapter(model);
    state.set_control_plane_store(Some(ControlPlaneStoreConfig::Json(file.0.clone())));
    let mut config = lead_config("Origin companion");
    config.tools = Some(
        crate::tools::ToolRegistry::new()
            .resolve_descriptors([
                "calculate",
                "spawn_helper_async",
                "list_helper_runs",
                "steer_helper",
                "stop_helper",
            ])
            .unwrap(),
    );
    config.settings.as_mut().unwrap().max_retries = Some(0);
    let owner = state.create_agent(config).unwrap().state.id;
    state.approvals.set_policy(
        &owner,
        ApprovalPolicy::default().with(RiskClass::Delegate, PolicyAction::Allow),
    );
    let state = Arc::new(RwLock::new(state));
    let runs = AgentRunCoordinator::new(state.clone(), Arc::new(Semaphore::new(2)));
    (JobService::new(state, runs), owner, file)
}

async fn save_job(service: &JobService, record: AgentJobRecord) {
    let save = {
        let mut state = service.state.write().await;
        state.jobs.insert(record.id.clone(), record);
        state.control_plane_persist_request()
    };
    save.save().await.unwrap();
}

#[test]
fn fp05_origin_runtime_pending_jobs_occupy_existing_eight_active_budget() {
    let mut jobs: HashMap<_, _> = (0..8)
        .map(|n| {
            let job = pending_job("owner", &format!("job_{n}"));
            (job.id.clone(), job)
        })
        .collect();
    assert!(
        ensure_capacity(&jobs).is_err(),
        "pending useful work cannot vacate active capacity"
    );
    jobs.remove("job_7");
    assert!(ensure_capacity(&jobs).is_ok());
}

#[test]
fn fp05_origin_authority_pending_job_capacity_uses_existing_claims() {
    for (jobs, allowed) in [(0, true), (7, true), (8, false), (9, false)] {
        let mut state = DaemonState::new();
        for index in 0..jobs {
            let job = pending_job("owner", &format!("pending_job_{index}"));
            state.jobs.insert(job.id.clone(), job);
        }
        let before = serde_json::to_value(state.control_plane_snapshot()).unwrap();
        assert_eq!(
            super::helper_completion::ensure_origin_capacity(&state).is_ok(),
            allowed,
            "{jobs} pending jobs share one finite eight-chain budget"
        );
        assert_eq!(
            serde_json::to_value(state.control_plane_snapshot()).unwrap(),
            before
        );
    }
}

#[tokio::test]
async fn fp05_origin_runtime_review_requires_exact_current_aggregate_and_undoes_refusal() {
    let mut failures = Vec::new();
    for variant in [
        "missing",
        "generation",
        "digest",
        "exact",
        "legacy_invented",
    ] {
        let model = ScriptedModel::new(vec![]);
        let (service, owner, file) = fixture(model.clone()).await;
        let mut before = completed_job(&owner, false);
        if variant == "legacy_invented" {
            before.final_output = None;
            before.attempts[0].final_output = None;
            before.delegation_evidence = None;
            before.attempts[0].delegation_evidence = None;
            before.validate().unwrap();
        }
        save_job(&service, before.clone()).await;
        let bytes = std::fs::read(&file.0).unwrap();
        let binding = match variant {
            "missing" => None,
            "generation" => Some(DelegationOutputBinding {
                generation: 2,
                sha256: hash("Visible aggregate output"),
            }),
            "digest" => Some(DelegationOutputBinding {
                generation: 1,
                sha256: "a".repeat(64),
            }),
            _ => Some(DelegationOutputBinding {
                generation: 1,
                sha256: hash("Visible aggregate output"),
            }),
        };
        let result = service
            .review_output_with_binding(
                &owner,
                &before.id,
                before.revision,
                JobReviewDecision::Accepted,
                "Exact owner review",
                binding,
            )
            .await;
        if variant == "exact" {
            match result {
                Ok(record)
                    if record.validate().is_ok()
                        && record.attempts[0]
                            .review
                            .as_ref()
                            .unwrap()
                            .final_output_generation
                            == Some(1)
                        && record.attempts[0]
                            .review
                            .as_ref()
                            .unwrap()
                            .final_output_sha256
                            .as_deref()
                            == Some(hash("Visible aggregate output").as_str()) => {}
                other => failures.push(format!("{variant}: {other:?}")),
            }
        } else if result.is_ok()
            || service.state.read().await.jobs[&before.id] != before
            || std::fs::read(&file.0).unwrap() != bytes
        {
            failures.push(format!("{variant}: refusal must preserve state/disk"));
        }
        assert!(model.requests().is_empty());
    }
    assert!(
        failures.is_empty(),
        "native review must bind exact displayed output: {failures:?}"
    );
}

#[tokio::test]
async fn fp05_origin_runtime_retry_preserves_attempt_and_clears_current_aggregate() {
    let (service, owner, _file) = fixture(ScriptedModel::new(vec![])).await;
    let before = completed_job(&owner, true);
    save_job(&service, before.clone()).await;
    let queued = service
        .retry(&owner, &before.id, before.revision, false)
        .await
        .unwrap();
    assert_eq!(queued.status, AgentJobStatus::Queued);
    assert!(
        queued.final_output.is_none(),
        "new work cannot expose the prior attempt's final aggregate"
    );
    assert_eq!(queued.attempts, before.attempts);
    queued.validate().unwrap();
}

#[tokio::test]
async fn fp05_origin_authority_exact_review_save_failure_restores_output_and_unreviewed_state() {
    let model = ScriptedModel::new(vec![]);
    let (service, owner, file) = fixture(model.clone()).await;
    let before = completed_job(&owner, false);
    save_job(&service, before.clone()).await;
    let bytes = std::fs::read(&file.0).unwrap();
    let blocked = std::env::temp_dir().join(format!(
        "anima-origin-review-refusal-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir(&blocked).unwrap();
    service
        .state
        .write()
        .await
        .set_control_plane_store(Some(ControlPlaneStoreConfig::Json(blocked.clone())));
    let result = service
        .review_output_with_binding(
            &owner,
            &before.id,
            before.revision,
            JobReviewDecision::Accepted,
            "Review exact output",
            Some(DelegationOutputBinding {
                generation: 1,
                sha256: hash("Visible aggregate output"),
            }),
        )
        .await;
    assert!(matches!(result, Err(JobError::Unavailable(_))));
    assert_eq!(service.state.read().await.jobs[&before.id], before);
    assert_eq!(std::fs::read(&file.0).unwrap(), bytes);
    assert!(model.requests().is_empty());
    std::fs::remove_dir(blocked).unwrap();
}

#[tokio::test]
async fn fp05_origin_runtime_startup_closes_pending_work_with_visible_reference_evidence() {
    let model = ScriptedModel::new(vec![]);
    let (service, owner, _file) = fixture(model.clone()).await;
    let pending = pending_job(&owner, "job_interrupted_origin");
    let refs = serde_json::to_value(pending.pending_delegations.as_ref().unwrap()).unwrap();
    save_job(&service, pending.clone()).await;
    service.start().await.unwrap();
    let after = service
        .list(&owner)
        .await
        .unwrap()
        .into_iter()
        .find(|job| job.id == pending.id)
        .unwrap();
    service.shutdown().await;
    assert_eq!(
        after.status,
        AgentJobStatus::NeedsReview,
        "startup cannot leave durable pending work with live replay rights"
    );
    assert_eq!(after.attempt, 1);
    assert!(after.pending_delegations.is_none());
    assert_eq!(after.attempts.len(), 1);
    let written = serde_json::to_value(&after).unwrap();
    let evidence = &written["delegationEvidence"];
    assert_eq!(evidence["launchRunId"], refs["launchRunId"]);
    assert_eq!(evidence["childRunIds"], refs["childRunIds"]);
    assert_eq!(evidence["completionRunIds"], refs["completionRunIds"]);
    assert_eq!(evidence["disposition"], "review_required");
    assert!(!evidence["reason"].as_str().unwrap().trim().is_empty());
    assert_eq!(written["attempts"][0]["delegationEvidence"], *evidence);
    assert!(model.requests().is_empty());
    after.validate().unwrap();
}

struct OriginModel {
    total: AtomicUsize,
    owner: AtomicUsize,
    gate: Gate,
}
#[async_trait::async_trait]
impl ModelAdapter for OriginModel {
    fn provider(&self) -> &str {
        "fp05-origin-offline"
    }
    async fn generate(
        &self,
        config: &AgentConfig,
        _: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        let count = self.total.fetch_add(1, Ordering::SeqCst) + 1;
        assert!(count <= 4, "fixed four-call source settlement ceiling");
        let (text, tool_calls) = if crate::agent_runs::config_helper_parent(config).is_some() {
            self.gate.entered.add_permits(1);
            self.gate.release.acquire().await.unwrap().forget();
            ("Child committed exact origin evidence", None)
        } else {
            match self.owner.fetch_add(1, Ordering::SeqCst) {
                0 => (
                    "",
                    Some(vec![ToolCall {
                        id: "origin-job-launch".into(),
                        name: "spawn_helper_async".into(),
                        args: BTreeMap::from([
                            ("name".into(), DataValue::String("Origin assistant".into())),
                            (
                                "task".into(),
                                DataValue::String("Return exact bounded evidence".into()),
                            ),
                            ("completion".into(), DataValue::String("origin".into())),
                        ]),
                    }]),
                ),
                1 => ("Accepted helper work; useful output is pending", None),
                2 => ("Final origin callback evidence", None),
                _ => panic!("extra origin model work"),
            }
        };
        Ok(ModelGenerateResponse {
            content: Content {
                text: text.into(),
                ..Default::default()
            },
            stop_reason: if tool_calls.is_some() {
                ModelStopReason::ToolCall
            } else {
                ModelStopReason::End
            },
            tool_calls,
            usage: TokenUsage::default(),
        })
    }
}

async fn settled_job(service: &JobService, id: &str, pending_is_settled: bool) -> AgentJobRecord {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let owner = service.state.read().await.jobs[id].agent_id.clone();
            let record = service
                .list(&owner)
                .await
                .unwrap()
                .into_iter()
                .find(|j| j.id == id)
                .unwrap();
            if !matches!(
                record.status,
                AgentJobStatus::Queued | AgentJobStatus::Running
            ) && (pending_is_settled || record.status != AgentJobStatus::PendingDelegations)
            {
                break record;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn fp05_origin_runtime_real_job_turn_defers_final_output_until_child_and_callback_settle() {
    let model = Arc::new(OriginModel {
        total: 0.into(),
        owner: 0.into(),
        gate: Gate::new(),
    });
    let (service, owner, _file) = fixture(model.clone()).await;
    let job = service
        .create(
            &owner,
            "Origin work",
            "Delegate one bounded evidence task",
            "origin-real-job",
        )
        .await
        .unwrap();
    service.start().await.unwrap();
    let pending = settled_job(&service, &job.id, true).await;
    let admitted = pending.status == AgentJobStatus::PendingDelegations;
    let pending_source = pending.pending_delegations.clone();
    model.gate.release();
    let final_job = if admitted {
        settled_job(&service, &job.id, false).await
    } else {
        pending.clone()
    };
    service.shutdown().await;
    service.runs.shutdown().await;
    assert!(
        admitted,
        "parent Accepted turn is not final useful output: {pending:?}"
    );
    assert!(
        pending.finished_at_ms.is_none() && pending.result.is_none() && pending.attempts.is_empty()
    );
    let refs = pending_source.unwrap();
    assert_eq!(refs.child_run_ids.len(), 1);
    assert_eq!(refs.completion_run_ids.len(), 1);
    assert_eq!(final_job.status, AgentJobStatus::Completed);
    assert_eq!(final_job.attempt, 1);
    assert_eq!(final_job.attempts.len(), 1);
    assert!(final_job.pending_delegations.is_none());
    assert!(final_job
        .result
        .as_deref()
        .unwrap()
        .contains("Final origin callback evidence"));
    let final_output = final_job.final_output.as_ref().unwrap();
    assert_eq!(final_output.generation, 1);
    assert_eq!(final_output.outputs[0].run_id, refs.completion_run_ids[0]);
    assert_eq!(model.total.load(Ordering::SeqCst), 4);
    final_job.validate().unwrap();
    let mut saved = crate::control_plane_store::load_control_plane_snapshot(
        &ControlPlaneStoreConfig::Json(_file.0.clone()),
    )
    .await
    .unwrap()
    .unwrap();
    let callback = saved
        .runs
        .iter()
        .find(|run| run.id == refs.completion_run_ids[0])
        .unwrap();
    assert_eq!(
        callback.reply_sha256.as_deref(),
        Some(hash("Final origin callback evidence").as_str())
    );
    assert_eq!(
        final_output.outputs[0].output_sha256,
        callback.reply_sha256.as_ref().unwrap().as_str()
    );
    for _ in 0..2 {
        let offline = ScriptedModel::new(vec![]);
        let mut restored = DaemonState::with_model_adapter(offline.clone());
        restored
            .restore_control_plane_snapshot(saved)
            .expect("exact final callback provenance survives a fresh restore");
        assert_eq!(restored.jobs[&job.id].final_output, final_job.final_output);
        assert_eq!(restored.jobs[&job.id].attempt, 1);
        assert!(offline.requests().is_empty());
        saved = restored.control_plane_snapshot();
    }
}

#[tokio::test]
async fn fp05_origin_authority_restored_final_output_rejects_parent_or_child_unknown_receipt() {
    let model = Arc::new(OriginModel {
        total: 0.into(),
        owner: 0.into(),
        gate: Gate::new(),
    });
    let (service, owner, file) = fixture(model.clone()).await;
    let job = service
        .create(
            &owner,
            "Strict source evidence",
            "Delegate bounded evidence",
            "strict-final-source",
        )
        .await
        .unwrap();
    service.start().await.unwrap();
    let pending = settled_job(&service, &job.id, true).await;
    model.gate.release();
    let complete = settled_job(&service, &job.id, false).await;
    service.shutdown().await;
    service.runs.shutdown().await;
    assert_eq!(complete.status, AgentJobStatus::Completed);
    assert_eq!(model.total.load(Ordering::SeqCst), 4);
    let refs = pending.pending_delegations.unwrap();
    let saved = crate::control_plane_store::load_control_plane_snapshot(
        &ControlPlaneStoreConfig::Json(file.0.clone()),
    )
    .await
    .unwrap()
    .unwrap();
    DaemonState::new()
        .restore_control_plane_snapshot(saved.clone())
        .expect("unaltered real final source fixture is valid");
    let mut failures = Vec::new();
    for (id, variant) in [&refs.launch_run_id, &refs.child_run_ids[0]]
        .into_iter()
        .flat_map(|id| ["started", "failed", "stopped"].map(|variant| (id, variant)))
    {
        let mut altered = saved.clone();
        let source = altered.runs.iter_mut().find(|run| &run.id == id).unwrap();
        match variant {
            "started" => source.action_receipts.push(crate::runs::ActionReceipt {
                tool_call_id: "persisted-unknown-effect".into(),
                name: "write_file".into(),
                status: crate::runs::ActionStatus::Started,
                started_at_ms: 3,
                finished_at_ms: None,
                duration_ms: None,
                arguments_preview: "{}".into(),
                arguments_truncated: false,
                result_preview: None,
                result_truncated: false,
                durability: crate::runs::ActionDurability::Persistent,
            }),
            "failed" => {
                // Keep the ordinary ledger/assignment shape valid so this tests
                // successful aggregate provenance rather than generic hash syntax.
                source.status = RunStatus::Failed;
                source.reply_sha256 = None;
                if let Some(helper) = &mut source.async_helper {
                    helper.completion = crate::runs::HelperCompletionState::ReviewRequired {
                        reason: "Saved child outcome contradicts final source success".into(),
                        run_id: helper.completion.run_id().map(str::to_owned),
                    };
                }
            }
            "stopped" => {
                source.stop = Some(crate::runs::RunStopRequest {
                    requested_at_ms: source.created_at_ms,
                })
            }
            _ => unreachable!(),
        }
        let offline = ScriptedModel::new(vec![]);
        let mut target = DaemonState::with_model_adapter(offline.clone());
        let before = serde_json::to_value(target.control_plane_snapshot()).unwrap();
        if target.restore_control_plane_snapshot(altered).is_ok()
            || serde_json::to_value(target.control_plane_snapshot()).unwrap() != before
        {
            failures.push(format!("{id}:{variant}"));
        }
        assert!(offline.requests().is_empty());
    }
    for variant in ["strip_evidence", "strip_final_output"] {
        let mut altered = saved.clone();
        let job = altered
            .jobs
            .iter_mut()
            .find(|job| job.id == complete.id)
            .unwrap();
        if variant == "strip_evidence" {
            job.delegation_evidence = None;
            job.attempts.last_mut().unwrap().delegation_evidence = None;
        } else {
            job.final_output = None;
            job.attempts.last_mut().unwrap().final_output = None;
        }
        let offline = ScriptedModel::new(vec![]);
        let mut target = DaemonState::with_model_adapter(offline.clone());
        let before = serde_json::to_value(target.control_plane_snapshot()).unwrap();
        if target.restore_control_plane_snapshot(altered).is_ok()
            || serde_json::to_value(target.control_plane_snapshot()).unwrap() != before
        {
            failures.push(variant.into());
        }
        assert!(offline.requests().is_empty());
    }
    assert!(failures.is_empty(),"saved final aggregate must reject contradictory source/evidence before mutation: {failures:?}");
}
