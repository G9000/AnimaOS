use std::collections::BTreeMap;
use std::future::Future;
use std::sync::Arc;

use anima_core::primitives::now_millis;
use anima_core::skill_evaluation::digest;
use anima_core::task_regressions::{
    compare_planned_tasks, evaluator_fingerprint, ActorFixture, ComparisonPlan, RegressionSuite,
    VariantPin,
};
use anima_core::trace_evaluation::TraceSettings;
use anima_core::{AgentConfig, AgentSettings};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;

use super::contracts::*;
use super::fixture_tools;
use crate::app::SharedDaemonState;

#[derive(Clone)]
pub(crate) struct TaskRegressionService {
    pub(super) state: SharedDaemonState,
    pub(super) transactions: Arc<Mutex<()>>,
    admission: Arc<dyn Fn() -> bool + Send + Sync>,
}

pub(super) fn unavailable(message: &str) -> RegressionError {
    RegressionError::Unavailable(message.into())
}
pub(super) fn conflict(message: &str) -> RegressionError {
    RegressionError::Conflict(message.into())
}

pub(super) fn source_fingerprint() -> String {
    static FINGERPRINT: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    FINGERPRINT.get_or_init(|| {
    let mut digest = Sha256::new();
    for bytes in [include_bytes!("service.rs").as_slice(), include_bytes!("runner.rs").as_slice(),
        include_bytes!("fixture_tools.rs").as_slice(), include_bytes!("contracts.rs").as_slice(),
        include_bytes!("../../../../packages/core-rust/crates/anima-core/src/runtime.rs").as_slice(),
        include_bytes!("../../../../packages/core-rust/crates/anima-core/src/execution_limits.rs").as_slice(),
        include_bytes!("../../../../packages/core-rust/crates/anima-core/src/execution_limits/context.rs").as_slice(),
        include_bytes!("../runtime_model.rs").as_slice(), include_bytes!("../owned_model.rs").as_slice(),
        include_bytes!("../execution_budgets.rs").as_slice(), include_bytes!("../execution_budgets/port.rs").as_slice(),
        include_bytes!("../routes/task_regressions.rs").as_slice(), include_bytes!("../../../../Cargo.lock").as_slice()] {
        digest.update((bytes.len() as u64).to_le_bytes()); digest.update(bytes);
    }
    hex::encode(digest.finalize())
    }).clone()
}

impl TaskRegressionService {
    pub(crate) fn new(
        state: SharedDaemonState,
        transactions: Arc<Mutex<()>>,
        admission: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Self {
        Self {
            state,
            transactions,
            admission,
        }
    }

    pub(super) async fn transaction<T, F, Fut>(&self, work: F) -> Result<T, RegressionError>
    where
        T: Send + 'static,
        F: FnOnce(Self) -> Fut + Send + 'static,
        Fut: Future<Output = Result<T, RegressionError>> + Send + 'static,
    {
        let service = self.clone();
        tokio::spawn(async move {
            let _guard = service.transactions.clone().lock_owned().await;
            work(service).await
        })
        .await
        .map_err(|_| {
            unavailable("Task comparison transaction exited; do not replay uncertain work")
        })?
    }

    pub(super) async fn verify_owner(&self) -> Result<(), RegressionError> {
        let owner = {
            let state = self.state.read().await;
            state
                .ensure_profile_owned()
                .map_err(|_| unavailable("Current profile authority unavailable"))?;
            state.profile_owner()
        };
        if let Some(owner) = owner {
            owner
                .verify_owned()
                .await
                .map_err(|_| unavailable("Current profile authority unavailable"))?;
        }
        Ok(())
    }

    pub(crate) async fn configuration(
        &self,
        id: &str,
    ) -> Result<ConfigurationSummary, RegressionError> {
        self.verify_owner().await?;
        let state = self.state.read().await;
        let config = state
            .agents
            .get(id)
            .ok_or(RegressionError::NotFound)?
            .config();
        if crate::agent_runs::is_helper_config(config) {
            return Err(conflict("Choose an owner agent configuration"));
        }
        Ok(ConfigurationSummary {
            agent_id: id.into(),
            config_digest: digest(config),
            provider: config.provider.clone(),
            model: config.model.clone(),
            system: config.system.clone(),
        })
    }

    pub(crate) async fn suites(&self) -> Result<Vec<SuiteSummary>, RegressionError> {
        self.verify_owner().await?;
        Ok(self
            .state
            .read()
            .await
            .task_regressions
            .suites
            .iter()
            .map(|s| s.summary.clone())
            .collect())
    }

    pub(crate) async fn define(
        &self,
        suite: RegressionSuite,
    ) -> Result<SuiteSummary, RegressionError> {
        suite.validate().map_err(RegressionError::Invalid)?;
        self.transaction(move |service| async move {
            service.verify_owner().await?;
            let mut state = service.state.write().await;
            if state.control_plane_store.is_none() {
                return Err(unavailable("Durable comparison storage unavailable"));
            }
            if state.task_regressions.suites.len() >= MAX_SUITES {
                return Err(conflict("Saved task suite capacity reached"));
            }
            let summary = SuiteSummary {
                id: format!("trs_{}", uuid::Uuid::new_v4()),
                name: suite.name.clone(),
                digest: suite.digest().map_err(RegressionError::Invalid)?,
                fixture_count: suite.fixtures.len(),
                created_at_ms: now_millis(),
            };
            state.task_regressions.suites.push(StoredSuite {
                summary: summary.clone(),
                suite,
            });
            let persist = state.control_plane_persist_request();
            drop(state);
            persist
                .save()
                .await
                .map_err(|_| unavailable("Suite storage could not be acknowledged"))?;
            Ok(summary)
        })
        .await
    }

    pub(crate) async fn start(
        &self,
        request: CompareRequest,
    ) -> Result<RegressionReport, RegressionError> {
        if request.request_key.trim().is_empty()
            || request.request_key.len() > 128
            || !(1..=3).contains(&request.repetitions)
            || request.baseline.agent_id != request.candidate.agent_id
        {
            return Err(RegressionError::Invalid(
                "Use one existing agent source, a bounded request key and 1..3 repetitions".into(),
            ));
        }
        self.transaction(move |service| async move {
            service.verify_owner().await?;
            let request_digest = digest(&request);
            let mut state = service.state.write().await;
            if let Some(run) = state.task_regressions.runs.iter().find(|r| r.request.request_key == request.request_key) {
                if run.request_digest != request_digest { return Err(conflict("Request key already names a different comparison")); }
                return service.project(&state, run);
            }
            if state.task_regressions.admissions.contains_key(&request.request_key) {
                return Err(conflict("This request was already admitted; its report expired and it will not replay. Use a new request key only for an intentional new comparison"));
            }
            if state.task_regressions.admissions.len() >= 1024 { return Err(conflict("Retained comparison admission capacity reached")); }
            if !(service.admission)() || state.task_regression_slots.is_closed() { return Err(conflict("Daemon admission is closed")); }
            if state.control_plane_store.is_none() { return Err(unavailable("Durable comparison admission unavailable")); }
            let suite = state.task_regressions.suites.iter().find(|s| s.summary.id == request.suite_id && s.summary.digest == request.suite_digest)
                .cloned().ok_or_else(|| conflict("Saved suite selection changed or is unavailable"))?;
            let baseline = capture_variant(&state, &request.baseline, &suite.suite)?;
            let candidate = capture_variant(&state, &request.candidate, &suite.suite)?;
            let physical = state.task_regression_slots.clone().try_acquire_owned().map_err(|_| conflict("A task comparison still owns its physical worker"))?;
            let now = now_millis();
            state.task_regressions.reservations.retain(|at| *at > now.saturating_sub(24 * 60 * 60 * 1000));
            if state.task_regressions.reservations.len() >= MAX_DAILY_RUNS { return Err(conflict("The eight-per-day task comparison allowance is exhausted")); }
            if state.task_regressions.runs.len() >= MAX_REPORTS {
                let index = state.task_regressions.runs.iter().position(|r| r.retired_at_ms.is_some()).ok_or_else(|| conflict("Retained task evidence capacity reached"))?;
                state.task_regressions.runs.remove(index);
            }
            let plan = ComparisonPlan { suite_digest: suite.summary.digest.clone(), repetitions: request.repetitions,
                baseline: core_pin(&baseline, &suite.summary.digest)?, candidate: core_pin(&candidate, &suite.summary.digest)?, evaluator_digest: evaluator_fingerprint() };
            plan.validate(&suite.suite).map_err(RegressionError::Invalid)?;
            let run = StoredRun { id: format!("trr_{}", uuid::Uuid::new_v4()), request, request_digest, suite, baseline, candidate, plan,
                tool_implementation_digest: fixture_tools::fingerprint(), host_source_digest: source_fingerprint(), status: RunStatus::Running, observations: Vec::new(), created_at_ms: now,
                finished_at_ms: None, retired_at_ms: None, error: None, usage_rows: Vec::new(), usage_attestations: Vec::new() };
            let cancel = anima_core::CancelSignal::new();
            state.task_regression_cancels.insert(run.id.clone(), cancel.clone());
            state.task_regressions.admissions.insert(run.request.request_key.clone(), (run.request_digest.clone(), run.id.clone()));
            state.task_regressions.reservations.push(now);
            state.task_regressions.runs.push(run.clone());
            let report = service.project(&state, &run)?;
            let persist = state.control_plane_persist_request(); drop(state);
            if persist.save().await.is_err() {
                let mut state = service.state.write().await;
                if let Some(row) = state.task_regressions.runs.iter_mut().find(|r| r.id == run.id) {
                    row.status = RunStatus::Inconclusive; row.error = Some("Admission persistence unacknowledged; no provider dispatched".into());
                    row.finished_at_ms = Some(now_millis()); row.retired_at_ms = row.finished_at_ms;
                }
                state.task_regression_cancels.remove(&run.id);
                return Err(unavailable("Comparison admission could not be acknowledged; reservation retained"));
            }
            let worker = service.clone();
            tokio::spawn(async move { worker.execute(run, cancel, physical).await; });
            Ok(report)
        }).await
    }

    pub(crate) async fn joined(
        &self,
        request: &CompareRequest,
    ) -> Result<Option<RegressionReport>, RegressionError> {
        self.verify_owner().await?;
        let state = self.state.read().await;
        if let Some(run) = state
            .task_regressions
            .runs
            .iter()
            .find(|run| run.request.request_key == request.request_key)
        {
            if run.request_digest != digest(request) {
                return Err(conflict("Request key already names a different comparison"));
            }
            return self.project(&state, run).map(Some);
        }
        if state
            .task_regressions
            .admissions
            .contains_key(&request.request_key)
        {
            return Err(conflict(
                "This request was already admitted; its expired report cannot be replayed",
            ));
        }
        Ok(None)
    }

    pub(super) fn current(state: &crate::state::DaemonState, run: &StoredRun) -> bool {
        [&run.baseline.pin, &run.candidate.pin].iter().all(|pin| {
            state
                .agents
                .get(&pin.agent_id)
                .is_some_and(|agent| digest(agent.config()) == pin.config_digest)
        }) && run.plan.evaluator_digest == evaluator_fingerprint()
            && run.tool_implementation_digest == fixture_tools::fingerprint()
            && run.host_source_digest == source_fingerprint()
    }

    pub(super) fn project(
        &self,
        state: &crate::state::DaemonState,
        run: &StoredRun,
    ) -> Result<RegressionReport, RegressionError> {
        let planned = run.suite.suite.fixtures.len() * run.plan.repetitions as usize * 2;
        let acknowledgement_pending = run.status == RunStatus::Completed
            && state.task_regression_cancels.contains_key(&run.id);
        let comparison = if run.status == RunStatus::Completed
            && !acknowledgement_pending
            && Self::current(state, run)
            && run.observations.len() == planned
        {
            compare_planned_tasks(&run.suite.suite, &run.plan, &run.observations).ok()
        } else {
            None
        };
        let observations = run
            .observations
            .iter()
            .map(observation_view)
            .collect::<Result<Vec<_>, _>>()
            .map_err(RegressionError::Invalid)?;
        let usage_complete = run.status == RunStatus::Completed
            && !acknowledgement_pending
            && run.retired_at_ms.is_some()
            && observations.len() == planned
            && observations.iter().all(|o| {
                o.metrics.unknown_usage_calls == 0
                    && run.usage_attestations.contains(&o.observation_digest)
            });
        let mock = [&run.baseline.pin.provider, &run.candidate.pin.provider]
            .iter()
            .all(|p| crate::runtime_model::is_mock_provider(p));
        Ok(RegressionReport { id: run.id.clone(), status: if !Self::current(state, run) { RunStatus::Stale } else if acknowledgement_pending { RunStatus::Running } else { run.status }, suite: run.suite.summary.clone(), baseline: run.baseline.pin.clone(), candidate: run.candidate.pin.clone(), repetitions: run.plan.repetitions,
            planned_trials: planned, completed_trials: observations.len(), created_at_ms: run.created_at_ms, finished_at_ms: run.finished_at_ms, retired_at_ms: run.retired_at_ms,
            evidence_scope: if mock { "runtimePlumbing" } else { "selectedAdapterTasks" }, evaluator_digest: run.plan.evaluator_digest.clone(), tool_implementation_digest: run.tool_implementation_digest.clone(), host_source_digest: run.host_source_digest.clone(), plan_digest: digest(&run.plan), comparison, observations, usage_complete,
            limitations: vec!["Counts describe only these isolated fixtures; no statistical improvement or automatic promotion".into(), "Registered fixture tools are not full daemon/native-tool lifecycle acceptance".into(), "Only selected provider/model/instructions/temperature and bounded limits are projected; canonical memory, personality, plugins, fallbacks and account tools are excluded".into(), "Artifact rendering/calculation, model grading and human acceptance are unsupported in this fixture runner; artifact dimensions remain inconclusive".into(), "Source fingerprint covers the declared host runner/contracts, fixture tools, owner route, core runtime, provider router, profile/budget custody and dependency lock; it is not a binary attestation".into()], error: run.error.clone() })
    }

    pub(crate) async fn report(&self, id: &str) -> Result<RegressionReport, RegressionError> {
        self.verify_owner().await?;
        let state = self.state.read().await;
        let row = state
            .task_regressions
            .runs
            .iter()
            .find(|r| r.id == id)
            .ok_or(RegressionError::NotFound)?;
        self.project(&state, row)
    }

    pub(crate) async fn cancel(&self, id: &str) -> Result<RegressionReport, RegressionError> {
        let id = id.to_owned();
        self.transaction(move |service| async move {
            service.verify_owner().await?;
            let mut state = service.state.write().await;
            if let Some(signal) = state.task_regression_cancels.get(&id) { signal.cancel(); }
            let row = state.task_regressions.runs.iter_mut().find(|r| r.id == id).ok_or(RegressionError::NotFound)?;
            if row.status == RunStatus::Running { row.status = RunStatus::Cancelled; row.finished_at_ms = Some(now_millis()); row.error = Some("Cancelled; physical custody and uncertain usage remain retained until adapter retirement".into()); }
            let report = service.project(&state, state.task_regressions.runs.iter().find(|r| r.id == id).ok_or(RegressionError::NotFound)?)?;
            let persist = state.control_plane_persist_request(); drop(state);
            persist.save().await.map_err(|_| unavailable("Cancellation persistence unacknowledged"))?;
            Ok(report)
        }).await
    }

    pub(crate) async fn close(&self) {
        let _ = self
            .transaction(|service| async move {
                let mut state = service.state.write().await;
                state.task_regression_slots.close();
                for cancel in state.task_regression_cancels.values() {
                    cancel.cancel();
                }
                state.task_regressions.interrupt_active(now_millis());
                let persist = state.control_plane_persist_request();
                drop(state);
                persist
                    .save()
                    .await
                    .map_err(|_| unavailable("Shutdown comparison custody unacknowledged"))
            })
            .await;
    }
}

fn capture_variant(
    state: &crate::state::DaemonState,
    selection: &VariantSelection,
    suite: &RegressionSuite,
) -> Result<CapturedVariant, RegressionError> {
    let original = state
        .agents
        .get(&selection.agent_id)
        .ok_or(RegressionError::NotFound)?
        .config();
    if digest(original) != selection.config_digest || crate::agent_runs::is_helper_config(original)
    {
        return Err(conflict("Selected agent configuration changed"));
    }
    let provider = crate::runtime_model::canonical_provider(
        selection
            .provider
            .as_deref()
            .or(original.provider.as_deref())
            .unwrap_or_default(),
    );
    let model = selection
        .model
        .as_deref()
        .unwrap_or(&original.model)
        .trim()
        .to_owned();
    if !crate::runtime_model::is_routable_provider(&provider)
        || provider == "chatgpt"
        || model.is_empty()
        || model.len() > 256
    {
        return Err(RegressionError::Invalid("Choose an explicit supported bounded provider/model; subscription transport is not supported here".into()));
    }
    let system = selection.system.clone().or_else(|| original.system.clone()).unwrap_or_else(|| "Complete the user's isolated task using the permitted fixture tools. Preserve unrelated contents.".into());
    if system.len() > 16 * 1024 {
        return Err(RegressionError::Invalid(
            "Candidate instructions exceed bounds".into(),
        ));
    }
    let actor = ActorFixture {
        prompt: "Public descriptor envelope".into(),
        initial_files: BTreeMap::new(),
        readable_files: suite
            .fixtures
            .iter()
            .flat_map(|f| f.actor.readable_files.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect(),
        writable_files: suite
            .fixtures
            .iter()
            .flat_map(|f| f.actor.writable_files.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect(),
        mock_effects: suite.fixtures.iter().any(|f| f.actor.mock_effects),
    };
    // No workspace or private oracle is needed to construct public descriptors.
    let tools = fixture_tools::public_descriptors(
        &actor,
        selection.tool_policy == ToolPolicy::FilesAndMockEffects,
    );
    let original_settings = original.settings.clone().unwrap_or_default();
    let config = AgentConfig {
        name: "isolated-task-regression".into(),
        model: model.clone(),
        provider: Some(provider.clone()),
        system: Some(system),
        bio: None,
        lore: None,
        knowledge: None,
        topics: None,
        adjectives: None,
        style: None,
        plugins: None,
        tools: Some(tools),
        fallbacks: Vec::new(),
        settings: Some(AgentSettings {
            temperature: original_settings.temperature,
            max_tokens: Some(
                original_settings
                    .max_tokens
                    .unwrap_or(1024)
                    .min(2048)
                    .max(1),
            ),
            max_retries: Some(0),
            timeout_ms: None,
            max_tool_iterations: Some(
                original_settings
                    .max_tool_iterations
                    .unwrap_or(8)
                    .min(12)
                    .max(1),
            ),
            execution_limits: original_settings.execution_limits,
            delegation: None,
            additional: BTreeMap::new(),
        }),
    };
    Ok(CapturedVariant {
        pin: VariantSummaryPin {
            agent_id: selection.agent_id.clone(),
            config_digest: selection.config_digest.clone(),
            effective_config_digest: digest(&config),
            provider,
            model,
            tool_policy: selection.tool_policy,
        },
        config,
    })
}

fn core_pin(variant: &CapturedVariant, suite_digest: &str) -> Result<VariantPin, RegressionError> {
    Ok(VariantPin {
        settings: TraceSettings::from_config(&variant.config, suite_digest.into())
            .map_err(RegressionError::Invalid)?,
        source_digest: variant.pin.config_digest.clone(),
        tool_policy_digest: digest(&(
            variant.pin.tool_policy,
            &variant.config.tools,
            fixture_tools::fingerprint(),
        )),
    })
}
