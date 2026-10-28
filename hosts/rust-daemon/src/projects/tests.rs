use super::records::{digest, ProjectCreationReceipt, ProjectError, MAX_CREATION_RECEIPTS};
use super::registry::{ProjectCreateRequest, ProjectRegistry};

fn create_input(key: &str) -> ProjectCreateRequest {
    ProjectCreateRequest {
        request_key: key.into(),
        title: "House research".into(),
        member_agent_ids: vec!["agent-b".into(), "agent-a".into()],
        instructions: "Use the selected evidence.".into(),
    }
}

#[test]
fn creation_replay_survives_delete_and_restart_without_recreating_membership() {
    let mut registry = ProjectRegistry::default();
    let (created, first) = registry.create(create_input("create-one"), 10).unwrap();
    assert!(first);
    let (replayed, second) = registry.create(create_input("create-one"), 11).unwrap();
    assert!(!second);
    assert_eq!(created.id, replayed.id);
    assert_eq!(registry.records().count(), 1);
    let mut mismatch = create_input("create-one");
    mismatch.instructions.push_str(" Changed request");
    assert_eq!(
        registry.create(mismatch, 12).unwrap_err(),
        ProjectError::StaleRevision
    );
    registry.delete(&created.id, 1, 13).unwrap();
    let restored =
        serde_json::to_vec(&(registry.snapshot(), registry.creation_receipts_snapshot())).unwrap();
    let (projects, receipts) = serde_json::from_slice(&restored).unwrap();
    let mut restarted = ProjectRegistry::from_snapshot(projects, receipts).unwrap();
    assert_eq!(
        restarted
            .create(create_input("create-one"), 14)
            .unwrap_err(),
        ProjectError::DeletedCreation
    );
    assert_eq!(
        restarted.records().count(),
        0,
        "lost create acknowledgement must not recreate a deleted project"
    );
    assert_eq!(restarted.creation_receipts_snapshot().len(), 1);
    let mut mismatch = create_input("create-one");
    mismatch.title = "Another project".into();
    assert_eq!(
        restarted.create(mismatch, 15).unwrap_err(),
        ProjectError::StaleRevision
    );
}

#[test]
fn creation_history_capacity_rejects_before_any_candidate_mutation() {
    let receipts = (0..MAX_CREATION_RECEIPTS)
        .map(|index| ProjectCreationReceipt {
            request_key: format!("deleted-{index}"),
            request_sha256: digest(&index).unwrap(),
            project_id: format!("project_{}", uuid::Uuid::new_v4()),
            created_at_ms: 1,
            deleted_at_ms: Some(2),
        })
        .collect();
    let mut registry = ProjectRegistry::from_snapshot(Vec::new(), receipts).unwrap();
    let before = registry.creation_receipts_snapshot();
    assert_eq!(
        registry
            .create(create_input("full-history"), 3)
            .unwrap_err(),
        ProjectError::ReceiptCapacity
    );
    assert_eq!(registry.records().count(), 0);
    assert_eq!(
        registry.creation_receipts_snapshot(),
        before,
        "capacity failure cannot evict or change receipts"
    );
}

#[test]
fn membership_readd_has_a_new_incarnation_and_keeps_one_project_identity() {
    let mut registry = ProjectRegistry::default();
    let (first, _) = registry.create(create_input("members"), 1).unwrap();
    let second = registry
        .replace_members(&first.id, 1, vec!["agent-b".into()], 2)
        .unwrap();
    assert_eq!(second.id, first.id);
    assert!(second.member("agent-a").is_none());
    assert_eq!(second.member("agent-b").unwrap().generation, 1);
    let third = registry
        .replace_members(&first.id, 2, vec!["agent-a".into(), "agent-b".into()], 3)
        .unwrap();
    assert_eq!(third.member("agent-a").unwrap().generation, 3);
    assert_eq!(third.member("agent-b").unwrap().generation, 1);
    assert_eq!(third.access_epoch, 3);
}

#[test]
fn candidate_mutation_does_not_publish_into_the_original_registry() {
    let original = ProjectRegistry::default();
    let mut candidate = original.clone();
    candidate.create(create_input("failed-save"), 1).unwrap();
    assert!(original.snapshot().is_empty());
    assert!(
        original.creation_receipts_snapshot().is_empty(),
        "failed candidate save must not consume a live receipt slot"
    );
}
