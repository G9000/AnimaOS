//! Importing Claude Code and Codex conversations. Logs are only ever read. A conversation becomes
//! an ordinary chat session of an agent (`SessionOrigin::Import`) whose
//! transcript is the imported turns; continuing it is a normal run in that
//! session.
//!
//! Per-message timestamps are kept, imports are
//! capped at [`MAX_IMPORT_MESSAGES`] newest messages of at most
//! [`MAX_MESSAGE_BYTES`] each, secrets are redacted, injected wrappers are
//! removed per block instead of dropping a whole turn, and Codex sub-agent
//! threads are not listed.

mod parse;
mod redact;
#[cfg(test)]
mod tests;
mod walk;

use std::fs::File;
use std::io::{BufReader, Read};

use anima_core::{Content, Message, MessageRole};

use crate::sessions::{derived_title, labelled_title, truncate_chars, ImportedFrom};

pub(crate) use parse::{ParsedSession, TurnRole};
pub(crate) use redact::redact_secrets;
pub(crate) use walk::{Candidate, Roots};

/// Logs larger than this are neither previewed nor imported.
pub(crate) const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;
/// Sessions per list page.
pub(crate) const PAGE_SIZE: usize = 50;
/// Messages per import, the newest kept.
pub(crate) const MAX_IMPORT_MESSAGES: usize = 2000;
/// Bytes per imported message.
pub(crate) const MAX_MESSAGE_BYTES: usize = 16 * 1024;
/// The newest messages a preview shows.
pub(crate) const PREVIEW_MESSAGES: usize = 40;
/// Characters per preview message.
pub(crate) const PREVIEW_TEXT_CHARS: usize = 2000;
/// The first message of an import that left older messages out.
pub(crate) const EARLIER_MESSAGES_NOTE: &str = "[Earlier messages not imported]";
/// How much of a log a list row reads for its title, folder, and id.
const HEAD_SCAN_BYTES: u64 = 1024 * 1024;
const LIST_TITLE_CHARS: usize = 120;
const MAX_CWD_CHARS: usize = 1024;
const MAX_FOREIGN_ID_CHARS: usize = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum ForeignSource {
    ClaudeCode,
    Codex,
}

impl ForeignSource {
    pub(crate) const ALL: [Self; 2] = [Self::ClaudeCode, Self::Codex];

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "claude-code" | "claude" => Some(Self::ClaudeCode),
            "codex" | "codex-cli" => Some(Self::Codex),
            _ => None,
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::Codex => "Codex",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ForeignError {
    /// No log has this handle (any more).
    NotFound,
    /// The log exceeds [`MAX_FILE_BYTES`].
    TooLarge,
    /// The log holds no readable conversation turn.
    Empty,
}

/// One row of the session list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ListedSession {
    pub(crate) handle: String,
    pub(crate) source: ForeignSource,
    pub(crate) title: String,
    pub(crate) cwd: Option<String>,
    pub(crate) modified_ms: u64,
    pub(crate) size_bytes: u64,
    pub(crate) foreign_session_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ListPage {
    pub(crate) sessions: Vec<ListedSession>,
    pub(crate) next_offset: Option<usize>,
}

/// One page of logs, newest first, from `offset`. Logs are paged before
/// they are read, and each row reads at most the first MiB of its log, so a
/// page never scans whole transcripts. Empty logs and Codex sub-agent
/// threads are left out of their page.
pub(crate) fn list_page(roots: &Roots, source: Option<ForeignSource>, offset: usize) -> ListPage {
    let (candidates, _) = walk::cached_walk(roots, false);
    let matching = candidates
        .iter()
        .filter(|candidate| source.is_none_or(|source| candidate.source == source))
        .collect::<Vec<_>>();
    let sessions = matching
        .iter()
        .skip(offset)
        .take(PAGE_SIZE)
        .filter_map(|candidate| list_row(candidate))
        .collect();
    let next = offset.saturating_add(PAGE_SIZE);
    ListPage {
        sessions,
        next_offset: (next < matching.len()).then_some(next),
    }
}

fn list_row(candidate: &Candidate) -> Option<ListedSession> {
    let file = File::open(&candidate.path).ok()?;
    let complete = candidate.size_bytes <= HEAD_SCAN_BYTES;
    let (parsed, _) =
        parse::parse_session(candidate.source, BufReader::new(file.take(HEAD_SCAN_BYTES)));
    if parsed.subagent || (complete && parsed.turns.is_empty()) {
        return None;
    }
    Some(ListedSession {
        handle: candidate.handle.clone(),
        source: candidate.source,
        title: display_title(&parsed, candidate.source),
        cwd: bounded_cwd(&parsed),
        modified_ms: candidate.modified_ms,
        size_bytes: candidate.size_bytes,
        foreign_session_id: bounded_foreign_id(&parsed),
    })
}

/// The log with this handle. Handles name only logs inside the roots; a
/// handle missing from a recent walk is looked up in a fresh one.
pub(crate) fn resolve(roots: &Roots, handle: &str) -> Result<Candidate, ForeignError> {
    if handle.len() != 64
        || !handle
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(ForeignError::NotFound);
    }
    let find = |candidates: &[Candidate]| {
        candidates
            .iter()
            .find(|candidate| candidate.handle == handle)
            .cloned()
    };
    let (candidates, walked) = walk::cached_walk(roots, false);
    if let Some(found) = find(&candidates) {
        return Ok(found);
    }
    if walked {
        return Err(ForeignError::NotFound);
    }
    let (candidates, _) = walk::cached_walk(roots, true);
    find(&candidates).ok_or(ForeignError::NotFound)
}

/// The whole log, parsed, when it is within [`MAX_FILE_BYTES`] and holds a
/// conversation.
pub(crate) fn load(candidate: &Candidate) -> Result<ParsedSession, ForeignError> {
    let file = File::open(&candidate.path).map_err(|_| ForeignError::NotFound)?;
    let metadata = file.metadata().map_err(|_| ForeignError::NotFound)?;
    if !metadata.is_file() {
        return Err(ForeignError::NotFound);
    }
    if metadata.len() > MAX_FILE_BYTES {
        return Err(ForeignError::TooLarge);
    }
    let (parsed, read) = parse::parse_session(
        candidate.source,
        BufReader::new(file.take(MAX_FILE_BYTES + 1)),
    );
    if read > MAX_FILE_BYTES {
        return Err(ForeignError::TooLarge);
    }
    if parsed.turns.is_empty() {
        return Err(ForeignError::Empty);
    }
    Ok(parsed)
}

/// The conversation's own title, one redacted line of at most 120 characters.
pub(crate) fn display_title(parsed: &ParsedSession, source: ForeignSource) -> String {
    parsed
        .title_guess
        .as_deref()
        .map(redact_secrets)
        .and_then(|title| {
            let line = title.split_whitespace().collect::<Vec<_>>().join(" ");
            let line = line
                .chars()
                .filter(|character| !character.is_control())
                .collect::<String>();
            (!line.is_empty()).then(|| truncate_chars(&line, LIST_TITLE_CHARS))
        })
        .unwrap_or_else(|| format!("{} session", source.label()))
}

/// The imported session's title: `Imported · <title, at most 60 characters>`.
pub(crate) fn import_title(parsed: &ParsedSession) -> String {
    labelled_title(
        "Imported",
        parsed
            .title_guess
            .as_deref()
            .map(redact_secrets)
            .as_deref()
            .and_then(derived_title),
        "Imported conversation",
    )
}

/// Where the session came from, for its record and for deduplication.
pub(crate) fn provenance(candidate: &Candidate, parsed: &ParsedSession) -> ImportedFrom {
    ImportedFrom {
        tool: candidate.source.as_str().to_string(),
        foreign_session_id: bounded_foreign_id(parsed),
        path_hash: candidate.handle.clone(),
        cwd: bounded_cwd(parsed),
    }
}

fn bounded_cwd(parsed: &ParsedSession) -> Option<String> {
    parsed
        .cwd
        .as_deref()
        .filter(|cwd| !cwd.chars().any(char::is_control))
        .map(|cwd| truncate_chars(cwd, MAX_CWD_CHARS))
}

fn bounded_foreign_id(parsed: &ParsedSession) -> Option<String> {
    parsed
        .foreign_session_id
        .as_deref()
        .filter(|id| {
            id.chars().count() <= MAX_FOREIGN_ID_CHARS && !id.chars().any(char::is_control)
        })
        .map(str::to_string)
}

/// One message as it will be stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ImportMessage {
    pub(crate) role: TurnRole,
    pub(crate) text: String,
    /// Never decreasing, and before the import itself.
    pub(crate) at_ms: u64,
    /// The log recorded this turn's time (otherwise `at_ms` is borrowed
    /// from a neighbour).
    pub(crate) timed: bool,
}

/// The turns as they will be stored: at most [`MAX_IMPORT_MESSAGES`], the
/// newest, after an [`EARLIER_MESSAGES_NOTE`] when older ones were left out;
/// each redacted and cut to [`MAX_MESSAGE_BYTES`]; with non-decreasing
/// times before `now_ms`. Roles still alternate, starting with the user.
pub(crate) fn prepare_messages(parsed: &ParsedSession, now_ms: u64) -> Vec<ImportMessage> {
    let turns = &parsed.turns;
    let mut kept = turns.as_slice();
    let mut note = None;
    if turns.len() > MAX_IMPORT_MESSAGES {
        let mut start = turns.len() - (MAX_IMPORT_MESSAGES - 1);
        if turns[start].role == TurnRole::User {
            start += 1;
        }
        kept = &turns[start..];
        note = Some(parse::ForeignTurn {
            role: TurnRole::User,
            text: EARLIER_MESSAGES_NOTE.to_string(),
            at_ms: kept.first().and_then(|turn| turn.at_ms),
        });
    }
    let ceiling = now_ms.saturating_sub(1).max(1);
    let first_known = note
        .iter()
        .chain(kept)
        .find_map(|turn| turn.at_ms)
        .unwrap_or(ceiling);
    let mut last = 0u64;
    note.iter()
        .chain(kept)
        .map(|turn| {
            let fallback = if last == 0 { first_known } else { last };
            let at_ms = turn.at_ms.unwrap_or(fallback).max(last).min(ceiling);
            last = at_ms;
            ImportMessage {
                role: turn.role,
                text: clean_text(&turn.text),
                at_ms,
                timed: turn.at_ms.is_some(),
            }
        })
        .collect()
}

/// Redacted and cut to [`MAX_MESSAGE_BYTES`] with a trailing `…`. Only a
/// bounded prefix is redacted, since the rest is cut anyway.
fn clean_text(text: &str) -> String {
    let input_cut = text.len() > MAX_MESSAGE_BYTES * 4;
    let redacted = redact_secrets(&text[..floor_boundary(text, MAX_MESSAGE_BYTES * 4)]);
    if redacted.len() <= MAX_MESSAGE_BYTES && !input_cut {
        return redacted;
    }
    let end = floor_boundary(&redacted, MAX_MESSAGE_BYTES - '…'.len_utf8());
    format!("{}…", redacted[..end].trim_end())
}

/// The largest char boundary at most `max_bytes` into `text`.
fn floor_boundary(text: &str, max_bytes: usize) -> usize {
    let mut end = max_bytes.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    end
}

/// The transcript messages of an import into `room_id`. Ids follow the
/// runtime's `msg-<ms>-<n>` shape, so their ordinal keeps the import order
/// among messages of the same millisecond.
pub(crate) fn transcript_messages(
    agent_id: &str,
    room_id: &str,
    prepared: &[ImportMessage],
    now_ms: u64,
) -> Vec<Message> {
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let nonce = &nonce[..12];
    prepared
        .iter()
        .enumerate()
        .map(|(index, message)| Message {
            id: format!("msg-{now_ms}-imp{nonce}-{index}"),
            agent_id: agent_id.to_string(),
            room_id: room_id.to_string(),
            content: Content {
                text: message.text.clone(),
                ..Content::default()
            },
            role: match message.role {
                TurnRole::User => MessageRole::User,
                TurnRole::Assistant => MessageRole::Assistant,
            },
            created_at_ms: message.at_ms,
        })
        .collect()
}

/// Where the hot transcript of an import starts: everything when the
/// history store keeps nothing across restarts, else the newest
/// `HOT_TAIL_MESSAGES`, starting at a user turn. The older messages go to
/// the history store directly, as if pruned.
pub(crate) fn hot_start(messages: &[Message], ephemeral_store: bool) -> usize {
    if ephemeral_store || messages.len() <= crate::sessions::pruning::HOT_TAIL_MESSAGES {
        return 0;
    }
    let mut start = messages.len() - crate::sessions::pruning::HOT_TAIL_MESSAGES;
    while start < messages.len() && messages[start].role != MessageRole::User {
        start += 1;
    }
    start
}
