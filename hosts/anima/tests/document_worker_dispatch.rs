//! Worker dispatch must precede normal supervisor/profile/UI startup.
mod common;
use std::process::{Command, Stdio};

fn command(arguments: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_anima"));
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("ANIMA") {
            command.env_remove(key);
        }
    }
    command
        .args(arguments)
        .env("ANIMA_HOME", "invalid-relative-profile")
        .env_remove("DATABASE_URL")
        .stdin(Stdio::null());
    command
}

#[test]
fn packaged_document_edit_worker_is_dispatched_before_profile_boot() {
    let scratch = tempfile::tempdir().unwrap();
    let output =
        common::output(&mut command(&["--document-edit-worker-v1"]), scratch.path()).unwrap();
    assert!(
        !output.status.success(),
        "empty worker framing must be refused"
    );
    #[cfg(windows)]
    assert!(
        output.stdout.starts_with(b"ANIMA-DOCUMENT-EDIT-READY-v1\n"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    #[cfg(not(windows))]
    assert!(String::from_utf8_lossy(&output.stderr)
        .contains("document edit worker requires Windows Job limits"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("absolute profile"));
    for args in [
        vec!["--document-edit-worker-v999"],
        vec!["--document-edit-worker-v1", "status"],
    ] {
        let output = common::output(&mut command(&args), scratch.path()).unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr)
            .contains("document edit worker requires exactly --document-edit-worker-v1"));
    }
}
