//! `anima logs [-f]`: the server's output in `logs/anima.log`.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

use crate::supervisor::LOG_FILE_NAME;

/// How many of the latest lines are shown.
const TAIL_LINES: usize = 200;
const FOLLOW_POLL: Duration = Duration::from_millis(250);

pub fn logs(follow: bool) -> io::Result<ExitCode> {
    let (paths, _) = super::load()?;
    let file = paths.logs.join(LOG_FILE_NAME);
    let mut stdout = io::stdout().lock();
    let mut offset = match std::fs::read(&file) {
        Ok(bytes) => {
            stdout.write_all(tail(&bytes, TAIL_LINES))?;
            stdout.flush()?;
            bytes.len() as u64
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if !follow {
                eprintln!("There is no log yet: {}", file.display());
                return Ok(ExitCode::SUCCESS);
            }
            0
        }
        Err(error) => return Err(error),
    };
    if follow {
        loop {
            thread::sleep(FOLLOW_POLL);
            offset = copy_new(&file, offset, &mut stdout)?;
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// The last `lines` lines of `text`.
fn tail(text: &[u8], lines: usize) -> &[u8] {
    let body = text.strip_suffix(b"\n").unwrap_or(text);
    let newlines = body.iter().rposition({
        let mut seen = 0;
        move |byte| {
            if *byte == b'\n' {
                seen += 1;
            }
            seen == lines
        }
    });
    match newlines {
        Some(index) => &text[index + 1..],
        None => text,
    }
}

/// Writes what was appended to `file` after `offset`; a file that shrank was
/// rotated, and is read from its start. Returns the new offset.
fn copy_new(file: &Path, offset: u64, out: &mut impl Write) -> io::Result<u64> {
    let mut handle = match File::open(file) {
        Ok(handle) => handle,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(offset),
        Err(error) => return Err(error),
    };
    let len = handle.metadata()?.len();
    let from = if len < offset { 0 } else { offset };
    if len == from {
        return Ok(from);
    }
    handle.seek(SeekFrom::Start(from))?;
    let mut added = Vec::new();
    handle.take(len - from).read_to_end(&mut added)?;
    out.write_all(&added)?;
    out.flush()?;
    Ok(from + added.len() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_keeps_the_last_lines() {
        let text = b"a\nb\nc\nd\n";
        assert_eq!(tail(text, 2), b"c\nd\n");
        assert_eq!(tail(text, 4), b"a\nb\nc\nd\n");
        assert_eq!(tail(text, 10), b"a\nb\nc\nd\n");
        assert_eq!(tail(b"a\nb", 1), b"b");
        assert_eq!(tail(b"", 3), b"");
    }

    #[test]
    fn copy_new_follows_appends_and_rotation() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("anima.log");
        std::fs::write(&file, "one\n").unwrap();
        let mut out = Vec::new();
        let offset = copy_new(&file, 0, &mut out).unwrap();
        assert_eq!(out, b"one\n");
        std::fs::write(&file, "one\ntwo\n").unwrap();
        let offset = copy_new(&file, offset, &mut out).unwrap();
        assert_eq!(out, b"one\ntwo\n");
        assert_eq!(copy_new(&file, offset, &mut out).unwrap(), offset);
        // Rotated: a new, shorter file.
        std::fs::write(&file, "x\n").unwrap();
        let offset = copy_new(&file, offset, &mut out).unwrap();
        assert_eq!(out, b"one\ntwo\nx\n");
        assert_eq!(offset, 2);
    }
}
