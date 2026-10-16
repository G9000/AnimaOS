use std::collections::{BTreeMap, BTreeSet};

use anima_core::document_content::{
    fit_document_snapshots, DocumentBlock, DocumentExtractionStatus as Status, DocumentFormat,
    DocumentLocator, DocumentOmission as Omission, DocumentSnapshot, ExtractedDocumentText,
    XlsxCoverage, XlsxSheetCoverage, XlsxSheetVisibility, MAX_DOCUMENT_RENDERED_BYTES,
    MAX_XLSX_CELLS, MAX_XLSX_CELL_TEXT_BYTES, MAX_XLSX_COLUMNS, MAX_XLSX_ROWS, MAX_XLSX_SHEETS,
    MAX_XLSX_SHEET_NAME_BYTES,
};
use quick_xml::{events::Event, name::ResolveResult, NsReader, XmlVersion};
use rawzip::{CompressionMethod, ZipArchive, ZipArchiveEntryWayfinder, ZipSliceArchive};

use crate::docx::{
    check_archive_policy, extras_ok, read_part, valid_xml_character, validate_declaration,
    validate_xml_characters,
};

const MAX_INPUT: usize = 2 * 1024 * 1024;
const MAX_ENTRIES: u64 = 512;
const MAX_PART: usize = 2 * 1024 * 1024;
const MAX_EXPANDED: usize = 8 * 1024 * 1024;
const MAX_STRINGS: usize = 4096;
const MAX_EVENTS: usize = 250_000;
const MAX_DEPTH: usize = 64;
const S: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const P: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const C: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
const WORKBOOK: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml";
const WORKSHEET: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml";
const STRINGS: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml";

type Entry = (ZipArchiveEntryWayfinder, CompressionMethod);

/// Read stored cell data only. No ZIP entries are unpacked, formulas evaluated,
/// relationships fetched, or visual/style semantics inferred. A late failure
/// discards the entire result, including previously parsed sheets.
pub fn extract_xlsx(bytes: &[u8]) -> ExtractedDocumentText {
    match extract(bytes) {
        Ok(value) => value,
        Err(status) => ExtractedDocumentText::empty_for(DocumentFormat::Xlsx, status),
    }
}

fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 512
        && !path.starts_with('/')
        && !path
            .chars()
            .any(|c| c.is_control() || matches!(c, '\\' | ':' | '%' | '?' | '#'))
        && path.split('/').all(|part| !matches!(part, "" | "." | ".."))
}

fn index(
    archive: &ZipSliceArchive<&[u8]>,
    bytes: &[u8],
) -> Result<BTreeMap<String, Entry>, Status> {
    if archive.entries_hint() > MAX_ENTRIES {
        return Err(Status::TooLarge);
    }
    check_archive_policy(bytes, archive.eocd_offset() as usize)?;
    let mut entries = archive.entries();
    let mut result = BTreeMap::new();
    let mut names = BTreeSet::new();
    let mut ranges = Vec::new();
    let mut count = 0u64;
    while let Some(entry) = entries.next_entry().map_err(|_| Status::Malformed)? {
        count = count.checked_add(1).ok_or(Status::TooLarge)?;
        if count > MAX_ENTRIES {
            return Err(Status::TooLarge);
        }
        let path = entry.file_path();
        let raw = path.as_bytes();
        let name = std::str::from_utf8(raw).map_err(|_| Status::Malformed)?;
        if raw.len() > 512 {
            return Err(Status::TooLarge);
        }
        if !safe_path(name.strip_suffix('/').unwrap_or(name))
            || !names.insert(name.to_ascii_lowercase())
        {
            return Err(Status::Malformed);
        }
        let lower = name.to_ascii_lowercase();
        if lower.contains("vbaproject") || lower.starts_with("xl/externallinks/") {
            return Err(Status::Unsupported);
        }
        let flags = entry.flags();
        if flags.is_encrypted() || flags.has_strong_encryption() || flags.is_masked() {
            return Err(Status::Encrypted);
        }
        let method = entry.compression_method();
        if !matches!(
            method,
            CompressionMethod::STORE | CompressionMethod::DEFLATE
        ) {
            return Err(Status::Unsupported);
        }
        // Only UTF-8, data descriptors, and DEFLATE tuning flags are supported.
        let central =
            usize::try_from(entry.central_directory_offset()).map_err(|_| Status::Malformed)?;
        let raw_flags = bytes
            .get(
                central.checked_add(8).ok_or(Status::Malformed)?
                    ..central.checked_add(10).ok_or(Status::Malformed)?,
            )
            .ok_or(Status::Malformed)?;
        let bits = u16::from_le_bytes([raw_flags[0], raw_flags[1]]);
        let allowed = if method == CompressionMethod::DEFLATE {
            0x080e
        } else {
            0x0808
        };
        if bits & !allowed != 0 {
            return Err(Status::Unsupported);
        }
        extras_ok(entry.extra_fields())?;
        if bytes.get(
            central.checked_add(34).ok_or(Status::Malformed)?
                ..central.checked_add(36).ok_or(Status::Malformed)?,
        ) != Some(&[0, 0])
        {
            return Err(Status::Unsupported);
        }
        let local = archive
            .get_entry(entry.wayfinder())
            .map_err(|_| Status::Malformed)?;
        let header = local.local_header();
        if header.file_path().as_bytes() != raw
            || header.compression_method() != method
            || header.flags() != flags
        {
            return Err(Status::Malformed);
        }
        extras_ok(header.extra_fields())?;
        if !flags.has_data_descriptor()
            && (header.crc32() != entry.crc32()
                || header.compressed_size_hint() != entry.compressed_size_hint()
                || header.uncompressed_size_hint() != entry.uncompressed_size_hint())
        {
            return Err(Status::Malformed);
        }
        let (_, mut end) = local.compressed_data_range();
        if let Some(descriptor) = local.data_descriptor().map_err(|_| Status::Malformed)? {
            if descriptor.crc32() != entry.crc32()
                || descriptor.compressed_size() != entry.compressed_size_hint()
                || descriptor.uncompressed_size() != entry.uncompressed_size_hint()
            {
                return Err(Status::Malformed);
            }
            let offset = usize::try_from(end).map_err(|_| Status::Malformed)?;
            let signed = bytes.get(offset..offset.checked_add(4).ok_or(Status::Malformed)?)
                == Some(b"PK\x07\x08");
            end = end
                .checked_add(if signed { 16 } else { 12 })
                .ok_or(Status::Malformed)?;
        }
        if end > archive.directory_offset() || entry.local_header_offset() >= end {
            return Err(Status::Malformed);
        }
        ranges.push((entry.local_header_offset(), end));
        result.insert(name.to_owned(), (entry.wayfinder(), method));
    }
    if count != archive.entries_hint() {
        return Err(Status::Malformed);
    }
    ranges.sort_unstable();
    if ranges.windows(2).any(|pair| pair[0].1 > pair[1].0) {
        return Err(Status::Malformed);
    }
    Ok(result)
}

#[derive(Default)]
struct Budget {
    expanded: usize,
    events: usize,
    cells: usize,
}

// Each tree is at most one bounded XML part. Trees are dropped after their
// specific table is consumed; depth/events also bound metadata-only structures.
struct Node {
    ns: String,
    name: String,
    attributes: BTreeMap<(String, String), String>,
    text: String,
    children: Vec<Node>,
}

impl Node {
    fn is(&self, ns: &str, name: &str) -> bool {
        self.ns == ns && self.name == name
    }
    fn attr(&self, name: &str) -> Option<&str> {
        self.attr_ns("", name)
    }
    fn attr_ns(&self, ns: &str, name: &str) -> Option<&str> {
        self.attributes
            .get(&(ns.to_owned(), name.to_owned()))
            .map(String::as_str)
    }
    fn required(&self, name: &str) -> Result<&str, Status> {
        self.attr(name).ok_or(Status::Malformed)
    }
    fn whitespace(&self) -> Result<(), Status> {
        if xml_space(&self.text) {
            Ok(())
        } else {
            Err(Status::Malformed)
        }
    }
    fn only_child(&self, name: &str) -> Result<Option<&Node>, Status> {
        let mut matching = self.children.iter().filter(|node| node.is(S, name));
        let first = matching.next();
        if matching.next().is_some() {
            return Err(Status::Malformed);
        }
        Ok(first)
    }
}

fn namespace(value: ResolveResult<'_>) -> Result<String, Status> {
    match value {
        ResolveResult::Unbound => Ok(String::new()),
        ResolveResult::Bound(ns) => Ok(std::str::from_utf8(ns.as_ref())
            .map_err(|_| Status::Malformed)?
            .to_owned()),
        ResolveResult::Unknown(_) => Err(Status::Malformed),
    }
}

fn xml_space(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n'))
}

fn text(stack: &mut [Node], value: &str) -> Result<(), Status> {
    if !value.chars().all(valid_xml_character) {
        return Err(Status::Malformed);
    }
    if let Some(node) = stack.last_mut() {
        node.text.push_str(value);
    } else if !xml_space(value) {
        return Err(Status::Malformed);
    }
    Ok(())
}

fn xml_name(raw: &[u8]) -> Result<(), Status> {
    fn start(c: char) -> bool {
        c == '_'
            || c.is_ascii_alphabetic()
            || matches!(c,
            '\u{c0}'..='\u{d6}' | '\u{d8}'..='\u{f6}' | '\u{f8}'..='\u{2ff}' |
            '\u{370}'..='\u{37d}' | '\u{37f}'..='\u{1fff}' | '\u{200c}'..='\u{200d}' |
            '\u{2070}'..='\u{218f}' | '\u{2c00}'..='\u{2fef}' | '\u{3001}'..='\u{d7ff}' |
            '\u{f900}'..='\u{fdcf}' | '\u{fdf0}'..='\u{fffd}' | '\u{10000}'..='\u{effff}')
    }
    let name = std::str::from_utf8(raw).map_err(|_| Status::Malformed)?;
    let mut pieces = 0;
    for piece in name.split(':') {
        pieces += 1;
        let mut chars = piece.chars();
        if pieces > 2 || !chars.next().is_some_and(start) || !chars.all(|c| start(c) || c.is_ascii_digit() || matches!(c, '-' | '.' | '\u{b7}' | '\u{300}'..='\u{36f}' | '\u{203f}'..='\u{2040}')) { return Err(Status::Malformed); }
    }
    Ok(())
}

fn xml(bytes: &[u8], budget: &mut Budget) -> Result<Node, Status> {
    validate_xml_characters(bytes)?;
    let mut reader = NsReader::from_reader(bytes);
    reader.config_mut().expand_empty_elements = true;
    reader.config_mut().check_comments = true;
    let mut stack: Vec<Node> = Vec::new();
    let mut root = None;
    let mut local_events = 0usize;
    loop {
        budget.events = budget.events.checked_add(1).ok_or(Status::TooLarge)?;
        local_events += 1;
        if budget.events > MAX_EVENTS {
            return Err(Status::TooLarge);
        }
        match reader.read_event().map_err(|_| Status::Malformed)? {
            Event::Start(tag) => {
                if stack.len() >= MAX_DEPTH {
                    return Err(Status::TooLarge);
                }
                if stack.is_empty() && root.is_some() {
                    return Err(Status::Malformed);
                }
                xml_name(tag.name().as_ref())?;
                let (ns, name) = reader.resolver().resolve_element(tag.name());
                let mut node = Node {
                    ns: namespace(ns)?,
                    name: std::str::from_utf8(name.as_ref())
                        .map_err(|_| Status::Malformed)?
                        .to_owned(),
                    attributes: BTreeMap::new(),
                    text: String::new(),
                    children: Vec::new(),
                };
                if node.is(S, "c") && stack.first().is_some_and(|root| root.is(S, "worksheet")) {
                    budget.cells = budget.cells.checked_add(1).ok_or(Status::TooLarge)?;
                    if budget.cells > MAX_XLSX_CELLS {
                        return Err(Status::TooLarge);
                    }
                }
                if let Some(parent) = stack.last() {
                    if (parent.is(S, "sst") && parent.children.len() >= MAX_STRINGS)
                        || (parent.is(S, "sheets") && parent.children.len() >= MAX_XLSX_SHEETS)
                    {
                        return Err(Status::TooLarge);
                    }
                }
                for attribute in tag.attributes() {
                    let attribute = attribute.map_err(|_| Status::Malformed)?;
                    xml_name(attribute.key.as_ref())?;
                    let value = attribute
                        .decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder())
                        .map_err(|_| Status::Malformed)?
                        .into_owned();
                    if !value.chars().all(valid_xml_character) {
                        return Err(Status::Malformed);
                    }
                    if attribute.key.as_ref() == b"xmlns"
                        || attribute.key.as_ref().starts_with(b"xmlns:")
                    {
                        continue;
                    }
                    let (ns, name) = reader.resolver().resolve_attribute(attribute.key);
                    let key = (
                        namespace(ns)?,
                        std::str::from_utf8(name.as_ref())
                            .map_err(|_| Status::Malformed)?
                            .to_owned(),
                    );
                    if node.attributes.insert(key, value).is_some() {
                        return Err(Status::Malformed);
                    }
                }
                stack.push(node);
            }
            Event::End(_) => {
                let node = stack.pop().ok_or(Status::Malformed)?;
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(node);
                } else if root.replace(node).is_some() {
                    return Err(Status::Malformed);
                }
            }
            Event::Text(value) => {
                // XML forbids this literal delimiter in character data. Check
                // raw bytes so escaped text and actual CDATA remain valid.
                if value.as_ref().windows(3).any(|bytes| bytes == b"]]>") {
                    return Err(Status::Malformed);
                }
                text(&mut stack, &value.decode().map_err(|_| Status::Malformed)?)?
            }
            Event::CData(value) => {
                if stack.is_empty() {
                    return Err(Status::Malformed);
                }
                text(&mut stack, &value.decode().map_err(|_| Status::Malformed)?)?;
            }
            Event::GeneralRef(value) => {
                if stack.is_empty() {
                    return Err(Status::Malformed);
                }
                let decoded = if let Some(character) =
                    value.resolve_char_ref().map_err(|_| Status::Malformed)?
                {
                    character.to_string()
                } else {
                    match value.decode().map_err(|_| Status::Malformed)?.as_ref() {
                        "amp" => "&",
                        "lt" => "<",
                        "gt" => ">",
                        "quot" => "\"",
                        "apos" => "'",
                        _ => return Err(Status::Malformed),
                    }
                    .to_owned()
                };
                text(&mut stack, &decoded)?;
            }
            Event::Decl(value) => {
                if local_events != 1 {
                    return Err(Status::Malformed);
                }
                validate_declaration(&value)?;
            }
            Event::DocType(_) => return Err(Status::Malformed),
            Event::PI(value) => {
                // Namespace-aware XML requires an NCName target. The payload
                // is ignored; no instruction, stylesheet or resource is used.
                let target = value.target();
                if target.contains(&b':') || target.eq_ignore_ascii_case(b"xml") {
                    return Err(Status::Malformed);
                }
                xml_name(target)?;
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !stack.is_empty() {
        return Err(Status::Malformed);
    }
    root.ok_or(Status::Malformed)
}

fn part(
    archive: &ZipSliceArchive<&[u8]>,
    entries: &BTreeMap<String, Entry>,
    name: &str,
    budget: &mut Budget,
) -> Result<Node, Status> {
    let (entry, method) = entries.get(name).ok_or(Status::Malformed)?;
    let remaining = MAX_EXPANDED
        .checked_sub(budget.expanded)
        .ok_or(Status::TooLarge)?;
    let bytes = read_part(archive, *entry, *method, MAX_PART.min(remaining))?;
    budget.expanded = budget
        .expanded
        .checked_add(bytes.len())
        .ok_or(Status::TooLarge)?;
    xml(&bytes, budget)
}

struct ContentTypes {
    overrides: BTreeMap<String, String>,
    defaults: BTreeMap<String, String>,
}

impl ContentTypes {
    fn require(&self, path: &str, kind: &str) -> Result<(), Status> {
        let actual = self.overrides.get(path).or_else(|| {
            path.rsplit_once('.')
                .and_then(|(_, extension)| self.defaults.get(extension))
        });
        if actual.map(String::as_str) == Some(kind) {
            Ok(())
        } else {
            Err(Status::Unsupported)
        }
    }
}

fn content_types(root: Node) -> Result<ContentTypes, Status> {
    if !root.is(C, "Types") {
        return Err(Status::Unsupported);
    }
    root.whitespace()?;
    let mut result = ContentTypes {
        overrides: BTreeMap::new(),
        defaults: BTreeMap::new(),
    };
    for child in &root.children {
        child.whitespace()?;
        if !child.children.is_empty() {
            return Err(Status::Malformed);
        }
        let kind = child.required("ContentType")?;
        let lower = kind.to_ascii_lowercase();
        if lower.contains("macroenabled")
            || lower.contains("vbaproject")
            || lower.contains("externallink")
        {
            return Err(Status::Unsupported);
        }
        let (map, key) = if child.is(C, "Override") {
            let name = child
                .required("PartName")?
                .strip_prefix('/')
                .ok_or(Status::Malformed)?;
            if !safe_path(name) {
                return Err(Status::Malformed);
            }
            (&mut result.overrides, name.to_owned())
        } else if child.is(C, "Default") {
            let extension = child.required("Extension")?;
            if extension.is_empty() || !extension.bytes().all(|b| b.is_ascii_alphanumeric()) {
                return Err(Status::Malformed);
            }
            (&mut result.defaults, extension.to_owned())
        } else {
            return Err(Status::Malformed);
        };
        if map.insert(key, kind.to_owned()).is_some() {
            return Err(Status::Malformed);
        }
    }
    Ok(result)
}

struct Relationship {
    kind: String,
    path: String,
}

fn relationships(root: Node, base: &str) -> Result<BTreeMap<String, Relationship>, Status> {
    if !root.is(P, "Relationships") {
        return Err(Status::Unsupported);
    }
    root.whitespace()?;
    let mut result = BTreeMap::new();
    let mut targets = BTreeSet::new();
    for child in &root.children {
        if !child.is(P, "Relationship") || !child.children.is_empty() {
            return Err(Status::Malformed);
        }
        child.whitespace()?;
        if child
            .attr("TargetMode")
            .is_some_and(|mode| mode != "Internal")
        {
            return Err(Status::Unsupported);
        }
        let target = child.required("Target")?;
        if !safe_path(target) {
            return Err(Status::Malformed);
        }
        let path = format!("{base}{target}");
        if !safe_path(&path) || !targets.insert(path.to_ascii_lowercase()) {
            return Err(Status::Malformed);
        }
        let id = child.required("Id")?;
        if id.is_empty() || id.len() > 128 || id.chars().any(char::is_control) {
            return Err(Status::Malformed);
        }
        if result
            .insert(
                id.to_owned(),
                Relationship {
                    kind: child.required("Type")?.to_owned(),
                    path,
                },
            )
            .is_some()
        {
            return Err(Status::Malformed);
        }
    }
    Ok(result)
}

fn positive(value: &str, maximum: u32) -> Result<u32, Status> {
    if value.is_empty() || value.starts_with('0') || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err(Status::Malformed);
    }
    let number = value.parse::<u32>().map_err(|_| Status::Malformed)?;
    if number > maximum {
        return Err(Status::Malformed);
    }
    Ok(number)
}

fn cell_reference(value: &str) -> Result<(u32, u32), Status> {
    let split = value
        .bytes()
        .position(|b| !b.is_ascii_uppercase())
        .ok_or(Status::Malformed)?;
    if split == 0 || split > 3 {
        return Err(Status::Malformed);
    }
    let mut column = 0u32;
    for byte in value[..split].bytes() {
        column = column
            .checked_mul(26)
            .and_then(|n| n.checked_add(u32::from(byte - b'A' + 1)))
            .ok_or(Status::Malformed)?;
    }
    if column > MAX_XLSX_COLUMNS {
        return Err(Status::Malformed);
    }
    Ok((positive(&value[split..], MAX_XLSX_ROWS)?, column))
}

#[derive(Clone, Default)]
struct CellText {
    value: String,
    overflow: bool,
}

impl CellText {
    fn append(&mut self, text: &str) {
        if self.overflow {
            return;
        }
        if self
            .value
            .len()
            .checked_add(text.len())
            .is_none_or(|n| n > MAX_XLSX_CELL_TEXT_BYTES)
        {
            self.overflow = true;
            self.value.clear();
        } else {
            self.value.push_str(text);
        }
    }
    fn from_text(text: &str) -> Self {
        let mut value = Self::default();
        value.append(text);
        value
    }
}

fn rich_string(node: &Node) -> Result<CellText, Status> {
    node.whitespace()?;
    let mut value = CellText::default();
    let mut plain = false;
    let mut rich = false;
    for child in &node.children {
        if child.is(S, "t") {
            if plain || rich || !child.children.is_empty() {
                return Err(Status::Malformed);
            }
            plain = true;
            value.append(&child.text);
        } else if child.is(S, "r") {
            if plain {
                return Err(Status::Malformed);
            }
            rich = true;
            child.whitespace()?;
            let text = child.only_child("t")?.ok_or(Status::Malformed)?;
            if !text.children.is_empty()
                || child
                    .children
                    .iter()
                    .any(|n| !n.is(S, "t") && !n.is(S, "rPr"))
            {
                return Err(Status::Malformed);
            }
            value.append(&text.text);
        } else if !child.is(S, "rPh") && !child.is(S, "phoneticPr") {
            return Err(Status::Unsupported);
        }
    }
    Ok(value)
}

fn shared_strings(root: Node) -> Result<Vec<CellText>, Status> {
    if !root.is(S, "sst") {
        return Err(Status::Unsupported);
    }
    root.whitespace()?;
    if root.children.len() > MAX_STRINGS {
        return Err(Status::TooLarge);
    }
    let mut result = Vec::new();
    for child in &root.children {
        if !child.is(S, "si") {
            return Err(Status::Malformed);
        }
        result.push(rich_string(child)?);
    }
    Ok(result)
}

fn scalar(kind: &str, raw: &str, strings: &[CellText]) -> Result<CellText, Status> {
    match kind {
        "s" => {
            if raw.is_empty() || !raw.bytes().all(|b| b.is_ascii_digit()) {
                return Err(Status::Malformed);
            }
            let index = raw.parse::<usize>().map_err(|_| Status::Malformed)?;
            strings.get(index).cloned().ok_or(Status::Malformed)
        }
        "b" => match raw {
            "0" => Ok(CellText::from_text("false")),
            "1" => Ok(CellText::from_text("true")),
            _ => Err(Status::Malformed),
        },
        "n" => {
            if raw.is_empty() {
                return Ok(CellText::default());
            }
            if !raw
                .bytes()
                .all(|b| b.is_ascii_digit() || matches!(b, b'+' | b'-' | b'.' | b'e' | b'E'))
                || !raw.parse::<f64>().is_ok_and(f64::is_finite)
            {
                return Err(Status::Malformed);
            }
            Ok(CellText::from_text(raw))
        }
        "e" | "str" => Ok(CellText::from_text(raw)),
        _ => Err(Status::Unsupported),
    }
}

fn cell_text(cell: &Node, strings: &[CellText]) -> Result<CellText, Status> {
    cell.whitespace()?;
    let kind = cell.attr("t").unwrap_or("n");
    if !matches!(kind, "s" | "b" | "n" | "e" | "str" | "inlineStr") {
        return Err(Status::Unsupported);
    }
    if cell
        .children
        .iter()
        .any(|node| node.ns != S || !matches!(node.name.as_str(), "v" | "f" | "is"))
    {
        return Err(Status::Unsupported);
    }
    let value = cell.only_child("v")?;
    let formula = cell.only_child("f")?;
    let inline = cell.only_child("is")?;
    if value.is_some_and(|node| !node.children.is_empty())
        || formula.is_some_and(|node| !node.children.is_empty())
    {
        return Err(Status::Malformed);
    }
    if kind == "inlineStr" {
        if value.is_some() || formula.is_some() {
            return Err(Status::Malformed);
        }
        return inline
            .map(rich_string)
            .transpose()
            .map(|text| text.unwrap_or_default());
    }
    if inline.is_some() {
        return Err(Status::Malformed);
    }
    // Empty numeric/bool cache elements are missing values, not a computed empty
    // result. An explicitly string-typed empty cache remains an empty string.
    let cached = value
        .filter(|node| !node.text.is_empty() || kind == "str")
        .map(|node| scalar(kind, &node.text, strings))
        .transpose()?;
    if let Some(formula) = formula {
        if formula.attr("t").is_some_and(|kind| kind != "normal") || formula.text.trim().is_empty()
        {
            return Err(Status::Unsupported);
        }
        let mut text = CellText::from_text("formula (not evaluated)=");
        text.append(&formula.text);
        if let Some(cached) = cached {
            text.append("; cached (possibly stale)=");
            text.append(&cached.value);
            if cached.overflow {
                text.value.clear();
                text.overflow = true;
            }
        } else {
            text.append("; cached=missing");
        }
        return Ok(text);
    }
    Ok(cached.unwrap_or_default())
}

fn worksheet(
    root: Node,
    strings: &[CellText],
    sheet: &mut XlsxSheetCoverage,
    value: &mut ExtractedDocumentText,
    total: &mut usize,
    text_bytes: &mut usize,
) -> Result<(), Status> {
    if !root.is(S, "worksheet") {
        return Err(Status::Unsupported);
    }
    root.whitespace()?;
    let data = root.only_child("sheetData")?.ok_or(Status::Malformed)?;
    data.whitespace()?;
    let mut last_row = 0;
    for row in &data.children {
        if !row.is(S, "row") {
            return Err(Status::Malformed);
        }
        row.whitespace()?;
        let row_number = positive(row.required("r")?, MAX_XLSX_ROWS)?;
        if row_number <= last_row {
            return Err(Status::Malformed);
        }
        last_row = row_number;
        let mut last_column = 0;
        for cell in &row.children {
            if !cell.is(S, "c") {
                return Err(Status::Malformed);
            }
            *total = total.checked_add(1).ok_or(Status::TooLarge)?;
            if *total > MAX_XLSX_CELLS {
                return Err(Status::TooLarge);
            }
            let (row, column) = cell_reference(cell.required("r")?)?;
            if row != row_number || column <= last_column {
                return Err(Status::Malformed);
            }
            last_column = column;
            sheet.observed_cell_count += 1;
            let text = cell_text(cell, strings)?;
            if text.overflow {
                if !value.omissions.contains(&Omission::CellTextLimit) {
                    value.omissions.push(Omission::CellTextLimit);
                }
            } else if !text.value.trim().is_empty() {
                // Bound retained text before the shared renderer performs exact
                // JSON/source metadata accounting and whole-cell fitting.
                if text_bytes
                    .checked_add(text.value.len())
                    .is_none_or(|n| n > MAX_DOCUMENT_RENDERED_BYTES)
                {
                    if !value.omissions.contains(&Omission::TextLimit) {
                        value.omissions.push(Omission::TextLimit);
                    }
                } else {
                    *text_bytes += text.value.len();
                    sheet.included_cell_count += 1;
                    value.blocks.push(DocumentBlock {
                        locator: DocumentLocator::XlsxCell {
                            sheet: sheet.index,
                            row,
                            column,
                        },
                        text: text.value,
                    });
                }
            }
        }
    }
    Ok(())
}

fn extract(bytes: &[u8]) -> Result<ExtractedDocumentText, Status> {
    if bytes.len() > MAX_INPUT {
        return Err(Status::TooLarge);
    }
    if bytes.starts_with(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]) {
        return Err(Status::Unsupported);
    }
    let archive = ZipArchive::from_slice(bytes).map_err(|_| Status::Malformed)?;
    let entries = index(&archive, bytes)?;
    // This slice resolves only the package/workbook relationships. Refuse other
    // relationship-bearing features rather than silently accepting unchecked
    // external links or inferring their cells from cached visual artifacts.
    if entries.keys().any(|path| {
        path.to_ascii_lowercase().ends_with(".rels")
            && path != "_rels/.rels"
            && path != "xl/_rels/workbook.xml.rels"
    }) {
        return Err(Status::Unsupported);
    }
    let mut budget = Budget::default();
    let types = content_types(part(
        &archive,
        &entries,
        "[Content_Types].xml",
        &mut budget,
    )?)?;
    types.require("xl/workbook.xml", WORKBOOK)?;
    let root_rels = relationships(part(&archive, &entries, "_rels/.rels", &mut budget)?, "")?;
    let mut office = root_rels
        .values()
        .filter(|rel| rel.kind == format!("{R}/officeDocument"));
    if office
        .next()
        .is_none_or(|rel| rel.path != "xl/workbook.xml")
        || office.next().is_some()
    {
        return Err(Status::Unsupported);
    }
    for rel in root_rels.values() {
        if rel.kind != format!("{R}/officeDocument") && rel.kind != format!("{R}/extended-properties")
            && rel.kind != "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" { return Err(Status::Unsupported); }
    }
    let rels = relationships(
        part(
            &archive,
            &entries,
            "xl/_rels/workbook.xml.rels",
            &mut budget,
        )?,
        "xl/",
    )?;
    let mut strings_path = None;
    for rel in rels.values() {
        let kind = rel
            .kind
            .strip_prefix(&format!("{R}/"))
            .ok_or(Status::Unsupported)?;
        match kind {
            "worksheet" => {
                if !rel.path.starts_with("xl/worksheets/") || !rel.path.ends_with(".xml") {
                    return Err(Status::Unsupported);
                }
                types.require(&rel.path, WORKSHEET)?;
            }
            "sharedStrings" => {
                if rel.path != "xl/sharedStrings.xml"
                    || strings_path.replace(rel.path.clone()).is_some()
                {
                    return Err(Status::Unsupported);
                }
                types.require(&rel.path, STRINGS)?;
            }
            "styles" if rel.path == "xl/styles.xml" => types.require(
                &rel.path,
                "application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml",
            )?,
            "theme" if rel.path.starts_with("xl/theme/") && rel.path.ends_with(".xml") => types
                .require(
                    &rel.path,
                    "application/vnd.openxmlformats-officedocument.theme+xml",
                )?,
            "calcChain" if rel.path == "xl/calcChain.xml" => types.require(
                &rel.path,
                "application/vnd.openxmlformats-officedocument.spreadsheetml.calcChain+xml",
            )?,
            _ => return Err(Status::Unsupported),
        }
        if !entries.contains_key(&rel.path) {
            return Err(Status::Malformed);
        }
    }
    let strings = if let Some(path) = strings_path {
        shared_strings(part(&archive, &entries, &path, &mut budget)?)?
    } else {
        Vec::new()
    };
    let workbook = part(&archive, &entries, "xl/workbook.xml", &mut budget)?;
    if !workbook.is(S, "workbook") {
        return Err(Status::Unsupported);
    }
    workbook.whitespace()?;
    if workbook
        .children
        .iter()
        .any(|node| node.is(S, "externalReferences"))
    {
        return Err(Status::Unsupported);
    }
    let sheets = workbook.only_child("sheets")?.ok_or(Status::Malformed)?;
    sheets.whitespace()?;
    if sheets.children.len() > MAX_XLSX_SHEETS {
        return Err(Status::TooLarge);
    }
    if sheets.children.is_empty() {
        return Err(Status::Malformed);
    }
    let mut value = ExtractedDocumentText::empty_for(DocumentFormat::Xlsx, Status::NoText);
    value.omissions.push(Omission::StoredCellsOnly);
    let mut coverage = XlsxCoverage {
        sheet_count: sheets.children.len() as u32,
        observed_cell_count: 0,
        included_cell_count: 0,
        sheets: Vec::new(),
    };
    let mut ids = BTreeSet::new();
    let mut names = BTreeSet::new();
    let mut selected = BTreeSet::new();
    let mut total = 0usize;
    let mut text_bytes = 0usize;
    for (index, node) in sheets.children.iter().enumerate() {
        if !node.is(S, "sheet") || !node.children.is_empty() {
            return Err(Status::Malformed);
        }
        node.whitespace()?;
        let name = node.required("name")?;
        if name.trim().is_empty()
            || name.len() > MAX_XLSX_SHEET_NAME_BYTES
            || name.chars().any(char::is_control)
            || !names.insert(name.to_lowercase())
        {
            return Err(Status::Malformed);
        }
        if !ids.insert(positive(node.required("sheetId")?, u32::MAX)?) {
            return Err(Status::Malformed);
        }
        let rel = rels
            .get(node.attr_ns(R, "id").ok_or(Status::Malformed)?)
            .ok_or(Status::Malformed)?;
        if rel.kind != format!("{R}/worksheet") || !selected.insert(rel.path.clone()) {
            return Err(Status::Malformed);
        }
        let visibility = match node.attr("state").unwrap_or("visible") {
            "visible" => XlsxSheetVisibility::Visible,
            "hidden" => XlsxSheetVisibility::Hidden,
            "veryHidden" => XlsxSheetVisibility::VeryHidden,
            _ => return Err(Status::Malformed),
        };
        let mut sheet = XlsxSheetCoverage {
            index: index as u32 + 1,
            name: name.to_owned(),
            visibility,
            observed_cell_count: 0,
            included_cell_count: 0,
        };
        worksheet(
            part(&archive, &entries, &rel.path, &mut budget)?,
            &strings,
            &mut sheet,
            &mut value,
            &mut total,
            &mut text_bytes,
        )?;
        coverage.observed_cell_count += sheet.observed_cell_count;
        coverage.included_cell_count += sheet.included_cell_count;
        coverage.sheets.push(sheet);
    }
    if rels
        .values()
        .filter(|rel| rel.kind == format!("{R}/worksheet"))
        .count()
        != selected.len()
    {
        return Err(Status::Malformed);
    }
    value.xlsx_coverage = Some(coverage);
    if !value.blocks.is_empty() {
        value.status = Status::Partial;
    }
    // Match PDF's extraction-side allowance for the largest admitted source ID.
    let snapshot = DocumentSnapshot {
        attachment_id: "a".repeat(128),
        source_sha256: "a".repeat(64),
        extraction: value,
    };
    let mut fitted = fit_document_snapshots(&[snapshot], MAX_DOCUMENT_RENDERED_BYTES)
        .map_err(|_| Status::Malformed)?;
    let mut value = fitted.remove(0).extraction;
    if let Some(index) = value
        .omissions
        .iter()
        .position(|reason| *reason == Omission::ContextLimit)
    {
        value.omissions.remove(index);
        if !value.omissions.contains(&Omission::TextLimit) {
            value.omissions.push(Omission::TextLimit);
        }
    }
    Ok(value)
}
