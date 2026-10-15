use super::*;
use crate::{
    history::conformance::history_message,
    sessions::{SessionKind, SessionOrigin, SessionRecord, TitleSource},
    state::DaemonState,
};
use anima_core::MessageRole;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::{RwLock, Semaphore};

#[tokio::test]
async fn predispatch_store_await_releases_daemon_lock_and_refuses_changed_source_without_provider()
{
    use crate::history::conformance::FlakyHistoryStore;
    for change in ["record", "session", "history"] {
        let (state, agent) = fixture().await;
        let mut row = state
            .read()
            .await
            .history
            .store()
            .get_message(&agent, "chat:speech", "reply-1")
            .await
            .unwrap()
            .unwrap();
        let store = Arc::new(FlakyHistoryStore::new());
        store
            .upsert_messages(std::slice::from_ref(&row))
            .await
            .unwrap();
        state
            .write()
            .await
            .set_history(crate::history::HistoryService::new(store.clone()));
        // The initial read succeeds; the actual predispatch get_message waits
        // before reading its current row. No synthetic source-reader shortcut.
        let gate = store.hold_message_read_after(1);
        let port = FakePort::new(1);
        let service = service(port.clone());
        let pending = spawn(&service, &state, &agent, uuid::Uuid::new_v4());
        tokio::time::timeout(Duration::from_secs(2), gate.entered.acquire())
            .await
            .unwrap()
            .unwrap()
            .forget();
        assert_eq!(port.calls.load(Ordering::SeqCst), 0);
        let mut guard = state
            .try_write()
            .expect("speech must release daemon lock across history IO");
        match change {
            "session" => {
                guard.sessions.remove(&agent, "chat:speech");
                guard.sessions.insert(SessionRecord::new(
                    &agent,
                    "chat:speech",
                    SessionKind::Chat,
                    SessionOrigin::Web,
                    "Speech".into(),
                    TitleSource::FirstMessage,
                    2,
                ));
            }
            "history" => {
                // Keep the same store: replacing HistoryService itself must
                // invalidate the selection even when all saved rows match.
                guard.set_history(crate::history::HistoryService::new(store.clone()));
            }
            "record" => {}
            _ => unreachable!(),
        }
        drop(guard);
        if change == "record" {
            // Text/ids/role stay unchanged; full saved-record equality matters.
            row.message.created_at_ms += 1;
            store.upsert_messages(&[row]).await.unwrap();
        }
        gate.release.add_permits(1);
        let result = tokio::time::timeout(Duration::from_secs(2), pending)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(result, Err(ServiceError::NotFound)), "{change}");
        assert_eq!(port.calls.load(Ordering::SeqCst), 0, "{change}");
        assert_eq!(service.outputs.available_permits(), 2, "{change}");
    }
}

const TEXT: &str = "  Selected assistant reply.\n";
const WAV: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/audio/m58-speech.wav"
));
struct FakePort {
    calls: AtomicUsize,
    entered: Semaphore,
    release: Semaphore,
    panic: bool,
}
impl FakePort {
    fn new(permits: usize) -> Arc<Self> {
        Arc::new(Self {
            calls: AtomicUsize::new(0),
            entered: Semaphore::new(0),
            release: Semaphore::new(permits),
            panic: false,
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
impl SpeechPort for FakePort {
    fn availability(&self) -> SpeechAvailability {
        SpeechAvailability::Available
    }
    async fn synthesize(
        &self,
        text: SpeechText,
        _cancel: CancelSignal,
        _deadline: Instant,
    ) -> Result<SpeechAudio, SpeechError> {
        assert_eq!(text.as_str(), TEXT);
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.entered.add_permits(1);
        self.release.acquire().await.unwrap().forget();
        assert!(!self.panic, "synthetic provider panic");
        SpeechAudio::from_pcm24k(WAV[44..].to_vec())
    }
}
async fn fixture() -> (SharedDaemonState, String) {
    let mut daemon = DaemonState::new();
    let config =
        serde_json::from_value(serde_json::json!({"name":"speech-test","model":"deterministic"}))
            .unwrap();
    let agent = daemon.create_agent(config).unwrap().state.id;
    daemon.sessions.insert(SessionRecord::new(
        &agent,
        "chat:speech",
        SessionKind::Chat,
        SessionOrigin::Web,
        "Speech".into(),
        TitleSource::FirstMessage,
        1,
    ));
    daemon
        .history
        .store()
        .upsert_messages(&[history_message(
            "reply-1",
            &agent,
            "chat:speech",
            MessageRole::Assistant,
            TEXT,
            1,
        )])
        .await
        .unwrap();
    (Arc::new(RwLock::new(daemon)), agent)
}
fn service(port: Arc<FakePort>) -> SpeechService {
    SpeechService::configured(SpeechAvailability::Available, Some(port))
}
fn spawn(
    service: &SpeechService,
    state: &SharedDaemonState,
    agent: &str,
    operation: uuid::Uuid,
) -> tokio::task::JoinHandle<Result<SpeechOutput, ServiceError>> {
    let service = service.clone();
    let state = state.clone();
    let agent = agent.to_string();
    tokio::spawn(async move {
        service
            .generate(
                state,
                agent,
                "chat:speech".into(),
                operation,
                "reply-1".into(),
                TEXT.into(),
            )
            .await
    })
}
async fn terminal(service: &SpeechService) {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if service
                .operations
                .lock()
                .unwrap()
                .values()
                .all(|e| e.terminal.is_some())
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn exact_saved_reply_without_workspace_returns_fixture_and_retains_physical_output_lease() {
    let (state, agent) = fixture().await;
    let port = FakePort::new(4);
    let service = service(port.clone());
    let one = spawn(&service, &state, &agent, uuid::Uuid::new_v4())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(one.duration_ms(), 250);
    let one = one.into_bytes();
    let alias = one.clone();
    assert_eq!(one.as_ref(), WAV);
    let two = spawn(&service, &state, &agent, uuid::Uuid::new_v4())
        .await
        .unwrap()
        .unwrap()
        .into_bytes();
    assert!(matches!(
        spawn(&service, &state, &agent, uuid::Uuid::new_v4())
            .await
            .unwrap(),
        Err(ServiceError::Busy)
    ));
    drop(one);
    assert!(matches!(
        spawn(&service, &state, &agent, uuid::Uuid::new_v4())
            .await
            .unwrap(),
        Err(ServiceError::Busy)
    ));
    drop(alias);
    drop(
        spawn(&service, &state, &agent, uuid::Uuid::new_v4())
            .await
            .unwrap()
            .unwrap(),
    );
    drop(two);
    assert_eq!(port.calls.load(Ordering::SeqCst), 3);
    assert_eq!(service.outputs.available_permits(), 2);
    assert!(state.read().await.workspace.is_none());
    let record = state
        .read()
        .await
        .history
        .store()
        .get_message(&agent, "chat:speech", "reply-1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.message.content.text, TEXT);
}

#[tokio::test]
async fn foreign_hidden_wrong_role_changed_text_and_missing_reply_never_dispatch() {
    for change in [
        "hidden",
        "role",
        "text",
        "agent",
        "session",
        "message-agent",
        "message-room",
        "missing",
    ] {
        let (state, agent) = fixture().await;
        let store = state.read().await.history.store();
        let mut row = store
            .get_message(&agent, "chat:speech", "reply-1")
            .await
            .unwrap()
            .unwrap();
        store.delete_session(&agent, "chat:speech").await.unwrap();
        match change {
            "hidden" => row.hidden = true,
            "role" => row.message.role = MessageRole::User,
            "text" => row.message.content.text = TEXT.trim().into(),
            "agent" => row.agent_id = "foreign".into(),
            "session" => row.session_id = "chat:foreign".into(),
            "message-agent" => row.message.agent_id = "foreign".into(),
            "message-room" => row.message.room_id = "chat:foreign".into(),
            _ => (),
        }
        if change != "missing" {
            store.upsert_messages(&[row]).await.unwrap();
        }
        let port = FakePort::new(1);
        let service = service(port.clone());
        assert!(
            matches!(
                spawn(&service, &state, &agent, uuid::Uuid::new_v4())
                    .await
                    .unwrap(),
                Err(ServiceError::NotFound)
            ),
            "{change}"
        );
        assert_eq!(port.calls.load(Ordering::SeqCst), 0, "{change}");
    }
}

#[tokio::test]
async fn mutation_during_provider_wait_fences_publication() {
    for change in [
        "record",
        "hidden",
        "role",
        "created",
        "session",
        "workspace",
        "store",
        "history",
    ] {
        let (state, agent) = fixture().await;
        let port = FakePort::new(0);
        let service = service(port.clone());
        let pending = spawn(&service, &state, &agent, uuid::Uuid::new_v4());
        port.entered().await;
        let store = state.read().await.history.store();
        match change {
            "record" | "hidden" | "role" => {
                let mut row = store
                    .get_message(&agent, "chat:speech", "reply-1")
                    .await
                    .unwrap()
                    .unwrap();
                if change == "record" {
                    row.message.created_at_ms += 1;
                } else if change == "hidden" {
                    row.hidden = true;
                } else {
                    row.message.role = MessageRole::User;
                }
                store.upsert_messages(&[row]).await.unwrap();
            }
            "created" => {
                state.write().await.sessions.remove(&agent, "chat:speech");
                state.write().await.sessions.insert(SessionRecord::new(
                    &agent,
                    "chat:speech",
                    SessionKind::Chat,
                    SessionOrigin::Web,
                    "Speech".into(),
                    TitleSource::FirstMessage,
                    2,
                ));
            }
            "session" => {
                state.write().await.sessions.remove(&agent, "chat:speech");
            }
            "workspace" => {
                state.write().await.workspace = Some(crate::control_plane_store::WorkspaceConfig {
                    root_path: std::path::PathBuf::from("synthetic-speech-root"),
                    company_name: "test".into(),
                    mission: "test".into(),
                    values: vec![],
                });
            }
            "store" => {
                state
                    .write()
                    .await
                    .set_history(crate::history::HistoryService::new(Arc::new(
                        crate::history::MemoryHistoryStore::new(),
                    )));
            }
            "history" => {
                state
                    .write()
                    .await
                    .set_history(crate::history::HistoryService::new(store));
            }
            _ => unreachable!(),
        }
        port.release.add_permits(1);
        assert!(
            matches!(pending.await.unwrap(), Err(ServiceError::NotFound)),
            "{change}"
        );
        assert_eq!(service.outputs.available_permits(), 2);
    }
}

#[tokio::test]
async fn cancel_before_admit_duplicate_drop_deadline_and_panic_retire_without_replay() {
    let (state, agent) = fixture().await;
    let port = FakePort::new(0);
    let service = service(port.clone());
    let id = uuid::Uuid::new_v4();
    service.cancel(&agent, "chat:speech", id).unwrap();
    assert!(matches!(
        spawn(&service, &state, &agent, id).await.unwrap(),
        Err(ServiceError::Duplicate)
    ));
    let id = uuid::Uuid::new_v4();
    let pending = spawn(&service, &state, &agent, id);
    port.entered().await;
    assert!(matches!(
        spawn(&service, &state, &agent, id).await.unwrap(),
        Err(ServiceError::Duplicate)
    ));
    pending.abort();
    let _ = pending.await;
    terminal(&service).await;
    assert_eq!(service.outputs.available_permits(), 2);
    assert!(matches!(
        spawn(&service, &state, &agent, id).await.unwrap(),
        Err(ServiceError::Duplicate)
    ));
    let mut deadline = service.clone();
    deadline.budget = Duration::from_millis(20);
    assert!(matches!(
        spawn(&deadline, &state, &agent, uuid::Uuid::new_v4())
            .await
            .unwrap(),
        Err(ServiceError::TimedOut)
    ));
    let panic_port = Arc::new(FakePort {
        calls: AtomicUsize::new(0),
        entered: Semaphore::new(0),
        release: Semaphore::new(1),
        panic: true,
    });
    let panics = SpeechService::configured(SpeechAvailability::Available, Some(panic_port));
    assert!(matches!(
        spawn(&panics, &state, &agent, uuid::Uuid::new_v4())
            .await
            .unwrap(),
        Err(ServiceError::Failed)
    ));
    terminal(&panics).await;
    assert_eq!(panics.outputs.available_permits(), 2);
    let restarted = SpeechService::disabled();
    assert!(restarted.operations.lock().unwrap().is_empty());
}

#[test]
fn map_is_bounded_scoped_and_tombstones_expire() {
    let service = SpeechService::disabled();
    let id = uuid::Uuid::new_v4();
    service.cancel("a", "s", id).unwrap();
    let one = service.admit(SpeechService::key("b", "s", id)).unwrap();
    let two = service.admit(SpeechService::key("a", "other", id)).unwrap();
    for _ in 0..100 {
        service.cancel("a", "s", uuid::Uuid::new_v4()).unwrap();
    }
    let mut map = service.operations.lock().unwrap();
    assert_eq!(map.len(), 64);
    assert!(map.contains_key(&one.key));
    assert!(map.contains_key(&two.key));
    for entry in map.values_mut().filter(|e| e.terminal.is_some()) {
        entry.terminal = Some(Instant::now() - TOMBSTONE_TTL);
    }
    prune(&mut map, Instant::now());
    assert_eq!(map.len(), 2);
    drop(map);
    drop(one);
    drop(two);
}

fn request(
    method: &str,
    path: &str,
    body: String,
    owner: bool,
) -> axum::http::Request<axum::body::Body> {
    let builder = axum::http::Request::builder()
        .method(method)
        .uri(path)
        .header("host", "127.0.0.1:8080")
        .header("content-type", "application/json");
    let builder = if owner {
        builder.header("origin", "http://localhost:4200")
    } else {
        builder
    };
    builder.body(axum::body::Body::from(body)).unwrap()
}
#[tokio::test]
async fn owner_routes_have_closed_capability_request_and_binary_contract() {
    use axum::{body::to_bytes, http::StatusCode};
    use tower::ServiceExt;
    let (state, agent) = fixture().await;
    let port = FakePort::new(2);
    let service = service(port.clone());
    let app =
        crate::app::app_with_state_and_speech(state, crate::app::DaemonConfig::default(), service);
    let path = format!("/api/agents/{agent}/sessions/chat%3Aspeech/voice/speech");
    let capability = app
        .clone()
        .oneshot(request("GET", &path, String::new(), true))
        .await
        .unwrap();
    assert_eq!(capability.status(), StatusCode::OK);
    assert_eq!(capability.headers()["cache-control"], "no-store");
    let json: serde_json::Value =
        serde_json::from_slice(&to_bytes(capability.into_body(), 4096).await.unwrap()).unwrap();
    assert_eq!(
        json,
        serde_json::json!({"version":1,"provider":"openai","model":"gpt-4o-mini-tts","voice":"alloy","format":"wav","available":true,"reason":"available","maxInputChars":2048,"maxInputBytes":8192,"maxAudioBytes":5760044,"maxDurationMs":120000})
    );
    let valid = serde_json::json!({"operationId":uuid::Uuid::new_v4().to_string(),"messageId":"reply-1","text":TEXT,"consent":true});
    for field in [
        "url",
        "path",
        "provider",
        "key",
        "model",
        "voice",
        "speed",
        "instructions",
    ] {
        let mut invalid = valid.clone();
        invalid[field] = serde_json::json!("forbidden");
        let response = app
            .clone()
            .oneshot(request("POST", &path, invalid.to_string(), true))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{field}");
        assert_eq!(response.headers()["cache-control"], "no-store");
    }
    for (field, value) in [
        ("consent", serde_json::json!(false)),
        ("operationId", serde_json::json!("not-uuid")),
        ("messageId", serde_json::json!("../wrong")),
        ("text", serde_json::json!(" ")),
        ("text", serde_json::json!("x".repeat(2049))),
    ] {
        let mut invalid = valid.clone();
        invalid[field] = value;
        assert_eq!(
            app.clone()
                .oneshot(request("POST", &path, invalid.to_string(), true))
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(
        app.clone()
            .oneshot(request("POST", &path, valid.to_string(), false))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        app.clone()
            .oneshot(request("POST", &path, " ".repeat(16 * 1024 + 1), true))
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(port.calls.load(Ordering::SeqCst), 0);
    let response = app
        .clone()
        .oneshot(request("POST", &path, valid.to_string(), true))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "audio/wav");
    assert_eq!(response.headers()["content-length"], WAV.len().to_string());
    assert_eq!(response.headers()["x-anima-speech-duration-ms"], "250");
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(
        to_bytes(response.into_body(), WAV.len())
            .await
            .unwrap()
            .as_ref(),
        WAV
    );
    assert_eq!(
        app.clone()
            .oneshot(request("POST", &path, valid.to_string(), true))
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    // Enabling speech does not enable transcription or alter its capability v1.
    let stt = app
        .oneshot(request(
            "GET",
            &path.replace("/speech", ""),
            String::new(),
            true,
        ))
        .await
        .unwrap();
    let json: serde_json::Value =
        serde_json::from_slice(&to_bytes(stt.into_body(), 4096).await.unwrap()).unwrap();
    assert_eq!(json["reason"], "disabled");
    assert_eq!(json["model"], "whisper-1");
}

#[tokio::test]
async fn all_speech_routes_bound_scope_waits_and_post_bounds_slow_body() {
    use axum::{
        body::{Body, Bytes},
        http::StatusCode,
    };
    use tower::ServiceExt;
    for method in ["GET", "POST", "DELETE", "BODY"] {
        let (state, agent) = fixture().await;
        let port = FakePort::new(0);
        let mut service = service(port.clone());
        service.budget = Duration::from_millis(5);
        let inspect = service.clone();
        let app = crate::app::app_with_state_and_speech(
            state.clone(),
            crate::app::DaemonConfig::default(),
            service,
        );
        let mut path = format!("/api/agents/{agent}/sessions/chat%3Aspeech/voice/speech");
        if method == "DELETE" {
            path.push_str(&format!("/{}", uuid::Uuid::new_v4()));
        }
        let mut request = request(
            if method == "BODY" { "POST" } else { method },
            &path,
            String::new(),
            true,
        );
        let guard = if method == "BODY" {
            *request.body_mut() =
                Body::from_stream(futures::stream::pending::<Result<Bytes, std::io::Error>>());
            None
        } else {
            Some(state.write().await)
        };
        let response = tokio::time::timeout(Duration::from_secs(2), app.oneshot(request))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT, "{method}");
        assert_eq!(response.headers()["cache-control"], "no-store");
        drop(guard);
        assert_eq!(port.calls.load(Ordering::SeqCst), 0);
        assert!(inspect.operations.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn speech_routes_have_separate_timeout_and_cancel_fences_provider_result() {
    use axum::http::StatusCode;
    use tower::ServiceExt;
    let (state, agent) = fixture().await;
    let port = FakePort::new(0);
    let service = service(port.clone());
    let app = crate::app::app_with_state_and_speech(
        state,
        crate::app::DaemonConfig {
            request_timeout: Duration::from_millis(1),
            ..Default::default()
        },
        service,
    );
    let path = format!("/api/agents/{agent}/sessions/chat%3Aspeech/voice/speech");
    let id = uuid::Uuid::new_v4();
    let body = serde_json::json!({"operationId":id.to_string(),"messageId":"reply-1","text":TEXT,"consent":true}).to_string();
    let pending = {
        let app = app.clone();
        let request = request("POST", &path, body, true);
        tokio::spawn(async move { app.oneshot(request).await.unwrap() })
    };
    port.entered().await;
    tokio::time::sleep(Duration::from_millis(10)).await;
    assert!(!pending.is_finished());
    assert_eq!(
        app.oneshot(request(
            "DELETE",
            &format!("{path}/{id}"),
            String::new(),
            true
        ))
        .await
        .unwrap()
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(pending.await.unwrap().status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn held_http_bodies_and_bytes_clones_keep_both_output_slots_until_retired() {
    use axum::{body::to_bytes, http::StatusCode};
    use tower::ServiceExt;
    let (state, agent) = fixture().await;
    let port = FakePort::new(4);
    let service = service(port.clone());
    let inspect = service.clone();
    let app =
        crate::app::app_with_state_and_speech(state, crate::app::DaemonConfig::default(), service);
    let path = format!("/api/agents/{agent}/sessions/chat%3Aspeech/voice/speech");
    let body = || {
        serde_json::json!({"operationId":uuid::Uuid::new_v4().to_string(),"messageId":"reply-1","text":TEXT,"consent":true}).to_string()
    };
    let one = app
        .clone()
        .oneshot(request("POST", &path, body(), true))
        .await
        .unwrap();
    let two = app
        .clone()
        .oneshot(request("POST", &path, body(), true))
        .await
        .unwrap();
    assert_eq!(inspect.outputs.available_permits(), 0);
    assert_eq!(
        app.clone()
            .oneshot(request("POST", &path, body(), true))
            .await
            .unwrap()
            .status(),
        StatusCode::TOO_MANY_REQUESTS
    );
    let one = to_bytes(one.into_body(), WAV.len()).await.unwrap();
    let alias = one.clone();
    drop(one);
    assert_eq!(inspect.outputs.available_permits(), 0);
    drop(alias);
    assert_eq!(inspect.outputs.available_permits(), 1);
    let three = app
        .oneshot(request("POST", &path, body(), true))
        .await
        .unwrap();
    assert_eq!(three.status(), StatusCode::OK);
    drop(two);
    drop(three);
    assert_eq!(inspect.outputs.available_permits(), 2);
    assert_eq!(port.calls.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn active_operations_are_nonqueued_and_scoped_cancel_does_not_cancel_another_scope() {
    let (state, agent) = fixture().await;
    let port = FakePort::new(0);
    let service = service(port.clone());
    let first_id = uuid::Uuid::new_v4();
    let one = spawn(&service, &state, &agent, first_id);
    port.entered().await;
    let second_id = uuid::Uuid::new_v4();
    let two = spawn(&service, &state, &agent, second_id);
    port.entered().await;
    assert!(matches!(
        spawn(&service, &state, &agent, uuid::Uuid::new_v4())
            .await
            .unwrap(),
        Err(ServiceError::Busy)
    ));
    service
        .cancel("foreign-agent", "chat:speech", first_id)
        .unwrap();
    service.cancel(&agent, "chat:other", second_id).unwrap();
    assert!(!one.is_finished());
    assert!(!two.is_finished());
    service.cancel(&agent, "chat:speech", first_id).unwrap();
    assert!(matches!(one.await.unwrap(), Err(ServiceError::Cancelled)));
    port.release.add_permits(1);
    drop(two.await.unwrap().unwrap());
    assert_eq!(port.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn disabled_or_non_owner_chat_scopes_never_dispatch() {
    for change in [
        "disabled",
        "foreign-agent",
        "missing-session",
        "connector",
        "system",
    ] {
        let (state, agent) = fixture().await;
        let port = FakePort::new(1);
        let service = if change == "disabled" {
            SpeechService::disabled()
        } else {
            service(port.clone())
        };
        match change {
            "missing-session" => {
                state.write().await.sessions.remove(&agent, "chat:speech");
            }
            "connector" | "system" => {
                state.write().await.sessions.remove(&agent, "chat:speech");
                let (kind, origin) = if change == "system" {
                    (SessionKind::Checkin, SessionOrigin::Schedule)
                } else {
                    (SessionKind::Chat, SessionOrigin::Telegram)
                };
                state.write().await.sessions.insert(SessionRecord::new(
                    &agent,
                    "chat:speech",
                    kind,
                    origin,
                    "Foreign".into(),
                    TitleSource::FirstMessage,
                    1,
                ));
            }
            _ => (),
        }
        let result = spawn(
            &service,
            &state,
            if change == "foreign-agent" {
                "foreign-agent"
            } else {
                &agent
            },
            uuid::Uuid::new_v4(),
        )
        .await
        .unwrap();
        assert!(result.is_err(), "{change}");
        assert_eq!(port.calls.load(Ordering::SeqCst), 0);
        assert!(state.read().await.workspace.is_none());
    }
}

#[tokio::test]
async fn generating_does_not_create_workspace_files_or_persist_audio_and_restart_does_not_replay() {
    let (state, agent) = fixture().await;
    let root =
        std::env::temp_dir().join(format!("anima-speech-no-output-{}", uuid::Uuid::new_v4()));
    assert!(!root.exists());
    state.write().await.workspace = Some(crate::control_plane_store::WorkspaceConfig {
        root_path: root.clone(),
        company_name: "test".into(),
        mission: "test".into(),
        values: vec![],
    });
    let port = FakePort::new(1);
    let service = service(port.clone());
    let id = uuid::Uuid::new_v4();
    let before = state
        .read()
        .await
        .history
        .store()
        .get_message(&agent, "chat:speech", "reply-1")
        .await
        .unwrap();
    drop(spawn(&service, &state, &agent, id).await.unwrap().unwrap());
    assert!(!root.exists());
    assert_eq!(
        state
            .read()
            .await
            .history
            .store()
            .get_message(&agent, "chat:speech", "reply-1")
            .await
            .unwrap(),
        before
    );
    assert_eq!(port.calls.load(Ordering::SeqCst), 1);
    drop(service);
    let restarted = SpeechService::configured(SpeechAvailability::Available, Some(port.clone()));
    assert!(restarted.operations.lock().unwrap().is_empty());
    assert_eq!(port.calls.load(Ordering::SeqCst), 1);
}
