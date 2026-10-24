//! Explicit credentials for route fixtures; every request still uses production authorization.
use axum::http::{header, request::Builder, HeaderValue};

pub(crate) const ADMIN_TOKEN: &str = "anima-test-owner-token";

pub(in crate::routes) fn policy(bind_is_loopback: bool) -> super::http::LocalOwnerPolicy {
    super::http::LocalOwnerPolicy::for_test(bind_is_loopback, Some(ADMIN_TOKEN))
}

/// Attach real signed session credentials to an intended owner request. Callers
/// testing rejection must build their request without this helper.
pub(crate) fn authenticate(mut request: Builder) -> Builder {
    if request
        .headers_ref()
        .and_then(|h| h.get(header::HOST))
        .is_none()
    {
        request = request.header(header::HOST, "127.0.0.1:8080");
    }
    if request
        .headers_ref()
        .and_then(|h| h.get(header::ORIGIN))
        .is_none()
    {
        request = request.header(header::ORIGIN, "http://localhost:4200");
    }
    let port = request
        .headers_ref()
        .and_then(|h| h.get(header::HOST))
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<axum::http::uri::Authority>().ok())
        .and_then(|v| v.port_u16())
        .unwrap_or(80);
    let session = crate::console_session::open_session(
        ADMIN_TOKEN,
        &crate::console_session::launch_code(ADMIN_TOKEN),
        &crate::console_session::SpentLaunchCodes::default(),
    )
    .expect("fixture launch code is valid");
    request
        .header(
            header::COOKIE,
            HeaderValue::from_str(&format!("anima_console_{port}={}", session.cookie)).unwrap(),
        )
        .header(crate::console_session::SESSION_HEADER, session.token)
}
