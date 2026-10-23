//! Owner configured, response-only admission evidence. No suite preserves the
//! existing authoring and activation paths.
use anima_core::skill_evaluation::{EvaluationSuite, EvaluationTrial};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub(crate) const MAX_EVALUATION_REPORTS: usize = 32;
pub(crate) const MAX_DAILY_EVALUATIONS: usize = 8;
pub(crate) const RUN_TIMEOUT_MS: u64 = 120_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SuiteRecord {
    pub(crate) workspace: String,
    pub(crate) revision: u64,
    pub(crate) suite: EvaluationSuite,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EvaluationIdentity {
    pub(crate) workspace: String,
    pub(crate) slug: String,
    pub(crate) suite_revision: u64,
    pub(crate) suite_digest: String,
    pub(crate) baseline_hash: Option<String>,
    pub(crate) candidate_hash: String,
    pub(crate) draft_id: String,
    pub(crate) draft_digest: String,
    pub(crate) model_config_digest: String,
    pub(crate) grader: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum EvaluationStatus {
    Running,
    Passed,
    Failed,
    Inconclusive,
    Interrupted,
    Stale,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EvaluationReport {
    pub(crate) id: String,
    pub(crate) identity: EvaluationIdentity,
    pub(crate) agent_id: String,
    pub(crate) model: String,
    pub(crate) provider: Option<String>,
    pub(crate) status: EvaluationStatus,
    pub(crate) reserved_at_ms: u64,
    pub(crate) finished_at_ms: Option<u64>,
    pub(crate) trials: Vec<EvaluationTrial>,
    pub(crate) error: Option<String>,
    pub(crate) response_only: bool,
    /// All-zero normalized usage can mean absent provider accounting.
    pub(crate) usage_complete: bool,
    pub(crate) retired_at_ms: Option<u64>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EvaluationRegistry {
    pub(crate) suites: BTreeMap<String, SuiteRecord>,
    pub(crate) reports: Vec<EvaluationReport>,
    /// Reservations survive pruning, failures, and restart. No refund on an
    /// ambiguous save and no automatic replay of reserved provider work.
    pub(crate) reservations: Vec<u64>,
}

impl EvaluationRegistry {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.suites.len() > super::MAX_SKILLS
            || self.reports.len() > MAX_EVALUATION_REPORTS
            || self.reservations.len() > MAX_DAILY_EVALUATIONS
        {
            return Err("Skill evaluation registry exceeds bounds".into());
        }
        for (slug, record) in &self.suites {
            if !super::is_valid_slug(slug)
                || record.revision == 0
                || record.workspace.is_empty()
                || record.workspace.len() > 4096
            {
                return Err("Invalid skill evaluation suite identity".into());
            }
            record.suite.validate().map_err(str::to_string)?;
        }
        let mut ids = std::collections::BTreeSet::new();
        for report in &self.reports {
            let hash =
                |value: &str| value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit());
            if report.id.is_empty()
                || report.id.len() > 128
                || !ids.insert(&report.id)
                || report.trials.len() > anima_core::skill_evaluation::MAX_FIXTURES * 6
                || !super::is_valid_slug(&report.identity.slug)
                || report.identity.workspace.len() > 4096
                || report.identity.workspace.is_empty()
                || report.identity.suite_revision == 0
                || !hash(&report.identity.suite_digest)
                || !hash(&report.identity.candidate_hash)
                || !hash(&report.identity.draft_digest)
                || !hash(&report.identity.model_config_digest)
                || report
                    .identity
                    .baseline_hash
                    .as_ref()
                    .is_some_and(|s| !hash(s))
                || report.identity.grader.len() > 64
                || report.reserved_at_ms == 0
                || report.identity.draft_id.len() > 128
                || report.agent_id.len() > 128
                || report.model.len() > 512
                || report.provider.as_ref().is_some_and(|p| p.len() > 128)
                || report.error.as_ref().is_some_and(|p| p.len() > 1024)
                || report.trials.iter().any(|t| {
                    t.response
                        .as_ref()
                        .is_some_and(|s| s.len() > anima_core::skill_evaluation::MAX_RESPONSE_BYTES)
                        || t.error.as_ref().is_some_and(|s| s.len() > 1024)
                        || t.fixture_id.len() > 64
                        || t.repetition > 2
                })
            {
                return Err("Invalid or oversized skill evaluation report".into());
            }
        }
        Ok(())
    }
    pub(crate) fn interrupt_active(&mut self, now: u64) {
        for report in &mut self.reports {
            if report.status == EvaluationStatus::Running {
                report.status = EvaluationStatus::Interrupted;
                report.error = Some(
                    "Evaluation interrupted; provider work is never replayed after restart".into(),
                );
                report.finished_at_ms = Some(now);
                report.usage_complete = false;
            }
        }
    }
    pub(crate) fn invalidate(&mut self, slug: &str) {
        for report in &mut self.reports {
            if report.identity.slug == slug
                && matches!(
                    report.status,
                    EvaluationStatus::Passed | EvaluationStatus::Running
                )
            {
                report.status = EvaluationStatus::Stale;
                report
                    .finished_at_ms
                    .get_or_insert_with(anima_core::primitives::now_millis);
                report.error =
                    Some("The skill or owner suite changed; this evidence is stale".into());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::{test_support::*, DraftApproval, SkillError};
    use anima_core::skill_evaluation::{EvaluationFixture, EvaluationOracle};
    use anima_core::{
        AgentConfig, Content, ModelAdapter, ModelGenerateRequest, ModelGenerateResponse,
        ModelStopReason, TokenUsage,
    };
    use async_trait::async_trait;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    #[derive(Default)]
    struct ResponseModel {
        calls: std::sync::Mutex<Vec<(AgentConfig, ModelGenerateRequest)>>,
        gate: std::sync::Mutex<Option<crate::agent_runs::test_support::Gate>>,
        fail_baseline: std::sync::atomic::AtomicBool,
        partial_usage_before_gate: std::sync::atomic::AtomicBool,
    }
    #[async_trait]
    impl ModelAdapter for ResponseModel {
        fn provider(&self) -> &str {
            "test"
        }
        async fn generate(
            &self,
            config: &AgentConfig,
            request: &ModelGenerateRequest,
        ) -> Result<ModelGenerateResponse, String> {
            self.calls
                .lock()
                .unwrap()
                .push((config.clone(), request.clone()));
            let gate = self.gate.lock().unwrap().clone();
            if let Some(gate) = gate {
                gate.entered.add_permits(1);
                gate.release.acquire().await.unwrap().forget();
            }
            if self.fail_baseline.load(std::sync::atomic::Ordering::SeqCst)
                && !request.system.contains("CANDIDATE_WORD")
            {
                return Err("provider unavailable".into());
            }
            Ok(ModelGenerateResponse {
                content: Content {
                    text: if request.system.contains("OVERSIZED_WORD") {
                        "x".repeat(anima_core::skill_evaluation::MAX_RESPONSE_BYTES + 1)
                    } else if request.system.contains("CANDIDATE_WORD") {
                        "correct".into()
                    } else {
                        "wrong".into()
                    },
                    ..Content::default()
                },
                tool_calls: None,
                stop_reason: if request.system.contains("TRUNCATED_WORD") {
                    ModelStopReason::MaxTokens
                } else if request.system.contains("TOOLS_WORD") {
                    ModelStopReason::ToolCall
                } else {
                    ModelStopReason::End
                },
                usage: TokenUsage {
                    prompt_tokens: 10,
                    completion_tokens: 1,
                    total_tokens: 11,
                    ..TokenUsage::default()
                },
            })
        }

        async fn stream(
            &self,
            config: &AgentConfig,
            request: &ModelGenerateRequest,
            sink: &dyn anima_core::ModelStreamSink,
        ) -> Result<(), String> {
            if self
                .partial_usage_before_gate
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                sink.emit(anima_core::ModelStreamFrame::Usage(TokenUsage {
                    prompt_tokens: 2,
                    completion_tokens: 1,
                    total_tokens: 3,
                    ..TokenUsage::default()
                }))
                .await?;
            }
            let response = self.generate(config, request).await?;
            sink.emit(anima_core::ModelStreamFrame::Final(response))
                .await
        }
    }
    async fn evaluated_daemon(
        label: &str,
    ) -> (
        crate::app::SharedDaemonState,
        super::super::SkillService,
        Arc<ResponseModel>,
        String,
        std::path::PathBuf,
    ) {
        let root = temp_workspace(label);
        let model = Arc::new(ResponseModel::default());
        let mut daemon = with_workspace(
            crate::state::DaemonState::with_model_adapter(model.clone()),
            &root,
        );
        let agent = daemon
            .create_agent(crate::agent_runs::test_support::companion_config("Eval"))
            .unwrap()
            .state
            .id;
        daemon.set_control_plane_store(Some(
            crate::control_plane_store::ControlPlaneStoreConfig::Json(
                root.join("evaluation-control.json"),
            ),
        ));
        daemon.skill_evaluations.suites.insert(
            "notes".into(),
            SuiteRecord {
                workspace: root.to_string_lossy().into_owned(),
                revision: 1,
                suite: suite(&agent),
            },
        );
        let state = Arc::new(RwLock::new(daemon));
        let skills = service(&state);
        (state, skills, model, agent, root)
    }
    async fn physically_retired(state: &crate::app::SharedDaemonState) {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if state
                    .read()
                    .await
                    .skill_evaluation_slots
                    .available_permits()
                    == 1
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }

    pub(super) fn suite(agent: &str) -> EvaluationSuite {
        EvaluationSuite {
            enforced: true,
            agent_id: agent.into(),
            fixtures: vec![
                EvaluationFixture {
                    id: "train".into(),
                    prompt: "question 1".into(),
                    heldout: false,
                    required: true,
                    oracle: EvaluationOracle::ExactText("correct".into()),
                },
                EvaluationFixture {
                    id: "heldout".into(),
                    prompt: "question 2".into(),
                    heldout: true,
                    required: true,
                    oracle: EvaluationOracle::ExactText("correct".into()),
                },
            ],
            min_improvement: 1,
            trials_per_fixture: 1,
            max_output_tokens: 100,
            trial_timeout_ms: 1000,
        }
    }
    #[tokio::test]
    async fn skill_evaluation_enforced_suite_gates_import_draft_without_evidence() {
        let root = temp_workspace("evaluation-import-red");
        let state = Arc::new(RwLock::new(with_workspace(
            crate::state::DaemonState::new(),
            &root,
        )));
        let skills = service(&state);
        let draft = skills
            .import(skill_text("Notes").into_bytes(), None)
            .await
            .unwrap();
        state.write().await.skill_evaluations.suites.insert(
            "notes".into(),
            SuiteRecord {
                workspace: root.to_string_lossy().into_owned(),
                revision: 1,
                suite: suite("missing-agent"),
            },
        );
        assert!(
            matches!(
                skills
                    .approve_draft(&draft.id, DraftApproval::default())
                    .await,
                Err(SkillError::Conflict(_)) | Err(SkillError::Unavailable(_))
            ),
            "an enforced suite cannot silently promote without evaluation"
        );
        assert!(!root.join("skills/notes/SKILL.md").exists());
    }
    #[tokio::test]
    async fn skill_evaluation_enforced_suite_gates_early_file_draft_branch() {
        let root = temp_workspace("evaluation-file-red");
        let state = Arc::new(RwLock::new(with_workspace(
            crate::state::DaemonState::new(),
            &root,
        )));
        let skills = service(&state);
        let bytes = skill_text("Notes");
        write_skill(&root, "notes", &bytes);
        state.write().await.skill_evaluations.suites.insert(
            "notes".into(),
            SuiteRecord {
                workspace: root.to_string_lossy().into_owned(),
                revision: 1,
                suite: suite("missing-agent"),
            },
        );
        let result = skills
            .approve_draft(
                "file:notes",
                DraftApproval {
                    hash: Some(crate::skills::skill_hash(bytes.as_bytes())),
                    ..DraftApproval::default()
                },
            )
            .await;
        assert!(
            matches!(
                result,
                Err(SkillError::Conflict(_)) | Err(SkillError::Unavailable(_))
            ),
            "file drafts must pass the same evaluation authority"
        );
        assert!(state.read().await.skills.get("notes").is_none());
    }
    #[tokio::test]
    async fn quality_skill_evaluation_compares_whole_effective_package_without_oracle_or_tools() {
        let (state, skills, model, _agent, root) = evaluated_daemon("evaluation-run").await;
        let draft = skills
            .import(skill_text("Notes").into_bytes(), None)
            .await
            .unwrap();
        let approval = DraftApproval {
            files: Some(vec![super::super::SkillSupportingFile {
                path: "references/answer.txt".into(),
                content: "CANDIDATE_WORD".into(),
            }]),
            ..DraftApproval::default()
        };
        let row = skills
            .start_evaluation(&draft.id, approval.clone())
            .await
            .unwrap();
        let report = skills.wait_evaluation(&row.id).await.unwrap();
        assert_eq!(report.status, EvaluationStatus::Passed);
        assert_eq!(report.trials.len(), 4);
        assert!(report.response_only && report.usage_complete);
        assert_eq!(model.calls.lock().unwrap().len(), 4);
        for (config, request) in model.calls.lock().unwrap().iter() {
            assert!(config.tools.as_ref().is_none_or(Vec::is_empty));
            assert!(config.plugins.as_ref().is_none_or(Vec::is_empty));
            assert!(config.fallbacks.is_empty());
            assert_eq!(request.messages.len(), 1);
            assert!(!request.system.contains("correct"));
            assert!(!request.messages[0].content.text.contains("correct"));
        }
        skills.approve_draft(&draft.id, approval).await.unwrap();
        assert_eq!(
            model.calls.lock().unwrap().len(),
            4,
            "approval reuses exact current evidence"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("skills/notes/references/answer.txt")).unwrap(),
            "CANDIDATE_WORD"
        );
        assert!(state.read().await.skills.get("notes").is_some());
    }
    #[tokio::test]
    async fn quality_skill_evaluation_missing_approved_baseline_refuses_before_provider_work() {
        let (_state, skills, model, _agent, root) = evaluated_daemon("evaluation-baseline").await;
        skills.save("notes", content("Notes")).await.unwrap();
        let draft = skills
            .import(skill_text("Notes").into_bytes(), None)
            .await
            .unwrap();
        std::fs::write(root.join("skills/notes/SKILL.md"), skill_text("Changed")).unwrap();
        assert!(skills
            .start_evaluation(&draft.id, DraftApproval::default())
            .await
            .is_err());
        assert!(model.calls.lock().unwrap().is_empty());
    }
    #[tokio::test]
    async fn quality_skill_evaluation_restart_interrupts_active_and_keeps_suite_and_quota() {
        let (state, skills, _model, _agent, _) = evaluated_daemon("evaluation-restore").await;
        let draft = skills
            .import(skill_text("Notes").into_bytes(), None)
            .await
            .unwrap();
        let row = skills
            .start_evaluation(&draft.id, DraftApproval::default())
            .await
            .unwrap();
        let mut snapshot = state.read().await.control_plane_snapshot();
        snapshot
            .skill_evaluations
            .reports
            .iter_mut()
            .find(|r| r.id == row.id)
            .unwrap()
            .status = EvaluationStatus::Running;
        let mut restored = crate::state::DaemonState::new();
        restored.restore_control_plane_snapshot(snapshot).unwrap();
        assert_eq!(restored.skill_evaluations.suites.len(), 1);
        assert_eq!(restored.skill_evaluations.reservations.len(), 1);
        assert_eq!(
            restored
                .skill_evaluations
                .reports
                .iter()
                .find(|r| r.id == row.id)
                .unwrap()
                .status,
            EvaluationStatus::Interrupted
        );
    }
    #[tokio::test]
    async fn quality_skill_evaluation_cancel_and_caller_drop_retain_physical_worker_custody() {
        let (state, skills, model, _agent, _) = evaluated_daemon("evaluation-custody").await;
        let gate = crate::agent_runs::test_support::Gate::new();
        *model.gate.lock().unwrap() = Some(gate.clone());
        let draft = skills
            .import(skill_text("Notes").into_bytes(), None)
            .await
            .unwrap();
        let row = skills
            .start_evaluation(&draft.id, DraftApproval::default())
            .await
            .unwrap();
        gate.entered().await;
        let waiting = skills.clone();
        let id = row.id.clone();
        let caller = tokio::spawn(async move { waiting.wait_evaluation(&id).await });
        caller.abort();
        skills.cancel_evaluation(&row.id).await.unwrap();
        assert_eq!(
            state
                .read()
                .await
                .skill_evaluation_slots
                .available_permits(),
            0
        );
        assert!(
            skills
                .start_evaluation(&draft.id, DraftApproval::default())
                .await
                .is_err(),
            "cancel cannot free a transport that is still physically running"
        );
        gate.release();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if state
                    .read()
                    .await
                    .skill_evaluation_slots
                    .available_permits()
                    == 1
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let report = skills.wait_evaluation(&row.id).await.unwrap();
        assert_eq!(report.status, EvaluationStatus::Interrupted);
        assert_eq!(report.trials.len(), 1);
        assert!(report.retired_at_ms.is_some());
        assert_eq!(model.calls.lock().unwrap().len(), 1);
    }
    #[tokio::test]
    async fn quality_skill_evaluation_provider_error_tool_request_truncation_and_oversize_never_pass(
    ) {
        for word in [
            "CANDIDATE_WORD TOOLS_WORD",
            "CANDIDATE_WORD TRUNCATED_WORD",
            "CANDIDATE_WORD OVERSIZED_WORD",
        ] {
            let (_state, skills, _model, _agent, _) =
                evaluated_daemon("evaluation-invalid-output").await;
            let draft = skills
                .import(skill_text("Notes").into_bytes(), None)
                .await
                .unwrap();
            let row = skills
                .start_evaluation(
                    &draft.id,
                    DraftApproval {
                        body: Some(word.into()),
                        ..DraftApproval::default()
                    },
                )
                .await
                .unwrap();
            assert_eq!(
                skills.wait_evaluation(&row.id).await.unwrap().status,
                EvaluationStatus::Inconclusive
            );
        }
        let (_state, skills, model, _agent, _) =
            evaluated_daemon("evaluation-provider-error").await;
        model
            .fail_baseline
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let draft = skills
            .import(skill_text("Notes").into_bytes(), None)
            .await
            .unwrap();
        let row = skills
            .start_evaluation(
                &draft.id,
                DraftApproval {
                    body: Some("CANDIDATE_WORD".into()),
                    ..DraftApproval::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(
            skills.wait_evaluation(&row.id).await.unwrap().status,
            EvaluationStatus::Inconclusive
        );
        assert_eq!(model.calls.lock().unwrap().len(), 1);
    }
    #[tokio::test]
    async fn quality_skill_evaluation_late_results_cannot_promote_changed_identity() {
        let (state, skills, model, _agent, _) = evaluated_daemon("evaluation-stale").await;
        let gate = crate::agent_runs::test_support::Gate::new();
        *model.gate.lock().unwrap() = Some(gate.clone());
        let draft = skills
            .import(skill_text("Notes").into_bytes(), None)
            .await
            .unwrap();
        let row = skills
            .start_evaluation(
                &draft.id,
                DraftApproval {
                    body: Some("CANDIDATE_WORD".into()),
                    ..DraftApproval::default()
                },
            )
            .await
            .unwrap();
        gate.entered().await;
        state
            .write()
            .await
            .skill_evaluations
            .suites
            .get_mut("notes")
            .unwrap()
            .revision += 1;
        gate.release();
        let report = skills.wait_evaluation(&row.id).await.unwrap();
        assert_eq!(report.status, EvaluationStatus::Stale);
        assert!(state.read().await.skills.get("notes").is_none());
        assert_eq!(model.calls.lock().unwrap().len(), 1);
    }
    #[tokio::test]
    async fn quality_skill_evaluation_suite_enabled_after_preflight_is_checked_in_final_transaction(
    ) {
        let (state, skills, _model, agent, root) = evaluated_daemon("evaluation-final-gate").await;
        state.write().await.skill_evaluations.suites.clear();
        let draft = skills
            .import(skill_text("Notes").into_bytes(), None)
            .await
            .unwrap();
        let held = skills.transactions.clone().lock_owned().await;
        let approving = skills.clone();
        let id = draft.id.clone();
        let held_count = Arc::strong_count(&skills.transactions);
        let caller =
            tokio::spawn(
                async move { approving.approve_draft(&id, DraftApproval::default()).await },
            );
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while Arc::strong_count(&skills.transactions) <= held_count {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        state.write().await.skill_evaluations.suites.insert(
            "notes".into(),
            SuiteRecord {
                workspace: root.to_string_lossy().into_owned(),
                revision: 1,
                suite: suite(&agent),
            },
        );
        drop(held);
        assert!(caller.await.unwrap().is_err());
        assert!(!root.join("skills/notes/SKILL.md").exists());
    }
    #[tokio::test]
    async fn quality_skill_evaluation_persists_reservation_before_dispatch_and_never_refunds_failure(
    ) {
        let (state, skills, model, _agent, _) = evaluated_daemon("evaluation-reservation").await;
        let draft = skills
            .import(skill_text("Notes").into_bytes(), None)
            .await
            .unwrap();
        state
            .write()
            .await
            .set_control_plane_store(Some(broken_store()));
        assert!(skills
            .start_evaluation(&draft.id, DraftApproval::default())
            .await
            .is_err());
        assert!(model.calls.lock().unwrap().is_empty());
        assert_eq!(state.read().await.skill_evaluations.reservations.len(), 1);
    }
    #[tokio::test]
    async fn quality_skill_evaluation_durable_daily_quota_bounds_explicit_owner_retries() {
        let (state, skills, model, _agent, _) = evaluated_daemon("evaluation-quota").await;
        model
            .fail_baseline
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let draft = skills
            .import(skill_text("Notes").into_bytes(), None)
            .await
            .unwrap();
        for _ in 0..MAX_DAILY_EVALUATIONS {
            let row = skills
                .start_evaluation(&draft.id, DraftApproval::default())
                .await
                .unwrap();
            assert_eq!(
                skills.wait_evaluation(&row.id).await.unwrap().status,
                EvaluationStatus::Inconclusive
            );
            physically_retired(&state).await;
        }
        assert!(skills
            .start_evaluation(&draft.id, DraftApproval::default())
            .await
            .is_err());
        assert_eq!(
            state.read().await.skill_evaluations.reservations.len(),
            MAX_DAILY_EVALUATIONS
        );
        assert_eq!(model.calls.lock().unwrap().len(), MAX_DAILY_EVALUATIONS);
    }
    #[tokio::test]
    async fn quality_skill_evaluation_preview_evidence_grants_no_approval_and_owner_save_invalidates_it(
    ) {
        let (state, skills, model, _agent, _) = evaluated_daemon("evaluation-authoring").await;
        let draft = skills
            .import(skill_text("Notes").into_bytes(), None)
            .await
            .unwrap();
        let row = skills
            .start_evaluation(
                &draft.id,
                DraftApproval {
                    body: Some("CANDIDATE_WORD".into()),
                    ..DraftApproval::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(
            skills.wait_evaluation(&row.id).await.unwrap().status,
            EvaluationStatus::Passed
        );
        assert!(state.read().await.skills.get("notes").is_none());
        skills.save("notes", content("Notes")).await.unwrap();
        assert_eq!(
            state
                .read()
                .await
                .skill_evaluations
                .reports
                .iter()
                .find(|r| r.id == row.id)
                .unwrap()
                .status,
            EvaluationStatus::Stale
        );
        assert_eq!(model.calls.lock().unwrap().len(), 4);
    }
    #[tokio::test]
    async fn quality_skill_evaluation_deadline_retires_facade_while_transport_and_late_usage_remain_owned(
    ) {
        let (state, skills, model, _agent, _) = evaluated_daemon("evaluation-timeout").await;
        let gate = crate::agent_runs::test_support::Gate::new();
        *model.gate.lock().unwrap() = Some(gate.clone());
        let draft = skills
            .import(skill_text("Notes").into_bytes(), None)
            .await
            .unwrap();
        let row = skills
            .start_evaluation(&draft.id, DraftApproval::default())
            .await
            .unwrap();
        gate.entered().await;
        let timed_out = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            skills.wait_evaluation(&row.id),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(timed_out.status, EvaluationStatus::Inconclusive);
        assert!(!timed_out.usage_complete);
        assert!(!timed_out.trials[0].usage_reported);
        assert_eq!(
            state
                .read()
                .await
                .skill_evaluation_slots
                .available_permits(),
            0
        );
        gate.release();
        physically_retired(&state).await;
        let late = skills.wait_evaluation(&row.id).await.unwrap();
        assert_eq!(late.status, EvaluationStatus::Inconclusive);
        assert!(late.trials[0].usage_reported);
        assert_eq!(late.trials[0].usage.total_tokens, 11);
        assert!(late.retired_at_ms.is_some());
        assert_eq!(model.calls.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn quality_skill_evaluation_partial_timeout_accounts_final_usage_once_after_physical_retirement(
    ) {
        let (state, skills, model, agent, _) = evaluated_daemon("evaluation-partial-usage").await;
        let store = Arc::new(crate::history::conformance::FlakyHistoryStore::new());
        store.set_usage_failing(true);
        let history = crate::history::HistoryService::new(store.clone());
        state.write().await.set_history(history.clone());
        model
            .partial_usage_before_gate
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let gate = crate::agent_runs::test_support::Gate::new();
        *model.gate.lock().unwrap() = Some(gate.clone());
        let draft = skills
            .import(skill_text("Notes").into_bytes(), None)
            .await
            .unwrap();
        let row = skills
            .start_evaluation(&draft.id, DraftApproval::default())
            .await
            .unwrap();
        gate.entered().await;
        let timed_out = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            skills.wait_evaluation(&row.id),
        )
        .await
        .unwrap()
        .unwrap();
        let premature_global_usage = history.pending_usage();
        gate.release();
        physically_retired(&state).await;

        assert_eq!(timed_out.status, EvaluationStatus::Inconclusive);
        assert_eq!(timed_out.trials[0].usage.total_tokens, 3);
        assert!(!timed_out.trials[0].usage_complete);
        assert!(
            premature_global_usage.is_empty(),
            "logical timeout evidence must not settle a physical call's global accounting"
        );
        let final_report = skills.wait_evaluation(&row.id).await.unwrap();
        assert_eq!(final_report.trials[0].usage.total_tokens, 11);
        assert!(final_report.trials[0].usage_complete);
        let pending = history.pending_usage();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, format!("{}_train_Baseline_0", row.id));
        assert_eq!(pending[0].total_tokens, 11);
        assert!(history
            .flush_once(
                &state,
                &skills.transactions,
                anima_core::primitives::now_millis()
            )
            .await
            .is_err());
        assert_eq!(history.pending_usage(), pending);
        store.set_usage_failing(false);
        history
            .flush_once(
                &state,
                &skills.transactions,
                anima_core::primitives::now_millis(),
            )
            .await
            .unwrap();
        let rows = history
            .store()
            .page_usage(&crate::history::UsagePageQuery {
                from_ms: 0,
                to_ms: u64::MAX >> 1,
                agent_id: Some(agent),
                session_id: None,
                before: None,
                limit: 20,
            })
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].total_tokens, 11);
        assert_eq!(model.calls.lock().unwrap().len(), 1);
    }
    #[tokio::test]
    async fn quality_skill_evaluation_closed_host_admission_refuses_new_reservation() {
        let (state, skills, model, _agent, _) = evaluated_daemon("evaluation-admission").await;
        let draft = skills
            .import(skill_text("Notes").into_bytes(), None)
            .await
            .unwrap();
        let closed = skills.with_evaluation_admission(Arc::new(|| false));
        assert!(closed
            .start_evaluation(&draft.id, DraftApproval::default())
            .await
            .is_err());
        assert!(state.read().await.skill_evaluations.reservations.is_empty());
        assert!(model.calls.lock().unwrap().is_empty());
    }
    #[tokio::test]
    async fn quality_skill_evaluation_transport_timeout_is_disabled_under_own_custody() {
        let (state, skills, model, agent, _) =
            evaluated_daemon("evaluation-transport-timeout").await;
        let mut config = state.read().await.agents[&agent].config().clone();
        config.settings.as_mut().unwrap().timeout_ms = Some(200);
        state
            .write()
            .await
            .agents
            .get_mut(&agent)
            .unwrap()
            .replace_config(config);
        let draft = skills
            .import(skill_text("Notes").into_bytes(), None)
            .await
            .unwrap();
        let row = skills
            .start_evaluation(&draft.id, DraftApproval::default())
            .await
            .unwrap();
        skills.wait_evaluation(&row.id).await.unwrap();
        for (config, _) in model.calls.lock().unwrap().iter() {
            assert_eq!(
                config.settings.as_ref().unwrap().timeout_ms,
                None,
                "RuntimeModelAdapter must not timeout and drop the underlying provider future"
            );
        }
    }
    #[tokio::test]
    async fn quality_skill_evaluation_transport_timeout_keeps_smaller_owner_deadline_logical() {
        let (state, skills, model, agent, _) = evaluated_daemon("evaluation-owner-timeout").await;
        let mut config = state.read().await.agents[&agent].config().clone();
        config.settings.as_mut().unwrap().timeout_ms = Some(200);
        state
            .write()
            .await
            .agents
            .get_mut(&agent)
            .unwrap()
            .replace_config(config);
        let gate = crate::agent_runs::test_support::Gate::new();
        *model.gate.lock().unwrap() = Some(gate.clone());
        let draft = skills
            .import(skill_text("Notes").into_bytes(), None)
            .await
            .unwrap();
        let row = skills
            .start_evaluation(&draft.id, DraftApproval::default())
            .await
            .unwrap();
        gate.entered().await;
        let logical = tokio::time::timeout(
            std::time::Duration::from_millis(750),
            skills.wait_evaluation(&row.id),
        )
        .await;
        let still_held = state
            .read()
            .await
            .skill_evaluation_slots
            .available_permits()
            == 0;
        gate.release();
        physically_retired(&state).await;
        assert!(
            logical.is_ok(),
            "The owner's 200ms deadline must settle the facade before the suite's 1000ms cap"
        );
        assert_eq!(
            logical.unwrap().unwrap().status,
            EvaluationStatus::Inconclusive
        );
        assert!(
            still_held,
            "Logical timeout must retain physical provider custody"
        );
    }
    #[tokio::test]
    async fn quality_skill_evaluation_unsupported_subscription_transport_refuses_before_quota_or_dispatch(
    ) {
        let (state, skills, model, agent, _) =
            evaluated_daemon("evaluation-unsupported-transport").await;
        let mut config = state.read().await.agents[&agent].config().clone();
        config.provider = Some("chatgpt".into());
        state
            .write()
            .await
            .agents
            .get_mut(&agent)
            .unwrap()
            .replace_config(config);
        let draft = skills
            .import(skill_text("Notes").into_bytes(), None)
            .await
            .unwrap();
        assert!(
            skills
                .start_evaluation(&draft.id, DraftApproval::default())
                .await
                .is_err(),
            "A transport that cannot enforce the requested output-token bound is ineligible"
        );
        assert!(state.read().await.skill_evaluations.reservations.is_empty());
        assert!(state.read().await.skill_evaluations.reports.is_empty());
        assert!(model.calls.lock().unwrap().is_empty());
    }
    #[tokio::test]
    async fn quality_skill_evaluation_report_read_detects_current_model_change() {
        let (state, skills, _model, agent, _) = evaluated_daemon("evaluation-report-model").await;
        let draft = skills
            .import(skill_text("Notes").into_bytes(), None)
            .await
            .unwrap();
        let row = skills
            .start_evaluation(
                &draft.id,
                DraftApproval {
                    body: Some("CANDIDATE_WORD".into()),
                    ..DraftApproval::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(
            skills.wait_evaluation(&row.id).await.unwrap().status,
            EvaluationStatus::Passed
        );
        physically_retired(&state).await;
        assert_eq!(
            skills
                .draft_evaluation_report(&draft.id)
                .await
                .unwrap()
                .unwrap()
                .status,
            EvaluationStatus::Passed
        );
        let mut config = state.read().await.agents[&agent].config().clone();
        config.model = "changed-model".into();
        state
            .write()
            .await
            .agents
            .get_mut(&agent)
            .unwrap()
            .replace_config(config);
        assert_eq!(
            skills
                .draft_evaluation_report(&draft.id)
                .await
                .unwrap()
                .unwrap()
                .status,
            EvaluationStatus::Stale
        );
    }
    #[tokio::test]
    async fn quality_skill_evaluation_report_read_detects_disk_baseline_change() {
        let (state, skills, _model, _agent, root) =
            evaluated_daemon("evaluation-report-baseline").await;
        skills.save("notes", content("Notes")).await.unwrap();
        let draft = skills
            .import(skill_text("Notes").into_bytes(), None)
            .await
            .unwrap();
        let row = skills
            .start_evaluation(
                &draft.id,
                DraftApproval {
                    body: Some("CANDIDATE_WORD".into()),
                    ..DraftApproval::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(
            skills.wait_evaluation(&row.id).await.unwrap().status,
            EvaluationStatus::Passed
        );
        physically_retired(&state).await;
        write_skill(&root, "notes", &skill_text("External change"));
        assert_eq!(
            skills
                .draft_evaluation_report(&draft.id)
                .await
                .unwrap()
                .unwrap()
                .status,
            EvaluationStatus::Stale
        );
    }
    #[tokio::test]
    async fn quality_skill_evaluation_report_read_preserves_owner_override_and_checks_file_source()
    {
        let (state, skills, _model, _agent, root) =
            evaluated_daemon("evaluation-report-file-source").await;
        let bytes = skill_text("Notes");
        write_skill(&root, "notes", &bytes);
        let row = skills
            .start_evaluation(
                "file:notes",
                DraftApproval {
                    body: Some("CANDIDATE_WORD".into()),
                    hash: Some(crate::skills::skill_hash(bytes.as_bytes())),
                    ..DraftApproval::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(
            skills.wait_evaluation(&row.id).await.unwrap().status,
            EvaluationStatus::Passed
        );
        physically_retired(&state).await;
        assert_eq!(skills.draft_evaluation_report("file:notes").await.unwrap().unwrap().status, EvaluationStatus::Passed, "The report's legitimate owner-edited candidate must not be replaced with a default approval for freshness checks");
        write_skill(&root, "notes", &skill_text("External change"));
        assert_eq!(
            skills
                .draft_evaluation_report("file:notes")
                .await
                .unwrap()
                .unwrap()
                .status,
            EvaluationStatus::Stale
        );
    }
    #[tokio::test]
    async fn quality_skill_evaluation_owner_controls_are_no_store_strict_and_disabled_suite_preserves_legacy(
    ) {
        use axum::{body::Body, http::Request};
        use tower::ServiceExt;
        let (state, skills, model, agent, _) = evaluated_daemon("evaluation-routes").await;
        let app = crate::app::app_with_state(state.clone(), crate::app::DaemonConfig::default());
        let request = |method: &str, path: &str, origin: &str, body: serde_json::Value| {
            Request::builder()
                .method(method)
                .uri(path)
                .header("host", "127.0.0.1:8080")
                .header("origin", origin)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap()
        };
        for (method, path) in [
            ("GET", "/api/skills/notes/evaluation-suite"),
            ("PUT", "/api/skills/notes/evaluation-suite"),
            ("GET", "/api/skill-drafts/skd_x/evaluation"),
            ("POST", "/api/skill-drafts/skd_x/evaluation"),
            ("POST", "/api/skill-drafts/skd_x/evaluation/cancel"),
        ] {
            let response = app
                .clone()
                .oneshot(request(
                    method,
                    path,
                    "https://untrusted.example",
                    serde_json::json!({}),
                ))
                .await
                .unwrap();
            assert_eq!(response.status(), axum::http::StatusCode::FORBIDDEN);
            assert_eq!(response.headers()["cache-control"], "no-store");
        }
        let mut invalid = serde_json::to_value(suite(&agent)).unwrap();
        invalid["unrecognized"] = serde_json::json!(true);
        let response = app
            .clone()
            .oneshot(request(
                "PUT",
                "/api/skills/notes/evaluation-suite",
                "http://localhost:4200",
                invalid,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
        assert_eq!(response.headers()["cache-control"], "no-store");
        assert_eq!(
            state.read().await.skill_evaluations.suites["notes"].revision,
            1
        );
        let response = app
            .clone()
            .oneshot(request(
                "POST",
                "/api/skill-drafts/skd_x/evaluation",
                "http://localhost:4200",
                serde_json::json!({"unrecognized": true}),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
        let mut disabled = suite(&agent);
        disabled.enforced = false;
        let response = app
            .clone()
            .oneshot(request(
                "PUT",
                "/api/skills/notes/evaluation-suite",
                "http://localhost:4200",
                serde_json::to_value(disabled).unwrap(),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        assert_eq!(response.headers()["cache-control"], "no-store");
        let draft = skills
            .import(skill_text("Notes").into_bytes(), None)
            .await
            .unwrap();
        skills
            .approve_draft(&draft.id, DraftApproval::default())
            .await
            .unwrap();
        assert!(model.calls.lock().unwrap().is_empty());
    }
}
