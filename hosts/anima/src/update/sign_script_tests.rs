//! The release workflow signs archives with `tools/release/sign-assets.sh`; these tests
//! prove its signatures pass [`verify`]. The round trip shells out to a real minisign
//! CLI, so it is ignored in the normal suite. Run it with `minisign` on `PATH` (or
//! `MINISIGN` naming a compatible binary):
//!
//! ```text
//! cargo test -p anima sign_script -- --include-ignored
//! ```
//!
//! The keypair is a throwaway generated per run, never the release key.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use semver::Version;
use sha2::{Digest, Sha256};

use super::{release_trusted_comment, verify, UpdateError};

const PASSWORD: &str = "throwaway test password";
const MAC_ARCHIVE: &str = "anima-aarch64-apple-darwin.tar.gz";
const WINDOWS_ARCHIVE: &str = "anima-x86_64-pc-windows-msvc.zip";

fn script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/release/sign-assets.sh")
}

fn run_script(args: &[&Path]) -> Output {
    Command::new("bash")
        .arg(script())
        .args(args)
        .env("MINISIGN_PASSWORD", PASSWORD)
        .output()
        .expect("run bash")
}

fn sha256_file(bytes: &[u8], name: &str) -> String {
    let hex: String = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("{hex}  {name}\n")
}

#[test]
#[ignore = "needs a minisign CLI; see tools/release/README.md"]
fn sign_script_signatures_pass_verify() {
    let dir = tempfile::tempdir().unwrap();
    let pair = minisign::KeyPair::generate_encrypted_keypair(Some(PASSWORD.into())).unwrap();
    let public_key = pair.pk.to_box().unwrap().into_string();
    let key_file = dir.path().join("release.key");
    std::fs::write(&key_file, pair.sk.to_box(None).unwrap().into_string()).unwrap();

    let archives = [
        (
            MAC_ARCHIVE,
            "aarch64-apple-darwin",
            b"mac archive bytes".as_slice(),
        ),
        (
            WINDOWS_ARCHIVE,
            "x86_64-pc-windows-msvc",
            b"windows archive bytes".as_slice(),
        ),
    ];
    let paths: Vec<PathBuf> = archives
        .iter()
        .map(|(name, _, bytes)| {
            let path = dir.path().join(name);
            std::fs::write(&path, bytes).unwrap();
            path
        })
        .collect();

    let mut args = vec![Path::new("0.2.0"), key_file.as_path()];
    args.extend(paths.iter().map(PathBuf::as_path));
    let output = run_script(&args);
    assert!(
        output.status.success(),
        "sign-assets.sh failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let version = Version::parse("0.2.0").unwrap();
    let signature_of =
        |name: &str| std::fs::read_to_string(dir.path().join(format!("{name}.minisig"))).unwrap();
    for (name, target, bytes) in archives {
        verify(
            bytes,
            &sha256_file(bytes, name),
            &signature_of(name),
            &public_key,
            &release_trusted_comment(&version, target, name),
        )
        .unwrap_or_else(|error| panic!("{name}: {error:?}"));
    }

    // Each signature names its own archive: the mac one is no use for Windows.
    let (name, _, bytes) = archives[0];
    let wrong = verify(
        bytes,
        &sha256_file(bytes, name),
        &signature_of(name),
        &public_key,
        &release_trusted_comment(&version, "x86_64-pc-windows-msvc", WINDOWS_ARCHIVE),
    );
    assert!(
        matches!(wrong, Err(UpdateError::ReleaseMismatch { .. })),
        "{wrong:?}"
    );
}

/// Bad input is refused before minisign runs, so this needs no minisign CLI.
#[cfg(unix)]
#[test]
fn sign_script_refuses_bad_input() {
    let dir = tempfile::tempdir().unwrap();
    let key_file = dir.path().join("release.key");
    std::fs::write(&key_file, "not used").unwrap();
    let archive = dir.path().join(MAC_ARCHIVE);
    std::fs::write(&archive, "bytes").unwrap();
    let installer = dir.path().join("anima-installer.sh");
    std::fs::write(&installer, "bytes").unwrap();

    let refused = |args: &[&Path], why: &str| {
        let output = run_script(args);
        assert!(!output.status.success(), "{why}: accepted");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("sign-assets:"), "{why}: {stderr}");
    };
    // The tag, not the version: the updater compares against "0.2.0".
    refused(&[Path::new("v0.2.0"), &key_file, &archive], "leading v");
    refused(&[Path::new("0.2"), &key_file, &archive], "not semver");
    // Every name is checked before anything is signed.
    refused(
        &[Path::new("0.2.0"), &key_file, &archive, &installer],
        "not an archive",
    );
    refused(&[Path::new("0.2.0"), &key_file], "no archives");
    assert!(!dir.path().join(format!("{MAC_ARCHIVE}.minisig")).exists());
}
