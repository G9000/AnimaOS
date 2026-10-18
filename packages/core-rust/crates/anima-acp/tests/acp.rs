//! Drives the scripted fake agent (tests/fixtures/fake_acp_agent.rs) through
//! the real stdio, custody and JSON-RPC paths.
use anima_acp::{
    AcpClientHost, AcpError, AcpEvent, AcpLaunch, AcpSession, BoxFuture, PermissionOptionKind,
    PermissionRequest, ToolCallStatus, ToolKind, TurnStatus,
};
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

const WAIT: Duration = Duration::from_secs(10);

/// Answers permissions with a fixed choice (or never, when `hang`) and
/// refuses writes when `refuse_writes`.
#[derive(Default)]
struct TestHost {
    answer: Option<String>,
    hang: bool,
    refuse_writes: bool,
    asked: Mutex<Vec<PermissionRequest>>,
    approved: Mutex<Vec<PathBuf>>,
}

impl AcpClientHost for TestHost {
    fn request_permission(&self, request: PermissionRequest) -> BoxFuture<'static, Option<String>> {
        self.asked.lock().unwrap().push(request);
        let answer = self.answer.clone();
        let hang = self.hang;
        Box::pin(async move {
            if hang {
                std::future::pending::<()>().await;
            }
            answer
        })
    }

    fn approve_write(&self, path: &Path) -> BoxFuture<'static, Result<(), String>> {
        self.approved.lock().unwrap().push(path.to_path_buf());
        let refuse = self.refuse_writes;
        Box::pin(async move {
            if refuse {
                Err("owner said no".into())
            } else {
                Ok(())
            }
        })
    }
}

fn launch(cwd: &Path, scenario: &str, extra: &[(&str, String)]) -> AcpLaunch {
    let mut env = vec![("FAKE_ACP_SCENARIO".to_string(), scenario.to_string())];
    env.extend(extra.iter().map(|(k, v)| (k.to_string(), v.clone())));
    AcpLaunch {
        program: PathBuf::from(env!("CARGO_BIN_EXE_fake-acp-agent")),
        args: Vec::new(),
        cwd: cwd.to_path_buf(),
        env,
        env_clear: false,
        resume_session_id: None,
        ask_modes: Vec::new(),
    }
}

async fn start(launch: AcpLaunch, host: Arc<TestHost>) -> AcpSession {
    AcpSession::start(launch, host)
        .await
        .expect("session starts")
}

async fn run_turn(session: &AcpSession, text: &str) -> (TurnStatus, Vec<AcpEvent>) {
    let handle = session.prompt(text.into()).await.expect("prompt starts");
    let status = session.wait(WAIT).await;
    let (events, _) = session.events_since(handle.cursor);
    (status, events)
}

fn messages(events: &[AcpEvent]) -> String {
    events
        .iter()
        .filter_map(|e| match e {
            AcpEvent::AgentMessage { text } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

/// Drops the agent's own stderr, which races stdout; keeps `[acp]` notes.
fn protocol(events: Vec<AcpEvent>) -> Vec<AcpEvent> {
    events
        .into_iter()
        .filter(|e| !matches!(e, AcpEvent::Stderr { text } if !text.contains("[acp]")))
        .collect()
}

fn finished(reason: &str) -> TurnStatus {
    TurnStatus::Finished {
        stop_reason: reason.into(),
    }
}

#[tokio::test]
async fn handshake_and_streamed_turn() {
    let dir = tempfile::tempdir().unwrap();
    let record = dir.path().join("record.jsonl");
    let session = start(
        launch(
            dir.path(),
            "basic",
            &[("FAKE_ACP_RECORD", record.display().to_string())],
        ),
        Arc::default(),
    )
    .await;
    assert_eq!(session.session_id(), "sess-1");
    let info = session.agent_info();
    assert_eq!(info.name.as_deref(), Some("fake-agent"));
    assert_eq!(info.protocol_version, 1);
    assert!(!info.load_session && info.prompt_embedded_context);
    assert!(info.auth_methods.is_empty());

    let (status, events) = run_turn(&session, "summarise my notes").await;
    let events = protocol(events);
    assert_eq!(status, finished("end_turn"));
    assert_eq!(events.len(), 7, "{events:#?}");
    assert_eq!(
        events[0],
        AcpEvent::AgentMessage {
            text: "Hello world".into()
        }
    );
    assert_eq!(
        events[1],
        AcpEvent::ToolCall {
            id: "t1".into(),
            title: "Read notes".into(),
            kind: ToolKind::Read,
            status: ToolCallStatus::Pending,
            locations: vec![PathBuf::from("/tmp/notes.txt")],
        }
    );
    assert_eq!(
        events[2],
        AcpEvent::ToolCallUpdate {
            id: "t1".into(),
            status: Some(ToolCallStatus::Completed),
            content_text: Some("3 lines".into()),
        }
    );
    let AcpEvent::Plan { entries } = &events[3] else {
        panic!("plan, got {:?}", events[3]);
    };
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[1].content, "Summarise");
    assert_eq!(entries[1].status, "pending");
    assert!(matches!(&events[4], AcpEvent::Stderr { text } if text.contains("malformed line")));
    assert_eq!(
        events[5],
        AcpEvent::AgentThought {
            text: "done".into()
        }
    );
    assert_eq!(
        events[6],
        AcpEvent::TurnEnded {
            stop_reason: "end_turn".into()
        }
    );

    // Agent stderr lands in the log too.
    wait_until(|| {
        session
            .events_since(0)
            .0
            .iter()
            .any(|e| matches!(e, AcpEvent::Stderr { text } if text.contains("fake agent starting")))
    })
    .await;

    // What the agent received: our capabilities and an absolute cwd.
    let lines = std::fs::read_to_string(&record).unwrap();
    let sent: Vec<Value> = lines
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let init = &sent[0]["params"];
    assert_eq!(sent[0]["method"], "initialize");
    assert_eq!(init["protocolVersion"], 1);
    assert_eq!(init["clientCapabilities"]["fs"]["readTextFile"], true);
    assert_eq!(init["clientCapabilities"]["fs"]["writeTextFile"], true);
    assert_eq!(init["clientCapabilities"]["terminal"], false);
    assert_eq!(init["clientInfo"]["name"], "anima");
    assert_eq!(sent[1]["method"], "session/new");
    assert_eq!(sent[1]["params"]["cwd"], dir.path().display().to_string());
    assert_eq!(sent[1]["params"]["mcpServers"], serde_json::json!([]));
    assert_eq!(sent[2]["method"], "session/prompt");
    assert_eq!(sent[2]["params"]["sessionId"], "sess-1");
    assert_eq!(sent[2]["params"]["prompt"][0]["text"], "summarise my notes");

    // A second turn works on the same session.
    let (status, _) = run_turn(&session, "again").await;
    assert_eq!(status, finished("end_turn"));
    assert!(session.stop().await);
}

#[tokio::test]
async fn permission_answer_reaches_the_agent() {
    let dir = tempfile::tempdir().unwrap();
    let host = Arc::new(TestHost {
        answer: Some("allow".into()),
        ..TestHost::default()
    });
    let session = start(launch(dir.path(), "permission", &[]), host.clone()).await;
    let (status, events) = run_turn(&session, "edit it").await;
    assert_eq!(status, finished("end_turn"));
    assert_eq!(messages(&events), "chose:allow");
    let asked = host.asked.lock().unwrap().clone();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].tool_call_id, "t9");
    assert_eq!(asked[0].title, "Edit main.rs");
    assert_eq!(asked[0].kind, ToolKind::Edit);
    assert_eq!(asked[0].locations, vec![PathBuf::from("/work/main.rs")]);
    assert!(asked[0].raw_input.is_some());
    let kinds: Vec<_> = asked[0].options.iter().map(|o| o.kind).collect();
    assert_eq!(
        kinds,
        vec![
            PermissionOptionKind::AllowOnce,
            PermissionOptionKind::AllowAlways,
            PermissionOptionKind::RejectOnce
        ]
    );
    assert!(events.iter().any(|e| matches!(
        e,
        AcpEvent::PermissionAnswered { tool_call_id, option_id: Some(id) } if tool_call_id == "t9" && id == "allow"
    )));
    assert!(session.stop().await);
}

#[tokio::test]
async fn permission_none_and_unoffered_answers_are_cancelled() {
    for answer in [None, Some("made-up".to_string())] {
        let dir = tempfile::tempdir().unwrap();
        let host = Arc::new(TestHost {
            answer,
            ..TestHost::default()
        });
        let session = start(launch(dir.path(), "permission", &[]), host).await;
        let (status, events) = run_turn(&session, "edit it").await;
        assert_eq!(status, finished("end_turn"));
        assert_eq!(messages(&events), "chose:cancelled");
        assert!(events.iter().any(|e| matches!(
            e,
            AcpEvent::PermissionAnswered {
                option_id: None,
                ..
            }
        )));
        assert!(session.stop().await);
    }
}

#[tokio::test]
async fn cancel_answers_a_pending_permission_as_cancelled() {
    let dir = tempfile::tempdir().unwrap();
    let host = Arc::new(TestHost {
        hang: true,
        ..TestHost::default()
    });
    let session = start(launch(dir.path(), "permission", &[]), host.clone()).await;
    let handle = session.prompt("edit it".into()).await.unwrap();
    wait_until(|| !host.asked.lock().unwrap().is_empty()).await;
    session.cancel().await.unwrap();
    assert_eq!(session.wait(WAIT).await, finished("cancelled"));
    let (events, _) = session.events_since(handle.cursor);
    assert_eq!(messages(&events), "chose:cancelled");
    assert!(session.stop().await);
}

#[tokio::test]
async fn fs_requests_are_confined_to_the_session_cwd() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().join("work");
    std::fs::create_dir(&cwd).unwrap();
    std::fs::write(dir.path().join("secret.txt"), "top secret").unwrap();
    std::fs::write(cwd.join("notes.txt"), "one\ntwo\nthree\n").unwrap();
    let absolute_outside = dir.path().join("abs-outside.txt");
    let ops = serde_json::json!([
        {"write": cwd.join("sub/inside.txt"), "content": "hello"},
        {"write": cwd.join("../outside.txt"), "content": "escaped"},
        {"write": absolute_outside, "content": "escaped"},
        {"read": cwd.join("notes.txt"), "line": 2, "limit": 1},
        {"read": cwd.join("../secret.txt")},
        {"read": dir.path().join("secret.txt")},
        {"read": cwd.join("missing.txt")},
        {"call": "terminal/create"},
    ]);
    let host = Arc::new(TestHost::default());
    let session = start(
        launch(&cwd, "fs", &[("FAKE_ACP_FS_OPS", ops.to_string())]),
        host.clone(),
    )
    .await;
    let (status, events) = run_turn(&session, "touch files").await;
    assert_eq!(status, finished("end_turn"));
    let results: Vec<String> = messages(&events).lines().map(str::to_owned).collect();
    assert_eq!(
        results,
        vec![
            "ok:",
            "err:-32602",
            "err:-32602",
            "ok:two",
            "err:-32602",
            "err:-32602",
            "err:-32002",
            "err:-32601",
        ],
        "{events:#?}"
    );
    assert_eq!(
        std::fs::read_to_string(cwd.join("sub/inside.txt")).unwrap(),
        "hello"
    );
    assert!(!dir.path().join("outside.txt").exists());
    assert!(!dir.path().join("abs-outside.txt").exists());
    // Only the confined write reached the host, with its resolved path.
    let approved = host.approved.lock().unwrap().clone();
    let inside = std::fs::canonicalize(cwd.join("sub/inside.txt")).unwrap();
    assert_eq!(approved, vec![inside.clone()]);
    assert!(events
        .iter()
        .any(|e| matches!(e, AcpEvent::FileWritten { path } if *path == inside)));
    assert!(session.stop().await);
}

#[tokio::test]
async fn host_can_refuse_a_write_inside_cwd() {
    let dir = tempfile::tempdir().unwrap();
    let ops = serde_json::json!([{"write": dir.path().join("x.txt"), "content": "no"}]);
    let host = Arc::new(TestHost {
        refuse_writes: true,
        ..TestHost::default()
    });
    let session = start(
        launch(dir.path(), "fs", &[("FAKE_ACP_FS_OPS", ops.to_string())]),
        host,
    )
    .await;
    let (_, events) = run_turn(&session, "write").await;
    assert_eq!(messages(&events), "err:-32602\n");
    assert!(!dir.path().join("x.txt").exists());
    assert!(session.stop().await);
}

#[tokio::test]
async fn cancel_mid_turn_ends_with_cancelled() {
    let dir = tempfile::tempdir().unwrap();
    let session = start(launch(dir.path(), "cancel", &[]), Arc::default()).await;
    let handle = session.prompt("work".into()).await.unwrap();
    assert_eq!(
        session.wait(Duration::from_millis(200)).await,
        TurnStatus::Running
    );
    assert!(matches!(
        session.prompt("again".into()).await,
        Err(AcpError::Busy)
    ));
    session.cancel().await.unwrap();
    assert_eq!(session.wait(WAIT).await, finished("cancelled"));
    let (events, _) = session.events_since(handle.cursor);
    assert_eq!(messages(&events), "working");
    assert!(events.iter().any(|e| matches!(
        e,
        AcpEvent::ToolCallUpdate {
            status: Some(ToolCallStatus::Failed),
            ..
        }
    )));
    assert!(session.stop().await);
}

#[tokio::test]
async fn agent_exit_mid_turn_resolves_the_turn() {
    let dir = tempfile::tempdir().unwrap();
    let session = start(launch(dir.path(), "exit", &[]), Arc::default()).await;
    let (status, events) = run_turn(&session, "go").await;
    assert_eq!(status, TurnStatus::Exited { code: Some(3) });
    assert_eq!(messages(&events), "bye");
    assert!(events
        .iter()
        .any(|e| matches!(e, AcpEvent::Exited { code: Some(3) })));
    assert!(matches!(
        session.prompt("more".into()).await,
        Err(AcpError::Exited { code: Some(3), .. })
    ));
    assert!(session.cancel().await.is_err());
    assert!(session.stop().await);
}

#[tokio::test]
async fn stop_retires_a_grandchild() {
    let dir = tempfile::tempdir().unwrap();
    let heartbeat = dir.path().join("heartbeat");
    let session = start(
        launch(
            dir.path(),
            "grandchild",
            &[("FAKE_ACP_HEARTBEAT", heartbeat.display().to_string())],
        ),
        Arc::default(),
    )
    .await;
    session.prompt("spawn".into()).await.unwrap();
    wait_until(|| heartbeat.is_file()).await;
    assert!(session.stop().await);
    assert!(matches!(session.status(), TurnStatus::Exited { .. }));
    let before = std::fs::read(&heartbeat).unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(before, std::fs::read(&heartbeat).unwrap());
    // Idempotent.
    assert!(session.stop().await);
}

#[tokio::test]
async fn resume_uses_session_load_when_advertised() {
    let dir = tempfile::tempdir().unwrap();
    let mut resume = launch(dir.path(), "load", &[]);
    resume.resume_session_id = Some("sess-1".into());
    let session = start(resume, Arc::default()).await;
    assert_eq!(session.session_id(), "sess-1");
    assert!(session.agent_info().load_session);
    let (replay, cursor) = session.events_since(0);
    assert_eq!(
        protocol(replay),
        vec![
            AcpEvent::UserMessage {
                text: "earlier question".into()
            },
            AcpEvent::AgentMessage {
                text: "earlier answer".into()
            },
        ]
    );
    let (status, _) = run_turn(&session, "continue").await;
    assert_eq!(status, finished("end_turn"));
    assert!(cursor > 0);
    assert!(session.stop().await);

    // An agent without loadSession refuses a resume instead of silently
    // starting fresh.
    let mut resume = launch(dir.path(), "basic", &[]);
    resume.resume_session_id = Some("sess-1".into());
    assert!(matches!(
        AcpSession::start(resume, Arc::new(TestHost::default())).await,
        Err(AcpError::ResumeUnsupported)
    ));
}

#[tokio::test]
async fn auth_required_surfaces_the_agents_methods() {
    let dir = tempfile::tempdir().unwrap();
    let result = AcpSession::start(
        launch(dir.path(), "auth", &[]),
        Arc::new(TestHost::default()),
    )
    .await;
    let Err(AcpError::AuthRequired { methods }) = result else {
        panic!("expected AuthRequired");
    };
    assert_eq!(methods.len(), 1);
    assert_eq!(methods[0].id, "oauth");
    assert_eq!(methods[0].name, "Log in");
    assert_eq!(methods[0].description.as_deref(), Some("Browser login"));
}

#[tokio::test]
async fn missing_program_is_a_spawn_error() {
    let dir = tempfile::tempdir().unwrap();
    let mut missing = launch(dir.path(), "basic", &[]);
    missing.program = dir.path().join("no-such-agent");
    assert!(matches!(
        AcpSession::start(missing, Arc::new(TestHost::default())).await,
        Err(AcpError::Spawn(_))
    ));
    let mut relative = launch(dir.path(), "basic", &[]);
    relative.cwd = PathBuf::from("relative");
    assert!(matches!(
        AcpSession::start(relative, Arc::new(TestHost::default())).await,
        Err(AcpError::Spawn(_))
    ));
}

/// Hosts keep sessions in multi-threaded runtimes, so every future must be Send.
#[allow(dead_code)]
fn futures_are_send(session: &AcpSession, host: Arc<TestHost>) {
    fn send<T: Send>(_: T) {}
    send(AcpSession::start(
        launch(Path::new("/"), "basic", &[]),
        host,
    ));
    send(session.prompt(String::new()));
    send(session.wait(WAIT));
    send(session.cancel());
    send(session.stop());
}

async fn wait_until(mut condition: impl FnMut() -> bool) {
    tokio::time::timeout(WAIT, async {
        while !condition() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("condition reached");
}

#[tokio::test]
async fn the_client_switches_an_agent_into_its_asking_mode() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let mut asking = launch(&cwd, "modes", &[]);
    asking.ask_modes = vec!["default".into()];
    let session = start(asking, Arc::new(TestHost::default())).await;
    assert_eq!(session.mode(), Some("default"));
    let (events, _) = session.events_since(0);
    assert!(events.iter().any(
        |event| matches!(event, AcpEvent::AgentMessage { text } if text.contains("mode default"))
    ));
    assert!(session.stop().await);

    // Without a preference the agent keeps the mode it chose.
    let session = start(launch(&cwd, "modes", &[]), Arc::new(TestHost::default())).await;
    assert_eq!(session.mode(), Some("acceptEdits"));
    assert!(session.stop().await);
}
