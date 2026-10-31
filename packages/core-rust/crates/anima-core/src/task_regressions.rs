//! Planned ordinary-runtime task comparisons with private, independent final-state oracles.
//!
//! Hosts retain physical mutation custody and freeze observations before calling this module.
//! Only [`ActorFixture`] may enter the actor's input or capability construction. Suites, plans,
//! private oracles, grading differences and reports belong to the trusted owner/controller.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::trace_evaluation::{
    self, GradedTrace, TaskFixture, TraceEventKind, TraceSettings, TraceVariant, VariantSummary,
};

/// Bounds keep the owner-facing initial task matrix small and its evidence finite.
pub const MAX_REGRESSION_FIXTURES: usize = 16;
/// Maximum distinct flat filenames across one fixture's public envelope and private oracle.
pub const MAX_REGRESSION_FILES: usize = 64;
/// Maximum UTF-8 bytes in any initial, expected or observed fixture file.
pub const MAX_REGRESSION_FILE_BYTES: usize = 64 * 1024;
/// Maximum canonical serialized bytes in a complete private suite definition.
pub const MAX_REGRESSION_SUITE_BYTES: usize = 4 * 1024 * 1024;
/// Maximum UTF-8 bytes in an actor prompt or private expected terminal output.
pub const MAX_REGRESSION_TEXT_BYTES: usize = 16 * 1024;
const MAX_NAME_BYTES: usize = 128;
const MAX_OBSERVATION_REFS: usize = 64;
const MAX_LOCATOR_BYTES: usize = 256;
const MAX_MOCK_EFFECTS: usize = 64;

/// A bounded saved task suite; the full value is private to its trusted controller.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RegressionSuite {
    pub name: String,
    pub fixtures: Vec<RegressionFixture>,
}

/// Separates public actor inputs from private expected observations.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RegressionFixture {
    pub id: String,
    pub actor: ActorFixture,
    pub oracle: FixtureOracle,
}

/// The only fixture material that may be given to runtime or fixture tools.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActorFixture {
    pub prompt: String,
    #[serde(default)]
    pub initial_files: BTreeMap<String, String>,
    #[serde(default)]
    pub readable_files: Vec<String>,
    #[serde(default)]
    pub writable_files: Vec<String>,
    #[serde(default)]
    pub mock_effects: bool,
}

/// Hidden expected outcomes, used only after the actor's mutation authority retires.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FixtureOracle {
    #[serde(default)]
    pub expected_files: BTreeMap<String, String>,
    pub expected_output: String,
    #[serde(default)]
    pub expected_mock_effects: Vec<MockEffect>,
}

/// One controller-owned synthetic effect; it never represents live external delivery.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MockEffect {
    pub key: String,
    pub payload: String,
}

/// One immutable effective actor configuration and its source/tool-policy identities.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VariantPin {
    pub settings: TraceSettings,
    pub source_digest: String,
    pub tool_policy_digest: String,
}

/// Admission-time pins for the exact declared matrix, including an optional A/A control.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ComparisonPlan {
    pub suite_digest: String,
    pub repetitions: u32,
    pub baseline: VariantPin,
    pub candidate: VariantPin,
    pub evaluator_digest: String,
}

/// A typed reference to an authoritative inspection fact, without a derived quality verdict.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtifactObservationRef {
    pub report_sha256: String,
    pub observation_locator: String,
}

/// Frozen real observations, supplied only by the trusted post-retirement controller.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TrialObservation {
    /// Effective source, configuration and tool policy stamped by the trusted retired trial.
    pub variant_pin: VariantPin,
    pub graded: GradedTrace,
    #[serde(default)]
    pub mock_effects: Vec<MockEffect>,
    #[serde(default)]
    pub artifact_refs: Vec<ArtifactObservationRef>,
}

/// Independently recomputed outcome for a unique planned trial.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RegressionTrialGrade {
    pub fixture_id: String,
    pub repetition: u32,
    pub variant: TraceVariant,
    pub failures: Vec<String>,
}

/// Complete matrix evidence, with no configuration promotion or artifact-quality authority.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RegressionComparison {
    pub matrix_complete: bool,
    pub baseline: VariantSummary,
    pub candidate: VariantSummary,
    pub trials: Vec<RegressionTrialGrade>,
}

fn flat_filename(path: &str) -> bool {
    if path.is_empty()
        || path.len() > MAX_NAME_BYTES
        || path.starts_with('.')
        || path.ends_with('.')
        || path.contains("..")
        || !path
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return false;
    }
    let stem = path
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        && !(stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
}

fn validate_paths<'a>(paths: impl IntoIterator<Item = &'a str>) -> Result<(), String> {
    let mut names = BTreeMap::new();
    for path in paths {
        if !flat_filename(path) {
            return Err("invalid flat fixture filename".into());
        }
        if let Some(existing) = names.insert(path.to_ascii_lowercase(), path) {
            if existing != path {
                return Err("case-aliased fixture filenames".into());
            }
        }
        if names.len() > MAX_REGRESSION_FILES {
            return Err("fixture filename count exceeds bounds".into());
        }
    }
    Ok(())
}

fn validate_file_map(files: &BTreeMap<String, String>) -> Result<(), String> {
    if files.len() > MAX_REGRESSION_FILES
        || files
            .values()
            .any(|value| value.len() > MAX_REGRESSION_FILE_BYTES)
    {
        return Err("fixture file bounds exceeded".into());
    }
    validate_paths(files.keys().map(String::as_str))
}

fn validate_capability_paths(paths: &[String]) -> Result<(), String> {
    if paths.len() > MAX_REGRESSION_FILES {
        return Err("fixture capability bounds exceeded".into());
    }
    let unique: BTreeSet<_> = paths.iter().map(|path| path.to_ascii_lowercase()).collect();
    if unique.len() != paths.len() {
        return Err("duplicate fixture path capability".into());
    }
    validate_paths(paths.iter().map(String::as_str))
}

fn validate_effects(effects: &[MockEffect]) -> Result<(), String> {
    if effects.len() > MAX_MOCK_EFFECTS
        || effects.iter().any(|effect| {
            effect.key.is_empty()
                || effect.key.len() > MAX_NAME_BYTES
                || !effect.key.bytes().all(|byte| byte.is_ascii_graphic())
                || effect.payload.len() > MAX_REGRESSION_FILE_BYTES
        })
    {
        return Err("invalid mock-effect observations".into());
    }
    Ok(())
}

fn sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

impl ActorFixture {
    /// Validates an independently authored, portable, flat file capability envelope.
    ///
    /// # Errors
    /// Rejects unsafe or aliased filenames, duplicate capabilities and oversized actor inputs.
    pub fn validate(&self) -> Result<(), String> {
        if self.prompt.trim().is_empty() || self.prompt.len() > MAX_REGRESSION_TEXT_BYTES {
            return Err("invalid actor prompt bounds".into());
        }
        validate_file_map(&self.initial_files)?;
        validate_capability_paths(&self.readable_files)?;
        validate_capability_paths(&self.writable_files)?;
        validate_paths(
            self.initial_files
                .keys()
                .chain(&self.readable_files)
                .chain(&self.writable_files)
                .map(String::as_str),
        )
    }
}

impl RegressionFixture {
    fn validate(&self) -> Result<(), String> {
        if self.id.trim().is_empty() || self.id.len() > MAX_NAME_BYTES {
            return Err("invalid regression fixture identity".into());
        }
        self.actor.validate()?;
        validate_file_map(&self.oracle.expected_files)?;
        if self.oracle.expected_output.len() > MAX_REGRESSION_TEXT_BYTES {
            return Err("expected output bounds exceeded".into());
        }
        validate_effects(&self.oracle.expected_mock_effects)?;
        // Oracle paths participate only in portable-name validation, never tool permissions.
        validate_paths(
            self.actor
                .initial_files
                .keys()
                .chain(&self.actor.readable_files)
                .chain(&self.actor.writable_files)
                .chain(self.oracle.expected_files.keys())
                .map(String::as_str),
        )
    }

    /// Materializes the hidden grader input after the actor's mutation authority has retired.
    #[must_use]
    pub fn task_fixture(&self) -> TaskFixture {
        TaskFixture {
            id: self.id.clone(),
            prompt: self.actor.prompt.clone(),
            initial_files: self.actor.initial_files.clone(),
            expected_files: self.oracle.expected_files.clone(),
            expected_output: self.oracle.expected_output.clone(),
        }
    }
}

impl RegressionSuite {
    /// Validates bounded fixtures while keeping capabilities independent of expected results.
    ///
    /// # Errors
    /// Rejects invalid names, empty/duplicate/oversized cases and suites exceeding 4 MiB.
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty()
            || self.name.len() > MAX_NAME_BYTES
            || self.fixtures.is_empty()
            || self.fixtures.len() > MAX_REGRESSION_FIXTURES
        {
            return Err("invalid regression suite identity or fixture count".into());
        }
        let mut ids = BTreeSet::new();
        for fixture in &self.fixtures {
            fixture.validate()?;
            if !ids.insert(&fixture.id) {
                return Err("duplicate regression fixture identity".into());
            }
        }
        if serde_jcs::to_vec(self)
            .map_err(|error| error.to_string())?
            .len()
            > MAX_REGRESSION_SUITE_BYTES
        {
            return Err("regression suite exceeds total byte bounds".into());
        }
        Ok(())
    }

    /// Hashes the complete validated suite, including private oracles and public capabilities.
    ///
    /// Both variant settings use this digest as `fixture_set_id`; it intentionally differs from
    /// the legacy trace evaluator's digest of [`TaskFixture`] values.
    ///
    /// # Errors
    /// Returns validation or canonical serialization errors.
    pub fn digest(&self) -> Result<String, String> {
        self.validate()?;
        let bytes = serde_jcs::to_vec(self).map_err(|error| error.to_string())?;
        Ok(hex::encode(Sha256::digest(bytes)))
    }
}

impl ArtifactObservationRef {
    /// Checks only reference syntax; resolution and artifact-quality inspection belong to the host.
    ///
    /// # Errors
    /// Rejects malformed digests and empty, oversized or unsupported observation locators.
    pub fn validate(&self) -> Result<(), String> {
        if !sha256(&self.report_sha256)
            || self.observation_locator.is_empty()
            || self.observation_locator.len() > MAX_LOCATOR_BYTES
            || !self.observation_locator.bytes().all(|byte| {
                byte.is_ascii_alphanumeric()
                    || matches!(byte, b'.' | b'_' | b':' | b'/' | b'#' | b'-')
            })
        {
            return Err("invalid artifact observation reference".into());
        }
        Ok(())
    }
}

impl ComparisonPlan {
    /// Checks suite, per-side configuration and the currently compiled deterministic evaluator.
    ///
    /// Equal variant pins are permitted for explicit A/A controls. Candidate runtime/evaluator
    /// builds are unsupported: both sides use one trusted compiled checker.
    ///
    /// # Errors
    /// Rejects missing settings, mismatched pins, unsupported repetitions or evaluator changes.
    pub fn validate(&self, suite: &RegressionSuite) -> Result<(), String> {
        if !(1..=3).contains(&self.repetitions)
            || self.suite_digest != suite.digest()?
            || self.evaluator_digest != evaluator_fingerprint()
        {
            return Err("regression plan suite, repetition or evaluator binding mismatch".into());
        }
        for pin in [&self.baseline, &self.candidate] {
            pin.settings.validate()?;
            if pin.settings.fixture_set_id != self.suite_digest
                || !sha256(&pin.source_digest)
                || !sha256(&pin.tool_policy_digest)
            {
                return Err("invalid regression variant pin".into());
            }
        }
        Ok(())
    }
}

/// Fingerprints the compiled source bytes owning this deterministic checker and trace regrading.
///
/// This scope covers `task_regressions.rs` and `trace_evaluation.rs`, not the entire runtime,
/// provider adapter, dependency build or host isolation. Hosts must pin those separately.
#[must_use]
pub fn evaluator_fingerprint() -> String {
    let mut hash = Sha256::new();
    hash.update(b"anima-task-regression-evaluator-source-v1\0");
    for source in [
        include_bytes!("task_regressions.rs").as_slice(),
        include_bytes!("trace_evaluation.rs").as_slice(),
    ] {
        hash.update((source.len() as u64).to_be_bytes());
        hash.update(source);
    }
    hex::encode(hash.finalize())
}

/// Regrades an exact planned matrix from frozen observations and side-specific trace settings.
///
/// Artifact references are retained facts only. Their presence or absence contributes no render,
/// formula, semantic or visual quality verdict. The host must resolve authoritative inspection
/// reports before making such claims. This function proves no live-provider performance itself.
///
/// # Errors
/// Rejects incomplete/duplicate/substituted rows, unknown served identity, invalid observations
/// and serialized verdicts inconsistent with the actual recorded files and terminal output.
pub fn compare_planned_tasks(
    suite: &RegressionSuite,
    plan: &ComparisonPlan,
    observations: &[TrialObservation],
) -> Result<RegressionComparison, String> {
    plan.validate(suite)?;
    if observations.len() != suite.fixtures.len() * plan.repetitions as usize * 2 {
        return Err("incomplete planned regression matrix".into());
    }
    let mut seen = BTreeSet::new();
    let mut report = RegressionComparison {
        matrix_complete: true,
        baseline: VariantSummary::default(),
        candidate: VariantSummary::default(),
        trials: Vec::with_capacity(observations.len()),
    };
    for observation in observations {
        let trace = &observation.graded.trace;
        let pin = match trace.variant {
            TraceVariant::Baseline => &plan.baseline,
            TraceVariant::Candidate => &plan.candidate,
        };
        if observation.variant_pin != *pin
            || trace.settings != pin.settings
            || trace.repetition >= plan.repetitions
            || !seen.insert((trace.fixture_id.as_str(), trace.repetition, trace.variant))
        {
            return Err(
                "mismatched regression variant pin, settings, repetition or duplicate trial".into(),
            );
        }
        let fixture = suite
            .fixtures
            .iter()
            .find(|fixture| fixture.id == trace.fixture_id)
            .ok_or("unknown regression fixture")?;
        let metrics = trace.metrics()?;
        let served = trace
            .events
            .iter()
            .filter(|event| matches!(event.event, TraceEventKind::Served { .. }))
            .count();
        if served as u64 != metrics.model_calls {
            return Err("regression model step lacks actual served identity".into());
        }
        validate_file_map(&observation.graded.observed_files)?;
        validate_paths(
            observation
                .graded
                .observed_directories
                .iter()
                .map(String::as_str),
        )?;
        validate_effects(&observation.mock_effects)?;
        if observation.artifact_refs.len() > MAX_OBSERVATION_REFS {
            return Err("artifact observation reference bounds exceeded".into());
        }
        let mut artifact_refs = BTreeSet::new();
        for reference in &observation.artifact_refs {
            reference.validate()?;
            if !artifact_refs.insert((&reference.report_sha256, &reference.observation_locator)) {
                return Err("duplicate artifact observation reference".into());
            }
        }
        let mut failures =
            trace_evaluation::regrade_observation(&fixture.task_fixture(), &observation.graded)?;
        if failures != observation.graded.oracle_failures {
            return Err("serialized oracle verdict does not match observed evidence".into());
        }
        if observation.mock_effects != fixture.oracle.expected_mock_effects {
            failures.push("mock effects differ".into());
        }
        let summary = match trace.variant {
            TraceVariant::Baseline => &mut report.baseline,
            TraceVariant::Candidate => &mut report.candidate,
        };
        summary.trials += 1;
        summary.correct += u32::from(failures.is_empty());
        trace_evaluation::add_metrics(&mut summary.metrics, &metrics);
        report.trials.push(RegressionTrialGrade {
            fixture_id: trace.fixture_id.clone(),
            repetition: trace.repetition,
            variant: trace.variant,
            failures,
        });
    }
    report.trials.sort_by(|left, right| {
        (&left.fixture_id, left.repetition, left.variant).cmp(&(
            &right.fixture_id,
            right.repetition,
            right.variant,
        ))
    });
    Ok(report)
}
