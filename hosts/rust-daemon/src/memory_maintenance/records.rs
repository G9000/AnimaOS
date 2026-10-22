use anima_memory::{Memory, MemoryScope, MemoryType, PreparedMemoryConsolidation};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use utoipa::ToSchema;

pub(super) const WINDOW_MS: u64 = 86_400_000;
pub(super) const CADENCE_MS: u64 = 6 * 60 * 60 * 1000;
pub(super) const MAX_ATTEMPTS: usize = 2;
pub(super) const MAX_PENDING: usize = 10;
const MAX_OPERATIONS: usize = 2000;
const MAX_SOURCES: usize = 10_000;

/// These images deliberately exclude recall statistics, which belong to the index.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MemoryImage {
    pub(crate) id: String,
    pub(crate) agent_id: String,
    pub(crate) agent_name: String,
    pub(crate) memory_type: String,
    pub(crate) content: String,
    pub(crate) importance: f64,
    pub(crate) created_at: u64,
    pub(crate) tags: Option<Vec<String>>,
    pub(crate) scope: String,
    pub(crate) room_id: Option<String>,
    pub(crate) world_id: Option<String>,
    pub(crate) session_id: Option<String>,
}
impl From<&Memory> for MemoryImage {
    fn from(m: &Memory) -> Self {
        Self {
            id: m.id.clone(),
            agent_id: m.agent_id.clone(),
            agent_name: m.agent_name.clone(),
            memory_type: m.memory_type.as_str().into(),
            content: m.content.clone(),
            importance: m.importance,
            created_at: m.created_at,
            tags: m.tags.clone(),
            scope: m.scope.as_str().into(),
            room_id: m.room_id.clone(),
            world_id: m.world_id.clone(),
            session_id: m.session_id.clone(),
        }
    }
}
impl MemoryImage {
    pub(super) fn memory(&self) -> Result<Memory, String> {
        if !image_bounds(
            &self.id,
            &self.agent_id,
            &self.agent_name,
            &self.content,
            self.importance,
            self.tags.as_deref(),
            self.room_id.as_deref(),
            self.world_id.as_deref(),
            self.session_id.as_deref(),
        ) {
            return Err("Invalid maintenance memory image".into());
        }
        Ok(Memory {
            id: self.id.clone(),
            agent_id: self.agent_id.clone(),
            agent_name: self.agent_name.clone(),
            memory_type: MemoryType::parse(&self.memory_type).map_err(|_| "Invalid memory type")?,
            content: self.content.clone(),
            importance: self.importance,
            created_at: self.created_at,
            tags: self.tags.clone(),
            scope: MemoryScope::parse(&self.scope).map_err(|_| "Invalid memory scope")?,
            room_id: self.room_id.clone(),
            world_id: self.world_id.clone(),
            session_id: self.session_id.clone(),
        })
    }
    pub(super) fn fingerprint(&self) -> String {
        hex::encode(Sha256::digest(
            serde_json::to_vec(self).expect("memory image serializes"),
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MaintenancePolicy {
    pub(crate) enabled: bool,
    pub(crate) revision: u64,
    pub(crate) next_due_ms: u64,
}
impl Default for MaintenancePolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            revision: 1,
            next_due_ms: 0,
        }
    }
}

/// Written only by authenticated owner routes, never inferred from model tags.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SourceApproval {
    pub(crate) agent_id: String,
    pub(crate) fingerprint: String,
    #[serde(default)]
    pub(crate) last_reviewed_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MaintenanceState {
    Reviewing,
    NoChange,
    Pending,
    PreparedApply,
    Applied,
    PreparedUndo,
    Undone,
    Dismissed,
    Interrupted,
    Failed,
    NeedsReview,
}
impl MaintenanceState {
    pub(super) fn pending(self) -> bool {
        matches!(
            self,
            Self::Reviewing
                | Self::Pending
                | Self::PreparedApply
                | Self::PreparedUndo
                | Self::NeedsReview
        )
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MaintenanceOperation {
    pub(crate) id: String,
    pub(crate) agent_id: String,
    pub(crate) revision: u64,
    pub(crate) policy_revision: u64,
    pub(crate) reserved_at_ms: u64,
    pub(crate) state: MaintenanceState,
    pub(crate) sources_before: Vec<MemoryImage>,
    pub(crate) sources_after: Vec<MemoryImage>,
    pub(crate) summary: Option<MemoryImage>,
    pub(crate) reason: String,
    /// True only when dispatch and completion both had Free authority.
    pub(crate) automatic_apply: bool,
}
impl MaintenanceOperation {
    pub(super) fn prepared(&self) -> Result<PreparedMemoryConsolidation, String> {
        PreparedMemoryConsolidation::from_images(
            self.sources_before
                .iter()
                .map(MemoryImage::memory)
                .collect::<Result<_, _>>()?,
            self.sources_after
                .iter()
                .map(MemoryImage::memory)
                .collect::<Result<_, _>>()?,
            self.summary
                .as_ref()
                .ok_or("Missing maintenance summary")?
                .memory()?,
        )
        .map_err(|e| e.message().to_string())
    }
    pub(super) fn finish(&mut self, state: MaintenanceState, reason: &str) {
        self.state = state;
        self.reason = reason.into();
        self.revision = self.revision.saturating_add(1);
    }
}

#[derive(Clone, Copy, Debug, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MaintenanceDecision {
    Accept,
    Dismiss,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MaintenanceRegistry {
    #[serde(default)]
    pub(crate) policies: BTreeMap<String, MaintenancePolicy>,
    #[serde(default)]
    pub(crate) sources: BTreeMap<String, SourceApproval>,
    #[serde(default)]
    pub(crate) operations: BTreeMap<String, MaintenanceOperation>,
}
impl MaintenanceRegistry {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.operations.len() > MAX_OPERATIONS
            || self.sources.len() > MAX_SOURCES
            || self.policies.len() > MAX_SOURCES
        {
            return Err("Maintenance snapshot exceeds its bound".into());
        }
        for (id, p) in &self.policies {
            if id.is_empty() || id.len() > 256 || p.revision == 0 {
                return Err("Invalid maintenance policy".into());
            }
        }
        for (id, approval) in &self.sources {
            if id.is_empty()
                || id.len() > 256
                || approval.agent_id.is_empty()
                || approval.agent_id.len() > 256
                || approval.fingerprint.len() != 64
                || !approval.fingerprint.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err("Invalid maintenance provenance".into());
            }
        }
        for (id, op) in &self.operations {
            if id != &op.id
                || uuid::Uuid::parse_str(id).is_err()
                || op.agent_id.is_empty()
                || op.agent_id.len() > 256
                || op.revision == 0
                || op.policy_revision == 0
                || !(2..=8).contains(&op.sources_before.len())
                || op.reason.len() > 1024
            {
                return Err("Invalid maintenance operation".into());
            }
            let mut seen = HashSet::new();
            for image in &op.sources_before {
                if image.agent_id != op.agent_id
                    || !seen.insert(&image.id)
                    || !eligible(&image.memory()?)
                {
                    return Err("Invalid maintenance cohort".into());
                }
            }
            if op.summary.is_some() {
                op.prepared()?;
            } else if !op.sources_after.is_empty()
                || matches!(
                    op.state,
                    MaintenanceState::Pending
                        | MaintenanceState::PreparedApply
                        | MaintenanceState::Applied
                        | MaintenanceState::PreparedUndo
                        | MaintenanceState::Undone
                )
            {
                return Err("Missing maintenance journal images".into());
            }
        }
        Ok(())
    }
    pub(super) fn can_reserve(&self, agent_id: &str, now: u64) -> bool {
        self.operations.len() < MAX_OPERATIONS
            && self
                .operations
                .values()
                .filter(|o| o.agent_id == agent_id && o.state.pending())
                .count()
                < MAX_PENDING
            && self
                .operations
                .values()
                .filter(|o| {
                    o.agent_id == agent_id && now.saturating_sub(o.reserved_at_ms) < WINDOW_MS
                })
                .count()
                < MAX_ATTEMPTS
    }
    pub(super) fn trusted(&self, memory: &Memory) -> bool {
        self.sources.get(&memory.id).is_some_and(|s| {
            s.agent_id == memory.agent_id
                && s.fingerprint == MemoryImage::from(memory).fingerprint()
        })
    }
    pub(super) fn trust(&mut self, memory: &Memory) -> Result<(), String> {
        if !eligible(memory) {
            return Err("Memory is not eligible for maintenance".into());
        }
        if self.sources.len() >= MAX_SOURCES && !self.sources.contains_key(&memory.id) {
            return Err("Maintenance provenance capacity reached".into());
        }
        let fingerprint = MemoryImage::from(memory).fingerprint();
        let last_reviewed_ms = self
            .sources
            .get(&memory.id)
            .filter(|s| s.fingerprint == fingerprint)
            .map_or(0, |s| s.last_reviewed_ms);
        self.sources.insert(
            memory.id.clone(),
            SourceApproval {
                agent_id: memory.agent_id.clone(),
                fingerprint,
                last_reviewed_ms,
            },
        );
        Ok(())
    }
}

pub(super) fn eligible(m: &Memory) -> bool {
    image_bounds(
        &m.id,
        &m.agent_id,
        &m.agent_name,
        &m.content,
        m.importance,
        m.tags.as_deref(),
        m.room_id.as_deref(),
        m.world_id.as_deref(),
        m.session_id.as_deref(),
    ) && m.memory_type == MemoryType::Fact
        && m.world_id.is_none()
        && m.session_id.is_none()
        && m.content.chars().count() <= 8000
        && !m.content.trim().is_empty()
        && !core_tagged(m)
        && !crate::memory_text::has_hidden_text(&m.content)
        && crate::threat_scan::blocking_ids(&m.content).is_empty()
        && m.tags.as_ref().is_none_or(|tags| {
            tags.len() < 20
                && tags.iter().all(|t| {
                    !crate::memory_text::has_hidden_text(t)
                        && crate::threat_scan::blocking_ids(t).is_empty()
                })
        })
}

#[allow(clippy::too_many_arguments)]
fn image_bounds(
    id: &str,
    agent_id: &str,
    agent_name: &str,
    content: &str,
    importance: f64,
    tags: Option<&[String]>,
    room_id: Option<&str>,
    world_id: Option<&str>,
    session_id: Option<&str>,
) -> bool {
    !id.is_empty()
        && id.len() <= 256
        && agent_id.len() <= 256
        && agent_name.len() <= 1024
        && content.len() <= 32_000
        && importance.is_finite()
        && (0.0..=1.0).contains(&importance)
        && tags.is_none_or(|t| t.len() <= 20 && t.iter().all(|s| s.len() <= 512))
        && [room_id, world_id, session_id]
            .iter()
            .all(|id| id.is_none_or(|id| id.len() <= 256))
}

pub(super) fn core_tagged(m: &Memory) -> bool {
    m.tags
        .as_deref()
        .is_some_and(crate::core_memory::names_core_tag)
}

#[cfg(test)]
pub(super) fn select_cohort(sources: Vec<Memory>) -> Vec<Memory> {
    select_cohort_by(sources, |_| 0)
}

pub(super) fn select_cohort_by(
    mut sources: Vec<Memory>,
    reviewed: impl Fn(&Memory) -> u64,
) -> Vec<Memory> {
    sources.sort_by(|a, b| {
        reviewed(a)
            .cmp(&reviewed(b))
            .then(a.created_at.cmp(&b.created_at))
            .then(a.id.cmp(&b.id))
    });
    let mut groups: BTreeMap<(String, Option<String>), Vec<Memory>> = BTreeMap::new();
    for source in sources {
        groups
            .entry((source.scope.as_str().into(), source.room_id.clone()))
            .or_default()
            .push(source);
    }
    let mut best: Vec<Memory> = Vec::new();
    for group in groups.into_values() {
        if group.len() < 2 {
            continue;
        }
        let sizes: Vec<_> = group
            .iter()
            .map(|source| {
                serde_json::to_vec(&serde_json::json!({"id":source.id,"content":source.content}))
                    .expect("source serializes")
                    .len()
                    + 1
            })
            .collect();
        let mut cheapest: Vec<_> = sizes.iter().copied().enumerate().collect();
        cheapest.sort_by_key(|(index, size)| (*size, *index));
        let mut cohort = Vec::new();
        let mut bytes = 2;
        for (index, source) in group.into_iter().enumerate() {
            let size = sizes[index];
            let partner_size = if cheapest[0].0 == index {
                cheapest[1].1
            } else {
                cheapest[0].1
            };
            if cohort.is_empty() && bytes + size + partner_size > 12 * 1024 {
                continue;
            }
            if bytes + size > 12 * 1024 {
                continue;
            }
            bytes += size;
            cohort.push(source);
            if cohort.len() == 8 {
                break;
            }
        }
        if cohort.len() >= 2
            && (best.is_empty()
                || reviewed(&cohort[0])
                    .cmp(&reviewed(&best[0]))
                    .then(cohort[0].created_at.cmp(&best[0].created_at))
                    .then(cohort[0].id.cmp(&best[0].id))
                    .is_lt())
        {
            best = cohort;
        }
    }
    best
}
