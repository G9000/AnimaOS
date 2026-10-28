//! Range edits preserve original run properties and unsupported paragraph structures.
use super::{
    package::Package,
    types::*,
    xml_spans::{escape, Node, Patch, A, W},
};
use std::{collections::BTreeMap, ops::Range};

pub(super) struct TextTarget<'a> {
    pub id: String,
    pub label: String,
    pub pieces: Vec<&'a Node>,
    pub text: String,
    pub styles: Vec<Vec<u8>>,
    pub editable: bool,
    bytes: &'a [u8],
}
pub(super) fn paragraph<'a>(
    node: &'a Node,
    bytes: &'a [u8],
    ns: &str,
    id: String,
    label: String,
) -> TextTarget<'a> {
    let mut result = TextTarget {
        id,
        label,
        pieces: Vec::new(),
        text: String::new(),
        styles: Vec::new(),
        editable: true,
        bytes,
    };
    for child in &node.children {
        if child.is(ns, "pPr") || child.is(ns, "endParaRPr") {
            continue;
        }
        if !child.is(ns, "r") {
            result.editable = false;
            for text in child.all(ns, "t") {
                result.text.push_str(&text.text);
            }
            continue;
        }
        let style = child
            .child(ns, "rPr")
            .map_or(Vec::new(), |n| bytes[n.span.clone()].to_vec());
        for piece in &child.children {
            if piece.is(ns, "rPr") {
                continue;
            }
            if piece.is(ns, "t") && piece.children.is_empty() {
                if bytes[piece.inner.clone()]
                    .windows(9)
                    .any(|window| window == b"<![CDATA[")
                {
                    result.editable = false;
                }
                result.text.push_str(&piece.text);
                result.pieces.push(piece);
                result.styles.push(style.clone());
            } else {
                result.editable = false;
            }
        }
    }
    if result.pieces.is_empty() {
        result.editable = false;
    }
    result
}
pub(super) fn targets(package: &Package) -> Result<Vec<TextTarget<'_>>> {
    let root = package.xml(&package.main)?;
    if !root.is(W, "document") {
        return Err(OfficeEditError::InvalidPackage);
    }
    let body = root
        .child(W, "body")
        .ok_or(OfficeEditError::InvalidPackage)?;
    let bytes = &package.part(&package.main)?.bytes;
    let mut nodes = Vec::new();
    collect(body, &mut nodes);
    Ok(nodes
        .into_iter()
        .enumerate()
        .map(|(i, node)| {
            paragraph(
                node,
                bytes,
                W,
                format!("word:{i}"),
                format!("Paragraph {}", i + 1),
            )
        })
        .collect())
}
fn collect<'a>(node: &'a Node, out: &mut Vec<&'a Node>) {
    if node.is(W, "p") {
        out.push(node);
        return;
    }
    if node.is(W, "body") || node.is(W, "tbl") || node.is(W, "tr") || node.is(W, "tc") {
        for child in &node.children {
            collect(child, out);
        }
    }
}
pub(super) fn inventory(package: &Package) -> Result<Vec<OfficeTarget>> {
    Ok(targets(package)?.iter().map(project).collect())
}
pub(super) fn project(target: &TextTarget<'_>) -> OfficeTarget {
    OfficeTarget {
        id: target.id.clone(),
        label: target.label.clone(),
        editable: target.editable,
        value: OfficeTargetValue::Text {
            text: target.text.clone(),
        },
    }
}

pub(super) struct TextEdit {
    pub patches: Vec<Patch>,
    pub touched: Vec<Range<usize>>,
    pub whitespace_changes: usize,
}

/// Map decoded Unicode scalars to their exact original XML byte ranges.
fn text_ranges(piece: &Node, bytes: &[u8]) -> Result<Vec<Range<usize>>> {
    let raw = std::str::from_utf8(&bytes[piece.inner.clone()])
        .map_err(|_| OfficeEditError::InvalidPackage)?;
    let mut ranges = Vec::new();
    let mut decoded = String::new();
    let mut pos = 0;
    while pos < raw.len() {
        let rest = &raw[pos..];
        if rest.starts_with("<!--") {
            let end = rest.find("-->").ok_or(OfficeEditError::InvalidPackage)?;
            pos += end + 3;
            continue;
        }
        if rest.starts_with('<') {
            return Err(unsupported("CDATA or markup inside edited text"));
        }
        let (character, length) = if let Some(reference) = rest.strip_prefix('&') {
            let end = reference.find(';').ok_or(OfficeEditError::InvalidPackage)?;
            let token = &reference[..end];
            let character = match token {
                "amp" => '&',
                "lt" => '<',
                "gt" => '>',
                "quot" => '"',
                "apos" => '\'',
                _ => {
                    let number = if let Some(hex) = token.strip_prefix("#x") {
                        u32::from_str_radix(hex, 16)
                    } else if let Some(decimal) = token.strip_prefix('#') {
                        decimal.parse::<u32>()
                    } else {
                        return Err(OfficeEditError::InvalidPackage);
                    }
                    .map_err(|_| OfficeEditError::InvalidPackage)?;
                    char::from_u32(number).ok_or(OfficeEditError::InvalidPackage)?
                }
            };
            (character, end + 2)
        } else {
            let character = rest.chars().next().ok_or(OfficeEditError::InvalidPackage)?;
            (character, character.len_utf8())
        };
        decoded.push(character);
        ranges.push(piece.inner.start + pos..piece.inner.start + pos + length);
        pos += length;
    }
    if decoded != piece.text {
        return Err(OfficeEditError::InvalidPackage);
    }
    Ok(ranges)
}

/// Modify only the value of xml:space, or insert the one required attribute.
fn whitespace_patch(piece: &Node, bytes: &[u8]) -> Result<Patch> {
    let tag = bytes
        .get(piece.span.start..piece.inner.start)
        .ok_or(OfficeEditError::InvalidPackage)?;
    let mut i = 1;
    while i < tag.len() && !tag[i].is_ascii_whitespace() && !matches!(tag[i], b'/' | b'>') {
        i += 1;
    }
    while i < tag.len() {
        while i < tag.len() && tag[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= tag.len() || matches!(tag[i], b'/' | b'>') {
            break;
        }
        let start = i;
        while i < tag.len()
            && !tag[i].is_ascii_whitespace()
            && !matches!(tag[i], b'=' | b'/' | b'>')
        {
            i += 1;
        }
        let name = &tag[start..i];
        while i < tag.len() && tag[i].is_ascii_whitespace() {
            i += 1;
        }
        if tag.get(i) != Some(&b'=') {
            return Err(OfficeEditError::InvalidPackage);
        }
        i += 1;
        while i < tag.len() && tag[i].is_ascii_whitespace() {
            i += 1;
        }
        let quote = *tag.get(i).ok_or(OfficeEditError::InvalidPackage)?;
        if !matches!(quote, b'\'' | b'"') {
            return Err(OfficeEditError::InvalidPackage);
        }
        i += 1;
        let value_start = i;
        while i < tag.len() && tag[i] != quote {
            i += 1;
        }
        if i >= tag.len() {
            return Err(OfficeEditError::InvalidPackage);
        }
        if name == b"xml:space" {
            return Ok(Patch {
                span: piece.span.start + value_start..piece.span.start + i,
                replacement: "preserve".into(),
            });
        }
        i += 1;
    }
    let mut end = tag
        .len()
        .checked_sub(1)
        .ok_or(OfficeEditError::InvalidPackage)?;
    if tag.get(end) != Some(&b'>') {
        return Err(OfficeEditError::InvalidPackage);
    }
    if end > 0 && tag[end - 1] == b'/' {
        end -= 1;
    }
    Ok(Patch {
        span: piece.span.start + end..piece.span.start + end,
        replacement: " xml:space=\"preserve\"".into(),
    })
}

pub(super) fn edit(
    target: &TextTarget<'_>,
    start: u32,
    end: u32,
    expected: &str,
    new: &str,
) -> Result<TextEdit> {
    if !target.editable {
        return Err(unsupported("nonplain text paragraph"));
    }
    if new.len() > 8192
        || new
            .chars()
            .any(|c| c.is_control() || !crate::docx::valid_xml_character(c))
    {
        return Err(invalid("text contains controls or exceeds its limit"));
    }
    let chars = target.text.chars().collect::<Vec<_>>();
    let (start, end) = (start as usize, end as usize);
    if start > end || end > chars.len() || chars[start..end].iter().collect::<String>() != expected
    {
        return Err(OfficeEditError::StaleTarget);
    }
    let mut offset = 0;
    let mut affected = Vec::new();
    for (i, piece) in target.pieces.iter().enumerate() {
        let len = piece.text.chars().count();
        if (start < offset + len && end > offset)
            || (start == end && start >= offset && start <= offset + len && affected.is_empty())
        {
            affected.push((i, offset));
        }
        offset += len;
    }
    let Some(&(first, _)) = affected.first() else {
        return Err(invalid("empty text target"));
    };
    if affected
        .iter()
        .any(|(i, _)| target.styles[*i] != target.styles[first])
    {
        return Err(unsupported("replacement crosses different text styles"));
    }
    let mut patches = Vec::new();
    let mut touched = Vec::new();
    let mut whitespace_changes = 0;
    for (n, (i, offset)) in affected.iter().enumerate() {
        let piece = target.pieces[*i];
        let chars = piece.text.chars().collect::<Vec<_>>();
        let local_start = start.saturating_sub(*offset).min(chars.len());
        let local_end = end.saturating_sub(*offset).min(chars.len());
        let replacement = format!(
            "{}{}{}",
            chars[..local_start].iter().collect::<String>(),
            if n == 0 { new } else { "" },
            chars[local_end..].iter().collect::<String>()
        );
        touched.push(piece.span.clone());
        let ranges = text_ranges(piece, target.bytes)?;
        let is_space = |c: char| matches!(c, ' ' | '\t' | '\r' | '\n');
        let leading = replacement.chars().next().is_some_and(is_space) && local_start == 0;
        let trailing =
            replacement.chars().next_back().is_some_and(is_space) && local_end == chars.len();
        if (leading || trailing)
            && piece.attr_ns("http://www.w3.org/XML/1998/namespace", "space") != Some("preserve")
        {
            let old_unselected_leading =
                local_start > 0 && chars.first().copied().is_some_and(is_space);
            let old_unselected_trailing =
                local_end < chars.len() && chars.last().copied().is_some_and(is_space);
            if old_unselected_leading || old_unselected_trailing {
                return Err(unsupported(
                    "whitespace preservation would change untouched text",
                ));
            }
            patches.push(whitespace_patch(piece, target.bytes)?);
            whitespace_changes += 1;
        }
        let insert = if n == 0 { escape(new) } else { String::new() };
        if piece.inner.start == piece.span.end {
            let closing = piece
                .span
                .end
                .checked_sub(2)
                .ok_or(OfficeEditError::InvalidPackage)?;
            patches.push(Patch {
                span: closing..piece.span.end,
                replacement: format!(">{insert}</{}>", piece.raw_name),
            });
        } else if local_start == local_end {
            let at = ranges.get(local_start).map_or_else(
                || ranges.last().map_or(piece.inner.start, |r| r.end),
                |r| r.start,
            );
            patches.push(Patch {
                span: at..at,
                replacement: insert,
            });
        } else {
            let mut selected: Vec<Range<usize>> = Vec::new();
            for range in &ranges[local_start..local_end] {
                if let Some(previous) = selected
                    .last_mut()
                    .filter(|previous| previous.end == range.start)
                {
                    previous.end = range.end;
                } else {
                    selected.push(range.clone());
                }
            }
            for (index, span) in selected.into_iter().enumerate() {
                patches.push(Patch {
                    span,
                    replacement: if index == 0 {
                        insert.clone()
                    } else {
                        String::new()
                    },
                });
            }
        }
    }
    Ok(TextEdit {
        patches,
        touched,
        whitespace_changes,
    })
}

pub(super) fn claim(claimed: &mut Vec<Range<usize>>, edit: &TextEdit) -> Result<()> {
    if edit.touched.iter().any(|range| {
        claimed
            .iter()
            .any(|prior| range.start < prior.end && prior.start < range.end)
    }) {
        return Err(invalid("multiple edits touch the same text run"));
    }
    claimed.extend(edit.touched.iter().cloned());
    Ok(())
}

pub(super) fn report_whitespace(
    changes: &mut Vec<OfficeChange>,
    edit: &TextEdit,
    index: usize,
    label: &str,
) {
    for _ in 0..edit.whitespace_changes {
        changes.push(OfficeChange {
            operation_index: Some(index),
            label: format!("{label}: whitespace preservation"),
            before: "Default text whitespace".into(),
            after: "Preserve text whitespace".into(),
            derived: true,
        });
    }
}
pub(super) fn apply(
    package: &Package,
    operations: &[OfficeEditOperation],
) -> Result<(BTreeMap<String, Vec<Patch>>, Vec<OfficeChange>)> {
    let targets = targets(package)?;
    let mut patches = Vec::new();
    let mut changes = Vec::new();
    let mut claimed = Vec::new();
    for (index, operation) in operations.iter().enumerate() {
        let OfficeEditOperation::WordReplaceText {
            target_id,
            start,
            end,
            expected_text,
            new_text,
        } = operation
        else {
            return Err(invalid("operation does not match Word format"));
        };
        let target = targets
            .iter()
            .find(|t| t.id == *target_id)
            .ok_or(OfficeEditError::StaleTarget)?;
        let edit = edit(target, *start, *end, expected_text, new_text)?;
        claim(&mut claimed, &edit)?;
        changes.push(OfficeChange {
            operation_index: Some(index),
            label: target.label.clone(),
            before: expected_text.clone(),
            after: new_text.clone(),
            derived: false,
        });
        report_whitespace(&mut changes, &edit, index, &target.label);
        patches.extend(edit.patches);
    }
    Ok((BTreeMap::from([(package.main.clone(), patches)]), changes))
}

// DrawingML uses the same plain text range rules but different paragraph properties.
pub(super) fn drawing_paragraph<'a>(
    node: &'a Node,
    bytes: &'a [u8],
    id: String,
    label: String,
) -> TextTarget<'a> {
    paragraph(node, bytes, A, id, label)
}
