use anima_core::{
    IdentityBinding, ManualExecutionClock, MemoryKind, MemoryPort, MemoryPortErrorCode,
    MemoryProvenance, MemoryRevision, MemoryScope, MemoryWrite,
};
use anima_memory::{
    AgentRelationship, IdentityRegistry, Memory, MemoryAuthority, MemoryEntity,
    MemoryManagerSnapshot, MemoryType, RelationshipEndpointKind as Endpoint, TemporalFact,
    TemporalRecordStatus, TemporalRelationship,
};
use anima_memory_store_sqlite::{MemoryImportManifest, SqliteMemoryStore};
use std::sync::Arc;

fn copied() -> MemoryManagerSnapshot {
    let entity = MemoryEntity {
        kind: Endpoint::Agent,
        id: "old-tavi".into(),
        name: "Tavi".into(),
        aliases: vec!["Baker".into()],
        summary: Some("Copied entity".into()),
        created_at: 10,
        updated_at: 20,
    };
    let relationship = AgentRelationship {
        id: "old-edge".into(),
        source_kind: Endpoint::Agent,
        source_agent_id: "old-tavi".into(),
        source_agent_name: "Tavi".into(),
        target_kind: Endpoint::External,
        target_agent_id: "visitor".into(),
        target_agent_name: "Visitor".into(),
        relationship_type: "met".into(),
        summary: Some("Legacy evidence".into()),
        strength: 0.5,
        confidence: 0.6,
        evidence_memory_ids: vec!["old-memory".into()],
        tags: Some(vec!["source-tag".into()]),
        room_id: None,
        world_id: None,
        session_id: None,
        created_at: 10,
        updated_at: 20,
    };
    let fact = TemporalFact {
        id: "old-fact".into(),
        subject_kind: Endpoint::Agent,
        subject_id: "old-tavi".into(),
        subject_name: "Tavi".into(),
        predicate: "liked".into(),
        object_kind: None,
        object_id: None,
        object_name: None,
        value: Some("apples".into()),
        valid_from: Some(10),
        valid_to: Some(20),
        observed_at: 12,
        confidence: 0.7,
        evidence_memory_ids: vec!["old-memory".into()],
        supersedes_fact_ids: vec!["earlier-fact".into()],
        status: TemporalRecordStatus::Superseded,
        tags: Some(vec!["legacy".into()]),
        room_id: None,
        world_id: None,
        session_id: None,
        created_at: 12,
        updated_at: 20,
    };
    let temporal = TemporalRelationship {
        id: "old-temporal-edge".into(),
        source_kind: Endpoint::Agent,
        source_id: "old-tavi".into(),
        source_name: "Tavi".into(),
        target_kind: Endpoint::External,
        target_id: "visitor".into(),
        target_name: "Visitor".into(),
        relationship_type: "knew".into(),
        summary: None,
        strength: 0.4,
        confidence: 0.5,
        valid_from: Some(10),
        valid_to: Some(20),
        observed_at: 12,
        evidence_memory_ids: vec!["old-memory".into()],
        supersedes_relationship_ids: vec!["earlier-edge".into()],
        status: TemporalRecordStatus::Retracted,
        tags: None,
        room_id: None,
        world_id: None,
        session_id: None,
        created_at: 12,
        updated_at: 20,
    };
    MemoryManagerSnapshot {
        memories: vec![Memory {
            id: "old-memory".into(),
            agent_id: "old-tavi".into(),
            agent_name: "Tavi".into(),
            memory_type: MemoryType::Fact,
            content: "Used to like apples".into(),
            importance: 0.8,
            created_at: 10,
            tags: Some(vec!["copied-source".into()]),
            scope: anima_memory::MemoryScope::Private,
            room_id: None,
            world_id: None,
            session_id: None,
        }],
        memory_entities: vec![entity],
        agent_relationships: vec![relationship],
        temporal_facts: vec![fact],
        temporal_relationships: vec![temporal],
    }
}
fn authority(owner: &str, individual: &str) -> MemoryAuthority {
    let scope = MemoryScope::agent(individual).unwrap();
    MemoryAuthority::trusted(
        IdentityBinding::new(owner, individual).unwrap(),
        vec![scope.clone()],
        vec![scope],
    )
    .unwrap()
}
fn manifest(auth: &MemoryAuthority) -> MemoryImportManifest {
    MemoryImportManifest::from_legacy_snapshot(
        "copied-profile-v1",
        auth.identity().clone(),
        "old-tavi",
        copied(),
    )
    .unwrap()
}

#[tokio::test]
async fn copied_typed_history_is_lossless_private_and_not_promoted_to_generation() {
    let temp = tempfile::tempdir().unwrap();
    let store = SqliteMemoryStore::open(
        temp.path().join("copy.sqlite"),
        Arc::new(ManualExecutionClock::default()),
    )
    .await
    .unwrap();
    let auth = authority("owner", "new-tavi");
    store
        .register_identity(auth.identity().clone())
        .await
        .unwrap();
    let bound = store.bind(auth.clone()).await.unwrap();
    let old_reader = copied();
    let outcome = bound.import_manifest(manifest(&auth)).await.unwrap();
    assert!(!outcome.replayed);
    assert!(
        bound
            .import_manifest(manifest(&auth))
            .await
            .unwrap()
            .replayed
    );
    let export = bound.export_profile_for_rollback().await.unwrap();
    assert_eq!(export.records.len(), 1);
    assert_eq!(export.records[0].id(), "old-memory");
    assert_eq!(
        export.records[0].scope(),
        &MemoryScope::agent("new-tavi").unwrap()
    );
    assert_eq!(export.legacy_archives.len(), 1);
    assert_eq!(export.legacy_archives[0].snapshot(), &old_reader);
    assert_eq!(export.legacy_archives[0].source_id(), "copied-profile-v1");
    assert_eq!(export.legacy_archives[0].evidence(), "legacy/unknown");
    assert!(
        export.relationships.is_empty(),
        "unscoped legacy edges are not individual relationship observations"
    );
    assert_eq!(
        bound.recent(10).await.unwrap().len(),
        1,
        "auxiliary legacy facts are not invented generation records"
    );
    assert_eq!(old_reader, copied(), "the old reader is untouched");
    for other in [
        authority("owner", "mira"),
        authority("other-owner", "new-tavi"),
    ] {
        store
            .register_identity(other.identity().clone())
            .await
            .unwrap();
        assert!(store
            .bind(other)
            .await
            .unwrap()
            .export_profile_for_rollback()
            .await
            .unwrap()
            .legacy_archives
            .is_empty());
    }
    store.shutdown().await.unwrap();
}

#[tokio::test]
async fn rollback_export_keeps_post_cutover_correction_and_forgetting() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("rollback.sqlite");
    let store = SqliteMemoryStore::open(&path, Arc::new(ManualExecutionClock::default()))
        .await
        .unwrap();
    let auth = authority("owner", "new-tavi");
    store
        .register_identity(auth.identity().clone())
        .await
        .unwrap();
    let bound = store.bind(auth.clone()).await.unwrap();
    bound.import_manifest(manifest(&auth)).await.unwrap();
    bound
        .write(
            MemoryWrite::new(
                "post-cutover",
                MemoryKind::Preference,
                MemoryScope::agent("new-tavi").unwrap(),
                "Now prefers pears",
                1.0,
                MemoryProvenance::new("user", "new-event", 30).unwrap(),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    bound
        .revise(
            MemoryRevision::correction("post-cutover", 1, "old-memory", "changed preference")
                .unwrap(),
        )
        .await
        .unwrap();
    store
        .register_identity(IdentityBinding::new("owner", "mira").unwrap())
        .await
        .unwrap();
    let edge = anima_memory::RelationshipObservation::new(
        "native-edge",
        MemoryScope::agent("new-tavi").unwrap(),
        anima_core::IndividualId::new("mira").unwrap(),
        "trust",
        "new evidence",
        0.8,
        0.9,
        30,
        vec!["post-cutover".into()],
    )
    .unwrap();
    bound
        .import_manifest(
            MemoryImportManifest::new("native-import", auth.identity().clone(), vec![], vec![edge])
                .unwrap(),
        )
        .await
        .unwrap();
    bound
        .revise(MemoryRevision::forget("post-cutover", 2, "withdrawn").unwrap())
        .await
        .unwrap();
    store.shutdown().await.unwrap();
    let reopened = SqliteMemoryStore::open(path, Arc::new(ManualExecutionClock::default()))
        .await
        .unwrap();
    let bound = reopened.bind(auth.clone()).await.unwrap();
    // Retry acknowledges the import without overwriting the current canonical lifecycle.
    assert!(
        bound
            .import_manifest(manifest(&auth))
            .await
            .unwrap()
            .replayed
    );
    let export = bound.export_profile_for_rollback().await.unwrap();
    assert_eq!(export.records.len(), 2);
    assert_eq!(export.relationships.len(), 1);
    assert_eq!(
        export.relationships[0].evidence_memory_ids(),
        &["post-cutover".to_owned()]
    );
    assert!(bound.relationships(10).await.unwrap().is_empty());
    let current = export
        .records
        .iter()
        .find(|r| r.id() == "post-cutover")
        .unwrap();
    assert_eq!(current.revision(), 3);
    assert_eq!(current.corrects(), Some("old-memory"));
    assert!(current.forgotten_at_ms().is_some());
    assert!(bound.recent(10).await.unwrap().is_empty());
    let encoded = serde_json::to_string(&export).unwrap();
    assert!(encoded.contains("post-cutover") && encoded.contains("old-temporal-edge"));
    reopened.shutdown().await.unwrap();
}

#[tokio::test]
async fn typed_collision_and_commit_failure_publish_neither_records_nor_archive() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("failure.sqlite");
    let store = SqliteMemoryStore::open(&path, Arc::new(ManualExecutionClock::default()))
        .await
        .unwrap();
    let auth = authority("owner", "new-tavi");
    store
        .register_identity(auth.identity().clone())
        .await
        .unwrap();
    let bound = store.bind(auth.clone()).await.unwrap();
    let raw = rusqlite::Connection::open(&path).unwrap();
    raw.execute_batch("CREATE TABLE fail_commit(owner TEXT, individual TEXT, FOREIGN KEY(owner,individual) REFERENCES memory_identities(owner,individual) DEFERRABLE INITIALLY DEFERRED); CREATE TRIGGER fail_import_commit AFTER INSERT ON memory_receipts BEGIN INSERT INTO fail_commit VALUES('missing','identity'); END;").unwrap();
    assert!(bound.import_manifest(manifest(&auth)).await.is_err());
    let export = bound.export_profile_for_rollback().await.unwrap();
    assert!(export.records.is_empty() && export.legacy_archives.is_empty());
    raw.execute_batch("DROP TRIGGER fail_import_commit; DROP TABLE fail_commit;")
        .unwrap();
    bound.import_manifest(manifest(&auth)).await.unwrap();
    let mut conflict = copied();
    conflict.memories.clear();
    conflict.temporal_facts[0].value = Some("different history".into());
    let collision = MemoryImportManifest::from_legacy_snapshot(
        "different-copy",
        auth.identity().clone(),
        "old-tavi",
        conflict,
    )
    .unwrap();
    assert_eq!(
        bound.import_manifest(collision).await.unwrap_err().code,
        MemoryPortErrorCode::Conflict
    );
    assert_eq!(
        bound
            .export_profile_for_rollback()
            .await
            .unwrap()
            .legacy_archives
            .len(),
        1
    );
    store.shutdown().await.unwrap();
}

#[test]
fn copied_profile_rejects_foreign_memories_and_unknown_snapshot_fields() {
    let auth = authority("owner", "new-tavi");
    let mut foreign = copied();
    foreign.memories[0].agent_id = "mira".into();
    assert!(MemoryImportManifest::from_legacy_snapshot(
        "copy",
        auth.identity().clone(),
        "old-tavi",
        foreign
    )
    .is_err());
    let mut json = serde_json::to_value(copied()).unwrap();
    json["unrecognized_future_evidence"] = true.into();
    assert!(serde_json::from_value::<MemoryManagerSnapshot>(json).is_err());
}

#[tokio::test]
async fn caller_interruption_keeps_import_in_custody_and_retry_is_exact() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("interrupted.sqlite");
    let store = SqliteMemoryStore::open(&path, Arc::new(ManualExecutionClock::default()))
        .await
        .unwrap();
    let auth = authority("owner", "new-tavi");
    store
        .register_identity(auth.identity().clone())
        .await
        .unwrap();
    let bound = store.bind(auth.clone()).await.unwrap();
    let lock = rusqlite::Connection::open(&path).unwrap();
    lock.execute_batch("BEGIN IMMEDIATE").unwrap();
    let importing = bound.clone();
    let payload = manifest(&auth);
    let task = tokio::spawn(async move { importing.import_manifest(payload).await });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(!task.is_finished());
    task.abort();
    let _ = task.await;
    lock.execute_batch("ROLLBACK").unwrap();
    // Export queues behind the admitted import and must see its complete transaction.
    let export = bound.export_profile_for_rollback().await.unwrap();
    assert_eq!(export.records.len(), 1);
    assert_eq!(export.legacy_archives.len(), 1);
    assert!(
        bound
            .import_manifest(manifest(&auth))
            .await
            .unwrap()
            .replayed
    );
    store.shutdown().await.unwrap();
}

#[tokio::test]
async fn copied_visibility_and_revoked_archive_grants_are_preserved() {
    let temp = tempfile::tempdir().unwrap();
    let store = SqliteMemoryStore::open(
        temp.path().join("scopes.sqlite"),
        Arc::new(ManualExecutionClock::default()),
    )
    .await
    .unwrap();
    let identity = IdentityBinding::new("owner", "new-tavi").unwrap();
    let private = MemoryScope::agent("new-tavi").unwrap();
    let shared = MemoryScope::workspace("old-world").unwrap();
    let room = MemoryScope::session("old-room").unwrap();
    let auth = MemoryAuthority::trusted(
        identity.clone(),
        vec![private.clone(), shared.clone(), room.clone()],
        vec![private.clone(), shared.clone(), room.clone()],
    )
    .unwrap();
    store.register_identity(identity.clone()).await.unwrap();
    let bound = store.bind(auth.clone()).await.unwrap();
    let mut snapshot = copied();
    let mut shared_memory = snapshot.memories[0].clone();
    shared_memory.id = "shared".into();
    shared_memory.scope = anima_memory::MemoryScope::Shared;
    shared_memory.world_id = Some("old-world".into());
    let mut room_memory = snapshot.memories[0].clone();
    room_memory.id = "room".into();
    room_memory.scope = anima_memory::MemoryScope::Room;
    room_memory.room_id = Some("old-room".into());
    snapshot.memories.extend([shared_memory, room_memory]);
    bound
        .import_manifest(
            MemoryImportManifest::from_legacy_snapshot(
                "scoped-copy",
                identity.clone(),
                "old-tavi",
                snapshot,
            )
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(bound.get("shared").await.unwrap().scope(), &shared);
    assert_eq!(bound.get("room").await.unwrap().scope(), &room);
    let narrowed =
        MemoryAuthority::trusted(identity, vec![private.clone()], vec![private]).unwrap();
    let narrowed = store.bind(narrowed).await.unwrap();
    assert_eq!(
        narrowed
            .export_profile_for_rollback()
            .await
            .unwrap_err()
            .code,
        MemoryPortErrorCode::UnauthorizedScope
    );
    assert_eq!(narrowed.recent(10).await.unwrap().len(), 1);
    store.shutdown().await.unwrap();
}

#[tokio::test]
async fn decoded_manifest_cannot_smuggle_an_unrelated_archive_or_hide_source_fields() {
    let temp = tempfile::tempdir().unwrap();
    let store = SqliteMemoryStore::open(
        temp.path().join("decoded.sqlite"),
        Arc::new(ManualExecutionClock::default()),
    )
    .await
    .unwrap();
    let auth = authority("owner", "new-tavi");
    store
        .register_identity(auth.identity().clone())
        .await
        .unwrap();
    let bound = store.bind(auth.clone()).await.unwrap();
    let mut json = serde_json::to_value(manifest(&auth)).unwrap();
    json["records"] = serde_json::json!([]);
    let decoded: MemoryImportManifest = serde_json::from_value(json).unwrap();
    assert!(bound.import_manifest(decoded).await.is_err());
    assert!(bound
        .export_profile_for_rollback()
        .await
        .unwrap()
        .legacy_archives
        .is_empty());
    store.shutdown().await.unwrap();
}

/// A physically separate fixture process; all paths are test-owned temporary copies.
#[test]
fn legacy_import_process_peer() {
    use std::io::Write;
    let Ok(path) = std::env::var("ANIMA_LEGACY_IMPORT_TEST_DB") else {
        return;
    };
    let mode = std::env::var("ANIMA_LEGACY_IMPORT_TEST_MODE").unwrap();
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            let store = SqliteMemoryStore::open(path, Arc::new(ManualExecutionClock::default()))
                .await
                .unwrap();
            let auth = authority("owner", "new-tavi");
            store
                .register_identity(auth.identity().clone())
                .await
                .unwrap();
            let bound = store.bind(auth.clone()).await.unwrap();
            let outcome = bound.import_manifest(manifest(&auth)).await.unwrap();
            if mode == "commit-and-park" {
                assert!(!outcome.replayed);
                println!("LEGACY_IMPORT_COMMITTED");
                std::io::stdout().flush().unwrap();
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(60));
                }
            }
            assert!(outcome.replayed);
            let export = bound.export_profile_for_rollback().await.unwrap();
            assert_eq!(export.records.len(), 1);
            assert_eq!(export.legacy_archives.len(), 1);
            assert_eq!(export.legacy_archives[0].snapshot(), &copied());
            store.shutdown().await.unwrap();
            println!("LEGACY_IMPORT_VERIFIED");
        });
}

#[test]
fn process_death_after_import_commit_preserves_archive_and_exact_retry() {
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("import-process.sqlite");
    let exe = std::env::current_exe().unwrap();
    let mut child = Command::new(&exe)
        .args(["--exact", "legacy_import_process_peer", "--nocapture"])
        .env("ANIMA_LEGACY_IMPORT_TEST_DB", &path)
        .env("ANIMA_LEGACY_IMPORT_TEST_MODE", "commit-and-park")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (ready, waiting) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if line.unwrap().contains("LEGACY_IMPORT_COMMITTED") {
                let _ = ready.send(());
                return;
            }
        }
    });
    let result = waiting.recv_timeout(std::time::Duration::from_secs(15));
    child.kill().unwrap();
    let _ = child.wait().unwrap();
    reader.join().unwrap();
    result.unwrap();
    let reopened = Command::new(exe)
        .args(["--exact", "legacy_import_process_peer", "--nocapture"])
        .env("ANIMA_LEGACY_IMPORT_TEST_DB", path)
        .env("ANIMA_LEGACY_IMPORT_TEST_MODE", "verify")
        .output()
        .unwrap();
    assert!(
        reopened.status.success(),
        "{}",
        String::from_utf8_lossy(&reopened.stderr)
    );
    assert!(String::from_utf8_lossy(&reopened.stdout).contains("LEGACY_IMPORT_VERIFIED"));
}
