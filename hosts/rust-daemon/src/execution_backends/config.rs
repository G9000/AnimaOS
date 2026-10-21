use super::TargetSummary;
use serde::Deserialize;
use std::path::{Path, PathBuf};

pub(crate) const CONFIG_ENV: &str = "ANIMAOS_EXECUTION_PROFILES_FILE";
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Configuration {
    pub version: u32,
    pub targets: Vec<ExecutionProfile>,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ExecutionProfile {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) backend: Backend,
    #[serde(default = "max_timeout")]
    pub(crate) max_timeout_ms: u64,
}
fn max_timeout() -> u64 {
    120_000
}
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum Backend {
    Local(LocalProfile),
    Docker(DockerProfile),
    Ssh(SshProfile),
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LocalProfile {
    pub executable: PathBuf,
    #[serde(default)]
    pub arguments: Vec<String>,
    pub workspace_root: PathBuf,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DockerProfile {
    pub executable: PathBuf,
    pub image: String,
    #[serde(default = "memory")]
    pub memory_mb: u32,
    #[serde(default = "cpus")]
    pub cpus: u32,
    #[serde(default = "pids")]
    pub pids: u32,
}
fn memory() -> u32 {
    256
}
fn cpus() -> u32 {
    1
}
fn pids() -> u32 {
    64
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SshProfile {
    pub executable: PathBuf,
    pub host: String,
    pub user: String,
    pub port: u16,
    pub identity_file: PathBuf,
    pub known_hosts_file: PathBuf,
    pub worker_entrypoint: String,
}
pub(super) fn absolute_file(path: &Path) -> bool {
    path.is_absolute() && path.is_file()
}
pub(super) fn safe_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c))
}
impl ExecutionProfile {
    pub(super) fn validate(&mut self) -> Result<(), String> {
        if !safe_id(&self.id)
            || self.label.trim().is_empty()
            || self.label.len() > 128
            || !(1..=300_000).contains(&self.max_timeout_ms)
        {
            return Err("Invalid execution profile identity or timeout".into());
        }
        match &mut self.backend {
            Backend::Local(p) => {
                if !absolute_file(&p.executable)
                    || !p.workspace_root.is_absolute()
                    || !p.workspace_root.is_dir()
                    || p.arguments.iter().any(|s| s.contains('\0'))
                {
                    return Err(
                        "Local execution requires absolute installed executable and workspace root"
                            .into(),
                    );
                }
                p.workspace_root = std::fs::canonicalize(&p.workspace_root)
                    .map_err(|_| "Local execution workspace is unavailable")?;
            }
            Backend::Docker(p) => {
                let valid_image = p
                    .image
                    .rsplit_once("@sha256:")
                    .is_some_and(|(name, digest)| {
                        !name.is_empty()
                            && !name.starts_with('-')
                            && name
                                .bytes()
                                .all(|c| c.is_ascii_alphanumeric() || b"./_:-".contains(&c))
                            && digest.len() == 64
                            && digest.bytes().all(|c| c.is_ascii_hexdigit())
                    });
                if !absolute_file(&p.executable)
                    || !valid_image
                    || !(32..=4096).contains(&p.memory_mb)
                    || !(1..=4).contains(&p.cpus)
                    || !(8..=256).contains(&p.pids)
                {
                    return Err("Docker execution requires an absolute installed CLI, digest-pinned image, and bounded resources".into());
                }
            }
            Backend::Ssh(p) => {
                let host_valid = !p.host.is_empty()
                    && p.host.len() <= 253
                    && !p.host.starts_with('-')
                    && p.host
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b".-:".contains(&c));
                let entry_valid = p.worker_entrypoint.starts_with('/')
                    && p.worker_entrypoint.len() <= 512
                    && p.worker_entrypoint
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"/._-".contains(&c));
                if !absolute_file(&p.executable)
                    || !host_valid
                    || !safe_id(&p.user)
                    || p.port == 0
                    || !absolute_file(&p.identity_file)
                    || !absolute_file(&p.known_hosts_file)
                    || !entry_valid
                    || std::fs::metadata(&p.known_hosts_file).map_or(true, |m| m.len() == 0)
                {
                    return Err("SSH execution requires fixed endpoint, key, host pins, and absolute Rust worker entrypoint".into());
                }
            }
        }
        Ok(())
    }
    pub(super) fn summary(&self) -> TargetSummary {
        TargetSummary {
            id: self.id.clone(),
            label: self.label.clone(),
            backend: self.backend.name().into(),
            max_timeout_ms: self.max_timeout_ms,
        }
    }
}
impl Backend {
    pub(super) fn name(&self) -> &'static str {
        match self {
            Self::Local(_) => "local",
            Self::Docker(_) => "docker",
            Self::Ssh(_) => "ssh",
        }
    }
}
