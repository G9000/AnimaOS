//! M48 synthetic coordinator request evidence. Default cases never construct
//! subscription auth or contact a provider. Real diagnostic execution is held.
use super::test_support::{accept_web, add_chat, lead_config, ScriptedModel, Step};
use super::AgentRunCoordinator;
use crate::control_plane_store::WorkspaceConfig;
use crate::runs::RunStatus;
use crate::runtime_model::RuntimeModelAdapter;
use crate::state::DaemonState;
use anima_core::{AgentConfig, Content, Message, MessageRole, ModelAdapter, ModelGenerateRequest};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    ffi::{OsStr, OsString},
    io::Write,
    path::PathBuf,
    sync::Arc,
    time::Duration,
};
use tokio::sync::{RwLock, Semaphore};

const INPUT: &str = "M48_SYNTHETIC_MANAGER_FIRST_REQUEST";
const CONTEXT: &str = "M48_SYNTHETIC_REQUEST_CONTEXT: Reply M48_CAPTURE_OK without calling tools.";
const SELECTED: &[&str] = &[
    "read_file",
    "list_dir",
    "glob",
    "grep",
    "write_file",
    "edit_file",
    "todo_read",
    "todo_write",
    "spawn_helper",
    "bash",
];
const AUGMENTED: &[&str] = &[
    "list_workspace_agents",
    "send_message",
    "broadcast_message",
    "delegate_to_agent",
    "rename_chat",
    "compact_chat",
    "export_chat",
    "show_result",
    "list_results",
];
const MAX_FIXTURE_BYTES: usize = 512 * 1024;
const REAL_OPT_IN_ERROR: &str =
    "M48 real diagnostic is held; root must explicitly set ANIMAOS_M48_RUN_REAL_DIAGNOSTIC=1";

async fn synthetic_first_request() -> Value {
    let private = tempfile::tempdir().expect("private synthetic directory");
    let workspace = private.path().join("workspace");
    std::fs::create_dir(&workspace).expect("synthetic workspace");
    let sentinel = workspace.join("protected.txt");
    let protected = b"M48 protected synthetic neighbor\n";
    std::fs::write(&sentinel, protected).expect("protected synthetic bytes");
    let modified = std::fs::metadata(&sentinel).unwrap().modified().unwrap();
    let model = ScriptedModel::new(vec![Step::Text(vec!["M48_CAPTURE_OK"])]);
    let mut daemon = DaemonState::with_model_adapter(model.clone());
    daemon.workspace = Some(WorkspaceConfig {
        root_path: workspace.clone(),
        company_name: "M48 synthetic workspace".into(),
        mission: "Offline request capture only".into(),
        values: vec![],
    });
    let mut config = lead_config("M48 synthetic manager");
    config.model = "gpt-5.5".into();
    config.provider = Some("chatgpt".into());
    config.system = Some(CONTEXT.into());
    config.tools = Some(
        daemon
            .tool_registry
            .resolve_descriptors(SELECTED.iter().copied())
            .unwrap(),
    );
    let settings = config.settings.as_mut().unwrap();
    settings.max_tool_iterations = Some(1);
    settings.max_retries = Some(0);
    settings.timeout_ms = Some(20_000);
    settings.max_tokens = Some(2048);
    settings.temperature = None;
    let agent = daemon.create_agent(config).unwrap().state.id;
    let original = daemon.get_agent(&agent).unwrap().state.config;
    let canonical = daemon.tool_registry.clone();
    let coordinator =
        AgentRunCoordinator::new(Arc::new(RwLock::new(daemon)), Arc::new(Semaphore::new(1)));
    add_chat(&coordinator, &agent, "m48-synthetic-chat").await;
    let run_id = accept_web(&coordinator, &agent, "m48-synthetic-chat", INPUT).await;
    let terminal = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let record = coordinator
                .state
                .read()
                .await
                .runs
                .get(&run_id)
                .unwrap()
                .clone();
            if record.status.is_terminal() {
                break record;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    // Drain accepted work and the existing optional learning worker before
    // counting every primary/secondary invocation. No process/runtime is started.
    coordinator.shutdown().await;
    let terminal = terminal.expect("controlled ordinary run settles");
    assert_eq!(terminal.status, RunStatus::Completed);
    assert_eq!(terminal.parent_run_id, None);
    assert!(terminal.tools_started.is_empty());
    assert!(terminal.action_receipts.is_empty());
    assert_eq!(terminal.steps.len(), 1);
    let requests = model.requests();
    let configs = model.configs();
    assert_eq!(
        requests.len(),
        1,
        "one actual ordinary coordinator model call"
    );
    assert_eq!(configs.len(), 1);
    assert!(model.secondary_requests().is_empty());
    let effective = &configs[0];
    let request = &requests[0];
    let tools = effective.tools.as_ref().unwrap();
    let names = tools
        .iter()
        .map(|tool| tool.name.as_str())
        .collect::<BTreeSet<_>>();
    let expected = SELECTED
        .iter()
        .chain(AUGMENTED)
        .copied()
        .collect::<BTreeSet<_>>();
    assert_eq!(names, expected);
    assert_eq!(
        tools.len(),
        expected.len(),
        "no duplicate advertised descriptors"
    );
    for tool in tools {
        assert_eq!(tool, &canonical.descriptor(&tool.name).unwrap());
    }
    let mut unchanged = effective.clone();
    unchanged.tools = original.tools.clone();
    assert_eq!(
        unchanged, original,
        "only the ordinary run-copy tool augmentation applies"
    );
    assert_eq!(request.messages.len(), 1);
    assert_eq!(request.messages[0].role, MessageRole::User);
    assert_eq!(request.messages[0].content.text, INPUT);
    assert!(request.messages[0].content.attachments.is_none());
    assert!(request.system.contains(CONTEXT));
    assert_eq!(request.temperature, None);
    assert_eq!(request.max_tokens, Some(2048));
    {
        let guard = coordinator.state.read().await;
        assert_eq!(guard.list_agents().len(), 1, "no helper was created");
        assert_eq!(guard.runs.for_agent(&agent).len(), 1);
        assert_eq!(guard.get_agent(&agent).unwrap().state.config, original);
        assert!(guard.control_plane_snapshot().approvals.is_empty());
    }
    // Existing host journal/memory bookkeeping is declared runtime evidence;
    // the protected workspace input is not a tool output and must stay exact.
    assert_eq!(std::fs::read(&sentinel).unwrap(), protected);
    assert_eq!(
        std::fs::metadata(&sentinel).unwrap().modified().unwrap(),
        modified
    );
    let envelope = json!({
        "fixtureVersion":1,
        "config":effective,
        "request":{
            "system":request.system, "messages":request.messages,
            "temperature":request.temperature, "maxTokens":request.max_tokens,
        },
    });
    assert!(serde_json::to_vec(&envelope).unwrap().len() <= MAX_FIXTURE_BYTES);
    envelope
}

#[tokio::test]
async fn m48_provider_request_manager_first_call_captures_augmented_tools() {
    let envelope = synthetic_first_request().await;
    assert_eq!(envelope["fixtureVersion"], 1);
    assert_eq!(envelope["config"]["model"], "gpt-5.5");
}

#[tokio::test]
#[ignore = "root-only synthetic create_new export; requires a fresh absolute private fixture path"]
async fn m48_provider_request_export_synthetic_first_call() {
    let path = PathBuf::from(
        std::env::var_os("ANIMAOS_M48_REQUEST_FIXTURE")
            .expect("root must select the private fixture path"),
    );
    assert!(
        path.is_absolute(),
        "the private fixture path must be absolute"
    );
    let envelope = synthetic_first_request().await;
    let bytes = serde_json::to_vec(&envelope).unwrap();
    assert!(bytes.len() <= MAX_FIXTURE_BYTES);
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .expect("root selected a fresh private fixture path with an existing parent");
    output
        .write_all(&bytes)
        .expect("synthetic fixture writes fully");
    output.sync_all().expect("synthetic fixture syncs");
    println!(
        "M48_REQUEST_FIXTURE {}",
        json!({
            "fixtureVersion":1, "bytes":bytes.len(), "sha256":hex::encode(Sha256::digest(&bytes)),
            "modelCalls":1, "tools":14, "externalProviderCalls":0,
        })
    );
}

fn diagnostic_adapter(opt_in: Option<OsString>) -> Result<RuntimeModelAdapter, &'static str> {
    if opt_in.as_deref() != Some(OsStr::new("1")) {
        return Err(REAL_OPT_IN_ERROR);
    }
    // The explicit opt-in above precedes auth construction, environment provider
    // loading, vault access, client creation and any filesystem/network work.
    RuntimeModelAdapter::try_from_env(crate::chatgpt_auth::ChatGptAuth::new(), None)
        .map_err(|_| "M48 diagnostic adapter is unavailable")
}

#[test]
fn m48_provider_request_real_driver_refuses_missing_or_inexact_opt_in() {
    for flag in [
        None,
        Some(OsString::from("0")),
        Some(OsString::from("true")),
        Some(OsString::from("1 ")),
    ] {
        assert_eq!(diagnostic_adapter(flag).err(), Some(REAL_OPT_IN_ERROR));
    }
}

fn safe_error_category(error: &str) -> &'static str {
    match error {
        "Model call timed out" => "timeout",
        "ChatGPT credential vault is unavailable" => "credential_unavailable",
        "ChatGPT is disconnected; sign in in settings" => "disconnected",
        "ChatGPT authorization failed; start sign-in again" => "authorization",
        "ChatGPT authorization changed; retry" => "authorization_changed",
        "ChatGPT subscription selected model is not supported for this account; review the model and account access" => "model_access",
        "ChatGPT subscription request failed (HTTP 400)" => "http_400_unclassified",
        "ChatGPT request failed or timed out" => "transport",
        _ if error.contains("(code: invalid_function_parameters") => "tool_schema",
        _ if error.contains("(code: invalid_request_error") || error.contains("(type: invalid_request_error") => "invalid_request",
        _ if error.contains("(code: model_not_found") || error.contains("(code: unsupported_model") => "model_unavailable",
        _ => "unclassified_safe_failure",
    }
}

#[tokio::test]
#[ignore = "HELD: root separately releases one guarded subscription request after gates"]
async fn m48_provider_request_real_zero_tool_route_diagnostic() {
    // This is the first operation: absent/inexact opt-in returns before auth or I/O.
    let adapter = diagnostic_adapter(std::env::var_os("ANIMAOS_M48_RUN_REAL_DIAGNOSTIC"))
        .unwrap_or_else(|error| panic!("{error}"));
    let mut config: AgentConfig = serde_json::from_value(json!({
        "name":"M48 route diagnostic", "provider":"chatgpt", "model":"gpt-5.5",
        "tools":[],
    }))
    .expect("fixed synthetic diagnostic config");
    // Assign admission controls explicitly through existing AgentSettings.
    let settings = config.settings.get_or_insert_with(Default::default);
    settings.max_retries = Some(0);
    settings.timeout_ms = Some(20_000);
    settings.max_tokens = Some(2048);
    let request = ModelGenerateRequest {
        system: "Reply with exactly M48_ROUTE_OK. Do not call tools.".into(),
        messages: vec![Message {
            id: "m48-route-message".into(),
            agent_id: "m48-route-agent".into(),
            room_id: "m48-route-room".into(),
            role: MessageRole::User,
            created_at_ms: 0,
            content: Content {
                text: "Reply with exactly M48_ROUTE_OK.".into(),
                attachments: None,
                metadata: None,
            },
        }],
        temperature: None,
        max_tokens: Some(2048),
    };
    // Direct adapter only: no coordinator, grants, titles, helpers or tool executor.
    let result =
        tokio::time::timeout(Duration::from_secs(30), adapter.generate(&config, &request)).await;
    match result {
        Ok(Ok(response)) => {
            let marker_match = response.content.text.trim() == "M48_ROUTE_OK";
            let unexpected_tools = response
                .tool_calls
                .as_ref()
                .is_some_and(|calls| !calls.is_empty());
            println!(
                "M48_ROUTE_DIAGNOSTIC {}",
                json!({
                    "markerMatch":marker_match, "bytes":response.content.text.len(),
                    "sha256":hex::encode(Sha256::digest(response.content.text.as_bytes())),
                    "reportedUsage":response.usage, "unexpectedTools":unexpected_tools,
                })
            );
            assert!(
                marker_match && !unexpected_tools,
                "M48 diagnostic did not satisfy its fixed zero-tool marker"
            );
        }
        Ok(Err(error)) => {
            println!(
                "M48_ROUTE_DIAGNOSTIC {}",
                json!({"result":"error","category":safe_error_category(&error),"usage":"unknown"})
            );
            panic!("M48 diagnostic failed; failed-call provider consumption is unknown; no retry is authorized");
        }
        Err(_) => {
            println!(
                "M48_ROUTE_DIAGNOSTIC {}",
                json!({"result":"error","category":"root_deadline","usage":"unknown"})
            );
            panic!("M48 diagnostic root deadline expired; provider consumption is unknown; no retry is authorized");
        }
    }
}
