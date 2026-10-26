//! Explicit authenticated owner statements; model text cannot select an observer or scope.
use super::*;
use anima_memory::{MemoryEffectReceipt, RelationshipObservation, ValidatedMemoryEffect};
use anima_memory_store_sqlite::BoundMemoryStore;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MemorySourceRequest {
    pub source_event_id: Uuid,
    operation: SourceOperation,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum SourceOperation {
    Correction {
        memory_id: String,
        expected_revision: u64,
        corrects: String,
        reason: String,
    },
    Forget {
        memory_id: String,
        expected_revision: u64,
        reason: String,
    },
    Relationship {
        target_agent_id: String,
        relationship_type: String,
        summary: String,
        strength: f64,
        confidence: f64,
        evidence_memory_ids: Vec<String>,
        valid_from_ms: Option<u64>,
        valid_to_ms: Option<u64>,
    },
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AcceptedMemorySource {
    pub version: u32,
    pub agent_id: String,
    pub identity: IdentityBinding,
    pub request: MemorySourceRequest,
    pub source_kind: EvidenceKind,
    pub observed_at_ms: u64,
    pub effect: MemoryEffect,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum MemorySourceOutcome {
    Applied { receipt: MemoryEffectReceipt },
    Rejected { code: MemoryPortErrorCode },
}
#[derive(Clone)]
pub(super) struct StoredMemorySource {
    pub accepted: AcceptedMemorySource,
    pub outcome: Option<MemorySourceOutcome>,
}

fn effect(
    identity: &IdentityBinding,
    request: &MemorySourceRequest,
    observed_at_ms: u64,
) -> Result<MemoryEffect, BridgeError> {
    if request.source_event_id.is_nil() {
        return Err(BridgeError::Invalid);
    }
    let source = format!(
        "daemon:memory:{}:{}",
        request.source_event_id,
        identity.individual_id().as_str()
    );
    let scope = MemoryScope::agent(identity.individual_id().as_str()).map_err(port_error)?;
    let (candidates, revisions, relationships) = match &request.operation {
        SourceOperation::Correction {
            memory_id,
            expected_revision,
            corrects,
            reason,
        } => {
            if memory_id == corrects {
                return Err(BridgeError::Invalid);
            }
            (
                vec![],
                vec![
                    MemoryRevision::correction(memory_id, *expected_revision, corrects, reason)
                        .map_err(port_error)?,
                ],
                vec![],
            )
        }
        SourceOperation::Forget {
            memory_id,
            expected_revision,
            reason,
        } => (
            vec![],
            vec![
                MemoryRevision::forget(memory_id, *expected_revision, reason)
                    .map_err(port_error)?,
            ],
            vec![],
        ),
        SourceOperation::Relationship {
            target_agent_id,
            relationship_type,
            summary,
            strength,
            confidence,
            evidence_memory_ids,
            valid_from_ms,
            valid_to_ms,
        } => {
            if evidence_memory_ids.is_empty() || evidence_memory_ids.len() > 63 {
                return Err(BridgeError::Invalid);
            }
            let statement_id = format!(
                "daemon:statement:{}:{}",
                request.source_event_id,
                identity.individual_id().as_str()
            );
            let candidate = MemoryCandidate::new(
                MemoryWrite::new(
                    &statement_id,
                    MemoryKind::Episode,
                    scope.clone(),
                    summary,
                    1.0,
                    MemoryProvenance::new("daemon/owner-user-statement", &source, observed_at_ms)
                        .map_err(port_error)?,
                )
                .map_err(port_error)?,
                CandidateEvidence::new(EvidenceKind::UserStatement, &source, None)
                    .map_err(port_error)?,
            )
            .map_err(port_error)?;
            let mut evidence = evidence_memory_ids.clone();
            evidence.push(statement_id);
            let relationship = RelationshipObservation::new(
                format!("daemon:relationship:{}", request.source_event_id),
                scope,
                IndividualId::new(target_agent_id).map_err(port_error)?,
                relationship_type,
                summary,
                *strength,
                *confidence,
                observed_at_ms,
                evidence,
            )
            .map_err(port_error)?
            .with_validity(*valid_from_ms, *valid_to_ms)
            .map_err(port_error)?;
            (vec![candidate], vec![], vec![relationship])
        }
    };
    MemoryEffect::new(
        source,
        identity.clone(),
        candidates,
        revisions,
        relationships,
    )
    .map_err(port_error)
}
struct SourcePolicy {
    expected: MemoryEffect,
}
impl MemoryProjectionPolicy for SourcePolicy {
    fn validate(
        &self,
        _: &MemoryAuthority,
        candidate: &MemoryCandidate,
    ) -> Result<(), MemoryPortError> {
        if self.expected.candidates().contains(candidate)
            && candidate.evidence().kind() == EvidenceKind::UserStatement
        {
            Ok(())
        } else {
            Err(MemoryPortError::unsupported(
                "owner source does not authorize this candidate",
            ))
        }
    }
    fn validate_revision(
        &self,
        _: &MemoryAuthority,
        revision: &MemoryRevision,
    ) -> Result<(), MemoryPortError> {
        if self.expected.revisions().contains(revision) {
            Ok(())
        } else {
            Err(MemoryPortError::unsupported(
                "owner source does not authorize this lifecycle mutation",
            ))
        }
    }
    fn validate_relationship(
        &self,
        _: &MemoryAuthority,
        relationship: &RelationshipObservation,
    ) -> Result<(), MemoryPortError> {
        if self.expected.relationships().contains(relationship) {
            Ok(())
        } else {
            Err(MemoryPortError::unsupported(
                "owner source does not authorize this relationship",
            ))
        }
    }
}
impl AcceptedMemorySource {
    fn validated(&self, authority: &MemoryAuthority) -> Result<ValidatedMemoryEffect, BridgeError> {
        if self.version != 1
            || &self.identity != authority.identity()
            || self.source_kind != EvidenceKind::UserStatement
        {
            return Err(BridgeError::Unavailable);
        }
        let expected = effect(&self.identity, &self.request, self.observed_at_ms)?;
        if expected != self.effect {
            return Err(BridgeError::Unavailable);
        }
        self.effect
            .clone()
            .validate(authority, &SourcePolicy { expected })
            .map_err(port_error)
    }
}
pub(super) fn port_error(error: MemoryPortError) -> BridgeError {
    match error.code {
        MemoryPortErrorCode::InvalidInput | MemoryPortErrorCode::Unsupported => {
            BridgeError::Invalid
        }
        MemoryPortErrorCode::UnauthorizedScope => BridgeError::Forbidden,
        MemoryPortErrorCode::NotFound => BridgeError::NotFound,
        MemoryPortErrorCode::Conflict => BridgeError::Conflict,
        MemoryPortErrorCode::Backend => BridgeError::MemoryUnavailable,
    }
}
fn response(outcome: MemorySourceOutcome) -> Result<serde_json::Value, BridgeError> {
    match outcome {
        MemorySourceOutcome::Applied { receipt } => Ok(
            serde_json::json!({"sourceEventId":receipt.source_event_id,"individualId":receipt.identity.individual_id().as_str(),"evidenceKind":"user_statement","memoryIds":receipt.memory_ids,"relationshipIds":receipt.relationship_ids}),
        ),
        MemorySourceOutcome::Rejected { code } => Err(port_error(MemoryPortError {
            code,
            message: "accepted owner memory source was rejected by canonical validation".into(),
        })),
    }
}

impl DaemonEngineBridge {
    pub(crate) async fn memory_source(
        &self,
        agent: &str,
        request: MemorySourceRequest,
    ) -> Result<serde_json::Value, BridgeError> {
        let _work = self.track()?;
        self.memory_source_reserved(agent, request).await
    }
    pub(crate) async fn memory_source_reserved(
        &self,
        agent: &str,
        request: MemorySourceRequest,
    ) -> Result<serde_json::Value, BridgeError> {
        let (identity, gate) = {
            let profiles = self.profiles.lock().map_err(|_| BridgeError::Unavailable)?;
            let profile = profiles.get(agent).ok_or(BridgeError::NotFound)?;
            (profile.profile.identity.clone(), profile.gate.clone())
        };
        let _gate = gate.try_lock_owned().map_err(|_| BridgeError::Busy)?;
        // Validate the bounded operation shape without depending on current
        // canonical lifecycle state, preserving exact committed replay.
        effect(&identity, &request, self.clock.now_ms())?;
        // An exact committed retry must survive subsequent revisions/forgetting.
        if let Some(stored) = self
            .outbox
            .memory_source(&identity, request.source_event_id)
            .await?
        {
            if stored.accepted.request != request {
                return Err(BridgeError::Conflict);
            }
            if let Some(outcome) = stored.outcome {
                return response(outcome);
            }
        } else {
            self.project(&identity).await?;
            let authority = authority(&identity)?;
            let bound = self
                .memory
                .bind(authority.clone())
                .await
                .map_err(port_error)?;
            match &request.operation {
                SourceOperation::Correction {
                    memory_id,
                    expected_revision,
                    corrects,
                    ..
                } => {
                    let replacement = bound.get_active(memory_id).await.map_err(port_error)?;
                    if replacement.revision() != *expected_revision {
                        return Err(BridgeError::Conflict);
                    }
                    let target = bound.get_active(corrects).await.map_err(port_error)?;
                    if replacement.scope() != target.scope() {
                        return Err(BridgeError::Forbidden);
                    }
                }
                SourceOperation::Forget {
                    memory_id,
                    expected_revision,
                    ..
                } => {
                    let record = bound.get(memory_id).await.map_err(port_error)?;
                    if !authority.allows_write(record.scope()) {
                        return Err(BridgeError::Forbidden);
                    }
                    if record.revision() != *expected_revision || record.forgotten_at_ms().is_some()
                    {
                        return Err(BridgeError::Conflict);
                    }
                }
                SourceOperation::Relationship {
                    target_agent_id,
                    evidence_memory_ids,
                    ..
                } => {
                    let target = {
                        let profiles =
                            self.profiles.lock().map_err(|_| BridgeError::Unavailable)?;
                        profiles
                            .get(target_agent_id)
                            .ok_or(BridgeError::NotFound)?
                            .profile
                            .identity
                            .clone()
                    };
                    if target == identity || target.owner_id() != identity.owner_id() {
                        return Err(BridgeError::Forbidden);
                    }
                    for id in evidence_memory_ids {
                        let record = bound.get_active(id).await.map_err(port_error)?;
                        if !authority.allows_write(record.scope()) {
                            return Err(BridgeError::Forbidden);
                        }
                    }
                }
            }
            let observed_at_ms = self.clock.now_ms();
            let effect = effect(&identity, &request, observed_at_ms)?;
            self.outbox
                .accept_memory_source(AcceptedMemorySource {
                    version: 1,
                    agent_id: agent.into(),
                    identity: identity.clone(),
                    request: request.clone(),
                    source_kind: EvidenceKind::UserStatement,
                    observed_at_ms,
                    effect,
                })
                .await?;
        }
        self.project(&identity).await?;
        response(
            self.outbox
                .memory_source(&identity, request.source_event_id)
                .await?
                .and_then(|stored| stored.outcome)
                .ok_or(BridgeError::MemoryUnavailable)?,
        )
    }
    pub(super) async fn project_memory_sources(
        &self,
        identity: &IdentityBinding,
        bound: &BoundMemoryStore,
        authority: &MemoryAuthority,
    ) -> Result<(), BridgeError> {
        for accepted in self.outbox.pending_memory_sources(identity).await? {
            let validated = accepted.validated(authority)?;
            match bound.apply_effect(validated).await {
                Ok(outcome) => {
                    self.outbox
                        .finish_memory_source(
                            &accepted,
                            MemorySourceOutcome::Applied {
                                receipt: outcome.receipt,
                            },
                        )
                        .await?
                }
                Err(error) if error.code == MemoryPortErrorCode::Backend => {
                    return Err(BridgeError::MemoryUnavailable)
                }
                Err(error) => {
                    let code = error.code;
                    self.outbox
                        .finish_memory_source(&accepted, MemorySourceOutcome::Rejected { code })
                        .await?;
                    return Err(port_error(error));
                }
            }
        }
        Ok(())
    }
    pub(crate) async fn relationships(
        &self,
        agent: &str,
    ) -> Result<serde_json::Value, BridgeError> {
        let _work = self.track()?;
        let identity = {
            let profiles = self.profiles.lock().map_err(|_| BridgeError::Unavailable)?;
            profiles
                .get(agent)
                .ok_or(BridgeError::NotFound)?
                .profile
                .identity
                .clone()
        };
        self.project(&identity).await?;
        let bound = self
            .memory
            .bind(authority(&identity)?)
            .await
            .map_err(port_error)?;
        let relationships = bound.relationships(64).await.map_err(port_error)?;
        Ok(
            serde_json::json!({"relationships":relationships.into_iter().map(|view|serde_json::json!({"observer":view.observer,"observation":view.observation})).collect::<Vec<_>>() }),
        )
    }
}
