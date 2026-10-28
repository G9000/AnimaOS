//! Keeps private document storage outside the effective workspace.

use std::path::{Path, PathBuf};

struct ResolvedPath {
    path: PathBuf,
    missing_ancestor: Option<PathBuf>,
}

fn resolved_path(path: &Path, directory: bool) -> Result<ResolvedPath, String> {
    let absolute =
        std::path::absolute(path).map_err(|_| "Document workspace boundary path is unavailable")?;
    let mut existing = absolute.as_path();
    let canonical_existing = loop {
        match std::fs::symlink_metadata(existing) {
            Ok(_) => {
                // A dangling link is an existing object, not a missing leaf.
                // Resolve it here so its error cannot be treated as absence.
                let canonical = existing
                    .canonicalize()
                    .map_err(|_| "Document workspace boundary path could not be resolved")?;
                let metadata = std::fs::symlink_metadata(&canonical)
                    .map_err(|_| "Document workspace boundary metadata is unavailable")?;
                if (directory && !metadata.is_dir()) || metadata.file_type().is_symlink() {
                    return Err("Document workspace boundary is not a directory".into());
                }
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    if metadata.file_attributes() & 0x400 != 0 {
                        return Err("Document workspace boundary is a reparse point".into());
                    }
                }
                break canonical;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                existing = existing
                    .parent()
                    .ok_or("Document workspace boundary has no existing ancestor")?;
            }
            Err(_) => return Err("Document workspace boundary metadata is unavailable".into()),
        }
    };
    let path = crate::profile_owner::ProfileOwner::resolve_path(&absolute)
        .map_err(|_| "Document workspace boundary path could not be resolved")?;
    Ok(ResolvedPath {
        path,
        missing_ancestor: (existing != absolute.as_path()).then_some(canonical_existing),
    })
}

fn uncertain_windows_siblings(left: &ResolvedPath, right: &ResolvedPath) -> bool {
    cfg!(windows)
        && left
            .missing_ancestor
            .as_ref()
            .zip(right.missing_ancestor.as_ref())
            .is_some_and(|(left, right)| left == right)
}

/// Resolves configured, environment, or launch-directory workspace paths without creating them.
pub(crate) fn overlaps(
    private_root: &Path,
    configured_workspace: Option<&Path>,
) -> Result<bool, String> {
    let workspace = crate::tools::workspace_root_path("document storage", configured_workspace)?;
    let private_root = resolved_path(private_root, true)?;
    let workspace = resolved_path(&workspace, true)?;
    if private_root.path.starts_with(&workspace.path)
        || workspace.path.starts_with(&private_root.path)
    {
        return Ok(true);
    }
    // Windows may assign case or 8.3 aliases when these missing names are
    // created. Distinct suffix spelling under the same existing directory
    // cannot prove separation until at least one root exists.
    if uncertain_windows_siblings(&private_root, &workspace) {
        return Err("Uncreated Windows document and workspace paths may alias".into());
    }
    Ok(false)
}

/// Keeps the selected local metadata target outside the effective workspace.
/// File validity remains the persistence layer's responsibility.
pub(crate) fn ensure_metadata_separate(
    metadata_file: &Path,
    configured_workspace: Option<&Path>,
) -> Result<(), String> {
    let workspace = crate::tools::workspace_root_path("document storage", configured_workspace)?;
    let metadata_file = resolved_path(metadata_file, false)?;
    let workspace = resolved_path(&workspace, true)?;
    if metadata_file.path.starts_with(&workspace.path) {
        return Err("Document metadata must be outside the workspace".into());
    }
    if uncertain_windows_siblings(&metadata_file, &workspace) {
        return Err("Uncreated Windows metadata and workspace paths may alias".into());
    }
    Ok(())
}

/// Refuses equal, ancestor, and descendant private-storage overlap.
pub(crate) fn ensure_separate(
    private_root: &Path,
    configured_workspace: Option<&Path>,
) -> Result<(), String> {
    if overlaps(private_root, configured_workspace)? {
        return Err("Document staging must be outside the workspace".into());
    }
    Ok(())
}

/// Checks an existing store's custody before admitting a proposed workspace configuration.
pub(crate) fn ensure_state_separate(
    state: &crate::state::DaemonState,
    configured_workspace: Option<&Path>,
) -> Result<(), String> {
    if let Some(root) = state.project_knowledge.cache_root()? {
        ensure_separate(&root, configured_workspace)?;
    }
    if let Some(store) = &state.document_edit_store {
        state
            .ensure_profile_owned()
            .map_err(|_| "Document profile ownership was lost")?;
        ensure_separate(store.canonical_root()?, configured_workspace)?;
        if let Some(metadata_file) = state
            .control_plane_store
            .as_ref()
            .and_then(crate::control_plane_store::ControlPlaneStoreConfig::file_path)
        {
            ensure_metadata_separate(metadata_file, configured_workspace)?;
        }
    }
    Ok(())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::document_edits::store::test_support::private_directory;

    fn not_admitted(decision: Result<bool, String>) {
        assert!(
            !matches!(decision, Ok(false)),
            "unresolved Windows aliases were admitted as separate roots"
        );
    }

    #[test]
    fn workspace_boundary_prospective_windows_case_alias_is_not_admitted() {
        let private = private_directory();
        let root = private.path().join("prospective-document-edits");
        let alias = private.path().join("PROSPECTIVE-DOCUMENT-EDITS");
        assert!(!root.exists() && !alias.exists());
        let decision = overlaps(&root, Some(&alias));
        std::fs::create_dir(&root).unwrap();
        if alias.exists() {
            assert!(root.canonicalize().unwrap() == alias.canonicalize().unwrap());
        } else {
            // Case-sensitive Windows directories still require a proven
            // boundary before either prospective name is created.
            std::fs::create_dir(&alias).unwrap();
            assert!(root.canonicalize().unwrap() != alias.canonicalize().unwrap());
        }
        not_admitted(decision);
    }

    #[test]
    fn workspace_boundary_potential_short_style_aliases_are_not_admitted() {
        let private = private_directory();
        let root = private
            .path()
            .join("prospective-document-edits-with-long-name");
        let alias = private.path().join("PROSPE~1");
        assert!(!root.exists() && !alias.exists());
        assert!(std::fs::read_dir(private.path()).unwrap().next().is_none());
        // This is a potential short-style spelling, not discovery of an
        // assigned 8.3 name. Until either sibling exists, distinct spelling
        // cannot prove that future Windows roots will remain separate.
        let decision = overlaps(&root, Some(&alias));
        not_admitted(decision);
        assert!(!root.exists() && !alias.exists());
        assert!(std::fs::read_dir(private.path()).unwrap().next().is_none());
    }

    #[test]
    fn workspace_boundary_existing_store_allows_separate_prospective_workspace() {
        let private = private_directory();
        let root = private.path().join("document-edits");
        std::fs::create_dir(&root).unwrap();
        let workspace = private.path().join("new-workspace/nested");
        assert!(!workspace.exists());
        assert!(!overlaps(&root, Some(&workspace)).unwrap());
        assert!(!workspace.exists());
        std::fs::create_dir_all(&workspace).unwrap();
        assert!(!overlaps(&root, Some(&workspace)).unwrap());
    }

    #[test]
    fn workspace_boundary_separate_missing_windows_branches_remain_supported() {
        let private = private_directory();
        let data = private.path().join("data");
        std::fs::create_dir(&data).unwrap();
        let root = data.join("document-edits");
        let workspace = private.path().join("missing-workspace");
        assert!(!root.exists() && !workspace.exists());
        assert!(!overlaps(&root, Some(&workspace)).unwrap());
        assert!(!root.exists() && !workspace.exists());
    }
}
