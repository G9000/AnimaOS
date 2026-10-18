use super::*;
use anima_core::{AgentStatus, MessageRole, TaskStatus};
use anima_service_adapters::document_content::{
    DocumentBlock, DocumentExtractionStatus as Status, DocumentLocator, DocumentOmission,
    XlsxCoverage, XlsxSheetCoverage, XlsxSheetVisibility,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Condvar, Mutex,
    },
};
use tokio::sync::RwLock;

const XLSX: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/documents/m66-selected.xlsx"
));
const DOCX: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/documents/m54-selected.docx"
));
const PDF: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/documents/m55-selected.pdf"
));

fn call(path: &str) -> ToolCall {
    ToolCall {
        id: "m67-read".into(),
        name: "read_document".into(),
        args: BTreeMap::from([("file_path".into(), DataValue::String(path.into()))]),
    }
}

#[derive(Default)]
struct Parser {
    calls: Mutex<Vec<(DocumentFormat, Vec<u8>, Instant)>>,
}

#[async_trait]
impl DocumentExtractor for Parser {
    async fn extract(
        &self,
        format: DocumentFormat,
        bytes: Vec<u8>,
        _cancel: CancelSignal,
        deadline: Option<Instant>,
    ) -> Result<ExtractedDocumentText, WorkerFailure> {
        self.calls
            .lock()
            .unwrap()
            .push((format, bytes.clone(), deadline.unwrap()));
        Ok(anima_document_content::extract_document(format, &bytes))
    }
}

fn request(
    root: &std::path::Path,
    path: &str,
    extractor: Arc<dyn DocumentExtractor>,
) -> ReadRequest {
    let (path, format) = arguments(&call(path)).unwrap();
    ReadRequest {
        root: root.to_owned(),
        path,
        format,
        extractor,
        cancel: CancelSignal::new(),
        deadline: Instant::now() + Duration::from_secs(2),
    }
}

fn envelope(text: &str) -> Value {
    assert!(text.len() <= MAX_DOCUMENT_RENDERED_BYTES);
    let value: Value = serde_json::from_str(text).unwrap();
    assert_eq!(value.as_object().unwrap().len(), 4);
    assert_eq!(value["kind"], "workspace_document");
    assert_eq!(value["trust"], "untrusted_data");
    let extraction: ExtractedDocumentText =
        serde_json::from_value(value["extraction"].clone()).unwrap();
    validate_document_extraction(&extraction).unwrap();
    value
}

#[test]
fn m67_document_arguments_are_strict_without_path_rewriting() {
    for path in [
        "",
        "../x.xlsx",
        "x/../x.pdf",
        "/x.docx",
        "C:/x.pdf",
        "x:stream.pdf",
        "\\\\host\\x.xlsx",
        "x\\x.pdf",
        "x//x.pdf",
        "x/./x.pdf",
        "x/",
        "x.xlsx\n",
        "x.txt",
        "x.pdf ",
    ] {
        assert!(arguments(&call(path)).is_err(), "{path:?}");
    }
    assert!(arguments(&call(&format!("{}.pdf", "x".repeat(1021)))).is_err());
    let mut extra = call("x.pdf");
    extra
        .args
        .insert("format".into(), DataValue::String("xlsx".into()));
    assert!(arguments(&extra).is_err());
    let mut wrong = call("x.pdf");
    wrong
        .args
        .insert("file_path".into(), DataValue::Number(1.0));
    assert!(arguments(&wrong).is_err());
    assert_eq!(
        arguments(&call(" reports/ spaced.XLSX")).unwrap(),
        (" reports/ spaced.XLSX".into(), DocumentFormat::Xlsx)
    );
    assert!(arguments(&call(&format!("{}.pdf", "x".repeat(1020)))).is_ok());
}

#[tokio::test]
async fn m67_real_parsers_return_exact_source_hash_and_native_locators() {
    let directory = tempfile::tempdir().unwrap();
    let parser = Arc::new(Parser::default());
    let slots = Arc::new(Semaphore::new(4));
    for (path, bytes, format) in [
        ("report.DOCX", DOCX, DocumentFormat::Docx),
        ("report.pdf", PDF, DocumentFormat::Pdf),
        ("账本.xlsx", XLSX, DocumentFormat::Xlsx),
    ] {
        std::fs::write(directory.path().join(path), bytes).unwrap();
        let text = execute(
            request(directory.path(), path, parser.clone()),
            slots.clone(),
        )
        .await
        .unwrap();
        let value = envelope(&text);
        assert_eq!(
            value["source"],
            json!({"path":path,"sha256":format!("{:x}",Sha256::digest(bytes))})
        );
        let extracted: ExtractedDocumentText =
            serde_json::from_value(value["extraction"].clone()).unwrap();
        assert_eq!(extracted.format, format);
        assert!(!extracted.blocks.is_empty());
        assert!(extracted.blocks.iter().all(|block| matches!(
            (format, block.locator),
            (DocumentFormat::Docx, DocumentLocator::DocxParagraph { .. })
                | (DocumentFormat::Pdf, DocumentLocator::PdfPage { .. })
                | (DocumentFormat::Xlsx, DocumentLocator::XlsxCell { .. })
        )));
        if format == DocumentFormat::Xlsx {
            assert!(text.contains("possibly stale"));
            assert_eq!(
                extracted.xlsx_coverage.unwrap().sheets[1].visibility,
                XlsxSheetVisibility::Hidden
            );
        }
        assert_eq!(std::fs::read(directory.path().join(path)).unwrap(), bytes);
    }
    let calls = parser.calls.lock().unwrap();
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[0].1, DOCX);
    assert_eq!(calls[1].1, PDF);
    assert_eq!(calls[2].1, XLSX);
    assert_eq!(slots.available_permits(), 4);
}

struct Fixed {
    value: ExtractedDocumentText,
    calls: AtomicUsize,
}
#[async_trait]
impl DocumentExtractor for Fixed {
    async fn extract(
        &self,
        _format: DocumentFormat,
        _bytes: Vec<u8>,
        _cancel: CancelSignal,
        _deadline: Option<Instant>,
    ) -> Result<ExtractedDocumentText, WorkerFailure> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.value.clone())
    }
}

#[tokio::test]
async fn m67_source_size_missing_and_nonregular_refuse_before_extractor() {
    let directory = tempfile::tempdir().unwrap();
    let parser = Arc::new(Fixed {
        value: ExtractedDocumentText::empty_for(DocumentFormat::Docx, Status::Malformed),
        calls: AtomicUsize::new(0),
    });
    let slots = Arc::new(Semaphore::new(4));
    std::fs::write(
        directory.path().join("large.docx"),
        vec![0; MAX_DOCUMENT_INPUT_BYTES as usize + 1],
    )
    .unwrap();
    std::fs::create_dir(directory.path().join("directory.docx")).unwrap();
    for path in ["large.docx", "missing.docx", "directory.docx"] {
        assert_eq!(
            execute(
                request(directory.path(), path, parser.clone()),
                slots.clone()
            )
            .await
            .unwrap_err(),
            UNAVAILABLE
        );
    }
    assert_eq!(parser.calls.load(Ordering::SeqCst), 0);
    std::fs::write(
        directory.path().join("exact.docx"),
        vec![0; MAX_DOCUMENT_INPUT_BYTES as usize],
    )
    .unwrap();
    let result = execute(
        request(directory.path(), "exact.docx", parser.clone()),
        slots.clone(),
    )
    .await
    .unwrap();
    assert_eq!(envelope(&result)["extraction"]["status"], "malformed");
    assert_eq!(parser.calls.load(Ordering::SeqCst), 1);
    assert_eq!(slots.available_permits(), 4);
}

#[tokio::test]
async fn m67_wrong_format_and_no_text_remain_typed_results() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("wrong.xlsx"),
        b"plain bytes, not a workbook",
    )
    .unwrap();
    let parser = Arc::new(Parser::default());
    let result = execute(
        request(directory.path(), "wrong.xlsx", parser),
        Arc::new(Semaphore::new(1)),
    )
    .await
    .unwrap();
    assert_eq!(envelope(&result)["extraction"]["status"], "malformed");
    let parser = Arc::new(Fixed {
        value: ExtractedDocumentText::empty_for(DocumentFormat::Docx, Status::NoText),
        calls: AtomicUsize::new(0),
    });
    std::fs::write(directory.path().join("empty.docx"), []).unwrap();
    let result = execute(
        request(directory.path(), "empty.docx", parser),
        Arc::new(Semaphore::new(1)),
    )
    .await
    .unwrap();
    assert_eq!(envelope(&result)["extraction"]["status"], "no_text");
}

#[tokio::test]
async fn m67_captured_snapshot_is_not_reread_after_extraction() {
    struct Replacing {
        path: PathBuf,
    }
    #[async_trait]
    impl DocumentExtractor for Replacing {
        async fn extract(
            &self,
            format: DocumentFormat,
            bytes: Vec<u8>,
            _cancel: CancelSignal,
            _deadline: Option<Instant>,
        ) -> Result<ExtractedDocumentText, WorkerFailure> {
            std::fs::write(&self.path, b"new current contents").unwrap();
            Ok(anima_document_content::extract_document(format, &bytes))
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("captured.xlsx");
    std::fs::write(&path, XLSX).unwrap();
    let parser = Arc::new(Replacing { path: path.clone() });
    let result = execute(
        request(directory.path(), "captured.xlsx", parser),
        Arc::new(Semaphore::new(1)),
    )
    .await
    .unwrap();
    let value = envelope(&result);
    assert_eq!(
        value["source"]["sha256"],
        format!("{:x}", Sha256::digest(XLSX))
    );
    assert!(result.contains("137.42"));
    assert_eq!(std::fs::read(path).unwrap(), b"new current contents");
}

fn large_xlsx() -> ExtractedDocumentText {
    let text = "\"\\账".repeat(120);
    ExtractedDocumentText {
        version: 2,
        format: DocumentFormat::Xlsx,
        status: Status::Partial,
        blocks: (1..=20)
            .map(|row| DocumentBlock {
                locator: DocumentLocator::XlsxCell {
                    sheet: 1,
                    row,
                    column: 1,
                },
                text: text.clone(),
            })
            .collect(),
        omissions: vec![DocumentOmission::StoredCellsOnly],
        pdf_coverage: None,
        xlsx_coverage: Some(XlsxCoverage {
            sheet_count: 1,
            observed_cell_count: 20,
            included_cell_count: 20,
            sheets: vec![XlsxSheetCoverage {
                index: 1,
                name: "引号\"\\".repeat(10),
                visibility: XlsxSheetVisibility::VeryHidden,
                observed_cell_count: 20,
                included_cell_count: 20,
            }],
        }),
    }
}

#[test]
fn m67_exact_envelope_fitting_keeps_whole_cells_and_coverage() {
    let original = large_xlsx();
    let path = format!("{}.xlsx", "\"".repeat(1000));
    let sha = "a".repeat(64);
    assert!(serialize(&path, &sha, &original).unwrap().len() > MAX_DOCUMENT_RENDERED_BYTES);
    let result = fit_envelope(
        &path,
        &sha,
        original.clone(),
        &CancelSignal::new(),
        Instant::now() + Duration::from_secs(2),
    )
    .unwrap();
    let value = envelope(&result);
    assert_eq!(value["source"]["path"], path);
    let extraction: ExtractedDocumentText =
        serde_json::from_value(value["extraction"].clone()).unwrap();
    assert!(!extraction.blocks.is_empty() && extraction.blocks.len() < original.blocks.len());
    for block in &extraction.blocks {
        assert!(original.blocks.contains(block));
    }
    let coverage = extraction.xlsx_coverage.unwrap();
    assert_eq!(coverage.observed_cell_count, 20);
    assert_eq!(
        coverage.included_cell_count as usize,
        extraction.blocks.len()
    );
    assert_eq!(
        coverage.sheets[0].included_cell_count,
        coverage.included_cell_count
    );
    assert!(extraction
        .omissions
        .contains(&DocumentOmission::ContextLimit));
    assert_eq!(
        coverage.sheets[0].visibility,
        XlsxSheetVisibility::VeryHidden
    );
}

#[test]
fn m67_fitting_refuses_invalid_output_and_omission_overflow() {
    let mut output = large_xlsx();
    output.omissions = vec![
        DocumentOmission::StoredCellsOnly,
        DocumentOmission::CellLimit,
        DocumentOmission::CellTextLimit,
        DocumentOmission::TextLimit,
        DocumentOmission::SelectionLimit,
    ];
    validate_document_extraction(&output).unwrap();
    assert_eq!(
        fit_envelope(
            &"\"".repeat(1000),
            &"a".repeat(64),
            output,
            &CancelSignal::new(),
            Instant::now() + Duration::from_secs(2)
        )
        .unwrap_err(),
        UNAVAILABLE
    );
    let mut bad = large_xlsx();
    bad.xlsx_coverage.as_mut().unwrap().included_cell_count = 1;
    assert_eq!(
        fit_envelope(
            "x.xlsx",
            &"a".repeat(64),
            bad,
            &CancelSignal::new(),
            Instant::now() + Duration::from_secs(2)
        )
        .unwrap_err(),
        UNAVAILABLE
    );
}

#[test]
fn m67_docx_envelope_fitting_never_slices_a_paragraph() {
    let mut original = large_xlsx();
    original.format = DocumentFormat::Docx;
    original.status = Status::Ready;
    original.omissions = vec![DocumentOmission::MainBodyOnly];
    original.xlsx_coverage = None;
    for (index, block) in original.blocks.iter_mut().enumerate() {
        block.locator = DocumentLocator::DocxParagraph {
            paragraph: index as u32 + 1,
        };
    }
    let path = format!("{}.docx", "\"".repeat(1000));
    assert!(
        serialize(&path, &"a".repeat(64), &original).unwrap().len() > MAX_DOCUMENT_RENDERED_BYTES
    );
    let result = fit_envelope(
        &path,
        &"a".repeat(64),
        original.clone(),
        &CancelSignal::new(),
        Instant::now() + Duration::from_secs(2),
    )
    .unwrap();
    let value = envelope(&result);
    let fitted: ExtractedDocumentText =
        serde_json::from_value(value["extraction"].clone()).unwrap();
    assert!(!fitted.blocks.is_empty() && fitted.blocks.len() < original.blocks.len());
    assert_eq!(fitted.blocks, original.blocks[..fitted.blocks.len()]);
    assert_eq!(fitted.status, Status::Partial);
    assert!(fitted.omissions.contains(&DocumentOmission::ContextLimit));
}

struct Pending {
    entered: Semaphore,
    retired: Arc<AtomicUsize>,
    deadline: Mutex<Option<Instant>>,
}
struct Retired(Arc<AtomicUsize>);
impl Drop for Retired {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
#[async_trait]
impl DocumentExtractor for Pending {
    async fn extract(
        &self,
        _format: DocumentFormat,
        _bytes: Vec<u8>,
        _cancel: CancelSignal,
        deadline: Option<Instant>,
    ) -> Result<ExtractedDocumentText, WorkerFailure> {
        let _retired = Retired(self.retired.clone());
        *self.deadline.lock().unwrap() = deadline;
        self.entered.add_permits(1);
        std::future::pending().await
    }
}

#[tokio::test]
async fn m67_injected_extractor_obeys_outer_cancel_and_absolute_deadline() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("held.xlsx"), XLSX).unwrap();
    for cancelled in [true, false] {
        let parser = Arc::new(Pending {
            entered: Semaphore::new(0),
            retired: Arc::new(AtomicUsize::new(0)),
            deadline: Mutex::new(None),
        });
        let mut input = request(directory.path(), "held.xlsx", parser.clone());
        input.deadline = Instant::now() + Duration::from_millis(250);
        let deadline = input.deadline;
        let cancel = input.cancel.clone();
        let slots = Arc::new(Semaphore::new(1));
        let task = tokio::spawn(execute(input, slots.clone()));
        let entered = tokio::time::timeout(Duration::from_secs(1), parser.entered.acquire())
            .await
            .unwrap()
            .unwrap();
        entered.forget();
        if cancelled {
            cancel.cancel();
        }
        let result = tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            result.unwrap_err(),
            if cancelled { CANCELLED } else { TIMED_OUT }
        );
        assert_eq!(*parser.deadline.lock().unwrap(), Some(deadline));
        assert_eq!(parser.retired.load(Ordering::SeqCst), 1);
        assert_eq!(slots.available_permits(), 1);
    }
}

#[tokio::test]
async fn m67_precancel_expired_and_capacity_refusal_do_not_dispatch() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("not-read.xlsx"), XLSX).unwrap();
    let parser = Arc::new(Parser::default());
    let slots = Arc::new(Semaphore::new(0));
    let cancelled = request(directory.path(), "not-read.xlsx", parser.clone());
    cancelled.cancel.cancel();
    assert_eq!(
        execute(cancelled, slots.clone()).await.unwrap_err(),
        CANCELLED
    );
    let mut expired = request(directory.path(), "not-read.xlsx", parser.clone());
    expired.deadline = Instant::now();
    assert_eq!(
        execute(expired, slots.clone()).await.unwrap_err(),
        TIMED_OUT
    );
    assert_eq!(
        execute(
            request(directory.path(), "not-read.xlsx", parser.clone()),
            slots
        )
        .await
        .unwrap_err(),
        UNAVAILABLE
    );
    assert!(parser.calls.lock().unwrap().is_empty());
}

struct Gate {
    released: Mutex<bool>,
    wake: Condvar,
}
impl Gate {
    fn wait(&self) {
        let guard = self.released.lock().unwrap();
        drop(
            self.wake
                .wait_timeout_while(guard, Duration::from_secs(2), |released| !*released)
                .unwrap(),
        );
    }
    fn release(&self) {
        *self.released.lock().unwrap() = true;
        self.wake.notify_all();
    }
}
struct Release(Arc<Gate>);
impl Drop for Release {
    fn drop(&mut self) {
        self.0.release();
    }
}

#[tokio::test]
async fn m67_cancel_drop_timeout_retain_all_four_physical_read_leases() {
    let slots = Arc::new(Semaphore::new(4));
    let gate = Arc::new(Gate {
        released: Mutex::new(false),
        wake: Condvar::new(),
    });
    let release = Release(gate.clone());
    let entered = Arc::new(Semaphore::new(0));
    let calls = Arc::new(AtomicUsize::new(0));
    let rejected = Arc::new(AtomicUsize::new(0));
    let mut tasks = Vec::new();
    let mut signals = Vec::new();
    for index in 0..4 {
        let cancel = CancelSignal::new();
        signals.push(cancel.clone());
        let slots = slots.clone();
        let gate = gate.clone();
        let entered = entered.clone();
        let calls = calls.clone();
        let deadline = Instant::now()
            + if index == 2 {
                Duration::from_millis(250)
            } else {
                Duration::from_secs(2)
            };
        tasks.push(tokio::spawn(async move {
            read_retained_with_slots(
                move || {
                    calls.fetch_add(1, Ordering::SeqCst);
                    entered.add_permits(1);
                    gate.wait();
                },
                &cancel,
                deadline,
                slots,
            )
            .await
        }));
    }
    tokio::time::timeout(Duration::from_secs(1), entered.acquire_many(4))
        .await
        .unwrap()
        .unwrap()
        .forget();
    signals[0].cancel();
    tasks[1].abort();
    let mut outcomes = Vec::new();
    for task in tasks.drain(..3) {
        outcomes.push(
            tokio::time::timeout(Duration::from_secs(1), task)
                .await
                .unwrap(),
        );
    }
    let retained = slots.available_permits();
    let count = rejected.clone();
    let fifth = read_retained_with_slots(
        move || {
            count.fetch_add(1, Ordering::SeqCst);
        },
        &CancelSignal::new(),
        Instant::now() + Duration::from_secs(1),
        slots.clone(),
    )
    .await;
    drop(release);
    let final_result = tokio::time::timeout(Duration::from_secs(1), tasks.pop().unwrap())
        .await
        .unwrap()
        .unwrap();
    // Acquiring all four permits proves the detached blocking closures have
    // physically retired before any final call-count/capacity assertions.
    let all = tokio::time::timeout(Duration::from_secs(1), slots.clone().acquire_many_owned(4))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        outcomes[0].as_ref().unwrap(),
        &Err(WorkerFailure::Cancelled)
    );
    assert!(outcomes[1].as_ref().unwrap_err().is_cancelled());
    assert_eq!(outcomes[2].as_ref().unwrap(), &Err(WorkerFailure::TimedOut));
    assert_eq!(final_result, Ok(()));
    assert_eq!(retained, 0);
    assert_eq!(fifth, Err(WorkerFailure::Busy));
    assert_eq!(calls.load(Ordering::SeqCst), 4);
    assert_eq!(rejected.load(Ordering::SeqCst), 0);
    drop(all);
    assert_eq!(slots.available_permits(), 4);
}

#[tokio::test]
async fn m67_handler_requires_root_and_cancel_and_returns_no_attachment_metadata() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("read.xlsx"), XLSX).unwrap();
    let parser = Arc::new(Parser::default());
    let context = ToolExecutionContext::new(
        Arc::new(RwLock::new(anima_memory::MemoryManager::new())),
        Arc::new(RwLock::new(
            crate::memory_embeddings::MemoryEmbeddingRuntime::disabled(),
        )),
        None,
        super::super::ToolRegistry::new(),
        super::super::new_shared_process_manager_with_limit(1),
        Some(directory.path().to_owned()),
        None,
    )
    .with_document_extractor(parser.clone());
    let agent = anima_core::AgentState {
        id: "m67-agent".into(),
        name: "M67".into(),
        status: AgentStatus::Running,
        config: crate::sessions::test_support::agent_config("M67"),
        created_at_ms: 1,
        token_usage: Default::default(),
    };
    let message = Message {
        id: "m67-message".into(),
        agent_id: agent.id.clone(),
        room_id: "m67-room".into(),
        content: Content::default(),
        role: MessageRole::User,
        created_at_ms: 1,
    };
    let missing = read_document(
        context.clone(),
        agent.clone(),
        message.clone(),
        call("read.xlsx"),
    )
    .await;
    assert_eq!(missing.status, TaskStatus::Error);
    let mut no_root = context.clone().with_cancel(Some(CancelSignal::new()));
    no_root.workspace_root = None;
    assert_eq!(
        read_document(no_root, agent.clone(), message.clone(), call("read.xlsx"))
            .await
            .status,
        TaskStatus::Error
    );
    assert!(parser.calls.lock().unwrap().is_empty());
    let result = read_document(
        context.with_cancel(Some(CancelSignal::new())),
        agent,
        message,
        call("read.xlsx"),
    )
    .await;
    assert_eq!(result.status, TaskStatus::Success, "{:?}", result.error);
    let content = result.data.unwrap();
    assert!(content.attachments.is_none());
    assert!(content.metadata.is_none());
    envelope(&content.text);
    assert_eq!(parser.calls.lock().unwrap().len(), 1);
}

#[cfg(unix)]
#[test]
fn m67_generic_reader_refuses_links_at_every_relative_component() {
    use std::os::unix::fs::symlink;
    let directory = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("read.xlsx"), XLSX).unwrap();
    symlink(outside.path(), directory.path().join("ancestor")).unwrap();
    symlink(
        outside.path().join("read.xlsx"),
        directory.path().join("final.xlsx"),
    )
    .unwrap();
    let root = WorkspaceBytes::open(directory.path()).unwrap();
    assert!(root
        .read_file_snapshot("ancestor/read.xlsx", MAX_DOCUMENT_INPUT_BYTES as usize)
        .is_err());
    assert!(root
        .read_file_snapshot("final.xlsx", MAX_DOCUMENT_INPUT_BYTES as usize)
        .is_err());
}

#[test]
fn m67_generic_reader_preserves_existing_hard_link_semantics() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("source.xlsx"), XLSX).unwrap();
    std::fs::hard_link(
        directory.path().join("source.xlsx"),
        directory.path().join("alias.xlsx"),
    )
    .unwrap();
    let root = WorkspaceBytes::open(directory.path()).unwrap();
    assert_eq!(
        root.read_file_snapshot("alias.xlsx", MAX_DOCUMENT_INPUT_BYTES as usize)
            .unwrap(),
        XLSX
    );
}
