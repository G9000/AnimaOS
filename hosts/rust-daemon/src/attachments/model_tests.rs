use super::*;
use crate::attachments::{files::save_upload, AttachmentKind, AttachmentRecord};
use crate::history::{HistoryStore, MemoryHistoryStore};
use anima_core::{
    AgentConfig, Attachment, AttachmentType, Content, DataValue, MessageRole, ModelAdapter,
    ModelGenerateRequest, ModelGenerateResponse, ModelStopReason, TokenUsage,
};
use async_trait::async_trait;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

const GIF: &[u8] = b"GIF89a\x01\x00\x01\x00\x00\x00\x00\x2c\x3b";

fn document_record(temp: &Temp, id: &str) -> AttachmentRecord {
    let bytes = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/documents/m54-selected.docx"
    ));
    let name = format!("{id}.docx");
    let saved = save_upload(&temp.0, "2026-10-16", &name, bytes).unwrap();
    AttachmentRecord {
        id: id.into(),
        agent_id: "agent".into(),
        session_id: "chat:session".into(),
        name,
        mime: crate::attachments::documents::DOCX_MIME.into(),
        size: bytes.len() as u64,
        kind: AttachmentKind::Document,
        path: saved.path,
        created_at_ms: 0,
        workspace_root: saved.workspace_root,
        sha256: saved.sha256,
    }
}

fn pdf_record(temp: &Temp, id: &str) -> AttachmentRecord {
    let bytes = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/documents/m55-selected.pdf"
    ));
    let name = format!("{id}.pdf");
    let saved = save_upload(&temp.0, "2026-10-16", &name, bytes).unwrap();
    AttachmentRecord {
        id: id.into(),
        agent_id: "agent".into(),
        session_id: "chat:session".into(),
        name,
        mime: crate::attachments::documents::PDF_MIME.into(),
        size: bytes.len() as u64,
        kind: AttachmentKind::Document,
        path: saved.path,
        created_at_ms: 0,
        workspace_root: saved.workspace_root,
        sha256: saved.sha256,
    }
}

fn document_content(rows: &[AttachmentRecord]) -> Content {
    use anima_core::document_content::*;
    let mut content =
        crate::agent_runs::attachments::attachment_content("original owner bytes", rows, None);
    let snapshots = rows
        .iter()
        .map(|row| DocumentSnapshot {
            attachment_id: row.id.clone(),
            source_sha256: row.sha256.clone(),
            extraction: ExtractedDocumentText {
                version: 2,
                format: crate::attachments::documents::document_format(row).unwrap(),
                status: DocumentExtractionStatus::Ready,
                blocks: vec![DocumentBlock {
                    locator: if row.mime == crate::attachments::documents::PDF_MIME {
                        DocumentLocator::PdfPage { page: 1 }
                    } else {
                        DocumentLocator::DocxParagraph { paragraph: 1 }
                    },
                    text: format!("derived {}", row.id),
                }],
                omissions: vec![if row.mime == crate::attachments::documents::PDF_MIME {
                    DocumentOmission::TextLayerOnly
                } else {
                    DocumentOmission::MainBodyOnly
                }],
                pdf_coverage: (row.mime == crate::attachments::documents::PDF_MIME).then_some(
                    PdfCoverage {
                        page_count: 2,
                        text_page_count: 1,
                        included_page_count: 1,
                    },
                ),
            },
        })
        .collect::<Vec<_>>();
    content.metadata.as_mut().unwrap().insert(
        DOCUMENT_EXTRACTS_METADATA_KEY.into(),
        document_snapshots_metadata(&snapshots).unwrap(),
    );
    content
}

#[tokio::test]
async fn document_snapshots_hydrate_newest_four_only_and_preserve_saved_history() {
    let temp = Temp::new();
    let rows = (0..6)
        .map(|i| document_record(&temp, &format!("doc{i}")))
        .collect::<Vec<_>>();
    let adapter = fixture(&rows, Some(temp.0.clone())).await;
    let mut input = request("agent", "chat:session", document_content(&rows[..2]));
    for pair in rows[2..].chunks(2) {
        let mut message = input.messages[0].clone();
        message.content = document_content(pair);
        input.messages.push(message);
    }
    let original = input.clone();
    let read_ids = Arc::new(Mutex::new(Vec::new()));
    let captured = read_ids.clone();
    let prepared = adapter
        .prepare_request_using_reader(
            &config("unknown"),
            &input,
            Arc::new(move |root, row| {
                captured.lock().unwrap().push(row.id.clone());
                read_attachment_bytes(root, row)
            }),
        )
        .await
        .unwrap();
    assert_eq!(*read_ids.lock().unwrap(), ["doc4", "doc5", "doc2", "doc3"]);
    assert!(!prepared.messages[0].content.text.contains("derived"));
    assert!(prepared.messages[2].content.text.contains("derived doc4"));
    assert!(prepared.messages[2]
        .content
        .text
        .contains("word/document.xml#paragraph=1"));
    assert_eq!(input, original);
    for message in &prepared.messages {
        for key in [
            ATTACHMENT_RECORDS_KEY,
            ATTACHMENT_INPUT_TEXT_KEY,
            anima_core::document_content::DOCUMENT_EXTRACTS_METADATA_KEY,
        ] {
            assert!(!message
                .content
                .metadata
                .as_ref()
                .is_some_and(|m| m.contains_key(key)));
        }
    }
}

#[tokio::test]
async fn document_snapshots_foreign_role_bad_metadata_and_changed_source_fail_closed() {
    let temp = Temp::new();
    let row = document_record(&temp, "doc");
    let adapter = fixture(&[row.clone()], Some(temp.0.clone())).await;
    for scenario in ["assistant", "foreign", "missing", "malformed", "hash"] {
        let mut input = request("agent", "chat:session", document_content(&[row.clone()]));
        match scenario {
            "assistant" => input.messages[0].role = MessageRole::Assistant,
            "foreign" => input.messages[0].room_id = "chat:foreign".into(),
            "missing" => {
                input.messages[0].content.metadata = None;
            }
            "malformed" => {
                input.messages[0].content.metadata.as_mut().unwrap().insert(
                    anima_core::document_content::DOCUMENT_EXTRACTS_METADATA_KEY.into(),
                    DataValue::String("forged".into()),
                );
            }
            "hash" => {
                use anima_core::document_content::*;
                let mut snapshots = parse_document_snapshots(&input.messages[0].content).unwrap();
                snapshots[0].source_sha256 = "0".repeat(64);
                input.messages[0].content.metadata.as_mut().unwrap().insert(
                    DOCUMENT_EXTRACTS_METADATA_KEY.into(),
                    document_snapshots_metadata(&snapshots).unwrap(),
                );
            }
            _ => unreachable!(),
        }
        let prepared = adapter
            .prepare_request_using_reader(
                &config("unknown"),
                &input,
                Arc::new(|_, _| panic!("ineligible document must not be read")),
            )
            .await
            .unwrap();
        assert!(
            !prepared.messages[0].content.text.contains("derived"),
            "{scenario}"
        );
    }
    std::fs::write(temp.0.join(&row.path), b"changed").unwrap();
    let prepared = adapter
        .prepare_request(
            &config("unknown"),
            &request("agent", "chat:session", document_content(&[row])),
        )
        .await
        .unwrap();
    assert!(!prepared.messages[0].content.text.contains("derived"));
}

#[tokio::test]
async fn document_snapshot_reloaded_binding_reaches_generate_and_stream_without_reparse() {
    use anima_core::{ModelStreamFrame, ModelStreamSink};
    struct StreamingCapture(Capture);
    #[async_trait]
    impl ModelAdapter for StreamingCapture {
        fn provider(&self) -> &str {
            "test"
        }
        async fn generate(
            &self,
            config: &AgentConfig,
            input: &ModelGenerateRequest,
        ) -> Result<ModelGenerateResponse, String> {
            self.0.generate(config, input).await
        }
        async fn stream(
            &self,
            config: &AgentConfig,
            input: &ModelGenerateRequest,
            sink: &dyn ModelStreamSink,
        ) -> Result<(), String> {
            sink.emit(ModelStreamFrame::Final(
                self.0.generate(config, input).await?,
            ))
            .await
        }
    }
    struct Sink;
    #[async_trait]
    impl ModelStreamSink for Sink {
        async fn emit(&self, _frame: ModelStreamFrame) -> Result<(), String> {
            Ok(())
        }
    }
    let temp = Temp::new();
    for row in [document_record(&temp, "doc"), pdf_record(&temp, "pdf")] {
        let store = Arc::new(MemoryHistoryStore::new());
        store.upsert_attachments(&[row.clone()]).await.unwrap();
        let inner = Arc::new(StreamingCapture(Capture::default()));
        let adapter = AttachmentModelAdapter::new(
            inner.clone(),
            store,
            Some(temp.0.clone()),
            "agent".into(),
            "chat:session".into(),
        );
        let saved = document_content(&[row.clone()]);
        let reloaded: Content =
            serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
        let input = request("agent", "chat:session", reloaded.clone());
        adapter.generate(&config("unknown"), &input).await.unwrap();
        adapter
            .stream(&config("unknown"), &input, &Sink)
            .await
            .unwrap();
        let requests = inner.0 .0.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0], requests[1]);
        assert!(requests[0].messages[0]
            .content
            .text
            .contains(&format!("derived {}", row.id)));
        assert!(requests[0].messages[0].content.text.contains(
            if row.mime == crate::attachments::documents::PDF_MIME {
                "pdf#page=1"
            } else {
                "word/document.xml#paragraph=1"
            }
        ));
        assert_eq!(input.messages[0].content, reloaded);
        drop(requests);
        let mut compaction = input.messages.clone();
        adapter.prepare_compaction_documents(&mut compaction).await;
        assert!(compaction[0]
            .content
            .text
            .starts_with("\n[UNTRUSTED DOCUMENT DATA"));
        assert!(compaction[0]
            .content
            .text
            .contains(&format!("derived {}", row.id)));
        assert!(reloaded.text.starts_with("original owner bytes"));
        assert!(compaction[0].content.text.ends_with(&reloaded.text));
        assert_eq!(input.messages[0].content, reloaded);
    }
}

#[tokio::test]
async fn mixed_history_hydration_has_four_reads_and_rejects_pdf_source_format_or_hash_mismatch() {
    use anima_core::document_content::*;
    let temp = Temp::new();
    let rows = (0..6)
        .map(|i| {
            let id = format!("mixed{i}");
            if i % 2 == 0 {
                document_record(&temp, &id)
            } else {
                pdf_record(&temp, &id)
            }
        })
        .collect::<Vec<_>>();
    let adapter = fixture(&rows, Some(temp.0.clone())).await;
    let mut input = request("agent", "chat:session", document_content(&rows[..2]));
    for pair in rows[2..].chunks(2) {
        let mut message = input.messages[0].clone();
        message.content = document_content(pair);
        input.messages.push(message);
    }
    let original = input.clone();
    let read_ids = Arc::new(Mutex::new(Vec::new()));
    let captured = read_ids.clone();
    let prepared = adapter
        .prepare_request_using_reader(
            &config("unknown"),
            &input,
            Arc::new(move |root, row| {
                captured.lock().unwrap().push(row.id.clone());
                read_attachment_bytes(root, row)
            }),
        )
        .await
        .unwrap();
    assert_eq!(
        *read_ids.lock().unwrap(),
        ["mixed4", "mixed5", "mixed2", "mixed3"]
    );
    assert!(!prepared.messages[0].content.text.contains("derived"));
    for message in &prepared.messages[1..] {
        assert!(message.content.text.contains("pdf#page=1"));
        assert!(message
            .content
            .text
            .contains("word/document.xml#paragraph=1"));
    }
    assert_eq!(input, original);
    let pdf = &rows[1];
    for scenario in ["foreign", "hash", "source_format"] {
        let mut input = request("agent", "chat:session", document_content(&[pdf.clone()]));
        if scenario == "foreign" {
            input.messages[0].room_id = "chat:foreign".into();
        } else {
            let mut snapshots = parse_document_snapshots(&input.messages[0].content).unwrap();
            if scenario == "hash" {
                snapshots[0].source_sha256 = "0".repeat(64);
            } else {
                // A fully valid DOCX snapshot cannot impersonate this PDF source.
                snapshots[0].extraction.format = DocumentFormat::Docx;
                snapshots[0].extraction.blocks[0].locator =
                    DocumentLocator::DocxParagraph { paragraph: 1 };
                snapshots[0].extraction.omissions = vec![DocumentOmission::MainBodyOnly];
                snapshots[0].extraction.pdf_coverage = None;
            }
            input.messages[0].content.metadata.as_mut().unwrap().insert(
                DOCUMENT_EXTRACTS_METADATA_KEY.into(),
                document_snapshots_metadata(&snapshots).unwrap(),
            );
        }
        let prepared = adapter
            .prepare_request_using_reader(
                &config("unknown"),
                &input,
                Arc::new(|_, _| panic!("invalid PDF binding must not read source")),
            )
            .await
            .unwrap();
        assert!(
            !prepared.messages[0].content.text.contains("derived"),
            "{scenario}"
        );
        let mut compacted = input.messages.clone();
        adapter.prepare_compaction_documents(&mut compacted).await;
        assert!(!compacted[0].content.text.contains("derived"), "{scenario}");
    }
    std::fs::write(temp.0.join(&pdf.path), b"changed").unwrap();
    let input = request("agent", "chat:session", document_content(&[pdf.clone()]));
    let prepared = adapter
        .prepare_request(&config("unknown"), &input)
        .await
        .unwrap();
    assert!(!prepared.messages[0].content.text.contains("derived"));
    let mut compacted = input.messages.clone();
    adapter.prepare_compaction_documents(&mut compacted).await;
    assert!(!compacted[0].content.text.contains("derived"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn attachment_model_cancellation_finishes_before_a_blocked_reader_is_released() {
    let temp = Temp::new();
    let row = record(&temp, "agent", "chat:session", "photo");
    let adapter = fixture(&[row.clone()], Some(temp.0.clone())).await;
    let input = request("agent", "chat:session", content(&[row]));
    let cancel = anima_core::CancelSignal::new();
    let task_cancel = cancel.clone();
    let (started, entered) = tokio::sync::oneshot::channel();
    let started = Mutex::new(Some(started));
    let (release, blocked) = std::sync::mpsc::channel();
    let blocked = Mutex::new(blocked);
    let task = tokio::spawn(async move {
        let async_thread = std::thread::current().id();
        let reader = move |_: &std::path::Path, _: &AttachmentRecord| {
            started
                .lock()
                .unwrap()
                .take()
                .unwrap()
                .send(std::thread::current().id() == async_thread)
                .unwrap();
            blocked.lock().unwrap().recv().unwrap();
            Ok(GIF.to_vec())
        };
        let config = config("gpt-4o-mini");
        let preparation = adapter.prepare_request_using_reader(&config, &input, Arc::new(reader));
        let cancelled =
            match futures::future::select(Box::pin(preparation), Box::pin(task_cancel.cancelled()))
                .await
            {
                futures::future::Either::Left(_) => false,
                futures::future::Either::Right(_) => true,
            };
        cancelled
    });
    let reader_on_async_thread = entered.await.unwrap();
    cancel.cancel();
    // Release a defective synchronous reader before asserting so RED cannot deadlock.
    if reader_on_async_thread {
        release.send(()).unwrap();
        let _ = task.await.unwrap();
        panic!("image reader blocked the async cancellation poll");
    }
    assert!(
        task.await.unwrap(),
        "cancellation completes while the reader gate is still closed"
    );
    release.send(()).unwrap();
}

struct Temp(std::path::PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("anima-hydration-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn config(model: &str) -> AgentConfig {
    serde_json::from_value(
        serde_json::json!({"name":"image test", "model":model, "provider":"openai"}),
    )
    .unwrap()
}

fn record(temp: &Temp, agent: &str, session: &str, id: &str) -> AttachmentRecord {
    let saved = save_upload(&temp.0, "2026-10-11", &format!("{id}.gif"), GIF).unwrap();
    AttachmentRecord {
        id: id.into(),
        agent_id: agent.into(),
        session_id: session.into(),
        name: format!("{id}.gif"),
        mime: "image/gif".into(),
        size: GIF.len() as u64,
        kind: AttachmentKind::Image,
        path: saved.path,
        created_at_ms: 0,
        workspace_root: saved.workspace_root,
        sha256: saved.sha256,
    }
}

fn content(records: &[AttachmentRecord]) -> Content {
    Content {
        text: "Describe the images".into(),
        attachments: Some(
            records
                .iter()
                .map(|record| Attachment {
                    attachment_type: AttachmentType::Image,
                    name: record.name.clone(),
                    data: record.path.clone(),
                    mime_type: Some(record.mime.clone()),
                })
                .collect(),
        ),
        metadata: Some(BTreeMap::from([
            (
                "attachmentRecords".into(),
                DataValue::Array(
                    records
                        .iter()
                        .map(|record| {
                            DataValue::Object(BTreeMap::from([
                                ("id".into(), DataValue::String(record.id.clone())),
                                ("name".into(), DataValue::String(record.name.clone())),
                                ("mime".into(), DataValue::String(record.mime.clone())),
                                ("path".into(), DataValue::String(record.path.clone())),
                                ("size".into(), DataValue::Number(record.size as f64)),
                                ("kind".into(), DataValue::String("image".into())),
                                ("createdAtMs".into(), DataValue::Number(0.0)),
                            ]))
                        })
                        .collect(),
                ),
            ),
            (
                "attachmentInputText".into(),
                DataValue::String("Describe the images".into()),
            ),
            ("ordinary".into(), DataValue::String("preserved".into())),
        ])),
    }
}

#[derive(Default)]
struct Capture(Mutex<Vec<ModelGenerateRequest>>);
#[async_trait]
impl ModelAdapter for Capture {
    fn provider(&self) -> &str {
        "openai"
    }
    async fn generate(
        &self,
        _config: &AgentConfig,
        request: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        self.0.lock().unwrap().push(request.clone());
        Ok(ModelGenerateResponse {
            content: Content {
                text: "Fixed reply".into(),
                ..Content::default()
            },
            tool_calls: None,
            usage: TokenUsage {
                prompt_tokens: 2,
                completion_tokens: 3,
                total_tokens: 5,
                ..TokenUsage::default()
            },
            stop_reason: ModelStopReason::End,
        })
    }
}

#[tokio::test]
async fn attachment_model_runtime_hydrates_only_outbound_copy_after_restart() {
    let temp = Temp::new();
    let capture = Arc::new(Capture::default());
    let mut state = crate::state::DaemonState::with_model_adapter(capture.clone());
    state.workspace = Some(crate::control_plane_store::WorkspaceConfig {
        root_path: temp.0.clone(),
        ..Default::default()
    });
    let agent = state.create_agent(config("gpt-4o-mini")).unwrap().state.id;
    let session = crate::sessions::new_chat_session_id();
    let record = record(&temp, &agent, &session, "photo");
    state
        .history
        .store()
        .upsert_attachments(&[record.clone()])
        .await
        .unwrap();
    let input = content(&[record.clone()]);
    for restored in [false, true] {
        if restored {
            let snapshot = state.control_plane_snapshot();
            state.restore_control_plane_snapshot(snapshot).unwrap();
        }
        let build = state.build_run_runtime(&agent, &session, &input).unwrap();
        let mut runtime = build.runtime;
        let result = runtime
            .run_in_room_with_context(session.clone(), runtime.messages().to_vec(), input.clone())
            .await;
        assert_eq!(result.status, anima_core::TaskStatus::Success);
        let requests = capture.0.lock().unwrap();
        let outbound = &requests.last().unwrap().messages.last().unwrap().content;
        assert!(outbound.attachments.as_ref().unwrap()[0]
            .data
            .starts_with("data:image/gif;base64,"));
        assert!(!outbound
            .metadata
            .as_ref()
            .unwrap()
            .contains_key("attachmentRecords"));
        assert!(!outbound
            .metadata
            .as_ref()
            .unwrap()
            .contains_key("attachmentInputText"));
        assert_eq!(
            outbound.metadata.as_ref().unwrap()["ordinary"],
            DataValue::String("preserved".into())
        );
        drop(requests);
        let snapshot = serde_json::to_string(&runtime.snapshot()).unwrap();
        assert!(!snapshot.contains("data:image"));
        assert_eq!(
            runtime
                .messages()
                .iter()
                .find(|message| message.role == MessageRole::User)
                .unwrap()
                .content
                .attachments
                .as_ref()
                .unwrap()[0]
                .data,
            record.path
        );
        assert_eq!(
            state
                .history
                .store()
                .get_attachment(&agent, "photo")
                .await
                .unwrap()
                .unwrap(),
            record
        );
    }
}

fn request(agent: &str, session: &str, content: Content) -> ModelGenerateRequest {
    ModelGenerateRequest {
        system: "System".into(),
        temperature: None,
        max_tokens: None,
        messages: vec![anima_core::Message {
            id: "user".into(),
            agent_id: agent.into(),
            room_id: session.into(),
            role: MessageRole::User,
            content,
            created_at_ms: 0,
        }],
    }
}

async fn fixture(
    rows: &[AttachmentRecord],
    root: Option<std::path::PathBuf>,
) -> AttachmentModelAdapter {
    let store = Arc::new(MemoryHistoryStore::new());
    store.upsert_attachments(rows).await.unwrap();
    AttachmentModelAdapter::new(
        Arc::new(Capture::default()),
        store,
        root,
        "agent".into(),
        "chat:session".into(),
    )
}

#[tokio::test]
async fn attachment_model_nonvision_unknown_and_legacy_never_read_files() {
    let temp = Temp::new();
    let row = record(&temp, "agent", "chat:session", "photo");
    let adapter = fixture(&[row.clone()], Some(temp.0.clone())).await;
    for model in ["unknown", "o3-mini"] {
        let input = request("agent", "chat:session", content(&[row.clone()]));
        let prepared = adapter
            .prepare_request_using_reader(
                &config(model),
                &input,
                Arc::new(|_, _| panic!("unsupported models must not read files")),
            )
            .await
            .unwrap();
        assert!(prepared.messages[0].content.attachments.is_none());
        assert!(prepared.messages[0].content.text.contains("cannot view"));
        assert!(prepared.messages[0].content.text.contains(&row.path));
    }
    let mut legacy = content(&[row]);
    legacy.metadata = None;
    let prepared = adapter
        .prepare_request_using_reader(
            &config("gpt-4o-mini"),
            &request("agent", "chat:session", legacy),
            Arc::new(|_, _| panic!("untrusted legacy references must not read files")),
        )
        .await
        .unwrap();
    assert!(prepared.messages[0].content.attachments.is_none());
}

#[tokio::test]
async fn attachment_model_caps_newest_four_and_does_not_read_dropped_context() {
    let temp = Temp::new();
    let rows = (0..6)
        .map(|index| record(&temp, "agent", "chat:session", &format!("photo{index}")))
        .collect::<Vec<_>>();
    let adapter = fixture(&rows, Some(temp.0.clone())).await;
    let mut input = request("agent", "chat:session", content(&rows[..2]));
    for (index, pair) in rows[2..].chunks(2).enumerate() {
        let mut message = input.messages[0].clone();
        message.id = format!("turn{index}");
        message.content = content(pair);
        input.messages.push(message);
    }
    let read_ids = Arc::new(Mutex::new(Vec::new()));
    let captured_ids = Arc::clone(&read_ids);
    let reader: AttachmentReader =
        Arc::new(move |root: &std::path::Path, row: &AttachmentRecord| {
            captured_ids.lock().unwrap().push(row.id.clone());
            crate::attachments::files::read_attachment_bytes(root, row)
        });
    let prepared = adapter
        .prepare_request_using_reader(&config("gpt-4o-mini"), &input, Arc::clone(&reader))
        .await
        .unwrap();
    assert_eq!(
        *read_ids.lock().unwrap(),
        ["photo4", "photo5", "photo2", "photo3"]
    );
    assert!(prepared.messages[0].content.attachments.is_none());
    assert!(prepared.messages[0].content.text.contains(&rows[0].path));
    assert_eq!(
        prepared
            .messages
            .iter()
            .filter_map(|message| message.content.attachments.as_ref())
            .flatten()
            .count(),
        4
    );
    assert_eq!(
        input.messages[0].content.attachments.as_ref().unwrap()[0].data,
        rows[0].path
    );
    let budget = anima_core::TokenEstimator::raw_message_tokens(input.messages.last().unwrap());
    let selected = anima_core::select_context(
        &input.messages,
        None,
        budget,
        &anima_core::TokenEstimator::default(),
    );
    assert_eq!(selected.messages.len(), 1);
    input.messages = selected.messages;
    read_ids.lock().unwrap().clear();
    adapter
        .prepare_request_using_reader(&config("gpt-4o-mini"), &input, Arc::clone(&reader))
        .await
        .unwrap();
    assert_eq!(*read_ids.lock().unwrap(), ["photo4", "photo5"]);
}

#[tokio::test]
async fn attachment_model_rejects_foreign_and_tampered_bindings_before_read() {
    let temp = Temp::new();
    let own = record(&temp, "agent", "chat:session", "own");
    let foreign = record(&temp, "agent", "chat:other", "foreign");
    let other_agent = record(&temp, "other-agent", "chat:session", "other-agent");
    let adapter = fixture(
        &[own.clone(), foreign.clone(), other_agent.clone()],
        Some(temp.0.clone()),
    )
    .await;
    let mut variants = vec![
        request("agent", "chat:session", content(&[foreign])),
        request("agent", "chat:session", content(&[other_agent])),
    ];
    for field in ["path", "mime", "name", "id"] {
        let mut input = request("agent", "chat:session", content(&[own.clone()]));
        let metadata = input.messages[0].content.metadata.as_mut().unwrap();
        let DataValue::Array(records) = metadata
            .get_mut(crate::attachments::ATTACHMENT_RECORDS_KEY)
            .unwrap()
        else {
            panic!()
        };
        let DataValue::Object(record) = &mut records[0] else {
            panic!()
        };
        record.insert(field.into(), DataValue::String("forged".into()));
        variants.push(input);
    }
    variants.push(request(
        "other-agent",
        "chat:session",
        content(&[own.clone()]),
    ));
    variants.push(request("agent", "chat:other", content(&[own])));
    for input in variants {
        let prepared = adapter
            .prepare_request_using_reader(
                &config("gpt-4o-mini"),
                &input,
                Arc::new(|_, _| panic!("foreign or tampered record must not read files")),
            )
            .await
            .unwrap();
        assert!(prepared.messages[0].content.attachments.is_none());
        assert!(prepared.messages[0].content.text.contains("unavailable"));
    }
}

#[tokio::test]
async fn attachment_model_unavailable_or_changed_file_keeps_remaining_input() {
    let temp = Temp::new();
    let other = Temp::new();
    let row = record(&temp, "agent", "chat:session", "photo");
    let adapter = fixture(&[row.clone()], Some(temp.0.clone())).await;
    let input = request("agent", "chat:session", content(&[row.clone()]));
    let prepared = adapter
        .prepare_request(&config("gpt-4o-mini"), &input)
        .await
        .unwrap();
    assert!(prepared.messages[0].content.attachments.is_some());
    for root in [None, Some(other.0.clone())] {
        let changed = fixture(&[row.clone()], root).await;
        let prepared = changed
            .prepare_request(&config("gpt-4o-mini"), &input)
            .await
            .unwrap();
        assert!(prepared.messages[0].content.attachments.is_none());
        assert!(prepared.messages[0]
            .content
            .text
            .starts_with("Describe the images"));
    }
    let mut bytes = GIF.to_vec();
    bytes[12] = 1;
    std::fs::write(temp.0.join(&row.path), bytes).unwrap();
    let prepared = adapter
        .prepare_request(&config("gpt-4o-mini"), &input)
        .await
        .unwrap();
    assert!(prepared.messages[0].content.attachments.is_none());
    std::fs::remove_file(temp.0.join(&row.path)).unwrap();
    let prepared = adapter
        .prepare_request(&config("gpt-4o-mini"), &input)
        .await
        .unwrap();
    assert!(prepared.messages[0].content.text.contains(&row.path));
    assert!(!prepared.messages[0].content.text.contains("data:"));
}

#[tokio::test]
async fn attachment_model_bounds_bytes_and_leaves_documents_text_only() {
    let temp = Temp::new();
    let row = record(&temp, "agent", "chat:session", "photo");
    let adapter = fixture(&[row.clone()], Some(temp.0.clone())).await;
    let mut input = request("agent", "chat:session", content(&[row.clone()]));
    input.messages[0]
        .content
        .attachments
        .as_mut()
        .unwrap()
        .push(Attachment {
            attachment_type: AttachmentType::File,
            name: "notes.pdf".into(),
            data: "uploads/notes.pdf".into(),
            mime_type: Some("application/pdf".into()),
        });
    let prepared = adapter
        .prepare_request_using_reader(
            &config("gpt-4o-mini"),
            &input,
            Arc::new(|_, _| Ok(vec![0; crate::attachments::MAX_IMAGE_BYTES + 1])),
        )
        .await
        .unwrap();
    assert_eq!(
        prepared.messages[0]
            .content
            .attachments
            .as_ref()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        prepared.messages[0].content.attachments.as_ref().unwrap()[0].attachment_type,
        AttachmentType::File
    );
    assert!(!prepared.messages[0].content.text.contains("data:"));
    let prepared = adapter
        .prepare_request_using_reader(
            &config("gpt-4o-mini"),
            &input,
            Arc::new(|_, _| Ok(vec![0; crate::attachments::MAX_IMAGE_BYTES])),
        )
        .await
        .unwrap();
    // A reader returning a different byte count than the immutable record is refused.
    assert_eq!(
        prepared.messages[0]
            .content
            .attachments
            .as_ref()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        input.messages[0].content.attachments.as_ref().unwrap()[0].data,
        row.path
    );
    let mut maximum = row;
    maximum.size = crate::attachments::MAX_IMAGE_BYTES as u64;
    let maximum_adapter = fixture(&[maximum.clone()], Some(temp.0.clone())).await;
    let prepared = maximum_adapter
        .prepare_request_using_reader(
            &config("gpt-4o-mini"),
            &request("agent", "chat:session", content(&[maximum])),
            Arc::new(|_, _| Ok(vec![0; crate::attachments::MAX_IMAGE_BYTES])),
        )
        .await
        .unwrap();
    assert_eq!(
        prepared.messages[0].content.attachments.as_ref().unwrap()[0]
            .data
            .len(),
        "data:image/gif;base64,".len() + MAX_ENCODED_IMAGE_BYTES
    );
}

#[tokio::test]
async fn attachment_model_forwards_stream_frames_and_usage_once_and_preserves_errors() {
    use anima_core::{ModelStreamFrame, ModelStreamSink};
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Inner {
        capture: Capture,
        generate_count: AtomicUsize,
        stream_count: AtomicUsize,
    }
    #[async_trait]
    impl ModelAdapter for Inner {
        fn provider(&self) -> &str {
            "openai"
        }
        async fn generate(
            &self,
            config: &AgentConfig,
            request: &ModelGenerateRequest,
        ) -> Result<ModelGenerateResponse, String> {
            self.generate_count.fetch_add(1, Ordering::SeqCst);
            self.capture.generate(config, request).await
        }
        async fn stream(
            &self,
            config: &AgentConfig,
            request: &ModelGenerateRequest,
            sink: &dyn ModelStreamSink,
        ) -> Result<(), String> {
            self.stream_count.fetch_add(1, Ordering::SeqCst);
            let response = self.capture.generate(config, request).await?;
            sink.emit(ModelStreamFrame::TextDelta("Fixed reply".into()))
                .await?;
            sink.emit(ModelStreamFrame::Usage(response.usage.clone()))
                .await?;
            sink.emit(ModelStreamFrame::Final(response)).await
        }
    }
    struct Sink(Mutex<Vec<ModelStreamFrame>>);
    #[async_trait]
    impl ModelStreamSink for Sink {
        async fn emit(&self, frame: ModelStreamFrame) -> Result<(), String> {
            self.0.lock().unwrap().push(frame);
            Ok(())
        }
    }
    struct Reject;
    #[async_trait]
    impl ModelStreamSink for Reject {
        async fn emit(&self, _frame: ModelStreamFrame) -> Result<(), String> {
            Err("consumer stopped".into())
        }
    }
    let temp = Temp::new();
    let row = record(&temp, "agent", "chat:session", "photo");
    let store = Arc::new(MemoryHistoryStore::new());
    store.upsert_attachments(&[row.clone()]).await.unwrap();
    let inner = Arc::new(Inner {
        capture: Capture::default(),
        generate_count: AtomicUsize::new(0),
        stream_count: AtomicUsize::new(0),
    });
    let adapter = AttachmentModelAdapter::new(
        inner.clone(),
        store,
        Some(temp.0.clone()),
        "agent".into(),
        "chat:session".into(),
    );
    let request = request("agent", "chat:session", content(&[row]));
    assert_eq!(adapter.provider(), "openai");
    let sink = Sink(Mutex::new(Vec::new()));
    adapter
        .stream(&config("gpt-4o-mini"), &request, &sink)
        .await
        .unwrap();
    assert_eq!(inner.generate_count.load(Ordering::SeqCst), 0);
    assert_eq!(inner.stream_count.load(Ordering::SeqCst), 1);
    assert_eq!(inner.capture.0.lock().unwrap().len(), 1);
    let response = adapter
        .generate(&config("gpt-4o-mini"), &request)
        .await
        .unwrap();
    assert_eq!(response.usage.total_tokens, 5);
    assert_eq!(inner.generate_count.load(Ordering::SeqCst), 1);
    assert_eq!(
        *sink.0.lock().unwrap(),
        vec![
            ModelStreamFrame::TextDelta("Fixed reply".into()),
            ModelStreamFrame::Usage(response.usage.clone()),
            ModelStreamFrame::Final(response)
        ]
    );
    assert_eq!(
        adapter
            .stream(&config("gpt-4o-mini"), &request, &Reject)
            .await
            .unwrap_err(),
        "consumer stopped"
    );
    assert!(
        !request.messages[0].content.attachments.as_ref().unwrap()[0]
            .data
            .starts_with("data:")
    );
}

#[tokio::test]
async fn attachment_model_failed_run_snapshot_and_history_retain_only_references() {
    struct Failing(Capture);
    #[async_trait]
    impl ModelAdapter for Failing {
        fn provider(&self) -> &str {
            "openai"
        }
        async fn generate(
            &self,
            config: &AgentConfig,
            request: &ModelGenerateRequest,
        ) -> Result<ModelGenerateResponse, String> {
            self.0.generate(config, request).await?;
            Err("planned model failure".into())
        }
    }
    let temp = Temp::new();
    let failing = Arc::new(Failing(Capture::default()));
    let mut state = crate::state::DaemonState::with_model_adapter(failing.clone());
    state.workspace = Some(crate::control_plane_store::WorkspaceConfig {
        root_path: temp.0.clone(),
        ..Default::default()
    });
    let mut config = config("gpt-4o-mini");
    config.settings = Some(anima_core::AgentSettings {
        max_retries: Some(0),
        ..Default::default()
    });
    let agent = state.create_agent(config).unwrap().state.id;
    let session = crate::sessions::new_chat_session_id();
    let row = record(&temp, &agent, &session, "photo");
    state
        .history
        .store()
        .upsert_attachments(&[row.clone()])
        .await
        .unwrap();
    let input = content(&[row]);
    let build = state.build_run_runtime(&agent, &session, &input).unwrap();
    let mut runtime = build.runtime;
    let result = runtime
        .run_in_room_with_context(session.clone(), runtime.messages().to_vec(), input)
        .await;
    assert_eq!(result.status, anima_core::TaskStatus::Error);
    assert_eq!(failing.0 .0.lock().unwrap().len(), 1);
    assert!(failing.0 .0.lock().unwrap()[0]
        .messages
        .last()
        .unwrap()
        .content
        .attachments
        .as_ref()
        .unwrap()[0]
        .data
        .starts_with("data:"));
    assert!(!serde_json::to_string(&runtime.snapshot())
        .unwrap()
        .contains("data:"));
    let row = crate::history::HistoryMessage {
        agent_id: agent.clone(),
        session_id: session.clone(),
        hidden: false,
        message: runtime
            .messages()
            .iter()
            .find(|message| message.role == MessageRole::User)
            .unwrap()
            .clone(),
    };
    state
        .history
        .store()
        .upsert_messages(&[row.clone()])
        .await
        .unwrap();
    let stored = state
        .history
        .store()
        .get_message(&agent, &session, &row.message.id)
        .await
        .unwrap()
        .unwrap();
    assert!(!serde_json::to_string(&stored.message)
        .unwrap()
        .contains("data:"));
}
