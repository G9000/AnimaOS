//! Public document criteria checked against freshly authorized, byte-bound native facts.

use std::{path::Path, time::Duration};

use anima_core::{
    execution_limits::ExecutionContext,
    outcomes::{DocumentCheck, OutcomeCriterion, OutcomeObservation, OutcomeVerdict},
    CancelSignal,
};
use anima_document_content::OfficeFormat;
use anima_document_visual::inspection::{
    ArtifactInspectionReport, CalculationEvidence, FormulaValue, RenderCoverage,
};

use crate::{
    attachments::documents::source_read,
    document_inspections::{DocumentInspectionService, InspectionRequest, InspectionSubject},
    runs::RunLink,
    tools::workspace_bytes::WorkspaceBytes,
};

/// Match the inspection service's bounded Office source capture.
const MAX_DOCUMENT_BYTES: usize = 4 * 1024 * 1024;
const SOURCE_READ_DEADLINE: Duration = Duration::from_secs(10);

/// Inspects each public document criterion under the current run's source authority.
///
/// A path criterion always starts a new inspection. Pinned reports are delivered
/// through the service, which rechecks the original source and live authority.
pub(super) async fn observe_document(
    criterion: &OutcomeCriterion,
    root: Option<&Path>,
    run: &RunLink,
    execution: Option<&ExecutionContext>,
    service: &DocumentInspectionService,
    cancel: CancelSignal,
) -> OutcomeObservation {
    if cancel.is_cancelled() {
        return inconclusive(criterion, "Document observation was cancelled");
    }
    match criterion {
        OutcomeCriterion::Document { path, check, .. } => {
            let Some(execution) = execution else {
                return inconclusive(
                    criterion,
                    "Fresh document inspection requires admitted execution accounting",
                );
            };
            if execution.check_live().is_err() {
                return inconclusive(
                    criterion,
                    "The admitted document inspection budget is stopped or exhausted",
                );
            }
            let Some(root) = root else {
                return inconclusive(
                    criterion,
                    "The authorized document workspace is unavailable",
                );
            };
            let root = root.to_path_buf();
            let source_path = path.clone();
            let remaining = Duration::from_millis(
                execution
                    .deadline_at_ms
                    .saturating_sub(execution.clock.now_ms()),
            );
            let captured = source_read::read_retained(
                move || capture_subject(&root, &source_path),
                &cancel,
                tokio::time::Instant::now() + SOURCE_READ_DEADLINE.min(remaining),
            )
            .await;
            let subject_sha256 = match captured {
                Ok(Ok(hash)) => hash,
                _ => {
                    return inconclusive(
                        criterion,
                        "The document source is unavailable, changed, linked, or exceeds its read limit",
                    );
                }
            };
            let request = InspectionRequest {
                subject: InspectionSubject::Workspace {
                    file_path: path.clone(),
                    expected_sha256: subject_sha256.clone(),
                },
            };
            let inspected = service
                .inspect_for_run(run, execution, request, cancel.clone())
                .await;
            if cancel.is_cancelled() {
                return inconclusive(criterion, "Document observation was cancelled");
            }
            let envelope = match inspected {
                Ok(envelope)
                    if envelope.version == 1
                        && envelope.report_sha256 == envelope.report.sha256()
                        && envelope.report.source().sha256 == subject_sha256 =>
                {
                    envelope
                }
                _ => {
                    let mut observation = inconclusive(
                        criterion,
                        "Fresh native document inspection is unavailable or its source authority changed",
                    );
                    observation.subject_sha256 = Some(subject_sha256);
                    observation.locator = Some(check.locator().into());
                    return observation;
                }
            };
            observation_from_report(criterion, &envelope.report)
        }
        OutcomeCriterion::ArtifactInspection { report_sha256, .. } => {
            let delivery = service.get(&run.agent_id, &run.session_id, report_sha256);
            let delivered = tokio::select! { biased;
                () = cancel.cancelled() => {
                    return inconclusive(criterion, "Document observation was cancelled");
                }
                delivered = delivery => delivered,
            };
            if cancel.is_cancelled() {
                return inconclusive(criterion, "Document observation was cancelled");
            }
            match delivered {
                Ok(envelope)
                    if envelope.version == 1
                        && envelope.report_sha256 == envelope.report.sha256() =>
                {
                    observation_from_report(criterion, &envelope.report)
                }
                _ => inconclusive(
                    criterion,
                    "The exact pinned report and its current source authority are unavailable",
                ),
            }
        }
        _ => inconclusive(
            criterion,
            "This criterion does not request native document facts",
        ),
    }
}

pub(super) fn capture_subject(root: &Path, path: &str) -> Result<String, ()> {
    let workspace = WorkspaceBytes::open(root).map_err(|_| ())?;
    crate::tools::credential_paths::check_path(&workspace.canonical_root, path).map_err(|_| ())?;
    let bytes = workspace
        .read_file_snapshot(path, MAX_DOCUMENT_BYTES)
        .map_err(|_| ())?;
    if bytes.is_empty() {
        return Err(());
    }
    Ok(super::digest(&bytes))
}

fn inconclusive(criterion: &OutcomeCriterion, detail: &str) -> OutcomeObservation {
    OutcomeObservation {
        criterion_id: criterion.id().into(),
        verdict: OutcomeVerdict::Inconclusive,
        subject_sha256: None,
        report_sha256: None,
        locator: None,
        detail: detail.into(),
    }
}

fn observation_from_report(
    criterion: &OutcomeCriterion,
    report: &ArtifactInspectionReport,
) -> OutcomeObservation {
    let check = match criterion {
        OutcomeCriterion::Document { check, .. } => *check,
        OutcomeCriterion::ArtifactInspection {
            report_sha256,
            subject_sha256,
            locator,
            ..
        } if report.sha256() == report_sha256 && &report.source().sha256 == subject_sha256 => {
            match locator.as_str() {
                "complete_rendering" => DocumentCheck::CompleteRendering,
                "recalculated_without_errors" => DocumentCheck::RecalculatedWithoutErrors,
                _ => return inconclusive(criterion, "The pinned report locator is unsupported"),
            }
        }
        _ => {
            return inconclusive(
                criterion,
                "The report does not match the exact public criterion",
            )
        }
    };
    if report.validate().is_err() {
        return inconclusive(criterion, "The native document report is invalid");
    }
    let (verdict, detail) = fact_verdict(report, check);
    OutcomeObservation {
        criterion_id: criterion.id().into(),
        verdict,
        subject_sha256: Some(report.source().sha256.clone()),
        report_sha256: Some(report.sha256().into()),
        locator: Some(check.locator().into()),
        detail: detail.into(),
    }
}

fn fact_verdict(
    report: &ArtifactInspectionReport,
    check: DocumentCheck,
) -> (OutcomeVerdict, &'static str) {
    if report.validate().is_err() {
        return (
            OutcomeVerdict::Inconclusive,
            "The native document report is invalid",
        );
    }
    match check {
        DocumentCheck::CompleteRendering => {
            let rendering = report.rendering();
            let complete = matches!(rendering.coverage, RenderCoverage::CompleteExportedView)
                && rendering.exported_page_count > 0
                && rendering.pages.len() == rendering.exported_page_count as usize
                && rendering
                    .pages
                    .iter()
                    .enumerate()
                    .all(|(index, page)| usize::try_from(page.ordinal).ok() == Some(index + 1));
            if complete {
                (OutcomeVerdict::Met,
                    "Every page in the engine's exported view has raster evidence; layout quality remains unverified")
            } else {
                (
                    OutcomeVerdict::Inconclusive,
                    "Raster coverage of the exported document view is incomplete",
                )
            }
        }
        DocumentCheck::RecalculatedWithoutErrors => {
            if report.source().format != OfficeFormat::Xlsx {
                return (
                    OutcomeVerdict::Inconclusive,
                    "Spreadsheet recalculation requires an XLSX source",
                );
            }
            let inventory = report.formula_inventory();
            let calculation = report.calculation();
            let source_coverage = calculation.source_inventory_sha256 == inventory.sha256
                && calculation.total_formulas as usize == inventory.formulas.len();
            match &calculation.evidence {
                CalculationEvidence::NoFormulas if source_coverage
                    && inventory.formulas.is_empty()
                    && calculation.inspected_formulas == 0
                    && calculation.errors.is_empty() => {
                    (OutcomeVerdict::Met, "The original XLSX inventory contains no formulas to recalculate")
                }
                CalculationEvidence::Recalculated { results } if source_coverage
                    && !inventory.formulas.is_empty()
                    && results.len() == inventory.formulas.len()
                    && calculation.inspected_formulas == calculation.total_formulas
                    && calculation.evaluated_inventory_sha256.is_some()
                    && calculation.result_sha256.is_some()
                    && results.iter().zip(&inventory.formulas).all(|(result, original)| {
                        &result.identity == original
                    }) => {
                    if calculation.errors.is_empty()
                        && results.iter().all(|result| !matches!(result.value, FormulaValue::Error { .. }))
                    {
                        (OutcomeVerdict::Met,
                            "The native engine recalculated every original formula, including unprinted cells, without errors")
                    } else {
                        (OutcomeVerdict::NotMet,
                            "The native engine reported formula errors in the original XLSX inventory")
                    }
                }
                _ => (OutcomeVerdict::Inconclusive,
                    "Complete native recalculation of the original formula inventory is unavailable"),
            }
        }
    }
}

#[cfg(test)]
#[path = "documents_tests.rs"]
mod tests;
