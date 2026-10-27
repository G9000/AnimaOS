//! Durable, conversation-owned display data. Results confer no execution authority.
use serde::{Deserialize, Serialize};
use serde_json::Number;
use sha2::{Digest, Sha256};
use std::collections::HashSet;

mod interactive;
pub(crate) use interactive::{DashboardWidget, TrackerColumn, TrackerRow};

pub(crate) const MAX_RESULT_BYTES: usize = 32 * 1024;
const MAX_RESULTS: usize = 32;
const MAX_SESSION_BYTES: usize = 256 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ChatResult {
    pub(crate) id: String,
    pub(crate) revision: u64,
    pub(crate) agent_id: String,
    pub(crate) session_id: String,
    pub(crate) clear_generation: u64,
    pub(crate) title: String,
    pub(crate) payload: ResultPayload,
    pub(crate) pinned: bool,
    pub(crate) created_at_ms: u64,
    pub(crate) updated_at_ms: u64,
    pub(crate) source_run_id: String,
    pub(crate) source_tool_call_id: String,
    pub(crate) request_key: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SaveResultInput {
    #[serde(default)]
    pub(crate) id: Option<String>,
    pub(crate) expected_revision: u64,
    pub(crate) request_key: String,
    pub(crate) title: String,
    pub(crate) payload: ResultPayload,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum ResultPayload {
    Chart {
        #[serde(rename = "chartType")]
        chart_type: ChartType,
        labels: Vec<String>,
        series: Vec<ChartSeries>,
    },
    Comparison {
        columns: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    Document {
        format: DocumentFormat,
        text: String,
    },
    Calculator {
        inputs: Vec<CalculatorInput>,
        expression: Calculation,
        #[serde(rename = "outputLabel")]
        output_label: String,
    },
    Tracker {
        columns: Vec<TrackerColumn>,
        rows: Vec<TrackerRow>,
    },
    Dashboard {
        widgets: Vec<DashboardWidget>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ChartType {
    Bar,
    Line,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DocumentFormat {
    Text,
    Markdown,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChartSeries {
    label: String,
    values: Vec<Number>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CalculatorInput {
    key: String,
    label: String,
    value: Number,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    min: Option<Number>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    max: Option<Number>,
}

/// A closed arithmetic tree, interpreted by native UI code. No source strings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Calculation {
    Input {
        key: String,
    },
    Constant {
        value: Number,
    },
    Add {
        left: Box<Calculation>,
        right: Box<Calculation>,
    },
    Subtract {
        left: Box<Calculation>,
        right: Box<Calculation>,
    },
    Multiply {
        left: Box<Calculation>,
        right: Box<Calculation>,
    },
    Divide {
        left: Box<Calculation>,
        right: Box<Calculation>,
    },
}

fn text(value: &str, max: usize, empty: bool) -> Result<(), String> {
    if value.len() > max || (!empty && value.trim().is_empty()) || value.contains('\0') {
        Err("Result text is empty or exceeds its limit".into())
    } else {
        Ok(())
    }
}
fn number(value: &Number) -> Result<f64, String> {
    value
        .as_f64()
        .filter(|n| n.is_finite() && n.abs() <= 9_007_199_254_740_991.0)
        .ok_or_else(|| "Result numbers must be finite safe numbers".into())
}
impl ResultPayload {
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Self::Chart { .. } => "chart",
            Self::Comparison { .. } => "comparison",
            Self::Document { .. } => "document",
            Self::Calculator { .. } => "calculator",
            Self::Tracker { .. } => "tracker",
            Self::Dashboard { .. } => "dashboard",
        }
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        if serde_json::to_vec(self)
            .map_err(|_| "Invalid result")?
            .len()
            > MAX_RESULT_BYTES
        {
            return Err("Result exceeds the 32 KiB limit".into());
        }
        match self {
            Self::Document { text: body, .. } => text(body, MAX_RESULT_BYTES, true),
            Self::Comparison { columns, rows } => {
                if columns.is_empty() || columns.len() > 12 || rows.len() > 100 {
                    return Err("Comparisons allow 1–12 columns and at most 100 rows".into());
                }
                for column in columns {
                    text(column, 160, false)?;
                }
                for row in rows {
                    if row.len() != columns.len() {
                        return Err("Comparison rows must match columns".into());
                    }
                    for cell in row {
                        text(cell, 2048, true)?;
                    }
                }
                Ok(())
            }
            Self::Chart { labels, series, .. } => {
                if labels.is_empty() || labels.len() > 100 || series.is_empty() || series.len() > 8
                {
                    return Err("Charts allow 1–100 labels and 1–8 series".into());
                }
                for label in labels {
                    text(label, 160, false)?;
                }
                for series in series {
                    text(&series.label, 160, false)?;
                    if series.values.len() != labels.len() {
                        return Err("Chart values must match labels".into());
                    }
                    for value in &series.values {
                        number(value)?;
                    }
                }
                Ok(())
            }
            Self::Calculator {
                inputs,
                expression,
                output_label,
            } => {
                text(output_label, 160, false)?;
                if inputs.is_empty() || inputs.len() > 12 {
                    return Err("Calculators allow 1–12 inputs".into());
                }
                let mut keys = HashSet::new();
                for input in inputs {
                    if input.key.is_empty()
                        || input.key.len() > 40
                        || !input
                            .key
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
                        || !keys.insert(input.key.as_str())
                    {
                        return Err(
                            "Calculator input keys must be unique simple identifiers".into()
                        );
                    }
                    text(&input.label, 160, false)?;
                    let value = number(&input.value)?;
                    let min = input.min.as_ref().map(number).transpose()?;
                    let max = input.max.as_ref().map(number).transpose()?;
                    if min.is_some_and(|min| value < min)
                        || max.is_some_and(|max| value > max)
                        || min.zip(max).is_some_and(|(min, max)| min > max)
                    {
                        return Err("Calculator defaults must be within their input bounds".into());
                    }
                }
                expression.validate(&keys, 0, &mut 0)
            }
            Self::Tracker { columns, rows } => interactive::validate_tracker(columns, rows),
            Self::Dashboard { widgets } => interactive::validate_dashboard(widgets),
        }
    }
}
impl Calculation {
    fn validate(
        &self,
        keys: &HashSet<&str>,
        depth: usize,
        nodes: &mut usize,
    ) -> Result<(), String> {
        *nodes += 1;
        if depth > 8 || *nodes > 64 {
            return Err("Calculator expression exceeds its depth or size limit".into());
        }
        match self {
            Self::Input { key } if keys.contains(key.as_str()) => Ok(()),
            Self::Input { .. } => Err("Calculator expression references an unknown input".into()),
            Self::Constant { value } => number(value).map(|_| ()),
            Self::Add { left, right }
            | Self::Subtract { left, right }
            | Self::Multiply { left, right }
            | Self::Divide { left, right } => {
                left.validate(keys, depth + 1, nodes)?;
                right.validate(keys, depth + 1, nodes)
            }
        }
    }
}

pub(crate) fn validate_session(session: &crate::sessions::SessionRecord) -> Result<(), String> {
    if session.chat_results.len() > MAX_RESULTS
        || serde_json::to_vec(&session.chat_results)
            .map_err(|_| "Invalid results")?
            .len()
            > MAX_SESSION_BYTES
    {
        return Err("Conversation result capacity exceeded".into());
    }
    let mut ids = HashSet::new();
    for result in &session.chat_results {
        if uuid::Uuid::parse_str(&result.id).is_err()
            || !ids.insert(&result.id)
            || result.revision == 0
            || result.revision > 9_007_199_254_740_991
            || result.agent_id != session.agent_id
            || result.session_id != session.id
            || result.clear_generation != session.clear_generation
            || result.updated_at_ms < result.created_at_ms
        {
            return Err("Saved result ownership, revision or identity is invalid".into());
        }
        text(&result.title, 200, false)?;
        text(&result.request_key, 128, false)?;
        text(&result.source_run_id, 512, false)?;
        text(&result.source_tool_call_id, 512, false)?;
        result.payload.validate()?;
    }
    Ok(())
}

pub(crate) fn upsert(
    session: &mut crate::sessions::SessionRecord,
    input: SaveResultInput,
    run: &str,
    call: &str,
    now: u64,
) -> Result<ChatResult, String> {
    validate_session(session)?;
    text(&input.title, 200, false)?;
    text(&input.request_key, 128, false)?;
    text(run, 512, false)?;
    text(call, 512, false)?;
    input.payload.validate()?;
    if input.id.is_none() && input.expected_revision != 0 {
        return Err("New results require expectedRevision 0".into());
    }
    // A create retry keeps its identity even after the report has been updated.
    let create_id = {
        let digest = Sha256::digest(
            serde_json::to_vec(&(
                &session.agent_id,
                &session.id,
                session.clear_generation,
                run,
                &input.request_key,
            ))
            .map_err(|_| "Invalid result identity")?,
        );
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(&digest[..16]);
        bytes[6] = (bytes[6] & 0x0f) | 0x40;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        uuid::Uuid::from_bytes(bytes).to_string()
    };
    if input.id.is_none() {
        if let Some(created) = session
            .chat_results
            .iter()
            .find(|saved| saved.id == create_id)
        {
            if created.title == input.title && created.payload == input.payload {
                return Ok(created.clone());
            }
            return Err(
                "This create request already produced a report which has since changed".into(),
            );
        }
    }
    if let Some(replay) = session
        .chat_results
        .iter()
        .find(|saved| saved.request_key == input.request_key && saved.source_run_id == run)
    {
        if input.id.as_ref().is_none_or(|id| id == &replay.id)
            && replay.title == input.title
            && replay.payload == input.payload
        {
            return Ok(replay.clone());
        }
        return Err("Result requestKey was already used for different content".into());
    }
    let previous = session.chat_results.clone();
    let saved = if let Some(id) = &input.id {
        let saved = session
            .chat_results
            .iter_mut()
            .find(|r| &r.id == id)
            .ok_or("Result not found in this conversation")?;
        if saved.revision != input.expected_revision {
            return Err("Result revision changed; refresh before updating".into());
        }
        saved.revision = saved
            .revision
            .checked_add(1)
            .ok_or("Result revision limit reached")?;
        saved.title = input.title;
        saved.payload = input.payload;
        saved.request_key = input.request_key;
        saved.source_run_id = run.into();
        saved.source_tool_call_id = call.into();
        saved.updated_at_ms = now.max(saved.updated_at_ms);
        saved.clone()
    } else {
        let saved = ChatResult {
            id: create_id,
            revision: 1,
            agent_id: session.agent_id.clone(),
            session_id: session.id.clone(),
            clear_generation: session.clear_generation,
            title: input.title,
            payload: input.payload,
            pinned: false,
            created_at_ms: now,
            updated_at_ms: now,
            source_run_id: run.into(),
            source_tool_call_id: call.into(),
            request_key: input.request_key,
        };
        session.chat_results.push(saved.clone());
        saved
    };
    if let Err(error) = validate_session(session) {
        session.chat_results = previous;
        return Err(error);
    }
    Ok(saved)
}

pub(crate) fn pin(
    session: &mut crate::sessions::SessionRecord,
    id: &str,
    revision: u64,
    pinned: bool,
    now: u64,
) -> Result<ChatResult, String> {
    validate_session(session)?;
    let saved = session
        .chat_results
        .iter_mut()
        .find(|r| r.id == id)
        .ok_or("Result not found in this conversation")?;
    if saved.revision != revision {
        return Err("Result revision changed; refresh before pinning".into());
    }
    let next = saved
        .revision
        .checked_add(1)
        .filter(|r| *r <= 9_007_199_254_740_991)
        .ok_or("Result revision limit reached")?;
    saved.revision = next;
    saved.pinned = pinned;
    saved.updated_at_ms = now.max(saved.updated_at_ms);
    Ok(saved.clone())
}

#[cfg(test)]
#[path = "chat_results/tests.rs"]
mod tests;
