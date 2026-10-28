//! Private directory custody for the derived SQLite index and its sidecars.

use std::path::PathBuf;
use std::sync::Arc;

use anima_document_store_sqlite::{DocumentStore, StoreError};
use cap_fs_ext::MetadataExt;

use crate::document_edits::store::DocumentEditStore;
use crate::profile_owner::ProfileOwner;
use crate::tools::workspace_bytes::{linked, WorkspaceBytes};

const DATABASE: &str = "knowledge.sqlite3";
const UNAVAILABLE: &str = "Project knowledge cache custody is unavailable";

pub(super) struct Cache {
    custody: DocumentEditStore,
    root: WorkspaceBytes,
    identity: (u64, u64),
    store: DocumentStore,
}

impl Cache {
    pub(super) fn root_path(&self) -> Result<PathBuf, String> {
        self.check()?;
        Ok(self.root.canonical_root.clone())
    }

    pub(super) fn open(root: PathBuf, owner: Option<ProfileOwner>) -> Result<Arc<Self>, String> {
        let mut custody = DocumentEditStore::open_existing(&root)?;
        if let Some(owner) = owner {
            custody = custody.with_owner(owner)?;
        }
        let root = WorkspaceBytes::open(custody.canonical_root()?).map_err(|_| UNAVAILABLE)?;
        let directory = root.held_directory().map_err(|_| UNAVAILABLE)?;
        match directory.symlink_metadata(DATABASE) {
            Ok(metadata) if metadata.is_file() && !linked(&metadata) && metadata.nlink() == 1 => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => root
                .create_new_bytes(DATABASE, &[])
                .map_err(|_| UNAVAILABLE)?,
            _ => return Err(UNAVAILABLE.into()),
        }
        for name in [
            DATABASE,
            "knowledge.sqlite3-wal",
            "knowledge.sqlite3-shm",
            "knowledge.sqlite3-journal",
        ] {
            validate_entry(&root, name)?;
        }
        let metadata = directory
            .symlink_metadata(DATABASE)
            .map_err(|_| UNAVAILABLE)?;
        let identity = (metadata.dev(), metadata.ino());
        let store =
            DocumentStore::open(root.canonical_root.join(DATABASE)).map_err(|_| UNAVAILABLE)?;
        let cache = Arc::new(Self {
            custody,
            root,
            identity,
            store,
        });
        cache.check()?;
        Ok(cache)
    }

    pub(super) fn check(&self) -> Result<(), String> {
        self.custody.canonical_root()?;
        for name in [
            DATABASE,
            "knowledge.sqlite3-wal",
            "knowledge.sqlite3-shm",
            "knowledge.sqlite3-journal",
        ] {
            validate_entry(&self.root, name)?;
        }
        let metadata = self
            .root
            .held_directory()
            .and_then(|dir| dir.symlink_metadata(DATABASE))
            .map_err(|_| UNAVAILABLE)?;
        if (metadata.dev(), metadata.ino()) != self.identity {
            return Err(UNAVAILABLE.into());
        }
        Ok(())
    }

    pub(super) fn run<T>(
        &self,
        work: impl FnOnce(&DocumentStore) -> Result<T, StoreError>,
    ) -> Result<T, String> {
        self.check()?;
        let result = work(&self.store).map_err(|_| UNAVAILABLE.to_owned());
        self.check()?;
        result
    }
}

fn validate_entry(root: &WorkspaceBytes, name: &str) -> Result<(), String> {
    match root
        .held_directory()
        .and_then(|dir| dir.symlink_metadata(name))
    {
        Ok(metadata) if metadata.is_file() && !linked(&metadata) && metadata.nlink() == 1 => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && name != DATABASE => Ok(()),
        _ => Err(UNAVAILABLE.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_hardlinked_cache_files_before_sqlite_open() {
        let private = crate::document_edits::store::test_support::private_directory();
        std::fs::write(private.path().join(DATABASE), b"").unwrap();
        std::fs::hard_link(private.path().join(DATABASE), private.path().join("alias")).unwrap();
        assert!(Cache::open(private.path().to_owned(), None).is_err());
    }

    #[test]
    fn cache_replacement_invalidates_retained_custody() {
        let private = crate::document_edits::store::test_support::private_directory();
        let cache = Cache::open(private.path().to_owned(), None).unwrap();
        let name = private.path().join(DATABASE);
        // Windows SQLite retains a file handle that can deny replacement itself.
        if std::fs::rename(&name, private.path().join("old.sqlite3")).is_ok() {
            std::fs::write(name, b"").unwrap();
            assert!(cache.check().is_err());
        }
    }
}
