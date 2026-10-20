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
    anima_command(home, &["supervise", "--no-tray"])
}

fn anima_command(home: &Path, args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_anima"));
    command.args(args);
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

fn spawn_supervisor(home: &Path) -> Child {
    supervise_command(home)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
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

    /// `extra_config` may set `workspace`; otherwise a `ws` folder in the
    /// home is the workspace (the supervisor refuses to run without one).
    fn start_in(home: tempfile::TempDir, server_port: u16, extra_config: &str) -> Self {
        let control_port = free_port();
        let root = home.path().join(".animaos-swarm");
        std::fs::create_dir_all(&root).unwrap();
        let workspace = if extra_config.contains("workspace =") {
            String::new()
        } else {
            let workspace = home.path().join("ws");
            std::fs::create_dir_all(&workspace).unwrap();
            format!("workspace = {:?}\n", workspace.to_string_lossy())
        };
        std::fs::write(
            root.join("config.toml"),
            format!("port = {server_port}\ncontrol_port = {control_port}\nautostart = false\n{workspace}{extra_config}\n[updates]\ncheck = false\n"),
        )
        .unwrap();
        Self {
            child: spawn_supervisor(home.path()),
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

    /// A new supervisor process in the same home, after the last one exited.
    fn respawn(&mut self) {
        assert!(
            self.child.try_wait().unwrap().is_some(),
            "the previous supervisor is still running"
        );
        self.child = spawn_supervisor(self.home.path());
    }

    /// The server's own `GET /api/system/drain`.
    fn server_drain(&self) -> Value {
        self.server_call(reqwest::Method::GET, "/api/system/drain")
            .json()
            .unwrap()
    }

    fn server_call(&self, method: reqwest::Method, path: &str) -> reqwest::blocking::Response {
        self.client
            .request(
                method,
                format!("http://127.0.0.1:{}{path}", self.server_port),
            )
            .bearer_auth(self.read_token("admin-token").expect("admin-token"))
            .send()
            .unwrap()
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

#[test]
fn file_workspace_is_reported_and_still_runs() {
    let home = tempfile::tempdir().unwrap();
    let file = home.path().join("afile");
    std::fs::write(&file, "x").unwrap();
    let config = format!("workspace = {:?}\n", file.to_string_lossy());
    let supervisor = Supervisor::start_in(home, free_port(), &config);
    let status = supervisor.wait_for_kind("running");
    let problem = status["workspaceProblem"].as_str().unwrap_or_default();
    assert!(
        problem.starts_with("Workspace path is not a folder"),
        "workspaceProblem: {problem:?}"
    );
}

#[test]
fn home_folder_workspace_is_refused_without_starting_the_server() {
    let home = tempfile::tempdir().unwrap();
    let config = format!("workspace = {:?}\n", home.path().to_string_lossy());
    let supervisor = Supervisor::start_in(home, free_port(), &config);
    let status = supervisor.wait_for_kind("stopped");
    assert_eq!(
        status["workspaceProblem"], "The workspace can't be your home folder",
        "{status}"
    );
    assert_eq!(status["pending"], Value::Null);
    assert!(refuses_connections(supervisor.server_port));
    let log = std::fs::read_to_string(supervisor.root().join("logs").join("anima.log")).unwrap();
    assert!(
        log.contains("Not starting the server: The workspace can't be your home folder"),
        "log:\n{log}"
    );
    assert!(!log.contains("Starting the server"), "log:\n{log}");

    // Asking again does not start it either.
    let (code, body) = supervisor.action("start", false);
    assert_eq!(code, 202, "{body}");
    supervisor.wait_for_kind("stopped");
    assert!(refuses_connections(supervisor.server_port));
}

#[test]
fn supervise_without_a_workspace_refuses() {
    let home = tempfile::tempdir().unwrap();
    let control_port = free_port();
    let root = home.path().join(".animaos-swarm");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("config.toml"),
        format!("control_port = {control_port}\n[updates]\ncheck = false\n"),
    )
    .unwrap();
    // A supervisor that does not refuse would run until it is killed.
    let mut child = KillOnDrop(
        supervise_command(home.path())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "the supervisor did not refuse");
        thread::sleep(Duration::from_millis(100));
    };
    let mut stderr = String::new();
    child
        .0
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    assert_eq!(status.code(), Some(2));
    assert_eq!(
        stderr.trim(),
        "Anima isn't set up yet. Run `anima setup` first."
    );
    // It never took the control port, so nothing was left running.
    assert!(refuses_connections(control_port));
    assert!(!root.join("control-token").exists());
}

#[test]
fn cancel_ends_the_server_drain() {
    let supervisor = Supervisor::start("");
    supervisor.wait_for_kind("running");

    // Cancel while the first drain call is still in flight.
    let (code, body) = supervisor.action("stop", false);
    assert_eq!(code, 202, "{body}");
    let (code, body) = supervisor.action("cancel", false);
    assert_eq!(code, 202, "{body}");
    supervisor.wait_for("the cancel to be confirmed", |status| {
        status["state"]["kind"] == "running" && status["pending"].is_null()
    });
    assert_eq!(supervisor.server_drain()["draining"], false);

    // A drain left on the server while running is ended by a cancel too.
    let begun = supervisor.server_call(reqwest::Method::POST, "/api/system/drain");
    assert_eq!(begun.status().as_u16(), 202);
    assert_eq!(supervisor.server_drain()["draining"], true);
    let (code, body) = supervisor.action("cancel", false);
    assert_eq!(code, 202, "{body}");
    assert_eq!(body["pending"], "cancel", "{body}");
    supervisor.wait_for("the cancel to be confirmed", |status| {
        status["state"]["kind"] == "running" && status["pending"].is_null()
    });
    assert_eq!(supervisor.server_drain()["draining"], false);
}

/// Kills a process by pid when dropped, in case the test fails before it
/// exits on its own.
#[cfg(unix)]
struct KillPidOnDrop(u32);

#[cfg(unix)]
impl Drop for KillPidOnDrop {
    fn drop(&mut self) {
        let _ = Command::new("kill")
            .args(["-9", &self.0.to_string()])
            .stderr(Stdio::null())
            .status();
    }
}

#[cfg(unix)]
fn process_alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[cfg(unix)]
#[test]
fn server_exits_when_its_supervisor_is_killed() {
    let mut supervisor = Supervisor::start("");
    supervisor.wait_for_kind("running");
    let children = Command::new("pgrep")
        .args(["-P", &supervisor.child.id().to_string()])
        .output()
        .unwrap();
    let server: u32 = String::from_utf8_lossy(&children.stdout)
        .lines()
        .next()
        .and_then(|line| line.trim().parse().ok())
        .expect("the supervisor has a server child");
    let _server_guard = KillPidOnDrop(server);

    // SIGKILL: the supervisor gets no chance to stop its server.
    supervisor.child.kill().unwrap();
    supervisor.child.wait().unwrap();

    let deadline = Instant::now() + Duration::from_secs(45);
    while process_alive(server) {
        assert!(
            Instant::now() < deadline,
            "the server outlived its supervisor"
        );
        thread::sleep(Duration::from_millis(200));
    }
    assert!(refuses_connections(supervisor.server_port));

    supervisor.respawn();
    supervisor.wait_for_kind("running");
}

/// Kills a child when dropped.
struct KillOnDrop(Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn stale_server_on_the_port_is_shut_down_first() {
    let home = tempfile::tempdir().unwrap();
    let port = free_port();
    let root = home.path().join(".animaos-swarm");
    std::fs::create_dir_all(&root).unwrap();
    // A server from an earlier supervisor shares its admin token.
    std::fs::write(root.join("admin-token"), "stale-server-admin-token\n").unwrap();
    let mut stale = KillOnDrop(
        anima_command(home.path(), &["run"])
            .env("ANIMAOS_RS_PORT", port.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(2))
        .no_proxy()
        .build()
        .unwrap();
    let deadline = Instant::now() + STARTUP;
    while !client
        .get(format!("http://127.0.0.1:{port}/health"))
        .send()
        .is_ok_and(|response| response.status().is_success())
    {
        assert!(Instant::now() < deadline, "the stale server never started");
        thread::sleep(Duration::from_millis(100));
    }

    let supervisor = Supervisor::start_in(home, port, "");
    supervisor.wait_for_kind("running");
    let exited = stale.0.try_wait().unwrap();
    assert!(exited.is_some(), "the stale server is still running");
    // The server answering now is the supervisor's own: it relays to it.
    let relayed: Value = supervisor
        .server_call(reqwest::Method::GET, "/api/system")
        .json()
        .unwrap();
    assert_eq!(relayed["state"]["kind"], "running", "{relayed}");
}
