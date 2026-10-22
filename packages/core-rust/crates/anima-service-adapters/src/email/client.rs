use super::*;
use imap_proto::{AttributeValue, MailboxDatum, Response, ResponseCode};
use lettre::{
    message::header::{ContentTransferEncoding, ContentType, HeaderName, HeaderValue},
    Message,
};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};
use tokio::net::TcpStream;
use wire::BoxIo;

pub(super) const OPERATION_TIMEOUT: Duration = Duration::from_secs(45);

#[derive(Clone, Debug, Default)]
pub struct EmailClient;
impl EmailClient {
    pub fn new() -> Self {
        Self
    }
}

pub(super) fn validate_inputs(
    account: &EmailAccount,
    secrets: &EmailSecrets,
) -> Result<(), EmailError> {
    account.validate()?;
    for password in [secrets.imap_password(), secrets.smtp_password()] {
        if password.is_empty()
            || password.len() > 4096
            || password.chars().any(|ch| matches!(ch, '\0' | '\r' | '\n'))
        {
            return Err(EmailError::Invalid);
        }
    }
    Ok(())
}

#[async_trait]
impl EmailTransport for EmailClient {
    async fn verify(
        &self,
        account: &EmailAccount,
        secrets: &EmailSecrets,
    ) -> Result<EmailInbox, EmailError> {
        validate_inputs(account, secrets)?;
        tokio::time::timeout(OPERATION_TIMEOUT, async {
            let mut session = imap_login(account, secrets).await?;
            let inbox = read_inbox(&mut session, None, true).await?;
            drop(session);
            super::smtp::verify(account, secrets).await?;
            Ok(inbox)
        })
        .await
        .map_err(|_| EmailError::Unavailable)?
    }
    async fn inbox(
        &self,
        account: &EmailAccount,
        secrets: &EmailSecrets,
        after: Option<EmailCursor>,
    ) -> Result<EmailInbox, EmailError> {
        validate_inputs(account, secrets)?;
        tokio::time::timeout(OPERATION_TIMEOUT, async {
            let mut session = imap_login(account, secrets).await?;
            read_inbox(&mut session, after, false).await
        })
        .await
        .map_err(|_| EmailError::Unavailable)?
    }
    async fn send(
        &self,
        account: &EmailAccount,
        secrets: &EmailSecrets,
        message: &EmailOutgoing,
    ) -> Result<(), EmailError> {
        validate_inputs(account, secrets)?;
        let bytes = build_message(account, message)?.formatted();
        if bytes.len() > MAX_MESSAGE_BYTES {
            return Err(EmailError::Invalid);
        }
        // A dropped future can occur after DATA was accepted. Never invite an
        // automatic retry when the operation's acceptance point is uncertain.
        tokio::time::timeout(
            OPERATION_TIMEOUT,
            super::smtp::send(account, secrets, message, &bytes),
        )
        .await
        .map_err(|_| EmailError::Unknown)?
    }
}

async fn imap_login(
    account: &EmailAccount,
    secrets: &EmailSecrets,
) -> Result<imap::Session<BoxIo>, EmailError> {
    let endpoint = &account.imap;
    let stream: BoxIo = Box::new(
        TcpStream::connect((endpoint.host.as_str(), endpoint.port))
            .await
            .map_err(|_| EmailError::Unavailable)?,
    );
    let mut session = match endpoint.security {
        EmailSecurity::Tls => {
            let mut session = imap::Session::new(wire::tls(stream, &endpoint.host).await?);
            session.greet().await?;
            session
        }
        EmailSecurity::Starttls => {
            let stream = upgrade_imap(stream).await?;
            imap::Session::new(wire::tls(stream, &endpoint.host).await?)
        }
    };
    session
        .login(&endpoint.username, secrets.imap_password())
        .await?;
    Ok(session)
}

pub(super) async fn upgrade_imap<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin>(
    stream: S,
) -> Result<S, EmailError> {
    let mut session = imap::Session::new(stream);
    session.greet().await?;
    session.command("STARTTLS", false, |_| Ok(())).await?;
    session.into_inner()
}

pub(super) fn build_message(
    account: &EmailAccount,
    message: &EmailOutgoing,
) -> Result<Message, EmailError> {
    account.validate()?;
    if message.id.is_empty()
        || message.id.len() > 512
        || message.to.is_empty()
        || message.to.len() > 20
        || message.subject.len() > 2048
        || message.subject.chars().any(char::is_control)
        || message.body.len() > MAX_OUTGOING_TEXT_BYTES
        || message.body.contains('\0')
        || message
            .in_reply_to
            .as_deref()
            .is_some_and(|id| !mime::valid_message_id(id))
    {
        return Err(EmailError::Invalid);
    }
    let domain = account
        .address
        .rsplit_once('@')
        .ok_or(EmailError::Invalid)?
        .1;
    let id = format!(
        "<anima-{}@{domain}>",
        hex::encode(Sha256::digest(message.id.as_bytes()))
    );
    let mut builder = Message::builder()
        .from(account.address.parse().map_err(|_| EmailError::Invalid)?)
        .subject(&message.subject)
        .message_id(Some(id))
        .header(ContentType::TEXT_PLAIN)
        .header(ContentTransferEncoding::Base64);
    for recipient in &message.to {
        validate_address(recipient)?;
        builder = builder.to(recipient.parse().map_err(|_| EmailError::Invalid)?);
    }
    if let Some(id) = &message.in_reply_to {
        builder = builder.in_reply_to(id.clone()).references(id.clone());
    }
    if message.automatic {
        builder = builder.raw_header(HeaderValue::new(
            HeaderName::new_from_ascii_str("Auto-Submitted"),
            "auto-replied".into(),
        ));
    }
    builder
        .body(message.body.clone())
        .map_err(|_| EmailError::Invalid)
}

pub(super) async fn read_inbox<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin>(
    session: &mut imap::Session<S>,
    after: Option<EmailCursor>,
    metadata_only: bool,
) -> Result<EmailInbox, EmailError> {
    let (mut uid_validity, mut uid_next, mut exists) = (0, 0, 0);
    session
        .command("EXAMINE INBOX", false, |response| {
            match response {
                Response::MailboxData(MailboxDatum::Exists(value)) => exists = value,
                Response::Data { outcome, .. } | Response::Done { outcome, .. } => {
                    match outcome.code {
                        Some(ResponseCode::UidValidity(value)) => uid_validity = value,
                        Some(ResponseCode::UidNext(value)) => uid_next = value,
                        _ => {}
                    }
                }
                _ => {}
            }
            Ok(())
        })
        .await?;
    if uid_validity == 0 || uid_next == 0 {
        return Err(EmailError::Unavailable);
    }
    if after.is_some_and(|cursor| cursor.uid_validity != uid_validity) {
        return Err(EmailError::CursorChanged);
    }
    let mut inbox = EmailInbox {
        uid_validity,
        uid_next,
        messages: Vec::new(),
    };
    if metadata_only
        || exists == 0
        || after.is_some_and(|cursor| cursor.last_uid >= uid_next.saturating_sub(1))
    {
        return Ok(inbox);
    }

    let first = if let Some(cursor) = after {
        // UIDs increase with sequence numbers. Find the first new UID with at
        // most 32 singleton requests even in a huge, sparse mailbox. Fetching
        // every UID since the cursor would permanently stall a large backlog.
        let (mut low, mut high) = (1u64, u64::from(exists) + 1);
        while low < high {
            let mid = low + (high - low) / 2;
            let mut found = None;
            session
                .command(&format!("FETCH {mid} (UID)"), false, |response| {
                    if let Response::Fetch(sequence, attributes) = response {
                        if u64::from(sequence) == mid {
                            found = uid(&attributes);
                        }
                    }
                    Ok(())
                })
                .await?;
            let found = found.ok_or(EmailError::Unavailable)?;
            if found <= cursor.last_uid {
                low = mid + 1;
            } else {
                high = mid;
            }
        }
        if low > u64::from(exists) {
            return Ok(inbox);
        }
        low as u32
    } else {
        exists.saturating_sub(MAX_INBOX_MESSAGES as u32 - 1).max(1)
    };
    let last = first
        .saturating_add(MAX_INBOX_MESSAGES as u32 - 1)
        .min(exists);
    let mut ids = BTreeSet::new();
    session
        .command(&format!("FETCH {first}:{last} (UID)"), false, |response| {
            if let Response::Fetch(sequence, attributes) = response {
                if sequence >= first && sequence <= last {
                    let id = uid(&attributes).ok_or(EmailError::Unavailable)?;
                    if id > 0 && id < uid_next && after.is_none_or(|cursor| id > cursor.last_uid) {
                        ids.insert(id);
                    }
                    if ids.len() > MAX_INBOX_MESSAGES {
                        return Err(EmailError::Unavailable);
                    }
                }
            }
            Ok(())
        })
        .await?;
    if ids.is_empty() {
        return Ok(inbox);
    }
    let mut messages: BTreeMap<_, _> = ids
        .iter()
        .map(|uid| (*uid, mime::placeholder(*uid, String::new())))
        .collect();
    let set = ids.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
    session
        .command(
            &format!(
                "UID FETCH {set} (UID RFC822.SIZE INTERNALDATE BODY.PEEK[]<0.{MAX_MESSAGE_BYTES}>)"
            ),
            false,
            |response| {
                if let Response::Fetch(_, attributes) = response {
                    let id = uid(&attributes).ok_or(EmailError::Unavailable)?;
                    if let Some(message) = messages.get_mut(&id) {
                        let mut size = None;
                        let mut body = None;
                        let mut received_at = String::new();
                        for attribute in &attributes {
                            match attribute {
                                AttributeValue::Rfc822Size(value) => size = Some(*value),
                                AttributeValue::BodySection {
                                    section: None,
                                    data: Some(bytes),
                                    ..
                                } => body = Some(bytes.as_ref()),
                                AttributeValue::InternalDate(value) => {
                                    received_at = chrono::DateTime::parse_from_str(
                                        value,
                                        "%d-%b-%Y %H:%M:%S %z",
                                    )
                                    .map(|date| date.to_rfc3339())
                                    .unwrap_or_default()
                                }
                                _ => {}
                            }
                        }
                        *message = if size.is_some_and(|size| size as usize <= MAX_MESSAGE_BYTES) {
                            body.map(|raw| mime::parse_message(id, raw, received_at.clone()))
                                .unwrap_or_else(|| mime::placeholder(id, received_at))
                        } else {
                            mime::placeholder(id, received_at)
                        };
                    }
                }
                Ok(())
            },
        )
        .await?;
    inbox.messages = messages.into_values().collect();
    Ok(inbox)
}

fn uid(attributes: &[AttributeValue<'_>]) -> Option<u32> {
    attributes.iter().find_map(|attribute| match attribute {
        AttributeValue::Uid(value) => Some(*value),
        _ => None,
    })
}
