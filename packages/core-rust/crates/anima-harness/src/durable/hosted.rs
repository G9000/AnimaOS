//! Optional host startup utilities. The durable harness itself never reads files.
use anima_core::{PersistenceProtection, PersistenceSnapshotSealKey};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io,
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
};
use uuid::Uuid;
fn invalid() -> io::Error {
    io::Error::other("Durable startup configuration or preprovisioned authority is invalid")
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoragePaths {
    pub execution: PathBuf,
    pub memory: PathBuf,
    pub owner: PathBuf,
    pub key: PathBuf,
}
pub struct StorageMaterial {
    pub owner: Uuid,
    pub protection: PersistenceProtection,
}
impl StoragePaths {
    pub fn paths(&self) -> [PathBuf; 4] {
        [
            self.execution.clone(),
            self.memory.clone(),
            self.owner.clone(),
            self.key.clone(),
        ]
    }
    pub fn validate(&self, extra: &[PathBuf]) -> io::Result<StorageMaterial> {
        let mut paths = self.paths().to_vec();
        paths.extend_from_slice(extra);
        validate_paths(&paths)?;
        if fs::metadata(&self.owner).map_err(|_| invalid())?.len() > 64
            || fs::metadata(&self.key).map_err(|_| invalid())?.len() != 32
        {
            return Err(invalid());
        }
        let owner = fs::read_to_string(&self.owner).map_err(|_| invalid())?;
        let owner = Uuid::parse_str(owner.trim()).map_err(|_| invalid())?;
        if owner.is_nil() {
            return Err(invalid());
        }
        let key: [u8; 32] = fs::read(&self.key)
            .map_err(|_| invalid())?
            .try_into()
            .map_err(|_| invalid())?;
        let protection = PersistenceProtection::allow_model_payloads(
            PersistenceSnapshotSealKey::new(key).map_err(|_| invalid())?,
            vec![],
        )
        .map_err(|_| invalid())?;
        Ok(StorageMaterial { owner, protection })
    }
}
/// Reject lexical, reparse and configured hardlink aliases before opening payloads.
pub fn validate_paths(paths: &[PathBuf]) -> io::Result<()> {
    let mut resolved = Vec::new();
    let mut pins = Vec::new();
    for path in paths {
        let canonical = safe_path(path)?;
        if resolved.iter().any(|p: &PathBuf| {
            canonical == *p || canonical.starts_with(p) || p.starts_with(&canonical)
        }) {
            return Err(invalid());
        }
        resolved.push(canonical);
        if path.exists() {
            let file = File::open(path).map_err(|_| invalid())?;
            file.try_lock().map_err(|_| invalid())?;
            pins.push(file);
        }
    }
    Ok(())
}
pub fn safe_path(path: &Path) -> io::Result<PathBuf> {
    if path.to_string_lossy().starts_with(r"\\.\") {
        return Err(invalid());
    }
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        return Err(invalid());
    }
    for component in path.components() {
        if let Component::Normal(name) = component {
            let name = name.to_str().ok_or_else(invalid)?;
            if name.is_empty()
                || name.contains(':')
                || name.ends_with(['.', ' '])
                || name.ends_with(".owner.lock")
                || name.ends_with("-wal")
                || name.ends_with("-shm")
            {
                return Err(invalid());
            }
            let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
            if ["CON", "PRN", "AUX", "NUL", "CLOCK$"].contains(&stem.as_str())
                || ((stem.starts_with("COM") || stem.starts_with("LPT"))
                    && stem.len() == 4
                    && stem.as_bytes()[3].is_ascii_digit())
            {
                return Err(invalid());
            }
        }
    }
    if fs::symlink_metadata(path).is_ok_and(|meta| !meta.is_file()) {
        return Err(invalid());
    }
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(meta) => {
                if meta.file_type().is_symlink() {
                    return Err(invalid());
                }
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    if meta.file_attributes() & 0x400 != 0 {
                        return Err(invalid());
                    }
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(_) => return Err(invalid()),
        }
    }
    let mut nearest = path;
    let mut suffix = Vec::new();
    while !nearest.exists() {
        suffix.push(nearest.file_name().ok_or_else(invalid)?.to_os_string());
        nearest = nearest.parent().ok_or_else(invalid)?;
    }
    let mut canonical = fs::canonicalize(nearest).map_err(|_| invalid())?;
    for name in suffix.into_iter().rev() {
        canonical.push(name)
    }
    #[cfg(windows)]
    {
        canonical = PathBuf::from(canonical.to_string_lossy().to_lowercase());
    }
    Ok(canonical)
}
/// Retain these leases for the lifetime of startup-owned stores; never unlink them.
pub struct StorageLease {
    _files: Vec<File>,
}
impl StorageLease {
    pub fn acquire(paths: &[PathBuf]) -> io::Result<Self> {
        validate_paths(paths)?;
        let mut files = Vec::new();
        for path in paths {
            let mut lock = path.as_os_str().to_os_string();
            lock.push(".owner.lock");
            let lock = PathBuf::from(lock);
            if fs::symlink_metadata(&lock).is_ok_and(|m| m.file_type().is_symlink()) {
                return Err(invalid());
            }
            let file = OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(lock)
                .map_err(|_| invalid())?;
            file.try_lock().map_err(|_| invalid())?;
            files.push(file)
        }
        Ok(Self { _files: files })
    }
}
#[derive(Default)]
struct WorkerState {
    closed: bool,
    count: usize,
}
#[derive(Clone, Default)]
pub struct HostWorkers(Arc<Mutex<WorkerState>>);
pub struct HostWork(HostWorkers);
impl HostWorkers {
    pub fn enter(&self) -> Option<HostWork> {
        let mut state = self.0.lock().ok()?;
        if state.closed || state.count >= 256 {
            return None;
        }
        state.count += 1;
        Some(HostWork(self.clone()))
    }
    pub fn close(&self) {
        if let Ok(mut state) = self.0.lock() {
            state.closed = true
        }
    }
    pub fn is_empty(&self) -> bool {
        self.0.lock().is_ok_and(|state| state.count == 0)
    }
}
impl Drop for HostWork {
    fn drop(&mut self) {
        if let Ok(mut state) = self.0 .0.lock() {
            state.count = state.count.saturating_sub(1)
        }
    }
}
