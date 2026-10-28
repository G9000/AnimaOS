//! Namespace-aware XML indexing with exact original byte ranges.
use super::types::{unsupported, OfficeEditError as Error, Result};
use quick_xml::{
    events::{BytesStart, Event},
    name::ResolveResult,
    NsReader, XmlVersion,
};
use std::{collections::BTreeMap, ops::Range};

pub(super) const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
pub(super) const S: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
pub(super) const P: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";
pub(super) const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
pub(super) const C: &str = "http://schemas.openxmlformats.org/drawingml/2006/chart";
pub(super) const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
pub(super) const REL: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
pub(super) const CT: &str = "http://schemas.openxmlformats.org/package/2006/content-types";

#[derive(Debug, Clone)]
pub(super) struct Node {
    pub ns: String,
    pub name: String,
    pub raw_name: String,
    pub attrs: BTreeMap<(String, String), String>,
    pub raw_attrs: Vec<(String, String)>,
    pub span: Range<usize>,
    pub inner: Range<usize>,
    pub children: Vec<Node>,
    pub text: String,
}
impl Node {
    pub fn is(&self, ns: &str, name: &str) -> bool {
        self.ns == ns && self.name == name
    }
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attr_ns("", name)
    }
    pub fn attr_ns(&self, ns: &str, name: &str) -> Option<&str> {
        self.attrs
            .get(&(ns.into(), name.into()))
            .map(String::as_str)
    }
    pub fn required(&self, name: &str) -> Result<&str> {
        self.attr(name).ok_or(Error::InvalidPackage)
    }
    pub fn child(&self, ns: &str, name: &str) -> Option<&Node> {
        self.children.iter().find(|n| n.is(ns, name))
    }
    pub fn all<'a>(&'a self, ns: &str, name: &str) -> Vec<&'a Node> {
        let mut out = Vec::new();
        self.collect(ns, name, &mut out);
        out
    }
    fn collect<'a>(&'a self, ns: &str, name: &str, out: &mut Vec<&'a Node>) {
        if self.is(ns, name) {
            out.push(self);
        }
        for child in &self.children {
            child.collect(ns, name, out);
        }
    }
    pub fn element(&self, inner: &str, updates: &[(&str, Option<&str>)]) -> String {
        let mut attrs = self.raw_attrs.clone();
        for (key, value) in updates {
            attrs.retain(|(name, _)| name != key);
            if let Some(value) = value {
                attrs.push(((*key).into(), (*value).into()));
            }
        }
        format!(
            "<{}{}>{inner}</{}>",
            self.raw_name,
            attrs
                .iter()
                .map(|(k, v)| format!(" {k}=\"{}\"", escape(v)))
                .collect::<String>(),
            self.raw_name
        )
    }
}

pub(super) fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
        .replace('\r', "&#13;")
        .replace('\n', "&#10;")
        .replace('\t', "&#9;")
}

fn resolved(value: ResolveResult<'_>) -> Result<String> {
    match value {
        ResolveResult::Unbound => Ok(String::new()),
        ResolveResult::Bound(ns) => std::str::from_utf8(ns.as_ref())
            .map(str::to_owned)
            .map_err(|_| Error::InvalidPackage),
        _ => Err(Error::InvalidPackage),
    }
}

fn allowed_ns(ns: &str) -> bool {
    matches!(
        ns,
        "" | W
            | S
            | P
            | A
            | C
            | R
            | REL
            | CT
            | "http://www.w3.org/XML/1998/namespace"
            | "http://www.w3.org/2001/XMLSchema-instance"
            | "http://www.w3.org/2001/XMLSchema"
            | "http://schemas.openxmlformats.org/markup-compatibility/2006"
            | "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"
            | "http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing"
            | "http://schemas.openxmlformats.org/drawingml/2006/picture"
            | "http://schemas.openxmlformats.org/package/2006/metadata/core-properties"
            | "http://schemas.openxmlformats.org/officeDocument/2006/extended-properties"
            | "http://schemas.openxmlformats.org/officeDocument/2006/custom-properties"
            | "http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes"
            | "http://purl.org/dc/elements/1.1/"
            | "http://purl.org/dc/terms/"
            | "http://purl.org/dc/dcmitype/"
    )
}

fn node(reader: &NsReader<&[u8]>, tag: &BytesStart<'_>, start: usize, end: usize) -> Result<Node> {
    fn name_valid(raw: &[u8]) -> bool {
        raw.split(|b| *b == b':').count() <= 2
            && raw.split(|b| *b == b':').all(|part| {
                part.first()
                    .is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_')
                    && part
                        .iter()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.'))
            })
    }
    if !name_valid(tag.name().as_ref()) {
        return Err(Error::InvalidPackage);
    }
    let (ns, name) = reader.resolver().resolve_element(tag.name());
    let ns = resolved(ns)?;
    if !allowed_ns(&ns) {
        return Err(unsupported("XML namespace"));
    }
    let mut result = Node {
        ns,
        name: std::str::from_utf8(name.as_ref())
            .map_err(|_| Error::InvalidPackage)?
            .into(),
        raw_name: std::str::from_utf8(tag.name().as_ref())
            .map_err(|_| Error::InvalidPackage)?
            .into(),
        attrs: BTreeMap::new(),
        raw_attrs: Vec::new(),
        span: start..end,
        inner: end..end,
        children: Vec::new(),
        text: String::new(),
    };
    for attr in tag.attributes() {
        let attr = attr.map_err(|_| Error::InvalidPackage)?;
        if !name_valid(attr.key.as_ref()) {
            return Err(Error::InvalidPackage);
        }
        let raw = std::str::from_utf8(attr.key.as_ref())
            .map_err(|_| Error::InvalidPackage)?
            .to_owned();
        let value = attr
            .decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder())
            .map_err(|_| Error::InvalidPackage)?
            .into_owned();
        if raw == "xmlns" || raw.starts_with("xmlns:") {
            if !allowed_ns(&value) {
                return Err(unsupported("namespace declaration"));
            }
        } else {
            let (ns, name) = reader.resolver().resolve_attribute(attr.key);
            let key = (
                resolved(ns)?,
                std::str::from_utf8(name.as_ref())
                    .map_err(|_| Error::InvalidPackage)?
                    .to_owned(),
            );
            if result.attrs.insert(key, value.clone()).is_some() {
                return Err(Error::InvalidPackage);
            }
        }
        result.raw_attrs.push((raw, value));
    }
    Ok(result)
}

pub(super) fn parse(bytes: &[u8], events: &mut usize) -> Result<Node> {
    crate::docx::validate_xml_characters(bytes).map_err(|_| Error::InvalidPackage)?;
    let mut reader = NsReader::from_reader(bytes);
    reader.config_mut().check_comments = true;
    let mut stack: Vec<Node> = Vec::new();
    let mut root = None;
    let mut count = 0;
    loop {
        let start = reader.buffer_position() as usize;
        let event = reader.read_event().map_err(|_| Error::InvalidPackage)?;
        let end = reader.buffer_position() as usize;
        *events += 1;
        count += 1;
        if *events > 500_000 {
            return Err(Error::ResourceLimit);
        }
        match event {
            Event::Start(tag) | Event::Empty(tag) => {
                let empty = bytes.get(end.saturating_sub(2)..end) == Some(b"/>");
                if stack.len() >= 64 || (stack.is_empty() && root.is_some()) {
                    return Err(Error::InvalidPackage);
                }
                let value = node(&reader, &tag, start, end)?;
                if empty {
                    attach(value, &mut stack, &mut root)?;
                } else {
                    stack.push(value);
                }
            }
            Event::End(_) => {
                let mut value = stack.pop().ok_or(Error::InvalidPackage)?;
                value.inner.end = start;
                value.span.end = end;
                attach(value, &mut stack, &mut root)?;
            }
            Event::Text(text) => {
                if text.as_ref().windows(3).any(|x| x == b"]]>") {
                    return Err(Error::InvalidPackage);
                }
                append(
                    &mut stack,
                    &text.decode().map_err(|_| Error::InvalidPackage)?,
                )?;
            }
            Event::CData(text) => {
                if stack.is_empty() {
                    return Err(Error::InvalidPackage);
                }
                append(
                    &mut stack,
                    &text.decode().map_err(|_| Error::InvalidPackage)?,
                )?;
            }
            Event::GeneralRef(reference) => {
                if stack.is_empty() {
                    return Err(Error::InvalidPackage);
                }
                let text = if let Some(ch) = reference
                    .resolve_char_ref()
                    .map_err(|_| Error::InvalidPackage)?
                {
                    ch.to_string()
                } else {
                    match reference
                        .decode()
                        .map_err(|_| Error::InvalidPackage)?
                        .as_ref()
                    {
                        "amp" => "&",
                        "lt" => "<",
                        "gt" => ">",
                        "quot" => "\"",
                        "apos" => "'",
                        _ => return Err(Error::InvalidPackage),
                    }
                    .into()
                };
                append(&mut stack, &text)?;
            }
            Event::Decl(decl) => {
                if count != 1 {
                    return Err(Error::InvalidPackage);
                }
                crate::docx::validate_declaration(&decl).map_err(|_| Error::InvalidPackage)?;
            }
            Event::DocType(_) | Event::PI(_) => {
                return Err(unsupported("XML declarations or processing instructions"))
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !stack.is_empty() {
        return Err(Error::InvalidPackage);
    }
    root.ok_or(Error::InvalidPackage)
}
fn append(stack: &mut [Node], text: &str) -> Result<()> {
    if !text.chars().all(crate::docx::valid_xml_character) {
        return Err(Error::InvalidPackage);
    }
    if let Some(node) = stack.last_mut() {
        node.text.push_str(text);
    } else if !text.trim().is_empty() {
        return Err(Error::InvalidPackage);
    }
    Ok(())
}
fn attach(value: Node, stack: &mut [Node], root: &mut Option<Node>) -> Result<()> {
    if let Some(parent) = stack.last_mut() {
        parent.children.push(value);
    } else if root.replace(value).is_some() {
        return Err(Error::InvalidPackage);
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub(super) struct Patch {
    pub span: Range<usize>,
    pub replacement: String,
}
pub(super) fn patch(bytes: &[u8], patches: &mut [Patch]) -> Result<Vec<u8>> {
    patches.sort_by_key(|p| p.span.start);
    let mut out = Vec::new();
    let mut offset = 0;
    for p in patches {
        if p.span.start < offset || p.span.end < p.span.start || p.span.end > bytes.len() {
            return Err(super::types::invalid("overlapping or invalid edits"));
        }
        out.extend_from_slice(&bytes[offset..p.span.start]);
        out.extend_from_slice(p.replacement.as_bytes());
        offset = p.span.end;
        if out.len() > 4 * 1024 * 1024 {
            return Err(Error::ResourceLimit);
        }
    }
    out.extend_from_slice(&bytes[offset..]);
    if out.len() > 4 * 1024 * 1024 {
        return Err(Error::ResourceLimit);
    }
    Ok(out)
}
