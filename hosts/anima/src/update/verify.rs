//! Proving an archive is ours, and taking the binary out of it.

use std::io::{Cursor, Read};

use minisign_verify::{PublicKey, Signature};
use semver::Version;
use sha2::{Digest, Sha256};

use super::UpdateError;

/// The minisign public key releases are signed with. While this is the committed
/// placeholder, [`release_key_configured`] is false and nothing verifies.
pub const RELEASE_PUBLIC_KEY: &str = include_str!("../../release-key.pub");

/// The largest binary we take out of an archive (a guard against decompression bombs).
const MAX_BINARY_BYTES: u64 = 512 * 1024 * 1024;

/// Whether this build carries a usable release key.
pub fn release_key_configured() -> bool {
    PublicKey::decode(RELEASE_PUBLIC_KEY).is_ok()
}

/// The trusted comment a release signature must carry, binding it to one release:
///
/// ```text
/// anima {version} {target} file:{asset_name}
/// ```
///
/// `version` has no leading `v`, e.g.
/// `anima 0.2.0 aarch64-apple-darwin file:anima-aarch64-apple-darwin.tar.gz`. Sign with
/// `minisign -S -t "<that comment>" -m <archive>`. The trusted comment is covered by the
/// signature, so a genuinely signed older archive cannot be passed off as a newer
/// release or as another platform's build.
pub fn release_trusted_comment(version: &Version, target: &str, asset_name: &str) -> String {
    format!("anima {version} {target} file:{asset_name}")
}

/// Accept `archive` only if all of these pass:
/// 1. it matches `sha256_file` (cargo-dist's `<hex>  <name>`), else [`UpdateError::Checksum`];
/// 2. `minisig` is a signature over it by `public_key`, else [`UpdateError::Signature`];
/// 3. that signature's trusted comment is exactly `expected_trusted_comment` (build it
///    with [`release_trusted_comment`]), else [`UpdateError::ReleaseMismatch`].
pub fn verify(
    archive: &[u8],
    sha256_file: &str,
    minisig: &str,
    public_key: &str,
    expected_trusted_comment: &str,
) -> Result<(), UpdateError> {
    let public_key = PublicKey::decode(public_key).map_err(|_| UpdateError::NotConfigured)?;

    let expected = sha256_file
        .split_whitespace()
        .next()
        .filter(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or(UpdateError::Checksum)?
        .to_ascii_lowercase();
    let actual: String = Sha256::digest(archive)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    if actual != expected {
        return Err(UpdateError::Checksum);
    }

    let signature = Signature::decode(minisig).map_err(|_| UpdateError::Signature)?;
    // Only prehashed signatures, the minisign default; legacy ones are refused.
    public_key
        .verify(archive, &signature, false)
        .map_err(|_| UpdateError::Signature)?;
    // Checked only once the signature (which covers the trusted comment) is genuine.
    let signed = signature.trusted_comment();
    if signed != expected_trusted_comment {
        return Err(UpdateError::ReleaseMismatch {
            expected: expected_trusted_comment.to_owned(),
            signed: signed.to_owned(),
        });
    }
    Ok(())
}

/// Take the `anima` (`anima.exe` on Windows) binary out of a release archive: a
/// `.zip` for Windows targets, a `.tar.gz` otherwise. The binary may sit in a
/// top-level folder. Any entry with an absolute or `..` path spoils the archive.
pub fn extract_binary(archive: &[u8], target: &str) -> Result<Vec<u8>, UpdateError> {
    if target.contains("windows") {
        extract_from_zip(archive, "anima.exe")
    } else {
        extract_from_tar_gz(archive, "anima")
    }
}

fn extract_from_tar_gz(archive: &[u8], binary_name: &str) -> Result<Vec<u8>, UpdateError> {
    let unreadable = |error: std::io::Error| UpdateError::Archive(error.to_string());
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(archive));
    let mut found = None;
    for entry in tar.entries().map_err(unreadable)? {
        let mut entry = entry.map_err(unreadable)?;
        let path = String::from_utf8_lossy(&entry.path_bytes()).into_owned();
        let name = entry_file_name(&path)?;
        if name == Some(binary_name) && entry.header().entry_type().is_file() {
            if found.is_some() {
                return Err(duplicate(binary_name));
            }
            found = Some(read_capped(&mut entry)?);
        }
    }
    found.ok_or_else(|| missing(binary_name))
}

fn extract_from_zip(archive: &[u8], binary_name: &str) -> Result<Vec<u8>, UpdateError> {
    let unreadable = |error: zip::result::ZipError| UpdateError::Archive(error.to_string());
    let mut zip = zip::ZipArchive::new(Cursor::new(archive)).map_err(unreadable)?;
    let mut found = None;
    for index in 0..zip.len() {
        let mut file = zip.by_index(index).map_err(unreadable)?;
        let name = entry_file_name(file.name())?;
        if name == Some(binary_name) && file.is_file() {
            if found.is_some() {
                return Err(duplicate(binary_name));
            }
            found = Some(read_capped(&mut file)?);
        }
    }
    found.ok_or_else(|| missing(binary_name))
}

/// The last component of an archive path, refusing paths that could leave the archive.
fn entry_file_name(path: &str) -> Result<Option<&str>, UpdateError> {
    let absolute = path.starts_with(['/', '\\']) || path.as_bytes().get(1) == Some(&b':');
    let mut components = path.split(['/', '\\']).filter(|part| !part.is_empty());
    if absolute || components.clone().any(|part| part == "..") {
        return Err(UpdateError::Archive(format!("unsafe path {path:?}")));
    }
    Ok(components.next_back())
}

fn read_capped(reader: &mut impl Read) -> Result<Vec<u8>, UpdateError> {
    let mut bytes = Vec::new();
    reader
        .take(MAX_BINARY_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| UpdateError::Archive(error.to_string()))?;
    if bytes.len() as u64 > MAX_BINARY_BYTES {
        return Err(UpdateError::TooLarge {
            limit: MAX_BINARY_BYTES,
        });
    }
    Ok(bytes)
}

fn missing(binary_name: &str) -> UpdateError {
    UpdateError::Archive(format!("no {binary_name} binary inside"))
}

fn duplicate(binary_name: &str) -> UpdateError {
    UpdateError::Archive(format!("more than one {binary_name} binary inside"))
}
