//! Bounded PDF page rendering and optional local text recognition from bytes.
//!
//! Production callers must run this synchronous operation inside an owned,
//! physically limited worker. Input and raster bounds do not limit the PDF
//! renderer's internal memory or time. This crate has no file or network APIs.

use std::{error::Error, fmt, io::Cursor};

use anima_service_adapters::document_content::DocumentExtractionStatus;
use serde::{Deserialize, Serialize};

#[cfg(windows)]
#[path = "windows.rs"]
mod backend;

/// Current page-evidence format, independent of text extraction versioning.
pub const PDF_EVIDENCE_VERSION: u32 = 1;
/// Maximum source size accepted by the existing document worker.
pub const MAX_PDF_INPUT_BYTES: usize = 2 * 1024 * 1024;
/// Maximum original PDF page count admitted by the existing text reader.
pub const MAX_PDF_PAGES: u32 = 100;
/// Maximum requested pages per operation, shared across all visual consumers.
pub const MAX_VISUAL_PAGES: usize = 2;
/// Longest output edge in pixels; pages preserve their aspect ratio.
pub const MAX_RASTER_EDGE: u32 = 1600;
/// Maximum encoded PNG bytes per page.
pub const MAX_PAGE_PNG_BYTES: usize = 2 * 1024 * 1024;
/// Maximum encoded raster bytes across an entire result.
pub const MAX_TOTAL_RASTER_BYTES: usize = 4 * 1024 * 1024;
/// Maximum recognized UTF-8 text bytes retained per page.
pub const MAX_OCR_TEXT_BYTES: usize = 8 * 1024;
/// Maximum recognizer language tag length.
pub const MAX_OCR_LANGUAGE_BYTES: usize = 128;
const MAX_PNG_DECODE_BYTES: usize = 32 * 1024 * 1024;
const MAX_PDF_PAGE_POINTS: f64 = 14_400.0;

/// One-based, strictly ascending page identities, checked before worker input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PdfPageSelection {
    pages: Vec<u32>,
}

impl PdfPageSelection {
    /// Creates a nonempty selection containing at most two original pages.
    ///
    /// # Errors
    /// Returns an error for zero, duplicate, unordered, or out-of-range pages.
    pub fn new(pages: Vec<u32>) -> Result<Self, PdfVisualError> {
        if pages.is_empty()
            || pages.len() > MAX_VISUAL_PAGES
            || pages.iter().any(|page| !(1..=MAX_PDF_PAGES).contains(page))
            || pages.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err(PdfVisualError::InvalidSelection);
        }
        Ok(Self { pages })
    }

    /// Returns the validated original page identities.
    pub fn pages(&self) -> &[u32] {
        &self.pages
    }
}

/// Local OCR outcome. Unavailable includes missing installed language support.
#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RecognizedText {
    Text { text: String, language: String },
    Empty,
    Unavailable,
}

impl<'de> Deserialize<'de> for RecognizedText {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
        enum WireRecognition {
            Text { text: String, language: String },
            Empty {},
            Unavailable {},
        }

        Ok(match WireRecognition::deserialize(deserializer)? {
            WireRecognition::Text { text, language } => Self::Text { text, language },
            WireRecognition::Empty {} => Self::Empty,
            WireRecognition::Unavailable {} => Self::Unavailable,
        })
    }
}

impl fmt::Debug for RecognizedText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text { text, language } => f
                .debug_struct("Text")
                .field("text_bytes", &text.len())
                .field("language", language)
                .finish(),
            Self::Empty => f.write_str("Empty"),
            Self::Unavailable => f.write_str("Unavailable"),
        }
    }
}

/// One rendered source page; PNG bytes are private derived content.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PdfPageEvidence {
    pub page: u32,
    pub width: u32,
    pub height: u32,
    pub png: Vec<u8>,
    pub recognized: RecognizedText,
}

impl fmt::Debug for PdfPageEvidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PdfPageEvidence")
            .field("page", &self.page)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("png", &format_args!("<redacted: {} bytes>", self.png.len()))
            .field("recognized", &self.recognized)
            .finish()
    }
}

/// Bounded evidence bound to source bytes; this value never grants source access.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PdfEvidence {
    pub version: u32,
    pub source_sha256: String,
    pub page_count: u32,
    pub pages: Vec<PdfPageEvidence>,
}

/// Stable failure classes with static descriptions that cannot echo source data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PdfVisualError {
    Unsupported,
    InvalidSelection,
    TooLarge,
    Malformed,
    Encrypted,
    Unavailable,
    InvalidOutput,
}

impl fmt::Display for PdfVisualError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Unsupported => "PDF page rendering is unsupported",
            Self::InvalidSelection => "PDF page selection is invalid",
            Self::TooLarge => "PDF page evidence exceeds its limits",
            Self::Malformed => "PDF source is malformed",
            Self::Encrypted => "encrypted PDF sources are unsupported",
            Self::Unavailable => "PDF page rendering is unavailable",
            Self::InvalidOutput => "PDF page evidence is invalid",
        })
    }
}

impl Error for PdfVisualError {}

/// Renders selected original pages with optional locally installed OCR support.
///
/// The existing strict PDF extractor rejects malformed and encrypted sources
/// before native rendering. No files or providers are opened. The host must own
/// worker lifetime, memory limits, cancellation, and the operation deadline.
///
/// # Errors
/// Returns a bounded failure for invalid input, unsupported platforms, native
/// failures, invalid page identities, or evidence exceeding the output limits.
pub fn render_pdf_pages(
    bytes: &[u8],
    selection: &PdfPageSelection,
) -> Result<PdfEvidence, PdfVisualError> {
    let page_count = preflight(bytes, selection)?;
    #[cfg(not(windows))]
    {
        let _ = page_count;
        Err(PdfVisualError::Unsupported)
    }
    #[cfg(windows)]
    {
        let value = backend::render(bytes, selection, page_count)?;
        validate_pdf_evidence(&value)?;
        if value
            .pages
            .iter()
            .map(|page| page.page)
            .ne(selection.pages().iter().copied())
        {
            return Err(PdfVisualError::InvalidOutput);
        }
        Ok(value)
    }
}

fn preflight(bytes: &[u8], selection: &PdfPageSelection) -> Result<u32, PdfVisualError> {
    if bytes.len() > MAX_PDF_INPUT_BYTES {
        return Err(PdfVisualError::TooLarge);
    }
    let extracted = anima_document_content::extract_pdf(bytes);
    match extracted.status {
        DocumentExtractionStatus::Ready
        | DocumentExtractionStatus::Partial
        | DocumentExtractionStatus::NoText => {}
        DocumentExtractionStatus::TooLarge => return Err(PdfVisualError::TooLarge),
        DocumentExtractionStatus::Encrypted => return Err(PdfVisualError::Encrypted),
        DocumentExtractionStatus::Unsupported => return Err(PdfVisualError::Unsupported),
        _ => return Err(PdfVisualError::Malformed),
    }
    let count = extracted
        .pdf_coverage
        .ok_or(PdfVisualError::Malformed)?
        .page_count;
    if count == 0 || count > MAX_PDF_PAGES {
        return Err(PdfVisualError::Malformed);
    }
    if selection.pages().iter().any(|page| *page > count) {
        return Err(PdfVisualError::InvalidSelection);
    }
    validate_source_geometry(bytes)?;
    Ok(count)
}

fn validate_source_geometry(bytes: &[u8]) -> Result<(), PdfVisualError> {
    use pdf_extract::{Document, LoadOptions};

    let document = Document::load_mem_with_options(
        bytes,
        LoadOptions {
            strict: true,
            password: None,
            filter: None,
        },
    )
    .map_err(|_| PdfVisualError::Malformed)?;
    for id in document.get_pages().values() {
        let page = document
            .get_dictionary(*id)
            .map_err(|_| PdfVisualError::Malformed)?;
        let media_box =
            inherited_object(&document, page, b"MediaBox")?.ok_or(PdfVisualError::Malformed)?;
        validate_rectangle(&document, media_box)?;
        if let Some(crop_box) = inherited_object(&document, page, b"CropBox")? {
            validate_rectangle(&document, crop_box)?;
        }
    }
    Ok(())
}

fn inherited_object<'a>(
    document: &'a pdf_extract::Document,
    mut dictionary: &'a pdf_extract::Dictionary,
    key: &[u8],
) -> Result<Option<&'a pdf_extract::Object>, PdfVisualError> {
    // A bounded parent walk rejects cycles without recursively walking attacker data.
    for _ in 0..128 {
        if let Ok(value) = dictionary.get(key) {
            return Ok(Some(value));
        }
        let Ok(parent) = dictionary.get(b"Parent") else {
            return Ok(None);
        };
        dictionary = document
            .dereference(parent)
            .and_then(|(_, object)| object.as_dict())
            .map_err(|_| PdfVisualError::Malformed)?;
    }
    Err(PdfVisualError::Malformed)
}

fn validate_rectangle(
    document: &pdf_extract::Document,
    object: &pdf_extract::Object,
) -> Result<(), PdfVisualError> {
    let (_, object) = document
        .dereference(object)
        .map_err(|_| PdfVisualError::Malformed)?;
    let coordinates = object.as_array().map_err(|_| PdfVisualError::Malformed)?;
    if coordinates.len() != 4 {
        return Err(PdfVisualError::Malformed);
    }
    let mut values = [0.0; 4];
    for (value, coordinate) in values.iter_mut().zip(coordinates) {
        let (_, coordinate) = document
            .dereference(coordinate)
            .map_err(|_| PdfVisualError::Malformed)?;
        *value = f64::from(
            coordinate
                .as_float()
                .map_err(|_| PdfVisualError::Malformed)?,
        );
        if !value.is_finite() {
            return Err(PdfVisualError::Malformed);
        }
    }
    pixel_size(values[2] - values[0], values[3] - values[1])?;
    Ok(())
}

/// Validates a complete result before the host publishes or serializes it.
///
/// PNG metadata, decoded dimensions, checksums, and aggregate byte limits are
/// checked with bounded decoder memory. Source ownership remains the host's job.
///
/// # Errors
/// Returns an error for invalid version, digest, page identities, PNGs, or OCR.
pub fn validate_pdf_evidence(value: &PdfEvidence) -> Result<(), PdfVisualError> {
    if value.version != PDF_EVIDENCE_VERSION
        || value.source_sha256.len() != 64
        || !value
            .source_sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        || !(1..=MAX_PDF_PAGES).contains(&value.page_count)
        || value.pages.is_empty()
        || value.pages.len() > MAX_VISUAL_PAGES
    {
        return Err(PdfVisualError::InvalidOutput);
    }
    let mut previous = 0;
    let mut total = 0usize;
    for page in &value.pages {
        if page.page <= previous || page.page > value.page_count {
            return Err(PdfVisualError::InvalidOutput);
        }
        previous = page.page;
        total = total
            .checked_add(page.png.len())
            .ok_or(PdfVisualError::TooLarge)?;
        if total > MAX_TOTAL_RASTER_BYTES {
            return Err(PdfVisualError::TooLarge);
        }
        validate_page(page)?;
    }
    Ok(())
}

fn validate_page(page: &PdfPageEvidence) -> Result<(), PdfVisualError> {
    if page.width == 0 || page.height == 0 || page.width.max(page.height) > MAX_RASTER_EDGE {
        return Err(PdfVisualError::InvalidOutput);
    }
    if page.png.len() > MAX_PAGE_PNG_BYTES {
        return Err(PdfVisualError::TooLarge);
    }
    if page.png.len() < 45
        || page.png.get(..8) != Some(&b"\x89PNG\r\n\x1a\n"[..])
        || page.png.get(8..16) != Some(&b"\0\0\0\rIHDR"[..])
        || !page
            .png
            .ends_with(&[0, 0, 0, 0, b'I', b'E', b'N', b'D', 174, 66, 96, 130])
    {
        return Err(PdfVisualError::InvalidOutput);
    }
    let header_width = u32::from_be_bytes(
        page.png[16..20]
            .try_into()
            .map_err(|_| PdfVisualError::InvalidOutput)?,
    );
    let header_height = u32::from_be_bytes(
        page.png[20..24]
            .try_into()
            .map_err(|_| PdfVisualError::InvalidOutput)?,
    );
    if header_width != page.width || header_height != page.height {
        return Err(PdfVisualError::InvalidOutput);
    }
    let mut decoder = png::Decoder::new(Cursor::new(&page.png));
    decoder.set_limits(png::Limits {
        bytes: MAX_PNG_DECODE_BYTES,
    });
    let mut reader = decoder
        .read_info()
        .map_err(|_| PdfVisualError::InvalidOutput)?;
    if reader.info().animation_control.is_some() {
        return Err(PdfVisualError::InvalidOutput);
    }
    let length = reader
        .output_buffer_size()
        .ok_or(PdfVisualError::InvalidOutput)?;
    if length > MAX_PNG_DECODE_BYTES {
        return Err(PdfVisualError::TooLarge);
    }
    let mut decoded = vec![0; length];
    reader
        .next_frame(&mut decoded)
        .map_err(|_| PdfVisualError::InvalidOutput)?;
    reader.finish().map_err(|_| PdfVisualError::InvalidOutput)?;
    if let RecognizedText::Text { text, language } = &page.recognized {
        if text.trim().is_empty()
            || text.len() > MAX_OCR_TEXT_BYTES
            || text
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
            || language.is_empty()
            || language.len() > MAX_OCR_LANGUAGE_BYTES
            || !language
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err(PdfVisualError::InvalidOutput);
        }
    }
    Ok(())
}

fn pixel_size(width: f64, height: f64) -> Result<(u32, u32), PdfVisualError> {
    if !width.is_finite()
        || !height.is_finite()
        || width <= 0.0
        || height <= 0.0
        || width > MAX_PDF_PAGE_POINTS
        || height > MAX_PDF_PAGE_POINTS
    {
        return Err(PdfVisualError::Malformed);
    }
    let scale = f64::from(MAX_RASTER_EDGE) / width.max(height);
    Ok((
        (width * scale).round().max(1.0) as u32,
        (height * scale).round().max(1.0) as u32,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_is_validated_before_raster_allocation() {
        for (width, height) in [
            (f64::NAN, 10.0),
            (10.0, f64::INFINITY),
            (-1.0, 10.0),
            (0.0, 10.0),
            (1e20, 10.0),
        ] {
            assert_eq!(pixel_size(width, height), Err(PdfVisualError::Malformed));
        }
        assert_eq!(pixel_size(240.0, 320.0).unwrap(), (1200, 1600));
        assert_eq!(pixel_size(320.0, 240.0).unwrap(), (1600, 1200));
    }
}
