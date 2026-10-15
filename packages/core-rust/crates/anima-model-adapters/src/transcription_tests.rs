use super::*;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;
use tokio::sync::oneshot;

fn audio() -> WavAudio {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&38_u32.to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&8000_u32.to_le_bytes());
    bytes.extend_from_slice(&16000_u32.to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    bytes.extend_from_slice(&[0, 0]);
    WavAudio::parse(bytes).expect("synthetic PCM fixture")
}

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(2)
}

fn pool() -> Arc<Semaphore> {
    Arc::new(Semaphore::new(OUTGOING_AUDIO_BODIES))
}

// A finite synthetic HTTP peer only: no external authority, provider or service.
// Every socket, accept loop and worker has a hard lifetime and is joined on drop.
struct Peer {
    endpoint: String,
    received: Option<oneshot::Receiver<Vec<u8>>>,
    calls: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Peer {
    fn new(response: Option<Vec<u8>>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("finite peer listener");
        listener
            .set_nonblocking(true)
            .expect("nonblocking listener");
        let endpoint = format!(
            "http://{}/v1/audio/transcriptions",
            listener.local_addr().unwrap()
        );
        let (sender, received) = oneshot::channel();
        let calls = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let worker_calls = Arc::clone(&calls);
        let worker_stop = Arc::clone(&stop);
        let worker = std::thread::spawn(move || {
            let limit = Instant::now() + Duration::from_secs(3);
            let mut sender = Some(sender);
            while Instant::now() < limit && !worker_stop.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        // Winsock accept inherits the listener's nonblocking mode.
                        // This finite worker uses blocking reads with socket deadlines.
                        stream.set_nonblocking(false).unwrap();
                        worker_calls.fetch_add(1, Ordering::SeqCst);
                        stream
                            .set_read_timeout(Some(Duration::from_secs(2)))
                            .unwrap();
                        stream
                            .set_write_timeout(Some(Duration::from_secs(2)))
                            .unwrap();
                        let request = read_request(&mut stream);
                        if let Some(sender) = sender.take() {
                            let _ = sender.send(request);
                        }
                        if let Some(response) = &response {
                            let _ = stream.write_all(response);
                        } else {
                            while Instant::now() < limit && !worker_stop.load(Ordering::SeqCst) {
                                std::thread::sleep(Duration::from_millis(1));
                            }
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            endpoint,
            received: Some(received),
            calls,
            stop,
            worker: Some(worker),
        }
    }

    fn adapter(&self) -> OpenAiTranscriptionAdapter {
        OpenAiTranscriptionAdapter::for_test(self.endpoint.clone(), pool()).unwrap()
    }

    async fn received(&mut self) -> Vec<u8> {
        tokio::time::timeout(Duration::from_secs(2), self.received.take().unwrap())
            .await
            .expect("finite peer request deadline")
            .expect("peer captured request")
    }

    async fn assert_one_call(&self) {
        // Retain the listener briefly so redirects/retries would be observed.
        tokio::time::sleep(Duration::from_millis(75)).await;
        assert_eq!(self.calls.load(Ordering::SeqCst), 1);
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            worker.join().expect("finite peer joined");
        }
    }
}

fn read_request(stream: &mut TcpStream) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 4096];
    let mut expected = None;
    while bytes.len() < 4 * 1024 * 1024 + 16 * 1024 {
        match stream.read(&mut chunk) {
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Ok(0) | Err(_) => break,
            Ok(count) => bytes.extend_from_slice(&chunk[..count]),
        }
        if expected.is_none() {
            if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                assert!(end < 16 * 1024, "bounded request headers");
                let headers = std::str::from_utf8(&bytes[..end]).unwrap();
                let length = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().unwrap())
                    })
                    .expect("multipart length is known");
                expected = Some(end + 4 + length);
            }
        }
        if expected.is_some_and(|length| bytes.len() >= length) {
            break;
        }
    }
    bytes
}

fn response(status: &str, content_type: &str, body: &[u8]) -> Vec<u8> {
    let mut bytes = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()
    ).into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

#[test]
fn constructor_is_api_key_only_and_global_body_leases_are_shared() {
    assert!(matches!(
        OpenAiTranscriptionAdapter::new(String::new()),
        Err(TranscriptionError::Unavailable)
    ));
    assert!(matches!(
        OpenAiTranscriptionAdapter::new("synthetic\nkey".into()),
        Err(TranscriptionError::Unavailable)
    ));
    let first = OpenAiTranscriptionAdapter::new("synthetic-key-one".into()).unwrap();
    let second = OpenAiTranscriptionAdapter::new("synthetic-key-two".into()).unwrap();
    assert_eq!(first.endpoint, ENDPOINT);
    assert_eq!(first.availability(), TranscriptionAvailability::Available);
    assert!(Arc::ptr_eq(&first.leases, &second.leases));
    assert!(OpenAiTranscriptionAdapter::for_test(
        "http://example.com/v1/audio/transcriptions".into(),
        pool()
    )
    .is_err());
}

#[tokio::test]
async fn request_is_fixed_multipart_and_success_is_trimmed() {
    let mut peer = Peer::new(Some(response(
        "200 OK",
        "text/plain; charset=utf-8",
        b" \nhello world\r\n",
    )));
    let adapter = peer.adapter();
    assert_eq!(
        adapter
            .transcribe(audio(), CancelSignal::new(), deadline())
            .await
            .unwrap(),
        "hello world"
    );
    let request = peer.received().await;
    let text = String::from_utf8_lossy(&request);
    let header_end = text.find("\r\n\r\n").unwrap();
    let headers = text[..header_end].to_ascii_lowercase();
    assert!(headers.starts_with("post /v1/audio/transcriptions http/1.1\r\n"));
    assert!(headers.contains("authorization: bearer synthetic-test-key\r\n"));
    assert!(headers.contains("content-type: multipart/form-data; boundary="));
    assert!(text.contains("name=\"model\"\r\n\r\nwhisper-1\r\n"));
    assert!(
        text.contains("name=\"file\"; filename=\"audio.wav\"\r\nContent-Type: audio/wav\r\n\r\n")
    );
    assert!(text.contains("name=\"response_format\"\r\n\r\ntext\r\n"));
    assert!(!text.contains("name=\"language\""));
    assert!(!text.contains("name=\"prompt\""));
    assert!(request
        .windows(audio().bytes().len())
        .any(|window| window == audio().bytes()));
    peer.assert_one_call().await;
}

#[tokio::test]
async fn m56_peer_regression_waits_for_the_complete_fragmented_request_body() {
    let mut peer = Peer::new(Some(response("200 OK", "text/plain", b"hello")));
    let authority = peer
        .endpoint
        .strip_prefix("http://")
        .unwrap()
        .split('/')
        .next()
        .unwrap();
    let mut client = TcpStream::connect(authority).expect("finite loopback client");
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    client
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    client
        .write_all(b"POST /v1/audio/transcriptions HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 4\r\nConnection: close\r\n\r\nAB")
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        matches!(
            peer.received.as_mut().unwrap().try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ),
        "peer must not publish or answer an incomplete request"
    );
    client.write_all(b"CD").unwrap();
    let request = peer.received().await;
    assert!(request.ends_with(b"ABCD"));
    let mut reply = Vec::new();
    client.read_to_end(&mut reply).unwrap();
    assert!(reply.starts_with(b"HTTP/1.1 200 OK\r\n"));
    peer.assert_one_call().await;
}

#[tokio::test]
async fn redirects_and_provider_failures_do_not_retry_or_expose_bodies() {
    for status in [
        "401 Unauthorized",
        "429 Too Many Requests",
        "500 Internal Server Error",
        "503 Service Unavailable",
    ] {
        let peer = Peer::new(Some(response(
            status,
            "text/plain",
            b"private-provider-body synthetic-test-key",
        )));
        let error = peer
            .adapter()
            .transcribe(audio(), CancelSignal::new(), deadline())
            .await
            .unwrap_err();
        assert_eq!(error, TranscriptionError::Failed);
        let diagnostic = format!("{error:?}");
        assert!(!diagnostic.contains("private-provider-body"));
        assert!(!diagnostic.contains("synthetic-test-key"));
        peer.assert_one_call().await;
    }
    let mut target = Peer::new(Some(response("200 OK", "text/plain", b"redirected")));
    let wire = format!("HTTP/1.1 307 Temporary Redirect\r\nLocation: {}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", target.endpoint).into_bytes();
    let peer = Peer::new(Some(wire));
    assert_eq!(
        peer.adapter()
            .transcribe(audio(), CancelSignal::new(), deadline())
            .await,
        Err(TranscriptionError::Failed)
    );
    peer.assert_one_call().await;
    assert_eq!(target.calls.load(Ordering::SeqCst), 0);
    // Drop the unused oneshot explicitly: no request should reach the redirect target.
    target.received.take();
}

#[tokio::test]
async fn response_must_be_bounded_plain_utf8_and_valid_text() {
    let cases = [
        ("application/json", b"{\"text\":\"hello\"}".to_vec()),
        ("text/plain; charset=iso-8859-1", b"hello".to_vec()),
        ("text/plain", vec![0xff, 0xfe]),
        ("text/plain", b" \r\n\t".to_vec()),
        ("text/plain", b"hello\0world".to_vec()),
        ("text/plain", vec![b'x'; MAX_TRANSCRIPT_BYTES + 1]),
    ];
    for (content_type, body) in cases {
        let peer = Peer::new(Some(response("200 OK", content_type, &body)));
        assert_eq!(
            peer.adapter()
                .transcribe(audio(), CancelSignal::new(), deadline())
                .await,
            Err(TranscriptionError::InvalidResponse)
        );
    }
    let peer = Peer::new(Some(response(
        "200 OK",
        "text/plain",
        &vec![b'x'; MAX_TRANSCRIPT_BYTES],
    )));
    assert_eq!(
        peer.adapter()
            .transcribe(audio(), CancelSignal::new(), deadline())
            .await
            .unwrap()
            .len(),
        MAX_TRANSCRIPT_BYTES
    );
}

#[tokio::test]
async fn chunked_response_cap_rejects_without_content_length() {
    let body = vec![b'x'; MAX_TRANSCRIPT_BYTES + 1];
    let mut wire = format!("HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n", body.len()).into_bytes();
    wire.extend_from_slice(&body);
    wire.extend_from_slice(b"\r\n0\r\n\r\n");
    let peer = Peer::new(Some(wire));
    assert_eq!(
        peer.adapter()
            .transcribe(audio(), CancelSignal::new(), deadline())
            .await,
        Err(TranscriptionError::InvalidResponse)
    );
}

#[tokio::test]
async fn precancel_and_expired_deadline_make_zero_calls() {
    let peer = Peer::new(None);
    let adapter = peer.adapter();
    let cancel = CancelSignal::new();
    cancel.cancel();
    assert_eq!(
        adapter.transcribe(audio(), cancel, deadline()).await,
        Err(TranscriptionError::Cancelled)
    );
    assert_eq!(
        adapter
            .transcribe(audio(), CancelSignal::new(), Instant::now())
            .await,
        Err(TranscriptionError::TimedOut)
    );
    assert_eq!(peer.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn cancellation_and_absolute_deadline_drop_a_stalled_http_await() {
    let mut peer = Peer::new(None);
    let adapter = peer.adapter();
    let cancel = CancelSignal::new();
    let signal = cancel.clone();
    let operation =
        tokio::spawn(async move { adapter.transcribe(audio(), cancel, deadline()).await });
    peer.received().await;
    signal.cancel();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), operation)
            .await
            .unwrap()
            .unwrap(),
        Err(TranscriptionError::Cancelled)
    );

    let mut peer = Peer::new(None);
    let adapter = peer.adapter();
    let end = Instant::now() + Duration::from_millis(150);
    let operation =
        tokio::spawn(async move { adapter.transcribe(audio(), CancelSignal::new(), end).await });
    peer.received().await;
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), operation)
            .await
            .unwrap()
            .unwrap(),
        Err(TranscriptionError::TimedOut)
    );
}

#[tokio::test]
async fn retained_physical_bytes_hold_nonqueued_permit_across_operation_drop() {
    let leases = pool();
    let first = leased_audio(audio(), Arc::clone(&leases)).unwrap();
    let clone = first.clone();
    let body = reqwest::Body::from(first.clone());
    let second = leased_audio(audio(), Arc::clone(&leases)).unwrap();
    assert!(matches!(
        leased_audio(audio(), Arc::clone(&leases)),
        Err(TranscriptionError::Busy)
    ));
    let mut operation = Box::pin(async move {
        let _retained = first;
        std::future::pending::<()>().await;
    });
    assert!(futures::poll!(&mut operation).is_pending());
    drop(operation);
    assert_eq!(leases.available_permits(), 0);
    drop(clone);
    assert_eq!(leases.available_permits(), 0);
    drop(body);
    assert_eq!(leases.available_permits(), 1);
    drop(second);
    assert_eq!(leases.available_permits(), 2);
}
