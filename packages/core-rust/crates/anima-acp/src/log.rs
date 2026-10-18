//! Bounded, cursor-addressed event log. Streamed chunks of one kind merge into
//! one event so a reader sees messages, not tokens.
use crate::AcpEvent;
use std::collections::VecDeque;

/// Total event text kept per session, like app sessions' ring.
pub(crate) const LOG_CAP: usize = 1024 * 1024;
/// A merged event stops growing here, so one long answer cannot be the only
/// thing left in the log.
const MERGE_CAP: usize = 64 * 1024;
/// Any single text is cut to this before it is stored.
const TEXT_CAP: usize = LOG_CAP / 4;

pub(crate) struct EventLog {
    events: VecDeque<Entry>,
    /// Sequence number of `events[0]`.
    first: u64,
    bytes: usize,
    dropped: u64,
    /// Highest cursor handed to a reader. Events below it are never merged
    /// into, so no reader misses appended text.
    delivered: u64,
    /// Message id of the last chunk, when the agent sends ids.
    last_message_id: Option<String>,
}

struct Entry {
    event: AcpEvent,
    weight: usize,
}

impl EventLog {
    pub(crate) fn new() -> Self {
        Self {
            events: VecDeque::new(),
            first: 0,
            bytes: 0,
            dropped: 0,
            delivered: 0,
            last_message_id: None,
        }
    }

    /// Cursor of the next event to be pushed.
    pub(crate) fn next(&self) -> u64 {
        self.first + self.events.len() as u64
    }

    pub(crate) fn dropped(&self) -> u64 {
        self.dropped
    }

    pub(crate) fn push(&mut self, event: AcpEvent) {
        self.push_chunk(event, None);
    }

    /// Pushes `event`, merging it into the previous event when both are text
    /// chunks of the same kind (and the same message id, when given).
    pub(crate) fn push_chunk(&mut self, mut event: AcpEvent, message_id: Option<&str>) {
        truncate_text(&mut event);
        let same_message = match (&self.last_message_id, message_id) {
            (Some(last), Some(id)) => last == id,
            _ => true,
        };
        if is_chunk(&event) {
            self.last_message_id = message_id.map(str::to_owned);
        } else {
            self.last_message_id = None;
        }
        let mergeable = same_message && self.next() > self.delivered;
        if mergeable {
            if let Some(last) = self.events.back_mut() {
                if last.weight + weight(&event) <= MERGE_CAP && merge(&mut last.event, &event) {
                    let added = weight(&event);
                    last.weight += added;
                    self.bytes += added;
                    self.evict();
                    return;
                }
            }
        }
        let weight = weight(&event);
        self.bytes += weight;
        self.events.push_back(Entry { event, weight });
        self.evict();
    }

    /// Events from `cursor` on, and the cursor to pass next time. A cursor
    /// older than the retained window starts at the oldest kept event.
    pub(crate) fn since(&mut self, cursor: u64) -> (Vec<AcpEvent>, u64) {
        let start = cursor.max(self.first);
        let skip = (start - self.first) as usize;
        let events = self
            .events
            .iter()
            .skip(skip)
            .map(|entry| entry.event.clone())
            .collect();
        let next = self.next();
        self.delivered = self.delivered.max(next);
        (events, next)
    }

    fn evict(&mut self) {
        while self.bytes > LOG_CAP && self.events.len() > 1 {
            if let Some(entry) = self.events.pop_front() {
                self.bytes -= entry.weight;
                self.first += 1;
                self.dropped += 1;
            }
        }
    }
}

fn is_chunk(event: &AcpEvent) -> bool {
    matches!(
        event,
        AcpEvent::AgentMessage { .. }
            | AcpEvent::AgentThought { .. }
            | AcpEvent::UserMessage { .. }
    )
}

fn merge(into: &mut AcpEvent, from: &AcpEvent) -> bool {
    match (into, from) {
        (AcpEvent::AgentMessage { text: a }, AcpEvent::AgentMessage { text: b })
        | (AcpEvent::AgentThought { text: a }, AcpEvent::AgentThought { text: b })
        | (AcpEvent::UserMessage { text: a }, AcpEvent::UserMessage { text: b }) => {
            a.push_str(b);
            true
        }
        (AcpEvent::Stderr { text: a }, AcpEvent::Stderr { text: b }) => {
            a.push('\n');
            a.push_str(b);
            true
        }
        _ => false,
    }
}

/// Approximate stored size: the event's free text plus a small fixed cost.
fn weight(event: &AcpEvent) -> usize {
    32 + match event {
        AcpEvent::AgentMessage { text }
        | AcpEvent::AgentThought { text }
        | AcpEvent::UserMessage { text }
        | AcpEvent::Stderr { text }
        | AcpEvent::TurnEnded { stop_reason: text } => text.len(),
        AcpEvent::TurnFailed { message, .. } => message.len(),
        AcpEvent::ToolCall {
            id,
            title,
            locations,
            ..
        } => id.len() + title.len() + paths(locations),
        AcpEvent::ToolCallUpdate {
            id, content_text, ..
        } => id.len() + content_text.as_ref().map_or(0, String::len),
        AcpEvent::Plan { entries } => entries.iter().map(|e| 16 + e.content.len()).sum(),
        AcpEvent::PermissionAsked {
            tool_call_id,
            title,
            options,
            ..
        } => {
            tool_call_id.len()
                + title.len()
                + options
                    .iter()
                    .map(|o| o.id.len() + o.name.len())
                    .sum::<usize>()
        }
        AcpEvent::PermissionAnswered {
            tool_call_id,
            option_id,
        } => tool_call_id.len() + option_id.as_ref().map_or(0, String::len),
        AcpEvent::FileWritten { path } => path.as_os_str().len(),
        AcpEvent::Exited { .. } => 0,
    }
}

fn paths(paths: &[std::path::PathBuf]) -> usize {
    paths.iter().map(|p| p.as_os_str().len()).sum()
}

fn truncate_text(event: &mut AcpEvent) {
    let text = match event {
        AcpEvent::AgentMessage { text }
        | AcpEvent::AgentThought { text }
        | AcpEvent::UserMessage { text }
        | AcpEvent::Stderr { text } => text,
        AcpEvent::ToolCallUpdate {
            content_text: Some(text),
            ..
        } => text,
        _ => return,
    };
    truncate(text, TEXT_CAP);
}

/// Cuts `text` to at most `max` bytes on a char boundary, marking the cut.
pub(crate) fn truncate(text: &mut String, max: usize) {
    if text.len() <= max {
        return;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text.push_str("…[truncated]");
}
