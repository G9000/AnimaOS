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
        let parts = components(relative)?;
        let mut directory = self.directory.try_clone()?;
        for component in &parts[..parts.len() - 1] {
            let metadata = directory.symlink_metadata(component)?;
            if linked(&metadata) {
                return Err(InstructionReadError::Linked);
            }
            if !metadata.is_dir() {
                return Err(InstructionReadError::NonRegular);
            }
            directory = directory.open_dir_nofollow(component)?;
            let metadata = directory.dir_metadata()?;
            if linked(&metadata) {
                return Err(InstructionReadError::Linked);
            }
            if !metadata.is_dir() {
                return Err(InstructionReadError::NonRegular);
            }
        }
        let name = parts.last().expect("validated components");
        let metadata = directory.symlink_metadata(name)?;
        if linked(&metadata) {
            return Err(InstructionReadError::Linked);
        }
        if !metadata.is_file() {
            return Err(InstructionReadError::NonRegular);
        }
        let file = open_regular_file(&directory, name)?;
        let expected = file.metadata()?.len();
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
        if bytes.len() as u64 != expected || file.metadata()?.len() != expected {
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
