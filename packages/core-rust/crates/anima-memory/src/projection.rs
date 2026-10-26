//! Sourced observations validated by host policy before retry-safe projection.
//!
//! Hosts commit accepted events and pending effects in their own transaction, then apply an
//! effect and acknowledge its receipt. These are separate stores: acknowledgement may be retried.
use crate::{access::unauthorized, MemoryAuthority};
use anima_core::{
    IdentityBinding, IndividualId, MemoryPortError, MemoryRevision, MemoryScope, MemoryWrite,
};
use async_trait::async_trait;
use serde::de::{SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

pub const MAX_MEMORY_EFFECT_ITEMS: usize = 64;
pub const MAX_MEMORY_EFFECT_BYTES: usize = 128 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    WitnessedEvent,
    UserStatement,
    ReceivedMessage,
    Inference,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "EvidenceWire")]
pub struct CandidateEvidence {
    kind: EvidenceKind,
    source_event_id: String,
    source_individual: Option<IndividualId>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EvidenceWire {
    kind: EvidenceKind,
    source_event_id: String,
    source_individual: Option<IndividualId>,
}
impl TryFrom<EvidenceWire> for CandidateEvidence {
    type Error = MemoryPortError;
    fn try_from(w: EvidenceWire) -> Result<Self, Self::Error> {
        Self::new(w.kind, w.source_event_id, w.source_individual)
    }
}
impl CandidateEvidence {
    pub fn new(
        kind: EvidenceKind,
        source_event_id: impl Into<String>,
        source_individual: Option<IndividualId>,
    ) -> Result<Self, MemoryPortError> {
        if (kind == EvidenceKind::ReceivedMessage) != source_individual.is_some() {
            return Err(MemoryPortError::invalid(
                "received messages require their sender; other evidence must not invent a sender",
            ));
        }
        Ok(Self {
            kind,
            source_event_id: identifier(source_event_id.into())?,
            source_individual,
        })
    }
    pub fn kind(&self) -> EvidenceKind {
        self.kind
    }
    pub fn source_event_id(&self) -> &str {
        &self.source_event_id
    }
    pub fn source_individual(&self) -> Option<&IndividualId> {
        self.source_individual.as_ref()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "CandidateWire")]
pub struct MemoryCandidate {
    write: MemoryWrite,
    evidence: CandidateEvidence,
}
impl MemoryCandidate {
    pub fn new(write: MemoryWrite, evidence: CandidateEvidence) -> Result<Self, MemoryPortError> {
        if write.provenance().source_identity() != evidence.source_event_id() {
            return Err(MemoryPortError::invalid(
                "candidate provenance must reference its source event",
            ));
        }
        Ok(Self { write, evidence })
    }
    pub fn write(&self) -> &MemoryWrite {
        &self.write
    }
    pub fn evidence(&self) -> &CandidateEvidence {
        &self.evidence
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CandidateWire {
    write: MemoryWrite,
    evidence: CandidateEvidence,
}
impl TryFrom<CandidateWire> for MemoryCandidate {
    type Error = MemoryPortError;
    fn try_from(w: CandidateWire) -> Result<Self, Self::Error> {
        Self::new(w.write, w.evidence)
    }
}

/// Directed perspective: the observer is the effect's bound individual, never the target.
/// Fields retain the legacy relationship semantics and temporal validity/evidence links.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "RelationshipWire")]
pub struct RelationshipObservation {
    id: String,
    scope: MemoryScope,
    target: IndividualId,
    relationship_type: String,
    summary: String,
    strength: f64,
    confidence: f64,
    observed_at_ms: u64,
    evidence_memory_ids: Vec<String>,
    valid_from_ms: Option<u64>,
    valid_to_ms: Option<u64>,
}
impl RelationshipObservation {
    pub fn new(
        id: impl Into<String>,
        scope: MemoryScope,
        target: IndividualId,
        relationship_type: impl Into<String>,
        summary: impl Into<String>,
        strength: f64,
        confidence: f64,
        observed_at_ms: u64,
        evidence_memory_ids: Vec<String>,
    ) -> Result<Self, MemoryPortError> {
        let summary = summary.into();
        if summary.trim().is_empty()
            || summary.len() > 4096
            || !strength.is_finite()
            || !(0.0..=1.0).contains(&strength)
            || !confidence.is_finite()
            || !(0.0..=1.0).contains(&confidence)
            || evidence_memory_ids.is_empty()
            || evidence_memory_ids.len() > 64
        {
            return Err(MemoryPortError::invalid(
                "invalid relationship summary, strength, confidence or evidence",
            ));
        }
        let mut evidence = Vec::new();
        for id in evidence_memory_ids {
            let id = identifier(id)?;
            if evidence.contains(&id) {
                return Err(MemoryPortError::invalid("duplicate relationship evidence"));
            }
            evidence.push(id);
        }
        Ok(Self {
            id: identifier(id.into())?,
            scope,
            target,
            relationship_type: identifier(relationship_type.into())?,
            summary,
            strength,
            confidence,
            observed_at_ms,
            evidence_memory_ids: evidence,
            valid_from_ms: None,
            valid_to_ms: None,
        })
    }
    pub fn with_validity(
        mut self,
        from_ms: Option<u64>,
        to_ms: Option<u64>,
    ) -> Result<Self, MemoryPortError> {
        if matches!((from_ms, to_ms),(Some(from),Some(to)) if from >= to) {
            return Err(MemoryPortError::invalid(
                "relationship validity must be increasing",
            ));
        }
        self.valid_from_ms = from_ms;
        self.valid_to_ms = to_ms;
        Ok(self)
    }
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn scope(&self) -> &MemoryScope {
        &self.scope
    }
    pub fn target(&self) -> &IndividualId {
        &self.target
    }
    pub fn relationship_type(&self) -> &str {
        &self.relationship_type
    }
    pub fn summary(&self) -> &str {
        &self.summary
    }
    pub fn strength(&self) -> f64 {
        self.strength
    }
    pub fn confidence(&self) -> f64 {
        self.confidence
    }
    pub fn observed_at_ms(&self) -> u64 {
        self.observed_at_ms
    }
    pub fn evidence_memory_ids(&self) -> &[String] {
        &self.evidence_memory_ids
    }
    pub fn valid_from_ms(&self) -> Option<u64> {
        self.valid_from_ms
    }
    pub fn valid_to_ms(&self) -> Option<u64> {
        self.valid_to_ms
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RelationshipWire {
    id: String,
    scope: MemoryScope,
    target: IndividualId,
    relationship_type: String,
    summary: String,
    strength: f64,
    confidence: f64,
    observed_at_ms: u64,
    evidence_memory_ids: EffectItems<String>,
    valid_from_ms: Option<u64>,
    valid_to_ms: Option<u64>,
}
impl TryFrom<RelationshipWire> for RelationshipObservation {
    type Error = MemoryPortError;
    fn try_from(w: RelationshipWire) -> Result<Self, Self::Error> {
        Self::new(
            w.id,
            w.scope,
            w.target,
            w.relationship_type,
            w.summary,
            w.strength,
            w.confidence,
            w.observed_at_ms,
            w.evidence_memory_ids.0,
        )?
        .with_validity(w.valid_from_ms, w.valid_to_ms)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "EffectWire")]
pub struct MemoryEffect {
    source_event_id: String,
    identity: IdentityBinding,
    candidates: Vec<MemoryCandidate>,
    revisions: Vec<MemoryRevision>,
    relationships: Vec<RelationshipObservation>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EffectWire {
    source_event_id: String,
    identity: IdentityBinding,
    candidates: EffectItems<MemoryCandidate>,
    revisions: EffectItems<MemoryRevision>,
    relationships: EffectItems<RelationshipObservation>,
}
impl TryFrom<EffectWire> for MemoryEffect {
    type Error = MemoryPortError;
    fn try_from(w: EffectWire) -> Result<Self, Self::Error> {
        Self::new(
            w.source_event_id,
            w.identity,
            w.candidates.0,
            w.revisions.0,
            w.relationships.0,
        )
    }
}
impl MemoryEffect {
    pub fn new(
        source_event_id: impl Into<String>,
        identity: IdentityBinding,
        candidates: Vec<MemoryCandidate>,
        revisions: Vec<MemoryRevision>,
        relationships: Vec<RelationshipObservation>,
    ) -> Result<Self, MemoryPortError> {
        let source_event_id = identifier(source_event_id.into())?;
        let count = candidates
            .len()
            .saturating_add(revisions.len())
            .saturating_add(relationships.len());
        if count == 0 || count > MAX_MEMORY_EFFECT_ITEMS {
            return Err(MemoryPortError::invalid(
                "memory effects require 1..64 items",
            ));
        }
        if candidates
            .iter()
            .any(|c| c.evidence.source_event_id != source_event_id)
        {
            return Err(MemoryPortError::invalid(
                "candidate source event does not match effect",
            ));
        }
        let effect = Self {
            source_event_id,
            identity,
            candidates,
            revisions,
            relationships,
        };
        if serde_json::to_vec(&effect)
            .map_err(|_| MemoryPortError::invalid("invalid effect"))?
            .len()
            > MAX_MEMORY_EFFECT_BYTES
        {
            return Err(MemoryPortError::invalid("memory effect exceeds byte limit"));
        }
        Ok(effect)
    }
    pub fn validate(
        self,
        authority: &MemoryAuthority,
        policy: &dyn MemoryProjectionPolicy,
    ) -> Result<ValidatedMemoryEffect, MemoryPortError> {
        if &self.identity != authority.identity() {
            return Err(unauthorized());
        }
        for candidate in &self.candidates {
            if !authority.allows_write(candidate.write.scope()) {
                return Err(unauthorized());
            }
            policy.validate(authority, candidate)?;
        }
        for revision in &self.revisions {
            policy.validate_revision(authority, revision)?;
        }
        for relationship in &self.relationships {
            if !authority.allows_write(relationship.scope()) {
                return Err(unauthorized());
            }
            policy.validate_relationship(authority, relationship)?;
        }
        Ok(ValidatedMemoryEffect(self))
    }
    pub fn source_event_id(&self) -> &str {
        &self.source_event_id
    }
    pub fn identity(&self) -> &IdentityBinding {
        &self.identity
    }
    pub fn candidates(&self) -> &[MemoryCandidate] {
        &self.candidates
    }
    pub fn revisions(&self) -> &[MemoryRevision] {
        &self.revisions
    }
    pub fn relationships(&self) -> &[RelationshipObservation] {
        &self.relationships
    }
}

/// No Deserialize: serialized model text cannot manufacture host validation.
#[derive(Clone, Debug)]
pub struct ValidatedMemoryEffect(MemoryEffect);
impl ValidatedMemoryEffect {
    pub fn effect(&self) -> &MemoryEffect {
        &self.0
    }
}

pub trait MemoryProjectionPolicy: Send + Sync {
    fn validate(
        &self,
        authority: &MemoryAuthority,
        candidate: &MemoryCandidate,
    ) -> Result<(), MemoryPortError>;
    fn validate_revision(
        &self,
        _authority: &MemoryAuthority,
        _revision: &MemoryRevision,
    ) -> Result<(), MemoryPortError> {
        Err(MemoryPortError::unsupported(
            "host policy must validate lifecycle changes",
        ))
    }
    fn validate_relationship(
        &self,
        _authority: &MemoryAuthority,
        _relationship: &RelationshipObservation,
    ) -> Result<(), MemoryPortError> {
        Err(MemoryPortError::unsupported(
            "host policy must validate relationship observations",
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryEffectReceipt {
    pub source_event_id: String,
    pub identity: IdentityBinding,
    pub memory_ids: Vec<String>,
    pub relationship_ids: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryProjectionOutcome {
    pub receipt: MemoryEffectReceipt,
    pub replayed: bool,
}
#[async_trait]
pub trait MemoryProjectionPort: Send + Sync {
    async fn apply_effect(
        &self,
        effect: ValidatedMemoryEffect,
    ) -> Result<MemoryProjectionOutcome, MemoryPortError>;
}

fn identifier(value: String) -> Result<String, MemoryPortError> {
    if value.is_empty()
        || value.len() > 256
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(MemoryPortError::invalid(
            "invalid source or relationship ID",
        ));
    }
    Ok(value)
}

struct EffectItems<T>(Vec<T>);
impl<'de, T: Deserialize<'de>> Deserialize<'de> for EffectItems<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ItemsVisitor<T>(std::marker::PhantomData<T>);
        impl<'de, T: Deserialize<'de>> Visitor<'de> for ItemsVisitor<T> {
            type Value = EffectItems<T>;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("at most 64 effect items")
            }
            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut sequence: A,
            ) -> Result<Self::Value, A::Error> {
                if sequence
                    .size_hint()
                    .is_some_and(|size| size > MAX_MEMORY_EFFECT_ITEMS)
                {
                    return Err(serde::de::Error::custom("effect collection exceeds limit"));
                }
                let mut values = Vec::new();
                while let Some(value) = sequence.next_element()? {
                    if values.len() == MAX_MEMORY_EFFECT_ITEMS {
                        return Err(serde::de::Error::custom("effect collection exceeds limit"));
                    }
                    values.push(value);
                }
                Ok(EffectItems(values))
            }
        }
        deserializer.deserialize_seq(ItemsVisitor(std::marker::PhantomData))
    }
}
