use super::service::*;
use super::*;
use crate::agent_runs::test_support::{
    approving_coordinator, coordinator_state, ledger_run, patient, ScriptedModel,
};
use crate::approvals::{
    ApprovalDecisionKind, ApprovalPolicy, ApprovalRequest, ApprovalResolution, ApprovalStatus,
    PendingApprovalStart, ResolvedBy,
};
use anima_core::{AgentConfigUpdate, CancelSignal, DataValue, ToolCall};
use anima_document_content::{OfficeEditReview, OfficeFormat, OfficeInventory, OfficePreservation};

struct Fixture {
    _root: tempfile::TempDir,
    private: super::store::test_support::PrivateTestDirectory,
    state: crate::app::SharedDaemonState,
    service: DocumentEditService,
    run: crate::runs::RunLink,
    call: ToolCall,
    coordinator: crate::agent_runs::AgentRunCoordinator,
}
async fn fixture() -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let private = super::store::test_support::private_directory();
    std::fs::write(root.path().join("source.docx"), b"source bytes").unwrap();
    let (coordinator, agent) = approving_coordinator(
        ScriptedModel::new(vec![]),
        ApprovalPolicy::default(),
        patient(),
    )
    .await;
    let run = ledger_run(&coordinator, &agent, "chat:document").await;
    let state = coordinator_state(&coordinator);
    let mut guard = state.write().await;
    guard.workspace = Some(crate::control_plane_store::WorkspaceConfig {
        root_path: root.path().to_path_buf(),
        ..Default::default()
    });
    let private_path = private.path().join("document-edits");
    super::store::initialize_owned_directory(&private_path).unwrap();
    guard.document_edit_store = Some(DocumentEditStore::open_existing(&private_path).unwrap());
    guard
        .agents
        .get_mut(&agent)
        .unwrap()
        .update_config(AgentConfigUpdate {
            tools: Some(
                crate::tools::ToolRegistry::new()
                    .resolve_descriptors([
                        "inspect_office_document",
                        "propose_document_edit",
                        "apply_document_edit",
                    ])
                    .unwrap(),
            ),
            ..Default::default()
        });
    let scope = scope_in(&guard, &run, "apply_document_edit").unwrap();
    let staged = guard
        .document_edit_store
        .as_ref()
        .unwrap()
        .stage(b"candidate bytes")
        .unwrap();
    let inventory = OfficeInventory {
        version: 1,
        format: OfficeFormat::Docx,
        targets: vec![],
        retained_hyperlinks: vec![],
        limitations: vec![],
    };
    let now = anima_core::primitives::now_millis();
    let draft = DocumentEditDraft {
        id: "proposal".into(),
        scope,
        source: DocumentEditSource::Workspace {
            file_path: "source.docx".into(),
            sha256: digest(b"source bytes"),
        },
        attachment_record: None,
        file_path: "copy.docx".into(),
        expected_destination: ExpectedDestination::Absent,
        candidate_sha256: staged.hash().into(),
        candidate_bytes: staged.size(),
        operations_sha256: "a".repeat(64),
        content_review: OfficeEditReview {
            version: 1,
            before: inventory.clone(),
            after: inventory,
            changes: vec![],
            preservation: OfficePreservation {
                unchanged_parts: 1,
                edited_parts: 0,
                unchanged_bytes_verified: true,
                container_normalized: true,
            },
        },
        created_at_ms: now,
        expires_at_ms: now + 60_000,
    };
    let hash = guard.document_edits.insert(draft, now).unwrap();
    drop(staged);
    let DataValue::Object(args) = crate::tools::generated_documents::schema_data(
        serde_json::json!({"proposal_id":"proposal","review_sha256":hash,"file_path":"copy.docx","expected_destination":{"state":"absent"}}),
    ) else {
        unreachable!()
    };
    let call = ToolCall {
        id: "apply-call".into(),
        name: "apply_document_edit".into(),
        args,
    };
    drop(guard);
    let service = DocumentEditService::new(state.clone(), coordinator.control_plane_transactions());
    Fixture {
        _root: root,
        private,
        state,
        service,
        run,
        call,
        coordinator,
    }
}

async fn rejects_private_store_scope(relation: &str) {
    let fixture = fixture().await;
    let store_root = fixture.private.path().join("document-edits");
    let target = match relation {
        "equal" => store_root,
        "ancestor" => fixture.private.path().to_path_buf(),
        "descendant" => {
            let path = store_root.join("nested-workspace");
            std::fs::create_dir(&path).unwrap();
            path
        }
        _ => panic!("unknown fixture relation"),
    };
    let mut guard = fixture.state.write().await;
    assert!(scope_in(&guard, &fixture.run, "apply_document_edit").is_ok());
    guard.workspace.as_mut().unwrap().root_path = target;
    assert!(
        scope_in(&guard, &fixture.run, "apply_document_edit").is_err(),
        "document scope admitted {relation} private storage overlap"
    );
}

#[tokio::test]
async fn workspace_boundary_scope_rejects_equal_private_store() {
    rejects_private_store_scope("equal").await;
}
#[tokio::test]
async fn workspace_boundary_scope_rejects_private_store_ancestor() {
    rejects_private_store_scope("ancestor").await;
}
#[tokio::test]
async fn workspace_boundary_scope_rejects_private_store_descendant() {
    rejects_private_store_scope("descendant").await;
}

pub(crate) async fn metadata_review_registry_fixture() -> DocumentEditRegistry {
    let fixture = fixture().await;
    let mut draft = fixture
        .state
        .read()
        .await
        .document_edits
        .get("proposal")
        .unwrap()
        .draft
        .clone();
    draft.content_review.changes = vec![anima_document_content::OfficeChange {
        operation_index: Some(0),
        label: "Private fixture paragraph".into(),
        before: "Private fixture original paragraph".into(),
        after: "Private fixture revised paragraph".into(),
        derived: false,
    }];
    let mut registry = DocumentEditRegistry::default();
    registry
        .insert(draft, anima_core::primitives::now_millis())
        .unwrap();
    registry
}

#[tokio::test]
async fn workspace_boundary_metadata_scope_rejects_workspace_containing_configured_file() {
    let fixture = fixture().await;
    let metadata_parent = fixture.private.path().join("work-area");
    std::fs::create_dir(&metadata_parent).unwrap();
    let metadata_file = metadata_parent.join("meeting-notes.json");
    let mut guard = fixture.state.write().await;
    guard.set_control_plane_store(Some(
        crate::control_plane_store::ControlPlaneStoreConfig::Json(metadata_file),
    ));
    assert!(scope_in(&guard, &fixture.run, "apply_document_edit").is_ok());
    assert!(!super::workspace_boundary::overlaps(
        guard
            .document_edit_store
            .as_ref()
            .unwrap()
            .canonical_root()
            .unwrap(),
        Some(&metadata_parent),
    )
    .unwrap());
    guard.workspace.as_mut().unwrap().root_path = metadata_parent;
    assert!(
        scope_in(&guard, &fixture.run, "apply_document_edit").is_err(),
        "document scope admitted a workspace containing its configured metadata file"
    );
}

fn owner(
    witness: &approval::DocumentEditWitness,
    call: &ToolCall,
    run: &crate::runs::RunLink,
) -> approval::DocumentEditOwnerApproval {
    let projected = witness.approval_call(call);
    let mut record = ApprovalRequest::pending(
        PendingApprovalStart {
            agent_id: &run.agent_id,
            session_id: &run.session_id,
            run_id: &run.run_id,
            call: &projected,
            timeout_ms: 1000,
            reason: None,
        },
        1,
    );
    record.resolve(
        ApprovalStatus::Allowed,
        ApprovalResolution {
            decision: Some(ApprovalDecisionKind::AllowOnce),
            note: None,
            matcher: None,
            rule_id: None,
            resolved_by: ResolvedBy::Owner,
            resolved_at_ms: 2,
        },
    );
    witness.resolved_approval(call, &record).unwrap()
}

async fn persistent(fixture: &Fixture) {
    fixture.state.write().await.set_control_plane_store(Some(
        crate::control_plane_store::ControlPlaneStoreConfig::Json(
            fixture.private.path().join("control-plane.json"),
        ),
    ));
}
async fn reserve_native(
    fixture: &Fixture,
) -> (
    crate::agent_runs::action_receipts::RunActionJournal,
    ApplyingDocument,
) {
    persistent(fixture).await;
    let witness = fixture
        .service
        .capture_apply(&fixture.run, &fixture.call)
        .await
        .unwrap()
        .unwrap();
    let proof = owner(&witness, &fixture.call, &fixture.run);
    let journal = crate::agent_runs::action_receipts::RunActionJournal::new(
        &fixture.state,
        fixture.coordinator.control_plane_transactions(),
        fixture.run.clone(),
        CancelSignal::new(),
        None,
    );
    journal.begin(&fixture.call).await.unwrap();
    let applying = fixture
        .service
        .reserve(
            fixture.run.clone(),
            fixture.call.clone(),
            witness,
            proof,
            CancelSignal::new(),
        )
        .await
        .unwrap();
    (journal, applying)
}
async fn publish_native(
    fixture: &Fixture,
    record_proof: bool,
) -> (
    crate::agent_runs::action_receipts::RunActionJournal,
    ApplyingDocument,
) {
    let (journal, applying) = reserve_native(fixture).await;
    fixture
        .service
        .begin_publication(
            fixture.run.clone(),
            fixture.call.clone(),
            CancelSignal::new(),
        )
        .await
        .unwrap();
    let workspace =
        crate::tools::workspace_bytes::WorkspaceBytes::open(fixture._root.path()).unwrap();
    let mut authority = None;
    workspace
        .create_new_bytes_atomic(&applying.draft.file_path, &applying.bytes, || {
            authority = Some(
                journal
                    .document_revalidate_now(fixture._root.path(), &fixture.call, true)
                    .map_err(std::io::Error::other)?,
            );
            Ok(())
        })
        .unwrap();
    assert_eq!(
        workspace_snapshot(fixture._root.path(), &applying.draft.file_path)
            .unwrap()
            .unwrap(),
        applying.bytes
    );
    drop(authority);
    if record_proof {
        fixture
            .service
            .record_readback(
                applying.draft.id.clone(),
                fixture.call.id.clone(),
                digest(&applying.bytes),
                applying.bytes.len() as u64,
                None,
            )
            .await
            .unwrap();
    }
    (journal, applying)
}

pub(crate) async fn publication_readback_proof_fixture() -> Vec<u8> {
    let fixture = fixture().await;
    let (_journal, applying) = publish_native(&fixture, true).await;
    let bytes = std::fs::read(
        fixture
            .private
            .path()
            .join("document-edits/proof_proposal.json"),
    )
    .unwrap();
    let proof: super::recovery::PublicationReadbackProof = serde_json::from_slice(&bytes).unwrap();
    let guard = fixture.state.read().await;
    assert!(proof.matches(
        guard.document_edits.get("proposal").unwrap(),
        guard.runs.get(&fixture.run.run_id).unwrap(),
        &fixture.call.id,
    ));
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["proposal_id"], "proposal");
    assert_eq!(value["file_path"], "copy.docx");
    assert_eq!(value["candidate_sha256"], digest(&applying.bytes));
    assert_eq!(value["source_sha256"], digest(b"source bytes"));
    assert!(value["scope"].is_object());
    assert!(value["owner_approval"].is_object());
    bytes
}

async fn fresh_transition(fixture: &Fixture, mut draft: DocumentEditDraft, label: &str) {
    let _transaction = fixture
        .coordinator
        .control_plane_transactions()
        .lock_owned()
        .await;
    let old = fixture.state.read().await.document_edits.clone();
    let mut next = old.clone();
    draft.id = label.into();
    draft.file_path = format!("{label}.docx");
    draft.expected_destination = ExpectedDestination::Absent;
    next.insert(draft, anima_core::primitives::now_millis())
        .unwrap();
    fixture.service.persist(&old, next).await.unwrap();
    assert!(fixture
        .state
        .read()
        .await
        .document_edits
        .get(label)
        .is_some());
}
async fn restarted(fixture: &Fixture) -> (crate::app::SharedDaemonState, DocumentEditService) {
    let config = crate::control_plane_store::ControlPlaneStoreConfig::Json(
        fixture.private.path().join("control-plane.json"),
    );
    let snapshot = crate::control_plane_store::load_control_plane_snapshot(&config)
        .await
        .unwrap()
        .unwrap();
    let mut state = crate::state::DaemonState::new();
    state.restore_control_plane_snapshot(snapshot).unwrap();
    state.set_control_plane_store(Some(config));
    state.document_edit_store = Some(
        DocumentEditStore::open_existing(&fixture.private.path().join("document-edits")).unwrap(),
    );
    let state = std::sync::Arc::new(tokio::sync::RwLock::new(state));
    let service = DocumentEditService::new(
        state.clone(),
        std::sync::Arc::new(tokio::sync::Mutex::new(())),
    );
    (state, service)
}

#[tokio::test]
async fn document_edits_observed_uncertain_reservation_is_a_live_base_for_ordinary_and_new_saves() {
    let fixture = fixture().await;
    persistent(&fixture).await;
    let witness = fixture
        .service
        .capture_apply(&fixture.run, &fixture.call)
        .await
        .unwrap()
        .unwrap();
    let proof = owner(&witness, &fixture.call, &fixture.run);
    let journal = crate::agent_runs::action_receipts::RunActionJournal::new(
        &fixture.state,
        fixture.coordinator.control_plane_transactions(),
        fixture.run.clone(),
        CancelSignal::new(),
        None,
    );
    journal.begin(&fixture.call).await.unwrap();
    let gate = fixture
        .state
        .write()
        .await
        .install_test_control_plane_uncertain_save_gate();
    gate.release.add_permits(1);
    assert_eq!(
        fixture
            .service
            .reserve(
                fixture.run.clone(),
                fixture.call.clone(),
                witness,
                proof,
                CancelSignal::new()
            )
            .await
            .err()
            .as_deref(),
        Some(UNCERTAIN)
    );
    let draft = fixture
        .state
        .read()
        .await
        .document_edits
        .get("proposal")
        .unwrap()
        .draft
        .clone();
    assert_eq!(
        fixture
            .state
            .read()
            .await
            .document_edits
            .get("proposal")
            .unwrap()
            .publication_phase,
        DocumentPublicationPhase::Reserved
    );
    fixture
        .state
        .write()
        .await
        .control_plane_persist_request()
        .save()
        .await
        .unwrap();
    fresh_transition(&fixture, draft, "after-uncertain-reserve").await;
    assert!(!fixture._root.path().join("copy.docx").exists());
}

#[tokio::test]
async fn document_edits_observed_uncertain_confirmation_keeps_provenance_and_accepts_new_saves() {
    let fixture = fixture().await;
    let (_journal, applying) = publish_native(&fixture, true).await;
    let draft = applying.draft.clone();
    let gate = fixture
        .state
        .write()
        .await
        .install_test_control_plane_uncertain_save_gate();
    gate.release.add_permits(1);
    assert_eq!(
        fixture
            .service
            .confirm(
                applying.draft.id,
                fixture.call.id.clone(),
                digest(&applying.bytes),
                None
            )
            .await
            .err()
            .as_deref(),
        Some(UNCERTAIN)
    );
    assert!(fixture
        .state
        .read()
        .await
        .document_edits
        .receipt_for_proposal("proposal")
        .is_some());
    assert!(fixture
        .state
        .read()
        .await
        .document_edits
        .get("proposal")
        .is_none());
    fixture
        .state
        .write()
        .await
        .control_plane_persist_request()
        .save()
        .await
        .unwrap();
    fresh_transition(&fixture, draft, "after-uncertain-confirm").await;
    assert_eq!(
        std::fs::read(fixture._root.path().join("copy.docx")).unwrap(),
        b"candidate bytes"
    );
    assert_eq!(
        fixture
            .state
            .read()
            .await
            .runs
            .get(&fixture.run.run_id)
            .unwrap()
            .action_receipts[0]
            .status,
        crate::runs::ActionStatus::Started,
        "reconciliation must not invent successful native action evidence"
    );
}

#[tokio::test]
async fn document_edits_restart_releases_only_reserved_unchanged_preimages() {
    let fixture = fixture().await;
    reserve_native(&fixture).await;
    let (state, service) = restarted(&fixture).await;
    let reports = service.reconcile_pending().await.unwrap();
    assert_eq!(
        reports[0].disposition,
        super::recovery::ReconciliationDisposition::NoEffectProven
    );
    assert!(state.read().await.document_edits.get("proposal").is_none());
    assert!(!fixture._root.path().join("copy.docx").exists());
    assert_eq!(
        state
            .read()
            .await
            .runs
            .get(&fixture.run.run_id)
            .unwrap()
            .action_receipts[0]
            .status,
        crate::runs::ActionStatus::Started
    );
}

#[tokio::test]
async fn document_edits_restart_finalizes_proven_readback_without_replaying_a_native_effect() {
    let fixture = fixture().await;
    publish_native(&fixture, true).await;
    let before = std::fs::read(fixture._root.path().join("copy.docx")).unwrap();
    let (state, service) = restarted(&fixture).await;
    let reports = service.reconcile_pending().await.unwrap();
    assert_eq!(
        reports[0].disposition,
        super::recovery::ReconciliationDisposition::PublicationProven
    );
    assert!(state
        .read()
        .await
        .document_edits
        .receipt_for_proposal("proposal")
        .is_some());
    assert!(state.read().await.document_edits.get("proposal").is_none());
    assert_eq!(
        std::fs::read(fixture._root.path().join("copy.docx")).unwrap(),
        before
    );
    assert_eq!(
        state
            .read()
            .await
            .runs
            .get(&fixture.run.run_id)
            .unwrap()
            .action_receipts[0]
            .status,
        crate::runs::ActionStatus::Started
    );
}

#[tokio::test]
async fn document_edits_failed_confirmation_pins_native_evidence_until_read_only_settlement() {
    let fixture = fixture().await;
    assert_eq!(
        fixture
            .state
            .read()
            .await
            .document_edits
            .pending_native_scopes()
            .count(),
        0,
        "a prepared review retains no native run evidence"
    );
    let (_journal, applying) = publish_native(&fixture, true).await;
    let gate = fixture
        .state
        .write()
        .await
        .install_test_control_plane_save_gate(true);
    gate.release.add_permits(1);
    assert!(fixture
        .service
        .confirm(
            applying.draft.id,
            fixture.call.id.clone(),
            digest(&applying.bytes),
            None,
        )
        .await
        .is_err());
    let before = std::fs::read(fixture._root.path().join("copy.docx")).unwrap();
    let now = anima_core::primitives::now_millis();
    let old = now - 25 * 60 * 60 * 1000;
    {
        let mut state = fixture.state.write().await;
        let source = state.runs.get_mut(&fixture.run.run_id).unwrap();
        source.created_at_ms = old;
        source.started_at_ms = Some(old);
        source.finish(crate::runs::RunStatus::Failed, None, old);
        source.mirrored = true;
        for index in 0..60 {
            let mut newer = crate::runs::RunRecord::running(
                crate::runs::RunStart {
                    agent_id: fixture.run.agent_id.clone(),
                    session_id: fixture.run.session_id.clone(),
                    source: crate::runs::RunSource::Web,
                    source_ref: None,
                    idempotency_key: None,
                    text: format!("newer terminal run {index}"),
                    model: "fixture".into(),
                    provider: None,
                    parent_run_id: None,
                },
                now - 1000 + index,
            );
            newer.finish(crate::runs::RunStatus::Completed, None, now - 1000 + index);
            newer.mirrored = true;
            state.runs.insert(newer);
        }
        let snapshot = state.control_plane_snapshot();
        for foreign_agent in [false, true] {
            let mut foreign = snapshot.clone();
            let source = foreign
                .runs
                .iter_mut()
                .find(|run| run.id == fixture.run.run_id)
                .unwrap();
            if foreign_agent {
                source.agent_id = "foreign-agent".into();
            } else {
                source.session_id = "chat:foreign".into();
            }
            assert!(!crate::runs::origin_pins(&foreign).contains(&fixture.run.run_id));
        }
        let pins = crate::runs::origin_pins(&snapshot);
        assert!(pins.contains(&fixture.run.run_id));
        let mut without_pin = state.runs.clone();
        without_pin.prune(now);
        assert!(without_pin.get(&fixture.run.run_id).is_none());
        state.runs.prune_with_pins(now, &pins);
        assert!(state.runs.get(&fixture.run.run_id).is_some());
        assert_eq!(state.runs.for_agent(&fixture.run.agent_id).len(), 51);
        state.control_plane_persist_request().save().await.unwrap();
    }
    let (state, service) = restarted(&fixture).await;
    assert!(state.read().await.runs.get(&fixture.run.run_id).is_some());
    assert_eq!(
        state
            .read()
            .await
            .document_edits
            .pending_native_scopes()
            .count(),
        1
    );
    let reports = service.reconcile_pending().await.unwrap();
    assert_eq!(
        reports[0].disposition,
        super::recovery::ReconciliationDisposition::PublicationProven
    );
    {
        let mut state = state.write().await;
        assert!(state
            .document_edits
            .receipt_for_proposal("proposal")
            .is_some());
        assert_eq!(state.document_edits.pending_native_scopes().count(), 0);
        assert_eq!(
            state.runs.get(&fixture.run.run_id).unwrap().action_receipts[0].status,
            crate::runs::ActionStatus::Started,
            "artifact settlement cannot fabricate a successful native action receipt"
        );
        let pins = crate::runs::origin_pins(&state.control_plane_snapshot());
        assert!(!pins.contains(&fixture.run.run_id));
        state.runs.prune_with_pins(now, &pins);
        assert!(state.runs.get(&fixture.run.run_id).is_none());
    }
    assert_eq!(
        std::fs::read(fixture._root.path().join("copy.docx")).unwrap(),
        before
    );
}

#[tokio::test]
async fn document_edits_restart_distinguishes_an_observed_candidate_from_proven_publication() {
    let fixture = fixture().await;
    publish_native(&fixture, false).await;
    let (state, service) = restarted(&fixture).await;
    let reports = service.reconcile_pending().await.unwrap();
    assert_eq!(
        reports[0].disposition,
        super::recovery::ReconciliationDisposition::ObservedCandidateEffectUnknown
    );
    assert!(state
        .read()
        .await
        .document_edits
        .receipt_for_proposal("proposal")
        .is_none());
    assert!(matches!(
        state
            .read()
            .await
            .document_edits
            .get("proposal")
            .unwrap()
            .status,
        DocumentEditStatus::RecoveryRequired { .. }
    ));
    assert_eq!(
        std::fs::read(fixture._root.path().join("copy.docx")).unwrap(),
        b"candidate bytes"
    );
}

#[tokio::test]
async fn document_edits_restart_keeps_publishing_without_an_effect_proof_ambiguous() {
    let fixture = fixture().await;
    reserve_native(&fixture).await;
    fixture
        .service
        .begin_publication(
            fixture.run.clone(),
            fixture.call.clone(),
            CancelSignal::new(),
        )
        .await
        .unwrap();
    let (state, service) = restarted(&fixture).await;
    let reports = service.reconcile_pending().await.unwrap();
    assert_eq!(
        reports[0].disposition,
        super::recovery::ReconciliationDisposition::EffectUnknown
    );
    assert!(matches!(
        state
            .read()
            .await
            .document_edits
            .get("proposal")
            .unwrap()
            .status,
        DocumentEditStatus::RecoveryRequired { .. }
    ));
    assert!(!fixture._root.path().join("copy.docx").exists());
}

#[tokio::test]
async fn document_edits_restart_never_releases_a_reserved_intent_with_a_changed_preimage() {
    let fixture = fixture().await;
    reserve_native(&fixture).await;
    std::fs::write(
        fixture._root.path().join("copy.docx"),
        b"external changed preimage",
    )
    .unwrap();
    let (state, service) = restarted(&fixture).await;
    let reports = service.reconcile_pending().await.unwrap();
    assert_eq!(
        reports[0].disposition,
        super::recovery::ReconciliationDisposition::EffectUnknown
    );
    assert!(state.read().await.document_edits.get("proposal").is_some());
    assert_eq!(
        std::fs::read(fixture._root.path().join("copy.docx")).unwrap(),
        b"external changed preimage"
    );
}

#[tokio::test]
async fn document_edits_restart_rejects_a_private_readback_proof_for_different_candidate_bytes() {
    let fixture = fixture().await;
    publish_native(&fixture, true).await;
    let path = fixture
        .private
        .path()
        .join("document-edits/proof_proposal.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    value["candidate_sha256"] = serde_json::json!("e".repeat(64));
    std::fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
    let (state, service) = restarted(&fixture).await;
    let reports = service.reconcile_pending().await.unwrap();
    assert_eq!(
        reports[0].disposition,
        super::recovery::ReconciliationDisposition::ObservedCandidateEffectUnknown
    );
    assert!(state
        .read()
        .await
        .document_edits
        .receipt_for_proposal("proposal")
        .is_none());
    assert!(state.read().await.document_edits.get("proposal").is_some());
}

#[tokio::test]
async fn document_edits_capture_rechecks_source_candidate_generation_and_grants() {
    let fixture = fixture().await;
    let witness = fixture
        .service
        .capture_apply(&fixture.run, &fixture.call)
        .await
        .unwrap()
        .unwrap();
    std::fs::write(fixture._root.path().join("source.docx"), b"changed").unwrap();
    assert!(fixture
        .service
        .revalidate_apply(&fixture.run, &fixture.call, &witness)
        .await
        .is_err());
    std::fs::write(fixture._root.path().join("source.docx"), b"source bytes").unwrap();
    fixture
        .state
        .write()
        .await
        .sessions
        .get_mut(&fixture.run.agent_id, &fixture.run.session_id)
        .unwrap()
        .clear_generation += 1;
    assert!(fixture
        .service
        .revalidate_apply(&fixture.run, &fixture.call, &witness)
        .await
        .is_err());
    fixture
        .state
        .write()
        .await
        .sessions
        .get_mut(&fixture.run.agent_id, &fixture.run.session_id)
        .unwrap()
        .clear_generation -= 1;
    let candidate = fixture
        .state
        .read()
        .await
        .document_edits
        .get("proposal")
        .unwrap()
        .draft
        .candidate_sha256
        .clone();
    std::fs::write(
        fixture
            .private
            .path()
            .join("document-edits")
            .join(format!("{candidate}.blob")),
        b"tampered bytes!",
    )
    .unwrap();
    assert!(fixture
        .service
        .capture_apply(&fixture.run, &fixture.call)
        .await
        .is_err());
    assert!(!fixture._root.path().join("copy.docx").exists());
}

#[tokio::test]
async fn document_edits_failed_reservation_save_exposes_no_applicable_intent_or_file_effect() {
    let fixture = fixture().await;
    let witness = fixture
        .service
        .capture_apply(&fixture.run, &fixture.call)
        .await
        .unwrap()
        .unwrap();
    let proof = owner(&witness, &fixture.call, &fixture.run);
    fixture.state.write().await.set_control_plane_store(Some(
        crate::control_plane_store::ControlPlaneStoreConfig::Json(
            fixture.private.path().to_path_buf(),
        ),
    ));
    assert!(fixture
        .service
        .reserve(
            fixture.run.clone(),
            fixture.call.clone(),
            witness,
            proof,
            CancelSignal::new()
        )
        .await
        .is_err());
    assert_eq!(
        fixture
            .state
            .read()
            .await
            .document_edits
            .get("proposal")
            .unwrap()
            .status,
        DocumentEditStatus::Prepared
    );
    assert!(!fixture._root.path().join("copy.docx").exists());
}

#[tokio::test]
async fn document_edits_unknown_upload_and_cross_session_review_are_refused() {
    let fixture = fixture().await;
    assert!(fixture
        .service
        .inspect(
            &fixture.run,
            SourceReference::Attachment {
                attachment_id: "known-elsewhere".into()
            },
            None,
            CancelSignal::new()
        )
        .await
        .is_err());
    assert!(fixture
        .service
        .review("proposal", &fixture.run.agent_id, "chat:other")
        .await
        .is_err());
    assert!(fixture
        .service
        .cancel(
            "proposal".into(),
            fixture.run.agent_id.clone(),
            "chat:other".into()
        )
        .await
        .is_err());
}

#[tokio::test]
async fn document_edits_download_requires_current_receipt_bytes_profile_and_canonical_workspace() {
    let fixture = fixture().await;
    let (_journal, applying) = publish_native(&fixture, true).await;
    fixture
        .service
        .confirm(
            applying.draft.id,
            fixture.call.id.clone(),
            digest(&applying.bytes),
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        fixture
            .service
            .published_download("copy.docx")
            .await
            .unwrap(),
        Some(b"candidate bytes".to_vec())
    );
    std::fs::write(fixture._root.path().join("unreceipted.docx"), b"unverified").unwrap();
    assert!(fixture
        .service
        .published_download("unreceipted.docx")
        .await
        .unwrap()
        .is_none());
    assert!(fixture
        .service
        .published_download("report.pdf")
        .await
        .unwrap()
        .is_none());
    std::fs::write(fixture._root.path().join("copy.docx"), b"modified bytes").unwrap();
    assert!(fixture
        .service
        .published_download("copy.docx")
        .await
        .unwrap()
        .is_none());
    std::fs::write(fixture._root.path().join("copy.docx"), b"candidate bytes").unwrap();
    let other = tempfile::tempdir().unwrap();
    std::fs::write(other.path().join("copy.docx"), b"candidate bytes").unwrap();
    fixture
        .state
        .write()
        .await
        .workspace
        .as_mut()
        .unwrap()
        .root_path = other.path().to_path_buf();
    assert!(fixture
        .service
        .published_download("copy.docx")
        .await
        .unwrap()
        .is_none());
    fixture
        .state
        .write()
        .await
        .workspace
        .as_mut()
        .unwrap()
        .root_path = fixture._root.path().to_path_buf();
    let changed = fixture.private.path().join("other-profile");
    super::store::initialize_owned_directory(&changed).unwrap();
    fixture.state.write().await.document_edit_store =
        Some(DocumentEditStore::open_existing(&changed).unwrap());
    assert!(fixture
        .service
        .published_download("copy.docx")
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn document_edits_native_gate_requires_allow_once_then_publishes_and_replays_without_writing()
{
    use crate::agent_runs::test_support::{decision, gated_context, pending_approvals, tool_input};
    let fixture = fixture().await;
    fixture.state.write().await.set_control_plane_store(Some(
        crate::control_plane_store::ControlPlaneStoreConfig::Json(
            fixture.private.path().join("control-plane.json"),
        ),
    ));
    let cancel = CancelSignal::new();
    let journal = crate::agent_runs::action_receipts::RunActionJournal::new(
        &fixture.state,
        fixture.coordinator.control_plane_transactions(),
        fixture.run.clone(),
        cancel.clone(),
        None,
    );
    let mut context = gated_context(&fixture.coordinator, &fixture.run, cancel)
        .await
        .with_action_journal(journal);
    context.document_edits = Some(fixture.service.clone());
    let agent = fixture
        .state
        .read()
        .await
        .get_agent(&fixture.run.agent_id)
        .unwrap()
        .state;
    let task_context = context.clone();
    let task_agent = agent.clone();
    let input = tool_input(&fixture.run.agent_id, &fixture.run.session_id);
    let call = fixture.call.clone();
    let applying =
        tokio::spawn(async move { task_context.execute_tool(task_agent, input, call).await });
    let pending = pending_approvals(&fixture.coordinator, 1).await.remove(0);
    assert!(!fixture._root.path().join("copy.docx").exists());
    assert!(fixture
        .coordinator
        .decide_approval(
            &pending.id,
            decision(ApprovalDecisionKind::AllowAlways, pending.revision)
        )
        .await
        .is_err());
    fixture
        .coordinator
        .decide_approval(
            &pending.id,
            decision(ApprovalDecisionKind::AllowOnce, pending.revision),
        )
        .await
        .unwrap();
    let result = applying.await.unwrap();
    assert_eq!(
        result.status,
        anima_core::TaskStatus::Success,
        "{:?}",
        result.error
    );
    assert_eq!(
        std::fs::read(fixture._root.path().join("copy.docx")).unwrap(),
        b"candidate bytes"
    );
    let receipt = fixture
        .state
        .read()
        .await
        .document_edits
        .receipt_for_proposal("proposal")
        .unwrap()
        .clone();
    let mut replay = fixture.call.clone();
    replay.id = "replay-call".into();
    let result = context
        .execute_tool(
            agent,
            tool_input(&fixture.run.agent_id, &fixture.run.session_id),
            replay,
        )
        .await;
    assert_eq!(
        result.status,
        anima_core::TaskStatus::Success,
        "{:?}",
        result.error
    );
    assert_eq!(
        fixture
            .state
            .read()
            .await
            .document_edits
            .receipt_for_proposal("proposal"),
        Some(&receipt)
    );
    assert!(fixture.state.read().await.approvals.pending().is_empty());
}

#[tokio::test]
async fn document_edits_source_change_during_native_owner_review_has_no_target_effect() {
    use crate::agent_runs::test_support::{decision, gated_context, pending_approvals, tool_input};
    let fixture = fixture().await;
    fixture.state.write().await.set_control_plane_store(Some(
        crate::control_plane_store::ControlPlaneStoreConfig::Json(
            fixture.private.path().join("control-plane.json"),
        ),
    ));
    let cancel = CancelSignal::new();
    let journal = crate::agent_runs::action_receipts::RunActionJournal::new(
        &fixture.state,
        fixture.coordinator.control_plane_transactions(),
        fixture.run.clone(),
        cancel.clone(),
        None,
    );
    let mut context = gated_context(&fixture.coordinator, &fixture.run, cancel)
        .await
        .with_action_journal(journal);
    context.document_edits = Some(fixture.service.clone());
    let agent = fixture
        .state
        .read()
        .await
        .get_agent(&fixture.run.agent_id)
        .unwrap()
        .state;
    let input = tool_input(&fixture.run.agent_id, &fixture.run.session_id);
    let call = fixture.call.clone();
    let applying = tokio::spawn(async move { context.execute_tool(agent, input, call).await });
    let pending = pending_approvals(&fixture.coordinator, 1).await.remove(0);
    std::fs::write(
        fixture._root.path().join("source.docx"),
        b"changed during review",
    )
    .unwrap();
    fixture
        .coordinator
        .decide_approval(
            &pending.id,
            decision(ApprovalDecisionKind::AllowOnce, pending.revision),
        )
        .await
        .unwrap();
    let result = applying.await.unwrap();
    assert_eq!(result.status, anima_core::TaskStatus::Error);
    assert!(!fixture._root.path().join("copy.docx").exists());
    assert_eq!(
        fixture
            .state
            .read()
            .await
            .document_edits
            .get("proposal")
            .unwrap()
            .status,
        DocumentEditStatus::Prepared
    );
}

#[tokio::test]
async fn document_edits_started_effect_settles_after_invalidation_without_reviving_write_authority()
{
    use crate::agent_runs::test_support::{decision, gated_context, pending_approvals, tool_input};
    use std::sync::{Arc, Mutex};
    use tokio::sync::{OwnedMutexGuard, Semaphore};
    let fixture = fixture().await;
    fixture.state.write().await.set_control_plane_store(Some(
        crate::control_plane_store::ControlPlaneStoreConfig::Json(
            fixture.private.path().join("control-plane.json"),
        ),
    ));
    let old_witness = fixture
        .service
        .capture_apply(&fixture.run, &fixture.call)
        .await
        .unwrap()
        .unwrap();
    let cancel = CancelSignal::new();
    let journal = crate::agent_runs::action_receipts::RunActionJournal::new(
        &fixture.state,
        fixture.coordinator.control_plane_transactions(),
        fixture.run.clone(),
        cancel.clone(),
        None,
    );
    let held: Arc<Mutex<Option<OwnedMutexGuard<()>>>> = Arc::new(Mutex::new(None));
    let entered = Arc::new(Semaphore::new(0));
    let hook_held = held.clone();
    let hook_entered = entered.clone();
    let transactions = fixture.coordinator.control_plane_transactions();
    let mut context = gated_context(&fixture.coordinator, &fixture.run, cancel.clone())
        .await
        .with_action_journal(journal.clone())
        .with_document_publication_hook(move || {
            *hook_held.lock().unwrap() = Some(
                transactions
                    .try_lock_owned()
                    .expect("publication begins after reservation transaction retires"),
            );
            hook_entered.add_permits(1);
        });
    context.document_edits = Some(fixture.service.clone());
    let agent = fixture
        .state
        .read()
        .await
        .get_agent(&fixture.run.agent_id)
        .unwrap()
        .state;
    let saved_tools = agent.config.tools.clone();
    let input = tool_input(&fixture.run.agent_id, &fixture.run.session_id);
    let call = fixture.call.clone();
    let applying = tokio::spawn(async move { context.execute_tool(agent, input, call).await });
    let pending = pending_approvals(&fixture.coordinator, 1).await.remove(0);
    fixture
        .coordinator
        .decide_approval(
            &pending.id,
            decision(ApprovalDecisionKind::AllowOnce, pending.revision),
        )
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), entered.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while std::fs::read(fixture._root.path().join("copy.docx"))
            .ok()
            .as_deref()
            != Some(b"candidate bytes".as_slice())
        {
            tokio::task::yield_now().await
        }
    })
    .await
    .unwrap();
    cancel.cancel();
    let (request, next, old_proof) = {
        let mut state = fixture.state.write().await;
        let old_proof = state
            .document_edits
            .get("proposal")
            .unwrap()
            .owner_approval
            .clone()
            .unwrap();
        assert_eq!(old_proof.id(), pending.id);
        let mut next = state.document_edits.clone();
        next.invalidate_agent(&fixture.run.agent_id);
        state
            .agents
            .get_mut(&fixture.run.agent_id)
            .unwrap()
            .update_config(AgentConfigUpdate {
                tools: Some(vec![]),
                ..Default::default()
            });
        let request = state
            .control_plane_persist_request()
            .with_document_edit_candidate(next.clone())
            .unwrap();
        (request, next, old_proof)
    };
    request.save_document_edit_candidate().await.unwrap();
    {
        let mut state = fixture.state.write().await;
        state.document_edits = next;
        state
            .agents
            .get_mut(&fixture.run.agent_id)
            .unwrap()
            .update_config(AgentConfigUpdate {
                tools: saved_tools,
                ..Default::default()
            });
        assert!(matches!(
            state.document_edits.get("proposal").unwrap().status,
            DocumentEditStatus::RecoveryRequired { .. }
        ));
        assert!(validate_final(
            &state,
            &fixture.run,
            &fixture.call,
            anima_core::primitives::now_millis()
        )
        .is_err());
    }
    assert!(
        fixture
            .service
            .capture_apply(&fixture.run, &fixture.call)
            .await
            .is_err(),
        "regrant must not revive the old applying witness"
    );
    drop(held.lock().unwrap().take());
    let _result = tokio::time::timeout(std::time::Duration::from_secs(5), applying)
        .await
        .unwrap()
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), journal.settle())
        .await
        .unwrap();
    let state = fixture.state.read().await;
    let receipt = state
        .document_edits
        .receipt_for_proposal("proposal")
        .unwrap();
    assert_eq!(receipt.approval_id, pending.id);
    assert!(state.document_edits.get("proposal").is_none());
    let action = state
        .runs
        .get(&fixture.run.run_id)
        .unwrap()
        .action_receipts
        .iter()
        .find(|action| action.tool_call_id == fixture.call.id)
        .unwrap();
    assert_ne!(
        action.status,
        crate::runs::ActionStatus::Started,
        "the actual observed effect must settle after cancellation"
    );
    assert!(action.finished_at_ms.is_some());
    drop(state);
    assert!(fixture
        .service
        .reserve(
            fixture.run.clone(),
            fixture.call.clone(),
            old_witness,
            old_proof,
            CancelSignal::new()
        )
        .await
        .is_err());
    assert_eq!(
        std::fs::read(fixture._root.path().join("copy.docx")).unwrap(),
        b"candidate bytes"
    );
}

#[test]
fn document_edits_destination_expectations_never_overwrite_a_changed_preimage() {
    assert!(destination_matches(None, &ExpectedDestination::Absent).is_ok());
    assert!(destination_matches(Some(b"existing"), &ExpectedDestination::Absent).is_err());
    assert!(destination_matches(
        Some(b"existing"),
        &ExpectedDestination::Present {
            sha256: digest(b"existing")
        }
    )
    .is_ok());
    assert!(destination_matches(
        Some(b"changed"),
        &ExpectedDestination::Present {
            sha256: digest(b"existing")
        }
    )
    .is_err());
    for path in [
        "uploads/a.docx",
        "../a.docx",
        ".git/a.docx",
        "secret.docx",
        "C:/a.docx",
        "a.pdf",
    ] {
        assert!(safe_path(path).is_err(), "{path}");
    }
}

#[tokio::test]
async fn document_owner_inspection_scope_requires_current_ordinary_chat_without_tool_grants() {
    let fixture = fixture().await;
    let mut state = fixture.state.write().await;
    state
        .agents
        .get_mut(&fixture.run.agent_id)
        .unwrap()
        .update_config(AgentConfigUpdate {
            tools: Some(vec![]),
            ..Default::default()
        });
    let before = super::owner_inspection::readonly_scope_in(
        &state,
        &fixture.run.agent_id,
        &fixture.run.session_id,
    )
    .unwrap();
    state
        .sessions
        .get_mut(&fixture.run.agent_id, &fixture.run.session_id)
        .unwrap()
        .clear_generation += 1;
    let after = super::owner_inspection::readonly_scope_in(
        &state,
        &fixture.run.agent_id,
        &fixture.run.session_id,
    )
    .unwrap();
    assert_ne!(before, after);
    assert!(super::owner_inspection::readonly_scope_in(
        &state,
        &fixture.run.agent_id,
        "chat:elsewhere"
    )
    .is_err());
    state
        .sessions
        .get_mut(&fixture.run.agent_id, &fixture.run.session_id)
        .unwrap()
        .pending_clear_run_id = Some("clearing".into());
    assert!(super::owner_inspection::readonly_scope_in(
        &state,
        &fixture.run.agent_id,
        &fixture.run.session_id
    )
    .is_err());
    let session = state
        .sessions
        .get_mut(&fixture.run.agent_id, &fixture.run.session_id)
        .unwrap();
    session.pending_clear_run_id = None;
    session.origin = crate::sessions::SessionOrigin::Api;
    assert!(super::owner_inspection::readonly_scope_in(
        &state,
        &fixture.run.agent_id,
        &fixture.run.session_id
    )
    .is_err());
}

async fn html_fixture() -> Fixture {
    let mut fixture = fixture().await;
    let source = b"<p>old</p><script>retained()</script>";
    std::fs::write(fixture._root.path().join("source.html"), source).unwrap();
    let edits: anima_document_content::OfficeEditSpec = serde_json::from_value(serde_json::json!({"operations":[{"type":"html_replace_text","start_utf8":3,"end_utf8":6,"expected_text":"old","new_text":"<new>"}]})).unwrap();
    let prepared =
        anima_document_content::prepare_office_edit(OfficeFormat::Html, source, &edits).unwrap();
    let mut state = fixture.state.write().await;
    let owner_scope =
        super::readonly_scope_in(&state, &fixture.run.agent_id, &fixture.run.session_id).unwrap();
    let preview: super::preview_context::PreviewEditContext = serde_json::from_value(serde_json::json!({"version":1,"source":{"kind":"workspace","path":"source.html","sha256":digest(source),"scope":owner_scope.source_scope("source.html", &digest(source)).unwrap()},"selection":{"kind":"html_text","start_utf8":3,"end_utf8":6,"expected_text":"old"}})).unwrap();
    let run_record = state.runs.get_mut(&fixture.run.run_id).unwrap();
    run_record.input.document_edit_context = Some(preview);
    run_record.document_edit_scope = Some(owner_scope);
    let mut draft = state.document_edits.get("proposal").unwrap().draft.clone();
    let staged = state
        .document_edit_store
        .as_ref()
        .unwrap()
        .stage(prepared.bytes())
        .unwrap();
    draft.source = DocumentEditSource::Workspace {
        file_path: "source.html".into(),
        sha256: digest(source),
    };
    draft.file_path = "copy.htm".into();
    draft.candidate_sha256 = staged.hash().into();
    draft.candidate_bytes = staged.size();
    draft.content_review = prepared.review().clone();
    state.document_edits = DocumentEditRegistry::default();
    let hash = state
        .document_edits
        .insert(draft, anima_core::primitives::now_millis())
        .unwrap();
    drop(staged);
    drop(state);
    let DataValue::Object(args) = crate::tools::generated_documents::schema_data(
        serde_json::json!({"proposal_id":"proposal","review_sha256":hash,"file_path":"copy.htm","expected_destination":{"state":"absent"}}),
    ) else {
        unreachable!()
    };
    fixture.call.args = args;
    fixture
}

#[tokio::test]
async fn html_document_edit_uses_exact_native_allow_once_publication_and_replay() {
    use crate::agent_runs::test_support::{decision, gated_context, pending_approvals, tool_input};
    let fixture = html_fixture().await;
    persistent(&fixture).await;
    let cancel = CancelSignal::new();
    let journal = crate::agent_runs::action_receipts::RunActionJournal::new(
        &fixture.state,
        fixture.coordinator.control_plane_transactions(),
        fixture.run.clone(),
        cancel.clone(),
        None,
    );
    let mut context = gated_context(&fixture.coordinator, &fixture.run, cancel)
        .await
        .with_action_journal(journal);
    context.document_edits = Some(fixture.service.clone());
    let agent = fixture
        .state
        .read()
        .await
        .get_agent(&fixture.run.agent_id)
        .unwrap()
        .state;
    let task_context = context.clone();
    let task_agent = agent.clone();
    let input = tool_input(&fixture.run.agent_id, &fixture.run.session_id);
    let call = fixture.call.clone();
    let applying =
        tokio::spawn(async move { task_context.execute_tool(task_agent, input, call).await });
    let pending = pending_approvals(&fixture.coordinator, 1).await.remove(0);
    assert!(!fixture._root.path().join("copy.htm").exists());
    assert!(fixture
        .coordinator
        .decide_approval(
            &pending.id,
            decision(ApprovalDecisionKind::AllowAlways, pending.revision)
        )
        .await
        .is_err());
    fixture
        .coordinator
        .decide_approval(
            &pending.id,
            decision(ApprovalDecisionKind::AllowOnce, pending.revision),
        )
        .await
        .unwrap();
    let result = applying.await.unwrap();
    assert_eq!(
        result.status,
        anima_core::TaskStatus::Success,
        "{:?}",
        result.error
    );
    assert_eq!(
        std::fs::read(fixture._root.path().join("copy.htm")).unwrap(),
        b"<p>&lt;new&gt;</p><script>retained()</script>"
    );
    let receipt = fixture
        .state
        .read()
        .await
        .document_edits
        .receipt_for_proposal("proposal")
        .unwrap()
        .clone();
    assert_eq!(receipt.format, OfficeFormat::Html);
    let mut replay = fixture.call.clone();
    replay.id = "html-replay".into();
    let result = context
        .execute_tool(
            agent,
            tool_input(&fixture.run.agent_id, &fixture.run.session_id),
            replay,
        )
        .await;
    assert_eq!(
        result.status,
        anima_core::TaskStatus::Success,
        "{:?}",
        result.error
    );
    assert_eq!(
        fixture
            .state
            .read()
            .await
            .document_edits
            .receipt_for_proposal("proposal"),
        Some(&receipt)
    );
}

#[tokio::test]
async fn html_document_edit_refuses_changed_source_under_the_review_witness() {
    let fixture = html_fixture().await;
    let witness = fixture
        .service
        .capture_apply(&fixture.run, &fixture.call)
        .await
        .unwrap()
        .unwrap();
    std::fs::write(fixture._root.path().join("source.html"), b"<p>changed</p>").unwrap();
    assert!(fixture
        .service
        .revalidate_apply(&fixture.run, &fixture.call, &witness)
        .await
        .is_err());
    assert!(!fixture._root.path().join("copy.htm").exists());
}

#[path = "preview_context_tests.rs"]
mod preview_context_tests;
