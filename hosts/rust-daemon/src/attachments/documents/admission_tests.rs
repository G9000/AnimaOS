use super::*;
use crate::{
    attachments::{files::save_upload, AttachmentRecord},
    runs::{RunRecord, RunStart},
    sessions::{SessionRecord, TitleSource},
};
use anima_core::document_content::{
    document_rendered_bytes, parse_document_snapshots, DocumentBlock,
};
use async_trait::async_trait;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

const DOCX: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/documents/m54-selected.docx"
));
struct Temp(PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
struct Extractor {
    calls: AtomicUsize,
    output: ExtractedDocumentText,
    change: Option<PathBuf>,
    cancel: bool,
}
#[async_trait]
impl DocumentExtractor for Extractor {
    async fn extract(
        &self,
        _bytes: Vec<u8>,
        cancel: CancelSignal,
        _deadline: Option<tokio::time::Instant>,
    ) -> Result<ExtractedDocumentText, WorkerFailure> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(path) = &self.change {
            std::fs::write(path, b"changed").unwrap();
        }
        if self.cancel {
            cancel.cancel();
        }
        Ok(self.output.clone())
    }
}
fn output() -> ExtractedDocumentText {
    ExtractedDocumentText {
        version: 1,
        status: Status::Ready,
        blocks: vec![DocumentBlock {
            paragraph: 1,
            text: "bounded selected content".into(),
        }],
        omissions: vec![DocumentOmission::MainBodyOnly],
    }
}
fn extractor() -> Arc<Extractor> {
    Arc::new(Extractor {
        calls: AtomicUsize::new(0),
        output: output(),
        change: None,
        cancel: false,
    })
}
async fn fixture() -> (
    Temp,
    SharedDaemonState,
    String,
    String,
    AgentConfig,
    AttachmentRecord,
) {
    let temp = Temp(std::env::temp_dir().join(format!("anima-doc-admit-{}", uuid::Uuid::new_v4())));
    std::fs::create_dir_all(&temp.0).unwrap();
    let mut state = crate::state::DaemonState::new();
    state.workspace = Some(crate::control_plane_store::WorkspaceConfig {
        root_path: temp.0.clone(),
        ..Default::default()
    });
    let config: AgentConfig = serde_json::from_value(
        serde_json::json!({"name":"doc admission","model":"test","provider":"test"}),
    )
    .unwrap();
    let agent = state.create_agent(config.clone()).unwrap().state.id;
    state.sessions.insert(SessionRecord::new(
        &agent,
        "chat:docs",
        SessionKind::Chat,
        SessionOrigin::Web,
        "Docs".into(),
        TitleSource::FirstMessage,
        1,
    ));
    let run = RunRecord::running(
        RunStart {
            agent_id: agent.clone(),
            session_id: "chat:docs".into(),
            source: RunSource::Web,
            source_ref: None,
            idempotency_key: None,
            text: "read".into(),
            model: "test".into(),
            provider: Some("test".into()),
            parent_run_id: None,
        },
        1,
    );
    let run_id = run.id.clone();
    state.runs.insert(run);
    let saved = save_upload(&temp.0, "2026-10-16", "selected.docx", DOCX).unwrap();
    let row = AttachmentRecord {
        id: "att-doc".into(),
        agent_id: agent.clone(),
        session_id: "chat:docs".into(),
        name: "selected.docx".into(),
        mime: super::super::DOCX_MIME.into(),
        size: DOCX.len() as u64,
        kind: AttachmentKind::Document,
        path: saved.path,
        created_at_ms: 1,
        workspace_root: saved.workspace_root,
        sha256: saved.sha256,
    };
    state
        .history
        .store()
        .upsert_attachments(&[row.clone()])
        .await
        .unwrap();
    (
        temp,
        Arc::new(tokio::sync::RwLock::new(state)),
        agent,
        run_id,
        config,
        row,
    )
}
async fn run(
    state: &SharedDaemonState,
    agent: &str,
    id: &str,
    config: &AgentConfig,
    input: &mut Content,
    extractor: Arc<dyn DocumentExtractor>,
    cancel: CancelSignal,
) {
    admit(
        state,
        agent,
        "chat:docs",
        id,
        RunSource::Web,
        config,
        input,
        extractor,
        cancel,
        None,
    )
    .await;
}

#[tokio::test]
async fn admission_scope_failure_and_changed_source_never_publish_text() {
    let (temp, state, agent, id, config, row) = fixture().await;
    let extraction = extractor();
    let mut input =
        crate::agent_runs::attachments::attachment_content("original", &[row.clone()], None);
    admit(
        &state,
        &agent,
        "chat:docs",
        &id,
        RunSource::Api,
        &config,
        &mut input,
        extraction.clone(),
        CancelSignal::new(),
        None,
    )
    .await;
    assert!(parse_document_snapshots(&input).unwrap().is_empty());
    let mut foreign = row.clone();
    foreign.id = "att-doc-foreign".into();
    foreign.session_id = "chat:foreign".into();
    let store = state.read().await.history.store();
    assert!(
        store
            .upsert_attachments(&[AttachmentRecord {
                session_id: "chat:foreign".into(),
                ..row.clone()
            }])
            .await
            .is_err(),
        "an existing ID must remain immutable"
    );
    store.upsert_attachments(&[foreign.clone()]).await.unwrap();
    input = crate::agent_runs::attachments::attachment_content("original", &[foreign], None);
    run(
        &state,
        &agent,
        &id,
        &config,
        &mut input,
        extraction.clone(),
        CancelSignal::new(),
    )
    .await;
    assert_eq!(extraction.calls.load(Ordering::SeqCst), 0);
    input = crate::agent_runs::attachments::attachment_content("original", &[row.clone()], None);
    let changed = Arc::new(Extractor {
        calls: AtomicUsize::new(0),
        output: output(),
        change: Some(temp.0.join(&row.path)),
        cancel: false,
    });
    run(
        &state,
        &agent,
        &id,
        &config,
        &mut input,
        changed,
        CancelSignal::new(),
    )
    .await;
    assert_eq!(
        parse_document_snapshots(&input).unwrap()[0]
            .extraction
            .status,
        Status::Unavailable
    );
    assert!(!input.text.contains("bounded selected content"));
}

#[tokio::test]
async fn admission_rejects_malformed_worker_output_and_late_stop_text() {
    let (_temp, state, agent, id, config, row) = fixture().await;
    let mut input = crate::agent_runs::attachments::attachment_content("original", &[row], None);
    let original = input.text.clone();
    let mut malformed = output();
    malformed.blocks[0].paragraph = 0;
    let bad = Arc::new(Extractor {
        calls: AtomicUsize::new(0),
        output: malformed,
        change: None,
        cancel: false,
    });
    run(
        &state,
        &agent,
        &id,
        &config,
        &mut input,
        bad,
        CancelSignal::new(),
    )
    .await;
    assert_eq!(
        parse_document_snapshots(&input).unwrap()[0]
            .extraction
            .status,
        Status::Unavailable
    );
    let stopped = Arc::new(Extractor {
        calls: AtomicUsize::new(0),
        output: output(),
        change: None,
        cancel: true,
    });
    run(
        &state,
        &agent,
        &id,
        &config,
        &mut input,
        stopped,
        CancelSignal::new(),
    )
    .await;
    let snapshot = &parse_document_snapshots(&input).unwrap()[0];
    assert_eq!(snapshot.extraction.status, Status::Cancelled);
    assert!(snapshot.extraction.blocks.is_empty());
    assert_eq!(input.text, original);
}

#[tokio::test]
async fn admission_context_fit_respects_fractional_calibration_and_selection_limit() {
    let (temp, state, agent, id, mut config, row) = fixture().await;
    let mut rows = vec![row.clone()];
    for i in 1..3 {
        let mut other = row.clone();
        other.id = format!("att-doc-{i}");
        let saved = save_upload(&temp.0, "2026-10-16", "selected.docx", DOCX).unwrap();
        other.path = saved.path;
        other.name = std::path::Path::new(&other.path)
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .into();
        rows.push(other);
    }
    let store = state.read().await.history.store();
    store.upsert_attachments(&rows).await.unwrap();
    state
        .write()
        .await
        .sessions
        .get_mut(&agent, "chat:docs")
        .unwrap()
        .context_calibration_permille = Some(500);
    let settings = config.settings.get_or_insert_with(Default::default);
    settings.additional.insert(
        "contextBudgetTokens".into(),
        anima_core::DataValue::Number(100.0),
    );
    let mut input = crate::agent_runs::attachments::attachment_content("original", &rows, None);
    let extraction = extractor();
    run(
        &state,
        &agent,
        &id,
        &config,
        &mut input,
        extraction.clone(),
        CancelSignal::new(),
    )
    .await;
    let snapshots = parse_document_snapshots(&input).unwrap();
    assert_eq!(extraction.calls.load(Ordering::SeqCst), 2);
    assert_eq!(snapshots.len(), 3);
    assert!(snapshots.iter().all(|s| s.extraction.blocks.is_empty()));
    assert!(snapshots[2]
        .extraction
        .omissions
        .contains(&DocumentOmission::SelectionLimit));
    assert_eq!(document_rendered_bytes(&input), 0);
    assert!(TokenEstimator::new(0.5).content_tokens(&input) < 100);
}

#[tokio::test]
async fn document_reservation_reduces_agent_file_budget_and_current_context_report() {
    let (_temp, state, agent, id, mut config, row) = fixture().await;
    config
        .settings
        .get_or_insert_with(Default::default)
        .additional
        .insert(
            "contextBudgetTokens".into(),
            anima_core::DataValue::Number(2_000.0),
        );
    let mut input = crate::agent_runs::attachments::attachment_content("original", &[row], None);
    run(
        &state,
        &agent,
        &id,
        &config,
        &mut input,
        extractor(),
        CancelSignal::new(),
    )
    .await;
    assert!(document_rendered_bytes(&input) > 0);
    let estimator = TokenEstimator::default();
    let available = crate::sessions::context::ContextBudget::for_config(&config)
        .history_tokens(estimator.content_tokens(&input));
    let observed = Arc::new(AtomicUsize::new(usize::MAX));
    let captured = observed.clone();
    let (build, _) = crate::agent_files::runtime::admit_run_with_loader(
        &state,
        Arc::new(tokio::sync::Mutex::new(())),
        &agent,
        "chat:docs",
        "chat:docs",
        &id,
        &input,
        &config,
        CancelSignal::new(),
        None,
        move |_, budget, _, _| {
            captured.store(budget.max_total_chars, Ordering::SeqCst);
            Box::pin(async {
                crate::agent_files::AcceptedAgentFiles::empty(
                    crate::agent_files::Availability::Available,
                )
            })
        },
    )
    .await
    .unwrap();
    assert_eq!(
        observed.load(Ordering::SeqCst),
        (available * anima_core::CHARS_PER_TOKEN) as usize
    );
    assert_eq!(
        build.context.raw_estimate_tokens,
        estimator.content_tokens(&input)
    );
}

/// A finite blocking fake proves both read checkpoints release the async run
/// before the physical read returns, while its lease stays owned by the task.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admission_stop_and_absolute_deadline_cover_first_and_postparse_reads() {
    for mode in ["stop", "deadline"] {
        for blocking_call in [1, 2] {
            let (_temp, state, agent, id, config, row) = fixture().await;
            let mut input =
                crate::agent_runs::attachments::attachment_content("original", &[row], None);
            let (entered, observed) = tokio::sync::oneshot::channel();
            let entered = std::sync::Mutex::new(Some(entered));
            let (release, blocked) = std::sync::mpsc::channel();
            let blocked = std::sync::Mutex::new(blocked);
            let calls = AtomicUsize::new(0);
            let reader: SourceReader = Arc::new(move |_, _| {
                if calls.fetch_add(1, Ordering::SeqCst) + 1 == blocking_call {
                    entered.lock().unwrap().take().unwrap().send(()).unwrap();
                    let _ = blocked
                        .lock()
                        .unwrap()
                        .recv_timeout(std::time::Duration::from_secs(2));
                }
                Ok(DOCX.to_vec())
            });
            let cancel = CancelSignal::new();
            let task_cancel = cancel.clone();
            let deadline = (mode == "deadline")
                .then(|| tokio::time::Instant::now() + std::time::Duration::from_millis(500));
            let task = tokio::spawn(async move {
                admit_with_reader(
                    &state,
                    &agent,
                    "chat:docs",
                    &id,
                    RunSource::Web,
                    &config,
                    &mut input,
                    extractor(),
                    task_cancel,
                    deadline,
                    reader,
                )
                .await;
                input
            });
            tokio::time::timeout(std::time::Duration::from_secs(1), observed)
                .await
                .unwrap()
                .unwrap();
            if mode == "stop" {
                cancel.cancel();
            }
            let completed = tokio::time::timeout(std::time::Duration::from_secs(1), task).await;
            let _ = release.send(());
            let input = completed
                .expect("admission waited for a blocked physical read")
                .unwrap();
            let snapshots = parse_document_snapshots(&input).unwrap();
            assert_eq!(snapshots.len(), 1);
            assert!(snapshots[0].extraction.blocks.is_empty());
            assert_eq!(
                snapshots[0].extraction.status,
                if mode == "stop" {
                    Status::Cancelled
                } else {
                    Status::Unavailable
                }
            );
            assert!(!input.text.contains("bounded selected content"));
        }
    }
}
