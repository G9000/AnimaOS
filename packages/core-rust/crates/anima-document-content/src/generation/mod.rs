//! Deterministic creation of a deliberately bounded, inert document subset.
//!
//! Version 1 source metadata is a consistency aid, never trusted on its own:
//! inspection regenerates the complete file and compares every byte. Changes to
//! version 1 rendering must retain the old renderer or introduce a new version.
mod office;
mod pdf;
mod schema;

use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fmt, io};

pub use schema::document_schema;
pub const MAX_GENERATED_DOCUMENT_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_GENERATED_SOURCE_BYTES: usize = 48 * 1024;
const MAX_TEXT_BYTES: usize = 8 * 1024;
const PDF_SOURCE_MARKER: &[u8] = b"\n%ANIMA-DOCUMENT:";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GeneratedFormat {
    Docx,
    Xlsx,
    Pptx,
    Pdf,
}

impl GeneratedFormat {
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Docx => "docx",
            Self::Xlsx => "xlsx",
            Self::Pptx => "pptx",
            Self::Pdf => "pdf",
        }
    }

    /// An extension, with or without its leading dot; ASCII case insensitive.
    pub fn from_extension(extension: &str) -> Option<Self> {
        match extension
            .strip_prefix('.')
            .unwrap_or(extension)
            .to_ascii_lowercase()
            .as_str()
        {
            "docx" => Some(Self::Docx),
            "xlsx" => Some(Self::Xlsx),
            "pptx" => Some(Self::Pptx),
            "pdf" => Some(Self::Pdf),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentSpec {
    pub format: GeneratedFormat,
    pub title: String,
    #[serde(default)]
    pub blocks: Vec<DocumentBlock>,
    #[serde(default)]
    pub sheets: Vec<DocumentSheet>,
    #[serde(default)]
    pub slides: Vec<DocumentSlide>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum DocumentBlock {
    Paragraph {
        text: String,
    },
    Heading {
        text: String,
        #[serde(default = "default_heading_level")]
        level: u8,
    },
    Bullets {
        items: Vec<String>,
    },
    Table {
        headers: Vec<String>,
        rows: Vec<Vec<CellValue>>,
    },
}
fn default_heading_level() -> u8 {
    2
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentSheet {
    pub name: String,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<CellValue>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentSlide {
    pub title: String,
    pub blocks: Vec<DocumentBlock>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CellValue {
    String(String),
    Number(f64),
    Bool(bool),
    Null,
}

impl CellValue {
    pub(super) fn text(&self) -> String {
        match self {
            Self::String(x) => x.clone(),
            Self::Number(x) => x.to_string(),
            Self::Bool(x) => x.to_string(),
            Self::Null => String::new(),
        }
    }
}

#[derive(Debug)]
pub struct GeneratedDocument {
    pub bytes: Vec<u8>,
    pub preview: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerationError(String);
impl fmt::Display for GenerationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for GenerationError {}
pub(super) fn error(message: impl Into<String>) -> GenerationError {
    GenerationError(message.into())
}
impl From<io::Error> for GenerationError {
    fn from(_: io::Error) -> Self {
        error("Document output failed or exceeded the 4 MiB output limit")
    }
}
impl From<rawzip::Error> for GenerationError {
    fn from(_: rawzip::Error) -> Self {
        error("Office package failed or exceeded the 4 MiB output limit")
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceEnvelope {
    version: u8,
    document: DocumentSpec,
}
#[derive(Serialize)]
struct SourceEnvelopeRef<'a> {
    version: u8,
    document: &'a DocumentSpec,
}

pub fn render_document(spec: &DocumentSpec) -> Result<GeneratedDocument, GenerationError> {
    validate(spec)?;
    let mut source = Output::with_limit(MAX_GENERATED_SOURCE_BYTES);
    serde_json::to_writer(
        &mut source,
        &SourceEnvelopeRef {
            version: 1,
            document: spec,
        },
    )
    .map_err(|_| error("Document source exceeds the 48 KiB limit or cannot be encoded"))?;
    let source = source.finish();
    let bytes = match spec.format {
        GeneratedFormat::Pdf => {
            let mut bytes = pdf::render(spec)?;
            // Keep startxref/EOF at the end, as required by readers that search
            // only the final 1024 bytes. Objects/xref offsets remain unchanged.
            let trailer = bytes
                .windows(b"\nstartxref\n".len())
                .rposition(|x| x == b"\nstartxref\n")
                .ok_or_else(|| error("Cannot locate generated PDF trailer"))?;
            let mut comment = PDF_SOURCE_MARKER.to_vec();
            comment.extend_from_slice(STANDARD.encode(&source).as_bytes());
            comment.push(b'\n');
            bytes.splice(trailer..trailer, comment);
            bytes
        }
        _ => office::render(spec, &source)?,
    };
    if bytes.len() > MAX_GENERATED_DOCUMENT_BYTES {
        return Err(error("Document exceeds the 4 MiB output limit"));
    }
    Ok(GeneratedDocument {
        bytes,
        preview: preview(spec),
    })
}

/// Accept only unchanged files produced by this renderer. No arbitrary XML,
/// PDF objects, archive members or embedded content are parsed here.
pub fn inspect_generated_document(
    format: GeneratedFormat,
    bytes: &[u8],
) -> Result<DocumentSpec, GenerationError> {
    if bytes.len() > MAX_GENERATED_DOCUMENT_BYTES {
        return Err(error("Document exceeds the 4 MiB input limit"));
    }
    let source = if format == GeneratedFormat::Pdf {
        let max_encoded = MAX_GENERATED_SOURCE_BYTES.div_ceil(3) * 4;
        let tail = &bytes[bytes
            .len()
            .saturating_sub(max_encoded + PDF_SOURCE_MARKER.len() + 128)..];
        let start = tail
            .windows(PDF_SOURCE_MARKER.len())
            .rposition(|x| x == PDF_SOURCE_MARKER)
            .ok_or_else(|| error("Unsupported document: missing generated source"))?
            + PDF_SOURCE_MARKER.len();
        let end = tail[start..]
            .iter()
            .position(|ch| *ch == b'\n')
            .ok_or_else(|| error("Invalid document source"))?;
        let encoded = &tail[start..start + end];
        if encoded.len() > max_encoded {
            return Err(error("Document source exceeds the 48 KiB limit"));
        }
        STANDARD
            .decode(encoded)
            .map_err(|_| error("Invalid document source"))?
    } else {
        // Only inspect the bounded EOCD comment, then regenerate. Even malformed
        // central directories are never traversed on this trusted-preview path.
        let lower = bytes.len().saturating_sub(MAX_GENERATED_SOURCE_BYTES + 22);
        let eocd = (lower..bytes.len().saturating_sub(21))
            .rev()
            .find(|&i| {
                bytes.get(i..i + 4) == Some(b"PK\x05\x06")
                    && i + 22 + u16::from_le_bytes([bytes[i + 20], bytes[i + 21]]) as usize
                        == bytes.len()
            })
            .ok_or_else(|| error("Unsupported document: missing generated source"))?;
        bytes[eocd + 22..].to_vec()
    };
    if source.len() > MAX_GENERATED_SOURCE_BYTES {
        return Err(error("Document source exceeds the 48 KiB limit"));
    }
    let envelope: SourceEnvelope =
        serde_json::from_slice(&source).map_err(|_| error("Invalid document source"))?;
    if envelope.version != 1 {
        return Err(error("Unsupported document source version"));
    }
    if envelope.document.format != format {
        return Err(error("Document format does not match source"));
    }
    let generated = render_document(&envelope.document)?;
    if generated.bytes != bytes {
        let canonical_source = serde_json::to_vec(&SourceEnvelopeRef {
            version: 1,
            document: &envelope.document,
        })
        .map_err(|_| error("Invalid document source"))?;
        if format == GeneratedFormat::Pdf
            || office::render_legacy(&envelope.document, &canonical_source)? != bytes
        {
            return Err(error("Document bytes do not match their generated source"));
        }
    }
    Ok(envelope.document)
}

fn validate(spec: &DocumentSpec) -> Result<(), GenerationError> {
    text(&spec.title, 200, false)?;
    match spec.format {
        GeneratedFormat::Docx | GeneratedFormat::Pdf => {
            if !spec.sheets.is_empty() || !spec.slides.is_empty() {
                return Err(error("DOCX/PDF accept only blocks"));
            }
            blocks(&spec.blocks, false)?;
        }
        GeneratedFormat::Xlsx => {
            if !spec.blocks.is_empty() || !spec.slides.is_empty() {
                return Err(error("XLSX accepts only sheets"));
            }
            count(spec.sheets.len(), 1, 16, "sheets")?;
            let mut names = BTreeSet::new();
            for sheet in &spec.sheets {
                text(&sheet.name, 124, false)?;
                if sheet.name.chars().count() > 31
                    || sheet
                        .name
                        .contains(['[', ']', ':', '*', '?', '/', '\\', '\n', '\r', '\t'])
                    || sheet.name.starts_with('\'')
                    || sheet.name.ends_with('\'')
                    || !names.insert(sheet.name.to_lowercase())
                {
                    return Err(error("Sheet names must be unique, at most 31 characters, and contain no Excel reserved characters"));
                }
                table(&sheet.columns, &sheet.rows)?;
            }
        }
        GeneratedFormat::Pptx => {
            if !spec.blocks.is_empty() || !spec.sheets.is_empty() {
                return Err(error("PPTX accepts only slides"));
            }
            count(spec.slides.len(), 1, 64, "slides")?;
            for slide in &spec.slides {
                text(&slide.title, 200, false)?;
                blocks(&slide.blocks, true)?;
            }
        }
    }
    Ok(())
}

fn blocks(value: &[DocumentBlock], allow_empty: bool) -> Result<(), GenerationError> {
    count(value.len(), if allow_empty { 0 } else { 1 }, 128, "blocks")?;
    for block in value {
        match block {
            DocumentBlock::Paragraph { text: value } => text(value, MAX_TEXT_BYTES, true)?,
            DocumentBlock::Heading { text: value, level } => {
                if !(1..=6).contains(level) {
                    return Err(error("Heading levels must be between 1 and 6"));
                }
                text(value, MAX_TEXT_BYTES, false)?;
            }
            DocumentBlock::Bullets { items } => {
                count(items.len(), 1, 128, "bullet items")?;
                for item in items {
                    text(item, MAX_TEXT_BYTES, false)?;
                }
            }
            DocumentBlock::Table { headers, rows } => table(headers, rows)?,
        }
    }
    Ok(())
}

fn table(headers: &[String], rows: &[Vec<CellValue>]) -> Result<(), GenerationError> {
    count(headers.len(), 1, 32, "table columns")?;
    count(rows.len(), 0, 1000, "table rows")?;
    for header in headers {
        text(header, 1024, false)?;
    }
    for row in rows {
        if row.len() != headers.len() {
            return Err(error("Every row must match the column count"));
        }
        for cell in row {
            match cell {
                CellValue::String(value) => text(value, MAX_TEXT_BYTES, true)?,
                CellValue::Number(value) if !value.is_finite() => {
                    return Err(error("Cell numbers must be finite"))
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn text(value: &str, max: usize, allow_empty: bool) -> Result<(), GenerationError> {
    if value.len() > max || (!allow_empty && value.trim().is_empty()) {
        return Err(error("Text is empty or exceeds its length limit"));
    }
    if value.chars().any(|ch| {
        (ch.is_control() && !matches!(ch, '\n' | '\r' | '\t'))
            || matches!(ch,'\u{fffe}'|'\u{ffff}'|'\u{202a}'..='\u{202e}'|'\u{2066}'..='\u{2069}')
    }) {
        return Err(error("Text contains unsupported control characters"));
    }
    Ok(())
}
fn count(value: usize, min: usize, max: usize, name: &str) -> Result<(), GenerationError> {
    if value < min || value > max {
        Err(error(format!("Expected {min} to {max} {name}")))
    } else {
        Ok(())
    }
}

pub(super) fn xml(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => result.push_str("&amp;"),
            '<' => result.push_str("&lt;"),
            '>' => result.push_str("&gt;"),
            '\"' => result.push_str("&quot;"),
            '\'' => result.push_str("&apos;"),
            '\n' => result.push_str("&#10;"),
            '\r' => result.push_str("&#13;"),
            '\t' => result.push_str("&#9;"),
            _ => result.push(ch),
        }
    }
    result
}

fn markdown(value: &str, cell: bool) -> String {
    let mut result = String::with_capacity(value.len());
    for ch in value.replace("\r\n", "\n").replace('\r', "\n").chars() {
        match ch {
            '&' => result.push_str("&amp;"),
            '<' => result.push_str("&lt;"),
            '>' => result.push_str("&gt;"),
            ch if ch.is_ascii_punctuation() => {
                result.push('\\');
                result.push(ch);
            }
            '\n' | '\r' if cell => result.push_str(" &#10; "),
            '\t' if cell => result.push(' '),
            _ => result.push(ch),
        }
    }
    result
}
fn preview(spec: &DocumentSpec) -> String {
    let mut out = format!("# {}\n\n", markdown(&spec.title, false));
    preview_blocks(&spec.blocks, &mut out);
    for sheet in &spec.sheets {
        out.push_str(&format!("## {}\n\n", markdown(&sheet.name, false)));
        preview_table(&sheet.columns, &sheet.rows, &mut out);
    }
    for (index, slide) in spec.slides.iter().enumerate() {
        out.push_str(&format!(
            "## Slide {}: {}\n\n",
            index + 1,
            markdown(&slide.title, false)
        ));
        preview_blocks(&slide.blocks, &mut out);
    }
    out
}
fn preview_blocks(blocks: &[DocumentBlock], out: &mut String) {
    for block in blocks {
        match block {
            DocumentBlock::Paragraph { text } => {
                out.push_str(&markdown(text, false));
                out.push_str("\n\n");
            }
            DocumentBlock::Heading { text, level } => out.push_str(&format!(
                "{} {}\n\n",
                "#".repeat(*level as usize),
                markdown(text, false)
            )),
            DocumentBlock::Bullets { items } => {
                for item in items {
                    out.push_str("- ");
                    out.push_str(&markdown(item, false).replace('\n', "\n  "));
                    out.push('\n');
                }
                out.push('\n');
            }
            DocumentBlock::Table { headers, rows } => preview_table(headers, rows, out),
        }
    }
}
fn preview_table(headers: &[String], rows: &[Vec<CellValue>], out: &mut String) {
    out.push_str("| ");
    out.push_str(
        &headers
            .iter()
            .map(|x| markdown(x, true))
            .collect::<Vec<_>>()
            .join(" | "),
    );
    out.push_str(" |\n| ");
    out.push_str(&vec!["---"; headers.len()].join(" | "));
    out.push_str(" |\n");
    for row in rows {
        out.push_str("| ");
        out.push_str(
            &row.iter()
                .map(|x| markdown(&x.text(), true))
                .collect::<Vec<_>>()
                .join(" | "),
        );
        out.push_str(" |\n");
    }
    out.push('\n');
}

/// Bounds buffers during serialization, including before the final size check.
pub(super) struct Output {
    bytes: Vec<u8>,
    limit: usize,
}
impl Output {
    pub fn new() -> Self {
        Self::with_limit(MAX_GENERATED_DOCUMENT_BYTES)
    }
    pub fn with_limit(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
        }
    }
    pub fn finish(self) -> Vec<u8> {
        self.bytes
    }
}
impl io::Write for Output {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.bytes.len().saturating_add(bytes.len()) > self.limit {
            return Err(io::Error::other("document output limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod compatibility_tests {
    use super::*;

    #[test]
    fn unchanged_legacy_office_previews_require_the_exact_generated_bytes() {
        for value in [
            serde_json::json!({"format":"docx","title":"Legacy","blocks":[{"type":"paragraph","text":"Original text"}]}),
            serde_json::json!({"format":"xlsx","title":"Legacy","sheets":[{"name":"Sheet","columns":["Value"],"rows":[[7]]}]}),
            serde_json::json!({"format":"pptx","title":"Legacy","slides":[{"title":"Original slide","blocks":[]}]}),
        ] {
            let document: DocumentSpec = serde_json::from_value(value).unwrap();
            let source = serde_json::to_vec(&SourceEnvelopeRef {
                version: 1,
                document: &document,
            })
            .unwrap();
            let mut legacy = office::render_legacy(&document, &source).unwrap();
            assert_ne!(legacy, render_document(&document).unwrap().bytes);
            assert!(inspect_generated_document(document.format, &legacy).is_ok());
            legacy[40] ^= 1;
            assert!(inspect_generated_document(document.format, &legacy).is_err());
        }
    }
}
