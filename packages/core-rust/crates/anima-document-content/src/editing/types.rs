//! Bounded, inert Office edit requests and content-review projections.
use serde::{Deserialize, Serialize};
use std::fmt;

/// Supported imported Office containers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OfficeFormat {
    Docx,
    Xlsx,
    Pptx,
}

/// An all-or-nothing batch against one exact host-verified source snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OfficeEditSpec {
    pub operations: Vec<OfficeEditOperation>,
}

/// Typed operations never accept XML, archive paths, scripts or relationships.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum OfficeEditOperation {
    WordReplaceText {
        target_id: String,
        start: u32,
        end: u32,
        expected_text: String,
        new_text: String,
    },
    SetCellValue {
        sheet_id: String,
        cell: String,
        value: OfficeCellValue,
    },
    SetCellFormula {
        sheet_id: String,
        cell: String,
        formula: String,
    },
    SetChartTitle {
        chart_id: String,
        expected_title: String,
        title: String,
    },
    SetChartSeriesRanges {
        series_id: String,
        categories: OfficeRange,
        values: OfficeRange,
    },
    SlideReplaceText {
        target_id: String,
        start: u32,
        end: u32,
        expected_text: String,
        new_text: String,
    },
    ReorderSlides {
        slide_ids: Vec<String>,
    },
}

/// Formula-looking strings remain strings unless an explicit formula operation is used.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum OfficeCellValue {
    String(String),
    Number(f64),
    Boolean(bool),
    Blank,
}

/// One fixed, internal worksheet rectangle, using inventory sheet IDs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OfficeRange {
    pub sheet_id: String,
    pub start: String,
    pub end: String,
}

/// Calculation states distinguish original caches from verified subset results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OfficeCalculationState {
    NotApplicable,
    Literal,
    StoredUnverified,
    Calculated,
    NeedsExternalRecalculation,
    CalculationError,
}

/// Closed target values used in plain-text content previews.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum OfficeTargetValue {
    Text {
        text: String,
    },
    Cell {
        sheet_id: String,
        address: String,
        value: OfficeCellValue,
        formula: Option<String>,
        calculation: OfficeCalculationState,
    },
    Chart {
        title: String,
        chart_type: String,
    },
    Series {
        categories: Option<OfficeRange>,
        values: Option<OfficeRange>,
    },
    Slide {
        ordinal: u32,
        hidden: bool,
    },
}

/// An opaque, source-snapshot-relative identity and its editable coverage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OfficeTarget {
    pub id: String,
    pub label: String,
    pub editable: bool,
    pub value: OfficeTargetValue,
}

/// A retained inert hyperlink; callers must render its target as text only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetainedOfficeHyperlink {
    pub relationship_id: String,
    pub target: String,
}

/// Complete bounded inventory; hosts must paginate it without implying omitted coverage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OfficeInventory {
    pub version: u8,
    pub format: OfficeFormat,
    pub targets: Vec<OfficeTarget>,
    pub retained_hyperlinks: Vec<RetainedOfficeHyperlink>,
    pub limitations: Vec<String>,
}

/// One direct operation or mandatory derived cache/metadata change.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OfficeChange {
    pub operation_index: Option<usize>,
    pub label: String,
    pub before: String,
    pub after: String,
    pub derived: bool,
}

/// Exact equality proof for all package bytes outside declared replacement spans.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OfficePreservation {
    pub unchanged_parts: usize,
    pub edited_parts: usize,
    pub unchanged_bytes_verified: bool,
    pub container_normalized: bool,
}

/// Full structural content review, never a claim of page/slide layout fidelity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OfficeEditReview {
    pub version: u8,
    pub before: OfficeInventory,
    pub after: OfficeInventory,
    pub changes: Vec<OfficeChange>,
    pub preservation: OfficePreservation,
}

/// Validated candidate; no deserialization path can manufacture this proof.
#[derive(Debug)]
pub struct PreparedOfficeEdit {
    pub(super) bytes: Vec<u8>,
    pub(super) review: OfficeEditReview,
}
impl PreparedOfficeEdit {
    /// Borrow the validated candidate bytes for host-owned staging.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// Borrow the complete review associated with exactly these bytes.
    pub fn review(&self) -> &OfficeEditReview {
        &self.review
    }
}

/// Actionable failures never return a partially edited candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OfficeEditError {
    InvalidPackage,
    UnsupportedFeature(String),
    ResourceLimit,
    StaleTarget,
    InvalidOperation(String),
    VerificationFailed,
}
impl fmt::Display for OfficeEditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPackage => f.write_str("invalid Office package"),
            Self::UnsupportedFeature(reason) => write!(f, "unsupported Office feature: {reason}"),
            Self::ResourceLimit => f.write_str("Office document exceeds editing limits"),
            Self::StaleTarget => f.write_str("document target changed; inspect the current source"),
            Self::InvalidOperation(reason) => write!(f, "invalid document edit: {reason}"),
            Self::VerificationFailed => {
                f.write_str("edited document failed preservation verification")
            }
        }
    }
}
impl std::error::Error for OfficeEditError {}

pub(super) type Result<T> = std::result::Result<T, OfficeEditError>;
pub(super) fn unsupported(reason: &str) -> OfficeEditError {
    OfficeEditError::UnsupportedFeature(reason.into())
}
pub(super) fn invalid(reason: &str) -> OfficeEditError {
    OfficeEditError::InvalidOperation(reason.into())
}
