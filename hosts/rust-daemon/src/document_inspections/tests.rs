use super::*;
use crate::agent_runs::test_support::{
    approving_coordinator, coordinator_state, ledger_run, patient, ScriptedModel,
};
use crate::approvals::ApprovalPolicy;
use anima_core::{AgentConfigUpdate, CancelSignal};

struct Fixture {
    root: tempfile::TempDir,
    _private: crate::document_edits::store::test_support::PrivateTestDirectory,
    service: DocumentInspectionService,
    state: crate::app::SharedDaemonState,
    run: crate::runs::RunLink,
    execution: anima_core::execution_limits::ExecutionContext,
    _coordinator: crate::agent_runs::AgentRunCoordinator,
}

async fn fixture() -> Fixture {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("source.docx"), b"captured source").unwrap();
    let private = crate::document_edits::store::test_support::private_directory();
    let private_path = private.path().join("document-edits");
    crate::document_edits::store::initialize_owned_directory(&private_path).unwrap();
    let (coordinator, agent) = approving_coordinator(
        ScriptedModel::new(vec![]),
        ApprovalPolicy::default(),
        patient(),
    )
    .await;
    let run = ledger_run(&coordinator, &agent, "chat:inspection").await;
    let state = coordinator_state(&coordinator);
    {
        let mut guard = state.write().await;
        guard.workspace = Some(crate::control_plane_store::WorkspaceConfig {
            root_path: root.path().to_path_buf(),
            ..Default::default()
        });
        guard.document_edit_store =
            Some(crate::document_edits::DocumentEditStore::open_existing(&private_path).unwrap());
        guard
            .update_agent(
                &agent,
                AgentConfigUpdate {
                    tools: Some(
                        crate::tools::ToolRegistry::new()
                            .resolve_descriptors(["inspect_office_document"])
                            .unwrap(),
                    ),
                    ..Default::default()
                },
            )
            .unwrap();
    }
    let (candidate, persist) = {
        let mut guard = state.write().await;
        guard.set_control_plane_store(Some(
            crate::control_plane_store::ControlPlaneStoreConfig::Json(
                private.path().join("control.json"),
            ),
        ));
        // ledger_run seeds a live test record; admit its one host scope once,
        // then retain the exact context throughout this fixture's inspections.
        let record = guard.runs.get(&run.run_id).unwrap().clone();
        let candidate = guard
            .execution_budgets
            .prepare_run(&record, anima_core::primitives::now_millis())
            .unwrap();
        (candidate, guard.control_plane_persist_request())
    };
    crate::execution_budgets::save_admission(&state, candidate, persist)
        .await
        .unwrap();
    let execution = crate::execution_budgets::context_for_run(
        &state,
        coordinator.control_plane_transactions(),
        &run.run_id,
        CancelSignal::new(),
    )
    .await
    .unwrap();
    Fixture {
        root,
        _private: private,
        service: DocumentInspectionService::new(state.clone()),
        state,
        run,
        execution,
        _coordinator: coordinator,
    }
}

fn request() -> InspectionRequest {
    InspectionRequest {
        subject: InspectionSubject::Workspace {
            file_path: "source.docx".into(),
            expected_sha256: digest(b"captured source"),
        },
    }
}

#[test]
fn inspection_request_rejects_unrecognized_fields_and_subjects() {
    for value in [
        serde_json::json!({"subject":{"kind":"workspace","file_path":"source.docx","expected_sha256":"a".repeat(64),"pass":true}}),
        serde_json::json!({"subject":{"kind":"workspace","file_path":"source.docx","expected_sha256":"a".repeat(64)},"expected_result":"pass"}),
        serde_json::json!({"subject":{"kind":"url","url":"https://example.org"}}),
    ] {
        assert!(serde_json::from_value::<InspectionRequest>(value).is_err());
    }
}

#[tokio::test]
async fn inspection_capture_rechecks_exact_source_and_current_grant() {
    let f = fixture().await;
    let cancel = CancelSignal::new();
    let capture = f
        .service
        .capture(
            &f.run.agent_id,
            &f.run.session_id,
            Some(&f.run),
            request(),
            &cancel,
        )
        .await
        .unwrap();
    f.service.validate_capture(&capture, &cancel).await.unwrap();
    std::fs::write(f.root.path().join("source.docx"), b"changed source").unwrap();
    assert!(f.service.validate_capture(&capture, &cancel).await.is_err());
    std::fs::write(f.root.path().join("source.docx"), b"captured source").unwrap();
    f.state
        .write()
        .await
        .update_agent(
            &f.run.agent_id,
            AgentConfigUpdate {
                tools: Some(vec![]),
                ..Default::default()
            },
        )
        .unwrap();
    f.state
        .write()
        .await
        .update_agent(
            &f.run.agent_id,
            AgentConfigUpdate {
                tools: Some(
                    crate::tools::ToolRegistry::new()
                        .resolve_descriptors(["inspect_office_document"])
                        .unwrap(),
                ),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(
        f.service.validate_capture(&capture, &cancel).await.is_err(),
        "regrant must never revive an inspection witness"
    );
}

#[tokio::test]
async fn inspection_owner_denies_attachments_without_current_selected_run() {
    let f = fixture().await;
    let result = f
        .service
        .inspect_owner(
            &f.run.agent_id,
            &f.run.session_id,
            InspectionRequest {
                subject: InspectionSubject::Attachment {
                    attachment_id: "upload".into(),
                    expected_sha256: "a".repeat(64),
                },
            },
            CancelSignal::new(),
        )
        .await;
    assert!(result.unwrap_err().contains("current owner run"));
}

#[tokio::test]
async fn inspection_wrong_hash_cancelled_read_and_unconfigured_engine_fail_closed() {
    let f = fixture().await;
    let mut wrong = request();
    if let InspectionSubject::Workspace {
        expected_sha256, ..
    } = &mut wrong.subject
    {
        *expected_sha256 = "a".repeat(64);
    }
    assert!(f
        .service
        .inspect_for_run(&f.run, &f.execution, wrong, CancelSignal::new())
        .await
        .is_err());
    let cancel = CancelSignal::new();
    cancel.cancel();
    assert!(f
        .service
        .inspect_for_run(&f.run, &f.execution, request(), cancel)
        .await
        .is_err());
    let error = f
        .service
        .inspect_for_run(&f.run, &f.execution, request(), CancelSignal::new())
        .await
        .unwrap_err();
    assert!(error.contains("unavailable") || error.contains("unsupported"));
    assert!(f
        .service
        .get(&f.run.agent_id, &f.run.session_id, &"a".repeat(64))
        .await
        .is_err());
}

#[tokio::test]
async fn inspection_session_clear_and_workspace_switch_retire_capture_authority() {
    let f = fixture().await;
    let cancel = CancelSignal::new();
    let capture = f
        .service
        .capture(&f.run.agent_id, &f.run.session_id, None, request(), &cancel)
        .await
        .unwrap();
    let workspace = f.state.read().await.workspace.clone();
    f.state.write().await.set_workspace(None);
    f.state.write().await.set_workspace(workspace);
    assert!(f.service.validate_capture(&capture, &cancel).await.is_err());
}

#[tokio::test]
async fn inspection_store_preserves_unknown_files_and_never_accepts_forged_records() {
    let f = fixture().await;
    let parent = f.state.read().await.document_edit_store.clone().unwrap();
    let store =
        crate::document_inspections::store::DocumentInspectionStore::open(parent.clone()).unwrap();
    let root = parent.inspection_directory().unwrap();
    let unknown = root.join("unrelated.txt");
    std::fs::write(&unknown, b"keep").unwrap();
    assert!(store.prune(anima_core::primitives::now_millis()).is_err());
    assert_eq!(std::fs::read(&unknown).unwrap(), b"keep");
    std::fs::remove_file(&unknown).unwrap();
    let forged = root.join(format!(
        "inspection_{}_{}.json",
        "a".repeat(32),
        "b".repeat(64)
    ));
    std::fs::write(&forged, b"{\"expires_at_ms\":0}").unwrap();
    assert!(store.prune(anima_core::primitives::now_millis()).is_err());
    assert!(forged.exists(), "invalid records cannot authorize pruning");
}

#[tokio::test]
async fn completed_producer_preserves_owner_delivery_but_cannot_publish_new_inspection() {
    let f = fixture().await;
    let cancel = CancelSignal::new();
    let capture = f
        .service
        .capture(
            &f.run.agent_id,
            &f.run.session_id,
            Some(&f.run),
            request(),
            &cancel,
        )
        .await
        .unwrap();
    f.state
        .write()
        .await
        .runs
        .get_mut(&f.run.run_id)
        .unwrap()
        .finish(
            crate::runs::RunStatus::Completed,
            None,
            anima_core::primitives::now_millis(),
        );
    assert!(f.service.validate_capture(&capture, &cancel).await.is_err());
    f.service
        .read_source(&capture.witness, &cancel, AuthorityPhase::OwnerDelivery)
        .await
        .unwrap();
    std::fs::write(f.root.path().join("source.docx"), b"changed").unwrap();
    assert!(f
        .service
        .read_source(&capture.witness, &cancel, AuthorityPhase::OwnerDelivery)
        .await
        .is_err());
    std::fs::write(f.root.path().join("source.docx"), b"captured source").unwrap();
    f.state
        .write()
        .await
        .update_agent(
            &f.run.agent_id,
            AgentConfigUpdate {
                tools: Some(vec![]),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(f
        .service
        .read_source(&capture.witness, &cancel, AuthorityPhase::OwnerDelivery)
        .await
        .is_err());
}

fn fixture_pdf() -> Vec<u8> {
    let objects: &[&[u8]] = &[
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << >> /Contents 4 0 R >>",
        b"<< /Length 0 >>\nstream\n\nendstream",
    ];
    let mut bytes = b"%PDF-1.4\n".to_vec();
    let mut offsets = vec![0];
    for (index, object) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        bytes.extend_from_slice(object);
        bytes.extend_from_slice(b"\nendobj\n");
    }
    let xref = bytes.len();
    bytes.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len()).as_bytes());
    for offset in &offsets[1..] {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            offsets.len()
        )
        .as_bytes(),
    );
    bytes
}

async fn publish_fixture(f: &Fixture) -> (InspectionEnvelope, Vec<u8>) {
    use anima_document_content::OfficeFormat;
    use anima_document_visual::{
        inspection::{admit_office, build_report, CalculationEvidence, RendererIdentity},
        PdfPageEvidence, RecognizedText,
    };
    let source = include_bytes!("../document_edits/fixtures/header-safe.docx");
    std::fs::write(f.root.path().join("source.docx"), source).unwrap();
    let request = InspectionRequest {
        subject: InspectionSubject::Workspace {
            file_path: "source.docx".into(),
            expected_sha256: digest(source),
        },
    };
    let capture = f
        .service
        .capture(
            &f.run.agent_id,
            &f.run.session_id,
            Some(&f.run),
            request,
            &CancelSignal::new(),
        )
        .await
        .unwrap();
    let mut png = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png, 1, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&[0, 0, 0, 255])
            .unwrap();
    }
    let page = PdfPageEvidence {
        page: 1,
        width: 1,
        height: 1,
        png: png.clone(),
        recognized: RecognizedText::Unavailable,
    };
    let admission = admit_office(OfficeFormat::Docx, source).unwrap();
    let report = build_report(
        &capture.witness.source_version,
        &admission,
        RendererIdentity {
            backend: "libreoffice_uno".into(),
            version: "test-observation".into(),
            executable_sha256: digest(b"test-engine"),
            configuration_sha256: digest(b"test-config"),
            font_manifest_sha256: digest(b"test-fonts"),
            rasterizer: "test-rasterizer".into(),
        },
        &fixture_pdf(),
        &[page.clone()],
        CalculationEvidence::NotApplicable,
    )
    .unwrap();
    let envelope = InspectionEnvelope {
        version: 1,
        report_sha256: report.sha256().to_owned(),
        report,
        pages: vec![InspectionPageRef {
            page: 1,
            png_sha256: digest(&png),
            width: 1,
            height: 1,
        }],
    };
    let store = f.service.store().await.unwrap();
    store
        .publish(
            &capture.witness.scope,
            &capture.witness.request,
            &envelope,
            &[page],
            capture.witness.expires_at_ms,
        )
        .unwrap();
    let key = record_key(&capture.witness.scope, &envelope.report_sha256).unwrap();
    f.state
        .read()
        .await
        .document_inspections
        .remember(key, capture.witness)
        .unwrap();
    (envelope, png)
}

#[tokio::test]
async fn immutable_report_delivery_checks_actual_page_bytes_and_fresh_owner_authority() {
    let f = fixture().await;
    let (envelope, png) = publish_fixture(&f).await;
    let hash = &envelope.report_sha256;
    f.state
        .write()
        .await
        .runs
        .get_mut(&f.run.run_id)
        .unwrap()
        .finish(
            crate::runs::RunStatus::Completed,
            None,
            anima_core::primitives::now_millis(),
        );
    assert_eq!(
        f.service
            .get(&f.run.agent_id, &f.run.session_id, hash)
            .await
            .unwrap()
            .report_sha256,
        *hash
    );
    assert_eq!(
        f.service
            .page(&f.run.agent_id, &f.run.session_id, hash, 1)
            .await
            .unwrap(),
        png
    );
    assert!(f
        .service
        .get(&f.run.agent_id, "different-session", hash)
        .await
        .is_err());
    let root = f
        .state
        .read()
        .await
        .document_edit_store
        .as_ref()
        .unwrap()
        .inspection_directory()
        .unwrap();
    let record_directory = std::fs::read_dir(&root)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let page_path = std::fs::read_dir(record_directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().is_some_and(|extension| extension == "png"))
        .unwrap();
    std::fs::write(&page_path, b"changed page").unwrap();
    assert!(f
        .service
        .get(&f.run.agent_id, &f.run.session_id, hash)
        .await
        .is_err());
    assert!(f
        .service
        .page(&f.run.agent_id, &f.run.session_id, hash, 1)
        .await
        .is_err());
    std::fs::write(&page_path, png).unwrap();
    f.state.write().await.document_inspections = InspectionRuntime::default();
    assert!(
        f.service
            .get(&f.run.agent_id, &f.run.session_id, hash)
            .await
            .is_err(),
        "persisted immutable facts must not recreate live grants after restart"
    );
    assert!(std::fs::read_dir(root).unwrap().count() > 0);
}

#[tokio::test]
async fn inspection_store_retains_eight_records_and_prunes_only_verified_expired_records() {
    let f = fixture().await;
    let (envelope, _) = publish_fixture(&f).await;
    let scope =
        readonly_scope_in(&*f.state.read().await, &f.run.agent_id, &f.run.session_id).unwrap();
    let source_request = InspectionRequest {
        subject: InspectionSubject::Workspace {
            file_path: "source.docx".into(),
            expected_sha256: envelope.report.source().sha256.clone(),
        },
    };
    let root = f
        .state
        .read()
        .await
        .document_edit_store
        .as_ref()
        .unwrap()
        .inspection_directory()
        .unwrap();
    let store = f.service.store().await.unwrap();
    let page = anima_document_visual::PdfPageEvidence {
        page: 1,
        width: 1,
        height: 1,
        png: f
            .service
            .page(
                &f.run.agent_id,
                &f.run.session_id,
                &envelope.report_sha256,
                1,
            )
            .await
            .unwrap(),
        recognized: anima_document_visual::RecognizedText::Unavailable,
    };
    let expires = anima_core::primitives::now_millis() + 60_000;
    for _ in 1..8 {
        store
            .publish(&scope, &source_request, &envelope, &[page.clone()], expires)
            .unwrap();
    }
    assert!(store
        .publish(&scope, &source_request, &envelope, &[page], expires)
        .is_err());
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 8);
    store
        .prune(anima_core::primitives::now_millis() + 31 * 60 * 1000)
        .unwrap();
    assert_eq!(std::fs::read_dir(root).unwrap().count(), 0);
}

#[cfg(windows)]
#[tokio::test]
async fn inspection_transaction_has_protected_private_permissions_before_writing_pages() {
    let f = fixture().await;
    let (envelope, png) = publish_fixture(&f).await;
    let scope =
        readonly_scope_in(&*f.state.read().await, &f.run.agent_id, &f.run.session_id).unwrap();
    let request = InspectionRequest {
        subject: InspectionSubject::Workspace {
            file_path: "source.docx".into(),
            expected_sha256: envelope.report.source().sha256.clone(),
        },
    };
    let root = f
        .state
        .read()
        .await
        .document_edit_store
        .as_ref()
        .unwrap()
        .inspection_directory()
        .unwrap();
    let page = anima_document_visual::PdfPageEvidence {
        page: 1,
        width: 1,
        height: 1,
        png,
        recognized: anima_document_visual::RecognizedText::Unavailable,
    };
    let observed_staging = std::cell::Cell::new(false);
    f.service
        .store()
        .await
        .unwrap()
        .publish_authorized(
            &scope,
            &request,
            &envelope,
            &[page],
            anima_core::primitives::now_millis() + 60_000,
            || {
                for entry in std::fs::read_dir(&root).unwrap() {
                    let path = entry.unwrap().path();
                    if path
                        .file_name()
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .starts_with("publishing_")
                    {
                        // Existing-directory initialization only verifies permissions;
                        // it must succeed without repairing an inherited, unprotected ACL.
                        crate::document_edits::store::initialize_owned_directory(&path)?;
                        let held =
                            crate::tools::workspace_bytes::WorkspaceBytes::open(&path).unwrap();
                        let handle = held.held_directory().unwrap().into_std_file();
                        let descriptor =
                            win_custody::ObjectSecurityDescriptor::query_file_handle(&handle)
                                .unwrap();
                        assert_ne!(
                            descriptor.control().unwrap() & 0x1000,
                            0,
                            "the transaction must start with SE_DACL_PROTECTED before page writes"
                        );
                        observed_staging.set(true);
                    }
                }
                Ok(())
            },
        )
        .unwrap();
    assert!(
        observed_staging.get(),
        "publication must pass through protected private staging"
    );
}

async fn candidate_request(f: &Fixture) -> InspectionRequest {
    use crate::document_edits::{DocumentEditDraft, DocumentEditSource, ExpectedDestination};
    use anima_document_content::{
        OfficeEditReview, OfficeFormat, OfficeInventory, OfficePreservation,
    };
    let mut state = f.state.write().await;
    let scope = scope_in(&state, &f.run, "inspect_office_document").unwrap();
    let staged = state
        .document_edit_store
        .as_ref()
        .unwrap()
        .stage(b"private candidate")
        .unwrap();
    let hash = staged.hash().to_owned();
    let now = anima_core::primitives::now_millis();
    let inventory = OfficeInventory {
        version: 1,
        format: OfficeFormat::Docx,
        targets: vec![],
        retained_hyperlinks: vec![],
        limitations: vec![],
    };
    state
        .document_edits
        .insert(
            DocumentEditDraft {
                id: "inspection-candidate".into(),
                scope,
                source: DocumentEditSource::Workspace {
                    file_path: "source.docx".into(),
                    sha256: digest(b"captured source"),
                },
                attachment_record: None,
                file_path: "copy.docx".into(),
                expected_destination: ExpectedDestination::Absent,
                candidate_sha256: hash.clone(),
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
            },
            now,
        )
        .unwrap();
    drop(staged);
    InspectionRequest {
        subject: InspectionSubject::Candidate {
            proposal_id: "inspection-candidate".into(),
            expected_sha256: hash,
        },
    }
}

#[tokio::test]
async fn staged_candidate_requires_live_draft_original_source_and_hash_bound_private_bytes() {
    let f = fixture().await;
    let request = candidate_request(&f).await;
    let cancel = CancelSignal::new();
    let capture = f
        .service
        .capture(
            &f.run.agent_id,
            &f.run.session_id,
            Some(&f.run),
            request.clone(),
            &cancel,
        )
        .await
        .unwrap();
    assert_eq!(capture.bytes, b"private candidate");
    std::fs::write(f.root.path().join("source.docx"), b"changed original").unwrap();
    assert!(f.service.validate_capture(&capture, &cancel).await.is_err());
    std::fs::write(f.root.path().join("source.docx"), b"captured source").unwrap();
    let root = f
        .state
        .read()
        .await
        .document_edit_store
        .as_ref()
        .unwrap()
        .canonical_root()
        .unwrap()
        .to_path_buf();
    let blob = root.join(format!("{}.blob", request.subject.expected_sha256()));
    std::fs::write(&blob, b"modified private!").unwrap();
    assert!(f.service.validate_capture(&capture, &cancel).await.is_err());
    std::fs::write(blob, b"private candidate").unwrap();
    f.state
        .write()
        .await
        .document_edits
        .invalidate_agent(&f.run.agent_id);
    assert!(f.service.validate_capture(&capture, &cancel).await.is_err());
    assert!(f
        .service
        .capture(&f.run.agent_id, &f.run.session_id, None, request, &cancel)
        .await
        .is_err());
}

#[tokio::test]
async fn unselected_upload_is_denied_before_history_or_worker_access() {
    let f = fixture().await;
    let error = f
        .service
        .inspect_for_run(
            &f.run,
            &f.execution,
            InspectionRequest {
                subject: InspectionSubject::Attachment {
                    attachment_id: "unselected-upload".into(),
                    expected_sha256: "a".repeat(64),
                },
            },
            CancelSignal::new(),
        )
        .await
        .unwrap_err();
    assert!(error.contains("Select this uploaded document"));
}

#[tokio::test]
async fn followup_owner_run_can_inspect_a_prepared_prior_candidate_without_inheriting_apply_authority(
) {
    let f = fixture().await;
    let request = candidate_request(&f).await;
    f.state
        .write()
        .await
        .runs
        .get_mut(&f.run.run_id)
        .unwrap()
        .finish(
            crate::runs::RunStatus::Completed,
            None,
            anima_core::primitives::now_millis(),
        );
    let followup = ledger_run(&f._coordinator, &f.run.agent_id, &f.run.session_id).await;
    assert_ne!(followup.run_id, f.run.run_id);
    let capture = f
        .service
        .capture(
            &followup.agent_id,
            &followup.session_id,
            Some(&followup),
            request,
            &CancelSignal::new(),
        )
        .await
        .unwrap();
    assert_eq!(capture.bytes, b"private candidate");
    let state = f.state.read().await;
    let record = state.document_edits.get("inspection-candidate").unwrap();
    assert_eq!(
        record.draft.scope.run_id, f.run.run_id,
        "inspection must preserve original proposal provenance"
    );
    assert_eq!(record.status, DocumentEditStatus::Prepared);
    assert!(
        record.owner_approval.is_none(),
        "inspection cannot mint apply authority"
    );
}

#[tokio::test]
async fn followup_inspection_rejects_a_cancelled_candidate_producer() {
    let f = fixture().await;
    let request = candidate_request(&f).await;
    f.state
        .write()
        .await
        .runs
        .get_mut(&f.run.run_id)
        .unwrap()
        .finish(
            RunStatus::Cancelled,
            None,
            anima_core::primitives::now_millis(),
        );
    let followup = ledger_run(&f._coordinator, &f.run.agent_id, &f.run.session_id).await;
    assert!(
        f.service
            .capture(
                &followup.agent_id,
                &followup.session_id,
                Some(&followup),
                request,
                &CancelSignal::new()
            )
            .await
            .is_err(),
        "a new run cannot revive a cancelled producer's candidate"
    );
}

#[tokio::test]
async fn followup_candidate_keeps_its_original_project_binding() {
    let f = fixture().await;
    let request = candidate_request(&f).await;
    f.state
        .write()
        .await
        .runs
        .get_mut(&f.run.run_id)
        .unwrap()
        .finish(
            RunStatus::Completed,
            None,
            anima_core::primitives::now_millis(),
        );
    let followup = ledger_run(&f._coordinator, &f.run.agent_id, &f.run.session_id).await;
    {
        let mut state = f.state.write().await;
        state
            .sessions
            .get_mut(&f.run.agent_id, &f.run.session_id)
            .unwrap()
            .project_binding_revision += 1;
        let witness =
            crate::projects::capture_run_witness(&state, &followup.agent_id, &followup.session_id)
                .unwrap();
        state
            .runs
            .get_mut(&followup.run_id)
            .unwrap()
            .project_witness = Some(witness);
        scope_in(&state, &followup, "inspect_office_document").unwrap();
    }
    assert!(
        f.service
            .capture(
                &followup.agent_id,
                &followup.session_id,
                Some(&followup),
                request,
                &CancelSignal::new()
            )
            .await
            .is_err(),
        "current project authority cannot replace the candidate producer's lease"
    );
}

#[tokio::test]
async fn final_response_registration_rechecks_authority_after_capture_validation() {
    let f = fixture().await;
    let cancel = CancelSignal::new();
    let capture = f
        .service
        .capture(
            &f.run.agent_id,
            &f.run.session_id,
            Some(&f.run),
            request(),
            &cancel,
        )
        .await
        .unwrap();
    f.service.validate_capture(&capture, &cancel).await.unwrap();
    f.state
        .write()
        .await
        .update_agent(
            &f.run.agent_id,
            AgentConfigUpdate {
                tools: Some(vec![]),
                ..Default::default()
            },
        )
        .unwrap();
    let state = f.state.read().await;
    assert!(remember_current(&state, "final-response".into(), &capture.witness, &cancel).is_err());
    assert!(state
        .document_inspections
        .witness("final-response")
        .is_err());
    assert!(state
        .document_inspections
        .remember("cancelled-incoming".into(), capture.witness)
        .is_err());
}

#[tokio::test]
async fn interrupted_publication_and_retirement_recover_without_disabling_other_reports() {
    let f = fixture().await;
    let (envelope, _) = publish_fixture(&f).await;
    let parent = f.state.read().await.document_edit_store.clone().unwrap();
    let root = parent.inspection_directory().unwrap();
    let completed = std::fs::read_dir(&root)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert!(
        completed.is_dir(),
        "one record must publish atomically as a directory"
    );
    let completed_name = completed.file_name().unwrap().to_str().unwrap();
    let publishing = root.join(completed_name.replacen("inspection_", "publishing_", 1));
    std::fs::rename(&completed, &publishing).unwrap();
    let manifest = std::fs::read_dir(&publishing)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .unwrap();
    std::fs::write(manifest, b"{truncated manifest").unwrap();
    let store = f.service.store().await.unwrap();
    store.prune(anima_core::primitives::now_millis()).unwrap();
    assert!(
        !publishing.exists(),
        "incomplete task-owned publication must be recoverable"
    );
    let (replacement, _) = publish_fixture(&f).await;
    assert_eq!(replacement.report_sha256, envelope.report_sha256);
    let completed = std::fs::read_dir(&root)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let retiring = root.join(completed.file_name().unwrap().to_str().unwrap().replacen(
        "inspection_",
        "retiring_",
        1,
    ));
    std::fs::rename(&completed, &retiring).unwrap();
    let page = std::fs::read_dir(&retiring)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().is_some_and(|extension| extension == "png"))
        .unwrap();
    std::fs::remove_file(page).unwrap();
    store.prune(anima_core::primitives::now_millis()).unwrap();
    assert!(
        !retiring.exists(),
        "partially retired task-owned record must finish retiring"
    );
    publish_fixture(&f).await;
}

#[tokio::test]
async fn recovery_preserves_unknown_files_inside_an_owned_transaction() {
    let f = fixture().await;
    publish_fixture(&f).await;
    let root = f
        .state
        .read()
        .await
        .document_edit_store
        .as_ref()
        .unwrap()
        .inspection_directory()
        .unwrap();
    let completed = std::fs::read_dir(&root)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let publishing = root.join(completed.file_name().unwrap().to_str().unwrap().replacen(
        "inspection_",
        "publishing_",
        1,
    ));
    std::fs::rename(completed, &publishing).unwrap();
    let unknown = publishing.join("unrelated.txt");
    std::fs::write(&unknown, b"keep").unwrap();
    assert!(f
        .service
        .store()
        .await
        .unwrap()
        .prune(anima_core::primitives::now_millis())
        .is_err());
    assert_eq!(std::fs::read(unknown).unwrap(), b"keep");
}

#[tokio::test]
async fn inspection_rejects_another_runs_execution_before_source_capture() {
    let f = fixture().await;
    let mut execution = f.execution.clone();
    execution.run_id = "another-run".into();
    std::fs::remove_file(f.root.path().join("source.docx")).unwrap();
    let error = f
        .service
        .inspect_for_run(&f.run, &execution, request(), CancelSignal::new())
        .await
        .unwrap_err();
    assert!(
        error.contains("execution binding"),
        "binding denial must precede source or worker access: {error}"
    );
}

#[tokio::test]
async fn inspection_rejects_a_fresh_ephemeral_context_with_copied_binding_fields() {
    use anima_core::execution_limits::{ExecutionContext, ExecutionLimits};
    let f = fixture().await;
    let mut execution = ExecutionContext::ephemeral(
        f.run.run_id.clone(),
        ExecutionLimits::default(),
        CancelSignal::new(),
    )
    .unwrap();
    execution.scope_id = f.execution.scope_id.clone();
    execution.deadline_at_ms = f.execution.deadline_at_ms;
    let error = f
        .service
        .inspect_for_run(&f.run, &execution, request(), CancelSignal::new())
        .await
        .unwrap_err();
    assert!(
        error.contains("execution binding"),
        "public binding fields cannot replace the admitted host context: {error}"
    );
}

#[tokio::test]
async fn exhausted_run_authority_denies_inspection_before_source_or_worker_access() {
    use anima_core::execution_limits::SettlementOutcome;
    let f = fixture().await;
    for logical_id in ["earlier-tool-one", "earlier-tool-two"] {
        let mut permit = f.execution.reserve_tool(logical_id).await.unwrap();
        permit
            .finish(SettlementOutcome::Known(Default::default()))
            .await
            .unwrap();
    }
    f.state
        .write()
        .await
        .execution_budgets
        .scopes
        .get_mut(&f.execution.scope_id)
        .unwrap()
        .limits
        .budget
        .max_capability_steps = Some(1);
    std::fs::remove_file(f.root.path().join("source.docx")).unwrap();
    let error = f
        .service
        .inspect_for_run(&f.run, &f.execution, request(), CancelSignal::new())
        .await
        .unwrap_err();
    assert!(
        error.contains("allowance exhausted"),
        "shared exhaustion must deny before source capture and worker dispatch: {error}"
    );
}

#[tokio::test]
async fn tiny_run_artifact_allowance_denies_physical_inspection_before_dispatch() {
    let f = fixture().await;
    f.state
        .write()
        .await
        .execution_budgets
        .scopes
        .get_mut(&f.execution.scope_id)
        .unwrap()
        .limits
        .budget
        .max_artifact_bytes = Some(1);
    let error = f
        .service
        .inspect_for_run(&f.run, &f.execution, request(), CancelSignal::new())
        .await
        .unwrap_err();
    assert!(error.to_ascii_lowercase().contains("artifact") || error.contains("allowance exhausted"),
        "artifact admission must refuse before selecting or launching the unconfigured worker: {error}");
}

#[tokio::test]
async fn publication_refuses_execution_expiry_during_its_source_read() {
    struct ExpiringClock {
        calls: std::sync::atomic::AtomicUsize,
        deadline: u64,
    }
    #[async_trait::async_trait]
    impl anima_core::ExecutionClock for ExpiringClock {
        fn now_ms(&self) -> u64 {
            if self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                self.deadline - 1
            } else {
                self.deadline
            }
        }
    }
    let f = fixture().await;
    let cancel = CancelSignal::new();
    let capture = f
        .service
        .capture(
            &f.run.agent_id,
            &f.run.session_id,
            Some(&f.run),
            request(),
            &cancel,
        )
        .await
        .unwrap();
    let mut execution = f.execution.clone();
    execution.clock = std::sync::Arc::new(ExpiringClock {
        calls: std::sync::atomic::AtomicUsize::new(0),
        deadline: execution.deadline_at_ms,
    });
    let state = f.state.read().await;
    let error = validate_publication_capture(&state, &capture.witness, &cancel, Some(&execution))
        .unwrap_err();
    assert!(
        error.contains("deadline"),
        "expiry after the final source read cannot authorize publication: {error}"
    );
}

#[cfg(windows)]
#[tokio::test]
#[ignore = "parent-only real pinned Office service/store/custody acceptance; requires engine and exact built daemon executable"]
async fn real_engine_service_publishes_rereads_and_settles_actual_bytes_after_custody() {
    use crate::document_inspections::worker;
    use anima_core::execution_limits::{ExecutionOperation, SettlementOutcome};
    use anima_document_content::render_document;
    use std::path::PathBuf;

    let engine = worker::EngineConfig::from_environment()
        .expect("valid explicit operator engine configuration is required")
        .expect("both operator PROGRAM and TREE_SHA256 environment values are required");
    let executable = PathBuf::from(
        std::env::var_os("ANIMA_DOCUMENT_INSPECTION_DAEMON_EXE")
            .expect("ANIMA_DOCUMENT_INSPECTION_DAEMON_EXE must name the exact built daemon"),
    );
    assert!(
        executable.is_absolute(),
        "the built daemon path must be absolute"
    );
    let executable = executable
        .canonicalize()
        .expect("the exact built daemon must exist");
    assert_eq!(
        executable.file_name().and_then(|name| name.to_str()),
        Some("anima-daemon.exe")
    );
    assert!(
        executable.is_file(),
        "the exact built daemon must be a regular file"
    );
    let native_worker = worker::DocumentInspectionWorker::with_engine_config(Some(engine))
        .with_test_executable(executable);
    assert!(
        native_worker.test_physical_capacity_available(),
        "run this parent-only acceptance in isolation"
    );

    let f = fixture().await;
    f.state.write().await.document_inspections.worker = native_worker.clone();
    let before = f
        .state
        .read()
        .await
        .execution_budgets
        .scopes
        .get(&f.execution.scope_id)
        .unwrap()
        .clone();
    assert!(
        before.reservations.is_empty(),
        "use the fixture's one original admitted context"
    );
    let spec = serde_json::from_value(serde_json::json!({
        "format": "docx", "title": "Synthetic service acceptance",
        "blocks": [{"type": "paragraph", "text": "Original synthetic Office bytes through the complete owner service."}]
    })).unwrap();
    let source = render_document(&spec).unwrap().bytes;
    let source_hash = digest(&source);
    let source_path = f.root.path().join("source.docx");
    std::fs::write(&source_path, &source).unwrap();
    let request = InspectionRequest {
        subject: InspectionSubject::Workspace {
            file_path: "source.docx".into(),
            expected_sha256: source_hash.clone(),
        },
    };

    let envelope = f
        .service
        .inspect_for_run(&f.run, &f.execution, request, CancelSignal::new())
        .await
        .expect("real engine, process custody, and private atomic publication must all succeed");
    envelope.validate().unwrap();
    assert_eq!(envelope.report.source().sha256, source_hash);
    assert_eq!(envelope.report.source().byte_length, source.len() as u64);
    assert!(
        !envelope.pages.is_empty(),
        "the actual PDF must yield page evidence"
    );
    assert!(envelope.report.rendering().pdf_byte_length > 0);
    let reread = f
        .service
        .get(&f.run.agent_id, &f.run.session_id, &envelope.report_sha256)
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(&reread).unwrap(),
        serde_json::to_value(&envelope).unwrap()
    );
    let mut artifact_bytes = (serde_json::to_vec(&envelope.report).unwrap().len() as u64)
        .checked_add(envelope.report.rendering().pdf_byte_length)
        .unwrap();
    for page in &envelope.pages {
        let bytes = f
            .service
            .page(
                &f.run.agent_id,
                &f.run.session_id,
                &envelope.report_sha256,
                page.page,
            )
            .await
            .unwrap();
        assert!(
            bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
            "reread actual stored raster bytes"
        );
        assert_eq!(digest(&bytes), page.png_sha256);
        artifact_bytes = artifact_bytes.checked_add(bytes.len() as u64).unwrap();
    }
    assert_eq!(
        std::fs::read(&source_path).unwrap(),
        source,
        "inspection never rewrites the original"
    );
    assert!(
        native_worker.test_physical_capacity_available(),
        "successful custody cleanup must release the shared physical slot"
    );

    let after = f
        .state
        .read()
        .await
        .execution_budgets
        .scopes
        .get(&f.execution.scope_id)
        .unwrap()
        .clone();
    assert_eq!(
        after.reservations.len(),
        1,
        "rereading immutable evidence cannot reserve another effect"
    );
    let reservation = after.reservations.values().next().unwrap();
    assert_eq!(reservation.request.run_id, f.run.run_id);
    assert!(reservation
        .request
        .logical_call_id
        .starts_with("document-inspection:"));
    let ExecutionOperation::NativeArtifact { max_artifact_bytes } = reservation.request.operation
    else {
        panic!("the actual inspection must use the same host native artifact authority");
    };
    assert!(artifact_bytes > 0 && artifact_bytes <= max_artifact_bytes);
    let settlement = reservation
        .settlement
        .as_ref()
        .expect("physical retirement must precede success delivery");
    assert_eq!(
        settlement.outcome,
        SettlementOutcome::Artifact { artifact_bytes }
    );
    assert!(
        settlement.physical_retired,
        "checked process, pipe and directory retirement must precede settlement"
    );
    let consumption = after.consumption().unwrap();
    assert_eq!(consumption.charged_artifact_bytes, artifact_bytes);
    assert_eq!(consumption.inflight_operations, 0);
    println!(
        "SERVICE_ACCEPTANCE {}",
        serde_json::json!({
            "source_sha256": source_hash, "report_sha256": envelope.report_sha256,
            "page_count": envelope.pages.len(), "artifact_bytes": artifact_bytes,
            "physical_retired": true, "inflight_operations": consumption.inflight_operations,
            "original_unchanged": true, "report_and_pages_reread": true
        })
    );
}
