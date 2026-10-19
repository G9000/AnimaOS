//! The server's output: `logs/anima.log`, rotated at 10 MiB keeping 3 files
//! (`anima.log`, `anima.log.1`, `anima.log.2`), plus a short in-memory tail
//! of the current server's lines for diagnosing a crash.

use std::collections::VecDeque;
use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use super::lock;

pub const LOG_FILE_NAME: &str = "anima.log";
const MAX_BYTES: u64 = 10 * 1024 * 1024;
const KEEP_FILES: usize = 3;
const TAIL_LINES: usize = 200;

/// A log file that rotates when the next line would take it past `max_bytes`.
pub struct RotatingLog {
    path: PathBuf,
    max_bytes: u64,
    keep: usize,
    file: Option<File>,
    len: u64,
}

impl RotatingLog {
    pub fn open(dir: &Path) -> Self {
        Self::with_limits(dir.join(LOG_FILE_NAME), MAX_BYTES, KEEP_FILES)
    }

    fn with_limits(path: PathBuf, max_bytes: u64, keep: usize) -> Self {
        Self {
            path,
            max_bytes,
            keep,
            file: None,
            len: 0,
        }
    }

    /// Appends one line (a newline is added). Failures are dropped: losing a
    /// log line must not take the server down.
    pub fn write_line(&mut self, line: &str) {
        let _ = self.try_write_line(line);
    }

    fn try_write_line(&mut self, line: &str) -> io::Result<()> {
        let needed = line.len() as u64 + 1;
        if self.file.is_none() {
            self.file = Some(open_append(&self.path)?);
            self.len = std::fs::metadata(&self.path)?.len();
        }
        if self.len > 0 && self.len + needed > self.max_bytes {
            self.rotate()?;
        }
        let file = self.file.as_mut().expect("opened above");
        file.write_all(line.as_bytes())?;
        file.write_all(b"\n")?;
        self.len += needed;
        Ok(())
    }

    fn rotate(&mut self) -> io::Result<()> {
        self.file = None;
        let numbered = |n: usize| {
            let mut name = self.path.as_os_str().to_os_string();
            name.push(format!(".{n}"));
            PathBuf::from(name)
        };
        let _ = std::fs::remove_file(numbered(self.keep - 1));
        for n in (1..self.keep - 1).rev() {
            let _ = std::fs::rename(numbered(n), numbered(n + 1));
        }
        std::fs::rename(&self.path, numbered(1))?;
        self.file = Some(open_append(&self.path)?);
        self.len = 0;
        Ok(())
    }
}

fn open_append(path: &Path) -> io::Result<File> {
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

/// The log and the current server's tail, shared by the output readers.
#[derive(Clone)]
pub struct ServerLog {
    file: Arc<Mutex<RotatingLog>>,
    tail: Arc<Mutex<VecDeque<String>>>,
}

impl ServerLog {
    pub fn open(dir: &Path) -> Self {
        Self {
            file: Arc::new(Mutex::new(RotatingLog::open(dir))),
            tail: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    /// A line of the server's output.
    pub fn server_line(&self, raw: &[u8]) {
        let line = strip_ansi(&String::from_utf8_lossy(raw));
        let line = line.trim_end_matches(['\r', '\n']);
        lock(&self.file).write_line(line);
        let mut tail = lock(&self.tail);
        if tail.len() == TAIL_LINES {
            tail.pop_front();
        }
        tail.push_back(line.to_string());
    }

    /// A line from the supervisor itself.
    pub fn note(&self, message: &str) {
        lock(&self.file).write_line(&format!("[anima supervisor] {message}"));
    }

    /// Forgets the previous server's tail before a new one starts.
    pub fn clear_tail(&self) {
        lock(&self.tail).clear();
    }

    pub fn tail_contains(&self, needles: &[&str]) -> bool {
        lock(&self.tail)
            .iter()
            .any(|line| needles.iter().any(|needle| line.contains(needle)))
    }

    pub fn last_line(&self) -> Option<String> {
        lock(&self.tail).back().cloned()
    }
}

/// Drops terminal color sequences (`ESC [ ... final`) from a line.
fn strip_ansi(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.clone().next() == Some('[') {
                chars.next();
                for c in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&c) {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotates_keeping_three_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(LOG_FILE_NAME);
        let mut log = RotatingLog::with_limits(path.clone(), 20, 3);
        for n in 0..10 {
            log.write_line(&format!("line-{n:04}")); // 10 bytes with the newline
        }
        let read = |name: &str| std::fs::read_to_string(dir.path().join(name)).unwrap();
        assert_eq!(read("anima.log"), "line-0008\nline-0009\n");
        assert_eq!(read("anima.log.1"), "line-0006\nline-0007\n");
        assert_eq!(read("anima.log.2"), "line-0004\nline-0005\n");
        assert!(!dir.path().join("anima.log.3").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn appends_to_an_existing_log() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(LOG_FILE_NAME), "old\n").unwrap();
        let mut log = RotatingLog::open(dir.path());
        log.write_line("new");
        assert_eq!(
            std::fs::read_to_string(dir.path().join(LOG_FILE_NAME)).unwrap(),
            "old\nnew\n"
        );
    }

    #[test]
    fn server_lines_lose_colors_and_feed_the_tail() {
        let dir = tempfile::tempdir().unwrap();
        let log = ServerLog::open(dir.path());
        log.server_line(b"\x1b[2m2026\x1b[0m \x1b[32m INFO\x1b[0m listening\n");
        log.server_line(b"anima: Address already in use (os error 48)\r\n");
        assert!(log.tail_contains(&["Address already in use"]));
        assert_eq!(
            log.last_line().as_deref(),
            Some("anima: Address already in use (os error 48)")
        );
        log.clear_tail();
        assert!(!log.tail_contains(&["Address already in use"]));
        let text = std::fs::read_to_string(dir.path().join(LOG_FILE_NAME)).unwrap();
        assert_eq!(
            text,
            "2026  INFO listening\nanima: Address already in use (os error 48)\n"
        );
    }
}
