use super::*;
use crate::agent_runs::test_support::{ledger_run, ScriptedModel};
use crate::approvals::{
    ApprovalDecisionKind, ApprovalRequest, ApprovalResolution, ApprovalStatus,
    PendingApprovalStart, ResolvedBy,
};
use crate::control_plane_store::{ControlPlaneStoreConfig, WorkspaceConfig};
use anima_core::{AgentConfig, DataValue, ToolDescriptor};
use anima_service_adapters::{GeneratedImage, ImageError, ImagePort, ImageRequest};
use async_trait::async_trait;
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicUsize, Ordering},
    time::Instant,
};
use tokio::sync::{RwLock, Semaphore};

struct Port {
    calls: AtomicUsize,
    fail: bool,
}

struct HeldPort {
    started: tokio::sync::Notify,
    release: tokio::sync::Notify,
}

#[async_trait]
impl ImagePort for HeldPort {
    async fn generate(
        &self,
        _request: ImageRequest,
        _cancel: CancelSignal,
        _deadline: Instant,
    ) -> Result<GeneratedImage, ImageError> {
        self.started.notify_one();
        self.release.notified().await;
        GeneratedImage::new(png(), None)
    }
}
#[async_trait]
impl ImagePort for Port {
    async fn generate(
        &self,
        _request: ImageRequest,
        _cancel: CancelSignal,
        _deadline: Instant,
    ) -> Result<GeneratedImage, ImageError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.fail {
            Err(ImageError::OutcomeUnknown)
        } else {
            GeneratedImage::new(png(), None)
        }
    }
}
fn png() -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 1024, 1024);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&vec![255; 1024 * 1024 * 4])
            .unwrap();
    }
    bytes
}
fn call(id: &str, name: &str) -> ToolCall {
    ToolCall {
        id: id.into(),
        name: name.into(),
        args: BTreeMap::from([("prompt".into(), DataValue::String("A red kite".into()))]),
    }
}
fn provider(port: &Arc<Port>) -> crate::images::ImageProvider {
    crate::images::ImageProvider {
        port: port.clone(),
        key_id: "test".into(),
    }
}
async fn approval(runs: &AgentRunCoordinator, witness: &ImageWitness) -> ImageOwnerApproval {
    let projected = witness.approval_call();
    let mut record = ApprovalRequest::pending(
        PendingApprovalStart {
            agent_id: &witness.run.agent_id,
            session_id: &witness.run.session_id,
            run_id: &witness.run.run_id,
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
    let proof = witness.resolve(&record).unwrap();
    runs.state.write().await.approvals.insert(record);
    let journal = super::super::action_receipts::RunActionJournal::new(
        &runs.state,
        runs.control_plane_transactions(),
        witness.run.clone(),
        CancelSignal::new(),
        None,
    );
    journal.begin(&witness.call).await.unwrap();
    proof
}
async fn fixture() -> (
    tempfile::TempDir,
    AgentRunCoordinator,
    RunLink,
    tokio::sync::OwnedMutexGuard<()>,
) {
    static SERIAL: std::sync::OnceLock<Arc<tokio::sync::Mutex<()>>> = std::sync::OnceLock::new();
    let serial = Arc::clone(SERIAL.get_or_init(|| Arc::new(tokio::sync::Mutex::new(()))))
        .lock_owned()
        .await;
    let temp = tempfile::tempdir().unwrap();
    let mut state = crate::state::DaemonState::with_model_adapter(ScriptedModel::new(vec![]));
    state.workspace = Some(WorkspaceConfig {
        root_path: temp.path().into(),
        company_name: "Test".into(),
        mission: "Test".into(),
        values: vec![],
    });
    state.set_control_plane_store(Some(ControlPlaneStoreConfig::Json(
        temp.path().join("state.json"),
    )));
    state.provider_keys = crate::provider_keys::KeyPool::open(
        None,
        &BTreeMap::from([("openai".into(), "synthetic-image-key".into())]),
    )
    .unwrap();
    let config: AgentConfig =
        serde_json::from_value(serde_json::json!({"name":"Image owner","model":"deterministic"}))
            .unwrap();
    let mut config = config;
    config.tools = Some(
        ["generate_image", "edit_image"]
            .into_iter()
            .map(|name| ToolDescriptor {
                name: name.into(),
                description: "Test image tool".into(),
                parameters_schema: BTreeMap::new(),
                examples: None,
            })
            .collect(),
    );
    let agent = state.create_agent(config).unwrap().state.id;
    let runs = AgentRunCoordinator::new(Arc::new(RwLock::new(state)), Arc::new(Semaphore::new(8)));
    let link = ledger_run(&runs, &agent, "chat:images").await;
    runs.state.read().await.live.runs().register(&link.run_id);
    (temp, runs, link, serial)
}

#[tokio::test]
async fn image_create_then_selected_edit_preserves_both_versions_and_deduplicates_dispatch() {
    let (_temp, runs, link, _serial) = fixture().await;
    let port = Arc::new(Port {
        calls: AtomicUsize::new(0),
        fail: false,
    });
    let witness = runs
        .capture_image(&link, &call("create", "generate_image"))
        .await
        .unwrap();
    let proof = approval(&runs, &witness).await;
    let first = runs
        .execute_image(proof.clone(), CancelSignal::new(), provider(&port))
        .await
        .unwrap();
    assert!(runs
        .execute_image(proof, CancelSignal::new(), provider(&port))
        .await
        .is_err());
    assert_eq!(port.calls.load(Ordering::SeqCst), 1);
    assert_eq!(first["observation"]["state"], "bytes_matched");
    assert_eq!(first["observation"]["limit"], "saved_bytes_only");
    assert_eq!(first["observation"]["operationId"], first["id"]);
    assert_eq!(first["observation"]["versionId"], first["version"]["id"]);
    assert_eq!(
        first["observation"]["attachmentId"],
        first["version"]["attachment"]["id"]
    );
    let attachment = first["version"]["attachment"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    runs.state
        .write()
        .await
        .runs
        .get_mut(&link.run_id)
        .unwrap()
        .input
        .attachment_ids = vec![attachment.clone()];
    let witness = runs
        .capture_image(&link, &call("edit", "edit_image"))
        .await
        .unwrap();
    let edited = runs
        .execute_image(
            approval(&runs, &witness).await,
            CancelSignal::new(),
            provider(&port),
        )
        .await
        .unwrap();
    assert_eq!(edited["version"]["parentAttachmentId"], attachment);
    let state = runs.state.read().await;
    let saved = &state
        .sessions
        .get(&link.agent_id, &link.session_id)
        .unwrap()
        .image_operations;
    assert_eq!(saved.len(), 2);
    assert_eq!(saved[0].output.as_ref().unwrap().id, attachment);
    assert_ne!(saved[0].output, saved[1].output);
    assert_eq!(port.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn changed_image_scope_and_missing_selection_do_not_send() {
    let (_temp, runs, link, _serial) = fixture().await;
    assert!(runs
        .capture_image(&link, &call("edit", "edit_image"))
        .await
        .is_err());
    let witness = runs
        .capture_image(&link, &call("create", "generate_image"))
        .await
        .unwrap();
    let proof = approval(&runs, &witness).await;
    runs.state
        .write()
        .await
        .sessions
        .get_mut(&link.agent_id, &link.session_id)
        .unwrap()
        .project_binding_revision += 1;
    let port = Arc::new(Port {
        calls: AtomicUsize::new(0),
        fail: false,
    });
    assert!(runs
        .execute_image(proof, CancelSignal::new(), provider(&port))
        .await
        .is_err());
    assert_eq!(port.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn unknown_image_provider_outcome_is_saved_and_never_replayed() {
    let (_temp, runs, link, _serial) = fixture().await;
    let witness = runs
        .capture_image(&link, &call("lost", "generate_image"))
        .await
        .unwrap();
    let proof = approval(&runs, &witness).await;
    let port = Arc::new(Port {
        calls: AtomicUsize::new(0),
        fail: true,
    });
    assert!(runs
        .execute_image(proof.clone(), CancelSignal::new(), provider(&port))
        .await
        .is_err());
    assert!(runs
        .execute_image(proof, CancelSignal::new(), provider(&port))
        .await
        .is_err());
    let state = runs.state.read().await;
    let operation = &state
        .sessions
        .get(&link.agent_id, &link.session_id)
        .unwrap()
        .image_operations[0];
    assert_eq!(operation.status, crate::images::ImageStatus::OutcomeUnknown);
    assert_eq!(port.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn stopped_image_request_cannot_publish_a_late_provider_result() {
    let (temp, runs, link, _serial) = fixture().await;
    let witness = runs
        .capture_image(&link, &call("stop", "generate_image"))
        .await
        .unwrap();
    let proof = approval(&runs, &witness).await;
    let port = Arc::new(HeldPort {
        started: tokio::sync::Notify::new(),
        release: tokio::sync::Notify::new(),
    });
    let cancel = CancelSignal::new();
    let worker = {
        let runs = runs.clone();
        let port = port.clone();
        let cancel = cancel.clone();
        tokio::spawn(async move {
            runs.execute_image(
                proof,
                cancel,
                crate::images::ImageProvider {
                    port,
                    key_id: "test".into(),
                },
            )
            .await
        })
    };
    port.started.notified().await;
    cancel.cancel();
    port.release.notify_one();
    assert!(worker.await.unwrap().is_err());
    let state = runs.state.read().await;
    let operation = &state
        .sessions
        .get(&link.agent_id, &link.session_id)
        .unwrap()
        .image_operations[0];
    assert_eq!(operation.status, crate::images::ImageStatus::OutcomeUnknown);
    assert!(operation.output.is_none());
    assert!(operation.version.is_none());
    assert!(!temp.path().join("uploads").exists());
}

#[tokio::test]
async fn cleared_image_scope_cannot_publish_a_late_provider_result() {
    let (temp, runs, link, _serial) = fixture().await;
    let witness = runs
        .capture_image(&link, &call("clear", "generate_image"))
        .await
        .unwrap();
    let proof = approval(&runs, &witness).await;
    let port = Arc::new(HeldPort {
        started: tokio::sync::Notify::new(),
        release: tokio::sync::Notify::new(),
    });
    let worker = {
        let runs = runs.clone();
        let port = port.clone();
        tokio::spawn(async move {
            runs.execute_image(
                proof,
                CancelSignal::new(),
                crate::images::ImageProvider {
                    port,
                    key_id: "test".into(),
                },
            )
            .await
        })
    };
    port.started.notified().await;
    {
        let mut state = runs.state.write().await;
        let session = state
            .sessions
            .get_mut(&link.agent_id, &link.session_id)
            .unwrap();
        session.clear_generation += 1;
        session.image_operations.clear();
    }
    port.release.notify_one();
    assert!(worker.await.unwrap().is_err());
    assert!(runs
        .state
        .read()
        .await
        .sessions
        .get(&link.agent_id, &link.session_id)
        .unwrap()
        .image_operations
        .is_empty());
    assert!(!temp.path().join("uploads").exists());
}

#[tokio::test]
async fn image_output_survives_a_full_day_of_friendly_name_collisions() {
    let (temp, runs, link, _serial) = fixture().await;
    let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let directory = temp.path().join("uploads").join(date);
    std::fs::create_dir_all(&directory).unwrap();
    for index in 0..100 {
        let name = if index == 0 {
            "image-1.png".into()
        } else {
            format!("image-1-{index}.png")
        };
        std::fs::write(directory.join(name), b"existing unrelated image").unwrap();
    }
    let witness = runs
        .capture_image(&link, &call("create", "generate_image"))
        .await
        .unwrap();
    let port = Arc::new(Port {
        calls: AtomicUsize::new(0),
        fail: false,
    });
    let result = runs
        .execute_image(
            approval(&runs, &witness).await,
            CancelSignal::new(),
            provider(&port),
        )
        .await
        .unwrap();
    assert_eq!(result["version"]["attachment"]["name"], "image-1.png");
    let row = runs
        .state
        .read()
        .await
        .sessions
        .get(&link.agent_id, &link.session_id)
        .unwrap()
        .image_operations[0]
        .output
        .clone()
        .unwrap();
    assert!(read_attachment_bytes(temp.path(), &row).is_ok());
    assert_eq!(
        std::fs::read(directory.join("image-1.png")).unwrap(),
        b"existing unrelated image"
    );
    assert_eq!(port.calls.load(Ordering::SeqCst), 1);
}
