//! The `anima` client commands against a real supervisor, in a temporary HOME
//! whose `config.toml` sets free ports.

mod common;

use std::io::{BufRead, BufReader};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
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

/// `anima <args>` for `home`, without the caller's Anima variables and never
/// touching the real keychain items.
fn anima_command(executable: &Path, home: &Path, args: &[&str]) -> Command {
    let mut command = Command::new(executable);
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
        .env("ANIMA_HOME", home)
        .env("ANIMA_NO_OPEN", "1")
        .env("ANIMA_NO_DIALOG", "1")
        .env("ANIMAOS_RS_KEYCHAIN_NAMESPACE", "test")
        // A supervisor `anima start` spawns shows no tray icon.
        .env("ANIMA_NO_TRAY", "1")
        .stdin(Stdio::null());
    command
}

/// A temporary HOME with its own ports, and whatever supervisor runs in it.
/// Dropping it quits that supervisor (which stops its server), kills it if it
/// does not exit, and shuts the server down directly if one is left, so no
/// process outlives the test.
struct Home {
    dir: common::PrivateHome,
    executable: PathBuf,
    server_port: u16,
    control_port: u16,
    supervisor: Option<Child>,
    client: reqwest::blocking::Client,
}

impl Home {
    /// Set up, with a `ws` folder in the home as the workspace.
    fn new() -> Self {
        let home = Self::not_set_up();
        let workspace = home.workspace();
        std::fs::create_dir_all(&workspace).unwrap();
        let config = format!(
            "workspace = {:?}\n{}",
            workspace.to_string_lossy(),
            home.config_text()
        );
        std::fs::write(home.root().join("config.toml"), config).unwrap();
        home
    }

    /// Before `anima setup`: ports, but no workspace.
    fn not_set_up() -> Self {
        let dir = common::PrivateHome::new().unwrap();
        // Update recovery may replace the running binary. Never let one test
        // move or delete Cargo's shared executable, used by the other tests.
        let source = Path::new(env!("CARGO_BIN_EXE_anima"));
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let executable = bin.join(source.file_name().unwrap());
        std::fs::copy(source, &executable).unwrap();
        let (server_port, control_port) = (free_port(), free_port());
        let root = dir.path().join(".animaos-swarm");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("config.toml"),
            format!("port = {server_port}\ncontrol_port = {control_port}\nautostart = false\n[updates]\ncheck = false\n"),
        )
        .unwrap();
        Self {
            dir,
            executable,
            server_port,
            control_port,
            supervisor: None,
            client: reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(10))
                .no_proxy()
                .build()
                .unwrap(),
        }
    }

    fn root(&self) -> PathBuf {
        self.dir.path().join(".animaos-swarm")
    }

    fn workspace(&self) -> PathBuf {
        self.dir.path().join("ws")
    }

    fn anima(&self, args: &[&str]) -> Output {
        common::output(
            &mut anima_command(&self.executable, self.dir.path(), args),
            self.dir.path(),
        )
        .unwrap()
    }

    /// Starts `anima supervise --no-tray` and waits until the server runs.
    fn start_supervisor(&mut self) {
        self.supervisor = Some(
            anima_command(
                &self.executable,
                self.dir.path(),
                &["supervise", "--no-tray"],
            )
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
        );
        self.wait_for_kind("running");
    }

    fn read_token(&self, name: &str) -> Option<String> {
        std::fs::read_to_string(self.root().join(name))
            .ok()
            .map(|token| token.trim().to_string())
            .filter(|token| !token.is_empty())
    }

    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.control_port)
    }

    fn status(&self) -> Option<Value> {
        let token = self.read_token("control-token")?;
        self.client
            .get(self.url("/v1/status"))
            .bearer_auth(token)
            .send()
            .ok()?
            .json()
            .ok()
    }

    fn action(&self, action: &str, force: bool) -> u16 {
        self.client
            .post(self.url(&format!("/v1/actions/{action}")))
            .bearer_auth(self.read_token("control-token").expect("control-token"))
            .json(&json!({ "force": force }))
            .send()
            .unwrap()
            .status()
            .as_u16()
    }

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

    fn config_text(&self) -> String {
        std::fs::read_to_string(self.root().join("config.toml")).unwrap()
    }
}

impl Drop for Home {
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
        while Instant::now() < deadline && !refuses_connections(self.control_port) {
            thread::sleep(Duration::from_millis(100));
        }
        if let Some(mut supervisor) = self.supervisor.take() {
            let _ = supervisor.kill();
            let _ = supervisor.wait();
        }
        // A server the supervisor did not stop (or left behind when it was
        // killed) is asked to shut down; its own stdin watcher covers the rest.
        if !refuses_connections(self.server_port) {
            if let Some(token) = self.read_token("admin-token") {
                let _ = self
                    .client
                    .post(format!(
                        "http://127.0.0.1:{}/api/system/shutdown",
                        self.server_port
                    ))
                    .bearer_auth(token)
                    .send();
            }
            let deadline = Instant::now() + Duration::from_secs(30);
            while Instant::now() < deadline && !refuses_connections(self.server_port) {
                thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

fn refuses_connections(port: u16) -> bool {
    TcpStream::connect(("127.0.0.1", port)).is_err()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn status_prints_running_line() {
    let mut home = Home::new();
    home.start_supervisor();
    home.wait_for("a connector count", |status| {
        status["connectors"].is_number()
    });
    let output = home.anima(&["status"]);
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    let stdout = text(&output.stdout);
    let first = stdout.lines().next().unwrap();
    assert!(first.starts_with("Running · v"), "{stdout}");
    assert!(
        first.contains(&format!("http://127.0.0.1:{}", home.server_port)),
        "{stdout}"
    );
    assert!(first.ends_with("0 connectors"), "{stdout}");
    assert!(
        stdout.contains(&format!("Workspace: {}", home.workspace().display())),
        "{stdout}"
    );
}

#[test]
fn status_not_running_exit_3() {
    let home = Home::new();
    let output = home.anima(&["status"]);
    assert_eq!(output.status.code(), Some(3));
    assert!(
        text(&output.stdout).contains("Anima is not running"),
        "{}",
        text(&output.stdout)
    );
}

#[test]
fn stop_command_reaches_stopped() {
    let mut home = Home::new();
    home.start_supervisor();
    let output = home.anima(&["stop"]);
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert!(text(&output.stdout).contains("stopped"));
    // The command returned only once the state was `stopped`.
    let status = home.status().unwrap();
    assert_eq!(status["state"]["kind"], "stopped");
    assert!(refuses_connections(home.server_port));
    let stdout = text(&home.anima(&["status"]).stdout);
    assert!(stdout.starts_with("Stopped · v"), "{stdout}");
}

#[test]
fn start_command_starts_a_stopped_server() {
    let mut home = Home::new();
    home.start_supervisor();
    assert_eq!(home.anima(&["stop"]).status.code(), Some(0));
    let output = home.anima(&["start"]);
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert_eq!(home.status().unwrap()["state"]["kind"], "running");
    assert!(!refuses_connections(home.server_port));
}

#[test]
fn start_command_spawns_a_supervisor_when_none_runs() {
    let home = Home::new();
    let output = home.anima(&["start"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}\nserver log: {}",
        text(&output.stderr),
        std::fs::read_to_string(home.root().join("logs").join("anima.log")).unwrap_or_default()
    );
    assert_eq!(home.status().unwrap()["state"]["kind"], "running");
    // A second `start` finds it running and leaves it alone.
    let output = home.anima(&["start"]);
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert!(text(&output.stdout).contains("already running"));
    // Without a LaunchAgent, quitting ends the detached supervisor too.
    let output = home.anima(&["quit"]);
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert!(refuses_connections(home.control_port));
    assert!(refuses_connections(home.server_port));
}

#[test]
fn explicit_start_refuses_before_setup() {
    let home = Home::not_set_up();
    for args in [&["start"][..]] {
        let output = home.anima(args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert_eq!(
            text(&output.stderr).trim(),
            "Anima isn't set up yet. Run `anima setup` first.",
            "{args:?}"
        );
        assert!(text(&output.stdout).is_empty(), "{args:?}");
    }
    // Nothing was started, and nothing is left running.
    assert!(refuses_connections(home.control_port));
    assert!(refuses_connections(home.server_port));
    assert!(home.read_token("control-token").is_none());
}

#[test]
fn restart_command_returns_when_running_again() {
    let mut home = Home::new();
    home.start_supervisor();
    let output = home.anima(&["restart"]);
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert_eq!(home.status().unwrap()["state"]["kind"], "running");
    assert!(!refuses_connections(home.server_port));
}

#[test]
fn a_command_during_another_action_is_refused() {
    let mut home = Home::new();
    home.start_supervisor();
    assert_eq!(home.action("stop", false), 202);
    let output = home.anima(&["restart"]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = text(&output.stderr);
    assert!(stderr.contains("Another action is in progress"), "{stderr}");
    assert!(stderr.contains("stop"), "{stderr}");
    // Let the stop finish, so the cleanup's quit is not refused in turn.
    home.wait_for_kind("stopped");
}

#[test]
fn quit_now_stops_the_supervisor_and_its_server() {
    let mut home = Home::new();
    home.start_supervisor();
    let output = home.anima(&["quit", "--now"]);
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    let deadline = Instant::now() + Duration::from_secs(60);
    while !refuses_connections(home.control_port) {
        assert!(Instant::now() < deadline, "the supervisor did not exit");
        thread::sleep(Duration::from_millis(100));
    }
    assert!(refuses_connections(home.server_port));
}

#[test]
fn stop_when_not_running_exits_3() {
    let home = Home::new();
    let output = home.anima(&["stop"]);
    assert_eq!(output.status.code(), Some(3));
    assert!(text(&output.stdout).contains("Anima is not running"));
}

#[test]
fn config_set_workspace_rejects_home() {
    let home = Home::new();
    let before = home.config_text();
    let home_path = home.dir.path().to_string_lossy().into_owned();
    let output = home.anima(&["config", "set", "workspace", &home_path]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        text(&output.stderr).contains("The workspace can't be your home folder"),
        "{}",
        text(&output.stderr)
    );
    assert_eq!(home.config_text(), before);
}

#[test]
fn config_set_workspace_rejects_a_file() {
    let home = Home::new();
    let before = home.config_text();
    let file = home.dir.path().join("afile");
    std::fs::write(&file, "x").unwrap();
    let output = home.anima(&["config", "set", "workspace", file.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        text(&output.stderr).contains("Workspace path is not a folder"),
        "{}",
        text(&output.stderr)
    );
    assert_eq!(home.config_text(), before);
}

#[test]
fn config_set_workspace_accepts_a_folder() {
    let home = Home::new();
    let workspace = home.dir.path().join("anima-workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let output = home.anima(&["config", "set", "workspace", workspace.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    let got = home.anima(&["config", "get", "workspace"]);
    let got = text(&got.stdout);
    assert!(got.contains("anima-workspace"), "{got}");
}

#[test]
fn config_set_port_persists() {
    let home = Home::new();
    let output = home.anima(&["config", "set", "port", "9123"]);
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    // Not running, so no restart hint.
    assert!(!text(&output.stdout).contains("Restart Anima"));
    assert!(home.config_text().contains("port = 9123"));
    let got = home.anima(&["config", "get", "port"]);
    assert_eq!(text(&got.stdout).trim(), "9123");
    // The other settings are untouched.
    assert!(home
        .config_text()
        .contains(&format!("control_port = {}", home.control_port)));
}

#[test]
fn config_set_rejects_bad_values() {
    let home = Home::new();
    for args in [
        ["config", "set", "port", "not-a-number"],
        ["config", "set", "port", "0"],
        ["config", "set", "control-port", "70000"],
        ["config", "set", "updates.check", "maybe"],
        ["config", "set", "colour", "blue"],
    ] {
        let output = home.anima(&args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(!text(&output.stderr).is_empty(), "{args:?}");
    }
    // A port both listeners would need is refused.
    let same = home.server_port.to_string();
    let output = home.anima(&["config", "set", "control-port", &same]);
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn config_get_without_a_key_lists_everything() {
    let home = Home::new();
    let output = home.anima(&["config", "get"]);
    let stdout = text(&output.stdout);
    assert_eq!(output.status.code(), Some(0));
    assert!(stdout.contains(&format!("port = {}", home.server_port)));
    assert!(stdout.contains(&format!("control-port = {}", home.control_port)));
    assert!(stdout.contains("updates.check = false"));
    assert!(stdout.contains("workspace ="));
}

#[test]
fn config_set_while_running_says_to_restart() {
    let mut home = Home::new();
    home.start_supervisor();
    let output = home.anima(&["config", "set", "updates.check", "false"]);
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert!(
        text(&output.stdout).contains("Restart Anima for this to take effect"),
        "{}",
        text(&output.stdout)
    );
}

#[test]
fn logs_prints_the_last_lines() {
    let home = Home::new();
    let logs = home.root().join("logs");
    std::fs::create_dir_all(&logs).unwrap();
    let lines: Vec<String> = (0..250).map(|n| format!("line-{n:03}")).collect();
    std::fs::write(logs.join("anima.log"), lines.join("\n") + "\n").unwrap();
    let output = home.anima(&["logs"]);
    assert_eq!(output.status.code(), Some(0));
    let stdout = text(&output.stdout);
    let printed: Vec<&str> = stdout.lines().collect();
    assert_eq!(printed.len(), 200);
    assert_eq!(printed[0], "line-050");
    assert_eq!(printed[199], "line-249");
}

#[test]
fn logs_follow_prints_new_lines() {
    let home = Home::new();
    let logs = home.root().join("logs");
    std::fs::create_dir_all(&logs).unwrap();
    let file = logs.join("anima.log");
    std::fs::write(&file, "first\n").unwrap();
    let mut follower = anima_command(&home.executable, home.dir.path(), &["logs", "--follow"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(follower.stdout.take().unwrap()).lines();
    assert_eq!(lines.next().unwrap().unwrap(), "first");
    {
        use std::io::Write;
        let mut handle = std::fs::OpenOptions::new()
            .append(true)
            .open(&file)
            .unwrap();
        handle.write_all(b"second\n").unwrap();
    }
    let second = lines.next().unwrap().unwrap();
    let _ = follower.kill();
    let _ = follower.wait();
    assert_eq!(second, "second");
}

// --- updates (no network: periodic checks are off, and `update` only runs
// while the committed release key is the placeholder) --------------------------

#[test]
fn unavailable_update_keeps_the_server_running() {
    if !cfg!(feature = "native-install")
        && !include_str!("../release-key.pub").contains("PLACEHOLDER")
    {
        return;
    }
    let expected = if cfg!(feature = "native-install") {
        "This installation is managed by an installer"
    } else {
        "Updates are not configured for this build"
    };
    let mut home = Home::new();
    home.start_supervisor();
    let output = home.anima(&["update"]);
    assert_eq!(output.status.code(), Some(1), "{}", text(&output.stdout));
    assert!(
        text(&output.stderr).contains(expected),
        "{}",
        text(&output.stderr)
    );
    // Nothing was drained or stopped.
    let status = home.status().unwrap();
    assert_eq!(status["state"]["kind"], "running");
    assert_eq!(status["pending"], Value::Null);
    assert!(status["update"]["lastError"]
        .as_str()
        .unwrap()
        .contains(expected));
    assert!(!refuses_connections(home.server_port));
}

#[test]
fn a_pending_update_for_this_version_is_confirmed_once_running() {
    let mut home = Home::new();
    let pending = home.root().join("run").join("update-pending.json");
    std::fs::create_dir_all(pending.parent().unwrap()).unwrap();
    let old = home.root().join("anima.old");
    std::fs::copy(&home.executable, &old).unwrap();
    std::fs::write(
        &pending,
        json!({ "from": "0.0.1", "to": env!("CARGO_PKG_VERSION"), "old": old }).to_string(),
    )
    .unwrap();
    home.start_supervisor();
    if cfg!(feature = "native-install") {
        thread::sleep(Duration::from_millis(300));
        assert!(
            pending.exists(),
            "a native install consumed a portable update marker"
        );
        assert_eq!(home.status().unwrap()["update"]["lastError"], Value::Null);
        return;
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while pending.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(100));
    }
    assert!(!pending.exists(), "the pending update was not confirmed");
    assert_eq!(home.status().unwrap()["update"]["lastError"], Value::Null);
}

#[test]
fn a_rolled_back_update_is_reported() {
    let mut home = Home::new();
    let failed = home.root().join("run").join("update-failed.json");
    std::fs::create_dir_all(failed.parent().unwrap()).unwrap();
    std::fs::write(
        &failed,
        json!({ "to": "9.9.9", "error": "The server was not running within 60 seconds" })
            .to_string(),
    )
    .unwrap();
    home.start_supervisor();
    if cfg!(feature = "native-install") {
        assert_eq!(home.status().unwrap()["update"]["lastError"], Value::Null);
        assert!(
            failed.exists(),
            "the portable failure marker should remain untouched"
        );
        return;
    }
    assert_eq!(
        home.status().unwrap()["update"]["lastError"],
        "Update to v9.9.9 failed to start; rolled back"
    );
    assert!(!failed.exists());
}
