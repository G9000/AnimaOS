use anima_core::{
    IdentityBinding, ManualExecutionClock, MemoryKind, MemoryPort, MemoryPortErrorCode,
    MemoryProvenance, MemoryQuery, MemoryRevision, MemoryScope, MemoryWrite,
};
use anima_memory::{IdentityRegistry, MemoryAuthority};
use anima_memory_store_sqlite::SqliteMemoryStore;
use std::sync::Arc;

fn write(id: &str, scope: MemoryScope, content: &str) -> MemoryWrite {
    MemoryWrite::new(
        id,
        MemoryKind::Preference,
        scope,
        content,
        1.0,
        MemoryProvenance::new("user", "event", 10).unwrap(),
    )
    .unwrap()
}
fn authority(owner: &str, agent: &str) -> MemoryAuthority {
    let scope = MemoryScope::agent(agent).unwrap();
    MemoryAuthority::trusted(
        IdentityBinding::new(owner, agent).unwrap(),
        vec![scope.clone()],
        vec![scope],
    )
    .unwrap()
}

#[tokio::test]
async fn repeated_parallel_initial_opens_share_one_memory_store_identity() {
    let temp = tempfile::tempdir().unwrap();
    for round in 0..64 {
        let path = temp.path().join(format!("parallel-{round}.sqlite"));
        let results =
            futures::future::join_all((0..8).map(|_| {
                SqliteMemoryStore::open(&path, Arc::new(ManualExecutionClock::default()))
            }))
            .await;
        let mut stores = Vec::new();
        let mut failures = Vec::new();
        for (index, result) in results.into_iter().enumerate() {
            match result {
                Ok(store) => stores.push(store),
                Err(error) => failures.push((index, error.code)),
            }
        }
        let mut identity = None;
        for store in stores {
            let id = store.store_id().await.unwrap();
            if let Some(expected) = &identity {
                assert_eq!(&id, expected);
            } else {
                identity = Some(id);
            }
            store.shutdown().await.unwrap();
        }
        assert!(
            failures.is_empty(),
            "round {round} initial memory open failures: {failures:?}"
        );
    }
}

#[tokio::test]
async fn trusted_lifecycle_get_inspects_own_forgotten_record_without_widening_visibility() {
    let temp = tempfile::tempdir().unwrap();
    let clock = Arc::new(ManualExecutionClock::new(100));
    let store = SqliteMemoryStore::open(temp.path().join("inspect.sqlite"), clock.clone())
        .await
        .unwrap();
    let a = authority("owner", "tavi");
    let b = authority("owner", "mira");
    let foreign = authority("other-owner", "tavi");
    for authority in [&a, &b, &foreign] {
        store
            .register_identity(authority.identity().clone())
            .await
            .unwrap();
    }
    let bound = store.bind(a.clone()).await.unwrap();
    bound
        .write(write(
            "forgotten",
            MemoryScope::agent("tavi").unwrap(),
            "private tea",
        ))
        .await
        .unwrap();
    bound
        .revise(MemoryRevision::forget("forgotten", 1, "owner request").unwrap())
        .await
        .unwrap();
    assert!(bound
        .get("forgotten")
        .await
        .unwrap()
        .forgotten_at_ms()
        .is_some());
    assert!(bound.recent(10).await.unwrap().is_empty());
    assert_eq!(
        bound.get_active("forgotten").await.unwrap_err().code,
        MemoryPortErrorCode::Conflict
    );
    bound
        .write(write(
            "evidence",
            MemoryScope::agent("tavi").unwrap(),
            "old evidence",
        ))
        .await
        .unwrap();
    bound
        .write(write(
            "replacement",
            MemoryScope::agent("tavi").unwrap(),
            "new evidence",
        ))
        .await
        .unwrap();
    bound
        .revise(MemoryRevision::correction("replacement", 1, "evidence", "corrected").unwrap())
        .await
        .unwrap();
    assert_eq!(
        bound.get_active("evidence").await.unwrap_err().code,
        MemoryPortErrorCode::Conflict
    );
    assert_eq!(bound.get_active("replacement").await.unwrap().revision(), 2);
    bound
        .write(
            write(
                "expired",
                MemoryScope::agent("tavi").unwrap(),
                "expired evidence",
            )
            .with_retention_deadline(101)
            .unwrap(),
        )
        .await
        .unwrap();
    clock.advance_ms(1).unwrap();
    assert_eq!(
        bound.get_active("expired").await.unwrap_err().code,
        MemoryPortErrorCode::Conflict
    );
    assert!(bound.get("expired").await.is_ok());
    assert_eq!(
        store
            .bind(b)
            .await
            .unwrap()
            .get("forgotten")
            .await
            .unwrap_err()
            .code,
        MemoryPortErrorCode::UnauthorizedScope
    );
    assert_eq!(
        store
            .bind(foreign)
            .await
            .unwrap()
            .get("forgotten")
            .await
            .unwrap_err()
            .code,
        MemoryPortErrorCode::NotFound
    );
    store.shutdown().await.unwrap();
}

#[tokio::test]
async fn memory_store_identity_is_stable_on_reopen_and_distinct_for_another_file() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("identity.sqlite");
    let clock = Arc::new(ManualExecutionClock::default());
    let store = SqliteMemoryStore::open(&path, clock.clone()).await.unwrap();
    let read = |path: &std::path::Path| {
        rusqlite::Connection::open(path)
            .unwrap()
            .query_row::<String, _, _>(
                "SELECT store_id FROM memory_store_metadata WHERE singleton=1",
                [],
                |row| row.get(0),
            )
            .unwrap()
    };
    let original = read(&path);
    assert!(!uuid::Uuid::parse_str(&original).unwrap().is_nil());
    assert_eq!(
        uuid::Uuid::parse_str(&original).unwrap().to_string(),
        original
    );
    assert_eq!(store.store_id().await.unwrap(), original);
    store.shutdown().await.unwrap();
    let reopened = SqliteMemoryStore::open(&path, clock.clone()).await.unwrap();
    assert_eq!(read(&path), original);
    assert_eq!(reopened.store_id().await.unwrap(), original);
    let other_path = temp.path().join("independent.sqlite");
    let other = SqliteMemoryStore::open(&other_path, clock).await.unwrap();
    assert_ne!(read(&other_path), original);
    assert_ne!(other.store_id().await.unwrap(), original);
    reopened.shutdown().await.unwrap();
    other.shutdown().await.unwrap();
}

#[tokio::test]
async fn missing_corrupt_or_nil_memory_store_identity_is_never_regenerated() {
    use rusqlite::OptionalExtension;
    for (index, mutation) in [
        "DELETE FROM memory_store_metadata",
        "UPDATE memory_store_metadata SET store_id='corrupt'",
        "UPDATE memory_store_metadata SET store_id='00000000-0000-0000-0000-000000000000'",
        "UPDATE memory_store_metadata SET store_id='550E8400-E29B-41D4-A716-446655440000'",
        "DROP TABLE memory_store_metadata",
    ]
    .into_iter()
    .enumerate()
    {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join(format!("damaged-{index}.sqlite"));
        let store = SqliteMemoryStore::open(&path, Arc::new(ManualExecutionClock::default()))
            .await
            .unwrap();
        store.shutdown().await.unwrap();
        let raw = rusqlite::Connection::open(&path).unwrap();
        raw.execute_batch(mutation).unwrap();
        let read = || {
            raw.query_row::<String, _, _>(
                "SELECT store_id FROM memory_store_metadata WHERE singleton=1",
                [],
                |row| row.get(0),
            )
            .optional()
        };
        let before = read();
        assert!(
            SqliteMemoryStore::open(&path, Arc::new(ManualExecutionClock::default()))
                .await
                .is_err(),
            "{mutation}"
        );
        match before {
            Ok(before) => assert_eq!(
                read().unwrap(),
                before,
                "failed open must not replace identity"
            ),
            Err(_) => assert!(
                read().is_err(),
                "failed open must not recreate metadata table"
            ),
        }
    }
}

#[tokio::test]
async fn rejected_effect_rolls_back_candidates_but_persists_observed_expiry_across_reopen() {
    use anima_core::{IndividualId, MemoryPortError};
    use anima_memory::{
        CandidateEvidence, EvidenceKind, MemoryCandidate, MemoryEffect, MemoryProjectionPolicy,
        MemoryProjectionPort, RelationshipObservation,
    };
    struct Accepted;
    impl MemoryProjectionPolicy for Accepted {
        fn validate(
            &self,
            _: &MemoryAuthority,
            _: &MemoryCandidate,
        ) -> Result<(), MemoryPortError> {
            Ok(())
        }
        fn validate_relationship(
            &self,
            _: &MemoryAuthority,
            _: &RelationshipObservation,
        ) -> Result<(), MemoryPortError> {
            Ok(())
        }
    }
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("rejected-expiry.sqlite");
    let clock = Arc::new(ManualExecutionClock::new(10));
    let store = SqliteMemoryStore::open(&path, clock.clone()).await.unwrap();
    let auth = authority("owner", "tavi");
    let scope = MemoryScope::agent("tavi").unwrap();
    store
        .register_identity(auth.identity().clone())
        .await
        .unwrap();
    store
        .register_identity(IdentityBinding::new("owner", "mira").unwrap())
        .await
        .unwrap();
    let bound = store.bind(auth.clone()).await.unwrap();
    bound
        .write(
            write("expires", scope.clone(), "expired private tea")
                .with_retention_deadline(20)
                .unwrap(),
        )
        .await
        .unwrap();
    let candidate = MemoryCandidate::new(
        MemoryWrite::new(
            "partial",
            MemoryKind::Episode,
            scope.clone(),
            "must roll back",
            1.0,
            MemoryProvenance::new("user", "failed-event", 10).unwrap(),
        )
        .unwrap(),
        CandidateEvidence::new(EvidenceKind::UserStatement, "failed-event", None).unwrap(),
    )
    .unwrap();
    let relation = RelationshipObservation::new(
        "failed-relation",
        scope,
        IndividualId::new("mira").unwrap(),
        "trust",
        "expired evidence",
        0.5,
        0.5,
        10,
        vec!["expires".into()],
    )
    .unwrap();
    let effect = MemoryEffect::new(
        "failed-event",
        auth.identity().clone(),
        vec![candidate],
        vec![],
        vec![relation],
    )
    .unwrap()
    .validate(&auth, &Accepted)
    .unwrap();
    clock.advance_ms(11).unwrap();
    assert_eq!(
        bound.apply_effect(effect).await.unwrap_err().code,
        MemoryPortErrorCode::UnauthorizedScope
    );
    let raw = rusqlite::Connection::open(&path).unwrap();
    for sql in [
        "SELECT COUNT(*) FROM memory_records WHERE id='partial'",
        "SELECT COUNT(*) FROM memory_history WHERE id='partial'",
        "SELECT COUNT(*) FROM memory_receipts WHERE source_id='failed-event'",
        "SELECT COUNT(*) FROM memory_relationships",
    ] {
        assert_eq!(
            raw.query_row::<u64, _, _>(sql, [], |row| row.get(0))
                .unwrap(),
            0,
            "rejected candidate data must roll back"
        );
    }
    drop(raw);
    store.shutdown().await.unwrap();
    let reopened = SqliteMemoryStore::open(&path, Arc::new(ManualExecutionClock::new(1)))
        .await
        .unwrap();
    let bound = reopened.bind(auth).await.unwrap();
    assert!(
        bound.recent(10).await.unwrap().is_empty(),
        "a rejected observation of expiry must prevent resurrection after rollback"
    );
    reopened.shutdown().await.unwrap();
}

#[tokio::test]
async fn ephemeral_memory_is_explicitly_refused_instead_of_becoming_persistent() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("ephemeral.sqlite");
    let store = SqliteMemoryStore::open(&path, Arc::new(ManualExecutionClock::default()))
        .await
        .unwrap();
    let auth = authority("owner", "assistant");
    store
        .register_identity(auth.identity().clone())
        .await
        .unwrap();
    let bound = store.bind(auth).await.unwrap();
    let mut payload = serde_json::to_value(write(
        "temporary",
        MemoryScope::agent("assistant").unwrap(),
        "only for this turn",
    ))
    .unwrap();
    payload["retention"]["policy"] = serde_json::json!("ephemeral");
    let ephemeral: MemoryWrite = serde_json::from_value(payload).unwrap();
    assert_eq!(
        bound.write(ephemeral).await.unwrap_err().code,
        MemoryPortErrorCode::Unsupported
    );
    let raw = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        raw.query_row::<i64, _, _>("SELECT COUNT(*) FROM memory_records", [], |row| row.get(0))
            .unwrap(),
        0
    );
    store.shutdown().await.unwrap();
}

#[tokio::test]
async fn expired_memory_cannot_reappear_after_clock_rollback_and_reopen() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("expiry.sqlite");
    let clock = Arc::new(ManualExecutionClock::new(10));
    let store = SqliteMemoryStore::open(&path, clock.clone()).await.unwrap();
    let auth = authority("owner", "assistant");
    store
        .register_identity(auth.identity().clone())
        .await
        .unwrap();
    let bound = store.bind(auth.clone()).await.unwrap();
    bound
        .write(
            write(
                "expires",
                MemoryScope::agent("assistant").unwrap(),
                "jasmine tea",
            )
            .with_retention_deadline(20)
            .unwrap(),
        )
        .await
        .unwrap();
    let query = || {
        MemoryQuery::new(
            "jasmine",
            vec![MemoryScope::agent("assistant").unwrap()],
            10,
        )
        .unwrap()
    };
    assert_eq!(
        bound
            .query(&auth.access_context().unwrap(), query())
            .await
            .unwrap()
            .hits()
            .len(),
        1
    );
    clock.advance_ms(11).unwrap();
    assert!(bound
        .query(&auth.access_context().unwrap(), query())
        .await
        .unwrap()
        .hits()
        .is_empty());
    store.shutdown().await.unwrap();
    let store = SqliteMemoryStore::open(&path, Arc::new(ManualExecutionClock::new(1)))
        .await
        .unwrap();
    let bound = store.bind(auth.clone()).await.unwrap();
    assert!(bound
        .query(&auth.access_context().unwrap(), query())
        .await
        .unwrap()
        .hits()
        .is_empty());
    store.shutdown().await.unwrap();
}

#[tokio::test]
async fn canonical_replay_cas_correction_forgetting_and_owner_isolation() {
    let temp = tempfile::tempdir().unwrap();
    let store = SqliteMemoryStore::open(
        temp.path().join("memory.sqlite"),
        Arc::new(ManualExecutionClock::default()),
    )
    .await
    .unwrap();
    let auth = authority("a", "tavi");
    store
        .register_identity(auth.identity().clone())
        .await
        .unwrap();
    let bound = store.bind(auth.clone()).await.unwrap();
    let scope = MemoryScope::agent("tavi").unwrap();
    let original = write("old", scope.clone(), "likes green tea");
    assert_eq!(bound.write(original.clone()).await.unwrap().revision(), 1);
    let audit = rusqlite::Connection::open(temp.path().join("memory.sqlite")).unwrap();
    let writer: String = audit
        .query_row(
            "SELECT writer FROM memory_records WHERE owner='a' AND id='old'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(writer, "tavi");
    assert_eq!(bound.write(original).await.unwrap().revision(), 1);
    assert_eq!(
        bound
            .write(write("old", scope.clone(), "likes coffee"))
            .await
            .unwrap_err()
            .code,
        MemoryPortErrorCode::Conflict
    );
    bound
        .write(write("new", scope.clone(), "likes black tea"))
        .await
        .unwrap();
    bound
        .revise(MemoryRevision::correction("new", 1, "old", "user correction").unwrap())
        .await
        .unwrap();
    assert_eq!(
        bound
            .revise(MemoryRevision::forget("new", 1, "stale").unwrap())
            .await
            .unwrap_err()
            .code,
        MemoryPortErrorCode::Conflict
    );
    let q = MemoryQuery::new("tea", vec![scope.clone()], 10).unwrap();
    let hits = bound
        .query(&auth.access_context().unwrap(), q.clone())
        .await
        .unwrap();
    assert_eq!(hits.hits().len(), 1);
    assert_eq!(hits.hits()[0].record().id(), "new");
    bound
        .revise(MemoryRevision::forget("new", 2, "user deletion").unwrap())
        .await
        .unwrap();
    assert!(bound
        .query(&auth.access_context().unwrap(), q.clone())
        .await
        .unwrap()
        .hits()
        .is_empty());
    let other = authority("b", "tavi");
    store
        .register_identity(other.identity().clone())
        .await
        .unwrap();
    let foreign = store.bind(other.clone()).await.unwrap();
    assert_eq!(
        foreign
            .revise(MemoryRevision::forget("new", 3, "attack").unwrap())
            .await
            .unwrap_err()
            .code,
        MemoryPortErrorCode::NotFound
    );
    assert!(foreign
        .query(&other.access_context().unwrap(), q)
        .await
        .unwrap()
        .hits()
        .is_empty());
    assert_eq!(
        bound
            .write(write(
                "attack",
                MemoryScope::agent("mira").unwrap(),
                "secret"
            ))
            .await
            .unwrap_err()
            .code,
        MemoryPortErrorCode::UnauthorizedScope
    );
    store.shutdown().await.unwrap();
    assert_eq!(
        bound
            .write(write("down", scope, "unavailable"))
            .await
            .unwrap_err()
            .code,
        MemoryPortErrorCode::Backend
    );
}

#[tokio::test]
async fn correction_targets_cannot_cross_scopes_and_all_memory_kinds_persist() {
    let temp = tempfile::tempdir().unwrap();
    let store = SqliteMemoryStore::open(
        temp.path().join("scope.sqlite"),
        Arc::new(ManualExecutionClock::default()),
    )
    .await
    .unwrap();
    let a = authority("owner", "tavi");
    let b = authority("owner", "mira");
    for auth in [&a, &b] {
        store
            .register_identity(auth.identity().clone())
            .await
            .unwrap();
    }
    let ta = store.bind(a.clone()).await.unwrap();
    let mb = store.bind(b.clone()).await.unwrap();
    mb.write(write(
        "private-mira",
        MemoryScope::agent("mira").unwrap(),
        "private tea",
    ))
    .await
    .unwrap();
    ta.write(write(
        "replacement",
        MemoryScope::agent("tavi").unwrap(),
        "public tea",
    ))
    .await
    .unwrap();
    assert_eq!(
        ta.revise(MemoryRevision::correction("replacement", 1, "private-mira", "attack").unwrap())
            .await
            .unwrap_err()
            .code,
        MemoryPortErrorCode::UnauthorizedScope
    );
    for (i, kind) in [
        MemoryKind::Fact,
        MemoryKind::Preference,
        MemoryKind::Episode,
        MemoryKind::Procedure,
        MemoryKind::Reflection,
    ]
    .into_iter()
    .enumerate()
    {
        ta.write(
            MemoryWrite::new(
                format!("kind-{i}"),
                kind,
                MemoryScope::agent("tavi").unwrap(),
                "portable memory",
                0.5,
                MemoryProvenance::new("test", "event", 1).unwrap(),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    }
    assert_eq!(ta.recent(6).await.unwrap().len(), 6);
    store.shutdown().await.unwrap();
}

#[tokio::test]
async fn deferred_constraint_commit_failure_exposes_no_record_or_history() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("commit.sqlite");
    let store = SqliteMemoryStore::open(&path, Arc::new(ManualExecutionClock::default()))
        .await
        .unwrap();
    let auth = authority("owner", "tavi");
    store
        .register_identity(auth.identity().clone())
        .await
        .unwrap();
    let bound = store.bind(auth).await.unwrap();
    let observer = rusqlite::Connection::open(&path).unwrap();
    observer.execute_batch("PRAGMA foreign_keys=ON; CREATE TABLE commit_parent(id TEXT PRIMARY KEY); CREATE TABLE commit_failure(id TEXT REFERENCES commit_parent(id) DEFERRABLE INITIALLY DEFERRED); CREATE TRIGGER fail_at_commit AFTER INSERT ON memory_records BEGIN INSERT INTO commit_failure VALUES('absent-parent'); END;").unwrap();
    assert_eq!(
        bound
            .write(write(
                "failed",
                MemoryScope::agent("tavi").unwrap(),
                "must remain invisible"
            ))
            .await
            .unwrap_err()
            .code,
        MemoryPortErrorCode::Backend
    );
    assert!(bound.recent(10).await.unwrap().is_empty());
    for table in [
        "memory_records",
        "memory_history",
        "memory_receipts",
        "commit_failure",
    ] {
        let count: u64 = observer
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0, "{table}");
    }
    observer
        .execute_batch("DROP TRIGGER fail_at_commit;")
        .unwrap();
    assert_eq!(
        bound
            .write(write(
                "failed",
                MemoryScope::agent("tavi").unwrap(),
                "now committed"
            ))
            .await
            .unwrap()
            .revision(),
        1
    );
    store.shutdown().await.unwrap();
}

#[tokio::test]
async fn retention_and_shared_membership_are_applied_before_lexical_ranking() {
    let temp = tempfile::tempdir().unwrap();
    let clock = Arc::new(ManualExecutionClock::new(100));
    let store = SqliteMemoryStore::open(temp.path().join("retention.sqlite"), clock.clone())
        .await
        .unwrap();
    let private = MemoryScope::agent("tavi").unwrap();
    let shared = MemoryScope::workspace("space:world-a").unwrap();
    let identity = IdentityBinding::new("owner", "tavi").unwrap();
    let auth = MemoryAuthority::trusted(
        identity.clone(),
        vec![private.clone(), shared.clone()],
        vec![private.clone(), shared.clone()],
    )
    .unwrap();
    store.register_identity(identity).await.unwrap();
    let bound = store.bind(auth.clone()).await.unwrap();
    bound
        .write(
            write("expires", private.clone(), "tea expires")
                .with_retention_deadline(101)
                .unwrap(),
        )
        .await
        .unwrap();
    bound
        .write(write("shared", shared.clone(), "shared tea"))
        .await
        .unwrap();
    assert_eq!(bound.recent(10).await.unwrap().len(), 2);
    clock.advance_ms(1).unwrap();
    assert_eq!(bound.recent(10).await.unwrap().len(), 1);
    let narrow = MemoryAuthority::trusted(
        auth.identity().clone(),
        vec![private.clone()],
        vec![private.clone()],
    )
    .unwrap();
    let limited = store.bind(narrow).await.unwrap();
    let forged = anima_core::KnowledgeAccessContext::trusted(vec![shared.clone()]).unwrap();
    assert_eq!(
        limited
            .query(&forged, MemoryQuery::new("tea", vec![shared], 10).unwrap())
            .await
            .unwrap_err()
            .code,
        MemoryPortErrorCode::UnauthorizedScope
    );
    assert!(limited.recent(10).await.unwrap().is_empty());
    store.shutdown().await.unwrap();
}

#[tokio::test]
async fn independent_connections_serialize_revision_compare_and_swap() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("cas.sqlite");
    let clock = Arc::new(ManualExecutionClock::default());
    let first = SqliteMemoryStore::open(&path, clock.clone()).await.unwrap();
    let auth = authority("owner", "tavi");
    first
        .register_identity(auth.identity().clone())
        .await
        .unwrap();
    let a = first.bind(auth.clone()).await.unwrap();
    a.write(write("cas", MemoryScope::agent("tavi").unwrap(), "race"))
        .await
        .unwrap();
    let second = SqliteMemoryStore::open(&path, clock).await.unwrap();
    let b = second.bind(auth).await.unwrap();
    let (left, right) = tokio::join!(
        a.revise(MemoryRevision::forget("cas", 1, "first").unwrap()),
        b.revise(MemoryRevision::forget("cas", 1, "second").unwrap())
    );
    assert_eq!(usize::from(left.is_ok()) + usize::from(right.is_ok()), 1);
    assert_eq!(
        left.err().or_else(|| right.err()).unwrap().code,
        MemoryPortErrorCode::Conflict
    );
    first.shutdown().await.unwrap();
    second.shutdown().await.unwrap();
}

#[tokio::test]
async fn incompatible_or_damaged_schema_never_silently_creates_an_empty_memory_store() {
    let temp = tempfile::tempdir().unwrap();
    let unknown = temp.path().join("unrelated.sqlite");
    let raw = rusqlite::Connection::open(&unknown).unwrap();
    raw.execute_batch("CREATE TABLE existing_user_data(id TEXT PRIMARY KEY);")
        .unwrap();
    drop(raw);
    assert!(
        SqliteMemoryStore::open(&unknown, Arc::new(ManualExecutionClock::default()))
            .await
            .is_err()
    );
    let path = temp.path().join("damaged.sqlite");
    let store = SqliteMemoryStore::open(&path, Arc::new(ManualExecutionClock::default()))
        .await
        .unwrap();
    store.shutdown().await.unwrap();
    let raw = rusqlite::Connection::open(&path).unwrap();
    raw.execute_batch("DROP TABLE memory_history;").unwrap();
    drop(raw);
    assert!(
        SqliteMemoryStore::open(&path, Arc::new(ManualExecutionClock::default()))
            .await
            .is_err()
    );
}
