//! Update tests. Nothing here touches the network or the installed binary: releases come
//! from JSON fixtures or a local server, and every signature is made with a throwaway
//! test keypair generated per run.

use std::io::{Cursor, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};

use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect};
use axum::routing::get;
use axum::Router;
use semver::Version;
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::*;

const MAC: &str = "aarch64-apple-darwin";
const WINDOWS: &str = "x86_64-pc-windows-msvc";
const LATEST: &str = include_str!("../../tests/fixtures/update/release-latest.json");
const RELEASE_BASE: &str = "https://github.com/G9000/animaOS-SWARM/releases/download/v0.2.0";

fn version(text: &str) -> Version {
    Version::parse(text).unwrap()
}

fn latest_with(edit: impl FnOnce(&mut Value)) -> String {
    let mut json: Value = serde_json::from_str(LATEST).unwrap();
    edit(&mut json);
    json.to_string()
}

fn without_asset(name: &str) -> String {
    latest_with(|json| {
        json["assets"]
            .as_array_mut()
            .unwrap()
            .retain(|asset| asset["name"] != name)
    })
}

// --- select_release ------------------------------------------------------------------

#[test]
fn newer_release_selected_for_target() {
    let mac = select_release(LATEST, &version("0.1.0"), MAC).expect("mac update offered");
    assert_eq!(
        mac,
        ReleaseInfo {
            version: version("0.2.0"),
            target: MAC.to_owned(),
            archive_name: format!("anima-{MAC}.tar.gz"),
            archive_url: format!("{RELEASE_BASE}/anima-{MAC}.tar.gz"),
            sha256_url: format!("{RELEASE_BASE}/anima-{MAC}.tar.gz.sha256"),
            sig_url: format!("{RELEASE_BASE}/anima-{MAC}.tar.gz.minisig"),
        }
    );

    let windows = select_release(LATEST, &version("0.1.0"), WINDOWS).expect("windows update");
    assert_eq!(
        windows.archive_url,
        format!("{RELEASE_BASE}/anima-{WINDOWS}.zip")
    );
    assert_eq!(
        windows.sig_url,
        format!("{RELEASE_BASE}/anima-{WINDOWS}.zip.minisig")
    );
}

#[test]
fn same_version_not_offered() {
    assert_eq!(select_release(LATEST, &version("0.2.0"), MAC), None);
    assert_eq!(select_release(LATEST, &version("0.3.0"), MAC), None);
}

#[test]
fn prerelease_ignored() {
    let flagged = latest_with(|json| json["prerelease"] = true.into());
    assert_eq!(select_release(&flagged, &version("0.1.0"), MAC), None);

    let draft = latest_with(|json| json["draft"] = true.into());
    assert_eq!(select_release(&draft, &version("0.1.0"), MAC), None);

    let pre_tag = latest_with(|json| json["tag_name"] = "v0.3.0-rc.1".into());
    assert_eq!(select_release(&pre_tag, &version("0.1.0"), MAC), None);
}

#[test]
fn missing_sig_asset_not_offered() {
    let json = without_asset(&format!("anima-{MAC}.tar.gz.minisig"));
    assert_eq!(select_release(&json, &version("0.1.0"), MAC), None);
    // The other platform is still complete.
    assert!(select_release(&json, &version("0.1.0"), WINDOWS).is_some());
}

#[test]
fn missing_checksum_asset_not_offered() {
    let json = without_asset(&format!("anima-{WINDOWS}.zip.sha256"));
    assert_eq!(select_release(&json, &version("0.1.0"), WINDOWS), None);
}

#[test]
fn missing_target_asset_not_offered() {
    let json = without_asset(&format!("anima-{MAC}.tar.gz"));
    assert_eq!(select_release(&json, &version("0.1.0"), MAC), None);
    assert_eq!(
        select_release(LATEST, &version("0.1.0"), "x86_64-unknown-linux-gnu"),
        None
    );
}

#[test]
fn non_https_asset_not_offered() {
    let json = latest_with(|json| {
        for asset in json["assets"].as_array_mut().unwrap() {
            let url = asset["browser_download_url"].as_str().unwrap().to_owned();
            asset["browser_download_url"] = url.replacen("https://", "http://", 1).into();
        }
    });
    assert_eq!(select_release(&json, &version("0.1.0"), MAC), None);
}

#[test]
fn unreadable_release_not_offered() {
    assert_eq!(select_release("not json", &version("0.1.0"), MAC), None);
    let bad_tag = latest_with(|json| json["tag_name"] = "latest".into());
    assert_eq!(select_release(&bad_tag, &version("0.1.0"), MAC), None);
}

// --- verify --------------------------------------------------------------------------

struct TestKey {
    public: String,
    secret: minisign::SecretKey,
    public_key: minisign::PublicKey,
}

/// A fresh, password-less TEST keypair. Never the release key.
fn test_key() -> TestKey {
    let pair = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
    TestKey {
        public: pair.pk.to_box().unwrap().into_string(),
        secret: pair.sk,
        public_key: pair.pk,
    }
}

/// The trusted comment for v0.2.0 of the mac archive (the `local_release` below).
const COMMENT: &str = "anima 0.2.0 aarch64-apple-darwin file:anima-aarch64-apple-darwin.tar.gz";

impl TestKey {
    /// Signed for v0.2.0 of the mac archive.
    fn sign(&self, bytes: &[u8]) -> String {
        self.sign_as(bytes, COMMENT)
    }

    fn sign_as(&self, bytes: &[u8], trusted_comment: &str) -> String {
        minisign::sign(
            Some(&self.public_key),
            &self.secret,
            Cursor::new(bytes),
            Some(trusted_comment),
            None,
        )
        .unwrap()
        .into_string()
    }
}

/// The checksum file in cargo-dist's `<hex>  <filename>` shape.
fn sha256_file(bytes: &[u8], name: &str) -> String {
    let hex: String = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("{hex}  {name}\n")
}

#[test]
fn verify_accepts_good_signature() {
    let key = test_key();
    let archive = b"pretend archive bytes".to_vec();
    let checksum = sha256_file(&archive, "anima-aarch64-apple-darwin.tar.gz");
    verify(
        &archive,
        &checksum,
        &key.sign(&archive),
        &key.public,
        COMMENT,
    )
    .unwrap();
    // Upper-case hex is the same digest.
    verify(
        &archive,
        &checksum.to_uppercase(),
        &key.sign(&archive),
        &key.public,
        COMMENT,
    )
    .unwrap();
}

#[test]
fn verify_rejects_tampered_archive() {
    let key = test_key();
    let archive = b"pretend archive bytes".to_vec();
    let checksum = sha256_file(&archive, "anima.tar.gz");
    let signature = key.sign(&archive);

    let mut tampered = archive.clone();
    tampered[3] ^= 0x01;
    assert!(matches!(
        verify(&tampered, &checksum, &signature, &key.public, COMMENT),
        Err(UpdateError::Checksum)
    ));

    // A checksum that matches the tampered bytes still fails on the signature.
    let forged = sha256_file(&tampered, "anima.tar.gz");
    assert!(matches!(
        verify(&tampered, &forged, &signature, &key.public, COMMENT),
        Err(UpdateError::Signature)
    ));
}

#[test]
fn verify_rejects_wrong_key() {
    let signer = test_key();
    let other = test_key();
    let archive = b"pretend archive bytes".to_vec();
    let checksum = sha256_file(&archive, "anima.tar.gz");
    assert!(matches!(
        verify(
            &archive,
            &checksum,
            &signer.sign(&archive),
            &other.public,
            COMMENT
        ),
        Err(UpdateError::Signature)
    ));
}

#[test]
fn release_trusted_comment_format() {
    assert_eq!(
        release_trusted_comment(&version("0.2.0"), MAC, "anima-aarch64-apple-darwin.tar.gz"),
        COMMENT
    );
}

/// A genuine signature only counts for the release its trusted comment names, so an
/// older signed archive re-published under a newer tag (or another platform's, or
/// another file) is refused.
#[test]
fn verify_binds_the_signature_to_the_release() {
    let key = test_key();
    let archive = b"pretend archive bytes".to_vec();
    let checksum = sha256_file(&archive, "anima-aarch64-apple-darwin.tar.gz");
    let signed = |comment: &str| {
        verify(
            &archive,
            &checksum,
            &key.sign_as(&archive, comment),
            &key.public,
            COMMENT,
        )
    };

    signed(COMMENT).unwrap();
    for other in [
        // An older version re-tagged as 0.2.0.
        "anima 0.1.0 aarch64-apple-darwin file:anima-aarch64-apple-darwin.tar.gz",
        "anima v0.2.0 aarch64-apple-darwin file:anima-aarch64-apple-darwin.tar.gz",
        // Another target.
        "anima 0.2.0 x86_64-pc-windows-msvc file:anima-aarch64-apple-darwin.tar.gz",
        // Another file.
        "anima 0.2.0 aarch64-apple-darwin file:anima-x86_64-pc-windows-msvc.zip",
        // The minisign default and near misses.
        "timestamp:1760000000\tfile:anima-aarch64-apple-darwin.tar.gz",
        "anima 0.2.0 aarch64-apple-darwin file:anima-aarch64-apple-darwin.tar.gz ",
        "",
    ] {
        match signed(other) {
            Err(UpdateError::ReleaseMismatch { expected, signed }) => {
                assert_eq!(expected, COMMENT);
                assert_eq!(signed, other);
            }
            outcome => panic!("comment {other:?} gave {outcome:?}"),
        }
    }
}

#[test]
fn verify_rejects_unusable_checksum_and_signature_files() {
    let key = test_key();
    let archive = b"pretend archive bytes".to_vec();
    let signature = key.sign(&archive);
    for checksum in ["", "   ", "abc123  anima.tar.gz", "zz  anima.tar.gz"] {
        assert!(
            matches!(
                verify(&archive, checksum, &signature, &key.public, COMMENT),
                Err(UpdateError::Checksum)
            ),
            "checksum file {checksum:?}"
        );
    }
    let checksum = sha256_file(&archive, "anima.tar.gz");
    assert!(matches!(
        verify(&archive, &checksum, "", &key.public, COMMENT),
        Err(UpdateError::Signature)
    ));
    assert!(matches!(
        verify(
            &archive,
            &checksum,
            "untrusted comment: x\nnot base64",
            &key.public,
            COMMENT
        ),
        Err(UpdateError::Signature)
    ));
}

#[test]
fn placeholder_key_disables_updates() {
    let placeholder = RELEASE_PUBLIC_KEY.contains("PLACEHOLDER");
    assert_eq!(release_key_configured(), !placeholder);
    if !placeholder {
        return;
    }
    // Nothing verifies against the placeholder, however well signed.
    let key = test_key();
    let archive = b"pretend archive bytes".to_vec();
    let checksum = sha256_file(&archive, "anima.tar.gz");
    assert!(matches!(
        verify(
            &archive,
            &checksum,
            &key.sign(&archive),
            RELEASE_PUBLIC_KEY,
            COMMENT
        ),
        Err(UpdateError::NotConfigured)
    ));
    // And `check` stops before any request.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let outcome = runtime
        .block_on(async { check(&http_client().unwrap(), REPO, &version("0.1.0"), MAC).await });
    assert!(matches!(outcome, Err(UpdateError::NotConfigured)));
    assert_eq!(
        UpdateError::NotConfigured.to_string(),
        "Updates are not configured for this build"
    );
}

// --- extract_binary ------------------------------------------------------------------

enum TarEntry<'a> {
    File(&'a str, &'a [u8]),
    /// A name written straight into the header, bypassing `tar`'s own path checks.
    RawFile(&'a str, &'a [u8]),
    Symlink(&'a str, &'a str),
}

fn tar_gz(entries: &[TarEntry]) -> Vec<u8> {
    let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    let mut builder = tar::Builder::new(encoder);
    for entry in entries {
        let mut header = tar::Header::new_gnu();
        header.set_mode(0o755);
        match entry {
            TarEntry::File(name, data) => {
                header.set_size(data.len() as u64);
                builder.append_data(&mut header, name, *data).unwrap();
            }
            TarEntry::RawFile(name, data) => {
                header.set_size(data.len() as u64);
                header.as_old_mut().name[..name.len()].copy_from_slice(name.as_bytes());
                header.set_cksum();
                builder.append(&header, *data).unwrap();
            }
            TarEntry::Symlink(name, target) => {
                header.set_entry_type(tar::EntryType::Symlink);
                header.set_size(0);
                builder.append_link(&mut header, name, target).unwrap();
            }
        }
    }
    builder.into_inner().unwrap().finish().unwrap()
}

fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, data) in entries {
        writer.start_file(*name, options).unwrap();
        writer.write_all(data).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

#[test]
fn extract_finds_binary_in_tar_gz_and_zip() {
    let binary = b"\x7fELF new anima".as_slice();

    // cargo-dist nests the Unix archive in a folder named after it.
    let nested = tar_gz(&[
        TarEntry::File("anima-aarch64-apple-darwin/README.md", b"readme"),
        TarEntry::File("anima-aarch64-apple-darwin/anima", binary),
        TarEntry::File("anima-aarch64-apple-darwin/LICENSE", b"license"),
    ]);
    assert_eq!(extract_binary(&nested, MAC).unwrap(), binary);

    let flat = tar_gz(&[TarEntry::File("anima", binary)]);
    assert_eq!(extract_binary(&flat, MAC).unwrap(), binary);

    let windows = zip(&[
        ("README.md", b"readme"),
        ("anima.exe", binary),
        ("anima.pdb", b"symbols"),
    ]);
    assert_eq!(extract_binary(&windows, WINDOWS).unwrap(), binary);

    let windows_nested = zip(&[("anima-x86_64-pc-windows-msvc/anima.exe", binary)]);
    assert_eq!(extract_binary(&windows_nested, WINDOWS).unwrap(), binary);
}

#[test]
fn extract_requires_the_exact_binary_name() {
    let near_misses = tar_gz(&[
        TarEntry::File("dir/anima.exe", b"wrong platform"),
        TarEntry::File("dir/anima-helper", b"not it"),
        TarEntry::File("anima/README.md", b"a folder named anima"),
    ]);
    assert!(matches!(
        extract_binary(&near_misses, MAC),
        Err(UpdateError::Archive(_))
    ));

    let unix_name_in_zip = zip(&[("anima", b"unix binary")]);
    assert!(matches!(
        extract_binary(&unix_name_in_zip, WINDOWS),
        Err(UpdateError::Archive(_))
    ));
}

#[test]
fn extract_skips_symlinks_named_like_the_binary() {
    let archive = tar_gz(&[TarEntry::Symlink("dir/anima", "/bin/sh")]);
    assert!(matches!(
        extract_binary(&archive, MAC),
        Err(UpdateError::Archive(_))
    ));
}

#[test]
fn extract_rejects_unsafe_paths() {
    for name in ["../anima", "dir/../../anima", "/usr/local/bin/anima"] {
        let archive = tar_gz(&[TarEntry::RawFile(name, b"escape")]);
        assert!(
            matches!(extract_binary(&archive, MAC), Err(UpdateError::Archive(_))),
            "tar entry {name}"
        );
    }
    for name in [
        "../anima.exe",
        "C:/Windows/anima.exe",
        "\\anima.exe",
        "a\\..\\..\\anima.exe",
    ] {
        let archive = zip(&[(name, b"escape")]);
        assert!(
            matches!(
                extract_binary(&archive, WINDOWS),
                Err(UpdateError::Archive(_))
            ),
            "zip entry {name}"
        );
    }
    // An unsafe entry spoils the archive even next to a good binary.
    let mixed = tar_gz(&[
        TarEntry::File("dir/anima", b"good"),
        TarEntry::RawFile("../evil", b"escape"),
    ]);
    assert!(matches!(
        extract_binary(&mixed, MAC),
        Err(UpdateError::Archive(_))
    ));
}

#[test]
fn extract_rejects_garbage() {
    assert!(matches!(
        extract_binary(b"not an archive", MAC),
        Err(UpdateError::Archive(_))
    ));
    assert!(matches!(
        extract_binary(b"not an archive", WINDOWS),
        Err(UpdateError::Archive(_))
    ));
}

// --- swap / rollback -----------------------------------------------------------------

fn dummy_exe(dir: &Path, bytes: &[u8]) -> std::path::PathBuf {
    let path = dir.join(if cfg!(windows) { "anima.exe" } else { "anima" });
    std::fs::write(&path, bytes).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    path
}

#[cfg(unix)]
fn mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn swap_keeps_old_and_rollback_restores() {
    let dir = tempfile::tempdir().unwrap();
    let exe = dummy_exe(dir.path(), b"old anima");

    let exe_name = exe.file_name().unwrap().to_string_lossy().into_owned();

    let old = swap_at(b"new anima", &exe).unwrap();
    assert_eq!(old, exe.with_file_name(format!("{exe_name}.old")));
    assert_eq!(std::fs::read(&exe).unwrap(), b"new anima");
    assert_eq!(std::fs::read(&old).unwrap(), b"old anima");
    #[cfg(unix)]
    assert_eq!(mode(&exe), 0o755);

    // Only the binary and its `.old` copy remain: no temp files left behind.
    let mut names: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, [exe_name.clone(), format!("{exe_name}.old")]);

    rollback(&exe, &old).unwrap();
    assert_eq!(std::fs::read(&exe).unwrap(), b"old anima");
    assert!(!old.exists());
    #[cfg(unix)]
    assert_eq!(mode(&exe), 0o755);
}

#[test]
fn swap_overwrites_a_previous_old_copy() {
    let dir = tempfile::tempdir().unwrap();
    let exe = dummy_exe(dir.path(), b"v1");
    swap_at(b"v2", &exe).unwrap();
    let old = swap_at(b"v3", &exe).unwrap();
    assert_eq!(std::fs::read(&exe).unwrap(), b"v3");
    assert_eq!(std::fs::read(&old).unwrap(), b"v2");
}

#[cfg(unix)]
#[test]
fn swap_sets_the_exec_bit() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let exe = dummy_exe(dir.path(), b"old");
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o600)).unwrap();
    swap_at(b"new", &exe).unwrap();
    assert_eq!(mode(&exe) & 0o100, 0o100);
}

#[test]
fn swap_of_a_missing_target_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("anima");
    assert!(matches!(swap_at(b"new", &missing), Err(UpdateError::Io(_))));
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

// --- check / download against a local server ----------------------------------------

#[derive(Clone, Default)]
struct Served {
    latest: Option<(StatusCode, String)>,
    archive: Vec<u8>,
    sha256: String,
    minisig: String,
    user_agents: Arc<Mutex<Vec<String>>>,
    accepts: Arc<Mutex<Vec<String>>>,
}

async fn serve(served: Served) -> String {
    async fn latest(State(served): State<Served>, headers: HeaderMap) -> impl IntoResponse {
        let text = |name| {
            headers
                .get(name)
                .map(|value: &axum::http::HeaderValue| value.to_str().unwrap().to_owned())
                .unwrap_or_default()
        };
        served
            .user_agents
            .lock()
            .unwrap()
            .push(text(header::USER_AGENT));
        served.accepts.lock().unwrap().push(text(header::ACCEPT));
        served.latest.clone().unwrap()
    }
    let app = Router::new()
        .route("/repos/G9000/animaOS-SWARM/releases/latest", get(latest))
        .route(
            "/archive",
            get(|State(s): State<Served>| async move { s.archive }),
        )
        .route(
            "/archive.sha256",
            get(|State(s): State<Served>| async move { s.sha256 }),
        )
        .route(
            "/archive.minisig",
            get(|State(s): State<Served>| async move { s.minisig }),
        )
        .route(
            "/redirect-to-http",
            get(|headers: HeaderMap| async move {
                let host = headers[header::HOST].to_str().unwrap().to_owned();
                Redirect::temporary(&format!("http://{host}/archive"))
            }),
        )
        .with_state(served);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    base
}

#[tokio::test]
async fn check_at_reads_the_latest_release() {
    let served = Served {
        latest: Some((StatusCode::OK, LATEST.to_owned())),
        ..Served::default()
    };
    let base = serve(served.clone()).await;
    let found = check_at(&http_client().unwrap(), &base, REPO, &version("0.1.0"), MAC)
        .await
        .unwrap();
    assert_eq!(found.unwrap().version, version("0.2.0"));
    assert_eq!(served.user_agents.lock().unwrap()[0], "anima/0.1.0");
    assert_eq!(
        served.accepts.lock().unwrap()[0],
        "application/vnd.github+json"
    );

    let current = check_at(&http_client().unwrap(), &base, REPO, &version("0.2.0"), MAC)
        .await
        .unwrap();
    assert_eq!(current, None);
}

#[tokio::test]
async fn check_at_without_any_release_offers_nothing() {
    let base = serve(Served {
        latest: Some((
            StatusCode::NOT_FOUND,
            r#"{"message":"Not Found"}"#.to_owned(),
        )),
        ..Served::default()
    })
    .await;
    let found = check_at(&http_client().unwrap(), &base, REPO, &version("0.1.0"), MAC).await;
    assert!(matches!(found, Ok(None)));
}

#[tokio::test]
async fn check_at_reports_server_errors() {
    let base = serve(Served {
        latest: Some((StatusCode::FORBIDDEN, "rate limited".to_owned())),
        ..Served::default()
    })
    .await;
    let found = check_at(&http_client().unwrap(), &base, REPO, &version("0.1.0"), MAC).await;
    assert!(matches!(found, Err(UpdateError::Http(_))), "{found:?}");

    // Nothing listening at all.
    let closed = check_at(
        &http_client().unwrap(),
        "http://127.0.0.1:9",
        REPO,
        &version("0.1.0"),
        MAC,
    )
    .await;
    assert!(matches!(closed, Err(UpdateError::Http(_))), "{closed:?}");
}

fn local_release(base: &str) -> ReleaseInfo {
    ReleaseInfo {
        version: version("0.2.0"),
        target: MAC.to_owned(),
        archive_name: format!("anima-{MAC}.tar.gz"),
        archive_url: format!("{base}/archive"),
        sha256_url: format!("{base}/archive.sha256"),
        sig_url: format!("{base}/archive.minisig"),
    }
}

#[tokio::test]
async fn download_verified_returns_a_signed_archive() {
    let key = test_key();
    let archive = tar_gz(&[TarEntry::File("anima-aarch64-apple-darwin/anima", b"new")]);
    let base = serve(Served {
        sha256: sha256_file(&archive, "anima-aarch64-apple-darwin.tar.gz"),
        minisig: key.sign(&archive),
        archive: archive.clone(),
        ..Served::default()
    })
    .await;
    let downloaded = download_verified(&http_client().unwrap(), &local_release(&base), &key.public)
        .await
        .unwrap();
    assert_eq!(downloaded, archive);
    assert_eq!(extract_binary(&downloaded, MAC).unwrap(), b"new");
}

#[tokio::test]
async fn download_verified_refuses_a_swapped_archive() {
    let key = test_key();
    let genuine = b"genuine archive".to_vec();
    let base = serve(Served {
        sha256: sha256_file(&genuine, "anima.tar.gz"),
        minisig: key.sign(&genuine),
        archive: b"swapped archive".to_vec(),
        ..Served::default()
    })
    .await;
    let outcome =
        download_verified(&http_client().unwrap(), &local_release(&base), &key.public).await;
    assert!(matches!(outcome, Err(UpdateError::Checksum)), "{outcome:?}");
}

#[tokio::test]
async fn download_verified_refuses_a_signature_for_another_release() {
    let key = test_key();
    let archive = b"genuinely signed 0.1.0 archive".to_vec();
    let base = serve(Served {
        sha256: sha256_file(&archive, "anima-aarch64-apple-darwin.tar.gz"),
        minisig: key.sign_as(
            &archive,
            "anima 0.1.0 aarch64-apple-darwin file:anima-aarch64-apple-darwin.tar.gz",
        ),
        archive,
        ..Served::default()
    })
    .await;
    // Offered as 0.2.0, so the 0.1.0 signature does not count.
    let outcome =
        download_verified(&http_client().unwrap(), &local_release(&base), &key.public).await;
    assert!(
        matches!(outcome, Err(UpdateError::ReleaseMismatch { .. })),
        "{outcome:?}"
    );
}

#[tokio::test]
async fn download_verified_refuses_another_signer() {
    let archive = b"archive".to_vec();
    let base = serve(Served {
        sha256: sha256_file(&archive, "anima.tar.gz"),
        minisig: test_key().sign(&archive),
        archive,
        ..Served::default()
    })
    .await;
    let outcome = download_verified(
        &http_client().unwrap(),
        &local_release(&base),
        &test_key().public,
    )
    .await;
    assert!(
        matches!(outcome, Err(UpdateError::Signature)),
        "{outcome:?}"
    );
}

#[tokio::test]
async fn download_caps_the_size() {
    let base = serve(Served {
        archive: vec![7; 4096],
        ..Served::default()
    })
    .await;
    let client = http_client().unwrap();
    let url = format!("{base}/archive");
    assert_eq!(download(&client, &url, 4096).await.unwrap().len(), 4096);
    assert!(matches!(
        download(&client, &url, 4095).await,
        Err(UpdateError::TooLarge { limit: 4095 })
    ));
}

#[tokio::test]
async fn download_refuses_redirects_to_plain_http() {
    let base = serve(Served {
        archive: b"bytes".to_vec(),
        ..Served::default()
    })
    .await;
    let outcome = download(
        &http_client().unwrap(),
        &format!("{base}/redirect-to-http"),
        MAX_ARCHIVE_BYTES,
    )
    .await;
    assert!(matches!(outcome, Err(UpdateError::Http(_))), "{outcome:?}");
}

#[test]
fn current_target_matches_release_assets() {
    if let Some(target) = current_target() {
        assert!(
            select_release(LATEST, &version("0.1.0"), target).is_some(),
            "{target} has no assets in the fixture"
        );
    }
}
