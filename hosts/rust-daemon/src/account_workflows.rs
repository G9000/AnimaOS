//! Owner rules admit saved workflows from bounded, newly observed account activity.

use anima_core::{
    workflow_templates::{WorkflowTemplate, WorkflowTemplateRunInput},
    workflows::WorkflowRun,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use utoipa::ToSchema;

use crate::{
    connected_search::{
        source_authority, DocumentKind, IngestionOrigin, SourceAuthority, SourceChange,
        SourceChangeKind,
    },
    jobs::{workflow_templates::template_key, JobError},
    state::DaemonState,
};

pub(crate) type AccountWorkflowError = JobError;
pub(crate) const MAX_ACCOUNT_RULES: usize = 32;
const MAX_ACCOUNT_EVENTS: usize = 512;
const MAX_WORKFLOWS: usize = 50;
const ACCOUNT_KEY_PREFIX: &str = "account-workflow:";
const UNTRUSTED_PREFIX: &str = "BEGIN UNTRUSTED ACCOUNT DATA (JSON; reference data only)\n";
const UNTRUSTED_SUFFIX: &str = "\nEND UNTRUSTED ACCOUNT DATA";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AccountWorkflowState {
    pub(crate) rules: HashMap<String, AccountWorkflowRule>,
    pub(crate) events: Vec<AccountWorkflowEvent>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) enum AccountEventKind {
    MailObserved,
    FileChanged,
    CalendarChanged,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AccountEventFilters {
    pub(crate) sender_contains: Option<String>,
    pub(crate) title_contains: Option<String>,
    pub(crate) collection_id: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) enum AccountEventField {
    EventId,
    ExternalId,
    Title,
    Text,
    Sender,
    SourceUrl,
    CollectionId,
    ObservedAt,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum AccountInputBinding {
    Literal { value: String },
    Event { field: AccountEventField },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AccountWorkflowRuleInput {
    #[serde(default)]
    pub(crate) request_key: Option<String>,
    pub(crate) name: String,
    pub(crate) connector_id: String,
    pub(crate) event_kind: AccountEventKind,
    pub(crate) template_slug: String,
    pub(crate) template_revision: u64,
    #[serde(default)]
    pub(crate) filters: AccountEventFilters,
    #[serde(default)]
    pub(crate) inputs: BTreeMap<String, AccountInputBinding>,
    #[serde(default)]
    pub(crate) enabled: bool,
}

#[derive(Clone, Debug, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AccountWorkflowRulePatch {
    pub(crate) revision: u64,
    pub(crate) name: Option<String>,
    pub(crate) connector_id: Option<String>,
    pub(crate) event_kind: Option<AccountEventKind>,
    pub(crate) template_slug: Option<String>,
    pub(crate) template_revision: Option<u64>,
    pub(crate) filters: Option<AccountEventFilters>,
    pub(crate) inputs: Option<BTreeMap<String, AccountInputBinding>>,
    pub(crate) enabled: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) enum AccountRuleOutcomeStatus {
    Started,
    Paused,
    Waiting,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AccountRuleOutcome {
    pub(crate) status: AccountRuleOutcomeStatus,
    pub(crate) message: String,
    pub(crate) at_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AccountWorkflowRule {
    pub(crate) id: String,
    pub(crate) agent_id: String,
    pub(crate) revision: u64,
    #[serde(default)]
    pub(crate) creation_request_key: Option<String>,
    #[serde(default)]
    pub(crate) creation_payload_sha256: Option<String>,
    pub(crate) name: String,
    pub(crate) connector_id: String,
    pub(crate) provider: String,
    pub(crate) connection_generation: String,
    pub(crate) event_kind: AccountEventKind,
    pub(crate) template_slug: String,
    pub(crate) template_revision: u64,
    pub(crate) filters: AccountEventFilters,
    pub(crate) inputs: BTreeMap<String, AccountInputBinding>,
    pub(crate) enabled: bool,
    pub(crate) deleted: bool,
    pub(crate) created_at_ms: u64,
    /// Definition/enablement time, unchanged by observations or outcomes.
    pub(crate) updated_at_ms: u64,
    pub(crate) baseline_at_ms: Option<u64>,
    pub(crate) last_outcome: Option<AccountRuleOutcome>,
}

impl AccountWorkflowRule {
    pub(crate) fn definition(&self) -> AccountWorkflowRuleInput {
        AccountWorkflowRuleInput {
            request_key: None,
            name: self.name.clone(),
            connector_id: self.connector_id.clone(),
            event_kind: self.event_kind,
            template_slug: self.template_slug.clone(),
            template_revision: self.template_revision,
            filters: self.filters.clone(),
            inputs: self.inputs.clone(),
            enabled: self.enabled,
        }
    }

    pub(crate) fn apply_patch(&self, patch: AccountWorkflowRulePatch) -> AccountWorkflowRuleInput {
        let mut input = self.definition();
        if let Some(value) = patch.name {
            input.name = value;
        }
        if let Some(value) = patch.connector_id {
            input.connector_id = value;
        }
        if let Some(value) = patch.event_kind {
            input.event_kind = value;
        }
        if let Some(value) = patch.template_slug {
            input.template_slug = value;
        }
        if let Some(value) = patch.template_revision {
            input.template_revision = value;
        }
        if let Some(value) = patch.filters {
            input.filters = value;
        }
        if let Some(value) = patch.inputs {
            input.inputs = value;
        }
        if let Some(value) = patch.enabled {
            input.enabled = value;
        }
        input
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AccountWorkflowEvent {
    pub(crate) id: String,
    pub(crate) rule_id: String,
    pub(crate) rule_revision: u64,
    pub(crate) agent_id: String,
    pub(crate) connector_id: String,
    pub(crate) connection_generation: String,
    pub(crate) event_id: String,
    pub(crate) event_kind: AccountEventKind,
    pub(crate) workflow_id: String,
    pub(crate) template_slug: String,
    pub(crate) template_revision: u64,
    pub(crate) payload_sha256: String,
    pub(crate) observed_at_ms: u64,
    pub(crate) created_at_ms: u64,
}

fn invalid(message: &str) -> JobError {
    JobError::Validation(message.into())
}
fn conflict(message: &str) -> JobError {
    JobError::Conflict(message.into())
}
fn bounded(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}
fn compatible(provider: &str, kind: AccountEventKind) -> bool {
    matches!(
        (provider, kind),
        (
            "gmail" | "outlook" | "email",
            AccountEventKind::MailObserved
        ) | ("gdrive", AccountEventKind::FileChanged)
            | ("gcalendar", AccountEventKind::CalendarChanged)
    )
}

fn validate_definition(input: &AccountWorkflowRuleInput) -> Result<(), JobError> {
    if input
        .request_key
        .as_ref()
        .is_some_and(|key| !bounded(key, 128))
    {
        return Err(invalid(
            "Rule request keys require 1-128 bytes without control characters",
        ));
    }
    if !bounded(&input.name, 160) || !bounded(&input.connector_id, 256) {
        return Err(invalid(
            "A rule requires a name up to 160 bytes and a connection ID up to 256 bytes",
        ));
    }
    anima_core::workflow_templates::validate_slug(&input.template_slug)
        .map_err(JobError::Validation)?;
    if input.template_revision == 0 || input.inputs.len() > 16 {
        return Err(invalid(
            "A rule requires a saved template revision and at most 16 parameter bindings",
        ));
    }
    if [
        &input.filters.sender_contains,
        &input.filters.title_contains,
        &input.filters.collection_id,
    ]
    .iter()
    .filter_map(|value| value.as_deref())
    .any(|value| !bounded(value, 256))
    {
        return Err(invalid(
            "Rule filters must contain 1-256 bytes of literal text",
        ));
    }
    let literal_bytes = input
        .inputs
        .values()
        .map(|binding| match binding {
            AccountInputBinding::Literal { value } => value.len(),
            AccountInputBinding::Event { .. } => 0,
        })
        .sum::<usize>();
    let event_bindings = input
        .inputs
        .values()
        .filter(|binding| matches!(binding, AccountInputBinding::Event { .. }))
        .count();
    if input.inputs.iter().any(|(name, binding)| {
        !bounded(name, 64)
            || match binding {
                AccountInputBinding::Literal { value } => {
                    value.len() > 8192 || value.contains('\0')
                }
                AccountInputBinding::Event { .. } => false,
            }
    }) || literal_bytes.saturating_add(event_bindings * 512) > 16 * 1024
    {
        return Err(invalid(
            "Rule parameter bindings exceed their name or value limits",
        ));
    }
    Ok(())
}

pub(crate) fn build_rule(
    state: &DaemonState,
    source: &SourceAuthority,
    input: AccountWorkflowRuleInput,
    id: String,
    now: u64,
) -> Result<AccountWorkflowRule, JobError> {
    validate_definition(&input)?;
    if source.connector_id != input.connector_id
        || !compatible(&source.provider, input.event_kind)
        || !source.enabled
        || !source.read_granted
    {
        return Err(conflict("Enable a current connection with its read permission before creating a matching account rule"));
    }
    let template = current_template(
        state,
        &source.agent_id,
        &input.template_slug,
        input.template_revision,
    )
    .ok_or_else(|| conflict("Select the current saved workflow template revision"))?;
    let values = input
        .inputs
        .iter()
        .map(|(name, binding)| {
            (
                name.clone(),
                match binding {
                    AccountInputBinding::Literal { value } => value.clone(),
                    AccountInputBinding::Event { .. } => {
                        account_data("example account event", "example-citation", false, 512)
                    }
                },
            )
        })
        .collect();
    template
        .materialize(
            "account-rule-validation",
            &WorkflowTemplateRunInput {
                request_key: "account-rule-validation".into(),
                revision: input.template_revision,
                inputs: values,
            },
            now.max(template.updated_at_ms),
        )
        .map_err(JobError::Validation)?;
    let creation_payload_sha256 = input.request_key.as_ref().map(|_| creation_digest(&input));
    Ok(AccountWorkflowRule {
        id,
        agent_id: source.agent_id.clone(),
        revision: 1,
        name: input.name,
        creation_request_key: input.request_key,
        creation_payload_sha256,
        connector_id: input.connector_id,
        provider: source.provider.clone(),
        connection_generation: source.generation.clone(),
        event_kind: input.event_kind,
        template_slug: input.template_slug,
        template_revision: input.template_revision,
        filters: input.filters,
        inputs: input.inputs,
        enabled: input.enabled,
        deleted: false,
        created_at_ms: now,
        updated_at_ms: now,
        baseline_at_ms: None,
        last_outcome: Some(AccountRuleOutcome {
            status: AccountRuleOutcomeStatus::Waiting,
            message:
                "Waiting for a fresh baseline; existing account items will not start workflows"
                    .into(),
            at_ms: now,
        }),
    })
}

pub(crate) fn creation_digest(input: &AccountWorkflowRuleInput) -> String {
    hex::encode(Sha256::digest(
        serde_json::to_vec(input).unwrap_or_default(),
    ))
}

fn current_template<'a>(
    state: &'a DaemonState,
    agent: &str,
    slug: &str,
    revision: u64,
) -> Option<&'a WorkflowTemplate> {
    state
        .workflow_templates
        .get(&template_key(agent, slug))
        .filter(|template| !template.deleted && template.revision == revision)
}

fn account_data(text: &str, citation: &str, source_truncated: bool, budget: usize) -> String {
    let mut end = text.len().min(budget);
    loop {
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        let payload = serde_json::json!({"value":&text[..end],"citationId":citation,
            "truncated":end<text.len(),"sourceTruncated":source_truncated});
        let value = format!("{UNTRUSTED_PREFIX}{payload}{UNTRUSTED_SUFFIX}");
        if value.len() <= budget || end == 0 {
            return value;
        }
        // Halving bounds work even for text whose JSON escaping expands sixfold.
        end /= 2;
    }
}

fn values(rule: &AccountWorkflowRule, event: &SourceChange) -> BTreeMap<String, String> {
    let literal_bytes = rule
        .inputs
        .values()
        .map(|binding| match binding {
            AccountInputBinding::Literal { value } => value.len(),
            AccountInputBinding::Event { .. } => 0,
        })
        .sum::<usize>();
    let derived_count = rule
        .inputs
        .values()
        .filter(|binding| matches!(binding, AccountInputBinding::Event { .. }))
        .count();
    let budget = ((16 * 1024 - literal_bytes) / derived_count.max(1)).min(8192);
    rule.inputs
        .iter()
        .map(|(name, binding)| {
            let value = match binding {
                AccountInputBinding::Literal { value } => value.clone(),
                AccountInputBinding::Event { field } => {
                    let text = match field {
                        AccountEventField::EventId => event.id.clone(),
                        AccountEventField::ExternalId => event.document.external_id.clone(),
                        AccountEventField::Title => event.document.title.clone(),
                        AccountEventField::Text => event.document.text.clone(),
                        AccountEventField::Sender => {
                            event.document.sender.clone().unwrap_or_default()
                        }
                        AccountEventField::SourceUrl => {
                            event.document.source_url.clone().unwrap_or_default()
                        }
                        AccountEventField::CollectionId => event.document.collection_id.clone(),
                        AccountEventField::ObservedAt => event.observed_at_ms.to_string(),
                    };
                    account_data(
                        &text,
                        &event.document.citation_id,
                        event.document.truncated,
                        budget,
                    )
                }
            };
            (name.clone(), value)
        })
        .collect()
}

fn matches(rule: &AccountWorkflowRule, event: &SourceChange) -> bool {
    let kind_matches = match rule.event_kind {
        AccountEventKind::MailObserved => {
            event.document.kind == DocumentKind::Mail && event.kind == SourceChangeKind::Observed
        }
        AccountEventKind::FileChanged => {
            event.document.kind == DocumentKind::File && event.kind == SourceChangeKind::Changed
        }
        AccountEventKind::CalendarChanged => event.document.kind == DocumentKind::Event,
    };
    kind_matches
        && rule.filters.sender_contains.as_ref().is_none_or(|needle| {
            event
                .document
                .sender
                .as_deref()
                .unwrap_or_default()
                .to_lowercase()
                .contains(&needle.to_lowercase())
        })
        && rule.filters.title_contains.as_ref().is_none_or(|needle| {
            event
                .document
                .title
                .to_lowercase()
                .contains(&needle.to_lowercase())
        })
        && rule
            .filters
            .collection_id
            .as_ref()
            .is_none_or(|collection| collection == &event.document.collection_id)
}

fn event_key(rule: &str, revision: u64, generation: &str, event: &str) -> String {
    let bytes = serde_json::to_vec(&(rule, revision, generation, event)).unwrap_or_default();
    format!("{ACCOUNT_KEY_PREFIX}{}", hex::encode(Sha256::digest(bytes)))
}

fn fingerprint(event: &SourceChange) -> String {
    // Observation time and local indexing freshness can change on a harmless retry.
    let value = serde_json::json!({"connectorId":event.connector_id,"generation":event.generation,
        "provider":event.provider,"kind":event.kind,"externalId":event.document.external_id,
        "collectionId":event.document.collection_id,"kindOfDocument":event.document.kind,
        "title":event.document.title,"text":event.document.text,"sender":event.document.sender,
        "url":event.document.source_url,"modifiedAt":event.document.source_modified_at,
        "start":event.document.event_start,"end":event.document.event_end,
        "revision":event.document.revision,"truncated":event.document.truncated});
    hex::encode(Sha256::digest(value.to_string().as_bytes()))
}

fn pause(rule: &mut AccountWorkflowRule, reason: &str, now: u64) {
    rule.enabled = false;
    rule.baseline_at_ms = None;
    rule.revision = rule.revision.saturating_add(1);
    rule.updated_at_ms = now.max(rule.updated_at_ms.saturating_add(1));
    rule.last_outcome = Some(AccountRuleOutcome {
        status: AccountRuleOutcomeStatus::Paused,
        message: reason.into(),
        at_ms: rule.updated_at_ms,
    });
}

/// Called under the same transaction as source invalidation or explicit disablement.
pub(crate) fn pause_for_source(
    state: &mut DaemonState,
    agent: &str,
    connector: &str,
    reason: &str,
) {
    let now = anima_core::primitives::now_millis();
    for rule in state.account_workflows.rules.values_mut().filter(|rule| {
        rule.agent_id == agent && rule.connector_id == connector && rule.enabled && !rule.deleted
    }) {
        pause(rule, reason, now);
    }
}

/// Called before control-plane publication so edits cannot silently change a rule's authority.
pub(crate) fn reconcile_authority(state: &mut DaemonState) {
    let now = anima_core::primitives::now_millis();
    let invalid = state
        .account_workflows
        .rules
        .values()
        .filter(|rule| rule.enabled && !rule.deleted)
        .filter_map(|rule| {
            let reason = if current_template(
                state,
                &rule.agent_id,
                &rule.template_slug,
                rule.template_revision,
            )
            .is_none()
            {
                "Saved workflow changed; select its current revision and enable the rule again"
            } else if !source_authority(state, &rule.agent_id, &rule.connector_id).is_some_and(
                |source| {
                    source.enabled
                        && source.read_granted
                        && source.provider == rule.provider
                        && source.generation == rule.connection_generation
                },
            ) {
                "Connection permission changed; review and enable the account rule again"
            } else {
                return None;
            };
            Some((rule.id.clone(), reason))
        })
        .collect::<Vec<_>>();
    for (id, reason) in invalid {
        if let Some(rule) = state.account_workflows.rules.get_mut(&id) {
            pause(rule, reason, now);
        }
    }
}

pub(crate) fn reserved_request_key(key: &str) -> bool {
    key.starts_with(ACCOUNT_KEY_PREFIX)
}

/// The source caller owns persistence and rolls the source and this admission back together.
pub(crate) fn admit_changes(
    state: &mut DaemonState,
    source: &SourceAuthority,
    changes: &[SourceChange],
    baseline: bool,
) -> Result<(), AccountWorkflowError> {
    let current = source_authority(state, &source.agent_id, &source.connector_id)
        .filter(|current| {
            current.generation == source.generation
                && current.provider == source.provider
                && current.enabled
                && current.read_granted
        })
        .ok_or_else(|| conflict("Account source authority changed before workflow admission"))?;
    if !source.enabled || !source.read_granted || current.agent_id != source.agent_id {
        return Err(conflict("Account source permission is unavailable"));
    }
    admit_current_changes(state, source, changes, baseline)
}

fn admit_current_changes(
    state: &mut DaemonState,
    source: &SourceAuthority,
    changes: &[SourceChange],
    baseline: bool,
) -> Result<(), JobError> {
    if changes.len() > 500
        || changes.iter().any(|event| {
            event.connector_id != source.connector_id
                || event.generation != source.generation
                || event.provider != source.provider
                || event.origin != IngestionOrigin::LivePoll
                || !bounded(&event.id, 256)
                || event.observed_at_ms == 0
        })
    {
        return Err(conflict("Account source event scope or batch is invalid"));
    }
    let mut next = state.account_workflows.clone();
    let mut workflows = Vec::new();
    let now = anima_core::primitives::now_millis();
    let mut ids: Vec<_> = next.rules.keys().cloned().collect();
    ids.sort();
    for id in ids {
        let mut rule = next.rules[&id].clone();
        if rule.agent_id != source.agent_id
            || rule.connector_id != source.connector_id
            || rule.deleted
            || !rule.enabled
        {
            continue;
        }
        if rule.connection_generation != source.generation || rule.provider != source.provider {
            pause(
                &mut rule,
                "Connection authorization changed; review and enable this rule again",
                now,
            );
            next.rules.insert(id, rule);
            continue;
        }
        let Some(template) = current_template(
            state,
            &rule.agent_id,
            &rule.template_slug,
            rule.template_revision,
        ) else {
            pause(
                &mut rule,
                "Saved workflow changed; select its current revision and enable the rule again",
                now,
            );
            next.rules.insert(id, rule);
            continue;
        };
        if source.sync_started_at_ms <= rule.updated_at_ms {
            continue;
        }
        if baseline || rule.baseline_at_ms.is_none() {
            rule.baseline_at_ms = Some(now.max(source.sync_started_at_ms));
            rule.last_outcome = Some(AccountRuleOutcome {
                status: AccountRuleOutcomeStatus::Waiting,
                message: "Baseline saved; waiting for newly observed matching account activity"
                    .into(),
                at_ms: now,
            });
            next.rules.insert(id, rule);
            continue;
        }
        for event in changes {
            if !matches(&rule, event) {
                continue;
            }
            let key = event_key(&rule.id, rule.revision, &source.generation, &event.id);
            let payload_sha256 = fingerprint(event);
            if let Some(original) = next.events.iter().find(|receipt| receipt.id == key) {
                if original.payload_sha256 != payload_sha256 {
                    return Err(conflict(
                        "This account event identity already names different source content",
                    ));
                }
                continue;
            }
            if next.events.len() >= MAX_ACCOUNT_EVENTS
                || state.workflows.len() + workflows.len() >= MAX_WORKFLOWS
            {
                return Err(conflict("Account workflow history capacity is exhausted; no source baseline or workflow was advanced"));
            }
            let request = WorkflowTemplateRunInput {
                request_key: key.clone(),
                revision: rule.template_revision,
                inputs: values(&rule, event),
            };
            let admitted_at = now
                .max(template.updated_at_ms)
                .max(rule.updated_at_ms)
                .max(source.sync_started_at_ms);
            let workflow=template.materialize(&uuid::Uuid::new_v4().to_string(),&request,admitted_at)
                .map_err(|_| conflict("Account event values cannot be materialized in the saved workflow; revise the rule bindings"))?;
            if state
                .workflows
                .values()
                .any(|run| run.agent_id == rule.agent_id && run.request_key == key)
            {
                return Err(conflict(
                    "Account workflow receipt is missing for an existing admitted request",
                ));
            }
            next.events.push(AccountWorkflowEvent {
                id: key,
                rule_id: rule.id.clone(),
                rule_revision: rule.revision,
                agent_id: rule.agent_id.clone(),
                connector_id: source.connector_id.clone(),
                connection_generation: source.generation.clone(),
                event_id: event.id.clone(),
                event_kind: rule.event_kind,
                workflow_id: workflow.id.clone(),
                template_slug: rule.template_slug.clone(),
                template_revision: rule.template_revision,
                payload_sha256,
                observed_at_ms: event.observed_at_ms,
                created_at_ms: admitted_at,
            });
            rule.last_outcome = Some(AccountRuleOutcome {
                status: AccountRuleOutcomeStatus::Started,
                message:
                    "Saved workflow started; its owner checkpoints and tool permissions still apply"
                        .into(),
                at_ms: admitted_at,
            });
            workflows.push(workflow);
        }
        next.rules.insert(id, rule);
    }
    state.account_workflows = next;
    for workflow in workflows {
        state.workflows.insert(workflow.id.clone(), workflow);
    }
    Ok(())
}

/// Account-triggered work retains its exact rule, template, and current source read authority.
pub(crate) fn dispatch_allowed(state: &DaemonState, workflow: &WorkflowRun) -> bool {
    let Some(receipt) = state
        .account_workflows
        .events
        .iter()
        .find(|event| event.workflow_id == workflow.id)
    else {
        return !workflow.request_key.starts_with(ACCOUNT_KEY_PREFIX);
    };
    let Some(rule) = state.account_workflows.rules.get(&receipt.rule_id) else {
        return false;
    };
    rule.enabled
        && !rule.deleted
        && rule.revision == receipt.rule_revision
        && rule.agent_id == workflow.agent_id
        && rule.connection_generation == receipt.connection_generation
        && rule.template_slug == receipt.template_slug
        && rule.template_revision == receipt.template_revision
        && current_template(
            state,
            &rule.agent_id,
            &rule.template_slug,
            rule.template_revision,
        )
        .is_some()
        && source_authority(state, &rule.agent_id, &rule.connector_id).is_some_and(|source| {
            source.enabled
                && source.read_granted
                && source.generation == receipt.connection_generation
        })
}

pub(crate) fn validate_snapshot(
    account: &AccountWorkflowState,
    workflows: &[WorkflowRun],
) -> Result<(), String> {
    if account.rules.len() > MAX_ACCOUNT_RULES || account.events.len() > MAX_ACCOUNT_EVENTS {
        return Err("Account workflow state exceeds its capacity".into());
    }
    for (id, rule) in &account.rules {
        validate_definition(&rule.definition())
            .map_err(|_| "Invalid saved account workflow rule")?;
        if id != &rule.id
            || !bounded(id, 128)
            || !bounded(&rule.agent_id, 128)
            || !bounded(&rule.connection_generation, 256)
            || rule.revision == 0
            || !compatible(&rule.provider, rule.event_kind)
            || rule.created_at_ms == 0
            || rule.updated_at_ms < rule.created_at_ms
            || (rule.deleted && rule.enabled)
            || rule
                .baseline_at_ms
                .is_some_and(|at| at < rule.updated_at_ms)
            || rule.last_outcome.as_ref().is_some_and(|outcome| {
                !bounded(&outcome.message, 512) || outcome.at_ms < rule.created_at_ms
            })
        {
            return Err("Invalid saved account workflow identity or lifecycle".into());
        }
        match (&rule.creation_request_key, &rule.creation_payload_sha256) {
            (None, None) => {}
            (Some(key), Some(digest))
                if bounded(key, 128)
                    && digest.len() == 64
                    && digest.bytes().all(|b| b.is_ascii_hexdigit()) => {}
            _ => return Err("Invalid account rule creation receipt".into()),
        }
    }
    let mut request_keys = HashSet::new();
    for rule in account.rules.values() {
        if let Some(key) = &rule.creation_request_key {
            if !request_keys.insert((&rule.agent_id, key)) {
                return Err("Duplicate account rule creation request key".into());
            }
        }
    }
    let (mut ids, mut linked) = (HashSet::new(), HashSet::new());
    for event in &account.events {
        let rule = account
            .rules
            .get(&event.rule_id)
            .ok_or("Account workflow receipt rule is missing")?;
        let workflow = workflows
            .iter()
            .find(|run| run.id == event.workflow_id)
            .ok_or("Account workflow receipt run is missing")?;
        if !ids.insert(&event.id)
            || !linked.insert(&event.workflow_id)
            || event.id
                != event_key(
                    &event.rule_id,
                    event.rule_revision,
                    &event.connection_generation,
                    &event.event_id,
                )
            || event.rule_revision == 0
            || event.rule_revision > rule.revision
            || event.agent_id != rule.agent_id
            || workflow.agent_id != event.agent_id
            || !bounded(&event.connector_id, 256)
            || !bounded(&event.connection_generation, 256)
            || !bounded(&event.event_id, 256)
            || event.payload_sha256.len() != 64
            || !event.payload_sha256.bytes().all(|b| b.is_ascii_hexdigit())
            || event.observed_at_ms == 0
            || event.created_at_ms < rule.created_at_ms
            || workflow.request_key != event.id
            || workflow.created_at_ms != event.created_at_ms
            || !workflow.source_template.as_ref().is_some_and(|source| {
                source.slug == event.template_slug && source.revision == event.template_revision
            })
        {
            return Err("Invalid saved account workflow receipt binding".into());
        }
    }
    if workflows
        .iter()
        .any(|run| run.request_key.starts_with(ACCOUNT_KEY_PREFIX) && !linked.contains(&run.id))
    {
        return Err("Account workflow run has no durable admission receipt".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
