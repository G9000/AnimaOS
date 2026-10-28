use super::workspace::*;
use super::*;
use anima_core::filesystem_checkpoints::{
    checkpoint_digest, checkpoint_scope_digest, PathSemantics,
};
use cap_fs_ext::{FollowSymlinks, MetadataExt, OpenOptionsFollowExt, OpenOptionsSyncExt};
use cap_std::fs::{Dir, File, OpenOptions};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write};

pub(super) struct CheckpointStore {
    pub(super) config: CaptureConfig,
    #[cfg(test)]
    fault: Option<TestPublishFault>,
    #[cfg(test)]
    pub(super) restore_fault: Option<TestRestoreFault>,
    #[cfg(test)]
    pub(super) restore_hook: Option<Box<dyn FnMut(TestRestorePhase) + Send>>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Commit {
    version: u32,
    scope_digest: String,
    sequence: u64,
    checkpoint_id: String,
    manifest_digest: String,
}

pub(super) struct Session {
    pub(super) workspace: HeldRoot,
    pub(super) store: HeldRoot,
    pub(super) partition: Dir,
    _parents: Vec<Dir>,
    lock_directory: Dir,
    lock: Option<File>,
    lock_identity: (u64, u64),
}

impl Drop for Session {
    fn drop(&mut self) {
        if self.lock.take().is_none() {
            return;
        }
        // Remove only our lock; preserve an observed substitution.
        if let Ok(metadata) = self.lock_directory.symlink_metadata("capture.lock") {
            if !linked(&metadata) && (metadata.dev(), metadata.ino()) == self.lock_identity {
                let _ = self.lock_directory.remove_file("capture.lock");
            }
        }
    }
}

struct Counter(usize, usize);
impl Write for Counter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 = self
            .0
            .checked_add(bytes.len())
            .ok_or_else(|| io::Error::other("budget"))?;
        if self.0 > self.1 {
            return Err(io::Error::other("budget"));
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
pub(super) fn encode(value: &impl Serialize, max: usize) -> Result<Vec<u8>, CaptureError> {
    serde_json::to_writer(Counter(0, max), value).map_err(|_| CaptureError::Budget)?;
    serde_json::to_vec(value).map_err(|_| CaptureError::Corrupt)
}
pub(super) fn digest_valid(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub(super) fn child(directory: &Dir, name: &str) -> Result<Dir, CaptureError> {
    match directory.create_dir(name) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(io_error(error)),
    }
    open_child(directory, name)
}
/// Immutable create only: never truncate/replace a collision.
pub(super) fn create(directory: &Dir, name: &str, bytes: &[u8]) -> Result<(), CaptureError> {
    let mut options = OpenOptions::new();
    options
        .write(true)
        .create_new(true)
        .follow(FollowSymlinks::No)
        .nonblock(true);
    let mut file = directory.open_with(name, &options).map_err(io_error)?;
    let metadata = file.metadata().map_err(io_error)?;
    if linked(&metadata) || !metadata.is_file() || metadata.nlink() != 1 {
        return Err(CaptureError::Corrupt);
    }
    file.write_all(bytes).map_err(io_error)?;
    file.sync_all().map_err(io_error)?;
    #[cfg(unix)]
    directory
        .try_clone()
        .map_err(io_error)?
        .into_std_file()
        .sync_all()
        .map_err(io_error)?;
    Ok(())
}
pub(super) fn inventory(
    directory: &Dir,
    depth: usize,
    remaining: &mut usize,
    bytes: &mut u64,
    limit: u64,
) -> Result<(), CaptureError> {
    if depth > 64 {
        return Err(CaptureError::Budget);
    }
    for name in names(directory, remaining)? {
        let metadata = directory.symlink_metadata(&name).map_err(io_error)?;
        if linked(&metadata) {
            return Err(CaptureError::Corrupt);
        }
        if metadata.is_dir() {
            inventory(
                &open_child(directory, &name)?,
                depth + 1,
                remaining,
                bytes,
                limit,
            )?;
        } else if metadata.is_file() && metadata.nlink() == 1 {
            *bytes = bytes
                .checked_add(metadata.len())
                .ok_or(CaptureError::Budget)?;
            if *bytes > limit {
                return Err(CaptureError::Budget);
            }
        } else {
            return Err(CaptureError::Corrupt);
        }
    }
    Ok(())
}

impl CheckpointStore {
    pub(super) fn new(config: CaptureConfig) -> Self {
        Self {
            config,
            #[cfg(test)]
            fault: None,
            #[cfg(test)]
            restore_fault: None,
            #[cfg(test)]
            restore_hook: None,
        }
    }
    pub(super) fn validate_config(&self) -> Result<(), CaptureError> {
        let scope = &self.config.scope;
        for id in [&scope.owner_id, &scope.workspace_id, &scope.environment_id] {
            if id.is_empty() || id.len() > 128 || id.chars().any(char::is_control) {
                return Err(CaptureError::InvalidScope);
            }
        }
        if !digest_valid(&scope.root_identity_digest)
            || !digest_valid(&scope.policy_digest)
            || (cfg!(windows) && scope.path_semantics != PathSemantics::WindowsInsensitive)
            || (cfg!(unix) && scope.path_semantics != PathSemantics::PosixSensitive)
        {
            return Err(CaptureError::InvalidScope);
        }
        let limits = &self.config.policy.limits;
        let maxima = CaptureLimits::default();
        if limits.file_bytes == 0
            || limits.file_bytes > maxima.file_bytes
            || limits.content_bytes == 0
            || limits.content_bytes > maxima.content_bytes
            || limits.entries == 0
            || limits.entries > maxima.entries
            || limits.manifest_bytes == 0
            || limits.manifest_bytes > maxima.manifest_bytes
            || limits.store_bytes == 0
            || limits.store_bytes > maxima.store_bytes
            || limits.history_records == 0
            || limits.history_records > maxima.history_records
        {
            return Err(CaptureError::Budget);
        }
        encode(&self.config.policy, 64 * 1024)?;
        if digest_value(
            b"anima-filesystem-checkpoint-capture-policy-v1\0",
            &self.config.policy,
        )? != scope.policy_digest
        {
            return Err(CaptureError::InvalidScope);
        }
        let mut prior = None;
        for prefix in &self.config.policy.excluded_prefixes {
            let key = path_key(prefix, scope.path_semantics)?;
            if prior.as_ref().is_some_and(|last: &String| last >= &key) {
                return Err(CaptureError::UnsafePath);
            }
            prior = Some(key);
        }
        if !self
            .config
            .policy
            .excluded_prefixes
            .iter()
            .any(|p| p.eq_ignore_ascii_case(".git"))
        {
            return Err(CaptureError::UnsafePath);
        }
        Ok(())
    }
    pub(super) fn session(&self) -> Result<Session, CaptureError> {
        self.validate_config()?;
        let workspace = HeldRoot::open(&self.config.workspace_root)?;
        if workspace.identity()? != self.config.scope.root_identity_digest {
            return Err(CaptureError::InvalidScope);
        }
        let store = HeldRoot::open(&self.config.store_root)?;
        provision::verify_private_root(&self.config.store_root, &store, false)?;
        // Original ancestors are already nofollow-opened and held. Subsequent
        // operations use capabilities, never these containment-check paths.
        let workspace_path = self
            .config
            .workspace_root
            .canonicalize()
            .map_err(io_error)?;
        let store_path = self.config.store_root.canonicalize().map_err(io_error)?;
        if store_path.starts_with(&workspace_path)
            || workspace_path.starts_with(&store_path)
            || store.identity()? == workspace.identity()?
        {
            return Err(CaptureError::InvalidScope);
        }
        let mut options = OpenOptions::new();
        options
            .read(true)
            .write(true)
            .create_new(true)
            .follow(FollowSymlinks::No)
            .nonblock(true);
        let lock = store
            .directory
            .open_with("capture.lock", &options)
            .map_err(|error| {
                if error.kind() == io::ErrorKind::AlreadyExists {
                    CaptureError::RecoveryRequired
                } else {
                    io_error(error)
                }
            })?;
        let lock_metadata = lock.metadata().map_err(io_error)?;
        let lock_identity = (lock_metadata.dev(), lock_metadata.ino());
        let lock_directory = store.directory.try_clone().map_err(io_error)?;
        let mut session = Session {
            partition: store.directory.try_clone().map_err(io_error)?,
            workspace,
            store,
            _parents: Vec::new(),
            lock_directory,
            lock: Some(lock),
            lock_identity,
        };
        let scopes = child(&session.store.directory, "scopes")?;
        session.partition = child(&scopes, &checkpoint_scope_digest(&self.config.scope))?;
        session._parents.push(scopes);
        for name in ["objects", "manifests", "commits", "acks"] {
            child(&session.partition, name)?;
        }
        let mut count = 200_000;
        let mut bytes = 0;
        inventory(
            &session.store.directory,
            0,
            &mut count,
            &mut bytes,
            self.config.policy.limits.store_bytes,
        )?;
        Ok(session)
    }
    /// Inspect only an existing partition. Immutable evidence reads must never
    /// provision storage, create a capture lock, or repair partial metadata.
    pub(super) fn read_session(&self) -> Result<Session, CaptureError> {
        self.validate_config()?;
        let workspace = HeldRoot::open(&self.config.workspace_root)?;
        if workspace.identity()? != self.config.scope.root_identity_digest {
            return Err(CaptureError::InvalidScope);
        }
        let store = HeldRoot::open(&self.config.store_root)?;
        provision::verify_private_root(&self.config.store_root, &store, false)?;
        let workspace_path = self
            .config
            .workspace_root
            .canonicalize()
            .map_err(io_error)?;
        let store_path = self.config.store_root.canonicalize().map_err(io_error)?;
        if store_path.starts_with(&workspace_path)
            || workspace_path.starts_with(&store_path)
            || store.identity()? == workspace.identity()?
        {
            return Err(CaptureError::InvalidScope);
        }
        match store.directory.symlink_metadata("capture.lock") {
            Ok(_) => return Err(CaptureError::RecoveryRequired),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(io_error(error)),
        }
        let scopes = open_child(&store.directory, "scopes")?;
        let partition = open_child(&scopes, &checkpoint_scope_digest(&self.config.scope))?;
        let lock_directory = store.directory.try_clone().map_err(io_error)?;
        let mut count = 200_000;
        let mut bytes = 0;
        inventory(
            &store.directory,
            0,
            &mut count,
            &mut bytes,
            self.config.policy.limits.store_bytes,
        )?;
        Ok(Session {
            workspace,
            store,
            partition,
            _parents: vec![scopes],
            lock_directory,
            lock: None,
            lock_identity: (0, 0),
        })
    }
    pub(super) fn selections(
        &self,
        directory: &Dir,
        selection: &CaptureSelection,
    ) -> Result<(Vec<String>, Vec<String>), CaptureError> {
        // Bound the complete caller input before cloning path strings.
        encode(selection, self.config.policy.limits.manifest_bytes)?;
        let mut paths = Vec::new();
        let mut omitted = Vec::new();
        match selection {
            CaptureSelection::Workspace {} => {
                let mut remaining = self.config.policy.limits.entries;
                collect_paths(
                    directory,
                    "",
                    &self.config.policy,
                    self.config.scope.path_semantics,
                    &mut paths,
                    &mut omitted,
                    &mut remaining,
                )?;
                paths.sort();
                omitted.sort();
            }
            CaptureSelection::Paths { paths: selected } => {
                if selected.len() > self.config.policy.limits.entries {
                    return Err(CaptureError::Budget);
                }
                paths = selected.clone();
            }
        }
        let mut seen = BTreeSet::new();
        let mut prior: Option<&str> = None;
        for path in &paths {
            let key = path_key(path, self.config.scope.path_semantics)?;
            if excluded(path, &self.config.policy, self.config.scope.path_semantics)
                || prior.is_some_and(|previous| previous >= path.as_str())
                || !seen.insert(key)
            {
                return Err(CaptureError::UnsafePath);
            }
            prior = Some(path);
        }
        Ok((paths, omitted))
    }
    pub(super) fn observe(
        &self,
        directory: &Dir,
        paths: &[String],
    ) -> Result<(Vec<CapturedEntry>, BTreeMap<String, Vec<u8>>, u64), CaptureError> {
        let mut entries = Vec::new();
        let mut blobs = BTreeMap::new();
        let mut total = 0u64;
        for path in paths {
            let (state, bytes) =
                workspace::observe(directory, path, self.config.policy.limits.file_bytes)?;
            if let Some(bytes) = bytes {
                total = total
                    .checked_add(bytes.len() as u64)
                    .ok_or(CaptureError::Budget)?;
                if total > self.config.policy.limits.content_bytes {
                    return Err(CaptureError::Budget);
                }
                blobs.insert(checkpoint_digest(&bytes), bytes);
            }
            entries.push(CapturedEntry {
                path: path.clone(),
                state,
            });
        }
        Ok((entries, blobs, total))
    }
    pub(super) fn history(&self, session: &Session) -> Result<Vec<CaptureRecord>, CaptureError> {
        let commits = open_child(&session.partition, "commits")?;
        let manifests = open_child(&session.partition, "manifests")?;
        let acks = open_child(&session.partition, "acks")?;
        let mut remaining = self.config.policy.limits.history_records;
        let commit_names = names(&commits, &mut remaining)?;
        let mut records = Vec::new();
        for (index, name) in commit_names.iter().enumerate() {
            let sequence = index as u64 + 1;
            if name != &format!("{sequence:020}.json") {
                return Err(CaptureError::Corrupt);
            }
            let (bytes, _) = read_file(&commits, name, 4096)?;
            let commit: Commit =
                serde_json::from_slice(&bytes).map_err(|_| CaptureError::RecoveryRequired)?;
            if commit.version != 1
                || commit.sequence != sequence
                || commit.scope_digest != checkpoint_scope_digest(&self.config.scope)
                || !digest_valid(&commit.manifest_digest)
                || !digest_valid(&commit.checkpoint_id)
            {
                return Err(CaptureError::Corrupt);
            }
            let (ack, _) =
                read_file(&acks, name, 64).map_err(|_| CaptureError::RecoveryRequired)?;
            if ack != checkpoint_digest(&bytes).as_bytes() {
                return Err(CaptureError::RecoveryRequired);
            }
            let (manifest, _) = read_file(
                &manifests,
                &format!("{}.json", commit.manifest_digest),
                self.config.policy.limits.manifest_bytes as u64,
            )?;
            if checkpoint_digest(&manifest) != commit.manifest_digest {
                return Err(CaptureError::Corrupt);
            }
            let record: CaptureRecord =
                serde_json::from_slice(&manifest).map_err(|_| CaptureError::Corrupt)?;
            self.validate_record(&record)?;
            if record.checkpoint_id != commit.checkpoint_id || record.sequence != sequence {
                return Err(CaptureError::Corrupt);
            }
            records.push(record);
        }
        Ok(records)
    }
    fn validate_record(&self, record: &CaptureRecord) -> Result<(), CaptureError> {
        if record.version != 1
            || record.profile != self.config.policy.profile
            || !digest_valid(&record.checkpoint_id)
            || record.scope_digest != checkpoint_scope_digest(&self.config.scope)
            || record.entries.len() > self.config.policy.limits.entries
        {
            return Err(CaptureError::Corrupt);
        }
        let mut keys = BTreeSet::new();
        let mut present = BTreeMap::new();
        let mut last: Option<&str> = None;
        let mut total = 0u64;
        for entry in &record.entries {
            let key = path_key(&entry.path, self.config.scope.path_semantics)?;
            if !keys.insert(key.clone())
                || last.is_some_and(|old| old >= entry.path.as_str())
                || excluded(
                    &entry.path,
                    &self.config.policy,
                    self.config.scope.path_semantics,
                )
            {
                return Err(CaptureError::Corrupt);
            }
            present.insert(key, matches!(&entry.state, CaptureState::File { .. }));
            last = Some(&entry.path);
            if let CaptureState::File {
                content,
                basic_mode,
            } = &entry.state
            {
                if !digest_valid(&content.sha256)
                    || content.byte_count > self.config.policy.limits.file_bytes
                {
                    return Err(CaptureError::Corrupt);
                }
                match basic_mode {
                    BasicMode::Windows { .. } if cfg!(windows) => {}
                    BasicMode::Posix { mode } if cfg!(unix) && *mode <= 0o7777 => {}
                    _ => return Err(CaptureError::Unsupported),
                }
                total = total
                    .checked_add(content.byte_count)
                    .ok_or(CaptureError::Budget)?;
            }
        }
        if total != record.content_bytes || total > self.config.policy.limits.content_bytes {
            return Err(CaptureError::Corrupt);
        }
        // Prefix lookup is O(entries * bounded depth), never pairwise scanning.
        for (key, is_file) in &present {
            for (at, _) in key.match_indices('/') {
                if let Some(parent_is_file) = present.get(&key[..at]) {
                    if *parent_is_file || *is_file {
                        return Err(CaptureError::Corrupt);
                    }
                }
            }
        }
        match &record.selection {
            CaptureSelection::Paths { paths } => {
                if paths.len() != record.entries.len()
                    || !record.excluded.is_empty()
                    || paths
                        .iter()
                        .zip(&record.entries)
                        .any(|(path, entry)| path != &entry.path)
                {
                    return Err(CaptureError::Corrupt);
                }
            }
            CaptureSelection::Workspace {} => {
                if record
                    .entries
                    .iter()
                    .any(|entry| matches!(&entry.state, CaptureState::Absent {}))
                {
                    return Err(CaptureError::Corrupt);
                }
            }
        }
        let mut prior: Option<&str> = None;
        for path in &record.excluded {
            path_key(path, self.config.scope.path_semantics)?;
            if prior.is_some_and(|old| old >= path.as_str())
                || !excluded(path, &self.config.policy, self.config.scope.path_semantics)
            {
                return Err(CaptureError::Corrupt);
            }
            prior = Some(path);
        }
        Ok(())
    }
    pub(super) fn read_blob(
        &self,
        session: &Session,
        content: &BlobReference,
    ) -> Result<Vec<u8>, CaptureError> {
        if !digest_valid(&content.sha256) {
            return Err(CaptureError::Corrupt);
        }
        let objects = open_child(&session.partition, "objects")?;
        let bucket =
            open_child(&objects, &content.sha256[..2]).map_err(|_| CaptureError::Corrupt)?;
        let (bytes, _) = read_file(
            &bucket,
            &format!("{}.blob", content.sha256),
            self.config.policy.limits.file_bytes,
        )
        .map_err(|_| CaptureError::Corrupt)?;
        if bytes.len() as u64 != content.byte_count || checkpoint_digest(&bytes) != content.sha256 {
            return Err(CaptureError::Corrupt);
        }
        Ok(bytes)
    }
    pub(super) fn capture(
        &mut self,
        selection: CaptureSelection,
    ) -> Result<CaptureRecord, CaptureError> {
        let session = self.session()?;
        self.capture_in_session(&session, selection)
    }
    pub(super) fn capture_in_session(
        &mut self,
        session: &Session,
        selection: CaptureSelection,
    ) -> Result<CaptureRecord, CaptureError> {
        self.capture_named_in_session(
            session,
            selection,
            checkpoint_digest(uuid::Uuid::new_v4().as_bytes()),
        )
    }
    /// A native admission persists this opaque identity before capture begins,
    /// so an interrupted preimage cannot disappear into an empty history.
    pub(super) fn capture_named_in_session(
        &mut self,
        session: &Session,
        selection: CaptureSelection,
        checkpoint_id: String,
    ) -> Result<CaptureRecord, CaptureError> {
        let history = self.history(&session)?;
        if !digest_valid(&checkpoint_id)
            || history
                .iter()
                .any(|record| record.checkpoint_id == checkpoint_id)
        {
            return Err(CaptureError::Corrupt);
        }
        if history.len() >= self.config.policy.limits.history_records {
            return Err(CaptureError::Budget);
        }
        let (paths, excluded) = self.selections(&session.workspace.directory, &selection)?;
        let (entries, blobs, content_bytes) = self.observe(&session.workspace.directory, &paths)?;
        let record = CaptureRecord {
            version: 1,
            checkpoint_id,
            sequence: history.len() as u64 + 1,
            scope_digest: checkpoint_scope_digest(&self.config.scope),
            profile: self.config.policy.profile,
            selection,
            entries,
            excluded,
            content_bytes,
        };
        self.validate_record(&record)?;
        let manifest = encode(&record, self.config.policy.limits.manifest_bytes)?;
        let manifest_digest = checkpoint_digest(&manifest);
        let commit = encode(
            &Commit {
                version: 1,
                scope_digest: record.scope_digest.clone(),
                sequence: record.sequence,
                checkpoint_id: record.checkpoint_id.clone(),
                manifest_digest: manifest_digest.clone(),
            },
            4096,
        )?;
        let mut count = 200_000;
        let mut occupied = 0u64;
        inventory(
            &session.store.directory,
            0,
            &mut count,
            &mut occupied,
            self.config.policy.limits.store_bytes,
        )?;
        let reserved = blobs.values().try_fold(
            manifest.len() as u64 + commit.len() as u64 + 64,
            |sum, bytes| {
                sum.checked_add(bytes.len() as u64)
                    .ok_or(CaptureError::Budget)
            },
        )?;
        if occupied.checked_add(reserved).ok_or(CaptureError::Budget)?
            > self.config.policy.limits.store_bytes
            || blobs.len().checked_add(259).ok_or(CaptureError::Budget)? > count
        {
            return Err(CaptureError::Budget);
        }
        let objects = open_child(&session.partition, "objects")?;
        for (hash, bytes) in blobs {
            let bucket = child(&objects, &hash[..2])?;
            let name = format!("{hash}.blob");
            match bucket.symlink_metadata(&name) {
                Ok(_) => {
                    let existing = self.read_blob(
                        &session,
                        &BlobReference {
                            sha256: hash,
                            byte_count: bytes.len() as u64,
                        },
                    )?;
                    if existing != bytes {
                        return Err(CaptureError::Corrupt);
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    create(&bucket, &name, &bytes)?
                }
                Err(error) => return Err(io_error(error)),
            }
        }
        create(
            &open_child(&session.partition, "manifests")?,
            &format!("{manifest_digest}.json"),
            &manifest,
        )?;
        #[cfg(test)]
        let fault = self.fault.take();
        #[cfg(test)]
        if matches!(fault, Some(TestPublishFault::BeforeIndexCommit)) {
            return Err(CaptureError::Interrupted);
        }
        let name = format!("{:020}.json", record.sequence);
        create(&open_child(&session.partition, "commits")?, &name, &commit)
            .map_err(|_| CaptureError::RecoveryRequired)?;
        #[cfg(test)]
        if matches!(fault, Some(TestPublishFault::AfterIndexCommitBeforeAck)) {
            return Err(CaptureError::RecoveryRequired);
        }
        create(
            &open_child(&session.partition, "acks")?,
            &name,
            checkpoint_digest(&commit).as_bytes(),
        )
        .map_err(|_| CaptureError::RecoveryRequired)?;
        Ok(record)
    }
    pub(super) fn list(&self) -> Result<Vec<CaptureSummary>, CaptureError> {
        let session = self.session()?;
        Ok(self
            .history(&session)?
            .into_iter()
            .rev()
            .map(|record| CaptureSummary {
                checkpoint_id: record.checkpoint_id,
                sequence: record.sequence,
                entry_count: record.entries.len(),
                content_bytes: record.content_bytes,
                profile: record.profile,
            })
            .collect())
    }
    pub(super) fn get(&self, id: &str) -> Result<CaptureRecord, CaptureError> {
        if !digest_valid(id) {
            return Err(CaptureError::Unavailable);
        }
        let session = self.session()?;
        self.history(&session)?
            .into_iter()
            .find(|record| record.checkpoint_id == id)
            .ok_or(CaptureError::Unavailable)
    }
    pub(super) fn diff(&self, id: &str) -> Result<Vec<CaptureDiff>, CaptureError> {
        let session = self.session()?;
        let record = self
            .history(&session)?
            .into_iter()
            .find(|record| record.checkpoint_id == id)
            .ok_or(CaptureError::Unavailable)?;
        let (live_paths, _) = self.selections(&session.workspace.directory, &record.selection)?;
        let semantics = self.config.scope.path_semantics;
        let mut paths = BTreeMap::new();
        for path in live_paths {
            paths.insert(path_key(&path, semantics)?, path);
        }
        for entry in &record.entries {
            // Prefer the observed live display spelling for one canonical key.
            paths
                .entry(path_key(&entry.path, semantics)?)
                .or_insert_with(|| entry.path.clone());
        }
        if paths.len() > self.config.policy.limits.entries {
            return Err(CaptureError::Budget);
        }
        let paths = paths.into_values().collect::<Vec<_>>();
        let (current, _, _) = self.observe(&session.workspace.directory, &paths)?;
        let mut before = BTreeMap::new();
        for entry in &record.entries {
            before.insert(path_key(&entry.path, semantics)?, &entry.state);
        }
        let absent = CaptureState::Absent {};
        let mut diff = Vec::new();
        for entry in current {
            let old = before
                .get(&path_key(&entry.path, semantics)?)
                .copied()
                .unwrap_or(&absent);
            let kind = match (old, &entry.state) {
                (CaptureState::Absent {}, CaptureState::File { .. }) => Some(DiffKind::Added),
                (CaptureState::File { .. }, CaptureState::Absent {}) => Some(DiffKind::Deleted),
                (
                    CaptureState::File {
                        content: a,
                        basic_mode: am,
                    },
                    CaptureState::File {
                        content: b,
                        basic_mode: bm,
                    },
                ) => {
                    if a != b {
                        Some(DiffKind::Modified)
                    } else if am != bm {
                        Some(DiffKind::ModeChanged)
                    } else {
                        None
                    }
                }
                _ => None,
            };
            if let Some(kind) = kind {
                diff.push(CaptureDiff {
                    path: entry.path,
                    kind,
                });
            }
        }
        diff.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(diff)
    }
    pub(super) fn read_content(&self, id: &str, path: &str) -> Result<Vec<u8>, CaptureError> {
        let session = self.session()?;
        let record = self
            .history(&session)?
            .into_iter()
            .find(|record| record.checkpoint_id == id)
            .ok_or(CaptureError::Unavailable)?;
        let state = record
            .entries
            .iter()
            .find(|entry| entry.path == path)
            .map(|entry| &entry.state)
            .ok_or(CaptureError::Unavailable)?;
        match state {
            CaptureState::File { content, .. } => self.read_blob(&session, content),
            _ => Err(CaptureError::Unavailable),
        }
    }
    #[cfg(test)]
    pub(super) fn set_publish_fault(&mut self, fault: TestPublishFault) {
        self.fault = Some(fault);
    }
}
