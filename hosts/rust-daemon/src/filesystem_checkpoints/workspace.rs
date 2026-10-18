use super::*;
use anima_core::filesystem_checkpoints::{checkpoint_digest, PathSemantics};
use cap_fs_ext::{DirExt, FollowSymlinks, MetadataExt, OpenOptionsFollowExt, OpenOptionsSyncExt};
use cap_std::fs::{Dir, File, Metadata, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Component, Path};

pub(super) fn io_error(error: io::Error) -> CaptureError {
    #[cfg(windows)]
    if matches!(error.raw_os_error(), Some(32 | 33)) {
        return CaptureError::Busy;
    }
    match error.kind() {
        io::ErrorKind::NotFound => CaptureError::Unavailable,
        _ => CaptureError::Unavailable,
    }
}

pub(super) fn linked(metadata: &Metadata) -> bool {
    crate::tools::workspace_bytes::linked(metadata)
}

pub(super) fn digest_value(domain: &[u8], value: &impl Serialize) -> Result<String, CaptureError> {
    let mut bytes = domain.to_vec();
    bytes.extend(serde_json::to_vec(value).map_err(|_| CaptureError::Corrupt)?);
    Ok(checkpoint_digest(&bytes))
}

/// Every original ancestor remains held for the operation. Never canonicalize
/// an untrusted linked root first: walk the original absolute path nofollow.
pub(super) struct HeldRoot {
    pub directory: Dir,
    _ancestors: Vec<Dir>,
}

impl HeldRoot {
    pub fn open(path: &Path) -> Result<Self, CaptureError> {
        if !path.is_absolute() {
            return Err(CaptureError::InvalidScope);
        }
        let mut anchor = PathBuf::new();
        let mut names = Vec::new();
        for component in path.components() {
            match component {
                Component::Prefix(_) | Component::RootDir => anchor.push(component.as_os_str()),
                Component::Normal(name) => names.push(name.to_os_string()),
                _ => return Err(CaptureError::UnsafePath),
            }
        }
        let mut directory =
            Dir::open_ambient_dir(anchor, cap_std::ambient_authority()).map_err(io_error)?;
        let mut ancestors = Vec::new();
        for name in names {
            let child = open_child(&directory, &name)?;
            ancestors.push(directory);
            directory = child;
        }
        Ok(Self {
            directory,
            _ancestors: ancestors,
        })
    }

    pub fn identity(&self) -> Result<String, CaptureError> {
        let metadata = self.directory.dir_metadata().map_err(io_error)?;
        digest_value(
            b"anima-filesystem-checkpoint-root-v1\0",
            &(metadata.dev(), metadata.ino()),
        )
    }
}

pub(super) fn open_child(directory: &Dir, name: impl AsRef<Path>) -> Result<Dir, CaptureError> {
    let metadata = directory.symlink_metadata(&name).map_err(io_error)?;
    if linked(&metadata) {
        return Err(CaptureError::Linked);
    }
    if !metadata.is_dir() {
        return Err(CaptureError::NonRegular);
    }
    let child = directory.open_dir_nofollow(name).map_err(io_error)?;
    let opened = child.dir_metadata().map_err(io_error)?;
    if linked(&opened)
        || !opened.is_dir()
        || metadata.dev() != opened.dev()
        || metadata.ino() != opened.ino()
    {
        return Err(CaptureError::Linked);
    }
    Ok(child)
}

pub(super) fn path_key(path: &str, semantics: PathSemantics) -> Result<String, CaptureError> {
    if path.is_empty() || path.len() > 1024 || path.split('/').count() > 64 {
        return Err(CaptureError::UnsafePath);
    }
    if semantics == PathSemantics::WindowsInsensitive && !path.is_ascii() {
        return Err(CaptureError::Unsupported);
    }
    for part in path.split('/') {
        if part.is_empty()
            || matches!(part, "." | "..")
            || part.contains(['\\', ':'])
            || part.chars().any(char::is_control)
        {
            return Err(CaptureError::UnsafePath);
        }
        if semantics == PathSemantics::WindowsInsensitive {
            let stem = part
                .split('.')
                .next()
                .unwrap_or_default()
                .to_ascii_lowercase();
            if part.ends_with(['.', ' '])
                || part.contains(['<', '>', '"', '|', '?', '*'])
                || matches!(stem.as_str(), "con" | "prn" | "aux" | "nul")
                || ((stem.starts_with("com") || stem.starts_with("lpt"))
                    && stem.len() == 4
                    && matches!(stem.as_bytes()[3], b'1'..=b'9'))
            {
                return Err(CaptureError::UnsafePath);
            }
        }
    }
    Ok(if semantics == PathSemantics::WindowsInsensitive {
        path.to_ascii_lowercase()
    } else {
        path.to_owned()
    })
}

pub(super) fn excluded(path: &str, policy: &CapturePolicy, semantics: PathSemantics) -> bool {
    let key = if semantics == PathSemantics::WindowsInsensitive {
        path.to_ascii_lowercase()
    } else {
        path.to_owned()
    };
    policy.excluded_prefixes.iter().any(|prefix| {
        let prefix = if semantics == PathSemantics::WindowsInsensitive {
            prefix.to_ascii_lowercase()
        } else {
            prefix.clone()
        };
        key == prefix
            || key
                .strip_prefix(&prefix)
                .is_some_and(|rest| rest.starts_with('/'))
    })
}

pub(super) fn basic_mode(metadata: &Metadata) -> BasicMode {
    #[cfg(windows)]
    {
        BasicMode::Windows {
            readonly: metadata.permissions().readonly(),
        }
    }
    #[cfg(unix)]
    {
        use cap_std::fs::PermissionsExt;
        BasicMode::Posix {
            mode: metadata.permissions().mode() & 0o7777,
        }
    }
}

/// Reuses the workspace_bytes containment/nofollow discipline, with a stricter
/// capture-specific Windows lease. Never retry with weaker sharing flags.
pub(super) fn open_file(directory: &Dir, name: &str) -> Result<File, CaptureError> {
    let before = directory.symlink_metadata(name).map_err(io_error)?;
    if linked(&before) {
        return Err(CaptureError::Linked);
    }
    if !before.is_file() {
        return Err(CaptureError::NonRegular);
    }
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No).nonblock(true);
    #[cfg(windows)]
    {
        use cap_std::fs::OpenOptionsExt;
        options.share_mode(1);
    }
    let file = directory.open_with(name, &options).map_err(io_error)?;
    let metadata = file.metadata().map_err(io_error)?;
    if linked(&metadata) || !metadata.is_file() || metadata.nlink() != 1 {
        return Err(CaptureError::Linked);
    }
    if metadata.dev() != before.dev() || metadata.ino() != before.ino() {
        return Err(CaptureError::Busy);
    }
    Ok(file)
}

pub(super) fn read_file(
    directory: &Dir,
    name: &str,
    max: u64,
) -> Result<(Vec<u8>, BasicMode), CaptureError> {
    let mut file = open_file(directory, name)?;
    read_open_file(&mut file, max)
}

pub(super) fn read_open_file(
    file: &mut File,
    max: u64,
) -> Result<(Vec<u8>, BasicMode), CaptureError> {
    file.seek(SeekFrom::Start(0)).map_err(io_error)?;
    let before = file.metadata().map_err(io_error)?;
    if linked(&before) || !before.is_file() || before.nlink() != 1 {
        return Err(CaptureError::Linked);
    }
    if before.len() > max {
        return Err(CaptureError::Budget);
    }
    let mut bytes = Vec::new();
    (&mut *file)
        .take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    let after = file.metadata().map_err(io_error)?;
    if bytes.len() as u64 > max {
        return Err(CaptureError::Budget);
    }
    if bytes.len() as u64 != before.len()
        || after.len() != before.len()
        || after.nlink() != 1
        || basic_mode(&before) != basic_mode(&after)
        || before.modified().ok() != after.modified().ok()
    {
        return Err(CaptureError::Busy);
    }
    Ok((bytes, basic_mode(&after)))
}

pub(super) struct HeldParent {
    pub directory: Dir,
    pub name: String,
    _ancestors: Vec<Dir>,
}

pub(super) fn held_parent(root: &Dir, path: &str) -> Result<HeldParent, CaptureError> {
    let parts = path.split('/').collect::<Vec<_>>();
    let mut directory = root.try_clone().map_err(io_error)?;
    let mut ancestors = Vec::new();
    for part in &parts[..parts.len() - 1] {
        let child = open_child(&directory, part)?;
        ancestors.push(directory);
        directory = child;
    }
    Ok(HeldParent {
        directory,
        name: parts.last().ok_or(CaptureError::UnsafePath)?.to_string(),
        _ancestors: ancestors,
    })
}

pub(super) fn open_read_write(parent: &HeldParent) -> Result<File, CaptureError> {
    let before = parent
        .directory
        .symlink_metadata(&parent.name)
        .map_err(io_error)?;
    if linked(&before) || !before.is_file() {
        return Err(CaptureError::Linked);
    }
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(true)
        .follow(FollowSymlinks::No)
        .nonblock(true);
    #[cfg(windows)]
    {
        use cap_std::fs::OpenOptionsExt;
        options.share_mode(1);
    }
    let file = parent
        .directory
        .open_with(&parent.name, &options)
        .map_err(io_error)?;
    let metadata = file.metadata().map_err(io_error)?;
    if linked(&metadata) || !metadata.is_file() || metadata.nlink() != 1 {
        return Err(CaptureError::Linked);
    }
    if metadata.dev() != before.dev() || metadata.ino() != before.ino() {
        return Err(CaptureError::Dirty);
    }
    Ok(file)
}

pub(super) fn observe(
    root: &Dir,
    path: &str,
    max: u64,
) -> Result<(CaptureState, Option<Vec<u8>>), CaptureError> {
    let parts = path.split('/').collect::<Vec<_>>();
    let mut directory = root.try_clone().map_err(io_error)?;
    let mut parents = Vec::new();
    for part in &parts[..parts.len() - 1] {
        match directory.symlink_metadata(part) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok((CaptureState::Absent {}, None))
            }
            Err(error) => return Err(io_error(error)),
            Ok(_) => {}
        }
        let child = open_child(&directory, part)?;
        parents.push(directory);
        directory = child;
    }
    let name = parts.last().ok_or(CaptureError::UnsafePath)?;
    match directory.symlink_metadata(name) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            Ok((CaptureState::Absent {}, None))
        }
        Err(error) => Err(io_error(error)),
        Ok(_) => {
            let (bytes, basic_mode) = read_file(&directory, name, max)?;
            let content = BlobReference {
                sha256: checkpoint_digest(&bytes),
                byte_count: bytes.len() as u64,
            };
            Ok((
                CaptureState::File {
                    content,
                    basic_mode,
                },
                Some(bytes),
            ))
        }
    }
}

pub(super) fn names(directory: &Dir, remaining: &mut usize) -> Result<Vec<String>, CaptureError> {
    let mut names = Vec::new();
    for entry in directory.entries().map_err(io_error)? {
        if *remaining == 0 {
            return Err(CaptureError::Budget);
        }
        // Reserve globally before retaining the name. Parent sibling buffers
        // remain charged while child directories are enumerated.
        *remaining -= 1;
        names.push(
            entry
                .map_err(io_error)?
                .file_name()
                .into_string()
                .map_err(|_| CaptureError::Unsupported)?,
        );
    }
    names.sort();
    Ok(names)
}

#[cfg(test)]
pub(super) fn names_in_budget(
    directory: &Dir,
    remaining: &mut usize,
) -> Result<Vec<String>, CaptureError> {
    names(directory, remaining)
}

pub(super) fn collect_paths(
    directory: &Dir,
    prefix: &str,
    policy: &CapturePolicy,
    semantics: PathSemantics,
    paths: &mut Vec<String>,
    omitted: &mut Vec<String>,
    remaining: &mut usize,
) -> Result<(), CaptureError> {
    for name in names(directory, remaining)? {
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        path_key(&path, semantics)?;
        if excluded(&path, policy, semantics) {
            omitted.push(path);
            continue;
        }
        let metadata = directory.symlink_metadata(&name).map_err(io_error)?;
        if linked(&metadata) {
            return Err(CaptureError::Linked);
        }
        if metadata.is_dir() {
            let child = open_child(directory, &name)?;
            collect_paths(&child, &path, policy, semantics, paths, omitted, remaining)?;
        } else if metadata.is_file() {
            if paths.len() >= policy.limits.entries {
                return Err(CaptureError::Budget);
            }
            paths.push(path);
        } else {
            return Err(CaptureError::NonRegular);
        }
    }
    Ok(())
}
