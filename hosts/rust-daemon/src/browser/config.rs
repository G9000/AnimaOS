//! Explicit startup-only verified installation loading. Constructors never read env.
use super::{
    records::{
        digest_valid, valid_id, BrowserInstallation, InstalledBrowserPolicies,
        InstalledBrowserPolicy,
    },
    transport::{
        canonical_digest, error, strict_decode, BrowserErrorCode, BrowserPhysicalRuntime,
        BrowserTransportFactory, PlaywrightTransportFactory,
    },
};
use anima_service_adapters::browser::{BrowserError, BrowserScope};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::Read,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

pub(crate) const BROWSER_CONFIG_ENV: &str = "ANIMAOS_RS_BROWSER_CONFIG_FILE";
const MAX_PROGRAM_BYTES: u64 = 512 * 1024 * 1024;
#[derive(Clone)]
pub(crate) struct VerifiedBrowserInstallation {
    pub(crate) id: String,
    pub(crate) digest: String,
    pub(super) node_executable: PathBuf,
    pub(super) node_sha256: String,
    pub(super) worker_script: PathBuf,
    pub(super) worker_sha256: String,
    pub(super) browser_executable: PathBuf,
    pub(super) browser_sha256: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InstallationInput {
    id: String,
    node_executable: PathBuf,
    node_sha256: String,
    worker_script: PathBuf,
    worker_sha256: String,
    browser_executable: PathBuf,
    browser_sha256: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PolicyInput {
    id: String,
    template: BrowserScope,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StartupInput {
    schema_version: u8,
    installation: InstallationInput,
    policies: Vec<PolicyInput>,
}

#[derive(Clone)]
pub(crate) struct VerifiedBrowserSetup {
    pub(crate) policies: InstalledBrowserPolicies,
    pub(crate) factory: Arc<dyn BrowserTransportFactory>,
}
pub(crate) fn load_startup_setup(path: &Path) -> Result<VerifiedBrowserSetup, BrowserError> {
    absolute_file(path)?;
    let bytes = read_small(path, 64 * 1024)?;
    let input: StartupInput = strict_decode(&bytes)?;
    if input.schema_version != 1
        || !valid_id(&input.installation.id)
        || input.policies.is_empty()
        || input.policies.len() > 16
    {
        return Err(error(BrowserErrorCode::Invalid));
    }
    let i = input.installation;
    let value = serde_json::json!({
        "id": i.id, "nodeExecutable": i.node_executable, "nodeSha256": i.node_sha256,
        "workerScript": i.worker_script, "workerSha256": i.worker_sha256,
        "browserExecutable": i.browser_executable, "browserSha256": i.browser_sha256,
    });
    let installation = VerifiedBrowserInstallation {
        id: i.id,
        digest: canonical_digest(&value)?,
        node_executable: i.node_executable,
        node_sha256: i.node_sha256,
        worker_script: i.worker_script,
        worker_sha256: i.worker_sha256,
        browser_executable: i.browser_executable,
        browser_sha256: i.browser_sha256,
    };
    installation.revalidate()?;
    let policies = InstalledBrowserPolicies::new(
        BrowserInstallation {
            id: installation.id.clone(),
            digest: installation.digest.clone(),
        },
        input
            .policies
            .into_iter()
            .map(|policy| InstalledBrowserPolicy {
                id: policy.id,
                template: policy.template,
            })
            .collect(),
    )
    .map_err(|_| error(BrowserErrorCode::Invalid))?;
    let factory = Arc::new(PlaywrightTransportFactory::new(
        installation,
        Arc::new(BrowserPhysicalRuntime::default()),
    ));
    Ok(VerifiedBrowserSetup { policies, factory })
}
impl VerifiedBrowserInstallation {
    pub(super) fn revalidate(&self) -> Result<(), BrowserError> {
        for (path, expected) in [
            (&self.node_executable, &self.node_sha256),
            (&self.worker_script, &self.worker_sha256),
            (&self.browser_executable, &self.browser_sha256),
        ] {
            if !digest_valid(expected) {
                return Err(error(BrowserErrorCode::Invalid));
            }
            absolute_file(path)?;
            let mut file = File::open(path).map_err(|_| error(BrowserErrorCode::Unavailable))?;
            let metadata = file
                .metadata()
                .map_err(|_| error(BrowserErrorCode::Unavailable))?;
            if !metadata.is_file()
                || metadata.len() == 0
                || metadata.len() > MAX_PROGRAM_BYTES
                || linked(&metadata)
            {
                return Err(error(BrowserErrorCode::Unavailable));
            }
            let mut hash = Sha256::new();
            let mut total = 0u64;
            let mut buffer = [0u8; 8192];
            loop {
                let read = file
                    .read(&mut buffer)
                    .map_err(|_| error(BrowserErrorCode::Unavailable))?;
                if read == 0 {
                    break;
                }
                total += read as u64;
                if total > MAX_PROGRAM_BYTES {
                    return Err(error(BrowserErrorCode::LimitsExceeded));
                }
                hash.update(&buffer[..read]);
            }
            if total != metadata.len() || format!("{:x}", hash.finalize()) != *expected {
                return Err(error(BrowserErrorCode::Unavailable));
            }
        }
        Ok(())
    }
}
fn linked(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    false
}
fn absolute_file(path: &Path) -> Result<(), BrowserError> {
    let text = path
        .to_str()
        .ok_or_else(|| error(BrowserErrorCode::Invalid))?;
    if !path.is_absolute()
        || text.len() > 1024
        || text.chars().any(char::is_control)
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return Err(error(BrowserErrorCode::Invalid));
    }
    #[cfg(windows)]
    {
        use std::path::Prefix;
        if !matches!(path.components().next(), Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::Disk(_)))
        {
            return Err(error(BrowserErrorCode::Invalid));
        }
    }
    let metadata = fs::symlink_metadata(path).map_err(|_| error(BrowserErrorCode::Unavailable))?;
    if !metadata.is_file() || linked(&metadata) {
        return Err(error(BrowserErrorCode::Unavailable));
    }
    Ok(())
}
fn read_small(path: &Path, maximum: usize) -> Result<Vec<u8>, BrowserError> {
    let file = File::open(path).map_err(|_| error(BrowserErrorCode::Unavailable))?;
    let mut bytes = Vec::new();
    file.take(maximum as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| error(BrowserErrorCode::Unavailable))?;
    if bytes.len() > maximum {
        return Err(error(BrowserErrorCode::LimitsExceeded));
    }
    Ok(bytes)
}
