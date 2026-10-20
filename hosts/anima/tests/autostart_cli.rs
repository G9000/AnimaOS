//! `anima autostart on|off` against a temporary HOME. `ANIMA_AUTOSTART_DRY_RUN`
//! keeps it away from the real launchd.
#![cfg(target_os = "macos")]

use std::path::Path;
use std::process::{Command, Output};

fn anima(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_anima"))
        .args(args)
        .env("HOME", home)
        .env("ANIMA_HOME", home)
        .env("ANIMA_AUTOSTART_DRY_RUN", "1")
        .output()
        .unwrap()
}

fn config(home: &Path) -> String {
    std::fs::read_to_string(home.join(".animaos-swarm").join("config.toml")).unwrap_or_default()
}

#[test]
fn autostart_on_and_off_install_and_remove_the_launch_agent() {
    let home = tempfile::tempdir().unwrap();
    let plist = home
        .path()
        .join("Library")
        .join("LaunchAgents")
        .join("dev.animaos.anima.plist");

    let on = anima(home.path(), &["autostart", "on"]);
    assert!(
        on.status.success(),
        "{}",
        String::from_utf8_lossy(&on.stderr)
    );
    let text = std::fs::read_to_string(&plist).unwrap();
    assert!(text.contains("<string>supervise</string>"));
    assert!(text.contains("<string>--background</string>"));
    assert!(text.contains(
        &home
            .path()
            .join(".animaos-swarm/logs/supervisor.log")
            .to_string_lossy()
            .into_owned()
    ));
    assert!(config(home.path()).contains("autostart = true"));

    let off = anima(home.path(), &["autostart", "off"]);
    assert!(
        off.status.success(),
        "{}",
        String::from_utf8_lossy(&off.stderr)
    );
    assert!(!plist.exists());
    assert!(config(home.path()).contains("autostart = false"));

    // Off again is not an error.
    assert!(anima(home.path(), &["autostart", "off"]).status.success());
}
