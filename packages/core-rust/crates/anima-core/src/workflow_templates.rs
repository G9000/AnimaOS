//! Named reusable workflow data. Interpolation never evaluates code or consults the environment.
use crate::workflows::{WorkflowInput, WorkflowRun, WorkflowStepSpec};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

pub const MAX_WORKFLOW_TEMPLATE_PARAMETERS: usize = 16;
pub const MAX_WORKFLOW_TEMPLATE_REVISIONS: usize = 16;
pub const MAX_WORKFLOW_TEMPLATE_INPUT_BYTES: usize = 8192;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowTemplateParameter {
    pub name: String,
    pub default: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowTemplateInput {
    pub slug: String,
    /// Zero creates a template. Updates require the current revision.
    pub revision: u64,
    pub title: String,
    #[serde(default)]
    pub parameters: Vec<WorkflowTemplateParameter>,
    pub steps: Vec<WorkflowStepSpec>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowTemplateRunInput {
    pub request_key: String,
    pub revision: u64,
    #[serde(default)]
    pub inputs: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowTemplateSource {
    pub slug: String,
    pub revision: u64,
    pub inputs: BTreeMap<String, String>,
    /// Retain the exact submitted values as well as resolved defaults for retry identity.
    pub request: WorkflowTemplateRunInput,
}

impl WorkflowTemplateSource {
    pub fn validate(&self, request_key: &str) -> Result<(), String> {
        validate_slug(&self.slug)?;
        validate_run_input(&self.request)?;
        if self.revision == 0
            || self.revision != self.request.revision
            || self.request.request_key != request_key
        {
            return Err("Invalid pinned workflow template revision or request key".into());
        }
        validate_values(&self.inputs)?;
        if self
            .request
            .inputs
            .iter()
            .any(|(name, value)| self.inputs.get(name) != Some(value))
        {
            return Err("Pinned template inputs differ from submitted values".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowTemplateSaveReceipt {
    pub request: WorkflowTemplateInput,
    pub saved_at_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowTemplate {
    pub agent_id: String,
    pub slug: String,
    pub revision: u64,
    pub title: String,
    pub parameters: Vec<WorkflowTemplateParameter>,
    pub steps: Vec<WorkflowStepSpec>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub save_history: Vec<WorkflowTemplateSaveReceipt>,
    #[serde(default)]
    pub deleted: bool,
}

impl WorkflowTemplate {
    pub fn new(agent_id: &str, input: WorkflowTemplateInput, now: u64) -> Result<Self, String> {
        if input.revision != 0 {
            return Err("A new workflow template requires revision zero".into());
        }
        validate_input(&input)?;
        let value = Self {
            agent_id: agent_id.into(),
            slug: input.slug.clone(),
            revision: 1,
            title: input.title.clone(),
            parameters: input.parameters.clone(),
            steps: input.steps.clone(),
            created_at_ms: now,
            updated_at_ms: now,
            save_history: vec![WorkflowTemplateSaveReceipt {
                request: input,
                saved_at_ms: now,
            }],
            deleted: false,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn save(&mut self, input: WorkflowTemplateInput, now: u64) -> Result<bool, String> {
        self.validate()?;
        validate_input(&input)?;
        if self
            .save_history
            .iter()
            .any(|receipt| receipt.request == input)
        {
            return Ok(false);
        }
        if self.deleted || input.slug != self.slug || input.revision != self.revision {
            return Err("Workflow template revision conflict; refresh before saving".into());
        }
        if self.save_history.len() >= MAX_WORKFLOW_TEMPLATE_REVISIONS {
            return Err(
                "Workflow template revision history capacity is exhausted; use another slug".into(),
            );
        }
        if now < self.updated_at_ms {
            return Err("Workflow template clock moved backwards".into());
        }
        self.title = input.title.clone();
        self.parameters = input.parameters.clone();
        self.steps = input.steps.clone();
        self.save_history.push(WorkflowTemplateSaveReceipt {
            request: input,
            saved_at_ms: now,
        });
        self.revision += 1;
        self.updated_at_ms = now;
        Ok(true)
    }

    pub fn delete(&mut self, revision: u64, now: u64) -> Result<bool, String> {
        self.validate()?;
        if self.deleted && revision.checked_add(1) == Some(self.revision) {
            return Ok(false);
        }
        if self.deleted || revision != self.revision {
            return Err("Workflow template revision conflict".into());
        }
        self.deleted = true;
        self.revision += 1;
        self.updated_at_ms = now.max(self.updated_at_ms);
        Ok(true)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.agent_id.trim().is_empty()
            || self.agent_id.len() > 128
            || self.agent_id.chars().any(char::is_control)
        {
            return Err("Invalid workflow template agent ID".into());
        }
        validate_slug(&self.slug)?;
        if self.save_history.is_empty()
            || self.save_history.len() > MAX_WORKFLOW_TEMPLATE_REVISIONS
            || self.revision != self.save_history.len() as u64 + u64::from(self.deleted)
            || self.updated_at_ms < self.created_at_ms
        {
            return Err("Invalid workflow template revision history or timestamps".into());
        }
        let mut previous = self.created_at_ms;
        for (index, receipt) in self.save_history.iter().enumerate() {
            validate_input(&receipt.request)?;
            if receipt.request.slug != self.slug
                || receipt.request.revision != index as u64
                || receipt.saved_at_ms < previous
                || receipt.saved_at_ms > self.updated_at_ms
                || (index == 0 && receipt.saved_at_ms != self.created_at_ms)
            {
                return Err("Invalid workflow template save receipt".into());
            }
            previous = receipt.saved_at_ms;
        }
        let latest = &self.save_history.last().unwrap().request;
        if latest.title != self.title
            || latest.parameters != self.parameters
            || latest.steps != self.steps
            || (!self.deleted && previous != self.updated_at_ms)
        {
            return Err("Workflow template differs from its latest saved definition".into());
        }
        Ok(())
    }

    pub fn materialize(
        &self,
        id: &str,
        request: &WorkflowTemplateRunInput,
        now: u64,
    ) -> Result<WorkflowRun, String> {
        self.validate()?;
        validate_run_input(request)?;
        if self.deleted || request.revision != self.revision {
            return Err("Workflow template revision conflict; refresh before running".into());
        }
        let names: HashSet<_> = self.parameters.iter().map(|p| p.name.as_str()).collect();
        if let Some(name) = request
            .inputs
            .keys()
            .find(|name| !names.contains(name.as_str()))
        {
            return Err(format!("Unknown workflow template parameter: {name}"));
        }
        let mut inputs = BTreeMap::new();
        for parameter in &self.parameters {
            let value = request
                .inputs
                .get(&parameter.name)
                .or(parameter.default.as_ref())
                .ok_or_else(|| {
                    format!("Missing workflow template parameter: {}", parameter.name)
                })?;
            inputs.insert(parameter.name.clone(), value.clone());
        }
        validate_values(&inputs)?;
        let mut run = WorkflowRun::new(
            id,
            &self.agent_id,
            WorkflowInput {
                title: interpolate(&self.title, &inputs)?,
                request_key: request.request_key.clone(),
                steps: self
                    .steps
                    .iter()
                    .map(|step| {
                        Ok(WorkflowStepSpec {
                            id: step.id.clone(),
                            kind: step.kind,
                            title: interpolate(&step.title, &inputs)?,
                            prompt: interpolate(&step.prompt, &inputs)?,
                        })
                    })
                    .collect::<Result<_, String>>()?,
            },
            now,
        )?;
        run.source_template = Some(WorkflowTemplateSource {
            slug: self.slug.clone(),
            revision: self.revision,
            inputs,
            request: request.clone(),
        });
        run.validate()?;
        Ok(run)
    }
}

pub fn validate_slug(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
    {
        return Err(
            "Template slug must use 1-64 lowercase letters, numbers, hyphens or underscores".into(),
        );
    }
    Ok(())
}

fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.bytes().enumerate().all(|(index, b)| {
            b.is_ascii_alphabetic() || b == b'_' || (index > 0 && b.is_ascii_digit())
        })
}

pub fn validate_run_input(input: &WorkflowTemplateRunInput) -> Result<(), String> {
    if input.revision == 0
        || input.request_key.trim().is_empty()
        || input.request_key.len() > 128
        || input.request_key.chars().any(char::is_control)
    {
        return Err(
            "Run requires a positive template revision and request key up to 128 bytes".into(),
        );
    }
    validate_values(&input.inputs)
}

fn validate_values(inputs: &BTreeMap<String, String>) -> Result<(), String> {
    if inputs.len() > MAX_WORKFLOW_TEMPLATE_PARAMETERS
        || inputs.iter().any(|(key, value)| {
            !valid_name(key)
                || value.len() > MAX_WORKFLOW_TEMPLATE_INPUT_BYTES
                || value.contains('\0')
        })
        || inputs.values().map(String::len).sum::<usize>() > 16 * 1024
    {
        return Err("Workflow template inputs exceed name, value or total bounds".into());
    }
    Ok(())
}

fn validate_input(input: &WorkflowTemplateInput) -> Result<(), String> {
    validate_slug(&input.slug)?;
    if input.parameters.len() > MAX_WORKFLOW_TEMPLATE_PARAMETERS {
        return Err("At most 16 template parameters are supported".into());
    }
    let mut defaults = BTreeMap::new();
    for parameter in &input.parameters {
        if !valid_name(&parameter.name) || defaults.contains_key(&parameter.name) {
            return Err("Invalid or duplicate workflow template parameter".into());
        }
        defaults.insert(
            parameter.name.clone(),
            parameter.default.clone().unwrap_or_else(|| "value".into()),
        );
    }
    validate_values(&defaults)?;
    // Validate both stored and expanded definitions. Step IDs are never interpolated.
    for text in
        std::iter::once(&input.title).chain(input.steps.iter().flat_map(|s| [&s.title, &s.prompt]))
    {
        interpolate(text, &defaults)?;
    }
    WorkflowRun::new(
        "template-validation",
        "agent",
        WorkflowInput {
            title: input.title.clone(),
            request_key: "template-validation".into(),
            steps: input.steps.clone(),
        },
        0,
    )?;
    Ok(())
}

fn interpolate(text: &str, inputs: &BTreeMap<String, String>) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = text;
    while !rest.is_empty() {
        if let Some(tail) = rest.strip_prefix("$${") {
            // Escape a placeholder as literal text without interpreting its contents.
            let end = tail
                .find('}')
                .ok_or("Unclosed escaped template placeholder")?;
            out.push_str("${");
            out.push_str(&tail[..=end]);
            rest = &tail[end + 1..];
        } else if let Some(tail) = rest.strip_prefix("${") {
            let end = tail
                .find('}')
                .ok_or("Unclosed workflow template placeholder")?;
            let name = &tail[..end];
            if !valid_name(name) {
                return Err(
                    "Workflow template placeholders require a simple parameter name".into(),
                );
            }
            out.push_str(
                inputs
                    .get(name)
                    .ok_or_else(|| format!("Unknown workflow template parameter: {name}"))?,
            );
            rest = &tail[end + 1..];
        } else {
            let c = rest.chars().next().unwrap();
            out.push(c);
            rest = &rest[c.len_utf8()..];
        }
        if out.len() > 32 * 1024 {
            return Err("Expanded template text exceeds 32 KiB".into());
        }
    }
    Ok(out)
}
