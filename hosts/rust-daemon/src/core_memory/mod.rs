//! Core memory: two small always-on blocks per agent, "About you" (the owner's
//! profile) and "Agent notes" (what the agent learned about its work, its
//! environment and itself). Entries are private `Fact`s tagged `core:user` or
//! `core:notes` in the ordinary memory store, so history, export and scanning
//! apply to them. The agent edits them through the `core_memory` tool, the owner
//! through the console, and reviews through proposals; all share `apply_edit`.

use anima_memory::{
    Memory, MemoryManager, MemoryPatch, MemoryScope, MemoryType, NewMemory, RecentMemoryOptions,
};

use crate::memory_text::{has_hidden_text, MEMORY_TEXT_HIDDEN};
use crate::threat_scan::{blocking_ids, refuse_flagged};

/// Characters (not bytes) all "About you" entries may use together.
pub(crate) const USER_BUDGET: usize = 1_500;
/// Characters (not bytes) all "Agent notes" entries may use together.
pub(crate) const NOTES_BUDGET: usize = 2_500;
/// Characters (not bytes) one entry may use.
pub(crate) const ENTRY_LIMIT: usize = 500;

const USER_TAG: &str = "core:user";
const NOTES_TAG: &str = "core:notes";
/// Core entries are always rendered; importance only affects ordinary recall.
const ENTRY_IMPORTANCE: f64 = 0.9;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CoreBlock {
    User,
    Notes,
}

impl CoreBlock {
    /// The tool and route spelling: `user` or `notes`.
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "user" => Some(Self::User),
            "notes" => Some(Self::Notes),
            _ => None,
        }
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Notes => "notes",
        }
    }

    pub(crate) const fn tag(self) -> &'static str {
        match self {
            Self::User => USER_TAG,
            Self::Notes => NOTES_TAG,
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::User => "About you",
            Self::Notes => "Agent notes",
        }
    }

    pub(crate) const fn budget(self) -> usize {
        match self {
            Self::User => USER_BUDGET,
            Self::Notes => NOTES_BUDGET,
        }
    }
}

/// One edit. The agent's edits use Hermes's semantics: `old_text` is a unique
/// substring of one entry in the block. The owner's console names the entry by
/// id instead.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CoreEdit {
    Add { text: String },
    Replace { old_text: String, text: String },
    Remove { old_text: String },
    ReplaceEntry { id: String, text: String },
    RemoveEntry { id: String },
}

/// What a scanner hit on new text does. The agent's tool and review edits
/// refuse; owner edits from the console apply and carry the ids as warnings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScanMode {
    Refuse,
    Warn,
}

/// The agent whose blocks an edit changes.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CoreAgent<'a> {
    pub(crate) id: &'a str,
    pub(crate) name: &'a str,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum CoreChange {
    Added(Memory),
    /// An exact duplicate `add`: nothing changed.
    AlreadyPresent(Memory),
    Replaced {
        previous: Memory,
        current: Memory,
    },
    /// The full removed record; `restore_entry` puts it back.
    Removed(Memory),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CoreOutcome {
    pub(crate) block: CoreBlock,
    pub(crate) change: CoreChange,
    /// The block's entries after the edit, in order.
    pub(crate) entries: Vec<Memory>,
    /// Scanner ids on the new text; only `ScanMode::Warn` returns any.
    pub(crate) warnings: Vec<String>,
}

impl CoreOutcome {
    /// Whether the store changed and needs persisting.
    pub(crate) fn changed(&self) -> bool {
        !matches!(self.change, CoreChange::AlreadyPresent(_))
    }

    pub(crate) fn used(&self) -> usize {
        used(&self.entries)
    }

    pub(crate) fn summary(&self) -> String {
        let label = self.block.label();
        match self.change {
            CoreChange::Added(_) => format!("Added an entry to {label}."),
            CoreChange::AlreadyPresent(_) => {
                format!("{label} already has that entry; nothing changed.")
            }
            CoreChange::Replaced { .. } => format!("Replaced an entry in {label}."),
            CoreChange::Removed(_) => format!("Removed an entry from {label}."),
        }
    }
}

/// Which facts may be core entries: the agent's own private Facts outside any
/// room, world or session, with visible text and sane bounds. This was the
/// memory-view pin rule, so every migrated pin passes it.
pub(crate) fn eligible_entry(memory: &Memory, agent: &str) -> bool {
    memory.agent_id == agent
        && memory.memory_type == MemoryType::Fact
        && memory.scope == MemoryScope::Private
        && memory.room_id.is_none()
        && memory.world_id.is_none()
        && memory.session_id.is_none()
        && valid_id(&memory.id)
        && !memory.content.trim().is_empty()
        && memory.content.len() <= 8000 * 4
        && memory.content.chars().count() <= 8000
        && clean_text(&memory.content)
        && clean_text(&memory.agent_name)
        && memory.importance.is_finite()
        && !memory.tags.as_ref().is_some_and(|tags| {
            tags.len() > 20
                || tags
                    .iter()
                    .any(|tag| !clean_text(tag) || tag.chars().count() > 40)
        })
}

fn valid_id(id: &str) -> bool {
    !id.trim().is_empty() && id.len() <= 256 && clean_text(id) && !id.chars().any(char::is_control)
}

fn clean_text(text: &str) -> bool {
    !has_hidden_text(text)
        && !text
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
}

/// The block a memory belongs to; an entry carrying both tags counts once, as
/// About you.
fn block_of(memory: &Memory) -> Option<CoreBlock> {
    let tags = memory.tags.as_deref()?;
    [CoreBlock::User, CoreBlock::Notes]
        .into_iter()
        .find(|block| tags.iter().any(|tag| tag == block.tag()))
}

/// Whether `memory` is a core entry: it carries a core tag and is eligible
/// for its agent's blocks. Such entries change only through `apply_edit`,
/// which keeps the entry cap and block budgets. A tagged memory that is not
/// eligible is an ordinary memory everywhere (export, edits, retention).
pub(crate) fn is_core_entry(memory: &Memory) -> bool {
    block_of(memory).is_some() && eligible_entry(memory, &memory.agent_id)
}

/// Whether a generic edit (`PATCH /api/memories/{id}`, `memory_update`) that
/// turned `before` into `after`, already applied to `manager`, keeps the core
/// limits. A core-tagged memory its content kept out of the blocks (legacy or
/// foreign data) joins its block once an edit makes it eligible, so it must
/// fit as an `apply_edit` add would: within the entry limit, and the block
/// within its budget with it counted. Edits of entries are refused earlier.
pub(crate) fn generic_edit_fits(manager: &MemoryManager, before: &Memory, after: &Memory) -> bool {
    let Some(block) = block_of(after).filter(|_| !is_core_entry(before) && is_core_entry(after))
    else {
        return true;
    };
    after.content.chars().count() <= ENTRY_LIMIT
        && used(&block_entries(manager, &after.agent_id, block)) <= block.budget()
}

/// The refusal for generic edits (`memory_update`, `memory_consolidate`) of a
/// core entry. Removal stays allowed: a delete cannot break a budget.
pub(crate) const CORE_ENTRY_EDIT_REFUSED: &str =
    "this is a core memory entry; use the core_memory tool to change it";

/// The refusal for the owner's generic memory routes (`PATCH /api/memories/{id}`
/// of a core entry, or a tag that makes a memory one). It names the route
/// that keeps the budgets and never echoes content.
pub(crate) const CORE_ENTRY_ROUTE_REFUSED: &str =
    "core memory entries change only through /api/agents/{agentId}/core-memory";

/// The refusal when a create request carries a core tag.
pub(crate) const CORE_TAG_RESERVED: &str =
    "the core:user and core:notes tags are reserved; add core entries through /api/agents/{agentId}/core-memory";

/// Whether `tags` names a core block (`core:user`, `core:notes`). Compared
/// trimmed and ignoring ASCII case, so a near-spelling cannot slip by.
pub(crate) fn names_core_tag(tags: &[String]) -> bool {
    tags.iter().any(|tag| {
        let tag = tag.trim();
        [CoreBlock::User, CoreBlock::Notes]
            .iter()
            .any(|block| tag.eq_ignore_ascii_case(block.tag()))
    })
}

/// The memory id's trailing counter, so entries created in the same
/// millisecond keep their creation order (`mem-<ms>-9` before `mem-<ms>-10`).
fn id_sequence(id: &str) -> u64 {
    id.rsplit('-')
        .next()
        .and_then(|suffix| suffix.parse().ok())
        .unwrap_or(0)
}

/// Both blocks for `agent_id`, About you first, each in creation order.
pub(crate) fn blocks_for(manager: &MemoryManager, agent_id: &str) -> (Vec<Memory>, Vec<Memory>) {
    let mut entries = manager.get_recent_filtered(
        RecentMemoryOptions {
            agent_id: Some(agent_id.into()),
            limit: Some(usize::MAX),
            ..RecentMemoryOptions::default()
        },
        |memory| memory.agent_id == agent_id && is_core_entry(memory),
    );
    entries.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| id_sequence(&left.id).cmp(&id_sequence(&right.id)))
            .then_with(|| left.id.cmp(&right.id))
    });
    entries
        .into_iter()
        .partition(|memory| block_of(memory) == Some(CoreBlock::User))
}

fn block_entries(manager: &MemoryManager, agent_id: &str, block: CoreBlock) -> Vec<Memory> {
    let (user, notes) = blocks_for(manager, agent_id);
    match block {
        CoreBlock::User => user,
        CoreBlock::Notes => notes,
    }
}

fn used(entries: &[Memory]) -> usize {
    entries
        .iter()
        .map(|memory| memory.content.chars().count())
        .sum()
}

/// `1500` as `1,500`.
fn grouped(value: usize) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

fn full_error(block: CoreBlock, used: usize) -> String {
    format!(
        "{} is full ({}/{} chars). Replace or remove an entry first.",
        block.label(),
        grouped(used),
        grouped(block.budget())
    )
}

/// New entry text: trimmed, visible, within the entry limit, and scanned.
fn checked_text(text: &str, scan: ScanMode) -> Result<(String, Vec<String>), String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("text must not be empty".into());
    }
    // Hidden or control characters would make the entry ineligible, and so
    // invisible yet still counted; refuse them even for owner edits.
    if !clean_text(text) {
        return Err(MEMORY_TEXT_HIDDEN.into());
    }
    let characters = text.chars().count();
    if characters > ENTRY_LIMIT {
        return Err(format!(
            "An entry is at most {} characters (this one is {}).",
            grouped(ENTRY_LIMIT),
            grouped(characters)
        ));
    }
    let warnings = match scan {
        ScanMode::Refuse => {
            refuse_flagged(text)?;
            Vec::new()
        }
        ScanMode::Warn => blocking_ids(text),
    };
    Ok((text.to_owned(), warnings))
}

/// `text` with each whitespace run as one space, as context shows an entry.
fn collapsed(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The one entry containing `old_text`. Both sides are compared
/// whitespace-collapsed, since the agent quotes entries as context shows them.
fn unique_match<'a>(
    entries: &'a [Memory],
    old_text: &str,
    block: CoreBlock,
) -> Result<&'a Memory, String> {
    let old_text = collapsed(old_text);
    if old_text.is_empty() {
        return Err("old_text must not be empty".into());
    }
    let mut matches = entries
        .iter()
        .filter(|memory| collapsed(&memory.content).contains(&old_text));
    match (matches.next(), matches.count()) {
        (None, _) => Err(format!("No entry in {} contains old_text.", block.label())),
        (Some(memory), 0) => Ok(memory),
        (Some(_), others) => Err(format!(
            "old_text matches {} entries in {}; use a longer part of one entry.",
            others + 1,
            block.label()
        )),
    }
}

/// The entry with `id`; only the owner's console targets entries by id.
fn entry_by_id<'a>(
    entries: &'a [Memory],
    id: &str,
    block: CoreBlock,
) -> Result<&'a Memory, String> {
    entries
        .iter()
        .find(|memory| memory.id == id)
        .ok_or_else(|| format!("No such entry in {}.", block.label()))
}

fn replace_entry(
    manager: &mut MemoryManager,
    block: CoreBlock,
    before: usize,
    previous: Memory,
    text: String,
    warnings: Vec<String>,
) -> Result<(CoreChange, Vec<String>), String> {
    let after = before - previous.content.chars().count() + text.chars().count();
    if after > block.budget() {
        return Err(format!(
            "{} would be over its limit ({}/{} chars). Shorten the text or remove an entry first.",
            block.label(),
            grouped(after),
            grouped(block.budget())
        ));
    }
    let current = manager
        .update_memory(
            &previous.id,
            MemoryPatch {
                content: Some(text),
                ..MemoryPatch::default()
            },
        )
        .map_err(|error| error.message().to_owned())?
        .ok_or_else(|| format!("No such entry in {}.", block.label()))?;
    Ok((CoreChange::Replaced { previous, current }, warnings))
}

fn remove_entry(manager: &mut MemoryManager, previous: Memory) -> (CoreChange, Vec<String>) {
    // The ordinary delete: the record leaves the store and its index, and any
    // citation of it is dropped, as for `memory_remove`.
    manager.delete_memory(&previous.id);
    (CoreChange::Removed(previous), Vec::new())
}

/// Applies one edit to `manager` in memory. The caller persists the change
/// (through `MemoryMutation`) when `CoreOutcome::changed` is true. An error
/// leaves `manager` unchanged.
pub(crate) fn apply_edit(
    manager: &mut MemoryManager,
    agent: CoreAgent<'_>,
    block: CoreBlock,
    edit: CoreEdit,
    scan: ScanMode,
) -> Result<CoreOutcome, String> {
    let entries = block_entries(manager, agent.id, block);
    let before = used(&entries);
    let (change, warnings) = match edit {
        CoreEdit::Add { text } => {
            let (text, warnings) = checked_text(&text, scan)?;
            if let Some(existing) = entries.iter().find(|memory| memory.content == text) {
                let change = CoreChange::AlreadyPresent(existing.clone());
                return Ok(CoreOutcome {
                    block,
                    change,
                    entries,
                    warnings,
                });
            }
            if before + text.chars().count() > block.budget() {
                return Err(full_error(block, before));
            }
            let added = manager
                .add(NewMemory {
                    agent_id: agent.id.into(),
                    agent_name: agent.name.into(),
                    memory_type: MemoryType::Fact,
                    content: text,
                    importance: ENTRY_IMPORTANCE,
                    tags: Some(vec![block.tag().into()]),
                    scope: Some(MemoryScope::Private),
                    room_id: None,
                    world_id: None,
                    session_id: None,
                })
                .map_err(|error| error.message().to_owned())?;
            (CoreChange::Added(added), warnings)
        }
        CoreEdit::Replace { old_text, text } => {
            let (text, warnings) = checked_text(&text, scan)?;
            let previous = unique_match(&entries, &old_text, block)?.clone();
            replace_entry(manager, block, before, previous, text, warnings)?
        }
        CoreEdit::ReplaceEntry { id, text } => {
            let (text, warnings) = checked_text(&text, scan)?;
            let previous = entry_by_id(&entries, &id, block)?.clone();
            replace_entry(manager, block, before, previous, text, warnings)?
        }
        CoreEdit::Remove { old_text } => {
            let previous = unique_match(&entries, &old_text, block)?.clone();
            remove_entry(manager, previous)
        }
        CoreEdit::RemoveEntry { id } => {
            let previous = entry_by_id(&entries, &id, block)?.clone();
            remove_entry(manager, previous)
        }
    };
    Ok(CoreOutcome {
        block,
        change,
        entries: block_entries(manager, agent.id, block),
        warnings,
    })
}

/// Undoes a `remove`: puts the removed record back with its id and creation
/// time, so it returns to its old position. Still subject to the block budget.
/// Citations `delete_memory` dropped are not restored.
pub(crate) fn restore_entry(manager: &mut MemoryManager, entry: &Memory) -> Result<(), String> {
    let Some(block) = block_of(entry).filter(|_| eligible_entry(entry, &entry.agent_id)) else {
        return Err("not a core memory entry".into());
    };
    if manager.get(&entry.id).is_some() {
        return Ok(());
    }
    let before = used(&block_entries(manager, &entry.agent_id, block));
    if before + entry.content.chars().count() > block.budget() {
        return Err(full_error(block, before));
    }
    let mut snapshot = manager.snapshot();
    snapshot.memories.push(entry.clone());
    manager.replace_snapshot(snapshot);
    Ok(())
}

/// One block with its usage line, even when empty (tool results). Entries
/// render in order until the next would pass the block's budget (older data
/// can exceed it); the rest are counted, not shown.
pub(crate) fn render_block(block: CoreBlock, entries: &[Memory]) -> String {
    let mut text = format!(
        "## {} [{}/{} chars]\n",
        block.label(),
        grouped(used(entries)),
        grouped(block.budget())
    );
    let mut shown_chars = 0;
    let mut shown = 0;
    for memory in entries {
        let characters = memory.content.chars().count();
        if shown_chars + characters > block.budget() {
            break;
        }
        shown_chars += characters;
        shown += 1;
        text.push_str(&entry_line(memory));
    }
    if shown < entries.len() {
        text.push_str(&format!(
            "- \u{2026} {} more not shown\n",
            entries.len() - shown
        ));
    }
    text
}

/// What a core entry whose text the scanner flags shows instead of its text,
/// or `None` when nothing flags it. Migrated pins and older data were never
/// scanned on write, so every place the model reads an entry scans it again.
pub(crate) fn flagged_marker(content: &str) -> Option<String> {
    let ids = blocking_ids(content);
    (!ids.is_empty()).then(|| format!("[entry hidden: flagged {}]", ids.join(", ")))
}

/// One `- entry` line; a flagged entry shows only its scanner ids.
fn entry_line(memory: &Memory) -> String {
    match flagged_marker(&memory.content) {
        // One line per entry, so an entry cannot forge another heading.
        None => format!("- {}\n", collapsed(&memory.content)),
        Some(marker) => format!("- {marker}\n"),
    }
}

/// Both blocks for run context; an empty block is left out, and two empty
/// blocks render as an empty string.
pub(crate) fn render(user: &[Memory], notes: &[Memory]) -> String {
    [(CoreBlock::User, user), (CoreBlock::Notes, notes)]
        .into_iter()
        .filter(|(_, entries)| !entries.is_empty())
        .map(|(block, entries)| render_block(block, entries))
        .collect()
}

/// The `core_memory` tool description; the numbers come from the budgets above.
pub(crate) fn tool_description() -> String {
    format!(
        "Edit your core memory: two short blocks shown at the top of every conversation. \
         Block user (About you) is the owner's profile; block notes (Agent notes) is what you have learned about your work, your environment and yourself. \
         Use it when the owner says \"remember ...\", corrects you, or states a lasting preference. \
         add appends an entry; replace swaps the one entry containing old_text for text; remove deletes the one entry containing old_text. \
         old_text is a short unique part of one entry. Keep each entry a short standalone statement of at most {} characters. \
         About you holds {} characters and Agent notes {}; when a block is full, replace or remove an entry first. \
         Put detailed or one-off facts in memory_add instead. Edits reach your context on the next run; the result shows the block now.",
        grouped(ENTRY_LIMIT),
        grouped(USER_BUDGET),
        grouped(NOTES_BUDGET)
    )
}

pub(crate) mod commit;
pub(crate) mod files;
pub(crate) mod migration;
#[cfg(test)]
mod tests;
