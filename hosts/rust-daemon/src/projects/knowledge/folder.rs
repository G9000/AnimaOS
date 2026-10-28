//! No-follow folder custody and bounded snapshots for knowledge indexing.

use std::path::{Path, PathBuf};

use anima_core::CancelSignal;
use cap_fs_ext::{DirExt, MetadataExt};
use cap_std::fs::{Dir, Metadata};
use sha2::{Digest, Sha256};

use crate::tools::workspace_bytes::{linked, WorkspaceBytes};

pub(super) const MAX_FILE_BYTES: usize = 2 * 1024 * 1024;
pub(super) const MAX_PASS_BYTES: usize = 32 * 1024 * 1024;
const MAX_DOCUMENTS: usize = 200;
const MAX_DEPTH: usize = 8;
const MAX_ENTRIES: usize = 4096;
const UNAVAILABLE: &str = "Knowledge folder is unavailable or changed";

#[derive(Debug)]
pub(super) struct FolderDocument {
    pub(super) relative_path: String,
    pub(super) bytes: Vec<u8>,
    pub(super) sha256: String,
}

#[derive(Debug)]
pub(super) struct FolderSnapshot {
    pub(super) documents: Vec<FolderDocument>,
    pub(super) omitted_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct KnowledgeFolderGrant {
    pub(crate) root_path: String,
    pub(crate) root_identity: String,
}

pub(super) fn scan_folder(
    grant: &KnowledgeFolderGrant,
    cancel: &CancelSignal,
    check: impl Fn() -> Result<(), String>,
) -> Result<FolderSnapshot, String> {
    scan_folder_with_budget(grant, cancel, MAX_PASS_BYTES, check)
}

pub(super) fn scan_folder_with_budget(
    grant: &KnowledgeFolderGrant,
    cancel: &CancelSignal,
    byte_budget: usize,
    check: impl Fn() -> Result<(), String>,
) -> Result<FolderSnapshot, String> {
    check()?;
    let root = open_grant(grant)?;
    let directory = root.held_directory().map_err(|_| UNAVAILABLE)?;
    let mut walker = Walker {
        root: &root,
        cancel,
        check: &check,
        remaining_bytes: byte_budget.min(MAX_PASS_BYTES),
        visited: 0,
        snapshot: FolderSnapshot {
            documents: Vec::new(),
            omitted_count: 0,
        },
    };
    walker.walk(&directory, "", 0)?;
    check()?;
    open_grant(grant)?;
    walker
        .snapshot
        .documents
        .sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    Ok(walker.snapshot)
}

pub(crate) fn capture_folder(
    root: &Path,
    protected_roots: &[PathBuf],
) -> Result<KnowledgeFolderGrant, String> {
    let held = open_root(root)?;
    ensure_root_allowed(&held.canonical_root, protected_roots)?;
    let metadata = held
        .held_directory()
        .and_then(|dir| dir.dir_metadata())
        .map_err(|_| UNAVAILABLE)?;
    Ok(KnowledgeFolderGrant {
        root_path: held.canonical_root.to_str().ok_or(UNAVAILABLE)?.to_owned(),
        root_identity: identity(&metadata),
    })
}

pub(super) fn ensure_root_allowed(root: &Path, protected: &[PathBuf]) -> Result<(), String> {
    if !root.is_absolute()
        || root.parent().is_none()
        || root.components().any(|part| matches!(part, std::path::Component::Normal(name) if name.to_str().is_none_or(|name| !allowed_component(name))))
    {
        return Err("Choose a regular document folder outside sensitive storage".into());
    }
    for key in ["USERPROFILE", "HOME", "APPDATA", "LOCALAPPDATA"] {
        if std::env::var_os(key)
            .and_then(|path| PathBuf::from(path).canonicalize().ok())
            .as_deref()
            == Some(root)
        {
            return Err("Choose a document folder instead of the home folder".into());
        }
    }
    for key in [
        "SystemRoot",
        "WINDIR",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramData",
    ] {
        if std::env::var_os(key)
            .and_then(|path| PathBuf::from(path).canonicalize().ok())
            .is_some_and(|path| root.starts_with(path))
        {
            return Err("System storage cannot be a project knowledge folder".into());
        }
    }
    for path in protected {
        let path =
            crate::profile_owner::ProfileOwner::resolve_path(path).map_err(|_| UNAVAILABLE)?;
        if root.starts_with(&path) || path.starts_with(root) {
            return Err(
                "Project knowledge folders must be separate from private profile storage".into(),
            );
        }
    }
    Ok(())
}

pub(super) fn open_grant(grant: &KnowledgeFolderGrant) -> Result<WorkspaceBytes, String> {
    let root = open_root(Path::new(&grant.root_path))?;
    let metadata = root
        .held_directory()
        .and_then(|dir| dir.dir_metadata())
        .map_err(|_| UNAVAILABLE)?;
    if identity(&metadata) != grant.root_identity {
        return Err(UNAVAILABLE.into());
    }
    Ok(root)
}

fn open_root(root: &Path) -> Result<WorkspaceBytes, String> {
    if !root.is_absolute() || root.as_os_str().len() > 4096 {
        return Err("Choose an absolute document folder".into());
    }
    let mut anchor = PathBuf::new();
    let mut names = Vec::new();
    for part in root.components() {
        match part {
            std::path::Component::Prefix(_) | std::path::Component::RootDir => {
                anchor.push(part.as_os_str())
            }
            std::path::Component::Normal(name) => names.push(name),
            _ => return Err(UNAVAILABLE.into()),
        }
    }
    let mut directory =
        Dir::open_ambient_dir(&anchor, cap_std::ambient_authority()).map_err(|_| UNAVAILABLE)?;
    for name in names {
        let before = directory.symlink_metadata(name).map_err(|_| UNAVAILABLE)?;
        if !before.is_dir() || linked(&before) {
            return Err(UNAVAILABLE.into());
        }
        directory = directory.open_dir_nofollow(name).map_err(|_| UNAVAILABLE)?;
        let after = directory.dir_metadata().map_err(|_| UNAVAILABLE)?;
        if !after.is_dir() || linked(&after) || identity(&before) != identity(&after) {
            return Err(UNAVAILABLE.into());
        }
    }
    let retained = directory.dir_metadata().map_err(|_| UNAVAILABLE)?;
    let root = WorkspaceBytes::open(root).map_err(|_| UNAVAILABLE)?;
    let captured = root
        .held_directory()
        .and_then(|dir| dir.dir_metadata())
        .map_err(|_| UNAVAILABLE)?;
    if identity(&retained) != identity(&captured) {
        return Err(UNAVAILABLE.into());
    }
    Ok(root)
}

fn identity(metadata: &Metadata) -> String {
    hex::encode(Sha256::digest(
        format!("{}:{}", metadata.dev(), metadata.ino()).as_bytes(),
    ))
}

pub(super) fn allowed_component(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 1024
        && !name.chars().any(char::is_control)
        && !name.contains(['/', '\\', ':'])
        && !crate::tools::generated_documents::excluded_component(name)
        && !matches!(
            name.to_lowercase().as_str(),
            "cookies" | "login data" | "web data" | "system32" | "keychain"
        )
}

fn allowed_metadata(name: &str, metadata: &Metadata) -> bool {
    (name.is_empty() || allowed_component(name))
        && !linked(metadata)
        && !crate::tools::generated_documents::hidden(metadata)
        && (!metadata.is_file() || metadata.nlink() == 1)
}

fn supported_file(path: &str) -> bool {
    path.rsplit_once('.').is_some_and(|(_, extension)| {
        matches!(
            extension.to_ascii_lowercase().as_str(),
            "txt"
                | "md"
                | "markdown"
                | "csv"
                | "tsv"
                | "json"
                | "log"
                | "html"
                | "htm"
                | "xml"
                | "rst"
                | "yaml"
                | "yml"
                | "docx"
                | "pdf"
                | "xlsx"
        )
    })
}

pub(super) fn verify_document(
    grant: &KnowledgeFolderGrant,
    relative: &str,
    expected_sha256: &str,
) -> Result<usize, String> {
    if !crate::projects::valid_relative_path(relative) {
        return Err(UNAVAILABLE.into());
    }
    let root = open_grant(grant)?;
    let bytes = root
        .read_file_snapshot_with_policy(relative, MAX_FILE_BYTES, allowed_metadata)
        .map_err(|_| UNAVAILABLE)?;
    if hex::encode(Sha256::digest(&bytes)) != expected_sha256 {
        return Err(UNAVAILABLE.into());
    }
    open_grant(grant)?;
    Ok(bytes.len())
}

struct Walker<'a, F> {
    root: &'a WorkspaceBytes,
    cancel: &'a CancelSignal,
    check: &'a F,
    remaining_bytes: usize,
    visited: usize,
    snapshot: FolderSnapshot,
}

impl<F: Fn() -> Result<(), String>> Walker<'_, F> {
    fn walk(&mut self, directory: &Dir, prefix: &str, depth: usize) -> Result<(), String> {
        (self.check)()?;
        let mut names = Vec::new();
        for entry in directory.entries().map_err(|_| UNAVAILABLE)? {
            if self.visited >= MAX_ENTRIES {
                self.snapshot.omitted_count += 1;
                break;
            }
            self.visited += 1;
            match entry {
                Ok(entry) => names.push(entry.file_name()),
                Err(_) => self.snapshot.omitted_count += 1,
            }
        }
        names.sort();
        for name in names {
            (self.check)()?;
            if self.cancel.is_cancelled() {
                return Err("Knowledge scan stopped".into());
            }
            let Some(name) = name.to_str() else {
                self.snapshot.omitted_count += 1;
                continue;
            };
            let metadata = match directory.symlink_metadata(name) {
                Ok(value) => value,
                Err(_) => {
                    self.snapshot.omitted_count += 1;
                    continue;
                }
            };
            if !allowed_metadata(name, &metadata) {
                self.snapshot.omitted_count += 1;
                continue;
            }
            let relative = if prefix.is_empty() {
                name.to_owned()
            } else {
                format!("{prefix}/{name}")
            };
            if relative.len() > 1024 {
                self.snapshot.omitted_count += 1;
                continue;
            }
            if metadata.is_dir() {
                if depth >= MAX_DEPTH {
                    self.snapshot.omitted_count += 1;
                    continue;
                }
                match directory.open_dir_nofollow(name) {
                    Ok(child)
                        if child.dir_metadata().is_ok_and(|opened| {
                            allowed_metadata(name, &opened)
                                && identity(&opened) == identity(&metadata)
                        }) =>
                    {
                        self.walk(&child, &relative, depth + 1)?
                    }
                    _ => self.snapshot.omitted_count += 1,
                }
            } else if metadata.is_file() {
                if !supported_file(&relative)
                    || self.snapshot.documents.len() >= MAX_DOCUMENTS
                    || metadata.len() > MAX_FILE_BYTES as u64
                    || metadata.len() > self.remaining_bytes as u64
                {
                    self.snapshot.omitted_count += 1;
                    continue;
                }
                match self.root.read_file_snapshot_with_policy(
                    &relative,
                    MAX_FILE_BYTES.min(self.remaining_bytes),
                    allowed_metadata,
                ) {
                    Ok(bytes) => {
                        self.remaining_bytes -= bytes.len();
                        let sha256 = hex::encode(Sha256::digest(&bytes));
                        self.snapshot.documents.push(FolderDocument {
                            relative_path: relative,
                            bytes,
                            sha256,
                        });
                    }
                    Err(_) => self.snapshot.omitted_count += 1,
                }
            } else {
                self.snapshot.omitted_count += 1;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_an_explicit_folder_and_rejects_protected_overlap() {
        let directory = tempfile::Builder::new()
            .prefix("anima-knowledge-test-")
            .tempdir()
            .unwrap();
        let documents = directory.path().join("documents");
        let private = directory.path().join("private-profile");
        std::fs::create_dir(&documents).unwrap();
        std::fs::create_dir(&private).unwrap();
        let grant = capture_folder(&documents, std::slice::from_ref(&private)).unwrap();
        assert_eq!(
            Path::new(&grant.root_path),
            documents.canonicalize().unwrap()
        );
        assert_eq!(grant.root_identity.len(), 64);
        assert!(capture_folder(directory.path(), std::slice::from_ref(&private)).is_err());
        assert!(capture_folder(&private, std::slice::from_ref(&private)).is_err());
    }

    #[test]
    fn rejects_sensitive_root_names_and_changed_folder_identity() {
        let directory = tempfile::Builder::new()
            .prefix("anima-knowledge-test-")
            .tempdir()
            .unwrap();
        for name in [".ssh", ".codex", ".agents", ".aws", "credentials"] {
            let root = directory.path().join(name);
            std::fs::create_dir(&root).unwrap();
            assert!(capture_folder(&root, &[]).is_err(), "sensitive root {name}");
        }
    }

    #[test]
    fn scans_current_bytes_and_omits_sensitive_oversized_and_hardlinked_files() {
        let directory = tempfile::Builder::new()
            .prefix("anima-knowledge-test-")
            .tempdir()
            .unwrap();
        std::fs::write(directory.path().join("notes.txt"), b"ordinary notes").unwrap();
        std::fs::write(directory.path().join("credentials.txt"), b"private").unwrap();
        std::fs::write(
            directory.path().join("large.txt"),
            vec![0; MAX_FILE_BYTES + 1],
        )
        .unwrap();
        std::fs::write(directory.path().join("linked.txt"), b"linked").unwrap();
        std::fs::hard_link(
            directory.path().join("linked.txt"),
            directory.path().join("alias.txt"),
        )
        .unwrap();
        let grant = capture_folder(directory.path(), &[]).unwrap();
        let first = scan_folder(&grant, &CancelSignal::new(), || Ok(())).unwrap();
        assert_eq!(first.documents.len(), 1);
        assert_eq!(first.documents[0].bytes, b"ordinary notes");
        assert_eq!(first.omitted_count, 4);
        std::fs::write(directory.path().join("notes.txt"), b"modified notes").unwrap();
        let second = scan_folder(&grant, &CancelSignal::new(), || Ok(())).unwrap();
        assert_ne!(first.documents[0].sha256, second.documents[0].sha256);
        std::fs::remove_file(directory.path().join("notes.txt")).unwrap();
        assert!(scan_folder(&grant, &CancelSignal::new(), || Ok(()))
            .unwrap()
            .documents
            .is_empty());
    }

    #[test]
    fn revocation_and_replaced_roots_abort_scan() {
        let directory = tempfile::Builder::new()
            .prefix("anima-knowledge-test-")
            .tempdir()
            .unwrap();
        let root = directory.path().join("source");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("notes.md"), b"knowledge").unwrap();
        let grant = capture_folder(&root, &[]).unwrap();
        assert!(scan_folder(&grant, &CancelSignal::new(), || Err("revoked".into())).is_err());
        std::fs::rename(&root, directory.path().join("old")).unwrap();
        std::fs::create_dir(&root).unwrap();
        assert!(scan_folder(&grant, &CancelSignal::new(), || Ok(())).is_err());
    }
}
