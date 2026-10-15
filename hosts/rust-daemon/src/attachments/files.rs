use super::{validation::validate_upload, *};
use crate::tools::workspace_bytes::WorkspaceBytes;
use sha2::{Digest, Sha256};
use std::path::Path;

#[derive(Debug)]
pub(crate) struct SavedUpload {
    pub(crate) path: String,
    pub(crate) workspace_root: String,
    pub(crate) sha256: String,
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn io_error(error: std::io::Error) -> AttachmentError {
    if error.kind() == std::io::ErrorKind::FileTooLarge {
        AttachmentError::TooLarge
    } else {
        AttachmentError::Unavailable
    }
}

pub(crate) fn save_upload(
    root: &Path,
    date: &str,
    name: &str,
    bytes: &[u8],
) -> Result<SavedUpload, AttachmentError> {
    let validated = validate_upload(name, bytes)?;
    if validated.name != name
        || name.contains(['/', '\\', ':'])
        || chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_err()
        || date.len() != 10
        || !date.bytes().enumerate().all(|(i, c)| {
            if i == 4 || i == 7 {
                c == b'-'
            } else {
                c.is_ascii_digit()
            }
        })
    {
        return Err(AttachmentError::Invalid);
    }
    let workspace = WorkspaceBytes::open(root).map_err(io_error)?;
    let workspace_root = workspace
        .canonical_root
        .to_str()
        .ok_or(AttachmentError::Unavailable)?
        .to_string();
    let (stem, extension) = name.rsplit_once('.').ok_or(AttachmentError::Invalid)?;
    for attempt in 0..MAX_UPLOAD_COLLISION_ATTEMPTS {
        let candidate = if attempt == 0 {
            name.to_string()
        } else {
            let suffix = format!("-{attempt}.{extension}");
            let mut stem = stem.to_string();
            while stem.len() + suffix.len() > MAX_UPLOAD_NAME_BYTES {
                stem.pop();
            }
            format!("{stem}{suffix}")
        };
        let path = format!("uploads/{date}/{candidate}");
        match workspace.create_new_bytes(&path, bytes) {
            Ok(()) => {
                return Ok(SavedUpload {
                    path,
                    workspace_root,
                    sha256: digest(bytes),
                })
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(io_error(error)),
        }
    }
    Err(AttachmentError::CollisionExhausted)
}

pub(crate) fn read_attachment_bytes(
    root: &Path,
    record: &AttachmentRecord,
) -> Result<Vec<u8>, AttachmentError> {
    let workspace = WorkspaceBytes::open(root).map_err(io_error)?;
    if workspace.canonical_root.to_str() != Some(record.workspace_root.as_str()) {
        return Err(AttachmentError::WorkspaceChanged);
    }
    let limit = attachment_read_limit(record)?;
    let bytes = workspace
        .read_bounded(&record.path, limit, record.size)
        .map_err(io_error)?;
    if digest(&bytes) != record.sha256 {
        return Err(AttachmentError::Changed);
    }
    let validated = validate_upload(&record.name, &bytes).map_err(|_| AttachmentError::Changed)?;
    if validated.kind != record.kind
        || validated.mime != record.mime
        || validated.name != record.name
    {
        return Err(AttachmentError::Changed);
    }
    Ok(bytes)
}

fn attachment_read_limit(record: &AttachmentRecord) -> Result<usize, AttachmentError> {
    let maximum = record.kind.max_bytes();
    if record.size > maximum as u64 {
        return Err(AttachmentError::TooLarge);
    }
    // read_bounded probes at most limit + 1 bytes. Even a file that grows
    // after its metadata check cannot spend the family's larger allowance.
    Ok(maximum.min(record.size as usize))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};
    struct Temp(std::path::PathBuf);
    impl Temp {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!("anima-attachment-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&p).unwrap();
            Self(p)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn record(root: &Path, saved: SavedUpload, bytes: &[u8]) -> AttachmentRecord {
        let _ = root;
        AttachmentRecord {
            id: "id".into(),
            agent_id: "agent".into(),
            session_id: "chat:a".into(),
            name: "notes.txt".into(),
            mime: "text/plain".into(),
            size: bytes.len() as u64,
            kind: AttachmentKind::Text,
            path: saved.path,
            created_at_ms: 0,
            workspace_root: saved.workspace_root,
            sha256: saved.sha256,
        }
    }
    #[test]
    fn attachment_exclusive_collisions_preserve_each_file() {
        let temp = Temp::new();
        let one = save_upload(&temp.0, "2026-09-23", "notes.txt", b"first").unwrap();
        let two = save_upload(&temp.0, "2026-09-23", "notes.txt", b"second").unwrap();
        assert_eq!(one.path, "uploads/2026-09-23/notes.txt");
        assert_eq!(two.path, "uploads/2026-09-23/notes-1.txt");
        assert_eq!(std::fs::read(temp.0.join(one.path)).unwrap(), b"first");
        assert_eq!(std::fs::read(temp.0.join(two.path)).unwrap(), b"second");
    }
    #[test]
    fn attachment_parallel_creates_do_not_truncate() {
        let temp = Temp::new();
        let barrier = Arc::new(Barrier::new(8));
        let threads = (0..8)
            .map(|n| {
                let root = temp.0.clone();
                let gate = barrier.clone();
                std::thread::spawn(move || {
                    gate.wait();
                    let bytes = format!("file {n}").into_bytes();
                    let saved = save_upload(&root, "2026-09-23", "notes.txt", &bytes).unwrap();
                    (saved.path, bytes)
                })
            })
            .collect::<Vec<_>>();
        let mut paths = std::collections::HashSet::new();
        for thread in threads {
            let (path, bytes) = thread.join().unwrap();
            assert!(paths.insert(path.clone()));
            assert_eq!(std::fs::read(temp.0.join(path)).unwrap(), bytes);
        }
    }
    #[test]
    fn attachment_collision_exhaustion_and_invalid_components_are_bounded() {
        let temp = Temp::new();
        for _ in 0..MAX_UPLOAD_COLLISION_ATTEMPTS {
            save_upload(&temp.0, "2026-09-23", "notes.txt", b"hello").unwrap();
        }
        assert_eq!(
            save_upload(&temp.0, "2026-09-23", "notes.txt", b"hello").unwrap_err(),
            AttachmentError::CollisionExhausted
        );
        for (date, name) in [
            ("../outside", "notes.txt"),
            ("2026-09-23", "../notes.txt"),
            ("2026-09-23", "C:notes.txt"),
        ] {
            assert!(save_upload(&temp.0, date, name, b"hello").is_err());
        }
    }
    #[test]
    fn attachment_read_pins_root_size_digest_and_classification() {
        let temp = Temp::new();
        let other = Temp::new();
        let bytes = b"hello";
        let saved = save_upload(&temp.0, "2026-09-23", "notes.txt", bytes).unwrap();
        let row = record(&temp.0, saved, bytes);
        assert_eq!(read_attachment_bytes(&temp.0, &row).unwrap(), bytes);
        assert_eq!(
            read_attachment_bytes(&other.0, &row).unwrap_err(),
            AttachmentError::WorkspaceChanged
        );
        let mut changed = row.clone();
        changed.mime = "application/pdf".into();
        assert_eq!(
            read_attachment_bytes(&temp.0, &changed).unwrap_err(),
            AttachmentError::Changed
        );
        std::fs::write(temp.0.join(&row.path), b"other").unwrap();
        assert_eq!(
            read_attachment_bytes(&temp.0, &row).unwrap_err(),
            AttachmentError::Changed
        );
        std::fs::write(temp.0.join(&row.path), vec![b'a'; MAX_TEXT_BYTES + 1]).unwrap();
        assert_eq!(
            read_attachment_bytes(&temp.0, &row).unwrap_err(),
            AttachmentError::TooLarge
        );
        std::fs::remove_file(temp.0.join(&row.path)).unwrap();
        assert_eq!(
            read_attachment_bytes(&temp.0, &row).unwrap_err(),
            AttachmentError::Unavailable
        );
    }

    #[test]
    fn document_physical_read_ceiling_is_expected_size_plus_one_sentinel() {
        let temp = Temp::new();
        let saved = save_upload(&temp.0, "2026-10-16", "notes.txt", b"small").unwrap();
        let mut row = record(&temp.0, saved, b"small");
        row.kind = AttachmentKind::Document;
        row.size = 2 * 1024 * 1024;
        assert_eq!(
            attachment_read_limit(&row).unwrap() + 1,
            2 * 1024 * 1024 + 1
        );
        row.size = MAX_DOCUMENT_BYTES as u64 + 1;
        assert_eq!(attachment_read_limit(&row), Err(AttachmentError::TooLarge));
        row.kind = AttachmentKind::Text;
        row.size = 5;
        assert_eq!(attachment_read_limit(&row), Ok(5));
        std::fs::write(temp.0.join(&row.path), b"grown beyond expected").unwrap();
        assert_eq!(
            read_attachment_bytes(&temp.0, &row),
            Err(AttachmentError::TooLarge)
        );
    }

    #[test]
    fn audio_physical_read_ceiling_uses_four_mib_record_size_and_sentinel() {
        let temp = Temp::new();
        let saved = save_upload(&temp.0, "2026-10-16", "notes.txt", b"small").unwrap();
        let mut row = record(&temp.0, saved, b"small");
        row.kind = AttachmentKind::Audio;
        row.size = anima_core::audio_content::MAX_AUDIO_INPUT_BYTES as u64;
        assert_eq!(
            attachment_read_limit(&row).unwrap() + 1,
            4 * 1024 * 1024 + 1
        );
        row.size += 1;
        assert_eq!(attachment_read_limit(&row), Err(AttachmentError::TooLarge));
        row.size = 5;
        assert_eq!(attachment_read_limit(&row), Ok(5));
    }

    #[cfg(windows)]
    #[test]
    fn attachment_windows_junction_parents_and_final_replacement_are_refused() {
        fn junction(link: &Path, target: &Path) {
            let output = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(link.to_string_lossy().replace('/', r"\"))
                .arg(target.to_string_lossy().replace('/', r"\"))
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "junction fixture failed: {} {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let temp = Temp::new();
        let outside = Temp::new();
        let upload_dir = temp.0.join("uploads");
        junction(&upload_dir, &outside.0);
        assert!(save_upload(&temp.0, "2026-09-23", "notes.txt", b"hello").is_err());
        std::fs::remove_dir(&upload_dir).unwrap();
        let saved = save_upload(&temp.0, "2026-09-23", "notes.txt", b"hello").unwrap();
        let row = record(&temp.0, saved, b"hello");
        let target = temp.0.join(&row.path);
        std::fs::remove_file(&target).unwrap();
        junction(&target, &outside.0);
        assert!(read_attachment_bytes(&temp.0, &row).is_err());
        std::fs::remove_dir(&target).unwrap();
        std::fs::write(outside.0.join("notes.txt"), b"hello").unwrap();
        let date = temp.0.join("uploads/2026-09-23");
        std::fs::remove_dir(&date).unwrap();
        junction(&date, &outside.0);
        assert!(read_attachment_bytes(&temp.0, &row).is_err());
        std::fs::remove_dir(&date).unwrap();
        assert_eq!(
            std::fs::read(outside.0.join("notes.txt")).unwrap(),
            b"hello"
        );
    }
    #[cfg(unix)]
    #[test]
    fn attachment_parent_links_final_replacements_and_fifo_are_refused() {
        use std::os::unix::fs::symlink;
        let temp = Temp::new();
        let outside = Temp::new();
        symlink(&outside.0, temp.0.join("uploads")).unwrap();
        assert!(save_upload(&temp.0, "2026-09-23", "notes.txt", b"hello").is_err());
        std::fs::remove_file(temp.0.join("uploads")).unwrap();
        let saved = save_upload(&temp.0, "2026-09-23", "notes.txt", b"hello").unwrap();
        let row = record(&temp.0, saved, b"hello");
        let target = temp.0.join(&row.path);
        std::fs::remove_file(&target).unwrap();
        std::fs::write(outside.0.join("secret"), b"hello").unwrap();
        symlink(outside.0.join("secret"), &target).unwrap();
        assert!(read_attachment_bytes(&temp.0, &row).is_err());
        std::fs::remove_file(&target).unwrap();
        assert!(std::process::Command::new("mkfifo")
            .arg(&target)
            .status()
            .unwrap()
            .success());
        assert!(read_attachment_bytes(&temp.0, &row).is_err());
    }
    #[test]
    fn instruction_named_uploads_remain_ordinary_upload_data() {
        let temp = Temp::new();
        for name in [
            "AGENTS.md",
            "SOUL.md",
            "agents.MD",
            "ANIMA.md",
            "AGENTS.override.md",
        ] {
            let validated = validate_upload(name, b"ordinary upload").unwrap();
            let saved =
                save_upload(&temp.0, "2026-09-23", &validated.name, b"ordinary upload").unwrap();
            assert!(saved.path.starts_with("uploads/2026-09-23/"));
            assert!(!temp.0.join(name).exists());
            assert_eq!(
                std::fs::read(temp.0.join(saved.path)).unwrap(),
                b"ordinary upload"
            );
        }
    }
}
