mod documents;
mod journal;
mod repair;
#[cfg(test)]
mod runtime_tests;
#[cfg(test)]
mod tests;
mod verification;

use crate::{
    app::SharedDaemonState,
    runs::{RunLink, RunRecord},
    state::DaemonState,
};
use anima_core::outcomes::{
    OutcomeDefinition, OutcomeDisposition, OutcomeIteration, OutcomeStatus, OutcomeVerdict,
    RunOutcome,
};
use anima_core::{
    AgentConfig, AgentRuntime, Content, Evaluator, EvaluatorResult, Message, Provider,
    ProviderResult, TaskResult, TaskStatus, ToolCall,
};
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::Mutex;

pub(crate) fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
fn hash<T: serde::Serialize>(value: &T) -> String {
    digest(&serde_json::to_vec(value).expect("outcome identity serializes"))
}
pub(crate) fn task_hash(record: &RunRecord) -> String {
    hash(&record.effective_input())
}
impl RunRecord {
    pub(crate) fn allows_outcome_review_delivery(&self) -> bool {
        self.stop.is_none()
            && self.reply_message_id.is_some()
            && self.outcome.as_ref().is_some_and(|outcome| {
                matches!(
                    outcome.status,
                    OutcomeStatus::AwaitingOwner | OutcomeStatus::Accepted
                ) && outcome.validate().is_ok()
                    && outcome.report_sha256.is_some()
                    && outcome.terminal_reply_sha256 == self.reply_sha256
            })
    }
    pub(crate) fn awaits_outcome_acceptance(&self) -> bool {
        self.allows_outcome_review_delivery()
            && self
                .outcome
                .as_ref()
                .is_some_and(|outcome| outcome.status == OutcomeStatus::AwaitingOwner)
    }
}
fn source_hash(record: &RunRecord) -> String {
    hash(&(
        "outcome-checks-v1",
        &record.source_ref,
        &record.project_witness,
        &record.project_context,
    ))
}
fn model_hash(record: &RunRecord) -> String {
    let actual: BTreeSet<_> = record
        .steps
        .iter()
        .filter(|step| step.provider.is_some() || step.model.is_some())
        .map(|step| (&step.provider, &step.model))
        .collect();
    hash(&(
        &record.model,
        &record.provider,
        &record.reasoning_effort,
        actual,
    ))
}
fn live_model_hash(state: &DaemonState, record: &RunRecord, config: &AgentConfig) -> String {
    let mut snapshot = record.clone();
    if !record.status.is_terminal() {
        snapshot.steps = state.live.runs().steps(&record.id);
    }
    hash(&(model_hash(&snapshot), config))
}
fn workspace_hash(root: Option<&Path>) -> String {
    hash(
        &root
            .and_then(|root| crate::tools::workspace_bytes::WorkspaceBytes::open(root).ok())
            .map(|workspace| workspace.canonical_root.to_string_lossy().to_string()),
    )
}

pub(crate) fn attach(
    record: &mut RunRecord,
    definition: OutcomeDefinition,
    root: Option<&Path>,
) -> Result<(), String> {
    if !matches!(
        record.source,
        crate::runs::RunSource::Web | crate::runs::RunSource::Api
    ) || record.parent_run_id.is_some()
        || record.recovery_of.is_some()
    {
        return Err("Outcomes currently require an original ordinary owner chat run".into());
    }
    record.outcome = Some(
        RunOutcome::new(
            definition,
            record.id.clone(),
            task_hash(record),
            source_hash(record),
            model_hash(record),
            workspace_hash(root),
        )
        .map_err(|error| error.to_string())?,
    );
    Ok(())
}

#[derive(Clone)]
pub(crate) struct OutcomeWriteWitness {
    pub(crate) revision: u64,
    pub(crate) path: String,
    pub(crate) expected_sha256: Option<String>,
}
impl OutcomeWriteWitness {
    pub(crate) fn matches(&self, source: Option<&[u8]>) -> bool {
        self.expected_sha256 == source.map(digest)
    }
}
pub(crate) fn write_witness(
    state: &DaemonState,
    run: &RunLink,
    call: &ToolCall,
) -> Result<Option<OutcomeWriteWitness>, String> {
    let record = current(state, run)?;
    let Some(outcome) = &record.outcome else {
        return Ok(None);
    };
    if outcome.status != OutcomeStatus::Correcting
        || !matches!(
            call.name.as_str(),
            "write_file" | "edit_file" | "multi_edit"
        )
    {
        return Ok(None);
    }
    repair::check_call(&outcome.repair_paths, call)?;
    let path = call
        .args
        .get("file_path")
        .and_then(|value| {
            if let anima_core::DataValue::String(path) = value {
                Some(path.replace('\\', "/"))
            } else {
                None
            }
        })
        .ok_or("Outcome write path missing")?;
    let criterion = outcome
        .definition
        .criteria
        .iter()
        .find(|criterion| criterion.path() == Some(path.as_str()))
        .ok_or("Outcome repair criterion missing")?;
    let observation = outcome
        .iterations
        .last()
        .and_then(|iteration| {
            iteration
                .observations
                .iter()
                .find(|observation| observation.criterion_id == criterion.id())
        })
        .ok_or("Outcome repair evidence missing")?;
    Ok(Some(OutcomeWriteWitness {
        revision: outcome.revision,
        path,
        expected_sha256: observation.subject_sha256.clone(),
    }))
}
pub(crate) fn validate_write_witness(
    state: &DaemonState,
    run: &RunLink,
    call: &ToolCall,
    witness: &OutcomeWriteWitness,
) -> Result<(), String> {
    let current = write_witness(state, run, call)?.ok_or("Outcome repair phase ended")?;
    if current.revision != witness.revision
        || current.path != witness.path
        || current.expected_sha256 != witness.expected_sha256
    {
        return Err("Outcome repair generation changed".into());
    }
    let record = state.runs.get(&run.run_id).ok_or("Outcome run missing")?;
    let outcome = record.outcome.as_ref().ok_or("Outcome record missing")?;
    let config = state
        .agents
        .get(&run.agent_id)
        .ok_or("Outcome agent missing")?
        .config();
    if !record.pending_steers.is_empty()
        || outcome.task_sha256 != task_hash(record)
        || outcome.source_sha256 != source_hash(record)
        || outcome.model_sha256 != live_model_hash(state, record, config)
    {
        return Err("Outcome repair task, source or model changed".into());
    }
    crate::projects::validate_outcome_sources(
        state,
        state.runs.get(&run.run_id).ok_or("Outcome run missing")?,
    )
    .map_err(|error| error.to_string())
}

fn observation_allowed(
    criterion: &anima_core::outcomes::OutcomeCriterion,
    original: &AgentConfig,
    current: &AgentConfig,
) -> bool {
    let tool = match criterion {
        anima_core::outcomes::OutcomeCriterion::FileText { .. }
        | anima_core::outcomes::OutcomeCriterion::FileSha256 { .. }
        | anima_core::outcomes::OutcomeCriterion::FileJson { .. } => "read_file",
        anima_core::outcomes::OutcomeCriterion::Document { .. }
        | anima_core::outcomes::OutcomeCriterion::ArtifactInspection { .. } => {
            "inspect_office_document"
        }
        _ => return true,
    };
    original.allows_tool(tool) && current.allows_tool(tool)
}

fn unavailable_observation(
    criterion: &anima_core::outcomes::OutcomeCriterion,
) -> anima_core::outcomes::OutcomeObservation {
    anima_core::outcomes::OutcomeObservation {
        criterion_id: criterion.id().into(),
        verdict: OutcomeVerdict::Inconclusive,
        subject_sha256: None,
        report_sha256: None,
        locator: None,
        detail: "The required current native read or inspection grant is unavailable".into(),
    }
}

fn current<'a>(state: &'a DaemonState, run: &RunLink) -> Result<&'a RunRecord, String> {
    state
        .ensure_profile_owned()
        .map_err(|error| error.to_string())?;
    let record = state
        .runs
        .get(&run.run_id)
        .filter(|record| record.agent_id == run.agent_id && record.session_id == run.session_id)
        .ok_or("Outcome run is unavailable")?;
    if state.sessions.get(&run.agent_id, &run.session_id).is_none()
        || !state.agents.contains_key(&run.agent_id)
    {
        return Err("Outcome owner or session is unavailable".into());
    }
    crate::projects::validate_active_lease(state, record).map_err(|error| error.to_string())?;
    Ok(record)
}

pub(super) fn validate_publication(
    state: &DaemonState,
    run: &RunLink,
    next: &RunOutcome,
) -> Result<(), String> {
    let record = current(state, run)?;
    crate::projects::validate_outcome_sources(state, record).map_err(|error| error.to_string())?;
    let config = state
        .agents
        .get(&run.agent_id)
        .ok_or("Outcome agent missing")?
        .config();
    if record.stop.is_some()
        || !record.pending_steers.is_empty()
        || next.task_sha256 != task_hash(record)
        || next.source_sha256 != source_hash(record)
        || next.model_sha256 != live_model_hash(state, record, config)
    {
        return Err("Outcome task, source, model or grants changed before publication".into());
    }
    let root = state
        .workspace
        .as_ref()
        .map(|workspace| Path::new(&workspace.root_path));
    if next.workspace_sha256 != workspace_hash(root) {
        return Err("Outcome workspace changed before publication".into());
    }
    if matches!(
        next.status,
        OutcomeStatus::CriteriaMet
            | OutcomeStatus::AwaitingOwner
            | OutcomeStatus::Accepted
            | OutcomeStatus::Correcting
    ) {
        let latest = next.iterations.last().ok_or("Outcome report missing")?;
        let workspace =
            root.and_then(|root| crate::tools::workspace_bytes::WorkspaceBytes::open(root).ok());
        for criterion in &next.definition.criteria {
            if let anima_core::outcomes::OutcomeCriterion::ArtifactInspection {
                subject_sha256,
                ..
            } = criterion
            {
                let anchored = next.definition.criteria.iter().any(|candidate| {
                    matches!(
                        candidate,
                        anima_core::outcomes::OutcomeCriterion::Document { .. }
                    ) && latest
                        .observations
                        .iter()
                        .find(|observation| observation.criterion_id == candidate.id())
                        .is_some_and(|observation| {
                            observation.subject_sha256.as_ref() == Some(subject_sha256)
                        })
                });
                if !anchored {
                    return Err(
                        "Pinned report has no matching current document source witness".into(),
                    );
                }
            }
            if let Some(path) = criterion.path() {
                let limit = if matches!(
                    criterion,
                    anima_core::outcomes::OutcomeCriterion::Document { .. }
                ) {
                    4 * 1024 * 1024
                } else {
                    1024 * 1024
                };
                let bytes = workspace
                    .as_ref()
                    .ok_or("Outcome workspace unavailable")?
                    .read_file_snapshot(path, limit);
                let actual = match bytes {
                    Ok(bytes) => Some(digest(&bytes)),
                    Err(crate::tools::workspace_bytes::InstructionReadError::Missing) => None,
                    Err(_) => return Err("Outcome bytes unavailable before publication".into()),
                };
                let expected = latest
                    .observations
                    .iter()
                    .find(|observation| observation.criterion_id == criterion.id())
                    .ok_or("Outcome evidence missing")?;
                if expected.subject_sha256 != actual {
                    return Err("Outcome bytes changed before publication".into());
                }
            }
        }
    }
    if next.status == OutcomeStatus::Accepted {
        let reply = record
            .reply_message_id
            .as_ref()
            .and_then(|id| {
                state
                    .agents
                    .get(&run.agent_id)?
                    .messages()
                    .iter()
                    .find(|message| &message.id == id)
            })
            .ok_or("Outcome final reply missing")?;
        if next.terminal_reply_sha256.as_deref()
            != Some(digest(reply.content.text.as_bytes()).as_str())
        {
            return Err("Outcome final reply changed".into());
        }
    }
    Ok(())
}

pub(crate) async fn close_report(
    state: &SharedDaemonState,
    transactions: Arc<Mutex<()>>,
    agent: &str,
    run_id: &str,
    revision: u64,
) -> Result<RunOutcome, String> {
    let (run, mut outcome) = {
        let guard = state.read().await;
        guard
            .ensure_profile_owned()
            .map_err(|error| error.to_string())?;
        let record = guard
            .runs
            .get(run_id)
            .filter(|record| record.agent_id == agent)
            .ok_or("Outcome run not found")?;
        if !record.status.is_terminal() {
            return Err("Stop the active run before closing its report".into());
        }
        if has_active_children(&guard, run_id) {
            return Err("Wait for retained child work to retire before closing this task".into());
        }
        let outcome = record.outcome.clone().ok_or("Outcome unavailable")?;
        (
            RunLink {
                run_id: run_id.into(),
                agent_id: agent.into(),
                session_id: record.session_id.clone(),
            },
            outcome,
        )
    };
    if outcome.revision != revision {
        return Err("Outcome revision changed; refresh before closing".into());
    }
    outcome
        .hold(OutcomeStatus::Stopped)
        .map_err(|error| error.to_string())?;
    journal::publish(state, transactions, &run, revision, outcome.clone()).await?;
    Ok(outcome)
}

fn has_active_children(state: &DaemonState, root: &str) -> bool {
    state
        .runs
        .snapshot_records(&state.agents.keys().cloned().collect())
        .iter()
        .any(|child| {
            (!child.status.is_terminal()
                || child
                    .async_helper
                    .as_ref()
                    .is_some_and(|helper| helper.completion.is_unresolved()))
                && (child.parent_run_id.as_deref() == Some(root)
                    || child
                        .async_helper
                        .as_ref()
                        .is_some_and(|helper| helper.parent_run_id == root)
                    || child
                        .helper_completion
                        .as_ref()
                        .is_some_and(|origin| origin.launch_parent_run_id == root))
        })
}

pub(crate) async fn check_tool(
    state: &SharedDaemonState,
    run: &RunLink,
    root: Option<&Path>,
    call: &ToolCall,
) -> Result<(), String> {
    let guard = state.read().await;
    let record = current(&guard, run)?;
    let Some(outcome) = &record.outcome else {
        let parent = record
            .parent_run_id
            .as_ref()
            .or_else(|| {
                record
                    .async_helper
                    .as_ref()
                    .map(|helper| &helper.parent_run_id)
            })
            .or_else(|| {
                record
                    .helper_completion
                    .as_ref()
                    .map(|origin| &origin.launch_parent_run_id)
            });
        if parent
            .and_then(|id| guard.runs.get(id))
            .and_then(|parent| parent.outcome.as_ref())
            .is_some_and(|outcome| outcome.status != OutcomeStatus::Pending)
        {
            return Err("The parent outcome entered verification; helper effects are held".into());
        }
        return Ok(());
    };
    match outcome.status {
        OutcomeStatus::Pending => return Ok(()),
        OutcomeStatus::Correcting => repair::check_call(&outcome.repair_paths, call)?,
        _ => {
            return Err(
                "Outcome verification has stopped further task effects; inspect its current status"
                    .into(),
            )
        }
    }
    if !record.pending_steers.is_empty() || task_hash(record) != outcome.task_sha256 {
        return Err("Outcome task input changed before correction".into());
    }
    if matches!(
        call.name.as_str(),
        "write_file" | "edit_file" | "multi_edit"
    ) {
        let path = call
            .args
            .get("file_path")
            .and_then(|value| {
                if let anima_core::DataValue::String(path) = value {
                    Some(path.replace('\\', "/"))
                } else {
                    None
                }
            })
            .ok_or("Missing correction path")?;
        let workspace = root
            .and_then(|root| crate::tools::workspace_bytes::WorkspaceBytes::open(root).ok())
            .ok_or("Correction workspace unavailable")?;
        let latest = outcome
            .iterations
            .last()
            .ok_or("Correction has no captured evidence")?;
        for criterion in outcome
            .definition
            .criteria
            .iter()
            .filter(|criterion| criterion.path() == Some(path.as_str()))
        {
            let old = latest
                .observations
                .iter()
                .find(|observation| observation.criterion_id == criterion.id())
                .ok_or("Correction evidence is unavailable")?;
            let limit = if matches!(
                criterion,
                anima_core::outcomes::OutcomeCriterion::Document { .. }
            ) {
                4 * 1024 * 1024
            } else {
                1024 * 1024
            };
            let actual = match workspace.read_file_snapshot(&path, limit) {
                Ok(bytes) => Some(digest(&bytes)),
                Err(crate::tools::workspace_bytes::InstructionReadError::Missing) => None,
                Err(_) => return Err("Correction bytes are unavailable".into()),
            };
            if old.subject_sha256 != actual {
                return Err(
                    "Correction target changed since inspection; recheck before editing".into(),
                );
            }
        }
    }
    Ok(())
}

#[derive(Clone)]
pub(crate) struct HostOutcomeReview {
    state: SharedDaemonState,
    transactions: Arc<Mutex<()>>,
    run: RunLink,
    root: Option<PathBuf>,
    config: AgentConfig,
    inspections: crate::document_inspections::DocumentInspectionService,
}

impl HostOutcomeReview {
    pub(crate) fn new(
        state: SharedDaemonState,
        transactions: Arc<Mutex<()>>,
        run: RunLink,
        root: Option<PathBuf>,
        config: AgentConfig,
        inspections: crate::document_inspections::DocumentInspectionService,
    ) -> Self {
        Self {
            state,
            transactions,
            run,
            root,
            config,
            inspections,
        }
    }

    async fn capture(&self) -> Result<RunOutcome, String> {
        let owner = { self.state.read().await.profile_owner() };
        if let Some(owner) = owner {
            owner
                .verify_owned()
                .await
                .map_err(|error| error.to_string())?;
        }
        let guard = self.state.read().await;
        let record = current(&guard, &self.run)?;
        crate::projects::validate_outcome_sources(&guard, record)
            .map_err(|error| format!("Outcome selected-source freshness is unverified: {error}"))?;
        let mut outcome = record
            .outcome
            .clone()
            .ok_or("Outcome definition is unavailable")?;
        let live_config = guard
            .agents
            .get(&self.run.agent_id)
            .ok_or("Outcome agent is unavailable")?
            .config();
        if hash(live_config) != hash(&self.config) {
            return Err("Outcome model configuration or grants changed".into());
        }
        let current_model = live_model_hash(&guard, record, &self.config);
        if record.stop.is_some()
            || !record.pending_steers.is_empty()
            || task_hash(record) != outcome.task_sha256
        {
            return Err("Outcome task input changed or stopped".into());
        }
        if record.action_receipts.iter().any(|receipt| {
            receipt.status == crate::runs::ActionStatus::Started
                || receipt.durability != crate::runs::ActionDurability::Persistent
        }) {
            return Err("A native effect is unresolved or lacks durable evidence".into());
        }
        if matches!(
            outcome.status,
            OutcomeStatus::CriteriaMet | OutcomeStatus::AwaitingOwner | OutcomeStatus::Accepted
        ) && (outcome.source_sha256 != source_hash(record)
            || outcome.model_sha256 != current_model)
        {
            return Err("Outcome source or actual model identity changed".into());
        }
        if has_active_children(&guard, &self.run.run_id) {
            return Err("Outcome helper work is still in flight".into());
        }
        outcome.source_sha256 = source_hash(record);
        outcome.model_sha256 = current_model;
        if workspace_hash(self.root.as_deref()) != outcome.workspace_sha256
            || workspace_hash(
                guard
                    .workspace
                    .as_ref()
                    .map(|workspace| Path::new(&workspace.root_path)),
            ) != outcome.workspace_sha256
        {
            return Err("Outcome workspace identity changed".into());
        }
        Ok(outcome)
    }

    async fn observations(
        &self,
        definition: &OutcomeDefinition,
        response: &str,
        execution: &anima_core::execution_limits::ExecutionContext,
        cancel: anima_core::CancelSignal,
    ) -> Vec<anima_core::outcomes::OutcomeObservation> {
        self.collect_observations(definition, response, None, Some(execution), cancel)
            .await
    }

    async fn refresh_observations(
        &self,
        definition: &OutcomeDefinition,
        response: &str,
        previous: &[anima_core::outcomes::OutcomeObservation],
        execution: Option<&anima_core::execution_limits::ExecutionContext>,
        cancel: anima_core::CancelSignal,
    ) -> Vec<anima_core::outcomes::OutcomeObservation> {
        self.collect_observations(definition, response, Some(previous), execution, cancel)
            .await
    }

    async fn collect_observations(
        &self,
        definition: &OutcomeDefinition,
        response: &str,
        previous: Option<&[anima_core::outcomes::OutcomeObservation]>,
        execution: Option<&anima_core::execution_limits::ExecutionContext>,
        cancel: anima_core::CancelSignal,
    ) -> Vec<anima_core::outcomes::OutcomeObservation> {
        let live = self
            .state
            .read()
            .await
            .agents
            .get(&self.run.agent_id)
            .map(|agent| agent.config().clone());
        let mut filtered = definition.clone();
        filtered.criteria.retain(|criterion| {
            live.as_ref()
                .is_some_and(|live| observation_allowed(criterion, &self.config, live))
                && !matches!(
                    criterion,
                    anima_core::outcomes::OutcomeCriterion::Document { .. }
                        | anima_core::outcomes::OutcomeCriterion::ArtifactInspection { .. }
                )
        });
        let ordinary = verification::observe(&filtered, self.root.as_deref(), response);
        let mut observations = Vec::new();
        for criterion in &definition.criteria {
            let mut observation = ordinary
                .iter()
                .find(|observation| observation.criterion_id == criterion.id())
                .cloned()
                .unwrap_or_else(|| unavailable_observation(criterion));
            if !live
                .as_ref()
                .is_some_and(|live| observation_allowed(criterion, &self.config, live))
            {
                observations.push(observation);
                continue;
            }
            if let anima_core::outcomes::OutcomeCriterion::ArtifactInspection {
                subject_sha256,
                ..
            } = criterion
            {
                let anchored = definition.criteria.iter().any(|candidate| {
                    if let anima_core::outcomes::OutcomeCriterion::Document { path, .. } = candidate
                    {
                        self.root.as_deref().is_some_and(|root| {
                            documents::capture_subject(root, path)
                                .is_ok_and(|actual| actual == *subject_sha256)
                        })
                    } else {
                        false
                    }
                });
                if !anchored {
                    observation.detail="A pinned report needs a path-based document criterion to recheck its current source bytes".into();
                    observations.push(observation);
                    continue;
                }
            }
            if matches!(
                criterion,
                anima_core::outcomes::OutcomeCriterion::Document { .. }
            ) && previous.is_some()
            {
                if let Some(prior) = previous.and_then(|previous| {
                    previous
                        .iter()
                        .find(|prior| prior.criterion_id == criterion.id())
                }) {
                    if let (Some(report), Some(subject), Some(locator)) =
                        (&prior.report_sha256, &prior.subject_sha256, &prior.locator)
                    {
                        let pinned = anima_core::outcomes::OutcomeCriterion::ArtifactInspection {
                            id: criterion.id().into(),
                            report_sha256: report.clone(),
                            subject_sha256: subject.clone(),
                            locator: locator.clone(),
                        };
                        observation = documents::observe_document(
                            &pinned,
                            self.root.as_deref(),
                            &self.run,
                            execution,
                            &self.inspections,
                            cancel.clone(),
                        )
                        .await;
                    }
                }
            } else if matches!(
                criterion,
                anima_core::outcomes::OutcomeCriterion::Document { .. }
                    | anima_core::outcomes::OutcomeCriterion::ArtifactInspection { .. }
            ) {
                observation = documents::observe_document(
                    criterion,
                    self.root.as_deref(),
                    &self.run,
                    execution,
                    &self.inspections,
                    cancel.clone(),
                )
                .await;
            }
            observations.push(observation);
        }
        observations
    }

    async fn hold(&self, status: OutcomeStatus) -> Result<(), String> {
        let outcome = self
            .state
            .read()
            .await
            .runs
            .get(&self.run.run_id)
            .and_then(|run| run.outcome.clone())
            .ok_or("Outcome run is unavailable")?;
        if outcome.status == OutcomeStatus::Stopped {
            return Ok(());
        }
        let expected = outcome.revision;
        let mut next = outcome;
        next.hold(status).map_err(|error| error.to_string())?;
        journal::publish(
            &self.state,
            self.transactions.clone(),
            &self.run,
            expected,
            next,
        )
        .await
    }

    pub(crate) async fn finalize(&self, runtime: &AgentRuntime, result: &mut TaskResult<Content>) {
        let response = result
            .data
            .as_ref()
            .map(|content| content.text.as_str())
            .unwrap_or("");
        let captured = self.capture().await;
        let mut valid = false;
        if let Ok(outcome) = &captured {
            if matches!(
                outcome.status,
                OutcomeStatus::CriteriaMet | OutcomeStatus::AwaitingOwner | OutcomeStatus::Accepted
            ) {
                if let Some(latest) = outcome.iterations.last() {
                    let now = self
                        .refresh_observations(
                            &outcome.definition,
                            response,
                            &latest.observations,
                            runtime.execution_context(),
                            runtime
                                .run_control()
                                .map(|control| control.cancel.clone())
                                .unwrap_or_else(anima_core::CancelSignal::new),
                        )
                        .await;
                    valid = result.status == TaskStatus::Success
                        && latest.reply_sha256 == digest(response.as_bytes())
                        && verification::same_subjects(&latest.observations, &now);
                }
            }
        }
        if !valid {
            let stopped = self
                .state
                .read()
                .await
                .runs
                .get(&self.run.run_id)
                .is_some_and(|run| run.stop.is_some());
            let status = if stopped {
                OutcomeStatus::Stopped
            } else if captured
                .as_ref()
                .is_ok_and(|outcome| outcome.status == OutcomeStatus::Exhausted)
            {
                OutcomeStatus::Exhausted
            } else {
                OutcomeStatus::NeedsReview
            };
            let saved = self.hold(status).await;
            if !stopped {
                result.status = TaskStatus::Error;
                result.error = Some(saved.err().unwrap_or_else(|| {
                    "Outcome criteria remain unmet or unverified; inspect the saved evidence".into()
                }));
            }
        } else if captured
            .as_ref()
            .is_ok_and(|outcome| outcome.status == OutcomeStatus::AwaitingOwner)
        {
            result.status = TaskStatus::Error;
            result.error = Some(
                "Outcome criteria met; authenticated owner acceptance is still required".into(),
            );
        }
    }
}

pub(crate) async fn owner_report(
    state: &SharedDaemonState,
    transactions: Arc<Mutex<()>>,
    agent: &str,
    run_id: &str,
    accept: Option<(u64, String)>,
) -> Result<RunOutcome, String> {
    let (run, root, config, reply, original) = {
        let guard = state.read().await;
        let record = guard
            .runs
            .get(run_id)
            .filter(|record| record.agent_id == agent)
            .ok_or("Outcome run not found")?;
        let link = RunLink {
            run_id: run_id.into(),
            agent_id: agent.into(),
            session_id: record.session_id.clone(),
        };
        current(&guard, &link)?;
        let config = guard
            .agents
            .get(agent)
            .ok_or("Outcome agent not found")?
            .config()
            .clone();
        let reply = record
            .reply_message_id
            .as_ref()
            .and_then(|id| {
                guard
                    .agents
                    .get(agent)?
                    .messages()
                    .iter()
                    .find(|message| &message.id == id)
            })
            .map(|message| message.content.text.clone());
        (
            link,
            guard
                .workspace
                .as_ref()
                .map(|workspace| PathBuf::from(&workspace.root_path)),
            config,
            reply,
            record
                .outcome
                .clone()
                .ok_or("Outcome is not configured for this run")?,
        )
    };
    if let Some((revision, report)) = &accept {
        if original.revision != *revision
            || original.status != OutcomeStatus::AwaitingOwner
            || original.report_sha256.as_ref() != Some(report)
        {
            return Err("Outcome revision or report changed; refresh before accepting".into());
        }
    }
    let review = HostOutcomeReview::new(
        state.clone(),
        transactions.clone(),
        run.clone(),
        root,
        config,
        crate::document_inspections::DocumentInspectionService::new(state.clone()),
    );
    if matches!(
        original.status,
        OutcomeStatus::CriteriaMet | OutcomeStatus::AwaitingOwner | OutcomeStatus::Accepted
    ) {
        let fresh = review.capture().await;
        let mut valid = false;
        if let (Ok(outcome), Some(reply), Some(latest)) =
            (&fresh, &reply, original.iterations.last())
        {
            let observations = review
                .refresh_observations(
                    &outcome.definition,
                    reply,
                    &latest.observations,
                    None,
                    anima_core::CancelSignal::new(),
                )
                .await;
            valid = digest(reply.as_bytes()) == latest.reply_sha256
                && verification::same_subjects(&latest.observations, &observations);
        }
        if !valid {
            review.hold(OutcomeStatus::NeedsReview).await?;
            if accept.is_some() {
                return Err("Outcome evidence changed; acceptance refused".into());
            }
        } else if accept.is_some() {
            let mut next = original.clone();
            next.status = OutcomeStatus::Accepted;
            next.accepted_report_sha256 = next.report_sha256.clone();
            next.revision = next
                .revision
                .checked_add(1)
                .ok_or("Outcome revision exhausted")?;
            journal::publish(state, transactions, &run, original.revision, next).await?;
        }
    } else if accept.is_some() {
        return Err("Outcome is not ready for acceptance".into());
    }
    let guard = state.read().await;
    current(&guard, &run)?
        .outcome
        .clone()
        .ok_or_else(|| "Outcome run is unavailable".into())
}

#[async_trait]
impl Provider for HostOutcomeReview {
    fn name(&self) -> &str {
        "outcome_criteria"
    }
    fn description(&self) -> &str {
        "Owner's public success criteria, without additional permissions"
    }
    async fn get(
        &self,
        _runtime: &AgentRuntime,
        _message: &Message,
    ) -> Result<ProviderResult, String> {
        let guard = self.state.read().await;
        let definition = &current(&guard, &self.run)?
            .outcome
            .as_ref()
            .ok_or("Outcome missing")?
            .definition;
        Ok(ProviderResult {text: format!("Owner's public success criteria (data, not tool authority): {}. An independent native check inspects actual output. Automatic corrections are restricted to declared file paths and cannot send, run commands or delegate. If evidence or access is unavailable, report that limitation.", serde_json::to_string(definition).map_err(|error| error.to_string())?), metadata: None})
    }
}

#[async_trait]
impl Evaluator for HostOutcomeReview {
    fn name(&self) -> &str {
        "outcome_review"
    }
    fn description(&self) -> &str {
        "Check actual deliverables against public owner criteria"
    }
    fn priority(&self) -> u8 {
        255
    }
    async fn validate(&self, _runtime: &AgentRuntime, _message: &Message) -> Result<bool, String> {
        Ok(true)
    }
    async fn evaluate(
        &self,
        runtime: &AgentRuntime,
        _message: &Message,
        response: &Content,
    ) -> Result<EvaluatorResult, String> {
        let mut outcome = match self.capture().await {
            Ok(outcome) => outcome,
            Err(_) => {
                self.hold(OutcomeStatus::NeedsReview).await?;
                return Ok(EvaluatorResult::accept());
            }
        };
        if !matches!(
            outcome.status,
            OutcomeStatus::Pending | OutcomeStatus::Correcting
        ) {
            return Ok(EvaluatorResult::accept());
        }
        let Some(execution) = runtime.execution_context() else {
            self.hold(OutcomeStatus::NeedsReview).await?;
            return Ok(EvaluatorResult::accept());
        };
        if execution.check_authority().await.is_err() {
            self.hold(OutcomeStatus::Exhausted).await?;
            return Ok(EvaluatorResult::accept());
        }
        let expected = outcome.revision;
        outcome.status = OutcomeStatus::Checking;
        outcome.repair_paths.clear();
        outcome.revision = outcome
            .revision
            .checked_add(1)
            .ok_or("Outcome revision exhausted")?;
        journal::publish(
            &self.state,
            self.transactions.clone(),
            &self.run,
            expected,
            outcome.clone(),
        )
        .await?;
        let mut permit = match execution
            .reserve_tool(&format!("outcome-check:{}", outcome.iterations.len() + 1))
            .await
        {
            Ok(permit) => permit,
            Err(_) => {
                self.hold(OutcomeStatus::Exhausted).await?;
                return Ok(EvaluatorResult::accept());
            }
        };
        permit.revalidate().await?;
        let observations = self
            .observations(
                &outcome.definition,
                &response.text,
                execution,
                execution.cancel.clone(),
            )
            .await;
        let again = self
            .refresh_observations(
                &outcome.definition,
                &response.text,
                &observations,
                Some(execution),
                execution.cancel.clone(),
            )
            .await;
        let settled = permit
            .finish(anima_core::execution_limits::SettlementOutcome::Known(
                Default::default(),
            ))
            .await;
        if settled.is_err() || !verification::same_subjects(&observations, &again) {
            self.hold(OutcomeStatus::NeedsReview).await?;
            return Ok(EvaluatorResult::accept());
        }
        let current = self.capture().await?;
        if current.revision != outcome.revision
            || current.source_sha256 != outcome.source_sha256
            || current.model_sha256 != outcome.model_sha256
        {
            self.hold(OutcomeStatus::NeedsReview).await?;
            return Ok(EvaluatorResult::accept());
        }
        let iteration = OutcomeIteration {
            ordinal: outcome.iterations.len() as u8 + 1,
            binding_sha256: outcome.binding_sha256(),
            semantic_sha256: outcome.semantic_sha256(),
            reply_sha256: digest(response.text.as_bytes()),
            observations,
        };
        let expected = outcome.revision;
        let disposition = outcome
            .settle(iteration)
            .map_err(|error| error.to_string())?;
        journal::publish(
            &self.state,
            self.transactions.clone(),
            &self.run,
            expected,
            outcome.clone(),
        )
        .await?;
        if disposition == OutcomeDisposition::Correct {
            let feedback: Vec<_> = outcome
                .iterations
                .last()
                .into_iter()
                .flat_map(|iteration| &iteration.observations)
                .filter(|observation| observation.verdict != OutcomeVerdict::Met)
                .map(|observation| format!("{}: {}", observation.criterion_id, observation.detail))
                .collect();
            Ok(EvaluatorResult::retry(format!("Public outcome checks require correction: {}. Repair only the declared files using permitted native file tools, or revise the response. External actions, commands and delegation are disabled during correction. Do not repeat prior effects.", feedback.join("; "))))
        } else {
            Ok(EvaluatorResult::accept())
        }
    }
}
