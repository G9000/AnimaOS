//! Typed cell edits retain styles; calculation caches explicitly reflect supported results.
use super::{
    formulas::{self, Computed, Coordinate},
    package::Package,
    types::*,
    xml_spans::{self, escape, Node, Patch, R, S},
};
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct Sheet<'a> {
    pub id: String,
    pub name: String,
    pub path: String,
    pub root: &'a Node,
    pub hidden: bool,
}
#[derive(Clone)]
pub(super) struct Cell<'a> {
    pub node: &'a Node,
    pub value: OfficeCellValue,
    pub formula: Option<String>,
    pub complex: bool,
    pub hidden: bool,
}
pub(super) fn sheets(package: &Package) -> Result<Vec<Sheet<'_>>> {
    let root = package.xml(&package.main)?;
    if !root.is(S, "workbook") {
        return Err(OfficeEditError::InvalidPackage);
    }
    let list = root
        .child(S, "sheets")
        .ok_or(OfficeEditError::InvalidPackage)?;
    if list.children.is_empty() || list.children.len() > 32 {
        return Err(OfficeEditError::ResourceLimit);
    }
    let mut ids = BTreeSet::new();
    let mut names = BTreeSet::new();
    let mut out = Vec::new();
    for node in &list.children {
        if !node.is(S, "sheet") {
            return Err(OfficeEditError::InvalidPackage);
        }
        let id = node
            .attr_ns(R, "id")
            .ok_or(OfficeEditError::InvalidPackage)?;
        let name = node.required("name")?;
        if name.is_empty()
            || name.chars().count() > 31
            || name.contains(['[', ']', ':', '*', '?', '/', '\\'])
            || !ids.insert(id)
            || !names.insert(name.to_lowercase())
        {
            return Err(OfficeEditError::InvalidPackage);
        }
        let relation = package.relation(&package.main, id)?;
        if relation.external || relation.kind != format!("{R}/worksheet") {
            return Err(OfficeEditError::InvalidPackage);
        }
        let root = package.xml(&relation.target)?;
        if !root.is(S, "worksheet") {
            return Err(OfficeEditError::InvalidPackage);
        }
        out.push(Sheet {
            id: format!("sheet:{id}"),
            name: name.into(),
            path: relation.target.clone(),
            root,
            hidden: matches!(node.attr("state"), Some("hidden" | "veryHidden")),
        });
    }
    Ok(out)
}
fn strings(package: &Package) -> Result<Vec<String>> {
    let Some(rels) = package.relationships.get(&package.main) else {
        return Ok(Vec::new());
    };
    let parts = rels
        .values()
        .filter(|r| r.kind == format!("{R}/sharedStrings"))
        .collect::<Vec<_>>();
    if parts.len() > 1 {
        return Err(OfficeEditError::InvalidPackage);
    }
    let Some(relation) = parts.first() else {
        return Ok(Vec::new());
    };
    let root = package.xml(&relation.target)?;
    if !root.is(S, "sst") || root.children.len() > 10_000 {
        return Err(OfficeEditError::ResourceLimit);
    }
    root.children
        .iter()
        .map(|node| {
            if !node.is(S, "si") {
                return Err(OfficeEditError::InvalidPackage);
            }
            Ok(string_text(node))
        })
        .collect()
}
fn string_text(node: &Node) -> String {
    node.children
        .iter()
        .filter_map(|n| {
            if n.is(S, "t") {
                Some(n.text.clone())
            } else if n.is(S, "r") {
                Some(
                    n.all(S, "t")
                        .iter()
                        .map(|t| t.text.as_str())
                        .collect::<String>(),
                )
            } else {
                None
            }
        })
        .collect()
}
pub(super) fn cells<'a>(
    package: &Package,
    sheet: &Sheet<'a>,
) -> Result<BTreeMap<Coordinate, Cell<'a>>> {
    let shared = strings(package)?;
    let mut column_changes = vec![0i32; 16_386];
    for column in sheet.root.all(S, "col") {
        let first = column
            .required("min")?
            .parse::<usize>()
            .map_err(|_| OfficeEditError::InvalidPackage)?;
        let last = column
            .required("max")?
            .parse::<usize>()
            .map_err(|_| OfficeEditError::InvalidPackage)?;
        if first == 0 || first > last || last > 16_384 {
            return Err(OfficeEditError::InvalidPackage);
        }
        if matches!(column.attr("hidden"), Some("1" | "true")) {
            column_changes[first] += 1;
            column_changes[last + 1] -= 1;
        }
    }
    let mut active = 0;
    let hidden_columns = column_changes
        .into_iter()
        .map(|change| {
            active += change;
            active > 0
        })
        .collect::<Vec<_>>();
    let data = sheet
        .root
        .child(S, "sheetData")
        .ok_or(OfficeEditError::InvalidPackage)?;
    let mut out = BTreeMap::new();
    let mut rows = BTreeSet::new();
    for row in &data.children {
        if !row.is(S, "row") {
            return Err(OfficeEditError::InvalidPackage);
        }
        let number = row
            .required("r")?
            .parse::<u32>()
            .map_err(|_| OfficeEditError::InvalidPackage)?;
        if !(1..=1_048_576).contains(&number) || !rows.insert(number) {
            return Err(OfficeEditError::InvalidPackage);
        }
        for node in &row.children {
            if !node.is(S, "c") {
                return Err(OfficeEditError::InvalidPackage);
            }
            let coord = formulas::coordinate(node.required("r")?)?;
            if coord.0 != number || out.len() >= 10_000 {
                return Err(OfficeEditError::ResourceLimit);
            }
            if node.all(S, "f").len() > 1 || node.all(S, "v").len() > 1 {
                return Err(OfficeEditError::InvalidPackage);
            }
            let raw = node.child(S, "v").map(|n| n.text.as_str());
            let value = match node.attr("t") {
                Some("s") => OfficeCellValue::String(
                    shared
                        .get(
                            raw.ok_or(OfficeEditError::InvalidPackage)?
                                .parse::<usize>()
                                .map_err(|_| OfficeEditError::InvalidPackage)?,
                        )
                        .ok_or(OfficeEditError::InvalidPackage)?
                        .clone(),
                ),
                Some("inlineStr") => OfficeCellValue::String(string_text(
                    node.child(S, "is").ok_or(OfficeEditError::InvalidPackage)?,
                )),
                Some("str") => OfficeCellValue::String(raw.unwrap_or("").into()),
                Some("b") => OfficeCellValue::Boolean(match raw {
                    Some("1") => true,
                    Some("0") => false,
                    _ => return Err(OfficeEditError::InvalidPackage),
                }),
                Some("e") => OfficeCellValue::String(raw.unwrap_or("").into()),
                None | Some("n") => match raw {
                    None => OfficeCellValue::Blank,
                    Some(n) => {
                        let n = n
                            .parse::<f64>()
                            .map_err(|_| OfficeEditError::InvalidPackage)?;
                        if !n.is_finite() {
                            return Err(OfficeEditError::InvalidPackage);
                        }
                        OfficeCellValue::Number(n)
                    }
                },
                _ => return Err(unsupported("cell value type")),
            };
            let formula = node.child(S, "f");
            let complex = formula.is_some_and(|f| !matches!(f.attr("t"), None | Some("normal")))
                || node.attr("cm").is_some()
                || node.attr("vm").is_some();
            if out
                .insert(
                    coord,
                    Cell {
                        node,
                        value,
                        formula: formula.map(|f| f.text.clone()),
                        complex,
                        hidden: matches!(row.attr("hidden"), Some("1" | "true"))
                            || hidden_columns[coord.1 as usize],
                    },
                )
                .is_some()
            {
                return Err(OfficeEditError::InvalidPackage);
            }
        }
    }
    Ok(out)
}
pub(super) fn fixed_names(package: &Package, sheets: &[Sheet<'_>]) -> Result<BTreeSet<String>> {
    let mut names = BTreeSet::new();
    if let Some(root) = package.xml(&package.main)?.child(S, "definedNames") {
        for node in &root.children {
            if !node.is(S, "definedName") {
                return Err(OfficeEditError::InvalidPackage);
            }
            super::charts::parse_range(&node.text, sheets)?;
            if !names.insert(node.required("name")?.to_ascii_uppercase()) {
                return Err(OfficeEditError::InvalidPackage);
            }
        }
    }
    Ok(names)
}
pub(super) fn inventory(package: &Package) -> Result<Vec<OfficeTarget>> {
    let sheets = sheets(package)?;
    let names = fixed_names(package, &sheets)?;
    let mut targets = Vec::new();
    let mut count = 0;
    for sheet in &sheets {
        let cells = cells(package, sheet)?;
        count += cells.len();
        if count > 10_000 {
            return Err(OfficeEditError::ResourceLimit);
        }
        for (coord, cell) in cells {
            if let Some(formula) = &cell.formula {
                if !formula.is_empty() {
                    formulas::admit(formula, &names)?;
                }
            }
            targets.push(OfficeTarget {
                id: format!("{}:{}", sheet.id, formulas::address(coord)),
                label: format!(
                    "{}!{}{}",
                    sheet.name,
                    formulas::address(coord),
                    if sheet.hidden {
                        " (hidden sheet)"
                    } else if cell.hidden {
                        " (hidden row/column)"
                    } else {
                        ""
                    }
                ),
                editable: !cell.complex && sheet.root.child(S, "sheetProtection").is_none(),
                value: OfficeTargetValue::Cell {
                    sheet_id: sheet.id.clone(),
                    address: formulas::address(coord),
                    value: cell.value,
                    formula: cell.formula.clone(),
                    calculation: if cell.formula.is_some() {
                        OfficeCalculationState::StoredUnverified
                    } else {
                        OfficeCalculationState::Literal
                    },
                },
            });
        }
        for field in [
            "formula",
            "formula1",
            "formula2",
            "calculatedColumnFormula",
            "totalsRowFormula",
        ] {
            for node in sheet.root.all(S, field) {
                formulas::admit(&node.text, &names)?;
            }
        }
    }
    for part in &package.parts {
        if let Some(root) = &part.xml {
            for node in root.all(super::xml_spans::C, "f") {
                super::charts::parse_range(&node.text, &sheets)?;
            }
        }
    }
    targets.extend(super::charts::inventory(package, &sheets)?);
    Ok(targets)
}
pub(super) fn q(node: &Node, name: &str) -> String {
    node.raw_name
        .rsplit_once(':')
        .map_or_else(|| name.to_owned(), |(prefix, _)| format!("{prefix}:{name}"))
}
fn raw<'a>(bytes: &'a [u8], node: &Node) -> Result<&'a str> {
    std::str::from_utf8(&bytes[node.span.clone()]).map_err(|_| OfficeEditError::InvalidPackage)
}
fn value_xml(
    node: Option<&Node>,
    template: &Node,
    address: &str,
    value: Option<&OfficeCellValue>,
    formula: Option<&str>,
) -> Result<String> {
    let tag = q(template, "c");
    let v = q(template, "v");
    let f = q(template, "f");
    let is = q(template, "is");
    let t = q(template, "t");
    let (kind, inner) = if let Some(formula) = formula {
        (None, format!("<{f}>{}</{f}>", escape(formula)))
    } else {
        match value.ok_or_else(|| invalid("cell value"))? {
            OfficeCellValue::String(text) => {
                if text.len() > 8192 || text.chars().any(|c| !crate::docx::valid_xml_character(c)) {
                    return Err(invalid("cell text"));
                }
                (
                    Some("inlineStr"),
                    format!(
                        "<{is}><{t} xml:space=\"preserve\">{}</{t}></{is}>",
                        escape(text)
                    ),
                )
            }
            OfficeCellValue::Number(number) => {
                if !number.is_finite() {
                    return Err(invalid("finite numeric value required"));
                }
                (None, format!("<{v}>{number}</{v}>"))
            }
            OfficeCellValue::Boolean(value) => {
                (Some("b"), format!("<{v}>{}</{v}>", u8::from(*value)))
            }
            OfficeCellValue::Blank => (None, String::new()),
        }
    };
    Ok(if let Some(node) = node {
        if node
            .children
            .iter()
            .any(|n| !n.is(S, "f") && !n.is(S, "v") && !n.is(S, "is"))
        {
            return Err(unsupported("cell extension content"));
        }
        node.element(&inner, &[("t", kind)])
    } else {
        format!(
            "<{tag} r=\"{address}\"{}>{inner}</{tag}>",
            kind.map_or(String::new(), |t| format!(" t=\"{t}\""))
        )
    })
}
pub(super) struct Applied {
    pub replacements: BTreeMap<String, Vec<u8>>,
    pub removed: BTreeSet<String>,
    pub changes: Vec<OfficeChange>,
}
pub(super) fn apply(package: &Package, operations: &[OfficeEditOperation]) -> Result<Applied> {
    let worksheet_list = sheets(package)?;
    let mut patches: BTreeMap<String, Vec<Patch>> = BTreeMap::new();
    let mut changes = Vec::new();
    let mut edited: BTreeMap<String, BTreeMap<Coordinate, String>> = BTreeMap::new();
    let mut cell_changes = false;
    for (index, op) in operations.iter().enumerate() {
        let (id, address, value, formula) = match op {
            OfficeEditOperation::SetCellValue {
                sheet_id,
                cell,
                value,
            } => (sheet_id, cell, Some(value), None),
            OfficeEditOperation::SetCellFormula {
                sheet_id,
                cell,
                formula,
            } => {
                let f = formula
                    .strip_prefix('=')
                    .ok_or_else(|| invalid("new formulas require one leading ="))?;
                formulas::parse(f)?;
                (sheet_id, cell, None, Some(f))
            }
            OfficeEditOperation::SetChartTitle { .. }
            | OfficeEditOperation::SetChartSeriesRanges { .. } => {
                super::charts::apply_operation(
                    package,
                    &worksheet_list,
                    op,
                    index,
                    &mut patches,
                    &mut changes,
                )?;
                continue;
            }
            _ => return Err(invalid("operation does not match spreadsheet format")),
        };
        cell_changes = true;
        let sheet = worksheet_list
            .iter()
            .find(|s| s.id == *id)
            .ok_or(OfficeEditError::StaleTarget)?;
        let coord = formulas::coordinate(address)?;
        let cells = cells(package, sheet)?;
        if sheet.root.child(S, "sheetProtection").is_some() {
            return Err(unsupported("protected worksheet"));
        }
        for node in sheet.root.all(S, "mergeCell") {
            let (first, last) = rectangle(node.required("ref")?)?;
            if contains(first, last, coord) && coord != first {
                return Err(unsupported("edit must use merged cell anchor"));
            }
        }
        for cell in cells.values() {
            if let Some(f) = cell.node.child(S, "f") {
                if cell.complex {
                    let bounds = f.attr("ref").map(rectangle).transpose()?;
                    if bounds.is_none_or(|(a, b)| contains(a, b, coord)) {
                        return Err(unsupported("shared, array or spill formula region"));
                    }
                }
            }
        }
        let old = cells.get(&coord);
        if old.is_some_and(|c| c.complex) {
            return Err(unsupported("cell metadata or complex formula"));
        }
        let xml = value_xml(
            old.map(|c| c.node),
            old.map_or(sheet.root, |c| c.node),
            &formulas::address(coord),
            value,
            formula,
        )?;
        if edited
            .entry(sheet.path.clone())
            .or_default()
            .insert(coord, xml)
            .is_some()
        {
            return Err(invalid("duplicate cell edit"));
        }
        changes.push(OfficeChange {
            operation_index: Some(index),
            label: format!("{}!{}", sheet.name, formulas::address(coord)),
            before: old.map_or(String::new(), |c| {
                c.formula
                    .clone()
                    .map_or_else(|| format!("{:?}", c.value), |f| format!("={f}"))
            }),
            after: formula.map_or_else(|| format!("{value:?}"), |f| format!("={f}")),
            derived: false,
        });
    }
    if cell_changes && package.has_pivot {
        return Err(unsupported("cell edits could stale pivot caches"));
    }
    for sheet in &worksheet_list {
        let Some(edits) = edited.get(&sheet.path) else {
            continue;
        };
        let data = sheet
            .root
            .child(S, "sheetData")
            .ok_or(OfficeEditError::InvalidPackage)?;
        let bytes = &package.part(&sheet.path)?.bytes;
        let mut by_row: BTreeMap<u32, Vec<(Coordinate, &String)>> = BTreeMap::new();
        for (coord, xml) in edits {
            by_row.entry(coord.0).or_default().push((*coord, xml));
        }
        if data.span.end == data.inner.end {
            let row_tag = q(data, "row");
            let inner = by_row
                .iter()
                .map(|(row, edits)| {
                    let cells = edits
                        .iter()
                        .map(|(_, xml)| xml.as_str())
                        .collect::<String>();
                    format!("<{row_tag} r=\"{row}\">{cells}</{row_tag}>")
                })
                .collect::<String>();
            patches.entry(sheet.path.clone()).or_default().push(Patch {
                span: data.span.clone(),
                replacement: data.element(&inner, &[]),
            });
            by_row.clear();
        }
        for (row, edits) in by_row {
            if let Some(existing) = data
                .children
                .iter()
                .find(|r| r.attr("r") == Some(row.to_string().as_str()))
            {
                let mut local = Vec::new();
                for (coord, xml) in edits {
                    if let Some(cell) = existing.children.iter().find(|n| {
                        n.attr("r").and_then(|r| formulas::coordinate(r).ok()) == Some(coord)
                    }) {
                        local.push(Patch {
                            span: cell.span.start - existing.inner.start
                                ..cell.span.end - existing.inner.start,
                            replacement: xml.clone(),
                        });
                    } else {
                        let insertion = existing
                            .children
                            .iter()
                            .find(|n| {
                                n.attr("r")
                                    .and_then(|r| formulas::coordinate(r).ok())
                                    .is_some_and(|c| c.1 > coord.1)
                            })
                            .map_or(existing.inner.end, |n| n.span.start);
                        local.push(Patch {
                            span: insertion - existing.inner.start
                                ..insertion - existing.inner.start,
                            replacement: xml.clone(),
                        });
                    }
                }
                let inner = xml_spans::patch(&bytes[existing.inner.clone()], &mut local)?;
                let inner =
                    std::str::from_utf8(&inner).map_err(|_| OfficeEditError::InvalidPackage)?;
                patches.entry(sheet.path.clone()).or_default().push(Patch {
                    span: existing.span.clone(),
                    replacement: existing.element(inner, &[("spans", None)]),
                });
            } else {
                let row_tag = q(data, "row");
                let inner = edits
                    .iter()
                    .map(|(_, xml)| xml.as_str())
                    .collect::<String>();
                let insertion = data
                    .children
                    .iter()
                    .find(|r| {
                        r.attr("r")
                            .and_then(|r| r.parse::<u32>().ok())
                            .is_some_and(|r| r > row)
                    })
                    .map_or(data.inner.end, |r| r.span.start);
                patches.entry(sheet.path.clone()).or_default().push(Patch {
                    span: insertion..insertion,
                    replacement: format!("<{row_tag} r=\"{row}\">{inner}</{row_tag}>"),
                });
            }
        }
        let all = cells(package, sheet)?;
        let max_row = all
            .keys()
            .chain(edits.keys())
            .map(|c| c.0)
            .max()
            .unwrap_or(1);
        let max_col = all
            .keys()
            .chain(edits.keys())
            .map(|c| c.1)
            .max()
            .unwrap_or(1);
        let bounds = format!("A1:{}", formulas::address((max_row, max_col)));
        if let Some(dimension) = sheet.root.child(S, "dimension") {
            patches.entry(sheet.path.clone()).or_default().push(Patch {
                span: dimension.span.clone(),
                replacement: dimension.element("", &[("ref", Some(&bounds))]),
            });
        }
    }
    let mut replacements = super::materialize(package, patches)?;
    let staged_bytes = package.encode(&replacements, &BTreeSet::new())?;
    let staged = Package::open(OfficeFormat::Xlsx, &staged_bytes)?;
    let mut derived: BTreeMap<String, Vec<Patch>> = BTreeMap::new();
    let mut removed = BTreeSet::new();
    if cell_changes {
        let staged_sheets = sheets(&staged)?;
        for sheet in &staged_sheets {
            let values = cells(&staged, sheet)?;
            let mut memo = BTreeMap::new();
            let mut visiting = BTreeSet::new();
            let mut budget = Budget::default();
            for (coord, cell) in &values {
                if cell.formula.is_some() {
                    let result = compute(*coord, &values, &mut memo, &mut visiting, &mut budget);
                    let bytes = &staged.part(&sheet.path)?.bytes;
                    let mut inner = String::new();
                    for child in &cell.node.children {
                        if !child.is(S, "v") {
                            inner.push_str(raw(bytes, child)?);
                        }
                    }
                    if let Computed::Number(value) = result {
                        let tag = q(cell.node, "v");
                        inner.push_str(&format!("<{tag}>{value}</{tag}>"));
                    }
                    derived.entry(sheet.path.clone()).or_default().push(Patch {
                        span: cell.node.span.clone(),
                        replacement: cell.node.element(&inner, &[("t", None)]),
                    });
                    changes.push(OfficeChange {
                        operation_index: None,
                        label: format!(
                            "{}!{} formula cache",
                            sheet.name,
                            formulas::address(*coord)
                        ),
                        before: format!("{:?} (stored)", cell.value),
                        after: format!("{result:?}"),
                        derived: true,
                    });
                }
            }
        }
        let root = staged.xml(&staged.main)?;
        let settings = [
            ("calcMode", Some("auto")),
            ("fullCalcOnLoad", Some("1")),
            ("forceFullCalc", Some("1")),
        ];
        let patch = if let Some(calc) = root.child(S, "calcPr") {
            Patch {
                span: calc.span.clone(),
                replacement: calc.element("", &settings),
            }
        } else {
            let tag = q(root, "calcPr");
            Patch {
                span: root.inner.end..root.inner.end,
                replacement: format!(
                    "<{tag} calcMode=\"auto\" fullCalcOnLoad=\"1\" forceFullCalc=\"1\"/>"
                ),
            }
        };
        derived.entry(staged.main.clone()).or_default().push(patch);
        changes.push(OfficeChange {
            operation_index: None,
            label: "Workbook recalculation".into(),
            before: "Original calculation settings".into(),
            after: "Recalculate on opening; deferred results have no cached value".into(),
            derived: true,
        });
        if let Some(rels) = staged.relationships.get(&staged.main) {
            for relation in rels.values().filter(|r| r.kind == format!("{R}/calcChain")) {
                removed.insert(relation.target.clone());
            }
        }
        if !removed.is_empty() {
            for part in &staged.parts {
                if let Some(root) = &part.xml {
                    for node in &root.children {
                        if (node.name == "Relationship"
                            && node.attr("Type") == Some(format!("{R}/calcChain").as_str()))
                            || (node.name == "Override"
                                && node.attr("PartName").is_some_and(|p| {
                                    removed.contains(p.strip_prefix('/').unwrap_or(p))
                                }))
                        {
                            derived.entry(part.name.clone()).or_default().push(Patch {
                                span: node.span.clone(),
                                replacement: String::new(),
                            });
                        }
                    }
                }
            }
            changes.push(OfficeChange {
                operation_index: None,
                label: "Calculation chain".into(),
                before: "Stored dependency chain".into(),
                after: "Removed with its relationship/content-type entry".into(),
                derived: true,
            });
        }
        super::charts::refresh(&staged, &staged_sheets, &mut derived, &mut changes)?;
    } else if operations
        .iter()
        .any(|o| matches!(o, OfficeEditOperation::SetChartSeriesRanges { .. }))
    {
        super::charts::refresh(&staged, &sheets(&staged)?, &mut derived, &mut changes)?;
    }
    replacements.extend(super::materialize(&staged, derived)?);
    Ok(Applied {
        replacements,
        removed,
        changes,
    })
}
#[derive(Default)]
struct Budget {
    nodes: usize,
    visits: usize,
}
fn compute(
    coord: Coordinate,
    cells: &BTreeMap<Coordinate, Cell<'_>>,
    memo: &mut BTreeMap<Coordinate, Computed>,
    visiting: &mut BTreeSet<Coordinate>,
    budget: &mut Budget,
) -> Computed {
    if let Some(value) = memo.get(&coord) {
        return value.clone();
    }
    if visiting.contains(&coord) {
        return Computed::Error;
    }
    if visiting.len() >= 64 || budget.nodes >= 10_000 || budget.visits >= 100_000 {
        return Computed::Deferred;
    }
    budget.nodes += 1;
    visiting.insert(coord);
    let result = match cells.get(&coord) {
        None => Computed::Blank,
        Some(cell) => {
            if cell.complex {
                Computed::Deferred
            } else if let Some(formula) = &cell.formula {
                match formulas::parse(formula) {
                    Ok(expr) => {
                        let mut local_visits = 0;
                        let value = formulas::evaluate(
                            &expr,
                            &mut |c| compute(c, cells, memo, visiting, budget),
                            &mut local_visits,
                        );
                        budget.visits += local_visits;
                        if budget.visits > 100_000 {
                            Computed::Deferred
                        } else if value == Computed::Blank {
                            Computed::Number(0.0)
                        } else {
                            value
                        }
                    }
                    Err(_) => Computed::Deferred,
                }
            } else {
                match cell.value {
                    OfficeCellValue::Number(v) => Computed::Number(v),
                    OfficeCellValue::Blank => Computed::Blank,
                    _ => Computed::Deferred,
                }
            }
        }
    };
    visiting.remove(&coord);
    memo.insert(coord, result.clone());
    result
}
pub(super) fn calculated_values(
    package: &Package,
    sheet: &Sheet<'_>,
) -> Result<BTreeMap<Coordinate, Computed>> {
    let values = cells(package, sheet)?;
    let mut out = BTreeMap::new();
    let mut visiting = BTreeSet::new();
    let mut budget = Budget::default();
    for coord in values.keys() {
        let _ = compute(*coord, &values, &mut out, &mut visiting, &mut budget);
    }
    Ok(out)
}
pub(super) fn annotate_calculations(
    package: &Package,
    inventory: &mut OfficeInventory,
) -> Result<()> {
    for sheet in sheets(package)? {
        let calculated = calculated_values(package, &sheet)?;
        for target in &mut inventory.targets {
            if let OfficeTargetValue::Cell {
                sheet_id,
                address,
                formula: Some(_),
                calculation,
                ..
            } = &mut target.value
            {
                if *sheet_id == sheet.id {
                    *calculation = match calculated.get(&formulas::coordinate(address)?) {
                        Some(Computed::Number(_)) => OfficeCalculationState::Calculated,
                        Some(Computed::Error) => OfficeCalculationState::CalculationError,
                        _ => OfficeCalculationState::NeedsExternalRecalculation,
                    };
                }
            }
        }
    }
    Ok(())
}
pub(super) fn rectangle(value: &str) -> Result<(Coordinate, Coordinate)> {
    let (first, last) = value.split_once(':').unwrap_or((value, value));
    let (first, last) = (formulas::coordinate(first)?, formulas::coordinate(last)?);
    if first.0 > last.0 || first.1 > last.1 {
        return Err(invalid("reversed range"));
    }
    Ok((first, last))
}
fn contains(first: Coordinate, last: Coordinate, value: Coordinate) -> bool {
    (first.0..=last.0).contains(&value.0) && (first.1..=last.1).contains(&value.1)
}
