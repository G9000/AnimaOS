use super::*;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use anima_service_adapters::{
    ImageInput, ImagePrompt, ImageUsage, IMAGE_BACKGROUND, IMAGE_COUNT, IMAGE_MODEL,
    IMAGE_OUTPUT_DIMENSION, IMAGE_OUTPUT_FORMAT, IMAGE_QUALITY, IMAGE_SIZE, MAX_IMAGE_OUTPUT_BYTES,
};
use base64::Engine;
use tokio::sync::oneshot;

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(3)
}

fn prompt() -> ImagePrompt {
    ImagePrompt::new("  Draw 🌍 exactly.\n".into()).unwrap()
}

fn request() -> ImageRequest {
    ImageRequest::Generate { prompt: prompt() }
}

fn pool() -> Arc<Semaphore> {
    Arc::new(Semaphore::new(OUTGOING_JSON_BODIES))
}

fn png(dimension: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, dimension, dimension);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer
            .write_image_data(&vec![0; dimension as usize * dimension as usize * 4])
            .unwrap();
    }
    bytes
}

fn output_body(dimension: u32) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({"data": [{"b64_json": base64::engine::general_purpose::STANDARD.encode(png(dimension))}]})).unwrap()
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
        let endpoint = format!("http://{}/v1/images", listener.local_addr().unwrap());
        let (sender, received) = oneshot::channel();
        let calls = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let worker_calls = Arc::clone(&calls);
        let worker_stop = Arc::clone(&stop);
        let worker = std::thread::spawn(move || {
            let limit = Instant::now() + Duration::from_secs(4);
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

    fn adapter(&self) -> OpenAiImageAdapter {
        OpenAiImageAdapter::for_test(self.endpoint.clone(), pool()).unwrap()
    }

    async fn received(&mut self) -> Vec<u8> {
        tokio::time::timeout(Duration::from_secs(2), self.received.take().unwrap())
            .await
            .unwrap()
            .unwrap()
    }

    async fn assert_one_call(&self) {
        tokio::time::sleep(Duration::from_millis(40)).await;
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
    let mut chunk = [0_u8; 8192];
    while bytes.len() < 16 * 1024 * 1024 {
        match socket.read(&mut chunk) {
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Ok(0) | Err(_) => break,
            Ok(count) => bytes.extend_from_slice(&chunk[..count]),
        }
        if expected.is_none() {
            if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                assert!(end < 4096);
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
fn constructor_is_fixed_authority_and_debug_redacts_key() {
    assert!(OpenAiImageAdapter::new(String::new()).is_err());
    assert!(OpenAiImageAdapter::new("bad\nkey".into()).is_err());
    let first = OpenAiImageAdapter::new("private-key-marker".into()).unwrap();
    let second = OpenAiImageAdapter::new("second-key".into()).unwrap();
    assert_eq!(first.endpoint, ENDPOINT);
    assert!(Arc::ptr_eq(&first.leases, &second.leases));
    assert!(!format!("{first:?}").contains("private-key-marker"));
    for endpoint in [
        "http://example.com/v1/images",
        "http://127.0.0.1/v1/images",
        "http://127.0.0.1:1234/other",
        "http://127.0.0.1:1234/v1/images?x=1",
    ] {
        assert!(OpenAiImageAdapter::for_test(endpoint.into(), pool()).is_err());
    }
}

#[tokio::test]
async fn image_create_and_edit_use_exact_single_output_payloads() {
    for edit in [false, true] {
        let mut peer = Peer::new(Some(response(
            "200 OK",
            "application/json",
            None,
            &output_body(IMAGE_OUTPUT_DIMENSION),
        )));
        let input = png(1);
        let operation = if edit {
            ImageRequest::Edit {
                prompt: prompt(),
                image: ImageInput::new(input.clone()).unwrap(),
            }
        } else {
            request()
        };
        let image = peer
            .adapter()
            .generate(operation, CancelSignal::new(), deadline())
            .await
            .unwrap();
        assert_eq!(image.png_bytes(), png(IMAGE_OUTPUT_DIMENSION));
        assert_eq!(image.usage(), None);
        let wire = peer.received().await;
        let header_end = wire
            .windows(4)
            .position(|part| part == b"\r\n\r\n")
            .unwrap();
        let headers = String::from_utf8_lossy(&wire[..header_end]).to_ascii_lowercase();
        assert!(headers.starts_with(if edit {
            "post /v1/images/edits http/1.1\r\n"
        } else {
            "post /v1/images/generations http/1.1\r\n"
        }));
        assert!(headers.contains("authorization: bearer synthetic-test-key\r\n"));
        assert!(headers.contains("accept-encoding: identity\r\n"));
        let mut expected = serde_json::json!({"model": IMAGE_MODEL, "prompt": prompt().as_str(), "quality": IMAGE_QUALITY, "size": IMAGE_SIZE, "output_format": IMAGE_OUTPUT_FORMAT, "n": IMAGE_COUNT, "background": IMAGE_BACKGROUND});
        if edit {
            expected["images"] = serde_json::json!([{"image_url": format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(input))}]);
        }
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&wire[header_end + 4..]).unwrap(),
            expected
        );
        peer.assert_one_call().await;
    }
}

#[tokio::test]
async fn image_loopback_waits_for_complete_fragmented_request() {
    let mut peer = Peer::new(Some(response(
        "400 Bad Request",
        "application/json",
        None,
        b"{}",
    )));
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
    client.write_all(b"POST /v1/images/generations HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 4\r\nConnection: close\r\n\r\nAB").unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while peer.calls.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        matches!(
            peer.received.as_mut().unwrap().try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ),
        "loopback peer must wait for all declared body bytes before replying"
    );
    client.write_all(b"CD").unwrap();
    assert!(peer.received().await.ends_with(b"ABCD"));
    let mut reply = Vec::new();
    client.read_to_end(&mut reply).unwrap();
    assert!(reply.starts_with(b"HTTP/1.1 400 Bad Request\r\n"));
    peer.assert_one_call().await;
}

#[tokio::test]
async fn image_usage_is_optional_and_preserves_text_and_image_counts() {
    let mut body: serde_json::Value =
        serde_json::from_slice(&output_body(IMAGE_OUTPUT_DIMENSION)).unwrap();
    body["usage"] = serde_json::json!({"input_tokens": 7, "output_tokens": 5, "total_tokens": 12, "input_tokens_details": {"text_tokens": 3, "image_tokens": 4}, "output_tokens_details": {"image_tokens": 5}});
    let peer = Peer::new(Some(response(
        "200 OK",
        "application/json; charset=utf-8",
        None,
        &serde_json::to_vec(&body).unwrap(),
    )));
    let image = peer
        .adapter()
        .generate(request(), CancelSignal::new(), deadline())
        .await
        .unwrap();
    assert_eq!(
        image.usage(),
        Some(&ImageUsage {
            input_tokens: Some(7),
            output_tokens: Some(5),
            total_tokens: Some(12),
            input_text_tokens: Some(3),
            input_image_tokens: Some(4),
            output_text_tokens: None,
            output_image_tokens: Some(5)
        })
    );
}

#[tokio::test]
async fn image_rejections_are_definitive_and_never_retried_or_disclosed() {
    for status in [
        "400 Bad Request",
        "401 Unauthorized",
        "429 Too Many Requests",
    ] {
        let peer = Peer::new(Some(response(
            status,
            "application/json",
            None,
            b"private upstream key image prompt marker",
        )));
        let error = peer
            .adapter()
            .generate(request(), CancelSignal::new(), deadline())
            .await
            .unwrap_err();
        assert_eq!(error, ImageError::Rejected);
        assert!(!format!("{error:?} {error}").contains("private upstream"));
        peer.assert_one_call().await;
    }
}

#[tokio::test]
async fn image_invalid_or_lost_responses_are_unknown_without_retry() {
    let good = output_body(IMAGE_OUTPUT_DIMENSION);
    let mut multiple: serde_json::Value = serde_json::from_slice(&good).unwrap();
    let extra = multiple["data"][0].clone();
    multiple["data"].as_array_mut().unwrap().push(extra);
    let cases = [
        response("500 Internal Server Error", "application/json", None, b"private upstream"),
        response("200 OK", "text/plain", None, &good),
        response("200 OK", "application/json", Some("gzip"), &good),
        response("200 OK", "application/json", None, b"{bad"),
        response("200 OK", "application/json", None, b"{\"data\":[{\"b64_json\":\"!!!!\"}]}"),
        response("200 OK", "application/json", None, b"{\"data\":[{\"url\":\"https://example.com/remote.png\"}]}"),
        response("200 OK", "application/json", None, &output_body(1)),
        response("200 OK", "application/json", None, &serde_json::to_vec(&multiple).unwrap()),
        format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", MAX_IMAGE_RESPONSE_BYTES + 1).into_bytes(),
        b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 100\r\nConnection: close\r\n\r\n{".to_vec(),
    ];
    for wire in cases {
        let peer = Peer::new(Some(wire));
        assert_eq!(
            peer.adapter()
                .generate(request(), CancelSignal::new(), deadline())
                .await
                .unwrap_err(),
            ImageError::OutcomeUnknown
        );
        peer.assert_one_call().await;
    }
}

#[tokio::test]
async fn image_redirects_are_not_followed() {
    let target = Peer::new(Some(response(
        "200 OK",
        "application/json",
        None,
        &output_body(IMAGE_OUTPUT_DIMENSION),
    )));
    let peer = Peer::new(Some(format!("HTTP/1.1 307 Temporary Redirect\r\nLocation: {}/generations\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", target.endpoint).into_bytes()));
    assert_eq!(
        peer.adapter()
            .generate(request(), CancelSignal::new(), deadline())
            .await
            .unwrap_err(),
        ImageError::OutcomeUnknown
    );
    peer.assert_one_call().await;
    assert_eq!(target.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn image_chunked_response_is_capped_before_json_parsing() {
    let mut body: serde_json::Value =
        serde_json::from_slice(&output_body(IMAGE_OUTPUT_DIMENSION)).unwrap();
    body["padding"] = serde_json::Value::String("x".repeat(MAX_IMAGE_RESPONSE_BYTES));
    let body = serde_json::to_vec(&body).unwrap();
    let mut wire = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n", body.len()).into_bytes();
    wire.extend_from_slice(&body);
    wire.extend_from_slice(b"\r\n0\r\n\r\n");
    let peer = Peer::new(Some(wire));
    assert_eq!(
        peer.adapter()
            .generate(request(), CancelSignal::new(), deadline())
            .await
            .unwrap_err(),
        ImageError::OutcomeUnknown
    );
    peer.assert_one_call().await;
}

#[tokio::test]
async fn image_decoded_png_has_an_independent_byte_cap() {
    let mut oversized = Vec::new();
    {
        let mut encoder = png::Encoder::new(
            &mut oversized,
            IMAGE_OUTPUT_DIMENSION,
            IMAGE_OUTPUT_DIMENSION,
        );
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .add_text_chunk("bounded".into(), "x".repeat(MAX_IMAGE_OUTPUT_BYTES))
            .unwrap();
        let mut writer = encoder.write_header().unwrap();
        writer
            .write_image_data(&vec![
                0;
                IMAGE_OUTPUT_DIMENSION as usize
                    * IMAGE_OUTPUT_DIMENSION as usize
                    * 4
            ])
            .unwrap();
    }
    assert!(oversized.len() > MAX_IMAGE_OUTPUT_BYTES);
    let body = serde_json::to_vec(&serde_json::json!({"data": [{"b64_json": base64::engine::general_purpose::STANDARD.encode(oversized)}]})).unwrap();
    assert!(body.len() < MAX_IMAGE_RESPONSE_BYTES);
    let peer = Peer::new(Some(response("200 OK", "application/json", None, &body)));
    assert_eq!(
        peer.adapter()
            .generate(request(), CancelSignal::new(), deadline())
            .await
            .unwrap_err(),
        ImageError::OutcomeUnknown
    );
    peer.assert_one_call().await;
}

#[tokio::test]
async fn image_preflight_control_and_busy_make_no_request() {
    let peer = Peer::new(None);
    let cancel = CancelSignal::new();
    cancel.cancel();
    assert_eq!(
        peer.adapter()
            .generate(request(), cancel, deadline())
            .await
            .unwrap_err(),
        ImageError::NotSent
    );
    assert_eq!(
        peer.adapter()
            .generate(request(), CancelSignal::new(), Instant::now())
            .await
            .unwrap_err(),
        ImageError::NotSent
    );
    let leases = pool();
    let _permits = Arc::clone(&leases)
        .try_acquire_many_owned(OUTGOING_JSON_BODIES as u32)
        .unwrap();
    let adapter = OpenAiImageAdapter::for_test(peer.endpoint.clone(), leases).unwrap();
    assert_eq!(
        adapter
            .generate(request(), CancelSignal::new(), deadline())
            .await
            .unwrap_err(),
        ImageError::NotSent
    );
    assert_eq!(peer.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn image_postsend_cancellation_and_timeout_are_unknown() {
    let mut peer = Peer::new(None);
    let adapter = peer.adapter();
    let cancel = CancelSignal::new();
    let signal = cancel.clone();
    let operation =
        tokio::spawn(async move { adapter.generate(request(), cancel, deadline()).await });
    peer.received().await;
    signal.cancel();
    assert_eq!(
        operation.await.unwrap().unwrap_err(),
        ImageError::OutcomeUnknown
    );
    peer.assert_one_call().await;

    let mut peer = Peer::new(None);
    let adapter = peer.adapter();
    let operation = tokio::spawn(async move {
        adapter
            .generate(
                request(),
                CancelSignal::new(),
                Instant::now() + Duration::from_millis(150),
            )
            .await
    });
    peer.received().await;
    assert_eq!(
        operation.await.unwrap().unwrap_err(),
        ImageError::OutcomeUnknown
    );
    peer.assert_one_call().await;
}

#[test]
fn image_retained_transport_body_keeps_lease_until_all_clones_drop() {
    let leases = pool();
    let permit = Arc::clone(&leases).try_acquire_owned().unwrap();
    let bytes = leased_json(vec![1, 2, 3], permit);
    let clone = bytes.clone();
    let body = reqwest::Body::from(bytes);
    assert_eq!(leases.available_permits(), OUTGOING_JSON_BODIES - 1);
    drop(clone);
    assert_eq!(leases.available_permits(), OUTGOING_JSON_BODIES - 1);
    drop(body);
    assert_eq!(leases.available_permits(), OUTGOING_JSON_BODIES);
}
