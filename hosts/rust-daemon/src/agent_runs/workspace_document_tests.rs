//! Workspace document evidence through the real coordinator and tool dispatcher.
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use anima_core::{
    document_content::{DocumentFormat, ExtractedDocumentText, MAX_DOCUMENT_RENDERED_BYTES},
    AgentConfigUpdate, CancelSignal, DataValue, MessageRole, ToolCall,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::sync::{RwLock, Semaphore};

use super::{
    test_support::{chat_request, lead_config, ScriptedModel, Step},
    AgentRunCoordinator, RunRoom,
};
use crate::{
    attachments::documents::worker::{DocumentExtractor, WorkerFailure},
    control_plane_store::{load_control_plane_snapshot, ControlPlaneStoreConfig, WorkspaceConfig},
    runs::{ActionStatus, RunSource, RunStatus},
    state::DaemonState,
};

const XLSX: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/documents/m66-selected.xlsx"
));
const RELATIVE: &str = "reports/ledger.xlsx";

fn read_call(id: &str) -> ToolCall {
    ToolCall {
        id: id.into(),
        name: "read_document".into(),
        args: BTreeMap::from([("file_path".into(), DataValue::String(RELATIVE.into()))]),
    }
}

struct Fixture {
    private: tempfile::TempDir,
    root: PathBuf,
    store: ControlPlaneStoreConfig,
    runs: AgentRunCoordinator,
    agent: String,
}

impl Fixture {
    fn new(model: Arc<ScriptedModel>, parser: Arc<dyn DocumentExtractor>, granted: bool) -> Self {
        let private = tempfile::tempdir().unwrap();
        let root = private.path().join("workspace");
        let file = root.join(RELATIVE);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, XLSX).unwrap();
        let store = ControlPlaneStoreConfig::Json(private.path().join("control-plane.json"));
        let mut state = DaemonState::with_model_adapter(model);
        state.workspace = Some(WorkspaceConfig {
            root_path: root.clone(),
            company_name: "M67 synthetic document fixture".into(),
            mission: "Read an existing workbook without uploading it".into(),
            values: vec![],
        });
        state.set_control_plane_store(Some(store.clone()));
        let mut config = lead_config("M67 document companion");
        config.tools = Some(
            state
                .tool_registry
                .resolve_descriptors(if granted {
                    vec!["read_document"]
                } else {
                    vec![]
                })
                .expect("ordinary creation resolves the registered document catalog"),
        );
        config.settings.as_mut().unwrap().max_retries = Some(0);
        let agent = state.create_agent(config).unwrap().state.id;
        let runs =
            AgentRunCoordinator::new(Arc::new(RwLock::new(state)), Arc::new(Semaphore::new(2)))
                .with_document_extractor(parser);
        Self {
            private,
            root,
            store,
            runs,
            agent,
        }
    }
}

#[derive(Default)]
struct RecordingParser(Mutex<Vec<(DocumentFormat, Vec<u8>)>>);

#[async_trait]
impl DocumentExtractor for RecordingParser {
    async fn extract(
        &self,
        format: DocumentFormat,
        bytes: Vec<u8>,
        cancel: CancelSignal,
        _deadline: Option<tokio::time::Instant>,
    ) -> Result<ExtractedDocumentText, WorkerFailure> {
        if cancel.is_cancelled() {
            return Err(WorkerFailure::Cancelled);
        }
        self.0.lock().unwrap().push((format, bytes.clone()));
        Ok(anima_document_content::extract_document(format, &bytes))
    }
}

#[tokio::test]
async fn m67_workspace_xlsx_tool_reaches_next_model_request_with_source_and_coverage() {
    let relative = RELATIVE;
    let expected_hash = format!("{:x}", Sha256::digest(XLSX));
    let model = ScriptedModel::new(vec![
        Step::Tools(vec![read_call("m67-read-ledger")]),
        Step::Text(vec![
            "reports/ledger.xlsx#sheet=1;row=2;column=1 stores 137.42; column=2 contains an unevaluated formula with a possibly stale cached value of 139.42.",
        ]),
    ]);
    let parser = Arc::new(RecordingParser::default());
    let fixture = Fixture::new(model.clone(), parser.clone(), true);
    let runs = &fixture.runs;
    let agent = &fixture.agent;
    let file = fixture.root.join(relative);
    let outcome = runs
        .run(chat_request(
            agent,
            "chat:m67-documents",
            "Read reports/ledger.xlsx and cite the stored number and formula cache.",
        ))
        .await
        .unwrap();

    // Extraction is read-only and must never
    // rewrite the copied workbook, upload it or require a provider process.
    assert_eq!(std::fs::read(&file).unwrap(), XLSX);
    let requests = model.requests();
    assert_eq!(
        requests.len(),
        2,
        "one tool turn and one final scripted turn"
    );
    let tool = requests[1]
        .messages
        .iter()
        .find(|message| message.role == MessageRole::Tool)
        .expect("the actual next model request contains a paired tool result");
    let envelope: Value = serde_json::from_str(&tool.content.text).unwrap();
    assert_eq!(
        envelope["kind"], "workspace_document",
        "the real tool must return document data, not the absent-handler refusal: {}",
        tool.content.text
    );
    assert_eq!(envelope["trust"], "untrusted_data");
    assert_eq!(
        envelope["source"],
        json!({"path": relative, "sha256": expected_hash})
    );
    assert!(tool.content.text.len() <= MAX_DOCUMENT_RENDERED_BYTES);
    assert!(tool.content.attachments.as_ref().is_none_or(Vec::is_empty));
    assert!(!tool.content.metadata.as_ref().is_some_and(|metadata| {
        metadata.contains_key(anima_core::document_content::DOCUMENT_EXTRACTS_METADATA_KEY)
    }));

    let extraction = &envelope["extraction"];
    assert_eq!(extraction["version"], 2);
    assert_eq!(extraction["format"], "xlsx");
    assert_eq!(extraction["status"], "partial");
    assert_eq!(extraction["omissions"], json!(["stored_cells_only"]));
    let blocks = extraction["blocks"].as_array().unwrap();
    assert_eq!(blocks.len(), 5);
    assert_eq!(blocks[0]["text"], "M66 selected ledger");
    assert_eq!(blocks[1]["text"], "137.42");
    assert_eq!(
        blocks[2],
        json!({
            "locator": {"kind":"xlsx_cell", "sheet":1, "row":2, "column":2},
            "text":"formula (not evaluated)=SUM(A2,2); cached (possibly stale)=139.42"
        })
    );
    assert_eq!(
        blocks[4],
        json!({
            "locator": {"kind":"xlsx_cell", "sheet":2, "row":1, "column":1},
            "text":"Hidden source marker: cobalt 7319."
        })
    );
    let coverage = &extraction["xlsx_coverage"];
    assert_eq!(coverage["sheet_count"], 2);
    assert_eq!(coverage["observed_cell_count"], 5);
    assert_eq!(coverage["included_cell_count"], 5);
    assert_eq!(coverage["sheets"][0]["name"], "Ledger");
    assert_eq!(coverage["sheets"][0]["visibility"], "visible");
    assert_eq!(coverage["sheets"][1]["name"], "Private notes");
    assert_eq!(coverage["sheets"][1]["visibility"], "hidden");
    assert_eq!(
        *parser.0.lock().unwrap(),
        [(DocumentFormat::Xlsx, XLSX.to_vec())]
    );

    // This answer is scripted; the independently checked evidence above proves
    // fixture plumbing, not model reasoning or general document answer quality.
    let completed = {
        let state = runs.state.read().await;
        let records = state.runs.for_session(agent, "chat:m67-documents");
        assert_eq!(records.len(), 1);
        (*records[0]).clone()
    };
    assert_eq!(completed.status, RunStatus::Completed);
    assert!(outcome
        .result
        .data
        .as_ref()
        .unwrap()
        .text
        .contains("reports/ledger.xlsx#sheet=1;row=2;column=1 stores 137.42"));
    let saved = load_control_plane_snapshot(&fixture.store)
        .await
        .unwrap()
        .unwrap();
    let saved_run = saved
        .runs
        .iter()
        .find(|run| run.id == completed.id)
        .unwrap()
        .clone();
    assert_eq!(saved_run.action_receipts.len(), 1);
    assert_eq!(saved_run.action_receipts[0].name, "read_document");
    assert_eq!(saved_run.action_receipts[0].status, ActionStatus::Succeeded);
    let original_messages = runs.state.read().await.agents[agent].messages().to_vec();
    let retained = fixture.private.path().join("retained-ledger.xlsx");
    std::fs::rename(&file, &retained).unwrap();
    let restored_model = ScriptedModel::new(vec![Step::Text(vec![
        "The saved result remains historical evidence.",
    ])]);
    let restored_parser = Arc::new(RecordingParser::default());
    let mut restored = DaemonState::with_model_adapter(restored_model.clone());
    restored.restore_control_plane_snapshot(saved).unwrap();
    restored.set_control_plane_store(Some(fixture.store.clone()));
    let restored =
        AgentRunCoordinator::new(Arc::new(RwLock::new(restored)), Arc::new(Semaphore::new(2)))
            .with_document_extractor(restored_parser.clone());
    {
        let state = restored.state.read().await;
        assert_eq!(state.agents[agent].messages(), original_messages);
        assert_eq!(
            state.runs.get(&completed.id).unwrap().action_receipts,
            saved_run.action_receipts
        );
    }
    assert!(
        restored_model.requests().is_empty(),
        "quiet restore starts no model work"
    );
    assert!(
        restored_parser.0.lock().unwrap().is_empty(),
        "quiet restore starts no extraction"
    );
    restored
        .run(chat_request(
            agent,
            "chat:m67-documents",
            "Keep the previous document result as historical evidence; do not reopen it.",
        ))
        .await
        .unwrap();
    let continued = restored_model.requests();
    assert_eq!(continued.len(), 1, "the new turn asks for no tool");
    let historical = continued[0]
        .messages
        .iter()
        .find(|message| message.role == MessageRole::Tool)
        .unwrap();
    assert_eq!(historical.content.text, tool.content.text);
    assert!(
        restored_parser.0.lock().unwrap().is_empty(),
        "history is reused without reparsing a missing source"
    );
    assert!(!file.exists());
    assert_eq!(std::fs::read(retained).unwrap(), XLSX);
}

#[tokio::test]
async fn m67_missing_or_revoked_document_grant_never_reaches_extractor() {
    for revoked in [false, true] {
        let model = ScriptedModel::new(vec![
            Step::Tools(vec![read_call("m67-refused-read")]),
            Step::Text(vec!["Document access is unavailable."]),
        ]);
        let parser = Arc::new(RecordingParser::default());
        let fixture = Fixture::new(model.clone(), parser.clone(), revoked);
        if revoked {
            fixture
                .runs
                .state
                .write()
                .await
                .update_agent(
                    &fixture.agent,
                    AgentConfigUpdate {
                        tools: Some(vec![]),
                        ..AgentConfigUpdate::default()
                    },
                )
                .unwrap();
        }
        fixture
            .runs
            .run(chat_request(
                &fixture.agent,
                "chat:m67-denied",
                "Read the workbook only if allowed.",
            ))
            .await
            .unwrap();
        assert!(parser.0.lock().unwrap().is_empty());
        let requests = model.requests();
        assert_eq!(requests.len(), 2);
        let refusal = requests[1]
            .messages
            .iter()
            .find(|message| message.role == MessageRole::Tool)
            .unwrap();
        let denied: Value = serde_json::from_str(&refusal.content.text).unwrap();
        assert_eq!(denied["status"], "error");
        assert_eq!(
            denied["error"],
            "This tool is not configured for this agent. Continue only with configured tools, or explain the limitation."
        );
        assert_eq!(
            refusal.content.metadata.as_ref().unwrap().get("toolCallId"),
            Some(&DataValue::String("m67-refused-read".into()))
        );
        assert!(!refusal.content.text.contains("M66 selected ledger"));
        assert!(!refusal.content.text.contains("workspace_document"));
        let state = fixture.runs.state.read().await;
        assert!(state
            .runs
            .for_agent(&fixture.agent)
            .iter()
            .all(|run| run.action_receipts.is_empty()));
        assert_eq!(std::fs::read(fixture.root.join(RELATIVE)).unwrap(), XLSX);
    }
}

struct ActiveExtraction(Arc<AtomicUsize>);
impl Drop for ActiveExtraction {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

struct CancellableParser {
    entered: Semaphore,
    calls: AtomicUsize,
    active: Arc<AtomicUsize>,
}

#[async_trait]
impl DocumentExtractor for CancellableParser {
    async fn extract(
        &self,
        format: DocumentFormat,
        bytes: Vec<u8>,
        cancel: CancelSignal,
        _deadline: Option<tokio::time::Instant>,
    ) -> Result<ExtractedDocumentText, WorkerFailure> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.active.fetch_add(1, Ordering::SeqCst);
        let _active = ActiveExtraction(Arc::clone(&self.active));
        self.entered.add_permits(1);
        cancel.cancelled().await;
        // A late successful result must be fenced even if a transport returns
        // valid data after Stop. The real physical-owner suite remains reused.
        Ok(anima_document_content::extract_document(format, &bytes))
    }
}

#[tokio::test]
async fn m67_stop_during_document_extraction_cannot_publish_late_cells_or_continue_model() {
    let model = ScriptedModel::new(vec![
        Step::Tools(vec![read_call("m67-stop-read")]),
        Step::Text(vec!["This continuation must not be requested."]),
    ]);
    let parser = Arc::new(CancellableParser {
        entered: Semaphore::new(0),
        calls: AtomicUsize::new(0),
        active: Arc::new(AtomicUsize::new(0)),
    });
    let fixture = Fixture::new(model.clone(), parser.clone(), true);
    let task = {
        let runs = fixture.runs.clone();
        let agent = fixture.agent.clone();
        tokio::spawn(async move {
            runs.run(chat_request(
                &agent,
                "chat:m67-stop",
                "Read the existing workbook.",
            ))
            .await
        })
    };
    tokio::time::timeout(Duration::from_secs(5), parser.entered.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    let run_id = fixture
        .runs
        .state
        .read()
        .await
        .runs
        .for_agent(&fixture.agent)[0]
        .id
        .clone();
    fixture
        .runs
        .stop_run(&fixture.agent, &run_id)
        .await
        .unwrap();
    let outcome = tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(outcome.result.error.as_deref(), Some("stopped"));
    assert_eq!(parser.calls.load(Ordering::SeqCst), 1);
    assert_eq!(parser.active.load(Ordering::SeqCst), 0);
    assert_eq!(model.requests().len(), 1);
    let state = fixture.runs.state.read().await;
    assert_eq!(
        state.runs.get(&run_id).unwrap().status,
        RunStatus::Cancelled
    );
    assert!(state.agents[&fixture.agent]
        .messages()
        .iter()
        .all(|message| !message.content.text.contains("M66 selected ledger")));
    assert!(state
        .runs
        .get(&run_id)
        .unwrap()
        .action_receipts
        .iter()
        .all(|receipt| receipt.status != ActionStatus::Succeeded));
    assert_eq!(std::fs::read(fixture.root.join(RELATIVE)).unwrap(), XLSX);
}

#[tokio::test]
async fn m67_granted_connector_origin_and_inherited_helper_use_existing_filesystem_authority() {
    let model = ScriptedModel::new(vec![
        Step::Tools(vec![read_call("m67-connector-read")]),
        Step::Text(vec!["Connector run inspected the stored workbook."]),
        Step::Tools(vec![read_call("m67-helper-read")]),
        Step::Text(vec!["Helper inspected the stored workbook."]),
    ]);
    let parser = Arc::new(RecordingParser::default());
    let fixture = Fixture::new(model.clone(), parser.clone(), true);
    let mut connector = chat_request(
        &fixture.agent,
        "connector:m67",
        "Read the existing workbook.",
    );
    connector.source = RunSource::Telegram;
    connector.source_ref = Some("m67-synthetic-connector-origin".into());
    fixture.runs.run(connector).await.unwrap();
    {
        let state = fixture.runs.state.read().await;
        let records = state.runs.for_session(&fixture.agent, "connector:m67");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].status, RunStatus::Completed);
    }
    let helper = {
        let mut state = fixture.runs.state.write().await;
        let parent = state.get_agent(&fixture.agent).unwrap().state;
        state
            .create_agent(super::helper_config(&parent, "M67 document helper".into()))
            .unwrap()
            .state
            .id
    };
    let mut delegated = chat_request(&helper, "helper:m67", "Read the existing workbook.");
    delegated.room = RunRoom::Delegated {
        parent_id: fixture.agent.clone(),
    };
    delegated.source = RunSource::Delegation;
    fixture.runs.run(delegated).await.unwrap();
    {
        let state = fixture.runs.state.read().await;
        let records = state.runs.for_agent(&helper);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].status, RunStatus::Completed);
    }
    let requests = model.requests();
    assert_eq!(requests.len(), 4);
    for request in [&requests[1], &requests[3]] {
        let tool = request
            .messages
            .iter()
            .find(|message| message.role == MessageRole::Tool)
            .unwrap();
        let evidence: Value = serde_json::from_str(&tool.content.text).unwrap();
        assert_eq!(evidence["kind"], "workspace_document");
        assert_eq!(evidence["source"]["path"], RELATIVE);
        assert_eq!(evidence["extraction"]["blocks"][1]["text"], "137.42");
    }
    assert_eq!(parser.0.lock().unwrap().len(), 2);
    assert_eq!(std::fs::read(fixture.root.join(RELATIVE)).unwrap(), XLSX);
}
