use super::*;
use crate::{
    attachments::files::save_upload,
    sessions::{SessionRecord, TitleSource},
    state::DaemonState,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::{RwLock, Semaphore};

pub(crate) const WAV: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/audio/m56-selected.wav"
));
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("anima-voice-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct FakePort {
    calls: AtomicUsize,
    entered: Arc<Semaphore>,
    release: Arc<Semaphore>,
    text: String,
}
impl FakePort {
    fn new(permits: usize) -> Arc<Self> {
        Arc::new(Self {
            calls: AtomicUsize::new(0),
            entered: Arc::new(Semaphore::new(0)),
            release: Arc::new(Semaphore::new(permits)),
            text: "  a private draft  \n".into(),
        })
    }
    async fn entered(&self) {
        tokio::time::timeout(Duration::from_secs(2), self.entered.acquire())
            .await
            .unwrap()
            .unwrap()
            .forget();
    }
}
#[async_trait::async_trait]
impl TranscriptionPort for FakePort {
    fn availability(&self) -> TranscriptionAvailability {
        TranscriptionAvailability::Available
    }
    async fn transcribe(
        &self,
        audio: WavAudio,
        _cancel: CancelSignal,
        _deadline: Instant,
    ) -> Result<String, TranscriptionError> {
        assert_eq!(audio.bytes(), WAV);
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.entered.add_permits(1);
        self.release.acquire().await.unwrap().forget();
        Ok(self.text.clone())
    }
}

fn config() -> anima_core::AgentConfig {
    anima_core::AgentConfig {
        name: "voice-test".into(),
        model: "deterministic".into(),
        provider: None,
        bio: None,
        lore: None,
        knowledge: None,
        topics: None,
        adjectives: None,
        style: None,
        system: None,
        tools: None,
        plugins: None,
        settings: None,
    }
}
async fn fixture() -> (Temp, SharedDaemonState, String, AttachmentRecord) {
    let temp = Temp::new();
    let mut daemon = DaemonState::new();
    daemon.workspace = Some(crate::control_plane_store::WorkspaceConfig {
        root_path: temp.0.clone(),
        company_name: "test".into(),
        mission: "test".into(),
        values: vec![],
    });
    let agent = daemon.create_agent(config()).unwrap().state.id;
    daemon.sessions.insert(SessionRecord::new(
        &agent,
        "chat:voice",
        SessionKind::Chat,
        SessionOrigin::Web,
        "Voice".into(),
        TitleSource::FirstMessage,
        1,
    ));
    let saved = save_upload(&temp.0, "2026-10-16", "speech.wav", WAV).unwrap();
    let record = AttachmentRecord {
        id: "att-voice".into(),
        agent_id: agent.clone(),
        session_id: "chat:voice".into(),
        name: "speech.wav".into(),
        mime: "audio/wav".into(),
        size: WAV.len() as u64,
        kind: AttachmentKind::Audio,
        path: saved.path,
        created_at_ms: 1,
        workspace_root: saved.workspace_root,
        sha256: saved.sha256,
    };
    daemon
        .history
        .store()
        .upsert_attachments(std::slice::from_ref(&record))
        .await
        .unwrap();
    (temp, Arc::new(RwLock::new(daemon)), agent, record)
}
fn service(port: Arc<FakePort>) -> VoiceService {
    VoiceService::configured(TranscriptionAvailability::Available, Some(port))
}
fn spawn(
    service: &VoiceService,
    state: &SharedDaemonState,
    agent: &str,
    operation: uuid::Uuid,
) -> tokio::task::JoinHandle<Result<String, VoiceError>> {
    let service = service.clone();
    let state = state.clone();
    let agent = agent.to_string();
    tokio::spawn(async move {
        service
            .transcribe(
                state,
                agent,
                "chat:voice".into(),
                operation,
                "att-voice".into(),
            )
            .await
    })
}
async fn terminal(service: &VoiceService) {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if service
                .operations
                .lock()
                .unwrap()
                .values()
                .all(|entry| entry.terminal.is_some())
            {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn explicit_call_returns_trimmed_draft_once_without_persistence_or_replay() {
    let (_temp, state, agent, _) = fixture().await;
    let port = FakePort::new(2);
    let service = service(port.clone());
    let op = uuid::Uuid::new_v4();
    assert_eq!(
        spawn(&service, &state, &agent, op).await.unwrap().unwrap(),
        "a private draft"
    );
    assert_eq!(
        spawn(&service, &state, &agent, op).await.unwrap(),
        Err(VoiceError::Duplicate)
    );
    assert_eq!(port.calls.load(Ordering::SeqCst), 1);
    assert!(state
        .read()
        .await
        .get_agent(&agent)
        .unwrap()
        .messages
        .is_empty());
    let restarted =
        VoiceService::configured(TranscriptionAvailability::Available, Some(port.clone()));
    assert!(restarted.operations.lock().unwrap().is_empty());
    assert_eq!(
        port.calls.load(Ordering::SeqCst),
        1,
        "construction never replays"
    );
    assert_eq!(
        spawn(&restarted, &state, &agent, op)
            .await
            .unwrap()
            .unwrap(),
        "a private draft"
    );
}

#[tokio::test]
async fn cancellation_before_admission_is_scoped_idempotent_and_never_calls_provider() {
    let (_temp, state, agent, _) = fixture().await;
    let port = FakePort::new(1);
    let service = service(port.clone());
    let op = uuid::Uuid::new_v4();
    service.cancel(&agent, "chat:voice", op).unwrap();
    service.cancel(&agent, "chat:voice", op).unwrap();
    assert_eq!(
        spawn(&service, &state, &agent, op).await.unwrap(),
        Err(VoiceError::Duplicate)
    );
    assert_eq!(port.calls.load(Ordering::SeqCst), 0);
    let other = service
        .admit(VoiceService::key(&agent, "chat:other", op))
        .unwrap();
    drop(other);
}

#[tokio::test]
async fn duplicate_busy_and_cancelled_late_results_never_publish() {
    let (_temp, state, agent, _) = fixture().await;
    let port = FakePort::new(0);
    let service = service(port.clone());
    let one = uuid::Uuid::new_v4();
    let two = uuid::Uuid::new_v4();
    let first = spawn(&service, &state, &agent, one);
    port.entered().await;
    let second = spawn(&service, &state, &agent, two);
    port.entered().await;
    assert_eq!(
        spawn(&service, &state, &agent, one).await.unwrap(),
        Err(VoiceError::Duplicate)
    );
    assert_eq!(
        spawn(&service, &state, &agent, uuid::Uuid::new_v4())
            .await
            .unwrap(),
        Err(VoiceError::Busy)
    );
    service.cancel(&agent, "chat:voice", one).unwrap();
    assert_eq!(first.await.unwrap(), Err(VoiceError::Cancelled));
    port.release.add_permits(2);
    assert_eq!(second.await.unwrap().unwrap(), "a private draft");
    assert_eq!(port.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn dropping_http_owner_signals_owned_task_and_retains_duplicate_tombstone() {
    let (_temp, state, agent, _) = fixture().await;
    let port = FakePort::new(0);
    let service = service(port.clone());
    let op = uuid::Uuid::new_v4();
    let request = spawn(&service, &state, &agent, op);
    port.entered().await;
    request.abort();
    let _ = request.await;
    terminal(&service).await;
    let map = service.operations.lock().unwrap();
    let entry = map
        .get(&VoiceService::key(&agent, "chat:voice", op))
        .unwrap();
    assert!(entry.cancel.is_cancelled());
    assert!(entry.terminal.is_some());
    drop(map);
    assert_eq!(
        spawn(&service, &state, &agent, op).await.unwrap(),
        Err(VoiceError::Duplicate)
    );
    assert_eq!(port.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn absolute_deadline_rejects_noncooperative_provider_result() {
    let (_temp, state, agent, _) = fixture().await;
    let port = FakePort::new(0);
    let mut service = service(port.clone());
    service.budget = Duration::from_millis(80);
    let task = spawn(&service, &state, &agent, uuid::Uuid::new_v4());
    port.entered().await;
    assert_eq!(task.await.unwrap(), Err(VoiceError::TimedOut));
    port.release.add_permits(1);
    terminal(&service).await;
    assert!(state
        .read()
        .await
        .get_agent(&agent)
        .unwrap()
        .messages
        .is_empty());
}

#[tokio::test]
async fn host_revalidates_provider_text_and_returns_only_static_failure() {
    for text in [
        " ".to_string(),
        "private\0body".to_string(),
        "x".repeat(anima_core::audio_content::MAX_TRANSCRIPT_BYTES + 1),
    ] {
        let (_temp, state, agent, _) = fixture().await;
        let mut port = FakePort::new(1);
        Arc::get_mut(&mut port).unwrap().text = text;
        assert_eq!(
            spawn(&service(port), &state, &agent, uuid::Uuid::new_v4())
                .await
                .unwrap(),
            Err(VoiceError::Failed)
        );
    }
}

#[tokio::test]
async fn changed_deleted_or_rebound_source_during_provider_await_is_rejected() {
    for change in [
        "bytes",
        "file-delete",
        "record",
        "record-delete",
        "workspace",
        "session",
    ] {
        let (temp, state, agent, mut record) = fixture().await;
        let port = FakePort::new(0);
        let service = service(port.clone());
        let task = spawn(&service, &state, &agent, uuid::Uuid::new_v4());
        port.entered().await;
        match change {
            "bytes" => {
                let mut bytes = WAV.to_vec();
                *bytes.last_mut().unwrap() ^= 1;
                std::fs::write(temp.0.join(&record.path), bytes).unwrap();
            }
            "file-delete" => std::fs::remove_file(temp.0.join(&record.path)).unwrap(),
            "record" => {
                let original = record.clone();
                record.sha256 = "different".into();
                let store = state.read().await.history.store();
                assert!(
                    store
                        .upsert_attachments(std::slice::from_ref(&record))
                        .await
                        .is_err(),
                    "durable attachment identity cannot be overwritten"
                );
                assert_eq!(
                    store.get_attachment(&agent, &original.id).await.unwrap(),
                    Some(original)
                );
                store.delete_session(&agent, "chat:voice").await.unwrap();
                store.upsert_attachments(&[record]).await.unwrap();
            }
            "record-delete" => {
                state
                    .read()
                    .await
                    .history
                    .store()
                    .delete_session(&agent, "chat:voice")
                    .await
                    .unwrap();
            }
            "workspace" => {
                state.write().await.workspace.as_mut().unwrap().root_path = temp.0.join("other");
            }
            "session" => {
                state.write().await.sessions.remove(&agent, "chat:voice");
            }
            _ => unreachable!(),
        }
        port.release.add_permits(1);
        assert_eq!(task.await.unwrap(), Err(VoiceError::NotFound), "{change}");
        assert_eq!(port.calls.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn wrong_scope_changed_hash_disabled_and_non_audio_never_call_provider() {
    for change in ["session", "agent", "hash", "not-audio", "disabled"] {
        let (_temp, state, agent, mut record) = fixture().await;
        let port = FakePort::new(1);
        let active = service(port.clone());
        let service = if change == "disabled" {
            VoiceService::disabled()
        } else {
            active
        };
        match change {
            "session" => record.session_id = "chat:other".into(),
            "agent" => record.agent_id = "other-agent".into(),
            "hash" => record.sha256 = "changed".into(),
            "not-audio" => record.kind = AttachmentKind::Text,
            _ => (),
        }
        state
            .read()
            .await
            .history
            .store()
            .delete_session(&agent, "chat:voice")
            .await
            .unwrap();
        state
            .read()
            .await
            .history
            .store()
            .upsert_attachments(&[record])
            .await
            .unwrap();
        assert!(
            spawn(&service, &state, &agent, uuid::Uuid::new_v4())
                .await
                .unwrap()
                .is_err(),
            "{change}"
        );
        assert_eq!(port.calls.load(Ordering::SeqCst), 0, "{change}");
    }
}

#[test]
fn operation_map_is_bounded_active_entries_never_evicted_and_terminal_expiry_is_finite() {
    let service = VoiceService::disabled();
    let one = service
        .admit(VoiceService::key("a", "s", uuid::Uuid::new_v4()))
        .unwrap();
    let two = service
        .admit(VoiceService::key("b", "s", uuid::Uuid::new_v4()))
        .unwrap();
    for _ in 0..200 {
        service.cancel("a", "s", uuid::Uuid::new_v4()).unwrap();
    }
    let mut map = service.operations.lock().unwrap();
    assert_eq!(map.len(), MAX_OPERATIONS);
    assert_eq!(
        map.values().filter(|e| e.terminal.is_none()).count(),
        MAX_ACTIVE
    );
    assert!(map.contains_key(&one.key));
    assert!(map.contains_key(&two.key));
    for entry in map.values_mut().filter(|e| e.terminal.is_some()) {
        entry.terminal = Some(Instant::now() - TOMBSTONE_TTL);
    }
    prune(&mut map, Instant::now());
    assert_eq!(map.len(), MAX_ACTIVE);
    drop(map);
    drop(one);
    drop(two);
    assert!(service
        .operations
        .lock()
        .unwrap()
        .values()
        .all(|e| e.terminal.is_some()));
}

#[test]
fn ordinary_audio_content_is_a_file_reference_with_no_document_snapshot() {
    let row = AttachmentRecord {
        id: "att-audio".into(),
        agent_id: "agent".into(),
        session_id: "chat:voice".into(),
        name: "voice.wav".into(),
        mime: "audio/wav".into(),
        size: WAV.len() as u64,
        kind: AttachmentKind::Audio,
        path: "uploads/2026-10-16/voice.wav".into(),
        created_at_ms: 1,
        workspace_root: "private".into(),
        sha256: "private-hash".into(),
    };
    let content = crate::agent_runs::attachments::attachment_content("hello", &[row], None);
    assert!(content.text.contains("audio has not been transcribed"));
    assert_eq!(
        content.attachments.as_ref().unwrap()[0].attachment_type,
        anima_core::AttachmentType::File
    );
    assert!(!content
        .metadata
        .as_ref()
        .unwrap()
        .contains_key(anima_core::document_content::DOCUMENT_EXTRACTS_METADATA_KEY));
}

#[tokio::test]
async fn owner_routes_validate_consent_closed_body_scope_and_expose_only_safe_capability() {
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;
    let (_temp, state, agent, _) = fixture().await;
    let port = FakePort::new(1);
    let service = service(port.clone());
    let app = crate::app::app_with_state_and_voice(
        state.clone(),
        crate::app::DaemonConfig::default(),
        service,
    );
    let prefix = format!("/api/agents/{agent}/sessions/chat%3Avoice/voice");
    let request = |method: &str, path: &str, body: String, owner: bool| {
        let builder = Request::builder()
            .method(method)
            .uri(path)
            .header("host", "127.0.0.1:8080")
            .header("content-type", "application/json");
        let builder = if owner {
            builder.header("origin", "http://localhost:4200")
        } else {
            builder
        };
        builder.body(Body::from(body)).unwrap()
    };
    let capability = app
        .clone()
        .oneshot(request("GET", &prefix, String::new(), true))
        .await
        .unwrap();
    assert_eq!(capability.status(), StatusCode::OK);
    let value: serde_json::Value =
        serde_json::from_slice(&to_bytes(capability.into_body(), 4096).await.unwrap()).unwrap();
    assert_eq!(
        value,
        serde_json::json!({"version":1,"provider":"openai","model":"whisper-1","available":true,"reason":"available"})
    );
    let post = format!("{prefix}/transcriptions");
    let valid = serde_json::json!({"operationId":uuid::Uuid::new_v4().to_string(),"attachmentId":"att-voice","provider":"openai","consent":true});
    for field in ["url", "path", "key", "model", "language", "prompt"] {
        let mut invalid = valid.clone();
        invalid[field] = serde_json::json!("forbidden");
        assert_eq!(
            app.clone()
                .oneshot(request("POST", &post, invalid.to_string(), true))
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
    let mut no_consent = valid.clone();
    no_consent["consent"] = serde_json::json!(false);
    assert_eq!(
        app.clone()
            .oneshot(request("POST", &post, no_consent.to_string(), true))
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        app.clone()
            .oneshot(request("POST", &post, valid.to_string(), false))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    let mut wrong_provider = valid.clone();
    wrong_provider["provider"] = serde_json::json!("chatgpt");
    assert_eq!(
        app.clone()
            .oneshot(request("POST", &post, wrong_provider.to_string(), true))
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(port.calls.load(Ordering::SeqCst), 0);
    let response = app
        .clone()
        .oneshot(request("POST", &post, valid.to_string(), true))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let text = String::from_utf8(
        to_bytes(response.into_body(), 32 * 1024)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(text.contains("a private draft"));
    assert!(!text.contains("workspaceRoot"));
    assert!(!text.contains("sha256"));
    assert_eq!(port.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        app.oneshot(request("POST", &post, valid.to_string(), true))
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
}

#[tokio::test]
async fn upload_and_ordinary_chat_with_audio_make_zero_transcription_calls() {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;
    let (_temp, state, agent, _) = fixture().await;
    let port = FakePort::new(1);
    let app = crate::app::app_with_state_and_voice(
        state.clone(),
        crate::app::DaemonConfig::default(),
        service(port.clone()),
    );
    let mut form = b"--voice\r\nContent-Disposition: form-data; name=\"file\"; filename=\"another.WAV\"\r\nContent-Type: application/octet-stream\r\n\r\n".to_vec();
    form.extend_from_slice(WAV);
    form.extend_from_slice(b"\r\n--voice--\r\n");
    let upload = Request::builder()
        .method("POST")
        .uri(format!(
            "/api/agents/{agent}/sessions/chat%3Avoice/attachments"
        ))
        .header("host", "127.0.0.1:8080")
        .header("origin", "http://localhost:4200")
        .header("content-type", "multipart/form-data; boundary=voice")
        .body(Body::from(form))
        .unwrap();
    assert_eq!(
        app.clone().oneshot(upload).await.unwrap().status(),
        StatusCode::CREATED
    );
    let chat = Request::builder()
        .method("POST")
        .uri(format!("/api/agents/{agent}/sessions/chat%3Avoice/runs"))
        .header("host", "127.0.0.1:8080")
        .header("origin", "http://localhost:4200")
        .header("content-type", "application/json")
        .header("idempotency-key", "voice-ordinary-chat")
        .body(Body::from(
            r#"{"text":"hello","attachmentIds":["att-voice"]}"#,
        ))
        .unwrap();
    assert_eq!(
        app.oneshot(chat).await.unwrap().status(),
        StatusCode::ACCEPTED
    );
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if state
                .read()
                .await
                .get_agent(&agent)
                .unwrap()
                .messages
                .iter()
                .any(|m| m.role == anima_core::MessageRole::Assistant)
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(port.calls.load(Ordering::SeqCst), 0);
    assert!(state
        .read()
        .await
        .get_agent(&agent)
        .unwrap()
        .messages
        .iter()
        .any(|m| m.content.text.contains("audio has not been transcribed")));
}

#[tokio::test]
async fn source_reads_share_the_absolute_deadline_and_retain_their_physical_owner() {
    for slow_read in [0, 1] {
        let (_temp, state, agent, _) = fixture().await;
        let port = FakePort::new(1);
        let mut service = service(port.clone());
        service.budget = Duration::from_millis(80);
        let reads = Arc::new(AtomicUsize::new(0));
        let finished = Arc::new(AtomicUsize::new(0));
        let reader_reads = reads.clone();
        let reader_finished = finished.clone();
        service.reader = Arc::new(move |root, row| {
            let call = reader_reads.fetch_add(1, Ordering::SeqCst);
            if call == slow_read {
                std::thread::sleep(Duration::from_millis(200));
            }
            let result = read_attachment_bytes(root, row);
            reader_finished.fetch_add(1, Ordering::SeqCst);
            result
        });
        assert_eq!(
            spawn(&service, &state, &agent, uuid::Uuid::new_v4())
                .await
                .unwrap(),
            Err(VoiceError::TimedOut)
        );
        assert_eq!(reads.load(Ordering::SeqCst), slow_read + 1);
        assert_eq!(
            finished.load(Ordering::SeqCst),
            slow_read,
            "blocking read is retained after cancelled await"
        );
        assert_eq!(
            port.calls.load(Ordering::SeqCst),
            slow_read,
            "pre-read deadline prevents any provider call"
        );
        tokio::time::timeout(Duration::from_secs(2), async {
            while finished.load(Ordering::SeqCst) != slow_read + 1 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn voice_route_outlives_ordinary_request_timeout_and_delete_fences_late_result() {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;
    let (_temp, state, agent, _) = fixture().await;
    let port = FakePort::new(0);
    let service = service(port.clone());
    let app = crate::app::app_with_state_and_voice(
        state,
        crate::app::DaemonConfig {
            request_timeout: Duration::from_millis(1),
            ..Default::default()
        },
        service,
    );
    let op = uuid::Uuid::new_v4();
    let prefix = format!("/api/agents/{agent}/sessions/chat%3Avoice/voice/transcriptions");
    let request = |method: &str, path: String, body: String| {
        Request::builder()
            .method(method)
            .uri(path)
            .header("host", "127.0.0.1:8080")
            .header("origin", "http://localhost:4200")
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap()
    };
    let post = request("POST", prefix.clone(), serde_json::json!({"operationId":op.to_string(),"attachmentId":"att-voice","provider":"openai","consent":true}).to_string());
    let pending = {
        let app = app.clone();
        tokio::spawn(async move { app.oneshot(post).await.unwrap() })
    };
    port.entered().await;
    tokio::time::sleep(Duration::from_millis(10)).await;
    assert!(
        !pending.is_finished(),
        "ordinary1ms timeout must not cut the45s voice operation"
    );
    assert_eq!(
        app.clone()
            .oneshot(request("DELETE", format!("{prefix}/{op}"), String::new()))
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(pending.await.unwrap().status(), StatusCode::CONFLICT);
    port.release.add_permits(1);
    assert_eq!(
        app.oneshot(request("DELETE", format!("{prefix}/{op}"), String::new()))
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn m56_review_regression_authenticated_pending_body_has_finite_outer_timeout() {
    use axum::{
        body::{Body, Bytes},
        http::{header, Request, StatusCode},
    };
    use tower::ServiceExt;
    let (_temp, state, agent, _) = fixture().await;
    let port = FakePort::new(0);
    let mut service = service(port.clone());
    service.budget = Duration::from_millis(5);
    let inspect = service.clone();
    let app =
        crate::app::app_with_state_and_voice(state, crate::app::DaemonConfig::default(), service);
    let request = Request::builder()
        .method("POST")
        .uri(format!(
            "/api/agents/{agent}/sessions/chat%3Avoice/voice/transcriptions"
        ))
        .header("host", "127.0.0.1:8080")
        .header("origin", "http://localhost:4200")
        .header("content-type", "application/json")
        .body(Body::from_stream(futures::stream::pending::<
            Result<Bytes, std::io::Error>,
        >()))
        .unwrap();
    let response = tokio::time::timeout(Duration::from_secs(2), app.oneshot(request))
        .await
        .expect("authenticated pending body must have a finite voice route boundary")
        .unwrap();
    assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
    assert_eq!(
        response.headers().get(header::CACHE_CONTROL).unwrap(),
        "no-store"
    );
    assert_eq!(port.calls.load(Ordering::SeqCst), 0);
    assert!(inspect.operations.lock().unwrap().is_empty());
}

#[tokio::test]
async fn m56_review_regression_fresh_wav_uploads_preserve_closed_validation_statuses() {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;
    let (_temp, state, agent, _) = fixture().await;
    let port = FakePort::new(1);
    let service = service(port.clone());
    let inspect = service.clone();
    let app =
        crate::app::app_with_state_and_voice(state, crate::app::DaemonConfig::default(), service);
    let mut malformed = WAV.to_vec();
    malformed[0] = b'X';
    let mut unsupported = WAV.to_vec();
    unsupported[20..22].copy_from_slice(&3u16.to_le_bytes());
    let data_size = 8_000u32 * 2 * 120 + 16;
    let mut too_long = b"RIFF".to_vec();
    too_long.extend_from_slice(&(data_size + 36).to_le_bytes());
    too_long.extend_from_slice(b"WAVEfmt ");
    too_long.extend_from_slice(&16u32.to_le_bytes());
    too_long.extend_from_slice(&1u16.to_le_bytes());
    too_long.extend_from_slice(&1u16.to_le_bytes());
    too_long.extend_from_slice(&8_000u32.to_le_bytes());
    too_long.extend_from_slice(&16_000u32.to_le_bytes());
    too_long.extend_from_slice(&2u16.to_le_bytes());
    too_long.extend_from_slice(&16u16.to_le_bytes());
    too_long.extend_from_slice(b"data");
    too_long.extend_from_slice(&data_size.to_le_bytes());
    too_long.resize(44 + data_size as usize, 0);
    for (name, bytes, expected) in [
        ("malformed.wav", malformed, StatusCode::UNPROCESSABLE_ENTITY),
        (
            "unsupported.wav",
            unsupported,
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
        ),
        ("long.wav", too_long, StatusCode::UNPROCESSABLE_ENTITY),
        (
            "large.wav",
            vec![0; anima_core::audio_content::MAX_AUDIO_INPUT_BYTES + 1],
            StatusCode::PAYLOAD_TOO_LARGE,
        ),
        ("invalid.txt", vec![0], StatusCode::BAD_REQUEST),
    ] {
        let mut form = format!("--review\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\nContent-Type: application/octet-stream\r\n\r\n").into_bytes();
        form.extend_from_slice(&bytes);
        form.extend_from_slice(b"\r\n--review--\r\n");
        let request = Request::builder()
            .method("POST")
            .uri(format!(
                "/api/agents/{agent}/sessions/chat%3Avoice/attachments"
            ))
            .header("host", "127.0.0.1:8080")
            .header("origin", "http://localhost:4200")
            .header("content-type", "multipart/form-data; boundary=review")
            .body(Body::from(form))
            .unwrap();
        assert_eq!(
            app.clone().oneshot(request).await.unwrap().status(),
            expected,
            "{name}"
        );
    }
    assert_eq!(port.calls.load(Ordering::SeqCst), 0);
    assert!(inspect.operations.lock().unwrap().is_empty());
}
