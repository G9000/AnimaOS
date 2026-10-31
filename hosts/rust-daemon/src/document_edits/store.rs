//! Bounded, content-addressed private staging under an already owned profile directory.
use crate::tools::workspace_bytes::{InstructionReadError, WorkspaceBytes};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock, Weak},
};

const MAX_CANDIDATE: usize = 4 * 1024 * 1024;
const MAX_STAGED_BYTES: u64 = 32 * 1024 * 1024;
const MAX_FILES: usize = 1024;
const MAX_READBACK_PROOFS: usize = 16;

/// Called only within the declared profile-directory initialization gate.
/// Existing roots are verified and never have their permissions reset.
pub(crate) fn initialize_owned_directory(path: &Path) -> Result<(), String> {
    if !path.is_absolute() {
        return Err("Document storage must be an absolute private directory".into());
    }
    let parent_path = path
        .parent()
        .ok_or("Document storage parent is unavailable")?;
    let name = path.file_name().ok_or("Document storage name is invalid")?;
    reject_reparse_directory(parent_path)?;
    let parent =
        WorkspaceBytes::open(parent_path).map_err(|_| "Document storage parent is unavailable")?;
    verify_private_directory(parent_path, &parent, true)?;
    let directory = parent
        .held_directory()
        .map_err(|_| "Document storage parent is unavailable")?;
    #[cfg(windows)]
    let created = {
        use win_custody::{current_user_sid_string, SecurityDescriptor};
        let user = current_user_sid_string()
            .map_err(|_| "Document private directory owner is unavailable")?;
        SecurityDescriptor::from_sddl(&format!("O:{user}D:P(A;OICI;FA;;;{user})(A;OICI;FA;;;SY)"))
            .map_err(|_| "Document private permissions are invalid")?
            .create_directory(path)
    };
    #[cfg(not(windows))]
    let created = {
        #[cfg(unix)]
        let mut builder = cap_std::fs::DirBuilder::new();
        #[cfg(not(unix))]
        let builder = cap_std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use cap_std::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        directory.create_dir_with(name, &builder)
    };
    match created {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(_) => return Err("Document private directory could not be created".into()),
    }
    let named = directory
        .symlink_metadata(name)
        .map_err(|_| "Document private directory is unavailable")?;
    if !named.is_dir() || crate::tools::workspace_bytes::linked(&named) {
        return Err("Document private directory is unsupported".into());
    }
    let held =
        WorkspaceBytes::open(path).map_err(|_| "Document private directory is unavailable")?;
    use cap_fs_ext::MetadataExt;
    let captured = held
        .held_directory()
        .and_then(|directory| directory.dir_metadata())
        .map_err(|_| "Document private directory is unavailable")?;
    if (named.dev(), named.ino()) != (captured.dev(), captured.ino()) {
        return Err("Document private directory changed".into());
    }
    verify_private_directory(path, &held, false)?;
    verify_private_directory(parent_path, &parent, true)
}

pub(crate) fn verify_private_directory(
    path: &Path,
    held: &WorkspaceBytes,
    parent: bool,
) -> Result<(), String> {
    use cap_fs_ext::MetadataExt;
    let directory = held
        .held_directory()
        .map_err(|_| "Document private directory is unavailable")?;
    let before = directory
        .dir_metadata()
        .map_err(|_| "Document private directory is unavailable")?;
    #[cfg(windows)]
    {
        use win_custody::{current_user_sid_string, ObjectSecurityDescriptor, OwnedSid};
        let _ = path;
        let file = directory
            .try_clone()
            .map_err(|_| "Document private directory is unavailable")?
            .into_std_file();
        let descriptor = ObjectSecurityDescriptor::query_file_handle(&file)
            .map_err(|_| "Document private permissions are unavailable")?;
        let user = OwnedSid::from_string(
            &current_user_sid_string().map_err(|_| "Document private owner is unavailable")?,
        )
        .map_err(|_| "Document private owner is invalid")?;
        verify_windows_descriptor(&descriptor, &user, parent)?;
    }
    #[cfg(unix)]
    {
        use cap_std::fs::PermissionsExt;
        let _ = (path, parent);
        if before.permissions().mode() & 0o077 != 0 {
            return Err("Document storage is not private to its owner".into());
        }
    }
    let after = directory
        .dir_metadata()
        .map_err(|_| "Document private directory is unavailable")?;
    if (before.dev(), before.ino()) != (after.dev(), after.ino()) {
        return Err("Document private directory changed".into());
    }
    Ok(())
}

fn reject_reparse_directory(path: &Path) -> Result<(), String> {
    let metadata =
        std::fs::symlink_metadata(path).map_err(|_| "Document private directory is unavailable")?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("Document private directory is unsupported".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("Document private directory is unsupported".into());
        }
    }
    Ok(())
}

#[cfg(windows)]
fn verify_windows_descriptor(
    descriptor: &win_custody::ObjectSecurityDescriptor,
    user: &win_custody::OwnedSid,
    parent: bool,
) -> Result<(), String> {
    use win_custody::OwnedSid;
    let system =
        OwnedSid::from_string("S-1-5-18").map_err(|_| "Document system owner is invalid")?;
    let admins = OwnedSid::from_string("S-1-5-32-544")
        .map_err(|_| "Document administrator owner is invalid")?;
    let creator =
        OwnedSid::from_string("S-1-3-0").map_err(|_| "Document creator owner is invalid")?;
    let owner = descriptor
        .owner()
        .map_err(|_| "Document private owner is unavailable")?;
    if !owner.matches(user) && !owner.matches(&system) && !owner.matches(&admins) {
        return Err("Document storage owner is untrusted".into());
    }
    let dacl = descriptor
        .dacl()
        .map_err(|_| "Document private permissions are unavailable")?;
    let mut user_access = 0;
    for ace in dacl.allowed_aces() {
        if ace.trustee_matches(user) {
            if ace.flags() & 0x08 == 0 {
                user_access |= map_generic_file_access(ace.mask());
            }
        } else if ace.trustee_matches(&system)
            || (parent && ace.trustee_matches(&admins))
            // An inherit-only CREATOR OWNER entry resolves to our explicit
            // owner on the fresh child; it grants nobody control of the parent.
            || (parent && ace.flags() & 0x08 != 0 && ace.trustee_matches(&creator))
        {
            continue;
        } else if !parent || !parent_read_only_access(ace.mask()) {
            return Err("Document storage is not private to its owner".into());
        }
    }
    if user_access & 0x001f01ff != 0x001f01ff
        || (!parent
            && descriptor
                .control()
                .map_err(|_| "Document private permissions are unavailable")?
                & 0x1000
                == 0)
    {
        return Err("Document storage is not private to its owner".into());
    }
    Ok(())
}

#[cfg(windows)]
fn map_generic_file_access(mut mask: u32) -> u32 {
    for (generic, mapped) in [
        (0x80000000, 0x00120089), // GENERIC_READ
        (0x40000000, 0x00120116), // GENERIC_WRITE
        (0x20000000, 0x001200a0), // GENERIC_EXECUTE
        (0x10000000, 0x001f01ff), // GENERIC_ALL
    ] {
        if mask & generic != 0 {
            mask = (mask & !generic) | mapped;
        }
    }
    mask
}

#[cfg(windows)]
fn parent_read_only_access(mask: u32) -> bool {
    // Unknown rights fail closed, including inheritable ACEs. Only list/read,
    // read EA/attributes, traverse, READ_CONTROL and SYNCHRONIZE are harmless
    // on the parent of an atomically protected private child.
    mask & (0x40000000 | 0x10000000) == 0 && map_generic_file_access(mask) & !0x001200a9 == 0
}

/// Retains a directory capability and a physical-operation lock across clones.
/// The caller creates and owns this dedicated profile directory before opening it.
#[derive(Clone)]
pub(crate) struct DocumentEditStore {
    root: Arc<WorkspaceBytes>,
    lock: Arc<Mutex<BTreeMap<String, usize>>>,
    max_bytes: u64,
    owner: Option<crate::profile_owner::ProfileOwner>,
}

impl DocumentEditStore {
    /// Creates or verifies the one private inspection child without accepting arbitrary directories.
    pub(crate) fn inspection_directory(&self) -> Result<PathBuf, String> {
        let root = self.canonical_root()?;
        let path = root.join("inspections");
        initialize_owned_directory(&path)?;
        Ok(path)
    }
    fn inspection_entry(&self, name: &std::ffi::OsStr) -> Result<bool, String> {
        if name != std::ffi::OsStr::new("inspections") {
            return Ok(false);
        }
        let path = self.root.canonical_root.join(name);
        reject_reparse_directory(&path)?;
        let held = WorkspaceBytes::open(&path)
            .map_err(|_| "Document inspection directory is unavailable")?;
        verify_private_directory(&path, &held, false)?;
        use cap_fs_ext::MetadataExt;
        let named = self
            .root
            .held_directory()
            .and_then(|directory| directory.symlink_metadata(name))
            .map_err(|_| "Document inspection directory is unavailable")?;
        let captured = held
            .held_directory()
            .and_then(|directory| directory.dir_metadata())
            .map_err(|_| "Document inspection directory is unavailable")?;
        if !named.is_dir()
            || crate::tools::workspace_bytes::linked(&named)
            || (named.dev(), named.ino()) != (captured.dev(), captured.ino())
        {
            return Err("Document inspection directory custody changed".into());
        }
        Ok(true)
    }
    pub(crate) fn ensure_readback_capacity(&self) -> Result<(), String> {
        if self.readback_proof_ids()?.len() >= MAX_READBACK_PROOFS {
            Err("Document proof capacity is full".into())
        } else {
            Ok(())
        }
    }
    pub(crate) fn write_readback_proof(
        &self,
        proof: &super::recovery::PublicationReadbackProof,
    ) -> Result<(), String> {
        self.ensure_owner()?;
        let name = proof_name(proof.proposal_id())?;
        let bytes = serde_json::to_vec(proof).map_err(|_| "Document readback proof is invalid")?;
        if bytes.len() > super::recovery::MAX_PROOF_BYTES {
            return Err("Document readback proof exceeds its limit".into());
        }
        let _guard = self
            .lock
            .lock()
            .map_err(|_| "Document storage is unavailable")?;
        match self
            .root
            .read_file_snapshot(&name, super::recovery::MAX_PROOF_BYTES)
        {
            Ok(existing) => {
                if self.root.instruction_link_count(&name).ok() != Some(Some(1)) {
                    return Err("Document readback proof is unavailable".into());
                }
                let existing: super::recovery::PublicationReadbackProof =
                    serde_json::from_slice(&existing)
                        .map_err(|_| "Document readback proof is invalid")?;
                if existing.same_publication(proof) {
                    return Ok(());
                } else {
                    return Err("Document readback proof identity changed".into());
                }
            }
            Err(InstructionReadError::Missing) => {}
            Err(_) => return Err("Document readback proof is unavailable".into()),
        }
        let directory = self
            .root
            .held_directory()
            .map_err(|_| "Document storage is unavailable")?;
        let mut proof_bytes = 0u64;
        let mut proof_count = 0usize;
        let mut count = 0usize;
        for entry in directory
            .entries()
            .map_err(|_| "Document storage is unavailable")?
        {
            let entry = entry.map_err(|_| "Document storage is unavailable")?;
            if self.inspection_entry(&entry.file_name())? {
                continue;
            }
            count += 1;
            if count >= MAX_FILES {
                return Err("Document staging capacity is full".into());
            }
            let metadata = directory
                .symlink_metadata(entry.file_name())
                .map_err(|_| "Document storage is unavailable")?;
            if !metadata.is_file() || crate::tools::workspace_bytes::linked(&metadata) {
                return Err("Document storage entry is unsupported".into());
            }
            if entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with("proof_"))
            {
                proof_count += 1;
                proof_bytes = proof_bytes
                    .checked_add(metadata.len())
                    .ok_or("Document proof capacity is full")?;
            }
        }
        // Proof metadata has its own reservation budget; it cannot consume the
        // 32 MiB candidate allowance after a file effect has already occurred.
        if proof_count >= MAX_READBACK_PROOFS
            || proof_bytes.saturating_add(bytes.len() as u64)
                > (MAX_READBACK_PROOFS * super::recovery::MAX_PROOF_BYTES) as u64
        {
            return Err("Document proof capacity is full".into());
        }
        self.root
            .create_new_bytes_atomic(&name, &bytes, || {
                self.ensure_owner().map_err(std::io::Error::other)
            })
            .map_err(|_| "Document readback proof could not be saved".to_string())
    }
    pub(crate) fn readback_proof(
        &self,
        id: &str,
    ) -> Result<Option<super::recovery::PublicationReadbackProof>, String> {
        self.ensure_owner()?;
        let name = proof_name(id)?;
        let _guard = self
            .lock
            .lock()
            .map_err(|_| "Document storage is unavailable")?;
        let bytes = match self
            .root
            .read_file_snapshot(&name, super::recovery::MAX_PROOF_BYTES)
        {
            Ok(bytes) => bytes,
            Err(InstructionReadError::Missing) => return Ok(None),
            Err(_) => return Err("Document readback proof is unavailable".into()),
        };
        if self.root.instruction_link_count(&name).ok() != Some(Some(1)) {
            return Err("Document readback proof is unavailable".into());
        }
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| "Document readback proof is invalid".into())
    }
    pub(crate) fn retire_readback_proof<G>(
        &self,
        id: &str,
        authorize: impl FnOnce() -> Result<G, String>,
    ) -> Result<(), String> {
        self.ensure_owner()?;
        let name = proof_name(id)?;
        let _guard = self
            .lock
            .lock()
            .map_err(|_| "Document storage is unavailable")?;
        let authority = authorize()?;
        let directory = self
            .root
            .held_directory()
            .map_err(|_| "Document storage is unavailable")?;
        match directory.symlink_metadata(&name) {
            Ok(metadata)
                if metadata.is_file() && !crate::tools::workspace_bytes::linked(&metadata) =>
            {
                directory
                    .remove_file(&name)
                    .map_err(|_| "Document readback proof could not be retired")?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            _ => return Err("Document readback proof is unavailable".into()),
        }
        drop(authority);
        Ok(())
    }
    pub(crate) fn candidate_hashes(&self) -> Result<Vec<String>, String> {
        self.ensure_owner()?;
        let _guard = self
            .lock
            .lock()
            .map_err(|_| "Document storage is unavailable")?;
        let directory = self
            .root
            .held_directory()
            .map_err(|_| "Document storage is unavailable")?;
        let mut hashes = Vec::new();
        let mut entries = 0usize;
        for entry in directory
            .entries()
            .map_err(|_| "Document storage is unavailable")?
        {
            entries += 1;
            if entries > MAX_FILES {
                return Err("Document storage entry limit exceeded".into());
            }
            let entry = entry.map_err(|_| "Document storage is unavailable")?;
            if self.inspection_entry(&entry.file_name())? {
                continue;
            }
            let name = entry
                .file_name()
                .to_str()
                .ok_or("Document storage entry is invalid")?
                .to_string();
            if name
                .strip_prefix("proof_")
                .and_then(|name| name.strip_suffix(".json"))
                .is_some_and(super::registry::valid_id)
            {
                continue;
            }
            let hash = name
                .strip_suffix(".blob")
                .filter(|hash| super::registry::valid_hash(hash))
                .ok_or("Document storage entry is invalid")?;
            hashes.push(hash.to_string());
        }
        Ok(hashes)
    }
    pub(crate) fn readback_proof_ids(&self) -> Result<Vec<String>, String> {
        self.ensure_owner()?;
        let _guard = self
            .lock
            .lock()
            .map_err(|_| "Document storage is unavailable")?;
        let directory = self
            .root
            .held_directory()
            .map_err(|_| "Document storage is unavailable")?;
        let mut ids = Vec::new();
        let mut count = 0usize;
        for entry in directory
            .entries()
            .map_err(|_| "Document storage is unavailable")?
        {
            count += 1;
            if count > MAX_FILES {
                return Err("Document storage entry limit exceeded".into());
            }
            let entry = entry.map_err(|_| "Document storage is unavailable")?;
            if self.inspection_entry(&entry.file_name())? {
                continue;
            }
            let name = entry.file_name();
            let name = name.to_str().ok_or("Document storage entry is invalid")?;
            if let Some(id) = name
                .strip_prefix("proof_")
                .and_then(|name| name.strip_suffix(".json"))
            {
                if !super::registry::valid_id(id) {
                    return Err("Document proof identity is invalid".into());
                }
                ids.push(id.to_string());
            } else if !name
                .strip_suffix(".blob")
                .is_some_and(super::registry::valid_hash)
            {
                return Err("Document storage entry is invalid".into());
            }
        }
        Ok(ids)
    }
    pub(crate) fn with_owner(
        mut self,
        owner: crate::profile_owner::ProfileOwner,
    ) -> Result<Self, String> {
        owner
            .ensure_owned()
            .map_err(|_| "Document profile ownership was lost")?;
        self.owner = Some(owner);
        Ok(self)
    }
    fn ensure_owner(&self) -> Result<(), String> {
        self.owner.as_ref().map_or(Ok(()), |owner| {
            owner
                .ensure_owned()
                .map_err(|_| "Document profile ownership was lost".into())
        })
    }
    pub(crate) fn canonical_root(&self) -> Result<&Path, String> {
        use cap_fs_ext::MetadataExt;
        self.ensure_owner()?;
        let named = WorkspaceBytes::open(&self.root.canonical_root)
            .map_err(|_| "Document storage custody is unavailable")?;
        let named = named
            .held_directory()
            .and_then(|directory| directory.dir_metadata())
            .map_err(|_| "Document storage custody is unavailable")?;
        let held = self
            .root
            .held_directory()
            .and_then(|directory| directory.dir_metadata())
            .map_err(|_| "Document storage custody is unavailable")?;
        if (named.dev(), named.ino()) != (held.dev(), held.ino()) {
            return Err("Document storage custody changed".into());
        }
        verify_private_directory(&self.root.canonical_root, &self.root, false)?;
        Ok(&self.root.canonical_root)
    }
    pub(crate) fn profile_key(&self) -> String {
        hex::encode(Sha256::digest(
            self.root
                .canonical_root
                .as_os_str()
                .to_string_lossy()
                .as_bytes(),
        ))
    }
    pub(crate) fn open_existing(root: &Path) -> Result<Self, String> {
        Self::open_with_limit(root, MAX_STAGED_BYTES)
    }
    fn open_with_limit(root: &Path, max_bytes: u64) -> Result<Self, String> {
        if !root.is_absolute() {
            return Err("Document storage requires an absolute owned directory".into());
        }
        reject_reparse_directory(root)?;
        let root = WorkspaceBytes::open(root).map_err(|_| "Document storage is unavailable")?;
        verify_private_directory(&root.canonical_root, &root, false)?;
        let lock = shared_lock(&root.canonical_root);
        Ok(Self {
            root: Arc::new(root),
            lock,
            max_bytes,
            owner: None,
        })
    }
    pub(crate) fn stage(&self, bytes: &[u8]) -> Result<StagedCandidate, String> {
        self.ensure_owner()?;
        if bytes.is_empty() || bytes.len() > MAX_CANDIDATE {
            return Err("Document candidate exceeds its storage limit".into());
        }
        let hash = hex::encode(Sha256::digest(bytes));
        let name = blob_name(&hash)?;
        let mut guard = self
            .lock
            .lock()
            .map_err(|_| "Document storage is unavailable")?;
        match self.root.read_file_snapshot(&name, MAX_CANDIDATE) {
            Ok(existing)
                if existing == bytes
                    && self.root.instruction_link_count(&name).ok() == Some(Some(1)) =>
            {
                *guard.entry(hash.clone()).or_default() += 1;
                return Ok(StagedCandidate {
                    hash,
                    size: bytes.len() as u64,
                    lock: self.lock.clone(),
                });
            }
            Ok(_) => return Err("Document candidate identity does not match storage".into()),
            Err(InstructionReadError::Missing) => {}
            Err(_) => return Err("Document candidate storage is unavailable".into()),
        }
        let directory = self
            .root
            .held_directory()
            .map_err(|_| "Document storage is unavailable")?;
        let mut total = 0u64;
        let mut count = 0usize;
        for entry in directory
            .entries()
            .map_err(|_| "Document storage is unavailable")?
        {
            let entry = entry.map_err(|_| "Document storage is unavailable")?;
            if self.inspection_entry(&entry.file_name())? {
                continue;
            }
            count += 1;
            if count > MAX_FILES {
                return Err("Document staging capacity is full".into());
            }
            let metadata = directory
                .symlink_metadata(entry.file_name())
                .map_err(|_| "Document storage is unavailable")?;
            if !metadata.is_file() || crate::tools::workspace_bytes::linked(&metadata) {
                return Err("Document storage contains an unsupported entry".into());
            }
            if entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.ends_with(".blob"))
            {
                total = total
                    .checked_add(metadata.len())
                    .ok_or("Document staging capacity is full")?;
            }
        }
        if total
            .checked_add(bytes.len() as u64)
            .is_none_or(|value| value > self.max_bytes)
        {
            return Err("Document staging capacity is full".into());
        }
        self.root
            .create_new_bytes_atomic(&name, bytes, || {
                self.ensure_owner().map_err(std::io::Error::other)
            })
            .map_err(|_| "Document candidate could not be staged")?;
        *guard.entry(hash.clone()).or_default() += 1;
        Ok(StagedCandidate {
            hash,
            size: bytes.len() as u64,
            lock: self.lock.clone(),
        })
    }
    pub(crate) fn load(&self, hash: &str, expected_bytes: u64) -> Result<Vec<u8>, String> {
        self.ensure_owner()?;
        let name = blob_name(hash)?;
        if expected_bytes == 0 || expected_bytes > MAX_CANDIDATE as u64 {
            return Err("Document candidate size is invalid".into());
        }
        let guard = self
            .lock
            .lock()
            .map_err(|_| "Document storage is unavailable")?;
        if guard.get(hash).copied().unwrap_or(0) != 0 {
            return Err("Document candidate is still being staged".into());
        }
        let bytes = self
            .root
            .read_file_snapshot(&name, expected_bytes as usize)
            .map_err(|_| "Document candidate is unavailable")?;
        if self.root.instruction_link_count(&name).ok() != Some(Some(1))
            || bytes.len() as u64 != expected_bytes
            || hex::encode(Sha256::digest(&bytes)) != hash
        {
            return Err("Document candidate no longer matches its review".into());
        }
        Ok(bytes)
    }
    /// The callback checks the current registry and returns a guard held across removal.
    /// Never expose this callback to model or HTTP input; call only after source readers retire.
    pub(crate) fn retire_if_unreferenced<G>(
        &self,
        hash: &str,
        authorize: impl FnOnce() -> Result<G, String>,
    ) -> Result<(), String> {
        self.ensure_owner()?;
        let name = blob_name(hash)?;
        let guard = self
            .lock
            .lock()
            .map_err(|_| "Document storage is unavailable")?;
        if guard.get(hash).copied().unwrap_or(0) != 0 {
            return Err("Document candidate still has a physical reader".into());
        }
        let authority = authorize()?;
        let directory = self
            .root
            .held_directory()
            .map_err(|_| "Document storage is unavailable")?;
        let metadata = directory
            .symlink_metadata(&name)
            .map_err(|_| "Document candidate is unavailable")?;
        if !metadata.is_file() || crate::tools::workspace_bytes::linked(&metadata) {
            return Err("Document candidate is unavailable".into());
        }
        directory
            .remove_file(&name)
            .map_err(|_| "Document candidate could not be retired".to_string())?;
        drop(authority);
        Ok(())
    }
}

fn proof_name(id: &str) -> Result<String, String> {
    if !super::registry::valid_id(id) {
        return Err("Document readback proof identity is invalid".into());
    }
    Ok(format!("proof_{id}.json"))
}

/// Holds an in-flight reference until proposal registration is durably settled.
pub(crate) struct StagedCandidate {
    hash: String,
    size: u64,
    lock: Arc<Mutex<BTreeMap<String, usize>>>,
}
impl StagedCandidate {
    pub(crate) fn hash(&self) -> &str {
        &self.hash
    }
    pub(crate) fn size(&self) -> u64 {
        self.size
    }
}
impl Drop for StagedCandidate {
    fn drop(&mut self) {
        let mut guard = self
            .lock
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if let Some(count) = guard.get_mut(&self.hash) {
            *count -= 1;
            if *count == 0 {
                guard.remove(&self.hash);
            }
        }
    }
}

fn shared_lock(root: &Path) -> Arc<Mutex<BTreeMap<String, usize>>> {
    static LOCKS: OnceLock<Mutex<BTreeMap<PathBuf, Weak<Mutex<BTreeMap<String, usize>>>>>> =
        OnceLock::new();
    let mut locks = LOCKS
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    locks.retain(|_, lock| lock.strong_count() > 0);
    if let Some(lock) = locks.get(root).and_then(Weak::upgrade) {
        return lock;
    }
    let lock = Arc::new(Mutex::new(BTreeMap::new()));
    locks.insert(root.to_path_buf(), Arc::downgrade(&lock));
    lock
}

fn blob_name(hash: &str) -> Result<String, String> {
    if !super::registry::valid_hash(hash) {
        return Err("Document candidate identity is invalid".into());
    }
    Ok(format!("{hash}.blob"))
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "store_test_support.rs"]
pub(crate) mod test_support;

#[cfg(all(test, windows))]
#[path = "store_windows_tests.rs"]
mod windows_tests;
