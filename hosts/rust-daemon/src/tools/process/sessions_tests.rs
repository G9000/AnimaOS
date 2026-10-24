//! The app and coding-agent tools through the real tool path: configured
//! tools, the shared session registry, and the JSON the chat renders.
use std::collections::BTreeMap;
use std::sync::Arc;

use anima_core::{
    AgentState, AgentStatus, Content, DataValue, Message, MessageRole, TaskStatus, ToolCall,
};
use anima_memory::MemoryManager;
use serde_json::{json, Value};
use tokio::sync::RwLock as AsyncRwLock;

use crate::memory_embeddings::MemoryEmbeddingRuntime;
use crate::tools::process::{
    new_shared_process_manager_with_limit, DEFAULT_MAX_BACKGROUND_PROCESSES,
};
use crate::tools::{ToolExecutionContext, ToolRegistry};

const TOOLS: [&str; 11] = [
    "app_start",
    "app_send",
    "app_read",
    "app_stop",
    "app_list",
    "acp_start",
    "acp_prompt",
    "acp_read",
    "acp_cancel",
    "acp_stop",
    "acp_list",
];

struct Harness {
    context: ToolExecutionContext,
    agent: AgentState,
    _root: tempfile::TempDir,
}

fn harness(agent_id: &str) -> Harness {
    let root = tempfile::tempdir().unwrap();
    let context = ToolExecutionContext::new(
        Arc::new(AsyncRwLock::new(MemoryManager::new())),
        Arc::new(AsyncRwLock::new(MemoryEmbeddingRuntime::disabled())),
        None,
        ToolRegistry::new(),
        new_shared_process_manager_with_limit(DEFAULT_MAX_BACKGROUND_PROCESSES),
        Some(root.path().to_path_buf()),
        None,
    )
    .with_execution_service(Arc::new(
        crate::execution_backends::ExecutionService::default().with_host_execution_for_test(),
    ));
    let mut config = crate::agent_runs::test_support::companion_config("sessions");
    config.tools = Some(ToolRegistry::new().resolve_descriptors(TOOLS).unwrap());
    let agent = AgentState {
        id: agent_id.into(),
        name: config.name.clone(),
        status: AgentStatus::Running,
        config,
        created_at_ms: 1,
        token_usage: Default::default(),
    };
    Harness {
        context,
        agent,
        _root: root,
    }
}

impl Harness {
    async fn call(&self, name: &str, args: Value) -> Result<Value, String> {
        let message = Message {
            id: "sessions-message".into(),
            agent_id: self.agent.id.clone(),
            room_id: "chat:sessions".into(),
            content: Content::default(),
            role: MessageRole::User,
            created_at_ms: 1,
        };
        let DataValue::Object(args) = data(args) else {
            panic!("tool arguments are an object");
        };
        let call = ToolCall {
            id: format!("call-{name}"),
            name: name.into(),
            args,
        };
        let result = self
            .context
            .clone()
            .execute_tool(self.agent.clone(), message, call)
            .await;
        match (result.status, result.data) {
            (TaskStatus::Success, Some(content)) => {
                Ok(serde_json::from_str(&content.text).unwrap())
            }
            (_, _) => Err(result.error.unwrap_or_default()),
        }
    }
}

fn data(value: Value) -> DataValue {
    match value {
        Value::Null => DataValue::Null,
        Value::Bool(value) => DataValue::Bool(value),
        Value::Number(value) => DataValue::Number(value.as_f64().unwrap()),
        Value::String(value) => DataValue::String(value),
        Value::Array(items) => DataValue::Array(items.into_iter().map(data).collect()),
        Value::Object(map) => DataValue::Object(
            map.into_iter()
                .map(|(key, value)| (key, data(value)))
                .collect::<BTreeMap<_, _>>(),
        ),
    }
}

fn assert_unsupported_containment(error: &str) {
    assert!(error.contains("containment is unavailable"), "{error}");
}

#[tokio::test(flavor = "multi_thread")]
async fn app_start_refuses_unsupported_containment_before_launching() {
    let owner = harness("sessions-app-owner");
    let marker = owner._root.path().join("app-started");
    std::fs::write(
        owner._root.path().join("launch.js"),
        "require('fs').writeFileSync('app-started', 'launched');",
    )
    .unwrap();
    let refused = owner
        .call("app_start", json!({"command": "node launch.js"}))
        .await
        .unwrap_err();
    assert_unsupported_containment(&refused);
    assert!(!marker.exists(), "the command must not run before refusal");
    assert!(owner.call("app_list", json!({})).await.unwrap()["sessions"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn failed_app_containment_does_not_consume_session_capacity() {
    let owner = harness("sessions-capacity");
    std::fs::write(
        owner._root.path().join("launch.js"),
        "require('fs').writeFileSync('app-started', 'launched');",
    )
    .unwrap();
    for _ in 0..5 {
        let refused = owner
            .call("app_start", json!({"command": "node launch.js"}))
            .await
            .unwrap_err();
        assert_unsupported_containment(&refused);
    }
    assert!(owner.call("app_list", json!({})).await.unwrap()["sessions"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(!owner._root.path().join("app-started").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn unknown_keys_are_refused_with_the_names_that_work() {
    let owner = harness("sessions-keys");
    let refused = owner
        .call("app_send", json!({"id": "missing-app", "keys": ["F13"]}))
        .await
        .unwrap_err();
    assert!(refused.contains("Ctrl+C"), "{refused}");
    assert!(owner.call("app_list", json!({})).await.unwrap()["sessions"]
        .as_array()
        .unwrap()
        .is_empty());
}

/// A minimal ACP agent: it asks to edit a file, writes it only when allowed,
/// says how it went, and ends the turn.
const FAKE_ACP: &str = r#"
import json, sys
def send(m): sys.stdout.write(json.dumps(m) + "\n"); sys.stdout.flush()
def read():
    line = sys.stdin.readline()
    return json.loads(line) if line else None
pending = {}
next_id = 100
while True:
    m = read()
    if m is None: break
    method = m.get("method")
    if method == "initialize":
        send({"jsonrpc": "2.0", "id": m["id"], "result": {"protocolVersion": 1,
              "agentCapabilities": {"loadSession": False}, "agentInfo": {"name": "fake", "title": "Fake Coder", "version": "1.0"}, "authMethods": []}})
    elif method == "session/new":
        cwd = m["params"]["cwd"]
        send({"jsonrpc": "2.0", "id": m["id"], "result": {"sessionId": "s-1"}})
    elif method == "session/prompt":
        prompt_id = m["id"]
        send({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "s-1", "update": {"sessionUpdate": "plan", "entries": [{"content": "Edit notes.txt", "priority": "high", "status": "in_progress"}]}}})
        send({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "s-1", "update": {"sessionUpdate": "tool_call", "toolCallId": "t1", "title": "Edit notes.txt", "kind": "edit", "status": "pending", "locations": [{"path": cwd + "/notes.txt"}]}}})
        send({"jsonrpc": "2.0", "id": next_id, "method": "session/request_permission", "params": {"sessionId": "s-1",
              "toolCall": {"toolCallId": "t1", "title": "Edit notes.txt", "kind": "edit", "locations": [{"path": cwd + "/notes.txt"}]},
              "options": [{"optionId": "yes", "name": "Allow", "kind": "allow_once"}, {"optionId": "always", "name": "Always", "kind": "allow_always"}, {"optionId": "no", "name": "Reject", "kind": "reject_once"}]}})
        answer = read()
        outcome = answer["result"]["outcome"]
        allowed = outcome.get("outcome") == "selected" and outcome.get("optionId") == "yes"
        text = "edited" if allowed else "not allowed to edit"
        send({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "s-1", "update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text}}}})
        send({"jsonrpc": "2.0", "id": prompt_id, "result": {"stopReason": "end_turn"}})
"#;

#[tokio::test(flavor = "multi_thread")]
async fn a_coding_agents_edit_request_is_judged_and_never_allowed_always() {
    if std::process::Command::new("python3")
        .arg("--version")
        .output()
        .map_or(true, |output| !output.status.success())
    {
        return;
    }
    let owner = harness("sessions-acp");
    let script = owner._root.path().join("fake_acp.py");
    std::fs::write(&script, FAKE_ACP).unwrap();
    let started = owner
        .call("acp_start", json!({"command": "python3 fake_acp.py"}))
        .await;
    if cfg!(unix) {
        let refused = started.unwrap_err();
        assert_unsupported_containment(&refused);
        assert!(!owner._root.path().join("notes.txt").exists());
        assert!(owner.call("acp_list", json!({})).await.unwrap()["sessions"]
            .as_array()
            .unwrap()
            .is_empty());
        return;
    }
    let started = started.unwrap();
    assert_eq!(started["kind"], "acp_session");
    assert_eq!(started["agent"]["name"], "Fake Coder");
    assert_eq!(started["status"], "idle");
    let id = started["id"].as_str().unwrap().to_string();

    // No run, so no owner to ask: an edit that would need one is cancelled.
    let turn = owner
        .call(
            "acp_prompt",
            json!({"id": id, "text": "fix it", "wait_ms": 10000}),
        )
        .await
        .unwrap();
    assert_eq!(turn["stopReason"], "end_turn", "{turn}");
    let events = turn["events"].as_array().unwrap();
    let kinds: Vec<&str> = events.iter().filter_map(|e| e["type"].as_str()).collect();
    assert!(kinds.contains(&"plan"), "{kinds:?}");
    assert!(kinds.contains(&"tool_call"), "{kinds:?}");
    let answered = events
        .iter()
        .find(|event| event["type"] == "permission_answered")
        .unwrap();
    assert_eq!(answered["decision"], "cancelled");
    assert!(events
        .iter()
        .any(|event| event["type"] == "agent_message" && event["text"] == "not allowed to edit"));

    let stopped = owner.call("acp_stop", json!({"id": id})).await.unwrap();
    assert_eq!(stopped["status"], "stopped");
    assert_eq!(stopped["treeGone"], true);
}
