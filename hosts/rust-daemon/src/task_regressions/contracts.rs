use std::collections::BTreeMap;

use anima_core::task_regressions::{
    ComparisonPlan, RegressionComparison, RegressionSuite, TrialObservation,
};
use anima_core::trace_evaluation::{TraceEventKind, TraceMetrics, TraceVariant};
use anima_core::AgentConfig;
use serde::{Deserialize, Serialize};

pub(super) const MAX_SUITES: usize = 16;
pub(super) const MAX_REPORTS: usize = 16;
pub(super) const MAX_DAILY_RUNS: usize = 8;
pub(super) const REPORT_BYTES: usize = 16 * 1024 * 1024;
pub(super) const RUN_TIMEOUT_MS: u64 = 120_000;

#[derive(Clone, Debug)]
pub(crate) enum RegressionError {
    NotFound,
    Invalid(String),
    Conflict(String),
    Unavailable(String),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum ToolPolicy {
    Files,
    #[default]
    FilesAndMockEffects,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct VariantSelection {
    pub(super) agent_id: String,
    pub(super) config_digest: String,
    pub(super) model: Option<String>,
    pub(super) provider: Option<String>,
    pub(super) system: Option<String>,
    #[serde(default)]
    pub(super) tool_policy: ToolPolicy,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CompareRequest {
    pub(super) suite_id: String,
    pub(super) suite_digest: String,
    pub(super) baseline: VariantSelection,
    pub(super) candidate: VariantSelection,
    pub(super) repetitions: u32,
    pub(super) request_key: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SuiteSummary {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) digest: String,
    pub(super) fixture_count: usize,
    pub(super) created_at_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct StoredSuite {
    pub(super) summary: SuiteSummary,
    pub(super) suite: RegressionSuite,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct VariantSummaryPin {
    pub(super) agent_id: String,
    pub(super) config_digest: String,
    pub(super) effective_config_digest: String,
    pub(super) provider: String,
    pub(super) model: String,
    pub(super) tool_policy: ToolPolicy,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CapturedVariant {
    pub(super) pin: VariantSummaryPin,
    pub(super) config: AgentConfig,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum RunStatus {
    Running,
    Completed,
    Inconclusive,
    Cancelled,
    Interrupted,
    Stale,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct StoredRun {
    pub(super) id: String,
    pub(super) request: CompareRequest,
    pub(super) request_digest: String,
    pub(super) suite: StoredSuite,
    pub(super) baseline: CapturedVariant,
    pub(super) candidate: CapturedVariant,
    pub(super) plan: ComparisonPlan,
    pub(super) tool_implementation_digest: String,
    pub(super) host_source_digest: String,
    pub(super) status: RunStatus,
    pub(super) observations: Vec<TrialObservation>,
    pub(super) created_at_ms: u64,
    pub(super) finished_at_ms: Option<u64>,
    pub(super) retired_at_ms: Option<u64>,
    pub(super) error: Option<String>,
    #[serde(default)]
    pub(super) usage_rows: Vec<String>,
    #[serde(default)]
    pub(super) usage_attestations: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RegressionRegistry {
    pub(super) suites: Vec<StoredSuite>,
    pub(super) runs: Vec<StoredRun>,
    pub(super) reservations: Vec<u64>,
    #[serde(default)]
    pub(super) admissions: BTreeMap<String, (String, String)>,
}

impl RegressionRegistry {
    #[cfg(test)]
    pub(crate) fn run_count(&self) -> usize {
        self.runs.len()
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.suites.len() > MAX_SUITES
            || self.runs.len() > MAX_REPORTS
            || self.reservations.len() > MAX_DAILY_RUNS
            || self.admissions.len() > 1024
        {
            return Err("task regression registry exceeded bounds".into());
        }
        let mut ids = std::collections::BTreeSet::new();
        for suite in &self.suites {
            if !ids.insert(&suite.summary.id)
                || suite.suite.digest()? != suite.summary.digest
                || suite.summary.fixture_count != suite.suite.fixtures.len()
            {
                return Err("task regression suite identity mismatch".into());
            }
        }
        ids.clear();
        for run in &self.runs {
            if !ids.insert(&run.id)
                || run.suite.suite.digest()? != run.plan.suite_digest
                || self.admissions.get(&run.request.request_key)
                    != Some(&(run.request_digest.clone(), run.id.clone()))
                || run.observations.len()
                    > run.suite.suite.fixtures.len() * run.plan.repetitions as usize * 2
                || serde_json::to_vec(run)
                    .map_err(|_| "invalid regression record")?
                    .len()
                    > REPORT_BYTES
            {
                return Err("task regression run identity or bounds mismatch".into());
            }
            // Historical evaluator pins are retained as facts; restore never executes or regrades them.
        }
        Ok(())
    }

    pub(crate) fn interrupt_active(&mut self, now: u64) {
        for run in &mut self.runs {
            if run.retired_at_ms.is_none() {
                if run.status == RunStatus::Running {
                    run.status = RunStatus::Interrupted;
                }
                run.finished_at_ms.get_or_insert(now);
                run.error = Some("Restart interrupted this comparison; uncertain trials will not replay and usage may be incomplete".into());
            }
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConfigurationSummary {
    pub(super) agent_id: String,
    pub(super) config_digest: String,
    pub(super) provider: Option<String>,
    pub(super) model: String,
    pub(super) system: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PublicTrial {
    pub(super) fixture_id: String,
    pub(super) repetition: u32,
    pub(super) variant: TraceVariant,
    pub(super) observation_digest: String,
    pub(super) observed_files: BTreeMap<String, String>,
    pub(super) observed_mock_effects: Vec<anima_core::task_regressions::MockEffect>,
    pub(super) final_output: Option<String>,
    pub(super) metrics: TraceMetrics,
    pub(super) artifact_refs: Vec<anima_core::task_regressions::ArtifactObservationRef>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RegressionReport {
    pub(super) id: String,
    pub(super) status: RunStatus,
    pub(super) suite: SuiteSummary,
    pub(super) baseline: VariantSummaryPin,
    pub(super) candidate: VariantSummaryPin,
    pub(super) repetitions: u32,
    pub(super) planned_trials: usize,
    pub(super) completed_trials: usize,
    pub(super) created_at_ms: u64,
    pub(super) finished_at_ms: Option<u64>,
    pub(super) retired_at_ms: Option<u64>,
    pub(super) evidence_scope: &'static str,
    pub(super) evaluator_digest: String,
    pub(super) tool_implementation_digest: String,
    pub(super) host_source_digest: String,
    pub(super) plan_digest: String,
    pub(super) comparison: Option<RegressionComparison>,
    pub(super) observations: Vec<PublicTrial>,
    pub(super) usage_complete: bool,
    pub(super) limitations: Vec<String>,
    pub(super) error: Option<String>,
}

pub(super) fn observation_view(value: &TrialObservation) -> Result<PublicTrial, String> {
    Ok(PublicTrial {
        fixture_id: value.graded.trace.fixture_id.clone(),
        repetition: value.graded.trace.repetition,
        variant: value.graded.trace.variant,
        observation_digest: anima_core::skill_evaluation::digest(value),
        observed_files: value.graded.observed_files.clone(),
        observed_mock_effects: value.mock_effects.clone(),
        final_output: value
            .graded
            .trace
            .events
            .last()
            .and_then(|event| match &event.event {
                TraceEventKind::RunFinished { output, .. } => output.clone(),
                _ => None,
            }),
        metrics: value.graded.trace.metrics()?,
        artifact_refs: value.artifact_refs.clone(),
    })
}
