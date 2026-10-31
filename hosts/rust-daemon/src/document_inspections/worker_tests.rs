use super::*;
use anima_document_visual::inspection::{CalculationEvidence, RendererIdentity};
use std::io::Cursor;

fn test_config() -> EngineConfig {
    EngineConfig {
        program_directory: std::path::PathBuf::from(if cfg!(windows) {
            "C:\\trusted\\LibreOffice\\program"
        } else {
            "/trusted/LibreOffice/program"
        }),
        tree_sha256: "a".repeat(64),
    }
}

fn framed(header: &[u8], source: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    write_frame(&mut bytes, header, MAX_HEADER).unwrap();
    write_frame(&mut bytes, source, MAX_INPUT).unwrap();
    bytes
}

#[test]
fn inspection_request_rejects_wrong_digest_trailing_and_oversized_source() {
    let request = Request::new(OfficeFormat::Docx, "v1".into(), b"source", test_config()).unwrap();
    let header = serde_json::to_vec(&request).unwrap();
    assert!(read_request(&mut Cursor::new(framed(&header, b"wrong"))).is_err());
    let mut frame = framed(&header, b"source");
    frame.push(1);
    assert!(read_request(&mut Cursor::new(frame)).is_err());
    assert!(read_request(&mut Cursor::new((MAX_HEADER as u32 + 1).to_le_bytes())).is_err());
    assert!(Request::new(
        OfficeFormat::Docx,
        "v1".into(),
        &vec![0; MAX_INPUT + 1],
        test_config()
    )
    .is_err());
    assert!(Request::new(OfficeFormat::Html, "v1".into(), b"source", test_config()).is_err());
}

#[test]
fn inspection_response_rejects_wrong_binding_and_unexpected_page_bytes() {
    let request = Request::new(OfficeFormat::Docx, "v1".into(), b"source", test_config()).unwrap();
    let header = serde_json::to_vec(&request).unwrap();
    let mut response = Response {
        version: 1,
        request_sha256: "0".repeat(64),
        source_sha256: request.source_sha256.clone(),
        result: ResponseKind::Rejected {
            message: "unavailable".into(),
        },
        pages: Vec::new(),
    };
    assert!(validate_response(&request, &header, &response, &[]).is_err());
    response.request_sha256 = digest(&header);
    assert!(validate_response(&request, &header, &response, &[]).is_ok());
    let page = PdfPageEvidence {
        page: 1,
        width: 1,
        height: 1,
        png: vec![1],
        recognized: anima_document_visual::RecognizedText::Unavailable,
    };
    assert!(validate_response(&request, &header, &response, &[page]).is_err());
    response.result = ResponseKind::Rejected {
        message: "engine\nprivate source".into(),
    };
    assert!(validate_response(&request, &header, &response, &[]).is_err());
}

#[tokio::test]
async fn inspection_protocol_does_not_send_original_before_resource_ready() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let (mut child_stdin, parent_stdin) = tokio::io::duplex(1024);
    let (parent_stdout, mut child_stdout) = tokio::io::duplex(1024);
    let request = Request::new(
        OfficeFormat::Docx,
        "v1".into(),
        b"private source",
        test_config(),
    )
    .unwrap();
    let task = tokio::spawn(exchange(
        request,
        b"private source".to_vec(),
        parent_stdin,
        parent_stdout,
    ));
    child_stdout.write_all(b"invalid readiness").await.unwrap();
    drop(child_stdout);
    let mut received = Vec::new();
    child_stdin.read_to_end(&mut received).await.unwrap();
    assert!(
        received.is_empty(),
        "source must remain private until checked Job readiness"
    );
    assert!(task.await.unwrap().is_err());
}

#[tokio::test]
async fn inspection_protocol_requires_eof_after_the_final_frame() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let (mut child_stdin, parent_stdin) = tokio::io::duplex(8192);
    let (parent_stdout, mut child_stdout) = tokio::io::duplex(8192);
    let request = Request::new(OfficeFormat::Docx, "v1".into(), b"source", test_config()).unwrap();
    let header = serde_json::to_vec(&request).unwrap();
    let response = Response {
        version: 1,
        request_sha256: digest(&header),
        source_sha256: digest(b"source"),
        result: ResponseKind::Rejected {
            message: "unavailable".into(),
        },
        pages: Vec::new(),
    };
    let task = tokio::spawn(exchange(
        request,
        b"source".to_vec(),
        parent_stdin,
        parent_stdout,
    ));
    child_stdout.write_all(READY).await.unwrap();
    let mut received = Vec::new();
    child_stdin.read_to_end(&mut received).await.unwrap();
    let mut response_bytes = Vec::new();
    write_response(&mut response_bytes, &response, &[]).unwrap();
    response_bytes.push(1);
    child_stdout.write_all(&response_bytes).await.unwrap();
    drop(child_stdout);
    assert!(task.await.unwrap().is_err());
}

#[test]
fn inspection_worker_admission_is_the_existing_physical_document_slot() {
    let slot = crate::attachments::documents::worker::shared_physical_slot();
    let worker = DocumentInspectionWorker::new();
    let permit = slot.clone().try_acquire_owned().unwrap();
    assert!(worker.slots.clone().try_acquire_owned().is_err());
    drop(permit);
    assert!(worker.slots.clone().try_acquire_owned().is_ok());
}

#[tokio::test]
async fn absent_engine_is_explicitly_unavailable_without_parsing_source() {
    let result = DocumentInspectionWorker::new()
        .inspect(
            b"not zip".to_vec(),
            OfficeFormat::Docx,
            "v1".into(),
            CancelSignal::new(),
            Instant::now() + std::time::Duration::from_secs(1),
            None,
        )
        .await;
    assert!(result.unwrap_err().contains("unavailable"));
}

#[test]
fn engine_config_and_controller_do_not_accept_model_commands_or_global_office() {
    assert!(EngineConfig::validate_parts(
        std::path::PathBuf::from("relative/program"),
        "a".repeat(64)
    )
    .is_err());
    assert!(EngineConfig::validate_parts(test_config().program_directory, "bad".into()).is_err());
    assert!(CONTROLLER.contains("calculateAll()"));
    assert!(CONTROLLER.contains("queryContentCells(FORMULA)"));
    assert!(!CONTROLLER.contains("officehelper"));
    assert!(!CONTROLLER.contains("nolockcheck"));
    assert!(!CONTROLLER.contains("shell=True"));
}

fn synthetic_response() -> (Request, Vec<u8>, Response, Vec<PdfPageEvidence>) {
    use anima_document_visual::inspection::{admit_office, build_report};
    let spec = serde_json::from_value(serde_json::json!({"format":"docx","title":"Protocol fixture","blocks":[{"type":"paragraph","text":"Original controlled bytes"}]})).unwrap();
    let original = anima_document_content::render_document(&spec)
        .unwrap()
        .bytes;
    let request = Request::new(OfficeFormat::Docx, "v1".into(), &original, test_config()).unwrap();
    let admission = admit_office(OfficeFormat::Docx, &original).unwrap();
    let mut png = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png, 1, 1);
        encoder.set_color(png::ColorType::Grayscale);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&[255])
            .unwrap();
    }
    let pages = vec![PdfPageEvidence {
        page: 1,
        width: 1,
        height: 1,
        png,
        recognized: RecognizedText::Unavailable,
    }];
    let renderer = RendererIdentity {
        backend: "libreoffice_uno".into(),
        version: "synthetic-protocol-only".into(),
        executable_sha256: digest(b"synthetic engine"),
        configuration_sha256: configuration_digest(&request.engine).unwrap(),
        font_manifest_sha256: digest(b"synthetic font manifest"),
        rasterizer: "windows_pdf:synthetic-protocol-only".into(),
    };
    let report = build_report(
        "v1",
        &admission,
        renderer,
        include_bytes!("../../tests/fixtures/documents/visual-one-page.pdf"),
        &pages,
        CalculationEvidence::NotApplicable,
    )
    .unwrap();
    let response = Response {
        version: 1,
        request_sha256: digest(&serde_json::to_vec(&request).unwrap()),
        source_sha256: request.source_sha256.clone(),
        result: ResponseKind::Inspected { report },
        pages: pages
            .iter()
            .map(|page| PageHeader {
                page: page.page,
                width: page.width,
                height: page.height,
                sha256: digest(&page.png),
                byte_length: page.png.len(),
                recognized: page.recognized.clone(),
            })
            .collect(),
    };
    let header = serde_json::to_vec(&request).unwrap();
    (request, header, response, pages)
}

#[test]
fn inspection_protocol_binds_original_version_configuration_and_each_page() {
    // Synthetic domain facts exercise framing only; no engine-render claim is made.
    let (mut request, header, response, mut pages) = synthetic_response();
    validate_response(&request, &header, &response, &pages).unwrap();
    pages[0].png[0] ^= 1;
    assert!(validate_response(&request, &header, &response, &pages).is_err());
    pages[0].png[0] ^= 1;
    request.source_version = "stale".into();
    assert!(validate_response(&request, &header, &response, &pages).is_err());
    request.source_version = "v1".into();
    request.engine.tree_sha256 = "b".repeat(64);
    assert!(validate_response(&request, &header, &response, &pages).is_err());
}

#[test]
fn inspection_response_aggregate_and_individual_frames_remain_bounded() {
    let (_, _, response, mut pages) = synthetic_response();
    pages[0].png.resize(MAX_PNG + 1, 0);
    assert!(write_response(&mut Vec::new(), &response, &pages).is_err());
    assert!(read_frame(
        &mut Cursor::new((MAX_PNG as u32 + 1).to_le_bytes()),
        MAX_PNG
    )
    .is_err());
}

pub(super) fn test_execution(
    max_artifact_bytes: u64,
) -> (
    ExecutionContext,
    Arc<anima_core::execution_limits::InMemoryExecutionBudget>,
) {
    use anima_core::execution_limits::{
        ExecutionLimits, ExecutionScope, InMemoryExecutionBudget, SystemBudgetClock,
    };
    use anima_core::ExecutionClock;
    let clock: Arc<dyn ExecutionClock> = Arc::new(SystemBudgetClock);
    let mut limits = ExecutionLimits::default();
    limits.budget.max_artifact_bytes = Some(max_artifact_bytes);
    let run_id = uuid::Uuid::new_v4().to_string();
    let scope = ExecutionScope::new(
        uuid::Uuid::new_v4().to_string(),
        run_id.clone(),
        limits,
        clock.now_ms(),
    )
    .unwrap();
    let scope_id = scope.id.clone();
    let deadline_at_ms = scope.deadline_at_ms;
    let port = Arc::new(InMemoryExecutionBudget::new(scope, clock.clone()).unwrap());
    let context = ExecutionContext {
        scope_id,
        run_id,
        port: port.clone(),
        clock,
        cancel: CancelSignal::new(),
        deadline_at_ms,
        failures: Arc::new(std::sync::Mutex::new(None)),
        secondary_cancel: None,
    };
    (context, port)
}

#[tokio::test]
async fn insufficient_shared_artifact_budget_refuses_before_the_absent_backend() {
    let (execution, port) = test_execution(MAX_ARTIFACT_BYTES - 1);
    let scope_id = execution.scope_id.clone();
    let result = DocumentInspectionWorker::new()
        .inspect(
            b"source".to_vec(),
            OfficeFormat::Docx,
            "v1".into(),
            CancelSignal::new(),
            Instant::now() + std::time::Duration::from_secs(60),
            Some(execution),
        )
        .await;
    let error = result.unwrap_err();
    assert!(
        !error.contains("configure the trusted operator installation"),
        "shared allowance must gate backend dispatch"
    );
    assert!(
        port.snapshot(&scope_id).unwrap().reservations.is_empty(),
        "refused admission must not start or reserve a native attempt"
    );
}

#[tokio::test]
async fn absent_backend_settles_admitted_inspection_as_proven_never_sent() {
    use anima_core::execution_limits::SettlementOutcome;
    let (execution, port) = test_execution(MAX_ARTIFACT_BYTES + 1);
    let scope_id = execution.scope_id.clone();
    let result = DocumentInspectionWorker::new()
        .inspect(
            b"source".to_vec(),
            OfficeFormat::Docx,
            "v1".into(),
            CancelSignal::new(),
            Instant::now() + std::time::Duration::from_secs(60),
            Some(execution),
        )
        .await;
    assert!(result
        .unwrap_err()
        .contains("configure the trusted operator installation"));
    let scope = port.snapshot(&scope_id).unwrap();
    assert_eq!(scope.reservations.len(), 1);
    let settlement = scope
        .reservations
        .values()
        .next()
        .unwrap()
        .settlement
        .as_ref()
        .unwrap();
    assert_eq!(settlement.outcome, SettlementOutcome::ProvenNeverSent);
    assert!(
        settlement.physical_retired,
        "no subprocess existed at backend validation"
    );
    assert_eq!(scope.consumption().unwrap().charged_artifact_bytes, 0);
    assert_eq!(scope.consumption().unwrap().inflight_operations, 0);
}

#[tokio::test]
async fn expired_execution_refuses_without_a_new_native_reservation() {
    let (mut execution, port) = test_execution(MAX_ARTIFACT_BYTES + 1);
    execution.deadline_at_ms = execution.clock.now_ms();
    let scope_id = execution.scope_id.clone();
    let result = DocumentInspectionWorker::new()
        .inspect(
            b"source".to_vec(),
            OfficeFormat::Docx,
            "v1".into(),
            CancelSignal::new(),
            Instant::now() + std::time::Duration::from_secs(60),
            Some(execution),
        )
        .await;
    assert!(result.unwrap_err().contains("deadline"));
    assert!(port.snapshot(&scope_id).unwrap().reservations.is_empty());
}

#[test]
fn inspection_artifact_charge_covers_report_pages_and_the_exported_pdf() {
    let (_, _, response, pages) = synthetic_response();
    let ResponseKind::Inspected { report } = response.result else {
        panic!("synthetic inspected report");
    };
    let expected = serde_json::to_vec(&report).unwrap().len() as u64
        + report.rendering().pdf_byte_length
        + pages.iter().map(|page| page.png.len() as u64).sum::<u64>();
    let output = InspectionOutput { report, pages };
    assert_eq!(inspection_artifact_bytes(&output).unwrap(), expected);
    assert!(expected <= MAX_ARTIFACT_BYTES);
}

#[cfg(windows)]
#[tokio::test]
async fn busy_physical_slot_settles_the_shared_reservation_as_never_sent() {
    use anima_core::execution_limits::SettlementOutcome;
    let (execution, port) = test_execution(MAX_ARTIFACT_BYTES + 1);
    let scope_id = execution.scope_id.clone();
    let worker = DocumentInspectionWorker {
        slots: Arc::new(Semaphore::new(0)),
        engine: Some(test_config()),
        test_executable: None,
    };
    let result = worker
        .inspect(
            b"source".to_vec(),
            OfficeFormat::Docx,
            "v1".into(),
            CancelSignal::new(),
            Instant::now() + std::time::Duration::from_secs(60),
            Some(execution),
        )
        .await;
    assert!(result.unwrap_err().contains("Busy"));
    let scope = port.snapshot(&scope_id).unwrap();
    let settlement = scope
        .reservations
        .values()
        .next()
        .unwrap()
        .settlement
        .as_ref()
        .unwrap();
    assert_eq!(settlement.outcome, SettlementOutcome::ProvenNeverSent);
    assert!(settlement.physical_retired);
    assert_eq!(scope.consumption().unwrap().charged_artifact_bytes, 0);
    assert_eq!(scope.consumption().unwrap().inflight_operations, 0);
}

#[test]
fn inspection_deadline_uses_the_existing_execution_time_remaining() {
    let (mut execution, _) = test_execution(MAX_ARTIFACT_BYTES + 1);
    execution.deadline_at_ms = execution.clock.now_ms() + 500;
    let now = Instant::now();
    let deadline =
        inspection_deadline(now + std::time::Duration::from_secs(60), Some(&execution)).unwrap();
    assert!(deadline <= now + std::time::Duration::from_millis(501));
    let earlier = Instant::now() + std::time::Duration::from_millis(10);
    assert_eq!(
        inspection_deadline(earlier, Some(&execution)).unwrap(),
        earlier
    );
}

#[test]
fn inspection_deadline_clamps_an_extreme_finite_budget_before_instant_addition() {
    let (mut execution, _) = test_execution(MAX_ARTIFACT_BYTES + 1);
    execution.deadline_at_ms = u64::MAX;
    let latest = Instant::now() + std::time::Duration::from_secs(61);
    let deadline = inspection_deadline(latest, Some(&execution)).unwrap();
    assert!(
        deadline <= latest,
        "a finite admitted duration must retain the native sixty-second cap"
    );
}

#[tokio::test]
async fn secondary_execution_cancellation_closes_inspection_admission() {
    let (mut execution, port) = test_execution(MAX_ARTIFACT_BYTES + 1);
    let scope_id = execution.scope_id.clone();
    let secondary = CancelSignal::new();
    secondary.cancel();
    execution.secondary_cancel = Some(secondary);
    let result = DocumentInspectionWorker::new()
        .inspect(
            b"source".to_vec(),
            OfficeFormat::Docx,
            "v1".into(),
            CancelSignal::new(),
            Instant::now() + std::time::Duration::from_secs(60),
            Some(execution),
        )
        .await;
    assert!(result.is_err());
    assert!(port.snapshot(&scope_id).unwrap().reservations.is_empty());
}
