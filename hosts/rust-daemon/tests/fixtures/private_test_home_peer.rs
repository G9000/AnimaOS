//! SDK fixture custody operations. Never changes an existing directory's ACL.

#[cfg(windows)]
mod windows {
    use std::io;
    use std::path::{Component, Path, PathBuf};
    use std::time::{Duration, Instant};

    use cap_fs_ext::{DirExt, MetadataExt as IdentityExt};
    use cap_std::fs::{Dir, Metadata, MetadataExt};
    use win_custody::{
        current_user_sid_string, ObjectSecurityDescriptor, OwnedSid, SecurityDescriptor,
    };

    const MAX_ENTRIES: usize = 10_000;
    const MAX_DEPTH: usize = 32;

    fn invalid(message: &str) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidInput, message)
    }

    fn ordinary(metadata: &Metadata) -> io::Result<()> {
        if metadata.file_type().is_symlink() || metadata.file_attributes() & 0x400 != 0 {
            return Err(invalid("fixture path is a reparse point"));
        }
        Ok(())
    }

    // Match the daemon's held directory custody: inspect and open every path
    // component without following links, including the final fixture root.
    fn open_directory(path: &Path) -> io::Result<Dir> {
        let mut anchor = PathBuf::new();
        let mut names = Vec::new();
        for component in path.components() {
            match component {
                Component::Prefix(_) | Component::RootDir => anchor.push(component.as_os_str()),
                Component::Normal(name) => names.push(name),
                _ => return Err(invalid("fixture path must be absolute without traversal")),
            }
        }
        if !anchor.is_absolute() || names.is_empty() {
            return Err(invalid("fixture directory must be below an absolute root"));
        }
        let mut directory = Dir::open_ambient_dir(anchor, cap_std::ambient_authority())?;
        for name in names {
            let metadata = directory.symlink_metadata(name)?;
            ordinary(&metadata)?;
            if !metadata.is_dir() {
                return Err(invalid("fixture parent is not a directory"));
            }
            directory = directory.open_dir_nofollow(name)?;
            ordinary(&directory.dir_metadata()?)?;
        }
        Ok(directory)
    }

    fn verify_private(directory: &Dir, user_sid: &str) -> io::Result<()> {
        let file = directory.try_clone()?.into_std_file();
        let descriptor = ObjectSecurityDescriptor::query_file_handle(&file)?;
        let user = OwnedSid::from_string(user_sid)?;
        let system = OwnedSid::from_string("S-1-5-18")?;
        let owner = descriptor.owner().map_err(io::Error::other)?;
        if !owner.matches(&user) || descriptor.control()? & 0x1000 == 0 {
            return Err(invalid(
                "fixture home owner or DACL protection is incorrect",
            ));
        }
        let dacl = descriptor.dacl().map_err(io::Error::other)?;
        if dacl.len() != 2 {
            return Err(invalid("fixture home must have exactly two access rules"));
        }
        let mut user_rules = 0;
        let mut system_rules = 0;
        for ace in dacl.allowed_aces() {
            // Exactly explicit allow entries, inherited by both files and
            // directories, with full control and no propagation exceptions.
            if ace.flags() != 0x03 || ace.mask() != 0x001f_01ff {
                return Err(invalid("fixture home access rule is incorrect"));
            }
            if ace.trustee_matches(&user) {
                user_rules += 1;
            } else if ace.trustee_matches(&system) {
                system_rules += 1;
            } else {
                return Err(invalid("fixture home has an unexpected trustee"));
            }
        }
        if user_rules != 1 || system_rules != 1 {
            return Err(invalid("fixture home trustee counts are incorrect"));
        }
        Ok(())
    }

    fn inspect_tree(
        directory: &Dir,
        depth: usize,
        visited: &mut usize,
        deadline: Instant,
    ) -> io::Result<()> {
        ordinary(&directory.dir_metadata()?)?;
        if depth > MAX_DEPTH || Instant::now() >= deadline {
            return Err(invalid("fixture cleanup inspection exceeded its bounds"));
        }
        for entry in directory.entries()? {
            *visited += 1;
            if *visited > MAX_ENTRIES || Instant::now() >= deadline {
                return Err(invalid("fixture cleanup inspection exceeded its bounds"));
            }
            let name = entry?.file_name();
            let metadata = directory.symlink_metadata(&name)?;
            ordinary(&metadata)?;
            if metadata.is_dir() {
                let child = directory.open_dir_nofollow(name)?;
                inspect_tree(&child, depth + 1, visited, deadline)?;
            } else if !metadata.is_file() {
                return Err(invalid("fixture cleanup entry is not an ordinary file"));
            }
        }
        Ok(())
    }

    fn operate(operation: &str, root: &Path, home: &Path) -> io::Result<()> {
        if !root.is_absolute() || home != root.join("home") {
            return Err(invalid(
                "fixture home must be the owned root's direct home child",
            ));
        }
        let held = open_directory(root)?;
        let identity = held.dir_metadata()?;
        match operation {
            "create" => {
                if held.entries()?.next().is_some() {
                    return Err(invalid(
                        "fixture creation requires a fresh empty owned root",
                    ));
                }
                let user = current_user_sid_string()?;
                SecurityDescriptor::from_sddl(&format!(
                    "O:{user}D:P(A;OICI;FA;;;{user})(A;OICI;FA;;;SY)"
                ))?
                .create_directory(home)?;
                let created = held.open_dir_nofollow("home")?;
                ordinary(&created.dir_metadata()?)?;
                verify_private(&created, &user)?;
            }
            "inspect" => {
                inspect_tree(&held, 0, &mut 0, Instant::now() + Duration::from_secs(5))?;
            }
            _ => return Err(invalid("unknown fixture custody operation")),
        }
        let current = open_directory(root)?.dir_metadata()?;
        if (current.dev(), current.ino()) != (identity.dev(), identity.ino()) {
            return Err(invalid("fixture root changed during custody operation"));
        }
        Ok(())
    }

    pub(super) fn run() -> io::Result<()> {
        let args: Vec<_> = std::env::args_os().skip(1).collect();
        if args.len() != 3 {
            return Err(invalid("expected create|inspect ROOT HOME"));
        }
        let operation = args[0]
            .to_str()
            .ok_or_else(|| invalid("fixture operation is not UTF-8"))?;
        operate(operation, Path::new(&args[1]), Path::new(&args[2]))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn creates_verified_private_home_and_never_modifies_an_existing_home() {
            let root = tempfile::tempdir().expect("owned fixture root");
            let home = root.path().join("home");
            operate("create", root.path(), &home).expect("private home creation");
            let file = home.join("evidence.txt");
            std::fs::write(&file, b"preserve existing bytes").expect("fixture evidence");
            assert!(operate("create", root.path(), &home).is_err());
            verify_private(
                &open_directory(&home).expect("held home"),
                &current_user_sid_string().expect("user SID"),
            )
            .expect("original private ACL");
            assert_eq!(
                std::fs::read(file).expect("unchanged evidence"),
                b"preserve existing bytes"
            );
            operate("inspect", root.path(), &home).expect("bounded cleanup inspection");
        }

        #[test]
        fn rejects_home_outside_the_direct_child() {
            let root = tempfile::tempdir().expect("owned fixture root");
            assert!(operate("create", root.path(), &root.path().join("other")).is_err());
            assert!(root
                .path()
                .read_dir()
                .expect("fixture entries")
                .next()
                .is_none());
        }
    }
}

#[cfg(windows)]
fn main() -> std::io::Result<()> {
    windows::run()
}

#[cfg(not(windows))]
fn main() -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "private home peer is only used on Windows",
    ))
}
