use anima_core::{
    IdentityBinding, IndividualId, ManualExecutionClock, MemoryKind, MemoryPort, MemoryPortError,
    MemoryPortErrorCode, MemoryProvenance, MemoryRevision, MemoryScope, MemoryWrite,
};
use anima_memory::{
    CandidateEvidence, EvidenceKind, IdentityRegistry, MemoryAuthority, MemoryCandidate,
    MemoryEffect, MemoryProjectionPolicy, MemoryProjectionPort, RelationshipObservation,
};
use anima_memory_store_sqlite::SqliteMemoryStore;
use std::sync::Arc;

struct Accepted;
impl MemoryProjectionPolicy for Accepted {
    fn validate(&self, _: &MemoryAuthority, _: &MemoryCandidate) -> Result<(), MemoryPortError> {
        Ok(())
    }
    fn validate_relationship(
        &self,
        _: &MemoryAuthority,
        _: &RelationshipObservation,
    ) -> Result<(), MemoryPortError> {
        Ok(())
    }
    fn validate_revision(
        &self,
        _: &MemoryAuthority,
        _: &MemoryRevision,
    ) -> Result<(), MemoryPortError> {
        Ok(())
    }
}
fn candidate(id: &str, content: &str, scope: MemoryScope) -> MemoryCandidate {
    MemoryCandidate::new(
        MemoryWrite::new(
            id,
            MemoryKind::Episode,
            scope,
            content,
            0.8,
            MemoryProvenance::new("witnessed", "event", 10).unwrap(),
        )
        .unwrap(),
        CandidateEvidence::new(EvidenceKind::WitnessedEvent, "event", None).unwrap(),
    )
    .unwrap()
}
#[tokio::test]
async fn effect_receipt_replays_after_restart_and_relationships_follow_visible_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("effect.sqlite");
    let clock = Arc::new(ManualExecutionClock::new(100));
    let identity = IdentityBinding::new("owner", "tavi").unwrap();
    let scope = MemoryScope::agent("tavi").unwrap();
    let authority =
        MemoryAuthority::trusted(identity.clone(), vec![scope.clone()], vec![scope.clone()])
            .unwrap();
    let relation = RelationshipObservation::new(
        "friendship",
        scope.clone(),
        IndividualId::new("mira").unwrap(),
        "trust",
        "Mira helped me",
        0.8,
        0.9,
        10,
        vec!["episode".into()],
    )
    .unwrap();
    let effect = MemoryEffect::new(
        "event",
        identity.clone(),
        vec![candidate("episode", "Mira helped me", scope.clone())],
        vec![],
        vec![relation],
    )
    .unwrap()
    .validate(&authority, &Accepted)
    .unwrap();
    let store = SqliteMemoryStore::open(&path, clock.clone()).await.unwrap();
    store.register_identity(identity.clone()).await.unwrap();
    store
        .register_identity(IdentityBinding::new("owner", "mira").unwrap())
        .await
        .unwrap();
    let bound = store.bind(authority.clone()).await.unwrap();
    assert!(!bound.apply_effect(effect.clone()).await.unwrap().replayed);
    assert_eq!(bound.relationships(10).await.unwrap()[0].observer, identity);
    store.shutdown().await.unwrap();
    let reopened = SqliteMemoryStore::open(&path, clock).await.unwrap();
    let bound = reopened.bind(authority.clone()).await.unwrap();
    assert!(bound.apply_effect(effect).await.unwrap().replayed);
    assert_eq!(bound.recent(10).await.unwrap().len(), 1);
    let conflicting = MemoryEffect::new(
        "event",
        identity,
        vec![candidate("episode", "different observation", scope.clone())],
        vec![],
        vec![],
    )
    .unwrap()
    .validate(&authority, &Accepted)
    .unwrap();
    assert_eq!(
        bound.apply_effect(conflicting).await.unwrap_err().code,
        MemoryPortErrorCode::Conflict
    );
    let mira_scope = MemoryScope::agent("mira").unwrap();
    let mira = MemoryAuthority::trusted(
        IdentityBinding::new("owner", "mira").unwrap(),
        vec![mira_scope.clone()],
        vec![mira_scope],
    )
    .unwrap();
    assert!(reopened
        .bind(mira)
        .await
        .unwrap()
        .relationships(10)
        .await
        .unwrap()
        .is_empty());
    bound
        .revise(MemoryRevision::forget("episode", 1, "withdraw evidence").unwrap())
        .await
        .unwrap();
    assert!(bound.relationships(10).await.unwrap().is_empty());
    reopened.shutdown().await.unwrap();
}

#[tokio::test]
async fn receipt_replay_does_not_bypass_a_narrowed_writer_grant() {
    let temp = tempfile::tempdir().unwrap();
    let store = SqliteMemoryStore::open(
        temp.path().join("grant.sqlite"),
        Arc::new(ManualExecutionClock::default()),
    )
    .await
    .unwrap();
    let identity = IdentityBinding::new("owner", "tavi").unwrap();
    let private = MemoryScope::agent("tavi").unwrap();
    let shared = MemoryScope::workspace("world:a").unwrap();
    let broad = MemoryAuthority::trusted(
        identity.clone(),
        vec![private.clone(), shared.clone()],
        vec![private.clone(), shared.clone()],
    )
    .unwrap();
    store.register_identity(identity.clone()).await.unwrap();
    let bound = store.bind(broad.clone()).await.unwrap();
    let effect = MemoryEffect::new(
        "event",
        identity,
        vec![candidate("shared", "public event", shared)],
        vec![],
        vec![],
    )
    .unwrap()
    .validate(&broad, &Accepted)
    .unwrap();
    bound.apply_effect(effect.clone()).await.unwrap();
    let narrow = MemoryAuthority::trusted(
        broad.identity().clone(),
        vec![private.clone()],
        vec![private],
    )
    .unwrap();
    assert_eq!(
        store
            .bind(narrow)
            .await
            .unwrap()
            .apply_effect(effect)
            .await
            .unwrap_err()
            .code,
        MemoryPortErrorCode::UnauthorizedScope
    );
    store.shutdown().await.unwrap();
}

#[tokio::test]
async fn correction_candidate_and_lifecycle_are_one_atomic_source_effect() {
    let temp = tempfile::tempdir().unwrap();
    let store = SqliteMemoryStore::open(
        temp.path().join("correction.sqlite"),
        Arc::new(ManualExecutionClock::default()),
    )
    .await
    .unwrap();
    let identity = IdentityBinding::new("owner", "tavi").unwrap();
    let scope = MemoryScope::agent("tavi").unwrap();
    let auth = MemoryAuthority::trusted(identity.clone(), vec![scope.clone()], vec![scope.clone()])
        .unwrap();
    store.register_identity(identity.clone()).await.unwrap();
    let bound = store.bind(auth.clone()).await.unwrap();
    bound
        .write(candidate("old", "old tea", scope.clone()).write().clone())
        .await
        .unwrap();
    let effect = MemoryEffect::new(
        "event",
        identity,
        vec![candidate("new", "new tea", scope)],
        vec![MemoryRevision::correction("new", 1, "old", "corrected user statement").unwrap()],
        vec![],
    )
    .unwrap()
    .validate(&auth, &Accepted)
    .unwrap();
    let first = bound.apply_effect(effect.clone()).await.unwrap();
    assert!(!first.replayed);
    let records = bound.recent(10).await.unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id(), "new");
    assert_eq!(records[0].revision(), 2);
    assert!(bound.apply_effect(effect).await.unwrap().replayed);
    store.shutdown().await.unwrap();
}
