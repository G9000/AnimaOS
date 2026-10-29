//! Private policy visitor for the full admitted package, including preserved-only parts.
use super::{
    package::Package,
    types::{OfficeEditError, OfficeFormat, Result},
    xml_spans::{A, S, W},
};
use quick_xml::{
    events::{BytesStart, Event},
    name::ResolveResult,
    NsReader, XmlVersion,
};

const CUSTOM_PROPERTIES: &[u8] =
    b"http://schemas.openxmlformats.org/officeDocument/2006/custom-properties";

/// Check retained logical XML content using a caller-supplied private policy.
///
/// The callback receives decoded document content, including preserved-only parts
/// and text joined across runs. It must not log, return, or otherwise expose that
/// content. The callback returns `true` only for text allowed by the policy;
/// this helper returns `false` if any text is rejected. Only that boolean is
/// returned. Parsing must run in the same physically isolated worker as Office
/// inspection and editing.
///
/// Each package is admitted and bounded before traversal. Word and DrawingML
/// paragraphs, spreadsheet cells, shared strings and comments are screened as
/// separate logical blocks so unrelated blocks are not joined. Individual XML
/// attribute values are also screened. Custom property names are screened with
/// their decoded values as logical `name=value` pairs. The callback determines
/// the policy; this helper does not change or serialize document bytes.
///
/// # Errors
/// Returns an error for malformed, unsupported or over-budget Office packages.
pub fn check_office_document_text(
    format: OfficeFormat,
    source: &[u8],
    mut policy: impl FnMut(&str) -> bool,
) -> Result<bool> {
    if format == OfficeFormat::Html {
        return super::html::check_text(source, policy);
    }
    let package = Package::open(format, source)?;
    for part in &package.parts {
        if part.xml.is_some() && !scan_xml(&part.bytes, &mut policy)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn boundary(reader: &NsReader<&[u8]>, tag: &BytesStart<'_>) -> bool {
    let (namespace, name) = reader.resolver().resolve_element(tag.name());
    let ResolveResult::Bound(namespace) = namespace else {
        return false;
    };
    let ns = namespace.as_ref();
    let local = name.as_ref();
    (ns == W.as_bytes() && local == b"p")
        || (ns == A.as_bytes() && local == b"p")
        || (ns == S.as_bytes() && matches!(local, b"c" | b"si" | b"comment"))
}

fn scan_xml(bytes: &[u8], policy: &mut impl FnMut(&str) -> bool) -> Result<bool> {
    fn screen(buffer: &mut String, policy: &mut impl FnMut(&str) -> bool) -> bool {
        let matched = !buffer.is_empty() && !policy(buffer);
        buffer.clear();
        matched
    }
    let mut reader = NsReader::from_reader(bytes);
    let mut blocks = Vec::new();
    let mut text = String::new();
    // (element depth, property name, accumulated child text)
    let mut custom_property: Option<(usize, String, String)> = None;
    loop {
        match reader
            .read_event()
            .map_err(|_| OfficeEditError::InvalidPackage)?
        {
            Event::Start(tag) | Event::Empty(tag) => {
                let is_empty = bytes.get(
                    (reader.buffer_position() as usize).saturating_sub(2)
                        ..reader.buffer_position() as usize,
                ) == Some(b"/>");
                let (namespace, local) = reader.resolver().resolve_element(tag.name());
                let is_custom_property = matches!(namespace, ResolveResult::Bound(ns) if ns.as_ref() == CUSTOM_PROPERTIES)
                    && local.as_ref() == b"property";
                let is_boundary = is_custom_property || boundary(&reader, &tag);
                if is_boundary && screen(&mut text, policy) {
                    return Ok(false);
                }
                let mut property_name = None;
                for attribute in tag.attributes() {
                    let attribute = attribute.map_err(|_| OfficeEditError::InvalidPackage)?;
                    let name = std::str::from_utf8(attribute.key.as_ref())
                        .map_err(|_| OfficeEditError::InvalidPackage)?;
                    if name == "xmlns" || name.starts_with("xmlns:") {
                        continue;
                    }
                    let value = attribute
                        .decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder())
                        .map_err(|_| OfficeEditError::InvalidPackage)?;
                    if !policy(&value) || !policy(&format!("{name}={value}")) {
                        return Ok(false);
                    }
                    if is_custom_property && name == "name" {
                        property_name = Some(value.into_owned());
                    }
                }
                if is_custom_property && !is_empty {
                    if custom_property.is_some() {
                        return Err(OfficeEditError::InvalidPackage);
                    }
                    if let Some(name) = property_name {
                        custom_property = Some((blocks.len(), name, String::new()));
                    }
                }
                if !is_empty {
                    blocks.push(is_boundary);
                }
            }
            Event::End(_) => {
                let is_boundary = blocks.pop().ok_or(OfficeEditError::InvalidPackage)?;
                if is_boundary && screen(&mut text, policy) {
                    return Ok(false);
                }
                if custom_property
                    .as_ref()
                    .is_some_and(|(depth, _, _)| *depth == blocks.len())
                {
                    let (_, name, value) = custom_property.take().unwrap();
                    if !policy(&format!("{name}={}", value.trim())) {
                        return Ok(false);
                    }
                }
            }
            Event::Text(value) => {
                let value = value
                    .decode()
                    .map_err(|_| OfficeEditError::InvalidPackage)?;
                text.push_str(&value);
                if let Some((_, _, property_value)) = &mut custom_property {
                    property_value.push_str(&value);
                }
            }
            Event::CData(value) => {
                let value = value
                    .decode()
                    .map_err(|_| OfficeEditError::InvalidPackage)?;
                text.push_str(&value);
                if let Some((_, _, property_value)) = &mut custom_property {
                    property_value.push_str(&value);
                }
            }
            Event::Comment(value) => {
                let comment = value
                    .decode()
                    .map_err(|_| OfficeEditError::InvalidPackage)?;
                if !policy(&comment) {
                    return Ok(false);
                }
            }
            Event::GeneralRef(value) => {
                let decoded = if let Some(character) = value
                    .resolve_char_ref()
                    .map_err(|_| OfficeEditError::InvalidPackage)?
                {
                    character.to_string()
                } else {
                    let decoded = value
                        .decode()
                        .map_err(|_| OfficeEditError::InvalidPackage)?;
                    match decoded.as_ref() {
                        "amp" => "&",
                        "lt" => "<",
                        "gt" => ">",
                        "quot" => "\"",
                        "apos" => "'",
                        _ => return Err(OfficeEditError::InvalidPackage),
                    }
                    .to_owned()
                };
                text.push_str(&decoded);
                if let Some((_, _, property_value)) = &mut custom_property {
                    property_value.push_str(&decoded);
                }
            }
            Event::Eof => break,
            _ => {}
        }
        if text.len() > super::package::MAX_BYTES
            || custom_property
                .as_ref()
                .is_some_and(|(_, _, value)| value.len() > super::package::MAX_BYTES)
        {
            return Err(OfficeEditError::ResourceLimit);
        }
    }
    if !blocks.is_empty() {
        return Err(OfficeEditError::InvalidPackage);
    }
    Ok(!screen(&mut text, policy))
}
