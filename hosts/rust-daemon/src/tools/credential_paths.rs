//! Credential files never become ordinary model-readable workspace content.
use std::path::{Component, Path};

pub(super) const REFUSED: &str = "Credential files cannot be accessed through agent file tools";

pub(super) fn sensitive(path: &Path) -> bool {
    path.components().any(|part| {
        let Component::Normal(name) = part else {
            return false;
        };
        let name = name.to_string_lossy().to_ascii_lowercase();
        name == ".env"
            || name.starts_with(".env.")
            || matches!(
                name.as_str(),
                ".envrc"
                    | ".netrc"
                    | "_netrc"
                    | ".npmrc"
                    | ".pypirc"
                    | ".git-credentials"
                    | ".ssh"
                    | ".aws"
                    | ".azure"
                    | ".kube"
                    | "auth.json"
                    | "credentials.json"
                    | "admin-token"
            )
            || name.ends_with(".admin-token")
    })
}

pub(super) fn check_path(root: &Path, input: &str) -> Result<(), String> {
    let path = Path::new(input);
    if sensitive(path) {
        return Err(REFUSED.into());
    }
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    // Check resolved names as well: an innocently named symlink must not expose
    // an existing credential file. Ordinary workspace capability checks follow.
    let mut ancestor = Some(path.as_path());
    while let Some(candidate) = ancestor {
        if let Ok(resolved) = std::fs::canonicalize(candidate) {
            if sensitive(&resolved) {
                return Err(REFUSED.into());
            }
            break;
        }
        ancestor = candidate.parent();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn credential_paths_recursive_grep_never_reads_opaque_credentials() {
        let root = tempfile::tempdir().unwrap();
        for name in [
            "auth.json",
            "credentials.json",
            "control-plane.json.admin-token",
        ] {
            std::fs::write(root.path().join(name), "opaque-fixture-credential").unwrap();
        }
        std::fs::write(root.path().join("notes.txt"), "public-fixture-value").unwrap();
        let output = super::super::filesystem::search::grep_workspace_files_from_root(
            root.path(),
            "fixture",
            ".",
            None,
        )
        .unwrap();
        assert!(!output.contains("opaque-fixture-credential"));
        assert!(!output.contains("auth.json"));
        assert!(output.contains("public-fixture-value"));
    }
    #[cfg(unix)]
    #[test]
    fn credential_paths_alias_to_secret_directory_refuses_missing_leaf() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join(".aws")).unwrap();
        std::os::unix::fs::symlink(root.path().join(".aws"), root.path().join("cloud")).unwrap();
        assert_eq!(
            check_path(root.path(), "cloud/new-config"),
            Err(REFUSED.into())
        );
        assert!(!root.path().join(".aws/new-config").exists());
    }
    #[test]
    fn credential_paths_refuse_secret_files_and_directories() {
        for input in [
            ".env",
            "nested/.env.production",
            ".SSH/id_rsa",
            "x/auth.json",
            "x/control-plane.json.admin-token",
            ".aws/config",
        ] {
            assert!(sensitive(Path::new(input)), "{input}");
        }
        for input in [
            "environment.rs",
            "notes.md",
            "fixtures/auth.example.json",
            "token-count.txt",
        ] {
            assert!(!sensitive(Path::new(input)), "{input}");
        }
    }
    #[test]
    fn credential_paths_check_before_missing_file_or_write() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(check_path(root.path(), ".env.local"), Err(REFUSED.into()));
        assert!(!root.path().join(".env.local").exists());
        assert!(check_path(root.path(), "notes.txt").is_ok());
    }
    #[test]
    fn credential_paths_block_real_file_reads_and_writes_without_effects() {
        let root = tempfile::tempdir().unwrap();
        let secret = root.path().join(".env");
        std::fs::write(&secret, "API_KEY=synthetic-local-secret").unwrap();
        let read = super::super::filesystem::search::read_workspace_file_from_root(
            root.path(),
            ".env",
            0,
            100,
        );
        assert_eq!(read, Err(REFUSED.into()));
        let write = super::super::filesystem::edit::write_workspace_file_from_root(
            root.path(),
            ".env",
            "replaced",
        );
        assert_eq!(write, Err(REFUSED.into()));
        assert_eq!(
            std::fs::read_to_string(secret).unwrap(),
            "API_KEY=synthetic-local-secret"
        );
    }
}
