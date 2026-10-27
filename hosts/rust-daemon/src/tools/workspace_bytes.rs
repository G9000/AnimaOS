//! Capability-relative exclusive uploads and bounded regular-file reads.
//! The generic workspace writer retains its existing overwrite semantics.
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt, OpenOptionsSyncExt};
use cap_std::fs::{Dir, File, Metadata, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

pub(crate) fn reparse_attributes(attributes: u32) -> bool {
    attributes & 0x400 != 0
}
pub(crate) fn linked(metadata: &Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use cap_std::fs::MetadataExt;
        if reparse_attributes(metadata.file_attributes()) {
            return true;
        }
    }
    false
}
fn unavailable() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "Workspace file is unavailable")
}
fn components(relative: &str) -> io::Result<Vec<&str>> {
    let parts = relative.split('/').collect::<Vec<_>>();
    if parts.iter().any(|part| {
        part.is_empty()
            || *part == "."
            || *part == ".."
            || part.contains(['\\', ':'])
            || part.chars().any(char::is_control)
    }) {
        return Err(unavailable());
    }
    Ok(parts)
}

pub(crate) struct WorkspaceBytes {
    pub(crate) canonical_root: PathBuf,
    directory: Dir,
}
#[derive(Debug)]
pub(crate) enum InstructionReadError {
    Missing,
    Linked,
    NonRegular,
    Oversized,
    Unreadable,
}
impl From<io::Error> for InstructionReadError {
    fn from(error: io::Error) -> Self {
        if error.kind() == io::ErrorKind::NotFound {
            Self::Missing
        } else {
            Self::Unreadable
        }
    }
}
impl WorkspaceBytes {
    /// Clone the held root capability for bounded package traversal and atomic
    /// directory mutations without reopening ambient paths.
    pub(crate) fn held_directory(&self) -> io::Result<Dir> {
        self.directory.try_clone()
    }
    /// Opened-handle link count, including Windows; an ordinary name can alias
    /// an instruction inode through a hard link without changing canonical paths.
    pub(crate) fn instruction_link_count(&self, relative: &str) -> io::Result<Option<u64>> {
        use cap_fs_ext::MetadataExt;
        let (directory, name) = match self.parent(relative, false) {
            Ok(parent) => parent,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        match open_regular_file(&directory, &name) {
            Ok(file) => Ok(Some(file.metadata()?.nlink())),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }
    /// Bounded marker existence only; never parses a repository marker or follows
    /// a marker link. Used only inside an already authorized directory chain.
    pub(crate) fn instruction_marker_exists(&self, relative: &str) -> bool {
        self.parent(relative, false)
            .ok()
            .and_then(|(directory, name)| directory.symlink_metadata(name).ok())
            .is_some_and(|metadata| !linked(&metadata) && (metadata.is_file() || metadata.is_dir()))
    }
    /// Instruction reads inspect and open each component through held nofollow
    /// capabilities. Size observations come only from the opened regular file.
    pub(crate) fn read_instruction_file(
        &self,
        relative: &str,
        max: usize,
    ) -> Result<Vec<u8>, InstructionReadError> {
        self.read_file_snapshot(relative, max)
    }
    /// Capture one bounded regular-file snapshot under this held root. Instruction
    /// reads retain their exact checks and error categories through this helper.
    pub(crate) fn read_file_snapshot(
        &self,
        relative: &str,
        max: usize,
    ) -> Result<Vec<u8>, InstructionReadError> {
        self.read_file_snapshot_with_policy(relative, max, |_, _| true)
    }
    /// Apply a caller's stricter browsing policy to metadata observed through
    /// held capabilities, including after opening and after the bounded read.
    /// Existing instruction/document callers keep their original policy.
    pub(crate) fn read_file_snapshot_with_policy(
        &self,
        relative: &str,
        max: usize,
        policy: impl Fn(&str, &Metadata) -> bool,
    ) -> Result<Vec<u8>, InstructionReadError> {
        let parts = components(relative)?;
        let mut directory = self.directory.try_clone()?;
        if !policy("", &directory.dir_metadata()?) {
            return Err(InstructionReadError::Unreadable);
        }
        for component in &parts[..parts.len() - 1] {
            let metadata = directory.symlink_metadata(component)?;
            if !policy(component, &metadata) {
                return Err(InstructionReadError::Unreadable);
            }
            if linked(&metadata) {
                return Err(InstructionReadError::Linked);
            }
            if !metadata.is_dir() {
                return Err(InstructionReadError::NonRegular);
            }
            directory = directory.open_dir_nofollow(component)?;
            let metadata = directory.dir_metadata()?;
            if !policy(component, &metadata) {
                return Err(InstructionReadError::Unreadable);
            }
            if linked(&metadata) {
                return Err(InstructionReadError::Linked);
            }
            if !metadata.is_dir() {
                return Err(InstructionReadError::NonRegular);
            }
        }
        let name = parts.last().expect("validated components");
        let metadata = directory.symlink_metadata(name)?;
        if !policy(name, &metadata) {
            return Err(InstructionReadError::Unreadable);
        }
        if linked(&metadata) {
            return Err(InstructionReadError::Linked);
        }
        if !metadata.is_file() {
            return Err(InstructionReadError::NonRegular);
        }
        let file = open_regular_file(&directory, name)?;
        let opened = file.metadata()?;
        if !policy(name, &opened) {
            return Err(InstructionReadError::Unreadable);
        }
        let expected = opened.len();
        if expected > max as u64 {
            return Err(InstructionReadError::Oversized);
        }
        let mut bytes = Vec::with_capacity(expected as usize);
        (&file)
            .take(max.saturating_add(1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > max {
            return Err(InstructionReadError::Oversized);
        }
        let final_metadata = file.metadata()?;
        if bytes.len() as u64 != expected
            || final_metadata.len() != expected
            || !policy(name, &final_metadata)
        {
            return Err(InstructionReadError::Unreadable);
        }
        Ok(bytes)
    }
    /// Canonical binding and capability are captured once per operation.
    pub(crate) fn open(root: &Path) -> io::Result<Self> {
        Self::open_canonical(root.canonicalize()?)
    }
    fn open_canonical(canonical_root: PathBuf) -> io::Result<Self> {
        // Only the filesystem/volume anchor is ambient authority. Every
        // canonical normal component is opened relative to a held directory,
        // so replacing any ancestor with a link cannot redirect root capture.
        let mut anchor = PathBuf::new();
        let mut names = Vec::new();
        for component in canonical_root.components() {
            match component {
                std::path::Component::Prefix(_) | std::path::Component::RootDir => {
                    anchor.push(component.as_os_str())
                }
                std::path::Component::Normal(name) => names.push(name.to_os_string()),
                _ => return Err(unavailable()),
            }
        }
        if !anchor.is_absolute() {
            return Err(unavailable());
        }
        let mut directory = Dir::open_ambient_dir(&anchor, cap_std::ambient_authority())?;
        for name in names {
            let metadata = directory.symlink_metadata(&name)?;
            if !metadata.is_dir() || linked(&metadata) {
                return Err(unavailable());
            }
            directory = directory.open_dir_nofollow(&name)?;
            let metadata = directory.dir_metadata()?;
            if !metadata.is_dir() || linked(&metadata) {
                return Err(unavailable());
            }
        }
        let metadata = directory.dir_metadata()?;
        if !metadata.is_dir() || linked(&metadata) {
            return Err(unavailable());
        }
        Ok(Self {
            canonical_root,
            directory,
        })
    }
    fn parent(&self, relative: &str, create: bool) -> io::Result<(Dir, String)> {
        let parts = components(relative)?;
        let mut directory = self.directory.try_clone()?;
        for component in &parts[..parts.len() - 1] {
            match directory.symlink_metadata(component) {
                Ok(metadata) if !metadata.is_dir() || linked(&metadata) => {
                    return Err(unavailable())
                }
                Ok(_) => {}
                Err(error) if create && error.kind() == io::ErrorKind::NotFound => {
                    match directory.create_dir(component) {
                        Ok(()) => {}
                        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                        Err(error) => return Err(error),
                    }
                }
                Err(error) => return Err(error),
            }
            directory = directory.open_dir_nofollow(component)?;
            let metadata = directory.dir_metadata()?;
            if !metadata.is_dir() || linked(&metadata) {
                return Err(unavailable());
            }
        }
        Ok((
            directory,
            parts.last().expect("nonempty components").to_string(),
        ))
    }
    /// Atomic collision-safe sibling of write_workspace_bytes; never truncates.
    pub(crate) fn create_new_bytes(&self, relative: &str, bytes: &[u8]) -> io::Result<()> {
        let (directory, name) = self.parent(relative, true)?;
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .follow(FollowSymlinks::No)
            .nonblock(true);
        let mut file = directory.open_with(&name, &options)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || linked(&metadata) {
            return Err(unavailable());
        }
        file.write_all(bytes)?;
        file.sync_all()?;
        Ok(())
    }
    /// Publish a complete synchronized file without replacing a colliding name.
    pub(crate) fn create_new_bytes_atomic<G>(
        &self,
        relative: &str,
        bytes: &[u8],
        authorize: impl FnOnce() -> io::Result<G>,
    ) -> io::Result<()> {
        let (directory, name) = self.parent(relative, true)?;
        let temporary = format!(".anima-document-{}.tmp", uuid::Uuid::new_v4());
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .follow(FollowSymlinks::No)
            .nonblock(true);
        #[cfg(unix)]
        {
            use cap_std::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let result: io::Result<()> = (|| {
            let mut file = directory.open_with(&temporary, &options)?;
            file.write_all(bytes)?;
            file.sync_all()?;
            drop(file);
            let authority = authorize()?;
            directory.hard_link(&temporary, &directory, &name)?;
            drop(authority);
            Ok(())
        })();
        let cleanup = directory.remove_file(&temporary);
        result?;
        cleanup?;
        #[cfg(unix)]
        directory.try_clone()?.into_std_file().sync_all()?;
        Ok(())
    }
    /// Replace an existing instruction through the retained parent capability.
    /// A synchronized exclusive sibling is published only after exact preimage,
    /// single-link, permission, and caller authority checks succeed.
    pub(crate) fn compare_replace_instruction_file<G>(
        &self,
        relative: &str,
        expected: &[u8],
        replacement: &[u8],
        authorize: impl FnOnce() -> io::Result<G>,
    ) -> io::Result<()> {
        use cap_fs_ext::MetadataExt;
        let (directory, name) = self.parent(relative, false)?;
        let original = open_regular_file(&directory, &name)?;
        let metadata = original.metadata()?;
        if metadata.nlink() != 1 || metadata.permissions().readonly() {
            return Err(unavailable());
        }
        let same_preimage = || -> io::Result<()> {
            let named = open_regular_file(&directory, &name)?;
            let current = named.metadata()?;
            if current.nlink() != 1
                || (current.dev(), current.ino()) != (metadata.dev(), metadata.ino())
            {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "Personality source changed; reload before saving",
                ));
            }
            let mut bytes = Vec::new();
            (&named)
                .take(expected.len().saturating_add(1) as u64)
                .read_to_end(&mut bytes)?;
            if bytes != expected || named.metadata()?.len() != expected.len() as u64 {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "Personality source changed; reload before saving",
                ));
            }
            Ok(())
        };
        same_preimage()?;
        let temporary = format!(".anima-personality-{}.tmp", uuid::Uuid::new_v4());
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .follow(FollowSymlinks::No)
            .nonblock(true);
        // The temporary has no broader POSIX mode even during creation.
        #[cfg(unix)]
        {
            use cap_std::fs::{OpenOptionsExt, PermissionsExt};
            options.mode(metadata.permissions().mode() & 0o777);
        }
        let mut file = directory.open_with(&temporary, &options)?;
        let result = (|| -> io::Result<()> {
            file.set_permissions(metadata.permissions())?;
            ensure_same_instruction_permissions(&original, &file)?;
            file.write_all(replacement)?;
            file.sync_all()?;
            same_preimage()?;
            ensure_same_instruction_permissions(&original, &file)?;
            let authority = authorize()?;
            same_preimage()?;
            // Windows rename needs the temporary's writable handle closed first.
            drop(file);
            directory.rename(&temporary, &directory, &name)?;
            drop(authority);
            #[cfg(unix)]
            directory
                .try_clone()
                .and_then(|held| held.into_std_file().sync_all())
                .map_err(|_| crate::control_plane_store::durability_uncertain_error())?;
            Ok(())
        })();
        if result.is_err() {
            let _ = directory.remove_file(&temporary);
        }
        result
    }

    pub(crate) fn read_bounded(
        &self,
        relative: &str,
        max: usize,
        expected: u64,
    ) -> io::Result<Vec<u8>> {
        let (directory, name) = self.parent(relative, false)?;
        let file = open_regular_file(&directory, &name)?;
        let length = file.metadata()?.len();
        if length > max as u64 {
            return Err(io::Error::new(
                io::ErrorKind::FileTooLarge,
                "Workspace file exceeds its size limit",
            ));
        }
        if length != expected {
            return Err(unavailable());
        }
        let mut bytes = Vec::with_capacity(length as usize);
        (&file).take((max + 1) as u64).read_to_end(&mut bytes)?;
        if bytes.len() > max {
            return Err(io::Error::new(
                io::ErrorKind::FileTooLarge,
                "Workspace file exceeds its size limit",
            ));
        }
        if bytes.len() as u64 != expected || file.metadata()?.len() != expected {
            return Err(unavailable());
        }
        Ok(bytes)
    }
}

fn ensure_same_instruction_permissions(original: &File, temporary: &File) -> io::Result<()> {
    #[cfg(windows)]
    {
        ensure_same_windows_acl(original, temporary)?;
    }
    #[cfg(unix)]
    {
        use cap_std::fs::MetadataExt;
        let before = original.metadata()?;
        let after = temporary.metadata()?;
        if before.uid() != after.uid()
            || before.gid() != after.gid()
            || before.mode() != after.mode()
        {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "SOUL.md ownership or permissions cannot be preserved safely; edit this file directly"));
        }
        #[cfg(any(
            target_os = "linux",
            target_os = "android",
            target_os = "macos",
            target_os = "ios",
            target_os = "tvos",
            target_os = "watchos",
            target_os = "visionos",
            target_os = "hurd"
        ))]
        {
            // A replacement inode could lose ACLs, labels, or other access
            // metadata. Refuse any extended attributes rather than weaken them.
            for file in [original, temporary] {
                match rustix::fs::flistxattr(file, &mut [] as &mut [u8]) {
                    Ok(0) => {},
                    Err(rustix::io::Errno::NOTSUP) => {},
                    _ => return Err(io::Error::new(io::ErrorKind::PermissionDenied, "SOUL.md extended access metadata cannot be preserved safely; edit this file directly")),
                }
            }
        }
        #[cfg(not(any(
            target_os = "linux",
            target_os = "android",
            target_os = "macos",
            target_os = "ios",
            target_os = "tvos",
            target_os = "watchos",
            target_os = "visionos",
            target_os = "hurd"
        )))]
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "SOUL.md access metadata cannot be verified on this platform; edit this file directly",
        ));
    }
    Ok(())
}

/// Version-bound adapter for win-custody exactly =0.1.1. Its validated owner
/// and DACL derive Debug recursively from complete SID byte vectors and ordered
/// ACE flags/masks; unlike ObjectSecurityDescriptor, these contain no pointers.
/// Reinspect these private representations and rerun the arbitrary-trustee and
/// access-change regressions before upgrading the dependency. Unsupported ACEs
/// still fail closed through dacl() validation.
#[cfg(windows)]
fn ensure_same_windows_acl(original: &File, temporary: &File) -> io::Result<()> {
    use win_custody::ObjectSecurityDescriptor;
    let acl_error = || {
        io::Error::new(io::ErrorKind::PermissionDenied,
        "SOUL.md has custom permissions that cannot be preserved safely; edit this file directly")
    };
    let fingerprint = |file: &File| -> io::Result<_> {
        let descriptor = ObjectSecurityDescriptor::query_kernel_object(file)?;
        let owner = descriptor.owner().map_err(|_| acl_error())?;
        let acl = descriptor.dacl().map_err(|_| acl_error())?;
        Ok((
            descriptor.control()?,
            format!("{owner:?}"),
            format!("{acl:?}"),
        ))
    };
    if fingerprint(original)? != fingerprint(temporary)? {
        return Err(acl_error());
    }
    Ok(())
}

pub(crate) fn open_regular_file(directory: &Dir, name: &str) -> io::Result<File> {
    // Precheck rejects known special files. The held no-follow/nonblocking handle
    // check closes the replacement gap without waiting on a replaced FIFO.
    let metadata = directory.symlink_metadata(name)?;
    if !metadata.is_file() || linked(&metadata) {
        return Err(unavailable());
    }
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No).nonblock(true);
    let file = directory.open_with(name, &options)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || linked(&metadata) {
        return Err(unavailable());
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finished_documents_atomic_publication_preserves_collisions_and_honors_final_authority() {
        let root = tempfile::tempdir().unwrap();
        let workspace = WorkspaceBytes::open(root.path()).unwrap();
        let target = root.path().join("report.docx");
        let result = workspace.create_new_bytes_atomic("report.docx", b"new document", || {
            std::fs::write(&target, b"concurrent owner document")?;
            Ok(())
        });
        assert!(result.is_err());
        assert_eq!(
            std::fs::read(&target).unwrap(),
            b"concurrent owner document"
        );
        std::fs::remove_file(&target).unwrap();
        let cancelled = workspace.create_new_bytes_atomic("report.docx", b"new document", || {
            Err::<(), _>(io::Error::new(io::ErrorKind::Interrupted, "cancelled"))
        });
        assert!(cancelled.is_err());
        assert!(!target.exists());
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }

    #[test]
    fn finished_documents_revision_rechecks_preimage_after_final_authorization() {
        let root = tempfile::tempdir().unwrap();
        let workspace = WorkspaceBytes::open(root.path()).unwrap();
        let target = root.path().join("report.docx");
        std::fs::write(&target, b"original").unwrap();
        let result = workspace.compare_replace_instruction_file(
            "report.docx",
            b"original",
            b"revision",
            || {
                std::fs::write(&target, b"owner changed during authorization")?;
                Ok(())
            },
        );
        assert!(result.is_err());
        assert_eq!(
            std::fs::read(target).unwrap(),
            b"owner changed during authorization"
        );
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
    }
    #[cfg(windows)]
    #[test]
    fn personality_windows_acl_comparison_preserves_arbitrary_trustees_and_access_fields() {
        let base = tempfile::tempdir().unwrap();
        let held = WorkspaceBytes::open(base.path()).unwrap();
        let user = win_custody::current_user_sid_string().unwrap();
        let own = format!("(A;;FA;;;{user})");
        let first = "S-1-5-21-111-222-333-4444";
        let second = "S-1-5-21-111-222-333-5555";
        let allow = format!("D:P{own}(A;;FR;;;{first})");
        let cases = [
            ("same arbitrary trustee", allow.clone(), true, true),
            (
                "different arbitrary trustee",
                format!("D:P{own}(A;;FR;;;{second})"),
                true,
                false,
            ),
            (
                "different access mask",
                format!("D:P{own}(A;;FW;;;{first})"),
                true,
                false,
            ),
            (
                "different protection control",
                format!("D:{own}(A;;FR;;;{first})"),
                false,
                false,
            ),
            (
                "unsupported deny ACE",
                format!("D:P{own}(D;;FW;;;{first})"),
                true,
                false,
            ),
        ];
        for (index, (label, candidate, protect, equal)) in cases.into_iter().enumerate() {
            let left = format!("left-{index}");
            let right = format!("right-{index}");
            std::fs::write(base.path().join(&left), "left").unwrap();
            std::fs::write(base.path().join(&right), "right").unwrap();
            win_custody::SecurityDescriptor::from_sddl(&allow)
                .unwrap()
                .apply_to_tree(&base.path().join(&left), true)
                .unwrap();
            win_custody::SecurityDescriptor::from_sddl(&candidate)
                .unwrap()
                .apply_to_tree(&base.path().join(&right), protect)
                .unwrap();
            assert_eq!(
                ensure_same_windows_acl(
                    &held.directory.open(&left).unwrap(),
                    &held.directory.open(&right).unwrap()
                )
                .is_ok(),
                equal,
                "{label}"
            );
        }
        for (name, acl) in [
            ("flags-left", format!("D:P{own}(A;OICI;FR;;;{first})")),
            ("flags-right", format!("D:P{own}(A;CI;FR;;;{first})")),
        ] {
            std::fs::create_dir(base.path().join(name)).unwrap();
            win_custody::SecurityDescriptor::from_sddl(&acl)
                .unwrap()
                .apply_to_tree(&base.path().join(name), true)
                .unwrap();
        }
        let flags_left = File::from_std(
            held.directory
                .open_dir("flags-left")
                .unwrap()
                .into_std_file(),
        );
        let flags_right = File::from_std(
            held.directory
                .open_dir("flags-right")
                .unwrap()
                .into_std_file(),
        );
        assert!(ensure_same_windows_acl(&flags_left, &flags_right).is_err());
        for (name, acl) in [
            (
                "ordered-left",
                format!("D:P{own}(A;;FR;;;{first})(A;;FR;;;{second})"),
            ),
            (
                "ordered-right",
                format!("D:P{own}(A;;FR;;;{second})(A;;FR;;;{first})"),
            ),
        ] {
            std::fs::write(base.path().join(name), "order").unwrap();
            win_custody::SecurityDescriptor::from_sddl(&acl)
                .unwrap()
                .apply_to_tree(&base.path().join(name), true)
                .unwrap();
        }
        assert!(ensure_same_windows_acl(
            &held.directory.open("ordered-left").unwrap(),
            &held.directory.open("ordered-right").unwrap()
        )
        .is_err());
    }

    #[test]
    fn personality_atomic_preimage_and_authority_failures_leave_original_untouched() {
        let base = tempfile::tempdir().unwrap();
        std::fs::write(base.path().join("SOUL.md"), "original").unwrap();
        let held = WorkspaceBytes::open(base.path()).unwrap();
        let error = held
            .compare_replace_instruction_file("SOUL.md", b"stale", b"new", || Ok(()))
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        let error = held
            .compare_replace_instruction_file("SOUL.md", b"original", b"new", || {
                Err::<(), _>(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "owner lost",
                ))
            })
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(
            std::fs::read(base.path().join("SOUL.md")).unwrap(),
            b"original"
        );
        assert_eq!(std::fs::read_dir(base.path()).unwrap().count(), 1);
        held.compare_replace_instruction_file("SOUL.md", b"original", b"saved", || Ok(()))
            .unwrap();
        assert_eq!(
            std::fs::read(base.path().join("SOUL.md")).unwrap(),
            b"saved"
        );
    }

    #[test]
    fn workspace_bytes_reparse_attributes_are_refused() {
        assert!(reparse_attributes(0x400));
        assert!(reparse_attributes(0x410));
        assert!(!reparse_attributes(0x10));
        assert!(!reparse_attributes(0x80));
    }
    #[test]
    fn workspace_bytes_relative_paths_cannot_escape() {
        for path in [
            "",
            ".",
            "..",
            "../file",
            "a/../file",
            "a//file",
            "/file",
            "C:/file",
            "x:stream",
            "a\\file",
            "a/./file",
        ] {
            assert!(components(path).is_err(), "{path}");
        }
        assert_eq!(
            components("uploads/2026-09-23/notes.txt").unwrap(),
            ["uploads", "2026-09-23", "notes.txt"]
        );
    }
    #[cfg(windows)]
    #[test]
    fn workspace_bytes_held_root_prevents_or_survives_junction_replacement() {
        let base = std::env::temp_dir().join(format!("anima-held-root-{}", uuid::Uuid::new_v4()));
        let root = base.join("workspace");
        let retained = base.join("retained");
        let outside = base.join("outside");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let held = WorkspaceBytes::open(&root).unwrap();
        if let Err(error) = std::fs::rename(&root, &retained) {
            // Windows may deny replacement while the capability is held.
            assert_eq!(error.raw_os_error(), Some(32));
            held.create_new_bytes("uploads/notes.txt", b"held").unwrap();
            assert_eq!(
                std::fs::read(root.join("uploads/notes.txt")).unwrap(),
                b"held"
            );
            assert!(!outside.join("uploads").exists());
            drop(held);
            std::fs::remove_dir_all(&base).unwrap();
            return;
        }
        assert!(std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&root)
            .arg(&outside)
            .output()
            .unwrap()
            .status
            .success());
        held.create_new_bytes("uploads/notes.txt", b"held").unwrap();
        assert_eq!(
            std::fs::read(retained.join("uploads/notes.txt")).unwrap(),
            b"held"
        );
        assert!(!outside.join("uploads").exists());
        drop(held);
        std::fs::remove_dir(&root).unwrap();
        std::fs::remove_dir_all(&base).unwrap();
    }
    #[cfg(windows)]
    #[test]
    fn workspace_bytes_initial_capture_refuses_replaced_ancestor() {
        let base =
            std::env::temp_dir().join(format!("anima-capture-root-{}", uuid::Uuid::new_v4()));
        let ancestor = base.join("ancestor");
        let root = ancestor.join("workspace");
        let retained = base.join("retained");
        let outside = base.join("outside");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(outside.join("workspace")).unwrap();
        let canonical = root.canonicalize().unwrap();
        std::fs::rename(&ancestor, &retained).unwrap();
        let result = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(ancestor.to_string_lossy().replace('/', r"\"))
            .arg(outside.to_string_lossy().replace('/', r"\"))
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "junction fixture: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        let opened = WorkspaceBytes::open_canonical(canonical);
        let refused = opened.is_err();
        drop(opened);
        std::fs::remove_dir(&ancestor).unwrap();
        std::fs::remove_dir_all(&base).unwrap();
        assert!(
            refused,
            "a swapped ancestor cannot grant authority over another workspace"
        );
    }
}
