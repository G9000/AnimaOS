use super::*;
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
