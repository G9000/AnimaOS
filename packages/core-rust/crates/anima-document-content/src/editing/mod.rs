//! Targeted edits to admitted inert Office packages, independent of hosts and storage.
mod charts;
mod formulas;
mod package;
mod policy;
mod presentation;
mod spreadsheet;
mod types;
mod word;
mod xml_spans;

use package::Package;
pub use policy::check_office_document_text;
use std::collections::{BTreeMap, BTreeSet};
use types::Result;
pub use types::{
    OfficeCalculationState, OfficeCellValue, OfficeChange, OfficeEditError, OfficeEditOperation,
    OfficeEditReview, OfficeEditSpec, OfficeFormat, OfficeInventory, OfficePreservation,
    OfficeRange, OfficeTarget, OfficeTargetValue, PreparedOfficeEdit, RetainedOfficeHyperlink,
};

/// Inspect a complete admitted Office package without fetching or executing its contents.
///
/// # Errors
/// Rejects malformed, active, unsupported or over-budget containers.
pub fn inspect_office_document(format: OfficeFormat, source: &[u8]) -> Result<OfficeInventory> {
    inventory(&Package::open(format, source)?, format)
}
fn inventory(package: &Package, format: OfficeFormat) -> Result<OfficeInventory> {
    let targets = match format {
        OfficeFormat::Docx => word::inventory(package)?,
        OfficeFormat::Xlsx => spreadsheet::inventory(package)?,
        OfficeFormat::Pptx => presentation::inventory(package)?,
    };
    let result=OfficeInventory{version:1,format,targets,retained_hyperlinks:package.links.clone(),limitations:vec!["Content preview only. Open in Office to check page layout and text fitting.".into(),"Internal images and inert hyperlinks are retained without rendering or following them.".into()]};
    if serde_json::to_vec(&result)
        .map_err(|_| OfficeEditError::VerificationFailed)?
        .len()
        > 512 * 1024
    {
        return Err(OfficeEditError::ResourceLimit);
    }
    Ok(result)
}

/// Prepare an all-or-nothing candidate and its complete structural content review.
///
/// This function does not authorize publication. Hosts must bind source bytes, review,
/// destination, owner decision and current grants before atomically applying it.
///
/// # Errors
/// Rejects unsupported operations, stale text, overlapping patches, unsafe packages,
/// over-budget output and any preservation verification failure.
pub fn prepare_office_edit(
    format: OfficeFormat,
    source: &[u8],
    edits: &OfficeEditSpec,
) -> Result<PreparedOfficeEdit> {
    if edits.operations.is_empty()
        || edits.operations.len() > 128
        || serde_json::to_vec(edits)
            .map_err(|_| OfficeEditError::ResourceLimit)?
            .len()
            > 64 * 1024
    {
        return Err(OfficeEditError::ResourceLimit);
    }
    let package = Package::open(format, source)?;
    let before = inventory(&package, format)?;
    let (replacements, removed, changes) = match format {
        OfficeFormat::Docx => {
            let (patches, changes) = word::apply(&package, &edits.operations)?;
            (materialize(&package, patches)?, BTreeSet::new(), changes)
        }
        OfficeFormat::Pptx => {
            let (patches, changes) = presentation::apply(&package, &edits.operations)?;
            (materialize(&package, patches)?, BTreeSet::new(), changes)
        }
        OfficeFormat::Xlsx => {
            let result = spreadsheet::apply(&package, &edits.operations)?;
            (result.replacements, result.removed, result.changes)
        }
    };
    let bytes = package.encode(&replacements, &removed)?;
    let candidate = Package::open(format, &bytes)?;
    let mut after = inventory(&candidate, format)?;
    if format == OfficeFormat::Xlsx
        && edits.operations.iter().any(|op| {
            matches!(
                op,
                OfficeEditOperation::SetCellValue { .. }
                    | OfficeEditOperation::SetCellFormula { .. }
            )
        })
    {
        spreadsheet::annotate_calculations(&candidate, &mut after)?;
    }
    let mut unchanged_parts = 0;
    for part in &package.parts {
        if removed.contains(&part.name) {
            if candidate.part(&part.name).is_ok() {
                return Err(OfficeEditError::VerificationFailed);
            }
            continue;
        }
        let current = candidate.part(&part.name)?;
        let expected = replacements.get(&part.name).unwrap_or(&part.bytes);
        if &current.bytes != expected {
            return Err(OfficeEditError::VerificationFailed);
        }
        if current.bytes == part.bytes {
            unchanged_parts += 1;
        }
    }
    if candidate.parts.len() + removed.len() != package.parts.len() {
        return Err(OfficeEditError::VerificationFailed);
    }
    let review = OfficeEditReview {
        version: 1,
        before,
        after,
        changes,
        preservation: OfficePreservation {
            unchanged_parts,
            edited_parts: package.parts.len() - unchanged_parts,
            unchanged_bytes_verified: true,
            container_normalized: true,
        },
    };
    if serde_json::to_vec(&review)
        .map_err(|_| OfficeEditError::VerificationFailed)?
        .len()
        > 64 * 1024
    {
        return Err(OfficeEditError::ResourceLimit);
    }
    Ok(PreparedOfficeEdit { bytes, review })
}

fn materialize(
    package: &Package,
    patches: BTreeMap<String, Vec<xml_spans::Patch>>,
) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut out = BTreeMap::new();
    for (name, mut patches) in patches {
        out.insert(
            name.clone(),
            xml_spans::patch(&package.part(&name)?.bytes, &mut patches)?,
        );
    }
    Ok(out)
}
