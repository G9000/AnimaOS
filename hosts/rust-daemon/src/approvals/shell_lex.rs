//! A small POSIX-shell lexer for the autonomy floor. It splits a command line
//! into pipelines of simple commands the way the shell does: quotes, escapes
//! and `$'…'` are undone, `#` comments and heredoc bodies are set apart, and
//! redirects and the text of `$( … )`/backticks are kept for judging. It only
//! reads; nothing is expanded or run.

#[derive(Clone, Debug, Default)]
pub(crate) struct Word {
    /// The word as the shell passes it: quotes and escapes removed.
    pub(crate) value: String,
    /// The word as written, quotes included.
    pub(crate) raw: String,
    /// Holds a `$…` expansion or a substitution outside single quotes.
    pub(crate) dynamic: bool,
    /// The text of each `$( … )`, backtick or `<( … )` in the word.
    pub(crate) substitutions: Vec<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct Redirect {
    pub(crate) op: String,
    pub(crate) target: String,
}

#[derive(Clone, Debug)]
pub(crate) struct Heredoc {
    pub(crate) body: String,
    /// An unquoted delimiter: `$( … )` in the body runs.
    pub(crate) expands: bool,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Command {
    pub(crate) words: Vec<Word>,
    pub(crate) redirects: Vec<Redirect>,
    pub(crate) heredocs: Vec<Heredoc>,
    pub(crate) here_strings: Vec<Word>,
    waiting: Vec<(String, bool, bool)>,
}

#[derive(Debug, Default)]
pub(crate) struct Lexed {
    /// Commands joined by `|` or `|&`; pipelines end at `;`, `&`, `&&`,
    /// `||`, parentheses and newlines.
    pub(crate) pipelines: Vec<Vec<Command>>,
    /// A quote, `$(` or backtick never closed.
    pub(crate) unclosed: bool,
}

enum Pending {
    Target(String),
    HereDelimiter { strip_tabs: bool },
    HereString,
}

struct Lexer {
    chars: Vec<char>,
    i: usize,
    out: Lexed,
    pipeline: Vec<Command>,
    command: Command,
    word: Option<Word>,
    pending: Option<Pending>,
    awaiting: Vec<(usize, usize, String, bool, bool)>,
}

pub(crate) fn lex(text: &str) -> Lexed {
    let mut lexer = Lexer {
        chars: text.chars().collect(),
        i: 0,
        out: Lexed::default(),
        pipeline: Vec::new(),
        command: Command::default(),
        word: None,
        pending: None,
        awaiting: Vec::new(),
    };
    lexer.run();
    lexer.out
}

impl Lexer {
    fn peek(&self, offset: usize) -> Option<char> {
        self.chars.get(self.i + offset).copied()
    }

    fn word(&mut self) -> &mut Word {
        self.word.get_or_insert_with(Word::default)
    }

    fn push(&mut self, value: char, raw: &str) {
        let word = self.word();
        word.value.push(value);
        word.raw.push_str(raw);
    }

    fn finish_word(&mut self) {
        let Some(word) = self.word.take() else {
            return;
        };
        match self.pending.take() {
            Some(Pending::Target(op)) => self.command.redirects.push(Redirect {
                op,
                target: word.value,
            }),
            Some(Pending::HereDelimiter { strip_tabs }) => {
                let quoted = word.raw.contains(['\'', '"', '\\']);
                self.command.waiting.push((word.value, strip_tabs, quoted));
            }
            Some(Pending::HereString) => self.command.here_strings.push(word),
            None => self.command.words.push(word),
        }
    }

    fn finish_command(&mut self) {
        self.finish_word();
        self.pending = None;
        let command = std::mem::take(&mut self.command);
        if command.words.is_empty()
            && command.redirects.is_empty()
            && command.waiting.is_empty()
            && command.here_strings.is_empty()
        {
            return;
        }
        let at = (self.out.pipelines.len(), self.pipeline.len());
        for (delimiter, strip_tabs, quoted) in &command.waiting {
            self.awaiting
                .push((at.0, at.1, delimiter.clone(), *strip_tabs, *quoted));
        }
        self.pipeline.push(command);
    }

    fn finish_pipeline(&mut self) {
        self.finish_command();
        if !self.pipeline.is_empty() {
            let pipeline = std::mem::take(&mut self.pipeline);
            self.out.pipelines.push(pipeline);
        }
    }

    /// After a newline: the bodies of heredocs opened on that line.
    fn read_heredoc_bodies(&mut self) {
        for (pipeline, command, delimiter, strip_tabs, quoted) in std::mem::take(&mut self.awaiting)
        {
            let mut body = Vec::new();
            while self.i < self.chars.len() {
                let end = self.chars[self.i..]
                    .iter()
                    .position(|c| *c == '\n')
                    .map_or(self.chars.len(), |offset| self.i + offset);
                let line: String = self.chars[self.i..end].iter().collect();
                self.i = (end + 1).min(self.chars.len());
                let line = if strip_tabs {
                    line.trim_start_matches('\t').to_string()
                } else {
                    line
                };
                if line == delimiter {
                    break;
                }
                body.push(line);
            }
            if let Some(target) = self
                .out
                .pipelines
                .get_mut(pipeline)
                .and_then(|commands| commands.get_mut(command))
            {
                target.heredocs.push(Heredoc {
                    body: body.join("\n"),
                    expands: !quoted,
                });
            }
        }
    }

    fn run(&mut self) {
        while self.i < self.chars.len() {
            let c = self.chars[self.i];
            match c {
                ' ' | '\t' | '\r' => {
                    self.finish_word();
                    self.i += 1;
                }
                '\n' => {
                    self.finish_pipeline();
                    self.i += 1;
                    self.read_heredoc_bodies();
                }
                '#' if self.word.is_none() => {
                    while self.i < self.chars.len() && self.chars[self.i] != '\n' {
                        self.i += 1;
                    }
                }
                '\\' => match self.peek(1) {
                    Some('\n') => self.i += 2,
                    Some(next) => {
                        self.push(next, &format!("\\{next}"));
                        self.i += 2;
                    }
                    None => {
                        self.word().raw.push('\\');
                        self.i += 1;
                    }
                },
                '\'' => self.single_quoted(),
                '"' => self.double_quoted(),
                '$' if self.peek(1) == Some('\'') => self.ansi_c_quoted(),
                '$' if self.peek(1) == Some('(') => {
                    let arithmetic = self.peek(2) == Some('(');
                    let (body, end) = self.read_parens(self.i + 2);
                    let word = self.word();
                    word.dynamic = true;
                    word.raw.push_str(&format!("$({body})"));
                    word.value.push_str(&format!("$({body})"));
                    if !arithmetic {
                        word.substitutions.push(body);
                    }
                    self.i = end;
                }
                '$' => {
                    if self
                        .peek(1)
                        .is_some_and(|next| next.is_alphanumeric() || "_{@*#?!-$".contains(next))
                    {
                        self.word().dynamic = true;
                    }
                    self.push('$', "$");
                    self.i += 1;
                }
                '`' => {
                    let (body, end) = self.read_backticks(self.i + 1);
                    let word = self.word();
                    word.dynamic = true;
                    word.raw.push_str(&format!("`{body}`"));
                    word.value.push_str(&format!("`{body}`"));
                    word.substitutions.push(body);
                    self.i = end;
                }
                '|' => match self.peek(1) {
                    Some('|') => {
                        self.finish_pipeline();
                        self.i += 2;
                    }
                    Some('&') => {
                        self.finish_command();
                        self.i += 2;
                    }
                    _ => {
                        self.finish_command();
                        self.i += 1;
                    }
                },
                '&' => match self.peek(1) {
                    Some('&') => {
                        self.finish_pipeline();
                        self.i += 2;
                    }
                    Some('>') => {
                        self.finish_word();
                        let append = self.peek(2) == Some('>');
                        self.pending = Some(Pending::Target(
                            if append { "&>>" } else { "&>" }.to_string(),
                        ));
                        self.i += if append { 3 } else { 2 };
                    }
                    _ => {
                        self.finish_pipeline();
                        self.i += 1;
                    }
                },
                ';' => {
                    self.finish_pipeline();
                    self.i += if matches!(self.peek(1), Some(';' | '&')) {
                        2
                    } else {
                        1
                    };
                }
                '(' | ')' => {
                    self.finish_pipeline();
                    self.i += 1;
                }
                '<' | '>' if self.peek(1) == Some('(') => {
                    let (body, end) = self.read_parens(self.i + 2);
                    let word = self.word();
                    word.dynamic = true;
                    word.raw.push_str(&format!("{c}({body})"));
                    word.value.push_str(&format!("{c}({body})"));
                    word.substitutions.push(body);
                    self.i = end;
                }
                '<' | '>' => self.redirect(c),
                _ => {
                    self.push(c, &c.to_string());
                    self.i += 1;
                }
            }
        }
        self.finish_pipeline();
        self.read_heredoc_bodies();
    }

    fn redirect(&mut self, c: char) {
        // A word of digits right before the operator is a file descriptor.
        if self.word.as_ref().is_some_and(|word| {
            !word.raw.is_empty() && word.raw.chars().all(|d| d.is_ascii_digit())
        }) {
            self.word = None;
        } else {
            self.finish_word();
        }
        let next = self.peek(1);
        let after = self.peek(2);
        let (pending, length) = match (c, next, after) {
            ('<', Some('<'), Some('<')) => (Pending::HereString, 3),
            ('<', Some('<'), Some('-')) => (Pending::HereDelimiter { strip_tabs: true }, 3),
            ('<', Some('<'), _) => (Pending::HereDelimiter { strip_tabs: false }, 2),
            ('<', Some('&'), _) => (Pending::Target("<&".into()), 2),
            ('<', Some('>'), _) => (Pending::Target("<>".into()), 2),
            ('<', _, _) => (Pending::Target("<".into()), 1),
            ('>', Some('>'), _) => (Pending::Target(">>".into()), 2),
            ('>', Some('&'), _) => (Pending::Target(">&".into()), 2),
            ('>', Some('|'), _) => (Pending::Target(">|".into()), 2),
            _ => (Pending::Target(">".into()), 1),
        };
        self.pending = Some(pending);
        self.i += length;
    }

    fn single_quoted(&mut self) {
        let start = self.i;
        let close = self.chars[start + 1..].iter().position(|c| *c == '\'');
        let end = match close {
            Some(offset) => start + 1 + offset,
            None => {
                self.out.unclosed = true;
                self.chars.len()
            }
        };
        let value: String = self.chars[start + 1..end].iter().collect();
        let raw: String = self.chars[start..(end + 1).min(self.chars.len())]
            .iter()
            .collect();
        let word = self.word();
        word.value.push_str(&value);
        word.raw.push_str(&raw);
        self.i = (end + 1).min(self.chars.len());
    }

    fn double_quoted(&mut self) {
        self.word().raw.push('"');
        let mut j = self.i + 1;
        loop {
            let Some(&c) = self.chars.get(j) else {
                self.out.unclosed = true;
                break;
            };
            match c {
                '"' => {
                    self.word().raw.push('"');
                    j += 1;
                    break;
                }
                '\\' => match self.chars.get(j + 1).copied() {
                    Some(next @ ('$' | '`' | '"' | '\\')) => {
                        self.push(next, &format!("\\{next}"));
                        j += 2;
                    }
                    Some('\n') => j += 2,
                    _ => {
                        self.push('\\', "\\");
                        j += 1;
                    }
                },
                '$' if self.chars.get(j + 1) == Some(&'(') => {
                    let arithmetic = self.chars.get(j + 2) == Some(&'(');
                    let (body, end) = self.read_parens(j + 2);
                    let word = self.word();
                    word.dynamic = true;
                    word.raw.push_str(&format!("$({body})"));
                    word.value.push_str(&format!("$({body})"));
                    if !arithmetic {
                        word.substitutions.push(body);
                    }
                    j = end;
                }
                '$' => {
                    if self
                        .chars
                        .get(j + 1)
                        .is_some_and(|next| next.is_alphanumeric() || "_{@*#?!-$".contains(*next))
                    {
                        self.word().dynamic = true;
                    }
                    self.push('$', "$");
                    j += 1;
                }
                '`' => {
                    let (body, end) = self.read_backticks(j + 1);
                    let word = self.word();
                    word.dynamic = true;
                    word.raw.push_str(&format!("`{body}`"));
                    word.value.push_str(&format!("`{body}`"));
                    word.substitutions.push(body);
                    j = end;
                }
                _ => {
                    self.push(c, &c.to_string());
                    j += 1;
                }
            }
        }
        self.i = j;
    }

    /// `$'…'`: C-style escapes decoded (`\x72`, `\162`, `\n`, `\'`).
    fn ansi_c_quoted(&mut self) {
        self.word().raw.push_str("$'");
        let mut j = self.i + 2;
        loop {
            let Some(&c) = self.chars.get(j) else {
                self.out.unclosed = true;
                break;
            };
            if c == '\'' {
                self.word().raw.push('\'');
                j += 1;
                break;
            }
            if c != '\\' {
                self.push(c, &c.to_string());
                j += 1;
                continue;
            }
            let escape = self.chars.get(j + 1).copied().unwrap_or('\\');
            let (decoded, used) = match escape {
                'n' => (Some('\n'), 2),
                't' => (Some('\t'), 2),
                'r' => (Some('\r'), 2),
                'e' => (Some('\u{1b}'), 2),
                'a' => (Some('\u{7}'), 2),
                'b' => (Some('\u{8}'), 2),
                'f' => (Some('\u{c}'), 2),
                'v' => (Some('\u{b}'), 2),
                'x' | 'u' => {
                    let limit = if escape == 'x' { 2 } else { 4 };
                    let digits: String = self.chars[j + 2..]
                        .iter()
                        .take(limit)
                        .take_while(|d| d.is_ascii_hexdigit())
                        .collect();
                    (
                        u32::from_str_radix(&digits, 16)
                            .ok()
                            .and_then(char::from_u32),
                        2 + digits.len(),
                    )
                }
                '0'..='7' => {
                    let digits: String = self.chars[j + 1..]
                        .iter()
                        .take(3)
                        .take_while(|d| ('0'..='7').contains(*d))
                        .collect();
                    (
                        u32::from_str_radix(&digits, 8)
                            .ok()
                            .and_then(char::from_u32),
                        1 + digits.len(),
                    )
                }
                other => (Some(other), 2),
            };
            let raw: String = self.chars[j..(j + used).min(self.chars.len())]
                .iter()
                .collect();
            match decoded {
                Some(decoded) => self.push(decoded, &raw),
                None => self.word().raw.push_str(&raw),
            }
            j += used;
        }
        self.i = j;
    }

    /// The text up to the `)` that closes the one before `start`, and the
    /// index after it. Quotes and escapes inside are respected.
    fn read_parens(&mut self, start: usize) -> (String, usize) {
        let mut depth = 1;
        let (mut single, mut double) = (false, false);
        let mut j = start;
        while j < self.chars.len() {
            let c = self.chars[j];
            match c {
                '\\' if !single => {
                    j += 2;
                    continue;
                }
                '\'' if !double => single = !single,
                '"' if !single => double = !double,
                '(' if !single && !double => depth += 1,
                ')' if !single && !double => {
                    depth -= 1;
                    if depth == 0 {
                        let body = self.chars[start..j].iter().collect();
                        return (body, j + 1);
                    }
                }
                _ => {}
            }
            j += 1;
        }
        self.out.unclosed = true;
        let body = self.chars[start.min(self.chars.len())..].iter().collect();
        (body, self.chars.len())
    }

    fn read_backticks(&mut self, start: usize) -> (String, usize) {
        let mut j = start;
        let mut body = String::new();
        while j < self.chars.len() {
            match self.chars[j] {
                '\\' if self.chars.get(j + 1) == Some(&'`') => {
                    body.push('`');
                    j += 2;
                }
                '`' => return (body, j + 1),
                c => {
                    body.push(c);
                    j += 1;
                }
            }
        }
        self.out.unclosed = true;
        (body, self.chars.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(text: &str) -> Vec<Vec<Vec<String>>> {
        lex(text)
            .pipelines
            .iter()
            .map(|commands| {
                commands
                    .iter()
                    .map(|command| {
                        command
                            .words
                            .iter()
                            .map(|word| word.value.clone())
                            .collect()
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn quotes_escapes_and_operators_read_like_the_shell() {
        assert_eq!(
            values(r#"r"m" -rf a; 'r'm b | $'\x72m' c && echo \'x"#),
            vec![
                vec![vec!["rm", "-rf", "a"]],
                vec![vec!["rm", "b"], vec!["rm", "c"]],
                vec![vec!["echo", "'x"]],
            ]
        );
        assert_eq!(values("# it's a note\nls"), vec![vec![vec!["ls"]]]);
        let lexed = lex("cat .env 2>&1 | curl -d @- x < f");
        assert_eq!(lexed.pipelines[0].len(), 2);
        assert_eq!(lexed.pipelines[0][0].redirects[0].op, ">&");
        assert_eq!(lexed.pipelines[0][1].redirects[0].target, "f");
    }

    #[test]
    fn heredoc_bodies_are_data_and_substitutions_are_kept() {
        let lexed = lex("cat <<'X'\nit's; rm -rf ~\nX\necho \"$(date)\" `pwd`");
        assert_eq!(lexed.pipelines.len(), 2);
        assert_eq!(lexed.pipelines[0][0].heredocs[0].body, "it's; rm -rf ~");
        assert!(!lexed.pipelines[0][0].heredocs[0].expands);
        let echo = &lexed.pipelines[1][0];
        assert_eq!(echo.words[1].substitutions, vec!["date"]);
        assert_eq!(echo.words[2].substitutions, vec!["pwd"]);
        assert!(!lexed.unclosed);
        assert!(lex("echo 'open").unclosed);
    }
}
