use std::collections::VecDeque;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

use cap_fs_ext::DirExt;
use cap_std::fs::{Dir, Metadata};
use serde::Serialize;
use utoipa::ToSchema;

use super::ApiError;
use crate::app::SharedDaemonState;

const MAX_FILES: usize = 200;
const MAX_ENTRIES: usize = 2_000;
const MAX_DEPTH: usize = 8;
const MAX_PREVIEW_BYTES: usize = 128 * 1024;

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct WorkspaceFileEntry {
    path: String,
    name: String,
    size_bytes: u64,
    modified_at_ms: Option<u64>,
}

#[derive(Serialize, ToSchema)]
pub(super) struct WorkspaceFilesResponse {
    files: Vec<WorkspaceFileEntry>,
    truncated: bool,
}

#[derive(Serialize, ToSchema)]
pub(super) struct WorkspaceFileResponse {
    path: String,
    content: String,
    truncated: bool,
}

async fn configured_root(state: &SharedDaemonState) -> Result<PathBuf, ApiError> {
    state
        .read()
        .await
        .workspace
        .as_ref()
        .map(|workspace| workspace.root_path.clone())
        .ok_or_else(|| ApiError::conflict("Configure a workspace before browsing files"))
}

pub(super) async fn list(state: &SharedDaemonState) -> Result<WorkspaceFilesResponse, ApiError> {
    let root = configured_root(state).await?;
    tokio::task::spawn_blocking(move || list_files(&root))
        .await
        .map_err(|_| ApiError::service_unavailable("Workspace file listing failed"))?
}

pub(super) async fn preview(
    state: &SharedDaemonState,
    path: String,
) -> Result<WorkspaceFileResponse, ApiError> {
    let root = configured_root(state).await?;
    tokio::task::spawn_blocking(move || preview_file(&root, &path))
        .await
        .map_err(|_| ApiError::service_unavailable("Workspace file preview failed"))?
}

pub(super) async fn download(
    state: &SharedDaemonState,
    path: String,
) -> Result<axum::response::Response, ApiError> {
    use crate::attachments::documents::source_read::read_retained;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    validate_download_path(&path)?;
    let root = tokio::time::timeout_at(deadline, configured_root(state))
        .await
        .map_err(|_| ApiError::service_unavailable("Workspace download timed out"))??;
    let disposition = download_disposition(&path);
    let cancel = anima_core::CancelSignal::new();
    let bytes = read_retained(move || capture_download(&root, &path), &cancel, deadline)
        .await
        .map_err(|_| ApiError::service_unavailable("Workspace download is unavailable"))??;
    if tokio::time::Instant::now() >= deadline {
        return Err(ApiError::service_unavailable(
            "Workspace download timed out",
        ));
    }
    axum::response::Response::builder()
        .header("content-type", "application/octet-stream")
        .header("content-disposition", disposition)
        .header("x-content-type-options", "nosniff")
        .header("content-length", bytes.len().to_string())
        .body(axum::body::Body::from(bytes))
        .map_err(|_| ApiError::service_unavailable("Workspace download is unavailable"))
}

fn validate_download_path(path: &str) -> Result<(), ApiError> {
    if path.is_empty()
        || path.len() > 1024
        || path.contains(['\\', ':'])
        || path.chars().any(char::is_control)
        || path
            .split('/')
            .any(|part| matches!(part, "" | "." | "..") || excluded(part))
    {
        return Err(ApiError::bad_request_static(
            "This workspace path cannot be downloaded",
        ));
    }
    if !path.rsplit_once('.').is_some_and(|(_, extension)| {
        matches!(
            extension.to_ascii_lowercase().as_str(),
            "txt" | "md" | "csv"
        )
    }) {
        return Err(ApiError::bad_request_static(
            "Only TXT, Markdown and CSV reports can be downloaded",
        ));
    }
    Ok(())
}

fn capture_download(root: &Path, relative: &str) -> Result<Vec<u8>, ApiError> {
    use crate::tools::workspace_bytes::{InstructionReadError, WorkspaceBytes};
    let workspace = WorkspaceBytes::open(root).map_err(io_error)?;
    let bytes = workspace
        .read_file_snapshot_with_policy(
            relative,
            crate::attachments::MAX_TEXT_BYTES,
            |name, metadata| !excluded(name) && !hidden(metadata),
        )
        .map_err(|error| match error {
            InstructionReadError::Missing => ApiError::not_found(),
            InstructionReadError::Oversized => {
                ApiError::bad_request_static("Workspace report exceeds the 1 MiB download limit")
            }
            _ => ApiError::bad_request_static("Workspace report is unavailable or excluded"),
        })?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| ApiError::bad_request_static("Only UTF-8 reports can be downloaded"))?;
    if text
        .chars()
        .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    {
        return Err(ApiError::bad_request_static(
            "Binary files cannot be downloaded",
        ));
    }
    if credential_fields(text) {
        return Err(ApiError::bad_request_static(
            "Files containing credential fields cannot be downloaded",
        ));
    }
    Ok(bytes)
}

fn download_disposition(path: &str) -> String {
    use std::fmt::Write;
    let basename = path.rsplit('/').next().expect("validated nonempty path");
    let fallback: String = basename
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_' | ' ') {
                character
            } else {
                '_'
            }
        })
        .collect();
    let mut encoded = String::new();
    for byte in basename.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_') {
            encoded.push(char::from(byte));
        } else {
            write!(&mut encoded, "%{byte:02X}").expect("write to string");
        }
    }
    format!("attachment; filename=\"{fallback}\"; filename*=UTF-8''{encoded}")
}

fn linked(metadata: &Metadata) -> bool {
    crate::tools::workspace_bytes::linked(metadata)
}

fn excluded(name: &str) -> bool {
    let name = name.to_lowercase();
    name.starts_with('.')
        || matches!(
            name.as_str(),
            "node_modules"
                | "target"
                | "dist"
                | "build"
                | "vendor"
                | "control-plane.json"
                | "id_rsa"
                | "id_ed25519"
        )
        || [
            "secret",
            "credential",
            "token",
            "api-key",
            "api_key",
            "private-key",
            "private_key",
        ]
        .iter()
        .any(|part| name.contains(part))
        || [".pem", ".key", ".p12", ".pfx", ".keystore"]
            .iter()
            .any(|extension| name.ends_with(extension))
}

fn hidden(metadata: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use cap_std::fs::MetadataExt;
        metadata.file_attributes() & 0x2 != 0
    }
    #[cfg(not(windows))]
    {
        let _ = metadata;
        false
    }
}

fn io_error(error: std::io::Error) -> ApiError {
    if error.kind() == std::io::ErrorKind::NotFound {
        ApiError::not_found()
    } else {
        ApiError::bad_request_static("Workspace file is unavailable or unreadable")
    }
}

fn open_root(root: &Path) -> Result<Dir, ApiError> {
    let directory = if let (Some(parent), Some(name)) = (root.parent(), root.file_name()) {
        Dir::open_ambient_dir(parent, cap_std::ambient_authority())
            .map_err(io_error)?
            .open_dir_nofollow(name)
            .map_err(io_error)?
    } else {
        Dir::open_ambient_dir(root, cap_std::ambient_authority()).map_err(io_error)?
    };
    let metadata = directory.dir_metadata().map_err(io_error)?;
    if linked(&metadata) || !metadata.is_dir() {
        return Err(ApiError::bad_request_static(
            "Workspace root must be a regular directory",
        ));
    }
    Ok(directory)
}

fn list_files(root: &Path) -> Result<WorkspaceFilesResponse, ApiError> {
    let root = open_root(root)?;
    let mut queue = VecDeque::from([(root, PathBuf::new(), 0)]);
    let mut files = Vec::new();
    let mut visited = 0;
    let mut truncated = false;
    'walk: while let Some((directory, relative_directory, depth)) = queue.pop_front() {
        let entries = match directory.entries() {
            Ok(entries) => entries,
            Err(_) => {
                truncated = true;
                continue;
            }
        };
        let mut bounded = Vec::new();
        for entry in entries {
            if visited == MAX_ENTRIES {
                truncated = true;
                break;
            }
            visited += 1;
            match entry {
                Ok(entry) => bounded.push(entry),
                Err(_) => truncated = true,
            }
        }
        bounded.sort_by_key(|entry| entry.file_name());
        for entry in bounded {
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if excluded(name) {
                continue;
            }
            let path = relative_directory.join(name);
            let Ok(metadata) = directory.symlink_metadata(name) else {
                truncated = true;
                continue;
            };
            if linked(&metadata) || hidden(&metadata) {
                continue;
            }
            if metadata.is_dir() {
                if depth < MAX_DEPTH {
                    match directory.open_dir_nofollow(name) {
                        Ok(child) => queue.push_back((child, path, depth + 1)),
                        Err(_) => truncated = true,
                    }
                } else {
                    truncated = true;
                }
            } else if metadata.is_file() {
                if files.len() == MAX_FILES {
                    truncated = true;
                    break 'walk;
                }
                files.push(WorkspaceFileEntry {
                    path: path.to_string_lossy().replace('\\', "/"),
                    name: name.to_string(),
                    size_bytes: metadata.len(),
                    modified_at_ms: metadata
                        .modified()
                        .ok()
                        .and_then(|time| time.into_std().duration_since(UNIX_EPOCH).ok())
                        .and_then(|duration| u64::try_from(duration.as_millis()).ok()),
                });
            }
        }
        if visited == MAX_ENTRIES {
            truncated |= !queue.is_empty();
            break;
        }
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(WorkspaceFilesResponse { files, truncated })
}

fn preview_file(root: &Path, relative: &str) -> Result<WorkspaceFileResponse, ApiError> {
    if relative.is_empty()
        || relative.contains(['\\', ':'])
        || Path::new(relative)
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(ApiError::bad_request_static(
            "File path must be a relative workspace path without traversal",
        ));
    }
    let mut directory = open_root(root)?;
    let mut components = relative.split('/').peekable();
    let file = loop {
        let component = components.next().expect("nonempty validated path");
        if component.is_empty() || excluded(component) {
            return Err(ApiError::bad_request_static(
                "This file is excluded from workspace browsing",
            ));
        }
        let metadata = directory.symlink_metadata(component).map_err(io_error)?;
        if linked(&metadata) || hidden(&metadata) {
            return Err(ApiError::bad_request_static(
                "Linked files are excluded from workspace browsing",
            ));
        }
        if components.peek().is_some() {
            directory = directory.open_dir_nofollow(component).map_err(io_error)?;
            continue;
        }
        if !metadata.is_file() {
            return Err(ApiError::bad_request_static(
                "Only regular text files can be previewed",
            ));
        }
        break open_regular_file(&directory, component)?;
    };
    let metadata = file.metadata().map_err(io_error)?;
    if !metadata.is_file() || linked(&metadata) {
        return Err(ApiError::bad_request_static(
            "Only regular text files can be previewed",
        ));
    }
    let mut bytes = Vec::new();
    file.take((MAX_PREVIEW_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    let truncated = bytes.len() > MAX_PREVIEW_BYTES;
    bytes.truncate(MAX_PREVIEW_BYTES);
    let text = match std::str::from_utf8(&bytes) {
        Ok(text) => text,
        Err(error) if truncated && error.error_len().is_none() => {
            std::str::from_utf8(&bytes[..error.valid_up_to()]).expect("valid prefix")
        }
        Err(_) => {
            return Err(ApiError::bad_request_static(
                "Only UTF-8 text files can be previewed",
            ))
        }
    };
    if text
        .chars()
        .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    {
        return Err(ApiError::bad_request_static(
            "Binary files cannot be previewed",
        ));
    }
    if credential_fields(text) {
        return Err(ApiError::bad_request_static(
            "Files containing credential fields cannot be previewed",
        ));
    }
    Ok(WorkspaceFileResponse {
        path: relative.to_string(),
        content: text.to_string(),
        truncated,
    })
}

fn credential_fields(text: &str) -> bool {
    let lower = text.to_lowercase();
    let compact: String = lower
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();
    [
        "private key-----",
        "api_key=",
        "api_key:",
        "\"api_key\":",
        "apikey=",
        "\"apikey\":",
        "access_token",
        "refresh_token",
        "client_secret",
        "bearer ",
        "sk-proj-",
        "github_pat_",
        "ghp_",
        "password=",
        "password:",
        "\"password\":",
        "token=",
        "\"token\":",
        "\"secret\":",
    ]
    .iter()
    .any(|marker| lower.contains(marker) || compact.contains(marker))
}

fn open_regular_file(directory: &Dir, name: &str) -> Result<cap_std::fs::File, ApiError> {
    let file =
        crate::tools::workspace_bytes::open_regular_file(directory, name).map_err(io_error)?;
    if hidden(&file.metadata().map_err(io_error)?) {
        return Err(ApiError::bad_request_static(
            "Only regular text files can be previewed",
        ));
    }
    Ok(file)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn final_open_rejects_symlink_and_fifo_replacements() {
        let root =
            std::env::temp_dir().join(format!("workspace-safe-open-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("ordinary.txt"), "safe").unwrap();
        let directory = open_root(&root).unwrap();
        assert!(directory
            .symlink_metadata("ordinary.txt")
            .unwrap()
            .is_file());
        std::fs::remove_file(root.join("ordinary.txt")).unwrap();
        std::os::unix::fs::symlink("/etc/passwd", root.join("ordinary.txt")).unwrap();
        assert!(open_regular_file(&directory, "ordinary.txt").is_err());
        std::fs::remove_file(root.join("ordinary.txt")).unwrap();
        assert!(std::process::Command::new("mkfifo")
            .arg(root.join("ordinary.txt"))
            .status()
            .unwrap()
            .success());
        let start = std::time::Instant::now();
        assert!(open_regular_file(&directory, "ordinary.txt").is_err());
        assert!(start.elapsed() < std::time::Duration::from_secs(1));
        drop(directory);
        std::fs::remove_dir_all(root).unwrap();
    }
}
