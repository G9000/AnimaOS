use super::*;

fn endpoint() -> EmailEndpoint {
    EmailEndpoint {
        host: "mail.example.org".into(),
        port: 993,
        security: EmailSecurity::Tls,
        username: "agent@example.org".into(),
    }
}
pub(super) fn account() -> EmailAccount {
    EmailAccount {
        address: "agent@example.org".into(),
        imap: endpoint(),
        smtp: endpoint(),
    }
}
#[test]
fn email_rejects_endpoint_injection_and_multiple_account_mailboxes() {
    let mut value = account();
    assert!(value.validate().is_ok());
    for host in [
        "",
        "mail.example.org/secret",
        "mail.example.org\r\nLOGIN",
        "https://mail.example.org",
        "bad host",
    ] {
        value.imap.host = host.into();
        assert_eq!(value.validate(), Err(EmailError::Invalid));
    }
    value = account();
    value.address = "one@example.org,other@example.org".into();
    assert_eq!(value.validate(), Err(EmailError::Invalid));
}
#[test]
fn email_secrets_debug_is_redacted() {
    let value = EmailSecrets::new("imap-sensitive", "smtp-sensitive");
    assert_eq!(value.imap_password(), "imap-sensitive");
    assert_eq!(value.smtp_password(), "smtp-sensitive");
    assert!(!format!("{value:?}").contains("sensitive"));
}
#[test]
fn email_decodes_plain_mime_and_excludes_attachments_and_html() {
    let raw = b"From: Alice <alice@example.org>\r\nSubject: =?UTF-8?B?SGVsbG8gw6k=?=\r\nMessage-ID: <original@example.org>\r\nContent-Type: multipart/mixed; boundary=x\r\n\r\n--x\r\nContent-Type: text/html\r\n\r\n<script>evil()</script>\r\n--x\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: base64\r\n\r\nSGVsbG8gZnJvbSBhbGljZQ==\r\n--x\r\nContent-Type: text/plain\r\nContent-Disposition: attachment\r\n\r\nSECRET ATTACHMENT\r\n--x--\r\n";
    let value = mime::parse_message(7, raw, "today".into());
    assert_eq!(value.uid, 7);
    assert_eq!(value.from, "alice@example.org");
    assert_eq!(value.subject, "Hello é");
    assert_eq!(value.body, "Hello from alice");
    assert_eq!(value.message_id.as_deref(), Some("<original@example.org>"));
    assert!(!value.automated);
}
#[test]
fn email_preserves_uid_but_rejects_ambiguous_and_malformed_from() {
    for from in [
        "Alice <alice@example.org>,other@example.org",
        "group: alice@example.org;",
        "Alice",
        "alice@example.org\r\nFrom: attacker@example.org",
    ] {
        let raw = format!("From: {from}\r\n\r\nHello");
        let value = mime::parse_message(8, raw.as_bytes(), String::new());
        assert_eq!(value.uid, 8);
        assert!(value.from.is_empty(), "{from}");
        assert!(value.automated);
    }
}
#[test]
fn email_filters_all_automatic_message_signals() {
    for header in [
        "Auto-Submitted: auto-replied",
        "List-Id: <list.example.org>",
        "Precedence: bulk",
        "Precedence: list",
        "Precedence: junk",
        "Return-Path: <>",
        "X-Auto-Response-Suppress: All",
        "List-Unsubscribe: <https://example.org/unsubscribe>",
    ] {
        let raw = format!("From: alice@example.org\r\n{header}\r\n\r\nHello");
        assert!(
            mime::parse_message(9, raw.as_bytes(), String::new()).automated,
            "{header}"
        );
    }
    for sender in [
        "no-reply@example.org",
        "mailer-daemon@example.org",
        "postmaster@example.org",
        "bounce+id@example.org",
    ] {
        let raw = format!("From: {sender}\r\n\r\nHello");
        assert!(mime::parse_message(9, raw.as_bytes(), String::new()).automated);
    }
}
#[test]
fn email_bounds_raw_decoded_text_and_authentication_headers() {
    let raw = format!(
        "From: alice@example.org\r\n{}\r\n{}",
        "Authentication-Results: mx.example.org; dmarc=pass\r\n".repeat(9),
        "a".repeat(MAX_TEXT_BYTES * 2)
    );
    let value = mime::parse_message(10, raw.as_bytes(), String::new());
    assert_eq!(value.body.len(), MAX_TEXT_BYTES);
    assert_eq!(value.authentication_results.len(), 8);
    let value = mime::parse_message(11, &vec![b'a'; MAX_MESSAGE_BYTES + 1], String::new());
    assert_eq!(value.uid, 11);
    assert!(value.automated && value.from.is_empty() && value.body.is_empty());
}

pub(super) fn outgoing() -> EmailOutgoing {
    EmailOutgoing {
        id: "draft-123".into(),
        to: vec!["alice@example.org".into()],
        subject: "Re: Hello".into(),
        body: "Reply text".into(),
        in_reply_to: Some("<original@example.org>".into()),
        automatic: true,
    }
}
#[test]
fn email_outgoing_headers_are_safe_and_stable() {
    let value = client::build_message(&account(), &outgoing()).unwrap();
    let raw = String::from_utf8(value.formatted()).unwrap();
    assert!(raw.contains("Auto-Submitted: auto-replied\r\n"));
    assert!(raw.contains("In-Reply-To: <original@example.org>\r\n"));
    assert!(raw.contains("References: <original@example.org>\r\n"));
    let message_id = raw
        .lines()
        .find(|line| line.starts_with("Message-ID:"))
        .unwrap();
    let again = String::from_utf8(
        client::build_message(&account(), &outgoing())
            .unwrap()
            .formatted(),
    )
    .unwrap();
    assert!(again.contains(message_id));
    let mut injection = outgoing();
    injection.in_reply_to = Some("<original@example.org>\r\nBcc: attacker@example.org".into());
    assert!(matches!(
        client::build_message(&account(), &injection),
        Err(EmailError::Invalid)
    ));
    injection = outgoing();
    injection.subject = "Re: Hello\nBcc: attacker@example.org".into();
    assert!(matches!(
        client::build_message(&account(), &injection),
        Err(EmailError::Invalid)
    ));
}

#[tokio::test]
async fn email_imap_rejects_oversized_literal_before_parser_allocation() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let (client, mut server) = tokio::io::duplex(4096);
    server
        .write_all(b"* 1 FETCH (BODY[] {500000000}\r\n")
        .await
        .unwrap();
    server.shutdown().await.unwrap();
    let mut wire = wire::ImapWire::new(client);
    let mut bytes = Vec::new();
    assert!(wire.read_to_end(&mut bytes).await.is_err());
    assert!(
        bytes.is_empty(),
        "oversized literal length must never reach the IMAP parser"
    );
}

async fn fixture_inbox(
    after: Option<EmailCursor>,
    metadata_only: bool,
) -> Result<EmailInbox, EmailError> {
    fixture_inbox_size(after, metadata_only, 28).await
}
async fn fixture_inbox_size(
    after: Option<EmailCursor>,
    metadata_only: bool,
    exists: u32,
) -> Result<EmailInbox, EmailError> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let (stream, server) = tokio::io::duplex(65536);
    let task = tokio::spawn(async move {
        let mut server = BufReader::new(server);
        server
            .get_mut()
            .write_all(b"* OK local fixture\r\n")
            .await
            .unwrap();
        loop {
            let mut line = String::new();
            if server.read_line(&mut line).await.unwrap() == 0 {
                break;
            }
            let tag = line.split_whitespace().next().unwrap();
            let mut response = String::new();
            if line.contains("EXAMINE") {
                let next = if exists == 28 { 100 } else { exists * 10 + 1 };
                response.push_str(&format!("* {exists} EXISTS\r\n* OK [UIDVALIDITY 7] identity\r\n* OK [UIDNEXT {next}] next\r\n"));
            } else if line.contains("FETCH") && !line.contains("BODY") {
                // Deliberately unordered: the adapter must retain the smallest
                // next page, not assume server emission order.
                assert!(
                    !line.contains("UID FETCH"),
                    "unbounded UID discovery is prohibited"
                );
                let range = line.split_whitespace().nth(2).unwrap();
                let (first, last) = range
                    .split_once(':')
                    .map(|(first, last)| {
                        (first.parse::<u32>().unwrap(), last.parse::<u32>().unwrap())
                    })
                    .unwrap_or_else(|| {
                        let value = range.parse::<u32>().unwrap();
                        (value, value)
                    });
                assert!(last - first < MAX_INBOX_MESSAGES as u32);
                for sequence in (first..=last).rev() {
                    let uid = if exists == 28 {
                        sequence + 20
                    } else {
                        sequence * 10
                    };
                    response.push_str(&format!("* {sequence} FETCH (UID {uid})\r\n"));
                }
            } else if line.contains("BODY") {
                let ids = line.split_whitespace().nth(3).unwrap();
                for id in ids.split(',') {
                    let uid: u32 = id.parse().unwrap();
                    let raw = if uid == 21 {
                        "broken MIME"
                    } else {
                        "From: alice@example.org\r\n\r\nHello"
                    };
                    let size = if uid == 22 {
                        MAX_MESSAGE_BYTES + 1
                    } else {
                        raw.len()
                    };
                    let sequence = if exists == 28 { uid - 20 } else { uid / 10 };
                    response.push_str(&format!("* {sequence} FETCH (UID {uid} RFC822.SIZE {size} BODY[] {{{}}}\r\n{raw})\r\n", raw.len()));
                }
            }
            response.push_str(&format!("{tag} OK completed\r\n"));
            if server
                .get_mut()
                .write_all(response.as_bytes())
                .await
                .is_err()
            {
                break;
            }
        }
    });
    let mut session = imap::Session::new(stream);
    session.greet().await?;
    session.login("fixture", "imap-sensitive-sentinel").await?;
    let result = client::read_inbox(&mut session, after, metadata_only).await;
    drop(session);
    task.await.unwrap();
    result
}

#[tokio::test]
async fn email_trace_logging_never_receives_credentials_or_provider_content() {
    struct Capture(std::sync::Mutex<Vec<String>>);
    impl log::Log for Capture {
        fn enabled(&self, _: &log::Metadata<'_>) -> bool {
            true
        }
        fn log(&self, record: &log::Record<'_>) {
            self.0.lock().unwrap().push(record.args().to_string());
        }
        fn flush(&self) {}
    }
    static LOGGER: Capture = Capture(std::sync::Mutex::new(Vec::new()));
    log::set_logger(&LOGGER).unwrap();
    let previous = log::max_level();
    log::set_max_level(log::LevelFilter::Trace);
    fixture_inbox(None, false).await.unwrap();
    log::set_max_level(previous);
    let logs = LOGGER.0.lock().unwrap().join("\n");
    assert!(
        !logs.contains("imap-sensitive-sentinel"),
        "IMAP dependencies must not log secrets"
    );
    assert!(
        !logs.contains("From: alice@example.org"),
        "IMAP dependencies must not log provider message bodies"
    );
}

#[tokio::test]
async fn email_imap_cursor_page_is_sorted_bounded_and_keeps_rejected_uids() {
    let inbox = fixture_inbox(
        Some(EmailCursor {
            uid_validity: 7,
            last_uid: 20,
        }),
        false,
    )
    .await
    .unwrap();
    assert_eq!(inbox.uid_validity, 7);
    assert_eq!(inbox.uid_next, 100);
    assert_eq!(
        inbox.messages.iter().map(|m| m.uid).collect::<Vec<_>>(),
        (21..=45).collect::<Vec<_>>()
    );
    assert!(inbox.messages[0].automated && inbox.messages[0].from.is_empty());
    assert!(inbox.messages[1].automated && inbox.messages[1].body.is_empty());
    assert_eq!(inbox.messages[2].body, "Hello");
}
#[tokio::test]
async fn email_imap_changed_cursor_is_rejected_before_fetch() {
    assert_eq!(
        fixture_inbox(
            Some(EmailCursor {
                uid_validity: 6,
                last_uid: 20
            }),
            false
        )
        .await,
        Err(EmailError::CursorChanged)
    );
    assert!(fixture_inbox(None, true).await.unwrap().messages.is_empty());
}

#[tokio::test]
async fn email_imap_latest_page_is_ascending_and_current_cursor_is_empty() {
    let inbox = fixture_inbox(None, false).await.unwrap();
    assert_eq!(
        inbox
            .messages
            .iter()
            .map(|message| message.uid)
            .collect::<Vec<_>>(),
        (24..=48).collect::<Vec<_>>()
    );
    assert!(fixture_inbox(
        Some(EmailCursor {
            uid_validity: 7,
            last_uid: 99
        }),
        false
    )
    .await
    .unwrap()
    .messages
    .is_empty());
}

#[test]
fn email_unsupported_and_deep_mime_remains_a_rejected_uid() {
    let html = b"From: alice@example.org\r\nContent-Type: text/html\r\n\r\n<script>do not execute</script>";
    let value = mime::parse_message(12, html, String::new());
    assert!(value.from.is_empty() && value.body.is_empty() && value.automated);
    let deep = format!(
        "From: alice@example.org\r\nContent-Type: multipart/mixed; boundary=x\r\n\r\n{}",
        "--x\r\nContent-Type: multipart/mixed; boundary=x\r\n\r\n".repeat(40)
    );
    let value = mime::parse_message(13, deep.as_bytes(), String::new());
    assert_eq!(value.uid, 13);
    assert!(value.from.is_empty() && value.automated);
}

#[tokio::test]
async fn email_imap_starttls_refusal_never_transmits_credentials() {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
    let (client, stream) = tokio::io::duplex(4096);
    let server = tokio::spawn(async move {
        let mut stream = BufReader::new(stream);
        stream
            .get_mut()
            .write_all(b"* OK local fixture\r\n")
            .await
            .unwrap();
        let mut first = String::new();
        stream.read_line(&mut first).await.unwrap();
        assert!(first.contains("STARTTLS"));
        let tag = first.split_whitespace().next().unwrap();
        stream
            .get_mut()
            .write_all(format!("{tag} NO upgrade refused\r\n").as_bytes())
            .await
            .unwrap();
        let mut rest = String::new();
        stream.read_to_string(&mut rest).await.unwrap();
        assert!(!rest.contains("LOGIN") && !rest.contains("sensitive"));
    });
    assert!(matches!(
        client::upgrade_imap(client).await,
        Err(EmailError::Unavailable)
    ));
    server.await.unwrap();
}

#[tokio::test]
async fn email_imap_large_sparse_backlog_advances_by_bounded_page() {
    let inbox = fixture_inbox_size(
        Some(EmailCursor {
            uid_validity: 7,
            last_uid: 5_000_000,
        }),
        false,
        1_000_000,
    )
    .await
    .unwrap();
    assert_eq!(inbox.messages.len(), 25);
    assert_eq!(inbox.messages.first().unwrap().uid, 5_000_010);
    assert_eq!(inbox.messages.last().unwrap().uid, 5_000_250);
}

#[test]
fn email_outgoing_accepts_existing_hundred_kilobyte_draft_limit() {
    let mut message = outgoing();
    message.body = "a".repeat(MAX_OUTGOING_TEXT_BYTES);
    let bytes = client::build_message(&account(), &message)
        .unwrap()
        .formatted();
    assert!(bytes.len() < MAX_MESSAGE_BYTES);
    message.body.push('a');
    assert!(matches!(
        client::build_message(&account(), &message),
        Err(EmailError::Invalid)
    ));
}

#[tokio::test]
async fn email_imap_rejects_deep_server_nesting_before_recursive_parser() {
    use tokio::io::AsyncReadExt;
    let text = format!(
        "* 1 FETCH (BODYSTRUCTURE {}{})\r\n",
        "(".repeat(2000),
        ")".repeat(2000)
    );
    let mut wire = wire::ImapWire::new(text.as_bytes());
    assert!(wire.read_to_end(&mut Vec::new()).await.is_err());
}

#[tokio::test]
async fn email_imap_expunge_invalidates_sequence_page() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let (client, mut server) = tokio::io::duplex(4096);
    let server = tokio::spawn(async move {
        let mut bytes = [0; 1024];
        server.read(&mut bytes).await.unwrap();
        server
            .write_all(b"* 1 EXPUNGE\r\nA000001 OK completed\r\n")
            .await
            .unwrap();
    });
    let mut session = imap::Session::new(client);
    assert_eq!(
        session.command("FETCH 1 (UID)", false, |_| Ok(())).await,
        Err(EmailError::Unavailable)
    );
    server.await.unwrap();
}
