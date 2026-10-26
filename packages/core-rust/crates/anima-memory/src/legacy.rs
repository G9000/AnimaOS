//! Lossless, inspectable legacy evidence. Unscoped auxiliary rows are never generation authority.
use crate::MemoryManagerSnapshot;
use anima_core::MemoryPortError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Exact profile-scoped source image retained beside canonical import receipts.
/// This is historical inspection data, not a replacement for current canonical lifecycle state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacySnapshotArchive {
    schema_version: u32,
    evidence: String,
    source_id: String,
    legacy_agent_id: String,
    snapshot: MemoryManagerSnapshot,
}
impl LegacySnapshotArchive {
    pub fn new(
        source_id: impl Into<String>,
        legacy_agent_id: impl Into<String>,
        snapshot: MemoryManagerSnapshot,
    ) -> Result<Self, MemoryPortError> {
        let archive = Self {
            schema_version: 1,
            evidence: "legacy/unknown".into(),
            source_id: source_id.into(),
            legacy_agent_id: legacy_agent_id.into(),
            snapshot,
        };
        archive.validate()?;
        Ok(archive)
    }
    pub fn source_id(&self) -> &str {
        &self.source_id
    }
    pub fn snapshot(&self) -> &MemoryManagerSnapshot {
        &self.snapshot
    }
    pub fn legacy_agent_id(&self) -> &str {
        &self.legacy_agent_id
    }
    pub fn evidence(&self) -> &str {
        &self.evidence
    }
    pub fn validate(&self) -> Result<(), MemoryPortError> {
        if self.schema_version != 1 || self.evidence != "legacy/unknown" {
            return Err(MemoryPortError::unsupported(
                "unsupported legacy snapshot archive schema",
            ));
        }
        if self.source_id.trim().is_empty()
            || self.source_id.trim() != self.source_id
            || self.source_id.len() > 256
        {
            return Err(MemoryPortError::invalid("invalid legacy archive source ID"));
        }
        if self.legacy_agent_id.trim().is_empty()
            || self.legacy_agent_id.trim() != self.legacy_agent_id
            || self.legacy_agent_id.len() > 256
            || self
                .snapshot
                .memories
                .iter()
                .any(|memory| memory.agent_id != self.legacy_agent_id)
        {
            return Err(MemoryPortError::invalid(
                "copied legacy snapshot must contain exactly the selected agent profile",
            ));
        }
        if [
            self.snapshot.memories.len(),
            self.snapshot.memory_entities.len(),
            self.snapshot.agent_relationships.len(),
            self.snapshot.temporal_facts.len(),
            self.snapshot.temporal_relationships.len(),
        ]
        .iter()
        .any(|count| *count > 1000)
        {
            return Err(MemoryPortError::invalid(
                "legacy archive exceeds record limit",
            ));
        }
        self.rows()?;
        // JSON cannot preserve non-finite numbers. Reject rather than silently archive nulls.
        let json = serde_json::to_string(self)
            .map_err(|_| MemoryPortError::invalid("legacy archive cannot be serialized"))?;
        if json.len() > 4 * 1024 * 1024 || serde_json::from_str::<Self>(&json).is_err() {
            return Err(MemoryPortError::invalid(
                "legacy archive is not losslessly serializable within byte limit",
            ));
        }
        Ok(())
    }
    /// Namespaced stable keys and full typed payloads, for collision checks across copied imports.
    pub fn rows(&self) -> Result<BTreeMap<String, String>, MemoryPortError> {
        let mut rows = BTreeMap::new();
        let mut add = |kind: &str, id: &str, value: String| -> Result<(), MemoryPortError> {
            if id.trim().is_empty() || rows.insert(format!("{kind}:{id}"), value).is_some() {
                return Err(MemoryPortError::invalid(
                    "legacy snapshot contains empty or duplicate typed IDs",
                ));
            }
            Ok(())
        };
        macro_rules! row {
            ($kind:expr, $id:expr, $value:expr) => {
                add(
                    $kind,
                    $id,
                    serde_json::to_string($value)
                        .map_err(|_| MemoryPortError::invalid("legacy row cannot be serialized"))?,
                )?
            };
        }
        for row in &self.snapshot.memories {
            row!("memory", &row.id, row);
        }
        for row in &self.snapshot.memory_entities {
            row!(&format!("entity/{}", row.kind.as_str()), &row.id, row);
        }
        for row in &self.snapshot.agent_relationships {
            row!("relationship", &row.id, row);
        }
        for row in &self.snapshot.temporal_facts {
            row!("temporal_fact", &row.id, row);
        }
        for row in &self.snapshot.temporal_relationships {
            row!("temporal_relationship", &row.id, row);
        }
        Ok(rows)
    }
}
