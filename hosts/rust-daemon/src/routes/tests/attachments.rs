use super::*;
use crate::agent_runs::test_support::{Gate, ScriptedModel, Step};
use crate::history::{
    ApprovalPageQuery, HistoryError, HistoryMessage, HistoryService, HistoryStore,
    MemoryHistoryStore, MessagePageQuery, UsagePageQuery,
};
use crate::sessions::{SessionKind, SessionOrigin, SessionRecord, TitleSource};
use crate::{
    approvals::ApprovalRequest, attachments::AttachmentRecord, runs::RunRecord,
    schedules::ScheduleFireRecord, usage::UsageRecord,
};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
const OWNER: &str = "http://localhost:4200";

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("anima-attachment-route-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        if self.0.starts_with(std::env::temp_dir())
            && self
                .0
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("anima-attachment-route-")
        {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
fn fixture() -> (Temp, Arc<RwLock<DaemonState>>, String) {
    let temp = Temp::new();
    let mut daemon = DaemonState::new();
    daemon.workspace = Some(WorkspaceConfig {
        root_path: temp.0.clone(),
        company_name: "test".into(),
        mission: "test".into(),
        values: vec![],
    });
    let agent = daemon
        .create_agent(test_config("attachments"))
        .unwrap()
        .state
        .id;
    daemon.sessions.insert(SessionRecord::new(
        &agent,
        "chat:files",
        SessionKind::Chat,
        SessionOrigin::Web,
        "Files".into(),
        TitleSource::FirstMessage,
        1,
    ));
    (temp, Arc::new(RwLock::new(daemon)), agent)
}
fn uri(agent: &str) -> String {
    format!("/api/agents/{agent}/sessions/chat%3Afiles/attachments")
}
fn form(name: &str, bytes: &[u8], mime: &str) -> Vec<u8> {
    let mut body=format!("--m9\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\nContent-Type: {mime}\r\n\r\n").into_bytes();
    body.extend_from_slice(bytes);
    body.extend_from_slice(b"\r\n--m9--\r\n");
    body
}
fn upload(path: &str, body: Vec<u8>, owner: bool) -> Request<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri(path)
        .header("host", "127.0.0.1:8080")
        .header("content-type", "multipart/form-data; boundary=m9");
    if owner {
        builder = builder.header("origin", OWNER);
    }
    builder.body(Body::from(body)).unwrap()
}
fn get(path: &str, owner: bool) -> Request<Body> {
    let mut builder = Request::builder()
        .uri(path)
        .header("host", "127.0.0.1:8080");
    if owner {
        builder = builder.header("origin", OWNER);
    }
    builder.body(Body::empty()).unwrap()
}
async fn json_body(response: axum::response::Response) -> Value {
    serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap()
}

/// In-process route tests use the real parser; fixed executable isolation is
/// exercised separately by the controller's physical worker tests.
struct RealDocxExtractor;
#[async_trait::async_trait]
impl crate::attachments::documents::worker::DocumentExtractor for RealDocxExtractor {
    async fn extract(
        &self,
        format: anima_core::document_content::DocumentFormat,
        bytes: Vec<u8>,
        cancel: anima_core::CancelSignal,
        _deadline: Option<tokio::time::Instant>,
    ) -> Result<
        anima_core::document_content::ExtractedDocumentText,
        crate::attachments::documents::worker::WorkerFailure,
    > {
        if cancel.is_cancelled() {
            return Err(crate::attachments::documents::worker::WorkerFailure::Cancelled);
        }
        Ok(anima_document_content::extract_document(format, &bytes))
    }
}

#[tokio::test]
async fn docx_selected_text_reaches_model_m54() {
    let temp = Temp::new();
    let gate = Gate::new();
    let model = ScriptedModel::gated(vec![Step::Text(vec!["done"])], gate.clone());
    let mut daemon = DaemonState::with_model_adapter(model.clone());
    daemon.workspace = Some(WorkspaceConfig {
        root_path: temp.0.clone(),
        company_name: "test".into(),
        mission: "test".into(),
        values: vec![],
    });
    let agent = daemon
        .create_agent(test_config("attachments"))
        .unwrap()
        .state
        .id;
    daemon.sessions.insert(SessionRecord::new(
        &agent,
        "chat:files",
        SessionKind::Chat,
        SessionOrigin::Web,
        "Files".into(),
        TitleSource::FirstMessage,
        1,
    ));
    let state = Arc::new(RwLock::new(daemon));
    let runs = AgentRunCoordinator::new(state.clone(), Arc::new(Semaphore::new(4)))
        .with_document_extractor(Arc::new(RealDocxExtractor));
    let app =
        crate::routes::router_with_runs(state.clone(), DaemonConfig::default(), |_| runs.clone());

    let selected = app
        .clone()
        .oneshot(upload(
            &uri(&agent),
            form(
                "m54-selected.docx",
                include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/tests/fixtures/documents/m54-selected.docx"
                )),
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            ),
            true,
        ))
        .await
        .unwrap();
    assert_eq!(selected.status(), StatusCode::CREATED);
    let selected_id = json_body(selected).await["attachment"]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let unselected = app
        .clone()
        .oneshot(upload(
            &uri(&agent),
            form(
                "m54-unselected.docx",
                include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/tests/fixtures/documents/m54-unselected.docx"
                )),
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            ),
            true,
        ))
        .await
        .unwrap();
    assert_eq!(unselected.status(), StatusCode::CREATED);
    let unselected_id = json_body(unselected).await["attachment"]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/agents/{agent}/sessions/chat%3Afiles/runs"))
        .header("host", "127.0.0.1:8080")
        .header("origin", OWNER)
        .header("content-type", "application/json")
        .header("idempotency-key", "m54-selected-docx")
        .body(Body::from(
            json!({
                "text": "Read the selected document",
                "attachmentIds": [selected_id],
            })
            .to_string(),
        ))
        .unwrap();
    let accepted = app.clone().oneshot(request).await.unwrap();
    assert_eq!(accepted.status(), StatusCode::ACCEPTED);
    let run_id = json_body(accepted).await["run"]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    tokio::time::timeout(Duration::from_secs(15), gate.entered())
        .await
        .expect("M54 recording adapter was not reached within the bounded wait");
    let requests = model.requests();
    let user_data = requests
        .iter()
        .flat_map(|request| request.messages.iter())
        .filter(|message| message.role == anima_core::MessageRole::User)
        .map(|message| message.content.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        user_data.contains("M54 selected record: cobalt harbour 7319."),
        "selected DOCX main-body text must reach model user data: {user_data}"
    );
    for excluded in [
        "NEVER_INLINE_2047",
        "HEADER_OMITTED_4096",
        "PHONETIC_GUIDE_NOT_BODY",
        unselected_id.as_str(),
    ] {
        assert!(
            !user_data.contains(excluded),
            "unselected or excluded DOCX content reached model user data: {excluded}"
        );
    }
    gate.release();
    for _ in 0..500 {
        if state
            .read()
            .await
            .runs
            .get(&run_id)
            .is_some_and(|run| run.status == crate::runs::RunStatus::Completed)
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("M54 attachment run did not complete");
}

#[tokio::test]
async fn pdf_selected_text_reaches_model_m55() {
    let temp = Temp::new();
    let gate = Gate::new();
    let model = ScriptedModel::gated(vec![Step::Text(vec!["done"])], gate.clone());
    let mut daemon = DaemonState::with_model_adapter(model.clone());
    daemon.workspace = Some(WorkspaceConfig {
        root_path: temp.0.clone(),
        company_name: "test".into(),
        mission: "test".into(),
        values: vec![],
    });
    let agent = daemon
        .create_agent(test_config("attachments"))
        .unwrap()
        .state
        .id;
    daemon.sessions.insert(SessionRecord::new(
        &agent,
        "chat:files",
        SessionKind::Chat,
        SessionOrigin::Web,
        "Files".into(),
        TitleSource::FirstMessage,
        1,
    ));
    let state = Arc::new(RwLock::new(daemon));
    let runs = AgentRunCoordinator::new(state.clone(), Arc::new(Semaphore::new(4)))
        .with_document_extractor(Arc::new(RealDocxExtractor));
    let app =
        crate::routes::router_with_runs(state.clone(), DaemonConfig::default(), |_| runs.clone());

    let selected = app
        .clone()
        .oneshot(upload(
            &uri(&agent),
            form(
                "m55-selected.pdf",
                include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/tests/fixtures/documents/m55-selected.pdf"
                )),
                "application/pdf",
            ),
            true,
        ))
        .await
        .unwrap();
    assert_eq!(selected.status(), StatusCode::CREATED);
    let selected_id = json_body(selected).await["attachment"]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let unselected = app
        .clone()
        .oneshot(upload(
            &uri(&agent),
            form(
                "m55-unselected.pdf",
                include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/tests/fixtures/documents/m55-unselected.pdf"
                )),
                "application/pdf",
            ),
            true,
        ))
        .await
        .unwrap();
    assert_eq!(unselected.status(), StatusCode::CREATED);
    let unselected_id = json_body(unselected).await["attachment"]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/agents/{agent}/sessions/chat%3Afiles/runs"))
        .header("host", "127.0.0.1:8080")
        .header("origin", OWNER)
        .header("content-type", "application/json")
        .header("idempotency-key", "m55-selected-pdf")
        .body(Body::from(
            json!({
                "text": "Read the selected PDF",
                "attachmentIds": [selected_id],
            })
            .to_string(),
        ))
        .unwrap();
    let accepted = app.clone().oneshot(request).await.unwrap();
    assert_eq!(accepted.status(), StatusCode::ACCEPTED);
    let run_id = json_body(accepted).await["run"]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    tokio::time::timeout(Duration::from_secs(15), gate.entered())
        .await
        .expect("M55 recording adapter was not reached within the bounded wait");
    let requests = model.requests();
    let user_data = requests
        .iter()
        .flat_map(|request| request.messages.iter())
        .filter(|message| message.role == anima_core::MessageRole::User)
        .map(|message| message.content.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        user_data.contains("M55 selected invoice total: USD 137.42."),
        "selected PDF text must reach model user data: {user_data}"
    );
    assert!(
        user_data.contains("pdf#page=1"),
        "selected PDF text must retain its page locator: {user_data}"
    );
    assert!(user_data.contains("DOCUMENT_DATA_NOT_AUTHORITY_M55"));
    for excluded in ["NEVER_INLINE_PDF_M55_9081", unselected_id.as_str()] {
        assert!(
            !user_data.contains(excluded),
            "unselected PDF content reached model user data: {excluded}"
        );
    }
    for message in requests
        .iter()
        .flat_map(|request| request.messages.iter())
        .filter(|message| message.role == anima_core::MessageRole::System)
    {
        for document_text in [
            "M55 selected invoice total: USD 137.42.",
            "DOCUMENT_DATA_NOT_AUTHORITY_M55",
            "NEVER_INLINE_PDF_M55_9081",
        ] {
            assert!(
                !message.content.text.contains(document_text),
                "PDF text entered system instructions: {document_text}"
            );
        }
    }
    gate.release();
    for _ in 0..500 {
        if state
            .read()
            .await
            .runs
            .get(&run_id)
            .is_some_and(|run| run.status == crate::runs::RunStatus::Completed)
        {
            let response = app
                .clone()
                .oneshot(get(
                    &format!("/api/agents/{agent}/sessions/chat%3Afiles/messages"),
                    true,
                ))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let page = json_body(response).await;
            let selected_message = page["messages"]
                .as_array()
                .unwrap()
                .iter()
                .find(|message| {
                    message["role"] == "user" && message["attachments"][0]["id"] == selected_id
                })
                .expect("selected PDF message must survive run wrap-up");
            let extraction = &selected_message["attachments"][0]["extraction"];
            assert_eq!(extraction["version"], 2);
            assert_eq!(extraction["format"], "pdf");
            assert_eq!(extraction["pageCount"], 2);
            assert_eq!(extraction["textPageCount"], 1);
            assert_eq!(extraction["includedPageCount"], 1);
            assert!(!selected_message
                .to_string()
                .contains("M55 selected invoice total: USD 137.42."));
            assert!(!selected_message
                .to_string()
                .contains("DOCUMENT_DATA_NOT_AUTHORITY_M55"));
            assert!(!selected_message.to_string().contains("documentExtracts"));
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("M55 PDF attachment run did not complete");
}

#[tokio::test]
async fn upload_is_durable_and_serve_is_scoped_authenticated_metadata_only() {
    let (temp, state, agent) = fixture();
    let app = router(state.clone(), DaemonConfig::default());
    let response = app
        .clone()
        .oneshot(upload(
            &uri(&agent),
            form("notes.txt", b"hello", "image/png"),
            true,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let value = json_body(response).await;
    let row = &value["attachment"];
    assert_eq!(row["mime"], "text/plain");
    assert_eq!(row["kind"], "text");
    assert_eq!(row["size"], 5);
    for key in ["workspaceRoot", "sha256", "bytes", "agentId", "sessionId"] {
        assert!(row.get(key).is_none(), "{key}");
    }
    let id = row["id"].as_str().unwrap();
    let path = row["path"].as_str().unwrap();
    assert!(path.starts_with("uploads/"));
    assert_eq!(std::fs::read(temp.0.join(path)).unwrap(), b"hello");
    let store = state.read().await.history.store();
    let private = store.get_attachment(&agent, id).await.unwrap().unwrap();
    assert_eq!(private.path, path);
    assert_eq!(
        private.workspace_root,
        temp.0.canonicalize().unwrap().to_str().unwrap()
    );
    let serve = format!("/api/agents/{agent}/attachments/{id}");
    let response = app.clone().oneshot(get(&serve, true)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "text/plain");
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert!(response.headers()["content-disposition"]
        .to_str()
        .unwrap()
        .starts_with("attachment;"));
    assert_eq!(
        &to_bytes(response.into_body(), 1024).await.unwrap()[..],
        b"hello"
    );
    assert_eq!(
        app.clone()
            .oneshot(get(&serve, false))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        app.clone()
            .oneshot(get(&format!("/api/agents/foreign/attachments/{id}"), true))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    state.write().await.sessions.remove(&agent, "chat:files");
    assert_eq!(
        app.oneshot(get(&serve, true)).await.unwrap().status(),
        StatusCode::NOT_FOUND
    );
}
#[tokio::test]
async fn attachment_auth_precedes_body_and_workspace_access() {
    let (_, state, agent) = fixture();
    state.write().await.workspace = None;
    let app = router(state, DaemonConfig::default());
    let polled = Arc::new(AtomicUsize::new(0));
    let counter = polled.clone();
    let stream = futures::stream::once(async move {
        counter.fetch_add(1, Ordering::SeqCst);
        Ok::<_, std::io::Error>(axum::body::Bytes::from_static(b"malformed"))
    });
    let request = Request::builder()
        .method("POST")
        .uri(uri(&agent))
        .header("host", "127.0.0.1:8080")
        .body(Body::from_stream(stream))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(polled.load(Ordering::SeqCst), 0);
    assert_eq!(
        app.oneshot(upload(
            &uri(&agent),
            form("notes.txt", b"hello", "text/plain"),
            true
        ))
        .await
        .unwrap()
        .status(),
        StatusCode::CONFLICT
    );
}
#[tokio::test]
async fn attachment_upload_refuses_invalid_session_kind_parts_and_content() {
    let (_temp, state, agent) = fixture();
    let app = router(state.clone(), DaemonConfig::default());
    for body in [
        b"bad form".to_vec(),
        b"--m9--\r\n".to_vec(),
        form("", "hello".as_bytes(), "text/plain"),
        form("file.png", b"<svg/>", "image/png"),
        form("file.txt", &[0xff], "text/plain"),
        form("file.html", b"<html/>", "text/html"),
        b"--m9\r\nContent-Disposition: form-data; name=\"text\"\r\n\r\nhello\r\n--m9--\r\n"
            .to_vec(),
    ] {
        assert_eq!(
            app.clone()
                .oneshot(upload(&uri(&agent), body, true))
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
    let mut multiple = form("file.txt", b"hello", "text/plain");
    multiple.truncate(multiple.len() - 8);
    multiple.extend_from_slice(
        b"--m9\r\nContent-Disposition: form-data; name=\"extra\"\r\n\r\nvalue\r\n--m9--\r\n",
    );
    assert_eq!(
        app.clone()
            .oneshot(upload(&uri(&agent), multiple, true))
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        app.clone()
            .oneshot(upload(
                &uri(&agent).replace("chat%3Afiles", "chat%3Amissing"),
                form("a.txt", b"hello", "text/plain"),
                true
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    state
        .write()
        .await
        .sessions
        .get_mut(&agent, "chat:files")
        .unwrap()
        .kind = SessionKind::Helper;
    assert_eq!(
        app.oneshot(upload(
            &uri(&agent),
            form("a.txt", b"hello", "text/plain"),
            true
        ))
        .await
        .unwrap()
        .status(),
        StatusCode::CONFLICT
    );
}
#[tokio::test]
async fn attachment_route_body_and_classified_limits_are_separate() {
    let (_temp, state, agent) = fixture();
    let app = router(state, DaemonConfig::default());
    let text = vec![b'a'; crate::attachments::MAX_TEXT_BYTES + 1];
    assert_eq!(
        app.clone()
            .oneshot(upload(
                &uri(&agent),
                form("a.txt", &text, "text/plain"),
                true
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    let body = vec![0; crate::attachments::MAX_UPLOAD_FORM_BYTES + 1];
    assert_eq!(
        app.clone()
            .oneshot(upload(&uri(&agent), body, true))
            .await
            .unwrap()
            .status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    let mut pdf = b"%PDF-1.7\nobject data".to_vec();
    pdf.resize(crate::attachments::MAX_DOCUMENT_BYTES, b' ');
    let response = app
        .oneshot(upload(
            &uri(&agent),
            form("a.pdf", &pdf, "application/pdf"),
            true,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
}
#[tokio::test]
async fn attachment_storage_and_history_failure_never_return_success() {
    let (temp, state, agent) = fixture();
    let flaky = Arc::new(crate::history::conformance::FlakyHistoryStore::new());
    flaky.set_failing(true);
    state
        .write()
        .await
        .set_history(HistoryService::new(flaky.clone()));
    let app = router(state.clone(), DaemonConfig::default());
    assert_eq!(
        app.clone()
            .oneshot(upload(
                &uri(&agent),
                form("a.txt", b"hello", "text/plain"),
                true
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    flaky.set_failing(false);
    assert!(flaky
        .get_attachment(&agent, "unknown")
        .await
        .unwrap()
        .is_none());
    state.write().await.workspace.as_mut().unwrap().root_path = temp.0.join("absent");
    assert_eq!(
        app.oneshot(upload(
            &uri(&agent),
            form("a.txt", b"hello", "text/plain"),
            true
        ))
        .await
        .unwrap()
        .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
}
#[tokio::test]
async fn attachment_modified_file_and_safe_unicode_disposition() {
    let (temp, state, agent) = fixture();
    let app = router(state, DaemonConfig::default());
    let response = app
        .clone()
        .oneshot(upload(
            &uri(&agent),
            form("界notes.txt", b"hello", "application/json"),
            true,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let row = json_body(response).await;
    let row = &row["attachment"];
    let serve = format!(
        "/api/agents/{agent}/attachments/{}",
        row["id"].as_str().unwrap()
    );
    let response = app.clone().oneshot(get(&serve, true)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let disposition = response.headers()["content-disposition"].to_str().unwrap();
    assert!(disposition.is_ascii());
    assert!(!disposition.contains(['\r', '\n']));
    assert!(disposition.contains("filename*=UTF-8''"));
    std::fs::write(temp.0.join(row["path"].as_str().unwrap()), b"other").unwrap();
    let response = app.clone().oneshot(get(&serve, true)).await.unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let error = json_body(response).await.to_string();
    assert!(!error.contains(temp.0.to_str().unwrap()));
    std::fs::remove_file(temp.0.join(row["path"].as_str().unwrap())).unwrap();
    assert_eq!(
        app.oneshot(get(&serve, true)).await.unwrap().status(),
        StatusCode::CONFLICT
    );
}

struct GateStore {
    inner: MemoryHistoryStore,
    entered: Semaphore,
    release: Semaphore,
    written: StdMutex<Vec<AttachmentRecord>>,
}
impl GateStore {
    fn new() -> Self {
        Self {
            inner: MemoryHistoryStore::new(),
            entered: Semaphore::new(0),
            release: Semaphore::new(0),
            written: StdMutex::new(Vec::new()),
        }
    }
}
#[async_trait]
impl HistoryStore for GateStore {
    fn label(&self) -> &'static str {
        "attachment-test-gate"
    }
    async fn upsert_attachments(
        &self,
        rows: &[crate::attachments::AttachmentRecord],
    ) -> Result<(), HistoryError> {
        self.entered.add_permits(1);
        self.release.acquire().await.unwrap().forget();
        self.inner.upsert_attachments(rows).await?;
        self.written.lock().unwrap().extend_from_slice(rows);
        Ok(())
    }
    async fn get_attachment(
        &self,
        agent_id: &str,
        attachment_id: &str,
    ) -> Result<Option<crate::attachments::AttachmentRecord>, HistoryError> {
        self.inner.get_attachment(agent_id, attachment_id).await
    }
    async fn upsert_messages(&self, messages: &[HistoryMessage]) -> Result<(), HistoryError> {
        self.inner.upsert_messages(messages).await
    }
    async fn upsert_runs(&self, runs: &[RunRecord]) -> Result<(), HistoryError> {
        self.inner.upsert_runs(runs).await
    }
    async fn upsert_approvals(&self, approvals: &[ApprovalRequest]) -> Result<(), HistoryError> {
        self.inner.upsert_approvals(approvals).await
    }
    async fn get_approval(
        &self,
        approval_id: &str,
    ) -> Result<Option<ApprovalRequest>, HistoryError> {
        self.inner.get_approval(approval_id).await
    }
    async fn page_approvals(
        &self,
        query: &ApprovalPageQuery,
    ) -> Result<Vec<ApprovalRequest>, HistoryError> {
        self.inner.page_approvals(query).await
    }
    async fn upsert_schedule_runs(&self, fires: &[ScheduleFireRecord]) -> Result<(), HistoryError> {
        self.inner.upsert_schedule_runs(fires).await
    }
    async fn page_schedule_runs(
        &self,
        agent_id: &str,
        schedule_id: &str,
        limit: usize,
    ) -> Result<Vec<ScheduleFireRecord>, HistoryError> {
        self.inner
            .page_schedule_runs(agent_id, schedule_id, limit)
            .await
    }
    async fn upsert_usage(&self, records: &[UsageRecord]) -> Result<(), HistoryError> {
        self.inner.upsert_usage(records).await
    }
    async fn page_usage(&self, query: &UsagePageQuery) -> Result<Vec<UsageRecord>, HistoryError> {
        self.inner.page_usage(query).await
    }
    async fn existing_message_ids(&self, ids: &[String]) -> Result<HashSet<String>, HistoryError> {
        self.inner.existing_message_ids(ids).await
    }
    async fn get_message(
        &self,
        agent_id: &str,
        session_id: &str,
        message_id: &str,
    ) -> Result<Option<HistoryMessage>, HistoryError> {
        self.inner
            .get_message(agent_id, session_id, message_id)
            .await
    }
    async fn get_run(&self, run_id: &str) -> Result<Option<RunRecord>, HistoryError> {
        self.inner.get_run(run_id).await
    }
    async fn page_messages(
        &self,
        query: &MessagePageQuery,
    ) -> Result<Vec<HistoryMessage>, HistoryError> {
        self.inner.page_messages(query).await
    }
    async fn visible_message_counts(
        &self,
        agent_id: &str,
        session_ids: &[String],
    ) -> Result<HashMap<String, usize>, HistoryError> {
        self.inner
            .visible_message_counts(agent_id, session_ids)
            .await
    }
    async fn search_messages(
        &self,
        agent_ids: &[String],
        query: &str,
        limit: usize,
    ) -> Result<Vec<HistoryMessage>, HistoryError> {
        self.inner.search_messages(agent_ids, query, limit).await
    }
    async fn search_sessions(
        &self,
        agent_ids: &[String],
        query: &str,
        limit: usize,
    ) -> Result<Vec<HistoryMessage>, HistoryError> {
        self.inner.search_sessions(agent_ids, query, limit).await
    }
    async fn delete_session(&self, agent_id: &str, session_id: &str) -> Result<(), HistoryError> {
        self.inner.delete_session(agent_id, session_id).await
    }
    async fn delete_agent(&self, agent_id: &str) -> Result<(), HistoryError> {
        self.inner.delete_agent(agent_id).await
    }
}

#[tokio::test]
async fn dropping_upload_handler_after_spawn_completes_durable_mutation() {
    let (_temp, state, agent) = fixture();
    let store = Arc::new(GateStore::new());
    state
        .write()
        .await
        .set_history(HistoryService::new(store.clone()));
    let runs = AgentRunCoordinator::new(state.clone(), Arc::new(Semaphore::new(4)));
    let app =
        crate::routes::router_with_runs(state.clone(), DaemonConfig::default(), |_| runs.clone());
    let request = upload(&uri(&agent), form("a.txt", b"hello", "text/plain"), true);
    let task = tokio::spawn(app.oneshot(request));
    store.entered.acquire().await.unwrap().forget();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    store.release.add_permits(1);
    let transaction = runs.control_plane_transaction().await;
    let rows = store.written.lock().unwrap().clone();
    assert_eq!(rows.len(), 1);
    assert!(store
        .get_attachment(&agent, &rows[0].id)
        .await
        .unwrap()
        .is_some());
    drop(transaction);
}
#[tokio::test]
async fn attachment_upload_serializes_with_session_deletion_without_resurrection() {
    let (_temp, state, agent) = fixture();
    let store = Arc::new(GateStore::new());
    state
        .write()
        .await
        .set_history(HistoryService::new(store.clone()));
    let runs = AgentRunCoordinator::new(state.clone(), Arc::new(Semaphore::new(4)));
    let app =
        crate::routes::router_with_runs(state.clone(), DaemonConfig::default(), |_| runs.clone());
    let uploading = tokio::spawn(app.clone().oneshot(upload(
        &uri(&agent),
        form("a.txt", b"hello", "text/plain"),
        true,
    )));
    store.entered.acquire().await.unwrap().forget();
    let delete = Request::builder()
        .method("DELETE")
        .uri(format!("/api/agents/{agent}/sessions/chat%3Afiles"))
        .header("host", "127.0.0.1:8080")
        .header("origin", OWNER)
        .body(Body::empty())
        .unwrap();
    let deleting = app.clone().oneshot(delete);
    tokio::pin!(deleting);
    assert!(futures::poll!(deleting.as_mut()).is_pending());
    store.release.add_permits(1);
    assert_eq!(
        uploading.await.unwrap().unwrap().status(),
        StatusCode::CREATED
    );
    assert_eq!(deleting.await.unwrap().status(), StatusCode::OK);
    let rows = store.written.lock().unwrap().clone();
    assert_eq!(rows.len(), 1);
    let serve = format!("/api/agents/{agent}/attachments/{}", rows[0].id);
    assert_eq!(
        app.oneshot(get(&serve, true)).await.unwrap().status(),
        StatusCode::NOT_FOUND
    );
    let history = state.read().await.history.clone();
    history
        .flush_once(&state, &runs.control_plane_transactions(), 1)
        .await
        .unwrap();
    assert!(store
        .get_attachment(&agent, &rows[0].id)
        .await
        .unwrap()
        .is_none());
}
#[tokio::test]
async fn global_api_key_still_protects_attachment_routes() {
    let (_temp, state, agent) = fixture();
    let app = router(state, DaemonConfig::default()).layer(axum::middleware::from_fn_with_state(
        ApiKeyPolicy::for_test(Some("api-key")),
        crate::routes::http::enforce_api_key,
    ));
    for request in [
        upload(&uri(&agent), form("a.txt", b"hello", "text/plain"), true),
        get(&format!("/api/agents/{agent}/attachments/id"), true),
    ] {
        assert_eq!(
            app.clone().oneshot(request).await.unwrap().status(),
            StatusCode::UNAUTHORIZED
        );
    }
}
#[test]
fn session_metadata_projection_preserves_legacy_and_matches_trusted_refs() {
    use anima_core::{Attachment, AttachmentType, DataValue};
    use std::collections::BTreeMap;
    let mut message = crate::sessions::test_support::message(
        "agent",
        "id",
        "chat:files",
        anima_core::MessageRole::User,
        "",
        1,
    );
    let attachment = Attachment {
        attachment_type: AttachmentType::File,
        name: "notes.txt".into(),
        data: "uploads/2026-09-23/notes.txt".into(),
        mime_type: Some("text/plain".into()),
    };
    message.content.attachments = Some(vec![attachment]);
    let entry = crate::sessions::views::PageMessage {
        message: message.clone(),
        hidden: false,
    };
    let legacy = serde_json::to_value(crate::routes::contracts::SessionMessageResponse::from(
        &entry,
    ))
    .unwrap();
    assert_eq!(legacy["attachments"][0]["name"], "notes.txt");
    assert!(legacy["attachments"][0].get("id").is_none());
    let public = BTreeMap::from([
        ("id".into(), DataValue::String("att-test".into())),
        ("name".into(), DataValue::String("notes.txt".into())),
        (
            "path".into(),
            DataValue::String("uploads/2026-09-23/notes.txt".into()),
        ),
        ("mime".into(), DataValue::String("text/plain".into())),
        ("kind".into(), DataValue::String("text".into())),
        ("size".into(), DataValue::Number(5.0)),
        ("createdAtMs".into(), DataValue::Number(100.0)),
    ]);
    message.content.metadata = Some(BTreeMap::from([(
        crate::attachments::ATTACHMENT_RECORDS_KEY.into(),
        DataValue::Array(vec![DataValue::Object(public)]),
    )]));
    let entry = crate::sessions::views::PageMessage {
        message: message.clone(),
        hidden: false,
    };
    let value = serde_json::to_value(crate::routes::contracts::SessionMessageResponse::from(
        &entry,
    ))
    .unwrap();
    assert_eq!(value["attachments"][0]["id"], "att-test");
    assert_eq!(value["attachments"][0]["size"], 5);
    assert_eq!(value["attachments"][0]["mime"], "text/plain");
    assert!(value["metadata"]
        .get(crate::attachments::ATTACHMENT_RECORDS_KEY)
        .is_none());
    message.content.attachments.as_mut().unwrap()[0].data = "uploads/other.txt".into();
    let entry = crate::sessions::views::PageMessage {
        message,
        hidden: false,
    };
    let value = serde_json::to_value(crate::routes::contracts::SessionMessageResponse::from(
        &entry,
    ))
    .unwrap();
    assert!(value["attachments"][0].get("id").is_none());
    for with_retry in [false, true] {
        let request:crate::routes::contracts::TaskRequest=serde_json::from_value(json!({"text":"hello","metadata":{"attachmentRecords":[{"id":"forged"}],"attachmentInputText":"forged","topic":"kept"}})).unwrap();
        let content = if with_retry {
            request.into_domain_with_client_retry_key()
        } else {
            request.into_domain()
        }
        .unwrap();
        let metadata = content.metadata.unwrap();
        assert!(!metadata.contains_key(crate::attachments::ATTACHMENT_RECORDS_KEY));
        assert!(!metadata.contains_key(crate::attachments::ATTACHMENT_INPUT_TEXT_KEY));
        assert!(metadata.contains_key("topic"));
    }
}
#[test]
fn attachment_openapi_routes_and_schema_are_registered() {
    use utoipa::OpenApi;
    let value = serde_json::to_value(crate::routes::ApiDoc::openapi()).unwrap();
    assert!(value["paths"]
        .get("/api/agents/{agent_id}/sessions/{session_id}/attachments")
        .is_some());
    assert!(value["paths"]
        .get("/api/agents/{agent_id}/attachments/{attachment_id}")
        .is_some());
    let schema = &value["components"]["schemas"]["UploadedAttachment"];
    assert!(schema["properties"].get("id").is_some());
    assert!(schema["properties"].get("workspaceRoot").is_none());
    assert!(schema["properties"].get("sha256").is_none());
}

#[test]
fn document_extraction_projection_is_user_selected_only_and_private_data_never_public() {
    use anima_core::document_content::*;
    let mut row = crate::history::conformance::attachment_record("att-doc", "agent", "chat:files");
    row.name = "selected.docx".into();
    row.mime = crate::attachments::documents::DOCX_MIME.into();
    row.kind = crate::attachments::AttachmentKind::Document;
    row.path = "uploads/2026-10-16/selected.docx".into();
    row.sha256 = "a".repeat(64);
    let mut content =
        crate::agent_runs::attachments::attachment_content("original", &[row.clone()], None);
    let snapshot = DocumentSnapshot {
        attachment_id: row.id.clone(),
        source_sha256: row.sha256.clone(),
        extraction: ExtractedDocumentText {
            version: 2,
            format: DocumentFormat::Docx,
            status: DocumentExtractionStatus::Ready,
            blocks: vec![DocumentBlock {
                locator: DocumentLocator::DocxParagraph { paragraph: 1 },
                text: "private extracted literal".into(),
            }],
            omissions: vec![DocumentOmission::MainBodyOnly],
            pdf_coverage: None,
        },
    };
    content.metadata.as_mut().unwrap().insert(
        DOCUMENT_EXTRACTS_METADATA_KEY.into(),
        document_snapshots_metadata(&[snapshot]).unwrap(),
    );
    let mut message = crate::sessions::test_support::message(
        "agent",
        "m",
        "chat:files",
        anima_core::MessageRole::User,
        "",
        1,
    );
    message.content = content.clone();
    let project = |message| {
        serde_json::to_value(crate::routes::contracts::SessionMessageResponse::from(
            &crate::sessions::views::PageMessage {
                message,
                hidden: false,
            },
        ))
        .unwrap()
    };
    let public = project(message.clone());
    assert_eq!(
        public["attachments"][0]["extraction"],
        json!({"version":1,"status":"ready","paragraphCount":1,"omissions":["main_body_only"]})
    );
    let mut legacy_message = message.clone();
    {
        use anima_core::DataValue;
        let DataValue::Array(rows) = legacy_message
            .content
            .metadata
            .as_mut()
            .unwrap()
            .get_mut(DOCUMENT_EXTRACTS_METADATA_KEY)
            .unwrap()
        else {
            panic!()
        };
        let DataValue::Object(snapshot) = &mut rows[0] else {
            panic!()
        };
        let DataValue::Object(extraction) = snapshot.get_mut("extraction").unwrap() else {
            panic!()
        };
        extraction.insert("version".into(), DataValue::Number(1.0));
        extraction.remove("format");
        let DataValue::Array(blocks) = extraction.get_mut("blocks").unwrap() else {
            panic!()
        };
        let DataValue::Object(block) = &mut blocks[0] else {
            panic!()
        };
        block.remove("locator");
        block.insert("paragraph".into(), DataValue::Number(1.0));
    }
    let legacy_bytes = serde_json::to_vec(&legacy_message).unwrap();
    assert_eq!(project(legacy_message.clone()), public);
    assert_eq!(serde_json::to_vec(&legacy_message).unwrap(), legacy_bytes);
    for private in [
        "private extracted literal",
        &row.sha256,
        "documentExtracts",
        "workspaceRoot",
    ] {
        assert!(!public.to_string().contains(private));
    }
    let generic =
        serde_json::to_value(crate::routes::contracts::ContentResponse::from(&content)).unwrap();
    assert!(generic["metadata"]
        .get(DOCUMENT_EXTRACTS_METADATA_KEY)
        .is_none());
    message.role = anima_core::MessageRole::Assistant;
    assert!(project(message.clone())["attachments"][0]
        .get("extraction")
        .is_none());
    message.role = anima_core::MessageRole::User;
    message.content.attachments.as_mut().unwrap()[0].data = "uploads/other.docx".into();
    assert!(project(message)["attachments"][0]
        .get("extraction")
        .is_none());
    for with_retry in [false, true] {
        let request: crate::routes::contracts::TaskRequest = serde_json::from_value(json!({"text":"hello","metadata":{"documentExtracts":[{"forged":true}],"topic":"keep"}})).unwrap();
        let admitted = if with_retry {
            request.into_domain_with_client_retry_key()
        } else {
            request.into_domain()
        }
        .unwrap();
        assert!(!admitted
            .metadata
            .as_ref()
            .unwrap()
            .contains_key(DOCUMENT_EXTRACTS_METADATA_KEY));
        let reserved = std::collections::BTreeMap::from([(
            DOCUMENT_EXTRACTS_METADATA_KEY.into(),
            anima_core::DataValue::String("forged".into()),
        )]);
        let attached = crate::agent_runs::attachments::attachment_content(
            "hello",
            &[row.clone()],
            Some(reserved),
        );
        assert!(!attached
            .metadata
            .unwrap()
            .contains_key(DOCUMENT_EXTRACTS_METADATA_KEY));
    }
}

#[test]
fn pdf_projection_is_closed_has_honest_coverage_and_refuses_mismatched_format() {
    use anima_core::document_content::*;
    let mut row = crate::history::conformance::attachment_record("att-pdf", "agent", "chat:files");
    row.name = "selected.pdf".into();
    row.mime = crate::attachments::documents::PDF_MIME.into();
    row.kind = crate::attachments::AttachmentKind::Document;
    row.path = "uploads/2026-10-16/selected.pdf".into();
    row.sha256 = "b".repeat(64);
    let project = |extraction: ExtractedDocumentText| {
        let mut message = crate::sessions::test_support::message(
            "agent",
            "m",
            "chat:files",
            anima_core::MessageRole::User,
            "",
            1,
        );
        message.content =
            crate::agent_runs::attachments::attachment_content("original", &[row.clone()], None);
        message.content.metadata.as_mut().unwrap().insert(
            DOCUMENT_EXTRACTS_METADATA_KEY.into(),
            document_snapshots_metadata(&[DocumentSnapshot {
                attachment_id: row.id.clone(),
                source_sha256: row.sha256.clone(),
                extraction,
            }])
            .unwrap(),
        );
        serde_json::to_value(crate::routes::contracts::SessionMessageResponse::from(
            &crate::sessions::views::PageMessage {
                message,
                hidden: false,
            },
        ))
        .unwrap()
    };
    for status in [
        DocumentExtractionStatus::Ready,
        DocumentExtractionStatus::Partial,
    ] {
        let public = project(ExtractedDocumentText {
            version: 2,
            format: DocumentFormat::Pdf,
            status,
            blocks: vec![DocumentBlock {
                locator: DocumentLocator::PdfPage { page: 1 },
                text: "private PDF extracted literal".into(),
            }],
            omissions: vec![DocumentOmission::TextLayerOnly],
            pdf_coverage: Some(PdfCoverage {
                page_count: 2,
                text_page_count: 1,
                included_page_count: 1,
            }),
        });
        assert_eq!(
            public["attachments"][0]["extraction"],
            json!({
                "version": 2, "format": "pdf", "status": serde_json::to_value(status).unwrap(),
                "pageCount": 2, "textPageCount": 1, "includedPageCount": 1, "omissions": ["text_layer_only"],
            })
        );
        for private in [
            "private PDF extracted literal",
            row.sha256.as_str(),
            "documentExtracts",
            "pdf_page",
            "locator",
            "paragraphCount",
        ] {
            assert!(!public.to_string().contains(private), "{private}");
        }
    }
    for status in [
        DocumentExtractionStatus::Malformed,
        DocumentExtractionStatus::Unavailable,
        DocumentExtractionStatus::Cancelled,
    ] {
        let public = project(ExtractedDocumentText::empty_for(
            DocumentFormat::Pdf,
            status,
        ));
        assert_eq!(
            public["attachments"][0]["extraction"],
            json!({
                "version": 2, "format": "pdf", "status": serde_json::to_value(status).unwrap(),
                "pageCount": null, "textPageCount": null, "includedPageCount": null, "omissions": [],
            })
        );
    }
    let mut no_text =
        ExtractedDocumentText::empty_for(DocumentFormat::Pdf, DocumentExtractionStatus::NoText);
    no_text.pdf_coverage = Some(PdfCoverage {
        page_count: 2,
        text_page_count: 0,
        included_page_count: 0,
    });
    no_text.omissions.push(DocumentOmission::TextLayerOnly);
    assert_eq!(
        project(no_text)["attachments"][0]["extraction"],
        json!({
            "version": 2, "format": "pdf", "status": "no_text", "pageCount": 2,
            "textPageCount": 0, "includedPageCount": 0, "omissions": ["text_layer_only"],
        })
    );
    let mismatched = project(ExtractedDocumentText::empty(
        DocumentExtractionStatus::Unavailable,
    ));
    assert!(mismatched["attachments"][0].get("extraction").is_none());
}
#[tokio::test]
async fn image_magic_owns_mime_and_collision_name_is_server_approved() {
    use base64::Engine;
    let (_temp, state, agent) = fixture();
    let app = router(state, DaemonConfig::default());
    let png=base64::engine::general_purpose::STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMB/6X6p2kAAAAASUVORK5CYII=").unwrap();
    let mut rows = Vec::new();
    for _ in 0..2 {
        let response = app
            .clone()
            .oneshot(upload(
                &uri(&agent),
                form("../../CON.png", &png, "text/html"),
                true,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let response = json_body(response).await;
        rows.push(response["attachment"].clone());
    }
    assert_eq!(rows[0]["name"], "attachment.png");
    assert_eq!(rows[1]["name"], "attachment-1.png");
    assert_ne!(rows[0]["path"], rows[1]["path"]);
    assert_eq!(rows[0]["mime"], "image/png");
    let response = app
        .oneshot(get(
            &format!(
                "/api/agents/{agent}/attachments/{}",
                rows[0]["id"].as_str().unwrap()
            ),
            true,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "image/png");
    assert!(response.headers()["content-disposition"]
        .to_str()
        .unwrap()
        .starts_with("inline;"));
    assert_eq!(
        &to_bytes(response.into_body(), 1024).await.unwrap()[..],
        png.as_slice()
    );
}

#[tokio::test]
async fn selected_wav_upload_is_owned_and_downloadable_m56() {
    let (_temp, state, agent) = fixture();
    let app = router(state, DaemonConfig::default());
    let wav = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/audio/m56-selected.wav"
    ));
    let response = app
        .clone()
        .oneshot(upload(
            &uri(&agent),
            form("selected.wav", wav, "audio/wav"),
            true,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let value = json_body(response).await;
    let row = &value["attachment"];
    assert_eq!(row["mime"], "audio/wav");
    assert_eq!(row["kind"], "audio");
    for key in ["workspaceRoot", "sha256", "bytes", "agentId", "sessionId"] {
        assert!(row.get(key).is_none(), "{key}");
    }
    let id = row["id"].as_str().unwrap();
    let serve = format!("/api/agents/{agent}/attachments/{id}");
    let response = app.clone().oneshot(get(&serve, true)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "audio/wav");
    assert_eq!(
        &to_bytes(response.into_body(), usize::MAX).await.unwrap()[..],
        wav.as_slice()
    );
    assert_eq!(
        app.oneshot(get(&serve, false)).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
}
