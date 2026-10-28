//! Safe local file projections shared by project context and owner previews.

use std::path::Path;

use cap_std::fs::Metadata;
use sha2::{Digest, Sha256};

use crate::tools::{
    generated_documents,
    workspace_bytes::{InstructionReadError, WorkspaceBytes},
};

#[derive(Clone, Debug)]
pub(crate) struct SafeFilePreview {
    pub(crate) path: String,
    pub(crate) content: String,
    pub(crate) truncated: bool,
    pub(crate) capture_sha256: String,
    pub(crate) workspace_key: String,
    pub(crate) read_bytes: usize,
    pub(crate) generated_document: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PreviewError {
    Missing,
    Excluded,
    Oversized,
    Unsupported,
    Unsafe,
    Unavailable,
}

pub(crate) fn workspace_key(root: &Path) -> String {
    hex::encode(Sha256::digest(
        root.as_os_str().to_string_lossy().as_bytes(),
    ))
}

pub(crate) fn hidden(metadata: &Metadata) -> bool {
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

pub(crate) fn capture(
    root: &Path,
    relative: &str,
    max_text_bytes: usize,
) -> Result<SafeFilePreview, PreviewError> {
    if !crate::projects::valid_relative_path(relative) {
        return Err(PreviewError::Excluded);
    }
    let workspace = WorkspaceBytes::open(root).map_err(|_| PreviewError::Unavailable)?;
    let format = generated_documents::format_from_path(relative);
    let generated_document = format.is_some();
    let max = if format.is_some() {
        anima_document_content::MAX_GENERATED_DOCUMENT_BYTES
    } else {
        max_text_bytes
    };
    let bytes = workspace
        .read_file_snapshot_with_policy(relative, max, |name, metadata| {
            !generated_documents::excluded_component(name) && !hidden(metadata)
        })
        .map_err(|error| match error {
            InstructionReadError::Missing => PreviewError::Missing,
            InstructionReadError::Oversized => PreviewError::Oversized,
            InstructionReadError::Linked | InstructionReadError::NonRegular => {
                PreviewError::Excluded
            }
            _ => PreviewError::Unavailable,
        })?;
    let capture_sha256 = hex::encode(Sha256::digest(&bytes));
    let content = if let Some(format) = format {
        let document = anima_document_content::inspect_generated_document(format, &bytes)
            .map_err(|_| PreviewError::Unsupported)?;
        if generated_documents::source_has_credentials(&document) {
            return Err(PreviewError::Unsafe);
        }
        anima_document_content::render_document(&document)
            .map_err(|_| PreviewError::Unsupported)?
            .preview
    } else {
        let text = std::str::from_utf8(&bytes).map_err(|_| PreviewError::Unsupported)?;
        if text
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
        {
            return Err(PreviewError::Unsupported);
        }
        if generated_documents::credential_fields(text) {
            return Err(PreviewError::Unsafe);
        }
        text.to_owned()
    };
    Ok(SafeFilePreview {
        path: relative.into(),
        content,
        truncated: false,
        capture_sha256,
        workspace_key: workspace_key(root),
        read_bytes: bytes.len(),
        generated_document,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_uses_exact_regular_workspace_bytes_and_refuses_secrets_or_escape() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("notes.md"), "selected evidence").unwrap();
        let preview = capture(temp.path(), "notes.md", 64 * 1024).unwrap();
        assert_eq!(preview.content, "selected evidence");
        assert_eq!(preview.read_bytes, 17);
        assert_eq!(
            preview.capture_sha256,
            hex::encode(Sha256::digest(b"selected evidence"))
        );
        assert_eq!(
            capture(temp.path(), "../notes.md", 64 * 1024).unwrap_err(),
            PreviewError::Excluded
        );
        std::fs::write(temp.path().join("report.txt"), "api_key=do-not-share").unwrap();
        assert_eq!(
            capture(temp.path(), "report.txt", 64 * 1024).unwrap_err(),
            PreviewError::Unsafe
        );
        std::fs::write(temp.path().join("secret.txt"), "ordinary text").unwrap();
        assert_eq!(
            capture(temp.path(), "secret.txt", 64 * 1024).unwrap_err(),
            PreviewError::Excluded
        );
        assert_eq!(
            capture(temp.path(), "notes.md", 4).unwrap_err(),
            PreviewError::Oversized
        );
    }
}
