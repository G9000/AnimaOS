//! The control API's client, used by every command that talks to a running
//! supervisor.

use std::io;
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use reqwest::blocking::{Client, Response};
use reqwest::StatusCode;
use serde::Deserialize;
use serde_json::json;

use crate::config::Config;
use crate::paths::AnimaPaths;
use crate::supervisor::{Action, SupervisorStatus};

/// How long to wait for the control port to accept a connection.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(1);
/// The supervisor answers an action within 10 seconds, or says it could not.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

/// What the supervisor made of a requested action.
#[derive(Debug)]
pub enum ActionOutcome {
    /// Under way; the status as it is now.
    Accepted(SupervisorStatus),
    /// Another action is in progress.
    Conflict {
        message: String,
        pending: Option<Action>,
    },
}

pub struct SupervisorClient {
    http: Client,
    base: String,
    token: String,
}

#[derive(Deserialize)]
struct ConflictBody {
    error: String,
    pending: Option<Action>,
}

impl SupervisorClient {
    /// `None` when nothing listens on the control port.
    pub fn connect(paths: &AnimaPaths, cfg: &Config) -> io::Result<Option<Self>> {
        let address = SocketAddr::from(([127, 0, 0, 1], cfg.control_port));
        if TcpStream::connect_timeout(&address, CONNECT_TIMEOUT).is_err() {
            return Ok(None);
        }
        let token = std::fs::read_to_string(&paths.control_token).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!(
                    "something listens on port {} but {} can't be read: {error}",
                    cfg.control_port,
                    paths.control_token.display()
                ),
            )
        })?;
        let http = Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .no_proxy()
            .build()
            .map_err(to_io)?;
        Ok(Some(Self {
            http,
            base: format!("http://127.0.0.1:{}", cfg.control_port),
            token: token.trim().to_string(),
        }))
    }

    pub fn status(&self) -> io::Result<SupervisorStatus> {
        let response = self
            .http
            .get(format!("{}/v1/status", self.base))
            .bearer_auth(&self.token)
            .send()
            .map_err(to_io)?;
        expect_success(response)?.json().map_err(to_io)
    }

    pub fn action(&self, action: Action, force: bool) -> io::Result<ActionOutcome> {
        // The action's own kebab-case name, as it appears in the route.
        let name = serde_json::to_value(action)
            .ok()
            .and_then(|value| value.as_str().map(str::to_string))
            .ok_or_else(|| io::Error::other("an action has no name"))?;
        let response = self
            .http
            .post(format!("{}/v1/actions/{name}", self.base))
            .bearer_auth(&self.token)
            .json(&json!({ "force": force }))
            .send()
            .map_err(to_io)?;
        if response.status() == StatusCode::CONFLICT {
            let body: ConflictBody = response.json().map_err(to_io)?;
            return Ok(ActionOutcome::Conflict {
                message: body.error,
                pending: body.pending,
            });
        }
        expect_success(response)?
            .json()
            .map(ActionOutcome::Accepted)
            .map_err(to_io)
    }
}

fn expect_success(response: Response) -> io::Result<Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    if status == StatusCode::UNAUTHORIZED {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the supervisor did not accept the control token",
        ));
    }
    let detail = response
        .json::<serde_json::Value>()
        .ok()
        .and_then(|body| body["error"].as_str().map(str::to_string))
        .unwrap_or_else(|| status.to_string());
    Err(io::Error::other(detail))
}

fn to_io(error: reqwest::Error) -> io::Error {
    io::Error::other(error.to_string())
}
