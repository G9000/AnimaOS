//! Byte-bound Office inspection facts, independent of hosts and authority.
//!
//! Admission and report construction are synchronous, bounded parsers; the host
//! must own their physical process limits. This module never executes an engine,
//! grants access, decides layout quality, or certifies that a worker was honest.
//! Reports bind native-worker observations to captured bytes. Their digest is an
//! integrity identity, not a signature or an authorization token.

mod admission;

use std::{collections::BTreeSet, error::Error, fmt, io::Cursor};

use anima_document_content::OfficeFormat;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{PdfPageEvidence, PdfPageSelection, MAX_PAGE_PNG_BYTES, MAX_PDF_INPUT_BYTES};

pub use admission::admit_office;

/// Current deterministic report and raster measurement contract.
pub const INSPECTION_REPORT_VERSION: u32 = 1;
/// Complete-export reports may retain at most ten actual rasters.
pub const MAX_INSPECTION_PAGES: usize = 10;
/// Aggregate encoded PNG bound, independently of the two-page renderer calls.
pub const MAX_INSPECTION_RASTER_BYTES: usize = 20 * 1024 * 1024;
/// Maximum serialized immutable report size, excluding separately held PNGs.
pub const MAX_INSPECTION_REPORT_BYTES: usize = 2 * 1024 * 1024;
/// Original Office input cap, matching the existing package admission boundary.
pub const MAX_OFFICE_INPUT_BYTES: usize = 4 * 1024 * 1024;
const MAX_FORMULAS: usize = 10_000;
const MAX_FORMULA_TEXT_BYTES: usize = 4096;
const MAX_FORMULA_RESULT_TEXT_BYTES: usize = 8192;
const INK_THRESHOLD: u8 = 245;

/// Stable failures never include document text or arbitrary parser diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InspectionError {
    Unsupported,
    InvalidSource,
    ResourceLimit,
    InvalidEvidence,
}

impl fmt::Display for InspectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Unsupported => "Office inspection does not support this source",
            Self::InvalidSource => "Office inspection source is invalid",
            Self::ResourceLimit => "Office inspection exceeds its limits",
            Self::InvalidEvidence => "Office inspection evidence is invalid",
        })
    }
}

impl Error for InspectionError {}

/// Source-relative cell identity, independent of stored formula caches.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormulaIdentity {
    pub sheet_id: String,
    /// Zero-based source workbook order, including empty and hidden sheets.
    pub sheet_ordinal: u32,
    pub address: String,
    /// Hash of exact decoded OOXML formula text, without a leading `=`.
    pub formula_sha256: String,
}

/// Complete admitted cell-formula inventory, including hidden/unprinted cells.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormulaInventory {
    pub sha256: String,
    pub formulas: Vec<FormulaIdentity>,
}

/// Admitted actual Office bytes. There is no deserialization proof constructor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfficeAdmission {
    format: OfficeFormat,
    source_sha256: String,
    source_byte_length: u64,
    inventory: FormulaInventory,
}

impl OfficeAdmission {
    pub fn format(&self) -> OfficeFormat {
        self.format
    }

    pub fn source_sha256(&self) -> &str {
        &self.source_sha256
    }

    pub fn source_byte_length(&self) -> u64 {
        self.source_byte_length
    }

    pub fn formulas(&self) -> &[FormulaIdentity] {
        &self.inventory.formulas
    }

    pub fn formula_inventory_sha256(&self) -> &str {
        &self.inventory.sha256
    }
}

/// Exact captured source identity; the host owns the opaque source version.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceIdentity {
    pub format: OfficeFormat,
    pub sha256: String,
    pub byte_length: u64,
    pub source_version: String,
}

/// Host-verified converter identity and a bounded inventory of font directories.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RendererIdentity {
    pub backend: String,
    pub version: String,
    pub executable_sha256: String,
    pub configuration_sha256: String,
    /// System/user font directories and pinned engine tree; not every font the
    /// OS might supply through registrations or transient font resources.
    pub font_manifest_sha256: String,
    /// Backend and version of the independent actual-PDF rasterizer.
    pub rasterizer: String,
}

/// Native-engine values are observations, including explicit calculation errors.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FormulaValue {
    Number { value: f64 },
    Text { value: String },
    Boolean { value: bool },
    Empty,
    Error { code: u32 },
}

/// One engine result bound to one original cell identity.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormulaResult {
    pub identity: FormulaIdentity,
    /// Actual engine syntax; it may normalize the original OOXML syntax.
    pub engine_formula: String,
    pub value: FormulaValue,
}

/// A worker's calculation observations. Counts, digests and errors are derived.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CalculationEvidence {
    NotApplicable,
    NotRequested,
    NoFormulas,
    Recalculated { results: Vec<FormulaResult> },
    Inconclusive { reason: String },
}

/// Explicit formula-error evidence, independent of exported page coverage.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormulaError {
    pub identity: FormulaIdentity,
    pub code: u32,
}

/// Derived formula coverage never relies on the printed view or cached values.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalculationReport {
    pub evidence: CalculationEvidence,
    pub source_inventory_sha256: String,
    /// Binds identities plus actual engine formula text, independently of source.
    pub evaluated_inventory_sha256: Option<String>,
    pub result_sha256: Option<String>,
    pub total_formulas: u32,
    pub inspected_formulas: u32,
    pub errors: Vec<FormulaError>,
}

/// Coverage describes only the view actually exported by the recorded backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportedView {
    WriterPrintLayout,
    CalcPrintedSheetsAndPrintAreas,
    ImpressExportedSlides,
}

/// Exact page coverage; omitted pages can never be described as complete.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RenderCoverage {
    CompleteExportedView,
    Partial {
        missing_pages: Vec<u32>,
        reason: String,
    },
}

/// Versioned integer-only pixel facts, alpha-composited over white.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RasterInk {
    pub measurement_version: u32,
    /// A pixel is ink when any composited RGB component is below this value.
    pub threshold: u8,
    pub non_background_pixels: u64,
    pub total_pixels: u64,
}

/// Hashes identify actual raster bytes; the report does not carry image bytes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PageEvidence {
    pub ordinal: u32,
    pub png_sha256: String,
    pub byte_length: u64,
    pub width: u32,
    pub height: u32,
    pub ink: RasterInk,
}

/// Actual derived PDF identity and bounded raster observations.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderEvidence {
    pub pdf_sha256: String,
    pub pdf_byte_length: u64,
    pub exported_page_count: u32,
    pub pages: Vec<PageEvidence>,
    pub coverage: RenderCoverage,
    pub view: ExportedView,
}

/// Explicit limits on what these native facts can establish.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InspectionLimitation {
    BackendSpecificOfficeRendering,
    FontManifestCoversDirectorySubset,
    ExportedViewOnly,
    HiddenAndUnprintedContentExcludedFromRendering,
    OriginalFormulaCachesAreUnverified,
    EngineFormulaSyntaxMayBeNormalized,
    RasterInkDoesNotDecideIntentionalBlanknessOrClipping,
    NoGenericLayoutOrOutcomePass,
    ResourceControlsDoNotEstablishOsIsolation,
}

/// Immutable byte-bound facts. Construction never grants source or page access.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "ReportWire")]
pub struct ArtifactInspectionReport {
    schema_version: u32,
    source: SourceIdentity,
    renderer: RendererIdentity,
    formula_inventory: FormulaInventory,
    rendering: RenderEvidence,
    calculation: CalculationReport,
    limitations: Vec<InspectionLimitation>,
    report_sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReportWire {
    schema_version: u32,
    source: SourceIdentity,
    renderer: RendererIdentity,
    formula_inventory: FormulaInventory,
    rendering: RenderEvidence,
    calculation: CalculationReport,
    limitations: Vec<InspectionLimitation>,
    report_sha256: String,
}

impl TryFrom<ReportWire> for ArtifactInspectionReport {
    type Error = InspectionError;

    fn try_from(wire: ReportWire) -> Result<Self, Self::Error> {
        let report = Self {
            schema_version: wire.schema_version,
            source: wire.source,
            renderer: wire.renderer,
            formula_inventory: wire.formula_inventory,
            rendering: wire.rendering,
            calculation: wire.calculation,
            limitations: wire.limitations,
            report_sha256: wire.report_sha256,
        };
        report.validate()?;
        Ok(report)
    }
}

impl ArtifactInspectionReport {
    pub fn sha256(&self) -> &str {
        &self.report_sha256
    }

    pub fn source(&self) -> &SourceIdentity {
        &self.source
    }

    pub fn renderer(&self) -> &RendererIdentity {
        &self.renderer
    }

    pub fn rendering(&self) -> &RenderEvidence {
        &self.rendering
    }

    pub fn calculation(&self) -> &CalculationReport {
        &self.calculation
    }

    pub fn formula_inventory(&self) -> &FormulaInventory {
        &self.formula_inventory
    }

    pub fn limitations(&self) -> &[InspectionLimitation] {
        &self.limitations
    }

    /// Checks bounded structure, all derived coverage and the content digest.
    ///
    /// The host must separately authenticate worker provenance and recheck the
    /// original source authority. A digest is not proof of engine execution.
    ///
    /// # Errors
    /// Rejects changed facts, unsupported versions, or inconsistent coverage.
    pub fn validate(&self) -> Result<(), InspectionError> {
        if self.schema_version != INSPECTION_REPORT_VERSION
            || self.source.format == OfficeFormat::Html
            || !valid_digest(&self.source.sha256)
            || self.source.byte_length == 0
            || self.source.byte_length > MAX_OFFICE_INPUT_BYTES as u64
            || !valid_text(&self.source.source_version, 256)
            || !valid_digest(&self.report_sha256)
            || self.limitations != limitations()
        {
            return Err(InspectionError::InvalidEvidence);
        }
        validate_renderer(&self.renderer)?;
        validate_inventory(&self.formula_inventory)?;
        if self.source.format != OfficeFormat::Xlsx && !self.formula_inventory.formulas.is_empty() {
            return Err(InspectionError::InvalidEvidence);
        }
        validate_rendering(self.source.format, &self.rendering)?;
        let calculation = calculation_report(
            self.source.format,
            &self.formula_inventory,
            self.calculation.evidence.clone(),
        )?;
        if calculation != self.calculation || self.facts_digest()? != self.report_sha256 {
            return Err(InspectionError::InvalidEvidence);
        }
        if serde_json::to_vec(self)
            .map_err(|_| InspectionError::InvalidEvidence)?
            .len()
            > MAX_INSPECTION_REPORT_BYTES
        {
            return Err(InspectionError::ResourceLimit);
        }
        Ok(())
    }

    fn facts_digest(&self) -> Result<String, InspectionError> {
        // A versioned tuple avoids maps, self-reference and optional field order.
        serialized_digest(&(
            self.schema_version,
            &self.source,
            &self.renderer,
            &self.formula_inventory,
            &self.rendering,
            &self.calculation,
            &self.limitations,
        ))
    }
}

/// Builds immutable facts by hashing actual captured PDF and raster bytes.
///
/// Page count comes from strict PDF parsing, never a worker-supplied count.
/// The host must verify the pinned renderer identity and actual worker custody.
/// Original caches or metadata are never used as evidence of calculation.
///
/// # Errors
/// Rejects malformed bytes, omitted formula identities, inconsistent results,
/// invalid rasters, unsupported renderer identities, or any exceeded bound.
pub fn build_report(
    source_version: &str,
    admission: &OfficeAdmission,
    renderer: RendererIdentity,
    pdf: &[u8],
    pages: &[PdfPageEvidence],
    calculation: CalculationEvidence,
) -> Result<ArtifactInspectionReport, InspectionError> {
    validate_renderer(&renderer)?;
    if !valid_text(source_version, 256) || pages.is_empty() {
        return Err(InspectionError::InvalidEvidence);
    }
    if pdf.len() > MAX_PDF_INPUT_BYTES || pages.len() > MAX_INSPECTION_PAGES {
        return Err(InspectionError::ResourceLimit);
    }
    let first = PdfPageSelection::new(vec![1]).map_err(|_| InspectionError::InvalidEvidence)?;
    let count = crate::preflight(pdf, &first).map_err(|_| InspectionError::InvalidEvidence)?;
    let mut evidence = Vec::with_capacity(pages.len());
    let mut total_bytes = 0usize;
    for page in pages {
        crate::validate_page(page).map_err(|_| InspectionError::InvalidEvidence)?;
        total_bytes = total_bytes
            .checked_add(page.png.len())
            .ok_or(InspectionError::ResourceLimit)?;
        if total_bytes > MAX_INSPECTION_RASTER_BYTES {
            return Err(InspectionError::ResourceLimit);
        }
        evidence.push(PageEvidence {
            ordinal: page.page,
            png_sha256: hash(&page.png),
            byte_length: page.png.len() as u64,
            width: page.width,
            height: page.height,
            ink: raster_ink(page)?,
        });
    }
    let coverage = coverage(count, &evidence)?;
    let mut report = ArtifactInspectionReport {
        schema_version: INSPECTION_REPORT_VERSION,
        source: SourceIdentity {
            format: admission.format,
            sha256: admission.source_sha256.clone(),
            byte_length: admission.source_byte_length,
            source_version: source_version.into(),
        },
        renderer,
        formula_inventory: admission.inventory.clone(),
        rendering: RenderEvidence {
            pdf_sha256: hash(pdf),
            pdf_byte_length: pdf.len() as u64,
            exported_page_count: count,
            pages: evidence,
            coverage,
            view: exported_view(admission.format)?,
        },
        calculation: calculation_report(admission.format, &admission.inventory, calculation)?,
        limitations: limitations(),
        report_sha256: String::new(),
    };
    report.report_sha256 = report.facts_digest()?;
    report.validate()?;
    Ok(report)
}

fn validate_renderer(value: &RendererIdentity) -> Result<(), InspectionError> {
    if value.backend != "libreoffice_uno"
        || !valid_text(&value.version, 128)
        || !valid_text(&value.rasterizer, 256)
        || !valid_digest(&value.executable_sha256)
        || !valid_digest(&value.configuration_sha256)
        || !valid_digest(&value.font_manifest_sha256)
    {
        return Err(InspectionError::InvalidEvidence);
    }
    Ok(())
}

fn validate_inventory(value: &FormulaInventory) -> Result<(), InspectionError> {
    if value.formulas.len() > MAX_FORMULAS || !valid_digest(&value.sha256) {
        return Err(InspectionError::InvalidEvidence);
    }
    let mut cells = BTreeSet::new();
    let mut sheets = std::collections::BTreeMap::new();
    let mut sheet_ordinals = std::collections::BTreeMap::new();
    for identity in &value.formulas {
        if identity.sheet_ordinal >= 32
            || !identity.sheet_id.starts_with("sheet:")
            || !valid_text(&identity.sheet_id, 512)
            || !valid_address(&identity.address)
            || !valid_digest(&identity.formula_sha256)
            || !cells.insert((identity.sheet_ordinal, &identity.address))
        {
            return Err(InspectionError::InvalidEvidence);
        }
        if sheets
            .insert(&identity.sheet_id, identity.sheet_ordinal)
            .is_some_and(|previous| previous != identity.sheet_ordinal)
            || sheet_ordinals
                .insert(identity.sheet_ordinal, &identity.sheet_id)
                .is_some_and(|previous| previous != &identity.sheet_id)
        {
            return Err(InspectionError::InvalidEvidence);
        }
    }
    if value
        .formulas
        .windows(2)
        .any(|pair| cell_key(&pair[0]) >= cell_key(&pair[1]))
        || serialized_digest(&value.formulas)? != value.sha256
    {
        return Err(InspectionError::InvalidEvidence);
    }
    Ok(())
}

fn valid_address(value: &str) -> bool {
    let split = value
        .bytes()
        .position(|b| !b.is_ascii_uppercase())
        .unwrap_or(value.len());
    if split == 0 || split > 3 || split == value.len() {
        return false;
    }
    let (column, row) = value.split_at(split);
    let column = column
        .bytes()
        .fold(0u32, |n, b| n * 26 + u32::from(b - b'A' + 1));
    !row.starts_with('0')
        && row.bytes().all(|b| b.is_ascii_digit())
        && row
            .parse::<u32>()
            .is_ok_and(|row| (1..=1_048_576).contains(&row))
        && (1..=16_384).contains(&column)
}

fn cell_key(identity: &FormulaIdentity) -> (u32, &str) {
    (identity.sheet_ordinal, &identity.address)
}

fn calculation_report(
    format: OfficeFormat,
    inventory: &FormulaInventory,
    mut evidence: CalculationEvidence,
) -> Result<CalculationReport, InspectionError> {
    let mut evaluated_inventory_sha256 = None;
    let mut result_sha256 = None;
    let mut inspected_formulas = 0;
    let mut errors = Vec::new();
    match &mut evidence {
        CalculationEvidence::NotApplicable if format != OfficeFormat::Xlsx => {}
        CalculationEvidence::NotRequested if format == OfficeFormat::Xlsx => {}
        CalculationEvidence::NoFormulas
            if format == OfficeFormat::Xlsx && inventory.formulas.is_empty() => {}
        CalculationEvidence::Inconclusive { reason }
            if format == OfficeFormat::Xlsx && valid_text(reason, 256) => {}
        CalculationEvidence::Recalculated { results }
            if format == OfficeFormat::Xlsx && !inventory.formulas.is_empty() =>
        {
            if results.len() != inventory.formulas.len() || results.len() > MAX_FORMULAS {
                return Err(InspectionError::InvalidEvidence);
            }
            results.sort_by(|a, b| cell_key(&a.identity).cmp(&cell_key(&b.identity)));
            for (result, expected) in results.iter().zip(&inventory.formulas) {
                if result.identity != *expected
                    || !valid_text(&result.engine_formula, MAX_FORMULA_TEXT_BYTES + 1)
                    || !result.engine_formula.starts_with('=')
                {
                    return Err(InspectionError::InvalidEvidence);
                }
                match &result.value {
                    FormulaValue::Number { value } if !value.is_finite() => {
                        return Err(InspectionError::InvalidEvidence)
                    }
                    FormulaValue::Text { value } if value.len() > MAX_FORMULA_RESULT_TEXT_BYTES => {
                        return Err(InspectionError::ResourceLimit)
                    }
                    FormulaValue::Error { code } => {
                        if *code == 0 || *code > u16::MAX.into() {
                            return Err(InspectionError::InvalidEvidence);
                        }
                        errors.push(FormulaError {
                            identity: result.identity.clone(),
                            code: *code,
                        });
                    }
                    _ => {}
                }
            }
            let evaluated = results
                .iter()
                .map(|r| (&r.identity, &r.engine_formula))
                .collect::<Vec<_>>();
            evaluated_inventory_sha256 = Some(serialized_digest(&evaluated)?);
            result_sha256 = Some(serialized_digest(results)?);
            inspected_formulas = results.len() as u32;
        }
        _ => return Err(InspectionError::InvalidEvidence),
    }
    Ok(CalculationReport {
        evidence,
        source_inventory_sha256: inventory.sha256.clone(),
        evaluated_inventory_sha256,
        result_sha256,
        total_formulas: inventory.formulas.len() as u32,
        inspected_formulas,
        errors,
    })
}

fn exported_view(format: OfficeFormat) -> Result<ExportedView, InspectionError> {
    match format {
        OfficeFormat::Docx => Ok(ExportedView::WriterPrintLayout),
        OfficeFormat::Xlsx => Ok(ExportedView::CalcPrintedSheetsAndPrintAreas),
        OfficeFormat::Pptx => Ok(ExportedView::ImpressExportedSlides),
        OfficeFormat::Html => Err(InspectionError::Unsupported),
    }
}

fn coverage(count: u32, pages: &[PageEvidence]) -> Result<RenderCoverage, InspectionError> {
    if !(1..=crate::MAX_PDF_PAGES).contains(&count)
        || pages.is_empty()
        || pages.len() > MAX_INSPECTION_PAGES
        || pages.iter().any(|p| p.ordinal == 0 || p.ordinal > count)
        || pages
            .windows(2)
            .any(|pair| pair[0].ordinal >= pair[1].ordinal)
    {
        return Err(InspectionError::InvalidEvidence);
    }
    let missing_pages = (1..=count)
        .filter(|ordinal| !pages.iter().any(|page| page.ordinal == *ordinal))
        .collect::<Vec<_>>();
    if missing_pages.is_empty() {
        Ok(RenderCoverage::CompleteExportedView)
    } else {
        Ok(RenderCoverage::Partial {
            missing_pages,
            reason: if count as usize > MAX_INSPECTION_PAGES {
                "raster_page_cap"
            } else {
                "exported_pages_not_rendered"
            }
            .into(),
        })
    }
}

fn validate_rendering(format: OfficeFormat, value: &RenderEvidence) -> Result<(), InspectionError> {
    if !valid_digest(&value.pdf_sha256)
        || value.pdf_byte_length == 0
        || value.pdf_byte_length > MAX_PDF_INPUT_BYTES as u64
        || value.view != exported_view(format)?
        || value.coverage != coverage(value.exported_page_count, &value.pages)?
    {
        return Err(InspectionError::InvalidEvidence);
    }
    let mut total_bytes = 0u64;
    for page in &value.pages {
        total_bytes = total_bytes
            .checked_add(page.byte_length)
            .ok_or(InspectionError::ResourceLimit)?;
        if !valid_digest(&page.png_sha256)
            || page.byte_length < 45
            || page.byte_length > MAX_PAGE_PNG_BYTES as u64
            || page.width == 0
            || page.height == 0
            || page.width.max(page.height) > crate::MAX_RASTER_EDGE
            || page.ink.measurement_version != 1
            || page.ink.threshold != INK_THRESHOLD
            || page.ink.total_pixels != u64::from(page.width) * u64::from(page.height)
            || page.ink.non_background_pixels > page.ink.total_pixels
        {
            return Err(InspectionError::InvalidEvidence);
        }
    }
    if total_bytes > MAX_INSPECTION_RASTER_BYTES as u64 {
        return Err(InspectionError::ResourceLimit);
    }
    Ok(())
}

fn raster_ink(page: &PdfPageEvidence) -> Result<RasterInk, InspectionError> {
    let mut decoder = png::Decoder::new(Cursor::new(&page.png));
    decoder.set_limits(png::Limits {
        bytes: crate::MAX_PNG_DECODE_BYTES,
    });
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder
        .read_info()
        .map_err(|_| InspectionError::InvalidEvidence)?;
    let length = reader
        .output_buffer_size()
        .ok_or(InspectionError::InvalidEvidence)?;
    if length > crate::MAX_PNG_DECODE_BYTES {
        return Err(InspectionError::ResourceLimit);
    }
    let mut decoded = vec![0; length];
    let frame = reader
        .next_frame(&mut decoded)
        .map_err(|_| InspectionError::InvalidEvidence)?;
    if frame.bit_depth != png::BitDepth::Eight
        || frame.width != page.width
        || frame.height != page.height
    {
        return Err(InspectionError::InvalidEvidence);
    }
    let channels = match frame.color_type {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => return Err(InspectionError::InvalidEvidence),
    };
    let mut non_background_pixels = 0u64;
    for pixel in decoded[..frame.buffer_size()].chunks_exact(channels) {
        let (rgb, alpha) = match channels {
            1 => ([pixel[0]; 3], 255),
            2 => ([pixel[0]; 3], pixel[1]),
            3 => ([pixel[0], pixel[1], pixel[2]], 255),
            _ => ([pixel[0], pixel[1], pixel[2]], pixel[3]),
        };
        let alpha = u32::from(alpha);
        let ink = rgb.iter().any(|component| {
            let composited = (u32::from(*component) * alpha + 255 * (255 - alpha) + 127) / 255;
            composited < u32::from(INK_THRESHOLD)
        });
        non_background_pixels += u64::from(ink);
    }
    reader
        .finish()
        .map_err(|_| InspectionError::InvalidEvidence)?;
    Ok(RasterInk {
        measurement_version: 1,
        threshold: INK_THRESHOLD,
        non_background_pixels,
        total_pixels: u64::from(page.width) * u64::from(page.height),
    })
}

fn limitations() -> Vec<InspectionLimitation> {
    vec![
        InspectionLimitation::BackendSpecificOfficeRendering,
        InspectionLimitation::FontManifestCoversDirectorySubset,
        InspectionLimitation::ExportedViewOnly,
        InspectionLimitation::HiddenAndUnprintedContentExcludedFromRendering,
        InspectionLimitation::OriginalFormulaCachesAreUnverified,
        InspectionLimitation::EngineFormulaSyntaxMayBeNormalized,
        InspectionLimitation::RasterInkDoesNotDecideIntentionalBlanknessOrClipping,
        InspectionLimitation::NoGenericLayoutOrOutcomePass,
        InspectionLimitation::ResourceControlsDoNotEstablishOsIsolation,
    ]
}

fn valid_text(value: &str, cap: usize) -> bool {
    !value.trim().is_empty() && value.len() <= cap && !value.chars().any(char::is_control)
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn serialized_digest(value: &impl Serialize) -> Result<String, InspectionError> {
    let bytes = serde_json::to_vec(value).map_err(|_| InspectionError::InvalidEvidence)?;
    if bytes.len() > MAX_INSPECTION_REPORT_BYTES {
        return Err(InspectionError::ResourceLimit);
    }
    Ok(hash(&bytes))
}

fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
