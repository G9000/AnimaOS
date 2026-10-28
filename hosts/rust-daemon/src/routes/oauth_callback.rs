//! What the page shown after a provider's sign-in says when the provider
//! refused. Only fixed wording is ever returned: nothing from the URL is
//! echoed, so a crafted link cannot put its own text on the page.

use axum::http::Uri;

/// Fixed wording for a standard OAuth `error` code in a callback URL, or
/// `None` for no code or one this does not know, which keeps the caller's
/// general message.
pub(super) fn denial_hint(provider: &str, uri: &Uri) -> Option<String> {
    let query = super::http::request_query(uri).ok()?;
    let code = query.get("error")?;
    let text = match (code.as_str(), provider) {
        ("access_denied", "Google") => "Google did not give access. If you cancelled, reconnect and choose Allow. If you did not, add the account you signed in with as a test user on the app's consent screen, then reconnect from Connectors.".to_string(),
        ("access_denied", _) => format!("{provider} did not give access. If you cancelled, reconnect and accept. If you did not, your organization may need an admin to approve the app. Then reconnect from Connectors."),
        (
            "invalid_request" | "invalid_scope" | "unauthorized_client" | "unsupported_response_type",
            _,
        ) => format!("{provider} rejected the request from the app. Check the app's credentials and settings in Connectors, then reconnect."),
        ("server_error" | "temporarily_unavailable", _) => {
            format!("{provider} had a problem on its side. Wait a moment, then reconnect from Connectors.")
        }
        ("consent_required" | "interaction_required" | "login_required", _) => {
            format!("{provider} needs you to sign in or approve again. Reconnect from Connectors.")
        }
        _ => return None,
    };
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uri(query: &str) -> Uri {
        format!("/api/connectors/mail/gmail/callback?{query}")
            .parse()
            .unwrap()
    }

    #[test]
    fn access_denied_names_the_provider_and_the_likely_fix() {
        let google = denial_hint("Google", &uri("error=access_denied&state=abc")).unwrap();
        assert!(google.starts_with("Google did not give access."));
        assert!(google.contains("test user"));
        let microsoft = denial_hint("Microsoft", &uri("error=access_denied&state=abc")).unwrap();
        assert!(microsoft.starts_with("Microsoft did not give access."));
        assert!(microsoft.contains("admin"));
        assert!(!microsoft.contains("test user"));
    }

    #[test]
    fn other_standard_errors_have_their_own_wording() {
        for code in [
            "invalid_request",
            "invalid_scope",
            "unauthorized_client",
            "unsupported_response_type",
        ] {
            let text = denial_hint("Google", &uri(&format!("error={code}"))).unwrap();
            assert!(text.contains("rejected the request"), "{code}");
        }
        for code in ["server_error", "temporarily_unavailable"] {
            let text = denial_hint("Microsoft", &uri(&format!("error={code}"))).unwrap();
            assert!(text.contains("problem on its side"), "{code}");
        }
        let text = denial_hint("Microsoft", &uri("error=interaction_required")).unwrap();
        assert!(text.contains("sign in or approve again"));
    }

    #[test]
    fn unknown_or_missing_errors_keep_the_general_message_and_are_never_echoed() {
        assert!(denial_hint("Google", &uri("state=abc&code=secret")).is_none());
        assert!(denial_hint("Google", &uri("error=provider-secret")).is_none());
        assert!(denial_hint("Google", &uri("error=%3Cscript%3Ealert(1)%3C%2Fscript%3E")).is_none());
        assert!(denial_hint("Google", &uri("error=")).is_none());
        // The fixed wording never contains what the URL carried.
        let text = denial_hint("Google", &uri("error=access_denied&state=nonce-sentinel")).unwrap();
        assert!(!text.contains("nonce-sentinel"));
    }
}
