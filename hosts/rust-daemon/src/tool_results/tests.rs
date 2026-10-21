use super::*;

fn result(text: &str) -> TaskResult<Content> {
    TaskResult::success(
        Content {
            text: text.into(),
            ..Content::default()
        },
        7,
    )
}

#[test]
fn immutable_retry_restart_and_utf8_paging_preserve_the_actual_payload() {
    let temp = tempfile::tempdir().unwrap();
    let store = ToolResultStore::new(temp.path().join("results"));
    let value = result(&"𐐀こんにちは\n".repeat(3000));
    let saved = store
        .save("agent", temp.path(), "run", "call", "read_file", &value)
        .unwrap();
    assert_eq!(
        saved,
        store
            .save("agent", temp.path(), "run", "call", "read_file", &value)
            .unwrap()
    );
    let restarted = ToolResultStore::new(temp.path().join("results"));
    let mut text = String::new();
    let mut offset = 0;
    loop {
        let page = restarted
            .page("agent", temp.path(), &saved.id, offset, 127)
            .unwrap();
        assert!(page.text.len() <= 127);
        text.push_str(&page.text);
        match page.next_offset {
            Some(next) => {
                assert!(next > offset);
                offset = next;
            }
            None => break,
        }
    }
    assert_eq!(text, serde_json::to_string(&value).unwrap());
    assert_eq!(saved.bytes, text.len());
}

#[test]
fn reads_are_bound_to_agent_workspace_and_validated_ids() {
    let temp = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let store = ToolResultStore::new(temp.path().join("results"));
    let saved = store
        .save(
            "agent",
            temp.path(),
            "run",
            "call",
            "read_file",
            &result("safe"),
        )
        .unwrap();
    assert!(store.page("other", temp.path(), &saved.id, 0, 100).is_err());
    assert!(store
        .page("agent", other.path(), &saved.id, 0, 100)
        .is_err());
    assert!(store
        .page("agent", temp.path(), "../../outside", 0, 100)
        .is_err());
    assert!(store
        .page("agent", temp.path(), &saved.id, 0, MAX_PAGE_BYTES + 1)
        .is_err());
    assert!(store.page("agent", temp.path(), &saved.id, 0, 0).is_err());
}

#[test]
fn corruption_and_oversized_outputs_are_refused() {
    let temp = tempfile::tempdir().unwrap();
    let store = ToolResultStore::new(temp.path().join("results"));
    assert!(store
        .save(
            "agent",
            temp.path(),
            "run",
            "call",
            "read_file",
            &result(&"x".repeat(MAX_ARTIFACT_BYTES))
        )
        .is_err());
    let saved = store
        .save(
            "agent",
            temp.path(),
            "run",
            "call",
            "read_file",
            &result("safe"),
        )
        .unwrap();
    std::fs::write(store.root.join(format!("{}.json", saved.id)), b"corrupt").unwrap();
    assert!(store.page("agent", temp.path(), &saved.id, 0, 100).is_err());
}

#[test]
fn upstream_truncation_remains_explicit() {
    let temp = tempfile::tempdir().unwrap();
    let store = ToolResultStore::new(temp.path().join("results"));
    let saved = store
        .save(
            "agent",
            temp.path(),
            "run",
            "call",
            "bash",
            &result("begin\n... [output truncated] ...\nend"),
        )
        .unwrap();
    assert!(!saved.capture_complete);
}

#[test]
fn native_mcp_shortening_and_collector_markers_remain_incomplete() {
    for text in [
        r#"{"textBlocks":["available"],"shortened":true,"omittedBlocks":0}"#,
        r#"{"textBlocks":["available"],"shortened":false,"omittedBlocks":2}"#,
        "head\n... [37 lines omitted] ...\ntail",
        "available\n...[response exceeded byte limit; truncated]",
    ] {
        assert!(!capture_complete(&result(text)), "{text}");
    }
    assert!(capture_complete(&result(
        r#"{"textBlocks":["available"],"shortened":false,"omittedBlocks":0}"#
    )));
}

#[test]
fn mixed_error_and_partial_data_keep_error_preview_and_original_payload() {
    let temp = tempfile::tempdir().unwrap();
    let store = ToolResultStore::new(temp.path().join("results"));
    let link = crate::runs::RunLink {
        agent_id: "agent".into(),
        run_id: "run".into(),
        session_id: "session".into(),
    };
    let call = anima_core::ToolCall {
        id: "mixed-error".into(),
        name: "read_file".into(),
        args: Default::default(),
    };
    let original = TaskResult {
        status: anima_core::TaskStatus::Error,
        data: Some(Content {
            text: "partial available output".repeat(2000),
            ..Content::default()
        }),
        error: Some("The operation failed; diagnostics [output truncated]".into()),
        duration_ms: 29,
    };
    assert!(!capture_complete(&original));
    let saved = store
        .save(
            "agent",
            temp.path(),
            "run",
            "mixed-error",
            "read_file",
            &original,
        )
        .unwrap();
    assert!(!saved.capture_complete);
    assert_eq!(
        store.download("agent", temp.path(), &saved.id).unwrap().1,
        serde_json::to_vec(&original).unwrap()
    );
    for retained in [Some(&store), None] {
        let projected = project_result(
            retained,
            "agent",
            Some(temp.path()),
            Some(&link),
            &call,
            original.clone(),
        );
        assert_eq!(projected.status, anima_core::TaskStatus::Error);
        assert_eq!(projected.duration_ms, 29);
        assert!(projected.data.is_some());
        let error = projected.error.unwrap();
        assert!(error.contains("The operation failed; diagnostics [output truncated]"));
        assert!(!error.contains("partial available output"));
        assert!(projected
            .data
            .unwrap()
            .text
            .contains("The operation failed; diagnostics [output truncated]"));
    }
}

#[test]
fn bounded_helper_management_json_keeps_exact_handles_and_escaped_provenance() {
    let temp = tempfile::tempdir().unwrap();
    let store = ToolResultStore::new(temp.path().join("results"));
    let link = crate::runs::RunLink {
        agent_id: "agent".into(),
        run_id: "parent".into(),
        session_id: "session".into(),
    };
    let mut call = anima_core::ToolCall {
        id: "helper-list".into(),
        name: "list_helper_runs".into(),
        args: Default::default(),
    };
    let text = serde_json::json!({"runs":[{
        "runId":"child", "agentId":"helper", "sessionId":"child-session",
        "replyMessageId":"reply", "source":"delegation", "parentRunId":"parent",
        "resultText":"\0".repeat(4000), "resultTruncated":true,
    }],"truncated":false})
    .to_string();
    assert!(text.len() <= 32 * 1024);
    let original = result(&text);
    assert!(serde_json::to_vec(&original).unwrap().len() > INLINE_LIMIT);
    for retention in [None, Some(&store)] {
        let projected = project_result(
            retention,
            "agent",
            Some(temp.path()),
            Some(&link),
            &call,
            original.clone(),
        );
        assert_eq!(projected, original);
        let parsed: serde_json::Value =
            serde_json::from_str(&projected.data.unwrap().text).unwrap();
        assert_eq!(parsed["runs"][0]["runId"], "child");
        assert_eq!(parsed["runs"][0]["source"], "delegation");
        assert_eq!(parsed["runs"][0]["parentRunId"], "parent");
        assert_eq!(parsed["runs"][0]["resultTruncated"], true);
    }
    call.name = "read_file".into();
    assert_ne!(
        project_result(
            None,
            "agent",
            Some(temp.path()),
            Some(&link),
            &call,
            original.clone()
        ),
        original
    );
    call.name = "list_helper_runs".into();
    let oversized = result(
        &serde_json::json!({"runs":[{"resultText":"x".repeat(33*1024)}],"truncated":false})
            .to_string(),
    );
    assert_ne!(
        project_result(
            None,
            "agent",
            Some(temp.path()),
            Some(&link),
            &call,
            oversized.clone()
        ),
        oversized
    );
    let malformed = result(&format!("{{\"runs\":{}", "x".repeat(18000)));
    assert_ne!(
        project_result(
            None,
            "agent",
            Some(temp.path()),
            Some(&link),
            &call,
            malformed.clone()
        ),
        malformed
    );
}

#[test]
fn concurrent_exact_retries_produce_one_immutable_artifact() {
    let temp = tempfile::tempdir().unwrap();
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let root = temp.path();
                scope.spawn(move || {
                    ToolResultStore::new(root.join("results"))
                        .save("agent", root, "run", "call", "tool", &result("same"))
                        .unwrap()
                })
            })
            .collect();
        let saved: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert!(saved.iter().all(|a| *a == saved[0]));
    });
    assert_eq!(
        std::fs::read_dir(temp.path().join("results"))
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|e| e == "json"))
            .count(),
        1
    );
}

#[test]
fn full_store_refuses_new_artifacts_but_preserves_existing_references() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = ToolResultStore::new(temp.path().join("results"));
    store.max_count = 1;
    let original = result("first");
    let saved = store
        .save("agent", temp.path(), "run", "one", "tool", &original)
        .unwrap();
    assert!(store
        .save(
            "agent",
            temp.path(),
            "run",
            "two",
            "tool",
            &result("second")
        )
        .is_err());
    assert_eq!(
        store
            .save("agent", temp.path(), "run", "one", "tool", &original)
            .unwrap(),
        saved
    );
    assert!(store.page("agent", temp.path(), &saved.id, 0, 100).is_ok());
    store.max_count = 1024;
    store.max_bytes = 1;
    assert!(store
        .save(
            "agent",
            temp.path(),
            "run",
            "two",
            "tool",
            &result("second")
        )
        .is_err());
}

#[test]
fn projection_saves_original_content_and_preserves_settled_status_on_failure() {
    use anima_core::{TaskStatus, ToolCall};
    let temp = tempfile::tempdir().unwrap();
    let store = ToolResultStore::new(temp.path().join("results"));
    let link = crate::runs::RunLink {
        agent_id: "agent".into(),
        run_id: "run".into(),
        session_id: "session".into(),
    };
    let call = ToolCall {
        id: "call".into(),
        name: "read_file".into(),
        args: Default::default(),
    };
    let original = result(&"final evidence\n".repeat(3000));
    let projected = project_result(
        Some(&store),
        "agent",
        Some(temp.path()),
        Some(&link),
        &call,
        original.clone(),
    );
    assert_eq!(projected.status, TaskStatus::Success);
    assert_eq!(projected.duration_ms, 7);
    let text = &projected.data.unwrap().text;
    assert!(text.starts_with("[saved-tool-result id=tr_"));
    assert!(text.len() < 2048);
    let saved = store
        .save("agent", temp.path(), "run", "call", "read_file", &original)
        .unwrap();
    assert_eq!(
        store.download("agent", temp.path(), &saved.id).unwrap().1,
        serde_json::to_vec(&original).unwrap()
    );
    let error: TaskResult<Content> = TaskResult::error(
        format!("Action outcome save uncertain: {}", "details".repeat(5000)),
        19,
    );
    let projected = project_result(None, "agent", Some(temp.path()), Some(&link), &call, error);
    assert_eq!(projected.status, TaskStatus::Error);
    assert_eq!(projected.duration_ms, 19);
    assert!(projected.data.is_none());
    assert!(projected
        .error
        .as_deref()
        .unwrap()
        .contains("Action outcome save uncertain"));
    assert!(projected.error.as_deref().unwrap().contains("not saved"));
}

#[test]
fn restart_with_leftover_lock_and_pending_file_still_saves_without_deleting_results() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("results");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join(".write-lock"), b"").unwrap();
    std::fs::write(root.join(".pending-orphan"), b"unfinished").unwrap();
    let store = ToolResultStore::new(root.clone());
    let saved = store
        .save(
            "agent",
            temp.path(),
            "run",
            "call",
            "tool",
            &result("saved after restart"),
        )
        .unwrap();
    assert!(store.page("agent", temp.path(), &saved.id, 0, 100).is_ok());
    assert_eq!(
        std::fs::read(root.join(".pending-orphan")).unwrap(),
        b"unfinished"
    );
}

#[tokio::test]
async fn model_reads_require_live_grant_current_workspace_and_known_arguments() {
    use crate::tools::{
        new_shared_process_manager_with_limit, ToolExecutionContext, ToolRegistry,
        DEFAULT_MAX_BACKGROUND_PROCESSES,
    };
    use anima_core::{DataValue, TaskStatus, ToolCall};
    use std::{collections::BTreeMap, sync::Arc};
    use tokio::sync::{RwLock, Semaphore};
    let temp = tempfile::tempdir().unwrap();
    let other_workspace = tempfile::tempdir().unwrap();
    let store = ToolResultStore::new(temp.path().join("results"));
    let registry = ToolRegistry::new();
    let mut config: anima_core::AgentConfig =
        serde_json::from_value(serde_json::json!({"name":"test", "model":"deterministic"}))
            .unwrap();
    config.tools = Some(registry.resolve_descriptors(["read_tool_result"]).unwrap());
    let mut daemon = crate::state::DaemonState::new();
    daemon.workspace = Some(crate::control_plane_store::WorkspaceConfig {
        root_path: temp.path().into(),
        company_name: "test".into(),
        mission: "test".into(),
        values: vec![],
    });
    let agent = daemon.create_agent(config.clone()).unwrap().state;
    let state = Arc::new(RwLock::new(daemon));
    let coordinator =
        crate::agent_runs::AgentRunCoordinator::new(state.clone(), Arc::new(Semaphore::new(1)));
    let context = ToolExecutionContext::new(
        Arc::new(RwLock::new(anima_memory::MemoryManager::new())),
        Arc::new(RwLock::new(
            crate::memory_embeddings::MemoryEmbeddingRuntime::disabled(),
        )),
        None,
        registry,
        new_shared_process_manager_with_limit(DEFAULT_MAX_BACKGROUND_PROCESSES),
        Some(temp.path().into()),
        None,
    )
    .with_team(coordinator, false)
    .with_tool_results(Some(store.clone()));
    let saved = store
        .save(
            &agent.id,
            temp.path(),
            "run",
            "call",
            "read_file",
            &result("owned payload"),
        )
        .unwrap();
    let call = ToolCall {
        id: "read-call".into(),
        name: "read_tool_result".into(),
        args: BTreeMap::from([("artifact_id".into(), DataValue::String(saved.id))]),
    };
    let message = anima_core::Message {
        id: "message".into(),
        agent_id: agent.id.clone(),
        room_id: "session".into(),
        content: Content::default(),
        role: anima_core::MessageRole::User,
        created_at_ms: 1,
    };
    let read = context
        .clone()
        .execute_tool(agent.clone(), message.clone(), call.clone())
        .await;
    assert_eq!(read.status, TaskStatus::Success);
    assert!(read.data.unwrap().text.contains("owned payload"));
    let mut unknown = call.clone();
    unknown
        .args
        .insert("path".into(), DataValue::String("arbitrary".into()));
    let refused = context
        .clone()
        .execute_tool(agent.clone(), message.clone(), unknown)
        .await;
    assert_eq!(refused.status, TaskStatus::Error);
    assert!(refused.error.unwrap().contains("unknown argument"));
    state.write().await.workspace.as_mut().unwrap().root_path = other_workspace.path().into();
    let refused = context
        .clone()
        .execute_tool(agent.clone(), message.clone(), call.clone())
        .await;
    assert_eq!(refused.status, TaskStatus::Error);
    assert!(refused.error.unwrap().contains("workspace changed"));
    state.write().await.workspace.as_mut().unwrap().root_path = temp.path().into();
    config.tools = Some(vec![]);
    state.write().await.restore_agent_config(&agent.id, config);
    let refused = context.execute_tool(agent, message, call).await;
    assert_eq!(refused.status, TaskStatus::Error);
    assert!(refused.error.unwrap().contains("permission was revoked"));
}
