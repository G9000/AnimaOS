//! Classifies model-call error strings so hosts can rotate keys and fall back.
//!
//! Adapters keep returning plain `String` errors across `ModelAdapter`, but they
//! format them predictably (`"{Provider} API error ({status}): {body}"`,
//! `"{Provider} request failed: …"`, an optional trailing `(retry after Ns)`), so
//! the class can be read back from the text.

/// Why a model call failed, as far as key rotation and fallback care.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelFailureKind {
    RateLimited,
    Overloaded,
    AuthFailed,
    Billing,
    Timeout,
    Transport,
    ModelNotFound,
    ContextTooLong,
    BadRequest,
    Other,
}

impl ModelFailureKind {
    /// Whether the key that made the call should rest and the next key be tried.
    pub fn rotates_key(self) -> bool {
        matches!(self, Self::RateLimited | Self::AuthFailed | Self::Billing)
    }

    /// Whether the call may move on to another model.
    pub fn falls_back(self) -> bool {
        !matches!(self, Self::ContextTooLong | Self::BadRequest | Self::Other)
    }
}

/// A classified model-call error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelFailure {
    pub kind: ModelFailureKind,
    /// The HTTP status, when the error text carries one.
    pub status: Option<u16>,
    /// The provider's `retry after` hint, in seconds.
    pub retry_after_s: Option<u64>,
}

const BILLING_BODIES: &[&str] = &[
    "insufficient_quota",
    "credit balance",
    "billing",
    "out of extra usage",
    "key limit exceeded",
];

/// Classifies an adapter error string. Never fails: unknown text is `Other`.
pub fn classify_model_error(message: &str) -> ModelFailure {
    let status = parse_status(message);
    let retry_after_s = parse_retry_after(message);
    let text = message.to_ascii_lowercase();
    let has = |needle: &str| text.contains(needle);
    let kind = if status == Some(402) || BILLING_BODIES.iter().any(|body| has(body)) {
        ModelFailureKind::Billing
    } else if has("context_length_exceeded")
        || has("prompt is too long")
        || has("context limit exceeded")
    {
        ModelFailureKind::ContextTooLong
    } else if status == Some(404) || has("model_not_found") {
        ModelFailureKind::ModelNotFound
    } else if let Some(status) = status {
        match status {
            429 => ModelFailureKind::RateLimited,
            500 | 502 | 503 | 504 | 529 => ModelFailureKind::Overloaded,
            401 | 403 => ModelFailureKind::AuthFailed,
            400..=499 => ModelFailureKind::BadRequest,
            _ => ModelFailureKind::Other,
        }
    } else if let Some(kind) = stream_error_token(message).and_then(stream_token_kind) {
        kind
    } else if has("timed out") {
        ModelFailureKind::Timeout
    } else if has("request failed") || has("stream read failed") {
        ModelFailureKind::Transport
    } else if has("usage limit reached") || has("rate limit reached") {
        ModelFailureKind::RateLimited
    } else if has("authorization expired") || has("authorization was rejected") {
        ModelFailureKind::AuthFailed
    } else if has("temporarily unavailable") {
        ModelFailureKind::Overloaded
    } else {
        ModelFailureKind::Other
    };
    ModelFailure {
        kind,
        status,
        retry_after_s,
    }
}

/// The provider's enumerated error type in a mid-stream error:
/// `"{Provider} stream error: {type}"` or `"Google stream failed: {STATUS}: …"`.
fn stream_error_token(message: &str) -> Option<&str> {
    const MARKERS: &[&str] = &["stream error: ", "stream failed: "];
    let start = MARKERS
        .iter()
        .find_map(|marker| message.find(marker).map(|at| at + marker.len()))?;
    let rest = &message[start..];
    let end = rest
        .find(|c: char| c == ':' || c.is_whitespace())
        .unwrap_or(rest.len());
    Some(&rest[..end]).filter(|token| !token.is_empty())
}

/// Anthropic error types, Google RPC statuses and OpenAI-style error codes.
fn stream_token_kind(token: &str) -> Option<ModelFailureKind> {
    use ModelFailureKind::*;
    Some(match token.to_ascii_lowercase().as_str() {
        "overloaded_error" | "api_error" | "server_error" | "unavailable" | "internal" => {
            Overloaded
        }
        "rate_limit_error" | "rate_limit_exceeded" | "resource_exhausted" => RateLimited,
        "deadline_exceeded" | "timeout_error" => Timeout,
        "authentication_error" | "permission_error" | "permission_denied" | "unauthenticated" => {
            AuthFailed
        }
        "billing_error" => Billing,
        "invalid_request_error" | "invalid_argument" => BadRequest,
        _ => return None,
    })
}

/// The first `(NNN)` or `(HTTP NNN)` in the text.
fn parse_status(message: &str) -> Option<u16> {
    let mut rest = message;
    while let Some(open) = rest.find('(') {
        rest = &rest[open + 1..];
        let inner = rest.strip_prefix("HTTP ").unwrap_or(rest);
        let digits = inner
            .get(..3)
            .filter(|d| d.bytes().all(|b| b.is_ascii_digit()));
        if let Some(digits) = digits {
            if inner[3..].starts_with(')') {
                return digits.parse().ok();
            }
        }
    }
    None
}

/// The seconds in the last `retry after Ns`.
fn parse_retry_after(message: &str) -> Option<u64> {
    const MARKER: &str = "retry after ";
    let start = message.rfind(MARKER)? + MARKER.len();
    let rest = &message[start..];
    let end = rest.bytes().take_while(u8::is_ascii_digit).count();
    if end == 0 || !rest[end..].starts_with('s') {
        return None;
    }
    rest[..end].parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_adapter_error_formats() {
        use ModelFailureKind::*;
        let cases: &[(&str, ModelFailureKind)] = &[
            (
                "Anthropic API error (429): rate_limit_error (retry after 30s)",
                RateLimited,
            ),
            ("OpenAI API error (402): insufficient_quota", Billing),
            ("OpenAI API error (403): key limit exceeded", Billing),
            ("Google API error (401): API key not valid", AuthFailed),
            ("OpenAI API error (403): forbidden", AuthFailed),
            (
                "After one retry: Anthropic API error (529): overloaded",
                Overloaded,
            ),
            ("OpenAI API error (503): unavailable", Overloaded),
            ("Model call timed out", Timeout),
            ("OpenAI request failed: operation timed out", Timeout),
            ("OpenAI request failed: connection refused", Transport),
            ("OpenAI API error (404): model_not_found", ModelNotFound),
            ("OpenAI API error (404): no such route", ModelNotFound),
            (
                "Anthropic API error (400): prompt is too long",
                ContextTooLong,
            ),
            (
                "OpenAI API error (400): context_length_exceeded",
                ContextTooLong,
            ),
            ("OpenAI API error (400): bad", BadRequest),
            ("OpenAI API error (422): unprocessable", BadRequest),
            ("ChatGPT usage limit reached", RateLimited),
            ("ChatGPT authorization expired; reconnect", AuthFailed),
            ("something unexpected", Other),
            ("OpenAI response parse failed: eof", Other),
        ];
        for (message, kind) in cases {
            assert_eq!(classify_model_error(message).kind, *kind, "{message}");
        }
    }

    #[test]
    fn classifies_mid_stream_provider_errors() {
        use ModelFailureKind::*;
        let cases: &[(&str, ModelFailureKind)] = &[
            ("Anthropic stream error: overloaded_error", Overloaded),
            ("Anthropic stream error: api_error", Overloaded),
            ("Anthropic stream error: rate_limit_error", RateLimited),
            ("Anthropic stream error: authentication_error", AuthFailed),
            ("Anthropic stream error: permission_error", AuthFailed),
            ("Anthropic stream error: billing_error", Billing),
            ("Anthropic stream error: timeout_error", Timeout),
            ("Anthropic stream error: invalid_request_error", BadRequest),
            ("Anthropic stream error", Other),
            (
                "Google stream failed: RESOURCE_EXHAUSTED: Resource has been exhausted",
                RateLimited,
            ),
            (
                "Google stream failed: UNAVAILABLE: The model is overloaded.",
                Overloaded,
            ),
            (
                "Google stream failed: INTERNAL: An internal error has occurred.",
                Overloaded,
            ),
            (
                "Google stream failed: DEADLINE_EXCEEDED: Deadline expired",
                Timeout,
            ),
            (
                "Google stream failed: PERMISSION_DENIED: denied",
                AuthFailed,
            ),
            ("Google stream failed: UNAUTHENTICATED: nope", AuthFailed),
            ("Google stream failed: no status here", Other),
            ("OpenRouter stream error: server_error", Overloaded),
            ("OpenRouter stream error (502)", Overloaded),
            ("OpenRouter stream error (429)", RateLimited),
            ("OpenRouter stream error (402)", Billing),
            (
                "provider stream read failed: error decoding response body",
                Transport,
            ),
        ];
        for (message, kind) in cases {
            assert_eq!(classify_model_error(message).kind, *kind, "{message}");
        }
    }

    #[test]
    fn a_stream_error_token_must_be_the_named_one() {
        // Words in free text after the token do not count.
        assert_eq!(
            classify_model_error("Google stream failed: CANCELLED: INTERNAL overloaded_error").kind,
            ModelFailureKind::Other
        );
    }

    #[test]
    fn reads_status_and_retry_hint() {
        let failure =
            classify_model_error("Anthropic API error (429): rate_limit_error (retry after 30s)");
        assert_eq!(failure.status, Some(429));
        assert_eq!(failure.retry_after_s, Some(30));
        let failure = classify_model_error("OpenAI API error (400): bad");
        assert_eq!(failure.status, Some(400));
        assert_eq!(failure.retry_after_s, None);
        let failure = classify_model_error("Model call timed out");
        assert_eq!(failure.status, None);
    }

    #[test]
    fn billing_and_context_bodies_win_over_the_status() {
        use ModelFailureKind::*;
        let kind = |m: &str| classify_model_error(m).kind;
        assert_eq!(kind("OpenAI API error (429): insufficient_quota"), Billing);
        assert_eq!(
            kind("Anthropic API error (400): Your credit balance is too low"),
            Billing
        );
        assert_eq!(
            kind("Anthropic API error (429): out of extra usage"),
            Billing
        );
        assert_eq!(
            kind("OpenAI API error (429): billing hard limit reached"),
            Billing
        );
    }

    #[test]
    fn key_rotation_and_fallback_follow_the_spec_table() {
        use ModelFailureKind::*;
        let table = [
            (RateLimited, true, true),
            (Overloaded, false, true),
            (AuthFailed, true, true),
            (Billing, true, true),
            (Timeout, false, true),
            (Transport, false, true),
            (ModelNotFound, false, true),
            (ContextTooLong, false, false),
            (BadRequest, false, false),
            (Other, false, false),
        ];
        for (kind, rotates, falls) in table {
            assert_eq!(kind.rotates_key(), rotates, "{kind:?} rotates_key");
            assert_eq!(kind.falls_back(), falls, "{kind:?} falls_back");
        }
    }
}
