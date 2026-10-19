//! The macOS binary carries an Info.plist (see `build.rs`), so `codesign` gives it the
//! identifier `dev.animaos.anima` rather than one derived from the file name. Signs a
//! copy ad hoc in a temporary folder; no keychain is touched.

#![cfg(target_os = "macos")]

use std::process::Command;

#[test]
fn codesign_identifier_is_dev_animaos_anima() {
    let dir = tempfile::tempdir().unwrap();
    let copy = dir.path().join("anima");
    std::fs::copy(env!("CARGO_BIN_EXE_anima"), &copy).unwrap();

    let sign = Command::new("/usr/bin/codesign")
        .args(["--force", "--sign", "-"])
        .arg(&copy)
        .output()
        .unwrap();
    assert!(
        sign.status.success(),
        "codesign: {}",
        String::from_utf8_lossy(&sign.stderr)
    );

    let display = Command::new("/usr/bin/codesign")
        .arg("-dv")
        .arg(&copy)
        .output()
        .unwrap();
    let details = String::from_utf8_lossy(&display.stderr);
    assert!(
        details
            .lines()
            .any(|line| line == "Identifier=dev.animaos.anima"),
        "{details}"
    );
}
