use super::*;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Answers {
    answers: Mutex<VecDeque<io::Result<Vec<SocketAddr>>>>,
    names: Mutex<Vec<String>>,
}

impl Answers {
    fn new(answers: Vec<io::Result<Vec<SocketAddr>>>) -> Arc<Self> {
        Arc::new(Self {
            answers: Mutex::new(answers.into()),
            names: Mutex::new(Vec::new()),
        })
    }
}

impl Resolve for Answers {
    fn resolve(&self, name: Name) -> Resolving {
        self.names.lock().unwrap().push(name.as_str().into());
        let answer = self.answers.lock().unwrap().pop_front().unwrap();
        Box::pin(async move { Ok(Box::new(answer?.into_iter()) as Addrs) })
    }
}

fn addresses(values: &[&str]) -> io::Result<Vec<SocketAddr>> {
    Ok(values
        .iter()
        .map(|value| SocketAddr::new(value.parse().unwrap(), 0))
        .collect())
}

#[test]
fn ssrf_hardening_checks_initial_and_redirect_url_literals() {
    for url in [
        "file:///etc/passwd",
        "http://127.0.0.1/",
        "http://2130706433/",
        "http://0x7f000001/",
        "http://[::ffff:127.0.0.1]/",
        "http://metadata.google.internal/",
        "http://metadata.goog/",
        "http://localhost./",
        "http://service.internal./",
        "http://printer/",
        "https://user:secret@example.com/",
    ] {
        assert!(reject_url(&Url::parse(url).unwrap()).is_err(), "{url}");
    }
    for url in [
        "https://example.com/path",
        "https://example.com./path",
        "https://8.8.8.8/",
        "https://[2606:4700:4700::1111]/",
    ] {
        assert!(reject_url(&Url::parse(url).unwrap()).is_ok(), "{url}");
    }
}

#[tokio::test]
async fn ssrf_hardening_resolver_refuses_private_mixed_empty_failed_and_scoped_answers() {
    let scoped = SocketAddr::V6(std::net::SocketAddrV6::new(
        "2606:4700:4700::1111".parse().unwrap(),
        0,
        0,
        1,
    ));
    for answer in [
        addresses(&["127.0.0.1"]),
        addresses(&["10.0.0.1"]),
        addresses(&["169.254.169.254"]),
        addresses(&["8.8.8.8", "192.168.1.1"]),
        addresses(&["::ffff:10.0.0.1"]),
        addresses(&["64:ff9b::a00:1"]),
        Ok(vec![scoped]),
        Ok(Vec::new()),
        Ok(vec!["8.8.8.8:0".parse().unwrap(); MAX_DNS_ADDRESSES + 1]),
        Err(io::Error::other("fixture DNS failure")),
    ] {
        let resolver = PublicResolver {
            inner: Answers::new(vec![answer]),
        };
        assert!(resolver
            .resolve("fetch.example.com".parse().unwrap())
            .await
            .is_err());
    }
}

#[tokio::test]
async fn ssrf_hardening_returns_exact_checked_answers_and_rechecks_rebinding() {
    let expected = addresses(&["93.184.216.34", "2606:4700:4700::1111"]).unwrap();
    let answers = Answers::new(vec![Ok(expected.clone()), addresses(&["127.0.0.1"])]);
    let resolver = PublicResolver {
        inner: answers.clone(),
    };
    let pinned: Vec<_> = resolver
        .resolve("fetch.example.com".parse().unwrap())
        .await
        .unwrap()
        .collect();
    assert_eq!(pinned, expected, "the connector gets the checked addresses");
    assert!(resolver
        .resolve("fetch.example.com".parse().unwrap())
        .await
        .is_err());
    assert_eq!(
        *answers.names.lock().unwrap(),
        ["fetch.example.com", "fetch.example.com"],
        "one lookup per connection; no validation/dial second lookup"
    );
}

#[tokio::test]
async fn ssrf_hardening_reqwest_never_connects_to_private_dns_answer() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let resolver = PublicResolver {
        inner: Answers::new(vec![Ok(vec![address])]),
    };
    let client = client_builder()
        .dns_resolver(Arc::new(resolver))
        .timeout(Duration::from_secs(1))
        .build()
        .unwrap();
    assert!(client
        .get(format!("http://fetch.example.com:{}/", address.port()))
        .send()
        .await
        .is_err());
    assert!(
        tokio::time::timeout(Duration::from_millis(30), listener.accept())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn ssrf_hardening_redirects_reject_private_literals_and_dns_without_changing_host() {
    for literal in [true, false] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let target = if literal {
            format!("http://{address}/private")
        } else {
            format!("http://redirect.example.com:{}/private", address.port())
        };
        let initial_received = Arc::new(AtomicBool::new(false));
        let received = initial_received.clone();
        let server = tokio::spawn(async move {
            let (mut socket, _) = tokio::time::timeout(Duration::from_secs(3), listener.accept())
                .await
                .expect("initial fixture connection must be bounded")
                .unwrap();
            let request = tokio::time::timeout(Duration::from_secs(3), async {
                let mut bytes = Vec::new();
                while !bytes.ends_with(b"\r\n\r\n") {
                    let mut part = [0; 512];
                    let length = socket.read(&mut part).await.unwrap();
                    assert_ne!(length, 0, "fixture request ended before its headers");
                    bytes.extend_from_slice(&part[..length]);
                    assert!(bytes.len() <= 4096, "fixture request exceeded its bound");
                }
                String::from_utf8(bytes).unwrap()
            })
            .await
            .expect("fixture request must be bounded");
            received.store(true, Ordering::Release);
            tokio::time::timeout(
                Duration::from_secs(3),
                socket.write_all(
                    format!("HTTP/1.1 302 Found\r\nLocation: {target}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes(),
                ),
            )
                .await
                .expect("fixture response must be bounded")
                .unwrap();
            assert!(
                tokio::time::timeout(Duration::from_millis(100), listener.accept())
                    .await
                    .is_err(),
                "a refused redirect must never open a connection"
            );
            request
        });
        let client = client_builder()
            // Only this test fixture's initial hop is locally routed. The
            // production builder never installs DNS overrides or exceptions.
            .resolve("initial.example.com", address)
            .dns_resolver(Arc::new(PublicResolver {
                inner: Answers::new(vec![Ok(vec![address])]),
            }))
            .timeout(Duration::from_secs(1))
            .build()
            .unwrap();
        let response = client
            .get(format!("http://initial.example.com:{}/", address.port()))
            .send()
            .await;
        if !initial_received.load(Ordering::Acquire) {
            server.abort();
            let _ = tokio::time::timeout(Duration::from_secs(1), server).await;
            panic!(
                "initial owned loopback fixture was not reached; redirect blocking was not exercised: {:?}",
                response.as_ref().err()
            );
        }
        assert!(response.is_err(), "private redirect must fail");
        let request = tokio::time::timeout(Duration::from_secs(5), server)
            .await
            .expect("redirect fixture must retire")
            .unwrap()
            .to_ascii_lowercase();
        assert!(request.contains(&format!("host: initial.example.com:{}", address.port())));
    }
}
