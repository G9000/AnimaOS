use std::collections::BTreeMap;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Mutex,
};
use std::time::Instant;

use anima_core::execution_limits::{ExecutionLimits, ModelCallContext, SettlementOutcome};
use anima_core::primitives::now_millis;
use anima_core::skill_evaluation::digest;
use anima_core::task_regressions::{compare_planned_tasks, TrialObservation};
use anima_core::trace_evaluation::{regrade_observation, GradedTrace, TraceCapture, TraceVariant};
use anima_core::{
    AgentConfig, AgentRuntime, CancelSignal, Content, ModelAdapter, ModelGenerateRequest,
    ModelGenerateResponse, ModelStreamFrame, ModelStreamSink, TokenUsage,
};
use async_trait::async_trait;
use futures::FutureExt;
use tokio::sync::OwnedSemaphorePermit;

use super::contracts::*;
use super::fixture_tools::FixtureWorkspace;
use super::service::{unavailable, TaskRegressionService};

struct RetiredTrial {
    fixture_index: usize,
    repetition: u32,
    variant: TraceVariant,
    observation: Result<TrialObservation, String>,
    usage: BTreeMap<String, TokenUsage>,
    served: BTreeMap<String, (String, String)>,
    complete: bool,
    duration_ms: u64,
}
#[derive(Clone, Default)]
struct CallFacts {
    usage: Option<TokenUsage>,
    attested_usage: Option<TokenUsage>,
    served: Option<(String, String)>,
    final_seen: bool,
}

enum ForwardEvent {
    Frame(ModelStreamFrame),
    Complete(TokenUsage),
    Done(Result<(), String>),
}
struct ConsumerLease(Arc<AtomicBool>);
impl Drop for ConsumerLease {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

struct AttestedAdapter {
    inner: Arc<dyn ModelAdapter>,
    service: TaskRegressionService,
    report_id: String,
    calls: AtomicUsize,
    complete: Arc<AtomicUsize>,
    workers: Mutex<Vec<tokio::task::JoinHandle<Result<(), String>>>>,
    facts: Arc<Mutex<BTreeMap<String, CallFacts>>>,
}

struct DetachedSink {
    sender: tokio::sync::mpsc::Sender<ForwardEvent>,
    facts: Arc<Mutex<BTreeMap<String, CallFacts>>>,
    key: String,
    bytes: AtomicUsize,
}

#[async_trait]
impl ModelStreamSink for DetachedSink {
    async fn emit(&self, frame: ModelStreamFrame) -> Result<(), String> {
        {
            let mut all = self
                .facts
                .lock()
                .map_err(|_| "Task usage witness unavailable")?;
            let fact = all.entry(self.key.clone()).or_default();
            match &frame {
                ModelStreamFrame::Usage(usage) => {
                    if usage.total_tokens > 0 {
                        fact.usage = Some(usage.clone());
                    }
                }
                ModelStreamFrame::Final(response) => {
                    fact.final_seen = true;
                    if response.usage.total_tokens > 0 {
                        fact.usage = Some(response.usage.clone());
                    }
                }
                ModelStreamFrame::Served {
                    provider, model, ..
                } => {
                    fact.served = Some((provider.clone(), model.clone()));
                }
                _ => {}
            }
        }
        let bytes = match &frame {
            ModelStreamFrame::TextDelta(text) => text.len(),
            ModelStreamFrame::Final(response) => serde_json::to_vec(&response.content)
                .map_err(|_| "Task stream invalid")?
                .len()
                .saturating_add(response.tool_calls.as_ref().map_or(0, |calls| {
                    calls.iter().fold(0usize, |total, call| {
                        total.saturating_add(
                            serde_json::to_vec(&call.args).map_or(usize::MAX, |v| v.len()),
                        )
                    })
                })),
            _ => 0,
        };
        if bytes > 1024 * 1024
            || self
                .bytes
                .fetch_add(bytes, Ordering::SeqCst)
                .saturating_add(bytes)
                > 4 * 1024 * 1024
        {
            return Err("Task stream evidence bound reached".into());
        }
        // Runtime guards may close the consumer. Keep polling the admitted producer
        // and retaining final usage without publishing late text back to the actor.
        let _ = self.sender.send(ForwardEvent::Frame(frame)).await;
        Ok(())
    }
    async fn complete_usage(&self, usage: TokenUsage) -> Result<(), String> {
        self.facts
            .lock()
            .map_err(|_| "Task usage witness unavailable")?
            .entry(self.key.clone())
            .or_default()
            .attested_usage = Some(usage.clone());
        let _ = self.sender.send(ForwardEvent::Complete(usage)).await;
        Ok(())
    }
}

impl AttestedAdapter {
    async fn retire(&self) -> Result<(), String> {
        let workers = {
            std::mem::take(
                &mut *self
                    .workers
                    .lock()
                    .map_err(|_| "Task transport custody unavailable")?,
            )
        };
        let mut failure = None;
        for worker in workers {
            if !matches!(worker.await, Ok(Ok(()))) {
                failure = Some("Task adapter producer ended without complete success".into());
            }
        }
        match failure {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}
#[async_trait]
impl ModelAdapter for AttestedAdapter {
    fn provider(&self) -> &str {
        self.inner.provider()
    }
    async fn generate(
        &self,
        _: &AgentConfig,
        _: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        Err("Task comparisons require the controlled runtime path".into())
    }
    async fn stream_controlled(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
        sink: &dyn ModelStreamSink,
        context: &ModelCallContext,
    ) -> Result<(), String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let eligible = Arc::new(AtomicBool::new(true));
        let _consumer = ConsumerLease(eligible.clone());
        let service = self.service.clone();
        let id = self.report_id.clone();
        let guarded = context.clone().with_dispatch_validator(move || {
            let service = service.clone();
            let id = id.clone();
            let eligible = eligible.clone();
            async move {
                if !eligible.load(Ordering::SeqCst) {
                    return Err("Task runtime consumer retired; further dispatch refused".into());
                }
                service
                    .verify_owner()
                    .await
                    .map_err(|_| "Current profile authority unavailable")?;
                if !service.active(&id).await {
                    return Err("Task comparison source or admission changed".into());
                }
                Ok(())
            }
        });
        let inner = self.inner.clone();
        let config = config.clone();
        let request = request.clone();
        let facts = self.facts.clone();
        let complete = self.complete.clone();
        let key = guarded.logical_call_id.clone();
        let (sender, mut receiver) = tokio::sync::mpsc::channel(16);
        let worker_sender = sender.clone();
        let worker = tokio::spawn(async move {
            let witness = DetachedSink {
                sender: worker_sender.clone(),
                facts: facts.clone(),
                key: key.clone(),
                bytes: AtomicUsize::new(0),
            };
            let result = inner
                .stream_controlled(&config, &request, &witness, &guarded)
                .await;
            let attested = facts
                .lock()
                .ok()
                .and_then(|all| all.get(&key).cloned())
                .is_some_and(|fact| {
                    fact.final_seen
                        && fact.attested_usage.is_some()
                        && fact.attested_usage == fact.usage
                });
            if result.is_ok() && attested {
                complete.fetch_add(1, Ordering::SeqCst);
            }
            let _ = worker_sender.send(ForwardEvent::Done(result.clone())).await;
            result
        });
        self.workers
            .lock()
            .map_err(|_| "Task transport custody unavailable")?
            .push(worker);
        drop(sender);
        while let Some(event) = receiver.recv().await {
            match event {
                ForwardEvent::Frame(frame) => sink.emit(frame).await?,
                ForwardEvent::Complete(usage) => sink.complete_usage(usage).await?,
                ForwardEvent::Done(result) => return result,
            }
        }
        Err("Task transport producer exited unexpectedly".into())
    }
}

impl TaskRegressionService {
    pub(super) async fn execute(
        &self,
        run: StoredRun,
        cancel: CancelSignal,
        _physical: OwnedSemaphorePermit,
    ) {
        let timer_service = self.clone();
        let timer_id = run.id.clone();
        let timer_cancel = cancel.clone();
        let timer = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(RUN_TIMEOUT_MS)).await;
            timer_service
                .stop(
                    &timer_id,
                    RunStatus::Inconclusive,
                    "Comparison deadline reached; physical custody remains retained",
                )
                .await;
            timer_cancel.cancel();
        });
        let outcome = std::panic::AssertUnwindSafe(self.execute_matrix(&run, cancel))
            .catch_unwind()
            .await;
        timer.abort();
        let error = match outcome {
            Ok(Ok(())) => None,
            Ok(Err(message)) => Some(message),
            Err(_) => {
                Some("Task runtime exited unexpectedly; uncertain work is not replayed".into())
            }
        };
        let id = run.id.clone();
        let _ = self.transaction(move |service| async move {
            let mut state = service.state.write().await;
            let current = state.task_regressions.runs.iter().find(|r| r.id == id).is_some_and(|r| Self::current(&state, r));
            let row = state.task_regressions.runs.iter_mut().find(|r| r.id == id).ok_or(RegressionError::NotFound)?;
            if row.status == RunStatus::Running {
                row.status = if !current { RunStatus::Stale } else if error.is_none() && compare_planned_tasks(&row.suite.suite, &row.plan, &row.observations).is_ok() { RunStatus::Completed } else { RunStatus::Inconclusive };
                row.finished_at_ms = Some(now_millis());
                row.error = error.or_else(|| (row.status == RunStatus::Inconclusive).then(|| "Incomplete or incompatible task evidence; no comparison verdict".into()));
            }
            row.finished_at_ms.get_or_insert_with(now_millis);
            row.retired_at_ms = Some(now_millis());
            let persist = state.control_plane_persist_request(); drop(state);
            let saved = persist.save().await;
            let mut state = service.state.write().await;
            if saved.is_err() {
                if let Some(row) = state.task_regressions.runs.iter_mut().find(|row| row.id == id) {
                    row.status = RunStatus::Inconclusive;
                    row.error = Some("Retirement persistence unacknowledged; outcome and usage acknowledgement remain incomplete".into());
                }
            }
            state.task_regression_cancels.remove(&id);
            saved.map_err(|_| unavailable("Task retirement storage could not be acknowledged"))
        }).await;
    }

    async fn execute_matrix(&self, run: &StoredRun, cancel: CancelSignal) -> Result<(), String> {
        if !self.active(&run.id).await {
            return Err("Comparison stopped before dispatch".into());
        }
        let mut limits = run
            .baseline
            .config
            .settings
            .as_ref()
            .and_then(|s| s.execution_limits.clone())
            .unwrap_or_else(ExecutionLimits::default);
        limits.max_model_attempts = limits.max_model_attempts.min(96);
        limits.max_retries = 0;
        limits.budget.max_wall_time_ms = Some(
            limits
                .budget
                .max_wall_time_ms
                .unwrap_or(RUN_TIMEOUT_MS)
                .min(RUN_TIMEOUT_MS),
        );
        limits.budget.max_artifact_bytes = Some(
            limits
                .budget
                .max_artifact_bytes
                .unwrap_or(REPORT_BYTES as u64)
                .min(REPORT_BYTES as u64),
        );
        // Logical stop closes report-bound dispatch validation. It does not drop an
        // already polled transport; the original retained physical deadline remains.
        let parent = crate::execution_budgets::standalone_context_with_limits(
            &self.state,
            self.transactions.clone(),
            format!("task-regression:{}", run.id),
            CancelSignal::new(),
            Some(limits),
        )
        .await?;
        let inner = self.state.read().await.profile_model_adapter();
        for (index, fixture) in run.suite.suite.fixtures.iter().enumerate() {
            for repetition in 0..run.plan.repetitions {
                let order = if (index + repetition as usize) % 2 == 0 {
                    [TraceVariant::Baseline, TraceVariant::Candidate]
                } else {
                    [TraceVariant::Candidate, TraceVariant::Baseline]
                };
                for variant in order {
                    if !self.active(&run.id).await || cancel.is_cancelled() {
                        return Err(
                            "Comparison stopped; remaining trials were not dispatched".into()
                        );
                    }
                    self.verify_owner()
                        .await
                        .map_err(|_| "Current profile authority unavailable")?;
                    let selected = if variant == TraceVariant::Baseline {
                        &run.baseline
                    } else {
                        &run.candidate
                    };
                    let pin = if variant == TraceVariant::Baseline {
                        &run.plan.baseline
                    } else {
                        &run.plan.candidate
                    };
                    let workspace = Arc::new(FixtureWorkspace::new(
                        &fixture.actor,
                        selected.pin.tool_policy == ToolPolicy::FilesAndMockEffects,
                    )?);
                    let child_id = format!("{}:trial-{index}-{repetition}-{variant:?}", run.id);
                    let mut admission = crate::execution_budgets::retained_child_context(
                        &self.state,
                        self.transactions.clone(),
                        &parent,
                        child_id,
                        CancelSignal::new(),
                    )
                    .await?;
                    admission.dispatch.revalidate().await?;
                    let adapter = Arc::new(AttestedAdapter {
                        inner: inner.clone(),
                        service: self.clone(),
                        report_id: run.id.clone(),
                        calls: AtomicUsize::new(0),
                        complete: Arc::new(AtomicUsize::new(0)),
                        workers: Mutex::new(Vec::new()),
                        facts: Arc::new(Mutex::new(BTreeMap::new())),
                    });
                    let mut runtime = AgentRuntime::new(selected.config.clone(), adapter.clone());
                    runtime.set_execution_context(admission.context.clone())?;
                    let capture = Arc::new(TraceCapture::new(
                        pin.settings.clone(),
                        &fixture.id,
                        repetition,
                        variant,
                    ));
                    runtime.set_run_observer(capture.clone());
                    let held = workspace.clone();
                    let tool_context = admission.context.clone();
                    let tool_service = self.clone();
                    let tool_report_id = run.id.clone();
                    let tool_logical_cancel = cancel.clone();
                    let started = Instant::now();
                    // No timeout/select drops the admitted runtime. A logical deadline closes
                    // admission while this worker retains the adapter, workspace and slot.
                    let result = std::panic::AssertUnwindSafe(runtime.run_with_tools(
                        Content {
                            text: fixture.actor.prompt.clone(),
                            ..Content::default()
                        },
                        move |_, _, call| {
                            let held = held.clone();
                            let context = tool_context.clone();
                            let service = tool_service.clone();
                            let id = tool_report_id.clone();
                            let logical_cancel = tool_logical_cancel.clone();
                            async move {
                                if context.check_live().is_ok()
                                    && service.active(&id).await
                                    && service.verify_owner().await.is_ok()
                                {
                                    held.execute_controlled(call, &context, &logical_cancel)
                                        .await
                                } else {
                                    anima_core::TaskResult::error("fixture_authority_closed", 0)
                                }
                            }
                        },
                    ))
                    .catch_unwind()
                    .await
                    .unwrap_or_else(|_| {
                        anima_core::TaskResult::error("Task runtime failed; no replay", 0)
                    });
                    if result.status != anima_core::TaskStatus::Success {
                        self.stop(&run.id, RunStatus::Inconclusive, "Runtime outcome is ineligible; physical producer and usage remain retained").await;
                    }
                    let retired = adapter.retire().await;
                    let dispatch_outcome =
                        if result.status == anima_core::TaskStatus::Success && retired.is_ok() {
                            SettlementOutcome::Known(TokenUsage::default())
                        } else {
                            SettlementOutcome::Unknown { observed: None }
                        };
                    let dispatch_retirement = admission
                        .dispatch
                        .finish_owned(dispatch_outcome, true)
                        .await;
                    let snapshot = workspace.seal();
                    let observed =
                        retired
                            .and_then(|_| capture.finish(&result))
                            .and_then(|trace| {
                                let (observed_files, observed_directories) = snapshot?;
                                let mut graded = GradedTrace {
                                    trace,
                                    oracle_failures: Vec::new(),
                                    observed_files,
                                    observed_directories,
                                };
                                graded.oracle_failures =
                                    regrade_observation(&fixture.task_fixture(), &graded)?;
                                Ok(TrialObservation {
                                    variant_pin: pin.clone(),
                                    graded,
                                    mock_effects: workspace.effects(),
                                    artifact_refs: Vec::new(),
                                })
                            });
                    let complete = adapter.calls.load(Ordering::SeqCst) > 0
                        && adapter.calls.load(Ordering::SeqCst)
                            == adapter.complete.load(Ordering::SeqCst);
                    let (usage, served) = {
                        let facts = adapter
                            .facts
                            .lock()
                            .map_err(|_| "Task usage evidence unavailable")?;
                        (
                            facts
                                .iter()
                                .filter_map(|(key, value)| {
                                    value.usage.clone().map(|usage| (key.clone(), usage))
                                })
                                .collect(),
                            facts
                                .iter()
                                .filter_map(|(key, value)| {
                                    value.served.clone().map(|served| (key.clone(), served))
                                })
                                .collect(),
                        )
                    };
                    self.save_trial(
                        run,
                        selected,
                        RetiredTrial {
                            fixture_index: index,
                            repetition,
                            variant,
                            observation: observed,
                            usage,
                            served,
                            complete,
                            duration_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
                        },
                    )
                    .await?;
                    dispatch_retirement?;
                }
            }
        }
        Ok(())
    }

    async fn active(&self, id: &str) -> bool {
        let state = self.state.read().await;
        !state.task_regression_slots.is_closed()
            && state
                .task_regressions
                .runs
                .iter()
                .find(|r| r.id == id)
                .is_some_and(|run| run.status == RunStatus::Running && Self::current(&state, run))
    }

    async fn stop(&self, id: &str, status: RunStatus, message: &str) {
        let (id, message) = (id.to_owned(), message.to_owned());
        let _ = self
            .transaction(move |service| async move {
                let mut state = service.state.write().await;
                if let Some(row) = state.task_regressions.runs.iter_mut().find(|r| r.id == id) {
                    if row.status == RunStatus::Running {
                        row.status = status;
                        row.finished_at_ms = Some(now_millis());
                        row.error = Some(message);
                    }
                }
                let persist = state.control_plane_persist_request();
                drop(state);
                persist
                    .save()
                    .await
                    .map_err(|_| unavailable("Task stop storage unacknowledged"))
            })
            .await;
    }

    async fn save_trial(
        &self,
        run: &StoredRun,
        selected: &CapturedVariant,
        trial: RetiredTrial,
    ) -> Result<(), String> {
        let RetiredTrial {
            fixture_index,
            repetition,
            variant,
            observation,
            usage,
            served,
            complete,
            duration_ms,
        } = trial;
        let (id, selected) = (run.id.clone(), selected.clone());
        self.transaction(move |service| async move {
            let mut state = service.state.write().await;
            let mut records = Vec::new();
            for (step, counters) in usage {
                if counters.total_tokens == 0 {
                    continue;
                }
                let key = format!("{id}:{fixture_index}:{repetition}:{variant:?}:{step}");
                let already = state
                    .task_regressions
                    .runs
                    .iter()
                    .find(|r| r.id == id)
                    .is_some_and(|r| r.usage_rows.contains(&key));
                if already {
                    continue;
                }
                let (provider, model) = served
                    .get(&step)
                    .cloned()
                    .unwrap_or_else(|| (selected.pin.provider.clone(), selected.pin.model.clone()));
                records.push(crate::usage::usage_record(
                    &crate::usage::UsageCall {
                        id: key,
                        agent_id: selected.pin.agent_id.clone(),
                        session_id: None,
                        run_id: None,
                        source: crate::usage::UsageSource::TaskRegression,
                        provider,
                        model,
                        duration_ms,
                        created_at_ms: now_millis(),
                    },
                    &counters,
                    &state.pricing_overrides,
                ));
            }
            let history = state.history.clone();
            history
                .enqueue_usage(records.clone())
                .map_err(|_| unavailable("Task usage custody unavailable"))?;
            let row = state
                .task_regressions
                .runs
                .iter_mut()
                .find(|r| r.id == id)
                .ok_or(RegressionError::NotFound)?;
            row.usage_rows.extend(records.iter().map(|r| r.id.clone()));
            match observation {
                Ok(value) => {
                    let duplicate = row.observations.iter().any(|prior| {
                        prior.graded.trace.fixture_id == value.graded.trace.fixture_id
                            && prior.graded.trace.repetition == repetition
                            && prior.graded.trace.variant == variant
                    });
                    if duplicate {
                        return Err(conflict_duplicate());
                    }
                    let bytes = serde_json::to_vec(&value)
                        .map_err(|_| unavailable("Task evidence serialization unavailable"))?
                        .len();
                    let prior_bytes = serde_json::to_vec(&row.observations)
                        .map_err(|_| unavailable("Task evidence serialization unavailable"))?
                        .len();
                    if prior_bytes.saturating_add(bytes) > REPORT_BYTES / 2 {
                        row.status = RunStatus::Inconclusive;
                        row.error = Some("Aggregate task evidence limit reached".into());
                    } else {
                        if complete {
                            row.usage_attestations.push(digest(&value));
                        }
                        row.observations.push(value);
                    }
                }
                Err(_) => {
                    if row.status == RunStatus::Running {
                        row.status = RunStatus::Inconclusive;
                    }
                    row.error =
                        Some("A trial did not produce complete trusted observations".into());
                }
            }
            let persist = state.control_plane_persist_request();
            drop(state);
            if persist.save().await.is_err() {
                // These calls already happened. Retain their immutable outbox rows
                // for retirement/history persistence, without replaying a provider.
                if let Some(row) = service
                    .state
                    .write()
                    .await
                    .task_regressions
                    .runs
                    .iter_mut()
                    .find(|r| r.id == id)
                {
                    row.status = RunStatus::Inconclusive;
                    row.error = Some("Trial storage acknowledgement failed; no replay".into());
                    row.finished_at_ms.get_or_insert_with(now_millis);
                }
                return Err(unavailable("Task trial storage unacknowledged"));
            }
            Ok(())
        })
        .await
        .map_err(|_| {
            "Task observation or usage persistence failed; remaining trials stopped".into()
        })
    }
}

fn conflict_duplicate() -> RegressionError {
    RegressionError::Conflict("Duplicate trial observation refused".into())
}
