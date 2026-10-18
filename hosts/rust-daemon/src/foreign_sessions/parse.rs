//! Parsers for Claude Code and Codex session logs (JSONL, one record per
//! line). Unparseable lines are skipped, so one bad record never hides the
//! rest; tool results, thinking, and reasoning are dropped, and injected
//! context wrappers in user turns are removed.

use std::io::BufRead;

use serde_json::{Map, Value};

use super::ForeignSource;

/// User-text wrappers the tools inject around context; never typed input.
const WRAPPER_TAGS: &[&str] = &[
    "user_instructions",
    "environment_context",
    "recommended_plugins",
    "skills_instructions",
    "permissions_instructions",
    "permissions-instructions",
    "turn_context",
    "turn_aborted",
    "command-name",
    "command-message",
    "command-args",
    "local-command-stdout",
    "local-command-stderr",
    "local-command-caveat",
    "system-reminder",
];
/// Codex injects the project's AGENTS.md as a user message with this header.
const AGENTS_MD_HEADER: &str = "# AGENTS.md instructions for ";
/// The user turn inserted when a log begins with an assistant reply.
pub(crate) const LEADING_ASSISTANT_STUB: &str =
    "(imported conversation begins with an assistant reply)";
/// Timestamps before 2000-01-01 are treated as missing.
const MIN_VALID_TIMESTAMP_MS: i64 = 946_684_800_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TurnRole {
    User,
    Assistant,
}

impl TurnRole {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "user" => Some(Self::User),
            "assistant" => Some(Self::Assistant),
            _ => None,
        }
    }
}

/// One turn of a foreign conversation; `at_ms` is its first record's time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ForeignTurn {
    pub(crate) role: TurnRole,
    pub(crate) text: String,
    pub(crate) at_ms: Option<u64>,
}

/// A parsed log: strictly alternating turns that start with a user turn.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ParsedSession {
    pub(crate) turns: Vec<ForeignTurn>,
    pub(crate) cwd: Option<String>,
    pub(crate) foreign_session_id: Option<String>,
    /// The tool's own title (Claude Code `custom-title` / `summary`), else
    /// the first user line.
    pub(crate) title_guess: Option<String>,
    /// A Codex sub-agent or reviewer thread, not a conversation the owner had.
    pub(crate) subagent: bool,
}

#[derive(Default)]
struct Builder {
    raw: Vec<ForeignTurn>,
    cwd: Option<String>,
    session_id: Option<String>,
    custom_title: Option<String>,
    summary: Option<String>,
    subagent: bool,
}

impl Builder {
    fn push(&mut self, role: TurnRole, text: String, at_ms: Option<u64>) {
        if !text.trim().is_empty() {
            self.raw.push(ForeignTurn { role, text, at_ms });
        }
    }

    fn finish(self) -> ParsedSession {
        let turns = merge_turns(self.raw);
        let title_guess = self
            .custom_title
            .or(self.summary)
            .or_else(|| first_user_title(&turns));
        ParsedSession {
            turns,
            cwd: self.cwd,
            foreign_session_id: self.session_id,
            title_guess,
            subagent: self.subagent,
        }
    }
}

/// Parses a log, reading at most what `reader` yields. Returns the session
/// and the number of bytes read.
pub(crate) fn parse_session(source: ForeignSource, reader: impl BufRead) -> (ParsedSession, u64) {
    let mut builder = Builder::default();
    let read = for_each_record(reader, |record| match source {
        ForeignSource::ClaudeCode => claude_record(&mut builder, record),
        ForeignSource::Codex => codex_record(&mut builder, record),
    });
    (builder.finish(), read)
}

fn for_each_record(mut reader: impl BufRead, mut visit: impl FnMut(&Map<String, Value>)) -> u64 {
    let mut line = Vec::new();
    let mut total = 0u64;
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => break,
            Ok(read) => total += read as u64,
        }
        let mut bytes = line.as_slice();
        if total as usize == line.len() {
            bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
        }
        let bytes = bytes.trim_ascii();
        if bytes.is_empty() {
            continue;
        }
        if let Ok(Value::Object(record)) = serde_json::from_slice::<Value>(bytes) {
            visit(&record);
        }
    }
    total
}

fn string_field<'a>(record: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    record
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn timestamp_ms(record: &Map<String, Value>) -> Option<u64> {
    let text = record.get("timestamp")?.as_str()?;
    let ms = chrono::DateTime::parse_from_rfc3339(text.trim())
        .ok()?
        .timestamp_millis();
    (ms >= MIN_VALID_TIMESTAMP_MS).then_some(ms as u64)
}

fn claude_record(builder: &mut Builder, record: &Map<String, Value>) {
    match record.get("type").and_then(Value::as_str) {
        Some("summary") => {
            if let Some(summary) = string_field(record, "summary") {
                builder.summary = Some(summary.to_string());
            }
        }
        Some("custom-title") => {
            if let Some(title) = string_field(record, "customTitle") {
                builder.custom_title = Some(title.to_string());
            }
        }
        Some("user" | "assistant") => {
            let flagged = |key: &str| record.get(key).and_then(Value::as_bool) == Some(true);
            if flagged("isSidechain") || flagged("isMeta") {
                return;
            }
            if builder.cwd.is_none() {
                builder.cwd = string_field(record, "cwd").map(str::to_string);
            }
            if builder.session_id.is_none() {
                builder.session_id = string_field(record, "sessionId").map(str::to_string);
            }
            let Some(message) = record.get("message").and_then(Value::as_object) else {
                return;
            };
            let Some(role) = message
                .get("role")
                .and_then(Value::as_str)
                .and_then(TurnRole::parse)
            else {
                return;
            };
            let text = flatten_content(role, message.get("content"));
            builder.push(role, text, timestamp_ms(record));
        }
        _ => {}
    }
}

fn codex_record(builder: &mut Builder, record: &Map<String, Value>) {
    let Some(payload) = record.get("payload").and_then(Value::as_object) else {
        return;
    };
    match record.get("type").and_then(Value::as_str) {
        Some("session_meta") => {
            if builder.cwd.is_none() {
                builder.cwd = string_field(payload, "cwd").map(str::to_string);
            }
            if builder.session_id.is_none() {
                builder.session_id = string_field(payload, "session_id")
                    .or_else(|| string_field(payload, "id"))
                    .map(str::to_string);
            }
            let spawned = payload
                .get("source")
                .and_then(Value::as_object)
                .is_some_and(|source| source.contains_key("subagent"))
                || string_field(payload, "parent_thread_id").is_some()
                || matches!(
                    string_field(payload, "thread_source"),
                    Some("subagent" | "guardian_review")
                );
            builder.subagent |= spawned;
        }
        Some("response_item") => match payload.get("type").and_then(Value::as_str) {
            Some("message") => {
                // Developer and system payloads are not conversation turns.
                let Some(role) = payload
                    .get("role")
                    .and_then(Value::as_str)
                    .and_then(TurnRole::parse)
                else {
                    return;
                };
                let text = flatten_content(role, payload.get("content"));
                builder.push(role, text, timestamp_ms(record));
            }
            Some(kind @ ("custom_tool_call" | "function_call" | "local_shell_call")) => {
                let name = string_field(payload, "name")
                    .or_else(|| string_field(payload, "tool"))
                    .unwrap_or(if kind == "local_shell_call" {
                        "shell"
                    } else {
                        "tool"
                    });
                builder.push(
                    TurnRole::Assistant,
                    format!("[ran tool: {name}]"),
                    timestamp_ms(record),
                );
            }
            _ => {}
        },
        _ => {}
    }
}

/// Plain text of a message `content` (a string or a list of blocks).
fn flatten_content(role: TurnRole, content: Option<&Value>) -> String {
    let clean = |text: &str| match role {
        TurnRole::User => strip_wrappers(text),
        TurnRole::Assistant => text.trim().to_string(),
    };
    match content {
        Some(Value::String(text)) => clean(text),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .map(|block| match block {
                Value::String(text) => clean(text),
                Value::Object(block) => match block.get("type").and_then(Value::as_str) {
                    Some("text" | "input_text" | "output_text") => block
                        .get("text")
                        .and_then(Value::as_str)
                        .map_or_else(String::new, clean),
                    Some("tool_use") => {
                        format!(
                            "[ran tool: {}]",
                            string_field(block, "name").unwrap_or("tool")
                        )
                    }
                    Some("image" | "input_image") => "[image]".to_string(),
                    // tool_result, thinking, reasoning, and unknown blocks.
                    _ => String::new(),
                },
                _ => String::new(),
            })
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n"),
        _ => String::new(),
    }
}

/// Removes leading injected wrapper elements (`<system-reminder>…</system-reminder>`
/// and the like) and returns what the owner typed, if anything. An unclosed
/// wrapper or an AGENTS.md injection drops the whole text.
pub(crate) fn strip_wrappers(text: &str) -> String {
    let mut rest = text.trim_start();
    loop {
        if rest.starts_with(AGENTS_MD_HEADER) {
            return String::new();
        }
        let Some(tag) = leading_wrapper_tag(rest) else {
            break;
        };
        let close = format!("</{tag}>");
        match rest.to_ascii_lowercase().find(&close) {
            Some(index) => rest = rest[index + close.len()..].trim_start(),
            None => return String::new(),
        }
    }
    rest.trim_end().to_string()
}

fn leading_wrapper_tag(text: &str) -> Option<String> {
    let after = text.strip_prefix('<')?;
    let name_len = after
        .bytes()
        .take_while(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        .count();
    let boundary = after[name_len..].chars().next();
    if name_len == 0 || !matches!(boundary, Some('>' | '/') | Some(' ' | '\t' | '\n' | '\r')) {
        return None;
    }
    let name = after[..name_len].to_ascii_lowercase();
    WRAPPER_TAGS.contains(&name.as_str()).then_some(name)
}

/// Merges consecutive same-role turns so roles strictly alternate, and puts
/// a user stub before a leading assistant turn.
pub(crate) fn merge_turns(raw: Vec<ForeignTurn>) -> Vec<ForeignTurn> {
    let mut merged: Vec<ForeignTurn> = Vec::new();
    for turn in raw {
        let text = turn.text.trim();
        if text.is_empty() {
            continue;
        }
        match merged.last_mut() {
            Some(last) if last.role == turn.role => {
                last.text.push_str("\n\n");
                last.text.push_str(text);
                if last.at_ms.is_none() {
                    last.at_ms = turn.at_ms;
                }
            }
            _ => merged.push(ForeignTurn {
                role: turn.role,
                text: text.to_string(),
                at_ms: turn.at_ms,
            }),
        }
    }
    for turn in &mut merged {
        turn.text = collapse_tool_lines(&turn.text);
    }
    if merged
        .first()
        .is_some_and(|turn| turn.role == TurnRole::Assistant)
    {
        let at_ms = merged[0].at_ms;
        merged.insert(
            0,
            ForeignTurn {
                role: TurnRole::User,
                text: LEADING_ASSISTANT_STUB.to_string(),
                at_ms,
            },
        );
    }
    merged
}

/// Folds each run of `[ran tool: X]` paragraphs into one line, counting
/// repeats in first-seen order: `[ran tools: Bash ×2, Read]`. MCP tool names
/// keep only their last part (`mcp__server__browser_batch` → `browser_batch`).
pub(crate) fn collapse_tool_lines(text: &str) -> String {
    fn flush(out: &mut Vec<String>, run: &mut Vec<(String, usize)>) {
        let total: usize = run.iter().map(|(_, count)| count).sum();
        match total {
            0 => {}
            1 => out.push(format!("[ran tool: {}]", run[0].0)),
            _ => out.push(format!(
                "[ran tools: {}]",
                run.iter()
                    .map(|(name, count)| if *count > 1 {
                        format!("{name} ×{count}")
                    } else {
                        name.clone()
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        }
        run.clear();
    }
    let mut out = Vec::new();
    let mut run: Vec<(String, usize)> = Vec::new();
    for paragraph in text.split("\n\n") {
        let tool = paragraph
            .trim()
            .strip_prefix("[ran tool: ")
            .and_then(|rest| rest.strip_suffix(']'))
            .filter(|name| !name.is_empty() && !name.contains(']'));
        match tool {
            Some(name) => {
                let name = if name.starts_with("mcp__") {
                    name.rsplit("__").next().unwrap_or(name)
                } else {
                    name
                };
                match run.iter_mut().find(|(known, _)| known == name) {
                    Some((_, count)) => *count += 1,
                    None => run.push((name.to_string(), 1)),
                }
            }
            None => {
                flush(&mut out, &mut run);
                out.push(paragraph.to_string());
            }
        }
    }
    flush(&mut out, &mut run);
    out.join("\n\n")
}

/// The first user turn's request: the line after a `## My request:` heading
/// (Codex attachment envelopes), else its first non-empty line.
fn first_user_title(turns: &[ForeignTurn]) -> Option<String> {
    let text = &turns
        .iter()
        .find(|turn| turn.role == TurnRole::User && turn.text != LEADING_ASSISTANT_STUB)?
        .text;
    let mut lines = text.lines().map(str::trim);
    let after_heading = lines
        .by_ref()
        .position(|line| {
            line.starts_with('#')
                && line
                    .trim_start_matches('#')
                    .trim()
                    .eq_ignore_ascii_case("my request:")
        })
        .and_then(|_| lines.find(|line| !line.is_empty()));
    after_heading
        .or_else(|| text.lines().map(str::trim).find(|line| !line.is_empty()))
        .map(|line| line.trim_start_matches('#').trim().to_string())
        .filter(|line| !line.is_empty())
}
