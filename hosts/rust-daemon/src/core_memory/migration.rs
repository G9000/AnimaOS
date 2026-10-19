//! The one-time move of the owner's memory-view pins into core memory. Older
//! control planes hold, per agent, the facts the owner pinned as "user
//! profile" or "agent notes". At boot each pinned fact that still qualifies
//! gets the matching core tag while its block has room; nothing is ever
//! deleted, and a pin that doesn't fit leaves its fact untouched. The control
//! plane is then saved without the pins, so a second boot has nothing to do.

use std::collections::HashSet;

use anima_memory::{MemoryManager, MemoryPatch};
use serde::Deserialize;

use super::{block_of, blocks_for, eligible_entry, grouped, CoreBlock, ENTRY_LIMIT};

/// Tags one memory may carry and still be a core entry (`eligible_entry`).
const MAX_TAGS: usize = 20;

/// One agent's pins as older control planes saved them (`memoryViews`). Read
/// only; the field is never written again.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct LegacyPins {
    pub(crate) agent_id: String,
    pub(crate) user_profile_ids: Vec<String>,
    pub(crate) agent_note_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SkippedPin {
    pub(crate) agent_id: String,
    pub(crate) memory_id: String,
    pub(crate) reason: String,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct PinMigration {
    /// Memory ids that were tagged into a block.
    pub(crate) moved: Vec<String>,
    /// Pins left as they were, with the reason (never the content).
    pub(crate) skipped: Vec<SkippedPin>,
}

/// Tags each still-eligible pinned fact of a live agent `core:user` (profile
/// pins) or `core:notes` (note pins), in pin order, while the block's budget
/// allows. A fact already in its block is left alone, so a repeat run is a
/// no-op. The caller persists when `moved` is not empty.
pub(crate) fn migrate_pins(
    manager: &mut MemoryManager,
    pins: &[LegacyPins],
    live_agents: &HashSet<String>,
) -> PinMigration {
    let mut report = PinMigration::default();
    for row in pins {
        let picks = row
            .user_profile_ids
            .iter()
            .map(|id| (CoreBlock::User, id))
            .chain(row.agent_note_ids.iter().map(|id| (CoreBlock::Notes, id)));
        for (block, id) in picks {
            let skip = |reason: String| SkippedPin {
                agent_id: row.agent_id.clone(),
                memory_id: id.clone(),
                reason,
            };
            if !live_agents.contains(&row.agent_id) {
                report
                    .skipped
                    .push(skip("the agent no longer exists".into()));
                continue;
            }
            match move_pin(manager, &row.agent_id, block, id) {
                Ok(true) => report.moved.push(id.clone()),
                Ok(false) => {}
                Err(reason) => report.skipped.push(skip(reason)),
            }
        }
    }
    report
}

/// `Ok(true)` when the fact was tagged, `Ok(false)` when it already was.
fn move_pin(
    manager: &mut MemoryManager,
    agent_id: &str,
    block: CoreBlock,
    id: &str,
) -> Result<bool, String> {
    let Some(memory) = manager.get(id) else {
        return Err("the fact no longer exists".into());
    };
    if !eligible_entry(&memory, agent_id) {
        return Err("the fact is no longer eligible".into());
    }
    match block_of(&memory) {
        Some(current) if current == block => return Ok(false),
        Some(current) => return Err(format!("the fact is already in {}", current.label())),
        None => {}
    }
    let characters = memory.content.chars().count();
    if characters > ENTRY_LIMIT {
        return Err(format!(
            "the fact is longer than an entry may be ({} characters)",
            grouped(characters)
        ));
    }
    let tags = memory.tags.clone().unwrap_or_default();
    if tags.len() >= MAX_TAGS {
        return Err("the fact has too many tags".into());
    }
    let (user, notes) = blocks_for(manager, agent_id);
    let entries = match block {
        CoreBlock::User => user,
        CoreBlock::Notes => notes,
    };
    if entries
        .iter()
        .any(|entry| entry.content.trim() == memory.content.trim())
    {
        return Err(format!("{} already has that entry", block.label()));
    }
    let used: usize = entries
        .iter()
        .map(|entry| entry.content.chars().count())
        .sum();
    if used + characters > block.budget() {
        return Err(format!(
            "{} has no room ({}/{} chars)",
            block.label(),
            grouped(used),
            grouped(block.budget())
        ));
    }
    let mut tags = tags;
    tags.push(block.tag().into());
    manager
        .update_memory(
            id,
            MemoryPatch {
                tags: Some(Some(tags)),
                ..MemoryPatch::default()
            },
        )
        .map_err(|error| format!("the fact could not be tagged: {}", error.message()))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use anima_memory::{Memory, MemoryScope, MemoryType, NewMemory};

    use super::*;

    const AGENT: &str = "agent-pins";

    fn fact(
        manager: &mut MemoryManager,
        content: &str,
        change: impl FnOnce(&mut NewMemory),
    ) -> Memory {
        let mut memory = NewMemory {
            agent_id: AGENT.into(),
            agent_name: "Anima".into(),
            memory_type: MemoryType::Fact,
            content: content.into(),
            importance: 0.5,
            tags: Some(vec!["saved-from-chat".into()]),
            scope: Some(MemoryScope::Private),
            room_id: None,
            world_id: None,
            session_id: None,
        };
        change(&mut memory);
        manager.add(memory).unwrap()
    }

    fn live() -> HashSet<String> {
        HashSet::from([AGENT.to_string()])
    }

    fn pins(profile: &[&Memory], notes: &[&Memory]) -> Vec<LegacyPins> {
        vec![LegacyPins {
            agent_id: AGENT.into(),
            user_profile_ids: profile.iter().map(|memory| memory.id.clone()).collect(),
            agent_note_ids: notes.iter().map(|memory| memory.id.clone()).collect(),
        }]
    }

    fn contents(entries: &[Memory]) -> Vec<&str> {
        entries
            .iter()
            .map(|memory| memory.content.as_str())
            .collect()
    }

    #[test]
    fn pins_become_tagged_entries_and_a_second_run_changes_nothing() {
        let mut manager = MemoryManager::new();
        let profile = fact(&mut manager, "prefers metric units", |_| {});
        let note = fact(&mut manager, "the build needs bun", |_| {});

        let report = migrate_pins(&mut manager, &pins(&[&profile], &[&note]), &live());

        assert_eq!(report.moved, vec![profile.id.clone(), note.id.clone()]);
        assert!(report.skipped.is_empty());
        let (user, notes) = blocks_for(&manager, AGENT);
        assert_eq!(contents(&user), ["prefers metric units"]);
        assert_eq!(contents(&notes), ["the build needs bun"]);
        let tagged = manager.get(&profile.id).unwrap();
        assert_eq!(
            tagged.tags.as_deref(),
            Some(&["saved-from-chat".to_string(), "core:user".to_string()][..]),
            "existing tags are kept"
        );
        assert_eq!(tagged.content, profile.content);
        assert_eq!(tagged.created_at, profile.created_at);

        let before = manager.snapshot();
        let again = migrate_pins(&mut manager, &pins(&[&profile], &[&note]), &live());
        assert_eq!(again, PinMigration::default());
        assert_eq!(manager.snapshot(), before);
    }

    #[test]
    fn over_budget_and_unfit_pins_are_skipped_with_a_reason_and_left_untouched() {
        let mut manager = MemoryManager::new();
        // Three 450-character facts fill About you to 1,350 of 1,500.
        let filler: Vec<Memory> = (0..3)
            .map(|index| fact(&mut manager, &format!("{index}{}", "a".repeat(449)), |_| {}))
            .collect();
        let over_budget = fact(&mut manager, &"b".repeat(200), |_| {});
        let too_long = fact(&mut manager, &"c".repeat(501), |_| {});
        let in_session = fact(&mut manager, "said in one chat", |memory| {
            memory.session_id = Some("session-1".into());
        });
        let foreign = fact(&mut manager, "another agent's fact", |memory| {
            memory.agent_id = "agent-other".into();
        });
        let mut picks: Vec<&Memory> = filler.iter().collect();
        picks.extend([&over_budget, &too_long, &in_session, &foreign]);
        let mut rows = pins(&picks, &[]);
        rows[0].user_profile_ids.push("mem-missing".into());
        rows.push(LegacyPins {
            agent_id: "agent-deleted".into(),
            user_profile_ids: vec![filler[0].id.clone()],
            agent_note_ids: vec![],
        });
        let untouched = [&over_budget, &too_long, &in_session, &foreign]
            .map(|memory| manager.get(&memory.id).unwrap());

        let report = migrate_pins(&mut manager, &rows, &live());

        assert_eq!(report.moved.len(), 3);
        let reasons: Vec<(&str, &str)> = report
            .skipped
            .iter()
            .map(|skip| (skip.memory_id.as_str(), skip.reason.as_str()))
            .collect();
        assert_eq!(
            reasons,
            [
                (
                    over_budget.id.as_str(),
                    "About you has no room (1,350/1,500 chars)"
                ),
                (
                    too_long.id.as_str(),
                    "the fact is longer than an entry may be (501 characters)"
                ),
                (in_session.id.as_str(), "the fact is no longer eligible"),
                (foreign.id.as_str(), "the fact is no longer eligible"),
                ("mem-missing", "the fact no longer exists"),
                (filler[0].id.as_str(), "the agent no longer exists"),
            ]
        );
        for memory in untouched {
            assert_eq!(manager.get(&memory.id), Some(memory));
        }
        for skip in &report.skipped {
            assert!(!skip.reason.contains("bbbb"), "reasons never echo content");
        }
    }

    #[test]
    fn a_fact_pinned_in_both_lists_or_already_in_core_memory_counts_once() {
        let mut manager = MemoryManager::new();
        let both = fact(&mut manager, "prefers metric units", |_| {});
        let duplicate = fact(&mut manager, "  prefers metric units ", |_| {});

        let report = migrate_pins(&mut manager, &pins(&[&both, &duplicate], &[&both]), &live());

        assert_eq!(report.moved, vec![both.id.clone()]);
        assert_eq!(
            report
                .skipped
                .iter()
                .map(|skip| skip.reason.as_str())
                .collect::<Vec<_>>(),
            [
                "About you already has that entry",
                "the fact is already in About you"
            ]
        );
    }

    #[test]
    fn older_control_plane_rows_deserialize_with_unknown_fields() {
        let rows: Vec<LegacyPins> = serde_json::from_value(serde_json::json!([
            {"agentId": "a", "version": 3, "userProfileIds": ["m1"], "agentNoteIds": []},
            {"agentId": "b"}
        ]))
        .unwrap();
        assert_eq!(rows[0].user_profile_ids, ["m1"]);
        assert!(rows[1].agent_note_ids.is_empty());
    }
}
