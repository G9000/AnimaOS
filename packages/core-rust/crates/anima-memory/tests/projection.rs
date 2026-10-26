use anima_core::{
    IdentityBinding, MemoryKind, MemoryPortError, MemoryProvenance, MemoryScope, MemoryWrite,
};
use anima_memory::{
    CandidateEvidence, EvidenceKind, MemoryAuthority, MemoryCandidate, MemoryEffect,
    MemoryProjectionPolicy,
};

struct UserStatementsOnly;
impl MemoryProjectionPolicy for UserStatementsOnly {
    fn validate(
        &self,
        _authority: &MemoryAuthority,
        candidate: &MemoryCandidate,
    ) -> Result<(), MemoryPortError> {
        if candidate.evidence().kind() == EvidenceKind::UserStatement {
            Ok(())
        } else {
            Err(MemoryPortError::invalid("unsupported candidate"))
        }
    }
}
fn candidate(kind: EvidenceKind) -> MemoryCandidate {
    MemoryCandidate::new(
        MemoryWrite::new(
            "preference",
            MemoryKind::Preference,
            MemoryScope::agent("tavi").unwrap(),
            "I like tea",
            1.0,
            MemoryProvenance::new("conversation", "event-a", 1).unwrap(),
        )
        .unwrap(),
        CandidateEvidence::new(kind, "event-a", None).unwrap(),
    )
    .unwrap()
}
#[test]
fn projection_requires_host_policy_and_matching_event_identity_and_scope() {
    let id = IdentityBinding::new("owner", "tavi").unwrap();
    let scope = MemoryScope::agent("tavi").unwrap();
    let authority = MemoryAuthority::trusted(id.clone(), vec![scope.clone()], vec![scope]).unwrap();
    assert!(MemoryEffect::new(
        "event-a",
        id.clone(),
        vec![candidate(EvidenceKind::UserStatement)],
        vec![],
        vec![]
    )
    .unwrap()
    .validate(&authority, &UserStatementsOnly)
    .is_ok());
    assert!(MemoryEffect::new(
        "event-a",
        id.clone(),
        vec![candidate(EvidenceKind::Inference)],
        vec![],
        vec![]
    )
    .unwrap()
    .validate(&authority, &UserStatementsOnly)
    .is_err());
    assert!(MemoryEffect::new(
        "event-b",
        id.clone(),
        vec![candidate(EvidenceKind::UserStatement)],
        vec![],
        vec![]
    )
    .is_err());
    assert!(MemoryEffect::new(
        "event-a",
        id,
        vec![candidate(EvidenceKind::UserStatement); 65],
        vec![],
        vec![]
    )
    .is_err());
    assert!(CandidateEvidence::new(EvidenceKind::ReceivedMessage, "event-a", None).is_err());
}

#[test]
fn persisted_pending_effects_revalidate_and_reject_invalid_wire() {
    let identity = IdentityBinding::new("owner", "tavi").unwrap();
    let effect = MemoryEffect::new(
        "event-a",
        identity,
        vec![candidate(EvidenceKind::UserStatement)],
        vec![],
        vec![],
    )
    .unwrap();
    let wire = serde_json::to_value(&effect).unwrap();
    let restored: MemoryEffect = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(effect, restored);
    let mut invalid = wire.clone();
    invalid["source_event_id"] = "foreign-event".into();
    assert!(serde_json::from_value::<MemoryEffect>(invalid).is_err());
    let mut oversized = wire;
    oversized["candidates"] = serde_json::json!(vec![candidate(EvidenceKind::UserStatement); 65]);
    assert!(serde_json::from_value::<MemoryEffect>(oversized).is_err());
}
