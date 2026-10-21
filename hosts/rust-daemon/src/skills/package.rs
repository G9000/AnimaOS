//! Capability-relative package I/O. Text is never interpreted or executed.
use super::{
    MAX_SKILL_FILE_BYTES, SKILLS_FOLDER, SKILL_FILE_NAME, SKILL_FILE_NOT_REGULAR,
    SKILL_FILE_OUTSIDE, SKILL_FILE_TOO_LARGE,
};
use crate::tools::workspace_bytes::{linked, open_regular_file, WorkspaceBytes};
use anima_core::skill_packages::{
    validate_skill_package, validate_supporting_path, SkillSupportingFile,
    MAX_SKILL_SUPPORTING_FILES, MAX_SKILL_SUPPORTING_FILE_BYTES,
};
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt, OpenOptionsSyncExt};
use cap_std::fs::{Dir, OpenOptions};
use std::{
    io::{self, Read, Write},
    path::Path,
};

#[derive(Clone, Debug)]
pub(crate) struct SkillPackage {
    pub(crate) bytes: Vec<u8>,
    pub(crate) files: Vec<SkillSupportingFile>,
}
impl SkillPackage {
    pub(crate) fn hash(&self) -> String {
        anima_core::skill_packages::skill_package_hash(&self.bytes, &self.files)
    }
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
fn child(parent: &Dir, name: &str, create: bool) -> io::Result<Dir> {
    match parent.symlink_metadata(name) {
        Ok(metadata) if linked(&metadata) => return Err(invalid(SKILL_FILE_OUTSIDE)),
        Ok(metadata) if !metadata.is_dir() => return Err(invalid(SKILL_FILE_NOT_REGULAR)),
        Ok(_) => {}
        Err(error) if create && error.kind() == io::ErrorKind::NotFound => {
            match parent.create_dir(name) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e),
            }
        }
        Err(error) => return Err(error),
    }
    let opened = parent.open_dir_nofollow(name)?;
    let metadata = opened.dir_metadata()?;
    if linked(&metadata) {
        return Err(invalid(SKILL_FILE_OUTSIDE));
    }
    if !metadata.is_dir() {
        return Err(invalid(SKILL_FILE_NOT_REGULAR));
    }
    Ok(opened)
}
fn bytes(directory: &Dir, name: &str, max: usize) -> Result<Vec<u8>, String> {
    let metadata = directory
        .symlink_metadata(name)
        .map_err(|e| e.to_string())?;
    if linked(&metadata) {
        return Err(SKILL_FILE_OUTSIDE.into());
    }
    if !metadata.is_file() {
        return Err(SKILL_FILE_NOT_REGULAR.into());
    }
    let file = open_regular_file(directory, name).map_err(|e| e.to_string())?;
    let before = file.metadata().map_err(|e| e.to_string())?;
    let oversized = || {
        if name == SKILL_FILE_NAME {
            SKILL_FILE_TOO_LARGE
        } else {
            "Supporting file must be at most 64 KiB"
        }
        .to_string()
    };
    if before.len() > max as u64 {
        return Err(oversized());
    }
    let mut content = Vec::new();
    (&file)
        .take(max as u64 + 1)
        .read_to_end(&mut content)
        .map_err(|e| e.to_string())?;
    if content.len() > max {
        return Err(oversized());
    }
    let after = file.metadata().map_err(|e| e.to_string())?;
    if linked(&after)
        || !after.is_file()
        || after.len() != before.len()
        || content.len() as u64 != after.len()
        || before.modified().ok() != after.modified().ok()
    {
        return Err("Skill package file changed during its read".into());
    }
    Ok(content)
}

pub(crate) fn read_package(workspace: &Path, slug: &str) -> Result<Option<SkillPackage>, String> {
    if !super::is_valid_slug(slug) {
        return Err(super::SKILL_SLUG_INVALID.into());
    }
    let held = WorkspaceBytes::open(workspace).map_err(|e| e.to_string())?;
    read_held_package(&held, slug)
}

/// A partial, unregistered folder without SKILL.md is still owner content;
/// an implicit supporting-file save must not silently replace it.
pub(crate) fn folder_has_content(workspace: &Path, slug: &str) -> Result<bool, String> {
    if !super::is_valid_slug(slug) {
        return Err(super::SKILL_SLUG_INVALID.into());
    }
    let held = WorkspaceBytes::open(workspace).map_err(|e| e.to_string())?;
    let directory = held.held_directory().map_err(|e| e.to_string())?;
    let folder = match child(&directory, SKILLS_FOLDER, false)
        .and_then(|skills| child(&skills, slug, false))
    {
        Ok(folder) => folder,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e.to_string()),
    };
    folder
        .entries()
        .map_err(|e| e.to_string())?
        .next()
        .transpose()
        .map(|entry| entry.is_some())
        .map_err(|e| e.to_string())
}
fn read_held_package(held: &WorkspaceBytes, slug: &str) -> Result<Option<SkillPackage>, String> {
    let directory = held.held_directory().map_err(|e| e.to_string())?;
    let folder = match child(&directory, SKILLS_FOLDER, false)
        .and_then(|skills| child(&skills, slug, false))
    {
        Ok(folder) => folder,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    match folder.symlink_metadata(SKILL_FILE_NAME) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
        Ok(_) => {}
    }
    let bytes = bytes(&folder, SKILL_FILE_NAME, MAX_SKILL_FILE_BYTES)?;
    let mut files = Vec::new();
    walk(&folder, "", &mut files, &mut 0)?;
    files.sort_by(|a, b| a.path.cmp(&b.path));
    validate_skill_package(&bytes, &files).map_err(str::to_string)?;
    Ok(Some(SkillPackage { bytes, files }))
}
fn walk(
    directory: &Dir,
    prefix: &str,
    files: &mut Vec<SkillSupportingFile>,
    examined: &mut usize,
) -> Result<(), String> {
    for entry in directory.entries().map_err(|e| e.to_string())? {
        *examined += 1;
        if *examined > 256 {
            return Err("Skill package contains too many filesystem entries".into());
        }
        let name = entry
            .map_err(|e| e.to_string())?
            .file_name()
            .into_string()
            .map_err(|_| "Skill package path is not UTF-8")?;
        let relative = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        let metadata = directory
            .symlink_metadata(&name)
            .map_err(|e| e.to_string())?;
        if linked(&metadata) {
            return Err(SKILL_FILE_OUTSIDE.into());
        }
        if relative == SKILL_FILE_NAME {
            if !metadata.is_file() {
                return Err(SKILL_FILE_NOT_REGULAR.into());
            }
            continue;
        }
        if metadata.is_dir() {
            validate_supporting_path(&format!("{relative}/probe.txt")).map_err(str::to_string)?;
            let opened = child(directory, &name, false).map_err(|e| e.to_string())?;
            walk(&opened, &relative, files, examined)?;
        } else {
            validate_supporting_path(&relative).map_err(str::to_string)?;
            if !metadata.is_file() {
                return Err("Supporting files must be regular files".into());
            }
            if files.len() >= MAX_SKILL_SUPPORTING_FILES {
                return Err("Skill package supports at most 32 supporting files".into());
            }
            let content = bytes(directory, &name, MAX_SKILL_SUPPORTING_FILE_BYTES)?;
            files.push(SkillSupportingFile {
                path: relative,
                content: String::from_utf8(content)
                    .map_err(|_| "Supporting file is not UTF-8 text")?,
            });
        }
    }
    Ok(())
}

/// Stage a complete package through held nofollow capabilities and swap folders.
/// Previous folders remain in transaction backups. A failed install restores them.
pub(crate) fn write_package(
    workspace: &Path,
    slug: &str,
    package: &SkillPackage,
) -> Result<(), String> {
    if !super::is_valid_slug(slug) {
        return Err(super::SKILL_SLUG_INVALID.into());
    }
    validate_skill_package(&package.bytes, &package.files).map_err(str::to_string)?;
    let held = WorkspaceBytes::open(workspace).map_err(|e| e.to_string())?;
    write_held_package(&held, slug, package).map_err(|e| e.to_string())
}
fn write_held_package(held: &WorkspaceBytes, slug: &str, package: &SkillPackage) -> io::Result<()> {
    let directory = held.held_directory()?;
    let skills = child(&directory, SKILLS_FOLDER, true)?;
    let existing = match child(&skills, slug, false) {
        Ok(folder) => Some(folder),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };
    if let Some(folder) = existing.as_ref() {
        check_tree(folder, &mut 0)?;
    }
    let transactions = child(&directory, ".anima-skill-transactions", true)?;
    sync_dir(&directory)?;
    let id = uuid::Uuid::new_v4();
    let pending = format!("{slug}-{id}-pending");
    let backup = format!("{slug}-{id}-previous");
    transactions.create_dir(&pending)?;
    let stage = child(&transactions, &pending, false)?;
    write_new(&stage, SKILL_FILE_NAME, &package.bytes)?;
    for file in &package.files {
        let parts = file.path.split('/').collect::<Vec<_>>();
        let mut parent = stage.try_clone()?;
        for part in &parts[..parts.len() - 1] {
            parent = child(&parent, part, true)?;
        }
        write_new(&parent, parts[parts.len() - 1], file.content.as_bytes())?;
        sync_dir(&parent)?;
    }
    sync_tree(&stage, &mut 0)?;
    sync_dir(&transactions)?;
    // Closing directory handles before rename also supports Windows sharing rules.
    drop(stage);
    drop(existing);
    let existed = match skills.symlink_metadata(slug) {
        Ok(metadata) => {
            if linked(&metadata) || !metadata.is_dir() {
                return Err(invalid(SKILL_FILE_OUTSIDE));
            }
            true
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => false,
        Err(e) => return Err(e),
    };
    if existed {
        skills.rename(slug, &transactions, &backup)?;
    }
    let mut installed = false;
    let outcome = (|| {
        sync_dir(&skills)?;
        sync_dir(&transactions)?;
        let staged = transactions.symlink_metadata(&pending)?;
        if linked(&staged) || !staged.is_dir() {
            return Err(invalid(SKILL_FILE_OUTSIDE));
        }
        transactions.rename(&pending, &skills, slug)?;
        installed = true;
        sync_dir(&skills)?;
        sync_dir(&transactions)
    })();
    if let Err(error) = outcome {
        let rollback = (|| {
            if installed {
                skills.rename(slug, &transactions, &pending)?;
            }
            if existed {
                if skills.symlink_metadata(slug).is_ok() {
                    return Err(invalid("Skill package target changed during rollback"));
                }
                transactions.rename(&backup, &skills, slug)?;
            }
            sync_dir(&skills)?;
            sync_dir(&transactions)
        })();
        if let Err(rollback) = rollback {
            return Err(io::Error::new(
                rollback.kind(),
                format!("Skill package install failed: {error}; rollback failed: {rollback}"),
            ));
        }
        return Err(error);
    }
    Ok(())
}
fn write_new(directory: &Dir, name: &str, content: &[u8]) -> io::Result<()> {
    let mut options = OpenOptions::new();
    options
        .write(true)
        .create_new(true)
        .follow(FollowSymlinks::No)
        .nonblock(true);
    let mut file = directory.open_with(name, &options)?;
    let metadata = file.metadata()?;
    if linked(&metadata) || !metadata.is_file() {
        return Err(invalid(SKILL_FILE_NOT_REGULAR));
    }
    file.write_all(content)?;
    file.sync_all()
}
fn check_tree(directory: &Dir, examined: &mut usize) -> io::Result<()> {
    for entry in directory.entries()? {
        *examined += 1;
        if *examined > 256 {
            return Err(invalid(
                "Skill package contains too many filesystem entries",
            ));
        }
        let name = entry?.file_name();
        let metadata = directory.symlink_metadata(&name)?;
        if linked(&metadata) {
            return Err(invalid(SKILL_FILE_OUTSIDE));
        }
        if metadata.is_dir() {
            let name = name
                .to_str()
                .ok_or_else(|| invalid("Skill package path is not UTF-8"))?;
            check_tree(&child(directory, name, false)?, examined)?;
        } else if !metadata.is_file() {
            return Err(invalid(SKILL_FILE_NOT_REGULAR));
        }
    }
    Ok(())
}
fn sync_tree(directory: &Dir, examined: &mut usize) -> io::Result<()> {
    for entry in directory.entries()? {
        *examined += 1;
        if *examined > 256 {
            return Err(invalid(
                "Skill package contains too many filesystem entries",
            ));
        }
        let name = entry?.file_name();
        let metadata = directory.symlink_metadata(&name)?;
        if linked(&metadata) {
            return Err(invalid(SKILL_FILE_OUTSIDE));
        }
        if metadata.is_dir() {
            let name = name
                .to_str()
                .ok_or_else(|| invalid("Skill package path is not UTF-8"))?;
            sync_tree(&child(directory, name, false)?, examined)?;
        }
    }
    sync_dir(directory)
}
fn sync_dir(directory: &Dir) -> io::Result<()> {
    #[cfg(unix)]
    {
        directory.try_clone()?.into_std_file().sync_all()
    }
    #[cfg(not(unix))]
    {
        let _ = directory;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::test_support::{skill_text, temp_workspace, write_skill};
    use std::fs;
    #[cfg(windows)]
    fn directory_link(link: &Path, target: &Path) -> bool {
        std::process::Command::new("cmd.exe")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .is_ok_and(|o| o.status.success())
    }
    #[cfg(unix)]
    fn directory_link(link: &Path, target: &Path) -> bool {
        std::os::unix::fs::symlink(target, link).is_ok()
    }
    #[test]
    fn package_links_are_refused_on_read_and_replace_even_inside_workspace() {
        let root = temp_workspace("package-links");
        write_skill(&root, "notes", &skill_text("notes"));
        let target = root.join("other");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("guide.md"), "Unapproved guide").unwrap();
        if !directory_link(&root.join("skills/notes/references"), &target) {
            return;
        }
        assert!(read_package(&root, "notes").is_err());
        let new = SkillPackage {
            bytes: skill_text("new").into_bytes(),
            files: vec![SkillSupportingFile {
                path: "templates/note.md".into(),
                content: "new".into(),
            }],
        };
        assert!(write_package(&root, "notes", &new).is_err());
        assert_eq!(
            fs::read_to_string(target.join("guide.md")).unwrap(),
            "Unapproved guide"
        );
        assert_eq!(
            fs::read_to_string(root.join("skills/notes/SKILL.md")).unwrap(),
            skill_text("notes")
        );
    }
    #[test]
    fn rejected_package_never_partially_replaces_existing_files() {
        let root = temp_workspace("package-reject-write");
        write_skill(&root, "notes", &skill_text("notes"));
        let new = SkillPackage {
            bytes: skill_text("new").into_bytes(),
            files: vec![SkillSupportingFile {
                path: "../escape.md".into(),
                content: "new".into(),
            }],
        };
        assert!(write_package(&root, "notes", &new).is_err());
        assert_eq!(
            fs::read_to_string(root.join("skills/notes/SKILL.md")).unwrap(),
            skill_text("notes")
        );
        assert!(!root.join("skills/escape.md").exists());
    }

    #[test]
    fn held_package_root_cannot_be_redirected_by_directory_replacement() {
        let root = temp_workspace("package-held-root");
        write_skill(&root, "notes", &skill_text("notes"));
        let held = WorkspaceBytes::open(&root).unwrap();
        let retained = root.with_extension("retained");
        let outside = temp_workspace("package-held-outside");
        let moved = match fs::rename(&root, &retained) {
            Ok(()) => true,
            Err(error) => {
                assert_eq!(
                    error.raw_os_error(),
                    Some(32),
                    "held Windows root may refuse rename"
                );
                false
            }
        };
        if moved {
            assert!(directory_link(&root, &outside));
        }
        let next = SkillPackage {
            bytes: skill_text("updated").into_bytes(),
            files: vec![SkillSupportingFile {
                path: "templates/note.md".into(),
                content: "Held root template".into(),
            }],
        };
        write_held_package(&held, "notes", &next).unwrap();
        assert_eq!(
            read_held_package(&held, "notes").unwrap().unwrap().hash(),
            next.hash()
        );
        let actual = if moved { &retained } else { &root };
        assert_eq!(
            fs::read_to_string(actual.join("skills/notes/templates/note.md")).unwrap(),
            "Held root template"
        );
        assert!(!outside.join("skills").exists());
        assert!(!outside.join(".anima-skill-transactions").exists());
    }
}
