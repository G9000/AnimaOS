//! Conservative automatic-mail admission. Sender headers are data, not authority.

pub(crate) fn canonical_sender(value: &str) -> Option<String> {
    if value.len() > 254 || value.is_empty() || !value.is_ascii() {
        return None;
    }
    let (local, domain) = value.split_once('@')?;
    if local.is_empty()
        || local.len() > 64
        || local.starts_with('.')
        || local.ends_with('.')
        || local.contains("..")
        || !local
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".!#$%&'*+-/=?^_`{|}~".contains(&b))
        || !domain.contains('.')
        || !domain.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
    {
        return None;
    }
    Some(value.to_ascii_lowercase())
}

pub(crate) fn authenticated_sender(sender: &str, authserv: &str, headers: &[String]) -> bool {
    let Some(sender) = canonical_sender(sender) else {
        return false;
    };
    if authserv.is_empty() || authserv.len() > 253 || headers.len() > 8 {
        return false;
    }
    let domain = sender.split_once('@').expect("validated mailbox").1;
    let mut trusted = Vec::new();
    for header in headers {
        let Some(header) = without_comments(header) else {
            return false;
        };
        let mut clauses = header.split(';');
        let server = clauses
            .next()
            .unwrap_or_default()
            .split_whitespace()
            .collect::<Vec<_>>();
        if server
            .first()
            .is_some_and(|s| s.eq_ignore_ascii_case(authserv))
        {
            if server.len() > 2 || server.get(1).is_some_and(|v| v.parse::<u32>().is_err()) {
                return false;
            }
            trusted.push(clauses.map(str::to_owned).collect::<Vec<_>>());
        }
    }
    // Ambiguous server stamps must never promote a forged or conflicting pass.
    if trusted.len() != 1 {
        return false;
    }
    let dmarc = trusted[0]
        .iter()
        .filter(|c| c.trim_start().to_ascii_lowercase().starts_with("dmarc="))
        .collect::<Vec<_>>();
    if dmarc.len() != 1 {
        return false;
    }
    let mut tokens = dmarc[0].split_whitespace();
    if !tokens
        .next()
        .is_some_and(|s| s.eq_ignore_ascii_case("dmarc=pass"))
    {
        return false;
    }
    let from = tokens
        .filter_map(|token| {
            token
                .split_once('=')
                .filter(|(key, _)| key.eq_ignore_ascii_case("header.from"))
                .map(|(_, value)| value)
        })
        .collect::<Vec<_>>();
    from.len() == 1 && from[0].eq_ignore_ascii_case(domain)
}

fn without_comments(header: &str) -> Option<String> {
    if header.len() > 4096 || !header.is_ascii() {
        return None;
    }
    let mut output = String::new();
    let mut depth = 0u8;
    let mut escaped = false;
    for c in header.chars() {
        if c.is_control() && c != '\t' {
            return None;
        }
        if depth > 0 {
            if escaped {
                escaped = false;
                continue;
            }
            match c {
                '\\' => escaped = true,
                '(' => {
                    depth = depth.checked_add(1)?;
                    if depth > 8 {
                        return None;
                    }
                }
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        output.push(' ');
                    }
                }
                _ => {}
            }
        } else {
            match c {
                '(' => {
                    depth = 1;
                    output.push(' ');
                }
                ')' | '"' | '\\' => return None,
                _ => output.push(c),
            }
        }
    }
    (depth == 0).then_some(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn authenticated(sender: &str, headers: &[&str]) -> bool {
        authenticated_sender(
            sender,
            "mx.example.com",
            &headers.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        )
    }

    #[test]
    fn canonical_identity_is_one_plain_mailbox() {
        assert_eq!(
            canonical_sender("Alice@Example.com"),
            Some("alice@example.com".into())
        );
        for input in [
            "Alice <alice@example.com>",
            "a@example.com,b@example.com",
            "a@example.com\r\nBcc:b@example.com",
            "a@",
            "a@@example.com",
            " a@example.com",
            "a b@example.com",
        ] {
            assert!(canonical_sender(input).is_none(), "{input:?}");
        }
    }

    #[test]
    fn only_the_pinned_servers_aligned_dmarc_pass_admits_mail() {
        assert!(authenticated("alice@example.com", &["mx.example.com; dkim=pass header.d=example.com; dmarc=pass (p=NONE) header.from=example.com"]));
        assert!(authenticated(
            "alice@example.com",
            &["mx.example.com 1; dmarc=pass header.from=example.com"]
        ));
        for header in [
            "attacker.example; dmarc=pass header.from=example.com",
            "mx.example.com.evil; dmarc=pass header.from=example.com",
            "mx.example.com; dmarc=fail header.from=example.com",
            "mx.example.com; dmarc=pass header.from=evil.example",
            "mx.example.com; spf=pass smtp.mailfrom=example.com",
            "mx.example.com; dkim=pass header.d=example.com",
            "mx.example.com; dmarc=pass header.from=example.com.evil",
            "mx.example.com; dmarc=passfail header.from=example.com",
            "mx.example.com; dmarc=pass header.from=example.com header.from=evil.example",
            "mx.example.com; dmarc=fail (dmarc=pass header.from=example.com)",
            "mx.example.com; reason=\"; dmarc=pass header.from=example.com\"",
            "mx.example.com; dmarc=pass reason=\"header.from=example.com\"",
            "mx.example.com; dmarc=pass header.from=example.com\r\nBcc: attacker@example.com",
        ] {
            assert!(!authenticated("alice@example.com", &[header]), "{header:?}");
        }
    }

    #[test]
    fn missing_pins_and_conflicting_trusted_results_fail_closed() {
        assert!(!authenticated_sender(
            "alice@example.com",
            "",
            &["; dmarc=pass header.from=example.com".into()]
        ));
        assert!(!authenticated("alice@example.com", &[]));
        assert!(!authenticated(
            "alice@example.com",
            &[
                "mx.example.com; dmarc=pass header.from=example.com",
                "mx.example.com; dmarc=fail header.from=example.com"
            ]
        ));
    }
}
