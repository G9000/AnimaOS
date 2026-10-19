//! The link to the `anima` supervisor that started this daemon. The console
//! only talks to the daemon, so `/api/system` relays its status reads and
//! lifecycle actions to the supervisor's control API with the supervisor's
//! bearer token, which the browser never sees.
use std::time::Duration;

use axum::body::Bytes;
use axum::http::StatusCode;
use tracing::warn;
use zeroize::Zeroizing;

const URL_ENV: &str = "ANIMA_SUPERVISOR_URL";
const TOKEN_ENV: &str = "ANIMA_SUPERVISOR_TOKEN";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

/// Where the supervisor's control API is, and the token it expects. No
/// `Debug`: it holds the token.
#[derive(Clone)]
pub(crate) struct SupervisorLink {
    url: String,
    token: Zeroizing<String>,
}

/// The supervisor's answer, status and body as it sent them.
pub(crate) struct Upstream {
    pub(crate) status: StatusCode,
    pub(crate) body: Bytes,
}

/// The supervisor could not be reached, or did not answer in time.
pub(crate) struct Unreachable;

/// The supervisor link read once at startup (`None` when the daemon was not
/// started by a supervisor) and the client that calls it. Shared by the
/// router's clones.
#[derive(Clone)]
pub(crate) struct SupervisorRelay {
    link: Option<SupervisorLink>,
    client: reqwest::Client,
}

impl SupervisorRelay {
    /// Configured only when both `ANIMA_SUPERVISOR_URL` and
    /// `ANIMA_SUPERVISOR_TOKEN` are set.
    pub(crate) fn from_env() -> Self {
        let read = |name: &str| std::env::var(name).ok().filter(|value| !value.is_empty());
        match (read(URL_ENV), read(TOKEN_ENV)) {
            (Some(url), Some(token)) => Self::new(Some((url, token))),
            _ => Self::new(None),
        }
    }

    /// A relay to `(url, token)`; `None` is an unsupervised daemon.
    pub(crate) fn new(link: Option<(String, String)>) -> Self {
        let Some((url, token)) = link else {
            return Self::unsupervised();
        };
        let valid = reqwest::Url::parse(&url)
            .map(|parsed| matches!(parsed.scheme(), "http" | "https"))
            .unwrap_or(false);
        if !valid {
            warn!("{URL_ENV} is not an http(s) URL; the supervisor relay is off");
            return Self::unsupervised();
        }
        // The token goes only to the supervisor: no system proxy sees it and
        // a redirect is the supervisor's answer, not something to follow.
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build();
        match client {
            Ok(client) => Self {
                link: Some(SupervisorLink {
                    url: url.trim_end_matches('/').to_string(),
                    token: Zeroizing::new(token),
                }),
                client,
            },
            Err(error) => {
                warn!(%error, "the supervisor relay is off: its HTTP client could not be built");
                Self::unsupervised()
            }
        }
    }

    pub(crate) fn unsupervised() -> Self {
        Self {
            link: None,
            client: reqwest::Client::new(),
        }
    }

    pub(crate) fn is_supervised(&self) -> bool {
        self.link.is_some()
    }

    /// `GET /v1/status`.
    pub(crate) async fn status(&self) -> Result<Upstream, Unreachable> {
        self.send("/v1/status", None).await
    }

    /// `POST /v1/actions/{action}` with `{ "force": force }`.
    pub(crate) async fn action(&self, action: &str, force: bool) -> Result<Upstream, Unreachable> {
        let body = serde_json::json!({ "force": force });
        self.send(&format!("/v1/actions/{action}"), Some(body))
            .await
    }

    async fn send(
        &self,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> Result<Upstream, Unreachable> {
        let link = self.link.as_ref().ok_or(Unreachable)?;
        let url = format!("{}{path}", link.url);
        let request = match body {
            Some(body) => self.client.post(url).json(&body),
            None => self.client.get(url),
        };
        let response = request
            .bearer_auth(link.token.as_str())
            .send()
            .await
            .map_err(unreachable)?;
        let status =
            StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
        let body = response.bytes().await.map_err(unreachable)?;
        Ok(Upstream { status, body })
    }
}

fn unreachable(error: reqwest::Error) -> Unreachable {
    warn!(error = %error.without_url(), "the supervisor did not answer");
    Unreachable
}
