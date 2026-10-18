//! Confines agent file requests to the session root.
//!
//! The returned path is fully resolved, and callers read or write exactly that
//! path, so what was checked is what is touched. The agent's own shell can
//! still race a symlink into place between check and use; that shell already
//! has the same reach, so this guard is about the client not widening it.
use std::{
    io,
    path::{Component, Path, PathBuf},
};

/// Resolves `requested` (absolute, or relative to `root`) and returns it only
/// if it stays inside `root`, which must already be canonical. Components
/// that do not exist yet are allowed, so writes can create files.
pub(crate) fn confine(root: &Path, requested: &Path) -> Result<PathBuf, String> {
    let joined = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        root.join(requested)
    };
    let lexical = normalise(&joined);
    let mut existing = lexical.as_path();
    let mut tail = Vec::new();
    let resolved = loop {
        match std::fs::canonicalize(existing) {
            Ok(path) => break path,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                // A dangling symlink resolves nowhere; writing through it would
                // land wherever it points.
                if std::fs::symlink_metadata(existing).is_ok() {
                    return Err(format!("{} is a dangling symlink", existing.display()));
                }
                let (Some(parent), Some(name)) = (existing.parent(), existing.file_name()) else {
                    return Err(format!("{} does not exist", joined.display()));
                };
                tail.push(name.to_owned());
                existing = parent;
            }
            Err(error) => return Err(format!("{}: {error}", existing.display())),
        }
    };
    let resolved = tail
        .iter()
        .rev()
        .fold(resolved, |path, name| path.join(name));
    if resolved.starts_with(root) {
        Ok(resolved)
    } else {
        Err(format!(
            "{} is outside the session directory",
            requested.display()
        ))
    }
}

/// Removes `.` and folds `..` without touching the file system.
fn normalise(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Applies ACP's 1-based `line` and `limit` to file text.
pub(crate) fn slice_lines(content: String, line: Option<u64>, limit: Option<u64>) -> String {
    if line.is_none() && limit.is_none() {
        return content;
    }
    let skip = line.unwrap_or(1).saturating_sub(1) as usize;
    let take = limit.map_or(usize::MAX, |n| n as usize);
    content
        .split_inclusive('\n')
        .skip(skip)
        .take(take)
        .collect()
}
