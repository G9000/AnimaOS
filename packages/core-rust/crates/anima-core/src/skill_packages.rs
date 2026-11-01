//! Portable skill package data. Stored scripts and assets remain inert; package
//! admission does not confer filesystem, network, or process execution authority.
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{borrow::Cow, collections::BTreeMap};

/// Maximum number of files besides the primary SKILL.md.
pub const MAX_SKILL_SUPPORTING_FILES: usize = 256;
/// Maximum decoded size of one supporting file.
pub const MAX_SKILL_SUPPORTING_FILE_BYTES: usize = 8 * 1024 * 1024;
/// Maximum combined decoded size, including SKILL.md.
pub const MAX_SKILL_PACKAGE_BYTES: usize = 32 * 1024 * 1024;
/// Maximum instruction file size, including its front matter.
pub const MAX_SKILL_INSTRUCTION_BYTES: usize = 36 * 1024;
/// Common package folders; packages may also contain other portable paths.
pub const SKILL_PACKAGE_FOLDERS: &[&str] =
    &["references", "templates", "scripts", "examples", "assets"];

/// Text stays readable in persisted records; inert binary assets use base64.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SkillFileEncoding {
    /// Ordinary visible UTF-8, retained for existing package records.
    #[default]
    Utf8,
    /// Canonical, padded standard base64 for an inert binary asset.
    Base64,
}
impl SkillFileEncoding {
    fn is_utf8(&self) -> bool {
        *self == Self::Utf8
    }
}

/// One portable package file, with no implied execution authority.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillSupportingFile {
    /// Portable relative path beneath the skill directory.
    pub path: String,
    /// Visible text, or standard base64 when encoding is base64.
    pub content: String,
    /// Omitted for legacy UTF-8 records.
    #[serde(default, skip_serializing_if = "SkillFileEncoding::is_utf8")]
    pub encoding: SkillFileEncoding,
}
impl SkillSupportingFile {
    /// Preserve downloaded bytes using the encoding required by their file type.
    ///
    /// # Errors
    /// Refuses unsafe paths, oversized data, or invalid text.
    pub fn from_bytes(path: String, bytes: Vec<u8>) -> Result<Self, &'static str> {
        validate_supporting_path(&path)?;
        if bytes.len() > MAX_SKILL_SUPPORTING_FILE_BYTES {
            return Err("A supporting file must be at most 8 MiB");
        }
        let (encoding, content) = if is_binary_supporting_path(&path) {
            (SkillFileEncoding::Base64, STANDARD.encode(bytes))
        } else {
            let text =
                String::from_utf8(bytes).map_err(|_| "Supporting text files must be UTF-8")?;
            validate_text(&text)?;
            (SkillFileEncoding::Utf8, text)
        };
        Ok(Self {
            path,
            content,
            encoding,
        })
    }

    /// Read the original bytes after checking encoding and per-file bounds.
    ///
    /// # Errors
    /// Refuses malformed or mismatched encoding, oversized data, or hidden text.
    pub fn bytes(&self) -> Result<Cow<'_, [u8]>, &'static str> {
        match self.encoding {
            SkillFileEncoding::Utf8 => {
                if is_binary_supporting_path(&self.path) {
                    return Err("Binary supporting files must use base64 encoding");
                }
                if self.content.len() > MAX_SKILL_SUPPORTING_FILE_BYTES {
                    return Err("A supporting file must be at most 8 MiB");
                }
                validate_text(&self.content)?;
                Ok(Cow::Borrowed(self.content.as_bytes()))
            }
            SkillFileEncoding::Base64 => {
                if !is_binary_supporting_path(&self.path) {
                    return Err("Supporting text files must use UTF-8 encoding");
                }
                // Check the wire representation before allocating decoded content.
                if self.content.len() > MAX_SKILL_SUPPORTING_FILE_BYTES.div_ceil(3) * 4 {
                    return Err("A supporting file must be at most 8 MiB");
                }
                let bytes = STANDARD
                    .decode(&self.content)
                    .map_err(|_| "Supporting binary content must be canonical standard base64")?;
                if bytes.len() > MAX_SKILL_SUPPORTING_FILE_BYTES {
                    return Err("A supporting file must be at most 8 MiB");
                }
                Ok(Cow::Owned(bytes))
            }
        }
    }

    /// Return the decoded file size, rejecting invalid encodings.
    ///
    /// # Errors
    /// Has the same validation errors as [`Self::bytes`].
    pub fn decoded_len(&self) -> Result<usize, &'static str> {
        self.bytes().map(|bytes| bytes.len())
    }
}

fn validate_text(text: &str) -> Result<(), &'static str> {
    if text.chars().any(|c| {
        (c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
            || matches!(c as u32, 0xE0000..=0xE007F | 0x202A..=0x202E | 0x2066..=0x2069 | 0xE0100..=0xE01EF)
    }) {
        return Err("Supporting text must be visible UTF-8 without binary or hidden direction characters");
    }
    Ok(())
}

/// Whether a file is a supported inert binary asset rather than readable text.
pub fn is_binary_supporting_path(path: &str) -> bool {
    path.rsplit_once('.').is_some_and(|(_, extension)| {
        [
            "png", "jpg", "jpeg", "webp", "gif", "ico", "bmp", "tif", "tiff", "avif", "pdf", "mp3",
            "mp4", "m4a", "wav", "aac", "flac", "ogg", "webm", "mov", "zip", "gz", "tgz", "tar",
            "bz2", "xz", "7z", "bin", "dat", "woff", "woff2", "ttf", "otf",
        ]
        .contains(&extension.to_ascii_lowercase().as_str())
    })
}

/// Check a portable relative path, including root files and nested libraries.
///
/// # Errors
/// Refuses escapes, hidden paths, reserved names, or executable binary formats.
pub fn validate_supporting_path(path: &str) -> Result<(), &'static str> {
    const INVALID: &str = "Supporting files need a portable relative path without hidden names, device names, escapes or executable binary formats";
    // This published skill metadata file is plain text and grants no authority.
    if path == ".skillignore" {
        return Ok(());
    }
    if path.len() > 240 || path.contains('\\') {
        return Err(INVALID);
    }
    let parts: Vec<_> = path.split('/').collect();
    if parts.is_empty() || parts.len() > 16 || parts[0].eq_ignore_ascii_case("SKILL.md") {
        return Err(INVALID);
    }
    for part in &parts {
        if part.is_empty()
            || part.starts_with('.')
            || part.ends_with(['.', ' '])
            || !part
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        {
            return Err(INVALID);
        }
        let stem = part
            .split('.')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        if ["con", "prn", "aux", "nul"].contains(&stem.as_str())
            || (stem.len() == 4
                && (stem.starts_with("com") || stem.starts_with("lpt"))
                && stem.as_bytes()[3].is_ascii_digit())
        {
            return Err(INVALID);
        }
    }
    if path.rsplit_once('.').is_some_and(|(_, extension)| {
        [
            "exe", "dll", "com", "msi", "scr", "cpl", "pif", "lnk", "sys", "drv", "so", "dylib",
            "pyc", "pyo",
        ]
        .contains(&extension.to_ascii_lowercase().as_str())
    }) {
        return Err(INVALID);
    }
    Ok(())
}

/// Validate the complete decoded package and all portable path collisions.
///
/// # Errors
/// Refuses invalid files, duplicate or conflicting paths, and resource overflows.
pub fn validate_skill_package(
    skill: &[u8],
    files: &[SkillSupportingFile],
) -> Result<(), &'static str> {
    if skill.len() > MAX_SKILL_INSTRUCTION_BYTES {
        return Err("SKILL.md must be at most 36 KiB");
    }
    if files.len() > MAX_SKILL_SUPPORTING_FILES {
        return Err("A skill package supports at most 256 supporting files");
    }
    let mut nodes = BTreeMap::new();
    let mut total = skill.len();
    for file in files {
        validate_supporting_path(&file.path)?;
        let parts = file.path.split('/').collect::<Vec<_>>();
        let mut prefix = String::new();
        for (index, part) in parts.iter().enumerate() {
            if index > 0 {
                prefix.push('/');
            }
            prefix.push_str(part);
            let is_file = index + 1 == parts.len();
            let key = prefix.to_ascii_lowercase();
            if let Some((previous, previous_is_file)) = nodes.get(&key) {
                if previous != &prefix || *previous_is_file || is_file {
                    return Err("Duplicate or conflicting supporting file paths are not allowed");
                }
            } else {
                nodes.insert(key, (prefix.clone(), is_file));
            }
        }
        total = total
            .checked_add(file.decoded_len()?)
            .ok_or("Skill package is too large")?;
        if total > MAX_SKILL_PACKAGE_BYTES {
            return Err("A complete skill package must be at most 32 MiB");
        }
    }
    if total > MAX_SKILL_PACKAGE_BYTES {
        return Err("A complete skill package must be at most 32 MiB");
    }
    Ok(())
}

/// Hash paths and decoded bytes, preserving hashes of existing text packages.
///
/// Callers must validate before approving a hash. Invalid encodings receive a
/// separate hash domain and cannot alias a valid file's approval identity.
pub fn skill_package_hash(skill: &[u8], files: &[SkillSupportingFile]) -> String {
    if files.is_empty() {
        return hex::encode(Sha256::digest(skill));
    }
    let mut hash = Sha256::new();
    hash.update(b"anima-skill-package-v1\0");
    hash.update((skill.len() as u64).to_be_bytes());
    hash.update(skill);
    let mut sorted = files.iter().collect::<Vec<_>>();
    sorted.sort_by(|a, b| a.path.cmp(&b.path));
    for file in sorted {
        hash.update((file.path.len() as u64).to_be_bytes());
        hash.update(file.path.as_bytes());
        match file.bytes() {
            Ok(bytes) => {
                hash.update((bytes.len() as u64).to_be_bytes());
                hash.update(bytes.as_ref());
            }
            Err(_) => {
                hash.update(u64::MAX.to_be_bytes());
                hash.update(b"invalid-encoding\0");
                hash.update([match file.encoding {
                    SkillFileEncoding::Utf8 => 0,
                    SkillFileEncoding::Base64 => 1,
                }]);
                hash.update((file.content.len() as u64).to_be_bytes());
                hash.update(file.content.as_bytes());
            }
        }
    }
    hex::encode(hash.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_packages_preserve_binary_assets_root_files_and_more_than_32_files() {
        let mut files = (0..80)
            .map(|i| file(&format!("lib/module_{i}.py"), "print('ready')\n"))
            .collect::<Vec<_>>();
        files.push(file("requirements.txt", "httpx==0.28.1\n"));
        let image = SkillSupportingFile::from_bytes(
            "assets/demo.png".into(),
            vec![0x89, b'P', b'N', b'G', 0, 0xff],
        )
        .unwrap();
        assert_eq!(image.encoding, SkillFileEncoding::Base64);
        assert_eq!(
            image.bytes().unwrap().as_ref(),
            &[0x89, b'P', b'N', b'G', 0, 0xff]
        );
        files.push(image);
        assert!(validate_skill_package(b"skill", &files).is_ok());
        let serialized = serde_json::to_vec(&files).unwrap();
        let restored: Vec<SkillSupportingFile> = serde_json::from_slice(&serialized).unwrap();
        assert_eq!(files, restored);
        assert_eq!(
            skill_package_hash(b"skill", &files),
            skill_package_hash(b"skill", &restored)
        );
    }

    #[test]
    fn legacy_json_defaults_to_utf8_and_text_hash_is_unchanged() {
        let legacy = r#"{"path":"scripts/run.py","content":"print('ok')"}"#;
        let parsed: SkillSupportingFile = serde_json::from_str(legacy).unwrap();
        assert_eq!(parsed.encoding, SkillFileEncoding::Utf8);
        assert_eq!(serde_json::to_string(&parsed).unwrap(), legacy);
        let mut expected = Sha256::new();
        expected.update(b"anima-skill-package-v1\0");
        expected.update(5_u64.to_be_bytes());
        expected.update(b"skill");
        expected.update(14_u64.to_be_bytes());
        expected.update(b"scripts/run.py");
        expected.update(11_u64.to_be_bytes());
        expected.update(b"print('ok')");
        assert_eq!(
            skill_package_hash(b"skill", &[parsed]),
            hex::encode(expected.finalize())
        );
    }

    #[test]
    fn binary_validation_rejects_malformed_data_text_obscuring_and_path_collisions() {
        let binary = |path: &str, content: &str| SkillSupportingFile {
            path: path.into(),
            content: content.into(),
            encoding: SkillFileEncoding::Base64,
        };
        assert!(validate_skill_package(b"skill", &[binary("assets/x.png", "invalid@@")]).is_err());
        assert!(
            validate_skill_package(b"skill", &[binary("scripts/run.py", "cHJpbnQoMSk=")]).is_err()
        );
        assert!(validate_skill_package(b"skill", &[file("assets/x.png", "not encoded")]).is_err());
        for files in [
            vec![file("lib", "a"), file("lib/module.py", "b")],
            vec![file("Lib/module.py", "a"), file("lib/other.py", "b")],
            vec![file("skill.md", "shadow main")],
        ] {
            assert!(validate_skill_package(b"skill", &files).is_err());
        }
        let first = binary("assets/x.bin", "AAE=");
        let second = binary("assets/x.bin", "AAI=");
        assert_ne!(
            skill_package_hash(b"skill", &[first]),
            skill_package_hash(b"skill", &[second])
        );
    }
    fn file(path: &str, content: &str) -> SkillSupportingFile {
        SkillSupportingFile {
            encoding: Default::default(),
            path: path.into(),
            content: content.into(),
        }
    }
    #[test]
    fn paths_reject_portability_escapes_devices_and_unsupported_files() {
        for path in [
            "../x",
            "/references/x.md",
            "C:/x.md",
            "references/../x.md",
            "references/./x.md",
            "references//x.md",
            "references\\x.md",
            "references/CON.md",
            "references/com1.txt",
            "references/a.txt:stream",
            "references/x.",
            "references/x ",
            "references/.hidden.md",
            "scripts/x.exe",
        ] {
            assert!(validate_supporting_path(path).is_err(), "{path}");
        }
        for path in [
            "references/api.md",
            "templates/nested/note.txt",
            "scripts/example.py",
            "examples/data.json",
            "assets/style.css",
            "elsewhere/x.md",
            "references/image.png",
            "LICENSE",
            ".skillignore",
            "requirements.txt",
        ] {
            assert!(validate_supporting_path(path).is_ok(), "{path}");
        }
    }
    #[test]
    fn package_hash_covers_paths_bytes_and_order_independently_and_preserves_legacy() {
        let bytes = b"legacy skill bytes";
        assert_eq!(
            skill_package_hash(bytes, &[]),
            hex::encode(Sha256::digest(bytes))
        );
        let a = file("templates/a.md", "a");
        let b = file("references/b.txt", "b");
        assert_eq!(
            skill_package_hash(bytes, &[a.clone(), b.clone()]),
            skill_package_hash(bytes, &[b.clone(), a.clone()])
        );
        assert_ne!(
            skill_package_hash(bytes, std::slice::from_ref(&a)),
            skill_package_hash(bytes, &[file("templates/a.md", "changed")])
        );
        assert_ne!(
            skill_package_hash(bytes, &[a]),
            skill_package_hash(bytes, &[file("templates/renamed.md", "a")])
        );
    }
    #[test]
    fn package_limits_duplicates_and_hidden_content_are_rejected() {
        let f = file("references/a.md", "a");
        assert!(validate_skill_package(b"skill", &[f.clone(), f]).is_err());
        assert!(validate_skill_package(
            b"skill",
            &[file("references/A.md", "a"), file("references/a.md", "b")]
        )
        .is_err());
        assert!(validate_skill_package(
            b"skill",
            &[file(
                "references/a.md",
                &"x".repeat(MAX_SKILL_SUPPORTING_FILE_BYTES + 1)
            )]
        )
        .is_err());
        let files = (0..=MAX_SKILL_SUPPORTING_FILES)
            .map(|i| file(&format!("references/{i}.md"), "a"))
            .collect::<Vec<_>>();
        assert!(validate_skill_package(b"skill", &files).is_err());
        let files = (0..5)
            .map(|i| {
                file(
                    &format!("references/{i}.md"),
                    &"x".repeat(MAX_SKILL_SUPPORTING_FILE_BYTES),
                )
            })
            .collect::<Vec<_>>();
        assert!(validate_skill_package(b"skill", &files).is_err());
        assert!(
            validate_skill_package(b"skill", &[file("references/a.md", "hidden\u{202e}text")])
                .is_err()
        );
        assert!(
            validate_skill_package(b"skill", &[file("references/a.md", "binary\0text")]).is_err()
        );
    }
}
