//! Default provider transport bounds; injected clients retain caller ownership.
use reqwest::header::HeaderMap;
use reqwest::Client;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
pub(crate) const PROVIDER_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
pub(crate) const PROVIDER_REQUEST_TIMEOUT: Duration = Duration::from_secs(300);
pub(crate) fn bounded_provider_client() -> Result<Client, String> {
    Client::builder()
        .connect_timeout(PROVIDER_CONNECT_TIMEOUT)
        .timeout(PROVIDER_REQUEST_TIMEOUT)
        .build()
        .map_err(|_| "bounded provider transport initialization failed".to_owned())
}
pub(crate) fn transport_error(provider: &str, operation: &str, error: reqwest::Error) -> String {
    format!("{provider} {operation} failed: {}", error.without_url())
}

/// Seconds until the provider says a failed request may be retried, read from
/// `retry-after` (seconds or an HTTP date) or a rate-limit reset header
/// (`anthropic-ratelimit-requests-reset` timestamp, `x-ratelimit-reset-requests`
/// duration such as `6m0s`).
pub(crate) fn retry_after_seconds(headers: &HeaderMap) -> Option<u64> {
    let text = |name: &str| headers.get(name)?.to_str().ok().map(str::trim);
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    let until = |epoch: u64| epoch.saturating_sub(now);
    if let Some(value) = text("retry-after") {
        if let Ok(seconds) = value.parse::<u64>() {
            return Some(seconds);
        }
        if let Some(epoch) = parse_http_date(value) {
            return Some(until(epoch));
        }
    }
    if let Some(epoch) = text("anthropic-ratelimit-requests-reset").and_then(parse_rfc3339) {
        return Some(until(epoch));
    }
    text("x-ratelimit-reset-requests").and_then(parse_reset_duration)
}

/// The error with ` (retry after Ns)` appended when the response carried a hint.
pub(crate) fn with_retry_hint(error: String, headers: &HeaderMap) -> String {
    match retry_after_seconds(headers) {
        Some(seconds) => format!("{error} (retry after {seconds}s)"),
        None => error,
    }
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn epoch_seconds(y: i64, mo: i64, d: i64, h: i64, mi: i64, s: i64) -> Option<u64> {
    // Header dates are untrusted: bound the year so the day arithmetic cannot overflow.
    if !(1970..=9999).contains(&y)
        || !(1..=12).contains(&mo)
        || !(1..=31).contains(&d)
        || !(0..=23).contains(&h)
        || !(0..=59).contains(&mi)
        || !(0..=60).contains(&s)
    {
        return None;
    }
    u64::try_from(days_from_civil(y, mo, d) * 86_400 + h * 3_600 + mi * 60 + s).ok()
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// `Sun, 06 Nov 1994 08:49:37 GMT`
fn parse_http_date(value: &str) -> Option<u64> {
    let mut parts = value.split_whitespace().skip(1);
    let day = parts.next()?.parse().ok()?;
    let name = parts.next()?;
    let month = MONTHS.iter().position(|month| *month == name)? as i64 + 1;
    let year = parts.next()?.parse().ok()?;
    let mut clock = parts
        .next()?
        .split(':')
        .map(|part| part.parse::<i64>().ok());
    let (h, mi, s) = (clock.next()??, clock.next()??, clock.next()??);
    epoch_seconds(year, month, day, h, mi, s)
}

/// `2024-05-01T12:30:00Z`, with optional fraction or numeric offset.
fn parse_rfc3339(value: &str) -> Option<u64> {
    let (date, time) = value.split_once(['T', 't'])?;
    let mut ymd = date.split('-').map(|part| part.parse::<i64>().ok());
    let (y, mo, d) = (ymd.next()??, ymd.next()??, ymd.next()??);
    let zone = time.find(['Z', 'z', '+', '-']);
    let (clock, zone) = zone.map_or((time, ""), |at| time.split_at(at));
    let mut hms = clock.split(':');
    let h = hms.next()?.parse().ok()?;
    let mi = hms.next()?.parse().ok()?;
    let s = hms.next()?.split('.').next()?.parse().ok()?;
    let offset = match zone.as_bytes().first() {
        Some(b'+' | b'-') => {
            let (hh, mm) = zone[1..].split_once(':')?;
            let (hh, mm) = (hh.parse::<i64>().ok()?, mm.parse::<i64>().ok()?);
            if !(0..=23).contains(&hh) || !(0..=59).contains(&mm) {
                return None;
            }
            let seconds = hh * 3_600 + mm * 60;
            if zone.starts_with('-') {
                -seconds
            } else {
                seconds
            }
        }
        _ => 0,
    };
    let epoch = i64::try_from(epoch_seconds(y, mo, d, h, mi, s)?).ok()? - offset;
    u64::try_from(epoch).ok()
}

/// `1s`, `6m0s`, `20ms`, `1h2m3.5s`; fractions round up.
fn parse_reset_duration(value: &str) -> Option<u64> {
    let mut rest = value;
    let mut millis = 0.0_f64;
    while !rest.is_empty() {
        let number_end = rest.find(|c: char| !(c.is_ascii_digit() || c == '.'))?;
        let number: f64 = rest[..number_end].parse().ok()?;
        rest = &rest[number_end..];
        let (factor, unit_len) = if rest.starts_with("ms") {
            (1.0, 2)
        } else if rest.starts_with('s') {
            (1_000.0, 1)
        } else if rest.starts_with('m') {
            (60_000.0, 1)
        } else if rest.starts_with('h') {
            (3_600_000.0, 1)
        } else {
            return None;
        };
        millis += number * factor;
        rest = &rest[unit_len..];
    }
    Some((millis / 1_000.0).ceil() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_transport_has_named_bounds() {
        assert_eq!(PROVIDER_CONNECT_TIMEOUT, Duration::from_secs(15));
        assert_eq!(PROVIDER_REQUEST_TIMEOUT, Duration::from_secs(300));
        assert!(bounded_provider_client().is_ok());
    }

    #[test]
    fn transport_error_omits_secret_request_url_and_header() {
        let error = Client::new()
            .get("https://private.test/?token=fake-secret-key")
            .header(reqwest::header::AUTHORIZATION, "fake-secret-key\n")
            .build()
            .unwrap_err();
        let error = transport_error("OpenAI", "request", error);
        assert!(error.starts_with("OpenAI request failed:"));
        assert!(!error.contains("fake-secret-key"));
        assert!(!error.contains("private.test"));
    }

    #[tokio::test]
    async fn scripted_transport_timeout_keeps_partial_frames_and_usage() {
        use anima_core::{ModelStreamFrame, ModelStreamSink};
        use std::sync::Mutex;
        struct Sink(Mutex<Vec<ModelStreamFrame>>);
        #[async_trait::async_trait]
        impl ModelStreamSink for Sink {
            async fn emit(&self, frame: ModelStreamFrame) -> Result<(), String> {
                self.0.lock().unwrap().push(frame);
                Ok(())
            }
        }
        let data = b"data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Partial\"}}],\"usage\":{\"prompt_tokens\":12,\"completion_tokens\":3,\"total_tokens\":15}}\n\n".to_vec();
        let chunks = futures::stream::iter([
            Ok(data),
            Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "request timed out",
            )),
        ]);
        let response = reqwest::Response::from(axum::http::Response::new(
            reqwest::Body::wrap_stream(chunks),
        ));
        let sink = Sink(Mutex::new(Vec::new()));
        let provider = crate::catalog::resolve_provider("openai").unwrap();
        let error = crate::stream::consume_openai_sse(response, &sink, provider.definition, false)
            .await
            .unwrap_err();
        assert!(error.starts_with("provider stream read failed:"));
        let frames = sink.0.lock().unwrap();
        assert!(frames
            .iter()
            .any(|frame| matches!(frame, ModelStreamFrame::TextDelta(text) if text == "Partial")));
        assert!(frames.iter().any(
            |frame| matches!(frame, ModelStreamFrame::Usage(usage) if usage.total_tokens == 15)
        ));
        assert!(!frames
            .iter()
            .any(|frame| matches!(frame, ModelStreamFrame::Final(_))));
    }

    #[test]
    fn retry_hints_read_every_documented_header_form() {
        use reqwest::header::{HeaderName, HeaderValue};
        let one = |name: &'static str, value: &str| {
            let mut headers = HeaderMap::new();
            headers.insert(
                HeaderName::from_static(name),
                HeaderValue::from_str(value).unwrap(),
            );
            retry_after_seconds(&headers)
        };
        assert_eq!(one("retry-after", "12"), Some(12));
        assert_eq!(one("retry-after", "Sun, 06 Nov 1994 08:49:37 GMT"), Some(0));
        assert!(one("retry-after", "Fri, 01 Jan 2999 00:00:00 GMT").unwrap() > 1_000_000);
        assert_eq!(one("retry-after", "soon"), None);
        // Untrusted dates with absurd years are rejected, not overflowed.
        for huge in [
            "Sun, 01 Jan 99999999999999999 00:00:00 GMT",
            "Sun, 01 Jan 9223372036854775807 00:00:00 GMT",
            "Sun, 01 Jan 0001 00:00:00 GMT",
        ] {
            assert_eq!(one("retry-after", huge), None, "{huge}");
        }
        assert_eq!(
            one(
                "anthropic-ratelimit-requests-reset",
                "99999999999999999-01-01T00:00:00Z"
            ),
            None
        );
        assert_eq!(
            one(
                "anthropic-ratelimit-requests-reset",
                "-9223372036854775808-01-01T00:00:00Z"
            ),
            None
        );
        assert_eq!(
            one(
                "anthropic-ratelimit-requests-reset",
                "2999-01-01T00:00:00+9223372036854775807:00"
            ),
            None
        );
        assert_eq!(
            one("anthropic-ratelimit-requests-reset", "1970-01-01T00:00:00Z"),
            Some(0)
        );
        assert!(
            one(
                "anthropic-ratelimit-requests-reset",
                "2999-01-01T00:00:00.5+02:00"
            )
            .unwrap()
                > 1_000_000
        );
        assert_eq!(one("x-ratelimit-reset-requests", "6m0s"), Some(360));
        assert_eq!(one("x-ratelimit-reset-requests", "20ms"), Some(1));
        assert_eq!(one("x-ratelimit-reset-requests", "1h2m3.5s"), Some(3_724));
        assert_eq!(one("x-ratelimit-reset-requests", "later"), None);
    }

    #[test]
    fn epoch_conversion_matches_known_dates() {
        assert_eq!(
            parse_http_date("Sun, 06 Nov 1994 08:49:37 GMT"),
            Some(784_111_777)
        );
        assert_eq!(parse_rfc3339("2000-03-01T00:00:00Z"), Some(951_868_800));
    }
}
