use super::*;
use std::io::Cursor;

#[test]
fn document_edit_worker_screens_private_headers_and_notes_without_returning_the_text() {
    for (format, source) in [
        (
            OfficeFormat::Docx,
            include_bytes!("fixtures/header-credential.docx").as_slice(),
        ),
        (
            OfficeFormat::Pptx,
            include_bytes!("fixtures/notes-credential.pptx").as_slice(),
        ),
    ] {
        let request = Request::inspect(format, source).unwrap();
        let header = serde_json::to_vec(&request).unwrap();
        let (response, candidate) = process_request(request, &header, source);
        assert!(matches!(response.result, ResponseKind::Rejected { .. }));
        assert!(candidate.is_empty());
        assert!(!serde_json::to_string(&response)
            .unwrap()
            .contains("synthetic-fixture"));
    }
    for (format, source) in [
        (
            OfficeFormat::Docx,
            include_bytes!("fixtures/header-safe.docx").as_slice(),
        ),
        (
            OfficeFormat::Pptx,
            include_bytes!("fixtures/notes-safe.pptx").as_slice(),
        ),
    ] {
        let request = Request::inspect(format, source).unwrap();
        let header = serde_json::to_vec(&request).unwrap();
        let (response, candidate) = process_request(request, &header, source);
        assert!(
            matches!(response.result, ResponseKind::Inventory { .. }),
            "{:?}",
            response
        );
        assert!(candidate.is_empty());
    }
}

#[test]
fn document_edit_worker_screens_the_entire_candidate_after_safe_input() {
    let source = include_bytes!("fixtures/header-safe.docx");
    let edits = OfficeEditSpec {
        operations: vec![
            anima_document_content::OfficeEditOperation::WordReplaceText {
                target_id: "word:0".into(),
                start: 0,
                end: 11,
                expected_text: "Public body".into(),
                new_text: "password=synthetic-fixture".into(),
            },
        ],
    };
    let request = Request::prepare(OfficeFormat::Docx, edits, source).unwrap();
    let header = serde_json::to_vec(&request).unwrap();
    let (response, candidate) = process_request(request, &header, source);
    assert!(matches!(response.result, ResponseKind::Rejected { .. }));
    assert!(candidate.is_empty());
    assert!(!serde_json::to_string(&response)
        .unwrap()
        .contains("synthetic-fixture"));
}

#[test]
fn document_edit_worker_uses_the_extractors_real_physical_slot() {
    let slot = crate::attachments::documents::worker::shared_physical_slot();
    let worker = DocumentEditWorker::new(slot.clone());
    let occupied = slot.clone().try_acquire_owned().unwrap();
    assert!(worker.slots.clone().try_acquire_owned().is_err());
    drop(occupied);
    assert!(worker.slots.clone().try_acquire_owned().is_ok());
}

#[test]
fn document_edit_worker_rejects_trailing_and_oversized_requests() {
    let request = Request::inspect(OfficeFormat::Docx, b"not a document").unwrap();
    let header = serde_json::to_vec(&request).unwrap();
    let mut frame = Vec::new();
    frame.extend_from_slice(&(header.len() as u32).to_le_bytes());
    frame.extend_from_slice(&header);
    frame.extend_from_slice(&(14u32).to_le_bytes());
    frame.extend_from_slice(b"not a document");
    frame.push(1);
    assert!(read_request(&mut Cursor::new(frame)).is_err());
    assert!(read_request(&mut Cursor::new((MAX_HEADER as u32 + 1).to_le_bytes())).is_err());
}

#[test]
fn document_edit_worker_rejects_wrong_request_digest_and_invented_candidates() {
    let request = Request::inspect(OfficeFormat::Docx, b"input").unwrap();
    let header = serde_json::to_vec(&request).unwrap();
    let response = Response {
        version: 1,
        request_sha256: "0".repeat(64),
        source_sha256: request.source_sha256.clone(),
        result: ResponseKind::Rejected {
            message: "unsupported".into(),
        },
    };
    assert!(validate_response(&request, &header, &response, &[]).is_err());
    let mut response = response;
    response.request_sha256 = digest(&header);
    assert!(validate_response(&request, &header, &response, b"unexpected").is_err());
}

#[tokio::test]
async fn document_edit_worker_never_sends_source_before_ready() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let (mut child_stdin, parent_stdin) = tokio::io::duplex(1024);
    let (parent_stdout, mut child_stdout) = tokio::io::duplex(1024);
    let request = Request::inspect(OfficeFormat::Docx, b"private input").unwrap();
    let task = tokio::spawn(exchange(
        request,
        b"private input".to_vec(),
        parent_stdin,
        parent_stdout,
    ));
    child_stdout.write_all(b"not-ready\n").await.unwrap();
    drop(child_stdout);
    let mut data = Vec::new();
    child_stdin.read_to_end(&mut data).await.unwrap();
    assert!(data.is_empty());
    assert!(task.await.unwrap().is_err());
}
