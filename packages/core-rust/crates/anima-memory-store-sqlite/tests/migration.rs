use anima_core::{
    IdentityBinding, ManualExecutionClock, MemoryKind, MemoryPortErrorCode, MemoryProvenance,
    MemoryRecord, MemoryScope, MemoryWrite,
};
use anima_memory::{IdentityRegistry, MemoryAuthority, RelationshipObservation};
use anima_memory_store_sqlite::{MemoryImportManifest, SqliteMemoryStore};
use std::sync::Arc;
fn record(id: &str, text: &str, scope: MemoryScope) -> MemoryRecord {
    MemoryRecord::from_write(
        MemoryWrite::new(
            id,
            MemoryKind::Fact,
            scope,
            text,
            0.8,
            MemoryProvenance::new("legacy-unknown", "copied-fixture", 1).unwrap(),
        )
        .unwrap(),
        1,
    )
    .unwrap()
}
#[tokio::test]
async fn copied_manifest_is_idempotent_collision_safe_and_rolls_back_as_one_unit() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("import.sqlite");
    let store = SqliteMemoryStore::open(&path, Arc::new(ManualExecutionClock::default()))
        .await
        .unwrap();
    let id = IdentityBinding::new("owner", "tavi").unwrap();
    let scope = MemoryScope::agent("tavi").unwrap();
    let auth =
        MemoryAuthority::trusted(id.clone(), vec![scope.clone()], vec![scope.clone()]).unwrap();
    store.register_identity(id.clone()).await.unwrap();
    let bound = store.bind(auth).await.unwrap();
    let copied = record("legacy", "remembers apples", scope.clone());
    let original = copied.clone();
    let manifest =
        MemoryImportManifest::new("fixture-v1", id.clone(), vec![copied], vec![]).unwrap();
    assert!(
        !bound
            .import_manifest(manifest.clone())
            .await
            .unwrap()
            .replayed
    );
    assert!(bound.import_manifest(manifest).await.unwrap().replayed);
    assert_eq!(original.content(), "remembers apples");
    let bad = MemoryImportManifest::new(
        "fixture-v2",
        id.clone(),
        vec![
            record("first", "must roll back", scope.clone()),
            record("legacy", "conflicting apples", scope.clone()),
        ],
        vec![],
    )
    .unwrap();
    assert_eq!(
        bound.import_manifest(bad).await.unwrap_err().code,
        MemoryPortErrorCode::Conflict
    );
    assert_eq!(bound.recent(10).await.unwrap().len(), 1);
    let corrected = MemoryImportManifest::new(
        "fixture-v2",
        id,
        vec![record("first", "must roll back", scope.clone())],
        vec![],
    )
    .unwrap();
    assert!(!bound.import_manifest(corrected).await.unwrap().replayed);
    assert_eq!(bound.recent(10).await.unwrap().len(), 2);
    store.shutdown().await.unwrap();
    let reopened = SqliteMemoryStore::open(path, Arc::new(ManualExecutionClock::default()))
        .await
        .unwrap();
    assert_eq!(
        reopened
            .bind(bound.authority().clone())
            .await
            .unwrap()
            .recent(10)
            .await
            .unwrap()
            .len(),
        2
    );
    reopened.shutdown().await.unwrap();
}

#[tokio::test]
async fn historical_import_preserves_forgotten_correction_targets() {
    let temp = tempfile::tempdir().unwrap();
    let store = SqliteMemoryStore::open(
        temp.path().join("history.sqlite"),
        Arc::new(ManualExecutionClock::default()),
    )
    .await
    .unwrap();
    let id = IdentityBinding::new("owner", "tavi").unwrap();
    let scope = MemoryScope::agent("tavi").unwrap();
    let auth =
        MemoryAuthority::trusted(id.clone(), vec![scope.clone()], vec![scope.clone()]).unwrap();
    store.register_identity(id.clone()).await.unwrap();
    let bound = store.bind(auth).await.unwrap();
    let mut forgotten = serde_json::to_value(record("old", "old apples", scope.clone())).unwrap();
    forgotten["revision"] = 2.into();
    forgotten["forgotten_at_ms"] = 50.into();
    forgotten["forget_reason"] = "user deletion".into();
    let forgotten: MemoryRecord = serde_json::from_value(forgotten).unwrap();
    let mut corrected = serde_json::to_value(record("new", "new apples", scope)).unwrap();
    corrected["revision"] = 2.into();
    corrected["supersedes"] = serde_json::json!(["old"]);
    corrected["corrects"] = "old".into();
    let corrected: MemoryRecord = serde_json::from_value(corrected).unwrap();
    store
        .register_identity(IdentityBinding::new("owner", "mira").unwrap())
        .await
        .unwrap();
    let historical_relationship = RelationshipObservation::new(
        "legacy-relation",
        MemoryScope::agent("tavi").unwrap(),
        anima_core::IndividualId::new("mira").unwrap(),
        "trust",
        "old evidence",
        0.8,
        0.9,
        1,
        vec!["old".into()],
    )
    .unwrap();
    bound
        .import_manifest(
            MemoryImportManifest::new(
                "historical",
                id,
                vec![forgotten, corrected],
                vec![historical_relationship],
            )
            .unwrap(),
        )
        .await
        .unwrap();
    let records = bound.recent(10).await.unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id(), "new");
    assert_eq!(records[0].corrects(), Some("old"));
    assert!(bound.relationships(10).await.unwrap().is_empty());
    store.shutdown().await.unwrap();
}
