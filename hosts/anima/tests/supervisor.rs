//! Spawns the real `anima supervise --no-tray` against a temporary HOME, with
//! its ports set in that home's `config.toml`.

use std::io::Read;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const STARTUP: Duration = Duration::from_secs(90);

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// `anima supervise --no-tray` for `home`, without the caller's Anima
/// variables and never touching the real keychain items.
fn supervise_command(home: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_anima"));
    command.args(["supervise", "--no-tray"]);
    for (key, _) in std::env::vars_os() {
        let key = key.to_string_lossy().into_owned();
        if key.starts_with("ANIMA") {
            command.env_remove(key);
        }
    }
    command
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("ANIMAOS_RS_KEYCHAIN_NAMESPACE", "test")
        .stdin(Stdio::null());
    command
}

/// A supervisor under test. Dropping it quits the supervisor (which stops
/// its server), and kills it if it does not exit, so no process outlives
/// the test.
struct Supervisor {
    child: Child,
    home: tempfile::TempDir,
    server_port: u16,
    control_port: u16,
    client: reqwest::blocking::Client,
}

impl Supervisor {
    fn start(extra_config: &str) -> Self {
        let home = tempfile::tempdir().unwrap();
        Self::start_in(home, free_port(), extra_config)
    }

    fn start_in(home: tempfile::TempDir, server_port: u16, extra_config: &str) -> Self {
        let control_port = free_port();
        let root = home.path().join(".animaos-swarm");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("config.toml"),
            format!("port = {server_port}\ncontrol_port = {control_port}\nautostart = false\n{extra_config}\n[updates]\ncheck = false\n"),
        )
        .unwrap();
        let child = supervise_command(home.path())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        Self {
            child,
            home,
            server_port,
            control_port,
            client: reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(10))
                .no_proxy()
                .build()
                .unwrap(),
        }
    }

    fn root(&self) -> PathBuf {
        self.home.path().join(".animaos-swarm")
    }

    fn read_token(&self, name: &str) -> Option<String> {
        std::fs::read_to_string(self.root().join(name))
            .ok()
            .map(|token| token.trim().to_string())
            .filter(|token| !token.is_empty())
    }

    fn token(&self) -> String {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(token) = self.read_token("control-token") {
                return token;
            }
            assert!(Instant::now() < deadline, "no control-token was written");
            thread::sleep(Duration::from_millis(100));
        }
    }

    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.control_port)
    }

    fn status(&self) -> Option<Value> {
        let response = self
            .client
            .get(self.url("/v1/status"))
            .bearer_auth(self.token())
            .send()
            .ok()?;
        assert_eq!(response.status().as_u16(), 200);
        response.json().ok()
    }

    fn action(&self, action: &str, force: bool) -> (u16, Value) {
        let response = self
            .client
            .post(self.url(&format!("/v1/actions/{action}")))
            .bearer_auth(self.token())
            .json(&json!({ "force": force }))
            .send()
            .unwrap();
        let status = response.status().as_u16();
        (status, response.json().unwrap())
    }

    /// Polls the status until `done` holds for it.
    fn wait_for(&self, what: &str, done: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + STARTUP;
        let mut last = None;
        loop {
            if let Some(status) = self.status() {
                if done(&status) {
                    return status;
                }
                last = Some(status);
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {what}; last status: {last:?}"
            );
            thread::sleep(Duration::from_millis(200));
        }
    }

    fn wait_for_kind(&self, kind: &str) -> Value {
        self.wait_for(kind, |status| status["state"]["kind"] == kind)
    }
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        if let Some(token) = self.read_token("control-token") {
            let _ = self
                .client
                .post(self.url("/v1/actions/quit"))
                .bearer_auth(token)
                .json(&json!({ "force": true }))
                .send();
        }
        let deadline = Instant::now() + Duration::from_secs(60);
        while Instant::now() < deadline {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            thread::sleep(Duration::from_millis(100));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn refuses_connections(port: u16) -> bool {
    TcpStream::connect(("127.0.0.1", port)).is_err()
}

#[test]
fn supervise_starts_child_and_reports_running() {
    let supervisor = Supervisor::start("");
    let status = supervisor.wait_for_kind("running");
    assert_eq!(
        status["serverUrl"],
        format!("http://127.0.0.1:{}", supervisor.server_port)
    );
    assert_eq!(status["pending"], Value::Null);
    assert_eq!(status["autostart"], false);
    assert_eq!(status["workspaceProblem"], Value::Null);
    assert!(status["version"].is_string());
    let data_dir = status["dataDir"].as_str().unwrap();
    assert!(Path::new(data_dir).ends_with(Path::new(".animaos-swarm").join("data")));
    // The connector count comes from the server once it has been read.
    let status = supervisor.wait_for("a connector count", |status| {
        status["connectors"].is_number()
    });
    assert_eq!(status["connectors"], 0);

    // The server relays the console's status read to this supervisor.
    let admin = supervisor.read_token("admin-token").expect("admin-token");
    let relayed: Value = supervisor
        .client
        .get(format!(
            "http://127.0.0.1:{}/api/system",
            supervisor.server_port
        ))
        .bearer_auth(admin)
        .send()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(relayed["state"]["kind"], "running", "{relayed}");

    // The server's output goes to the log.
    let log = std::fs::read_to_string(supervisor.root().join("logs").join("anima.log")).unwrap();
    assert!(log.contains("anima-daemon listening"), "log:\n{log}");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for name in ["control-token", "admin-token"] {
            let mode = std::fs::metadata(supervisor.root().join(name))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600, "{name}");
        }
    }
}

#[test]
fn missing_token_is_401() {
    let supervisor = Supervisor::start("");
    let token = supervisor.token();
    assert_eq!(token.len(), 64);
    assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
    let deadline = Instant::now() + STARTUP;
    let probe = loop {
        if let Ok(response) = supervisor.client.get(supervisor.url("/v1/status")).send() {
            break response;
        }
        assert!(Instant::now() < deadline, "control API never answered");
        thread::sleep(Duration::from_millis(100));
    };
    assert_eq!(probe.status().as_u16(), 401);
    let wrong = supervisor
        .client
        .get(supervisor.url("/v1/status"))
        .bearer_auth("not-the-token")
        .send()
        .unwrap();
    assert_eq!(wrong.status().as_u16(), 401);
    let action = supervisor
        .client
        .post(supervisor.url("/v1/actions/stop"))
        .json(&json!({}))
        .send()
        .unwrap();
    assert_eq!(action.status().as_u16(), 401);
    // Nothing was stopped.
    supervisor.wait_for_kind("running");
}

#[test]
fn second_supervisor_exits_already_running() {
    let supervisor = Supervisor::start("");
    supervisor.wait_for_kind("running");
    let mut second = supervise_command(supervisor.home.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    let code = loop {
        if let Some(status) = second.try_wait().unwrap() {
            break status.code();
        }
        if Instant::now() > deadline {
            let _ = second.kill();
            let _ = second.wait();
            panic!("the second supervisor did not exit");
        }
        thread::sleep(Duration::from_millis(100));
    };
    let mut stdout = String::new();
    second
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut stdout)
        .unwrap();
    assert_eq!(code, Some(0));
    assert!(
        stdout.contains("Anima is already running"),
        "stdout: {stdout}"
    );
    // The first one is unaffected.
    supervisor.wait_for_kind("running");
}

#[test]
fn stop_then_start() {
    let supervisor = Supervisor::start("");
    supervisor.wait_for_kind("running");

    let (code, body) = supervisor.action("stop", false);
    assert_eq!(code, 202, "{body}");
    assert_eq!(body["pending"], "stop", "{body}");
    // The drain takes at least a second, so the stop is still pending.
    let (code, conflict) = supervisor.action("restart", false);
    assert_eq!(code, 409, "{conflict}");
    assert_eq!(
        conflict,
        json!({"error": "Another action is in progress", "pending": "stop"})
    );
    let stopped = supervisor.wait_for_kind("stopped");
    assert_eq!(stopped["pending"], Value::Null);
    assert!(
        refuses_connections(supervisor.server_port),
        "the server port still accepts connections"
    );

    let (code, body) = supervisor.action("start", false);
    assert_eq!(code, 202, "{body}");
    supervisor.wait_for_kind("running");
    assert!(!refuses_connections(supervisor.server_port));
}

#[test]
fn server_port_in_use_reports_crash() {
    let occupied = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = occupied.local_addr().unwrap().port();
    let supervisor = Supervisor::start_in(tempfile::tempdir().unwrap(), port, "");
    let message = format!("Port {port} is in use");
    let crashed = |status: &Value| {
        status["state"]["kind"] == "crashed" && status["state"]["message"] == message.as_str()
    };
    let first = supervisor.wait_for("a port-in-use crash", crashed);
    thread::sleep(Duration::from_secs(1));
    let second = supervisor.wait_for("a port-in-use crash", crashed);
    let retry = |status: &Value| status["state"]["retryInSecs"].as_u64().unwrap();
    assert!(
        retry(&second) >= retry(&first),
        "retryInSecs went from {} to {}",
        retry(&first),
        retry(&second)
    );
    drop(occupied);
}

#[test]
fn missing_workspace_still_runs() {
    let home = tempfile::tempdir().unwrap();
    let missing = home.path().join("no-such-workspace");
    let config = format!("workspace = {:?}\n", missing.to_string_lossy());
    let supervisor = Supervisor::start_in(home, free_port(), &config);
    let status = supervisor.wait_for_kind("running");
    let problem = status["workspaceProblem"].as_str().unwrap_or_default();
    assert!(
        problem.starts_with("Workspace folder not found"),
        "workspaceProblem: {problem:?}"
    );
    assert_eq!(status["workspace"], missing.to_string_lossy().as_ref());
}
