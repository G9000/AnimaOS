use super::*;
use crate::memory_provider::{
    binding::MemoryProviderBinding,
    outbox::{Admission, PublicationState},
};
use crate::workspace_context::ContextMode;
use serde_json::json;

const OP: &str = "00000000-0000-4000-8000-000000000001";
const PROPOSAL: &str = "00000000-0000-4000-8000-000000000002";
const RECORD: &str = "00000000-0000-4000-8000-000000000003";
const REVIEW: &str = "00000000-0000-4000-8000-000000000004";

fn fixture(remote: &str) -> Publication {
    let binding: MemoryProviderBinding = serde_json::from_value(json!({
        "version":1,"provider":"mother","connectionId":"connection",
        "workspaceId":"00000000-0000-4000-8000-000000000005",
        "generation":"00000000-0000-4000-8000-000000000006","subject":"agent",
        "publication":"selected","accessMode":"companion","personaSource":"local"
    }))
    .unwrap();
    let candidate = json!({"kind":"memory","status":"active","title":"A synthetic preference","content":"Selected synthetic content","basis":"owner_statement","references":[],"sources":[]});
    let mut request = json!({"workspaceId":binding.workspace_id,"generation":binding.generation,"operationId":OP,"expectedRevision":0,"record":candidate});
    if remote == "mother_propose_memory" {
        request["source"] = json!("Explicit synthetic selection");
        request["basis"] = json!("owner_statement");
    }
    Publication {
        admission: Admission {
            publication_id: OP.into(),
            operation_id: OP.into(),
            agent_id: "agent".into(),
            binding,
            connection_version: 1,
            descriptor_digest: "digest".into(),
            local_id: "local".into(),
            source_digest: Some("source-digest".into()),
            source_revision: None,
            caller_digest: None,
            source_attribution: None,
            candidate,
            remote_tool: remote.into(),
            request,
            target_key: "new-local".into(),
        },
        payload_hash: "payload-hash".into(),
        state: PublicationState::Sending,
        resume_state: Some(PublicationState::Queued),
        revision: 2,
        attempts: 1,
        next_attempt_at_ms: 0,
        created_at_ms: 0,
        updated_at_ms: 0,
        last_error: None,
        proposal_id: None,
        canonical_id: None,
        canonical_revision: None,
        receipt: None,
    }
}

fn pending(p: &Publication) -> Value {
    json!({"id":PROPOSAL,"revision":1,"status":"pending","actor":"synthetic-client","workspaceId":p.admission.binding.workspace_id,"generation":p.admission.binding.generation,"source":p.admission.request["source"],"basis":p.admission.request["basis"],"record":p.admission.candidate,"targetId":null,"expectedRevision":0,"conflict":false,"receipt":null})
}

fn canonical(p: &Publication, operation: &str) -> Value {
    let mut record = p.admission.candidate.clone();
    record["id"] = json!(RECORD);
    record["revision"] = json!(1);
    record["actor"] = json!("synthetic-client");
    record["createdAt"] = json!("2026-11-14T00:00:00Z");
    record["updatedAt"] = json!("2026-11-14T00:00:00Z");
    json!({"operationId":operation,"generation":p.admission.binding.generation,"committedAt":"2026-11-14T00:00:00Z","record":record})
}

#[test]
fn publication_receipt_submission_is_review_pending_not_saved() {
    let p = fixture("mother_propose_memory");
    let value = json!({"operationId":OP,"generation":p.admission.binding.generation,"proposal":pending(&p)});
    assert!(
        matches!(submission(&p,value,10).unwrap(), Transition::Submitted {proposal_id,..} if proposal_id == PROPOSAL)
    );
}

#[test]
fn publication_receipt_approval_uses_immutable_owner_edited_receipt() {
    let mut p = fixture("mother_propose_memory");
    p.proposal_id = Some(PROPOSAL.into());
    let mut value = pending(&p);
    let mut receipt = canonical(&p, REVIEW);
    receipt["record"]["content"] = json!("Owner revised this synthetic fact");
    value["status"] = json!("saved");
    value["revision"] = json!(2);
    value["receipt"] = json!({"proposalId":PROPOSAL,"status":"saved","operationId":REVIEW,"generation":p.admission.binding.generation,"receipt":receipt});
    match proposal(&p, value.clone(), 10).unwrap() {
        Transition::Approved {
            proposal_id,
            canonical_id,
            revision,
            receipt,
        } => {
            assert_eq!(proposal_id, PROPOSAL);
            assert_eq!(canonical_id, RECORD);
            assert_eq!(revision, 1);
            assert_eq!(receipt, value);
        }
        other => panic!("approval must use the owner's receipt: {other:?}"),
    }
    value["receipt"]["receipt"]["generation"] = json!("restored-generation");
    assert!(proposal(&p, value, 10).is_err());
}

#[test]
fn publication_receipt_rejects_wrong_operation_target_revision_or_original_candidate() {
    let mut p = fixture("mother_write_record");
    let value = canonical(&p, OP);
    assert!(matches!(
        submission(&p, value.clone(), 10).unwrap(),
        Transition::Saved { .. }
    ));
    for (key, replacement) in [
        ("operationId", json!(REVIEW)),
        ("generation", json!("another-generation")),
    ] {
        let mut bad = value.clone();
        bad[key] = replacement;
        assert!(submission(&p, bad, 10).is_err());
    }
    let mut bad = value.clone();
    bad["record"]["revision"] = json!(2);
    assert!(submission(&p, bad, 10).is_err());
    p.admission.request["id"] = json!(PROPOSAL);
    p.admission.request["expectedRevision"] = json!(0);
    assert!(submission(&p, value, 10).is_err());
    let p = fixture("mother_propose_memory");
    let mut bad = pending(&p);
    bad["record"]["content"] = json!("Other content");
    assert!(submission(
        &p,
        json!({"operationId":OP,"generation":p.admission.binding.generation,"proposal":bad}),
        10
    )
    .is_err());
}

#[test]
fn publication_receipt_conflict_dismissal_and_hidden_approval_are_distinct() {
    let mut p = fixture("mother_propose_memory");
    p.proposal_id = Some(PROPOSAL.into());
    let mut value = pending(&p);
    value["conflict"] = json!(true);
    assert!(matches!(
        proposal(&p, value, 10).unwrap(),
        Transition::Conflict { .. }
    ));
    let mut value = pending(&p);
    value["status"] = json!("dismissed");
    value["revision"] = json!(2);
    value["receipt"] = json!({"proposalId":PROPOSAL,"status":"dismissed","operationId":REVIEW,"generation":p.admission.binding.generation,"receipt":null});
    assert!(matches!(
        proposal(&p, value.clone(), 10).unwrap(),
        Transition::Dismissed { .. }
    ));
    value["status"] = json!("saved");
    assert!(proposal(&p, value, 10).is_err());
}

#[test]
fn publication_receipt_envelopes_require_complete_evidence_and_preserve_error_codes() {
    let wire = |data: Value, error: bool| {
        json!({"isError":error,"shortened":false,"omittedBlocks":0,"structuredData":data})
            .to_string()
    };
    assert_eq!(
        envelope(&wire(json!({"ok":true}), false)).unwrap(),
        json!({"ok":true})
    );
    for code in ["NOT_FOUND", "FORBIDDEN", "STORE_UNAVAILABLE"] {
        assert_eq!(
            envelope(&wire(
                json!({"error":{"code":code,"message":"private remote detail"}}),
                true
            )),
            Err(EvidenceError::Remote(code.into()))
        );
    }
    assert!(envelope(&"x".repeat(16 * 1024 + 1)).is_err());
    let mut partial: Value = serde_json::from_str(&wire(json!({"ok":true}), false)).unwrap();
    partial["shortened"] = json!(true);
    assert!(envelope(&partial.to_string()).is_err());
}

#[test]
fn publication_receipt_authority_requires_exact_workspace_generation_role_and_subject() {
    let p = fixture("mother_propose_memory");
    let value = json!({"workspace":{"id":p.admission.binding.workspace_id,"generation":p.admission.binding.generation},"access":{"version":1,"mode":"companion","subject":"agent"}});
    authority(&p.admission.binding, &value).unwrap();
    let mut binding = p.admission.binding;
    binding.access_mode = ContextMode::Shared;
    assert!(authority(&binding, &value).is_err());
}
