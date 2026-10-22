//! Portable skill package data. Supporting text remains inert; admission does
//! not confer filesystem or process execution authority.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub const MAX_SKILL_SUPPORTING_FILES: usize = 32;
pub const MAX_SKILL_SUPPORTING_FILE_BYTES: usize = 64 * 1024;
pub const MAX_SKILL_PACKAGE_BYTES: usize = 512 * 1024;
pub const SKILL_PACKAGE_FOLDERS: &[&str] =
    &["references", "templates", "scripts", "examples", "assets"];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillSupportingFile {
    pub path: String,
    pub content: String,
}

pub fn validate_supporting_path(path: &str) -> Result<(), &'static str> {
    const INVALID: &str = "Supporting files need a portable relative text file path under references/, templates/, scripts/, examples/ or assets/";
    if path.len() > 240 || path.contains('\\') {
        return Err(INVALID);
    }
    let parts: Vec<_> = path.split('/').collect();
    if parts.len() < 2 || parts.len() > 8 || !SKILL_PACKAGE_FOLDERS.contains(&parts[0]) {
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
    let extension = parts
        .last()
        .and_then(|p| p.rsplit_once('.'))
        .map(|(_, e)| e.to_ascii_lowercase());
    if !extension.as_deref().is_some_and(|e| {
        [
            "md", "txt", "json", "yaml", "yml", "toml", "csv", "tsv", "xml", "html", "css", "svg",
            "js", "mjs", "cjs", "ts", "tsx", "jsx", "py", "sh", "bash", "ps1", "sql", "rs", "r",
            "jinja", "jinja2", "j2", "mustache", "hbs",
        ]
        .contains(&e)
    }) {
        return Err(INVALID);
    }
    Ok(())
}
pub fn validate_skill_package(
    skill: &[u8],
    files: &[SkillSupportingFile],
) -> Result<(), &'static str> {
    if files.len() > MAX_SKILL_SUPPORTING_FILES {
        return Err("A skill package supports at most 32 supporting files");
    }
    let mut seen = BTreeSet::new();
    let mut total = skill.len();
    for file in files {
        validate_supporting_path(&file.path)?;
        if !seen.insert(file.path.to_ascii_lowercase()) {
            return Err("Duplicate supporting file paths are not allowed");
        }
        if file.content.len() > MAX_SKILL_SUPPORTING_FILE_BYTES {
            return Err("A supporting file must be at most 64 KiB");
        }
        if file.content.chars().any(|c| (c.is_control() && !matches!(c,'\n'|'\r'|'\t')) || matches!(c as u32,0xE0000..=0xE007F|0x202A..=0x202E|0x2066..=0x2069|0xE0100..=0xE01EF)) { return Err("Supporting files must be visible UTF-8 text without binary or hidden direction characters"); }
        total = total
            .checked_add(file.content.len())
            .ok_or("Skill package is too large")?;
        if total > MAX_SKILL_PACKAGE_BYTES {
            return Err("A complete skill package must be at most 512 KiB");
        }
    }
    if total > MAX_SKILL_PACKAGE_BYTES {
        return Err("A complete skill package must be at most 512 KiB");
    }
    Ok(())
}
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
        hash.update((file.content.len() as u64).to_be_bytes());
        hash.update(file.content.as_bytes());
    }
    hex::encode(hash.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn file(path: &str, content: &str) -> SkillSupportingFile {
        SkillSupportingFile {
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
            "elsewhere/x.md",
            "references/image.png",
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
            skill_package_hash(bytes, &[a.clone()]),
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
        let files = (0..33)
            .map(|i| file(&format!("references/{i}.md"), "a"))
            .collect::<Vec<_>>();
        assert!(validate_skill_package(b"skill", &files).is_err());
        let files = (0..9)
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
