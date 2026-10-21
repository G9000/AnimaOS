//! Prepared, exact-image memory consolidation and conservative recovery/undo.

use anima_core::primitives::now_millis;

use super::{
    build_index_text, validate_importance, Memory, MemoryConsolidation, MemoryConsolidationError,
    MemoryConsolidationSource, MemoryManager, MemoryScope, MemoryType,
};

/// Semantic records match the entire prepared operation, or recovery must stop.
/// Graph citations are checked separately when undoing an applied operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryConsolidationState {
    Before,
    After,
    Diverged,
}

/// Exact semantic images for a single reversible consolidation.
///
/// Hosts can persist the images using their own storage format, then call
/// `from_images` to validate them on recovery. Private fields prevent applying a
/// journal that changes source fields beyond their type and consolidation tag.
#[derive(Clone, Debug, PartialEq)]
pub struct PreparedMemoryConsolidation {
    sources_before: Vec<Memory>,
    sources_after: Vec<Memory>,
    summary: Memory,
}

impl PreparedMemoryConsolidation {
    pub fn sources_before(&self) -> &[Memory] {
        &self.sources_before
    }

    pub fn sources_after(&self) -> &[Memory] {
        &self.sources_after
    }

    pub fn summary(&self) -> &Memory {
        &self.summary
    }

    /// Reconstruct a persisted operation only if every image is a canonical
    /// consolidation. This checks structure; the manager checks current records
    /// and caller visibility before any mutation.
    pub fn from_images(
        sources_before: Vec<Memory>,
        sources_after: Vec<Memory>,
        summary: Memory,
    ) -> Result<Self, MemoryConsolidationError> {
        let tags = summary
            .tags
            .as_ref()
            .filter(|tags| tags.len() == 3)
            .ok_or(MemoryConsolidationError::InvalidInput)?;
        let expected = Self::prepare_images(
            sources_before,
            &summary.content,
            &tags[2],
            &summary.id,
            summary.created_at,
        )?;
        if expected.summary != summary || expected.sources_after != sources_after {
            return Err(MemoryConsolidationError::InvalidInput);
        }
        Ok(expected)
    }

    fn prepare_images(
        sources_before: Vec<Memory>,
        content: &str,
        group_tag: &str,
        summary_id: &str,
        created_at: u64,
    ) -> Result<Self, MemoryConsolidationError> {
        validate_request(sources_before.len(), content, group_tag, summary_id)?;
        let mut importance = 0.0_f64;
        for (index, source) in sources_before.iter().enumerate() {
            validate_source_input(&source.id, &source.content)?;
            if source.id == summary_id
                || sources_before[..index]
                    .iter()
                    .any(|other| other.id == source.id)
            {
                return Err(MemoryConsolidationError::InvalidInput);
            }
            if source.memory_type != MemoryType::Fact
                || source.world_id.is_some()
                || source.session_id.is_some()
            {
                return Err(MemoryConsolidationError::SourceUnavailable);
            }
            let first = &sources_before[0];
            if source.agent_id != first.agent_id
                || source.scope != first.scope
                || source.room_id != first.room_id
                || source.scope == MemoryScope::Room
                    && !source
                        .room_id
                        .as_deref()
                        .is_some_and(|room| !room.is_empty())
            {
                return Err(MemoryConsolidationError::IncompatibleCohort);
            }
            if source.tags.as_ref().is_some_and(|tags| tags.len() >= 20) {
                return Err(MemoryConsolidationError::TagCapacity);
            }
            importance = importance.max(
                validate_importance(source.importance)
                    .map_err(|_| MemoryConsolidationError::InvalidImportance)?,
            );
        }
        let first = &sources_before[0];
        let summary = Memory {
            id: summary_id.into(),
            agent_id: first.agent_id.clone(),
            agent_name: first.agent_name.clone(),
            memory_type: MemoryType::Fact,
            content: content.trim().into(),
            importance,
            created_at,
            tags: Some(vec![
                "runtime".into(),
                "tool-memory-consolidate".into(),
                group_tag.into(),
            ]),
            scope: first.scope,
            room_id: first.room_id.clone(),
            world_id: None,
            session_id: None,
        };
        let sources_after = sources_before
            .iter()
            .map(|source| {
                let mut archived = source.clone();
                archived.memory_type = MemoryType::Reflection;
                archived
                    .tags
                    .get_or_insert_with(Vec::new)
                    .push(group_tag.into());
                archived
            })
            .collect();
        Ok(Self {
            sources_before,
            sources_after,
            summary,
        })
    }
}

impl MemoryManager {
    /// Prepare and reserve exact semantic images without changing the manager.
    /// The host supplies the durable summary ID and group tag, persists this
    /// preparation, then applies it under the same serialized mutation lock.
    #[allow(clippy::too_many_arguments)]
    pub fn prepare_fact_consolidation(
        &self,
        agent_id: &str,
        room_id: Option<&str>,
        sources: &[MemoryConsolidationSource],
        content: &str,
        group_tag: &str,
        summary_id: &str,
        visible: impl Fn(&Memory) -> bool,
    ) -> Result<PreparedMemoryConsolidation, MemoryConsolidationError> {
        validate_request(sources.len(), content, group_tag, summary_id)?;
        if self.memories.contains_key(summary_id) {
            return Err(MemoryConsolidationError::StateDiverged);
        }
        let mut before = Vec::with_capacity(sources.len());
        for (index, source) in sources.iter().enumerate() {
            validate_source_input(&source.id, &source.expected_content)?;
            if sources[..index].iter().any(|other| other.id == source.id) {
                return Err(MemoryConsolidationError::InvalidInput);
            }
            let memory = self
                .memories
                .get(&source.id)
                .filter(|memory| {
                    memory.agent_id == agent_id
                        && memory.memory_type == MemoryType::Fact
                        && memory.world_id.is_none()
                        && memory.session_id.is_none()
                        && visible(memory)
                })
                .ok_or(MemoryConsolidationError::SourceUnavailable)?;
            if memory.content != source.expected_content {
                return Err(MemoryConsolidationError::SourceChanged);
            }
            if memory.scope == MemoryScope::Room
                && !room_id
                    .is_some_and(|room| !room.is_empty() && memory.room_id.as_deref() == Some(room))
            {
                return Err(MemoryConsolidationError::IncompatibleCohort);
            }
            before.push(memory.clone());
        }
        PreparedMemoryConsolidation::prepare_images(
            before,
            content,
            group_tag,
            summary_id,
            now_millis(),
        )
    }

    /// Classify the complete semantic transaction; partial or edited records are
    /// never mistaken for a successful application or an untouched operation.
    pub fn fact_consolidation_state(
        &self,
        prepared: &PreparedMemoryConsolidation,
    ) -> MemoryConsolidationState {
        let summary = self.memories.get(&prepared.summary.id);
        if summary.is_none()
            && prepared
                .sources_before
                .iter()
                .all(|source| self.memories.get(&source.id) == Some(source))
        {
            MemoryConsolidationState::Before
        } else if summary == Some(&prepared.summary)
            && prepared
                .sources_after
                .iter()
                .all(|source| self.memories.get(&source.id) == Some(source))
        {
            MemoryConsolidationState::After
        } else {
            MemoryConsolidationState::Diverged
        }
    }

    /// Check all exact source images and visibility before changing any record.
    /// Other memories, graph evidence, and host-owned recall metrics stay intact.
    pub fn apply_fact_consolidation(
        &mut self,
        prepared: &PreparedMemoryConsolidation,
        visible: impl Fn(&Memory) -> bool,
    ) -> Result<MemoryConsolidation, MemoryConsolidationError> {
        if self.fact_consolidation_state(prepared) != MemoryConsolidationState::Before {
            return Err(MemoryConsolidationError::StateDiverged);
        }
        if !prepared.sources_before.iter().all(&visible) || !visible(&prepared.summary) {
            return Err(MemoryConsolidationError::SourceUnavailable);
        }
        for memory in prepared
            .sources_after
            .iter()
            .chain(std::iter::once(&prepared.summary))
        {
            self.index
                .add_document(memory.id.clone(), build_index_text(memory));
            self.memories.insert(memory.id.clone(), memory.clone());
        }
        Ok(MemoryConsolidation {
            summary: prepared.summary.clone(),
            archived_ids: prepared
                .sources_after
                .iter()
                .map(|source| source.id.clone())
                .collect(),
        })
    }

    /// Restore only this operation's sources and remove its summary. Undo stops
    /// when semantic records changed or new graph evidence cites the summary.
    pub fn undo_fact_consolidation(
        &mut self,
        prepared: &PreparedMemoryConsolidation,
        visible: impl Fn(&Memory) -> bool,
    ) -> Result<(), MemoryConsolidationError> {
        if self.fact_consolidation_state(prepared) != MemoryConsolidationState::After {
            return Err(MemoryConsolidationError::StateDiverged);
        }
        if !prepared.sources_after.iter().all(&visible) || !visible(&prepared.summary) {
            return Err(MemoryConsolidationError::SourceUnavailable);
        }
        let cited = |ids: &[String]| ids.iter().any(|id| id == &prepared.summary.id);
        if self
            .agent_relationships
            .values()
            .any(|relationship| cited(&relationship.evidence_memory_ids))
            || self
                .temporal_facts
                .values()
                .any(|fact| cited(&fact.evidence_memory_ids))
            || self
                .temporal_relationships
                .values()
                .any(|relationship| cited(&relationship.evidence_memory_ids))
        {
            return Err(MemoryConsolidationError::SummaryCited);
        }
        for memory in &prepared.sources_before {
            self.index
                .add_document(memory.id.clone(), build_index_text(memory));
            self.memories.insert(memory.id.clone(), memory.clone());
        }
        self.memories.remove(&prepared.summary.id);
        self.index.remove_document(&prepared.summary.id);
        Ok(())
    }
}

fn validate_request(
    source_count: usize,
    content: &str,
    group_tag: &str,
    summary_id: &str,
) -> Result<(), MemoryConsolidationError> {
    if !(2..=8).contains(&source_count)
        || content.len() > 8000 * 4
        || content.chars().count() > 8000
        || content.trim().is_empty()
        || !group_tag.strip_prefix("cg:").is_some_and(|suffix| {
            suffix.len() == 32 && suffix.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
        || summary_id.len() > 256
        || summary_id.trim().is_empty()
    {
        return Err(MemoryConsolidationError::InvalidInput);
    }
    Ok(())
}

fn validate_source_input(id: &str, content: &str) -> Result<(), MemoryConsolidationError> {
    if id.len() > 256
        || id.trim().is_empty()
        || content.len() > 8000 * 4
        || content.chars().count() > 8000
        || content.is_empty()
    {
        return Err(MemoryConsolidationError::InvalidInput);
    }
    Ok(())
}
