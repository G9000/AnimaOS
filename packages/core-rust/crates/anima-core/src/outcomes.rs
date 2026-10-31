use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashSet};
use std::fmt;

pub const MAX_OUTCOME_CRITERIA: usize = 16;
pub const MAX_OUTCOME_CORRECTIONS: u8 = 2;
pub const MAX_OUTCOME_ITERATIONS: usize = 3;
const MAX_ID_BYTES: usize = 128;
const MAX_PATH_BYTES: usize = 512;
const MAX_TEXT_BYTES: usize = 8_192;
const MAX_DETAIL_BYTES: usize = 4_096;
const MAX_JSON_BYTES: usize = 16_384;
const MAX_JSON_NODES: usize = 1_024;
const MAX_JSON_DEPTH: usize = 16;
const MAX_DEFINITION_BYTES: usize = 65_536;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeAcceptance {
    #[default]
    ReportOnly,
    OwnerRequired,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OutcomeDefinition {
    pub criteria: Vec<OutcomeCriterion>,
    #[serde(default = "default_max_corrections")]
    pub max_corrections: u8,
    #[serde(default)]
    pub acceptance: OutcomeAcceptance,
}

impl Default for OutcomeDefinition {
    fn default() -> Self {
        Self {
            criteria: Vec::new(),
            max_corrections: default_max_corrections(),
            acceptance: OutcomeAcceptance::default(),
        }
    }
}

const fn default_max_corrections() -> u8 {
    MAX_OUTCOME_CORRECTIONS
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentCheck {
    CompleteRendering,
    RecalculatedWithoutErrors,
}

impl DocumentCheck {
    pub fn locator(self) -> &'static str {
        match self {
            Self::CompleteRendering => "complete_rendering",
            Self::RecalculatedWithoutErrors => "recalculated_without_errors",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum OutcomeCriterion {
    FileText {
        id: String,
        path: String,
        contains: String,
    },
    FileSha256 {
        id: String,
        path: String,
        sha256: String,
    },
    FileJson {
        id: String,
        path: String,
        pointer: String,
        expected: Value,
    },
    ResponseText {
        id: String,
        contains: String,
    },
    Document {
        id: String,
        path: String,
        check: DocumentCheck,
    },
    ArtifactInspection {
        id: String,
        report_sha256: String,
        locator: String,
        subject_sha256: String,
    },
    Quality {
        id: String,
        rubric: String,
    },
}

impl OutcomeCriterion {
    pub fn id(&self) -> &str {
        match self {
            Self::FileText { id, .. }
            | Self::FileSha256 { id, .. }
            | Self::FileJson { id, .. }
            | Self::ResponseText { id, .. }
            | Self::Document { id, .. }
            | Self::ArtifactInspection { id, .. }
            | Self::Quality { id, .. } => id,
        }
    }

    pub fn path(&self) -> Option<&str> {
        match self {
            Self::FileText { path, .. }
            | Self::FileSha256 { path, .. }
            | Self::FileJson { path, .. }
            | Self::Document { path, .. } => Some(path),
            _ => None,
        }
    }

    fn actionable(&self) -> bool {
        self.path().is_some() || matches!(self, Self::ResponseText { .. })
    }
}

impl OutcomeDefinition {
    pub fn validate(&self) -> Result<(), OutcomeError> {
        if self.criteria.is_empty() || self.criteria.len() > MAX_OUTCOME_CRITERIA {
            return Err(OutcomeError::InvalidCriteria);
        }
        if self.max_corrections > MAX_OUTCOME_CORRECTIONS {
            return Err(OutcomeError::CorrectionLimit);
        }
        let mut ids = HashSet::new();
        for criterion in &self.criteria {
            validate_id(criterion.id())?;
            if !ids.insert(criterion.id()) {
                return Err(OutcomeError::DuplicateCriterion);
            }
            if let Some(path) = criterion.path() {
                validate_path(path)?;
            }
            match criterion {
                OutcomeCriterion::FileText { contains, .. }
                | OutcomeCriterion::ResponseText { contains, .. } => {
                    validate_text(contains, MAX_TEXT_BYTES)?;
                }
                OutcomeCriterion::FileSha256 { sha256, .. } => validate_sha256(sha256)?,
                OutcomeCriterion::FileJson {
                    pointer, expected, ..
                } => {
                    validate_pointer(pointer)?;
                    validate_json(expected)?;
                }
                OutcomeCriterion::ArtifactInspection {
                    report_sha256,
                    locator,
                    subject_sha256,
                    ..
                } => {
                    validate_sha256(report_sha256)?;
                    validate_sha256(subject_sha256)?;
                    if !valid_document_locator(locator) {
                        return Err(OutcomeError::InvalidCriteria);
                    }
                }
                OutcomeCriterion::Document { .. } => {}
                OutcomeCriterion::Quality { rubric, .. } => {
                    validate_text(rubric, MAX_TEXT_BYTES)?;
                }
            }
        }
        if serde_json::to_vec(self)
            .map_err(|_| OutcomeError::InvalidCriteria)?
            .len()
            > MAX_DEFINITION_BYTES
        {
            return Err(OutcomeError::InvalidCriteria);
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        digest(b"anima-outcome-definition-v1\0", self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeStatus {
    Pending,
    Checking,
    Correcting,
    CriteriaMet,
    AwaitingOwner,
    Accepted,
    NeedsInput,
    NeedsReview,
    Exhausted,
    Stopped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeVerdict {
    Met,
    NotMet,
    Inconclusive,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OutcomeObservation {
    pub criterion_id: String,
    pub verdict: OutcomeVerdict,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locator: Option<String>,
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OutcomeIteration {
    pub ordinal: u8,
    pub binding_sha256: String,
    pub semantic_sha256: String,
    pub reply_sha256: String,
    pub observations: Vec<OutcomeObservation>,
}

impl OutcomeIteration {
    pub fn digest(&self) -> String {
        digest(b"anima-outcome-report-v1\0", self)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RunOutcome {
    pub definition: OutcomeDefinition,
    pub revision: u64,
    pub root_run_id: String,
    pub task_sha256: String,
    pub source_sha256: String,
    pub model_sha256: String,
    pub workspace_sha256: String,
    pub iterations: Vec<OutcomeIteration>,
    pub consumed_corrections: u8,
    pub status: OutcomeStatus,
    pub report_sha256: Option<String>,
    pub accepted_report_sha256: Option<String>,
    pub repair_paths: Vec<String>,
    pub terminal_reply_sha256: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeDisposition {
    Pass,
    Correct,
    Hold,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutcomeError {
    InvalidCriteria,
    DuplicateCriterion,
    InvalidId,
    InvalidPath,
    InvalidText,
    InvalidSha256,
    InvalidJson,
    InvalidPointer,
    InvalidObservation,
    UnsupportedEvidence,
    IncompleteObservations,
    StaleBinding,
    InvalidOrdinal,
    CorrectionLimit,
    InvalidState,
    Terminal,
}

impl fmt::Display for OutcomeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidCriteria => "outcome requires bounded, nonempty criteria",
            Self::DuplicateCriterion => "outcome criterion IDs must be unique",
            Self::InvalidId => "outcome identifier is invalid",
            Self::InvalidPath => "outcome file path must be exact, relative and portable",
            Self::InvalidText => "outcome text is empty or exceeds its bound",
            Self::InvalidSha256 => "outcome digest must be lowercase SHA-256 hex",
            Self::InvalidJson => "outcome JSON expectation exceeds its bound",
            Self::InvalidPointer => "outcome JSON pointer is invalid",
            Self::InvalidObservation => "outcome evidence does not match its criterion",
            Self::UnsupportedEvidence => "outcome evidence cannot establish independent quality",
            Self::IncompleteObservations => {
                "outcome observations require exact unique criterion coverage"
            }
            Self::StaleBinding => "outcome evidence binding is stale",
            Self::InvalidOrdinal => "outcome iteration ordinal is invalid",
            Self::CorrectionLimit => "outcome correction or iteration limit exceeded",
            Self::InvalidState => "outcome record has inconsistent state",
            Self::Terminal => "outcome is terminal",
        })
    }
}

impl std::error::Error for OutcomeError {}

impl RunOutcome {
    pub fn new(
        definition: OutcomeDefinition,
        root_run_id: String,
        task_sha256: String,
        source_sha256: String,
        model_sha256: String,
        workspace_sha256: String,
    ) -> Result<Self, OutcomeError> {
        let record = Self {
            definition,
            revision: 1,
            root_run_id,
            task_sha256,
            source_sha256,
            model_sha256,
            workspace_sha256,
            iterations: Vec::new(),
            consumed_corrections: 0,
            status: OutcomeStatus::Pending,
            report_sha256: None,
            accepted_report_sha256: None,
            repair_paths: Vec::new(),
            terminal_reply_sha256: None,
        };
        record.validate()?;
        Ok(record)
    }

    /// Identifies the definition and execution identity independently of reducer progress.
    pub fn semantic_sha256(&self) -> String {
        digest(
            b"anima-outcome-semantic-v1\0",
            &serde_json::json!({
                "definitionSha256": self.definition.digest(),
                "rootRunId": self.root_run_id,
                "taskSha256": self.task_sha256,
                "sourceSha256": self.source_sha256,
                "modelSha256": self.model_sha256,
                "workspaceSha256": self.workspace_sha256,
            }),
        )
    }

    pub fn binding_sha256(&self) -> String {
        digest(
            b"anima-outcome-binding-v1\0",
            &serde_json::json!({
                "definitionSha256": self.definition.digest(),
                "revision": self.revision,
                "rootRunId": self.root_run_id,
                "taskSha256": self.task_sha256,
                "sourceSha256": self.source_sha256,
                "modelSha256": self.model_sha256,
                "workspaceSha256": self.workspace_sha256,
                "consumedCorrections": self.consumed_corrections,
            }),
        )
    }

    pub fn validate(&self) -> Result<(), OutcomeError> {
        self.definition.validate()?;
        validate_id(&self.root_run_id)?;
        for value in [
            &self.task_sha256,
            &self.source_sha256,
            &self.model_sha256,
            &self.workspace_sha256,
        ] {
            validate_sha256(value)?;
        }
        for value in [
            &self.report_sha256,
            &self.accepted_report_sha256,
            &self.terminal_reply_sha256,
        ]
        .into_iter()
        .flatten()
        {
            validate_sha256(value)?;
        }
        if self.consumed_corrections > self.definition.max_corrections
            || self.iterations.len() > MAX_OUTCOME_ITERATIONS
            || self.iterations.len() > usize::from(self.consumed_corrections) + 1
        {
            return Err(OutcomeError::CorrectionLimit);
        }
        if self.revision == 0 || self.revision <= self.iterations.len() as u64 {
            return Err(OutcomeError::InvalidState);
        }
        let mut corrections = 0;
        let mut latest_disposition = None;
        for (index, iteration) in self.iterations.iter().enumerate() {
            if usize::from(iteration.ordinal) != index + 1 {
                return Err(OutcomeError::InvalidOrdinal);
            }
            validate_iteration(&self.definition, iteration)?;
            let result = disposition(&self.definition, iteration, corrections);
            latest_disposition = Some(result);
            if result == OutcomeDisposition::Correct {
                corrections += 1;
            } else if index + 1 < self.iterations.len() {
                return Err(OutcomeError::InvalidState);
            }
        }
        if corrections != self.consumed_corrections {
            return Err(OutcomeError::InvalidState);
        }
        if self.repair_paths.len() > MAX_OUTCOME_CRITERIA {
            return Err(OutcomeError::InvalidState);
        }
        let mut paths = HashSet::new();
        for path in &self.repair_paths {
            validate_path(path)?;
            if !paths.insert(path)
                || !self
                    .definition
                    .criteria
                    .iter()
                    .any(|criterion| criterion.path() == Some(path.as_str()))
            {
                return Err(OutcomeError::InvalidPath);
            }
        }
        if self.status != OutcomeStatus::Correcting && !self.repair_paths.is_empty() {
            return Err(OutcomeError::InvalidState);
        }
        let latest = self.iterations.last();
        if self.report_sha256.as_ref() != latest.map(OutcomeIteration::digest).as_ref() {
            return Err(OutcomeError::InvalidState);
        }
        let passing = matches!(
            self.status,
            OutcomeStatus::CriteriaMet | OutcomeStatus::AwaitingOwner | OutcomeStatus::Accepted
        );
        if passing {
            let latest = latest.ok_or(OutcomeError::InvalidState)?;
            if !latest
                .observations
                .iter()
                .all(|observation| observation.verdict == OutcomeVerdict::Met)
                || latest.semantic_sha256 != self.semantic_sha256()
                || self.terminal_reply_sha256.as_deref() != Some(latest.reply_sha256.as_str())
                || !self.repair_paths.is_empty()
            {
                return Err(OutcomeError::InvalidState);
            }
            if (self.status == OutcomeStatus::CriteriaMet
                && self.definition.acceptance != OutcomeAcceptance::ReportOnly)
                || (matches!(
                    self.status,
                    OutcomeStatus::AwaitingOwner | OutcomeStatus::Accepted
                ) && self.definition.acceptance != OutcomeAcceptance::OwnerRequired)
            {
                return Err(OutcomeError::InvalidState);
            }
        }
        if self.status == OutcomeStatus::Accepted {
            if self.accepted_report_sha256.is_none()
                || self.accepted_report_sha256 != self.report_sha256
            {
                return Err(OutcomeError::InvalidState);
            }
        } else if self.accepted_report_sha256.is_some() {
            return Err(OutcomeError::InvalidState);
        }
        if self.status == OutcomeStatus::Pending
            && (!self.iterations.is_empty() || self.terminal_reply_sha256.is_some())
        {
            return Err(OutcomeError::InvalidState);
        }
        if self.status == OutcomeStatus::Checking
            && latest.is_some()
            && latest_disposition != Some(OutcomeDisposition::Correct)
        {
            return Err(OutcomeError::InvalidState);
        }
        if self.status == OutcomeStatus::Correcting {
            let latest = latest.ok_or(OutcomeError::InvalidState)?;
            if latest_disposition != Some(OutcomeDisposition::Correct)
                || self.repair_paths != repair_paths(&self.definition, latest)
            {
                return Err(OutcomeError::InvalidState);
            }
        }
        Ok(())
    }

    pub fn settle(
        &mut self,
        iteration: OutcomeIteration,
    ) -> Result<OutcomeDisposition, OutcomeError> {
        self.validate()?;
        if !matches!(
            self.status,
            OutcomeStatus::Pending | OutcomeStatus::Checking | OutcomeStatus::Correcting
        ) {
            return Err(OutcomeError::Terminal);
        }
        if self.iterations.len() >= MAX_OUTCOME_ITERATIONS {
            return Err(OutcomeError::CorrectionLimit);
        }
        if usize::from(iteration.ordinal) != self.iterations.len() + 1 {
            return Err(OutcomeError::InvalidOrdinal);
        }
        validate_iteration(&self.definition, &iteration)?;
        if iteration.binding_sha256 != self.binding_sha256()
            || iteration.semantic_sha256 != self.semantic_sha256()
        {
            return Err(OutcomeError::StaleBinding);
        }
        let next_revision = self
            .revision
            .checked_add(1)
            .ok_or(OutcomeError::InvalidState)?;
        let result = disposition(&self.definition, &iteration, self.consumed_corrections);
        let mut next = self.clone();
        next.accepted_report_sha256 = None;
        next.repair_paths.clear();
        next.terminal_reply_sha256 = Some(iteration.reply_sha256.clone());
        next.report_sha256 = Some(iteration.digest());
        match result {
            OutcomeDisposition::Pass => {
                next.status = match next.definition.acceptance {
                    OutcomeAcceptance::ReportOnly => OutcomeStatus::CriteriaMet,
                    OutcomeAcceptance::OwnerRequired => OutcomeStatus::AwaitingOwner,
                };
            }
            OutcomeDisposition::Correct => {
                next.consumed_corrections += 1;
                next.status = OutcomeStatus::Correcting;
                next.repair_paths = repair_paths(&next.definition, &iteration);
            }
            OutcomeDisposition::Hold => {
                next.status = if iteration
                    .observations
                    .iter()
                    .any(|observation| observation.verdict == OutcomeVerdict::Inconclusive)
                    || has_unsupported_failure(&next.definition, &iteration)
                {
                    OutcomeStatus::NeedsReview
                } else {
                    OutcomeStatus::Exhausted
                };
            }
        }
        next.iterations.push(iteration);
        next.revision = next_revision;
        next.validate()?;
        *self = next;
        Ok(result)
    }

    pub fn hold(&mut self, status: OutcomeStatus) -> Result<(), OutcomeError> {
        self.validate()?;
        if self.status == OutcomeStatus::Stopped {
            return Err(OutcomeError::Terminal);
        }
        if !matches!(
            status,
            OutcomeStatus::NeedsInput
                | OutcomeStatus::NeedsReview
                | OutcomeStatus::Exhausted
                | OutcomeStatus::Stopped
        ) {
            return Err(OutcomeError::InvalidState);
        }
        let next_revision = self
            .revision
            .checked_add(1)
            .ok_or(OutcomeError::InvalidState)?;
        self.status = status;
        self.repair_paths.clear();
        self.accepted_report_sha256 = None;
        self.revision = next_revision;
        Ok(())
    }
}

fn validate_iteration(
    definition: &OutcomeDefinition,
    iteration: &OutcomeIteration,
) -> Result<(), OutcomeError> {
    validate_sha256(&iteration.binding_sha256)?;
    validate_sha256(&iteration.semantic_sha256)?;
    validate_sha256(&iteration.reply_sha256)?;
    if iteration.observations.len() != definition.criteria.len() {
        return Err(OutcomeError::IncompleteObservations);
    }
    let mut ids = HashSet::new();
    for observation in &iteration.observations {
        validate_id(&observation.criterion_id)?;
        validate_text(&observation.detail, MAX_DETAIL_BYTES)?;
        if !ids.insert(observation.criterion_id.as_str()) {
            return Err(OutcomeError::IncompleteObservations);
        }
        let criterion = definition
            .criteria
            .iter()
            .find(|criterion| criterion.id() == observation.criterion_id)
            .ok_or(OutcomeError::IncompleteObservations)?;
        for value in [&observation.subject_sha256, &observation.report_sha256]
            .into_iter()
            .flatten()
        {
            validate_sha256(value)?;
        }
        if let Some(locator) = &observation.locator {
            validate_text(locator, MAX_PATH_BYTES)?;
        }
        match criterion {
            OutcomeCriterion::FileText { .. }
            | OutcomeCriterion::FileSha256 { .. }
            | OutcomeCriterion::FileJson { .. } => {
                if observation.report_sha256.is_some()
                    || observation.locator.is_some()
                    || (observation.verdict == OutcomeVerdict::Met
                        && observation.subject_sha256.is_none())
                {
                    return Err(OutcomeError::InvalidObservation);
                }
                if let OutcomeCriterion::FileSha256 { sha256, .. } = criterion {
                    if observation.verdict == OutcomeVerdict::Met
                        && observation.subject_sha256.as_ref() != Some(sha256)
                    {
                        return Err(OutcomeError::InvalidObservation);
                    }
                }
            }
            OutcomeCriterion::ResponseText { .. } => {
                if observation.report_sha256.is_some()
                    || observation.locator.is_some()
                    || (observation.verdict != OutcomeVerdict::Inconclusive
                        && observation.subject_sha256.as_ref() != Some(&iteration.reply_sha256))
                    || observation
                        .subject_sha256
                        .as_ref()
                        .is_some_and(|value| value != &iteration.reply_sha256)
                {
                    return Err(OutcomeError::InvalidObservation);
                }
            }
            OutcomeCriterion::Document { check, .. } => {
                if observation.verdict == OutcomeVerdict::Inconclusive {
                    if observation
                        .locator
                        .as_deref()
                        .is_some_and(|locator| locator != check.locator())
                    {
                        return Err(OutcomeError::InvalidObservation);
                    }
                } else if observation.subject_sha256.is_none()
                    || observation.report_sha256.is_none()
                    || observation.locator.as_deref() != Some(check.locator())
                {
                    return Err(OutcomeError::InvalidObservation);
                }
            }
            OutcomeCriterion::ArtifactInspection {
                report_sha256,
                locator,
                subject_sha256,
                ..
            } => {
                let exact = observation.report_sha256.as_ref() == Some(report_sha256)
                    && observation.locator.as_ref() == Some(locator)
                    && observation.subject_sha256.as_ref() == Some(subject_sha256);
                if !exact
                    && (observation.verdict != OutcomeVerdict::Inconclusive
                        || observation.report_sha256.is_some()
                        || observation.locator.is_some()
                        || observation.subject_sha256.is_some())
                {
                    return Err(OutcomeError::InvalidObservation);
                }
            }
            OutcomeCriterion::Quality { .. } => {
                if observation.verdict == OutcomeVerdict::Met {
                    return Err(OutcomeError::UnsupportedEvidence);
                }
                if observation.subject_sha256.is_some()
                    || observation.report_sha256.is_some()
                    || observation.locator.is_some()
                {
                    return Err(OutcomeError::InvalidObservation);
                }
            }
        }
    }
    Ok(())
}

fn has_unsupported_failure(definition: &OutcomeDefinition, iteration: &OutcomeIteration) -> bool {
    iteration.observations.iter().any(|observation| {
        observation.verdict == OutcomeVerdict::NotMet
            && definition.criteria.iter().any(|criterion| {
                criterion.id() == observation.criterion_id && !criterion.actionable()
            })
    })
}

fn disposition(
    definition: &OutcomeDefinition,
    iteration: &OutcomeIteration,
    consumed: u8,
) -> OutcomeDisposition {
    if iteration
        .observations
        .iter()
        .all(|observation| observation.verdict == OutcomeVerdict::Met)
    {
        OutcomeDisposition::Pass
    } else if iteration
        .observations
        .iter()
        .any(|observation| observation.verdict == OutcomeVerdict::Inconclusive)
        || has_unsupported_failure(definition, iteration)
        || consumed >= definition.max_corrections
    {
        OutcomeDisposition::Hold
    } else {
        OutcomeDisposition::Correct
    }
}

fn repair_paths(definition: &OutcomeDefinition, iteration: &OutcomeIteration) -> Vec<String> {
    iteration
        .observations
        .iter()
        .filter(|observation| observation.verdict == OutcomeVerdict::NotMet)
        .filter_map(|observation| {
            definition
                .criteria
                .iter()
                .find(|criterion| criterion.id() == observation.criterion_id)
        })
        .filter_map(OutcomeCriterion::path)
        .map(str::to_owned)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn validate_id(value: &str) -> Result<(), OutcomeError> {
    if value.is_empty()
        || value.len() > MAX_ID_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(OutcomeError::InvalidId);
    }
    Ok(())
}

fn validate_text(value: &str, limit: usize) -> Result<(), OutcomeError> {
    if value.trim().is_empty() || value.len() > limit || value.contains('\0') {
        return Err(OutcomeError::InvalidText);
    }
    Ok(())
}

fn validate_sha256(value: &str) -> Result<(), OutcomeError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(OutcomeError::InvalidSha256);
    }
    Ok(())
}

fn validate_path(value: &str) -> Result<(), OutcomeError> {
    if value.is_empty()
        || value.len() > MAX_PATH_BYTES
        || value.chars().any(|character| {
            character.is_control()
                || matches!(
                    character,
                    ':' | '*' | '?' | '[' | ']' | '{' | '}' | '<' | '>' | '"' | '|'
                )
        })
    {
        return Err(OutcomeError::InvalidPath);
    }
    for part in value.split(['/', '\\']) {
        if part.is_empty()
            || part == "."
            || part == ".."
            || part.ends_with(['.', ' '])
            || part.trim() != part
        {
            return Err(OutcomeError::InvalidPath);
        }
        let name = part
            .split('.')
            .next()
            .unwrap_or_default()
            .to_ascii_uppercase();
        if matches!(name.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || ["COM", "LPT"].iter().any(|prefix| {
                name.strip_prefix(*prefix).is_some_and(|suffix| {
                    suffix.len() == 1 && matches!(suffix.as_bytes()[0], b'1'..=b'9')
                })
            })
        {
            return Err(OutcomeError::InvalidPath);
        }
    }
    Ok(())
}

fn validate_pointer(value: &str) -> Result<(), OutcomeError> {
    if value.len() > MAX_PATH_BYTES
        || (!value.is_empty() && !value.starts_with('/'))
        || value.chars().any(char::is_control)
    {
        return Err(OutcomeError::InvalidPointer);
    }
    let mut characters = value.chars();
    while let Some(character) = characters.next() {
        if character == '~' && !matches!(characters.next(), Some('0' | '1')) {
            return Err(OutcomeError::InvalidPointer);
        }
    }
    Ok(())
}

fn valid_document_locator(value: &str) -> bool {
    matches!(value, "complete_rendering" | "recalculated_without_errors")
}

fn validate_json(value: &Value) -> Result<(), OutcomeError> {
    let mut pending = vec![(value, 0)];
    let mut count = 0;
    while let Some((value, depth)) = pending.pop() {
        count += 1;
        if count > MAX_JSON_NODES || depth > MAX_JSON_DEPTH {
            return Err(OutcomeError::InvalidJson);
        }
        match value {
            Value::Object(values) => {
                if values.len() > MAX_JSON_NODES
                    || values.keys().any(|key| key.len() > MAX_JSON_BYTES)
                {
                    return Err(OutcomeError::InvalidJson);
                }
                pending.extend(values.values().map(|value| (value, depth + 1)));
            }
            Value::Array(values) => {
                if values.len() > MAX_JSON_NODES {
                    return Err(OutcomeError::InvalidJson);
                }
                pending.extend(values.iter().map(|value| (value, depth + 1)));
            }
            Value::String(value) if value.len() > MAX_JSON_BYTES => {
                return Err(OutcomeError::InvalidJson)
            }
            _ => {}
        }
    }
    if serde_json::to_vec(value)
        .map_err(|_| OutcomeError::InvalidJson)?
        .len()
        > MAX_JSON_BYTES
    {
        return Err(OutcomeError::InvalidJson);
    }
    Ok(())
}

fn digest(domain: &[u8], value: &impl Serialize) -> String {
    fn sort(value: &mut Value) {
        match value {
            Value::Object(values) => {
                let ordered: std::collections::BTreeMap<_, _> =
                    std::mem::take(values).into_iter().collect();
                for (key, mut value) in ordered {
                    sort(&mut value);
                    values.insert(key, value);
                }
            }
            Value::Array(values) => values.iter_mut().for_each(sort),
            _ => {}
        }
    }
    let mut value =
        serde_json::to_value(value).expect("outcome data has an infallible JSON representation");
    sort(&mut value);
    let bytes =
        serde_json::to_vec(&value).expect("JSON values have an infallible byte representation");
    let mut hash = Sha256::new();
    hash.update(domain);
    hash.update(bytes);
    hex::encode(hash.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sha(value: char) -> String {
        value.to_string().repeat(64)
    }

    fn file(id: &str, path: &str) -> OutcomeCriterion {
        OutcomeCriterion::FileText {
            id: id.into(),
            path: path.into(),
            contains: "ready".into(),
        }
    }

    fn definition(criteria: Vec<OutcomeCriterion>) -> OutcomeDefinition {
        OutcomeDefinition {
            criteria,
            max_corrections: 2,
            acceptance: OutcomeAcceptance::ReportOnly,
        }
    }

    fn run(definition: OutcomeDefinition) -> RunOutcome {
        RunOutcome::new(
            definition,
            "root-1".into(),
            sha('1'),
            sha('2'),
            sha('3'),
            sha('4'),
        )
        .expect("valid outcome")
    }

    fn observation(id: &str, verdict: OutcomeVerdict) -> OutcomeObservation {
        OutcomeObservation {
            criterion_id: id.into(),
            verdict,
            subject_sha256: Some(sha('5')),
            report_sha256: None,
            locator: None,
            detail: "Host observed exact subject bytes".into(),
        }
    }

    fn iteration(run: &RunOutcome, observations: Vec<OutcomeObservation>) -> OutcomeIteration {
        OutcomeIteration {
            ordinal: u8::try_from(run.iterations.len() + 1).expect("bounded iterations"),
            binding_sha256: run.binding_sha256(),
            semantic_sha256: run.semantic_sha256(),
            reply_sha256: sha('6'),
            observations,
        }
    }

    #[test]
    fn public_definition_wire_defaults_and_unknown_fields_are_strict() {
        let value =
            json!({"criteria":[{"kind":"file_text","id":"a","path":"a.txt","contains":"ready"}]});
        let parsed: OutcomeDefinition = serde_json::from_value(value.clone()).expect("definition");
        assert_eq!(parsed.max_corrections, 2);
        assert_eq!(parsed.acceptance, OutcomeAcceptance::ReportOnly);
        let mut unknown = value.clone();
        unknown["command"] = json!("run something");
        assert!(serde_json::from_value::<OutcomeDefinition>(unknown).is_err());
        let mut unknown = value;
        unknown["criteria"][0]["command"] = json!("run something");
        assert!(serde_json::from_value::<OutcomeDefinition>(unknown).is_err());
        let criterion = OutcomeCriterion::ArtifactInspection {
            id: "render".into(),
            report_sha256: sha('a'),
            locator: "complete_rendering".into(),
            subject_sha256: sha('b'),
        };
        let encoded = serde_json::to_value(criterion).expect("criterion wire");
        assert_eq!(encoded["kind"], "artifact_inspection");
        assert_eq!(encoded["reportSha256"], sha('a'));
    }

    #[test]
    fn definitions_require_bounded_unique_criteria_and_safe_paths() {
        assert!(definition(vec![]).validate().is_err());
        assert!(definition(vec![file("a", "a.txt"), file("a", "b.txt")])
            .validate()
            .is_err());
        assert!(
            definition((0..17).map(|i| file(&format!("a{i}"), "a.txt")).collect())
                .validate()
                .is_err()
        );
        for path in [
            "../a", "/a", "C:\\a", "a/../b", "a/*.txt", "a/?.txt", "a//b", "a/./b", "a\\..\\b",
            "NUL.txt", "a:stream", "a/b.",
        ] {
            assert!(
                definition(vec![file("a", path)]).validate().is_err(),
                "{path}"
            );
        }
        let mut bounded = definition(vec![file("a", "reports/a.txt")]);
        assert!(bounded.validate().is_ok());
        bounded.max_corrections = 3;
        assert!(bounded.validate().is_err());
        bounded.max_corrections = 2;
        bounded.criteria = vec![OutcomeCriterion::FileSha256 {
            id: "a".into(),
            path: "a.txt".into(),
            sha256: "bad".into(),
        }];
        assert!(bounded.validate().is_err());
    }

    #[test]
    fn json_expectations_are_bounded_and_digest_is_canonical() {
        let first = definition(vec![OutcomeCriterion::FileJson {
            id: "a".into(),
            path: "a.json".into(),
            pointer: "/a~1b/0".into(),
            expected: json!({"b":2,"a":1}),
        }]);
        let mut second = first.clone();
        if let OutcomeCriterion::FileJson { expected, .. } = &mut second.criteria[0] {
            *expected = serde_json::from_str("{\"a\":1,\"b\":2}").expect("json");
        }
        assert_eq!(first.digest(), second.digest());
        if let OutcomeCriterion::FileJson { pointer, .. } = &mut second.criteria[0] {
            *pointer = "/invalid~2escape".into();
        }
        assert!(second.validate().is_err());
        if let OutcomeCriterion::FileJson {
            pointer, expected, ..
        } = &mut second.criteria[0]
        {
            *pointer = String::new();
            *expected = json!("x".repeat(20_000));
        }
        assert!(second.validate().is_err());
    }

    #[test]
    fn successful_evidence_does_not_grant_owner_acceptance() {
        let mut requested = definition(vec![file("a", "a.txt")]);
        requested.acceptance = OutcomeAcceptance::OwnerRequired;
        let mut state = run(requested);
        let evidence = iteration(&state, vec![observation("a", OutcomeVerdict::Met)]);
        assert_eq!(state.settle(evidence), Ok(OutcomeDisposition::Pass));
        assert_eq!(state.status, OutcomeStatus::AwaitingOwner);
        assert!(state.report_sha256.is_some());
        assert!(state.accepted_report_sha256.is_none());
        assert!(state.validate().is_ok());
        let mut forged = state.clone();
        forged.accepted_report_sha256 = Some(sha('0'));
        forged.status = OutcomeStatus::Accepted;
        assert!(forged.validate().is_err());
        let mut self_accepted =
            serde_json::to_value(observation("a", OutcomeVerdict::Met)).expect("wire");
        self_accepted["accepted"] = json!(true);
        assert!(serde_json::from_value::<OutcomeObservation>(self_accepted).is_err());
    }

    #[test]
    fn incomplete_duplicate_unknown_stale_or_out_of_order_results_are_atomic_errors() {
        let mut state = run(definition(vec![file("a", "a.txt"), file("b", "b.txt")]));
        let before = state.clone();
        let incomplete = iteration(&state, vec![observation("a", OutcomeVerdict::Met)]);
        assert!(state.settle(incomplete).is_err());
        let duplicate = iteration(
            &state,
            vec![
                observation("a", OutcomeVerdict::Met),
                observation("a", OutcomeVerdict::Met),
            ],
        );
        assert!(state.settle(duplicate).is_err());
        let unknown = iteration(
            &state,
            vec![
                observation("a", OutcomeVerdict::Met),
                observation("unknown", OutcomeVerdict::Met),
            ],
        );
        assert!(state.settle(unknown).is_err());
        let mut stale = iteration(
            &state,
            vec![
                observation("a", OutcomeVerdict::Met),
                observation("b", OutcomeVerdict::Met),
            ],
        );
        stale.binding_sha256 = sha('0');
        assert!(state.settle(stale).is_err());
        let mut stale_semantic = iteration(
            &state,
            vec![
                observation("a", OutcomeVerdict::Met),
                observation("b", OutcomeVerdict::Met),
            ],
        );
        stale_semantic.semantic_sha256 = sha('0');
        assert_eq!(
            state.settle(stale_semantic),
            Err(OutcomeError::StaleBinding)
        );
        let mut wrong_ordinal = iteration(
            &state,
            vec![
                observation("a", OutcomeVerdict::Met),
                observation("b", OutcomeVerdict::Met),
            ],
        );
        wrong_ordinal.ordinal = 2;
        assert!(state.settle(wrong_ordinal).is_err());
        assert_eq!(state, before);
    }

    #[test]
    fn file_and_response_evidence_cannot_be_substituted_or_accepted_without_identity() {
        let mut state = run(definition(vec![file("a", "a.txt")]));
        let mut missing = observation("a", OutcomeVerdict::Met);
        missing.subject_sha256 = None;
        assert!(state.settle(iteration(&state, vec![missing])).is_err());
        let mut wrong_kind = observation("a", OutcomeVerdict::Met);
        wrong_kind.report_sha256 = Some(sha('a'));
        wrong_kind.locator = Some("pages/0".into());
        assert!(state.settle(iteration(&state, vec![wrong_kind])).is_err());
        let mut response = run(definition(vec![OutcomeCriterion::ResponseText {
            id: "reply".into(),
            contains: "ready".into(),
        }]));
        assert!(response
            .settle(iteration(
                &response,
                vec![observation("reply", OutcomeVerdict::Met)]
            ))
            .is_err());
        let mut correct = observation("reply", OutcomeVerdict::Met);
        correct.subject_sha256 = Some(sha('6'));
        assert_eq!(
            response.settle(iteration(&response, vec![correct])),
            Ok(OutcomeDisposition::Pass)
        );
        assert_eq!(response.status, OutcomeStatus::CriteriaMet);
    }

    #[test]
    fn artifact_evidence_must_match_the_exact_public_reference() {
        let mut state = run(definition(vec![OutcomeCriterion::ArtifactInspection {
            id: "render".into(),
            report_sha256: sha('a'),
            locator: "complete_rendering".into(),
            subject_sha256: sha('b'),
        }]));
        let mut evidence = observation("render", OutcomeVerdict::Met);
        evidence.report_sha256 = Some(sha('a'));
        evidence.locator = Some("complete_rendering".into());
        assert!(state
            .settle(iteration(&state, vec![evidence.clone()]))
            .is_err());
        evidence.subject_sha256 = Some(sha('b'));
        assert_eq!(
            state.settle(iteration(&state, vec![evidence])),
            Ok(OutcomeDisposition::Pass)
        );
    }

    #[test]
    fn automatic_corrections_are_capped_and_repair_paths_are_native_file_subjects_only() {
        let mut state = run(definition(vec![
            file("a", "a.txt"),
            file("b", "a.txt"),
            OutcomeCriterion::ResponseText {
                id: "reply".into(),
                contains: "ready".into(),
            },
        ]));
        for ordinal in 1..=3 {
            let mut response = observation("reply", OutcomeVerdict::NotMet);
            response.subject_sha256 = Some(sha('6'));
            let evidence = iteration(
                &state,
                vec![
                    observation("a", OutcomeVerdict::NotMet),
                    observation("b", OutcomeVerdict::NotMet),
                    response,
                ],
            );
            let expected = if ordinal < 3 {
                OutcomeDisposition::Correct
            } else {
                OutcomeDisposition::Hold
            };
            assert_eq!(state.settle(evidence), Ok(expected));
            assert_eq!(state.consumed_corrections, ordinal.min(2));
            if ordinal < 3 {
                assert_eq!(state.repair_paths, vec!["a.txt"]);
            }
        }
        assert_eq!(state.status, OutcomeStatus::Exhausted);
        assert_eq!(state.iterations.len(), 3);
        assert!(state.repair_paths.is_empty());
        assert!(state.validate().is_ok());
        let extra = iteration(&state, vec![]);
        assert!(state.settle(extra).is_err());
        let mut no_corrections = definition(vec![file("a", "a.txt")]);
        no_corrections.max_corrections = 0;
        let mut state = run(no_corrections);
        assert_eq!(
            state.settle(iteration(
                &state,
                vec![observation("a", OutcomeVerdict::NotMet)]
            )),
            Ok(OutcomeDisposition::Hold)
        );
        assert_eq!(state.consumed_corrections, 0);
    }

    #[test]
    fn inconclusive_and_unsupported_quality_hold_without_self_grading() {
        let mut state = run(definition(vec![file("a", "a.txt")]));
        assert_eq!(
            state.settle(iteration(
                &state,
                vec![observation("a", OutcomeVerdict::Inconclusive)]
            )),
            Ok(OutcomeDisposition::Hold)
        );
        assert_eq!(state.status, OutcomeStatus::NeedsReview);
        assert_eq!(state.consumed_corrections, 0);
        let mut quality = run(definition(vec![OutcomeCriterion::Quality {
            id: "quality".into(),
            rubric: "Readable and complete".into(),
        }]));
        let mut claim = observation("quality", OutcomeVerdict::Met);
        claim.subject_sha256 = None;
        assert!(quality
            .settle(iteration(&quality, vec![claim.clone()]))
            .is_err());
        claim.verdict = OutcomeVerdict::Inconclusive;
        assert_eq!(
            quality.settle(iteration(&quality, vec![claim])),
            Ok(OutcomeDisposition::Hold)
        );
        assert_eq!(quality.status, OutcomeStatus::NeedsReview);
    }

    #[test]
    fn restored_records_reject_tampered_reports_counters_and_acceptance() {
        let mut state = run(definition(vec![file("a", "a.txt")]));
        state
            .settle(iteration(
                &state,
                vec![observation("a", OutcomeVerdict::Met)],
            ))
            .expect("settled");
        let mut tampered = state.clone();
        tampered.iterations[0].observations[0].detail = "Changed later".into();
        assert!(tampered.validate().is_err());
        let mut tampered = state.clone();
        tampered.consumed_corrections = 2;
        assert!(tampered.validate().is_err());
        let mut tampered = state.clone();
        tampered.terminal_reply_sha256 = Some(sha('0'));
        assert!(tampered.validate().is_err());
        let wire = serde_json::to_vec(&state).expect("record wire");
        let restored: RunOutcome = serde_json::from_slice(&wire).expect("restore");
        assert_eq!(state, restored);
        assert!(restored.validate().is_ok());
    }

    fn assert_passing_records_reject_mutation(mutate: impl Fn(&mut RunOutcome)) {
        for status in [
            OutcomeStatus::CriteriaMet,
            OutcomeStatus::AwaitingOwner,
            OutcomeStatus::Accepted,
        ] {
            let mut requested = definition(vec![file("a", "a.txt")]);
            if status != OutcomeStatus::CriteriaMet {
                requested.acceptance = OutcomeAcceptance::OwnerRequired;
            }
            let mut state = run(requested);
            state
                .settle(iteration(
                    &state,
                    vec![observation("a", OutcomeVerdict::Met)],
                ))
                .expect("settled");
            if status == OutcomeStatus::Accepted {
                state.status = status;
                state.accepted_report_sha256 = state.report_sha256.clone();
                state.revision += 1;
            }
            assert!(state.validate().is_ok(), "valid {status:?}");
            mutate(&mut state);
            assert!(state.validate().is_err(), "changed identity for {status:?}");
        }
    }

    #[test]
    fn passing_records_reject_changed_definition() {
        assert_passing_records_reject_mutation(|state| {
            if let OutcomeCriterion::FileText { contains, .. } = &mut state.definition.criteria[0] {
                *contains = "different requirement".into();
            }
        });
    }

    #[test]
    fn passing_records_reject_changed_root_run() {
        assert_passing_records_reject_mutation(|state| state.root_run_id = "different-root".into());
    }

    #[test]
    fn passing_records_reject_changed_task() {
        assert_passing_records_reject_mutation(|state| state.task_sha256 = sha('a'));
    }

    #[test]
    fn passing_records_reject_changed_source() {
        assert_passing_records_reject_mutation(|state| state.source_sha256 = sha('a'));
    }

    #[test]
    fn passing_records_reject_changed_model() {
        assert_passing_records_reject_mutation(|state| state.model_sha256 = sha('a'));
    }

    #[test]
    fn passing_records_reject_changed_workspace() {
        assert_passing_records_reject_mutation(|state| state.workspace_sha256 = sha('a'));
    }

    #[test]
    fn report_only_records_cannot_claim_owner_acceptance() {
        let mut state = run(definition(vec![file("a", "a.txt")]));
        state
            .settle(iteration(
                &state,
                vec![observation("a", OutcomeVerdict::Met)],
            ))
            .expect("settled");
        state.status = OutcomeStatus::Accepted;
        state.accepted_report_sha256 = state.report_sha256.clone();
        state.revision += 1;
        assert!(state.validate().is_err());
    }

    #[test]
    fn restored_checking_cannot_reopen_passed_or_held_history() {
        for verdict in [
            OutcomeVerdict::Met,
            OutcomeVerdict::NotMet,
            OutcomeVerdict::Inconclusive,
        ] {
            let mut requested = definition(vec![file("a", "a.txt")]);
            requested.max_corrections = 0;
            let mut state = run(requested);
            state
                .settle(iteration(&state, vec![observation("a", verdict)]))
                .expect("settled");
            state.status = OutcomeStatus::Checking;
            state.repair_paths.clear();
            state.revision += 1;
            assert!(state.validate().is_err(), "cannot reopen {verdict:?}");
        }
    }

    #[test]
    fn exhausted_correction_history_cannot_be_claimed_again() {
        let mut state = run(definition(vec![file("a", "a.txt")]));
        for _ in 0..3 {
            state
                .settle(iteration(
                    &state,
                    vec![observation("a", OutcomeVerdict::NotMet)],
                ))
                .expect("settled");
        }
        assert_eq!(state.status, OutcomeStatus::Exhausted);
        for status in [OutcomeStatus::Checking, OutcomeStatus::Correcting] {
            let mut reopened = state.clone();
            reopened.status = status;
            reopened.revision += 1;
            if status == OutcomeStatus::Correcting {
                reopened.repair_paths = vec!["a.txt".into()];
            }
            assert!(
                reopened.validate().is_err(),
                "cannot reopen exhausted history as {status:?}"
            );
        }
    }

    #[test]
    fn rejected_settlement_preserves_a_record_with_unadmitted_checking_history() {
        let mut requested = definition(vec![file("a", "a.txt")]);
        requested.max_corrections = 0;
        let mut state = run(requested);
        state
            .settle(iteration(
                &state,
                vec![observation("a", OutcomeVerdict::NotMet)],
            ))
            .expect("exhausted");
        state.status = OutcomeStatus::Checking;
        state.revision += 1;
        let before = state.clone();
        assert!(state
            .settle(iteration(
                &state,
                vec![observation("a", OutcomeVerdict::Met)]
            ))
            .is_err());
        assert_eq!(state, before);
    }

    #[test]
    fn checking_can_claim_an_admitted_correction() {
        let mut state = run(definition(vec![file("a", "a.txt")]));
        state.status = OutcomeStatus::Checking;
        state.revision += 1;
        assert!(state.validate().is_ok());
        assert_eq!(
            state.settle(iteration(
                &state,
                vec![observation("a", OutcomeVerdict::NotMet)]
            )),
            Ok(OutcomeDisposition::Correct)
        );
        state.status = OutcomeStatus::Checking;
        state.repair_paths.clear();
        state.revision += 1;
        assert!(state.validate().is_ok());
        assert_eq!(
            state.settle(iteration(
                &state,
                vec![observation("a", OutcomeVerdict::Met)]
            )),
            Ok(OutcomeDisposition::Pass)
        );
        assert!(state.validate().is_ok());
    }

    #[test]
    fn iteration_wire_requires_a_semantic_snapshot() {
        let state = run(definition(vec![file("a", "a.txt")]));
        let mut wire = serde_json::to_value(iteration(
            &state,
            vec![observation("a", OutcomeVerdict::Met)],
        ))
        .expect("iteration wire");
        wire.as_object_mut()
            .expect("iteration object")
            .remove("semanticSha256");
        assert!(serde_json::from_value::<OutcomeIteration>(wire).is_err());
    }

    #[test]
    fn restored_records_require_valid_snapshot_digest_formats() {
        let mut state = run(definition(vec![file("a", "a.txt")]));
        state
            .settle(iteration(
                &state,
                vec![observation("a", OutcomeVerdict::Met)],
            ))
            .expect("settled");
        for field in ["bindingSha256", "semanticSha256", "replySha256"] {
            let mut wire = serde_json::to_value(&state).expect("record wire");
            wire["iterations"][0][field] = json!("invalid digest");
            let restored: RunOutcome = serde_json::from_value(wire).expect("record structure");
            assert_eq!(
                restored.validate(),
                Err(OutcomeError::InvalidSha256),
                "{field}"
            );
        }
    }

    #[test]
    fn stopped_records_are_terminal_and_cannot_regain_a_pass() {
        let mut state = run(definition(vec![file("a", "a.txt")]));
        state.hold(OutcomeStatus::Stopped).expect("stop");
        assert!(state
            .settle(iteration(
                &state,
                vec![observation("a", OutcomeVerdict::Met)]
            ))
            .is_err());
        assert!(state.hold(OutcomeStatus::NeedsReview).is_err());
        assert_eq!(state.status, OutcomeStatus::Stopped);
        assert!(state.validate().is_ok());
    }

    #[test]
    fn later_checks_bind_current_execution_identity_without_rewriting_history() {
        let mut state = run(definition(vec![file("a", "a.txt")]));
        let first = iteration(&state, vec![observation("a", OutcomeVerdict::NotMet)]);
        let old_binding = first.binding_sha256.clone();
        let old_semantic = first.semantic_sha256.clone();
        state.settle(first).expect("first failed check");
        assert_eq!(state.semantic_sha256(), old_semantic);
        assert_ne!(state.binding_sha256(), old_binding);
        state.source_sha256 = sha('a');
        state.model_sha256 = sha('b');
        state.workspace_sha256 = sha('c');
        assert_ne!(state.semantic_sha256(), old_semantic);
        assert!(state.validate().is_ok());
        let mut stale = iteration(&state, vec![observation("a", OutcomeVerdict::Met)]);
        stale.binding_sha256 = old_binding.clone();
        assert_eq!(state.settle(stale), Err(OutcomeError::StaleBinding));
        let final_check = iteration(&state, vec![observation("a", OutcomeVerdict::Met)]);
        assert_eq!(state.settle(final_check), Ok(OutcomeDisposition::Pass));
        assert_eq!(state.iterations[0].binding_sha256, old_binding);
        assert_eq!(state.iterations[0].semantic_sha256, old_semantic);
        assert!(state.validate().is_ok());
    }

    #[test]
    fn document_checks_require_finite_factual_evidence_and_can_scope_native_repairs() {
        let requested = definition(vec![OutcomeCriterion::Document {
            id: "document".into(),
            path: "report.docx".into(),
            check: DocumentCheck::CompleteRendering,
        }]);
        assert!(requested.validate().is_ok());
        let encoded = serde_json::to_value(&requested).expect("document wire");
        assert_eq!(encoded["criteria"][0]["kind"], "document");
        assert_eq!(encoded["criteria"][0]["check"], "complete_rendering");
        let mut state = run(requested.clone());
        let mut evidence = observation("document", OutcomeVerdict::Met);
        assert!(state
            .settle(iteration(&state, vec![evidence.clone()]))
            .is_err());
        evidence.report_sha256 = Some(sha('a'));
        evidence.locator = Some("arbitrary_fact".into());
        assert!(state
            .settle(iteration(&state, vec![evidence.clone()]))
            .is_err());
        evidence.locator = Some("recalculated_without_errors".into());
        assert!(state
            .settle(iteration(&state, vec![evidence.clone()]))
            .is_err());
        evidence.locator = Some("complete_rendering".into());
        assert_eq!(
            state.settle(iteration(&state, vec![evidence.clone()])),
            Ok(OutcomeDisposition::Pass)
        );
        let mut failed = run(requested);
        evidence.verdict = OutcomeVerdict::NotMet;
        assert_eq!(
            failed.settle(iteration(&failed, vec![evidence])),
            Ok(OutcomeDisposition::Correct)
        );
        assert_eq!(failed.repair_paths, vec!["report.docx"]);
        assert!(failed.validate().is_ok());
        let arbitrary = definition(vec![OutcomeCriterion::ArtifactInspection {
            id: "old".into(),
            report_sha256: sha('a'),
            locator: "arbitrary_fact".into(),
            subject_sha256: sha('b'),
        }]);
        assert!(arbitrary.validate().is_err());
    }
}
