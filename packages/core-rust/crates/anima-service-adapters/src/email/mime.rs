use super::*;
use mailparse::{DispositionType, MailAddr, MailHeaderMap, ParsedMail};

pub(super) fn placeholder(uid: u32, received_at: String) -> EmailMessage {
    EmailMessage {
        uid,
        from: String::new(),
        subject: String::new(),
        body: String::new(),
        received_at,
        message_id: None,
        automated: true,
        authentication_results: Vec::new(),
    }
}

pub(super) fn parse_message(uid: u32, raw: &[u8], received_at: String) -> EmailMessage {
    let rejected = placeholder(uid, received_at);
    if raw.len() > MAX_MESSAGE_BYTES {
        return rejected;
    }
    // Bound MIME recursion and fanout before mailparse constructs its recursive
    // tree. Every nested multipart needs a Content-Type; boundary markers also
    // bound a wide multipart with omitted per-part headers.
    let lower = raw.to_ascii_lowercase();
    if lower
        .windows(b"content-type:".len())
        .filter(|w| *w == b"content-type:")
        .count()
        > 32
        || raw.windows(3).filter(|w| *w == b"\n--").count() > 128
    {
        return rejected;
    }
    let header_end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .or_else(|| raw.windows(2).position(|w| w == b"\n\n"));
    if header_end.is_none_or(|length| length > 64 * 1024) {
        return rejected;
    }
    let Ok(mail) = mailparse::parse_mail(raw) else {
        return rejected;
    };
    let Some(body) = plain_body(&mail) else {
        return rejected;
    };
    let headers = &mail.headers;
    let froms = headers.get_all_headers("From");
    let from = if froms.len() == 1 && froms[0].get_value_raw().len() <= 2048 {
        mailparse::addrparse_header(froms[0])
            .ok()
            .and_then(|addresses| match &addresses[..] {
                [MailAddr::Single(value)] if validate_address(&value.addr).is_ok() => {
                    Some(value.addr.to_ascii_lowercase())
                }
                _ => None,
            })
            .unwrap_or_default()
    } else {
        String::new()
    };
    let authentication_results = headers.get_all_values("Authentication-Results");
    let oversized_auth = authentication_results.len() > 8
        || authentication_results
            .iter()
            .any(|value| value.len() > 4096);
    let automated = from.is_empty() || oversized_auth || automatic_sender(&from, headers);
    let message_ids = headers.get_all_values("Message-ID");
    EmailMessage {
        uid,
        from,
        subject: bounded_text(headers.get_first_value("Subject").unwrap_or_default(), 2048),
        body: bounded_text(body, MAX_TEXT_BYTES),
        received_at: if rejected.received_at.is_empty() {
            bounded_text(headers.get_first_value("Date").unwrap_or_default(), 128)
        } else {
            rejected.received_at
        },
        message_id: if message_ids.len() == 1 {
            message_ids
                .into_iter()
                .next()
                .filter(|id| valid_message_id(id))
        } else {
            None
        },
        automated,
        authentication_results: authentication_results
            .into_iter()
            .filter(|value| value.len() <= 4096)
            .take(8)
            .collect(),
    }
}

fn plain_body(mail: &ParsedMail<'_>) -> Option<String> {
    if mail.get_content_disposition().disposition == DispositionType::Attachment
        || mail
            .get_content_disposition()
            .params
            .contains_key("filename")
        || mail.ctype.params.contains_key("name")
    {
        return None;
    }
    if mail.ctype.mimetype.eq_ignore_ascii_case("text/plain") && mail.subparts.is_empty() {
        return mail.get_body().ok();
    }
    if !mail
        .ctype
        .mimetype
        .to_ascii_lowercase()
        .starts_with("multipart/")
    {
        return None;
    }
    mail.subparts.iter().find_map(plain_body)
}

fn automatic_sender(from: &str, headers: &[mailparse::MailHeader<'_>]) -> bool {
    const PATTERNS: &[&str] = &[
        "noreply",
        "no-reply",
        "no_reply",
        "donotreply",
        "do-not-reply",
        "mailer-daemon",
        "postmaster",
        "bounce",
        "notifications@",
        "automated@",
        "auto-confirm",
        "auto-reply",
        "automailer",
    ];
    PATTERNS.iter().any(|pattern| from.contains(pattern))
        || headers.get_all_values("Auto-Submitted").iter().any(|v| {
            !v.split(';')
                .next()
                .unwrap_or_default()
                .trim()
                .eq_ignore_ascii_case("no")
        })
        || ["List-Id", "List-Unsubscribe", "X-Auto-Response-Suppress"]
            .iter()
            .any(|name| !headers.get_all_headers(name).is_empty())
        || headers
            .get_all_values("Precedence")
            .iter()
            .any(|v| ["bulk", "list", "junk"].contains(&v.trim().to_ascii_lowercase().as_str()))
        || headers
            .get_all_values("Return-Path")
            .iter()
            .any(|v| v.trim() == "<>")
}

pub(super) fn valid_message_id(value: &str) -> bool {
    value.len() <= 998
        && value.len() >= 5
        && value.starts_with('<')
        && value.ends_with('>')
        && value[1..value.len() - 1]
            .bytes()
            .all(|ch| ch.is_ascii_graphic() && ch != b'<' && ch != b'>')
        && value[1..value.len() - 1].split('@').count() == 2
        && value[1..value.len() - 1]
            .split('@')
            .all(|part| !part.is_empty())
}

fn bounded_text(mut text: String, limit: usize) -> String {
    if text.len() > limit {
        let mut end = limit;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    text.retain(|ch| !ch.is_control() || ch == '\n' || ch == '\r' || ch == '\t');
    text
}
