//! `anima setup --yes` against the real binary, in a temporary HOME whose
//! `config.toml` sets free ports. Autostart and the browser are switched off,
//! so nothing outside the temporary folder is touched.

use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn refuses_connections(port: u16) -> bool {
    TcpStream::connect(("127.0.0.1", port)).is_err()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// A temporary HOME with its own ports. Dropping it quits whatever supervisor
/// setup started (which stops its server), and shuts the server down directly
/// if one is left, so no process outlives the test.
struct Home {
    dir: tempfile::TempDir,
    server_port: u16,
    control_port: u16,
    client: reqwest::blocking::Client,
}

impl Home {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let (server_port, control_port) = (free_port(), free_port());
        let root = dir.path().join(".animaos-swarm");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("config.toml"),
            format!(
                "port = {server_port}\ncontrol_port = {control_port}\n[updates]\ncheck = false\n"
            ),
        )
        .unwrap();
        Self {
            dir,
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
        self.dir.path().join(".animaos-swarm")
    }

    fn workspace(&self) -> PathBuf {
        self.dir.path().join("ws")
    }

    /// `anima <args>` without the caller's Anima or provider-key variables.
    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_anima"));
        command.args(args);
        for (key, _) in std::env::vars_os() {
            let key = key.to_string_lossy().into_owned();
            if key.starts_with("ANIMA") {
                command.env_remove(key);
            }
        }
        for (_, name) in anima_daemon::provider_env_key_names() {
            command.env_remove(name);
        }
        command
            .env("HOME", self.dir.path())
            .env("USERPROFILE", self.dir.path())
            .env("ANIMAOS_RS_KEYCHAIN_NAMESPACE", "test")
            .env("ANIMA_NO_TRAY", "1")
            .env("ANIMA_SETUP_NO_AUTOSTART", "1")
            .env("ANIMA_SETUP_NO_OPEN", "1")
            .stdin(Stdio::null());
        command
    }

    fn setup(&self, extra: &[&str]) -> Output {
        self.setup_with(extra, &[])
    }

    fn setup_with(&self, extra: &[&str], env: &[(&str, &str)]) -> Output {
        let workspace = self.workspace();
        let mut args = vec!["setup", "--yes", "--workspace", workspace.to_str().unwrap()];
        args.extend_from_slice(extra);
        let mut command = self.command(&args);
        for (key, value) in env {
            command.env(key, value);
        }
        command.output().unwrap()
    }

    fn token(&self, name: &str) -> Option<String> {
        std::fs::read_to_string(self.root().join(name))
            .ok()
            .map(|token| token.trim().to_string())
            .filter(|token| !token.is_empty())
    }

    fn config(&self) -> toml::Table {
        std::fs::read_to_string(self.root().join("config.toml"))
            .unwrap()
            .parse()
            .unwrap()
    }

    fn server_get(&self, path: &str) -> Value {
        let response = self
            .client
            .get(format!("http://127.0.0.1:{}{path}", self.server_port))
            .bearer_auth(self.token("admin-token").expect("admin-token"))
            .send()
            .unwrap();
        assert!(
            response.status().is_success(),
            "{path}: {}",
            response.status()
        );
        response.json().unwrap()
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        if let Some(token) = self.token("control-token") {
            let _ = self
                .client
                .post(format!(
                    "http://127.0.0.1:{}/v1/actions/quit",
                    self.control_port
                ))
                .bearer_auth(token)
                .json(&json!({ "force": true }))
                .send();
        }
        let deadline = Instant::now() + Duration::from_secs(60);
        while Instant::now() < deadline && !refuses_connections(self.control_port) {
            thread::sleep(Duration::from_millis(100));
        }
        if !refuses_connections(self.server_port) {
            if let Some(token) = self.token("admin-token") {
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

fn expect_success(output: &Output) {
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout: {}\nstderr: {}",
        text(&output.stdout),
        text(&output.stderr)
    );
}

#[test]
fn setup_yes_writes_config_and_tokens() {
    let home = Home::new();
    let output = home.setup(&[]);
    expect_success(&output);

    let config = home.config();
    assert_eq!(
        config["workspace"].as_str(),
        Some(home.workspace().to_str().unwrap())
    );
    assert_eq!(
        config["port"].as_integer(),
        Some(i64::from(home.server_port))
    );
    // Written explicitly: with the OS change skipped, nothing is installed.
    assert_eq!(config["autostart"].as_bool(), Some(false));
    assert!(home.workspace().is_dir());

    let (control, admin) = (
        home.token("control-token").expect("control-token"),
        home.token("admin-token").expect("admin-token"),
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for name in ["control-token", "admin-token"] {
            let mode = std::fs::metadata(home.root().join(name))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600, "{name}");
        }
    }

    // Anima is running, with the workspace.
    let status = home.command(&["status"]).output().unwrap();
    expect_success(&status);
    assert!(
        text(&status.stdout).contains(home.workspace().to_str().unwrap()),
        "{}",
        text(&status.stdout)
    );

    // Running it again keeps the tokens and the answers.
    expect_success(&home.setup(&[]));
    assert_eq!(
        home.token("control-token").as_deref(),
        Some(control.as_str())
    );
    assert_eq!(home.token("admin-token").as_deref(), Some(admin.as_str()));
    assert_eq!(home.config(), config);
}

#[test]
fn setup_yes_rejects_home_workspace() {
    let home = Home::new();
    let output = home
        .command(&[
            "setup",
            "--yes",
            "--workspace",
            home.dir.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2), "{}", text(&output.stderr));
    assert!(
        text(&output.stderr).contains("home folder"),
        "{}",
        text(&output.stderr)
    );
    // Nothing was started or saved.
    assert!(refuses_connections(home.control_port));
    assert_eq!(home.config().get("workspace"), None);
}

#[test]
fn setup_imports_provider_key() {
    let home = Home::new();
    let output = home.setup_with(&[], &[("OPENAI_API_KEY", "sk-test-123")]);
    expect_success(&output);
    // The key never appears in what setup prints.
    for stream in [&output.stdout, &output.stderr] {
        assert!(!text(stream).contains("sk-test-123"), "{}", text(stream));
    }
    assert!(text(&output.stdout).contains("OPENAI_API_KEY"));
    let note = "Imported. Note: memory embeddings, speech-to-text and coding-agent key \
                passthrough still read the environment variable; set it in your login \
                environment if you use them.";
    assert_eq!(
        text(&output.stdout).matches(note).count(),
        1,
        "{}",
        text(&output.stdout)
    );

    let listed = home.server_get("/api/provider-keys");
    let openai = listed["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|provider| provider["id"] == "openai")
        .unwrap_or_else(|| panic!("no openai in {listed}"));
    let keys = openai["keys"].as_array().unwrap();
    assert!(
        keys.iter()
            .any(|key| key["label"] == "Imported from shell" && key["source"] != "env"),
        "{openai}"
    );
    // The key is stored by Anima, not just seen in the shell's environment.
    let stored = std::fs::read_to_string(home.root().join("data").join("provider-keys.json"))
        .expect("provider-keys.json");
    assert!(stored.contains("sk-test-123"));
}

#[test]
fn setup_reimporting_a_stored_key_is_not_an_error() {
    let home = Home::new();
    let env = [("OPENAI_API_KEY", "sk-test-456")];
    expect_success(&home.setup_with(&[], &env));
    let again = home.setup_with(&[], &env);
    expect_success(&again);
    // Nothing new was imported, so no note.
    assert!(
        !text(&again.stdout).contains("Imported. Note:"),
        "{}",
        text(&again.stdout)
    );
    let stderr = text(&again.stderr);
    assert!(!stderr.contains("sk-test-456"), "{stderr}");
    let listed = home.server_get("/api/provider-keys");
    let imported = listed["providers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|provider| provider["id"] == "openai")
        .flat_map(|provider| provider["keys"].as_array().unwrap())
        .filter(|key| key["label"] == "Imported from shell")
        .count();
    assert_eq!(imported, 1);
}
