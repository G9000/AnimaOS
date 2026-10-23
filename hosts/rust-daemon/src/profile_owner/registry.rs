//! Versioned private claim registry for file/directory overlap exclusion.
//!
//! A record is active only while its separate OS lease is held. The gate
//! serializes conflict checks and immutable record publication; stale files
//! never confer authority and are never stolen by PID or age.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use atomicwrites::{AllowOverwrite, AtomicFile};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{
    canonical_resource, file_metadata, metadata_only, ownership_lost_error, ProfileResource,
};

const VERSION: u32 = 1;
const MAX_ENTRIES: usize = 8192;
const MAX_RESOURCES: usize = 64;
const MAX_ANCESTORS: usize = 256;
const MAX_RECORD_BYTES: u64 = 256 * 1024;
const GATE_WAIT: Duration = Duration::from_secs(5);
const GATE_NAME: &str = ".gate.owner.lock";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Resource {
    locator: String,
    ancestors: Vec<String>,
    directory: bool,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    version: u32,
    normalization: String,
    primary: String,
    resources: Vec<Resource>,
}

pub(super) struct Claim {
    // Drop immutable metadata before releasing authority, so another claimant
    // can safely replace an inactive record after obtaining its lease.
    metadata: File,
    lease: File,
    root: File,
    metadata_path: PathBuf,
    lease_path: PathBuf,
    root_path: PathBuf,
    root_locator_path: PathBuf,
    metadata_identity: (u64, u64),
    lease_identity: (u64, u64),
    root_identity: (u64, u64),
    digest: [u8; 32],
}

impl Claim {
    pub(super) fn acquire<T>(
        resources: &[ProfileResource],
        prepare: impl FnOnce(&Path) -> io::Result<T>,
    ) -> io::Result<(Option<Self>, T)> {
        if resources.is_empty() {
            return Ok((None, prepare(Path::new(""))?));
        }
        if resources.len() > MAX_RESOURCES {
            return Err(ownership_lost_error());
        }
        let root_locator_path = registry_path()?;
        reject_symlink_root(&root_locator_path)?;
        let root_path = canonical_resource(&root_locator_path)?;
        // The registry namespace itself is never a configurable payload, nor
        // can a directory payload contain the registry and mutate its claims.
        let root_locator = locator(&root_path);
        let root_ancestors = ancestors(&root_path)?;
        let requests: Vec<_> = resources
            .iter()
            .map(recorded_resource)
            .collect::<io::Result<_>>()?;
        for (index, request) in requests.iter().enumerate() {
            if !request.directory
                && requests.iter().enumerate().any(|(other, child)| {
                    index != other && child.ancestors.contains(&request.locator)
                })
            {
                return Err(ownership_lost_error());
            }
        }
        for request in &requests {
            if request.locator == root_locator
                || request.ancestors.contains(&root_locator)
                || root_ancestors.contains(&request.locator)
            {
                return Err(ownership_lost_error());
            }
        }
        initialize_root(&root_path)?;
        let root = open_root(&root_path)?;
        let root_identity = identity(&root)?;
        verify_root(&root_path, &root, root_identity)?;
        verify_root(&root_locator_path, &root, root_identity)?;
        let gate_path = root_path.join(GATE_NAME);
        let gate = open_lease(&gate_path)?;
        let gate_identity = identity(&gate)?;
        let deadline = Instant::now() + GATE_WAIT;
        loop {
            match gate.try_lock() {
                Ok(()) => break,
                Err(std::fs::TryLockError::WouldBlock) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(5))
                }
                Err(_) => return Err(ownership_lost_error()),
            }
        }
        verify_named(&gate_path, &gate, gate_identity, false)?;
        verify_root(&root_path, &root, root_identity)?;
        let primary = requests
            .first()
            .ok_or_else(ownership_lost_error)?
            .locator
            .clone();
        let lease_path = root_path.join(format!("{primary}.owner.lock"));
        let lease = open_lease(&lease_path)?;
        lease.try_lock().map_err(|_| ownership_lost_error())?;
        let lease_identity = identity(&lease)?;
        let mut entries = 0usize;
        for entry in std::fs::read_dir(&root_path)? {
            let entry = entry?;
            entries += 1;
            if entries > MAX_ENTRIES {
                return Err(ownership_lost_error());
            }
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                return Err(ownership_lost_error());
            };
            if name == GATE_NAME || name == format!("{primary}.owner.lock") {
                continue;
            }
            let Some(other_primary) = name.strip_suffix(".owner.lock") else {
                continue;
            };
            let other_lease = open_lease(&entry.path())?;
            match other_lease.try_lock() {
                Ok(()) => continue, // Inactive metadata confers no authority.
                Err(std::fs::TryLockError::WouldBlock) => {}
                Err(_) => return Err(ownership_lost_error()),
            }
            if !digest_string(other_primary) {
                return Err(ownership_lost_error());
            }
            let metadata_path = root_path.join(format!("{other_primary}.json"));
            let metadata = open_metadata(&metadata_path)?;
            let bytes = read_metadata(&metadata)?;
            let record: Record =
                serde_json::from_slice(&bytes).map_err(|_| ownership_lost_error())?;
            validate_record(&record, other_primary)?;
            if requests.iter().any(|request| {
                record
                    .resources
                    .iter()
                    .any(|active| overlaps(request, active))
            }) {
                return Err(ownership_lost_error());
            }
        }
        // Path conflicts are settled before this callback may create payloads.
        // Keep the same gate through exact leases, stable pins and publication.
        let prepared = prepare(&root_path)?;
        verify_root(&root_path, &root, root_identity)?;
        verify_named(&gate_path, &gate, gate_identity, false)?;
        verify_named(&lease_path, &lease, lease_identity, false)?;
        let metadata_path = root_path.join(format!("{primary}.json"));
        let record = Record {
            version: VERSION,
            normalization: normalization(),
            primary,
            resources: requests,
        };
        let bytes = serde_json::to_vec(&record).map_err(|_| ownership_lost_error())?;
        if bytes.len() as u64 > MAX_RECORD_BYTES {
            return Err(ownership_lost_error());
        }
        AtomicFile::new(&metadata_path, AllowOverwrite)
            .write(|file| {
                file.write_all(&bytes)?;
                file.sync_all()
            })
            .map_err(|_| ownership_lost_error())?;
        #[cfg(unix)]
        root.sync_all()?;
        let metadata = open_metadata(&metadata_path)?;
        let metadata_identity = identity(&metadata)?;
        let claim = Self {
            metadata,
            lease,
            root,
            metadata_path,
            lease_path,
            root_path,
            root_locator_path,
            metadata_identity,
            lease_identity,
            root_identity,
            digest: Sha256::digest(&bytes).into(),
        };
        claim.ensure_owned()?;
        drop(gate);
        Ok((Some(claim), prepared))
    }

    pub(super) fn with_gate<T>(&self, work: impl FnOnce(&Path) -> io::Result<T>) -> io::Result<T> {
        self.ensure_owned()?;
        let path = self.root_path.join(GATE_NAME);
        let gate = open_lease(&path)?;
        let identity = identity(&gate)?;
        let deadline = Instant::now() + GATE_WAIT;
        loop {
            match gate.try_lock() {
                Ok(()) => break,
                Err(std::fs::TryLockError::WouldBlock) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(5))
                }
                Err(_) => return Err(ownership_lost_error()),
            }
        }
        verify_named(&path, &gate, identity, false)?;
        self.ensure_owned()?;
        let result = work(&self.root_path)?;
        self.ensure_owned()?;
        Ok(result)
    }

    pub(super) fn probe_payload(&self, payload: &super::MetadataFacts) -> io::Result<()> {
        probe_payload(&self.root_path, payload)
    }

    pub(super) fn ensure_owned(&self) -> io::Result<()> {
        #[cfg(not(test))]
        if registry_path()? != self.root_locator_path {
            return Err(ownership_lost_error());
        }
        verify_root(&self.root_path, &self.root, self.root_identity)?;
        verify_root(&self.root_locator_path, &self.root, self.root_identity)?;
        verify_named(&self.lease_path, &self.lease, self.lease_identity, false)?;
        verify_named(
            &self.metadata_path,
            &self.metadata,
            self.metadata_identity,
            false,
        )?;
        // A separate read handle avoids shared seek offsets across concurrent
        // verification calls. Active metadata is immutable under the protocol.
        let readable = open_metadata(&self.metadata_path)?;
        if identity(&readable)? != self.metadata_identity {
            return Err(ownership_lost_error());
        }
        let digest: [u8; 32] = Sha256::digest(read_metadata(&readable)?).into();
        if digest != self.digest {
            return Err(ownership_lost_error());
        }
        Ok(())
    }
}

/// Separate empty OS lease: locking a real SQLite payload would interfere with
/// SQLite's own byte locks on Windows. Its retained descriptor prevents inode
/// reuse, while this versioned identity lease excludes renamed aliases.
pub(super) struct PayloadLease {
    file: File,
    path: PathBuf,
    identity: (u64, u64),
}
impl PayloadLease {
    pub(super) fn acquire(root: &Path, payload: &super::MetadataFacts) -> io::Result<Self> {
        let path = payload_lease_path(root, payload)?;
        let file = open_lease(&path)?;
        file.try_lock().map_err(|_| ownership_lost_error())?;
        let identity = identity(&file)?;
        verify_named(&path, &file, identity, false)?;
        Ok(Self {
            file,
            path,
            identity,
        })
    }
    pub(super) fn ensure_owned(&self) -> io::Result<()> {
        verify_named(&self.path, &self.file, self.identity, false)
    }
}
pub(super) fn probe_payload(root: &Path, payload: &super::MetadataFacts) -> io::Result<()> {
    let path = payload_lease_path(root, payload)?;
    let file = match open_file(&path, true, false) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    let metadata = file_metadata(&file)?;
    if !metadata.is_file() || metadata.nlink() != 1 || file.metadata()?.len() != 0 {
        return Err(ownership_lost_error());
    }
    let expected = identity(&file)?;
    file.try_lock().map_err(|_| ownership_lost_error())?;
    verify_named(&path, &file, expected, false)
}
fn payload_lease_path(root: &Path, payload: &super::MetadataFacts) -> io::Result<PathBuf> {
    if payload.ino() == 0 {
        return Err(ownership_lost_error());
    }
    let mut hash = Sha256::new();
    hash.update(b"anima-profile-payload-identity-v1\0");
    hash.update(payload.dev().to_be_bytes());
    hash.update(payload.ino().to_be_bytes());
    Ok(root.join(format!("{}.v1.payload.lock", hex::encode(hash.finalize()))))
}

fn recorded_resource(resource: &ProfileResource) -> io::Result<Resource> {
    Ok(Resource {
        locator: locator(resource.path()),
        ancestors: ancestors(resource.path())?,
        directory: resource.is_directory(),
    })
}
fn overlaps(a: &Resource, b: &Resource) -> bool {
    // Even two future files cannot coexist as ancestor/descendant: creating
    // the child parent would turn the first profile's future file into a dir.
    a.locator == b.locator || b.ancestors.contains(&a.locator) || a.ancestors.contains(&b.locator)
}
fn ancestors(path: &Path) -> io::Result<Vec<String>> {
    let mut result = Vec::new();
    for parent in path.ancestors().skip(1) {
        if result.len() >= MAX_ANCESTORS {
            return Err(ownership_lost_error());
        }
        result.push(locator(parent));
    }
    Ok(result)
}
fn locator(path: &Path) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        hex::encode(Sha256::digest(path.as_os_str().as_bytes()))
    }
    #[cfg(not(unix))]
    {
        hex::encode(Sha256::digest(
            path.to_string_lossy().to_uppercase().as_bytes(),
        ))
    }
}
// Changing path folding requires a protocol change. Active claims produced
// under another Unicode mapping fail closed instead of silently missing aliases.
fn normalization() -> String {
    #[cfg(unix)]
    {
        "unix-path-bytes-v1".into()
    }
    #[cfg(not(unix))]
    {
        let (major, minor, patch) = std::char::UNICODE_VERSION;
        format!("windows-uppercase-unicode-{major}.{minor}.{patch}-v1")
    }
}

fn digest_string(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
fn validate_record(record: &Record, primary: &str) -> io::Result<()> {
    if record.version != VERSION
        || record.normalization != normalization()
        || record.primary != primary
        || record.resources.is_empty()
        || record.resources.len() > MAX_RESOURCES
        || record
            .resources
            .first()
            .is_none_or(|resource| resource.locator != primary)
        || record.resources.iter().any(|resource| {
            !digest_string(&resource.locator)
                || resource.ancestors.len() > MAX_ANCESTORS
                || resource.ancestors.contains(&resource.locator)
                || resource
                    .ancestors
                    .iter()
                    .any(|ancestor| !digest_string(ancestor))
        })
    {
        return Err(ownership_lost_error());
    }
    Ok(())
}
fn identity(file: &File) -> io::Result<(u64, u64)> {
    let metadata = file_metadata(file)?;
    if metadata.ino() == 0 {
        return Err(ownership_lost_error());
    }
    Ok((metadata.dev(), metadata.ino()))
}
fn verify_named(
    path: &Path,
    retained: &File,
    expected: (u64, u64),
    directory: bool,
) -> io::Result<()> {
    let named = metadata_only(path)?;
    if identity(retained)? != expected
        || (named.dev(), named.ino()) != expected
        || named.is_dir() != directory
        || (!directory && (!named.is_file() || named.nlink() != 1))
    {
        return Err(ownership_lost_error());
    }
    Ok(())
}
fn open_lease(path: &Path) -> io::Result<File> {
    let file = open_file(path, true, true)?;
    let metadata = file_metadata(&file)?;
    if !metadata.is_file() || metadata.nlink() != 1 || file.metadata()?.len() != 0 {
        return Err(ownership_lost_error());
    }
    Ok(file)
}
fn open_metadata(path: &Path) -> io::Result<File> {
    let file = open_file(path, false, false)?;
    let metadata = file_metadata(&file)?;
    if !metadata.is_file() || metadata.nlink() != 1 || file.metadata()?.len() > MAX_RECORD_BYTES {
        return Err(ownership_lost_error());
    }
    Ok(file)
}
fn read_metadata(file: &File) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    file.take(MAX_RECORD_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_RECORD_BYTES {
        return Err(ownership_lost_error());
    }
    Ok(bytes)
}
fn open_file(path: &Path, write: bool, create: bool) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(write)
        .create(create)
        .truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options
            .share_mode(if write {
                0x00000001 | 0x00000002
            } else {
                0x00000001
            })
            .custom_flags(0x00200000);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(
            (rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32,
        );
    }
    options.open(path)
}
fn open_root(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options
            .access_mode(0)
            .share_mode(0x00000001 | 0x00000002)
            .custom_flags(0x02000000 | 0x00200000);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(
            (rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32,
        );
    }
    options.open(path)
}
fn initialize_root(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = std::fs::DirBuilder::new();
        builder.mode(0o700);
        match builder.create(path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    #[cfg(not(unix))]
    std::fs::create_dir_all(path)?;
    private_root(path)
}
fn private_root(path: &Path) -> io::Result<()> {
    reject_symlink_root(path)?;
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(ownership_lost_error());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.uid() != rustix::process::geteuid().as_raw()
            || metadata.permissions().mode() & 0o777 != 0o700
        {
            return Err(ownership_lost_error());
        }
    }
    Ok(())
}
fn reject_symlink_root(path: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(ownership_lost_error());
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if metadata.file_attributes() & 0x00000400 != 0 {
                    return Err(ownership_lost_error());
                }
            }
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}
fn verify_root(path: &Path, retained: &File, expected: (u64, u64)) -> io::Result<()> {
    private_root(path)?;
    verify_named(path, retained, expected, true)
}
fn registry_path() -> io::Result<PathBuf> {
    #[cfg(test)]
    {
        test_registry_path()
    }
    #[cfg(not(test))]
    {
        production_registry_path()
    }
}

pub(super) fn production_registry_path() -> io::Result<PathBuf> {
    #[cfg(windows)]
    {
        use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY};
        // SHGetKnownFolderPath with the ambient token can expand USERPROFILE
        // and redirect by package identity. The OS profile registration and
        // token user SID are shared by packaged and development processes.
        let sid = win_custody::current_user_sid_string().map_err(|_| ownership_lost_error())?;
        let key = winreg::RegKey::predef(HKEY_LOCAL_MACHINE)
            .open_subkey_with_flags(
                format!(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\ProfileList\{sid}"),
                KEY_READ | KEY_WOW64_64KEY,
            )
            .map_err(|_| ownership_lost_error())?;
        let value = key
            .get_raw_value("ProfileImagePath")
            .map_err(|_| ownership_lost_error())?;
        literal_profile_path(&value).map(|profile| {
            profile
                .join("AppData")
                .join("Local")
                .join("AnimaOS")
                .join("profile-ownership-v1")
        })
    }
    #[cfg(unix)]
    {
        Ok(PathBuf::from(format!(
            "/tmp/anima-profile-ownership-{}",
            rustix::process::geteuid().as_raw()
        )))
    }
    #[cfg(not(any(unix, windows)))]
    {
        Err(ownership_lost_error())
    }
}

#[cfg(windows)]
fn literal_profile_path(value: &winreg::RegValue<'_>) -> io::Result<PathBuf> {
    use winreg::enums::{REG_EXPAND_SZ, REG_SZ};
    if !matches!(value.vtype, REG_SZ | REG_EXPAND_SZ)
        || value.bytes.len() % 2 != 0
        || value.bytes.len() > 64 * 1024
    {
        return Err(ownership_lost_error());
    }
    let mut words: Vec<_> = value
        .bytes
        .chunks_exact(2)
        .map(|word| u16::from_le_bytes([word[0], word[1]]))
        .collect();
    while words.last() == Some(&0) {
        words.pop();
    }
    let literal = String::from_utf16(&words).map_err(|_| ownership_lost_error())?;
    if literal.contains(['%', '\0']) {
        return Err(ownership_lost_error());
    }
    let path = PathBuf::from(literal);
    if !path.is_absolute()
        || super::reserved_payload_name(&path)
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )
        })
    {
        return Err(ownership_lost_error());
    }
    Ok(path)
}

#[cfg(test)]
pub(crate) fn test_registry_path() -> io::Result<PathBuf> {
    if let Some(path) = TEST_ROOT_OVERRIDE.with(|slot| slot.borrow().clone()) {
        return Ok(path);
    }
    if let Some(path) = std::env::var_os("ANIMA_TEST_PROFILE_OWNER_REGISTRY_DIR") {
        return Ok(PathBuf::from(path));
    }
    static ROOT: std::sync::OnceLock<Result<tempfile::TempDir, io::Error>> =
        std::sync::OnceLock::new();
    ROOT.get_or_init(tempfile::tempdir)
        .as_ref()
        .map(|root| root.path().to_path_buf())
        .map_err(|_| ownership_lost_error())
}

#[cfg(test)]
thread_local! { static TEST_ROOT_OVERRIDE: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) }; }
#[cfg(test)]
struct TestRootOverride(Option<PathBuf>);
#[cfg(test)]
impl TestRootOverride {
    fn set(path: PathBuf) -> Self {
        Self(TEST_ROOT_OVERRIDE.with(|slot| slot.replace(Some(path))))
    }
}
#[cfg(test)]
impl Drop for TestRootOverride {
    fn drop(&mut self) {
        TEST_ROOT_OVERRIDE.with(|slot| *slot.borrow_mut() = self.0.take());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile_owner::ProfileOwner;

    #[cfg(windows)]
    #[test]
    fn profile_owner_os_profile_registration_requires_a_literal_absolute_supported_path() {
        use winreg::enums::{REG_BINARY, REG_EXPAND_SZ, REG_MULTI_SZ, REG_SZ};
        let value = |path: &str, vtype| winreg::RegValue {
            bytes: path
                .encode_utf16()
                .chain([0])
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>()
                .into(),
            vtype,
        };
        for kind in [REG_SZ, REG_EXPAND_SZ] {
            assert_eq!(
                literal_profile_path(&value(r"C:\Users\profile", kind)).unwrap(),
                PathBuf::from(r"C:\Users\profile")
            );
        }
        for path in [
            r"%USERPROFILE%",
            r"%SystemDrive%\Users\profile",
            r"relative\profile",
            r"C:profile",
            r"\\.\NUL",
            r"\\?\GLOBALROOT\Device\Null",
            r"C:\Users\profile.",
            r"C:\Users\profile:stream",
            "C:\\Users\\one\0\\other",
        ] {
            assert!(
                literal_profile_path(&value(path, REG_EXPAND_SZ)).is_err(),
                "unsupported authoritative profile registration was accepted"
            );
        }
        for kind in [REG_BINARY, REG_MULTI_SZ] {
            assert!(literal_profile_path(&value(r"C:\Users\profile", kind)).is_err());
        }
        let malformed = winreg::RegValue {
            bytes: vec![0xff].into(),
            vtype: REG_SZ,
        };
        assert!(literal_profile_path(&malformed).is_err());
        let invalid_utf16 = winreg::RegValue {
            bytes: vec![0, 0xd8].into(),
            vtype: REG_SZ,
        };
        assert!(literal_profile_path(&invalid_utf16).is_err());
    }

    #[test]
    fn profile_owner_corrupt_active_record_refuses_before_mkdir_but_stale_record_is_ignored() {
        let fixture = tempfile::tempdir().unwrap();
        let registry = fixture.path().join("registry");
        let _private = TestRootOverride::set(registry.clone());
        initialize_root(&registry).unwrap();
        let primary = "f".repeat(64);
        let lease = open_lease(&registry.join(format!("{primary}.owner.lock"))).unwrap();
        lease.try_lock().unwrap();
        std::fs::write(
            registry.join(format!("{primary}.json")),
            b"malformed active record",
        )
        .unwrap();
        let payload = fixture.path().join("payload/state.json");
        assert!(ProfileOwner::acquire([payload.clone()]).is_err());
        assert!(
            !payload.parent().unwrap().exists(),
            "a refused claim created the payload parent"
        );
        drop(lease);
        let owner = ProfileOwner::acquire([payload]).unwrap();
        owner.ensure_owned().unwrap();
    }

    #[test]
    fn profile_owner_unknown_active_normalization_protocol_is_refused() {
        let fixture = tempfile::tempdir().unwrap();
        let registry = fixture.path().join("registry");
        let _private = TestRootOverride::set(registry.clone());
        initialize_root(&registry).unwrap();
        let primary = "e".repeat(64);
        let lease = open_lease(&registry.join(format!("{primary}.owner.lock"))).unwrap();
        lease.try_lock().unwrap();
        let record = Record {
            version: VERSION,
            normalization: "unknown-future-case-mapping".into(),
            primary: primary.clone(),
            resources: vec![Resource {
                locator: primary.clone(),
                ancestors: Vec::new(),
                directory: false,
            }],
        };
        std::fs::write(
            registry.join(format!("{primary}.json")),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
        assert!(ProfileOwner::acquire([fixture.path().join("payload.json")]).is_err());
    }

    #[test]
    fn profile_owner_registry_subtree_and_ancestor_are_never_configured_payloads() {
        let fixture = tempfile::tempdir().unwrap();
        let registry = fixture.path().join("registry");
        let _private = TestRootOverride::set(registry.clone());
        assert!(ProfileOwner::acquire([registry.join("injected.json")]).is_err());
        assert!(!registry.exists());
        assert!(ProfileOwner::acquire_resources([ProfileResource::Directory(
            fixture.path().to_path_buf()
        )])
        .is_err());
        assert!(!registry.exists());
        assert!(ProfileOwner::acquire([fixture.path().to_path_buf()]).is_err());
        assert!(!registry.exists());
    }

    #[test]
    fn profile_owner_stale_same_primary_record_is_safely_reused() {
        let fixture = tempfile::tempdir().unwrap();
        let registry = fixture.path().join("registry");
        let _private = TestRootOverride::set(registry.clone());
        let payload = fixture.path().join("payload.json");
        let owner = ProfileOwner::acquire([payload.clone()]).unwrap();
        let metadata_path = owner.0.registry.as_ref().unwrap().metadata_path.clone();
        let lease_path = owner.0.registry.as_ref().unwrap().lease_path.clone();
        drop(owner);
        assert!(metadata_path.exists());
        assert!(lease_path.exists());
        let successor = ProfileOwner::acquire([payload]).unwrap();
        assert_eq!(
            successor.0.registry.as_ref().unwrap().metadata_path,
            metadata_path
        );
        successor.ensure_owned().unwrap();
    }

    #[test]
    fn profile_owner_future_file_ancestor_of_registry_is_refused_before_registry_mkdir() {
        let fixture = tempfile::tempdir().unwrap();
        let future_file = fixture.path().join("future-payload");
        let registry = future_file.join("registry");
        let _private = TestRootOverride::set(registry.clone());
        assert!(ProfileOwner::acquire([future_file.clone()]).is_err());
        assert!(!future_file.exists());
        assert!(!registry.exists());
    }

    #[cfg(unix)]
    #[test]
    fn profile_owner_registry_root_symlink_is_refused() {
        let fixture = tempfile::tempdir().unwrap();
        let target = fixture.path().join("target");
        initialize_root(&target).unwrap();
        let locator = fixture.path().join("registry-link");
        std::os::unix::fs::symlink(&target, &locator).unwrap();
        let _private = TestRootOverride::set(locator);
        assert!(ProfileOwner::acquire([fixture.path().join("payload.json")]).is_err());
        assert_eq!(std::fs::read_dir(target).unwrap().count(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn profile_owner_retargeted_registry_parent_permanently_revokes_owner() {
        let fixture = tempfile::tempdir().unwrap();
        let first = fixture.path().join("first");
        let second = fixture.path().join("second");
        std::fs::create_dir(&first).unwrap();
        std::fs::create_dir(&second).unwrap();
        let parent = fixture.path().join("parent-link");
        std::os::unix::fs::symlink(&first, &parent).unwrap();
        let _private = TestRootOverride::set(parent.join("registry"));
        let owner = ProfileOwner::acquire([fixture.path().join("payload.json")]).unwrap();
        std::fs::remove_file(&parent).unwrap();
        std::os::unix::fs::symlink(&second, &parent).unwrap();
        assert!(owner.ensure_owned().is_err());
        std::fs::remove_file(&parent).unwrap();
        std::os::unix::fs::symlink(&first, &parent).unwrap();
        assert!(owner.ensure_owned().is_err());
        assert!(owner.lost_signal().is_cancelled());
    }

    #[cfg(windows)]
    #[test]
    fn profile_owner_active_registry_metadata_denies_write_and_delete() {
        let fixture = tempfile::tempdir().unwrap();
        let _private = TestRootOverride::set(fixture.path().join("registry"));
        let owner = ProfileOwner::acquire([fixture.path().join("payload.json")]).unwrap();
        let path = &owner.0.registry.as_ref().unwrap().metadata_path;
        assert!(std::fs::write(path, b"tampered").is_err());
        assert!(std::fs::remove_file(path).is_err());
        owner.ensure_owned().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn profile_owner_replaced_active_registry_metadata_permanently_revokes_owner() {
        let fixture = tempfile::tempdir().unwrap();
        let _private = TestRootOverride::set(fixture.path().join("registry"));
        let owner = ProfileOwner::acquire([fixture.path().join("payload.json")]).unwrap();
        let path = owner.0.registry.as_ref().unwrap().metadata_path.clone();
        let bytes = std::fs::read(&path).unwrap();
        let moved = path.with_extension("replaced");
        std::fs::rename(&path, &moved).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        assert!(owner.ensure_owned().is_err());
        std::fs::rename(moved, path).unwrap();
        assert!(owner.ensure_owned().is_err());
        assert!(owner.lost_signal().is_cancelled());
    }
}
