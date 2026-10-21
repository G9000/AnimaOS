//! Host-independent, durable sequential workflows. Jobs remain owned by the host.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const MAX_WORKFLOW_STEPS: usize = 16;
pub const MAX_WORKFLOW_STEP_ID_BYTES: usize = 64;
pub const MAX_WORKFLOW_TITLE_CHARS: usize = 160;
pub const MAX_WORKFLOW_REQUEST_KEY_BYTES: usize = 128;
pub const MAX_WORKFLOW_PROMPT_BYTES: usize = 8 * 1024;
pub const MAX_WORKFLOW_DEFINITION_BYTES: usize = 32 * 1024;
pub const MAX_WORKFLOW_INPUT_BYTES: usize = 8 * 1024;
pub const MAX_WORKFLOW_RESULT_BYTES: usize = 64 * 1024;
pub const MAX_WORKFLOW_ATTEMPTS: u32 = 3;
pub const MAX_WORKFLOW_DECISIONS: usize = 64;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowStepKind {
    Task,
    Approval,
    Input,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowStatus {
    Running,
    AwaitingApproval,
    AwaitingInput,
    NeedsReview,
    Completed,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowStepStatus {
    Pending,
    Running,
    AwaitingApproval,
    AwaitingInput,
    NeedsReview,
    Completed,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowResumeAction {
    Approve,
    Answer,
    Retry,
    Resolve,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowInput {
    pub title: String,
    pub request_key: String,
    pub steps: Vec<WorkflowStepSpec>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowStepSpec {
    pub id: String,
    pub title: String,
    pub kind: WorkflowStepKind,
    pub prompt: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowStep {
    pub id: String,
    pub title: String,
    pub kind: WorkflowStepKind,
    pub prompt: String,
    pub status: WorkflowStepStatus,
    pub job_id: Option<String>,
    pub attempt: u32,
    pub attempts: Vec<String>,
    pub result: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowResumeRequest {
    pub revision: u64,
    pub action: WorkflowResumeAction,
    pub value: Option<String>,
    #[serde(default)]
    pub acknowledge_uncertain: bool,
}

impl WorkflowResumeRequest {
    /// Checks request shape independently of the workflow's revision and state.
    pub fn validate(&self) -> Result<(), String> {
        validate_resume_request(self)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowDecision {
    pub step_index: usize,
    pub request: WorkflowResumeRequest,
    pub decided_at_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowRun {
    pub id: String,
    pub agent_id: String,
    pub title: String,
    pub request_key: String,
    pub status: WorkflowStatus,
    pub revision: u64,
    pub current_step: usize,
    pub steps: Vec<WorkflowStep>,
    pub decisions: Vec<WorkflowDecision>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_template: Option<crate::workflow_templates::WorkflowTemplateSource>,
}

impl WorkflowRun {
    pub fn new(
        id: impl Into<String>,
        agent_id: impl Into<String>,
        input: WorkflowInput,
        now: u64,
    ) -> Result<Self, String> {
        validate_definition(&input)?;
        let mut run = Self {
            id: id.into(),
            agent_id: agent_id.into(),
            title: input.title,
            request_key: input.request_key,
            status: WorkflowStatus::Running,
            revision: 1,
            current_step: 0,
            steps: input
                .steps
                .into_iter()
                .map(|spec| WorkflowStep {
                    id: spec.id,
                    title: spec.title,
                    kind: spec.kind,
                    prompt: spec.prompt,
                    status: WorkflowStepStatus::Pending,
                    job_id: None,
                    attempt: 0,
                    attempts: Vec::new(),
                    result: None,
                })
                .collect(),
            decisions: Vec::new(),
            created_at_ms: now,
            updated_at_ms: now,
            error: None,
            source_template: None,
        };
        run.activate_current_step();
        run.validate()?;
        Ok(run)
    }

    pub fn definition_matches(&self, input: &WorkflowInput) -> bool {
        self.title == input.title
            && self.request_key == input.request_key
            && self.steps.len() == input.steps.len()
            && self.steps.iter().zip(&input.steps).all(|(saved, spec)| {
                saved.id == spec.id
                    && saved.title == spec.title
                    && saved.kind == spec.kind
                    && saved.prompt == spec.prompt
            })
    }

    pub fn validate(&self) -> Result<(), String> {
        if let Some(source) = &self.source_template {
            source.validate(&self.request_key)?;
        }
        validate_reference(&self.id, "workflow ID")?;
        validate_reference(&self.agent_id, "agent ID")?;
        validate_definition(&WorkflowInput {
            title: self.title.clone(),
            request_key: self.request_key.clone(),
            steps: self
                .steps
                .iter()
                .map(|step| WorkflowStepSpec {
                    id: step.id.clone(),
                    title: step.title.clone(),
                    kind: step.kind,
                    prompt: step.prompt.clone(),
                })
                .collect(),
        })?;
        if self.revision == 0 || self.updated_at_ms < self.created_at_ms {
            return Err("Invalid workflow revision or timestamps".into());
        }
        if self.revision == 1 && self.updated_at_ms != self.created_at_ms {
            return Err("An unchanged workflow must retain its creation timestamp".into());
        }
        if self.current_step > self.steps.len()
            || (self.status == WorkflowStatus::Completed) != (self.current_step == self.steps.len())
        {
            return Err("Workflow current step does not match its status".into());
        }
        match (&self.error, self.status) {
            (Some(error), WorkflowStatus::NeedsReview)
                if !error.trim().is_empty() && error.len() <= MAX_WORKFLOW_RESULT_BYTES => {}
            (None, status) if status != WorkflowStatus::NeedsReview => {}
            _ => return Err("Workflow review error does not match its status".into()),
        }
        let mut job_ids = HashSet::new();
        for (index, step) in self.steps.iter().enumerate() {
            if step.attempt as usize != step.attempts.len() || step.attempt > MAX_WORKFLOW_ATTEMPTS
            {
                return Err("Workflow attempt count does not match its saved jobs".into());
            }
            for job_id in &step.attempts {
                validate_reference(job_id, "workflow job ID")?;
                if !job_ids.insert(job_id.as_str()) {
                    return Err("Workflow job IDs must be unique across all attempts".into());
                }
            }
            if let Some(job_id) = &step.job_id {
                if step.attempts.last() != Some(job_id) {
                    return Err("Current workflow job must be the latest saved attempt".into());
                }
            }
            if step.kind != WorkflowStepKind::Task && (step.attempt != 0 || step.job_id.is_some()) {
                return Err("Workflow checkpoints cannot own jobs".into());
            }
            if index < self.current_step {
                if step.status != WorkflowStepStatus::Completed {
                    return Err("Every earlier workflow step must be completed".into());
                }
            } else if index > self.current_step {
                let expected = if self.status == WorkflowStatus::Cancelled {
                    WorkflowStepStatus::Cancelled
                } else {
                    WorkflowStepStatus::Pending
                };
                if step.status != expected || step.attempt != 0 || step.job_id.is_some() {
                    return Err("A later workflow step cannot have started".into());
                }
            } else {
                let valid = match self.status {
                    WorkflowStatus::Running => {
                        step.kind == WorkflowStepKind::Task
                            && matches!(
                                step.status,
                                WorkflowStepStatus::Pending | WorkflowStepStatus::Running
                            )
                    }
                    WorkflowStatus::AwaitingApproval => {
                        step.kind == WorkflowStepKind::Approval
                            && step.status == WorkflowStepStatus::AwaitingApproval
                    }
                    WorkflowStatus::AwaitingInput => {
                        step.kind == WorkflowStepKind::Input
                            && step.status == WorkflowStepStatus::AwaitingInput
                    }
                    WorkflowStatus::NeedsReview => {
                        step.kind == WorkflowStepKind::Task
                            && step.status == WorkflowStepStatus::NeedsReview
                    }
                    WorkflowStatus::Cancelled => step.status == WorkflowStepStatus::Cancelled,
                    WorkflowStatus::Completed => false,
                };
                if !valid {
                    return Err("Current workflow step does not match its status".into());
                }
            }
            if step.status == WorkflowStepStatus::Completed {
                match (step.kind, &step.result) {
                    (WorkflowStepKind::Task, Some(result))
                        if result.len() <= MAX_WORKFLOW_RESULT_BYTES => {}
                    (WorkflowStepKind::Input, Some(result))
                        if !result.trim().is_empty()
                            && result.len() <= MAX_WORKFLOW_INPUT_BYTES => {}
                    (WorkflowStepKind::Approval, None) => {}
                    _ => return Err("Completed workflow step has an invalid saved result".into()),
                }
            } else if step.result.is_some() {
                return Err("Unfinished workflow steps cannot contain results".into());
            }
        }
        if self.decisions.len() > MAX_WORKFLOW_DECISIONS {
            return Err("Workflow decision history limit reached".into());
        }
        let mut last_revision = 0;
        let mut last_timestamp = self.created_at_ms;
        for decision in &self.decisions {
            validate_resume_request(&decision.request)?;
            if decision.step_index >= self.steps.len()
                || decision.request.revision <= last_revision
                || decision.request.revision >= self.revision
                || decision.decided_at_ms < last_timestamp
                || decision.decided_at_ms > self.updated_at_ms
                || (decision.request.revision + 1 == self.revision
                    && decision.decided_at_ms != self.updated_at_ms)
            {
                return Err("Invalid workflow decision step, revision or timestamps".into());
            }
            last_revision = decision.request.revision;
            last_timestamp = decision.decided_at_ms;
        }
        if !self.history_is_reachable() {
            return Err("Saved workflow state cannot result from its recorded history".into());
        }
        Ok(())
    }

    pub fn attach_job(&mut self, job_id: impl Into<String>, now: u64) -> Result<(), String> {
        self.validate()?;
        let job_id = job_id.into();
        validate_reference(&job_id, "workflow job ID")?;
        if self.status != WorkflowStatus::Running
            || self.steps[self.current_step].status != WorkflowStepStatus::Pending
            || self.steps[self.current_step].attempt >= MAX_WORKFLOW_ATTEMPTS
        {
            return Err("Workflow is not awaiting task admission".into());
        }
        if self
            .steps
            .iter()
            .any(|step| step.attempts.contains(&job_id))
        {
            return Err("Workflow job ID was already used by a saved attempt".into());
        }
        let revision = self.next_revision(now)?;
        let step = &mut self.steps[self.current_step];
        step.attempt += 1;
        step.attempts.push(job_id.clone());
        step.job_id = Some(job_id);
        step.status = WorkflowStepStatus::Running;
        self.revision = revision;
        self.updated_at_ms = now;
        Ok(())
    }

    pub fn complete_task(&mut self, result: impl Into<String>, now: u64) -> Result<(), String> {
        self.validate()?;
        let result = result.into();
        if self.status != WorkflowStatus::Running
            || self.steps[self.current_step].status != WorkflowStepStatus::Running
        {
            return Err("Only a running workflow task can complete".into());
        }
        if result.len() > MAX_WORKFLOW_RESULT_BYTES {
            return Err("Workflow result exceeds 64 KiB".into());
        }
        let revision = self.next_revision(now)?;
        self.steps[self.current_step].result = Some(result);
        self.steps[self.current_step].status = WorkflowStepStatus::Completed;
        self.current_step += 1;
        self.activate_current_step();
        self.revision = revision;
        self.updated_at_ms = now;
        Ok(())
    }

    pub fn needs_review(&mut self, error: impl Into<String>, now: u64) -> Result<(), String> {
        self.validate()?;
        let error = error.into();
        if self.status != WorkflowStatus::Running
            || !matches!(
                self.steps[self.current_step].status,
                WorkflowStepStatus::Pending | WorkflowStepStatus::Running
            )
        {
            return Err("Only a pending or running workflow task can require review".into());
        }
        if error.trim().is_empty() || error.len() > MAX_WORKFLOW_RESULT_BYTES {
            return Err("Workflow review requires a bounded nonempty error".into());
        }
        let revision = self.next_revision(now)?;
        self.steps[self.current_step].status = WorkflowStepStatus::NeedsReview;
        self.status = WorkflowStatus::NeedsReview;
        self.error = Some(error);
        self.revision = revision;
        self.updated_at_ms = now;
        Ok(())
    }

    pub fn resume(&mut self, request: WorkflowResumeRequest, now: u64) -> Result<bool, String> {
        self.validate()?;
        if let Some(saved) = self
            .decisions
            .iter()
            .find(|saved| saved.request.revision == request.revision)
        {
            return if saved.request == request {
                Ok(false)
            } else {
                Err("A different workflow decision is already saved at that revision".into())
            };
        }
        if request.revision != self.revision {
            return Err("Workflow revision conflict; refresh the saved workflow".into());
        }
        validate_resume_request(&request)?;
        if self.decisions.len() >= MAX_WORKFLOW_DECISIONS {
            return Err("Workflow decision history limit reached".into());
        }
        let valid = match request.action {
            WorkflowResumeAction::Approve => self.status == WorkflowStatus::AwaitingApproval,
            WorkflowResumeAction::Answer => self.status == WorkflowStatus::AwaitingInput,
            WorkflowResumeAction::Retry | WorkflowResumeAction::Resolve => {
                self.status == WorkflowStatus::NeedsReview
            }
        };
        if !valid {
            return Err("Workflow decision does not match the current checkpoint".into());
        }
        if request.action == WorkflowResumeAction::Retry
            && self.steps[self.current_step].attempt >= MAX_WORKFLOW_ATTEMPTS
        {
            return Err("Workflow task has exhausted its three attempts".into());
        }
        let revision = self.next_revision(now)?;
        let step_index = self.current_step;
        let step = &mut self.steps[self.current_step];
        if request.action == WorkflowResumeAction::Retry {
            step.status = WorkflowStepStatus::Pending;
            step.job_id = None;
            self.status = WorkflowStatus::Running;
        } else {
            step.result = request.value.clone();
            step.status = WorkflowStepStatus::Completed;
            self.current_step += 1;
            self.activate_current_step();
        }
        self.error = None;
        self.decisions.push(WorkflowDecision {
            step_index,
            request,
            decided_at_ms: now,
        });
        self.revision = revision;
        self.updated_at_ms = now;
        Ok(true)
    }

    pub fn cancel(&mut self, revision: u64, now: u64) -> Result<bool, String> {
        self.validate()?;
        if self.status == WorkflowStatus::Cancelled {
            return if revision.checked_add(1) == Some(self.revision) {
                Ok(false)
            } else {
                Err("Workflow cancellation revision conflict".into())
            };
        }
        if revision != self.revision {
            return Err("Workflow revision conflict; refresh the saved workflow".into());
        }
        if self.status == WorkflowStatus::Completed {
            return Err("A completed workflow cannot be cancelled".into());
        }
        let revision = self.next_revision(now)?;
        for step in &mut self.steps[self.current_step..] {
            step.status = WorkflowStepStatus::Cancelled;
        }
        self.status = WorkflowStatus::Cancelled;
        self.error = None;
        self.revision = revision;
        self.updated_at_ms = now;
        Ok(true)
    }

    fn next_revision(&self, now: u64) -> Result<u64, String> {
        if now < self.updated_at_ms {
            return Err("Workflow timestamps cannot move backwards".into());
        }
        self.revision
            .checked_add(1)
            .ok_or_else(|| "Workflow revision exhausted".into())
    }

    fn activate_current_step(&mut self) {
        let Some(step) = self.steps.get_mut(self.current_step) else {
            self.status = WorkflowStatus::Completed;
            return;
        };
        (self.status, step.status) = match step.kind {
            WorkflowStepKind::Task => (WorkflowStatus::Running, WorkflowStepStatus::Pending),
            WorkflowStepKind::Approval => (
                WorkflowStatus::AwaitingApproval,
                WorkflowStepStatus::AwaitingApproval,
            ),
            WorkflowStepKind::Input => (
                WorkflowStatus::AwaitingInput,
                WorkflowStepStatus::AwaitingInput,
            ),
        };
    }

    /// Decisions are explicit events; attaching, settling and reviewing jobs are
    /// implicit events evidenced by the saved attempts. Walk only these bounded
    /// events to reject impossible restored progress without inventing receipts.
    fn history_is_reachable(&self) -> bool {
        let mut stack = vec![ReplayState {
            step: 0,
            phase: initial_phase(self.steps[0].kind),
            attempt: 0,
            attached: false,
            decision: 0,
            revision: 1,
        }];
        let mut visited = HashSet::new();
        while let Some(state) = stack.pop() {
            if state.revision > self.revision || !visited.insert(state) {
                continue;
            }
            let next_decision = self.decisions.get(state.decision);
            if next_decision.is_some_and(|decision| decision.request.revision < state.revision) {
                continue;
            }
            if state.decision == self.decisions.len() && state.step == self.current_step {
                if state.revision == self.revision && self.replay_matches_current(state) {
                    return true;
                }
                if self.status == WorkflowStatus::Cancelled
                    && state.revision.checked_add(1) == Some(self.revision)
                    && self.replay_matches_attempt(state)
                {
                    return true;
                }
            }
            let Some(step) = self.steps.get(state.step) else {
                continue;
            };
            if let Some(decision) =
                next_decision.filter(|decision| decision.request.revision == state.revision)
            {
                if decision.step_index != state.step {
                    continue;
                }
                let mut next = state;
                next.revision += 1;
                next.decision += 1;
                match (state.phase, decision.request.action) {
                    (WorkflowStepStatus::NeedsReview, WorkflowResumeAction::Retry)
                        if state.attempt < MAX_WORKFLOW_ATTEMPTS =>
                    {
                        next.phase = WorkflowStepStatus::Pending;
                        next.attached = false;
                        stack.push(next);
                    }
                    (WorkflowStepStatus::NeedsReview, WorkflowResumeAction::Resolve)
                        if self.replay_can_complete(state, decision.request.value.as_deref()) =>
                    {
                        stack.push(self.replay_advance(next));
                    }
                    (WorkflowStepStatus::AwaitingApproval, WorkflowResumeAction::Approve)
                        if self.replay_can_complete(state, None) =>
                    {
                        stack.push(self.replay_advance(next));
                    }
                    (WorkflowStepStatus::AwaitingInput, WorkflowResumeAction::Answer)
                        if self.replay_can_complete(state, decision.request.value.as_deref()) =>
                    {
                        stack.push(self.replay_advance(next));
                    }
                    _ => {}
                }
                continue;
            }
            if state.revision >= self.revision {
                continue;
            }
            let mut next = state;
            next.revision += 1;
            match state.phase {
                WorkflowStepStatus::Pending => {
                    if (state.attempt as usize) < step.attempts.len() {
                        let mut attached = next;
                        attached.attempt += 1;
                        attached.attached = true;
                        attached.phase = WorkflowStepStatus::Running;
                        stack.push(attached);
                    }
                    next.phase = WorkflowStepStatus::NeedsReview;
                    stack.push(next);
                }
                WorkflowStepStatus::Running => {
                    if self.replay_can_complete(state, step.result.as_deref()) {
                        stack.push(self.replay_advance(next));
                    }
                    next.phase = WorkflowStepStatus::NeedsReview;
                    stack.push(next);
                }
                _ => {}
            }
        }
        false
    }

    fn replay_matches_attempt(&self, state: ReplayState) -> bool {
        let Some(step) = self.steps.get(state.step) else {
            return self.status == WorkflowStatus::Completed;
        };
        state.attempt == step.attempt
            && if state.attached {
                step.job_id.as_ref() == step.attempts.last()
            } else {
                step.job_id.is_none()
            }
    }

    fn replay_matches_current(&self, state: ReplayState) -> bool {
        if self.status == WorkflowStatus::Completed {
            return state.step == self.steps.len();
        }
        if self.status == WorkflowStatus::Cancelled {
            return false;
        }
        self.replay_matches_attempt(state) && self.steps[state.step].status == state.phase
    }

    fn replay_can_complete(&self, state: ReplayState, result: Option<&str>) -> bool {
        let step = &self.steps[state.step];
        step.status == WorkflowStepStatus::Completed
            && self.replay_matches_attempt(state)
            && step.result.as_deref() == result
    }

    fn replay_advance(&self, mut state: ReplayState) -> ReplayState {
        state.step += 1;
        state.attempt = 0;
        state.attached = false;
        state.phase = self
            .steps
            .get(state.step)
            .map(|step| initial_phase(step.kind))
            .unwrap_or(WorkflowStepStatus::Completed);
        state
    }
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
struct ReplayState {
    step: usize,
    phase: WorkflowStepStatus,
    attempt: u32,
    attached: bool,
    decision: usize,
    revision: u64,
}

fn initial_phase(kind: WorkflowStepKind) -> WorkflowStepStatus {
    match kind {
        WorkflowStepKind::Task => WorkflowStepStatus::Pending,
        WorkflowStepKind::Approval => WorkflowStepStatus::AwaitingApproval,
        WorkflowStepKind::Input => WorkflowStepStatus::AwaitingInput,
    }
}

fn validate_reference(value: &str, name: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
        return Err(format!("Invalid {name}"));
    }
    Ok(())
}

fn validate_definition(input: &WorkflowInput) -> Result<(), String> {
    if input.steps.is_empty() || input.steps.len() > MAX_WORKFLOW_STEPS {
        return Err("Workflow must have between 1 and 16 steps".into());
    }
    if input.title.trim().is_empty() || input.title.chars().count() > MAX_WORKFLOW_TITLE_CHARS {
        return Err("Workflow title must be nonempty and at most 160 characters".into());
    }
    if input.request_key.trim().is_empty()
        || input.request_key.len() > MAX_WORKFLOW_REQUEST_KEY_BYTES
    {
        return Err("Workflow request key must be nonempty and at most 128 bytes".into());
    }
    let mut ids = HashSet::new();
    let mut definition_bytes = input.title.len() + input.request_key.len();
    for step in &input.steps {
        if step.id.is_empty()
            || step.id.len() > MAX_WORKFLOW_STEP_ID_BYTES
            || !step
                .id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            || !ids.insert(step.id.as_str())
        {
            return Err("Workflow step IDs must be unique, 1-64 ASCII letters, digits, underscores or hyphens".into());
        }
        if step.title.trim().is_empty()
            || step.prompt.trim().is_empty()
            || step.title.chars().count() > MAX_WORKFLOW_TITLE_CHARS
            || step.prompt.len() > MAX_WORKFLOW_PROMPT_BYTES
        {
            return Err(
                "Workflow step title and prompt must be nonempty and within their limits".into(),
            );
        }
        definition_bytes += step.id.len() + step.title.len() + step.prompt.len();
    }
    if definition_bytes > MAX_WORKFLOW_DEFINITION_BYTES {
        return Err("Workflow definition exceeds 32 KiB".into());
    }
    Ok(())
}

fn validate_resume_request(request: &WorkflowResumeRequest) -> Result<(), String> {
    if request.revision == 0 {
        return Err("Workflow decision revision must be positive".into());
    }
    match request.action {
        WorkflowResumeAction::Approve | WorkflowResumeAction::Retry => {
            if request.value.is_some() {
                return Err("This workflow decision does not accept a value".into());
            }
        }
        WorkflowResumeAction::Answer | WorkflowResumeAction::Resolve => {
            if !request.value.as_ref().is_some_and(|value| {
                !value.trim().is_empty() && value.len() <= MAX_WORKFLOW_INPUT_BYTES
            }) {
                return Err(
                    "Workflow answer or resolution requires nonempty text up to 8 KiB".into(),
                );
            }
        }
    }
    if matches!(
        request.action,
        WorkflowResumeAction::Retry | WorkflowResumeAction::Resolve
    ) && !request.acknowledge_uncertain
    {
        return Err(
            "Acknowledge uncertain prior effects before retrying or resolving a task".into(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn definition(kinds: &[WorkflowStepKind]) -> WorkflowInput {
        WorkflowInput {
            title: "Saved work".into(),
            request_key: "request-1".into(),
            steps: kinds
                .iter()
                .enumerate()
                .map(|(i, kind)| WorkflowStepSpec {
                    id: format!("step-{i}"),
                    title: format!("Step {i}"),
                    kind: *kind,
                    prompt: format!("Instructions {i}"),
                })
                .collect(),
        }
    }

    fn run(kinds: &[WorkflowStepKind]) -> WorkflowRun {
        WorkflowRun::new("workflow-1", "agent-1", definition(kinds), 10).unwrap()
    }

    fn request(
        run: &WorkflowRun,
        action: WorkflowResumeAction,
        value: Option<&str>,
    ) -> WorkflowResumeRequest {
        WorkflowResumeRequest {
            revision: run.revision,
            action,
            value: value.map(str::to_owned),
            acknowledge_uncertain: matches!(
                action,
                WorkflowResumeAction::Retry | WorkflowResumeAction::Resolve
            ),
        }
    }

    #[test]
    fn workflows_progress_preserves_completed_tasks_through_checkpoints() {
        let mut run = run(&[
            WorkflowStepKind::Task,
            WorkflowStepKind::Approval,
            WorkflowStepKind::Input,
            WorkflowStepKind::Task,
        ]);
        assert_eq!(run.revision, 1);
        assert_eq!(run.steps[0].status, WorkflowStepStatus::Pending);
        run.attach_job("job-1", 11).unwrap();
        run.complete_task("first saved result", 12).unwrap();
        assert_eq!(run.status, WorkflowStatus::AwaitingApproval);
        let approve = request(&run, WorkflowResumeAction::Approve, None);
        run.resume(approve, 13).unwrap();
        assert_eq!(run.status, WorkflowStatus::AwaitingInput);
        let answer = request(&run, WorkflowResumeAction::Answer, Some("owner answer"));
        run.resume(answer, 14).unwrap();
        assert_eq!(run.steps[0].result.as_deref(), Some("first saved result"));
        assert_eq!(run.steps[2].result.as_deref(), Some("owner answer"));
        assert_eq!(run.steps[3].status, WorkflowStepStatus::Pending);
        run.attach_job("job-2", 15).unwrap();
        run.complete_task("", 16).unwrap();
        assert_eq!(run.status, WorkflowStatus::Completed);
        assert_eq!(run.current_step, 4);
        assert_eq!(run.steps[3].result.as_deref(), Some(""));
        run.validate().unwrap();
    }

    #[test]
    fn workflows_checkpoint_restore_replays_exact_decision_after_later_progress() {
        let mut run = run(&[WorkflowStepKind::Approval, WorkflowStepKind::Input]);
        let approve = request(&run, WorkflowResumeAction::Approve, None);
        assert!(run.resume(approve.clone(), 11).unwrap());
        let encoded = serde_json::to_string(&run).unwrap();
        let mut restored: WorkflowRun = serde_json::from_str(&encoded).unwrap();
        restored.validate().unwrap();
        let answer = request(&restored, WorkflowResumeAction::Answer, Some("yes"));
        restored.resume(answer, 12).unwrap();
        let final_state = restored.clone();
        assert!(!restored.resume(approve.clone(), 1).unwrap());
        assert_eq!(restored, final_state);
        let mut changed = approve;
        changed.acknowledge_uncertain = true;
        assert!(restored.resume(changed, 13).is_err());
        let mut stale = request(&restored, WorkflowResumeAction::Approve, None);
        stale.revision = 0;
        assert!(restored.resume(stale, 13).is_err());
    }

    #[test]
    fn workflows_decisions_record_the_exact_step_the_owner_resumed() {
        let mut run = run(&[WorkflowStepKind::Approval, WorkflowStepKind::Task]);
        let approve = request(&run, WorkflowResumeAction::Approve, None);
        run.resume(approve, 11).unwrap();
        run.needs_review("effects uncertain", 12).unwrap();
        let resolve = request(&run, WorkflowResumeAction::Resolve, Some("done"));
        run.resume(resolve, 13).unwrap();
        assert_eq!(run.decisions[0].step_index, 0);
        assert_eq!(run.decisions[1].step_index, 1);
        let wire = serde_json::to_value(&run).unwrap();
        assert_eq!(wire["decisions"][1]["stepIndex"], 1);
        run.validate().unwrap();
    }

    #[test]
    fn workflows_restore_rejects_owner_resolution_stamped_for_another_step() {
        let mut run = run(&[WorkflowStepKind::Task, WorkflowStepKind::Task]);
        run.needs_review("owner must verify", 11).unwrap();
        let resolve = request(&run, WorkflowResumeAction::Resolve, Some("done"));
        run.resume(resolve, 12).unwrap();
        run.validate().unwrap();
        let mut forged = run.clone();
        forged.decisions[0].step_index = 1;
        assert!(
            forged.validate().is_err(),
            "accepted resolution evidence for a different step"
        );
        let mut forged = run;
        forged.decisions[0].step_index = usize::MAX;
        assert!(
            forged.validate().is_err(),
            "accepted an out-of-range resolution step"
        );
    }

    #[test]
    fn workflows_uncertainty_blocks_progress_and_retry_preserves_all_attempts() {
        let mut run = run(&[WorkflowStepKind::Task, WorkflowStepKind::Task]);
        for attempt in 1..=MAX_WORKFLOW_ATTEMPTS {
            let now = 10 + u64::from(attempt) * 10;
            run.attach_job(format!("job-{attempt}"), now).unwrap();
            run.needs_review("effects uncertain", now + 1).unwrap();
            assert!(run.complete_task("late output", now + 2).is_err());
            assert_eq!(run.current_step, 0);
            let mut retry = request(&run, WorkflowResumeAction::Retry, None);
            retry.acknowledge_uncertain = false;
            let before = run.clone();
            assert!(run.resume(retry.clone(), now + 2).is_err());
            assert_eq!(run, before);
            retry.acknowledge_uncertain = true;
            if attempt < MAX_WORKFLOW_ATTEMPTS {
                run.resume(retry, now + 2).unwrap();
                assert!(run.steps[0].job_id.is_none());
                assert_eq!(run.steps[0].attempt, attempt);
            } else {
                assert!(run.resume(retry, now + 2).is_err());
            }
        }
        assert_eq!(run.steps[0].attempts, ["job-1", "job-2", "job-3"]);
        let resolve = request(
            &run,
            WorkflowResumeAction::Resolve,
            Some("owner verified result"),
        );
        run.resume(resolve, 50).unwrap();
        assert_eq!(run.steps[0].attempts.len(), 3);
        assert_eq!(
            run.steps[0].result.as_deref(),
            Some("owner verified result")
        );
        assert_eq!(run.current_step, 1);
        run.validate().unwrap();
    }

    #[test]
    fn workflows_review_before_job_admission_can_be_retried_or_resolved() {
        let mut retry_run = run(&[WorkflowStepKind::Task]);
        retry_run.needs_review("retained history full", 11).unwrap();
        assert_eq!(retry_run.steps[0].attempt, 0);
        let retry = request(&retry_run, WorkflowResumeAction::Retry, None);
        retry_run.resume(retry, 12).unwrap();
        retry_run.attach_job("new-slot", 13).unwrap();
        retry_run.complete_task("done", 14).unwrap();
        retry_run.validate().unwrap();

        let mut resolved = run(&[WorkflowStepKind::Task]);
        resolved.needs_review("retained history full", 11).unwrap();
        let decision = request(
            &resolved,
            WorkflowResumeAction::Resolve,
            Some("owner handled it"),
        );
        resolved.resume(decision, 12).unwrap();
        assert_eq!(resolved.status, WorkflowStatus::Completed);
        assert_eq!(resolved.steps[0].attempts.len(), 0);
        resolved.validate().unwrap();
    }

    #[test]
    fn workflows_cancel_is_exactly_idempotent_and_late_results_cannot_advance() {
        let mut run = run(&[
            WorkflowStepKind::Task,
            WorkflowStepKind::Task,
            WorkflowStepKind::Approval,
        ]);
        run.attach_job("done-job", 11).unwrap();
        run.complete_task("keep me", 12).unwrap();
        run.attach_job("racing-job", 13).unwrap();
        let revision = run.revision;
        assert!(run.cancel(revision, 14).unwrap());
        let saved = run.clone();
        assert!(!run.cancel(revision, 1).unwrap());
        assert_eq!(run, saved);
        assert!(run.cancel(run.revision, 15).is_err());
        assert!(run.complete_task("late", 15).is_err());
        assert!(run.attach_job("later", 15).is_err());
        assert_eq!(run.steps[0].result.as_deref(), Some("keep me"));
        assert_eq!(run.steps[1].job_id.as_deref(), Some("racing-job"));
        assert_eq!(run.steps[2].status, WorkflowStepStatus::Cancelled);
        run.validate().unwrap();
    }

    #[test]
    fn workflows_reject_wrong_checkpoint_actions_and_invalid_values_atomically() {
        let mut run = run(&[WorkflowStepKind::Input]);
        for (action, value) in [
            (WorkflowResumeAction::Approve, None),
            (WorkflowResumeAction::Answer, Some("  ")),
            (WorkflowResumeAction::Retry, None),
            (WorkflowResumeAction::Resolve, Some("result")),
        ] {
            let saved = run.clone();
            let request = request(&run, action, value);
            assert!(run.resume(request, 11).is_err());
            assert_eq!(run, saved);
        }
        let huge = "a".repeat(MAX_WORKFLOW_INPUT_BYTES + 1);
        let request = request(&run, WorkflowResumeAction::Answer, Some(&huge));
        assert!(run.resume(request, 11).is_err());
    }

    #[test]
    fn workflows_duplicate_historical_job_ids_cannot_be_attached() {
        let mut run = run(&[WorkflowStepKind::Task, WorkflowStepKind::Task]);
        run.attach_job("job", 11).unwrap();
        run.needs_review("failed", 12).unwrap();
        let retry = request(&run, WorkflowResumeAction::Retry, None);
        run.resume(retry, 13).unwrap();
        assert!(run.attach_job("job", 14).is_err());
        run.attach_job("job-2", 14).unwrap();
        run.complete_task("done", 15).unwrap();
        assert!(run.attach_job("job", 16).is_err());
        assert!(run.attach_job("job-2", 16).is_err());
    }

    #[test]
    fn workflows_definition_bounds_and_identity_are_enforced() {
        let good = definition(&[WorkflowStepKind::Task]);
        let run = WorkflowRun::new("id", "agent", good.clone(), 10).unwrap();
        assert!(run.definition_matches(&good));
        let mut changed = good.clone();
        changed.steps[0].prompt.push('!');
        assert!(!run.definition_matches(&changed));
        let mut invalids = vec![];
        let mut invalid = good.clone();
        invalid.steps.clear();
        invalids.push(invalid);
        let mut invalid = good.clone();
        invalid.steps = vec![invalid.steps[0].clone(); MAX_WORKFLOW_STEPS + 1];
        invalids.push(invalid);
        let mut invalid = good.clone();
        invalid.steps.push(invalid.steps[0].clone());
        invalids.push(invalid);
        let mut invalid = good.clone();
        invalid.steps[0].id = "bad/id".into();
        invalids.push(invalid);
        let mut invalid = good.clone();
        invalid.steps[0].id = "x".repeat(65);
        invalids.push(invalid);
        let mut invalid = good.clone();
        invalid.title = "é".repeat(161);
        invalids.push(invalid);
        let mut invalid = good.clone();
        invalid.request_key = "é".repeat(65);
        invalids.push(invalid);
        let mut invalid = good.clone();
        invalid.request_key.clear();
        invalids.push(invalid);
        let mut invalid = good.clone();
        invalid.steps[0].prompt = "a".repeat(MAX_WORKFLOW_PROMPT_BYTES + 1);
        invalids.push(invalid);
        let mut invalid = definition(&[WorkflowStepKind::Task; 4]);
        for step in &mut invalid.steps {
            step.prompt = "a".repeat(MAX_WORKFLOW_PROMPT_BYTES);
        }
        invalids.push(invalid);
        for invalid in invalids {
            assert!(WorkflowRun::new("id", "agent", invalid, 10).is_err());
        }
        assert!(WorkflowRun::new("", "agent", good.clone(), 10).is_err());
        assert!(WorkflowRun::new("id", "", good, 10).is_err());
    }

    fn assert_blank_definition_field_rejected(field: &str) {
        for blank in ["", " \t\r\n", "\u{00a0}"] {
            let mut input = definition(&[WorkflowStepKind::Task]);
            let mut restored = run(&[WorkflowStepKind::Task]);
            match field {
                "title" => {
                    input.title = blank.into();
                    restored.title = blank.into();
                }
                "requestKey" => {
                    input.request_key = blank.into();
                    restored.request_key = blank.into();
                }
                "stepTitle" => {
                    input.steps[0].title = blank.into();
                    restored.steps[0].title = blank.into();
                }
                "prompt" => {
                    input.steps[0].prompt = blank.into();
                    restored.steps[0].prompt = blank.into();
                }
                _ => unreachable!(),
            }
            assert!(
                WorkflowRun::new("workflow", "agent", input, 10).is_err(),
                "accepted blank {field} at creation: {blank:?}"
            );
            assert!(
                restored.validate().is_err(),
                "accepted blank {field} at restore: {blank:?}"
            );
        }
    }

    #[test]
    fn workflows_rejects_blank_workflow_title() {
        assert_blank_definition_field_rejected("title");
    }

    #[test]
    fn workflows_rejects_blank_step_title() {
        assert_blank_definition_field_rejected("stepTitle");
    }

    #[test]
    fn workflows_rejects_blank_prompt() {
        assert_blank_definition_field_rejected("prompt");
    }

    #[test]
    fn workflows_rejects_blank_request_key() {
        assert_blank_definition_field_rejected("requestKey");
    }

    #[test]
    fn workflows_request_wire_contract_is_camel_case_strict_and_defaults_acknowledgement() {
        let request: WorkflowResumeRequest =
            serde_json::from_value(json!({"revision":1,"action":"approve"})).unwrap();
        assert!(!request.acknowledge_uncertain);
        assert!(serde_json::from_value::<WorkflowResumeRequest>(
            json!({"revision":1,"action":"approve","unexpected":true})
        )
        .is_err());
        let mut input = serde_json::to_value(definition(&[WorkflowStepKind::Task])).unwrap();
        assert_eq!(input["requestKey"], "request-1");
        input["steps"][0]["unknown"] = json!(true);
        assert!(serde_json::from_value::<WorkflowInput>(input).is_err());
    }

    #[test]
    fn workflows_restore_rejects_impossible_links_progress_and_revisions() {
        let initial = run(&[WorkflowStepKind::Task, WorkflowStepKind::Approval]);
        let mut invalids = vec![];
        let mut invalid = initial.clone();
        invalid.revision = 0;
        invalids.push(invalid);
        let mut invalid = initial.clone();
        invalid.revision = 2;
        invalids.push(invalid);
        let mut invalid = initial.clone();
        invalid.current_step = 1;
        invalids.push(invalid);
        let mut invalid = initial.clone();
        invalid.steps[1].status = WorkflowStepStatus::Completed;
        invalids.push(invalid);
        let mut invalid = initial.clone();
        invalid.steps[0].result = Some("premature".into());
        invalids.push(invalid);
        let mut invalid = initial.clone();
        invalid.steps[0].attempt = 1;
        invalids.push(invalid);
        let mut invalid = initial.clone();
        invalid.steps[0].attempts.push("hidden".into());
        invalids.push(invalid);
        let mut invalid = initial.clone();
        invalid.updated_at_ms = 9;
        invalids.push(invalid);
        let mut invalid = initial.clone();
        invalid.status = WorkflowStatus::NeedsReview;
        invalid.steps[0].status = WorkflowStepStatus::NeedsReview;
        invalid.revision = 2;
        invalids.push(invalid);
        for invalid in invalids {
            assert!(invalid.validate().is_err(), "accepted {invalid:?}");
        }
        let mut active = initial;
        active.attach_job("job", 11).unwrap();
        let mut invalid = active.clone();
        invalid.steps[0].job_id = Some("different".into());
        assert!(invalid.validate().is_err());
        active.complete_task("done", 12).unwrap();
        let mut invalid = active.clone();
        invalid.steps[0].job_id = None;
        assert!(invalid.validate().is_err());
        let approval = request(&active, WorkflowResumeAction::Approve, None);
        active.resume(approval, 13).unwrap();
        let mut invalid = active.clone();
        invalid.decisions[0].request.revision = 1;
        assert!(invalid.validate().is_err());
        let mut invalid = active.clone();
        invalid.decisions[0].decided_at_ms = 14;
        assert!(invalid.validate().is_err());
        let mut invalid = active;
        invalid.decisions.clear();
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn workflows_rejects_oversized_results_and_clock_regression_without_mutation() {
        let mut run = run(&[WorkflowStepKind::Task]);
        assert!(run.attach_job("job", 9).is_err());
        run.attach_job("job", 11).unwrap();
        let before = run.clone();
        assert!(run
            .complete_task("a".repeat(MAX_WORKFLOW_RESULT_BYTES + 1), 12)
            .is_err());
        assert!(run.complete_task("done", 10).is_err());
        assert_eq!(run, before);
    }

    #[test]
    fn workflows_owner_decision_history_is_bounded() {
        let mut run = run(&[WorkflowStepKind::Task]);
        for n in 0..MAX_WORKFLOW_DECISIONS {
            run.needs_review("history capacity", 11 + n as u64).unwrap();
            let retry = request(&run, WorkflowResumeAction::Retry, None);
            run.resume(retry, 11 + n as u64).unwrap();
        }
        run.needs_review("history capacity", 80).unwrap();
        let before = run.clone();
        let request = request(&run, WorkflowResumeAction::Retry, None);
        assert!(run.resume(request, 81).is_err());
        assert_eq!(run, before);
        run.cancel(run.revision, 82).unwrap();
        run.validate().unwrap();
    }
}
