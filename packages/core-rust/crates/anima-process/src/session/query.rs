//! Answers the terminal queries a TUI blocks on. A real terminal replies to
//! them on the program's input; our PTY master has no terminal behind it, so
//! without a reply programs such as Codex wait forever.
//!
//! The byte scanner follows openai/codex#41436. Unlike that, the cursor
//! report carries the real cursor from our screen model instead of a
//! fixed 1;1.

const ESC: u8 = 0x1b;
/// DEC private-mode queries carry a bounded numeric mode.
const MAX_MODE_DIGITS: usize = 10;
/// Longest handled sequence: `ESC [ ? <digits> $ p`.
const MAX_QUERY_BYTES: usize = MAX_MODE_DIGITS + 5;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Query {
    /// `ESC[5n`: device status.
    Status,
    /// `ESC[6n`: cursor position.
    CursorPosition,
    /// `ESC[18t`: text area size.
    WindowSize,
    /// `ESC[c` / `ESC[0c`: primary device attributes.
    DeviceAttributes,
    /// `ESC[?<mode>$p`: DEC private-mode state.
    PrivateMode(Vec<u8>),
}

impl Query {
    /// The reply, given the cursor (zero-based row, column) and screen size.
    pub(crate) fn reply(&self, cursor: (u16, u16), size: (u16, u16)) -> Vec<u8> {
        match self {
            Self::Status => b"\x1b[0n".to_vec(),
            Self::CursorPosition => format!("\x1b[{};{}R", cursor.0 + 1, cursor.1 + 1).into_bytes(),
            Self::WindowSize => format!("\x1b[8;{};{}t", size.0, size.1).into_bytes(),
            // VT100 with advanced video: the most widely assumed baseline.
            Self::DeviceAttributes => b"\x1b[?1;2c".to_vec(),
            // Mode not recognised.
            Self::PrivateMode(mode) => [b"\x1b[?".as_slice(), mode, b";0$y"].concat(),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Piece {
    Output(Vec<u8>),
    Query(Query),
}

/// Splits a PTY output stream into output and queries, holding back a query
/// split across reads. Every other byte passes through untouched.
#[derive(Default)]
pub(crate) struct QueryScanner {
    pending: Vec<u8>,
}

impl QueryScanner {
    pub(crate) fn scan(&mut self, data: &[u8]) -> Vec<Piece> {
        let mut pieces = Vec::new();
        let mut output = Vec::new();
        for &byte in data {
            if self.pending.is_empty() && byte != ESC {
                output.push(byte);
                continue;
            }
            if byte == ESC {
                // A fresh ESC ends any partial sequence; pass it through.
                output.append(&mut self.pending);
            }
            self.pending.push(byte);
            let still_open = self.pending.len() == 1
                || self.pending == b"\x1b["
                || (self.pending[1] == b'['
                    && !(0x40..=0x7e).contains(&byte)
                    && self.pending.len() < MAX_QUERY_BYTES);
            if still_open {
                continue;
            }
            match classify(&self.pending) {
                Some(query) => {
                    if !output.is_empty() {
                        pieces.push(Piece::Output(std::mem::take(&mut output)));
                    }
                    pieces.push(Piece::Query(query));
                    self.pending.clear();
                }
                None => output.append(&mut self.pending),
            }
        }
        if !output.is_empty() {
            pieces.push(Piece::Output(output));
        }
        pieces
    }

    /// The incomplete sequence held back when the stream ends.
    pub(crate) fn flush(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.pending)
    }
}

fn classify(sequence: &[u8]) -> Option<Query> {
    match sequence {
        b"\x1b[5n" => return Some(Query::Status),
        b"\x1b[6n" => return Some(Query::CursorPosition),
        b"\x1b[18t" => return Some(Query::WindowSize),
        b"\x1b[c" | b"\x1b[0c" => return Some(Query::DeviceAttributes),
        _ => {}
    }
    let mode = sequence.strip_prefix(b"\x1b[?")?.strip_suffix(b"$p")?;
    (!mode.is_empty() && mode.len() <= MAX_MODE_DIGITS && mode.iter().all(u8::is_ascii_digit))
        .then(|| Query::PrivateMode(mode.to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect(scanner: &mut QueryScanner, chunks: &[&[u8]]) -> (Vec<u8>, Vec<Query>) {
        let mut output = Vec::new();
        let mut queries = Vec::new();
        for chunk in chunks {
            for piece in scanner.scan(chunk) {
                match piece {
                    Piece::Output(bytes) => output.extend(bytes),
                    Piece::Query(query) => queries.push(query),
                }
            }
        }
        (output, queries)
    }

    #[test]
    fn handled_queries_are_removed_and_reported_in_order() {
        let mut scanner = QueryScanner::default();
        let (output, queries) = collect(
            &mut scanner,
            &[b"ab\x1b[6ncd\x1b[5n\x1b[18t\x1b[c\x1b[?2026$pe"],
        );
        assert_eq!(output, b"abcde");
        assert_eq!(
            queries,
            vec![
                Query::CursorPosition,
                Query::Status,
                Query::WindowSize,
                Query::DeviceAttributes,
                Query::PrivateMode(b"2026".to_vec()),
            ]
        );
    }

    #[test]
    fn a_query_split_across_reads_is_still_answered() {
        let mut scanner = QueryScanner::default();
        let (output, queries) = collect(&mut scanner, &[b"x\x1b", b"[", b"6", b"ny"]);
        assert_eq!(output, b"xy");
        assert_eq!(queries, vec![Query::CursorPosition]);
    }

    #[test]
    fn other_sequences_pass_through_untouched() {
        let mut scanner = QueryScanner::default();
        let input: &[u8] = b"\x1b[31mred\x1b[0m\x1b]0;title\x07\x1b[?1049h\x1b[?99999999999$p";
        let (output, queries) = collect(&mut scanner, &[input]);
        assert_eq!(output, input);
        assert!(queries.is_empty());
    }

    #[test]
    fn replies_use_the_real_cursor_and_size() {
        assert_eq!(
            Query::CursorPosition.reply((2, 9), (40, 120)),
            b"\x1b[3;10R"
        );
        assert_eq!(
            Query::WindowSize.reply((0, 0), (40, 120)),
            b"\x1b[8;40;120t"
        );
        assert_eq!(
            Query::PrivateMode(b"2026".to_vec()).reply((0, 0), (1, 1)),
            b"\x1b[?2026;0$y"
        );
    }
}
