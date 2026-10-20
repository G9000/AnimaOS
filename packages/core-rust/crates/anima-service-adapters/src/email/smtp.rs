use super::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader},
    net::TcpStream,
};
use wire::BoxIo;

pub(super) async fn verify(
    account: &EmailAccount,
    secrets: &EmailSecrets,
) -> Result<(), EmailError> {
    let mut stream = connect(&account.smtp, secrets.smtp_password()).await?;
    // Authentication plus NOOP proves the connection without submitting mail.
    expect(command(&mut stream, b"NOOP\r\n").await?, &[250])?;
    Ok(())
}
pub(super) async fn send(
    account: &EmailAccount,
    secrets: &EmailSecrets,
    message: &EmailOutgoing,
    bytes: &[u8],
) -> Result<(), EmailError> {
    let mut stream = connect(&account.smtp, secrets.smtp_password()).await?;
    submit(&mut stream, &account.address, &message.to, bytes).await
}

async fn connect(endpoint: &EmailEndpoint, password: &str) -> Result<BufReader<BoxIo>, EmailError> {
    let raw: BoxIo = Box::new(
        TcpStream::connect((endpoint.host.as_str(), endpoint.port))
            .await
            .map_err(|_| EmailError::Unavailable)?,
    );
    let mut stream = match endpoint.security {
        EmailSecurity::Tls => {
            let mut stream = BufReader::new(wire::tls(raw, &endpoint.host).await?);
            expect(read_response(&mut stream).await?, &[220])?;
            stream
        }
        EmailSecurity::Starttls => {
            let raw = upgrade_smtp(BufReader::new(raw)).await?;
            BufReader::new(wire::tls(raw, &endpoint.host).await?)
        }
    };
    // Reissue EHLO on the encrypted stream; pre-upgrade capability data is
    // discarded. Password-based mechanisms are only used inside verified TLS.
    let capabilities = hello(&mut stream).await?;
    authenticate(&mut stream, endpoint, password, &capabilities).await?;
    Ok(stream)
}

async fn upgrade_smtp<S: AsyncRead + AsyncWrite + Unpin>(
    mut plain: BufReader<S>,
) -> Result<S, EmailError> {
    expect(read_response(&mut plain).await?, &[220])?;
    let capabilities = hello(&mut plain).await?;
    if !capabilities.iter().any(|line| {
        line.split_whitespace()
            .next()
            .is_some_and(|word| word.eq_ignore_ascii_case("STARTTLS"))
    }) {
        return Err(EmailError::Unavailable);
    }
    expect(command(&mut plain, b"STARTTLS\r\n").await?, &[220])?;
    if !plain.buffer().is_empty() {
        return Err(EmailError::Unavailable);
    }
    Ok(plain.into_inner())
}

async fn hello<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut BufReader<S>,
) -> Result<Vec<String>, EmailError> {
    expect(command(stream, b"EHLO anima.local\r\n").await?, &[250])
}

async fn authenticate<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut BufReader<S>,
    endpoint: &EmailEndpoint,
    password: &str,
    capabilities: &[String],
) -> Result<(), EmailError> {
    let mechanisms: Vec<_> = capabilities
        .iter()
        .filter_map(|line| {
            let upper = line.to_ascii_uppercase();
            upper
                .strip_prefix("AUTH ")
                .or_else(|| upper.strip_prefix("AUTH="))
                .map(str::to_owned)
        })
        .collect();
    if mechanisms
        .iter()
        .any(|line| line.split_whitespace().any(|word| word == "PLAIN"))
    {
        let raw = Zeroizing::new(format!("\0{}\0{password}", endpoint.username));
        let encoded = Zeroizing::new(STANDARD.encode(raw.as_bytes()));
        let login = Zeroizing::new(format!("AUTH PLAIN {}\r\n", encoded.as_str()));
        let response = command(stream, login.as_bytes()).await?;
        if response.0 == 334 {
            let login = Zeroizing::new(format!("{}\r\n", encoded.as_str()));
            expect(command(stream, login.as_bytes()).await?, &[235])?;
        } else {
            expect(response, &[235])?;
        }
    } else if mechanisms
        .iter()
        .any(|line| line.split_whitespace().any(|word| word == "LOGIN"))
    {
        expect(command(stream, b"AUTH LOGIN\r\n").await?, &[334])?;
        let username = Zeroizing::new(format!(
            "{}\r\n",
            STANDARD.encode(endpoint.username.as_bytes())
        ));
        expect(command(stream, username.as_bytes()).await?, &[334])?;
        let mut password = Zeroizing::new(STANDARD.encode(password.as_bytes()));
        password.push_str("\r\n");
        expect(command(stream, password.as_bytes()).await?, &[235])?;
    } else {
        return Err(EmailError::Unauthorized);
    }
    Ok(())
}

fn expect(response: (u16, Vec<String>), codes: &[u16]) -> Result<Vec<String>, EmailError> {
    if codes.contains(&response.0) {
        return Ok(response.1);
    }
    Err(if [530, 534, 535, 538].contains(&response.0) {
        EmailError::Unauthorized
    } else {
        EmailError::Unavailable
    })
}

async fn command<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut BufReader<S>,
    bytes: &[u8],
) -> Result<(u16, Vec<String>), EmailError> {
    stream
        .get_mut()
        .write_all(bytes)
        .await
        .map_err(|_| EmailError::Unavailable)?;
    stream
        .get_mut()
        .flush()
        .await
        .map_err(|_| EmailError::Unavailable)?;
    read_response(stream).await
}

async fn read_response<R: AsyncBufRead + Unpin>(
    reader: &mut R,
) -> Result<(u16, Vec<String>), EmailError> {
    let mut lines = Vec::new();
    let mut code = None;
    let mut total = 0;
    loop {
        if lines.len() >= 64 {
            return Err(EmailError::Unavailable);
        }
        let mut line = Vec::new();
        loop {
            let data = reader
                .fill_buf()
                .await
                .map_err(|_| EmailError::Unavailable)?;
            if data.is_empty() {
                return Err(EmailError::Unavailable);
            }
            let end = data
                .iter()
                .position(|ch| *ch == b'\n')
                .map(|position| position + 1);
            let size = end.unwrap_or(data.len());
            if line.len() + size > 2048 || total + size > 64 * 1024 {
                return Err(EmailError::Unavailable);
            }
            line.extend_from_slice(&data[..size]);
            reader.consume(size);
            total += size;
            if end.is_some() {
                break;
            }
        }
        if line.len() < 6
            || !line.ends_with(b"\r\n")
            || !line[..3].iter().all(u8::is_ascii_digit)
            || !matches!(line[3], b'-' | b' ')
        {
            return Err(EmailError::Unavailable);
        }
        let current: u16 = std::str::from_utf8(&line[..3])
            .ok()
            .and_then(|digits| digits.parse().ok())
            .ok_or(EmailError::Unavailable)?;
        if code.is_some_and(|value| value != current) {
            return Err(EmailError::Unavailable);
        }
        code = Some(current);
        let text = String::from_utf8(line[4..line.len() - 2].to_vec())
            .map_err(|_| EmailError::Unavailable)?;
        lines.push(text);
        if line[3] == b' ' {
            return Ok((current, lines));
        }
    }
}

async fn submit<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut BufReader<S>,
    from: &str,
    to: &[String],
    bytes: &[u8],
) -> Result<(), EmailError> {
    expect(
        command(stream, format!("MAIL FROM:<{from}>\r\n").as_bytes()).await?,
        &[250],
    )?;
    for recipient in to {
        expect(
            command(stream, format!("RCPT TO:<{recipient}>\r\n").as_bytes()).await?,
            &[250, 251, 252],
        )?;
    }
    expect(command(stream, b"DATA\r\n").await?, &[354])?;
    let mut escaped = Vec::with_capacity(bytes.len() + 16);
    let mut line_start = true;
    for byte in bytes {
        if line_start && *byte == b'.' {
            escaped.push(b'.');
        }
        escaped.push(*byte);
        line_start = *byte == b'\n';
    }
    if !escaped.ends_with(b"\r\n") {
        escaped.extend_from_slice(b"\r\n");
    }
    escaped.extend_from_slice(b".\r\n");
    // All failures once DATA transmission starts are uncertain. This session
    // makes one attempt, and never reconnects or resubmits the message.
    stream
        .get_mut()
        .write_all(&escaped)
        .await
        .map_err(|_| EmailError::Unknown)?;
    stream
        .get_mut()
        .flush()
        .await
        .map_err(|_| EmailError::Unknown)?;
    let response = read_response(stream)
        .await
        .map_err(|_| EmailError::Unknown)?;
    if response.0 != 250 {
        return Err(EmailError::Unknown);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn email_smtp_bounds_response_lines_before_allocation() {
        let bytes = format!("250 {}", "x".repeat(8192));
        assert_eq!(
            read_response(&mut BufReader::new(bytes.as_bytes())).await,
            Err(EmailError::Unavailable)
        );
    }

    async fn fixture_submit(unknown: bool) -> (Result<(), EmailError>, Vec<String>) {
        let (stream, server) = tokio::io::duplex(4096);
        let task = tokio::spawn(async move {
            let mut server = BufReader::new(server);
            let mut transcript = Vec::new();
            let mut data = false;
            loop {
                let mut line = String::new();
                if server.read_line(&mut line).await.unwrap() == 0 {
                    break;
                }
                transcript.push(line.clone());
                let response = if data {
                    if line == ".\r\n" {
                        if unknown {
                            break;
                        }
                        data = false;
                        "250 accepted\r\n"
                    } else {
                        continue;
                    }
                } else if line == "DATA\r\n" {
                    data = true;
                    "354 continue\r\n"
                } else {
                    "250 OK\r\n"
                };
                if server
                    .get_mut()
                    .write_all(response.as_bytes())
                    .await
                    .is_err()
                {
                    break;
                }
            }
            transcript
        });
        let mut stream = BufReader::new(stream);
        let result = submit(
            &mut stream,
            "agent@example.org",
            &["alice@example.org".into()],
            b"Subject: Test\r\n\r\n.line\r\n",
        )
        .await;
        drop(stream);
        (result, task.await.unwrap())
    }

    #[tokio::test]
    async fn email_smtp_unknown_send_is_not_retried() {
        let (result, transcript) = fixture_submit(true).await;
        assert_eq!(result, Err(EmailError::Unknown));
        assert_eq!(
            transcript.iter().filter(|line| *line == "DATA\r\n").count(),
            1
        );
    }
    #[tokio::test]
    async fn email_smtp_escapes_data_and_accepts_only_final_acknowledgement() {
        let (result, transcript) = fixture_submit(false).await;
        assert_eq!(result, Ok(()));
        assert!(transcript.contains(&"..line\r\n".into()));
        assert!(transcript.contains(&"RCPT TO:<alice@example.org>\r\n".into()));
    }

    #[tokio::test]
    async fn email_smtp_starttls_unavailable_never_sends_auth() {
        use tokio::io::AsyncReadExt;
        let (client, stream) = tokio::io::duplex(4096);
        let server = tokio::spawn(async move {
            let mut stream = BufReader::new(stream);
            stream
                .get_mut()
                .write_all(b"220 local fixture\r\n")
                .await
                .unwrap();
            let mut first = String::new();
            stream.read_line(&mut first).await.unwrap();
            assert_eq!(first, "EHLO anima.local\r\n");
            stream
                .get_mut()
                .write_all(b"250-fixture\r\n250 AUTH PLAIN LOGIN\r\n")
                .await
                .unwrap();
            let mut rest = String::new();
            stream.read_to_string(&mut rest).await.unwrap();
            assert!(rest.is_empty());
        });
        assert!(matches!(
            upgrade_smtp(BufReader::new(client)).await,
            Err(EmailError::Unavailable)
        ));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn email_smtp_auth_rejection_is_redacted_and_sends_no_mail() {
        let (client, server) = tokio::io::duplex(4096);
        let server = tokio::spawn(async move {
            let mut server = BufReader::new(server);
            let mut line = String::new();
            server.read_line(&mut line).await.unwrap();
            assert!(line.starts_with("AUTH PLAIN "));
            server
                .get_mut()
                .write_all(b"535 sensitive provider diagnostic\r\n")
                .await
                .unwrap();
        });
        let mut client = BufReader::new(client);
        let result = authenticate(
            &mut client,
            &super::super::tests::account().smtp,
            "sensitive",
            &["AUTH PLAIN LOGIN".into()],
        )
        .await;
        assert_eq!(result, Err(EmailError::Unauthorized));
        assert!(!format!("{result:?}").contains("sensitive"));
        server.await.unwrap();
    }
}
