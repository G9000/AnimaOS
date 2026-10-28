use std::collections::BTreeSet;
use std::io::Read;

use anima_service_adapters::document_content::{
    DocumentBlock, DocumentExtractionStatus as Status, DocumentFormat, DocumentLocator,
    DocumentOmission as Omission, ExtractedDocumentText, DOCUMENT_CONTENT_VERSION,
    MAX_DOCUMENT_PARAGRAPHS, MAX_DOCUMENT_RENDERED_BYTES,
};
use quick_xml::{events::Event, name::ResolveResult, NsReader, XmlVersion};
use rawzip::{
    extra_fields::{ExtraFieldId, ExtraFields},
    CompressionMethod, ZipArchive, ZipArchiveEntryWayfinder, ZipSliceArchive,
};

const MAX_INPUT: usize = 2 * 1024 * 1024;
const MAX_XML: usize = 4 * 1024 * 1024;
const MAX_TYPES: usize = 64 * 1024;
const W: &[u8] = b"http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const TYPES: &[u8] = b"http://schemas.openxmlformats.org/package/2006/content-types";
const MAIN_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml";

/// Extract transitional WordprocessingML main-body text. Full selected XML and
/// its CRC are validated before any bounded prefix is returned.
pub fn extract_docx(bytes: &[u8]) -> ExtractedDocumentText {
    extract_docx_with_text_limit(bytes, MAX_DOCUMENT_RENDERED_BYTES)
}

pub(crate) fn extract_docx_with_text_limit(
    bytes: &[u8],
    text_limit: usize,
) -> ExtractedDocumentText {
    match extract(bytes, text_limit) {
        Ok(value) => value,
        Err(status) => ExtractedDocumentText::empty(status),
    }
}

pub(super) fn extras_ok(mut extras: ExtraFields<'_>) -> Result<(), Status> {
    for (id, _) in extras.by_ref() {
        if id == ExtraFieldId::ZIP64 {
            return Err(Status::Unsupported);
        }
    }
    if !extras.remaining_bytes().is_empty() {
        return Err(Status::Malformed);
    }
    Ok(())
}

fn extract(bytes: &[u8], text_limit: usize) -> Result<ExtractedDocumentText, Status> {
    if bytes.len() > MAX_INPUT {
        return Err(Status::TooLarge);
    }
    if bytes.starts_with(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]) {
        // Compound-file magic does not establish validated encryption. This
        // slice has no compound-file reader; only checked ZIP encryption flags
        // can produce the encrypted outcome.
        return Err(Status::Unsupported);
    }
    let archive = ZipArchive::from_slice(bytes).map_err(|_| Status::Malformed)?;
    if archive.entries_hint() > 256 {
        return Err(Status::TooLarge);
    }
    check_archive_policy(bytes, archive.eocd_offset() as usize)?;
    let mut names = BTreeSet::new();
    let mut ranges = Vec::new();
    let mut entries = archive.entries();
    let mut total = 0u64;
    let mut count = 0u64;
    let mut main = None;
    let mut types = None;
    while let Some(entry) = entries.next_entry().map_err(|_| Status::Malformed)? {
        count += 1;
        if count > 256 {
            return Err(Status::TooLarge);
        }
        let path = entry.file_path();
        let raw_name = path.as_bytes();
        if raw_name.len() > 512 {
            return Err(Status::TooLarge);
        }
        let name = std::str::from_utf8(raw_name).map_err(|_| Status::Malformed)?;
        if name.is_empty()
            || name.starts_with('/')
            || name.contains(['\\', ':', '\0', '%'])
            || name
                .trim_end_matches('/')
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
            || !names.insert(name.to_ascii_lowercase())
        {
            return Err(Status::Malformed);
        }
        if name.to_ascii_lowercase().contains("vbaproject") {
            return Err(Status::Unsupported);
        }
        let flags = entry.flags();
        if flags.is_encrypted() || flags.has_strong_encryption() || flags.is_masked() {
            return Err(Status::Encrypted);
        }
        if !matches!(
            entry.compression_method(),
            CompressionMethod::STORE | CompressionMethod::DEFLATE
        ) {
            return Err(Status::Unsupported);
        }
        extras_ok(entry.extra_fields())?;
        // Policy only, anchored at the library's validated central record.
        let offset = entry.central_directory_offset() as usize;
        if bytes.get(
            offset.checked_add(34).ok_or(Status::Malformed)?
                ..offset.checked_add(36).ok_or(Status::Malformed)?,
        ) != Some(&[0, 0])
        {
            return Err(Status::Unsupported);
        }
        total = total
            .checked_add(entry.uncompressed_size_hint())
            .ok_or(Status::TooLarge)?;
        if total > 16 * 1024 * 1024 {
            return Err(Status::TooLarge);
        }
        let local = archive
            .get_entry(entry.wayfinder())
            .map_err(|_| Status::Malformed)?;
        let header = local.local_header();
        if header.file_path().as_bytes() != raw_name
            || header.compression_method() != entry.compression_method()
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
            end = end
                .checked_add(
                    if bytes
                        .get(end as usize..(end as usize).checked_add(4).ok_or(Status::Malformed)?)
                        == Some(b"PK\x07\x08")
                    {
                        16
                    } else {
                        12
                    },
                )
                .ok_or(Status::Malformed)?;
        }
        if end > archive.directory_offset() || entry.local_header_offset() >= end {
            return Err(Status::Malformed);
        }
        ranges.push((entry.local_header_offset(), end));
        if name == "word/document.xml" {
            main = Some((entry.wayfinder(), entry.compression_method()));
        }
        if name == "[Content_Types].xml" {
            types = Some((entry.wayfinder(), entry.compression_method()));
        }
    }
    if count != archive.entries_hint() {
        return Err(Status::Malformed);
    }
    ranges.sort_unstable();
    if ranges.windows(2).any(|pair| pair[0].1 > pair[1].0) {
        return Err(Status::Malformed);
    }
    let (types_entry, types_method) = types.ok_or(Status::Malformed)?;
    let types_xml = read_part(&archive, types_entry, types_method, MAX_TYPES)?;
    validate_types(&types_xml)?;
    let (main_entry, main_method) = main.ok_or(Status::Malformed)?;
    let xml = read_part(&archive, main_entry, main_method, MAX_XML)?;
    parse_main(&xml, text_limit)
}

/// rawzip owns parsing/offset discovery. Its public API omits multi-disk and
/// ZIP64 EOCD policy, so compare only fixed fields at its validated EOCD offset.
pub(super) fn check_archive_policy(bytes: &[u8], eocd: usize) -> Result<(), Status> {
    let end = eocd.checked_add(22).ok_or(Status::Malformed)?;
    let record = bytes.get(eocd..end).ok_or(Status::Malformed)?;
    if &record[..4] != b"PK\x05\x06" {
        return Err(Status::Malformed);
    }
    if record[4..8] != [0; 4]
        || record[8..10] != record[10..12]
        || record[10..12] == [0xff; 2]
        || record[12..16] == [0xff; 4]
        || record[16..20] == [0xff; 4]
        || eocd
            .checked_sub(20)
            .and_then(|start| bytes.get(start..start + 4))
            == Some(b"PK\x06\x07")
    {
        return Err(Status::Unsupported);
    }
    Ok(())
}

pub(super) fn read_part(
    archive: &ZipSliceArchive<&[u8]>,
    entry: ZipArchiveEntryWayfinder,
    method: CompressionMethod,
    cap: usize,
) -> Result<Vec<u8>, Status> {
    let local = archive.get_entry(entry).map_err(|_| Status::Malformed)?;
    let data = local.data();
    let cap = cap.min(data.len().saturating_mul(100));
    if method == CompressionMethod::STORE {
        read_bounded(local.verifying_reader(data), cap)
    } else {
        let mut decoder = flate2::bufread::DeflateDecoder::new(data);
        let output = read_bounded(local.verifying_reader(&mut decoder), cap)?;
        if decoder.total_in() != data.len() as u64 {
            return Err(Status::Malformed);
        }
        Ok(output)
    }
}

fn read_bounded(mut verified: impl Read, cap: usize) -> Result<Vec<u8>, Status> {
    let mut output = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        let read = verified.read(&mut buffer).map_err(|_| Status::Malformed)?;
        if read == 0 {
            break;
        }
        if output.len().saturating_add(read) > cap {
            return Err(Status::TooLarge);
        }
        output.extend_from_slice(&buffer[..read]);
    }
    Ok(output)
}

fn validate_types(xml: &[u8]) -> Result<(), Status> {
    validate_xml_characters(xml)?;
    let mut reader = NsReader::from_reader(xml);
    reader.config_mut().expand_empty_elements = true;
    let mut depth = 0usize;
    let mut root_seen = false;
    let mut main_seen = false;
    let mut events = 0;
    loop {
        events += 1;
        if events > 100_000 {
            return Err(Status::TooLarge);
        }
        let event = reader.read_event().map_err(|_| Status::Malformed)?;
        match event {
            Event::Start(ref tag) => {
                depth += 1;
                if depth > 64 {
                    return Err(Status::TooLarge);
                }
                let (ns, local) = reader.resolver().resolve_element(tag.name());
                if !matches!(ns, ResolveResult::Bound(ns) if ns.as_ref() == TYPES) {
                    return Err(Status::Unsupported);
                }
                if depth == 1 {
                    if root_seen || local.as_ref() != b"Types" {
                        return Err(Status::Malformed);
                    }
                    root_seen = true;
                } else if depth != 2 || !matches!(local.as_ref(), b"Override" | b"Default") {
                    return Err(Status::Malformed);
                }
                let mut part = None;
                let mut content_type = None;
                for attr in tag.attributes() {
                    let attr = attr.map_err(|_| Status::Malformed)?;
                    if matches!(
                        reader.resolver().resolve_attribute(attr.key).0,
                        ResolveResult::Unknown(_)
                    ) {
                        return Err(Status::Malformed);
                    }
                    let value = attr
                        .decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder())
                        .map_err(|_| Status::Malformed)?
                        .into_owned();
                    if attr.key.as_ref() == b"PartName" {
                        part = Some(value.clone());
                    }
                    if attr.key.as_ref() == b"ContentType" {
                        content_type = Some(value);
                    }
                }
                if let Some(kind) = content_type {
                    let lowercase = kind.to_ascii_lowercase();
                    if lowercase.contains("macroenabled") || lowercase.contains("vbaproject") {
                        return Err(Status::Unsupported);
                    }
                    if part.as_deref() == Some("/word/document.xml") {
                        if main_seen || kind != MAIN_TYPE {
                            return Err(Status::Unsupported);
                        }
                        main_seen = true;
                    }
                }
            }
            Event::End(_) => {
                depth = depth.checked_sub(1).ok_or(Status::Malformed)?;
            }
            Event::DocType(_) | Event::GeneralRef(_) => return Err(Status::Malformed),
            Event::Eof => break,
            Event::Text(t) => {
                if !t.decode().map_err(|_| Status::Malformed)?.trim().is_empty() {
                    return Err(Status::Malformed);
                }
            }
            Event::CData(_) => return Err(Status::Malformed),
            Event::Decl(declaration) => {
                if events != 1 {
                    return Err(Status::Malformed);
                }
                validate_declaration(&declaration)?;
            }
            Event::PI(instruction) if instruction.target().eq_ignore_ascii_case(b"xml") => {
                return Err(Status::Malformed)
            }
            _ => {}
        }
    }
    if depth != 0 || !root_seen || !main_seen {
        return Err(Status::Malformed);
    }
    Ok(())
}

struct Element {
    local: Vec<u8>,
    skip: bool,
}

fn parse_main(xml: &[u8], text_limit: usize) -> Result<ExtractedDocumentText, Status> {
    validate_xml_characters(xml)?;
    let mut reader = NsReader::from_reader(xml);
    reader.config_mut().expand_empty_elements = true;
    let mut stack: Vec<Element> = Vec::new();
    let mut root_seen = false;
    let mut body_seen = false;
    let mut body_depth = None;
    let mut paragraph_depth = None;
    let mut paragraph = 0u32;
    let mut text = String::new();
    let mut field_depth = 0u32;
    let mut overflow = false;
    let mut text_limited = false;
    let mut paragraph_limited = false;
    let mut blocks = Vec::new();
    let mut stored_bytes = 0;
    let mut events = 0;
    loop {
        events += 1;
        if events > 100_000 {
            return Err(Status::TooLarge);
        }
        let event = reader.read_event().map_err(|_| Status::Malformed)?;
        match event {
            Event::Start(ref tag) => {
                let depth = stack.len() + 1;
                if depth > 64 {
                    return Err(Status::TooLarge);
                }
                let (ns, local) = reader.resolver().resolve_element(tag.name());
                if matches!(ns, ResolveResult::Unknown(_)) {
                    return Err(Status::Malformed);
                }
                let word = matches!(ns, ResolveResult::Bound(ns) if ns.as_ref() == W);
                let local = local.as_ref();
                if depth == 1 {
                    if root_seen {
                        return Err(Status::Malformed);
                    }
                    if !word || local != b"document" {
                        return Err(Status::Unsupported);
                    }
                    root_seen = true;
                }
                let skip = stack.last().is_some_and(|e| e.skip)
                    || !word
                    || matches!(
                        local,
                        b"rt"
                            | b"drawing"
                            | b"pict"
                            | b"object"
                            | b"txbxContent"
                            | b"del"
                            | b"moveFrom"
                            | b"fldSimple"
                    );
                if word && local == b"body" {
                    if depth != 2 || body_seen {
                        return Err(Status::Malformed);
                    }
                    body_seen = true;
                    body_depth = Some(depth);
                }
                if !skip && body_depth.is_some() && local == b"p" {
                    if paragraph_depth.is_some() {
                        return Err(Status::Malformed);
                    }
                    paragraph += 1;
                    paragraph_depth = Some(depth);
                    text.clear();
                    overflow = false;
                    if paragraph > MAX_DOCUMENT_PARAGRAPHS as u32 {
                        paragraph_limited = true;
                    }
                }
                for attr in tag.attributes() {
                    let attr = attr.map_err(|_| Status::Malformed)?;
                    if matches!(
                        reader.resolver().resolve_attribute(attr.key).0,
                        ResolveResult::Unknown(_)
                    ) {
                        return Err(Status::Malformed);
                    }
                    attr.decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder())
                        .map_err(|_| Status::Malformed)?;
                }
                if !skip && local == b"fldChar" {
                    for attr in tag.attributes() {
                        let attr = attr.map_err(|_| Status::Malformed)?;
                        let (ns, local) = reader.resolver().resolve_attribute(attr.key);
                        if matches!(ns, ResolveResult::Bound(ns) if ns.as_ref() == W)
                            && local.as_ref() == b"fldCharType"
                        {
                            match attr
                                .decoded_and_normalized_value(
                                    XmlVersion::Implicit1_0,
                                    reader.decoder(),
                                )
                                .map_err(|_| Status::Malformed)?
                                .as_ref()
                            {
                                "begin" => field_depth += 1,
                                "end" => {
                                    field_depth =
                                        field_depth.checked_sub(1).ok_or(Status::Malformed)?
                                }
                                "separate" => {}
                                _ => return Err(Status::Malformed),
                            }
                        }
                    }
                }
                if !skip && paragraph_depth.is_some() && field_depth == 0 {
                    let added = match local {
                        b"tab" => "\t",
                        b"br" | b"cr" => "\n",
                        _ => "",
                    };
                    append_text(&mut text, added, &mut overflow, text_limit);
                }
                stack.push(Element {
                    local: if word { local.to_vec() } else { Vec::new() },
                    skip,
                });
            }
            Event::End(_) => {
                let depth = stack.len();
                if paragraph_depth == Some(depth) {
                    if !text.trim().is_empty() && !text_limited && !paragraph_limited {
                        let cost = if text_limit == MAX_DOCUMENT_RENDERED_BYTES {
                            serde_text_cost(&text) + 240
                        } else {
                            text.len()
                        };
                        if overflow || stored_bytes + cost > text_limit {
                            text_limited = true;
                        } else {
                            stored_bytes += cost;
                            blocks.push(DocumentBlock {
                                locator: DocumentLocator::DocxParagraph { paragraph },
                                text: text.clone(),
                            });
                        }
                    } else if overflow {
                        text_limited = true;
                    }
                    paragraph_depth = None;
                }
                if body_depth == Some(depth) {
                    body_depth = None;
                }
                stack.pop().ok_or(Status::Malformed)?;
            }
            Event::Text(value) => {
                let decoded = value.decode().map_err(|_| Status::Malformed)?;
                if stack.last().is_some_and(|e| !e.skip && e.local == b"t")
                    && paragraph_depth.is_some()
                    && field_depth == 0
                {
                    append_text(&mut text, &decoded, &mut overflow, text_limit);
                } else if stack.is_empty() && !decoded.trim().is_empty() {
                    return Err(Status::Malformed);
                }
            }
            Event::CData(value) => {
                if stack.is_empty() {
                    return Err(Status::Malformed);
                }
                let decoded = value.decode().map_err(|_| Status::Malformed)?;
                if stack.last().is_some_and(|e| !e.skip && e.local == b"t")
                    && paragraph_depth.is_some()
                    && field_depth == 0
                {
                    append_text(&mut text, &decoded, &mut overflow, text_limit);
                }
            }
            Event::GeneralRef(value) => {
                if stack.is_empty() {
                    return Err(Status::Malformed);
                }
                let reference = value.decode().map_err(|_| Status::Malformed)?;
                let decoded =
                    if let Some(c) = value.resolve_char_ref().map_err(|_| Status::Malformed)? {
                        if !valid_xml_character(c) {
                            return Err(Status::Malformed);
                        }
                        c.to_string()
                    } else {
                        match reference.as_ref() {
                            "amp" => "&",
                            "lt" => "<",
                            "gt" => ">",
                            "quot" => "\"",
                            "apos" => "'",
                            _ => return Err(Status::Malformed),
                        }
                        .to_string()
                    };
                if stack.last().is_some_and(|e| !e.skip && e.local == b"t")
                    && paragraph_depth.is_some()
                    && field_depth == 0
                {
                    append_text(&mut text, &decoded, &mut overflow, text_limit);
                }
            }
            Event::DocType(_) => return Err(Status::Malformed),
            Event::Decl(declaration) => {
                if events != 1 {
                    return Err(Status::Malformed);
                }
                validate_declaration(&declaration)?;
            }
            Event::PI(instruction) if instruction.target().eq_ignore_ascii_case(b"xml") => {
                return Err(Status::Malformed)
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !root_seen || !body_seen || !stack.is_empty() || field_depth != 0 {
        return Err(Status::Malformed);
    }
    let mut omissions = vec![Omission::MainBodyOnly];
    if text_limited {
        omissions.push(Omission::TextLimit);
    }
    if paragraph_limited {
        omissions.push(Omission::ParagraphLimit);
    }
    let status = if blocks.is_empty() {
        if text_limited || paragraph_limited {
            Status::ContextOmitted
        } else {
            Status::NoText
        }
    } else if text_limited || paragraph_limited {
        Status::Partial
    } else {
        Status::Ready
    };
    Ok(ExtractedDocumentText {
        version: DOCUMENT_CONTENT_VERSION,
        format: DocumentFormat::Docx,
        status,
        blocks,
        omissions,
        pdf_coverage: None,
        xlsx_coverage: None,
    })
}

fn append_text(text: &mut String, value: &str, overflow: &mut bool, text_limit: usize) {
    if *overflow {
        return;
    }
    if text.len().saturating_add(value.len()) > text_limit {
        *overflow = true;
    } else {
        text.push_str(value);
    }
}

fn serde_text_cost(text: &str) -> usize {
    2 + text
        .chars()
        .map(|c| match c {
            '"' | '\\' | '\n' | '\r' | '\t' | '\u{8}' | '\u{c}' => 2,
            c if c <= '\u{1f}' => 6,
            c => c.len_utf8(),
        })
        .sum::<usize>()
}

pub(super) fn valid_xml_character(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r') || c >= '\u{20}' && c != '\u{fffe}' && c != '\u{ffff}'
}

pub(super) fn validate_declaration(value: &quick_xml::events::BytesDecl<'_>) -> Result<(), Status> {
    if value.version().map_err(|_| Status::Malformed)?.as_ref() != b"1.0" {
        return Err(Status::Unsupported);
    }
    if let Some(encoding) = value.encoding() {
        let encoding = encoding.map_err(|_| Status::Malformed)?;
        if !encoding.eq_ignore_ascii_case(b"UTF-8") && !encoding.eq_ignore_ascii_case(b"US-ASCII") {
            return Err(Status::Unsupported);
        }
    }
    // BytesDecl accessors return the first known attribute and intentionally do
    // not validate duplicate/unknown attributes or XML declaration ordering.
    let content = std::str::from_utf8(value).map_err(|_| Status::Malformed)?;
    let declaration = quick_xml::events::BytesStart::from_content(content, 3);
    let mut previous = 0;
    for attribute in declaration.attributes() {
        let attribute = attribute.map_err(|_| Status::Malformed)?;
        let order = match attribute.key.as_ref() {
            b"version" => 1,
            b"encoding" => 2,
            b"standalone" => {
                if !matches!(attribute.value.as_ref(), b"yes" | b"no") {
                    return Err(Status::Malformed);
                }
                3
            }
            _ => return Err(Status::Malformed),
        };
        if order <= previous {
            return Err(Status::Malformed);
        }
        previous = order;
    }
    Ok(())
}

pub(super) fn validate_xml_characters(xml: &[u8]) -> Result<(), Status> {
    let xml = std::str::from_utf8(xml).map_err(|_| Status::Malformed)?;
    if !xml.chars().all(valid_xml_character) {
        return Err(Status::Malformed);
    }
    Ok(())
}
