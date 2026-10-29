//! Conservative source spans for literal HTML text edits; no DOM or active content runs here.
use super::types::{
    invalid, OfficeChange, OfficeEditError as Error, OfficeEditOperation, OfficeEditReview,
    OfficeEditSpec, OfficeFormat, OfficeInventory, OfficePreservation, OfficeTarget,
    OfficeTargetValue, PreparedOfficeEdit, Result,
};
use std::ops::Range;

const MAX_SOURCE: usize = 1024 * 1024;
const MAX_TEXT: usize = 8 * 1024;
const TARGET_BYTES: usize = 1024;
const MAX_TARGETS: usize = 4096;

fn source_text(source: &[u8]) -> Result<&str> {
    if source.is_empty() || source.len() > MAX_SOURCE {
        return Err(Error::ResourceLimit);
    }
    let text = std::str::from_utf8(source).map_err(|_| invalid("HTML must be UTF-8"))?;
    if text.contains('\0') {
        return Err(invalid("HTML contains a null character"));
    }
    Ok(text)
}

fn raw(name: &str) -> bool {
    matches!(
        name,
        "script" | "style" | "textarea" | "title" | "xmp" | "iframe" | "noembed" | "noframes"
    )
}
fn suppressed(name: &str) -> bool {
    matches!(name, "template" | "noscript" | "svg" | "math" | "object")
}
fn void(name: &str) -> bool {
    matches!(
        name,
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}
fn whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n' | 0x0c)
}
fn name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b':')
}

struct Tag {
    end: usize,
    name: String,
    closing: bool,
    empty: bool,
}

fn tag(bytes: &[u8], start: usize) -> Result<Tag> {
    let mut at = start + 1;
    let closing = bytes.get(at) == Some(&b'/');
    at += usize::from(closing);
    let name_start = at;
    if !bytes.get(at).is_some_and(u8::is_ascii_alphabetic) {
        return Err(invalid("HTML tag syntax is unsupported"));
    }
    while bytes.get(at).is_some_and(|byte| name_byte(*byte)) {
        at += 1;
    }
    let name = std::str::from_utf8(&bytes[name_start..at])
        .map_err(|_| Error::InvalidPackage)?
        .to_ascii_lowercase();
    loop {
        let before = at;
        while bytes.get(at).is_some_and(|byte| whitespace(*byte)) {
            at += 1;
        }
        if bytes.get(at) == Some(&b'>') {
            return Ok(Tag {
                end: at + 1,
                name,
                closing,
                empty: false,
            });
        }
        if !closing && bytes.get(at..at + 2) == Some(b"/>") {
            return Ok(Tag {
                end: at + 2,
                name,
                closing,
                empty: true,
            });
        }
        if closing || before == at {
            return Err(invalid("HTML attributes or closing tag are malformed"));
        }
        let attribute = at;
        while bytes.get(at).is_some_and(|byte| {
            !whitespace(*byte) && !matches!(*byte, b'=' | b'>' | b'/' | b'\'' | b'"' | b'<' | b'`')
        }) {
            at += 1;
        }
        if attribute == at {
            return Err(invalid("HTML attribute syntax is unsupported"));
        }
        let attribute_end = at;
        while bytes.get(at).is_some_and(|byte| whitespace(*byte)) {
            at += 1;
        }
        if bytes.get(at) != Some(&b'=') {
            at = attribute_end;
            continue;
        }
        at += 1;
        while bytes.get(at).is_some_and(|byte| whitespace(*byte)) {
            at += 1;
        }
        if let Some(quote @ (b'\'' | b'"')) = bytes.get(at).copied() {
            at += 1;
            while bytes.get(at).is_some_and(|byte| *byte != quote) {
                at += 1;
            }
            if bytes.get(at) != Some(&quote) {
                return Err(invalid("HTML attribute quote is unclosed"));
            }
            at += 1;
        } else {
            let value = at;
            while bytes
                .get(at)
                .is_some_and(|byte| !whitespace(*byte) && *byte != b'>')
            {
                if matches!(bytes[at], b'<' | b'\'' | b'"' | b'=' | b'`') {
                    return Err(invalid("HTML unquoted attribute is malformed"));
                }
                at += 1;
            }
            if value == at {
                return Err(invalid("HTML attribute value is missing"));
            }
        }
    }
}

fn spans(text: &str) -> Result<Vec<Range<usize>>> {
    let bytes = text.as_bytes();
    let mut at = 0;
    let mut stack: Vec<(String, bool)> = Vec::new();
    let mut result = Vec::new();
    while at < bytes.len() {
        if bytes[at] != b'<' {
            let start = at;
            while at < bytes.len() && bytes[at] != b'<' {
                at += 1;
            }
            if !stack.iter().any(|(_, blocked)| *blocked) && !text[start..at].trim().is_empty() {
                result.push(start..at);
            }
            continue;
        }
        if bytes.get(at..at + 4) == Some(b"<!--") {
            at += text[at + 4..]
                .find("-->")
                .ok_or_else(|| invalid("HTML comment is unclosed"))?
                + 7;
            continue;
        }
        if bytes
            .get(at..at + 9)
            .is_some_and(|value| value.eq_ignore_ascii_case(b"<!doctype"))
        {
            let end = text[at..]
                .find('>')
                .ok_or_else(|| invalid("HTML doctype is unclosed"))?
                + at;
            if !text[at..=end].eq_ignore_ascii_case("<!doctype html>") || !stack.is_empty() {
                return Err(invalid("Only a plain HTML doctype is supported"));
            }
            at = end + 1;
            continue;
        }
        let current = tag(bytes, at)?;
        at = current.end;
        if current.name == "plaintext" {
            return Err(invalid("HTML plaintext context cannot be edited"));
        }
        if current.closing {
            if stack.pop().as_ref().map(|(name, _)| name.as_str()) != Some(current.name.as_str()) {
                return Err(invalid("HTML tags must be explicitly balanced"));
            }
        } else if raw(&current.name) {
            if current.empty {
                return Err(invalid(
                    "HTML raw text elements require an explicit end tag",
                ));
            }
            // The final end tag conservatively excludes script escaped states and fake inner ends.
            let closing = format!("</{}", current.name);
            let rest = &bytes[at..];
            let end = rest
                .windows(closing.len())
                .rposition(|window| window.eq_ignore_ascii_case(closing.as_bytes()))
                .map(|offset| at + offset)
                .ok_or_else(|| invalid("HTML raw text element is unclosed"))?;
            let end_tag = tag(bytes, end)?;
            if !end_tag.closing || end_tag.name != current.name {
                return Err(invalid("HTML raw text end tag is malformed"));
            }
            at = end_tag.end;
        } else if !void(&current.name) && !current.empty {
            if stack.len() >= 128 {
                return Err(Error::ResourceLimit);
            }
            let blocked = suppressed(&current.name);
            stack.push((current.name, blocked));
        }
    }
    if !stack.is_empty() {
        return Err(invalid("HTML tags must be explicitly balanced"));
    }
    Ok(result)
}

fn entity_boundary(text: &str, boundary: usize) -> bool {
    let bytes = text.as_bytes();
    if boundary == 0 || boundary == bytes.len() {
        return true;
    }
    let mut start = boundary;
    while start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'#') {
        start -= 1;
    }
    !(start > 0
        && bytes[start - 1] == b'&'
        && (bytes[boundary].is_ascii_alphanumeric() || matches!(bytes[boundary], b'#' | b';')))
}

pub(super) fn inventory(source: &[u8]) -> Result<OfficeInventory> {
    let text = source_text(source)?;
    let mut targets = Vec::new();
    for span in spans(text)? {
        let mut start = span.start;
        while start < span.end {
            let mut end = (start + TARGET_BYTES).min(span.end);
            while end > start
                && (!text.is_char_boundary(end)
                    || !entity_boundary(&text[span.clone()], end - span.start))
            {
                end -= 1;
            }
            if end == start || targets.len() >= MAX_TARGETS {
                return Err(Error::ResourceLimit);
            }
            targets.push(OfficeTarget {
                id: format!("html:{start}:{end}"),
                label: format!("Text section {}", targets.len() + 1),
                editable: true,
                value: OfficeTargetValue::HtmlText {
                    start_utf8: start as u32,
                    end_utf8: end as u32,
                    text: text[start..end].into(),
                },
            });
            start = end;
        }
    }
    let inventory = OfficeInventory {
        version: 1,
        format: OfficeFormat::Html,
        targets,
        retained_hyperlinks: Vec::new(),
        limitations: vec![
            "Text content only. Entities such as &amp; remain in their original source spelling.".into(),
            "Markup, attributes, comments, scripts, styles, templates and foreign content are preserved without rendering, fetching or executing them.".into(),
            "Explicitly balanced HTML is required. This content review does not verify browser layout or executable behavior.".into(),
        ],
    };
    if serde_json::to_vec(&inventory)
        .map_err(|_| Error::VerificationFailed)?
        .len()
        > 500 * 1024
    {
        return Err(Error::ResourceLimit);
    }
    Ok(inventory)
}

pub(super) fn check_text(source: &[u8], mut policy: impl FnMut(&str) -> bool) -> Result<bool> {
    Ok(policy(source_text(source)?))
}

fn escape(text: &str) -> String {
    let mut escaped = String::new();
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            other => escaped.push(other),
        }
    }
    escaped
}

pub(super) fn prepare(source: &[u8], edits: &OfficeEditSpec) -> Result<PreparedOfficeEdit> {
    let text = source_text(source)?;
    let admitted = spans(text)?;
    let before = inventory(source)?;
    let mut patches = Vec::new();
    let mut changes = Vec::new();
    for (index, operation) in edits.operations.iter().enumerate() {
        let OfficeEditOperation::HtmlReplaceText {
            start_utf8,
            end_utf8,
            expected_text,
            new_text,
        } = operation
        else {
            return Err(invalid("HTML supports only literal text replacement"));
        };
        let start = *start_utf8 as usize;
        let end = *end_utf8 as usize;
        if expected_text.is_empty()
            || expected_text.len() > MAX_TEXT
            || new_text.len() > MAX_TEXT
            || new_text.contains('\0')
        {
            return Err(Error::ResourceLimit);
        }
        let span = admitted
            .iter()
            .find(|span| span.start <= start && end <= span.end && start < end)
            .ok_or(Error::StaleTarget)?;
        if text.get(start..end) != Some(expected_text.as_str())
            || !entity_boundary(&text[span.clone()], start - span.start)
            || !entity_boundary(&text[span.clone()], end - span.start)
        {
            return Err(Error::StaleTarget);
        }
        let escaped = escape(new_text);
        changes.push(OfficeChange {
            operation_index: Some(index),
            label: "Selected HTML text".into(),
            before: expected_text.clone(),
            after: escaped.clone(),
            derived: false,
        });
        patches.push((start..end, escaped));
    }
    patches.sort_by_key(|(span, _)| span.start);
    let mut bytes = Vec::new();
    let mut previous = 0;
    for (span, replacement) in &patches {
        if span.start < previous {
            return Err(invalid("HTML text replacements overlap"));
        }
        bytes.extend_from_slice(&source[previous..span.start]);
        bytes.extend_from_slice(replacement.as_bytes());
        if bytes.len() > MAX_SOURCE {
            return Err(Error::ResourceLimit);
        }
        previous = span.end;
    }
    bytes.extend_from_slice(&source[previous..]);
    if bytes.len() > MAX_SOURCE {
        return Err(Error::ResourceLimit);
    }
    let after = inventory(&bytes)?;
    let review = OfficeEditReview {
        version: 1,
        before,
        after,
        changes,
        preservation: OfficePreservation {
            unchanged_parts: 0,
            edited_parts: 1,
            unchanged_bytes_verified: true,
            container_normalized: false,
        },
    };
    if serde_json::to_vec(&review)
        .map_err(|_| Error::VerificationFailed)?
        .len()
        > 64 * 1024
    {
        return Err(Error::ResourceLimit);
    }
    Ok(PreparedOfficeEdit { bytes, review })
}
