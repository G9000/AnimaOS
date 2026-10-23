//! A detached custodian owns the physical slot through adapter retirement.
//! Deadlines and cancellation settle the facade; they never drop model work.
use super::evaluation::*;
use super::evaluation_service::{identity_current, CapturedEvaluation};
use super::{DraftApproval, SkillError, SkillService};
use anima_core::primitives::now_millis;
use anima_core::skill_evaluation::{
    grade, EvaluationTrial, EvaluationVerdict, TrialVariant, MAX_RESPONSE_BYTES,
};
use anima_core::{
    AgentConfig, Content, Message, MessageRole, ModelGenerateRequest, ModelGenerateResponse,
    ModelStopReason, ModelStreamFrame, ModelStreamSink, TokenUsage,
};
use async_trait::async_trait;
use futures::FutureExt;
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

fn bounded(mut text: String) -> String {
    while text.len() > 1024 {
        text.pop();
    }
    text
}
fn unavailable(message: &str) -> SkillError {
    SkillError::Unavailable(message.into())
}

impl SkillService {
    async fn evaluation_transaction<T, F, Fut>(&self, work: F) -> Result<T, SkillError>
    where
        T: Send + 'static,
        F: FnOnce(SkillService) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<T, SkillError>> + Send + 'static,
    {
        let service = self.clone();
        tokio::spawn(async move {
            let _transaction = service.transactions.clone().lock_owned().await;
            work(service).await
        })
        .await
        .unwrap_or_else(|_| Err(unavailable("Evaluation transaction exited unexpectedly")))
    }
    /// An explicit owner preview may retry a terminal result, consuming quota.
    pub(crate) async fn start_evaluation(
        &self,
        id: &str,
        approval: DraftApproval,
    ) -> Result<EvaluationReport, SkillError> {
        self.start_or_join_evaluation(id, approval, true).await
    }

    pub(super) async fn start_or_join_evaluation(
        &self,
        id: &str,
        approval: DraftApproval,
        force: bool,
    ) -> Result<EvaluationReport, SkillError> {
        let captured = self.capture_evaluation(id, &approval).await?;
        let id = id.to_owned();
        self.locked(move |service, _root| async move {
            let mut state = service.state.write().await;
            if !identity_current(&state, &captured.identity) { return Err(SkillError::conflict("Evaluation source changed before admission")); }
            if let Some(row) = state.skill_evaluations.reports.iter().rev().find(|r| r.identity == captured.identity) {
                if row.status == EvaluationStatus::Running || !force { return Ok(row.clone()); }
            }
            // Joining already admitted work is allowed during drain; new
            // reservations must pass the host's admission probe.
            if service.evaluation_admission.as_ref().is_some_and(|probe| !probe()) {
                return Err(SkillError::conflict("The daemon is draining; evaluation admission is closed"));
            }
            if state.control_plane_store.is_none() || state.skill_evaluation_slots.is_closed() { return Err(unavailable("Durable evaluation admission is unavailable")); }
            let permit = state.skill_evaluation_slots.clone().try_acquire_owned().map_err(|_| SkillError::conflict("A skill evaluation transport is still running"))?;
            let now = now_millis().max(1);
            let registry = &mut state.skill_evaluations;
            registry.reservations.retain(|at| *at > now.saturating_sub(24 * 60 * 60 * 1000));
            if registry.reservations.len() >= MAX_DAILY_EVALUATIONS { return Err(SkillError::conflict("The eight-per-day skill evaluation quota is exhausted")); }
            if registry.reports.len() >= MAX_EVALUATION_REPORTS {
                let oldest = registry.reports.iter().position(|r| r.status != EvaluationStatus::Running).ok_or_else(|| SkillError::conflict("Evaluation report capacity reached"))?;
                registry.reports.remove(oldest);
            }
            let row = EvaluationReport {
                id: format!("ske_{}", uuid::Uuid::new_v4()), identity: captured.identity.clone(), agent_id: captured.suite.suite.agent_id.clone(),
                model: captured.config.model.clone(), provider: captured.config.provider.clone(), status: EvaluationStatus::Running,
                reserved_at_ms: now, finished_at_ms: None, retired_at_ms: None, trials: Vec::new(), error: None, response_only: true, usage_complete: false,
            };
            registry.reservations.push(now);
            registry.reports.push(row.clone());
            let persist = state.control_plane_persist_request(); drop(state);
            if let Err(error) = persist.save().await {
                // Never refund a possibly committed quota reservation. No
                // provider work begins until the save is acknowledged.
                if let Some(current) = service.state.write().await.skill_evaluations.reports.iter_mut().find(|r| r.id == row.id) {
                    current.status = EvaluationStatus::Inconclusive;
                    current.finished_at_ms = Some(now_millis()); current.retired_at_ms = current.finished_at_ms;
                    current.error = Some("Evaluation admission storage could not be acknowledged; no provider work was dispatched".into());
                }
                return Err(unavailable(&bounded(error.to_string())));
            }
            let worker = service.clone(); let row_id = row.id.clone();
            // Spawn inside the detached admission transaction: dropping the
            // HTTP/approval caller cannot strand a committed reservation.
            tokio::spawn(async move {
                worker.execute_evaluation(row_id, id, approval, captured, permit).await;
            });
            Ok(row)
        }).await
    }

    /// Bounded logical wait only. The physical custodian can outlive it.
    pub(crate) async fn wait_evaluation(&self, id: &str) -> Result<EvaluationReport, SkillError> {
        let deadline =
            Instant::now() + Duration::from_millis(RUN_TIMEOUT_MS + super::SKILL_IO_TIMEOUT_MS);
        loop {
            let row = self
                .state
                .read()
                .await
                .skill_evaluations
                .reports
                .iter()
                .find(|r| r.id == id)
                .cloned()
                .ok_or(SkillError::NotFound)?;
            if row.status != EvaluationStatus::Running {
                return Ok(row);
            }
            if Instant::now() >= deadline {
                self.finish_evaluation(
                    id,
                    EvaluationStatus::Inconclusive,
                    "Evaluation exceeded its absolute result deadline",
                )
                .await?;
            } else {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }
    }
    pub(crate) async fn cancel_evaluation(&self, id: &str) -> Result<EvaluationReport, SkillError> {
        self.finish_evaluation(
            id,
            EvaluationStatus::Interrupted,
            "Evaluation cancelled; provider custody remains until transport retirement",
        )
        .await
    }
    async fn finish_evaluation(
        &self,
        id: &str,
        status: EvaluationStatus,
        error: &str,
    ) -> Result<EvaluationReport, SkillError> {
        let (id, error) = (id.to_owned(), bounded(error.to_owned()));
        self.evaluation_transaction(move |service| async move {
            let mut state = service.state.write().await;
            let row = state
                .skill_evaluations
                .reports
                .iter_mut()
                .find(|r| r.id == id)
                .ok_or(SkillError::NotFound)?;
            if row.status == EvaluationStatus::Running {
                row.status = status;
                row.error = Some(error);
                row.finished_at_ms = Some(now_millis());
                row.usage_complete = false;
            }
            let result = row.clone();
            let persist = state.control_plane_persist_request();
            drop(state);
            persist
                .save()
                .await
                .map_err(|e| unavailable(&bounded(e.to_string())))?;
            Ok(result)
        })
        .await
    }

    async fn active_evaluation(&self, id: &str, identity: &EvaluationIdentity) -> bool {
        let state = self.state.read().await;
        !state.skill_evaluation_slots.is_closed()
            && identity_current(&state, identity)
            && state
                .skill_evaluations
                .reports
                .iter()
                .any(|r| r.id == id && r.status == EvaluationStatus::Running)
    }

    async fn execute_evaluation(
        &self,
        report_id: String,
        draft_id: String,
        approval: DraftApproval,
        captured: CapturedEvaluation,
        _physical: tokio::sync::OwnedSemaphorePermit,
    ) {
        let absolute = Instant::now() + Duration::from_millis(RUN_TIMEOUT_MS);
        let mut config = captured.config.clone();
        config.tools = None;
        config.plugins = None;
        config.fallbacks.clear();
        config.knowledge = None;
        config.bio = None;
        config.lore = None;
        let settings = config.settings.get_or_insert_with(Default::default);
        settings.max_retries = Some(0);
        settings.max_tool_iterations = Some(0);
        settings.max_tokens = Some(
            settings
                .max_tokens
                .unwrap_or(captured.suite.suite.max_output_tokens)
                .min(captured.suite.suite.max_output_tokens),
        );
        // RuntimeModelAdapter interprets Some(timeout_ms) as a dropping
        // timeout around the underlying provider future. Keep the owner's
        // smaller bound as our logical deadline while disabling that wrapper
        // on this sterile copy, so only actual transport retirement frees the
        // physical permit.
        let logical_timeout_ms = settings
            .timeout_ms
            .take()
            .unwrap_or(captured.suite.suite.trial_timeout_ms)
            .min(captured.suite.suite.trial_timeout_ms);
        let adapter = self.state.read().await.model_adapter.clone();
        'fixtures: for fixture in &captured.suite.suite.fixtures {
            for repetition in 0..captured.suite.suite.trials_per_fixture {
                for variant in [TrialVariant::Baseline, TrialVariant::Candidate] {
                    if Instant::now() >= absolute
                        || !self.active_evaluation(&report_id, &captured.identity).await
                    {
                        break 'fixtures;
                    }
                    let package = if variant == TrialVariant::Candidate {
                        Some(&captured.candidate)
                    } else {
                        captured.baseline.as_ref()
                    };
                    let request = trial_request(
                        &config,
                        &captured.suite.suite.agent_id,
                        &fixture.prompt,
                        package,
                    );
                    let started = Instant::now();
                    let deadline =
                        absolute.min(started + Duration::from_millis(logical_timeout_ms));
                    let sink = Arc::new(EvaluationSink {
                        service: self.clone(),
                        report_id: report_id.clone(),
                        identity: captured.identity.clone(),
                        deadline,
                        data: Mutex::new(StreamData::default()),
                    });
                    let trial = EvaluationTrial {
                        fixture_id: fixture.id.clone(),
                        repetition,
                        variant,
                        response: None,
                        error: None,
                        usage: TokenUsage::default(),
                        usage_reported: false,
                        usage_complete: false,
                        duration_ms: 0,
                    };
                    let timer_service = self.clone();
                    let timer_sink = sink.clone();
                    let timer_id = report_id.clone();
                    let timer_trial = trial.clone();
                    let timer_config = config.clone();
                    let timer = tokio::spawn(async move {
                        tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)).await;
                        let observed = timer_sink.trial(
                            timer_trial,
                            started,
                            Some(
                                "Evaluation trial timed out; provider usage may be incomplete"
                                    .into(),
                            ),
                        );
                        timer_service
                            .save_evaluation_trial(&timer_id, observed, &timer_config, true)
                            .await;
                    });
                    // Do not timeout/drop this future. Sink rejection asks a
                    // cooperative adapter to stop; noncooperative work retains
                    // this permit until it really exits.
                    let stream = std::panic::AssertUnwindSafe(adapter.stream(
                        &config,
                        &request,
                        sink.as_ref(),
                    ))
                    .catch_unwind()
                    .await
                    .unwrap_or_else(|_| Err("Evaluation adapter exited unexpectedly".into()));
                    timer.abort();
                    let error = stream.err().map(bounded);
                    let observed = sink.trial(trial, started, error);
                    let failed = observed.error.is_some();
                    self.save_evaluation_trial(&report_id, observed, &config, failed)
                        .await;
                    if failed {
                        break 'fixtures;
                    }
                }
            }
        }
        // Read complete packages again outside the final state lock. The
        // state identity is checked again while settling under that lock.
        let fresh = self
            .capture_evaluation(&draft_id, &approval)
            .await
            .ok()
            .is_some_and(|s| s.identity == captured.identity);
        let service = self.clone();
        let _ = self
            .evaluation_transaction(move |_unused| async move {
                let mut state = service.state.write().await;
                let current = fresh && identity_current(&state, &captured.identity);
                let closed = state.skill_evaluation_slots.is_closed();
                let expired = Instant::now() >= absolute;
                let Some(row) = state
                    .skill_evaluations
                    .reports
                    .iter_mut()
                    .find(|r| r.id == report_id)
                else {
                    return Ok(());
                };
                if row.status == EvaluationStatus::Running {
                    row.status = if closed {
                        EvaluationStatus::Interrupted
                    } else if !current {
                        EvaluationStatus::Stale
                    } else if expired {
                        EvaluationStatus::Inconclusive
                    } else {
                        match grade(&captured.suite.suite, &row.trials) {
                            EvaluationVerdict::Passed => EvaluationStatus::Passed,
                            EvaluationVerdict::Failed => EvaluationStatus::Failed,
                            EvaluationVerdict::Inconclusive => EvaluationStatus::Inconclusive,
                        }
                    };
                    if !current {
                        row.error = Some("Evaluation identity changed before settlement".into());
                    } else if closed {
                        row.error = Some("Evaluation admission closed during shutdown".into());
                    } else if expired {
                        row.error = Some("Evaluation exceeded its absolute result deadline".into());
                    } else if row.status == EvaluationStatus::Inconclusive {
                        row.error = Some("Evaluation did not produce every bounded trial".into());
                    } else if row.status == EvaluationStatus::Failed {
                        row.error = Some(
                            "Required regression, heldout pass, or minimum improvement rule failed"
                                .into(),
                        );
                    }
                    row.finished_at_ms = Some(now_millis());
                }
                row.retired_at_ms = Some(now_millis());
                row.usage_complete = row.trials.len()
                    == captured.suite.suite.fixtures.len()
                        * captured.suite.suite.trials_per_fixture as usize
                        * 2
                    && row.trials.iter().all(|t| t.usage_complete);
                let persist = state.control_plane_persist_request();
                drop(state);
                if let Err(error) = persist.save().await {
                    if let Some(row) = service
                        .state
                        .write()
                        .await
                        .skill_evaluations
                        .reports
                        .iter_mut()
                        .find(|r| r.id == report_id)
                    {
                        row.status = EvaluationStatus::Inconclusive;
                        row.error =
                            Some("Evaluation report storage could not be acknowledged".into());
                    }
                    return Err(unavailable(&bounded(error.to_string())));
                }
                Ok(())
            })
            .await;
    }

    async fn save_evaluation_trial(
        &self,
        report_id: &str,
        trial: EvaluationTrial,
        config: &AgentConfig,
        terminal: bool,
    ) {
        let (id, config) = (report_id.to_owned(), config.clone());
        let _ = self
            .evaluation_transaction(move |service| async move {
                let mut state = service.state.write().await;
                let current = state
                    .skill_evaluations
                    .reports
                    .iter()
                    .find(|r| r.id == id)
                    .is_some_and(|r| identity_current(&state, &r.identity));
                let closed = state.skill_evaluation_slots.is_closed();
                let Some(row) = state
                    .skill_evaluations
                    .reports
                    .iter_mut()
                    .find(|r| r.id == id)
                else {
                    return Ok(());
                };
                if let Some(previous) = row.trials.iter_mut().find(|t| {
                    t.fixture_id == trial.fixture_id
                        && t.repetition == trial.repetition
                        && t.variant == trial.variant
                }) {
                    *previous = trial.clone();
                } else {
                    row.trials.push(trial.clone());
                }
                if terminal
                    && matches!(
                        row.status,
                        EvaluationStatus::Running | EvaluationStatus::Passed
                    )
                {
                    row.status = if closed {
                        EvaluationStatus::Interrupted
                    } else if !current {
                        EvaluationStatus::Stale
                    } else {
                        EvaluationStatus::Inconclusive
                    };
                    row.error = trial.error.clone();
                    row.finished_at_ms = Some(now_millis());
                    row.usage_complete = false;
                }
                let agent_id = row.agent_id.clone();
                let usage = trial.usage_reported.then(|| {
                    crate::usage::usage_record(
                        &crate::usage::UsageCall {
                            id: format!(
                                "{}_{}_{:?}_{}",
                                id, trial.fixture_id, trial.variant, trial.repetition
                            ),
                            agent_id,
                            session_id: None,
                            run_id: None,
                            source: crate::usage::UsageSource::SkillEvaluation,
                            provider: config.provider.clone().unwrap_or_else(|| "unknown".into()),
                            model: config.model.clone(),
                            duration_ms: trial.duration_ms,
                            created_at_ms: now_millis().saturating_sub(trial.duration_ms),
                        },
                        &trial.usage,
                        &state.pricing_overrides,
                    )
                });
                let history = state.history.clone();
                let persist = state.control_plane_persist_request();
                drop(state);
                if let Some(usage) = usage {
                    history.enqueue_usage(vec![usage]);
                }
                if let Err(error) = persist.save().await {
                    if let Some(row) = service
                        .state
                        .write()
                        .await
                        .skill_evaluations
                        .reports
                        .iter_mut()
                        .find(|r| r.id == id)
                    {
                        if row.status == EvaluationStatus::Running {
                            row.status = EvaluationStatus::Inconclusive;
                        }
                        row.error =
                            Some("Evaluation trial storage could not be acknowledged".into());
                        row.finished_at_ms.get_or_insert_with(now_millis);
                    }
                    return Err(unavailable(&bounded(error.to_string())));
                }
                Ok(())
            })
            .await;
    }
}

fn trial_request(
    config: &AgentConfig,
    agent_id: &str,
    prompt: &str,
    package: Option<&super::package::SkillPackage>,
) -> ModelGenerateRequest {
    let mut system = config.system.clone().unwrap_or_default();
    system.push_str("\nRespond using text only. No tools or persistent conversation are available. This is a response-only evaluation, not filesystem or process verification.\n");
    if let Some(package) = package {
        system.push_str("Skill package instructions (supporting files are provided as text, never executed):\nSKILL.md\n");
        system.push_str(&String::from_utf8_lossy(&package.bytes));
        let mut files: Vec<_> = package.files.iter().collect();
        files.sort_by(|a, b| a.path.cmp(&b.path));
        for file in files {
            system.push_str(&format!(
                "\nSupporting file: {}\n{}\n",
                file.path, file.content
            ));
        }
    }
    ModelGenerateRequest {
        system,
        messages: vec![Message {
            id: uuid::Uuid::new_v4().to_string(),
            agent_id: agent_id.into(),
            room_id: uuid::Uuid::new_v4().to_string(),
            role: MessageRole::User,
            content: Content {
                text: prompt.into(),
                ..Content::default()
            },
            created_at_ms: now_millis(),
        }],
        temperature: config.settings.as_ref().and_then(|s| s.temperature),
        max_tokens: config.settings.as_ref().and_then(|s| s.max_tokens),
    }
}
#[derive(Clone, Default)]
struct StreamData {
    bytes: usize,
    rejected: bool,
    usage: TokenUsage,
    usage_reported: bool,
    final_usage_reported: bool,
    response: Option<ModelGenerateResponse>,
}
struct EvaluationSink {
    service: SkillService,
    report_id: String,
    identity: EvaluationIdentity,
    deadline: Instant,
    data: Mutex<StreamData>,
}
impl EvaluationSink {
    fn trial(
        &self,
        mut trial: EvaluationTrial,
        started: Instant,
        error: Option<String>,
    ) -> EvaluationTrial {
        let data = self.data.lock().unwrap_or_else(|p| p.into_inner());
        trial.usage = data.usage.clone();
        trial.usage_reported = data.usage_reported;
        trial.usage_complete = data.final_usage_reported;
        trial.duration_ms = started.elapsed().as_millis().min(u64::MAX as u128) as u64;
        trial.response = data.response.as_ref().map(|r| r.content.text.clone());
        trial.error = error.or_else(|| {
            if Instant::now() >= self.deadline { Some("Evaluation trial timed out; late output is ineligible".into()) }
            else if data.rejected { Some("Evaluation rejected output, requested tools, fallback, truncation, or stale authority".into()) }
            else if data.response.is_none() { Some("Evaluation stream ended without a final response".into()) }
            else { None }
        });
        trial
    }
}
#[async_trait]
impl ModelStreamSink for EvaluationSink {
    async fn emit(&self, frame: ModelStreamFrame) -> Result<(), String> {
        let permitted = Instant::now() < self.deadline
            && self
                .service
                .active_evaluation(&self.report_id, &self.identity)
                .await;
        let mut data = self.data.lock().unwrap_or_else(|p| p.into_inner());
        let usage = match &frame {
            ModelStreamFrame::Usage(u) => Some(u),
            ModelStreamFrame::Final(r) => Some(&r.usage),
            _ => None,
        };
        if let Some(usage) = usage.filter(|u| **u != TokenUsage::default()) {
            data.usage = usage.clone();
            data.usage_reported = true;
        }
        if let ModelStreamFrame::Final(response) = &frame {
            data.final_usage_reported = response.usage != TokenUsage::default();
        }
        if !permitted || data.rejected || data.response.is_some() {
            data.rejected = true;
            return Err("Skill evaluation stream is no longer eligible".into());
        }
        match frame {
            ModelStreamFrame::TextDelta(text) => {
                data.bytes = data.bytes.saturating_add(text.len());
                if data.bytes > MAX_RESPONSE_BYTES {
                    data.rejected = true;
                    return Err("Skill evaluation output bound exceeded".into());
                }
            }
            ModelStreamFrame::Usage(_) => {}
            ModelStreamFrame::Served { .. } => {
                data.rejected = true;
                return Err("Skill evaluations cannot use a fallback model".into());
            }
            ModelStreamFrame::Final(response) => {
                if response.content.text.len() > MAX_RESPONSE_BYTES
                    || response.stop_reason != ModelStopReason::End
                    || response.tool_calls.as_ref().is_some_and(|v| !v.is_empty())
                    || response
                        .content
                        .attachments
                        .as_ref()
                        .is_some_and(|v| !v.is_empty())
                    || response
                        .content
                        .metadata
                        .as_ref()
                        .is_some_and(|v| !v.is_empty())
                {
                    data.rejected = true;
                    return Err(
                        "Skill evaluation requires a complete bounded text-only response".into(),
                    );
                }
                data.response = Some(response);
            }
        }
        Ok(())
    }
}
