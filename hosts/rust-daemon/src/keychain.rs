//! Optional namespace for OS keychain service names, so an installed daemon
//! and a development daemon never share (or overwrite) each other's items.
//!
//! `ANIMAOS_RS_KEYCHAIN_NAMESPACE` unset means the service names are exactly
//! the historical ones.
use std::borrow::Cow;
use std::sync::OnceLock;

pub(crate) const NAMESPACE_ENV: &str = "ANIMAOS_RS_KEYCHAIN_NAMESPACE";
pub(crate) const NAMESPACE_ERROR: &str =
    "ANIMAOS_RS_KEYCHAIN_NAMESPACE must be 1-32 of a-z, 0-9, -";

const MAX_NAMESPACE_LEN: usize = 32;

/// Accepts `[a-z0-9-]{1,32}`.
pub(crate) fn validate_namespace(raw: &str) -> Result<(), &'static str> {
    let valid = (1..=MAX_NAMESPACE_LEN).contains(&raw.len())
        && raw
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if valid {
        Ok(())
    } else {
        Err(NAMESPACE_ERROR)
    }
}

/// Validates `ANIMAOS_RS_KEYCHAIN_NAMESPACE` when it is set; an unset
/// variable is fine. Called at startup so a bad value fails before binding.
pub(crate) fn validate_namespace_env() -> Result<(), &'static str> {
    match std::env::var(NAMESPACE_ENV) {
        Ok(raw) => validate_namespace(&raw),
        Err(std::env::VarError::NotPresent) => Ok(()),
        Err(std::env::VarError::NotUnicode(_)) => Err(NAMESPACE_ERROR),
    }
}

fn service_name_with<'a>(ns: Option<&str>, base: &'a str) -> Cow<'a, str> {
    match ns {
        Some(ns) => Cow::Owned(format!("{ns}.{base}")),
        None => Cow::Borrowed(base),
    }
}

/// The keychain service name for `base`, prefixed with the process-wide
/// namespace when `ANIMAOS_RS_KEYCHAIN_NAMESPACE` is set. The variable is
/// read once; an invalid value is ignored here because startup rejects it.
pub(crate) fn service_name(base: &str) -> Cow<'_, str> {
    static NAMESPACE: OnceLock<Option<String>> = OnceLock::new();
    let ns = NAMESPACE.get_or_init(|| {
        std::env::var(NAMESPACE_ENV)
            .ok()
            .filter(|raw| validate_namespace(raw).is_ok())
    });
    service_name_with(ns.as_deref(), base)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_name_without_namespace_is_unchanged() {
        assert_eq!(
            service_name_with(None, "animaos.mcp.bearer"),
            "animaos.mcp.bearer"
        );
    }

    #[test]
    fn service_name_with_namespace_prefixes() {
        assert_eq!(
            service_name_with(Some("installed"), "anima.chatgpt.subscription.v1"),
            "installed.anima.chatgpt.subscription.v1"
        );
    }

    #[test]
    fn invalid_namespaces_rejected() {
        let too_long = "a".repeat(33);
        for raw in ["", "Installed", "a.b", too_long.as_str()] {
            assert!(
                validate_namespace(raw).is_err(),
                "{raw:?} should be rejected"
            );
        }
        assert!(validate_namespace("installed").is_ok());
        assert!(validate_namespace(&"a".repeat(32)).is_ok());
        assert!(validate_namespace("dev-2").is_ok());
    }
}
