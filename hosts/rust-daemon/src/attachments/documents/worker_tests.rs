use super::*;
use anima_core::document_content::{DocumentExtractionStatus, DocumentOmission};
use std::io::Cursor;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn frame(bytes: &[u8]) -> Vec<u8> {
    let mut result = (bytes.len() as u32).to_le_bytes().to_vec();
    result.extend_from_slice(bytes);
    result
}

#[test]
fn worker_request_rejects_oversize_short_and_trailing_input() {
    let payload = b"selected bytes";
    assert_eq!(
        read_request(&mut Cursor::new(frame(payload))).unwrap(),
        payload
    );
    assert!(read_request(&mut Cursor::new((MAX_INPUT as u32 + 1).to_le_bytes())).is_err());
    assert!(read_request(&mut Cursor::new(vec![1, 0, 0, 0])).is_err());
    let mut extra = frame(payload);
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
    let (parent, peer) = tokio::io::duplex(256);
    let (parent_read, parent_write) = tokio::io::split(parent);
    let (mut peer_read, mut peer_write) = tokio::io::split(peer);
    let exchange = tokio::spawn(exchange(parent_write, parent_read, b"selected".to_vec()));
    let mut header = [0; 4];
    assert!(
        tokio::time::timeout(Duration::from_millis(20), peer_read.read_exact(&mut header))
            .await
            .is_err()
    );
    peer_write.write_all(READY).await.unwrap();
    peer_read.read_exact(&mut header).await.unwrap();
    assert_eq!(u32::from_le_bytes(header), 8);
    let mut input = Vec::new();
    peer_read.read_to_end(&mut input).await.unwrap();
    assert_eq!(input, b"selected");
    let value = ExtractedDocumentText::empty(DocumentExtractionStatus::NoText);
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

async fn response(response: Vec<u8>) -> Result<Option<ExtractedDocumentText>, WorkerFailure> {
    let (parent, peer) = tokio::io::duplex(256);
    let (parent_read, parent_write) = tokio::io::split(parent);
    let (mut peer_read, mut peer_write) = tokio::io::split(peer);
    let peer = tokio::spawn(async move {
        peer_write.write_all(READY).await.unwrap();
        let mut request = [0; 4];
        peer_read.read_exact(&mut request).await.unwrap();
        assert_eq!(request, [0; 4]);
        let mut eof = [0; 1];
        assert_eq!(peer_read.read(&mut eof).await.unwrap(), 0);
        let _ = peer_write.write_all(&response).await;
        let _ = peer_write.shutdown().await;
    });
    let result = tokio::time::timeout(
        Duration::from_secs(1),
        exchange(parent_write, parent_read, Vec::new()),
    )
    .await
    .unwrap();
    peer.await.unwrap();
    result
}

#[tokio::test]
async fn malformed_unknown_extra_crashed_and_flooded_results_are_rejected() {
    let valid = serde_json::to_vec(&ExtractedDocumentText::empty(
        DocumentExtractionStatus::NoText,
    ))
    .unwrap();
    let mut trailing = frame(&valid);
    trailing.push(b'x');
    for bad in [
        Vec::new(),
        vec![1, 0, 0, 0],
        frame(b"{}"),
        frame(b"{\"version\":1,\"status\":\"no_text\",\"blocks\":[],\"omissions\":[],\"path\":\"secret\"}"),
        frame(b"{\"version\":1,\"status\":\"ready\",\"blocks\":[],\"omissions\":[]}"),
        ((MAX_FRAME + 1) as u32).to_le_bytes().to_vec(),
        trailing,
    ] {
        assert_eq!(response(bad).await, Err(WorkerFailure::InvalidOutput));
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
        DocumentWorker.extract(Vec::new(), cancel, None).await,
        Err(WorkerFailure::Cancelled)
    );
    assert_eq!(
        DocumentWorker
            .extract(Vec::new(), CancelSignal::new(), Some(Instant::now()))
            .await,
        Err(WorkerFailure::TimedOut)
    );
    assert_eq!(
        DocumentWorker
            .extract(vec![0; MAX_INPUT + 1], CancelSignal::new(), None)
            .await,
        Err(WorkerFailure::InvalidOutput)
    );
}
