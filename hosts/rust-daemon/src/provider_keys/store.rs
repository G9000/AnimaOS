//! The key file: `{ "version": 1, "providers": { "<id>": { "strategy", "keys": [...] } } }`.
//! Written whole through `<file>.tmp` (mode 0600) and renamed over the target, so
//! a crash never leaves a half-written file and the keys are never world-readable.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::Strategy;

pub(super) const FILE_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct StoredFile {
    pub version: u32,
    #[serde(default)]
    pub providers: BTreeMap<String, StoredProvider>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub(super) struct StoredProvider {
    #[serde(default)]
    pub strategy: Strategy,
    #[serde(default)]
    pub keys: Vec<StoredKey>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct StoredKey {
    pub id: String,
    #[serde(default)]
    pub label: Option<String>,
    pub key: String,
    #[serde(default)]
    pub added_at_ms: u64,
    #[serde(default)]
    pub uses: u64,
    #[serde(default)]
    pub resting_until_ms: Option<u64>,
    #[serde(default)]
    pub last_error: Option<String>,
}

impl std::fmt::Debug for StoredKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoredKey")
            .field("id", &self.id)
            .field("label", &self.label)
            .field("key", &super::mask_key(&self.key))
            .field("uses", &self.uses)
            .field("resting_until_ms", &self.resting_until_ms)
            .finish()
    }
}

/// Reads the key file. A missing file is an empty one; an unreadable or
/// malformed file is an error so the owner's keys are never overwritten.
pub(super) fn load(path: &Path) -> Result<StoredFile, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(StoredFile {
                version: FILE_VERSION,
                providers: BTreeMap::new(),
            });
        }
        Err(error) => return Err(format!("cannot read {}: {error}", path.display())),
    };
    let file: StoredFile = serde_json::from_str(&text)
        .map_err(|error| format!("{} is not a valid key file: {error}", path.display()))?;
    if file.version != FILE_VERSION {
        return Err(format!(
            "{} has unsupported version {}",
            path.display(),
            file.version
        ));
    }
    Ok(file)
}

/// Atomically replaces the key file.
pub(super) fn save(path: &Path, file: &StoredFile) -> Result<(), String> {
    let body = serde_json::to_vec_pretty(file).map_err(|error| error.to_string())?;
    write_private(path, &body).map_err(|error| format!("cannot write {}: {error}", path.display()))
}

fn tmp_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    path.with_file_name(name)
}

fn write_private(path: &Path, body: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = tmp_path(path);
    // A leftover tmp file could carry looser permissions; mode only applies on create.
    let _ = std::fs::remove_file(&tmp);
    let result = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&tmp)?;
        file.write_all(body)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}
