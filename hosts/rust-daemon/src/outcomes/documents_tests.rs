use super::*;
use anima_document_visual::inspection::{
    CalculationReport, ExportedView, FormulaError, FormulaIdentity, FormulaInventory,
    FormulaResult, InspectionLimitation, PageEvidence, RasterInk, RenderEvidence, RendererIdentity,
    SourceIdentity,
};

fn identity(address: &str) -> FormulaIdentity {
    FormulaIdentity {
        sheet_id: "sheet:hidden".into(),
        sheet_ordinal: 1,
        address: address.into(),
        formula_sha256: super::super::digest(b"1+1"),
    }
}

fn result(address: &str, value: FormulaValue) -> FormulaResult {
    FormulaResult {
        identity: identity(address),
        engine_formula: "=1+1".into(),
        value,
    }
}

fn report(
    format: OfficeFormat,
    pages: &[u32],
    count: u32,
    formulas: Vec<FormulaIdentity>,
    evidence: CalculationEvidence,
) -> ArtifactInspectionReport {
    let source = SourceIdentity {
        format,
        sha256: super::super::digest(b"original source"),
        byte_length: 100,
        source_version: "captured-outcome-test".into(),
    };
    let renderer = RendererIdentity {
        backend: "libreoffice_uno".into(),
        version: "test".into(),
        executable_sha256: super::super::digest(b"engine"),
        configuration_sha256: super::super::digest(b"configuration"),
        font_manifest_sha256: super::super::digest(b"fonts"),
        rasterizer: "windows_pdf:test".into(),
    };
    let inventory = FormulaInventory {
        sha256: super::super::digest(&serde_json::to_vec(&formulas).unwrap()),
        formulas,
    };
    let missing_pages = (1..=count)
        .filter(|ordinal| !pages.contains(ordinal))
        .collect::<Vec<_>>();
    let rendering = RenderEvidence {
        pdf_sha256: super::super::digest(b"exported pdf"),
        pdf_byte_length: 100,
        exported_page_count: count,
        pages: pages
            .iter()
            .map(|ordinal| PageEvidence {
                ordinal: *ordinal,
                png_sha256: super::super::digest(&ordinal.to_le_bytes()),
                byte_length: 100,
                width: 1,
                height: 1,
                ink: RasterInk {
                    measurement_version: 1,
                    threshold: 245,
                    non_background_pixels: 0,
                    total_pixels: 1,
                },
            })
            .collect(),
        coverage: if missing_pages.is_empty() {
            RenderCoverage::CompleteExportedView
        } else {
            RenderCoverage::Partial {
                missing_pages,
                reason: "exported_pages_not_rendered".into(),
            }
        },
        view: match format {
            OfficeFormat::Docx => ExportedView::WriterPrintLayout,
            OfficeFormat::Xlsx => ExportedView::CalcPrintedSheetsAndPrintAreas,
            OfficeFormat::Pptx => ExportedView::ImpressExportedSlides,
            OfficeFormat::Html => panic!("HTML has no Office inspection fixture"),
        },
    };
    let mut calculation = CalculationReport {
        evidence,
        source_inventory_sha256: inventory.sha256.clone(),
        evaluated_inventory_sha256: None,
        result_sha256: None,
        total_formulas: inventory.formulas.len() as u32,
        inspected_formulas: 0,
        errors: Vec::new(),
    };
    if let CalculationEvidence::Recalculated { results } = &calculation.evidence {
        let evaluated = results
            .iter()
            .map(|result| (&result.identity, &result.engine_formula))
            .collect::<Vec<_>>();
        calculation.evaluated_inventory_sha256 = Some(super::super::digest(
            &serde_json::to_vec(&evaluated).unwrap(),
        ));
        calculation.result_sha256 =
            Some(super::super::digest(&serde_json::to_vec(results).unwrap()));
        calculation.inspected_formulas = results.len() as u32;
        calculation.errors = results
            .iter()
            .filter_map(|result| match result.value {
                FormulaValue::Error { code } => Some(FormulaError {
                    identity: result.identity.clone(),
                    code,
                }),
                _ => None,
            })
            .collect();
    }
    let limitations = vec![
        InspectionLimitation::BackendSpecificOfficeRendering,
        InspectionLimitation::FontManifestCoversDirectorySubset,
        InspectionLimitation::ExportedViewOnly,
        InspectionLimitation::HiddenAndUnprintedContentExcludedFromRendering,
        InspectionLimitation::OriginalFormulaCachesAreUnverified,
        InspectionLimitation::EngineFormulaSyntaxMayBeNormalized,
        InspectionLimitation::RasterInkDoesNotDecideIntentionalBlanknessOrClipping,
        InspectionLimitation::NoGenericLayoutOrOutcomePass,
        InspectionLimitation::ResourceControlsDoNotEstablishOsIsolation,
    ];
    let sha = super::super::digest(
        &serde_json::to_vec(&(
            1_u32,
            &source,
            &renderer,
            &inventory,
            &rendering,
            &calculation,
            &limitations,
        ))
        .unwrap(),
    );
    serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "source": source,
        "renderer": renderer,
        "formula_inventory": inventory,
        "rendering": rendering,
        "calculation": calculation,
        "limitations": limitations,
        "report_sha256": sha,
    }))
    .expect("synthetic report must satisfy the immutable native-fact contract")
}

#[test]
fn complete_rendering_requires_actual_complete_coverage() {
    let complete = report(
        OfficeFormat::Docx,
        &[1, 2],
        2,
        vec![],
        CalculationEvidence::NotApplicable,
    );
    assert_eq!(
        fact_verdict(&complete, DocumentCheck::CompleteRendering).0,
        OutcomeVerdict::Met
    );
    let partial = report(
        OfficeFormat::Docx,
        &[1],
        2,
        vec![],
        CalculationEvidence::NotApplicable,
    );
    assert_eq!(
        fact_verdict(&partial, DocumentCheck::CompleteRendering).0,
        OutcomeVerdict::Inconclusive
    );
    assert!(
        fact_verdict(&complete, DocumentCheck::CompleteRendering)
            .1
            .contains("exported"),
        "rendering coverage must not claim clipping or layout quality"
    );
}

#[test]
fn calculation_requires_all_original_formulas_even_outside_printed_pages() {
    let formulas = vec![identity("C40"), identity("D40")];
    let results = vec![
        result("C40", FormulaValue::Number { value: 2.0 }),
        result("D40", FormulaValue::Number { value: 2.0 }),
    ];
    let report = report(
        OfficeFormat::Xlsx,
        &[1],
        2,
        formulas,
        CalculationEvidence::Recalculated { results },
    );
    assert_eq!(
        fact_verdict(&report, DocumentCheck::RecalculatedWithoutErrors).0,
        OutcomeVerdict::Met,
        "formula coverage is independent of partial printed page coverage"
    );
}

#[test]
fn engine_formula_errors_are_not_met() {
    let report = report(
        OfficeFormat::Xlsx,
        &[1],
        1,
        vec![identity("C40")],
        CalculationEvidence::Recalculated {
            results: vec![result("C40", FormulaValue::Error { code: 532 })],
        },
    );
    assert_eq!(
        fact_verdict(&report, DocumentCheck::RecalculatedWithoutErrors).0,
        OutcomeVerdict::NotMet
    );
}

#[test]
fn caches_unrequested_calculation_and_missing_engine_are_inconclusive() {
    for evidence in [
        CalculationEvidence::NotRequested,
        CalculationEvidence::Inconclusive {
            reason: "engine unavailable".into(),
        },
    ] {
        let report = report(OfficeFormat::Xlsx, &[1], 1, vec![identity("C40")], evidence);
        assert_eq!(
            fact_verdict(&report, DocumentCheck::RecalculatedWithoutErrors).0,
            OutcomeVerdict::Inconclusive
        );
    }
    let word = report(
        OfficeFormat::Docx,
        &[1],
        1,
        vec![],
        CalculationEvidence::NotApplicable,
    );
    assert_eq!(
        fact_verdict(&word, DocumentCheck::RecalculatedWithoutErrors).0,
        OutcomeVerdict::Inconclusive
    );
}

#[test]
fn validated_empty_xlsx_inventory_has_no_formula_errors_to_recalculate() {
    let report = report(
        OfficeFormat::Xlsx,
        &[1],
        1,
        vec![],
        CalculationEvidence::NoFormulas,
    );
    let (verdict, detail) = fact_verdict(&report, DocumentCheck::RecalculatedWithoutErrors);
    assert_eq!(verdict, OutcomeVerdict::Met);
    assert!(detail.contains("no formulas"));
}

#[test]
fn pinned_reports_require_exact_subject_report_and_finite_factual_locator() {
    let report = report(
        OfficeFormat::Docx,
        &[1],
        1,
        vec![],
        CalculationEvidence::NotApplicable,
    );
    let criterion = |report_sha256: &str, subject_sha256: &str, locator: &str| {
        OutcomeCriterion::ArtifactInspection {
            id: "rendered".into(),
            report_sha256: report_sha256.into(),
            subject_sha256: subject_sha256.into(),
            locator: locator.into(),
        }
    };
    let exact = criterion(
        report.sha256(),
        &report.source().sha256,
        "complete_rendering",
    );
    let observed = observation_from_report(&exact, &report);
    assert_eq!(observed.verdict, OutcomeVerdict::Met);
    assert_eq!(observed.report_sha256.as_deref(), Some(report.sha256()));
    assert_eq!(
        observed.subject_sha256.as_deref(),
        Some(report.source().sha256.as_str())
    );
    for criterion in [
        criterion(
            &"a".repeat(64),
            &report.source().sha256,
            "complete_rendering",
        ),
        criterion(report.sha256(), &"b".repeat(64), "complete_rendering"),
        criterion(report.sha256(), &report.source().sha256, "looks good"),
    ] {
        let observed = observation_from_report(&criterion, &report);
        assert_eq!(observed.verdict, OutcomeVerdict::Inconclusive);
        assert!(
            observed.report_sha256.is_none()
                && observed.subject_sha256.is_none()
                && observed.locator.is_none(),
            "mismatched pins cannot emit authoritative references"
        );
    }
}

#[test]
fn fresh_document_observation_cites_its_report_subject_and_check() {
    let report = report(
        OfficeFormat::Pptx,
        &[1],
        1,
        vec![],
        CalculationEvidence::NotApplicable,
    );
    let criterion = OutcomeCriterion::Document {
        id: "slides".into(),
        path: "slides.pptx".into(),
        check: DocumentCheck::CompleteRendering,
    };
    let observed = observation_from_report(&criterion, &report);
    assert_eq!(observed.verdict, OutcomeVerdict::Met);
    assert_eq!(observed.locator.as_deref(), Some("complete_rendering"));
    assert_eq!(
        observed.subject_sha256.as_deref(),
        Some(report.source().sha256.as_str())
    );
    assert_eq!(observed.report_sha256.as_deref(), Some(report.sha256()));
}

#[test]
fn source_capture_hashes_current_bytes_before_native_inspection() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("source.docx");
    std::fs::write(&path, b"original source").unwrap();
    let first = capture_subject(root.path(), "source.docx").unwrap();
    assert_eq!(first, super::super::digest(b"original source"));
    std::fs::write(&path, b"changed source").unwrap();
    let second = capture_subject(root.path(), "source.docx").unwrap();
    assert_ne!(
        first, second,
        "a new check must bind the current source bytes"
    );
}

#[test]
fn source_capture_rejects_escapes_missing_empty_and_oversized_documents() {
    let root = tempfile::tempdir().unwrap();
    assert!(capture_subject(root.path(), "../source.docx").is_err());
    assert!(capture_subject(root.path(), "missing.docx").is_err());
    std::fs::write(root.path().join("source.docx"), []).unwrap();
    assert!(capture_subject(root.path(), "source.docx").is_err());
    std::fs::write(
        root.path().join("source.docx"),
        vec![0; MAX_DOCUMENT_BYTES + 1],
    )
    .unwrap();
    assert!(capture_subject(root.path(), "source.docx").is_err());
}

#[tokio::test]
async fn fresh_document_observation_without_admitted_accounting_stays_inconclusive() {
    let state = std::sync::Arc::new(tokio::sync::RwLock::new(
        crate::state::DaemonState::with_model_adapter(
            crate::agent_runs::test_support::ScriptedModel::new(vec![]),
        ),
    ));
    let service = crate::document_inspections::DocumentInspectionService::new(state);
    let run = crate::runs::RunLink {
        run_id: "run-no-admission".into(),
        agent_id: "agent-no-admission".into(),
        session_id: "chat:no-admission".into(),
    };
    let criterion = OutcomeCriterion::Document {
        id: "rendered".into(),
        path: "report.docx".into(),
        check: DocumentCheck::CompleteRendering,
    };
    let observation =
        observe_document(&criterion, None, &run, None, &service, CancelSignal::new()).await;
    assert_eq!(observation.verdict, OutcomeVerdict::Inconclusive);
    assert!(observation.detail.contains("admitted execution accounting"));
    assert!(observation.report_sha256.is_none());
}
