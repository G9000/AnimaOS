//! Owner edits: change a memory, delete one with its citations, delete an entity.

use super::{
    build_index_text, entity_key, unique_strings, validate_importance, Memory, MemoryError,
    MemoryManager, MemoryScope, MemoryType, NewMemory, RelationshipEndpointKind,
};

/// An exact-current-content source for an explicit Fact consolidation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryConsolidationSource {
    pub id: String,
    pub expected_content: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MemoryConsolidation {
    pub summary: Memory,
    /// Source IDs in caller order; the original records remain as Reflections.
    pub archived_ids: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryConsolidationError {
    InvalidInput,
    SourceUnavailable,
    SourceChanged,
    IncompatibleCohort,
    TagCapacity,
    InvalidImportance,
}

impl MemoryConsolidationError {
    pub const fn message(self) -> &'static str {
        match self {
            Self::InvalidInput => "Invalid memory consolidation input.",
            Self::SourceUnavailable => "Memory source unavailable.",
            Self::SourceChanged => "Memory source changed; read it again before consolidating.",
            Self::IncompatibleCohort => "Memory sources must share an eligible scope and room.",
            Self::TagCapacity => "Memory source has no capacity for the consolidation tag.",
            Self::InvalidImportance => "Memory source importance is invalid.",
        }
    }
}

/// A partial change to a memory. Absent fields keep their value.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MemoryPatch {
    /// Trimmed; empty after trimming is `InvalidMemoryContent`.
    pub content: Option<String>,
    /// Validated like `MemoryManager::add`.
    pub importance: Option<f64>,
    /// `None` keeps, `Some(None)` clears, `Some(Some(v))` replaces (an empty list clears).
    pub tags: Option<Option<Vec<String>>>,
}

/// What `delete_memory` changed besides the memory itself. Ids are sorted ascending.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MemoryDeletion {
    /// Agent relationships that cited the memory and were left with no evidence.
    pub removed_relationship_ids: Vec<String>,
    /// Agent relationships that cited the memory and keep other evidence.
    pub updated_relationship_ids: Vec<String>,
    /// Temporal facts that cited the memory (kept, citation removed).
    pub updated_fact_ids: Vec<String>,
    /// Temporal relationships that cited the memory (kept, citation removed).
    pub updated_temporal_relationship_ids: Vec<String>,
}

/// What `delete_entity` removed besides the entity itself. Ids are sorted ascending.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EntityDeletion {
    pub removed_relationship_ids: Vec<String>,
    pub removed_temporal_relationship_ids: Vec<String>,
    pub removed_fact_ids: Vec<String>,
}

impl MemoryManager {
    /// Validate the whole exact-CAS cohort, then add one Fact and archive its sources.
    /// The caller owns persistence/rollback and must not infer summary faithfulness from success.
    pub fn consolidate_facts(
        &mut self,
        agent_id: &str,
        room_id: Option<&str>,
        sources: &[MemoryConsolidationSource],
        content: &str,
        group_tag: &str,
        visible: impl Fn(&Memory) -> bool,
    ) -> Result<MemoryConsolidation, MemoryConsolidationError> {
        if !(2..=8).contains(&sources.len())
            || content.len() > 8000 * 4
            || content.chars().count() > 8000
            || content.trim().is_empty()
            || !group_tag.strip_prefix("cg:").is_some_and(|suffix| {
                suffix.len() == 32 && suffix.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
        {
            return Err(MemoryConsolidationError::InvalidInput);
        }

        let mut staged: Vec<Memory> = Vec::with_capacity(sources.len());
        let mut importance = 0.0_f64;
        for (index, source) in sources.iter().enumerate() {
            if source.id.len() > 256
                || source.id.trim().is_empty()
                || source.expected_content.len() > 8000 * 4
                || source.expected_content.chars().count() > 8000
                || source.expected_content.is_empty()
                || sources[..index]
                    .iter()
                    .any(|previous| previous.id == source.id)
            {
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
            if (memory.scope == MemoryScope::Room
                && !room_id.is_some_and(|room| {
                    !room.is_empty() && memory.room_id.as_deref() == Some(room)
                }))
                || staged.first().is_some_and(|first| {
                    first.scope != memory.scope || first.room_id != memory.room_id
                })
            {
                return Err(MemoryConsolidationError::IncompatibleCohort);
            }
            if memory.tags.as_ref().is_some_and(|tags| tags.len() >= 20) {
                return Err(MemoryConsolidationError::TagCapacity);
            }
            importance = importance.max(
                validate_importance(memory.importance)
                    .map_err(|_| MemoryConsolidationError::InvalidImportance)?,
            );
            staged.push(memory.clone());
        }

        let first = &staged[0];
        let summary = self
            .add(NewMemory {
                agent_id: first.agent_id.clone(),
                agent_name: first.agent_name.clone(),
                memory_type: MemoryType::Fact,
                content: content.trim().to_string(),
                importance,
                tags: Some(vec![
                    "runtime".into(),
                    "tool-memory-consolidate".into(),
                    group_tag.into(),
                ]),
                scope: Some(first.scope),
                room_id: first.room_id.clone(),
                world_id: None,
                session_id: None,
            })
            .map_err(|_| MemoryConsolidationError::InvalidImportance)?;
        let mut archived_ids = Vec::with_capacity(staged.len());
        for mut memory in staged {
            memory.memory_type = MemoryType::Reflection;
            memory
                .tags
                .get_or_insert_with(Vec::new)
                .push(group_tag.into());
            self.index
                .add_document(memory.id.clone(), build_index_text(&memory));
            archived_ids.push(memory.id.clone());
            self.memories.insert(memory.id.clone(), memory);
        }
        Ok(MemoryConsolidation {
            summary,
            archived_ids,
        })
    }

    /// Find a byte-exact duplicate for the native default-scope Fact add path.
    /// This lookup changes nothing and returns only the deterministic oldest match.
    pub fn find_exact_default_fact(
        &self,
        agent_id: &str,
        content: &str,
        visible: impl Fn(&Memory) -> bool,
    ) -> Option<Memory> {
        self.memories
            .values()
            .filter(|memory| {
                memory.agent_id == agent_id
                    && memory.memory_type == MemoryType::Fact
                    && memory.scope == MemoryScope::Private
                    && memory.room_id.is_none()
                    && memory.world_id.is_none()
                    && memory.session_id.is_none()
                    && memory.content == content
                    && visible(memory)
            })
            .min_by(|left, right| {
                left.created_at
                    .cmp(&right.created_at)
                    .then_with(|| left.id.cmp(&right.id))
            })
            .cloned()
    }

    /// Ok(None): no such memory. Validation happens before anything changes.
    pub fn update_memory(
        &mut self,
        id: &str,
        patch: MemoryPatch,
    ) -> Result<Option<Memory>, MemoryError> {
        if !self.memories.contains_key(id) {
            return Ok(None);
        }
        let content = match patch.content {
            Some(content) => {
                let content = content.trim().to_string();
                if content.is_empty() {
                    return Err(MemoryError::InvalidMemoryContent);
                }
                Some(content)
            }
            None => None,
        };
        let importance = match patch.importance {
            Some(importance) => Some(validate_importance(importance)?),
            None => None,
        };
        let tags = patch.tags.map(|tags| {
            tags.map(unique_strings)
                .filter(|tags: &Vec<String>| !tags.is_empty())
        });

        let memory = self
            .memories
            .get_mut(id)
            .expect("memory presence was checked above");
        if let Some(content) = content {
            memory.content = content;
        }
        if let Some(importance) = importance {
            memory.importance = importance;
        }
        if let Some(tags) = tags {
            memory.tags = tags;
        }
        let updated = memory.clone();
        self.index
            .add_document(updated.id.clone(), build_index_text(&updated));
        Ok(Some(updated))
    }

    /// Clear a memory's session so it no longer hides from agent context.
    /// Every other field stays; `MemoryPatch` deliberately cannot change a session.
    pub fn detach_session(&mut self, id: &str) -> Result<Memory, MemoryError> {
        let memory = self
            .memories
            .get_mut(id)
            .ok_or(MemoryError::MemoryNotFound)?;
        memory.session_id = None;
        let updated = memory.clone();
        self.index
            .add_document(updated.id.clone(), build_index_text(&updated));
        Ok(updated)
    }

    /// None: no such memory.
    ///
    /// Removes the memory and its index entry, then strips its id from every
    /// citation. Only agent relationships left with no evidence are removed;
    /// temporal facts and temporal relationships are kept even with no evidence.
    /// This is deliberately not `prune_relationship_evidence`, which would also
    /// remove relationships that never cited anything.
    pub fn delete_memory(&mut self, id: &str) -> Option<MemoryDeletion> {
        self.memories.remove(id)?;
        self.index.remove_document(id);

        let mut deletion = MemoryDeletion::default();
        let mut emptied = Vec::new();
        for relationship in self.agent_relationships.values_mut() {
            if !relationship.evidence_memory_ids.iter().any(|v| v == id) {
                continue;
            }
            relationship.evidence_memory_ids.retain(|v| v != id);
            if relationship.evidence_memory_ids.is_empty() {
                emptied.push(relationship.id.clone());
            } else {
                deletion
                    .updated_relationship_ids
                    .push(relationship.id.clone());
            }
        }
        for relationship_id in &emptied {
            self.agent_relationships.remove(relationship_id);
        }
        deletion.removed_relationship_ids = emptied;

        for fact in self.temporal_facts.values_mut() {
            if fact.evidence_memory_ids.iter().any(|v| v == id) {
                fact.evidence_memory_ids.retain(|v| v != id);
                deletion.updated_fact_ids.push(fact.id.clone());
            }
        }
        for relationship in self.temporal_relationships.values_mut() {
            if relationship.evidence_memory_ids.iter().any(|v| v == id) {
                relationship.evidence_memory_ids.retain(|v| v != id);
                deletion
                    .updated_temporal_relationship_ids
                    .push(relationship.id.clone());
            }
        }

        deletion.removed_relationship_ids.sort();
        deletion.updated_relationship_ids.sort();
        deletion.updated_fact_ids.sort();
        deletion.updated_temporal_relationship_ids.sort();
        Some(deletion)
    }

    /// Ok(None): no such entity. Err(EntityOwnsMemories): an agent entity that is
    /// the `agent_id` of any memory (loading would recreate it from its memories).
    ///
    /// Relationships and facts that name the entity are removed rather than edited,
    /// because loading recreates an entity from anything that names it.
    pub fn delete_entity(
        &mut self,
        kind: RelationshipEndpointKind,
        id: &str,
    ) -> Result<Option<EntityDeletion>, MemoryError> {
        let key = entity_key(kind, id);
        if !self.memory_entities.contains_key(&key) {
            return Ok(None);
        }
        if kind == RelationshipEndpointKind::Agent
            && self.memories.values().any(|memory| memory.agent_id == id)
        {
            return Err(MemoryError::EntityOwnsMemories);
        }

        let mut removed_relationship_ids: Vec<String> = self
            .agent_relationships
            .values()
            .filter(|r| {
                (r.source_kind == kind && r.source_agent_id == id)
                    || (r.target_kind == kind && r.target_agent_id == id)
            })
            .map(|r| r.id.clone())
            .collect();
        removed_relationship_ids.sort();
        for relationship_id in &removed_relationship_ids {
            self.agent_relationships.remove(relationship_id);
        }

        let mut removed_temporal_relationship_ids: Vec<String> = self
            .temporal_relationships
            .values()
            .filter(|r| {
                (r.source_kind == kind && r.source_id == id)
                    || (r.target_kind == kind && r.target_id == id)
            })
            .map(|r| r.id.clone())
            .collect();
        removed_temporal_relationship_ids.sort();
        for relationship_id in &removed_temporal_relationship_ids {
            self.temporal_relationships.remove(relationship_id);
        }
        for relationship in self.temporal_relationships.values_mut() {
            relationship
                .supersedes_relationship_ids
                .retain(|v| !removed_temporal_relationship_ids.contains(v));
        }

        let mut removed_fact_ids: Vec<String> = self
            .temporal_facts
            .values()
            .filter(|f| {
                (f.subject_kind == kind && f.subject_id == id)
                    || (f.object_kind == Some(kind) && f.object_id.as_deref() == Some(id))
            })
            .map(|f| f.id.clone())
            .collect();
        removed_fact_ids.sort();
        for fact_id in &removed_fact_ids {
            self.temporal_facts.remove(fact_id);
        }
        for fact in self.temporal_facts.values_mut() {
            fact.supersedes_fact_ids
                .retain(|v| !removed_fact_ids.contains(v));
        }

        self.memory_entities.remove(&key);
        Ok(Some(EntityDeletion {
            removed_relationship_ids,
            removed_temporal_relationship_ids,
            removed_fact_ids,
        }))
    }
}
