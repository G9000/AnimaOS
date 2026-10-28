//! Owned private fixture roots; no existing or ambient directory ACL is changed.
use std::path::{Path, PathBuf};

pub(crate) struct PrivateTestDirectory {
    _container: tempfile::TempDir,
    path: PathBuf,
}

impl PrivateTestDirectory {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

pub(crate) fn private_directory() -> PrivateTestDirectory {
    let container = tempfile::tempdir().unwrap();
    let path = container.path().join("profile");
    #[cfg(windows)]
    {
        use win_custody::{current_user_sid_string, SecurityDescriptor};
        let user = current_user_sid_string().unwrap();
        SecurityDescriptor::from_sddl(&format!("O:{user}D:P(A;OICI;FA;;;{user})(A;OICI;FA;;;SY)"))
            .unwrap()
            .create_directory(&path)
            .unwrap();
    }
    #[cfg(not(windows))]
    {
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&path).unwrap();
    }
    let held = crate::tools::workspace_bytes::WorkspaceBytes::open(&path).unwrap();
    super::verify_private_directory(&path, &held, false).unwrap();
    PrivateTestDirectory {
        _container: container,
        path,
    }
}
