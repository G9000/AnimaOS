use super::workspace::*;
use super::*;
use std::path::Path;

/// The host selects the parent. Provision only a fresh empty child; never reset
/// an existing root's ACL. Stable named Windows queries are protected by all
/// retained ancestor/root directory handles, which deny delete sharing.
pub(super) fn verify_private_root(
    path: &Path,
    held: &HeldRoot,
    parent: bool,
) -> Result<(), CaptureError> {
    let identity = held.identity()?;
    #[cfg(windows)]
    {
        use win_custody::{current_user_sid_string, ObjectSecurityDescriptor, OwnedSid};
        let descriptor =
            ObjectSecurityDescriptor::query_file(path).map_err(|_| CaptureError::InsecureStore)?;
        let user = OwnedSid::from_string(
            &current_user_sid_string().map_err(|_| CaptureError::InsecureStore)?,
        )
        .map_err(|_| CaptureError::InsecureStore)?;
        let system = OwnedSid::from_string("S-1-5-18").map_err(|_| CaptureError::InsecureStore)?;
        let admins =
            OwnedSid::from_string("S-1-5-32-544").map_err(|_| CaptureError::InsecureStore)?;
        let creator = OwnedSid::from_string("S-1-3-0").map_err(|_| CaptureError::InsecureStore)?;
        let dacl = descriptor.dacl().map_err(|_| CaptureError::InsecureStore)?;
        let mut user_control = false;
        for ace in dacl.allowed_aces() {
            if ace.trustee_matches(&user) {
                if ace.flags() & 0x08 == 0 && ace.mask() & 0x001f01ff == 0x001f01ff {
                    user_control = true;
                }
            } else if !ace.trustee_matches(&system)
                && !(parent && ace.trustee_matches(&admins))
                && !(parent && ace.flags() & 0x08 != 0 && ace.trustee_matches(&creator))
            {
                return Err(CaptureError::InsecureStore);
            }
        }
        if !user_control
            || (!parent
                && descriptor
                    .control()
                    .map_err(|_| CaptureError::InsecureStore)?
                    & 0x1000
                    == 0)
        {
            return Err(CaptureError::InsecureStore);
        }
    }
    #[cfg(unix)]
    {
        use cap_std::fs::PermissionsExt;
        let _ = (path, parent);
        if held
            .directory
            .dir_metadata()
            .map_err(io_error)?
            .permissions()
            .mode()
            & 0o077
            != 0
        {
            return Err(CaptureError::InsecureStore);
        }
    }
    if held.identity()? != identity {
        return Err(CaptureError::InsecureStore);
    }
    Ok(())
}

impl CheckpointStore {
    pub(super) fn provision_private_root(config: CaptureConfig) -> Result<Self, CaptureError> {
        let store = Self::new(config);
        store.validate_config()?;
        let workspace = HeldRoot::open(&store.config.workspace_root)?;
        if workspace.identity()? != store.config.scope.root_identity_digest {
            return Err(CaptureError::InvalidScope);
        }
        let path = &store.config.store_root;
        let parent_path = path.parent().ok_or(CaptureError::UnsafePath)?;
        let name = path.file_name().ok_or(CaptureError::UnsafePath)?;
        let parent = HeldRoot::open(parent_path)?;
        verify_private_root(parent_path, &parent, true)?;
        let canonical_workspace = store
            .config
            .workspace_root
            .canonicalize()
            .map_err(io_error)?;
        let candidate = parent_path.canonicalize().map_err(io_error)?.join(name);
        if candidate.starts_with(&canonical_workspace)
            || canonical_workspace.starts_with(&candidate)
        {
            return Err(CaptureError::InvalidScope);
        }
        let created = {
            #[cfg(not(unix))]
            let builder = cap_std::fs::DirBuilder::new();
            #[cfg(unix)]
            let mut builder = cap_std::fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use cap_std::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match parent.directory.create_dir_with(name, &builder) {
                Ok(()) => true,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => false,
                Err(error) => return Err(io_error(error)),
            }
        };
        let held = HeldRoot::open(path)?;
        if created {
            #[cfg(windows)]
            {
                use win_custody::{current_user_sid_string, SecurityDescriptor};
                let sid = current_user_sid_string().map_err(|_| CaptureError::InsecureStore)?;
                SecurityDescriptor::from_sddl(&format!("D:P(A;OICI;FA;;;{sid})(A;OICI;FA;;;SY)"))
                    .map_err(|_| CaptureError::InsecureStore)?
                    .apply_to_tree(path, true)
                    .map_err(|_| CaptureError::InsecureStore)?;
            }
            #[cfg(unix)]
            parent
                .directory
                .try_clone()
                .map_err(io_error)?
                .into_std_file()
                .sync_all()
                .map_err(io_error)?;
        }
        verify_private_root(path, &held, false)?;
        Ok(store)
    }
}
