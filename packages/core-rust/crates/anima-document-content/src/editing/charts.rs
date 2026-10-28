//! Existing chart references and caches stay synchronized with staged worksheet data.
use super::{
    formulas::{self, Computed},
    package::Package,
    spreadsheet::{self, Sheet},
    types::*,
    word,
    xml_spans::{escape, Node, Patch, A, C, R, S},
};
use std::collections::{BTreeMap, BTreeSet};

struct Chart<'a> {
    id: String,
    path: String,
    root: &'a Node,
    chart: &'a Node,
    kind: String,
    supported: bool,
}
pub(super) fn parse_range(value: &str, sheets: &[Sheet<'_>]) -> Result<OfficeRange> {
    let (name, range) = value
        .rsplit_once('!')
        .ok_or_else(|| unsupported("chart or defined-name reference"))?;
    if name.contains(['[', ']', '\\', ':']) {
        return Err(unsupported("external chart or defined-name reference"));
    }
    let name = if name.starts_with('\'') {
        name.strip_prefix('\'')
            .and_then(|n| n.strip_suffix('\''))
            .ok_or(OfficeEditError::InvalidPackage)?
            .replace("''", "'")
    } else {
        name.to_owned()
    };
    let sheet = sheets
        .iter()
        .find(|s| s.name.eq_ignore_ascii_case(&name))
        .ok_or_else(|| unsupported("unknown worksheet reference"))?;
    let (first, last) = spreadsheet::rectangle(range)?;
    Ok(OfficeRange {
        sheet_id: sheet.id.clone(),
        start: formulas::address(first),
        end: formulas::address(last),
    })
}
fn range_formula(range: &OfficeRange, sheets: &[Sheet<'_>]) -> Result<String> {
    let sheet = sheets
        .iter()
        .find(|s| s.id == range.sheet_id)
        .ok_or(OfficeEditError::StaleTarget)?;
    let first = formulas::coordinate(&range.start)?;
    let last = formulas::coordinate(&range.end)?;
    if first.0 > last.0 || first.1 > last.1 {
        return Err(invalid("chart range is reversed"));
    }
    let absolute = |c| {
        let address = formulas::address(c);
        let split = address
            .find(|c: char| c.is_ascii_digit())
            .unwrap_or(address.len());
        format!("${}${}", &address[..split], &address[split..])
    };
    Ok(format!(
        "'{}'!{}:{}",
        sheet.name.replace('\'', "''"),
        absolute(first),
        absolute(last)
    ))
}
fn charts<'a>(package: &'a Package, sheets: &[Sheet<'_>]) -> Result<Vec<Chart<'a>>> {
    let mut paths = BTreeSet::new();
    for sheet in sheets {
        for drawing in sheet.root.all(S, "drawing") {
            let relation = package.relation(
                &sheet.path,
                drawing
                    .attr_ns(R, "id")
                    .ok_or(OfficeEditError::InvalidPackage)?,
            )?;
            if relation.external || relation.kind != format!("{R}/drawing") {
                return Err(OfficeEditError::InvalidPackage);
            }
            for chart in package.xml(&relation.target)?.all(C, "chart") {
                let relation = package.relation(
                    &relation.target,
                    chart
                        .attr_ns(R, "id")
                        .ok_or(OfficeEditError::InvalidPackage)?,
                )?;
                if relation.external || relation.kind != format!("{R}/chart") {
                    return Err(OfficeEditError::InvalidPackage);
                }
                paths.insert(relation.target.clone());
            }
        }
    }
    let mut out = Vec::new();
    for (index, path) in paths.into_iter().enumerate() {
        if index >= 32 {
            return Err(OfficeEditError::ResourceLimit);
        }
        let root = package.xml(&path)?;
        if !root.is(C, "chartSpace") {
            return Err(OfficeEditError::InvalidPackage);
        }
        let chart = root
            .child(C, "chart")
            .ok_or(OfficeEditError::InvalidPackage)?;
        let plot = chart
            .child(C, "plotArea")
            .ok_or(OfficeEditError::InvalidPackage)?;
        let kinds = plot
            .children
            .iter()
            .filter(|n| n.name.ends_with("Chart") && n.ns == C)
            .collect::<Vec<_>>();
        let supported = kinds.len() == 1
            && matches!(
                kinds[0].name.as_str(),
                "barChart" | "lineChart" | "pieChart"
            );
        let kind = kinds
            .first()
            .map_or("unknown", |n| n.name.as_str())
            .to_owned();
        if chart.all(C, "ser").len() > 64 {
            return Err(OfficeEditError::ResourceLimit);
        }
        out.push(Chart {
            id: format!("chart:{index}"),
            path,
            root,
            chart,
            kind,
            supported,
        });
    }
    Ok(out)
}
fn title(chart: &Chart<'_>) -> String {
    chart
        .chart
        .child(C, "title")
        .map(|n| n.all(A, "t").iter().map(|n| n.text.as_str()).collect())
        .unwrap_or_default()
}
fn series_range(node: &Node, name: &str, sheets: &[Sheet<'_>]) -> Result<Option<OfficeRange>> {
    let Some(container) = node.child(C, name) else {
        return Ok(None);
    };
    let formulas = container.all(C, "f");
    if formulas.len() != 1 {
        return Ok(None);
    }
    parse_range(&formulas[0].text, sheets).map(Some)
}
pub(super) fn inventory(package: &Package, sheets: &[Sheet<'_>]) -> Result<Vec<OfficeTarget>> {
    let mut out = Vec::new();
    for chart in charts(package, sheets)? {
        out.push(OfficeTarget {
            id: chart.id.clone(),
            label: format!("Chart: {}", title(&chart)),
            editable: chart.supported,
            value: OfficeTargetValue::Chart {
                title: title(&chart),
                chart_type: chart.kind.clone(),
            },
        });
        for (index, series) in chart.chart.all(C, "ser").iter().enumerate() {
            out.push(OfficeTarget {
                id: format!("{}:series:{index}", chart.id),
                label: format!("{} series {}", chart.id, index + 1),
                editable: chart.supported,
                value: OfficeTargetValue::Series {
                    categories: series_range(series, "cat", sheets)?,
                    values: series_range(series, "val", sheets)?,
                },
            });
        }
    }
    Ok(out)
}
fn values(
    package: &Package,
    sheets: &[Sheet<'_>],
    range: &OfficeRange,
    numeric: bool,
) -> Result<Vec<String>> {
    let sheet = sheets
        .iter()
        .find(|s| s.id == range.sheet_id)
        .ok_or(OfficeEditError::StaleTarget)?;
    let first = formulas::coordinate(&range.start)?;
    let last = formulas::coordinate(&range.end)?;
    if first.0 > last.0 || first.1 > last.1 || (first.0 != last.0 && first.1 != last.1) {
        return Err(unsupported("chart series must use one-dimensional ranges"));
    }
    let count = u64::from(last.0 - first.0 + 1) * u64::from(last.1 - first.1 + 1);
    if count > 1000 {
        return Err(OfficeEditError::ResourceLimit);
    }
    let cells = spreadsheet::cells(package, sheet)?;
    let calculated = spreadsheet::calculated_values(package, sheet)?;
    let mut out = Vec::new();
    for row in first.0..=last.0 {
        for column in first.1..=last.1 {
            let cell = cells.get(&(row, column));
            let value = if let Some(cell) = cell.filter(|c| c.formula.is_some()) {
                let _ = cell;
                match calculated.get(&(row, column)) {
                    Some(Computed::Number(v)) => v.to_string(),
                    _ => return Err(unsupported("chart inputs need external recalculation")),
                }
            } else {
                match cell.map(|c| &c.value) {
                    Some(OfficeCellValue::Number(v)) => v.to_string(),
                    Some(OfficeCellValue::String(v)) if !numeric => v.clone(),
                    Some(OfficeCellValue::Boolean(v)) if !numeric => v.to_string(),
                    _ => {
                        return Err(unsupported(
                            "chart inputs contain blanks or unsupported values",
                        ))
                    }
                }
            };
            out.push(value);
        }
    }
    Ok(out)
}
fn cache(template: &Node, items: &[String], numeric: bool) -> String {
    let cache = spreadsheet::q(template, if numeric { "numCache" } else { "strCache" });
    let count = spreadsheet::q(template, "ptCount");
    let point = spreadsheet::q(template, "pt");
    let value = spreadsheet::q(template, "v");
    let format = if numeric {
        let name = spreadsheet::q(template, "formatCode");
        let original = template.all(C, "formatCode");
        format!(
            "<{name}>{}</{name}>",
            escape(original.first().map_or("General", |n| n.text.as_str()))
        )
    } else {
        String::new()
    };
    format!(
        "<{cache}>{format}<{count} val=\"{}\"/>{}</{cache}>",
        items.len(),
        items
            .iter()
            .enumerate()
            .map(|(i, v)| format!(
                "<{point} idx=\"{i}\"><{value}>{}</{value}></{point}>",
                escape(v)
            ))
            .collect::<String>()
    )
}
fn reference(template: &Node, formula: &str, items: &[String], numeric: bool) -> String {
    let reference = spreadsheet::q(template, if numeric { "numRef" } else { "strRef" });
    let f = spreadsheet::q(template, "f");
    format!(
        "<{reference}><{f}>{}</{f}>{}</{reference}>",
        escape(formula),
        cache(template, items, numeric)
    )
}
pub(super) fn apply_operation(
    package: &Package,
    sheets: &[Sheet<'_>],
    operation: &OfficeEditOperation,
    index: usize,
    patches: &mut BTreeMap<String, Vec<Patch>>,
    changes: &mut Vec<OfficeChange>,
) -> Result<()> {
    let charts = charts(package, sheets)?;
    match operation {
        OfficeEditOperation::SetChartTitle {
            chart_id,
            expected_title,
            title: new,
        } => {
            let chart = charts
                .iter()
                .find(|c| c.id == *chart_id)
                .ok_or(OfficeEditError::StaleTarget)?;
            if !chart.supported {
                return Err(unsupported("chart type"));
            }
            if title(chart) != *expected_title {
                return Err(OfficeEditError::StaleTarget);
            }
            let node = chart
                .chart
                .child(C, "title")
                .ok_or_else(|| unsupported("adding chart titles"))?;
            let paragraphs = node.all(A, "p");
            if paragraphs.len() != 1 {
                return Err(unsupported("rich chart title"));
            }
            let target = word::drawing_paragraph(
                paragraphs[0],
                &package.part(&chart.path)?.bytes,
                chart_id.clone(),
                "Chart title".into(),
            );
            let edit = word::edit(
                &target,
                0,
                target.text.chars().count() as u32,
                expected_title,
                new,
            )?;
            changes.push(OfficeChange {
                operation_index: Some(index),
                label: "Chart title".into(),
                before: expected_title.clone(),
                after: new.clone(),
                derived: false,
            });
            word::report_whitespace(changes, &edit, index, "Chart title");
            patches
                .entry(chart.path.clone())
                .or_default()
                .extend(edit.patches);
        }
        OfficeEditOperation::SetChartSeriesRanges {
            series_id,
            categories,
            values: range,
        } => {
            let (chart, series) = charts
                .iter()
                .find_map(|chart| {
                    chart
                        .chart
                        .all(C, "ser")
                        .into_iter()
                        .enumerate()
                        .find(|(i, _)| format!("{}:series:{i}", chart.id) == *series_id)
                        .map(|(_, s)| (chart, s))
                })
                .ok_or(OfficeEditError::StaleTarget)?;
            if !chart.supported {
                return Err(unsupported("chart type"));
            }
            let cat = series
                .child(C, "cat")
                .ok_or_else(|| unsupported("chart categories"))?;
            let val = series
                .child(C, "val")
                .ok_or_else(|| unsupported("chart values"))?;
            let category_values = values(package, sheets, categories, false)?;
            let value_values = values(package, sheets, range, true)?;
            if category_values.len() != value_values.len() {
                return Err(invalid(
                    "chart categories and values must have equal lengths",
                ));
            }
            let category_formula = range_formula(categories, sheets)?;
            let value_formula = range_formula(range, sheets)?;
            patches.entry(chart.path.clone()).or_default().extend([
                Patch {
                    span: cat.span.clone(),
                    replacement: cat.element(
                        &reference(cat, &category_formula, &category_values, false),
                        &[],
                    ),
                },
                Patch {
                    span: val.span.clone(),
                    replacement: val
                        .element(&reference(val, &value_formula, &value_values, true), &[]),
                },
            ]);
            changes.push(OfficeChange {
                operation_index: Some(index),
                label: series_id.clone(),
                before: format!(
                    "{:?} / {:?}",
                    series_range(series, "cat", sheets)?,
                    series_range(series, "val", sheets)?
                ),
                after: format!("{category_formula} / {value_formula}"),
                derived: false,
            });
        }
        _ => return Err(invalid("chart operation")),
    }
    Ok(())
}
pub(super) fn refresh(
    package: &Package,
    sheets: &[Sheet<'_>],
    patches: &mut BTreeMap<String, Vec<Patch>>,
    changes: &mut Vec<OfficeChange>,
) -> Result<()> {
    for chart in charts(package, sheets)? {
        if !chart.supported {
            return Err(unsupported("cell edits could stale an unsupported chart"));
        }
        for series in chart.chart.all(C, "ser") {
            let categories = series_range(series, "cat", sheets)?
                .ok_or_else(|| unsupported("chart categories cannot be refreshed"))?;
            let value_range = series_range(series, "val", sheets)?
                .ok_or_else(|| unsupported("chart values cannot be refreshed"))?;
            if values(package, sheets, &categories, false)?.len()
                != values(package, sheets, &value_range, true)?.len()
            {
                return Err(invalid("chart cache count mismatch"));
            }
        }
        for node in chart
            .root
            .all(C, "strRef")
            .into_iter()
            .chain(chart.root.all(C, "numRef"))
        {
            let f = node.child(C, "f").ok_or(OfficeEditError::InvalidPackage)?;
            let range = parse_range(&f.text, sheets)?;
            let numeric = node.is(C, "numRef");
            let points = values(package, sheets, &range, numeric)?;
            let children = node
                .children
                .iter()
                .filter(|n| !n.is(C, "strCache") && !n.is(C, "numCache"))
                .map(|n| {
                    std::str::from_utf8(&package.part(&chart.path)?.bytes[n.span.clone()])
                        .map(str::to_owned)
                        .map_err(|_| OfficeEditError::InvalidPackage)
                })
                .collect::<Result<Vec<_>>>()?
                .concat();
            patches.entry(chart.path.clone()).or_default().push(Patch {
                span: node.span.clone(),
                replacement: node
                    .element(&format!("{children}{}", cache(node, &points, numeric)), &[]),
            });
            changes.push(OfficeChange {
                operation_index: None,
                label: format!("{} data cache: {}", chart.id, f.text),
                before: "Stored chart cache".into(),
                after: format!("Verified {} points: {}", points.len(), points.join(", ")),
                derived: true,
            });
        }
    }
    Ok(())
}
