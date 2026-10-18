//! Plain-text transcript of a terminal session: escape sequences removed,
//! line endings normalised, bounded in bytes and lines.

/// Removes escape sequences from a terminal byte stream across reads, so the
/// transcript reads like what a person saw scroll past.
#[derive(Default)]
pub(crate) struct EscapeStripper {
    state: StripState,
    /// A UTF-8 character split across reads.
    partial: Vec<u8>,
    last_was_cr: bool,
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum StripState {
    #[default]
    Text,
    Escape,
    Csi,
    /// OSC, DCS, APC, PM and SOS strings, ended by BEL or ESC \.
    String,
    StringEscape,
}

impl EscapeStripper {
    pub(crate) fn strip(&mut self, data: &[u8]) -> String {
        let mut text = Vec::with_capacity(data.len());
        for &byte in data {
            match self.state {
                StripState::Text => match byte {
                    0x1b => self.state = StripState::Escape,
                    b'\r' => {
                        text.push(b'\n');
                        self.last_was_cr = true;
                        continue;
                    }
                    b'\n' if self.last_was_cr => {}
                    b'\n' | b'\t' => text.push(byte),
                    0x08 => {
                        text.pop();
                    }
                    0x00..=0x1f | 0x7f => {}
                    _ => text.push(byte),
                },
                StripState::Escape => {
                    self.state = match byte {
                        b'[' => StripState::Csi,
                        b']' | b'P' | b'_' | b'^' | b'X' => StripState::String,
                        // Charset designations take one more byte.
                        b'(' | b')' | b'*' | b'+' => StripState::Escape,
                        _ => StripState::Text,
                    }
                }
                StripState::Csi => {
                    if (0x40..=0x7e).contains(&byte) {
                        self.state = StripState::Text;
                    }
                }
                StripState::String => match byte {
                    0x07 => self.state = StripState::Text,
                    0x1b => self.state = StripState::StringEscape,
                    _ => {}
                },
                StripState::StringEscape => {
                    self.state = if byte == b'\\' {
                        StripState::Text
                    } else {
                        StripState::String
                    };
                }
            }
            self.last_was_cr = false;
        }
        self.partial.extend_from_slice(&text);
        let valid = match std::str::from_utf8(&self.partial) {
            Ok(_) => self.partial.len(),
            Err(error) if error.error_len().is_none() => error.valid_up_to(),
            Err(_) => {
                let lossy = String::from_utf8_lossy(&self.partial).into_owned();
                self.partial.clear();
                return lossy;
            }
        };
        let rest = self.partial.split_off(valid);
        let done = std::mem::replace(&mut self.partial, rest);
        String::from_utf8(done).unwrap_or_default()
    }
}

/// The session transcript, addressed by absolute character offsets so a
/// reader can ask for "everything after what I last saw" even after the
/// oldest text was dropped.
pub(crate) struct Transcript {
    text: String,
    /// Absolute offset (in bytes of the whole stream) of `text[0]`.
    start: u64,
    max_bytes: usize,
    max_lines: usize,
}

pub(crate) struct Slice {
    pub(crate) text: String,
    pub(crate) next: u64,
    /// Part of what was asked for had already been dropped.
    pub(crate) truncated: bool,
}

impl Transcript {
    pub(crate) fn new(max_bytes: usize, max_lines: usize) -> Self {
        Self {
            text: String::new(),
            start: 0,
            max_bytes,
            max_lines,
        }
    }

    pub(crate) fn end(&self) -> u64 {
        self.start + self.text.len() as u64
    }

    pub(crate) fn push(&mut self, text: &str) {
        self.text.push_str(text);
        let mut cut = self.text.len().saturating_sub(self.max_bytes);
        let lines = self.text.matches('\n').count();
        if lines > self.max_lines {
            let excess = lines - self.max_lines;
            if let Some((index, _)) = self.text.match_indices('\n').nth(excess - 1) {
                cut = cut.max(index + 1);
            }
        }
        if cut > 0 {
            while !self.text.is_char_boundary(cut) {
                cut += 1;
            }
            self.text.drain(..cut);
            self.start += cut as u64;
        }
    }

    pub(crate) fn since(&self, cursor: u64, max_bytes: usize) -> Slice {
        let mut truncated = cursor < self.start;
        let mut from = (cursor.clamp(self.start, self.end()) - self.start) as usize;
        // Return the newest output when more is pending than fits.
        if self.text.len() - from > max_bytes {
            from = self.text.len() - max_bytes;
            while !self.text.is_char_boundary(from) {
                from += 1;
            }
            truncated = true;
        }
        Slice {
            text: self.text[from..].to_string(),
            next: self.end(),
            truncated,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_sequences_are_removed_across_reads() {
        let mut stripper = EscapeStripper::default();
        let mut text = stripper.strip(b"\x1b[31mred\x1b");
        text += &stripper.strip(b"[0m \x1b]0;title\x07plain\x1b(B ok\r\n");
        text += &stripper.strip(b"bar\x08z\r\nend");
        assert_eq!(text, "red plain ok\nbaz\nend");
    }

    #[test]
    fn a_character_split_across_reads_is_kept_whole() {
        let mut stripper = EscapeStripper::default();
        let bytes = "é✓".as_bytes();
        let first = stripper.strip(&bytes[..1]);
        let second = stripper.strip(&bytes[1..4]);
        let third = stripper.strip(&bytes[4..]);
        assert_eq!(format!("{first}{second}{third}"), "é✓");
        assert_eq!(first, "");
    }

    #[test]
    fn the_transcript_drops_the_oldest_text_by_bytes_and_lines() {
        let mut transcript = Transcript::new(16, 2);
        transcript.push("one\ntwo\nthree\n");
        let all = transcript.since(0, 1024);
        assert_eq!(all.text, "two\nthree\n");
        assert!(all.truncated);
        transcript.push("0123456789abcdef");
        let latest = transcript.since(all.next, 1024);
        assert_eq!(latest.text, "0123456789abcdef");
        assert!(!latest.truncated);
        assert_eq!(transcript.since(0, 4).text, "cdef");
    }
}
