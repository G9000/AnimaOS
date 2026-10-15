use super::*;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;
use tokio::sync::oneshot;

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(2)
}

fn text() -> SpeechText {
    SpeechText::new("  Read 🌍 exactly.\n".into()).unwrap()
}

fn pool() -> Arc<Semaphore> {
    Arc::new(Semaphore::new(OUTGOING_JSON_BODIES))
}

struct Peer {
    endpoint: String,
    received: Option<oneshot::Receiver<Vec<u8>>>,
    calls: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Peer {
    fn new(response: Option<Vec<u8>>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}/v1/audio/speech", listener.local_addr().unwrap());
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
                    Ok((mut socket, _)) => {
                        socket.set_nonblocking(false).unwrap();
                        socket
                            .set_read_timeout(Some(Duration::from_secs(2)))
                            .unwrap();
                        socket
                            .set_write_timeout(Some(Duration::from_secs(2)))
                            .unwrap();
                        worker_calls.fetch_add(1, Ordering::SeqCst);
                        let request = read_request(&mut socket);
                        if let Some(sender) = sender.take() {
                            let _ = sender.send(request);
                        }
                        if let Some(bytes) = &response {
                            let _ = socket.write_all(bytes);
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

    fn adapter(&self) -> OpenAiSpeechAdapter {
        OpenAiSpeechAdapter::for_test(self.endpoint.clone(), pool()).unwrap()
    }

    async fn received(&mut self) -> Vec<u8> {
        tokio::time::timeout(Duration::from_secs(2), self.received.take().unwrap())
            .await
            .unwrap()
            .unwrap()
    }

    async fn assert_one_call(&self) {
        tokio::time::sleep(Duration::from_millis(75)).await;
        assert_eq!(self.calls.load(Ordering::SeqCst), 1);
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            worker.join().unwrap();
        }
    }
}

fn read_request(socket: &mut TcpStream) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut expected = None;
    let mut chunk = [0_u8; 4096];
    while bytes.len() < 16 * 1024 {
        match socket.read(&mut chunk) {
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Ok(0) | Err(_) => break,
            Ok(count) => bytes.extend_from_slice(&chunk[..count]),
        }
        if expected.is_none() {
            if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                assert!(end < 4 * 1024);
                let headers = std::str::from_utf8(&bytes[..end]).unwrap();
                let length = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().unwrap())
                    })
                    .expect("fixed JSON length");
                expected = Some(end + 4 + length);
            }
        }
        if expected.is_some_and(|length| bytes.len() >= length) {
            break;
        }
    }
    bytes
}

fn response(status: &str, mime: &str, encoding: Option<&str>, body: &[u8]) -> Vec<u8> {
    let encoding = encoding
        .map(|value| format!("Content-Encoding: {value}\r\n"))
        .unwrap_or_default();
    let mut wire = format!("HTTP/1.1 {status}\r\nContent-Type: {mime}\r\n{encoding}Content-Length: {}\r\nConnection: close\r\n\r\n", body.len()).into_bytes();
    wire.extend_from_slice(body);
    wire
}

#[test]
fn constructor_has_fixed_authority_and_rejects_foreign_test_endpoints() {
    assert!(matches!(
        OpenAiSpeechAdapter::new(String::new()),
        Err(SpeechError::Unavailable)
    ));
    assert!(matches!(
        OpenAiSpeechAdapter::new("bad\nkey".into()),
        Err(SpeechError::Unavailable)
    ));
    let first = OpenAiSpeechAdapter::new("synthetic-one".into()).unwrap();
    let second = OpenAiSpeechAdapter::new("synthetic-two".into()).unwrap();
    assert_eq!(first.endpoint, ENDPOINT);
    assert_eq!(first.availability(), SpeechAvailability::Available);
    assert!(Arc::ptr_eq(&first.leases, &second.leases));
    for endpoint in [
        "http://example.com/v1/audio/speech",
        "http://127.0.0.1:1234/other",
        "http://127.0.0.1/v1/audio/speech",
        "http://127.0.0.1:1234/v1/audio/speech?x=1",
    ] {
        assert!(OpenAiSpeechAdapter::for_test(endpoint.into(), pool()).is_err());
    }
}

#[tokio::test]
async fn request_has_exact_defaults_and_preserves_input() {
    let mut peer = Peer::new(Some(response("200 OK", "audio/pcm", None, &[0; 12_000])));
    let output = peer
        .adapter()
        .synthesize(text(), CancelSignal::new(), deadline())
        .await
        .unwrap();
    assert_eq!(output.duration_ms(), 250);
    let request = peer.received().await;
    let header_end = request
        .windows(4)
        .position(|part| part == b"\r\n\r\n")
        .unwrap();
    let headers = String::from_utf8_lossy(&request[..header_end]).to_ascii_lowercase();
    assert!(headers.starts_with("post /v1/audio/speech http/1.1\r\n"));
    assert!(headers.contains("authorization: bearer synthetic-test-key\r\n"));
    assert!(headers.contains("content-type: application/json\r\n"));
    let body: serde_json::Value = serde_json::from_slice(&request[header_end + 4..]).unwrap();
    assert_eq!(body.as_object().unwrap().len(), 5);
    assert_eq!(
        body,
        serde_json::json!({
            "model": "gpt-4o-mini-tts", "voice": "alloy", "input": "  Read 🌍 exactly.\n",
            "response_format": "pcm", "speed": 1.0
        })
    );
    assert!(!headers.contains("idempotency"));
    peer.assert_one_call().await;
}

#[tokio::test]
async fn finite_peer_waits_for_complete_fragmented_request_body() {
    let mut peer = Peer::new(Some(response("200 OK", "audio/pcm", None, b"xx")));
    let authority = peer
        .endpoint
        .strip_prefix("http://")
        .unwrap()
        .split('/')
        .next()
        .unwrap();
    let mut client = TcpStream::connect(authority).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    client
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    client.write_all(b"POST /v1/audio/speech HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 4\r\nConnection: close\r\n\r\nAB").unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(matches!(
        peer.received.as_mut().unwrap().try_recv(),
        Err(oneshot::error::TryRecvError::Empty)
    ));
    client.write_all(b"CD").unwrap();
    assert!(peer.received().await.ends_with(b"ABCD"));
    let mut reply = Vec::new();
    client.read_to_end(&mut reply).unwrap();
    assert!(reply.starts_with(b"HTTP/1.1 200 OK\r\n"));
    peer.assert_one_call().await;
}

#[tokio::test]
async fn response_rejects_status_mime_encoding_empty_odd_and_oversize() {
    let cases = [
        response("401 Unauthorized", "audio/pcm", None, b"secret"),
        response("307 Temporary Redirect", "audio/pcm", None, b"xx"),
        response("200 OK", "audio/wav", None, b"xx"),
        response("200 OK", "audio/pcm; charset=utf-8", None, b"xx"),
        response("200 OK", "audio/pcm", Some("gzip"), b"xx"),
        response("200 OK", "audio/pcm", None, b""),
        response("200 OK", "audio/pcm", None, b"x"),
        response(
            "200 OK",
            "audio/pcm",
            None,
            &vec![0; MAX_SPEECH_PCM_BYTES + 1],
        ),
    ];
    for (index, wire) in cases.into_iter().enumerate() {
        let peer = Peer::new(Some(wire));
        let result = peer
            .adapter()
            .synthesize(text(), CancelSignal::new(), deadline())
            .await;
        assert_eq!(
            result.unwrap_err(),
            if index < 2 {
                SpeechError::Failed
            } else {
                SpeechError::InvalidResponse
            }
        );
        peer.assert_one_call().await;
    }
    let peer = Peer::new(Some(response(
        "200 OK",
        " APPLICATION/OCTET-STREAM ",
        Some("identity"),
        b"xx",
    )));
    assert_eq!(
        peer.adapter()
            .synthesize(text(), CancelSignal::new(), deadline())
            .await
            .unwrap()
            .duration_ms(),
        1
    );
}

#[tokio::test]
async fn chunked_actual_body_cap_and_redirect_target_are_not_followed() {
    let body = vec![0_u8; MAX_SPEECH_PCM_BYTES + 2];
    let mut wire = format!("HTTP/1.1 200 OK\r\nContent-Type: audio/pcm\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n", body.len()).into_bytes();
    wire.extend_from_slice(&body);
    wire.extend_from_slice(b"\r\n0\r\n\r\n");
    let peer = Peer::new(Some(wire));
    assert_eq!(
        peer.adapter()
            .synthesize(text(), CancelSignal::new(), deadline())
            .await
            .unwrap_err(),
        SpeechError::InvalidResponse
    );

    let mut target = Peer::new(Some(response("200 OK", "audio/pcm", None, b"xx")));
    let redirect = format!("HTTP/1.1 307 Temporary Redirect\r\nLocation: {}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", target.endpoint).into_bytes();
    let peer = Peer::new(Some(redirect));
    assert_eq!(
        peer.adapter()
            .synthesize(text(), CancelSignal::new(), deadline())
            .await
            .unwrap_err(),
        SpeechError::Failed
    );
    peer.assert_one_call().await;
    assert_eq!(target.calls.load(Ordering::SeqCst), 0);
    target.received.take();
}

#[tokio::test]
async fn precancel_and_expired_deadline_make_no_request() {
    let peer = Peer::new(None);
    let cancel = CancelSignal::new();
    cancel.cancel();
    assert_eq!(
        peer.adapter()
            .synthesize(text(), cancel, deadline())
            .await
            .unwrap_err(),
        SpeechError::Cancelled
    );
    assert_eq!(
        peer.adapter()
            .synthesize(text(), CancelSignal::new(), Instant::now())
            .await
            .unwrap_err(),
        SpeechError::TimedOut
    );
    assert_eq!(peer.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn cancellation_and_deadline_release_stalled_await() {
    let mut peer = Peer::new(None);
    let adapter = peer.adapter();
    let cancel = CancelSignal::new();
    let signal = cancel.clone();
    let operation =
        tokio::spawn(async move { adapter.synthesize(text(), cancel, deadline()).await });
    peer.received().await;
    signal.cancel();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), operation)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err(),
        SpeechError::Cancelled
    );

    let mut peer = Peer::new(None);
    let adapter = peer.adapter();
    let end = Instant::now() + Duration::from_millis(150);
    let operation =
        tokio::spawn(async move { adapter.synthesize(text(), CancelSignal::new(), end).await });
    peer.received().await;
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), operation)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err(),
        SpeechError::TimedOut
    );
}

#[tokio::test]
async fn retained_request_bytes_hold_physical_permit_after_operation_drop() {
    let leases = pool();
    let first = leased_json(vec![1, 2], Arc::clone(&leases)).unwrap();
    let clone = first.clone();
    let body = reqwest::Body::from(first.clone());
    let second = leased_json(vec![3, 4], Arc::clone(&leases)).unwrap();
    assert!(matches!(
        leased_json(vec![5], Arc::clone(&leases)),
        Err(SpeechError::Busy)
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
