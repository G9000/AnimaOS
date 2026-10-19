//! The server child (`anima run`) and the server routes the supervisor calls.

use std::collections::BTreeMap;
use std::io;
use std::path::Path;
use std::process::{ExitStatus, Stdio};
use std::sync::Arc;
use std::time::Duration;

use anima_daemon::system::InFlight;
use process_wrap::tokio::{ChildWrapper, CommandWrap, KillOnDrop};
use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::sync::{oneshot, watch};

use super::logs::ServerLog;
use super::tokens::AdminToken;

/// Every call to the server gives up after this long.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
/// A health probe is answered at once or not at all.
const HEALTH_TIMEOUT: Duration = Duration::from_secs(2);

/// A running `anima run`. Dropping it kills the server's process tree.
pub struct ServerChild {
    exit: watch::Receiver<Option<ExitStatus>>,
    kill: Option<oneshot::Sender<()>>,
}

impl ServerChild {
    /// Starts `exe run` with `env` added to this process's environment, its
    /// output going to `log`. On Unix it leads its own process group; on
    /// Windows it runs in a job object without a console window.
    pub fn spawn(exe: &Path, env: &BTreeMap<String, String>, log: &ServerLog) -> io::Result<Self> {
        let mut command = tokio::process::Command::new(exe);
        command
            .arg("run")
            .envs(env)
            // Held open for the child's lifetime: when this process dies,
            // the server sees end of input and shuts itself down.
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut command = CommandWrap::from(command);
        command.wrap(KillOnDrop);
        #[cfg(windows)]
        {
            use process_wrap::tokio::{CreationFlags, JobObject};
            let mut flags = CreationFlags(Default::default());
            flags.0 .0 = 0x0800_0000; // CREATE_NO_WINDOW
            command.wrap(flags).wrap(JobObject);
        }
        #[cfg(unix)]
        command.wrap(process_wrap::tokio::ProcessGroup::leader());
        let mut child = command.spawn()?;
        if let Some(stdout) = child.stdout().take() {
            tokio::spawn(forward(stdout, log.clone()));
        }
        if let Some(stderr) = child.stderr().take() {
            tokio::spawn(forward(stderr, log.clone()));
        }
        let (exit_tx, exit) = watch::channel(None);
        let (kill, kill_rx) = oneshot::channel();
        let lifeline = child.stdin().take();
        tokio::spawn(async move {
            watch_child(child, kill_rx, exit_tx).await;
            drop(lifeline);
        });
        Ok(Self {
            exit,
            kill: Some(kill),
        })
    }

    /// Kills the server's process tree; `exited` reports when it is gone.
    pub fn kill(&mut self) {
        if let Some(kill) = self.kill.take() {
            let _ = kill.send(());
        }
    }

    pub fn is_alive(&self) -> bool {
        self.exit.borrow().is_none()
    }

    /// A handle that resolves when the server exits.
    pub fn exit_watch(&self) -> watch::Receiver<Option<ExitStatus>> {
        self.exit.clone()
    }
}

/// Resolves with the exit status once the server behind `exit` is gone;
/// never resolves without one.
pub async fn exited(exit: &mut Option<watch::Receiver<Option<ExitStatus>>>) -> Option<ExitStatus> {
    let Some(exit) = exit.as_mut() else {
        return std::future::pending().await;
    };
    match exit.wait_for(Option::is_some).await {
        Ok(status) => *status,
        // The watcher ended without a status: the child is gone all the same.
        Err(_) => None,
    }
}

async fn watch_child(
    mut child: Box<dyn ChildWrapper>,
    kill: oneshot::Receiver<()>,
    exit: watch::Sender<Option<ExitStatus>>,
) {
    let status = tokio::select! {
        status = child.wait() => status,
        _ = kill => {
            let _ = child.start_kill();
            child.wait().await
        }
    };
    let status = match status {
        Ok(status) => status,
        // Waiting failed: make sure it is gone, then report it like a crash.
        Err(_) => {
            let _ = child.start_kill();
            default_failure()
        }
    };
    let _ = exit.send(Some(status));
}

#[cfg(unix)]
fn default_failure() -> ExitStatus {
    use std::os::unix::process::ExitStatusExt;
    ExitStatus::from_raw(1 << 8)
}

#[cfg(windows)]
fn default_failure() -> ExitStatus {
    use std::os::windows::process::ExitStatusExt;
    ExitStatus::from_raw(1)
}

async fn forward(stream: impl AsyncRead + Unpin, log: ServerLog) {
    let mut reader = BufReader::new(stream);
    let mut line = Vec::new();
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line).await {
            Ok(0) | Err(_) => break,
            Ok(_) => log.server_line(&line),
        }
    }
}

/// The answer of `GET /api/system/drain`.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DrainStatus {
    pub draining: bool,
    pub in_flight: InFlight,
}

/// What `POST /api/system/shutdown` came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownRequest {
    Accepted,
    /// Nothing is listening: the server can't be asked, only killed.
    Unreachable,
    /// The server may have taken it (a timeout, an error status).
    Unknown,
}

/// The server's routes, called with its admin token and no Origin (plus
/// the server's API key when one is configured).
#[derive(Clone)]
pub struct ServerApi {
    client: reqwest::Client,
    base: String,
    admin: AdminToken,
    api_key: Option<String>,
    /// Drain calls go out one at a time, in order.
    drain_order: Arc<tokio::sync::Mutex<()>>,
}

impl ServerApi {
    pub fn new(port: u16, admin: AdminToken, api_key: Option<String>) -> io::Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(io::Error::other)?;
        Ok(Self {
            client,
            base: format!("http://127.0.0.1:{port}"),
            admin,
            api_key,
            drain_order: Arc::new(tokio::sync::Mutex::new(())),
        })
    }

    pub fn set_port(&mut self, port: u16) {
        self.base = format!("http://127.0.0.1:{port}");
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let request = self
            .client
            .request(method, format!("{}{path}", self.base))
            .bearer_auth(self.admin.as_str());
        match &self.api_key {
            Some(key) => request.header("x-api-key", key),
            None => request,
        }
    }

    async fn send(&self, method: reqwest::Method, path: &str) -> Option<reqwest::Response> {
        let response = self.request(method, path).send().await.ok()?;
        response.status().is_success().then_some(response)
    }

    async fn get_json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Option<T> {
        self.send(reqwest::Method::GET, path)
            .await?
            .json()
            .await
            .ok()
    }

    /// `GET /health` answered 200.
    pub async fn healthy(&self) -> bool {
        let probe = self
            .request(reqwest::Method::GET, "/health")
            .timeout(HEALTH_TIMEOUT)
            .send()
            .await;
        matches!(probe, Ok(response) if response.status().is_success())
    }

    /// Runs one drain call to completion on its own task, after the drain
    /// calls before it: dropping the caller neither cuts it off nor lets a
    /// later call overtake it (a cancel can't land before a begin).
    async fn drain_call<T, F, Fut>(&self, call: F) -> Option<T>
    where
        T: Send + 'static,
        F: FnOnce(ServerApi) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Option<T>> + Send + 'static,
    {
        let order = Arc::clone(&self.drain_order);
        let api = self.clone();
        tokio::spawn(async move {
            let _turn = order.lock_owned().await;
            call(api).await
        })
        .await
        .ok()
        .flatten()
    }

    /// `POST /api/system/drain`.
    pub async fn begin_drain(&self) -> Option<DrainStatus> {
        self.drain_call(|api| async move {
            api.send(reqwest::Method::POST, "/api/system/drain")
                .await?
                .json()
                .await
                .ok()
        })
        .await
    }

    /// `GET /api/system/drain`.
    pub async fn drain_status(&self) -> Option<DrainStatus> {
        self.drain_call(|api| async move { api.get_json("/api/system/drain").await })
            .await
    }

    /// `DELETE /api/system/drain`, then `GET` to confirm the server is no
    /// longer draining.
    pub async fn end_drain(&self) -> bool {
        self.drain_call(|api| async move {
            api.send(reqwest::Method::DELETE, "/api/system/drain")
                .await?;
            let status: DrainStatus = api.get_json("/api/system/drain").await?;
            (!status.draining).then_some(())
        })
        .await
        .is_some()
    }

    /// `POST /api/system/shutdown`.
    pub async fn shutdown(&self) -> ShutdownRequest {
        match self
            .request(reqwest::Method::POST, "/api/system/shutdown")
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => ShutdownRequest::Accepted,
            Err(error) if error.is_connect() => ShutdownRequest::Unreachable,
            _ => ShutdownRequest::Unknown,
        }
    }

    /// The connectors across all agents, from the server's per-agent lists.
    pub async fn connector_count(&self) -> Option<u32> {
        #[derive(Deserialize)]
        struct Agents {
            agents: Vec<AgentSummary>,
        }
        #[derive(Deserialize)]
        struct AgentSummary {
            state: AgentState,
        }
        #[derive(Deserialize)]
        struct AgentState {
            id: String,
        }
        #[derive(Deserialize)]
        struct Connectors {
            connectors: Vec<serde_json::Value>,
        }
        let agents: Agents = self.get_json("/api/agents?view=summary").await?;
        let mut total = 0u32;
        for agent in agents.agents {
            let path = format!("/api/agents/{}/connectors", encode_segment(&agent.state.id));
            let connectors: Connectors = self.get_json(&path).await?;
            total = total.saturating_add(connectors.connectors.len() as u32);
        }
        Some(total)
    }
}

/// Percent-encodes everything but unreserved characters, for one path segment.
fn encode_segment(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::encode_segment;

    #[test]
    fn path_segments_are_encoded() {
        assert_eq!(encode_segment("agent-1_x.y~"), "agent-1_x.y~");
        assert_eq!(encode_segment("a/b c"), "a%2Fb%20c");
    }
}
