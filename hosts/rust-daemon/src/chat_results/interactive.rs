//! Closed dashboard composition and typed, owner-editable tracker data.
use super::{number, text, ResultPayload};
use serde::{Deserialize, Serialize};
use serde_json::Number;
use std::collections::{BTreeMap, HashSet};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DashboardWidget {
    id: String,
    title: String,
    payload: ResultPayload,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TrackerColumn {
    key: String,
    label: String,
    kind: TrackerColumnKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum TrackerColumnKind {
    Text,
    Number,
    Checkbox,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TrackerRow {
    id: String,
    cells: BTreeMap<String, TrackerCell>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
enum TrackerCell {
    Text(String),
    Number(Number),
    Checkbox(bool),
}

fn identifier(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 40
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err(
            "Tracker keys and result IDs require 1–40 ASCII letters, digits or underscores".into(),
        );
    }
    Ok(())
}

pub(super) fn validate_tracker(
    columns: &[TrackerColumn],
    rows: &[TrackerRow],
) -> Result<(), String> {
    if columns.is_empty() || columns.len() > 12 || rows.len() > 100 {
        return Err("Trackers allow 1–12 columns and at most 100 rows".into());
    }
    let mut keys = HashSet::new();
    for column in columns {
        identifier(&column.key)?;
        text(&column.label, 160, false)?;
        if !keys.insert(column.key.as_str()) {
            return Err("Tracker column keys must be unique".into());
        }
    }
    let mut ids = HashSet::new();
    for row in rows {
        identifier(&row.id)?;
        if !ids.insert(row.id.as_str()) {
            return Err("Tracker row IDs must be unique".into());
        }
        if row.cells.len() != columns.len() {
            return Err("Tracker cells must match all declared columns".into());
        }
        for column in columns {
            match (row.cells.get(&column.key), &column.kind) {
                (Some(TrackerCell::Text(value)), TrackerColumnKind::Text) => {
                    text(value, 2048, true)?
                }
                (Some(TrackerCell::Number(value)), TrackerColumnKind::Number) => {
                    number(value)?;
                }
                (Some(TrackerCell::Checkbox(_)), TrackerColumnKind::Checkbox) => {}
                _ => return Err("Tracker cell types must match their declared column kinds".into()),
            }
        }
    }
    Ok(())
}

pub(super) fn validate_dashboard(widgets: &[DashboardWidget]) -> Result<(), String> {
    if widgets.is_empty() || widgets.len() > 8 {
        return Err("Dashboards allow 1–8 widgets".into());
    }
    let mut ids = HashSet::new();
    for widget in widgets {
        identifier(&widget.id)?;
        text(&widget.title, 160, false)?;
        if !ids.insert(widget.id.as_str()) {
            return Err("Dashboard widget IDs must be unique".into());
        }
        if matches!(widget.payload, ResultPayload::Dashboard { .. }) {
            return Err("Dashboards cannot contain nested dashboards".into());
        }
        widget.payload.validate()?;
    }
    Ok(())
}
