use super::*;
use serde_json::json;

fn admission() -> Admission {
    let publication_id = uuid::Uuid::new_v4().to_string();
    let operation_id = uuid::Uuid::new_v4().to_string();
    Admission {
        publication_id, operation_id: operation_id.clone(), agent_id: "agent".into(),
        binding: serde_json::from_value(json!({"version":1,"provider":"mother","connectionId":"connection","workspaceId":"workspace","generation":"generation","subject":"agent","publication":"selected","personaSource":"local","accessMode":"companion"})).unwrap(),
        connection_version: 1, descriptor_digest: "descriptor".into(), local_id: "local-1".into(),
        source_digest: Some("source-digest".into()), source_revision: None, caller_digest: None, source_attribution: None,
        candidate: json!({"text":"synthetic selected memory"}), remote_tool: "mother_propose_memory".into(),
        request: json!({"workspaceId":"workspace","generation":"generation","operationId":operation_id,"record":{"text":"synthetic selected memory"}}),
        target_key: "local-1".into(),
    }
}
fn store(dir: &tempfile::TempDir) -> PublicationStore {
    PublicationStore::open(
        &dir.path().join("publication.sqlite"),
        OutboxLimits::default(),
    )
    .unwrap()
}

#[test]
fn admitted_snapshot_survives_reopen_and_duplicate_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let first = admission();
    let saved = store(&dir).admit(first.clone(), 10).unwrap();
    let reopened = store(&dir);
    assert_eq!(
        reopened.get(&first.publication_id).unwrap(),
        Some(saved.clone())
    );
    assert_eq!(reopened.admit(first.clone(), 20).unwrap(), saved);
    let mut changed = first;
    changed.candidate["text"] = json!("different");
    assert!(matches!(
        reopened.admit(changed, 30),
        Err(OutboxError::IdempotencyConflict)
    ));
}

#[test]
fn crash_recovery_keeps_exact_request_and_prevents_blind_retry_or_cancel() {
    let dir = tempfile::tempdir().unwrap();
    let s = store(&dir);
    let p = s.admit(admission(), 1).unwrap();
    let sending = s.claim(&p.admission.publication_id, p.revision, 2).unwrap();
    drop(s);
    let s = store(&dir);
    assert_eq!(s.recover_interrupted(3).unwrap(), 1);
    let unknown = s.get(&p.admission.publication_id).unwrap().unwrap();
    assert_eq!(unknown.state, PublicationState::Unknown);
    assert_eq!(unknown.admission.request, sending.admission.request);
    assert_eq!(
        unknown.admission.operation_id,
        sending.admission.operation_id
    );
    assert!(s
        .transition(
            &p.admission.publication_id,
            unknown.revision,
            Transition::Cancelled,
            4
        )
        .is_err());
    assert!(s
        .transition(
            &p.admission.publication_id,
            unknown.revision,
            Transition::Retry {
                error: "uncertain".into(),
                next_attempt_at_ms: 4
            },
            4
        )
        .is_err());
}

#[test]
fn cas_and_target_order_prevent_overtaking_even_when_blocked() {
    let dir = tempfile::tempdir().unwrap();
    let s = store(&dir);
    let first = s.admit(admission(), 1).unwrap();
    let second = s.admit(admission(), 2).unwrap();
    assert!(matches!(
        s.claim(&second.admission.publication_id, second.revision, 3),
        Err(OutboxError::TargetBusy)
    ));
    let blocked = s
        .transition(
            &first.admission.publication_id,
            first.revision,
            Transition::Blocked {
                error: "offline".into(),
            },
            3,
        )
        .unwrap();
    assert!(matches!(
        s.claim(&second.admission.publication_id, second.revision, 4),
        Err(OutboxError::TargetBusy)
    ));
    assert!(matches!(
        s.transition(
            &first.admission.publication_id,
            first.revision,
            Transition::Cancelled,
            4
        ),
        Err(OutboxError::StaleRevision)
    ));
    s.transition(
        &first.admission.publication_id,
        blocked.revision,
        Transition::Cancelled,
        4,
    )
    .unwrap();
    s.claim(&second.admission.publication_id, second.revision, 5)
        .unwrap();
}

#[test]
fn proposal_has_no_mapping_until_owner_approval_and_mapping_is_immutable() {
    let dir = tempfile::tempdir().unwrap();
    let s = store(&dir);
    let p = s.admit(admission(), 1).unwrap();
    let id = &p.admission.publication_id;
    let p = s.claim(id, p.revision, 2).unwrap();
    assert!(s
        .transition(
            id,
            p.revision,
            Transition::Saved {
                canonical_id: "invented".into(),
                revision: 1,
                receipt: json!({})
            },
            3
        )
        .is_err());
    let p = s
        .transition(
            id,
            p.revision,
            Transition::Submitted {
                proposal_id: "proposal-1".into(),
                receipt: json!({"proposal":{"id":"proposal-1"}}),
                next_attempt_at_ms: 4,
            },
            3,
        )
        .unwrap();
    assert!(s.mapping(id).unwrap().is_none());
    let receipt = json!({"record":{"id":"canonical-1","revision":2,"text":"owner edited"}});
    let p = s
        .transition(
            id,
            p.revision,
            Transition::Approved {
                proposal_id: "proposal-1".into(),
                canonical_id: "canonical-1".into(),
                revision: 2,
                receipt: receipt.clone(),
            },
            4,
        )
        .unwrap();
    let m = s.mapping(id).unwrap().unwrap();
    assert_eq!(m.canonical_id, "canonical-1");
    assert_eq!(m.receipt, receipt);
    assert_eq!(m.local_id, "local-1");
    assert!(s
        .transition(
            id,
            p.revision,
            Transition::Approved {
                proposal_id: "proposal-1".into(),
                canonical_id: "other".into(),
                revision: 3,
                receipt: json!({})
            },
            5
        )
        .is_err());
    assert_eq!(s.mapping(id).unwrap(), Some(m));
}

#[test]
fn admission_limits_reject_without_partial_rows() {
    let dir = tempfile::tempdir().unwrap();
    let s = PublicationStore::open(
        &dir.path().join("publication.sqlite"),
        OutboxLimits {
            max_records: 1,
            ..Default::default()
        },
    )
    .unwrap();
    let first = admission();
    s.admit(first.clone(), 1).unwrap();
    assert!(matches!(s.admit(admission(), 2), Err(OutboxError::Limit)));
    assert_eq!(s.list("agent", 10).unwrap().len(), 1);
    assert!(
        s.admit(first, 3).is_ok(),
        "idempotent repeat should not consume capacity"
    );
}

#[test]
fn configured_disk_cap_reserves_space_for_every_admitted_outcome() {
    let dir = tempfile::tempdir().unwrap();
    let s = PublicationStore::open(
        &dir.path().join("publication.sqlite"),
        OutboxLimits {
            max_disk_bytes: 4 * 1024 * 1024,
            ..Default::default()
        },
    )
    .unwrap();
    let mut admitted = Vec::new();
    for _ in 0..64 {
        let mut input = admission();
        input.remote_tool = "mother_write_record".into();
        match s.admit(input.clone(), 1) {
            Ok(p) => admitted.push(p),
            Err(OutboxError::Limit) => {
                assert!(s.get(&input.publication_id).unwrap().is_none());
                break;
            }
            Err(error) => panic!("capacity should reject before SQLite fills: {error}"),
        }
    }
    assert!(!admitted.is_empty() && admitted.len() < 64);
    assert_eq!(
        s.counts("agent").unwrap().values().sum::<u64>(),
        admitted.len() as u64
    );
    assert!(matches!(s.admit(admission(), 2), Err(OutboxError::Limit)));
    let overhead = serde_json::to_vec(&json!({"text":""})).unwrap().len();
    let receipt = json!({"text":"x".repeat(receipt_limit(s.limits) - overhead)});
    for p in &admitted {
        let id = &p.admission.publication_id;
        let p = s.claim(id, p.revision, 3).unwrap();
        let saved = s
            .transition(
                id,
                p.revision,
                Transition::Saved {
                    canonical_id: format!("canonical-{id}"),
                    revision: 1,
                    receipt: receipt.clone(),
                },
                4,
            )
            .unwrap();
        assert_eq!(saved.state, PublicationState::Saved);
        assert_eq!(s.mapping(id).unwrap().unwrap().receipt, receipt);
    }
    // Terminal records retain their evidence but no longer reserve future growth.
    let mut changed_source = admission();
    changed_source.source_digest = Some("new source after completed publications".into());
    assert!(s.admit(changed_source, 5).is_ok());
    let conn = s.connection.lock().unwrap();
    let pages: u64 = conn
        .query_row("PRAGMA page_count", [], |r| r.get(0))
        .unwrap();
    let ceiling: u64 = conn
        .query_row("PRAGMA max_page_count", [], |r| r.get(0))
        .unwrap();
    assert!(pages <= ceiling);
}

#[test]
fn disk_full_rejects_atomic_admission_and_preserves_existing_rows() {
    let dir = tempfile::tempdir().unwrap();
    let s = store(&dir);
    let first = s.admit(admission(), 1).unwrap();
    {
        let conn = s.connection.lock().unwrap();
        let pages: u64 = conn
            .query_row("PRAGMA page_count", [], |r| r.get(0))
            .unwrap();
        conn.pragma_update(None, "max_page_count", pages).unwrap();
    }
    let mut large = admission();
    large.candidate = json!({"text":"x".repeat(64 * 1024)});
    assert!(matches!(
        s.admit(large.clone(), 2),
        Err(OutboxError::Storage(_))
    ));
    assert!(s.get(&large.publication_id).unwrap().is_none());
    assert_eq!(s.get(&first.admission.publication_id).unwrap(), Some(first));
}

#[test]
fn failed_mapping_write_rolls_back_saved_state() {
    let dir = tempfile::tempdir().unwrap();
    let s = store(&dir);
    let p = s.admit(admission(), 1).unwrap();
    let id = &p.admission.publication_id;
    let p = s.claim(id, p.revision, 2).unwrap();
    let p = s
        .transition(
            id,
            p.revision,
            Transition::Submitted {
                proposal_id: "proposal".into(),
                receipt: json!({}),
                next_attempt_at_ms: 3,
            },
            3,
        )
        .unwrap();
    s.connection.lock().unwrap().execute_batch("CREATE TRIGGER fail_mapping BEFORE INSERT ON publication_mappings BEGIN SELECT RAISE(ABORT, 'synthetic disk write failure'); END").unwrap();
    assert!(s
        .transition(
            id,
            p.revision,
            Transition::Approved {
                proposal_id: "proposal".into(),
                canonical_id: "canonical".into(),
                revision: 1,
                receipt: json!({})
            },
            4
        )
        .is_err());
    assert_eq!(s.get(id).unwrap(), Some(p));
    assert!(s.mapping(id).unwrap().is_none());
}

#[test]
fn unknown_blocked_resume_preserves_reconciliation_and_backoff() {
    let dir = tempfile::tempdir().unwrap();
    let s = store(&dir);
    let p = s.admit(admission(), 1).unwrap();
    let id = &p.admission.publication_id;
    let p = s.claim(id, p.revision, 2).unwrap();
    let p = s
        .transition(
            id,
            p.revision,
            Transition::Unknown {
                error: "timeout".into(),
                next_attempt_at_ms: 50,
            },
            3,
        )
        .unwrap();
    let p = s
        .transition(
            id,
            p.revision,
            Transition::Blocked {
                error: "authority changed".into(),
            },
            4,
        )
        .unwrap();
    let p = s.transition(id, p.revision, Transition::Resume, 5).unwrap();
    assert_eq!(p.state, PublicationState::Unknown);
    assert!(s.claim(id, p.revision, 49).is_err());
    let p = s.claim(id, p.revision, 50).unwrap();
    assert_eq!(p.resume_state, Some(PublicationState::Unknown));
    assert!(s
        .transition(
            id,
            p.revision,
            Transition::Retry {
                error: "no".into(),
                next_attempt_at_ms: 51
            },
            51
        )
        .is_err());
    let p = s
        .transition(
            id,
            p.revision,
            Transition::ReconciledAbsent {
                next_attempt_at_ms: 60,
            },
            51,
        )
        .unwrap();
    assert_eq!(p.state, PublicationState::Queued);
}

#[test]
fn different_handles_cannot_claim_same_revision_or_reuse_operation_id() {
    let dir = tempfile::tempdir().unwrap();
    let first = store(&dir);
    let second = store(&dir);
    let p = first.admit(admission(), 1).unwrap();
    let mut reused = p.admission.clone();
    reused.publication_id = uuid::Uuid::new_v4().to_string();
    assert!(matches!(
        second.admit(reused, 2),
        Err(OutboxError::IdempotencyConflict)
    ));
    first
        .claim(&p.admission.publication_id, p.revision, 3)
        .unwrap();
    assert!(matches!(
        second.claim(&p.admission.publication_id, p.revision, 3),
        Err(OutboxError::StaleRevision)
    ));
}

#[test]
fn due_and_counts_preserve_conflict_order_until_confirmed_dismissal() {
    let dir = tempfile::tempdir().unwrap();
    let s = store(&dir);
    let first = s.admit(admission(), 1).unwrap();
    let second = s.admit(admission(), 2).unwrap();
    let mut other = admission();
    other.target_key = "independent".into();
    other.local_id = "independent".into();
    let other = s.admit(other, 3).unwrap();
    assert_eq!(s.due(3, 100).unwrap(), vec![first.clone(), other]);
    assert_eq!(s.counts("agent").unwrap().get("queued"), Some(&3));
    let id = &first.admission.publication_id;
    let p = s.claim(id, first.revision, 4).unwrap();
    let p = s
        .transition(
            id,
            p.revision,
            Transition::Submitted {
                proposal_id: "proposal".into(),
                receipt: json!({}),
                next_attempt_at_ms: 5,
            },
            5,
        )
        .unwrap();
    let p = s
        .transition(
            id,
            p.revision,
            Transition::Conflict {
                error: "target changed".into(),
                receipt: json!({"conflict":true}),
            },
            6,
        )
        .unwrap();
    assert!(s
        .due(6, 100)
        .unwrap()
        .iter()
        .all(|p| p.admission.publication_id != second.admission.publication_id));
    let p = s.transition(id, p.revision, Transition::Resume, 7).unwrap();
    assert_eq!(p.state, PublicationState::SubmittedForReview);
    s.transition(
        id,
        p.revision,
        Transition::Dismissed {
            receipt: json!({"status":"dismissed"}),
        },
        8,
    )
    .unwrap();
    assert_eq!(s.due(8, 100).unwrap()[0], second);
    assert_eq!(s.counts("agent").unwrap().values().sum::<u64>(), 3);
}

#[test]
fn direct_receipt_maps_and_oversized_transition_does_not_replace_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let s = store(&dir);
    let mut input = admission();
    input.remote_tool = "mother_write_record".into();
    let p = s.admit(input, 1).unwrap();
    let id = &p.admission.publication_id;
    let p = s.claim(id, p.revision, 2).unwrap();
    let oversized = json!({"text":"x".repeat(300*1024)});
    assert!(matches!(
        s.transition(
            id,
            p.revision,
            Transition::Saved {
                canonical_id: "canonical".into(),
                revision: 1,
                receipt: oversized
            },
            3
        ),
        Err(OutboxError::Limit)
    ));
    assert_eq!(s.get(id).unwrap(), Some(p.clone()));
    assert!(s.mapping(id).unwrap().is_none());
    let receipt = json!({"record":{"id":"canonical","revision":1}});
    s.transition(
        id,
        p.revision,
        Transition::Saved {
            canonical_id: "canonical".into(),
            revision: 1,
            receipt: receipt.clone(),
        },
        3,
    )
    .unwrap();
    assert_eq!(s.mapping(id).unwrap().unwrap().receipt, receipt);
    assert_eq!(s.mappings("agent", "local-1", 100).unwrap().len(), 1);
}

#[test]
fn admission_rejects_mismatched_wire_identity_without_persisting_it() {
    let dir = tempfile::tempdir().unwrap();
    let s = store(&dir);
    let mut input = admission();
    input.request["generation"] = json!("other-generation");
    assert!(matches!(s.admit(input, 1), Err(OutboxError::Invalid(_))));
    assert!(s.list("agent", 100).unwrap().is_empty());
}

#[test]
fn maximum_admitted_payload_keeps_room_for_recovery_and_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let s = store(&dir);
    let mut input = admission();
    input.remote_tool = "mother_write_record".into();
    let limit = s.limits.max_record_bytes - MUTABLE_METADATA_HEADROOM - receipt_limit(s.limits);
    // Find the exact admitted boundary; duplicate IDs from rejected attempts leave no row.
    let mut low = 0;
    let mut high = limit;
    while low < high {
        let size = (low + high).div_ceil(2);
        let mut probe = input.clone();
        probe.publication_id = uuid::Uuid::new_v4().to_string();
        probe.operation_id = uuid::Uuid::new_v4().to_string();
        probe.request["operationId"] = json!(probe.operation_id);
        probe.candidate = json!({"text":"x".repeat(size)});
        match s.admit(probe, 1) {
            Ok(_) => low = size,
            Err(OutboxError::Limit) => high = size - 1,
            Err(error) => panic!("unexpected boundary error: {error}"),
        }
    }
    input.target_key = "boundary-target".into();
    input.candidate = json!({"text":"x".repeat(low.saturating_sub(16))});
    let p = s.admit(input.clone(), 1).unwrap();
    let id = &p.admission.publication_id;
    let p = s.claim(id, p.revision, 2).unwrap();
    assert_eq!(p.state, PublicationState::Sending);
    s.recover_interrupted(3).unwrap();
    let p = s.get(id).unwrap().unwrap();
    assert_eq!(p.state, PublicationState::Unknown);
    let receipt = json!({"text":"x".repeat(receipt_limit(s.limits) - 32)});
    s.transition(
        id,
        p.revision,
        Transition::Saved {
            canonical_id: "canonical".into(),
            revision: 1,
            receipt,
        },
        4,
    )
    .unwrap();
    input.publication_id = uuid::Uuid::new_v4().to_string();
    input.operation_id = uuid::Uuid::new_v4().to_string();
    input.request["operationId"] = json!(input.operation_id);
    input.candidate = json!({"text":"x".repeat(limit)});
    input.source_digest = Some("oversized new source".into());
    let rejected_id = input.publication_id.clone();
    assert!(matches!(s.admit(input, 5), Err(OutboxError::Limit)));
    assert!(s.get(&rejected_id).unwrap().is_none());
}

#[test]
fn definitive_direct_conflict_releases_target_for_new_operation_without_retrying_old_one() {
    let dir = tempfile::tempdir().unwrap();
    let s = store(&dir);
    let mut input = admission();
    input.remote_tool = "mother_write_record".into();
    let first = s.admit(input, 1).unwrap();
    let second = s.admit(admission(), 2).unwrap();
    let id = &first.admission.publication_id;
    let p = s.claim(id, first.revision, 3).unwrap();
    let p = s
        .transition(
            id,
            p.revision,
            Transition::Conflict {
                error: "stale revision".into(),
                receipt: json!({"code":"CONFLICT"}),
            },
            4,
        )
        .unwrap();
    assert!(s.transition(id, p.revision, Transition::Resume, 5).is_err());
    assert!(s
        .transition(id, p.revision, Transition::Cancelled, 5)
        .is_err());
    assert!(s.claim(id, p.revision, 5).is_err());
    assert_eq!(s.due(5, 100).unwrap(), vec![second.clone()]);
    s.claim(&second.admission.publication_id, second.revision, 5)
        .unwrap();
    assert_eq!(s.get(id).unwrap(), Some(p));
}

#[test]
fn rejected_proposal_submission_releases_target_without_retrying_or_rebasing_it() {
    let dir = tempfile::tempdir().unwrap();
    let s = store(&dir);
    let first = s.admit(admission(), 1).unwrap();
    let second = s.admit(admission(), 2).unwrap();
    let id = &first.admission.publication_id;
    let p = s.claim(id, first.revision, 3).unwrap();
    let p = s
        .transition(
            id,
            p.revision,
            Transition::Conflict {
                error: "stale target revision".into(),
                receipt: json!({"error":{"code":"CONFLICT"}}),
            },
            4,
        )
        .unwrap();
    assert!(p.proposal_id.is_none());
    assert_eq!(p.admission, first.admission);
    for transition in [
        Transition::Resume,
        Transition::Cancelled,
        Transition::Blocked {
            error: "reopen".into(),
        },
        Transition::Retry {
            error: "retry".into(),
            next_attempt_at_ms: 5,
        },
    ] {
        assert!(s.transition(id, p.revision, transition, 5).is_err());
    }
    assert!(s.claim(id, p.revision, 5).is_err());
    assert_eq!(s.due(5, 100).unwrap(), vec![second.clone()]);
    s.claim(&second.admission.publication_id, second.revision, 5)
        .unwrap();
    assert_eq!(s.get(id).unwrap(), Some(p));
}

#[test]
fn source_dedup_reads_full_mapping_history_and_is_atomic_at_admission() {
    let dir = tempfile::tempdir().unwrap();
    let s = store(&dir);
    let mut original = None;
    for index in 0..257 {
        let mut input = admission();
        input.remote_tool = "mother_write_record".into();
        input.source_digest = Some(format!("source-{index}"));
        let p = s.admit(input, 1).unwrap();
        let id = &p.admission.publication_id;
        let p = s.claim(id, p.revision, 2).unwrap();
        let p = s
            .transition(
                id,
                p.revision,
                Transition::Saved {
                    canonical_id: format!("canonical-{index}"),
                    revision: 1,
                    receipt: json!({"synthetic":true}),
                },
                3,
            )
            .unwrap();
        if index == 0 {
            original = Some(p);
        }
    }
    let original = original.unwrap();
    let visible = s.mappings("agent", "local-1", 256).unwrap();
    assert_eq!(visible.len(), 256);
    assert!(visible
        .iter()
        .all(|mapping| mapping.source_digest.as_deref() != Some("source-0")));
    assert!(s
        .has_source_mapping(
            "workspace",
            "generation",
            "agent",
            "local-1",
            "source-0",
            None
        )
        .unwrap());
    assert!(s
        .has_source_mapping(
            "workspace",
            "generation",
            "agent",
            "local-1",
            "source-0",
            Some("canonical-0")
        )
        .unwrap());
    assert!(!s
        .has_source_mapping(
            "workspace",
            "generation",
            "agent",
            "local-1",
            "source-0",
            Some("other-target")
        )
        .unwrap());
    assert!(!s
        .has_source_mapping(
            "workspace",
            "other-generation",
            "agent",
            "local-1",
            "source-0",
            None
        )
        .unwrap());
    assert!(!s
        .has_source_mapping(
            "other-workspace",
            "generation",
            "agent",
            "local-1",
            "source-0",
            None
        )
        .unwrap());
    assert!(!s
        .has_source_mapping(
            "workspace",
            "generation",
            "other-agent",
            "local-1",
            "source-0",
            None
        )
        .unwrap());
    assert!(!s
        .has_source_mapping(
            "workspace",
            "generation",
            "agent",
            "other-local",
            "source-0",
            None
        )
        .unwrap());
    assert_eq!(s.admit(original.admission.clone(), 4).unwrap(), original);
    let mut duplicate = admission();
    duplicate.source_digest = Some("source-0".into());
    let duplicate_id = duplicate.publication_id.clone();
    assert!(matches!(
        s.admit(duplicate.clone(), 5),
        Err(OutboxError::IdempotencyConflict)
    ));
    assert!(s.get(&duplicate_id).unwrap().is_none());
    duplicate.request["id"] = json!("canonical-0");
    assert!(matches!(
        s.admit(duplicate.clone(), 5),
        Err(OutboxError::IdempotencyConflict)
    ));
    duplicate.request["id"] = json!("other-target");
    assert!(s.admit(duplicate, 5).is_ok());
}
