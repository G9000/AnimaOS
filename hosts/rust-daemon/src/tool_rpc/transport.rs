//! Private one-frame loopback bridge; never a second tool registry.
use std::{
    net::{Ipv4Addr, SocketAddr},
    time::Duration,
};

use async_trait::async_trait;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinHandle,
    time::Instant,
};
use tokio_util::sync::CancellationToken;

use super::BrokerReply;

#[async_trait]
pub(super) trait Dispatch: Send {
    async fn dispatch(&mut self, bytes: &[u8]) -> BrokerReply;
}

pub(super) struct Transport {
    pub(super) endpoint: SocketAddr,
    closed: CancellationToken,
    owner: Option<JoinHandle<Result<(), ()>>>,
}
impl Transport {
    pub(super) async fn bind(dispatcher: Box<dyn Dispatch>, deadline: Instant) -> Result<Self, ()> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(|_| ())?;
        let endpoint = listener.local_addr().map_err(|_| ())?;
        let closed = CancellationToken::new();
        let owner = tokio::spawn(serve(listener, dispatcher, deadline, closed.clone()));
        Ok(Self {
            endpoint,
            closed,
            owner: Some(owner),
        })
    }
    pub(super) fn close(&self) {
        self.closed.cancel();
    }
    #[cfg(test)]
    pub(super) fn retirement_joined(&self) -> bool {
        self.owner.is_none()
    }
    pub(super) async fn retire(&mut self) -> Result<(), ()> {
        let result = match self.owner.as_mut() {
            Some(owner) => owner.await.map_err(|_| ())?,
            None => return Ok(()),
        };
        self.owner.take();
        result
    }
}
impl Drop for Transport {
    fn drop(&mut self) {
        self.close();
    }
}

async fn read_frame(stream: &mut TcpStream) -> Result<Vec<u8>, ()> {
    let length = stream.read_u32().await.map_err(|_| ())? as usize;
    if length == 0 || length > 65_536 {
        return Err(());
    }
    let mut bytes = vec![0; length];
    stream.read_exact(&mut bytes).await.map_err(|_| ())?;
    let mut extra = [0];
    if stream.read(&mut extra).await.map_err(|_| ())? != 0 {
        return Err(());
    }
    Ok(bytes)
}

async fn serve(
    listener: TcpListener,
    mut dispatcher: Box<dyn Dispatch>,
    deadline: Instant,
    closed: CancellationToken,
) -> Result<(), ()> {
    for _ in 0..128 {
        let accepted = tokio::select! { biased;
            _ = closed.cancelled() => return Ok(()),
            _ = tokio::time::sleep_until(deadline) => return Ok(()),
            accepted = listener.accept() => accepted,
        };
        let (mut stream, address) = accepted.map_err(|_| ())?;
        if !address.ip().is_loopback() {
            continue;
        }
        let read = tokio::select! { biased;
            _ = closed.cancelled() => return Ok(()),
            _ = tokio::time::sleep_until(deadline) => return Ok(()),
            value = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut stream)) => value,
        };
        let Ok(Ok(bytes)) = read else {
            continue;
        };
        // Sequence is transport correlation only. The native broker remains
        // responsible for exact envelope, credentials, epoch, scope and grants.
        let sequence = serde_json::from_slice::<serde_json::Value>(&bytes)
            .ok()
            .and_then(|value| value.get("request")?.get("sequence")?.as_u64())
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or(0);
        // Never select/cancel this future: an admitted native outcome/save must
        // settle even if close/deadline/socket loss occurs while it is running.
        let reply = dispatcher.dispatch(&bytes).await;
        let frame = response_frame(sequence, reply)?;
        tokio::select! { biased;
            _ = closed.cancelled() => return Ok(()),
            _ = tokio::time::sleep_until(deadline) => return Ok(()),
            _ = tokio::time::timeout(Duration::from_secs(5), async {
                stream.write_all(&frame).await?;
                stream.shutdown().await
            }) => {},
        }
    }
    Ok(())
}

pub(super) fn response_frame(sequence: u32, mut reply: BrokerReply) -> Result<Vec<u8>, ()> {
    fn encode(sequence: u32, reply: &BrokerReply) -> Result<Vec<u8>, ()> {
        serde_json::to_vec(&serde_json::json!({"version":1,"sequence":sequence,"reply":reply}))
            .map_err(|_| ())
    }
    let mut body = encode(sequence, &reply)?;
    if body.len() > 65_536 {
        let suffix = "\n[Native result text omitted to fit transport frame; no tool was rerun.]";
        // Each UTF-8 source byte costs at most six serialized JSON bytes.
        let mut end = reply.text.len().min(10_000);
        while !reply.text.is_char_boundary(end) {
            end -= 1;
        }
        reply.text.truncate(end);
        reply.text.push_str(suffix);
        body = encode(sequence, &reply)?;
    }
    if body.len() > 65_536 {
        return Err(());
    }
    let mut frame = Vec::with_capacity(body.len() + 4);
    frame.extend_from_slice(&(body.len() as u32).to_be_bytes());
    frame.extend(body);
    Ok(frame)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool_rpc::BrokerDisposition;
    use std::sync::Arc;
    use std::{
        sync::atomic::{AtomicUsize, Ordering},
        time::Duration,
    };
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpStream,
        sync::Semaphore,
    };

    struct Peer {
        calls: Arc<AtomicUsize>,
        entered: CancellationToken,
        release: Option<Arc<Semaphore>>,
        settled: CancellationToken,
    }
    #[async_trait]
    impl Dispatch for Peer {
        async fn dispatch(&mut self, _bytes: &[u8]) -> BrokerReply {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.entered.cancel();
            if let Some(gate) = &self.release {
                gate.acquire().await.unwrap().forget();
            }
            self.settled.cancel();
            BrokerReply {
                disposition: BrokerDisposition::Success,
                text: "native result".into(),
            }
        }
    }
    async fn peer(
        release: Option<Arc<Semaphore>>,
    ) -> (
        Transport,
        Arc<AtomicUsize>,
        CancellationToken,
        CancellationToken,
    ) {
        let calls = Arc::new(AtomicUsize::new(0));
        let entered = CancellationToken::new();
        let settled = CancellationToken::new();
        let transport = Transport::bind(
            Box::new(Peer {
                calls: calls.clone(),
                entered: entered.clone(),
                release,
                settled: settled.clone(),
            }),
            Instant::now() + Duration::from_secs(5),
        )
        .await
        .expect("owned IPv4 loopback listener");
        (transport, calls, entered, settled)
    }
    async fn send(endpoint: SocketAddr, body: &[u8], trailing: &[u8]) -> TcpStream {
        let mut stream = TcpStream::connect(endpoint).await.unwrap();
        stream.write_u32(body.len() as u32).await.unwrap();
        stream.write_all(body).await.unwrap();
        stream.write_all(trailing).await.unwrap();
        stream.shutdown().await.unwrap();
        stream
    }
    const FRAME: &[u8] = br#"{"token":"private","request":{"version":1,"cellEpoch":"epoch","sequence":1,"tool":"read_file","args":{}}}"#;

    #[tokio::test]
    async fn fp06_transport_exact_frame_then_eof_dispatches_once_and_retires() {
        let (mut transport, calls, _, _) = peer(None).await;
        assert!(transport.endpoint.is_ipv4());
        assert!(transport.endpoint.ip().is_loopback());
        let mut stream = send(transport.endpoint, FRAME, b"").await;
        let length = stream.read_u32().await.unwrap();
        let mut body = vec![0; length as usize];
        stream.read_exact(&mut body).await.unwrap();
        let response: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(response["sequence"], 1);
        assert_eq!(response["reply"]["disposition"], "success");
        assert_eq!(
            stream.read_u8().await.unwrap_err().kind(),
            std::io::ErrorKind::UnexpectedEof
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        transport.close();
        transport.retire().await.unwrap();
    }

    #[tokio::test]
    async fn fp06_transport_partial_oversize_and_trailing_frames_never_dispatch() {
        let (mut transport, calls, _, _) = peer(None).await;
        let mut trailing = send(transport.endpoint, FRAME, b"extra").await;
        let mut discard = Vec::new();
        let _ = trailing.read_to_end(&mut discard).await;
        let mut oversized = TcpStream::connect(transport.endpoint).await.unwrap();
        oversized.write_u32(65_537).await.unwrap();
        oversized.shutdown().await.unwrap();
        let _ = oversized.read_to_end(&mut Vec::new()).await;
        let mut partial = TcpStream::connect(transport.endpoint).await.unwrap();
        partial.write_all(&[0, 0]).await.unwrap();
        partial.shutdown().await.unwrap();
        let _ = partial.read_to_end(&mut Vec::new()).await;
        transport.close();
        transport.retire().await.unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn fp06_transport_disconnect_and_close_preserve_admitted_native_future() {
        let release = Arc::new(Semaphore::new(0));
        let (mut transport, calls, entered, settled) = peer(Some(release.clone())).await;
        let stream = send(transport.endpoint, FRAME, b"").await;
        tokio::time::timeout(Duration::from_secs(1), entered.cancelled())
            .await
            .unwrap();
        drop(stream);
        transport.close();
        let early = tokio::time::timeout(Duration::from_millis(30), transport.retire()).await;
        let unsettled = !settled.is_cancelled();
        release.add_permits(1);
        transport.retire().await.unwrap();
        assert!(
            early.is_err(),
            "retirement cannot discard native settlement"
        );
        assert!(unsettled);
        assert!(settled.is_cancelled());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn fp06_transport_response_budget_counts_json_escaping_and_preserves_disposition() {
        let bytes = response_frame(
            7,
            BrokerReply {
                disposition: BrokerDisposition::ReviewRequired,
                text: "\0".repeat(65_000),
            },
        )
        .expect("bounded projection");
        assert!(bytes.len() <= 65_536 + 4);
        assert_eq!(
            u32::from_be_bytes(bytes[..4].try_into().unwrap()) as usize,
            bytes.len() - 4
        );
        let body: serde_json::Value = serde_json::from_slice(&bytes[4..]).unwrap();
        assert_eq!(body["reply"]["disposition"], "review_required");
        assert!(body["reply"]["text"].as_str().unwrap().contains("omitted"));
    }
}
