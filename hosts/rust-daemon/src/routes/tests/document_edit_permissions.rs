//! Acknowledged permission changes retire exact document review authority.
use crate::app::{DaemonConfig, SharedDaemonState};
use crate::control_plane_store::{
    load_control_plane_snapshot, ControlPlaneStoreConfig, WorkspaceConfig,
};
use crate::document_edits::{
    DocumentEditDraft, DocumentEditService, DocumentEditSource, DocumentEditStore,
    ExpectedDestination,
};
use crate::runs::{RunLink, RunRecord, RunSource, RunStart};
use crate::sessions::{SessionKind, SessionOrigin, SessionRecord, TitleSource};
use crate::state::DaemonState;
use anima_core::{DataValue, ToolCall};
use anima_document_content::{OfficeEditReview, OfficeFormat, OfficeInventory, OfficePreservation};
use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tower::ServiceExt;

struct Fixture {
    _workspace: tempfile::TempDir,
    _private: crate::document_edits::store::test_support::PrivateTestDirectory,
    state: SharedDaemonState,
    app: Router,
    run: RunLink,
    call: ToolCall,
    service: DocumentEditService,
    store: ControlPlaneStoreConfig,
}

async fn fixture() -> Fixture {
    let workspace = tempfile::tempdir().unwrap();
    let private = crate::document_edits::store::test_support::private_directory();
    std::fs::write(workspace.path().join("source.docx"), b"source bytes").unwrap();
    let root = private.path().join("document-edits");
    crate::document_edits::store::initialize_owned_directory(&root).unwrap();
    let mut state = DaemonState::new();
    let mut config: anima_core::AgentConfig = serde_json::from_value(
        serde_json::json!({"name":"document-owner","model":"deterministic"}),
    )
    .unwrap();
    config.tools = Some(
        state
            .tool_registry
            .resolve_descriptors([
                "inspect_office_document",
                "propose_document_edit",
                "apply_document_edit",
            ])
            .unwrap(),
    );
    let agent = state.create_agent(config).unwrap().state.id;
    state.workspace = Some(WorkspaceConfig {
        root_path: workspace.path().to_owned(),
        ..Default::default()
    });
    state.document_edit_store = Some(DocumentEditStore::open_existing(&root).unwrap());
    state.sessions.insert(SessionRecord::new(
        &agent,
        "chat:document",
        SessionKind::Chat,
        SessionOrigin::Web,
        "Document".into(),
        TitleSource::Owner,
        1,
    ));
    let record = RunRecord::running(
        RunStart {
            agent_id: agent.clone(),
            session_id: "chat:document".into(),
            source: RunSource::Web,
            source_ref: None,
            idempotency_key: None,
            text: "Edit selected document".into(),
            model: "deterministic".into(),
            provider: None,
            parent_run_id: None,
        },
        1,
    );
    let run = RunLink {
        agent_id: agent,
        session_id: record.session_id.clone(),
        run_id: record.id.clone(),
    };
    state.runs.insert(record);
    let scope =
        crate::document_edits::service::scope_in(&state, &run, "apply_document_edit").unwrap();
    let staged = state
        .document_edit_store
        .as_ref()
        .unwrap()
        .stage(b"candidate bytes")
        .unwrap();
    let inventory = OfficeInventory {
        version: 1,
        format: OfficeFormat::Docx,
        targets: Vec::new(),
        retained_hyperlinks: Vec::new(),
        limitations: Vec::new(),
    };
    let now = anima_core::primitives::now_millis();
    let hash = state
        .document_edits
        .insert(
            DocumentEditDraft {
                id: "proposal".into(),
                scope,
                source: DocumentEditSource::Workspace {
                    file_path: "source.docx".into(),
                    sha256: crate::document_edits::service::digest(b"source bytes"),
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
                    changes: Vec::new(),
                    preservation: OfficePreservation {
                        unchanged_parts: 1,
                        edited_parts: 1,
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
    let DataValue::Object(args) = crate::tools::generated_documents::schema_data(
        serde_json::json!({
            "proposal_id":"proposal","review_sha256":hash,"file_path":"copy.docx","expected_destination":{"state":"absent"}
        }),
    ) else {
        unreachable!()
    };
    let call = ToolCall {
        id: "apply-call".into(),
        name: "apply_document_edit".into(),
        args,
    };
    let store = ControlPlaneStoreConfig::Json(private.path().join("control-plane.json"));
    state.set_control_plane_store(Some(store.clone()));
    state.control_plane_persist_request().save().await.unwrap();
    let state = Arc::new(RwLock::new(state));
    let service = DocumentEditService::new(state.clone(), Arc::new(Mutex::new(())));
    let app = crate::app::app_with_state_for_owner_test(state.clone(), DaemonConfig::default());
    Fixture {
        _workspace: workspace,
        _private: private,
        state,
        app,
        run,
        call,
        service,
        store,
    }
}

fn request(fixture: &Fixture, change: &str, restore: bool) -> Request<Body> {
    let (method, path, body) = if change == "tool" {
        (
            "PATCH",
            format!("/api/agents/{}", fixture.run.agent_id),
            serde_json::json!({"tools":if restore {vec!["inspect_office_document","propose_document_edit","apply_document_edit"]} else {vec!["inspect_office_document","propose_document_edit"]}}),
        )
    } else {
        (
            "PUT",
            format!("/api/agents/{}/approval-policy", fixture.run.agent_id),
            serde_json::json!({"write":if restore {"allow"} else {"deny"},"exec":"ask","network":"allow","delegate":"allow"}),
        )
    };
    crate::routes::owner_test_support::authenticate(
        Request::builder()
            .method(method)
            .uri(path)
            .header("host", "127.0.0.1:8080")
            .header("origin", "http://localhost:4200")
            .header("content-type", "application/json"),
    )
    .body(Body::from(body.to_string()))
    .unwrap()
}

#[tokio::test]
async fn acknowledged_revoke_then_regrant_cannot_revive_an_old_document_review() {
    for change in ["tool", "policy"] {
        let fixture = fixture().await;
        let witness = fixture
            .service
            .capture_apply(&fixture.run, &fixture.call)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            fixture
                .app
                .clone()
                .oneshot(request(&fixture, change, false))
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
        assert!(fixture
            .state
            .read()
            .await
            .document_edits
            .get("proposal")
            .is_none());
        let saved = load_control_plane_snapshot(&fixture.store)
            .await
            .unwrap()
            .unwrap();
        assert!(saved.document_edits.get("proposal").is_none());
        assert_eq!(
            fixture
                .app
                .clone()
                .oneshot(request(&fixture, change, true))
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
        assert!(fixture
            .service
            .revalidate_apply(&fixture.run, &fixture.call, &witness)
            .await
            .is_err());
        assert!(!fixture._workspace.path().join("copy.docx").exists());
    }
}

#[tokio::test]
async fn known_failed_permission_save_preserves_the_prior_document_review() {
    for change in ["tool", "policy"] {
        let fixture = fixture().await;
        let witness = fixture
            .service
            .capture_apply(&fixture.run, &fixture.call)
            .await
            .unwrap()
            .unwrap();
        fixture
            .state
            .write()
            .await
            .set_control_plane_store(Some(ControlPlaneStoreConfig::Json(
                fixture._private.path().to_owned(),
            )));
        assert_eq!(
            fixture
                .app
                .clone()
                .oneshot(request(&fixture, change, false))
                .await
                .unwrap()
                .status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert!(fixture
            .state
            .read()
            .await
            .document_edits
            .get("proposal")
            .is_some());
        fixture
            .service
            .revalidate_apply(&fixture.run, &fixture.call, &witness)
            .await
            .unwrap();
        let saved = load_control_plane_snapshot(&fixture.store)
            .await
            .unwrap()
            .unwrap();
        assert!(saved.document_edits.get("proposal").is_some());
    }
}

#[tokio::test]
async fn uncertain_policy_save_retires_authority_after_the_real_acknowledgement() {
    let fixture = fixture().await;
    let witness = fixture
        .service
        .capture_apply(&fixture.run, &fixture.call)
        .await
        .unwrap()
        .unwrap();
    let gate = fixture
        .state
        .write()
        .await
        .install_test_control_plane_uncertain_save_gate();
    let operation = tokio::spawn(
        fixture
            .app
            .clone()
            .oneshot(request(&fixture, "policy", false)),
    );
    gate.entered.acquire().await.unwrap().forget();
    assert!(
        fixture
            .state
            .read()
            .await
            .document_edits
            .get("proposal")
            .is_some(),
        "private candidate is not public before acknowledgement"
    );
    gate.release.add_permits(1);
    assert_eq!(
        operation.await.unwrap().unwrap().status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert!(fixture
        .state
        .read()
        .await
        .document_edits
        .get("proposal")
        .is_none());
    assert!(fixture
        .service
        .revalidate_apply(&fixture.run, &fixture.call, &witness)
        .await
        .is_err());
    let saved = load_control_plane_snapshot(&fixture.store)
        .await
        .unwrap()
        .unwrap();
    assert!(saved.document_edits.get("proposal").is_none());
}
