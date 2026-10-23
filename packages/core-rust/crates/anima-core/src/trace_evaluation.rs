//! Opt-in ordinary-runtime trace measurement and independent task oracles.
//! Reports are evidence only; they confer no execution or promotion authority.
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    AgentConfig, Content, LockRecover, RunFrame, RunObserver, TaskResult, TaskStatus, TokenUsage,
};

pub const TRACE_VERSION: u32 = 1;
pub const MAX_TRACE_EVENTS: usize = 20_000;
pub const MAX_TRACE_BYTES: usize = 4 * 1024 * 1024;
const MAX_FILES: usize = 128;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskFixture {
    pub id: String,
    pub prompt: String,
    pub initial_files: BTreeMap<String, String>,
    pub expected_files: BTreeMap<String, String>,
    pub expected_output: String,
}

fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 256
        && !path.contains(['\\', ':', '\0'])
        && path
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != "..")
}

impl TaskFixture {
    fn validate(&self) -> Result<(), String> {
        if self.id.trim().is_empty()
            || self.id.len() > 128
            || self.prompt.is_empty()
            || self.prompt.len() > 65_536
            || self.expected_output.len() > 65_536
        {
            return Err("invalid task fixture identity or output bounds".into());
        }
        for tree in [&self.initial_files, &self.expected_files] {
            if tree.len() > MAX_FILES
                || tree.keys().any(|p| !safe_path(p))
                || tree.values().map(String::len).sum::<usize>() > MAX_TRACE_BYTES
            {
                return Err("invalid fixture filesystem oracle".into());
            }
        }
        Ok(())
    }
}

/// Binds both initial state and independently specified expected state/output.
pub fn fixture_set_digest(fixtures: &[TaskFixture]) -> Result<String, String> {
    if fixtures.is_empty() || fixtures.len() > 64 {
        return Err("invalid fixture count".into());
    }
    let mut ids = BTreeSet::new();
    for f in fixtures {
        f.validate()?;
        if !ids.insert(&f.id) {
            return Err("duplicate fixture id".into());
        }
    }
    let bytes = serde_jcs::to_vec(fixtures).map_err(|e| e.to_string())?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TraceSettings {
    pub provider: String,
    pub model: String,
    pub configuration_id: String,
    pub fixture_set_id: String,
}

impl TraceSettings {
    /// Prefer this to hand-written metadata. Include tool/policy/context settings
    /// not present in AgentConfig in the experiment identity used by the caller.
    pub fn from_config(config: &AgentConfig, fixture_set_id: String) -> Result<Self, String> {
        let provider = config
            .provider
            .clone()
            .filter(|v| !v.is_empty())
            .ok_or("trace config requires explicit provider")?;
        let configuration_id = hex::encode(Sha256::digest(
            serde_jcs::to_vec(config).map_err(|e| e.to_string())?,
        ));
        Ok(Self {
            provider,
            model: config.model.clone(),
            configuration_id,
            fixture_set_id,
        })
    }
    fn validate(&self) -> Result<(), String> {
        if [
            &self.provider,
            &self.model,
            &self.configuration_id,
            &self.fixture_set_id,
        ]
        .iter()
        .any(|s| s.trim().is_empty() || s.len() > 4096)
        {
            return Err("missing experiment settings".into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TraceVariant {
    Baseline,
    Candidate,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum TraceEventKind {
    ModelStarted {
        step_id: String,
    },
    Usage {
        step_id: String,
        usage: TokenUsage,
    },
    Served {
        step_id: String,
        provider: String,
        model: String,
    },
    ModelFinished {
        step_id: String,
    },
    ToolStarted {
        step_id: String,
        call_id: String,
        name: String,
        arguments: String,
    },
    ToolFinished {
        step_id: String,
        call_id: String,
        name: String,
        success: bool,
        result: String,
        recovered: bool,
    },
    RunFinished {
        success: bool,
        output: Option<String>,
        error: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TraceEvent {
    pub sequence: u64,
    pub elapsed_ms: u64,
    pub event: TraceEventKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskTrace {
    pub version: u32,
    pub settings: TraceSettings,
    pub fixture_id: String,
    pub repetition: u32,
    pub variant: TraceVariant,
    pub events: Vec<TraceEvent>,
}

#[derive(Default)]
struct CaptureState {
    events: Vec<TraceEvent>,
    bytes: usize,
    fault: Option<String>,
    finished: bool,
}

/// Inline bounded observer with no I/O. Dedicated to a single ordinary run.
/// Attach via `AgentRuntime::set_run_observer`, then call `finish` exactly once
/// with the actual return value of the awaited runtime invocation.
pub struct TraceCapture {
    settings: TraceSettings,
    fixture_id: String,
    repetition: u32,
    variant: TraceVariant,
    started: Instant,
    state: Mutex<CaptureState>,
}

impl TraceCapture {
    pub fn new(
        settings: TraceSettings,
        fixture_id: impl Into<String>,
        repetition: u32,
        variant: TraceVariant,
    ) -> Self {
        Self {
            settings,
            fixture_id: fixture_id.into(),
            repetition,
            variant,
            started: Instant::now(),
            state: Mutex::new(CaptureState::default()),
        }
    }
    fn append(&self, state: &mut CaptureState, event: TraceEventKind) {
        if state.fault.is_some() || state.finished {
            return;
        }
        let bytes = match serde_json::to_vec(&event) {
            Ok(bytes) => bytes.len(),
            Err(e) => {
                state.fault = Some(e.to_string());
                return;
            }
        };
        if state.events.len() >= MAX_TRACE_EVENTS
            || bytes > MAX_TRACE_BYTES.saturating_sub(state.bytes)
        {
            state.fault = Some("trace capture limit reached; incomplete evidence".into());
            return;
        }
        state.bytes += bytes;
        state.events.push(TraceEvent {
            sequence: state.events.len() as u64 + 1,
            elapsed_ms: u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX),
            event,
        });
    }
    pub fn finish(&self, result: &TaskResult<Content>) -> Result<TaskTrace, String> {
        let mut state = self.state.lock_recover();
        if state.finished {
            return Err("capture already finished".into());
        }
        self.append(
            &mut state,
            TraceEventKind::RunFinished {
                success: result.status == TaskStatus::Success,
                output: result.data.as_ref().map(|c| c.text.clone()),
                error: result.error.clone(),
            },
        );
        state.finished = true;
        if let Some(fault) = &state.fault {
            return Err(fault.clone());
        }
        let trace = TaskTrace {
            version: TRACE_VERSION,
            settings: self.settings.clone(),
            fixture_id: self.fixture_id.clone(),
            repetition: self.repetition,
            variant: self.variant,
            events: state.events.clone(),
        };
        trace.metrics()?;
        Ok(trace)
    }
}

impl RunObserver for TraceCapture {
    fn on_frame(&self, frame: RunFrame) {
        let mut state = self.state.lock_recover();
        let event = match frame {
            RunFrame::StepStarted { step_id } => TraceEventKind::ModelStarted { step_id },
            RunFrame::StepUsage { step_id, usage } => TraceEventKind::Usage { step_id, usage },
            RunFrame::ModelServed {
                step_id,
                provider,
                model,
                ..
            } => TraceEventKind::Served {
                step_id,
                provider,
                model,
            },
            RunFrame::StepFinished { step_id, .. } => TraceEventKind::ModelFinished { step_id },
            RunFrame::ToolStarted { step_id, tool_call } => {
                let arguments = match serde_jcs::to_string(&tool_call.args) {
                    Ok(a) => a,
                    Err(e) => {
                        state.fault = Some(e.to_string());
                        return;
                    }
                };
                TraceEventKind::ToolStarted {
                    step_id,
                    call_id: tool_call.id,
                    name: tool_call.name,
                    arguments,
                }
            }
            RunFrame::ToolFinished {
                step_id,
                tool_call_id,
                name,
                status,
                result,
                recovered,
                ..
            } => TraceEventKind::ToolFinished {
                step_id,
                call_id: tool_call_id,
                name,
                success: status == TaskStatus::Success,
                result,
                recovered,
            },
            RunFrame::TextDelta { .. } => return,
            RunFrame::Steered { .. } => {
                state.fault = Some("owner steering changed the fixture input".into());
                return;
            }
        };
        self.append(&mut state, event);
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TraceMetrics {
    pub model_calls: u64,
    pub tool_calls: u64,
    pub tool_errors: u64,
    pub run_errors: u64,
    pub exact_repeat_calls: u64,
    pub recovered_tool_results: u64,
    pub result_bytes: u64,
    pub final_output_bytes: u64,
    pub known_usage_calls: u64,
    pub unknown_usage_calls: u64,
    pub known_usage: Option<TokenUsage>,
    pub elapsed_ms: u64,
}

impl TaskTrace {
    /// Validates completeness before returning any metrics. Runtime model-step
    /// boundaries are distinct from unobservable adapter transport attempts.
    pub fn metrics(&self) -> Result<TraceMetrics, String> {
        self.settings.validate()?;
        if self.version != TRACE_VERSION
            || self.fixture_id.trim().is_empty()
            || self.fixture_id.len() > 128
            || self.events.is_empty()
            || self.events.len() > MAX_TRACE_EVENTS
        {
            return Err("missing or unsupported trace".into());
        }
        let mut metrics = TraceMetrics::default();
        // Step state retains identity uniqueness, model phase and usage phase.
        let mut steps: BTreeMap<&str, (bool, bool)> = BTreeMap::new(); // finished, usage observed
        let mut current_step: Option<&str> = None;
        let mut served_steps = BTreeSet::new();
        // The ordinary runtime permits duplicate tool-call IDs in a batch;
        // retain every pending invocation instead of overwriting one by ID.
        let mut tools: BTreeMap<(&str, &str), Vec<&str>> = BTreeMap::new();
        let mut signatures = BTreeSet::new();
        let mut terminal = false;
        let mut bytes = 0usize;
        for (index, item) in self.events.iter().enumerate() {
            bytes = bytes.saturating_add(
                serde_json::to_vec(&item.event)
                    .map_err(|e| e.to_string())?
                    .len(),
            );
            if terminal
                || item.sequence != index as u64 + 1
                || item.elapsed_ms < metrics.elapsed_ms
                || bytes > MAX_TRACE_BYTES
            {
                return Err("trace ordering, sequence or bounds invalid".into());
            }
            metrics.elapsed_ms = item.elapsed_ms;
            match &item.event {
                TraceEventKind::ModelStarted { step_id } => {
                    if step_id.is_empty()
                        || steps.values().any(|v| !v.0)
                        || steps.insert(step_id, (false, false)).is_some()
                        || !tools.is_empty()
                    {
                        return Err("duplicate model start or unfinished tool".into());
                    }
                    current_step = Some(step_id);
                    metrics.model_calls += 1;
                }
                TraceEventKind::Usage { step_id, usage } => {
                    let step = steps
                        .get_mut(step_id.as_str())
                        .ok_or("usage without model start")?;
                    if current_step != Some(step_id.as_str()) || step.0 || step.1 {
                        return Err("duplicate or late step usage".into());
                    }
                    step.1 = true;
                    // Default all-zero TokenUsage has no reported/unknown flag.
                    // Conservatively keep it unknown rather than inventing zero.
                    if usage.total_tokens > 0 {
                        if usage.prompt_tokens.checked_add(usage.completion_tokens)
                            != Some(usage.total_tokens)
                            || usage.cached_prompt_tokens > usage.prompt_tokens
                            || usage.reasoning_tokens > usage.completion_tokens
                        {
                            return Err("inconsistent reported usage".into());
                        }
                        metrics.known_usage_calls += 1;
                        metrics
                            .known_usage
                            .get_or_insert_with(TokenUsage::default)
                            .saturating_add(usage);
                    }
                }
                TraceEventKind::Served {
                    step_id,
                    provider,
                    model,
                } => {
                    if current_step != Some(step_id.as_str())
                        || steps.get(step_id.as_str()) != Some(&(false, false))
                        || !served_steps.insert(step_id.as_str())
                        || provider != &self.settings.provider
                        || model != &self.settings.model
                    {
                        return Err(
                            "late, duplicate or unmatched actual served provider/model".into()
                        );
                    }
                }
                TraceEventKind::ModelFinished { step_id } => {
                    let step = steps
                        .get_mut(step_id.as_str())
                        .ok_or("model finish without start")?;
                    if current_step != Some(step_id.as_str()) || step.0 {
                        return Err("misattributed or duplicate model finish".into());
                    }
                    step.0 = true;
                }
                TraceEventKind::ToolStarted {
                    step_id,
                    call_id,
                    name,
                    arguments,
                } => {
                    if current_step != Some(step_id.as_str())
                        || !steps.get(step_id.as_str()).is_some_and(|step| step.0)
                        || call_id.is_empty()
                        || name.is_empty()
                    {
                        return Err("tool start outside current finished-model phase".into());
                    }
                    let args: serde_json::Value =
                        serde_json::from_str(arguments).map_err(|e| e.to_string())?;
                    if serde_jcs::to_string(&args).map_err(|e| e.to_string())? != *arguments {
                        return Err("noncanonical tool arguments".into());
                    }
                    tools
                        .entry((step_id.as_str(), call_id.as_str()))
                        .or_default()
                        .push(name.as_str());
                    metrics.tool_calls += 1;
                    if !signatures.insert((name, arguments)) {
                        metrics.exact_repeat_calls += 1;
                    }
                }
                TraceEventKind::ToolFinished {
                    step_id,
                    call_id,
                    name,
                    success,
                    result,
                    recovered,
                } => {
                    if current_step != Some(step_id.as_str())
                        || !steps.get(step_id.as_str()).is_some_and(|step| step.0)
                    {
                        return Err("tool finish outside current finished-model phase".into());
                    }
                    let key = (step_id.as_str(), call_id.as_str());
                    let pending = tools
                        .get_mut(&key)
                        .ok_or("unmatched or duplicate tool finish")?;
                    let index = pending
                        .iter()
                        .position(|n| *n == name.as_str())
                        .ok_or("tool finish name mismatch")?;
                    pending.remove(index);
                    if pending.is_empty() {
                        tools.remove(&key);
                    }
                    metrics.tool_errors += u64::from(!success);
                    metrics.recovered_tool_results += u64::from(*recovered);
                    metrics.result_bytes += result.len() as u64;
                }
                TraceEventKind::RunFinished {
                    success,
                    output,
                    error,
                } => {
                    if steps.is_empty()
                        || steps.values().any(|v| !v.0)
                        || !tools.is_empty()
                        || (*success && (output.is_none() || error.is_some()))
                        || (!success && error.is_none())
                    {
                        return Err(
                            "incomplete runtime trace or contradictory terminal result".into()
                        );
                    }
                    metrics.run_errors = u64::from(!success);
                    metrics.final_output_bytes = output.as_ref().map_or(0, |s| s.len() as u64);
                    terminal = true;
                }
            }
        }
        if !terminal {
            return Err("missing runtime terminal result".into());
        }
        metrics.unknown_usage_calls = metrics.model_calls - metrics.known_usage_calls;
        Ok(metrics)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GradedTrace {
    pub trace: TaskTrace,
    pub oracle_failures: Vec<String>,
    /// Independent filesystem observations, retained so comparison regrades
    /// against its actual fixture instead of trusting a serialized verdict.
    pub observed_files: BTreeMap<String, String>,
    pub observed_directories: BTreeSet<String>,
}

fn snapshot_tree(root: &Path) -> Result<(BTreeMap<String, String>, BTreeSet<String>), String> {
    let meta = std::fs::symlink_metadata(root).map_err(|e| e.to_string())?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err("oracle root must be an isolated directory".into());
    }
    let mut files = BTreeMap::new();
    let mut directories = BTreeSet::new();
    let mut bytes = 0u64;
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            let meta = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
            if meta.file_type().is_symlink() {
                return Err("symlink is not valid oracle evidence".into());
            }
            let relative = path
                .strip_prefix(root)
                .map_err(|e| e.to_string())?
                .to_str()
                .ok_or("non-UTF-8 oracle path")?
                .replace('\\', "/");
            if !safe_path(&relative) {
                return Err("unsafe oracle path".into());
            }
            if meta.is_dir() {
                directories.insert(relative);
                pending.push(path);
            } else if meta.is_file() {
                bytes = bytes.saturating_add(meta.len());
                if files.len() >= MAX_FILES || bytes > MAX_TRACE_BYTES as u64 {
                    return Err("oracle tree exceeded bounds".into());
                }
                files.insert(
                    relative,
                    std::fs::read_to_string(path).map_err(|e| e.to_string())?,
                );
            } else {
                return Err("special file is not valid oracle evidence".into());
            }
            if directories.len() > MAX_FILES {
                return Err("oracle directory bound exceeded".into());
            }
        }
    }
    Ok((files, directories))
}

/// Read-only grading: neither model claims nor successful unrelated checks can
/// replace exact independently specified workspace/output observations.
pub fn grade_fixture(
    fixture: &TaskFixture,
    trace: TaskTrace,
    root: &Path,
) -> Result<GradedTrace, String> {
    fixture.validate()?;
    trace.metrics()?;
    if trace.fixture_id != fixture.id {
        return Err("trace belongs to a different fixture".into());
    }
    let (files, directories) = snapshot_tree(root)?;
    let failures = oracle_failures(fixture, &trace, &files, &directories);
    Ok(GradedTrace {
        trace,
        oracle_failures: failures,
        observed_files: files,
        observed_directories: directories,
    })
}

fn oracle_failures(
    fixture: &TaskFixture,
    trace: &TaskTrace,
    files: &BTreeMap<String, String>,
    directories: &BTreeSet<String>,
) -> Vec<String> {
    let expected_directories: BTreeSet<String> = fixture
        .expected_files
        .keys()
        .flat_map(|p| {
            p.match_indices('/')
                .map(|(i, _)| p[..i].to_string())
                .collect::<Vec<_>>()
        })
        .collect();
    let mut failures = Vec::new();
    if directories != &expected_directories {
        failures.push("workspace directory set differs".into());
    }
    for (path, expected) in &fixture.expected_files {
        if files.get(path) != Some(expected) {
            failures.push(format!("file missing or differs: {path}"));
        }
    }
    for path in files.keys() {
        if !fixture.expected_files.contains_key(path) {
            failures.push(format!("unrequested file: {path}"));
        }
    }
    match trace.events.last().map(|e| &e.event) {
        Some(TraceEventKind::RunFinished {
            success: true,
            output: Some(output),
            ..
        }) if output == &fixture.expected_output => (),
        _ => failures.push("terminal output or runtime status differs".into()),
    }
    failures
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VariantSummary {
    pub trials: u32,
    pub correct: u32,
    pub metrics: TraceMetrics,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TraceComparison {
    pub version: u32,
    pub settings: TraceSettings,
    pub repetitions: u32,
    pub baseline: VariantSummary,
    pub candidate: VariantSummary,
}

fn add_metrics(to: &mut TraceMetrics, from: &TraceMetrics) {
    to.model_calls += from.model_calls;
    to.tool_calls += from.tool_calls;
    to.tool_errors += from.tool_errors;
    to.run_errors += from.run_errors;
    to.exact_repeat_calls += from.exact_repeat_calls;
    to.recovered_tool_results += from.recovered_tool_results;
    to.result_bytes += from.result_bytes;
    to.final_output_bytes += from.final_output_bytes;
    to.known_usage_calls += from.known_usage_calls;
    to.unknown_usage_calls += from.unknown_usage_calls;
    to.elapsed_ms = to.elapsed_ms.saturating_add(from.elapsed_ms);
    if let Some(usage) = &from.known_usage {
        to.known_usage
            .get_or_insert_with(TokenUsage::default)
            .saturating_add(usage);
    }
}

/// Rejects missing, duplicate or incompatible evidence instead of comparing
/// different fixtures/providers/configurations or silently reducing repetitions.
pub fn compare_traces(
    fixtures: &[TaskFixture],
    repetitions: u32,
    trials: &[GradedTrace],
) -> Result<TraceComparison, String> {
    let fixture_set_id = fixture_set_digest(fixtures)?;
    if !(1..=32).contains(&repetitions) || trials.len() != fixtures.len() * repetitions as usize * 2
    {
        return Err("incomplete matched trial matrix".into());
    }
    let settings = trials
        .first()
        .ok_or("missing traces")?
        .trace
        .settings
        .clone();
    settings.validate()?;
    if settings.fixture_set_id != fixture_set_id {
        return Err("trace fixture-set binding mismatch".into());
    }
    let ids: BTreeSet<&str> = fixtures.iter().map(|f| f.id.as_str()).collect();
    let mut seen = BTreeSet::new();
    let mut report = TraceComparison {
        version: TRACE_VERSION,
        settings: settings.clone(),
        repetitions,
        baseline: VariantSummary::default(),
        candidate: VariantSummary::default(),
    };
    for trial in trials {
        let trace = &trial.trace;
        if trace.settings != settings
            || !ids.contains(trace.fixture_id.as_str())
            || trace.repetition >= repetitions
            || !seen.insert((trace.fixture_id.as_str(), trace.repetition, trace.variant))
        {
            return Err("mismatched settings, fixture, repetition or duplicate trial".into());
        }
        let metrics = trace.metrics()?;
        let summary = match trace.variant {
            TraceVariant::Baseline => &mut report.baseline,
            TraceVariant::Candidate => &mut report.candidate,
        };
        summary.trials += 1;
        let fixture = fixtures
            .iter()
            .find(|f| f.id == trace.fixture_id)
            .ok_or("unknown fixture")?;
        if trial.observed_files.len() > MAX_FILES
            || trial.observed_directories.len() > MAX_FILES
            || trial
                .observed_files
                .keys()
                .chain(trial.observed_directories.iter())
                .any(|p| !safe_path(p))
            || trial
                .observed_files
                .values()
                .map(String::len)
                .sum::<usize>()
                > MAX_TRACE_BYTES
        {
            return Err("invalid recorded workspace observation".into());
        }
        let failures = oracle_failures(
            fixture,
            trace,
            &trial.observed_files,
            &trial.observed_directories,
        );
        if failures != trial.oracle_failures {
            return Err("serialized oracle verdict does not match observed evidence".into());
        }
        summary.correct += u32::from(failures.is_empty());
        add_metrics(&mut summary.metrics, &metrics);
    }
    Ok(report)
}
