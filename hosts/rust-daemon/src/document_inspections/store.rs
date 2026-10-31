//! Bounded immutable report directories with recoverable publication and retirement.
use super::service::{record_key, InspectionEnvelope, InspectionPageRef, InspectionRequest};
use crate::{
    document_edits::{service::digest, DocumentEditStore, OwnerInspectionScope},
    tools::workspace_bytes::WorkspaceBytes,
};
use anima_document_visual::{
    inspection::{MAX_INSPECTION_PAGES, MAX_INSPECTION_RASTER_BYTES, MAX_INSPECTION_REPORT_BYTES},
    PdfPageEvidence, MAX_PAGE_PNG_BYTES,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock, Weak},
};

const MAX_RECORDS: usize = 8;
const MAX_BYTES: u64 = 64 * 1024 * 1024;
const MAX_RECORD_BYTES: usize = MAX_INSPECTION_REPORT_BYTES + 64 * 1024;
const MAX_ENTRIES: usize = MAX_INSPECTION_PAGES + 1;
const LIFETIME_MS: u64 = 30 * 60 * 1000;

#[derive(Clone)]
pub(super) struct DocumentInspectionStore {
    parent: DocumentEditStore,
    root: Arc<WorkspaceBytes>,
    lock: Arc<Mutex<()>>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    version: u32,
    publication_id: String,
    scope_key: String,
    scope: OwnerInspectionScope,
    request: InspectionRequest,
    envelope: InspectionEnvelope,
    created_at_ms: u64,
    expires_at_ms: u64,
}
impl Record {
    fn base(&self) -> String {
        format!("inspection_{}_{}", self.publication_id, self.scope_key)
    }
    fn name(&self) -> String {
        format!("{}.json", self.base())
    }
    fn page_name(&self, page: u32) -> String {
        format!("{}_page_{page:010}.png", self.base())
    }
    fn transaction_name(&self, phase: PublicationPhase) -> String {
        format!(
            "{}_{}_{}",
            phase.prefix(),
            self.publication_id,
            self.scope_key
        )
    }
    fn validate(&self, named: &str) -> Result<(), String> {
        self.envelope.validate()?;
        if self.version != 1
            || !hex_id(&self.publication_id, 32)
            || self.scope_key != record_key(&self.scope, &self.envelope.report_sha256)?
            || named != self.name()
            || self.envelope.report.source().sha256 != self.request.subject.expected_sha256()
            || self.created_at_ms >= self.expires_at_ms
            || self.expires_at_ms.saturating_sub(self.created_at_ms) > LIFETIME_MS
        {
            return Err("Document inspection record is invalid".into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PublicationPhase {
    Publishing,
    Complete,
    Retiring,
}
impl PublicationPhase {
    fn prefix(self) -> &'static str {
        match self {
            Self::Publishing => "publishing",
            Self::Complete => "inspection",
            Self::Retiring => "retiring",
        }
    }
}
struct OwnedDirectory {
    name: String,
    base: String,
    phase: PublicationPhase,
    root: WorkspaceBytes,
    names: BTreeSet<String>,
    bytes: u64,
}

impl DocumentInspectionStore {
    pub(super) fn open(parent: DocumentEditStore) -> Result<Self, String> {
        let path = parent.inspection_directory()?;
        let root = WorkspaceBytes::open(&path)
            .map_err(|_| "Document inspection storage is unavailable")?;
        Ok(Self {
            parent,
            lock: shared_lock(&root.canonical_root),
            root: Arc::new(root),
        })
    }
    fn ensure_custody(&self) -> Result<(), String> {
        use cap_fs_ext::MetadataExt;
        let path = self.parent.inspection_directory()?;
        let named = WorkspaceBytes::open(&path)
            .map_err(|_| "Document inspection storage is unavailable")?;
        let named = named
            .held_directory()
            .and_then(|dir| dir.dir_metadata())
            .map_err(|_| "Document inspection storage is unavailable")?;
        let held = self
            .root
            .held_directory()
            .and_then(|dir| dir.dir_metadata())
            .map_err(|_| "Document inspection storage is unavailable")?;
        if (named.dev(), named.ino()) != (held.dev(), held.ino()) {
            return Err("Document inspection storage custody changed".into());
        }
        Ok(())
    }
    fn ensure_child(&self, name: &str, held: &WorkspaceBytes) -> Result<(), String> {
        use cap_fs_ext::MetadataExt;
        self.ensure_custody()?;
        let named = self
            .root
            .held_directory()
            .and_then(|dir| dir.symlink_metadata(name))
            .map_err(|_| "Document inspection record directory is unavailable")?;
        let captured = held
            .held_directory()
            .and_then(|dir| dir.dir_metadata())
            .map_err(|_| "Document inspection record directory is unavailable")?;
        if !named.is_dir()
            || crate::tools::workspace_bytes::linked(&named)
            || (named.dev(), named.ino()) != (captured.dev(), captured.ino())
        {
            return Err("Document inspection record directory custody changed".into());
        }
        Ok(())
    }
    fn open_owned(&self, name: &str) -> Result<OwnedDirectory, String> {
        let (phase, base) = directory_identity(name)
            .ok_or("Document inspection storage contains an unsupported entry")?;
        let path = self.root.canonical_root.join(name);
        let root = WorkspaceBytes::open(&path)
            .map_err(|_| "Document inspection record directory is unavailable")?;
        self.ensure_child(name, &root)?;
        crate::document_edits::store::initialize_owned_directory(&path)?;
        self.ensure_child(name, &root)?;
        let directory = root
            .held_directory()
            .map_err(|_| "Document inspection storage is unavailable")?;
        let mut names = BTreeSet::new();
        let mut bytes = 0_u64;
        let mut raster_bytes = 0_u64;
        for entry in directory
            .entries()
            .map_err(|_| "Document inspection storage is unavailable")?
        {
            let entry = entry.map_err(|_| "Document inspection storage is unavailable")?;
            let file = entry
                .file_name()
                .to_str()
                .ok_or("Document inspection storage entry is invalid")?
                .to_owned();
            if names.len() >= MAX_ENTRIES || !owned_file(&file, &base) {
                return Err("Document inspection storage contains an unsupported entry".into());
            }
            let metadata = directory
                .symlink_metadata(&file)
                .map_err(|_| "Document inspection storage is unavailable")?;
            let limit = if file.ends_with(".json") {
                MAX_RECORD_BYTES
            } else {
                MAX_PAGE_PNG_BYTES
            };
            if !metadata.is_file()
                || crate::tools::workspace_bytes::linked(&metadata)
                || root.instruction_link_count(&file).ok() != Some(Some(1))
                || metadata.len() > limit as u64
            {
                return Err("Document inspection storage contains an unsupported entry".into());
            }
            bytes = bytes
                .checked_add(metadata.len())
                .ok_or("Document inspection storage is full")?;
            if file.ends_with(".png") {
                raster_bytes = raster_bytes
                    .checked_add(metadata.len())
                    .ok_or("Document inspection storage is full")?;
            }
            if raster_bytes > MAX_INSPECTION_RASTER_BYTES as u64 {
                return Err("Document inspection storage is full".into());
            }
            names.insert(file);
        }
        self.ensure_child(name, &root)?;
        Ok(OwnedDirectory {
            name: name.to_owned(),
            base,
            phase,
            root,
            names,
            bytes,
        })
    }
    fn directories(&self) -> Result<Vec<OwnedDirectory>, String> {
        self.ensure_custody()?;
        let directory = self
            .root
            .held_directory()
            .map_err(|_| "Document inspection storage is unavailable")?;
        let mut directories = Vec::new();
        let mut total = 0_u64;
        for entry in directory
            .entries()
            .map_err(|_| "Document inspection storage is unavailable")?
        {
            let entry = entry.map_err(|_| "Document inspection storage is unavailable")?;
            let name = entry
                .file_name()
                .to_str()
                .ok_or("Document inspection storage entry is invalid")?
                .to_owned();
            if directories.len() >= MAX_RECORDS {
                return Err("Document inspection storage is full".into());
            }
            let owned = self.open_owned(&name)?;
            total = total
                .checked_add(owned.bytes)
                .ok_or("Document inspection storage is full")?;
            if total > MAX_BYTES {
                return Err("Document inspection storage is full".into());
            }
            directories.push(owned);
        }
        self.ensure_custody()?;
        Ok(directories)
    }
    fn read_record(&self, owned: &OwnedDirectory) -> Result<Record, String> {
        if owned.phase != PublicationPhase::Complete {
            return Err("Document inspection publication is incomplete".into());
        }
        let name = format!("{}.json", owned.base);
        let bytes = owned
            .root
            .read_file_snapshot(&name, MAX_RECORD_BYTES)
            .map_err(|_| "Document inspection record is unavailable")?;
        let record: Record =
            serde_json::from_slice(&bytes).map_err(|_| "Document inspection record is invalid")?;
        record.validate(&name)?;
        let referenced: BTreeSet<_> = record
            .envelope
            .pages
            .iter()
            .map(|page| record.page_name(page.page))
            .chain(std::iter::once(record.name()))
            .collect();
        if record.base() != owned.name || referenced != owned.names {
            return Err("Document inspection publication is incomplete".into());
        }
        self.ensure_child(&owned.name, &owned.root)?;
        Ok(record)
    }
    fn inventory(&self) -> Result<(Vec<Record>, u64), String> {
        let mut records = Vec::new();
        let mut total = 0_u64;
        for owned in self.directories()? {
            total = total
                .checked_add(owned.bytes)
                .ok_or("Document inspection storage is full")?;
            records.push(self.read_record(&owned)?);
        }
        Ok((records, total))
    }
    fn reopen_record(
        &self,
        name: &str,
        record: &Record,
        file_id: (u64, u64),
    ) -> Result<OwnedDirectory, String> {
        let owned = self.open_owned(name)?;
        if directory_file_id(&owned.root)? != file_id || owned.base != record.base() {
            return Err("Document inspection record directory custody changed".into());
        }
        let expected =
            serde_json::to_vec(record).map_err(|_| "Document inspection record is invalid")?;
        let bytes = owned
            .root
            .read_file_snapshot(&record.name(), MAX_RECORD_BYTES)
            .map_err(|_| "Document inspection record is unavailable")?;
        let names: BTreeSet<_> = record
            .envelope
            .pages
            .iter()
            .map(|page| record.page_name(page.page))
            .chain(std::iter::once(record.name()))
            .collect();
        if bytes != expected || names != owned.names {
            return Err("Document inspection publication changed".into());
        }
        for page in &record.envelope.pages {
            let bytes = owned
                .root
                .read_file_snapshot(&record.page_name(page.page), MAX_PAGE_PNG_BYTES)
                .map_err(|_| "Document inspection page is unavailable")?;
            if digest(&bytes) != page.png_sha256 {
                return Err("Document inspection page changed".into());
            }
        }
        self.ensure_child(name, &owned.root)?;
        Ok(owned)
    }
    fn remove_transaction(&self, owned: OwnedDirectory) -> Result<(), String> {
        if owned.phase == PublicationPhase::Complete {
            return Err("Document inspection record must be retired before removal".into());
        }
        self.ensure_child(&owned.name, &owned.root)?;
        let directory = owned
            .root
            .held_directory()
            .map_err(|_| "Document inspection storage is unavailable")?;
        for name in &owned.names {
            self.ensure_child(&owned.name, &owned.root)?;
            let metadata = directory
                .symlink_metadata(name)
                .map_err(|_| "Document inspection transaction changed")?;
            if !metadata.is_file()
                || crate::tools::workspace_bytes::linked(&metadata)
                || owned.root.instruction_link_count(name).ok() != Some(Some(1))
            {
                return Err("Document inspection transaction changed".into());
            }
            directory
                .remove_file(name)
                .map_err(|_| "Document inspection transaction could not be retired")?;
        }
        self.ensure_child(&owned.name, &owned.root)?;
        let name = owned.name.clone();
        drop(directory);
        drop(owned);
        self.root
            .held_directory()
            .and_then(|dir| dir.remove_dir(&name))
            .map_err(|_| "Document inspection transaction could not be retired")?;
        self.sync_root()
    }
    fn recover_locked(&self) -> Result<(), String> {
        // Inspect all contents before deleting anything; unknown files and links
        // refuse recovery and are preserved. Partial known staging files are inert.
        let directories = self.directories()?;
        for owned in directories {
            if owned.phase != PublicationPhase::Complete {
                self.remove_transaction(owned)?;
            }
        }
        Ok(())
    }
    fn rename_owned(&self, from: &str, to: &str) -> Result<(), String> {
        self.ensure_custody()?;
        let directory = self
            .root
            .held_directory()
            .map_err(|_| "Document inspection storage is unavailable")?;
        #[cfg(windows)]
        directory
            .rename(from, &directory, to)
            .map_err(|_| "Document inspection transaction could not be published")?;
        #[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
        rustix::fs::renameat_with(
            &directory,
            from,
            &directory,
            to,
            rustix::fs::RenameFlags::NOREPLACE,
        )
        .map_err(|_| "Document inspection transaction could not be published")?;
        #[cfg(not(any(
            windows,
            target_os = "linux",
            target_os = "android",
            target_os = "macos"
        )))]
        {
            let _ = (directory, from, to);
            return Err("Document inspection atomic directory publication is unavailable".into());
        }
        self.sync_root()
    }
    fn sync_root(&self) -> Result<(), String> {
        #[cfg(unix)]
        self.root
            .held_directory()
            .and_then(|dir| dir.into_std_file().sync_all())
            .map_err(|_| "Document inspection directory could not be synchronized")?;
        self.ensure_custody()
    }
    fn prune_locked(&self, now: u64) -> Result<(), String> {
        self.recover_locked()?;
        let (records, _) = self.inventory()?;
        for record in records
            .into_iter()
            .filter(|record| record.expires_at_ms <= now)
        {
            let complete = self.open_owned(&record.base())?;
            self.read_record(&complete)?;
            self.ensure_child(&complete.name, &complete.root)?;
            let retiring = record.transaction_name(PublicationPhase::Retiring);
            let file_id = directory_file_id(&complete.root)?;
            let name = complete.name.clone();
            // Windows capability handles deliberately deny directory renames.
            // Retain the parent and identity witness, then close every child handle.
            drop(complete);
            self.rename_owned(&name, &retiring)?;
            self.remove_transaction(self.reopen_record(&retiring, &record, file_id)?)?;
        }
        self.ensure_custody()
    }
    #[cfg(test)]
    pub(super) fn prune(&self, now: u64) -> Result<(), String> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| "Document inspection storage is unavailable")?;
        self.prune_locked(now)
    }
    #[cfg(test)]
    pub(super) fn publish(
        &self,
        scope: &OwnerInspectionScope,
        request: &InspectionRequest,
        envelope: &InspectionEnvelope,
        pages: &[PdfPageEvidence],
        expires_at_ms: u64,
    ) -> Result<(), String> {
        self.publish_authorized(scope, request, envelope, pages, expires_at_ms, || Ok(()))
    }
    pub(super) fn publish_authorized<G>(
        &self,
        scope: &OwnerInspectionScope,
        request: &InspectionRequest,
        envelope: &InspectionEnvelope,
        pages: &[PdfPageEvidence],
        expires_at_ms: u64,
        authorize: impl Fn() -> Result<G, String>,
    ) -> Result<(), String> {
        envelope.validate()?;
        if pages.len() > MAX_INSPECTION_PAGES || pages.len() != envelope.pages.len() {
            return Err("Document inspection pages exceed their limit".into());
        }
        let mut page_bytes = 0_usize;
        for (page, expected) in pages.iter().zip(&envelope.pages) {
            page_bytes = page_bytes
                .checked_add(page.png.len())
                .ok_or("Document inspection pages exceed their limit")?;
            if page_bytes > MAX_INSPECTION_RASTER_BYTES
                || page.png.is_empty()
                || page.png.len() > MAX_PAGE_PNG_BYTES
                || page.page != expected.page
                || page.width != expected.width
                || page.height != expected.height
                || digest(&page.png) != expected.png_sha256
            {
                return Err("Document inspection page identity changed".into());
            }
        }
        let now = anima_core::primitives::now_millis();
        let record = Record {
            version: 1,
            publication_id: uuid::Uuid::new_v4().simple().to_string(),
            scope_key: record_key(scope, &envelope.report_sha256)?,
            scope: scope.clone(),
            request: request.clone(),
            envelope: envelope.clone(),
            created_at_ms: now,
            expires_at_ms,
        };
        record.validate(&record.name())?;
        let bytes =
            serde_json::to_vec(&record).map_err(|_| "Document inspection record is invalid")?;
        if bytes.len() > MAX_RECORD_BYTES {
            return Err("Document inspection report exceeds its limit".into());
        }
        let _guard = self
            .lock
            .lock()
            .map_err(|_| "Document inspection storage is unavailable")?;
        self.prune_locked(now)?;
        let (records, total) = self.inventory()?;
        if records.len() >= MAX_RECORDS
            || total
                .saturating_add(bytes.len() as u64)
                .saturating_add(page_bytes as u64)
                > MAX_BYTES
        {
            return Err("Document inspection capacity is full".into());
        }
        let publishing = record.transaction_name(PublicationPhase::Publishing);
        let result = (|| {
            drop(authorize()?);
            self.ensure_custody()?;
            #[cfg(windows)]
            {
                use win_custody::{current_user_sid_string, SecurityDescriptor};
                let user = current_user_sid_string()
                    .map_err(|_| "Document private directory owner is unavailable")?;
                // An inherited DACL is not protected. Supply the same private
                // descriptor as the profile store at exclusive directory creation.
                SecurityDescriptor::from_sddl(&format!(
                    "O:{user}D:P(A;OICI;FA;;;{user})(A;OICI;FA;;;SY)"
                ))
                .map_err(|_| "Document private permissions are invalid")?
                .create_directory(&self.root.canonical_root.join(&publishing))
                .map_err(|_| "Document inspection transaction could not be created")?;
            }
            #[cfg(not(windows))]
            {
                let directory = self
                    .root
                    .held_directory()
                    .map_err(|_| "Document inspection storage is unavailable")?;
                #[cfg(unix)]
                let mut builder = cap_std::fs::DirBuilder::new();
                #[cfg(not(unix))]
                let builder = cap_std::fs::DirBuilder::new();
                #[cfg(unix)]
                {
                    use cap_std::fs::DirBuilderExt;
                    builder.mode(0o700);
                }
                directory
                    .create_dir_with(&publishing, &builder)
                    .map_err(|_| "Document inspection transaction could not be created")?;
            }
            let owned = self.open_owned(&publishing)?;
            // Exact private staging names never reach readers. A partial write
            // is recoverable without trusting its incomplete JSON or PNG content.
            for page in pages {
                drop(authorize()?);
                owned
                    .root
                    .create_new_bytes(&record.page_name(page.page), &page.png)
                    .map_err(|_| "Document inspection page could not be saved")?;
            }
            drop(authorize()?);
            owned
                .root
                .create_new_bytes(&record.name(), &bytes)
                .map_err(|_| "Document inspection report could not be saved")?;
            #[cfg(unix)]
            owned
                .root
                .held_directory()
                .and_then(|dir| dir.into_std_file().sync_all())
                .map_err(|_| "Document inspection transaction could not be synchronized")?;
            self.ensure_child(&publishing, &owned.root)?;
            let file_id = directory_file_id(&owned.root)?;
            // The retained private parent and captured identity bridge this
            // handle-free rename; destination content and privacy are rechecked.
            drop(owned);
            let authority = authorize()?;
            self.rename_owned(&publishing, &record.base())?;
            self.reopen_record(&record.base(), &record, file_id)?;
            drop(authority);
            Ok(())
        })();
        if result.is_err() {
            // A rename may precede a later synchronization error. Recover only
            // pending directories; completed records stay immutable and bounded.
            let _ = self.recover_locked();
        }
        result
    }
    fn find(
        &self,
        scope: &OwnerInspectionScope,
        request: &InspectionRequest,
        hash: &str,
    ) -> Result<Record, String> {
        self.recover_locked()?;
        let key = record_key(scope, hash)?;
        let (records, _) = self.inventory()?;
        records
            .into_iter()
            .find(|record| {
                record.scope_key == key
                    && record.scope == *scope
                    && record.request == *request
                    && record.envelope.report_sha256 == hash
                    && record.expires_at_ms > anima_core::primitives::now_millis()
            })
            .ok_or_else(|| "Document inspection report is unavailable or expired".into())
    }
    pub(super) fn report(
        &self,
        scope: &OwnerInspectionScope,
        request: &InspectionRequest,
        hash: &str,
    ) -> Result<InspectionEnvelope, String> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| "Document inspection storage is unavailable")?;
        let record = self.find(scope, request, hash)?;
        for page in &record.envelope.pages {
            self.read_page(&record, page)?;
        }
        self.ensure_custody()?;
        Ok(record.envelope)
    }
    fn read_page(&self, record: &Record, page: &InspectionPageRef) -> Result<Vec<u8>, String> {
        let owned = self.open_owned(&record.base())?;
        self.read_record(&owned)?;
        let name = record.page_name(page.page);
        let bytes = owned
            .root
            .read_file_snapshot(&name, MAX_PAGE_PNG_BYTES)
            .map_err(|_| "Document inspection page is unavailable")?;
        if owned.root.instruction_link_count(&name).ok() != Some(Some(1))
            || digest(&bytes) != page.png_sha256
        {
            return Err("Document inspection page changed".into());
        }
        self.ensure_child(&owned.name, &owned.root)?;
        Ok(bytes)
    }
    pub(super) fn page(
        &self,
        scope: &OwnerInspectionScope,
        request: &InspectionRequest,
        hash: &str,
        page: &InspectionPageRef,
    ) -> Result<Vec<u8>, String> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| "Document inspection storage is unavailable")?;
        let record = self.find(scope, request, hash)?;
        if !record.envelope.pages.contains(page) {
            return Err("Document inspection page identity changed".into());
        }
        let bytes = self.read_page(&record, page)?;
        self.ensure_custody()?;
        Ok(bytes)
    }
}

fn directory_file_id(root: &WorkspaceBytes) -> Result<(u64, u64), String> {
    use cap_fs_ext::MetadataExt;
    let metadata = root
        .held_directory()
        .and_then(|directory| directory.dir_metadata())
        .map_err(|_| "Document inspection record directory is unavailable")?;
    Ok((metadata.dev(), metadata.ino()))
}
fn hex_id(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn directory_identity(name: &str) -> Option<(PublicationPhase, String)> {
    for phase in [
        PublicationPhase::Publishing,
        PublicationPhase::Complete,
        PublicationPhase::Retiring,
    ] {
        let Some(suffix) = name
            .strip_prefix(phase.prefix())
            .and_then(|value| value.strip_prefix('_'))
        else {
            continue;
        };
        let (id, key) = suffix.split_once('_')?;
        if hex_id(id, 32) && hex_id(key, 64) {
            return Some((phase, format!("inspection_{suffix}")));
        }
    }
    None
}
fn owned_file(name: &str, base: &str) -> bool {
    if name == format!("{base}.json") {
        return true;
    }
    let Some(page) = name
        .strip_prefix(base)
        .and_then(|suffix| suffix.strip_prefix("_page_"))
        .and_then(|suffix| suffix.strip_suffix(".png"))
    else {
        return false;
    };
    page.len() == 10
        && page.bytes().all(|b| b.is_ascii_digit())
        && page.parse::<u32>().is_ok_and(|page| page != 0)
}
fn shared_lock(root: &Path) -> Arc<Mutex<()>> {
    static LOCKS: OnceLock<Mutex<BTreeMap<PathBuf, Weak<Mutex<()>>>>> = OnceLock::new();
    let mut locks = LOCKS
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    locks.retain(|_, lock| lock.strong_count() > 0);
    if let Some(lock) = locks.get(root).and_then(Weak::upgrade) {
        return lock;
    }
    let lock = Arc::new(Mutex::new(()));
    locks.insert(root.to_path_buf(), Arc::downgrade(&lock));
    lock
}
