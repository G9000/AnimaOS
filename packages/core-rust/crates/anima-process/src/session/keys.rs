//! Named keys and typed text, encoded the way a terminal sends them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Enter,
    Tab,
    BackTab,
    Esc,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Backspace,
    Delete,
    Space,
    CtrlC,
    CtrlD,
    CtrlL,
    CtrlZ,
}

impl Key {
    pub const NAMES: &'static [&'static str] = &[
        "Enter",
        "Tab",
        "BackTab",
        "Esc",
        "Up",
        "Down",
        "Left",
        "Right",
        "Home",
        "End",
        "PageUp",
        "PageDown",
        "Backspace",
        "Delete",
        "Space",
        "Ctrl+C",
        "Ctrl+D",
        "Ctrl+L",
        "Ctrl+Z",
    ];

    /// Parses a key name as the tools accept it, ignoring case.
    pub fn parse(name: &str) -> Option<Self> {
        let name = name.trim();
        let key = match name.to_ascii_lowercase().as_str() {
            "enter" | "return" => Self::Enter,
            "tab" => Self::Tab,
            "backtab" | "shift+tab" => Self::BackTab,
            "esc" | "escape" => Self::Esc,
            "up" => Self::Up,
            "down" => Self::Down,
            "left" => Self::Left,
            "right" => Self::Right,
            "home" => Self::Home,
            "end" => Self::End,
            "pageup" => Self::PageUp,
            "pagedown" => Self::PageDown,
            "backspace" => Self::Backspace,
            "delete" => Self::Delete,
            "space" => Self::Space,
            "ctrl+c" => Self::CtrlC,
            "ctrl+d" => Self::CtrlD,
            "ctrl+l" => Self::CtrlL,
            "ctrl+z" => Self::CtrlZ,
            _ => return None,
        };
        Some(key)
    }

    /// The bytes for this key. Arrow, Home and End keys follow the app's
    /// cursor-key mode, which full-screen apps switch on.
    pub(crate) fn bytes(self, application_cursor: bool) -> &'static [u8] {
        let cursor = |normal: &'static [u8], application: &'static [u8]| {
            if application_cursor {
                application
            } else {
                normal
            }
        };
        match self {
            Self::Enter => b"\r",
            Self::Tab => b"\t",
            Self::BackTab => b"\x1b[Z",
            Self::Esc => b"\x1b",
            Self::Up => cursor(b"\x1b[A", b"\x1bOA"),
            Self::Down => cursor(b"\x1b[B", b"\x1bOB"),
            Self::Right => cursor(b"\x1b[C", b"\x1bOC"),
            Self::Left => cursor(b"\x1b[D", b"\x1bOD"),
            Self::Home => cursor(b"\x1b[H", b"\x1bOH"),
            Self::End => cursor(b"\x1b[F", b"\x1bOF"),
            Self::PageUp => b"\x1b[5~",
            Self::PageDown => b"\x1b[6~",
            Self::Backspace => b"\x7f",
            Self::Delete => b"\x1b[3~",
            Self::Space => b" ",
            Self::CtrlC => b"\x03",
            Self::CtrlD => b"\x04",
            Self::CtrlL => b"\x0c",
            Self::CtrlZ => b"\x1a",
        }
    }
}

/// Text as a person would paste it. Multi-line text goes in as one bracketed
/// paste when the app asked for that, so a chat TUI does not submit at the
/// first newline.
pub(crate) fn text_bytes(text: &str, bracketed_paste: bool) -> Vec<u8> {
    // Terminals send CR for a typed newline.
    let typed = text.replace("\r\n", "\r").replace('\n', "\r");
    if bracketed_paste && typed.contains('\r') {
        // A paste may not end the bracket early.
        let inner = typed.replace("\x1b[201~", "");
        [b"\x1b[200~".as_slice(), inner.as_bytes(), b"\x1b[201~"].concat()
    } else {
        typed.into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_listed_name_parses() {
        for name in Key::NAMES {
            assert!(Key::parse(name).is_some(), "{name}");
        }
        assert_eq!(Key::parse(" ctrl+C "), Some(Key::CtrlC));
        assert_eq!(Key::parse("F13"), None);
    }

    #[test]
    fn arrows_follow_cursor_key_mode() {
        assert_eq!(Key::Up.bytes(false), b"\x1b[A");
        assert_eq!(Key::Up.bytes(true), b"\x1bOA");
        assert_eq!(Key::Enter.bytes(true), b"\r");
    }

    #[test]
    fn multi_line_text_is_one_bracketed_paste_when_the_app_asks() {
        assert_eq!(text_bytes("a\nb", false), b"a\rb");
        assert_eq!(text_bytes("a\nb", true), b"\x1b[200~a\rb\x1b[201~");
        assert_eq!(text_bytes("one line", true), b"one line");
        assert_eq!(text_bytes("x\n\x1b[201~y", true), b"\x1b[200~x\ry\x1b[201~");
    }
}
