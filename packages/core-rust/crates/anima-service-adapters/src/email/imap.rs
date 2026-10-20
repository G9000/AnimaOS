//! Minimal tagged IMAP driver. No commands, server responses or credentials are
//! logged, including when process-wide TRACE logging is enabled.
use super::*;
use imap_proto::{Response, Status};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub(super) struct Session<S> {
    stream: wire::ImapWire<S>,
    buffer: Vec<u8>,
    next_tag: u32,
}
impl<S: AsyncRead + AsyncWrite + Unpin> Session<S> {
    pub(super) fn new(stream: S) -> Self {
        Self {
            stream: wire::ImapWire::new(stream),
            buffer: Vec::new(),
            next_tag: 1,
        }
    }
    pub(super) fn into_inner(self) -> Result<S, EmailError> {
        if !self.buffer.is_empty() {
            return Err(EmailError::Unavailable);
        }
        self.stream.into_inner()
    }
    pub(super) async fn greet(&mut self) -> Result<(), EmailError> {
        match self.read_response().await? {
            Response::Data {
                status: Status::Ok | Status::PreAuth,
                ..
            } => Ok(()),
            _ => Err(EmailError::Unavailable),
        }
    }
    pub(super) async fn login(&mut self, username: &str, password: &str) -> Result<(), EmailError> {
        let username = quote(username);
        let password = quote(password);
        let command = Zeroizing::new(format!("LOGIN {} {}", username.as_str(), password.as_str()));
        self.command(&command, true, |_| Ok(())).await
    }
    pub(super) async fn command(
        &mut self,
        command: &str,
        authentication: bool,
        mut response: impl FnMut(Response<'static>) -> Result<(), EmailError>,
    ) -> Result<(), EmailError> {
        let tag = format!("A{:06}", self.next_tag);
        self.next_tag = self
            .next_tag
            .checked_add(1)
            .ok_or(EmailError::Unavailable)?;
        self.stream
            .write_all(tag.as_bytes())
            .await
            .map_err(|_| EmailError::Unavailable)?;
        self.stream
            .write_all(b" ")
            .await
            .map_err(|_| EmailError::Unavailable)?;
        self.stream
            .write_all(command.as_bytes())
            .await
            .map_err(|_| EmailError::Unavailable)?;
        self.stream
            .write_all(b"\r\n")
            .await
            .map_err(|_| EmailError::Unavailable)?;
        self.stream
            .flush()
            .await
            .map_err(|_| EmailError::Unavailable)?;
        for _ in 0..128 {
            let item = self.read_response().await?;
            match item {
                Response::Done {
                    tag: ref actual,
                    ref status,
                    ..
                } => {
                    if actual.0 != tag {
                        return Err(EmailError::Unavailable);
                    }
                    if *status != Status::Ok {
                        return Err(if authentication {
                            EmailError::Unauthorized
                        } else {
                            EmailError::Unavailable
                        });
                    }
                    response(item)?;
                    return Ok(());
                }
                // Sequence-based binary search must be restarted if messages
                // are expunged during the selected mailbox snapshot.
                Response::Expunge(_)
                | Response::Vanished { .. }
                | Response::Data {
                    status: Status::Bye,
                    ..
                } => return Err(EmailError::Unavailable),
                _ => response(item)?,
            }
        }
        Err(EmailError::Unavailable)
    }
    async fn read_response(&mut self) -> Result<Response<'static>, EmailError> {
        loop {
            match imap_proto::parser::parse_response(&self.buffer) {
                Ok((remaining, item)) => {
                    let used = self.buffer.len() - remaining.len();
                    let item = item.into_owned();
                    self.buffer.drain(..used);
                    return Ok(item);
                }
                Err(nom::Err::Incomplete(_)) => {}
                Err(_) => return Err(EmailError::Unavailable),
            }
            let mut bytes = [0; 4096];
            let size = self
                .stream
                .read(&mut bytes)
                .await
                .map_err(|_| EmailError::Unavailable)?;
            if size == 0 || self.buffer.len() + size > MAX_MESSAGE_BYTES + wire::MAX_LINE_BYTES {
                return Err(EmailError::Unavailable);
            }
            self.buffer.extend_from_slice(&bytes[..size]);
        }
    }
}

fn quote(value: &str) -> Zeroizing<String> {
    let mut result = Zeroizing::new(String::with_capacity(value.len() + 2));
    result.push('"');
    for ch in value.chars() {
        if ch == '\\' || ch == '"' {
            result.push('\\');
        }
        result.push(ch);
    }
    result.push('"');
    result
}
