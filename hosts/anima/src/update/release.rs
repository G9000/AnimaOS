//! Finding a newer release on GitHub and downloading its files.

use std::time::Duration;

use reqwest::header::{ACCEPT, USER_AGENT};
use reqwest::{redirect, Client, StatusCode};
use semver::Version;
use serde::Deserialize;

use super::{extract_binary, release_trusted_comment, verify, UpdateError};

pub const GITHUB_API: &str = "https://api.github.com";
pub const REPO: &str = "G9000/animaOS-SWARM";
/// The largest release archive we download.
pub const MAX_ARCHIVE_BYTES: u64 = 200 * 1024 * 1024;
/// The largest API response, checksum or signature file we read.
const MAX_SMALL_BYTES: u64 = 4 * 1024 * 1024;
const CHECK_TIMEOUT: Duration = Duration::from_secs(15);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// A newer release whose archive, checksum and signature are all published for our target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseInfo {
    pub version: Version,
    /// The target this release was selected for.
    pub target: String,
    /// The archive's asset name, e.g. `anima-aarch64-apple-darwin.tar.gz`.
    pub archive_name: String,
    pub archive_url: String,
    pub sha256_url: String,
    pub sig_url: String,
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<GithubAsset>,
}

#[derive(Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

/// The release target this binary was built for, if releases are published for it.
pub fn current_target() -> Option<&'static str> {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some("aarch64-apple-darwin")
    } else if cfg!(all(windows, target_arch = "x86_64", target_env = "msvc")) {
        Some("x86_64-pc-windows-msvc")
    } else {
        None
    }
}

/// A client for update traffic: rustls, and redirects followed only to `https` URLs
/// (GitHub sends asset downloads to its CDN).
pub fn http_client() -> reqwest::Result<Client> {
    Client::builder()
        .use_rustls_tls()
        .user_agent(concat!("anima/", env!("CARGO_PKG_VERSION")))
        .redirect(redirect::Policy::custom(|attempt| {
            if attempt.url().scheme() != "https" {
                attempt.error("refusing to follow a redirect to a non-https URL")
            } else if attempt.previous().len() >= 10 {
                attempt.error("too many redirects")
            } else {
                attempt.follow()
            }
        }))
        .build()
}

/// The release in a GitHub `releases/latest` response, if it is a stable release newer
/// than `current` with every file for `target` uploaded. A release still missing its
/// archive, checksum or signature is not offered (and is not an error).
pub fn select_release(api_json: &str, current: &Version, target: &str) -> Option<ReleaseInfo> {
    let release: GithubRelease = serde_json::from_str(api_json).ok()?;
    if release.draft || release.prerelease {
        return None;
    }
    let tag = release.tag_name.as_str();
    let version = Version::parse(tag.strip_prefix('v').unwrap_or(tag)).ok()?;
    if !version.pre.is_empty() || version <= *current {
        return None;
    }

    let extension = if target.contains("windows") {
        "zip"
    } else {
        "tar.gz"
    };
    let archive = format!("anima-{target}.{extension}");
    let url_of = |name: &str| {
        release
            .assets
            .iter()
            .find(|asset| asset.name == name)
            .map(|asset| asset.browser_download_url.clone())
            .filter(|url| url.starts_with("https://"))
    };
    Some(ReleaseInfo {
        archive_url: url_of(&archive)?,
        sha256_url: url_of(&format!("{archive}.sha256"))?,
        sig_url: url_of(&format!("{archive}.minisig"))?,
        version,
        target: target.to_owned(),
        archive_name: archive,
    })
}

/// Ask GitHub for the latest release of `repo`. Returns
/// [`UpdateError::NotConfigured`] without any request when this build has no
/// release key; callers show "Updates are not configured for this build".
pub async fn check(
    client: &Client,
    repo: &str,
    current: &Version,
    target: &str,
) -> Result<Option<ReleaseInfo>, UpdateError> {
    if !super::release_key_configured() {
        return Err(UpdateError::NotConfigured);
    }
    check_at(client, GITHUB_API, repo, current, target).await
}

/// [`check`] against another API base URL (tests use a local server).
pub async fn check_at(
    client: &Client,
    api_base: &str,
    repo: &str,
    current: &Version,
    target: &str,
) -> Result<Option<ReleaseInfo>, UpdateError> {
    let url = format!(
        "{}/repos/{repo}/releases/latest",
        api_base.trim_end_matches('/')
    );
    let response = client
        .get(&url)
        .header(USER_AGENT, format!("anima/{current}"))
        .header(ACCEPT, "application/vnd.github+json")
        .timeout(CHECK_TIMEOUT)
        .send()
        .await?;
    // A repository with no published release answers 404.
    if response.status() == StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let body = read_body(response, MAX_SMALL_BYTES).await?;
    let text = String::from_utf8_lossy(&body);
    Ok(select_release(&text, current, target))
}

/// Download `url`, refusing a body larger than `limit` bytes.
pub async fn download(client: &Client, url: &str, limit: u64) -> Result<Vec<u8>, UpdateError> {
    let response = client.get(url).timeout(DOWNLOAD_TIMEOUT).send().await?;
    read_body(response, limit).await
}

/// A release's archive with its published checksum and signature, not yet verified.
pub struct ReleaseDownload {
    archive: Vec<u8>,
    sha256: Vec<u8>,
    minisig: Vec<u8>,
}

/// Download `release`'s checksum, signature and archive.
pub async fn download_release(
    client: &Client,
    release: &ReleaseInfo,
) -> Result<ReleaseDownload, UpdateError> {
    Ok(ReleaseDownload {
        sha256: download(client, &release.sha256_url, MAX_SMALL_BYTES).await?,
        minisig: download(client, &release.sig_url, MAX_SMALL_BYTES).await?,
        archive: download(client, &release.archive_url, MAX_ARCHIVE_BYTES).await?,
    })
}

impl ReleaseDownload {
    /// The archive, only if [`verify`] accepts it against `public_key` (the release
    /// key in production: [`RELEASE_PUBLIC_KEY`](super::RELEASE_PUBLIC_KEY)) *and* the
    /// signature's trusted comment names this exact release: [`release_trusted_comment`]
    /// of its version, target and archive name.
    pub fn verify(self, release: &ReleaseInfo, public_key: &str) -> Result<Vec<u8>, UpdateError> {
        verify(
            &self.archive,
            &String::from_utf8_lossy(&self.sha256),
            &String::from_utf8_lossy(&self.minisig),
            public_key,
            &release_trusted_comment(&release.version, &release.target, &release.archive_name),
        )?;
        Ok(self.archive)
    }
}

/// [`download_release`], then [`ReleaseDownload::verify`].
#[cfg_attr(not(test), allow(dead_code))] // the supervisor uses `download_binary`
pub async fn download_verified(
    client: &Client,
    release: &ReleaseInfo,
    public_key: &str,
) -> Result<Vec<u8>, UpdateError> {
    download_release(client, release)
        .await?
        .verify(release, public_key)
}

/// The verified `anima` binary of `release`: downloaded, verified against
/// `public_key` like [`download_verified`], and taken out of its archive.
/// `stage` hears `"Downloading v{version}"`, then `"Verifying"`.
pub async fn download_binary(
    client: &Client,
    release: &ReleaseInfo,
    public_key: &str,
    stage: impl Fn(String),
) -> Result<Vec<u8>, UpdateError> {
    stage(format!("Downloading v{}", release.version));
    let download = download_release(client, release).await?;
    stage("Verifying".to_string());
    let release = release.clone();
    let public_key = public_key.to_owned();
    // Hashing, checking and unpacking up to 200 MiB is blocking work.
    tokio::task::spawn_blocking(move || {
        let archive = download.verify(&release, &public_key)?;
        extract_binary(&archive, &release.target)
    })
    .await
    .map_err(|error| UpdateError::Io(std::io::Error::other(error)))?
}

async fn read_body(mut response: reqwest::Response, limit: u64) -> Result<Vec<u8>, UpdateError> {
    let status = response.status();
    if !status.is_success() {
        let url = response.url();
        // Without the query: CDN redirects carry signed tokens there.
        let host = url.host_str().unwrap_or_default();
        return Err(UpdateError::Http(format!(
            "{host}{} answered {status}",
            url.path()
        )));
    }
    let too_large = UpdateError::TooLarge { limit };
    if response
        .content_length()
        .is_some_and(|length| length > limit)
    {
        return Err(too_large);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if (body.len() + chunk.len()) as u64 > limit {
            return Err(too_large);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}
