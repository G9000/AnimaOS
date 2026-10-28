//! Presentation text targets keep slide relationships, geometry and templates intact.
use super::{
    package::Package,
    types::*,
    word::{self, TextTarget},
    xml_spans::{Node, Patch, A, P, R},
};
use std::collections::{BTreeMap, BTreeSet};

struct Slide<'a> {
    id: String,
    path: String,
    node: &'a Node,
    order: &'a Node,
    ordinal: usize,
}
fn slides(package: &Package) -> Result<Vec<Slide<'_>>> {
    let main = package.xml(&package.main)?;
    if !main.is(P, "presentation") {
        return Err(OfficeEditError::InvalidPackage);
    }
    let list = main
        .child(P, "sldIdLst")
        .ok_or(OfficeEditError::InvalidPackage)?;
    if list.children.len() > 128 {
        return Err(OfficeEditError::ResourceLimit);
    }
    let mut ids = BTreeSet::new();
    let mut rels = BTreeSet::new();
    let mut out = Vec::new();
    for (ordinal, node) in list.children.iter().enumerate() {
        if !node.is(P, "sldId")
            || !ids.insert(
                node.required("id")?
                    .parse::<u32>()
                    .map_err(|_| OfficeEditError::InvalidPackage)?,
            )
        {
            return Err(OfficeEditError::InvalidPackage);
        }
        let id = node
            .attr_ns(R, "id")
            .ok_or(OfficeEditError::InvalidPackage)?;
        if !rels.insert(id) {
            return Err(OfficeEditError::InvalidPackage);
        }
        let relation = package.relation(&package.main, id)?;
        if relation.kind != format!("{R}/slide") || relation.external {
            return Err(OfficeEditError::InvalidPackage);
        }
        let slide = package.xml(&relation.target)?;
        if !slide.is(P, "sld") {
            return Err(OfficeEditError::InvalidPackage);
        }
        out.push(Slide {
            id: id.into(),
            path: relation.target.clone(),
            node: slide,
            order: node,
            ordinal,
        });
    }
    Ok(out)
}
fn text_targets<'a>(package: &'a Package, slide: &Slide<'a>) -> Result<Vec<TextTarget<'a>>> {
    let mut out = Vec::new();
    let mut ids = BTreeSet::new();
    let bytes = &package.part(&slide.path)?.bytes;
    for shape in slide
        .node
        .all(P, "sp")
        .into_iter()
        .chain(slide.node.all(P, "graphicFrame"))
    {
        let properties = shape.all(P, "cNvPr");
        if properties.len() != 1 {
            return Err(OfficeEditError::InvalidPackage);
        }
        let properties = properties[0];
        let id = properties.required("id")?;
        if !ids.insert(id) {
            return Err(OfficeEditError::InvalidPackage);
        }
        let label = properties.attr("name").unwrap_or("Text");
        let paragraphs = if shape.is(P, "sp") {
            shape
                .child(P, "txBody")
                .map(|n| n.all(A, "p"))
                .unwrap_or_default()
        } else {
            shape
                .all(A, "tc")
                .into_iter()
                .flat_map(|n| n.all(A, "p"))
                .collect()
        };
        for (i, paragraph) in paragraphs.into_iter().enumerate() {
            out.push(word::drawing_paragraph(
                paragraph,
                bytes,
                format!("text:{}:{id}:{i}", slide.id),
                format!("Slide {}: {label}, paragraph {}", slide.ordinal + 1, i + 1),
            ));
        }
    }
    Ok(out)
}
pub(super) fn inventory(package: &Package) -> Result<Vec<OfficeTarget>> {
    let mut out = Vec::new();
    for slide in slides(package)? {
        out.push(OfficeTarget {
            id: format!("slide:{}", slide.id),
            label: format!("Slide {}", slide.ordinal + 1),
            editable: true,
            value: OfficeTargetValue::Slide {
                ordinal: slide.ordinal as u32 + 1,
                hidden: slide.node.attr("show") == Some("0"),
            },
        });
        out.extend(text_targets(package, &slide)?.iter().map(word::project));
    }
    Ok(out)
}
pub(super) fn apply(
    package: &Package,
    operations: &[OfficeEditOperation],
) -> Result<(BTreeMap<String, Vec<Patch>>, Vec<OfficeChange>)> {
    let slides = slides(package)?;
    let mut patches: BTreeMap<String, Vec<Patch>> = BTreeMap::new();
    let mut changes = Vec::new();
    let mut claimed: BTreeMap<String, Vec<std::ops::Range<usize>>> = BTreeMap::new();
    let mut reordered = false;
    for (index, operation) in operations.iter().enumerate() {
        match operation {
            OfficeEditOperation::SlideReplaceText {
                target_id,
                start,
                end,
                expected_text,
                new_text,
            } => {
                let mut found = false;
                for slide in &slides {
                    let targets = text_targets(package, slide)?;
                    if let Some(target) = targets.iter().find(|t| t.id == *target_id) {
                        let edit = word::edit(target, *start, *end, expected_text, new_text)?;
                        word::claim(claimed.entry(slide.path.clone()).or_default(), &edit)?;
                        changes.push(OfficeChange {
                            operation_index: Some(index),
                            label: target.label.clone(),
                            before: expected_text.clone(),
                            after: new_text.clone(),
                            derived: false,
                        });
                        word::report_whitespace(&mut changes, &edit, index, &target.label);
                        patches
                            .entry(slide.path.clone())
                            .or_default()
                            .extend(edit.patches);
                        found = true;
                        break;
                    }
                }
                if !found {
                    return Err(OfficeEditError::StaleTarget);
                }
            }
            OfficeEditOperation::ReorderSlides { slide_ids } => {
                if reordered {
                    return Err(invalid("only one slide permutation is allowed"));
                }
                reordered = true;
                let current = slides
                    .iter()
                    .map(|s| format!("slide:{}", s.id))
                    .collect::<Vec<_>>();
                if slide_ids.len() != current.len()
                    || slide_ids.iter().collect::<BTreeSet<_>>()
                        != current.iter().collect::<BTreeSet<_>>()
                {
                    return Err(invalid(
                        "slide ordering must be a permutation of every existing slide",
                    ));
                }
                let bytes = &package.part(&package.main)?.bytes;
                let mut inner = String::new();
                for id in slide_ids {
                    let slide = slides
                        .iter()
                        .find(|s| format!("slide:{}", s.id) == *id)
                        .ok_or(OfficeEditError::StaleTarget)?;
                    inner.push_str(
                        std::str::from_utf8(&bytes[slide.order.span.clone()])
                            .map_err(|_| OfficeEditError::InvalidPackage)?,
                    );
                }
                let list = package
                    .xml(&package.main)?
                    .child(P, "sldIdLst")
                    .ok_or(OfficeEditError::InvalidPackage)?;
                patches
                    .entry(package.main.clone())
                    .or_default()
                    .push(Patch {
                        span: list.inner.clone(),
                        replacement: inner,
                    });
                changes.push(OfficeChange {
                    operation_index: Some(index),
                    label: "Slide order (custom shows unchanged)".into(),
                    before: current.join(", "),
                    after: slide_ids.join(", "),
                    derived: false,
                });
            }
            _ => return Err(invalid("operation does not match presentation format")),
        }
    }
    Ok((patches, changes))
}
