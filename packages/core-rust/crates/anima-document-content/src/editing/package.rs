//! Complete archive admission; retained parts are never extracted or executed.
use super::{
    types::{unsupported, OfficeEditError as Error, OfficeFormat, Result, RetainedOfficeHyperlink},
    xml_spans::{self, Node, CT, R, REL, W},
};
use rawzip::{CompressionMethod, ZipArchive};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
};

pub(super) const MAX_BYTES: usize = 4 * 1024 * 1024;
#[derive(Debug, Clone)]
pub(super) struct Part {
    pub name: String,
    pub bytes: Vec<u8>,
    pub xml: Option<Node>,
}
#[derive(Debug)]
pub(super) struct Relationship {
    pub kind: String,
    pub target: String,
    pub external: bool,
}
#[derive(Debug)]
pub(super) struct Package {
    pub parts: Vec<Part>,
    pub main: String,
    pub links: Vec<RetainedOfficeHyperlink>,
    pub relationships: BTreeMap<String, BTreeMap<String, Relationship>>,
    pub has_pivot: bool,
}

pub(super) fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 512
        && !name.starts_with('/')
        && !name
            .chars()
            .any(|c| c.is_control() || matches!(c, '\\' | ':' | '%' | '?' | '#'))
        && name.split('/').all(|s| !matches!(s, "" | "." | ".."))
}
fn resolve(base: &str, target: &str) -> Result<String> {
    if target.is_empty()
        || target.len() > 512
        || target.starts_with('/')
        || target
            .chars()
            .any(|c| c.is_control() || matches!(c, '\\' | ':' | '%' | '?' | '#'))
    {
        return Err(Error::InvalidPackage);
    }
    let mut pieces: Vec<&str> = base
        .rsplit_once('/')
        .map_or(Vec::new(), |(p, _)| p.split('/').collect());
    for p in target.split('/') {
        match p {
            "" => return Err(Error::InvalidPackage),
            "." => {}
            ".." => {
                pieces.pop().ok_or(Error::InvalidPackage)?;
            }
            _ => pieces.push(p),
        }
    }
    let path = pieces.join("/");
    if !safe_name(&path) {
        return Err(Error::InvalidPackage);
    }
    Ok(path)
}
fn rel_owner(name: &str) -> Result<String> {
    if name == "_rels/.rels" {
        return Ok(String::new());
    }
    let (base, file) = name.rsplit_once("/_rels/").ok_or(Error::InvalidPackage)?;
    Ok(format!(
        "{base}/{}",
        file.strip_suffix(".rels").ok_or(Error::InvalidPackage)?
    ))
}

/// Recognize retained inert hyperlinks without providing any fetching capability.
fn safe_hyperlink(target: &str) -> bool {
    if target.is_empty()
        || target.len() > 2048
        || target.chars().any(|c| {
            c.is_control() || c.is_whitespace() || matches!(c, '\\' | '<' | '>' | '"' | '\'')
        })
    {
        return false;
    }
    let raw = target.as_bytes();
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'%' {
            let Some(pair) = raw.get(i + 1..i + 3) else {
                return false;
            };
            let Ok(text) = std::str::from_utf8(pair) else {
                return false;
            };
            let Ok(byte) = u8::from_str_radix(text, 16) else {
                return false;
            };
            if byte <= 32 || byte == 127 || byte == b'\\' {
                return false;
            }
            i += 3;
        } else {
            i += 1;
        }
    }
    let lower = target.to_ascii_lowercase();
    if [
        "password=",
        "passwd=",
        "token=",
        "secret=",
        "api_key=",
        "apikey=",
    ]
    .iter()
    .any(|key| lower.contains(key))
    {
        return false;
    }
    let Some((scheme, rest)) = target.split_once(':') else {
        return false;
    };
    match scheme.to_ascii_lowercase().as_str() {
        "http" | "https" => {
            let Some(rest) = rest.strip_prefix("//") else {
                return false;
            };
            let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
            valid_authority(authority)
        }
        "mailto" => {
            let address = rest.split('?').next().unwrap_or("");
            let Some((local, domain)) = address.split_once('@') else {
                return false;
            };
            !local.is_empty()
                && !domain.is_empty()
                && !domain.contains('@')
                && local
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'+' | b'-'))
                && domain
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-'))
        }
        _ => false,
    }
}
fn valid_authority(authority: &str) -> bool {
    if authority.is_empty() || authority.contains(['@', '%']) {
        return false;
    }
    let valid_port = |port: &str| {
        !port.is_empty() && port.bytes().all(|c| c.is_ascii_digit()) && port.parse::<u16>().is_ok()
    };
    if let Some(rest) = authority.strip_prefix('[') {
        let Some((address, tail)) = rest.split_once(']') else {
            return false;
        };
        return address.parse::<std::net::Ipv6Addr>().is_ok()
            && (tail.is_empty() || tail.strip_prefix(':').is_some_and(valid_port));
    }
    let host = if let Some((host, port)) = authority.rsplit_once(':') {
        if !valid_port(port) {
            return false;
        }
        host
    } else {
        authority
    };
    let host = host.strip_suffix('.').unwrap_or(host);
    !host.is_empty()
        && host.len() <= 253
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-')
        })
}

impl Package {
    pub fn open(format: OfficeFormat, bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_BYTES {
            return Err(Error::ResourceLimit);
        }
        let archive = ZipArchive::from_slice(bytes).map_err(|_| Error::InvalidPackage)?;
        crate::docx::check_archive_policy(bytes, archive.eocd_offset() as usize)
            .map_err(|_| Error::InvalidPackage)?;
        let eocd = archive.eocd_offset() as usize;
        if eocd + 22 + u16::from_le_bytes([bytes[eocd + 20], bytes[eocd + 21]]) as usize
            != bytes.len()
        {
            return Err(Error::InvalidPackage);
        }
        if archive.entries_hint() > 1024 {
            return Err(Error::ResourceLimit);
        }
        let mut entries = archive.entries();
        let mut parts = Vec::new();
        let mut names = BTreeSet::new();
        let mut ranges = Vec::new();
        let mut expanded = 0usize;
        let mut events = 0usize;
        while let Some(entry) = entries.next_entry().map_err(|_| Error::InvalidPackage)? {
            if parts.len() >= 1024 {
                return Err(Error::ResourceLimit);
            }
            let name = std::str::from_utf8(entry.file_path().as_bytes())
                .map_err(|_| Error::InvalidPackage)?
                .to_owned();
            let directory = name.ends_with('/');
            if !safe_name(name.strip_suffix('/').unwrap_or(&name))
                || !names.insert(name.to_ascii_lowercase())
            {
                return Err(Error::InvalidPackage);
            }
            let central = entry.central_directory_offset() as usize;
            let flags = entry.flags();
            let method = entry.compression_method();
            if flags.is_encrypted()
                || flags.has_strong_encryption()
                || flags.is_masked()
                || !matches!(
                    method,
                    CompressionMethod::STORE | CompressionMethod::DEFLATE
                )
            {
                return Err(unsupported("ZIP encryption or compression"));
            }
            let raw_flags = bytes
                .get(central + 8..central + 10)
                .ok_or(Error::InvalidPackage)?;
            let bits = u16::from_le_bytes([raw_flags[0], raw_flags[1]]);
            if bits
                & !(if method == CompressionMethod::DEFLATE {
                    0x080e
                } else {
                    0x0808
                })
                != 0
                || bytes.get(central + 34..central + 36) != Some(&[0, 0])
            {
                return Err(Error::InvalidPackage);
            }
            let attrs = bytes
                .get(central + 38..central + 42)
                .ok_or(Error::InvalidPackage)?;
            let attrs = u32::from_le_bytes([attrs[0], attrs[1], attrs[2], attrs[3]]);
            if (attrs >> 16) & 0o170000 == 0o120000 {
                return Err(unsupported("archive links"));
            }
            crate::docx::extras_ok(entry.extra_fields()).map_err(|_| Error::InvalidPackage)?;
            let local = archive
                .get_entry(entry.wayfinder())
                .map_err(|_| Error::InvalidPackage)?;
            let header = local.local_header();
            if header.file_path().as_bytes() != name.as_bytes()
                || header.compression_method() != method
                || header.flags() != flags
            {
                return Err(Error::InvalidPackage);
            }
            crate::docx::extras_ok(header.extra_fields()).map_err(|_| Error::InvalidPackage)?;
            if !flags.has_data_descriptor()
                && (header.crc32() != entry.crc32()
                    || header.compressed_size_hint() != entry.compressed_size_hint()
                    || header.uncompressed_size_hint() != entry.uncompressed_size_hint())
            {
                return Err(Error::InvalidPackage);
            }
            let (_, mut end) = local.compressed_data_range();
            if let Some(descriptor) = local.data_descriptor().map_err(|_| Error::InvalidPackage)? {
                if descriptor.crc32() != entry.crc32()
                    || descriptor.compressed_size() != entry.compressed_size_hint()
                    || descriptor.uncompressed_size() != entry.uncompressed_size_hint()
                {
                    return Err(Error::InvalidPackage);
                }
                end += if bytes.get(end as usize..end as usize + 4) == Some(b"PK\x07\x08") {
                    16
                } else {
                    12
                };
            }
            if end > archive.directory_offset() || entry.local_header_offset() >= end {
                return Err(Error::InvalidPackage);
            }
            ranges.push((entry.local_header_offset(), end));
            let remaining = (16 * 1024 * 1024usize)
                .checked_sub(expanded)
                .ok_or(Error::ResourceLimit)?;
            let body = crate::docx::read_part(
                &archive,
                entry.wayfinder(),
                method,
                MAX_BYTES.min(remaining),
            )
            .map_err(|_| Error::InvalidPackage)?;
            expanded += body.len();
            let xml = if name.ends_with(".xml") || name.ends_with(".rels") {
                Some(xml_spans::parse(&body, &mut events)?)
            } else {
                None
            };
            if directory && !body.is_empty() {
                return Err(Error::InvalidPackage);
            }
            parts.push(Part {
                name,
                bytes: body,
                xml,
            });
        }
        ranges.sort_unstable();
        if parts.len() as u64 != archive.entries_hint()
            || ranges.windows(2).any(|p| p[0].1 > p[1].0)
        {
            return Err(Error::InvalidPackage);
        }
        let mut result = Self {
            parts,
            main: String::new(),
            links: Vec::new(),
            relationships: BTreeMap::new(),
            has_pivot: false,
        };
        result.validate(format)?;
        Ok(result)
    }
    pub fn part(&self, name: &str) -> Result<&Part> {
        self.parts
            .iter()
            .find(|p| p.name == name)
            .ok_or(Error::InvalidPackage)
    }
    pub fn xml(&self, name: &str) -> Result<&Node> {
        self.part(name)?.xml.as_ref().ok_or(Error::InvalidPackage)
    }
    pub fn rels(&self, name: &str) -> Result<&BTreeMap<String, Relationship>> {
        self.relationships.get(name).ok_or(Error::InvalidPackage)
    }
    pub fn relation(&self, name: &str, id: &str) -> Result<&Relationship> {
        self.rels(name)?.get(id).ok_or(Error::InvalidPackage)
    }
    fn validate(&mut self, format: OfficeFormat) -> Result<()> {
        let types = self.xml("[Content_Types].xml")?;
        if !types.is(CT, "Types") {
            return Err(Error::InvalidPackage);
        }
        let mut overrides = BTreeMap::new();
        let mut defaults = BTreeMap::new();
        for node in &types.children {
            let kind = node.required("ContentType")?.to_owned();
            if node.is(CT, "Override") {
                let path = node
                    .required("PartName")?
                    .strip_prefix('/')
                    .ok_or(Error::InvalidPackage)?;
                if !safe_name(path) || overrides.insert(path.to_owned(), kind).is_some() {
                    return Err(Error::InvalidPackage);
                }
            } else if node.is(CT, "Default") {
                let ext = node.required("Extension")?;
                if !ext.bytes().all(|b| b.is_ascii_alphanumeric())
                    || defaults.insert(ext.to_ascii_lowercase(), kind).is_some()
                {
                    return Err(Error::InvalidPackage);
                }
            } else {
                return Err(Error::InvalidPackage);
            }
        }
        for part in &self.parts {
            if part.name.ends_with('/') || part.name == "[Content_Types].xml" {
                continue;
            }
            let kind = overrides
                .get(&part.name)
                .or_else(|| {
                    part.name
                        .rsplit_once('.')
                        .and_then(|(_, ext)| defaults.get(&ext.to_ascii_lowercase()))
                })
                .ok_or_else(|| unsupported("missing content type"))?;
            if !allowed_type(kind) {
                return Err(unsupported("active or unknown content type"));
            }
            if matches!(kind.as_str(), "application/vnd.openxmlformats-officedocument.spreadsheetml.pivotTable+xml" | "application/vnd.openxmlformats-officedocument.spreadsheetml.pivotCacheDefinition+xml" | "application/vnd.openxmlformats-officedocument.spreadsheetml.pivotCacheRecords+xml") {
                self.has_pivot = true;
            }
            if part.xml.is_none() {
                if !((kind == "image/png" && part.bytes.starts_with(b"\x89PNG\r\n\x1a\n"))
                    || (kind == "image/jpeg" && part.bytes.starts_with(b"\xff\xd8\xff")))
                {
                    return Err(unsupported("media format"));
                }
            }
            if let Some(root) = &part.xml {
                validate_content(root)?;
            }
            if part.name.ends_with(".rels") {
                let owner = rel_owner(&part.name)?;
                if !owner.is_empty() {
                    self.part(&owner)?;
                }
                let root = part.xml.as_ref().ok_or(Error::InvalidPackage)?;
                if !root.is(REL, "Relationships") {
                    return Err(Error::InvalidPackage);
                }
                let mut rels = BTreeMap::new();
                for node in &root.children {
                    if !node.is(REL, "Relationship") || !node.children.is_empty() {
                        return Err(Error::InvalidPackage);
                    }
                    let id = node.required("Id")?;
                    let kind = node.required("Type")?;
                    if matches!(
                        kind.rsplit('/').next(),
                        Some("pivotTable" | "pivotCacheDefinition" | "pivotCacheRecords")
                    ) {
                        self.has_pivot = true;
                    }
                    let raw = node.required("Target")?;
                    if !allowed_relation(kind) {
                        return Err(unsupported("active or unknown relationship"));
                    }
                    let external = match node.attr("TargetMode") {
                        None | Some("Internal") => false,
                        Some("External") => true,
                        _ => return Err(Error::InvalidPackage),
                    };
                    let target = if external {
                        if kind != format!("{R}/hyperlink") || !safe_hyperlink(raw) {
                            return Err(unsupported("active or unsafe external relationship"));
                        }
                        self.links.push(RetainedOfficeHyperlink {
                            relationship_id: format!("{}:{id}", part.name),
                            target: raw.into(),
                        });
                        raw.to_owned()
                    } else {
                        let target = resolve(&owner, raw)?;
                        self.part(&target)?;
                        target
                    };
                    if id.is_empty()
                        || rels
                            .insert(
                                id.to_owned(),
                                Relationship {
                                    kind: kind.into(),
                                    target,
                                    external,
                                },
                            )
                            .is_some()
                    {
                        return Err(Error::InvalidPackage);
                    }
                }
                self.relationships.insert(owner, rels);
            }
        }
        let roots = self.rels("")?;
        let mains = roots
            .values()
            .filter(|r| r.kind == format!("{R}/officeDocument") && !r.external)
            .collect::<Vec<_>>();
        if mains.len() != 1 {
            return Err(Error::InvalidPackage);
        }
        self.main = mains[0].target.clone();
        let expected = match format {
            OfficeFormat::Docx => "wordprocessingml.document.main",
            OfficeFormat::Xlsx => "spreadsheetml.sheet.main",
            OfficeFormat::Pptx => "presentationml.presentation.main",
            OfficeFormat::Html => return Err(Error::InvalidPackage),
        };
        if overrides.get(&self.main).map(String::as_str)
            != Some(
                format!("application/vnd.openxmlformats-officedocument.{expected}+xml").as_str(),
            )
        {
            return Err(unsupported("document format mismatch"));
        }
        for part in &self.parts {
            if let Some(root) = &part.xml {
                self.validate_refs(&part.name, root, 0)?;
            }
        }
        let names = if format == OfficeFormat::Xlsx {
            let sheets = super::spreadsheet::sheets(self)?;
            super::spreadsheet::fixed_names(self, &sheets)?
        } else {
            BTreeSet::new()
        };
        for part in &self.parts {
            if let Some(root) = &part.xml {
                validate_formulas(root, &names)?;
            }
        }
        Ok(())
    }
    fn validate_refs(&self, part: &str, node: &Node, parent_context: u8) -> Result<()> {
        for ((ns, attribute), id) in &node.attrs {
            if ns == R {
                let relation = self.relation(part, id)?;
                let expected = match (node.ns.as_str(), node.name.as_str(), attribute.as_str()) {
                    (super::xml_spans::A, "blip", "embed" | "link") => "image",
                    (W, "hyperlink", "id")
                    | (super::xml_spans::A, "hlinkClick" | "hlinkMouseOver", "id") => "hyperlink",
                    (W, "headerReference", "id") => "header",
                    (W, "footerReference", "id") => "footer",
                    (super::xml_spans::S, "sheet", "id") => "worksheet",
                    (super::xml_spans::S, "drawing", "id") => "drawing",
                    (super::xml_spans::S, "tablePart", "id") => "table",
                    (super::xml_spans::S, "pivotTablePart", "id") => "pivotTable",
                    (super::xml_spans::S, "pivotCache", "id") => "pivotCacheDefinition",
                    (super::xml_spans::C, "chart", "id") => "chart",
                    (super::xml_spans::P, "sldId", "id") => "slide",
                    (super::xml_spans::P, "sld", "id")
                        if parent_context == 4 && part == self.main =>
                    {
                        let list = self
                            .xml(&self.main)?
                            .child(super::xml_spans::P, "sldIdLst")
                            .ok_or(Error::InvalidPackage)?;
                        if !list.children.iter().any(|slide| {
                            slide.is(super::xml_spans::P, "sldId")
                                && slide.attr_ns(R, "id") == Some(id)
                        }) {
                            return Err(Error::InvalidPackage);
                        }
                        "slide"
                    }
                    (super::xml_spans::P, "sldMasterId", "id") => "slideMaster",
                    (super::xml_spans::P, "sldLayoutId", "id") => "slideLayout",
                    (super::xml_spans::P, "notesMasterId", "id") => "notesMaster",
                    (super::xml_spans::P, "handoutMasterId", "id") => "handoutMaster",
                    _ => return Err(unsupported("relationship consumer")),
                };
                if relation.kind != format!("{R}/{expected}")
                    || (relation.external && expected != "hyperlink")
                {
                    return Err(unsupported("relationship type does not match its consumer"));
                }
                if expected == "image" && self.part(&relation.target)?.xml.is_some() {
                    return Err(unsupported("nonimage drawing relationship"));
                }
            }
        }
        let context = match (parent_context, node.ns.as_str(), node.name.as_str()) {
            (0, super::xml_spans::P, "presentation") if part == self.main => 1,
            (1, super::xml_spans::P, "custShowLst") => 2,
            (2, super::xml_spans::P, "custShow") => 3,
            (3, super::xml_spans::P, "sldLst") => 4,
            _ => 0,
        };
        for child in &node.children {
            self.validate_refs(part, child, context)?;
        }
        Ok(())
    }
    pub fn encode(
        &self,
        replacements: &BTreeMap<String, Vec<u8>>,
        removed: &BTreeSet<String>,
    ) -> Result<Vec<u8>> {
        let mut output = crate::generation::Output::with_limit(MAX_BYTES);
        let mut zip = rawzip::ZipArchiveWriter::new(&mut output);
        for part in &self.parts {
            if removed.contains(&part.name) {
                continue;
            }
            let bytes = replacements.get(&part.name).unwrap_or(&part.bytes);
            let (mut entry, config) = zip
                .new_file(&part.name)
                .compression_method(CompressionMethod::DEFLATE)
                .start()
                .map_err(|_| Error::ResourceLimit)?;
            let encoder =
                flate2::write::DeflateEncoder::new(&mut entry, flate2::Compression::default());
            let mut writer = config.wrap(encoder);
            writer.write_all(bytes).map_err(|_| Error::ResourceLimit)?;
            let (encoder, descriptor) = writer.finish().map_err(|_| Error::ResourceLimit)?;
            encoder.finish().map_err(|_| Error::ResourceLimit)?;
            entry.finish(descriptor).map_err(|_| Error::ResourceLimit)?;
        }
        zip.finish().map_err(|_| Error::ResourceLimit)?;
        Ok(output.finish())
    }
}

fn allowed_type(kind: &str) -> bool {
    if matches!(
        kind,
        "application/xml"
            | "image/png"
            | "image/jpeg"
            | "application/vnd.openxmlformats-package.relationships+xml"
            | "application/vnd.openxmlformats-package.core-properties+xml"
    ) {
        return true;
    }
    let Some(kind) = kind
        .strip_prefix("application/vnd.openxmlformats-officedocument.")
        .and_then(|x| x.strip_suffix("+xml"))
    else {
        return false;
    };
    matches!(
        kind,
        "extended-properties"
            | "custom-properties"
            | "theme"
            | "themeOverride"
            | "drawing"
            | "drawingml.chart"
            | "wordprocessingml.document.main"
            | "wordprocessingml.styles"
            | "wordprocessingml.numbering"
            | "wordprocessingml.settings"
            | "wordprocessingml.webSettings"
            | "wordprocessingml.fontTable"
            | "wordprocessingml.header"
            | "wordprocessingml.footer"
            | "wordprocessingml.footnotes"
            | "wordprocessingml.endnotes"
            | "wordprocessingml.comments"
            | "spreadsheetml.sheet.main"
            | "spreadsheetml.worksheet"
            | "spreadsheetml.styles"
            | "spreadsheetml.sharedStrings"
            | "spreadsheetml.calcChain"
            | "spreadsheetml.table"
            | "spreadsheetml.comments"
            | "spreadsheetml.pivotTable"
            | "spreadsheetml.pivotCacheDefinition"
            | "spreadsheetml.pivotCacheRecords"
            | "presentationml.presentation.main"
            | "presentationml.slide"
            | "presentationml.slideMaster"
            | "presentationml.slideLayout"
            | "presentationml.notesSlide"
            | "presentationml.notesMaster"
            | "presentationml.handoutMaster"
            | "presentationml.presProps"
            | "presentationml.viewProps"
            | "presentationml.tableStyles"
    )
}
fn allowed_relation(kind: &str) -> bool {
    if matches!(
        kind,
        "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties"
    ) {
        return true;
    }
    let Some(kind) = kind.strip_prefix(&format!("{R}/")) else {
        return false;
    };
    matches!(
        kind,
        "officeDocument"
            | "extended-properties"
            | "custom-properties"
            | "styles"
            | "numbering"
            | "settings"
            | "webSettings"
            | "fontTable"
            | "header"
            | "footer"
            | "footnotes"
            | "endnotes"
            | "comments"
            | "theme"
            | "themeOverride"
            | "image"
            | "hyperlink"
            | "worksheet"
            | "sharedStrings"
            | "calcChain"
            | "drawing"
            | "chart"
            | "table"
            | "pivotTable"
            | "pivotCacheDefinition"
            | "pivotCacheRecords"
            | "slide"
            | "slideMaster"
            | "slideLayout"
            | "notesSlide"
            | "notesMaster"
            | "handoutMaster"
            | "presProps"
            | "viewProps"
            | "tableStyles"
    )
}
fn validate_content(node: &Node) -> Result<()> {
    if matches!(
        node.name.as_str(),
        "altChunk"
            | "object"
            | "oleObj"
            | "control"
            | "externalData"
            | "attachedTemplate"
            | "connection"
            | "webPublishObject"
            | "macro"
            | "script"
    ) {
        return Err(unsupported("active embedded content"));
    }
    if node.is(W, "fldSimple") {
        let instruction = node.attr_ns(W, "instr").ok_or(Error::InvalidPackage)?;
        if !matches!(
            instruction.split_whitespace().next(),
            Some("PAGE" | "NUMPAGES")
        ) {
            return Err(unsupported("Word field instruction"));
        }
    }
    if node.is(W, "instrText") && !matches!(node.text.trim(), "PAGE" | "NUMPAGES") {
        return Err(unsupported("Word field instruction"));
    }
    if node.is(super::xml_spans::S, "definedName") {
        let name = node.attr("name").unwrap_or("").to_ascii_lowercase();
        if name.ends_with("auto_open")
            || name.ends_with("auto_close")
            || ["function", "vbProcedure", "xlm"]
                .iter()
                .any(|key| matches!(node.attr(key), Some("1" | "true")))
        {
            return Err(unsupported("executable defined name"));
        }
    }
    for ((_, name), value) in &node.attrs {
        if name == "action"
            && !matches!(
                value.as_str(),
                "" | "ppaction://hlinksldjump"
                    | "ppaction://hlinkshowjump?jump=nextslide"
                    | "ppaction://hlinkshowjump?jump=previousslide"
                    | "ppaction://hlinkshowjump?jump=firstslide"
                    | "ppaction://hlinkshowjump?jump=lastslide"
                    | "ppaction://hlinkshowjump?jump=endshow"
            )
        {
            return Err(unsupported("executable presentation action"));
        }
    }
    for child in &node.children {
        validate_content(child)?;
    }
    Ok(())
}

fn validate_formulas(node: &Node, names: &BTreeSet<String>) -> Result<()> {
    if node.ns == super::xml_spans::S
        && matches!(
            node.name.as_str(),
            "f" | "formula"
                | "formula1"
                | "formula2"
                | "calculatedColumnFormula"
                | "totalsRowFormula"
        )
        && !node.text.is_empty()
    {
        super::formulas::admit(&node.text, names)?;
    }
    for child in &node.children {
        validate_formulas(child, names)?;
    }
    Ok(())
}
