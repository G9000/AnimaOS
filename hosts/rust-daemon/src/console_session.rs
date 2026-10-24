//! Console sessions authenticate owner reads, approvals and mutations across
//! the daemon API, including lifecycle controls, drain and shutdown.
//!
//! An Origin header proves nothing on its own: any local process, an
//! agent's shell included, can send one. So only the owner can start a
//! session:
//!
//! 1. The tray's "Open Console" and `anima` mint a one-time launch code with
//!    the admin token and open `http://127.0.0.1:<port>/?anima-login=<code>`.
//! 2. The console trades the code at `POST /api/system/session` for a
//!    session in two halves: an `HttpOnly` cookie, and a token it keeps in
//!    its origin's storage and sends as `x-anima-console-session`.
//! 3. Every owner request from the browser needs both halves.
//!
//! Each half covers a different leak. Cookies are scoped to a host, not a
//! port, so a page an agent serves on another loopback port receives the
//! cookie, but it can't read this origin's storage. The token sits in the
//! browser profile on disk; the cookie is a browser-session cookie (no
//! `Max-Age`), so it is usually not written there, though a browser that
//! restores sessions may keep it. Both halves are signed with the admin
//! token: a session lasts twelve hours, outlives server restarts, and a new
//! admin token ends them all.
//!
//! What this does not stop: the launch code travels in a URL, so for its
//! fifteen seconds a process of the same user that watches the program
//! opening the browser, or the browser's history, can trade it first. The
//! owner's console then reports the code as already used. Reading the
//! browser's profile and cookie stores asks for approval
//! (`approvals/detection.rs`), but a process of the same user that already
//! has the owner's approval can still obtain a session.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

/// The query parameter that carries a launch code to the console.
pub const LAUNCH_PARAM: &str = "anima-login";
/// The header that carries a session's token half.
pub(crate) const SESSION_HEADER: &str = "x-anima-console-session";
const SESSION_TTL_SECS: u64 = 12 * 60 * 60;
/// Long enough for the browser to load the console, short for a thief.
const LAUNCH_TTL_SECS: u64 = 15;
/// How far ahead of this clock an issue time may be: codes are minted on
/// this machine.
const CLOCK_SKEW_SECS: u64 = 5;

#[derive(Clone, Copy)]
enum Purpose {
    Launch,
    Cookie,
    Header,
}

impl Purpose {
    fn label(self) -> &'static str {
        match self {
            Self::Launch => "launch",
            Self::Cookie => "cookie",
            Self::Header => "header",
        }
    }
}

/// A one-time code that opens a console session, valid for fifteen seconds.
pub fn launch_code(admin_token: &str) -> String {
    sign(admin_token, Purpose::Launch, now(), &random_id())
}

/// The two halves of a new session.
pub(crate) struct NewSession {
    pub(crate) cookie: String,
    pub(crate) token: String,
}

/// Why a launch code opened no session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LaunchRefusal {
    /// Not a code this admin token signed.
    Invalid,
    Expired,
    /// Already traded: maybe by someone other than the owner's console.
    Spent,
}

/// Launch codes already traded, until they expire.
#[derive(Clone, Default)]
pub(crate) struct SpentLaunchCodes(Arc<Mutex<HashMap<String, u64>>>);

impl SpentLaunchCodes {
    /// Records `id` as spent; `false` when it already was.
    fn spend(&self, id: &str, expires: u64, now: u64) -> bool {
        let mut spent = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        spent.retain(|_, expiry| *expiry >= now);
        spent.insert(id.to_string(), expires).is_none()
    }
}

/// Trades a valid, unspent launch code for a new session.
pub(crate) fn open_session(
    admin_token: &str,
    code: &str,
    spent: &SpentLaunchCodes,
) -> Result<NewSession, LaunchRefusal> {
    open_session_at(admin_token, code, spent, now())
}

fn open_session_at(
    admin_token: &str,
    code: &str,
    spent: &SpentLaunchCodes,
    now: u64,
) -> Result<NewSession, LaunchRefusal> {
    let (issued, id) = verify(admin_token, Purpose::Launch, code, LAUNCH_TTL_SECS, now)?;
    if !spent.spend(id, issued + LAUNCH_TTL_SECS, now) {
        return Err(LaunchRefusal::Spent);
    }
    let session = random_id();
    Ok(NewSession {
        cookie: sign(admin_token, Purpose::Cookie, now, &session),
        token: sign(admin_token, Purpose::Header, now, &session),
    })
}

/// Whether `cookie` and `token` are the two halves of one live session.
pub(crate) fn session_is_valid(admin_token: &str, cookie: &str, token: &str) -> bool {
    session_is_valid_at(admin_token, cookie, token, now())
}

fn session_is_valid_at(admin_token: &str, cookie: &str, token: &str, now: u64) -> bool {
    let cookie = verify(admin_token, Purpose::Cookie, cookie, SESSION_TTL_SECS, now);
    let token = verify(admin_token, Purpose::Header, token, SESSION_TTL_SECS, now);
    matches!((cookie, token), (Ok(cookie), Ok(token)) if cookie == token)
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

fn random_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// `{issued}.{id}.{mac}`: digits, then 32 and 64 hex digits.
fn sign(key: &str, purpose: Purpose, issued: u64, id: &str) -> String {
    format!("{issued}.{id}.{}", hex(&mac(key, purpose, issued, id)))
}

fn mac(key: &str, purpose: Purpose, issued: u64, id: &str) -> Vec<u8> {
    // HMAC accepts a key of any length.
    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(key.as_bytes()) else {
        return Vec::new();
    };
    mac.update(format!("anima-console-v1|{}|{issued}|{id}", purpose.label()).as_bytes());
    mac.finalize().into_bytes().to_vec()
}

/// The issue time and id of a value `sign` made for `purpose`, while it is
/// at most `ttl` seconds old.
fn verify<'a>(
    key: &str,
    purpose: Purpose,
    value: &'a str,
    ttl: u64,
    now: u64,
) -> Result<(u64, &'a str), LaunchRefusal> {
    let (issued, id) = signed(key, purpose, value).ok_or(LaunchRefusal::Invalid)?;
    let fresh = issued <= now.saturating_add(CLOCK_SKEW_SECS) && now.saturating_sub(issued) <= ttl;
    if fresh {
        Ok((issued, id))
    } else {
        Err(LaunchRefusal::Expired)
    }
}

/// The issue time and id of a value `sign` made for `purpose`, at any age.
fn signed<'a>(key: &str, purpose: Purpose, value: &'a str) -> Option<(u64, &'a str)> {
    if key.is_empty() {
        return None;
    }
    let mut parts = value.split('.');
    let (Some(issued), Some(id), Some(presented), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return None;
    };
    let issued_at = issued
        .bytes()
        .all(|byte| byte.is_ascii_digit())
        .then(|| issued.parse::<u64>().ok())
        .flatten()?;
    if id.len() != 32 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let expected = hex(&mac(key, purpose, issued_at, id));
    constant_time_eq(presented.as_bytes(), expected.as_bytes()).then_some((issued_at, id))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .fold(0u8, |diff, (a, b)| diff | (a ^ b))
            == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "admin-token";

    fn session(spent: &SpentLaunchCodes, now: u64) -> NewSession {
        let code = sign(KEY, Purpose::Launch, now, &random_id());
        open_session_at(KEY, &code, spent, now).expect("a fresh code opens a session")
    }

    #[test]
    fn a_launch_code_opens_one_session() {
        let spent = SpentLaunchCodes::default();
        let code = launch_code(KEY);
        let opened = open_session(KEY, &code, &spent).expect("opens");
        assert!(session_is_valid(KEY, &opened.cookie, &opened.token));
        assert_eq!(
            open_session(KEY, &code, &spent).err(),
            Some(LaunchRefusal::Spent),
            "a spent code opens nothing, and says so"
        );
        for (key, code) in [
            ("other-admin-token", launch_code(KEY)),
            (KEY, launch_code("")),
            ("", launch_code("")),
        ] {
            assert_eq!(
                open_session(key, &code, &spent).err(),
                Some(LaunchRefusal::Invalid)
            );
        }
    }

    #[test]
    fn launch_codes_expire() {
        let spent = SpentLaunchCodes::default();
        let code = sign(KEY, Purpose::Launch, 1_000, &random_id());
        assert_eq!(LAUNCH_TTL_SECS, 15);
        for late in [1_000 + LAUNCH_TTL_SECS + 1, 1_000 - CLOCK_SKEW_SECS - 1] {
            assert_eq!(
                open_session_at(KEY, &code, &spent, late).err(),
                Some(LaunchRefusal::Expired)
            );
        }
        assert!(open_session_at(KEY, &code, &spent, 1_000 + LAUNCH_TTL_SECS).is_ok());
    }

    #[test]
    fn a_session_needs_both_matching_halves() {
        let spent = SpentLaunchCodes::default();
        let now = 5_000_000;
        let one = session(&spent, now);
        let two = session(&spent, now);
        assert!(session_is_valid_at(KEY, &one.cookie, &one.token, now));
        // Halves of different sessions, a half used as the other, or a
        // forged mac don't make a session.
        assert!(!session_is_valid_at(KEY, &one.cookie, &two.token, now));
        assert!(!session_is_valid_at(KEY, &one.token, &one.cookie, now));
        assert!(!session_is_valid_at(KEY, &one.cookie, &one.cookie, now));
        let (head, _) = one.token.rsplit_once('.').unwrap();
        let forged = format!("{head}.{}", "0".repeat(64));
        assert!(!session_is_valid_at(KEY, &one.cookie, &forged, now));
        assert!(!session_is_valid_at(
            "rotated",
            &one.cookie,
            &one.token,
            now
        ));
        assert!(!session_is_valid_at(KEY, "", "", now));
        // Sessions last twelve hours.
        assert_eq!(SESSION_TTL_SECS, 12 * 60 * 60);
        assert!(session_is_valid_at(
            KEY,
            &one.cookie,
            &one.token,
            now + SESSION_TTL_SECS
        ));
        assert!(!session_is_valid_at(
            KEY,
            &one.cookie,
            &one.token,
            now + SESSION_TTL_SECS + 1
        ));
    }

    #[test]
    fn malformed_values_are_rejected() {
        let id = random_id();
        let good = sign(KEY, Purpose::Header, 10, &id);
        assert!(verify(KEY, Purpose::Header, &good, 100, 10).is_ok());
        for bad in [
            String::new(),
            format!("{good}.extra"),
            good.replacen("10.", "+10.", 1),
            format!("10.{}.{}", &id[..31], good.rsplit('.').next().unwrap()),
        ] {
            assert!(
                verify(KEY, Purpose::Header, &bad, 100, 10).is_err(),
                "{bad}"
            );
        }
    }
}
