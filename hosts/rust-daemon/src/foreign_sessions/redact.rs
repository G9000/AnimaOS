//! Secret redaction for imported text: PEM private-key blocks, then every
//! shape the log redactor knows (`sk-…`, `sk-ant-…`, `ghp_…`, `xox…`,
//! `AKIA…`, bearer tokens, JWTs, `password=` / `token=` / `api_key=`
//! assignments, and long mixed-case tokens).

use std::sync::OnceLock;

use regex::Regex;

pub(crate) const REDACTED_PRIVATE_KEY: &str = "[redacted private key]";

fn private_key_block() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(
            r"(?s)-----BEGIN [A-Z0-9 ]*PRIVATE KEY(?: BLOCK)?-----.*?(?:-----END [A-Z0-9 ]*PRIVATE KEY(?: BLOCK)?-----|\z)",
        )
        .expect("private key pattern is valid")
    })
}

/// `text` with recognizable secrets masked. An unterminated private-key
/// block is masked through the end of the text.
pub(crate) fn redact_secrets(text: &str) -> String {
    let without_keys = private_key_block().replace_all(text, REDACTED_PRIVATE_KEY);
    crate::logs::redact::redact(&without_keys)
}
