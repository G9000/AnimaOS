//! Desktop entry-point checks with a dedicated Anima profile on every OS.

mod common;

use std::fs;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

struct Profile {
    dir: common::PrivateHome,
    port: u16,
}

impl Profile {
    fn new() -> Self {
        let dir = common::PrivateHome::new().unwrap();
        let port = free_port();
        let control = free_port();
        let profile = Self { dir, port };
        fs::create_dir_all(profile.root()).unwrap();
        fs::write(
            profile.root().join("config.toml"),
            format!("port = {port}\ncontrol_port = {control}\nautostart = false\n[updates]\ncheck = false\n"),
        )
        .unwrap();
        profile
    }

    fn root(&self) -> PathBuf {
        self.dir.path().join(".animaos-swarm")
    }

    fn command(&self, arguments: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_anima"));
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("ANIMA") {
                command.env_remove(key);
            }
        }
        for (_, variable) in anima_daemon::provider_env_key_names() {
            command.env_remove(variable);
        }
        command
            .args(arguments)
            .env_remove("DATABASE_URL")
            .env("ANIMA_HOME", self.dir.path())
            .env("ANIMAOS_RS_KEYCHAIN_NAMESPACE", "test")
            .env("ANIMAOS_RS_MEMORY_EMBEDDINGS", "disabled")
            .env("ANIMA_NO_TRAY", "1")
            .env("ANIMA_NO_OPEN", "1")
            .env("ANIMA_NO_DIALOG", "1")
            .stdin(Stdio::null());
        command
    }

    fn run(&self, arguments: &[&str]) -> Output {
        common::output(&mut self.command(arguments), self.dir.path()).unwrap()
    }
}

impl Drop for Profile {
    fn drop(&mut self) {
        if self.root().join("control-token").is_file() {
            let _ = self.run(&["quit", "--now"]);
        }
    }
}

fn success(output: &Output) {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn explicit_profile_home_is_used_on_every_platform() {
    let profile = Profile::new();
    let output = profile.run(&["config", "get", "port"]);
    success(&output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        profile.port.to_string()
    );
}

#[test]
fn invalid_profile_and_corrupt_config_are_reported_without_resetting_data() {
    let profile = Profile::new();
    let invalid = profile
        .command(&["config", "get", "port"])
        .env("ANIMA_HOME", "relative-profile")
        .output()
        .unwrap();
    assert!(!invalid.status.success());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("absolute profile"));

    let config = profile.root().join("config.toml");
    fs::write(&config, "port = 'not a number'\n").unwrap();
    let output = profile.run(&[]);
    assert!(!output.status.success());
    assert_eq!(
        fs::read_to_string(config).unwrap(),
        "port = 'not a number'\n"
    );
    assert!(!profile.root().join("admin-token").exists());
}

#[test]
fn an_alternate_profile_cannot_change_normal_login_startup() {
    let profile = Profile::new();
    let config = fs::read(profile.root().join("config.toml")).unwrap();
    for mode in ["on", "off"] {
        let output = profile.run(&["autostart", mode]);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("alternate ANIMA_HOME profile"));
    }
    assert_eq!(
        fs::read(profile.root().join("config.toml")).unwrap(),
        config
    );
}

#[test]
fn an_occupied_server_port_is_reported_as_a_startup_failure() {
    let profile = Profile::new();
    let _occupied = TcpListener::bind(("127.0.0.1", profile.port)).unwrap();
    let output = profile.run(&[]);
    assert!(!output.status.success());
    assert!(!output.stderr.is_empty());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("Opening"));
}

#[test]
fn no_argument_launch_prepares_onboarding_without_terminal_setup() {
    let profile = Profile::new();
    let output = profile.run(&[]);
    success(&output);
    let config: toml::Value =
        toml::from_str(&fs::read_to_string(profile.root().join("config.toml")).unwrap()).unwrap();
    let workspace = PathBuf::from(config["workspace"].as_str().unwrap());
    assert_eq!(workspace, profile.dir.path().join("anima-workspace"));
    assert!(workspace.is_dir());
    assert_eq!(config["autostart"].as_bool(), Some(false));
    assert!(profile.root().join("admin-token").is_file());
    success(&profile.run(&["status"]));

    let config_before = fs::read(profile.root().join("config.toml")).unwrap();
    let token_before = fs::read(profile.root().join("admin-token")).unwrap();
    fs::write(workspace.join("keep.txt"), "keep this workspace").unwrap();
    success(&profile.run(&[]));
    assert_eq!(
        fs::read(profile.root().join("config.toml")).unwrap(),
        config_before
    );
    assert_eq!(
        fs::read(profile.root().join("admin-token")).unwrap(),
        token_before
    );
    assert_eq!(
        fs::read_to_string(workspace.join("keep.txt")).unwrap(),
        "keep this workspace"
    );
}
