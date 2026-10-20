use super::*;
use std::{
    io,
    pin::Pin,
    sync::Arc,
    task::{ready, Context, Poll},
};
use tokio::io::{AsyncBufRead, AsyncRead, AsyncWrite, BufReader, ReadBuf};
use tokio_rustls::{
    rustls::{self, pki_types::ServerName},
    TlsConnector,
};

pub(super) const MAX_LINE_BYTES: usize = 64 * 1024;
const MAX_CONNECTION_BYTES: usize = 8 * 1024 * 1024;
pub(super) trait EmailIo: AsyncRead + AsyncWrite + Unpin + Send + fmt::Debug {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send + fmt::Debug> EmailIo for T {}
pub(super) type BoxIo = Box<dyn EmailIo>;

pub(super) async fn tls(stream: BoxIo, host: &str) -> Result<BoxIo, EmailError> {
    let roots = rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|_| EmailError::Unavailable)?
    .with_root_certificates(roots)
    .with_no_client_auth();
    let name = ServerName::try_from(host.to_owned()).map_err(|_| EmailError::Invalid)?;
    let stream = TlsConnector::from(Arc::new(config))
        .connect(name, stream)
        .await
        .map_err(|_| EmailError::Unavailable)?;
    Ok(Box::new(stream))
}

/// Inspect complete IMAP framing lines before exposing literal lengths to the
/// upstream parser, which otherwise reserves memory from the claimed length.
pub(super) struct ImapWire<S> {
    reader: BufReader<S>,
    line: Vec<u8>,
    pending: Vec<u8>,
    pending_offset: usize,
    literal_remaining: usize,
    remaining: usize,
    fetch_nesting: Option<(usize, bool, bool)>,
}
impl<S: AsyncRead> ImapWire<S> {
    pub(super) fn new(inner: S) -> Self {
        Self {
            reader: BufReader::new(inner),
            line: Vec::new(),
            pending: Vec::new(),
            pending_offset: 0,
            literal_remaining: 0,
            remaining: MAX_CONNECTION_BYTES,
            fetch_nesting: None,
        }
    }
    pub(super) fn into_inner(self) -> Result<S, EmailError> {
        if !self.reader.buffer().is_empty()
            || !self.line.is_empty()
            || self.pending_offset < self.pending.len()
            || self.literal_remaining != 0
        {
            return Err(EmailError::Unavailable);
        }
        Ok(self.reader.into_inner())
    }
}
impl<S> fmt::Debug for ImapWire<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ImapWire([bounded])")
    }
}
fn limit_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Email protocol response exceeded limit",
    )
}

fn guard_fetch_nesting(
    mut line: &[u8],
    nesting: &mut Option<(usize, bool, bool)>,
) -> io::Result<()> {
    if nesting.is_none() {
        let upper = line.to_ascii_uppercase();
        let Some(start) = upper.windows(8).position(|bytes| bytes == b" FETCH (") else {
            return Ok(());
        };
        line = &line[start + 7..];
        *nesting = Some((0, false, false));
    }
    let (depth, quoted, escaped) = nesting.as_mut().expect("FETCH framing initialized");
    for byte in line {
        if *escaped {
            *escaped = false;
            continue;
        }
        if *quoted {
            if *byte == b'\\' {
                *escaped = true;
            } else if *byte == b'"' {
                *quoted = false;
            }
            continue;
        }
        match byte {
            b'"' => *quoted = true,
            b'(' => {
                *depth += 1;
                if *depth > 64 {
                    return Err(limit_error());
                }
            }
            b')' => {
                *depth = depth.saturating_sub(1);
                if *depth == 0 {
                    *nesting = None;
                    break;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

impl<S: AsyncRead + Unpin> AsyncRead for ImapWire<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        output: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if output.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        loop {
            if this.pending_offset < this.pending.len() {
                let size = output
                    .remaining()
                    .min(this.pending.len() - this.pending_offset);
                output.put_slice(&this.pending[this.pending_offset..this.pending_offset + size]);
                this.pending_offset += size;
                return Poll::Ready(Ok(()));
            }
            if this.remaining == 0 {
                return Poll::Ready(Err(limit_error()));
            }
            let data = ready!(Pin::new(&mut this.reader).poll_fill_buf(cx))?;
            if data.is_empty() {
                return if this.literal_remaining != 0 || !this.line.is_empty() {
                    Poll::Ready(Err(io::Error::from(io::ErrorKind::UnexpectedEof)))
                } else {
                    Poll::Ready(Ok(()))
                };
            }
            if this.literal_remaining > 0 {
                let size = this
                    .literal_remaining
                    .min(data.len())
                    .min(output.remaining())
                    .min(this.remaining);
                output.put_slice(&data[..size]);
                Pin::new(&mut this.reader).consume(size);
                this.literal_remaining -= size;
                this.remaining -= size;
                return Poll::Ready(Ok(()));
            }
            let end = data
                .iter()
                .position(|byte| *byte == b'\n')
                .map(|position| position + 1);
            let size = end.unwrap_or(data.len());
            if this.line.len() + size > MAX_LINE_BYTES || size > this.remaining {
                return Poll::Ready(Err(limit_error()));
            }
            this.line.extend_from_slice(&data[..size]);
            Pin::new(&mut this.reader).consume(size);
            this.remaining -= size;
            if end.is_none() {
                continue;
            }
            let line = this
                .line
                .strip_suffix(b"\r\n")
                .or_else(|| this.line.strip_suffix(b"\n"))
                .unwrap_or(&this.line);
            guard_fetch_nesting(line, &mut this.fetch_nesting)?;
            if let Some(prefix) = line.strip_suffix(b"}") {
                if let Some(open) = prefix.iter().rposition(|ch| *ch == b'{') {
                    let digits = prefix[open + 1..]
                        .strip_suffix(b"+")
                        .unwrap_or(&prefix[open + 1..]);
                    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
                        return Poll::Ready(Err(limit_error()));
                    }
                    let length = std::str::from_utf8(digits)
                        .ok()
                        .and_then(|value| value.parse::<usize>().ok());
                    match length {
                        Some(length) if length <= MAX_MESSAGE_BYTES && length <= this.remaining => {
                            this.literal_remaining = length
                        }
                        _ => return Poll::Ready(Err(limit_error())),
                    }
                }
            }
            this.pending = std::mem::take(&mut this.line);
            this.pending_offset = 0;
        }
    }
}
impl<S: AsyncRead + AsyncWrite + Unpin> AsyncWrite for ImapWire<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(self.reader.get_mut()).poll_write(cx, buf)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(self.reader.get_mut()).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(self.reader.get_mut()).poll_shutdown(cx)
    }
}
