//! Short private Office working directories retained by the physical lease.
use crate::tools::workspace_bytes::{linked, WorkspaceBytes};
use cap_fs_ext::{DirExt, MetadataExt};
use std::{
    io,
    path::{Component, Path, PathBuf, Prefix},
};
use win_custody::{current_user_sid_string, SecurityDescriptor};

pub(super) struct PrivateDirectory {
    path: PathBuf,
    parent: Option<WorkspaceBytes>,
    held: Option<WorkspaceBytes>,
    identity: Option<(u64, u64)>,
    parent_identity: Option<(u64, u64)>,
    _ancestors: Vec<cap_std::fs::Dir>,
    created: bool,
    cleanup_attempted: bool,
    #[cfg(test)]
    temporary: Option<tempfile::TempDir>,
}
impl PrivateDirectory {
    pub(super) fn plan() -> io::Result<Self> {
        // This is operator configuration, never a document request. The task
        // wrapper preserves the original root without its nested per-run TEMP.
        let root = std::env::var_os("ANIMAOS_TASK_TEMP_ROOT")
            .map_or_else(std::env::temp_dir, PathBuf::from);
        Self::plan_in(&root)
    }
    fn plan_in(root: &Path) -> io::Result<Self> {
        if !root.is_absolute()
            || !matches!(root.components().next(), Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)))
            || root
                .components()
                .any(|c| matches!(c, Component::CurDir | Component::ParentDir))
        {
            return Err(io::Error::other(
                "Office temporary root must be local and absolute",
            ));
        }
        let mut ancestors = Vec::new();
        let mut components = root.components();
        let mut anchor = PathBuf::from(
            components
                .next()
                .ok_or_else(|| io::Error::other("Missing Office drive"))?
                .as_os_str(),
        );
        anchor.push("\\");
        let mut directory =
            cap_std::fs::Dir::open_ambient_dir(anchor, cap_std::ambient_authority())?;
        for component in components {
            let name = match component {
                Component::RootDir => continue,
                Component::Normal(name) => name,
                _ => return Err(io::Error::other("Invalid Office root component")),
            };
            let named = directory.symlink_metadata(name)?;
            if !named.is_dir() || linked(&named) {
                return Err(io::Error::other("Office temporary root is redirected"));
            }
            let child = directory.open_dir_nofollow(name)?;
            let captured = child.dir_metadata()?;
            if !captured.is_dir()
                || linked(&captured)
                || (named.dev(), named.ino()) != (captured.dev(), captured.ino())
            {
                return Err(io::Error::other("Office root component changed"));
            }
            // Hold every predecessor without delete sharing. Each next open is
            // relative and no-follow, bound to its named observation by identity.
            ancestors.push(directory);
            directory = child;
        }
        let literal = directory.dir_metadata()?;
        ancestors.push(directory);
        let parent = WorkspaceBytes::open(root)?;
        crate::document_edits::store::verify_private_directory(
            &parent.canonical_root,
            &parent,
            true,
        )
        .map_err(io::Error::other)?;
        let metadata = parent.held_directory()?.dir_metadata()?;
        if (metadata.dev(), metadata.ino()) != (literal.dev(), literal.ino()) {
            return Err(io::Error::other("Office root custody changed"));
        }
        let path = parent
            .canonical_root
            .join(format!("ai-{}", uuid::Uuid::new_v4().simple()));
        use std::os::windows::ffi::OsStrExt;
        if path.as_os_str().encode_wide().count() > 100 {
            return Err(io::Error::other("Office temporary root is too long"));
        }
        Ok(Self {
            path,
            parent: Some(parent),
            held: None,
            identity: None,
            parent_identity: Some((metadata.dev(), metadata.ino())),
            _ancestors: ancestors,
            created: false,
            cleanup_attempted: false,
            #[cfg(test)]
            temporary: None,
        })
    }
    fn verify_parent(&self) -> io::Result<()> {
        let parent = self
            .parent
            .as_ref()
            .ok_or_else(|| io::Error::other("Missing Office parent custody"))?;
        let named = WorkspaceBytes::open(&parent.canonical_root)?;
        let named_metadata = named.held_directory()?.dir_metadata()?;
        let captured = parent.held_directory()?.dir_metadata()?;
        if Some((named_metadata.dev(), named_metadata.ino())) != self.parent_identity
            || (named_metadata.dev(), named_metadata.ino()) != (captured.dev(), captured.ino())
        {
            return Err(io::Error::other("Office parent custody changed"));
        }
        crate::document_edits::store::verify_private_directory(&parent.canonical_root, parent, true)
            .map_err(io::Error::other)
    }
    pub(super) fn create_and_verify(&mut self) -> io::Result<()> {
        self.verify_parent()?;
        if self.created {
            return Err(io::Error::other("Office directory already created"));
        }
        let user = current_user_sid_string()?;
        SecurityDescriptor::from_sddl(&format!("O:{user}D:P(A;OICI;FA;;;{user})(A;OICI;FA;;;SY)"))?
            .create_directory(&self.path)?;
        // The lease owns this path BEFORE creation. Every subsequent failure
        // retains this unverified object and the worker slot in quarantine.
        self.created = true;
        let held = WorkspaceBytes::open(&self.path)?;
        crate::document_edits::store::verify_private_directory(&self.path, &held, false)
            .map_err(io::Error::other)?;
        let captured = held.held_directory()?.dir_metadata()?;
        let parent = self
            .parent
            .as_ref()
            .ok_or_else(|| io::Error::other("Missing Office parent"))?;
        let name = self
            .path
            .file_name()
            .ok_or_else(|| io::Error::other("Missing Office name"))?;
        let named = parent.held_directory()?.symlink_metadata(name)?;
        if !named.is_dir()
            || linked(&named)
            || (named.dev(), named.ino()) != (captured.dev(), captured.ino())
        {
            return Err(io::Error::other("Office directory custody changed"));
        }
        self.verify_parent()?;
        self.identity = Some((captured.dev(), captured.ino()));
        self.held = Some(held);
        Ok(())
    }
    #[cfg(test)]
    pub(super) fn new(temporary: tempfile::TempDir) -> Self {
        Self {
            path: temporary.path().to_owned(),
            parent: None,
            held: None,
            identity: None,
            parent_identity: None,
            _ancestors: Vec::new(),
            created: false,
            cleanup_attempted: false,
            temporary: Some(temporary),
        }
    }
    pub(super) fn path(&self) -> &Path {
        &self.path
    }
    pub(super) fn close(&mut self) -> io::Result<()> {
        if self.cleanup_attempted {
            return if self.path.try_exists()? {
                Err(io::Error::other("Office cleanup remains uncertain"))
            } else {
                Ok(())
            };
        }
        #[cfg(test)]
        if let Some(temporary) = self.temporary.take() {
            self.cleanup_attempted = true;
            return temporary.close();
        }
        if !self.created {
            return Ok(());
        }
        self.verify_parent()?;
        let parent = self
            .parent
            .as_ref()
            .ok_or_else(|| io::Error::other("Missing Office parent custody"))?;
        let directory = parent.held_directory()?;
        let name = self
            .path
            .file_name()
            .ok_or_else(|| io::Error::other("Missing Office name"))?;
        let named = directory.symlink_metadata(name)?;
        let held = self
            .held
            .as_ref()
            .ok_or_else(|| io::Error::other("Unverified Office directory"))?;
        let captured = held.held_directory()?.dir_metadata()?;
        if !named.is_dir()
            || linked(&named)
            || Some((named.dev(), named.ino())) != self.identity
            || (named.dev(), named.ino()) != (captured.dev(), captured.ino())
        {
            return Err(io::Error::other("Office directory custody changed"));
        }
        crate::document_edits::store::verify_private_directory(&self.path, held, false)
            .map_err(io::Error::other)?;
        // Single checked removal under a trusted held parent. This is not an
        // atomic handle-delete primitive. A failure is never retried.
        self.held.take();
        self.cleanup_attempted = true;
        directory.remove_dir_all(name)?;
        if directory.try_exists(name)? {
            return Err(io::Error::other("Office directory remained after cleanup"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn office_root_rejects_relative_network_device_and_parent_paths() {
        for root in [
            "relative",
            r"\\server\share\temp",
            r"\\.\C:\temp",
            r"C:\temp\..\other",
        ] {
            assert!(PrivateDirectory::plan_in(Path::new(root)).is_err());
        }
    }
    #[test]
    fn office_directory_is_private_and_owned_until_checked_cleanup() {
        let mut directory = PrivateDirectory::plan().unwrap();
        assert!(!directory.path().exists());
        directory.create_and_verify().unwrap();
        let path = directory.path().to_owned();
        crate::document_edits::store::verify_private_directory(
            &path,
            directory.held.as_ref().unwrap(),
            false,
        )
        .unwrap();
        assert!(std::fs::rename(&path, path.with_extension("replaced")).is_err());
        std::fs::write(path.join("owned"), b"synthetic").unwrap();
        directory.close().unwrap();
        assert!(!path.exists());
    }
    #[test]
    fn collision_is_not_adopted_or_removed() {
        let mut directory = PrivateDirectory::plan().unwrap();
        std::fs::create_dir(directory.path()).unwrap();
        assert!(directory.create_and_verify().is_err());
        directory.close().unwrap();
        assert!(directory.path().exists());
        std::fs::remove_dir(directory.path()).unwrap();
    }
    #[test]
    fn created_but_unverified_directory_cannot_release_custody() {
        let mut directory = PrivateDirectory::plan().unwrap();
        let user = current_user_sid_string().unwrap();
        SecurityDescriptor::from_sddl(&format!("O:{user}D:P(A;OICI;FA;;;{user})(A;OICI;FA;;;SY)"))
            .unwrap()
            .create_directory(directory.path())
            .unwrap();
        directory.created = true;
        assert!(directory.close().is_err());
        assert!(directory.path().exists());
        std::fs::remove_dir(directory.path()).unwrap();
    }
}
