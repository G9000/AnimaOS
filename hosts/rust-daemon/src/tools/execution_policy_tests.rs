use super::*;
use anima_core::{AgentStatus, MessageRole, TaskStatus};
use tokio::sync::RwLock;

#[tokio::test]
async fn hardening_host_tools_cannot_inherit_execution_authority_from_descriptors() {
    let names = [
        "bash",
        "bg_start",
        "execute_code",
        "app_start",
        "app_send",
        "acp_start",
        "acp_prompt",
    ];
    let registry = ToolRegistry::new();
    let descriptors: Vec<_> = names
        .iter()
        .map(|name| registry.descriptor(name).unwrap())
        .collect();
    let root = tempfile::tempdir().unwrap();
    let context = ToolExecutionContext::new(
        Arc::new(RwLock::new(anima_memory::MemoryManager::new())),
        Arc::new(RwLock::new(
            crate::memory_embeddings::MemoryEmbeddingRuntime::disabled(),
        )),
        None,
        registry,
        new_shared_process_manager_with_limit(1),
        Some(root.path().into()),
        None,
    );
    let agent = AgentState {
        id: "host-boundary-owner".into(),
        name: "fixture".into(),
        status: AgentStatus::Running,
        config: serde_json::from_value(
            serde_json::json!({"name":"fixture","model":"fixture","tools":descriptors}),
        )
        .unwrap(),
        created_at_ms: 1,
        token_usage: Default::default(),
    };
    let message = Message {
        id: "input".into(),
        agent_id: agent.id.clone(),
        room_id: "room".into(),
        content: Content::default(),
        role: MessageRole::User,
        created_at_ms: 1,
    };
    for name in names {
        let result = context
            .clone()
            .execute_tool(
                agent.clone(),
                message.clone(),
                ToolCall {
                    id: format!("call-{name}"),
                    name: name.into(),
                    args: BTreeMap::from([(
                        "command".into(),
                        DataValue::String("echo unexpected>must-not-exist".into()),
                    )]),
                },
            )
            .await;
        assert_eq!(result.status, TaskStatus::Error, "{name}");
        assert!(
            result
                .error
                .as_deref()
                .unwrap_or_default()
                .contains("Host code execution is disabled"),
            "{name}: {:?}",
            result.error
        );
    }
    assert!(!root.path().join("must-not-exist").exists());
}

#[tokio::test]
async fn execution_run_preflight_accepts_explicit_integral_timeout() {
    let root = tempfile::tempdir().unwrap();
    let transport = root.path().join("fixture-transport.exe");
    std::fs::write(&transport, b"fixture transport; must never launch").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&transport, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let service = crate::execution_backends::ExecutionService::from_json(
        &serde_json::json!({
            "version": 1,
            "targets": [{
                "id": "isolated", "label": "Fixture",
                "backend": { "kind": "docker", "executable": transport,
                    "image": format!("fixture@sha256:{}", "a".repeat(64)) }
            }]
        })
        .to_string(),
    )
    .unwrap();
    let registry = ToolRegistry::new();
    let descriptor = registry.descriptor("execution_run").unwrap();
    let context = ToolExecutionContext::new(
        Arc::new(RwLock::new(anima_memory::MemoryManager::new())),
        Arc::new(RwLock::new(
            crate::memory_embeddings::MemoryEmbeddingRuntime::disabled(),
        )),
        None,
        registry,
        new_shared_process_manager_with_limit(1),
        Some(root.path().into()),
        None,
    )
    .with_execution_service(Arc::new(service));
    let agent = AgentState {
        id: "timeout-fixture".into(),
        name: "fixture".into(),
        status: AgentStatus::Running,
        config: serde_json::from_value(
            serde_json::json!({"name":"fixture", "model":"fixture", "tools":[descriptor]}),
        )
        .unwrap(),
        created_at_ms: 1,
        token_usage: Default::default(),
    };
    let message = Message {
        id: "input".into(),
        agent_id: agent.id.clone(),
        room_id: "room".into(),
        content: Content::default(),
        role: MessageRole::User,
        created_at_ms: 1,
    };
    let result = context
        .execute_tool(
            agent,
            message,
            ToolCall {
                id: "explicit-timeout".into(),
                name: "execution_run".into(),
                args: BTreeMap::from([
                    ("targetId".into(), DataValue::String("isolated".into())),
                    ("command".into(), DataValue::String("echo fixture".into())),
                    ("timeoutMs".into(), DataValue::Number(30_000.0)),
                ]),
            },
        )
        .await;
    assert_eq!(result.status, TaskStatus::Error);
    assert_eq!(
        result.error.as_deref(),
        Some("Execution requires an ordinary owned native action journal")
    );
}
