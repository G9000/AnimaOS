use super::*;
use crate::{
    attachments::{files::save_upload, AttachmentRecord},
    runs::{RunRecord, RunStart},
    sessions::{SessionRecord, TitleSource},
};
use anima_core::document_content::{
    document_rendered_bytes, parse_document_snapshots, DocumentBlock, DocumentLocator, PdfCoverage,
    XlsxCoverage, XlsxSheetCoverage, XlsxSheetVisibility,
};
use async_trait::async_trait;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

const DOCX: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/documents/m54-selected.docx"
));
const PDF: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/documents/m55-selected.pdf"
));
const XLSX: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/documents/m66-selected.xlsx"
));

fn pdf_output() -> ExtractedDocumentText {
    ExtractedDocumentText {
        version: 2,
        format: DocumentFormat::Pdf,
        status: Status::Ready,
        blocks: vec![DocumentBlock {
            locator: DocumentLocator::PdfPage { page: 1 },
            text: "bounded selected PDF content".into(),
        }],
        omissions: vec![DocumentOmission::TextLayerOnly],
        pdf_coverage: Some(PdfCoverage {
            page_count: 2,
            text_page_count: 1,
            included_page_count: 1,
        }),
        xlsx_coverage: None,
    }
}

struct FormatExtractor(std::sync::Mutex<Vec<DocumentFormat>>);
#[async_trait]
impl DocumentExtractor for FormatExtractor {
    async fn extract(
        &self,
        format: DocumentFormat,
        _bytes: Vec<u8>,
        _cancel: CancelSignal,
        _deadline: Option<tokio::time::Instant>,
    ) -> Result<ExtractedDocumentText, WorkerFailure> {
        self.0.lock().unwrap().push(format);
        Ok(match format {
            DocumentFormat::Docx => output(),
            DocumentFormat::Pdf => pdf_output(),
            DocumentFormat::Xlsx => xlsx_output(),
        })
    }
}

struct XlsxCapture(std::sync::Mutex<Vec<(DocumentFormat, Vec<u8>)>>);
#[async_trait]
impl DocumentExtractor for XlsxCapture {
    async fn extract(
        &self,
        format: DocumentFormat,
        bytes: Vec<u8>,
        _cancel: CancelSignal,
        _deadline: Option<tokio::time::Instant>,
    ) -> Result<ExtractedDocumentText, WorkerFailure> {
        self.0.lock().unwrap().push((format, bytes));
        Ok(xlsx_output())
    }
}

fn xlsx_output() -> ExtractedDocumentText {
    let cells = [
        (1, 1, 1, "M66 selected ledger"),
        (1, 2, 1, "137.42"),
        (
            1,
            2,
            2,
            "formula (not evaluated)=SUM(A2,2); cached (possibly stale)=139.42",
        ),
        (1, 2, 3, "true"),
        (2, 1, 1, "Hidden source marker: cobalt 7319"),
    ];
    ExtractedDocumentText {
        version: 2,
        format: DocumentFormat::Xlsx,
        status: Status::Partial,
        blocks: cells
            .into_iter()
            .map(|(sheet, row, column, text)| DocumentBlock {
                locator: DocumentLocator::XlsxCell { sheet, row, column },
                text: text.into(),
            })
            .collect(),
        omissions: vec![DocumentOmission::StoredCellsOnly],
        pdf_coverage: None,
        xlsx_coverage: Some(XlsxCoverage {
            sheet_count: 2,
            observed_cell_count: 5,
            included_cell_count: 5,
            sheets: vec![
                XlsxSheetCoverage {
                    index: 1,
                    name: "Ledger".into(),
                    visibility: XlsxSheetVisibility::Visible,
                    observed_cell_count: 4,
                    included_cell_count: 4,
                },
                XlsxSheetCoverage {
                    index: 2,
                    name: "Private notes".into(),
                    visibility: XlsxSheetVisibility::Hidden,
                    observed_cell_count: 1,
                    included_cell_count: 1,
                },
            ],
        }),
    }
}

fn selected_record(
    temp: &Temp,
    template: &AttachmentRecord,
    id: &str,
    pdf: bool,
) -> AttachmentRecord {
    let (extension, mime, bytes) = if pdf {
        ("pdf", super::super::PDF_MIME, PDF)
    } else {
        ("docx", super::super::DOCX_MIME, DOCX)
    };
    let name = format!("{id}.{extension}");
    let saved = save_upload(&temp.0, "2026-10-16", &name, bytes).unwrap();
    AttachmentRecord {
        id: id.into(),
        name,
        mime: mime.into(),
        size: bytes.len() as u64,
        path: saved.path,
        sha256: saved.sha256,
        workspace_root: saved.workspace_root,
        ..template.clone()
    }
}

fn selected_xlsx_record(temp: &Temp, template: &AttachmentRecord, id: &str) -> AttachmentRecord {
    let name = format!("{id}.xlsx");
    let saved = save_upload(&temp.0, "2026-10-16", &name, XLSX).unwrap();
    AttachmentRecord {
        id: id.into(),
        name,
        mime: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".into(),
        size: XLSX.len() as u64,
        path: saved.path,
        sha256: saved.sha256,
        workspace_root: saved.workspace_root,
        ..template.clone()
    }
}

#[tokio::test]
async fn mixed_documents_share_two_attempts_ten_statuses_and_four_source_reads() {
    let (temp, state, agent, id, config, row) = fixture().await;
    let rows = (0..12)
        .map(|i| selected_record(&temp, &row, &format!("mixed{i}"), i % 2 == 1))
        .collect::<Vec<_>>();
    state
        .read()
        .await
        .history
        .store()
        .upsert_attachments(&rows)
        .await
        .unwrap();
    let extraction = Arc::new(FormatExtractor(std::sync::Mutex::new(Vec::new())));
    let reads = Arc::new(AtomicUsize::new(0));
    let captured = reads.clone();
    let mut input = crate::agent_runs::attachments::attachment_content("original", &rows, None);
    admit_with_reader(
        &state,
        &agent,
        "chat:docs",
        &id,
        RunSource::Web,
        &config,
        &mut input,
        extraction.clone(),
        CancelSignal::new(),
        None,
        Arc::new(move |root, record| {
            captured.fetch_add(1, Ordering::SeqCst);
            read_attachment_bytes(root, record)
        }),
    )
    .await;
    let snapshots = parse_document_snapshots(&input).unwrap();
    assert_eq!(
        *extraction.0.lock().unwrap(),
        [DocumentFormat::Docx, DocumentFormat::Pdf]
    );
    assert_eq!(reads.load(Ordering::SeqCst), 4);
    assert_eq!(snapshots.len(), 10);
    assert_eq!(
        snapshots
            .iter()
            .filter(|s| !s.extraction.blocks.is_empty())
            .count(),
        2
    );
    assert!(document_rendered_bytes(&input) <= MAX_DOCUMENT_RENDERED_BYTES);
    assert_eq!(
        snapshots[1].extraction.pdf_coverage,
        pdf_output().pdf_coverage
    );
    for snapshot in &snapshots[2..] {
        assert_eq!(snapshot.extraction.status, Status::ContextOmitted);
        assert_eq!(
            snapshot.extraction.omissions,
            [DocumentOmission::SelectionLimit]
        );
    }
}

#[tokio::test]
async fn oversized_and_failed_supported_selections_consume_the_attempt_allowance() {
    let (temp, state, agent, id, config, row) = fixture().await;
    let mut oversized = selected_record(&temp, &row, "oversized", false);
    oversized.size = MAX_DOCUMENT_INPUT_BYTES + 1;
    let pdf = selected_record(&temp, &row, "malformed", true);
    let third = selected_record(&temp, &row, "never-attempted", false);
    let rows = [oversized, pdf, third];
    state
        .read()
        .await
        .history
        .store()
        .upsert_attachments(&rows)
        .await
        .unwrap();
    let extraction = Arc::new(Extractor {
        calls: AtomicUsize::new(0),
        output: ExtractedDocumentText::empty_for(DocumentFormat::Pdf, Status::Malformed),
        change: None,
        cancel: false,
    });
    let mut input = crate::agent_runs::attachments::attachment_content("original", &rows, None);
    run(
        &state,
        &agent,
        &id,
        &config,
        &mut input,
        extraction.clone(),
        CancelSignal::new(),
    )
    .await;
    let snapshots = parse_document_snapshots(&input).unwrap();
    assert_eq!(extraction.calls.load(Ordering::SeqCst), 1);
    assert_eq!(snapshots[0].extraction.status, Status::TooLarge);
    assert_eq!(snapshots[1].extraction.status, Status::Malformed);
    assert_eq!(snapshots[1].extraction.format, DocumentFormat::Pdf);
    assert_eq!(snapshots[2].extraction.status, Status::ContextOmitted);
    assert_eq!(
        snapshots[2].extraction.omissions,
        [DocumentOmission::SelectionLimit]
    );
    assert!(snapshots.iter().all(|s| s.extraction.blocks.is_empty()));
}

#[tokio::test]
async fn pdf_worker_format_mismatch_and_cancel_are_empty_pdf_outcomes() {
    let (temp, state, agent, id, config, row) = fixture().await;
    let pdf = selected_record(&temp, &row, "selected", true);
    state
        .read()
        .await
        .history
        .store()
        .upsert_attachments(&[pdf.clone()])
        .await
        .unwrap();
    for cancelled in [false, true] {
        let extraction = Arc::new(Extractor {
            calls: AtomicUsize::new(0),
            output: output(),
            change: None,
            cancel: cancelled,
        });
        let mut input =
            crate::agent_runs::attachments::attachment_content("original", &[pdf.clone()], None);
        run(
            &state,
            &agent,
            &id,
            &config,
            &mut input,
            extraction,
            CancelSignal::new(),
        )
        .await;
        let snapshots = parse_document_snapshots(&input).unwrap();
        assert_eq!(snapshots[0].extraction.format, DocumentFormat::Pdf);
        assert_eq!(
            snapshots[0].extraction.status,
            if cancelled {
                Status::Cancelled
            } else {
                Status::Unavailable
            }
        );
        assert!(snapshots[0].extraction.blocks.is_empty());
    }
}

#[test]
fn document_recognition_requires_exact_kind_mime_and_extension() {
    let mut row = crate::history::conformance::attachment_record("document", "agent", "chat:docs");
    row.kind = AttachmentKind::Document;
    for (name, mime, expected) in [
        (
            "selected.PDF",
            super::super::PDF_MIME,
            Some(DocumentFormat::Pdf),
        ),
        (
            "selected.DOCX",
            super::super::DOCX_MIME,
            Some(DocumentFormat::Docx),
        ),
        ("selected.pdf", super::super::DOCX_MIME, None),
        ("selected.docx", super::super::PDF_MIME, None),
        ("selected.pdf", "application/pdf; charset=utf-8", None),
        ("selected", super::super::PDF_MIME, None),
        (
            "selected.XLSX",
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            Some(DocumentFormat::Xlsx),
        ),
        (
            "selected.xlsx",
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            Some(DocumentFormat::Xlsx),
        ),
        ("selected.xlsx", super::super::DOCX_MIME, None),
    ] {
        row.name = name.into();
        row.mime = mime.into();
        assert_eq!(document_format(&row), expected);
    }
    row.name = "selected.pdf".into();
    row.kind = AttachmentKind::Image;
    assert_eq!(document_format(&row), None);
}

#[tokio::test]
async fn m66_selected_xlsx_admission_keeps_selection_original_and_coverage() {
    let (temp, state, agent, id, config, row) = fixture().await;
    let selected = selected_xlsx_record(&temp, &row, "m66-selected");
    let unselected = selected_xlsx_record(&temp, &row, "m66-unselected");
    state
        .read()
        .await
        .history
        .store()
        .upsert_attachments(&[selected.clone(), unselected.clone()])
        .await
        .unwrap();

    // The unselected attachment exists in the durable store, but only the
    // explicitly attached workbook may be read or passed to the extractor.
    let mut input = crate::agent_runs::attachments::attachment_content(
        "summarize this selected workbook",
        &[selected.clone()],
        None,
    );
    let original_text = input.text.clone();
    let original_refs = input.attachments.clone();
    let reads = Arc::new(std::sync::Mutex::new(Vec::new()));
    let read_capture = reads.clone();
    let extractor = Arc::new(XlsxCapture(std::sync::Mutex::new(Vec::new())));
    admit_with_reader(
        &state,
        &agent,
        "chat:docs",
        &id,
        RunSource::Web,
        &config,
        &mut input,
        extractor.clone(),
        CancelSignal::new(),
        None,
        Arc::new(move |root, record| {
            read_capture.lock().unwrap().push(record.id.clone());
            read_attachment_bytes(root, record)
        }),
    )
    .await;

    assert_eq!(
        *reads.lock().unwrap(),
        [selected.id.clone(), selected.id.clone()]
    );
    assert_eq!(
        *extractor.0.lock().unwrap(),
        [(DocumentFormat::Xlsx, XLSX.to_vec())]
    );
    assert_eq!(input.text, original_text);
    assert_eq!(input.attachments, original_refs);
    assert_eq!(std::fs::read(temp.0.join(&selected.path)).unwrap(), XLSX);

    let snapshots = parse_document_snapshots(&input).unwrap();
    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].attachment_id, selected.id);
    let extraction = &snapshots[0].extraction;
    assert_eq!(extraction.format, DocumentFormat::Xlsx);
    let coverage = extraction.xlsx_coverage.as_ref().unwrap();
    assert_eq!(coverage.sheet_count, 2);
    assert_eq!(coverage.observed_cell_count, 5);
    assert_eq!(coverage.included_cell_count, 5);
    assert_eq!(coverage.sheets[0].visibility, XlsxSheetVisibility::Visible);
    assert_eq!(coverage.sheets[1].visibility, XlsxSheetVisibility::Hidden);
    assert_eq!(
        extraction.blocks[2].locator,
        DocumentLocator::XlsxCell {
            sheet: 1,
            row: 2,
            column: 2,
        }
    );
    assert_eq!(
        extraction.blocks[2].text,
        "formula (not evaluated)=SUM(A2,2); cached (possibly stale)=139.42"
    );
    assert!(document_rendered_bytes(&input) > 0);
    assert!(!snapshots
        .iter()
        .any(|snapshot| snapshot.attachment_id == unselected.id));
}
struct Temp(PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
struct Extractor {
    calls: AtomicUsize,
    output: ExtractedDocumentText,
    change: Option<PathBuf>,
    cancel: bool,
}
#[async_trait]
impl DocumentExtractor for Extractor {
    async fn extract(
        &self,
        _format: DocumentFormat,
        _bytes: Vec<u8>,
        cancel: CancelSignal,
        _deadline: Option<tokio::time::Instant>,
    ) -> Result<ExtractedDocumentText, WorkerFailure> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(path) = &self.change {
            std::fs::write(path, b"changed").unwrap();
        }
        if self.cancel {
            cancel.cancel();
        }
        Ok(self.output.clone())
    }
}
fn output() -> ExtractedDocumentText {
    ExtractedDocumentText {
        version: 2,
        format: DocumentFormat::Docx,
        status: Status::Ready,
        blocks: vec![DocumentBlock {
            locator: DocumentLocator::DocxParagraph { paragraph: 1 },
            text: "bounded selected content".into(),
        }],
        omissions: vec![DocumentOmission::MainBodyOnly],
        pdf_coverage: None,
        xlsx_coverage: None,
    }
}
fn extractor() -> Arc<Extractor> {
    Arc::new(Extractor {
        calls: AtomicUsize::new(0),
        output: output(),
        change: None,
        cancel: false,
    })
}
async fn fixture() -> (
    Temp,
    SharedDaemonState,
    String,
    String,
    AgentConfig,
    AttachmentRecord,
) {
    let temp = Temp(std::env::temp_dir().join(format!("anima-doc-admit-{}", uuid::Uuid::new_v4())));
    std::fs::create_dir_all(&temp.0).unwrap();
    let mut state = crate::state::DaemonState::new();
    state.workspace = Some(crate::control_plane_store::WorkspaceConfig {
        root_path: temp.0.clone(),
        ..Default::default()
    });
    let config: AgentConfig = serde_json::from_value(
        serde_json::json!({"name":"doc admission","model":"test","provider":"test"}),
    )
    .unwrap();
    let agent = state.create_agent(config.clone()).unwrap().state.id;
    state.sessions.insert(SessionRecord::new(
        &agent,
        "chat:docs",
        SessionKind::Chat,
        SessionOrigin::Web,
        "Docs".into(),
        TitleSource::FirstMessage,
        1,
    ));
    let run = RunRecord::running(
        RunStart {
            agent_id: agent.clone(),
            session_id: "chat:docs".into(),
            source: RunSource::Web,
            source_ref: None,
            idempotency_key: None,
            text: "read".into(),
            model: "test".into(),
            provider: Some("test".into()),
            parent_run_id: None,
        },
        1,
    );
    let run_id = run.id.clone();
    state.runs.insert(run);
    let saved = save_upload(&temp.0, "2026-10-16", "selected.docx", DOCX).unwrap();
    let row = AttachmentRecord {
        id: "att-doc".into(),
        agent_id: agent.clone(),
        session_id: "chat:docs".into(),
        name: "selected.docx".into(),
        mime: super::super::DOCX_MIME.into(),
        size: DOCX.len() as u64,
        kind: AttachmentKind::Document,
        path: saved.path,
        created_at_ms: 1,
        workspace_root: saved.workspace_root,
        sha256: saved.sha256,
    };
    state
        .history
        .store()
        .upsert_attachments(&[row.clone()])
        .await
        .unwrap();
    (
        temp,
        Arc::new(tokio::sync::RwLock::new(state)),
        agent,
        run_id,
        config,
        row,
    )
}
async fn run(
    state: &SharedDaemonState,
    agent: &str,
    id: &str,
    config: &AgentConfig,
    input: &mut Content,
    extractor: Arc<dyn DocumentExtractor>,
    cancel: CancelSignal,
) {
    admit(
        state,
        agent,
        "chat:docs",
        id,
        RunSource::Web,
        config,
        input,
        extractor,
        cancel,
        None,
    )
    .await;
}

#[tokio::test]
async fn admission_scope_failure_and_changed_source_never_publish_text() {
    let (temp, state, agent, id, config, row) = fixture().await;
    let extraction = extractor();
    let mut input =
        crate::agent_runs::attachments::attachment_content("original", &[row.clone()], None);
    admit(
        &state,
        &agent,
        "chat:docs",
        &id,
        RunSource::Api,
        &config,
        &mut input,
        extraction.clone(),
        CancelSignal::new(),
        None,
    )
    .await;
    assert!(parse_document_snapshots(&input).unwrap().is_empty());
    let mut foreign = row.clone();
    foreign.id = "att-doc-foreign".into();
    foreign.session_id = "chat:foreign".into();
    let store = state.read().await.history.store();
    assert!(
        store
            .upsert_attachments(&[AttachmentRecord {
                session_id: "chat:foreign".into(),
                ..row.clone()
            }])
            .await
            .is_err(),
        "an existing ID must remain immutable"
    );
    store.upsert_attachments(&[foreign.clone()]).await.unwrap();
    input = crate::agent_runs::attachments::attachment_content("original", &[foreign], None);
    run(
        &state,
        &agent,
        &id,
        &config,
        &mut input,
        extraction.clone(),
        CancelSignal::new(),
    )
    .await;
    assert_eq!(extraction.calls.load(Ordering::SeqCst), 0);
    input = crate::agent_runs::attachments::attachment_content("original", &[row.clone()], None);
    let changed = Arc::new(Extractor {
        calls: AtomicUsize::new(0),
        output: output(),
        change: Some(temp.0.join(&row.path)),
        cancel: false,
    });
    run(
        &state,
        &agent,
        &id,
        &config,
        &mut input,
        changed,
        CancelSignal::new(),
    )
    .await;
    assert_eq!(
        parse_document_snapshots(&input).unwrap()[0]
            .extraction
            .status,
        Status::Unavailable
    );
    assert!(!input.text.contains("bounded selected content"));
}

#[tokio::test]
async fn admission_rejects_malformed_worker_output_and_late_stop_text() {
    let (_temp, state, agent, id, config, row) = fixture().await;
    let mut input = crate::agent_runs::attachments::attachment_content("original", &[row], None);
    let original = input.text.clone();
    let mut malformed = output();
    malformed.blocks[0].locator = DocumentLocator::DocxParagraph { paragraph: 0 };
    let bad = Arc::new(Extractor {
        calls: AtomicUsize::new(0),
        output: malformed,
        change: None,
        cancel: false,
    });
    run(
        &state,
        &agent,
        &id,
        &config,
        &mut input,
        bad,
        CancelSignal::new(),
    )
    .await;
    assert_eq!(
        parse_document_snapshots(&input).unwrap()[0]
            .extraction
            .status,
        Status::Unavailable
    );
    let stopped = Arc::new(Extractor {
        calls: AtomicUsize::new(0),
        output: output(),
        change: None,
        cancel: true,
    });
    run(
        &state,
        &agent,
        &id,
        &config,
        &mut input,
        stopped,
        CancelSignal::new(),
    )
    .await;
    let snapshot = &parse_document_snapshots(&input).unwrap()[0];
    assert_eq!(snapshot.extraction.status, Status::Cancelled);
    assert!(snapshot.extraction.blocks.is_empty());
    assert_eq!(input.text, original);
}

#[tokio::test]
async fn admission_context_fit_respects_fractional_calibration_and_selection_limit() {
    let (temp, state, agent, id, mut config, row) = fixture().await;
    let mut rows = vec![row.clone()];
    for i in 1..3 {
        let mut other = row.clone();
        other.id = format!("att-doc-{i}");
        let saved = save_upload(&temp.0, "2026-10-16", "selected.docx", DOCX).unwrap();
        other.path = saved.path;
        other.name = std::path::Path::new(&other.path)
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .into();
        rows.push(other);
    }
    let store = state.read().await.history.store();
    store.upsert_attachments(&rows).await.unwrap();
    state
        .write()
        .await
        .sessions
        .get_mut(&agent, "chat:docs")
        .unwrap()
        .context_calibration_permille = Some(500);
    let settings = config.settings.get_or_insert_with(Default::default);
    settings.additional.insert(
        "contextBudgetTokens".into(),
        anima_core::DataValue::Number(100.0),
    );
    let mut input = crate::agent_runs::attachments::attachment_content("original", &rows, None);
    let extraction = extractor();
    run(
        &state,
        &agent,
        &id,
        &config,
        &mut input,
        extraction.clone(),
        CancelSignal::new(),
    )
    .await;
    let snapshots = parse_document_snapshots(&input).unwrap();
    assert_eq!(extraction.calls.load(Ordering::SeqCst), 2);
    assert_eq!(snapshots.len(), 3);
    assert!(snapshots.iter().all(|s| s.extraction.blocks.is_empty()));
    assert!(snapshots[2]
        .extraction
        .omissions
        .contains(&DocumentOmission::SelectionLimit));
    assert_eq!(document_rendered_bytes(&input), 0);
    assert!(TokenEstimator::new(0.5).content_tokens(&input) < 100);
}

#[tokio::test]
async fn document_reservation_reduces_agent_file_budget_and_current_context_report() {
    let (_temp, state, agent, id, mut config, row) = fixture().await;
    config
        .settings
        .get_or_insert_with(Default::default)
        .additional
        .insert(
            "contextBudgetTokens".into(),
            anima_core::DataValue::Number(2_000.0),
        );
    let mut input = crate::agent_runs::attachments::attachment_content("original", &[row], None);
    run(
        &state,
        &agent,
        &id,
        &config,
        &mut input,
        extractor(),
        CancelSignal::new(),
    )
    .await;
    assert!(document_rendered_bytes(&input) > 0);
    let estimator = TokenEstimator::default();
    let available = crate::sessions::context::ContextBudget::for_config(&config)
        .history_tokens(estimator.content_tokens(&input));
    let observed = Arc::new(AtomicUsize::new(usize::MAX));
    let captured = observed.clone();
    let (build, _) = crate::agent_files::runtime::admit_run_with_loader(
        &state,
        Arc::new(tokio::sync::Mutex::new(())),
        &agent,
        "chat:docs",
        "chat:docs",
        &id,
        &input,
        &config,
        CancelSignal::new(),
        None,
        move |_, budget, _, _| {
            captured.store(budget.max_total_chars, Ordering::SeqCst);
            Box::pin(async {
                crate::agent_files::AcceptedAgentFiles::empty(
                    crate::agent_files::Availability::Available,
                )
            })
        },
    )
    .await
    .unwrap();
    assert_eq!(
        observed.load(Ordering::SeqCst),
        (available * anima_core::CHARS_PER_TOKEN) as usize
    );
    assert_eq!(
        build.context.raw_estimate_tokens,
        estimator.content_tokens(&input)
    );
}

/// A finite blocking fake proves both read checkpoints release the async run
/// before the physical read returns, while its lease stays owned by the task.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admission_stop_and_absolute_deadline_cover_first_and_postparse_reads() {
    for mode in ["stop", "deadline"] {
        for blocking_call in [1, 2] {
            let (_temp, state, agent, id, config, row) = fixture().await;
            let mut input =
                crate::agent_runs::attachments::attachment_content("original", &[row], None);
            let (entered, observed) = tokio::sync::oneshot::channel();
            let entered = std::sync::Mutex::new(Some(entered));
            let (release, blocked) = std::sync::mpsc::channel();
            let blocked = std::sync::Mutex::new(blocked);
            let calls = AtomicUsize::new(0);
            let reader: SourceReader = Arc::new(move |_, _| {
                if calls.fetch_add(1, Ordering::SeqCst) + 1 == blocking_call {
                    entered.lock().unwrap().take().unwrap().send(()).unwrap();
                    let _ = blocked
                        .lock()
                        .unwrap()
                        .recv_timeout(std::time::Duration::from_secs(2));
                }
                Ok(DOCX.to_vec())
            });
            let cancel = CancelSignal::new();
            let task_cancel = cancel.clone();
            let deadline = (mode == "deadline")
                .then(|| tokio::time::Instant::now() + std::time::Duration::from_millis(500));
            let task = tokio::spawn(async move {
                admit_with_reader(
                    &state,
                    &agent,
                    "chat:docs",
                    &id,
                    RunSource::Web,
                    &config,
                    &mut input,
                    extractor(),
                    task_cancel,
                    deadline,
                    reader,
                )
                .await;
                input
            });
            tokio::time::timeout(std::time::Duration::from_secs(1), observed)
                .await
                .unwrap()
                .unwrap();
            if mode == "stop" {
                cancel.cancel();
            }
            let completed = tokio::time::timeout(std::time::Duration::from_secs(1), task).await;
            let _ = release.send(());
            let input = completed
                .expect("admission waited for a blocked physical read")
                .unwrap();
            let snapshots = parse_document_snapshots(&input).unwrap();
            assert_eq!(snapshots.len(), 1);
            assert!(snapshots[0].extraction.blocks.is_empty());
            assert_eq!(
                snapshots[0].extraction.status,
                if mode == "stop" {
                    Status::Cancelled
                } else {
                    Status::Unavailable
                }
            );
            assert!(!input.text.contains("bounded selected content"));
        }
    }
}
