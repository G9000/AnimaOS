use super::*;
use anima_service_adapters::document_content::{
    DocumentExtractionStatus, DocumentOmission, PdfCoverage, XlsxCoverage, XlsxSheetCoverage,
    XlsxSheetVisibility,
};
use std::io::Cursor;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn frame(bytes: &[u8]) -> Vec<u8> {
    let mut result = (bytes.len() as u32).to_le_bytes().to_vec();
    result.extend_from_slice(bytes);
    result
}

fn request(format: u8, bytes: &[u8]) -> Vec<u8> {
    let mut result = vec![format];
    result.extend(frame(bytes));
    result
}

fn no_text(format: DocumentFormat) -> ExtractedDocumentText {
    let mut value = ExtractedDocumentText::empty_for(format, DocumentExtractionStatus::NoText);
    if format == DocumentFormat::Pdf {
        value.pdf_coverage = Some(PdfCoverage {
            page_count: 0,
            text_page_count: 0,
            included_page_count: 0,
        });
    }
    if format == DocumentFormat::Xlsx {
        value.xlsx_coverage = Some(XlsxCoverage {
            sheet_count: 1,
            observed_cell_count: 0,
            included_cell_count: 0,
            sheets: vec![XlsxSheetCoverage {
                index: 1,
                name: "Ledger".into(),
                visibility: XlsxSheetVisibility::Visible,
                observed_cell_count: 0,
                included_cell_count: 0,
            }],
        });
    }
    value
}

#[test]
fn worker_request_rejects_oversize_short_and_trailing_input() {
    let payload = b"selected bytes";
    for (byte, format) in [
        (0, DocumentFormat::Docx),
        (1, DocumentFormat::Pdf),
        (2, DocumentFormat::Xlsx),
    ] {
        assert_eq!(
            read_request(&mut Cursor::new(request(byte, payload))).unwrap(),
            (format, payload.to_vec())
        );
        let mut oversize = vec![byte];
        oversize.extend((MAX_INPUT as u32 + 1).to_le_bytes());
        assert!(read_request(&mut Cursor::new(oversize)).is_err());
    }
    for byte in [3, 127, 255] {
        assert!(read_request(&mut Cursor::new(request(byte, payload))).is_err());
    }
    assert!(read_request(&mut Cursor::new(vec![0, 1, 0, 0, 0])).is_err());
    assert!(read_request(&mut Cursor::new(frame(payload))).is_err());
    let mut extra = request(0, payload);
    extra.push(0);
    assert!(read_request(&mut Cursor::new(extra)).is_err());
}

#[test]
fn worker_result_is_strict_and_bounded_before_emission() {
    let value = ExtractedDocumentText::empty(DocumentExtractionStatus::NoText);
    let mut result = Vec::new();
    write_result(&mut result, &value).unwrap();
    assert_eq!(
        u32::from_le_bytes(result[..4].try_into().unwrap()) as usize,
        result.len() - 4
    );
    assert_eq!(
        serde_json::from_slice::<ExtractedDocumentText>(&result[4..]).unwrap(),
        value
    );
    let mut invalid = value;
    invalid.omissions = vec![DocumentOmission::MainBodyOnly; 2];
    let mut output = Vec::new();
    assert!(write_result(&mut output, &invalid).is_err());
    assert!(output.is_empty());
}

#[tokio::test]
async fn no_document_bytes_are_written_before_exact_ready() {
    for (byte, format) in [
        (0, DocumentFormat::Docx),
        (1, DocumentFormat::Pdf),
        (2, DocumentFormat::Xlsx),
    ] {
        let (parent, peer) = tokio::io::duplex(256);
        let (parent_read, parent_write) = tokio::io::split(parent);
        let (mut peer_read, mut peer_write) = tokio::io::split(peer);
        let exchange = tokio::spawn(exchange(
            format,
            parent_write,
            parent_read,
            b"selected".to_vec(),
        ));
        let mut header = [0; 4];
        assert!(
            tokio::time::timeout(Duration::from_millis(20), peer_read.read_exact(&mut header))
                .await
                .is_err()
        );
        peer_write.write_all(READY).await.unwrap();
        assert_eq!(peer_read.read_u8().await.unwrap(), byte);
        peer_read.read_exact(&mut header).await.unwrap();
        assert_eq!(u32::from_le_bytes(header), 8);
        let mut input = Vec::new();
        peer_read.read_to_end(&mut input).await.unwrap();
        assert_eq!(input, b"selected");
        let value = no_text(format);
        peer_write
            .write_all(&frame(&serde_json::to_vec(&value).unwrap()))
            .await
            .unwrap();
        peer_write.shutdown().await.unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), exchange)
                .await
                .unwrap()
                .unwrap()
                .unwrap(),
            Some(value)
        );
    }
}

#[tokio::test]
async fn wrong_or_v1_ready_never_receives_format_length_or_document_bytes() {
    for ready in [
        b"ANIMA-DOCUMENT-WRONG-v2\n".as_slice(),
        b"ANIMA-DOCX-READY-v1\n".as_slice(),
    ] {
        let (parent, peer) = tokio::io::duplex(256);
        let (parent_read, parent_write) = tokio::io::split(parent);
        let (mut peer_read, mut peer_write) = tokio::io::split(peer);
        peer_write.write_all(ready).await.unwrap();
        peer_write.shutdown().await.unwrap();
        assert_eq!(
            tokio::time::timeout(
                Duration::from_secs(1),
                exchange(
                    DocumentFormat::Pdf,
                    parent_write,
                    parent_read,
                    b"selected secret".to_vec()
                )
            )
            .await
            .unwrap(),
            Err(WorkerFailure::InvalidOutput)
        );
        let mut request = Vec::new();
        peer_read.read_to_end(&mut request).await.unwrap();
        assert!(request.is_empty());
    }
}

async fn response(
    format: DocumentFormat,
    response: Vec<u8>,
) -> Result<Option<ExtractedDocumentText>, WorkerFailure> {
    let (parent, peer) = tokio::io::duplex(256);
    let (parent_read, parent_write) = tokio::io::split(parent);
    let (mut peer_read, mut peer_write) = tokio::io::split(peer);
    let peer = tokio::spawn(async move {
        peer_write.write_all(READY).await.unwrap();
        let mut request = [0; 5];
        peer_read.read_exact(&mut request).await.unwrap();
        assert_eq!(
            request,
            [
                match format {
                    DocumentFormat::Docx => 0,
                    DocumentFormat::Pdf => 1,
                    DocumentFormat::Xlsx => 2,
                },
                0,
                0,
                0,
                0
            ]
        );
        let mut eof = [0; 1];
        assert_eq!(peer_read.read(&mut eof).await.unwrap(), 0);
        let _ = peer_write.write_all(&response).await;
        let _ = peer_write.shutdown().await;
    });
    let result = tokio::time::timeout(
        Duration::from_secs(1),
        exchange(format, parent_write, parent_read, Vec::new()),
    )
    .await
    .unwrap();
    peer.await.unwrap();
    result
}

#[tokio::test]
async fn malformed_unknown_extra_crashed_and_flooded_results_are_rejected() {
    for format in [
        DocumentFormat::Docx,
        DocumentFormat::Pdf,
        DocumentFormat::Xlsx,
    ] {
        let valid = serde_json::to_vec(&no_text(format)).unwrap();
        let mut trailing = frame(&valid);
        trailing.push(b'x');
        for bad in [
        Vec::new(),
        vec![1, 0, 0, 0],
        frame(b"{}"),
        frame(b"{\"version\":1,\"status\":\"no_text\",\"blocks\":[],\"omissions\":[]}"),
        frame(b"{\"version\":1,\"format\":\"docx\",\"status\":\"no_text\",\"blocks\":[],\"omissions\":[]}"),
        frame(b"{\"version\":3,\"format\":\"docx\",\"status\":\"no_text\",\"blocks\":[],\"omissions\":[]}"),
        frame(b"{\"version\":2,\"format\":\"unknown\",\"status\":\"no_text\",\"blocks\":[],\"omissions\":[]}"),
        frame(b"{\"version\":2,\"format\":\"docx\",\"status\":\"no_text\",\"blocks\":[],\"omissions\":[],\"path\":\"secret\"}"),
        frame(b"{\"version\":1,\"status\":\"no_text\",\"blocks\":[],\"omissions\":[],\"path\":\"secret\"}"),
        frame(b"{\"version\":1,\"status\":\"ready\",\"blocks\":[],\"omissions\":[]}"),
        ((MAX_FRAME + 1) as u32).to_le_bytes().to_vec(),
        trailing,
    ] {
        assert_eq!(response(format, bad).await, Err(WorkerFailure::InvalidOutput));
    }
        let expected = no_text(format);
        assert_eq!(
            response(format, frame(&serde_json::to_vec(&expected).unwrap())).await,
            Ok(Some(expected))
        );
        let wrong_format = match format {
            DocumentFormat::Docx => DocumentFormat::Pdf,
            DocumentFormat::Pdf | DocumentFormat::Xlsx => DocumentFormat::Docx,
        };
        assert_eq!(
            response(
                format,
                frame(&serde_json::to_vec(&no_text(wrong_format)).unwrap())
            )
            .await,
            Err(WorkerFailure::InvalidOutput)
        );
    }
}

#[tokio::test]
async fn stderr_ceiling_is_exact_and_does_not_wait_for_flood_eof() {
    assert_eq!(drain_stderr(&vec![0; MAX_STDERR][..]).await, Ok(None));
    assert_eq!(
        drain_stderr(&vec![0; MAX_STDERR + 1][..]).await,
        Err(WorkerFailure::InvalidOutput)
    );
    let (reader, mut writer) = tokio::io::duplex(MAX_STDERR + 1);
    writer.write_all(&vec![0; MAX_STDERR + 1]).await.unwrap();
    // Writer deliberately remains open: rejecting a flood must not need EOF.
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), drain_stderr(reader))
            .await
            .unwrap(),
        Err(WorkerFailure::InvalidOutput)
    );
}

#[tokio::test]
async fn pre_cancelled_and_expired_calls_never_need_a_worker() {
    let cancel = CancelSignal::new();
    cancel.cancel();
    assert_eq!(
        DocumentWorker
            .extract(DocumentFormat::Docx, Vec::new(), cancel, None)
            .await,
        Err(WorkerFailure::Cancelled)
    );
    assert_eq!(
        DocumentWorker
            .extract(
                DocumentFormat::Pdf,
                Vec::new(),
                CancelSignal::new(),
                Some(Instant::now())
            )
            .await,
        Err(WorkerFailure::TimedOut)
    );
    assert_eq!(
        DocumentWorker
            .extract(
                DocumentFormat::Pdf,
                vec![0; MAX_INPUT + 1],
                CancelSignal::new(),
                None
            )
            .await,
        Err(WorkerFailure::InvalidOutput)
    );
}

#[test]
fn knowledge_worker_request_and_output_use_a_separate_bounded_envelope() {
    for (code, format) in [
        (3, DocumentFormat::Docx),
        (4, DocumentFormat::Pdf),
        (5, DocumentFormat::Xlsx),
    ] {
        let (mode, parsed, bytes) =
            read_request_mode(&mut Cursor::new(request(code, b"knowledge"))).unwrap();
        assert_eq!(mode, ExtractionMode::Knowledge);
        assert_eq!(parsed, format);
        assert_eq!(bytes, b"knowledge");
    }
    let mut value = ExtractedDocumentText::empty(DocumentExtractionStatus::Ready);
    value
        .blocks
        .push(anima_service_adapters::document_content::DocumentBlock {
            locator: anima_service_adapters::document_content::DocumentLocator::DocxParagraph {
                paragraph: 1,
            },
            text: "x".repeat(32 * 1024),
        });
    assert!(write_result(&mut Vec::new(), &value).is_err());
    let mut output = Vec::new();
    write_result_mode(&mut output, &value, ExtractionMode::Knowledge).unwrap();
    assert!(output.len() > 16 * 1024);
    value.blocks[0].text =
        "x".repeat(anima_service_adapters::document_content::MAX_KNOWLEDGE_DOCUMENT_TEXT_BYTES + 1);
    assert!(write_result_mode(&mut Vec::new(), &value, ExtractionMode::Knowledge).is_err());
}

#[tokio::test]
async fn knowledge_exchange_rejects_oversize_frames_before_allocating_the_body() {
    let (parent, peer) = tokio::io::duplex(256);
    let (parent_read, parent_write) = tokio::io::split(parent);
    let (mut peer_read, mut peer_write) = tokio::io::split(peer);
    let peer = tokio::spawn(async move {
        peer_write.write_all(READY).await.unwrap();
        let mut request = Vec::new();
        peer_read.read_to_end(&mut request).await.unwrap();
        assert_eq!(request, [3, 0, 0, 0, 0]);
        peer_write
            .write_all(&((MAX_KNOWLEDGE_FRAME + 1) as u32).to_le_bytes())
            .await
            .unwrap();
        // Deliberately hold stdout open: rejecting a length must not need EOF.
        tokio::time::sleep(Duration::from_millis(100)).await;
    });
    let result = tokio::time::timeout(
        Duration::from_millis(50),
        exchange_mode(
            DocumentFormat::Docx,
            ExtractionMode::Knowledge,
            parent_write,
            parent_read,
            Vec::new(),
        ),
    )
    .await
    .unwrap();
    assert_eq!(result, Err(WorkerFailure::InvalidOutput));
    peer.await.unwrap();
}
